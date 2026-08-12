use axum::{extract::Path, http::StatusCode, Json};
use serde::Deserialize;
use tokio::process::Command;

const NET_INTERFACES: &str = "/etc/network/interfaces";

// P1: lock RMW — serializa sync_interfaces (dos handlers escribiendo
// /etc/network/interfaces a la vez perdían cambios)
static IPADDR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Deserialize)]
pub struct AddAddress {
    pub interface: String,
    pub address: String,
}

/// Sincroniza /etc/network/interfaces con un cambio de IP (runtime -> persistente).
/// Solo toca bloques "iface <iface> inet static". Si no existe el bloque,
/// el cambio es solo runtime (p.ej. loopback, wg0, ppp, VLANs manuales).
fn sync_interfaces(iface: &str, addr: &str, remove: bool) -> Result<(), String> {
    let _g = IPADDR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let content = std::fs::read_to_string(NET_INTERFACES)
        .map_err(|e| format!("Error leyendo {}: {}", NET_INTERFACES, e))?;
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    let target = format!("iface {} inet static", iface);
    let mut start: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        if l.trim() == target {
            start = Some(i);
            break;
        }
    }
    let Some(start) = start else {
        return Ok(()); // sin bloque static -> runtime only
    };

    // Fin del bloque: siguiente linea que no sea opcion indentada ni vacia
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        if !l.starts_with(' ') && !l.starts_with('\t') && !l.is_empty() {
            end = i;
            break;
        }
    }

    let addr_line = format!("    address {}", addr);
    if remove {
        lines.retain(|l| l != &addr_line);
    } else {
        let exists = lines[start..end].iter().any(|l| l.trim() == addr_line.trim());
        if exists {
            return Ok(());
        }
        lines.insert(start + 1, addr_line);
    }

    // P0/P1: escritura atomica (tmp+rename) — antes fs::write directo
    let tmp = format!("{}.tmp-{}", NET_INTERFACES, std::process::id());
    std::fs::write(&tmp, lines.join("\n") + "\n")
        .map_err(|e| format!("Error escribiendo {}: {}", NET_INTERFACES, e))?;
    std::fs::rename(&tmp, NET_INTERFACES)
        .map_err(|e| format!("Error renombrando {}: {}", NET_INTERFACES, e))
}

/// Valida "ip/prefix" (IPv4 o IPv6) y nombre de interfaz segura.
fn valid_cidr(addr: &str) -> bool {
    let Some((ip, prefix)) = addr.split_once('/') else { return false; };
    if ip.parse::<std::net::Ipv4Addr>().is_err() && ip.parse::<std::net::Ipv6Addr>().is_err() {
        return false;
    }
    prefix.parse::<u8>().map(|p| p <= 128).unwrap_or(false)
}

fn valid_iface(iface: &str) -> bool {
    !iface.is_empty()
        && iface.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
        && !iface.starts_with("ppp")
        && !iface.starts_with("ifb")
        && iface != "lo"
}

pub async fn list() -> Result<Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let output = Command::new("ip")
        .args(["-json", "addr", "show"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    #[derive(Deserialize)]
    struct AddrInfo {
        ifname: String,
        addr_info: Vec<AddrDetail>,
    }
    #[derive(Deserialize)]
    struct AddrDetail {
        local: String,
        family: String,
    }

    let addrs: Vec<AddrInfo> = serde_json::from_slice(&output.stdout)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let result: Vec<serde_json::Value> = addrs
        .into_iter()
        .filter(|a| {
            // Excluir interfaces dinamicas PPP e ifb (intermedias tc)
            !a.ifname.starts_with("ppp") && a.ifname != "ifb_eth3" && !a.ifname.starts_with("ifb_ppp")
        })
        .flat_map(|a| {
            a.addr_info
                .into_iter()
                .filter(|ai| ai.family == "inet")
                .map(move |ai| {
                    serde_json::json!({
                        "interface": a.ifname,
                        "address": ai.local,
                        "family": ai.family,
                    })
                })
        })
        .collect();

    Ok(Json(result))
}

pub async fn add(body: Json<AddAddress>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // P1: validar CIDR y iface ANTES de tocar el sistema (antes aceptaba
    // IPs en lo/eth0/ppp* → rompia MWAN o rutas)
    if !valid_cidr(&body.address) {
        return Err((StatusCode::BAD_REQUEST, format!("address invalida (CIDR): {}", body.address)));
    }
    if !valid_iface(&body.interface) {
        return Err((StatusCode::BAD_REQUEST, format!("interface invalida: {}", body.interface)));
    }
    let output = Command::new("ip")
        .args(["addr", "add", &body.address, "dev", &body.interface])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err((StatusCode::BAD_REQUEST, stderr.to_string()));
    }

    // Persistir en /etc/network/interfaces (best-effort)
    let _ = sync_interfaces(&body.interface, &body.address, false);

    Ok(Json(serde_json::json!({
        "success": true,
        "interface": body.interface,
        "address": body.address,
    })))
}

pub async fn delete(
    Path((ifname, addr)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // URL decode simple: reemplazar %2F por /
    let decoded = addr.replace("%2F", "/").replace("%2f", "/");
    // P1: validar antes de tocar (misma regla que add)
    if !valid_cidr(&decoded) {
        return Err((StatusCode::BAD_REQUEST, format!("address invalida (CIDR): {}", decoded)));
    }
    if !valid_iface(&ifname) {
        return Err((StatusCode::BAD_REQUEST, format!("interface invalida: {}", ifname)));
    }

    let output = Command::new("ip")
        .args(["addr", "del", decoded.as_ref(), "dev", &ifname])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err((StatusCode::BAD_REQUEST, stderr.to_string()));
    }

    // Quitar de /etc/network/interfaces (best-effort)
    let _ = sync_interfaces(&ifname, &decoded, true);

    Ok(Json(serde_json::json!({
        "success": true,
        "interface": ifname,
        "address": &decoded,
    })))
}
