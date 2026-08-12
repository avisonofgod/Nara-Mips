use axum::{http::StatusCode, Json};
use tokio::process::Command;

pub async fn list() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("cat")
        .arg("/var/lib/misc/dnsmasq.leases")
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut leases = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            // Formato dnsmasq: expiry mac ip hostname [hostname-con-espacios] clientid
            // FIX (2026-08-08): el clientid es SIEMPRE la ultima parte — antes
            // parts[3..].join(" ") lo pegaba al hostname ("* 01:xx" o
            // "nombre 01:xx") y el buscador por hostname fallaba.
            let id = parts[parts.len() - 1];
            let hostname_raw = parts[3..parts.len() - 1].join(" ");
            // dnsmasq usa "*" cuando el cliente no envio hostname
            let hostname = if hostname_raw == "*" || hostname_raw.is_empty() { String::new() } else { hostname_raw };
            leases.push(serde_json::json!({
                "expires": parts[0],
                "mac": parts[1],
                "ip": parts[2],
                "hostname": hostname,
                "id": id,
            }));
        }
    }
    Ok(Json(serde_json::json!([leases, {"rows": leases.len()}])))
}
