use std::sync::Mutex;
use axum::{Json, http::StatusCode};
use serde::{Deserialize, Serialize};

const RADIUS_PATH: &str = "/etc/zpot/radius-servers.json";

static RADIUS_SERVERS: std::sync::OnceLock<Mutex<Vec<RadiusServer>>> = std::sync::OnceLock::new();

fn radius_servers() -> &'static Mutex<Vec<RadiusServer>> {
    RADIUS_SERVERS.get_or_init(|| {
        // Intentar cargar desde disco
        if let Ok(data) = std::fs::read_to_string(RADIUS_PATH) {
            if let Ok(servers) = serde_json::from_str::<Vec<RadiusServer>>(&data) {
                return Mutex::new(servers);
            }
        }
        // Fallback: servidor por defecto
        Mutex::new(vec![RadiusServer {
            name: "radius-main".into(),
            r#type: "auth".into(),
            ip: "161.97.67.63".into(),
            auth_port: 1812,
            acct_port: 1813,
            secret: "85River@B".into(),
            // #11 (2026-08-08): timeout 6s (antes 3) — el server tarda en
            // responder (SQL) y el pppd marcaba "Interim accounting failed"
            timeout: 6,
            retries: 2,
            status: "up".into(),
        }])
    })
}

fn save_servers(servers: &[RadiusServer]) {
    if let Some(parent) = std::path::Path::new(RADIUS_PATH).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(servers) {
        // P1: escritura atomica (tmp+rename)
        let tmp = format!("{}.tmp-{}", RADIUS_PATH, std::process::id());
        let _ = std::fs::write(&tmp, &json);
        let _ = std::fs::rename(&tmp, RADIUS_PATH);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadiusServer {
    pub name: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub ip: String,
    #[serde(rename = "authPort")]
    pub auth_port: u16,
    #[serde(rename = "acctPort")]
    pub acct_port: u16,
    pub secret: String,
    pub timeout: u32,
    pub retries: u32,
    pub status: String,
}

pub async fn get_servers() -> Json<Vec<serde_json::Value>> {
    let servers = radius_servers().lock().unwrap_or_else(|e| e.into_inner()).clone();
    // P0: enmascarar el secret en la API (antes se devolvia completo)
    let masked: Vec<serde_json::Value> = servers.into_iter().map(|s| {
        serde_json::json!({
            "name": s.name,
            "type": s.r#type,
            "ip": s.ip,
            "authPort": s.auth_port,
            "acctPort": s.acct_port,
            "secret": "***",
            "timeout": s.timeout,
            "retries": s.retries,
            "status": s.status,
        })
    }).collect();
    Json(masked)
}

pub async fn post_server(
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let new: RadiusServer = serde_json::from_slice(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("JSON invalido: {}", e)))?;
    // P0: validar campos + evitar duplicados (re-POST con mismo name = update)
    if new.name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name requerido".into()));
    }
    if new.ip.parse::<std::net::Ipv4Addr>().is_err() {
        return Err((StatusCode::BAD_REQUEST, format!("ip invalida: {}", new.ip)));
    }
    if new.auth_port == 0 || new.acct_port == 0 {
        return Err((StatusCode::BAD_REQUEST, "puertos invalidos".into()));
    }
    if new.secret.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "secret requerido".into()));
    }
    let mut servers = radius_servers().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = servers.iter_mut().find(|s| s.name == new.name) {
        *existing = new; // update por nombre (antes agregaba duplicado)
    } else {
        servers.push(new);
    }
    save_servers(&servers);
    Ok(Json(serde_json::json!({"status": "ok"})))
}

/// DELETE /api/radius/servers — elimina un servidor por nombre.
/// P2: antes NO existia DELETE (pendiente del changelog RADIUS).
/// Protege el fallback "radius-main" implicito: si es el unico y el archivo
/// no existe en disco, se niega (el sistema depende de el para PPP).
pub async fn delete_server(
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let payload: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("JSON invalido: {}", e)))?;
    let name = payload.get("name").and_then(|v| v.as_str())
        .ok_or((StatusCode::BAD_REQUEST, "name requerido".to_string()))?;
    let mut servers = radius_servers().lock().unwrap_or_else(|e| e.into_inner());
    let before = servers.len();
    servers.retain(|s| s.name != name);
    if servers.len() == before {
        return Err((StatusCode::NOT_FOUND, format!("servidor '{}' no existe", name)));
    }
    // No permitir dejar la lista vacia si el archivo no existe (el sistema
    // PPP se quedaria sin servidor). Si queda vacia, restaurar el fallback.
    if servers.is_empty() && !std::path::Path::new(RADIUS_PATH).exists() {
        servers.push(RadiusServer {
            name: "radius-main".into(),
            r#type: "auth".into(),
            ip: "161.97.67.63".into(),
            auth_port: 1812,
            acct_port: 1813,
            secret: "85River@B".into(),
            timeout: 6,
            retries: 2,
            status: "up".into(),
        });
        return Err((StatusCode::BAD_REQUEST,
            "no se puede eliminar el ultimo servidor (fallback del sistema)".into()));
    }
    save_servers(&servers);
    Ok(Json(serde_json::json!({"status": "ok", "deleted": name})))
}

/// Resuelve un servidor RADIUS por nombre (para NAS PPP). Consulta la lista
/// en memoria (que incluye fallback hardcodeado + POSTs guardados).
pub fn get_server_by_name(name: &str) -> Option<RadiusServer> {
    radius_servers()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|s| s.name == name)
        .cloned()
}

/// Devuelve el primer servidor auth disponible (para NAS PPP sin nombre).
pub fn get_default_auth_server() -> Option<RadiusServer> {
    radius_servers()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|s| s.r#type == "auth")
        .cloned()
}
