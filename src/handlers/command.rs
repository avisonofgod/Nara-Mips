use axum::{http::StatusCode, Json};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Write;
use tokio::process::Command;

use crate::routeros_parser;

const DNSMASQ_CONF: &str = "/etc/dnsmasq.conf";

#[derive(Deserialize)]
pub struct CmdRequest {
    pub cmd: String,
}

pub async fn run(body: Json<CmdRequest>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let cmd = body.cmd.trim();
    // Scripts del panel (/system/scripts): "sh <path>" SOLO si path esta en
    // la whitelist (mismos dirs que /api/system/scripts)
    if let Some(spath) = cmd.strip_prefix("sh ") {
        let spath = spath.trim();
        if is_allowed_script(spath) {
            return run_script(spath).await;
        }
        return Err((StatusCode::FORBIDDEN, format!("Script no permitido: {}", spath)));
    }

    let parsed = routeros_parser::parse(cmd)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, format!("Comando invalido: {}", cmd)))?;

    match parsed.path.as_str() {
        "/ip/address/add" => cmd_ip_address_add(&parsed.args).await,
        "/ip/address/remove" => cmd_ip_address_remove(&parsed.args).await,
        "/ip/address/print" => cmd_ip_address_print().await,
        "/ip/route/add" => cmd_ip_route_add(&parsed.args).await,
        "/ip/route/remove" => cmd_ip_route_remove(&parsed.args).await,
        "/ip/route/print" => cmd_ip_route_print().await,
        "/ip/pool/add" => cmd_ip_pool_add(&parsed.args).await,
        "/ip/pool/remove" => cmd_ip_pool_remove(&parsed.args).await,
        "/ip/pool/print" => cmd_ip_pool_print().await,
        "/ip/dhcp-server/add" => cmd_dhcp_server_add(&parsed.args).await,
        "/ip/dhcp-server/remove" => cmd_dhcp_server_remove(&parsed.args).await,
        "/ip/dhcp-server/print" => cmd_dhcp_server_print().await,
        "/ip/dhcp-server/lease/print" => cmd_dhcp_lease_print().await,
        "/interface/wireguard/add" => cmd_wireguard_add(&parsed.args).await,
        "/interface/wireguard/remove" => cmd_wireguard_remove(&parsed.args).await,
        "/interface/wireguard/print" => cmd_wireguard_print().await,
        "/interface/wireguard/peers/print" => cmd_wireguard_peers_print().await,
        "/interface/wireguard/peers/add" => cmd_wireguard_peers_add(&parsed.args).await,
        "/interface/wireguard/peers/remove" => cmd_wireguard_peers_remove(&parsed.args).await,
        "/interface/bridge/add" => cmd_bridge_add(&parsed.args).await,
        "/interface/bridge/remove" => cmd_bridge_remove(&parsed.args).await,
        "/interface/bridge/print" => cmd_bridge_print().await,
        "/interface/bridge/port/add" => cmd_bridge_port_add(&parsed.args).await,
        "/interface/bridge/port/remove" => cmd_bridge_port_remove(&parsed.args).await,
        "/interface/bridge/port/print" => cmd_bridge_port_print().await,
        "/ip/route/table/add" => cmd_mwan_table_add(&parsed.args).await,
        "/ip/route/table/print" => cmd_mwan_table_print().await,
        "/ip/firewall/filter/add" => cmd_firewall_filter_add(&parsed.args).await,
        "/ip/firewall/filter/remove" => cmd_firewall_filter_remove(&parsed.args).await,
        "/ip/firewall/filter/print" => cmd_firewall_filter_print().await,
        "/ip/firewall/nat/add" => cmd_firewall_nat_add(&parsed.args).await,
        "/ip/firewall/nat/remove" => cmd_firewall_nat_remove(&parsed.args).await,
        "/ip/firewall/nat/print" => cmd_firewall_nat_print().await,
        "/interface/print" => cmd_interface_print().await,
        "/interface/vlan/add" => cmd_vlan_add(&parsed.args).await,
        "/interface/vlan/remove" => cmd_vlan_remove(&parsed.args).await,
        "/interface/vlan/print" => cmd_vlan_print().await,
        _ => Err((StatusCode::NOT_FOUND, format!("Comando no implementado: {}", parsed.path))),
    }
}

/// Whitelist de scripts ejecutables desde el panel: mismos dirs que
/// /api/system/scripts (scripts/ del proyecto + watchdog ppp-*).
/// P0: se CANONICALIZA la ruta antes de comparar (evita traversal con ../).
fn is_allowed_script(path: &str) -> bool {
    let canon = std::fs::canonicalize(path)
        .unwrap_or_else(|_| std::path::PathBuf::from(path));
    let canon = canon.to_string_lossy().to_string();
    (canon.starts_with("/root/zpot-rs/scripts/") && canon.ends_with(".sh"))
        || (canon.starts_with("/usr/local/bin/ppp-") && canon.ends_with(".sh"))
}

/// Ejecuta un script del panel y devuelve JSON (output/stderr/exit).
/// Los scripts de reboot se lanzan SIN esperar (el sistema muere antes de
/// poder responder); el resto espera la salida completa con timeout 30s.
async fn run_script(path: &str) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // P0: SOLO fire-and-forget si el NOMBRE EXACTO es el script de reboot
    // (antes: cualquier ruta que contuviera "reboot" disparaba sin esperar)
    let fname = std::path::Path::new(path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("");
    if fname == "reboot-alpine.sh" {
        let _ = Command::new("sh").arg(path).spawn();
        return Ok(Json(serde_json::json!({
            "ok": true,
            "output": "REBOOT lanzado — el servidor se reinicia, la conexion se cortara unos segundos",
        })));
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        Command::new("sh").arg(path).output(),
    )
    .await
    .map_err(|_| (StatusCode::GATEWAY_TIMEOUT, "timeout ejecutando script (30s)".into()))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "output": String::from_utf8_lossy(&output.stdout).trim().to_string(),
        "stderr": String::from_utf8_lossy(&output.stderr).trim().to_string(),
        "exit": output.status.code().unwrap_or(-1),
    })))
}

// ==================== IP ADDRESS ====================

async fn cmd_ip_address_add(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let iface = args.get("interface").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta interface".into()))?;
    let addr = args.get("address").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta address".into()))?;
    run_ip_cmd(&["addr", "add", addr, "dev", iface]).await
}

async fn cmd_ip_address_remove(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let iface = args.get("interface").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta interface".into()))?;
    let addr = args.get("address").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta address".into()))?;
    run_ip_cmd(&["addr", "del", addr, "dev", iface]).await
}

async fn cmd_ip_address_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("ip")
        .args(["-json", "addr", "show"])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let addrs: Vec<serde_json::Value> = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .into_iter()
        .flat_map(|iface_val| {
            let name = iface_val.get("ifname").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let addr_info = iface_val.get("addr_info").and_then(|a| a.as_array())
                .cloned()
                .unwrap_or_default();
            addr_info.into_iter()
                .map(move |addr| serde_json::json!({
                    "interface": name.clone(),
                    "address": addr.get("local").and_then(|v| v.as_str()).unwrap_or(""),
                    "prefix": addr.get("prefixlen").and_then(|v| v.as_u64()).unwrap_or(0),
                    "scope": addr.get("scope").and_then(|v| v.as_str()).unwrap_or(""),
                }))
        })
        .collect();
    Ok(Json(serde_json::json!([addrs, {"rows": addrs.len()}])))
}

// ==================== IP ROUTE ====================

async fn cmd_ip_route_add(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let dst = args.get("dst-address").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta dst-address".into()))?;
    let gw = args.get("gateway").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta gateway".into()))?;
    let table = args.get("table").map(|s| s.as_str()).unwrap_or("main");
    let mut cmd_args = vec!["route", "add", dst, "via", gw];
    if table != "main" {
        cmd_args.extend_from_slice(&["table", table]);
    }
    run_ip_cmd(&cmd_args).await
}

async fn cmd_ip_route_remove(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let dst = args.get("dst-address").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta dst-address".into()))?;
    let gw = args.get("gateway").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta gateway".into()))?;
    run_ip_cmd(&["route", "del", dst, "via", gw]).await
}

async fn cmd_ip_route_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("ip")
        .args(["-json", "route", "show"])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let routes: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let result: Vec<serde_json::Value> = routes.into_iter().map(|r| {
        serde_json::json!({
            "dst": r.get("dst").and_then(|v| v.as_str()).unwrap_or("0.0.0.0/0"),
            "gateway": r.get("gateway").and_then(|v| v.as_str()).unwrap_or(""),
            "dev": r.get("dev").and_then(|v| v.as_str()).unwrap_or(""),
            "table": r.get("table").and_then(|v| v.as_str()).unwrap_or("main"),
            "protocol": r.get("protocol").and_then(|v| v.as_str()).unwrap_or(""),
        })
    }).collect();
    Ok(Json(serde_json::json!([result, {"rows": result.len()}])))
}

// ==================== POOL DHCP ====================

async fn cmd_ip_pool_add(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = args.get("name").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta name".into()))?;
    let ranges = args.get("ranges").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta ranges".into()))?;
    let conf = format!("\n# Pool: {}\ndhcp-range={},{}", name, ranges.replace("-", ","), "12h");
    std::fs::OpenOptions::new().append(true).open(DNSMASQ_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .write_all(conf.as_bytes())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    reload_dnsmasq().await
}

async fn cmd_ip_pool_remove(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = args.get("name").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta name".into()))?;
    let content = std::fs::read_to_string(DNSMASQ_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let _new_content: Vec<&str> = content.lines()
        .skip_while(|l| l.trim() != &format!("# Pool: {}", name))
        .skip(1)
        .skip_while(|l| !l.starts_with("#"))
        .collect();
    // Simple: remove lines containing the pool name and its dhcp-range
    let filtered: String = content.lines()
        .filter(|l| !l.contains(&format!("# Pool: {}", name)) && !l.contains(&format!("Pool: {}", name)))
        .collect::<Vec<&str>>()
        .join("\n");
    std::fs::write(DNSMASQ_CONF, &filtered)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    reload_dnsmasq().await
}

async fn cmd_ip_pool_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let content = std::fs::read_to_string(DNSMASQ_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut pools = Vec::new();
    let mut current_name = String::new();
    for line in content.lines() {
        if line.starts_with("# Pool: ") {
            current_name = line.trim_start_matches("# Pool: ").to_string();
        } else if line.starts_with("dhcp-range=") && !current_name.is_empty() {
            let range = line.trim_start_matches("dhcp-range=");
            pools.push(serde_json::json!({
                "name": current_name,
                "ranges": range,
            }));
            current_name.clear();
        }
    }
    Ok(Json(serde_json::json!([pools, {"rows": pools.len()}])))
}

// ==================== DHCP SERVER ====================

async fn cmd_dhcp_server_add(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let iface = args.get("interface").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta interface".into()))?;
    let pool = args.get("address-pool").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta address-pool".into()))?;
    let lease_time = args.get("lease-time").map(|s| s.as_str()).unwrap_or("12h");
    let conf = format!("\n# DHCP Server: {}\ninterface={}\ndhcp-range=tag:{},{}", iface, iface, pool, lease_time);
    std::fs::OpenOptions::new().append(true).open(DNSMASQ_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .write_all(conf.as_bytes())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    reload_dnsmasq().await
}

async fn cmd_dhcp_server_remove(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let iface = args.get("interface").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta interface".into()))?;
    let content = std::fs::read_to_string(DNSMASQ_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let filtered: String = content.lines()
        .filter(|l| !l.contains(&format!("# DHCP Server: {}", iface)) && !l.contains(&format!("interface={}", iface)))
        .collect::<Vec<&str>>()
        .join("\n");
    std::fs::write(DNSMASQ_CONF, &filtered)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    reload_dnsmasq().await
}

async fn cmd_dhcp_server_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let content = std::fs::read_to_string(DNSMASQ_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut servers = Vec::new();
    let mut current_iface = String::new();
    for line in content.lines() {
        if line.starts_with("# DHCP Server: ") {
            current_iface = line.trim_start_matches("# DHCP Server: ").to_string();
        } else if line.starts_with("interface=") && !current_iface.is_empty() {
            let ifname = line.trim_start_matches("interface=");
            servers.push(serde_json::json!({
                "interface": ifname,
                "address-pool": current_iface,
                "lease-time": "12h",
            }));
            current_iface.clear();
        }
    }
    Ok(Json(serde_json::json!([servers, {"rows": servers.len()}])))
}

async fn cmd_dhcp_lease_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("cat")
        .arg("/var/lib/misc/dnsmasq.leases")
        .output().await;
    let stdout = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Ok(Json(serde_json::json!([[], {"rows": 0}]))),
    };
    let leases: Vec<serde_json::Value> = stdout.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            serde_json::json!({
                "expires": if parts.len() > 0 { parts[0] } else { "" },
                "mac": if parts.len() > 1 { parts[1] } else { "" },
                "ip": if parts.len() > 2 { parts[2] } else { "" },
                "hostname": if parts.len() > 3 { parts[3] } else { "" },
            })
        })
        .collect();
    Ok(Json(serde_json::json!([leases, {"rows": leases.len()}])))
}

// ==================== WIREGUARD ====================

async fn cmd_wireguard_add(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = args.get("name").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta name".into()))?;
    let listen_port = args.get("listen-port").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta listen-port".into()))?;
    let private_key = args.get("private-key").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta private-key".into()))?;
    let output = Command::new("ip")
        .args(["link", "add", name, "type", "wireguard"])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    let key_output = Command::new("wg")
        .args(["set", name, "listen-port", listen_port, "private-key", private_key])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    // P0: si wg set falla, NO dejar interfaz zombie — rollback y error claro
    if !key_output.status.success() {
        let _ = Command::new("ip").args(["link", "del", name]).output().await;
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&key_output.stderr).to_string()));
    }
    Command::new("ip").args(["link", "set", "dev", name, "up"]).output().await.ok();
    Ok(Json(serde_json::json!({"success": true, "interface": name})))
}

async fn cmd_wireguard_remove(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = args.get("name").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta name".into()))?;
    run_ip_cmd(&["link", "del", name]).await
}

async fn cmd_wireguard_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("wg")
        .args(["show", "all", "dump"])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut interfaces = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() { continue; }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            interfaces.push(serde_json::json!({
                "interface": parts[0],
                "private-key": "***",
                "public-key": parts[1],
                "listen-port": parts[2],
                "fwmark": parts.get(3).unwrap_or(&""),
            }));
        }
    }
    Ok(Json(serde_json::json!([interfaces, {"rows": interfaces.len()}])))
}

async fn cmd_wireguard_peers_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("wg")
        .args(["show", "all", "dump"])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut peers = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() { continue; }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 5 {
            peers.push(serde_json::json!({
                "interface": parts[0],
                "public-key": parts[1],
                "endpoint": parts.get(3).unwrap_or(&""),
                "allowed-ips": parts.get(4).unwrap_or(&""),
                "latest-handshake": parts.get(5).unwrap_or(&""),
                "transfer-rx": parts.get(6).unwrap_or(&""),
                "transfer-tx": parts.get(7).unwrap_or(&""),
            }));
        }
    }
    Ok(Json(serde_json::json!([peers, {"rows": peers.len()}])))
}

async fn cmd_wireguard_peers_add(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let iface = args.get("interface").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta interface".into()))?;
    let pubkey = args.get("public-key").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta public-key".into()))?;
    let allowed_ips = args.get("allowed-ips").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta allowed-ips".into()))?;
    let endpoint = args.get("endpoint").map(|s| s.as_str()).unwrap_or("");
    let mut wg_args = vec!["set", iface, "peer", pubkey, "allowed-ips", allowed_ips];
    if !endpoint.is_empty() {
        wg_args.extend_from_slice(&["endpoint", endpoint]);
    }
    let output = Command::new("wg").args(&wg_args).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

async fn cmd_wireguard_peers_remove(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let iface = args.get("interface").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta interface".into()))?;
    let pubkey = args.get("public-key").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta public-key".into()))?;
    let output = Command::new("wg").args(["set", iface, "peer", pubkey, "remove"]).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

// ==================== BRIDGE ====================

async fn cmd_bridge_add(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = args.get("name").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta name".into()))?;
    let _ = run_ip_cmd(&["link", "add", name, "type", "bridge"]).await?;
    Command::new("ip").args(["link", "set", "dev", name, "up"]).output().await.ok();
    Ok(Json(serde_json::json!({"success": true, "interface": name})))
}

async fn cmd_bridge_remove(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = args.get("name").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta name".into()))?;
    run_ip_cmd(&["link", "del", name]).await
}

async fn cmd_bridge_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("ip")
        .args(["-json", "link", "show", "type", "bridge"])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let bridges: Vec<serde_json::Value> = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .into_iter().map(|b| {
            serde_json::json!({
                "name": b.get("ifname").and_then(|v| v.as_str()).unwrap_or(""),
                "mac": b.get("address").and_then(|v| v.as_str()).unwrap_or(""),
                "mtu": b.get("mtu").and_then(|v| v.as_u64()).unwrap_or(1500),
                "status": b.get("operstate").and_then(|v| v.as_str()).unwrap_or(""),
            })
        }).collect();
    Ok(Json(serde_json::json!([bridges, {"rows": bridges.len()}])))
}

async fn cmd_bridge_port_add(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let bridge = args.get("bridge").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta bridge".into()))?;
    let iface = args.get("interface").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta interface".into()))?;
    run_ip_cmd(&["link", "set", iface, "master", bridge]).await
}

async fn cmd_bridge_port_remove(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let iface = args.get("interface").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta interface".into()))?;
    run_ip_cmd(&["link", "set", iface, "nomaster"]).await
}

async fn cmd_bridge_port_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("bridge")
        .args(["-json", "link", "show"])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let ports: Vec<serde_json::Value> = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout)
        .unwrap_or_default()
        .into_iter().map(|p| {
            serde_json::json!({
                "interface": p.get("ifname").and_then(|v| v.as_str()).unwrap_or(""),
                "master": p.get("master").and_then(|v| v.as_str()).unwrap_or(""),
                "state": p.get("state").and_then(|v| v.as_str()).unwrap_or(""),
            })
        }).collect();
    Ok(Json(serde_json::json!([ports, {"rows": ports.len()}])))
}

// ==================== MWAN (Route Tables) ====================

async fn cmd_mwan_table_add(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = args.get("name").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta name".into()))?;
    let table_id = args.get("table-id").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta table-id".into()))?;
    // Check if already exists in /etc/iproute2/rt_tables
    let rt_tables = std::fs::read_to_string("/etc/iproute2/rt_tables")
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "no se puede leer rt_tables".into()))?;
    if !rt_tables.lines().any(|l| l.trim().ends_with(name)) {
        let entry = format!("\n{} {}", table_id, name);
        std::fs::OpenOptions::new().append(true).open("/etc/iproute2/rt_tables")
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .write_all(entry.as_bytes())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    Ok(Json(serde_json::json!({"success": true})))
}

async fn cmd_mwan_table_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let content = std::fs::read_to_string("/etc/iproute2/rt_tables")
        .unwrap_or_default();
    let tables: Vec<serde_json::Value> = content.lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with("#") && t.contains(char::is_numeric)
        })
        .map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            serde_json::json!({
                "id": parts.first().unwrap_or(&""),
                "name": parts.get(1).unwrap_or(&""),
            })
        }).collect();
    Ok(Json(serde_json::json!([tables, {"rows": tables.len()}])))
}

// ==================== FIREWALL ====================

async fn cmd_firewall_filter_add(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let chain = args.get("chain").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta chain".into()))?;
    let action = args.get("action").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta action".into()))?;
    let mut rule = format!("add rule inet filter {} {} ", chain, action);
    if let Some(in_iface) = args.get("in-interface") {
        rule.push_str(&format!("iif {} ", in_iface));
    }
    if let Some(out_iface) = args.get("out-interface") {
        rule.push_str(&format!("oif {} ", out_iface));
    }
    if let Some(src) = args.get("src-address") {
        rule.push_str(&format!("ip saddr {} ", src));
    }
    if let Some(dst) = args.get("dst-address") {
        rule.push_str(&format!("ip daddr {} ", dst));
    }
    if let Some(proto) = args.get("protocol") {
        rule.push_str(&format!("{} ", proto));
    }
    let output = Command::new("nft").args(&rule.split_whitespace().collect::<Vec<&str>>()).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

async fn cmd_firewall_filter_remove(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let handle = args.get("handle").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta handle".into()))?;
    let chain = args.get("chain").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta chain".into()))?;
    let output = Command::new("nft").args(["delete", "rule", "inet", "filter", chain, "handle", handle]).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

async fn cmd_firewall_filter_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    list_nft_rules("filter").await
}

async fn cmd_firewall_nat_add(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let chain = args.get("chain").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta chain".into()))?;
    let action = args.get("action").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta action".into()))?;
    let mut rule = format!("add rule inet nat {} {} ", chain, action);
    if let Some(out_iface) = args.get("out-interface") {
        rule.push_str(&format!("oif {} ", out_iface));
    }
    if let Some(src) = args.get("src-address") {
        rule.push_str(&format!("ip saddr {} ", src));
    }
    if let Some(to_src) = args.get("to-src") {
        rule.push_str(&format!("snat to {} ", to_src));
    }
    if let Some(to_dst) = args.get("to-dst") {
        rule.push_str(&format!("dnat to {} ", to_dst));
    }
    let output = Command::new("nft").args(&rule.split_whitespace().collect::<Vec<&str>>()).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

async fn cmd_firewall_nat_remove(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let handle = args.get("handle").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta handle".into()))?;
    let chain = args.get("chain").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta chain".into()))?;
    let output = Command::new("nft").args(["delete", "rule", "inet", "nat", chain, "handle", handle]).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

async fn cmd_firewall_nat_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    list_nft_rules("nat").await
}

// ==================== INTERFACES ====================

async fn cmd_interface_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("ip")
        .args(["-json", "link", "show"])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let links: Vec<serde_json::Value> = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let result: Vec<serde_json::Value> = links.into_iter().map(|l| {
        serde_json::json!({
            "ifname": l.get("ifname").and_then(|v| v.as_str()).unwrap_or(""),
            "operstate": l.get("operstate").and_then(|v| v.as_str()).unwrap_or(""),
            "mac": l.get("address").and_then(|v| v.as_str()).unwrap_or(""),
            "mtu": l.get("mtu").and_then(|v| v.as_u64()).unwrap_or(1500),
            "type": l.get("link_type").and_then(|v| v.as_str()).unwrap_or(""),
            "master": l.get("master").and_then(|v| v.as_str()).unwrap_or(""),
        })
    }).collect();
    Ok(Json(serde_json::json!([result, {"rows": result.len()}])))
}

// ==================== VLANS ====================

async fn cmd_vlan_add(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let parent = args.get("parent").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta parent".into()))?;
    let vlan_id = args.get("id").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta id".into()))?;
    let name = args.get("name").map(|s| s.as_str()).unwrap_or("");
    let iface_name = if name.is_empty() { format!("{}.{}", parent, vlan_id) } else { name.to_string() };
    let output = Command::new("ip")
        .args(["link", "add", "link", parent, "name", &iface_name, "type", "vlan", "id", vlan_id])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Command::new("ip").args(["link", "set", "dev", &iface_name, "up"]).output().await.ok();
    Ok(Json(serde_json::json!({"success": true, "interface": iface_name})))
}

async fn cmd_vlan_remove(args: &HashMap<String, String>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = args.get("name").ok_or_else(|| (StatusCode::BAD_REQUEST, "falta name".into()))?;
    run_ip_cmd(&["link", "del", name]).await
}

async fn cmd_vlan_print() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("ip")
        .args(["-d", "link", "show"])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut vlans = Vec::new();
    for line in stdout.lines() {
        if line.contains("vlan protocol") {
            let name = line.split(':').nth(1).unwrap_or("").trim()
                .split('@').next().unwrap_or("").to_string();
            let vlan_id: u64 = line.split("id ").nth(1)
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let parent = line.split(':').nth(1).unwrap_or("").trim()
                .split('@').nth(1).unwrap_or("").to_string();
            let status = if line.contains("state UP") { "up" } else { "down" };
            vlans.push(serde_json::json!({
                "name": name,
                "id": vlan_id,
                "parent": parent,
                "status": status,
            }));
        }
    }
    let count = vlans.len();
    Ok(Json(serde_json::json!([vlans, {"rows": count}])))
}

// ==================== HELPERS ====================

async fn run_ip_cmd(args: &[&str]) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("ip").args(args).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !output.status.success() {
        return Err((StatusCode::BAD_REQUEST, String::from_utf8_lossy(&output.stderr).to_string()));
    }
    Ok(Json(serde_json::json!({"success": true})))
}

async fn list_nft_rules(table: &str) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let output = Command::new("nft")
        .args(["-j", "list", "table", "inet", table])
        .output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let data: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error parseando nft: {}", e)))?;
    let mut rules = Vec::new();
    if let Some(nftables) = data.get("nftables").and_then(|a| a.as_array()) {
        for entry in nftables {
            if let Some(chain) = entry.get("chain") {
                let chain_name = chain.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(rules_arr) = entry.get("rules").and_then(|a| a.as_array()) {
                    for rule in rules_arr {
                        let handle = rule.get("handle").and_then(|v| v.as_u64()).unwrap_or(0);
                        rules.push(serde_json::json!({
                            "chain": chain_name,
                            "handle": handle,
                            "expr": "nft",
                        }));
                    }
                }
            }
        }
    }
    Ok(Json(serde_json::json!([rules, {"rows": rules.len()}])))
}

async fn reload_dnsmasq() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let ok = crate::handlers::helpers::service_action("dnsmasq", "reload").await;
    if !ok {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Error recargando dnsmasq (rc-service/init.d)")));
    }
    Ok(Json(serde_json::json!({"success": true})))
}
