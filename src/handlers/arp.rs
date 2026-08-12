use axum::{http::StatusCode, Json};
use tokio::process::Command;

pub async fn list() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("ip")
        .args(["neigh", "show"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        let parts: Vec<&str> = line.split_whitespace().collect();
        // ip neigh show: ADDR dev IFACE [lladdr MAC] [REACHABLE|STALE|...] [other flags]
        if parts.len() >= 2 {
            let addr = parts[0];
            let ifname = if parts.len() > 1 && parts[1] == "dev" { parts.get(2).unwrap_or(&"") } else { "" };
            // Buscar lladdr (MAC) y state
            let mut lladdr = "";
            let mut state = "";
            for i in 0..parts.len() {
                if parts[i] == "lladdr" && i + 1 < parts.len() { lladdr = parts[i + 1]; }
                if parts[i] == "REACHABLE" || parts[i] == "STALE" || parts[i] == "DELAY" || parts[i] == "PROBE" || parts[i] == "FAILED" || parts[i] == "INCOMPLETE" || parts[i] == "PERMANENT" || parts[i] == "NOARP" { state = parts[i]; }
            }
            // Solo IPv4
            if !addr.contains(':') {
            entries.push(serde_json::json!({
                "address": addr,
                "lladdr": lladdr,
                "state": state,
                "ifname": ifname,
            }));
            }
        }
    }
    Ok(Json(serde_json::json!([entries, {"rows": entries.len()}])))
}
