use axum::{Json, http::StatusCode};
use serde::Deserialize;
use std::net::IpAddr;

#[derive(Deserialize)]
pub struct Forwarder {
    pub address: String,
}

const RESOLV: &str = "/etc/resolv.conf";

fn read_resolv() -> Result<Vec<String>, String> {
    std::fs::read_to_string(RESOLV)
        .map(|s| s.lines().map(|l| l.to_string()).collect())
        .map_err(|e| e.to_string())
}

fn write_resolv(lines: &[String]) -> Result<(), String> {
    let tmp = format!("{}.tmp-{}", RESOLV, std::process::id());
    let content = lines.join("\n") + "\n";
    std::fs::write(&tmp, content).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, RESOLV).map_err(|e| e.to_string())
}

pub async fn list() -> Result<Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let lines = read_resolv().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let result: Vec<serde_json::Value> = lines
        .iter()
        .filter(|l| l.starts_with("nameserver "))
        .take(5)
        .map(|l| {
            let addr = l.trim_start_matches("nameserver ").trim();
            serde_json::json!({"address": addr, "type": "upstream"})
        })
        .collect();
    Ok(Json(result))
}

pub async fn add(body: Json<Forwarder>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let addr = body.address.trim();
    if addr.parse::<IpAddr>().is_err() {
        return Err((StatusCode::BAD_REQUEST, format!("'{}' no es una IP valida", addr)));
    }
    let mut lines = read_resolv().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if lines.iter().any(|l| l.as_str() == format!("nameserver {}", addr)) {
        return Ok(Json(serde_json::json!({"success": true, "address": addr, "duplicate": true})));
    }
    lines.push(format!("nameserver {}", addr));
    write_resolv(&lines).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"success": true, "address": addr})))
}

pub async fn delete(body: Json<Forwarder>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let addr = body.address.trim();
    if addr.parse::<IpAddr>().is_err() {
        return Err((StatusCode::BAD_REQUEST, format!("'{}' no es una IP valida", addr)));
    }
    let mut lines = read_resolv().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    lines.retain(|l| l.as_str() != format!("nameserver {}", addr));
    write_resolv(&lines).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"success": true, "address": addr})))
}
