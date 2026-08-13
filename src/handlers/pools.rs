use axum::{http::StatusCode, Json};

const DNSMASQ_CONF: &str = "/etc/dnsmasq.conf";

// P1: lock RMW — serializa add/delete de pools (el sync 60s y los handlers
// concurrentes leían-modificaban-escribían dnsmasq.conf a la vez)
static POOLS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Parsea una línea dhcp-range y devuelve sus componentes.
/// Maneja líneas con múltiples dhcp-range (corruptas por falta de \n).
fn parse_range(line: &str) -> Vec<serde_json::Value> {
    let mut result = Vec::new();
    let val = line.trim_start_matches("dhcp-range=");
    // Si la línea contiene "dhcp-range=" en medio, está corrupta (1ddhcp-range=...)
    // Buscamos "dhcp-range=" después de la primera ocurrencia
    let rest = val;
    let parts: Vec<&str> = rest.split("dhcp-range=").collect();
    if parts.len() > 1 {
        // La línea original tenía formato eth2...,1ddhcp-range=wg0...
        // parts[0] = "interface:eth2,192.168.30.2,192.168.30.245,1d"
        // parts[1] = "interface:wg0,10.7.0.100,10.7.0.200,12h"
        for p in parts {
            if p.is_empty() { continue; }
            let entry = parse_single(p);
            if entry["start"] != "" { result.push(entry); }
        }
    } else {
        let entry = parse_single(val);
        if entry["start"] != "" { result.push(entry); }
    }
    result
}

fn parse_single(val: &str) -> serde_json::Value {
    let parts: Vec<&str> = val.split(',').collect();
    let mut start = "";
    let mut end = "";
    let mut iface = "";
    let mut lease = "";

    // Detectar interfaz: "interface:eth3" o solo "eth3" como primer elemento
    let offset = if parts.first().map_or(false, |p| p.starts_with("interface:")) {
        iface = parts[0].trim_start_matches("interface:").split('@').next().unwrap_or("");
        1 // saltar el primer elemento
    } else if parts.len() >= 3 && !parts[0].contains('.') && parts[0].chars().any(|c| c.is_alphabetic()) {
        // Primer elemento es nombre de interfaz sin "interface:" (ej: "eth3")
        iface = parts[0].split('@').next().unwrap_or("");
        1
    } else {
        0
    };

    // El resto: start, end, [lease]
    if parts.len() > offset { start = parts[offset]; }
    if parts.len() > offset + 1 { end = parts[offset + 1]; }
    if parts.len() > offset + 2 {
        let last = parts[offset + 2];
        if last.chars().any(|c| c.is_alphabetic()) { lease = last; }
    }

    serde_json::json!({
        "start": start,
        "end": end,
        "interface": iface,
        "lease": lease,
    })
}

pub async fn list() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let content = std::fs::read_to_string(DNSMASQ_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error leyendo {}: {}", DNSMASQ_CONF, e)))?;
    let mut pools = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if !line.starts_with("dhcp-range=") { continue; }
        let entries = parse_range(line);
        pools.extend(entries);
    }
    Ok(Json(serde_json::json!([pools, {"rows": pools.len()}])))
}

pub async fn create(body: Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let start = body.get("start").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let end = body.get("end").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let iface = body.get("interface").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let clean_iface = iface.split('@').next().unwrap_or("").to_string();
    let lease = body.get("lease").and_then(|v| v.as_str()).unwrap_or("12h").trim().to_string();

    if start.is_empty() || end.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "start y end son requeridos".into()));
    }
    // P0: validar IPs reales + iface/lease sin caracteres peligrosos
    if start.parse::<std::net::Ipv4Addr>().is_err() || end.parse::<std::net::Ipv4Addr>().is_err() {
        return Err((StatusCode::BAD_REQUEST, format!("start/end deben ser IPs IPv4 validas: {}/{}", start, end)));
    }
    if !clean_iface.is_empty()
        && !clean_iface.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.') {
        return Err((StatusCode::BAD_REQUEST, "interface invalida".into()));
    }
    if lease.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err((StatusCode::BAD_REQUEST, "lease invalido".into()));
    }

    let _g = POOLS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut content = std::fs::read_to_string(DNSMASQ_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error leyendo {}: {}", DNSMASQ_CONF, e)))?;
    if !content.ends_with('\n') {
        content.push('\n');
    }
    let line = if !clean_iface.is_empty() {
        format!("dhcp-range=interface:{},{},{},{}\n", clean_iface, start, end, lease)
    } else {
        format!("dhcp-range={},{},{}\n", start, end, lease)
    };
    content.push_str(&line);
    // P0: escribir atómico (tmp+rename) y VERIFICAR reload — rollback si falla
    let tmp = format!("{}.tmp-{}", DNSMASQ_CONF, std::process::id());
    if std::fs::write(&tmp, content.as_bytes()).is_err() {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, "no se pudo escribir config".into()));
    }
    drop(_g); // soltar antes del .await (reload)
    let reload = crate::handlers::helpers::service_action_output("dnsmasq", "reload").await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if reload.status.success() {
        let _ = std::fs::rename(&tmp, DNSMASQ_CONF);
        Ok(Json(serde_json::json!({"success": true})))
    } else {
        let _ = std::fs::remove_file(&tmp);
        Err((StatusCode::INTERNAL_SERVER_ERROR,
            format!("dnsmasq reload fallo — cambio NO aplicado: {}", String::from_utf8_lossy(&reload.stderr).trim())))
    }
}

pub async fn update(body: Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // start identifica el pool existente; end/interface/lease son los NUEVOS
    let start = body.get("start").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let new_end = body.get("end").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let new_iface = body.get("interface").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let clean_iface = new_iface.split('@').next().unwrap_or("").to_string();
    let new_lease = body.get("lease").and_then(|v| v.as_str()).unwrap_or("12h").trim().to_string();

    if start.is_empty() || new_end.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "start y end son requeridos".into()));
    }
    if start.parse::<std::net::Ipv4Addr>().is_err() || new_end.parse::<std::net::Ipv4Addr>().is_err() {
        return Err((StatusCode::BAD_REQUEST, format!("start/end deben ser IPs IPv4 validas: {}/{}", start, new_end)));
    }
    if !clean_iface.is_empty()
        && !clean_iface.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.') {
        return Err((StatusCode::BAD_REQUEST, "interface invalida".into()));
    }
    if new_lease.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err((StatusCode::BAD_REQUEST, "lease invalido".into()));
    }

    let _g = POOLS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let content = std::fs::read_to_string(DNSMASQ_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error leyendo {}: {}", DNSMASQ_CONF, e)))?;
    let mut new_lines = Vec::new();
    let mut updated = 0usize;
    for line in content.lines() {
        let t = line.trim();
        if !t.starts_with("dhcp-range=") {
            new_lines.push(line.to_string());
            continue;
        }
        // Detectar el start de la linea (misma logica que delete)
        let body_part = t.trim_start_matches("dhcp-range=");
        let fields: Vec<&str> = body_part.split(',').collect();
        let start_field = if fields.len() >= 2 && fields[0].contains('.') { fields[0] }
            else if fields.len() >= 3 && !fields[1].contains('.') && fields[1].parse::<std::net::Ipv4Addr>().is_ok() { fields[1] }
            else if fields.len() >= 2 && fields[1].parse::<std::net::Ipv4Addr>().is_ok() { fields[1] }
            else { "" };
        if start_field == start {
            updated += 1;
            let new_line = if !clean_iface.is_empty() {
                format!("dhcp-range=interface:{},{},{},{}", clean_iface, start, new_end, new_lease)
            } else {
                format!("dhcp-range={},{},{}", start, new_end, new_lease)
            };
            new_lines.push(new_line);
        } else {
            new_lines.push(line.to_string());
        }
    }
    if updated == 0 {
        return Err((StatusCode::BAD_REQUEST, format!("no se encontro rango con start {}", start)));
    }
    let tmp = format!("{}.tmp-{}", DNSMASQ_CONF, std::process::id());
    std::fs::write(&tmp, new_lines.join("\n") + "\n")
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    drop(_g); // soltar antes del .await (reload)
    let reload = crate::handlers::helpers::service_action_output("dnsmasq", "reload").await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if reload.status.success() {
        let _ = std::fs::rename(&tmp, DNSMASQ_CONF);
        Ok(Json(serde_json::json!({"success": true, "updated": updated})))
    } else {
        let _ = std::fs::remove_file(&tmp);
        Err((StatusCode::INTERNAL_SERVER_ERROR,
            format!("dnsmasq reload fallo — cambio NO aplicado: {}", String::from_utf8_lossy(&reload.stderr).trim())))
    }
}

pub async fn delete(body: Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let start = body.get("start").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if start.is_empty() { return Err((StatusCode::BAD_REQUEST, "start requerido".into())); }
    // P0: validar IP y borrar SOLO rangos con start EXACTO (antes substring
    // borraba rangos de mas: start="1" borraba casi todo)
    if start.parse::<std::net::Ipv4Addr>().is_err() {
        return Err((StatusCode::BAD_REQUEST, format!("'{}' no es una IP valida", start)));
    }
    let _g = POOLS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let content = std::fs::read_to_string(DNSMASQ_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error leyendo {}: {}", DNSMASQ_CONF, e)))?;
    let mut new_lines = Vec::new();
    let mut removed = 0usize;
    for line in content.lines() {
        let t = line.trim();
        if !t.starts_with("dhcp-range=") {
            new_lines.push(line);
            continue;
        }
        // Extraer campos: dhcp-range=[interface:]START,END[,lease]
        let body_part = t.trim_start_matches("dhcp-range=");
        let fields: Vec<&str> = body_part.split(',').collect();
        let start_field = if fields.len() >= 2 && fields[0].contains('.') { fields[0] }
            else if fields.len() >= 3 && !fields[1].contains('.') && fields[1].parse::<std::net::Ipv4Addr>().is_ok() { fields[1] }
            else if fields.len() >= 2 && fields[1].parse::<std::net::Ipv4Addr>().is_ok() { fields[1] }
            else { "" };
        if start_field == start {
            removed += 1;
            continue;
        }
        new_lines.push(line);
    }
    if removed == 0 {
        return Err((StatusCode::BAD_REQUEST, format!("no se encontro rango con start {}", start)));
    }
    let tmp = format!("{}.tmp-{}", DNSMASQ_CONF, std::process::id());
    std::fs::write(&tmp, new_lines.join("\n") + "\n")
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    drop(_g); // soltar antes del .await (reload)
    let reload = crate::handlers::helpers::service_action_output("dnsmasq", "reload").await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if reload.status.success() {
        let _ = std::fs::rename(&tmp, DNSMASQ_CONF);
        Ok(Json(serde_json::json!({"success": true, "removed": removed})))
    } else {
        let _ = std::fs::remove_file(&tmp);
        Err((StatusCode::INTERNAL_SERVER_ERROR,
            format!("dnsmasq reload fallo — cambio NO aplicado: {}", String::from_utf8_lossy(&reload.stderr).trim())))
    }
}
