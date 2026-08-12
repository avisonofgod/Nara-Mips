use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeEntry {
    pub name: String,
    pub ports: Vec<PortEntry>,
    pub mac: String,
    pub mtu: u64,
    pub stp: bool,
    pub priority: u64,
    pub ageing: u64,
    pub max_age: u64,
    pub fwd_delay: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PortEntry {
    pub iface: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct BridgeCreate {
    pub name: String,
    pub ports: Vec<String>,
}

pub async fn list() -> Result<Json<Vec<BridgeEntry>>, (StatusCode, String)> {
    let output = Command::new("ip")
        .args(["-json", "link", "show", "type", "bridge"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !output.status.success() {
        return Ok(Json(vec![]));
    }

    let links: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut bridges = Vec::new();

    for link in &links {
        let ifname = link["ifname"].as_str().unwrap_or("");
        if ifname.is_empty() { continue; }

        // Obtener ports del bridge usando ip link show master
        let ports_output = Command::new("ip")
            .args(["link", "show", "master", ifname])
            .output()
            .await;

        let mut ports = Vec::new();
        if let Ok(o) = ports_output {
            let out_str = String::from_utf8_lossy(&o.stdout);
            // ip link show master formato: "N: ifname: ..."
            for line in out_str.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() { continue; }
                // Solo lineas que empiezan con digito (indice)
                let first_char = trimmed.chars().next().unwrap_or(' ');
                if !first_char.is_ascii_digit() { continue; }
                // Extraer ifname despues del primer ':'
                if let Some(rest) = trimmed.splitn(2, ':').nth(1) {
                    let rest = rest.trim();
                    if let Some(iface) = rest.splitn(2, ':').next() {
                        let iface = iface.trim().to_string();
                        if iface.is_empty() || iface.len() > 20 { continue; }
                        let state = if line.contains("state UP") { "up" } else { "down" };
                        ports.push(PortEntry { iface, state: state.to_string() });
                    }
                }
            }
        }

        // Obtener detalle bridge ip -d link show
        let detail = Command::new("ip")
            .args(["-d", "link", "show", ifname])
            .output()
            .await;

        let detail_str = if let Ok(o) = detail {
            String::from_utf8_lossy(&o.stdout).to_string()
        } else {
            String::new()
        };

        let stp = detail_str.contains("stp_state 1");
        let mut priority = 32768u64;
        let mut ageing = 300u64;
        let mut max_age = 20u64;
        let mut fwd_delay = 15u64;

        if let Some(p) = detail_str.split("priority ").nth(1) {
            if let Some(v) = p.split_whitespace().next() {
                if let Ok(n) = v.parse::<u64>() {
                    priority = n;
                }
            }
        }
        if let Some(a) = detail_str.split("ageing_time ").nth(1) {
            if let Some(v) = a.split_whitespace().next() {
                // ageing_time en centisegundos, convertir a segundos
                if let Ok(n) = v.parse::<u64>() {
                    ageing = n / 100;
                }
            }
        }
        if let Some(m) = detail_str.split("max_age ").nth(1) {
            if let Some(v) = m.split_whitespace().next() {
                if let Ok(n) = v.parse::<u64>() {
                    max_age = n / 100;
                }
            }
        }
        if let Some(f) = detail_str.split("forward_delay ").nth(1) {
            if let Some(v) = f.split_whitespace().next() {
                if let Ok(n) = v.parse::<u64>() {
                    fwd_delay = n / 100;
                }
            }
        }

        bridges.push(BridgeEntry {
            name: ifname.to_string(),
            ports,
            mac: link["address"].as_str().unwrap_or("").to_string(),
            mtu: link["mtu"].as_u64().unwrap_or(1500),
            stp,
            priority,
            ageing,
            max_age,
            fwd_delay,
        });
    }

    Ok(Json(bridges))
}

pub async fn create(Json(body): Json<BridgeCreate>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if body.name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "nombre requerido".into()));
    }
    if body.name.chars().any(|c| !c.is_alphanumeric() && c != '_' && c != '-' && c != '.') {
        return Err((StatusCode::BAD_REQUEST, "nombre invalido".into()));
    }
    // P2: persistencia — antes el bridge se perdia al reboot
    persist_bridge(&body.name, &body.ports, false)?;

    // Crear bridge
    let add = Command::new("ip")
        .args(["link", "add", "name", &body.name, "type", "bridge"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !add.status.success() {
        let err = String::from_utf8_lossy(&add.stderr);
        return Err((StatusCode::BAD_REQUEST, err.to_string()));
    }

    // Agregar ports
    for port in &body.ports {
        let set = Command::new("ip")
            .args(["link", "set", "dev", port, "master", &body.name])
            .output()
            .await;

        if let Ok(o) = set {
            if !o.status.success() {
                let err = String::from_utf8_lossy(&o.stderr);
                eprintln!("Warning: no se pudo agregar {} al bridge {}: {}", port, body.name, err);
            }
        }

        // Levantar el port
        let _ = Command::new("ip")
            .args(["link", "set", port, "up"])
            .output()
            .await;
    }

    // Levantar bridge
    let _ = Command::new("ip")
        .args(["link", "set", &body.name, "up"])
        .output()
        .await;

    Ok(Json(serde_json::json!({"ok":true, "name": body.name})))
}

pub async fn delete(Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = body.get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "campo 'name' requerido".into()))?;

    if name == "bridgeLan" || name == "br0" {
        return Err((StatusCode::BAD_REQUEST, "no se puede eliminar bridge principal".into()));
    }
    // P0: SOLO permitir borrar si el nombre es un bridge REAL del sistema
    // (antes: `ip link delete eth0/eth3/wg0` eliminaba la interfaz fisica)
    let real = Command::new("ip")
        .args(["-j", "link", "show", "type", "bridge"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let is_bridge = String::from_utf8_lossy(&real.stdout)
        .contains(&format!("\"ifname\":\"{}\"", name));
    if !is_bridge {
        return Err((StatusCode::BAD_REQUEST,
            format!("'{}' no es un bridge real — negado para proteger la red", name)));
    }

    // P2: quitar persistencia (el bloque de interfaces)
    persist_bridge(name, &[], true)?;

    let del = Command::new("ip")
        .args(["link", "delete", name])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !del.status.success() {
        let err = String::from_utf8_lossy(&del.stderr);
        return Err((StatusCode::BAD_REQUEST, err.to_string()));
    }

    Ok(Json(serde_json::json!({"ok":true})))
}

#[derive(Debug, Deserialize)]
pub struct PortOp {
    pub bridge: String,
    pub port: String,
}

// P2: persistencia de bridges en /etc/network/interfaces (antes se perdian
// al reboot). remove=true quita el bloque.
fn persist_bridge(name: &str, ports: &[String], remove: bool) -> Result<(), (StatusCode, String)> {
    const NET_CONF: &str = "/etc/network/interfaces";
    static BRIDGE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = BRIDGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let content = std::fs::read_to_string(NET_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error leyendo {}: {}", NET_CONF, e)))?;

    // Quitar bloque existente (auto X / iface X inet manual ...)
    let mut out = String::new();
    let mut in_block = false;
    for line in content.lines() {
        let t = line.trim();
        if t == format!("auto {}", name) || t == format!("iface {} inet manual", name) {
            in_block = true;
            continue;
        }
        if in_block {
            if t.starts_with("auto ") || t.starts_with("iface ") || t.starts_with("allow-") {
                in_block = false;
            } else {
                continue; // linea indentada del bloque
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if in_block && !out.is_empty() && !out.ends_with('\n') { out.push('\n'); }

    if !remove {
        out.push_str(&format!("auto {}\niface {} inet manual\n", name, name));
        if !ports.is_empty() {
            out.push_str(&format!("    bridge_ports {}\n", ports.join(" ")));
        }
        out.push('\n');
    }

    // Escritura atomica
    let tmp = format!("{}.tmp-br-{}", NET_CONF, std::process::id());
    std::fs::write(&tmp, out.as_bytes())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    std::fs::rename(&tmp, NET_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

// P2: lee los ports persistidos (bridge_ports) del bloque del bridge en
// /etc/network/interfaces
fn read_bridge_ports(name: &str) -> Vec<String> {
    const NET_CONF: &str = "/etc/network/interfaces";
    let Ok(content) = std::fs::read_to_string(NET_CONF) else { return Vec::new(); };
    let mut in_block = false;
    for line in content.lines() {
        let t = line.trim();
        if t == format!("iface {} inet manual", name) {
            in_block = true;
            continue;
        }
        if in_block {
            if t.starts_with("auto ") || t.starts_with("iface ") || t.starts_with("allow-") {
                break;
            }
            if let Some(rest) = t.strip_prefix("bridge_ports") {
                return rest.split_whitespace().map(|s| s.to_string()).collect();
            }
        }
    }
    Vec::new()
}

pub async fn port_add(Json(body): Json<PortOp>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if body.port.is_empty() || body.bridge.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "bridge y port requeridos".into()));
    }
    let set = Command::new("ip")
        .args(["link", "set", "dev", &body.port, "master", &body.bridge])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !set.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&set.stderr).to_string()));
    }
    let _ = Command::new("ip").args(["link", "set", &body.port, "up"]).output().await;
    // P2: persistir el port en /etc/network/interfaces (antes solo runtime)
    let mut ports = read_bridge_ports(&body.bridge);
    if !ports.iter().any(|p| p == &body.port) {
        ports.push(body.port.clone());
        persist_bridge(&body.bridge, &ports, false)?;
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn port_remove(Json(body): Json<PortOp>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if body.port.is_empty() || body.bridge.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "bridge y port requeridos".into()));
    }
    let set = Command::new("ip")
        .args(["link", "set", "dev", &body.port, "nomaster"])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !set.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&set.stderr).to_string()));
    }
    // P2: quitar el port persistido (antes solo runtime)
    let mut ports = read_bridge_ports(&body.bridge);
    ports.retain(|p| p != &body.port);
    persist_bridge(&body.bridge, &ports, false)?;
    Ok(Json(serde_json::json!({"ok": true})))
}
