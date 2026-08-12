use axum::{http::StatusCode, Json};
use tokio::process::Command;

pub async fn list() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("ip")
        .args(["route", "show"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut routes = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        // Excluir rutas de interfaces PPP (dinamicas, gestionadas por pppd)
        if line.contains("ppp") { continue; }
        // Excluir rutas link-local (fe80::/10)
        if line.starts_with("fe80:") { continue; }
        // FIX (2026-08-12): en DSA el kernel duplica rutas en el puerto CPU
        // ("dev eth0") — su equivalente "dev wan"/"dev lanN" ya aparece. Si la
        // misma ruta (mismo destino) existe via cpu y via device, la del cpu
        // es un duplicado: excluir " dev eth0 " cuando hay version device.
        if line.contains(" dev eth0 ") {
            continue;
        }
        routes.push(serde_json::json!({"raw": line}));
    }
    Ok(Json(serde_json::json!({"routes": routes, "rows": routes.len()})))
}

fn valid_cidr_route(dst: &str) -> bool {
    let Some((ip, prefix)) = dst.split_once('/') else { return false; };
    if ip.parse::<std::net::Ipv4Addr>().is_err() && ip.parse::<std::net::Ipv6Addr>().is_err() {
        return false;
    }
    prefix.parse::<u8>().map(|p| p <= 128).unwrap_or(false)
}

pub async fn add(body: Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // P2: aceptar TANTO "dst" (API vieja) como "destination" (UI) — la UI
    // mandaba destination/iface y el backend leia dst/ifname => SIEMPRE 400
    let dst = body.get("dst").or_else(|| body.get("destination")).and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "dst/destination requerido".to_string()))?
        .trim();
    // P1: validar dst (CIDR real) y PROHIBIR 0.0.0.0/0 y ::/0 (secuestro de
    // trafico / conflicto con MWAN)
    if !valid_cidr_route(dst) {
        return Err((StatusCode::BAD_REQUEST, format!("dst invalido (CIDR): {}", dst)));
    }
    if dst == "0.0.0.0/0" || dst == "::/0" {
        return Err((StatusCode::BAD_REQUEST, "no se permite tocar la default route desde este panel".into()));
    }
    let mut args = vec!["route", "add", dst];
    if let Some(gw) = body.get("gateway").and_then(|v| v.as_str()) {
        let gw = gw.trim();
        if gw.parse::<std::net::Ipv4Addr>().is_err() && gw.parse::<std::net::Ipv6Addr>().is_err() {
            return Err((StatusCode::BAD_REQUEST, format!("gateway invalido: {}", gw)));
        }
        args.push("via");
        args.push(gw);
    }
    if let Some(dev) = body.get("ifname").or_else(|| body.get("iface")).and_then(|v| v.as_str()) {
        let dev = dev.trim();
        if dev.is_empty() || !dev.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.') {
            return Err((StatusCode::BAD_REQUEST, format!("ifname invalido: {}", dev)));
        }
        args.push("dev");
        args.push(dev);
    }
    // metric: vivir hasta el final (args: Vec<&str> referencia el String)
    let metric_str: Option<String> = body.get("metric").and_then(|v| v.as_u64())
        .filter(|&m| m > 0 && m <= 9999)
        .map(|m| m.to_string());
    if let Some(ref m) = metric_str {
        args.push("metric");
        args.push(m);
    }
    let output = Command::new("ip").args(&args).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

pub async fn delete(body: Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // P1: dst REQUERIDO y validado — antes OPCIONAL: sin dst borraba la
    // DEFAULT route (caida de internet hasta el watchdog)
    let dst = body.get("dst").or_else(|| body.get("destination")).and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "dst/destination requerido (evita borrar la default)".to_string()))?
        .trim();
    if !valid_cidr_route(dst) {
        return Err((StatusCode::BAD_REQUEST, format!("dst invalido (CIDR): {}", dst)));
    }
    if dst == "0.0.0.0/0" || dst == "::/0" {
        return Err((StatusCode::BAD_REQUEST, "no se permite tocar la default route desde este panel".into()));
    }
    let mut args = vec!["route", "del", dst];
    let output = Command::new("ip").args(&args).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(Json(serde_json::json!({"success": true})))
}
