use axum::{http::StatusCode, Json};
use tokio::process::Command;

use crate::naming;

const NETWORK_CONF: &str = "/etc/network/interfaces";

// P1: lock RMW — serializa las escrituras de /etc/network/interfaces
// (create/update vlan, delete vlan, set_vlan_title concurrentes)
static INTERFACES_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub async fn list_interfaces() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let out = Command::new("ip")
        .args(["-o", "link", "show"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut list: Vec<serde_json::Value> = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(4, ": ").collect();
        if parts.len() < 3 {
            continue;
        }
        let name = parts[1].trim();
        let rest = parts[2];

        // P2 (revisa.md): ocultar ifb0/ifb1 y ifb_pppN — son interfaces
        // internas del QoS (no configurables por el operador)
        if name == "ifb0" || name == "ifb1" || name.starts_with("ifb_ppp") {
            continue;
        }

        // RiverOs: ocultar el puerto CPU del switch (eth0 de OpenWrt DSA)
        // y presentar los puertos físicos como eth0..eth4 (wan->eth0,
        // lan2..lan5 -> eth1..eth4).
        if naming::is_cpu_port(&name) {
            continue;
        }
        let name_display = naming::display_name(&name);

        // Parse state from ip output
        let mut state = if rest.contains("state UP") {
            "up".to_string()
        } else if rest.contains("state DOWN") {
            "down".to_string()
        } else {
            "unknown".to_string()
        };

        // Parse MAC address — look for link/ether <mac>
        let mut mac = String::new();
        if let Some(pos) = rest.find("link/ether ") {
            let mac_str = &rest[pos + 11..];
            if let Some(space) = mac_str.find(' ') {
                mac = mac_str[..space].to_string();
            } else {
                mac = mac_str.to_string();
            }
        }

        // Parse MTU
        let mut mtu = String::from("1500");
        if let Some(pos) = rest.find("mtu ") {
            let mtu_str = &rest[pos + 4..];
            if let Some(space) = mtu_str.find(' ') {
                mtu = mtu_str[..space].to_string();
            } else {
                mtu = mtu_str.to_string();
            }
        }

        let is_vlan = name.contains('.') || name.contains('@');
        let is_loopback = name == "lo";

        let iftype = if is_loopback {
            "loopback"
        } else if is_vlan {
            "vlan"
        } else if name.starts_with("ppp") {
            "ppp"
        } else if name.starts_with("wg") {
            "wireguard"
        } else if name.starts_with("br") {
            "bridge"
        } else if name.starts_with("eth") || name.starts_with("en") {
            "ethernet"
        } else {
            "other"
        };

        let is_physical = iftype == "ethernet";

        // Get IPv4 address
        let mut ip = String::new();
        let ip_out = Command::new("ip")
            .args(["-o", "-4", "addr", "show", "dev", name])
            .output()
            .await;
        if let Ok(ip_out) = ip_out {
            let ip_stdout = String::from_utf8_lossy(&ip_out.stdout);
            for ip_line in ip_stdout.lines() {
                let ip_parts: Vec<&str> = ip_line.splitn(5, ' ').collect();
                if ip_parts.len() >= 4 {
                    ip = ip_parts[3].split('/').next().unwrap_or("").to_string();
                }
            }
        }

        // Get RX/TX from /sys/class/net/<name>/statistics/
        let rx = get_stat(name, "rx_bytes").unwrap_or(0);
        let tx = get_stat(name, "tx_bytes").unwrap_or(0);

        // Workaround: driver igc (Intel I226-V) reporta NO-CARRIER falsamente
        // Si kernel dice DOWN pero responde ping, forzar UP
        if state == "down" && iftype == "ethernet" && !mac.is_empty() {
            // Leer IP desde /etc/zpot/mwan.json para WANs configuradas
            let mut if_ip = ip.clone();
            if if_ip.is_empty() {
                if let Ok(mwan_data) = std::fs::read_to_string("/etc/zpot/mwan.json") {
                    if let Ok(mwan_json) = serde_json::from_str::<serde_json::Value>(&mwan_data) {
                        if let Some(wans) = mwan_json["wans"].as_object() {
                            for (_, wan) in wans {
                                if wan["iface"].as_str() == Some(name) {
                                    if let Some(wan_ip) = wan["ip"].as_str() {
                                        if !wan_ip.is_empty() {
                                            if_ip = wan_ip.to_string();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !if_ip.is_empty() {
                let ping_out = Command::new("ping")
                    .args(["-c", "1", "-W", "1", "-I", &if_ip, "8.8.8.8"])
                    .output()
                    .await;
                if let Ok(o) = ping_out {
                    if o.status.success() {
                        eprintln!("[IFACE] {} UP (verified via ping {} OK)", name, if_ip);
                        state = "up".to_string();
                        ip = if_ip;
                    }
                }
            }
        }

        // Speed
        let speed = get_stat_speed(name);

        // Description — get from /etc/network/interfaces comment if any
        let description = get_description(name);

        // Filtro: excluir interfaces PPP (dinamicas) e ifb (intermedias tc)
        if name.starts_with("ppp") || name.starts_with("ifb_") {
            continue;
        }

        list.push(serde_json::json!({
            "name": name_display,
            "real": name.split('@').next().unwrap_or(&name).to_string(),
            "state": state,
            "mac": mac,
            "mtu": mtu,
            "type": iftype,
            "speed_label": speed,
            "rx_bytes": rx,
            "tx_bytes": tx,
            "description": description,
            "vlan": is_vlan,
            "ip": ip,
            "physical": is_physical,
        }));
    }

    Ok(Json(serde_json::json!(list)))
}

fn get_stat(name: &str, stat: &str) -> Option<u64> {
    let path = format!("/sys/class/net/{}/statistics/{}", name, stat);
    let content = std::fs::read_to_string(&path).ok()?;
    content.trim().parse::<u64>().ok()
}

fn get_stat_speed(name: &str) -> String {
    let path = format!("/sys/class/net/{}/speed", name);
    if let Ok(content) = std::fs::read_to_string(&path) {
        let s = content.trim().to_string();
        if s == "4294967295" || s == "-1" {
            return "N/A".to_string();
        }
        if let Ok(speed) = s.parse::<u64>() {
            if speed >= 1000 {
                return format!("{} Gbps", speed / 1000);
            } else {
                return format!("{} Mbps", speed);
            }
        }
        s
    } else {
        "N/A".to_string()
    }
}

fn get_description(name: &str) -> String {
    let path = "/etc/network/interfaces";
    if let Ok(content) = std::fs::read_to_string(path) {
        let mut desc = String::new();
        let mut in_block = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == format!("auto {}", name) || trimmed == format!("iface {} inet", name) || trimmed.starts_with(&format!("iface {} ", name)) {
                in_block = true;
                continue;
            }
            if in_block {
                if trimmed.starts_with("auto ") || trimmed.starts_with("iface ") || trimmed.starts_with("#") {
                    continue;
                }
                if let Some(val) = trimmed.strip_prefix("description ") {
                    desc = val.trim().to_string();
                    break;
                }
                if !trimmed.is_empty() && !trimmed.starts_with('\t') && !trimmed.starts_with(' ') {
                    break;
                }
            }
        }
        desc
    } else {
        String::new()
    }
}

#[derive(serde::Deserialize)]
pub struct VlanCreate {
    name: String,
    vlan_id: i32,
    parent: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    native: bool,
}

pub async fn list_vlans() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let out = Command::new("ip")
        .args(["-o", "link", "show"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut list: Vec<serde_json::Value> = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(4, ": ").collect();
        if parts.len() < 3 {
            continue;
        }
        let name = parts[1].trim();
        let rest = parts[2];
        let state = if rest.contains("state UP") {
            "up"
        } else if rest.contains("state DOWN") {
            "down"
        } else {
            "unknown"
        };

        let clean_name = name.split('@').next().unwrap_or(name);
        if !clean_name.contains('.') {
            continue;
        }

        let parent = clean_name.split('.').next().unwrap_or("");
        let vlan_id = clean_name.split('.').nth(1).unwrap_or("");
        let vlan_id_clean = vlan_id.split('@').next().unwrap_or(vlan_id);
        // P2: vlan_id NUMERICO (antes string "881" -> parseInt en la UI,
        // inconsistente al crear vs listar)
        let vlan_id_num: Option<u64> = vlan_id_clean.parse().ok();

        let mut ip = String::new();
        let mut prefix = String::new();
        // P2: usar clean_name (sin "@parent") — `ip addr show dev eth3.881@eth3`
        // falla y dejaba ip/prefix vacios
        let ip_out = Command::new("ip")
            .args(["-o", "-4", "addr", "show", "dev", clean_name])
            .output()
            .await;
        if let Ok(ip_out) = ip_out {
            let s = String::from_utf8_lossy(&ip_out.stdout);
            for l in s.lines() {
                // P2: split_whitespace — `ip -o` ALINEA con multiples espacios
                // y splitn(5,' ') producia campos vacios (ip siempre "")
                let p: Vec<&str> = l.split_whitespace().collect();
                if p.len() >= 4 && p[2] == "inet" {
                    let cidr = p[3];
                    ip = cidr.split('/').next().unwrap_or("").to_string();
                    if let Some(plen) = cidr.split('/').nth(1) {
                        prefix = format!("/{}", plen);
                    }
                }
            }
        }

        list.push(serde_json::json!({
            "name": clean_name,
            "vlan_id": vlan_id_num.unwrap_or(0),
            "parent": parent,
            "ip": ip,
            "prefix": prefix,
            "native": false,
            "status": state,
        }));
    }

    // Add native VLANs from /etc/network/interfaces
    if let Ok(content) = std::fs::read_to_string(NETWORK_CONF) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("auto ") {
                let ifname = trimmed[5..].trim();
                if ifname.contains('.') {
                    continue;
                }
                // Skip eth0, eth1+, wg*, lo (no son VLANs)
                if ifname == "lo" || ifname == "eth0" || ifname.starts_with("wg") || ifname.starts_with("eth") {
                    continue;
                }

                // Check if already in list
                let exists = list.iter().any(|v| v["name"] == ifname);
                if !exists {
                    let state_out = Command::new("ip")
                        .args(["-o", "link", "show", "dev", ifname])
                        .output()
                        .await;
                    let running = if let Ok(o) = state_out {
                        let s = String::from_utf8_lossy(&o.stdout);
                        if s.contains("state UP") {
                            "up"
                        } else if s.contains("state DOWN") {
                            "down"
                        } else {
                            "unknown"
                        }
                    } else {
                        "unknown"
                    };

                    let mut ip = String::new();
                    let ip_out = Command::new("ip")
                        .args(["-o", "-4", "addr", "show", "dev", ifname])
                        .output()
                        .await;
                    if let Ok(ip_out) = ip_out {
                        let s = String::from_utf8_lossy(&ip_out.stdout);
                        for l in s.lines() {
                            let p: Vec<&str> = l.splitn(5, ' ').collect();
                            if p.len() >= 4 {
                                ip = p[3].split('/').next().unwrap_or("").to_string();
                            }
                        }
                    }

                    list.push(serde_json::json!({
                        "name": ifname,
                        "vlan_id": null,
                        "parent": ifname,
                        "ip": ip,
                        "native": true,
                        "status": running,
                    }));
                }
            }
        }
    }

    // Determinar native/tagged desde bridge vlan show
    let mut native_vids: Vec<String> = Vec::new();
    if let Ok(bv_out) = Command::new("bridge").args(["vlan", "show"]).output().await {
        let bv_stdout = String::from_utf8_lossy(&bv_out.stdout);
        for line in bv_stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("port") || trimmed.starts_with("bridge") { continue; }
            let has_untagged = trimmed.contains("untagged");
            let has_pvid = trimmed.contains("PVID");
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() < 2 { continue; }
            let vid = parts[0].trim();
            if (has_untagged || has_pvid) && !native_vids.contains(&vid.to_string()) {
                native_vids.push(vid.to_string());
            }
        }
    }
    for v in &mut list {
        let vlan_id = v["vlan_id"].as_str().unwrap_or("");
        if vlan_id.is_empty() { continue; }
        v["native"] = if native_vids.contains(&vlan_id.to_string()) {
            serde_json::json!("nativa")
        } else {
            serde_json::json!("tagged")
        };
    }

    Ok(Json(serde_json::json!(list)))
}

pub async fn create_vlan(body: Json<VlanCreate>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let ifname = if body.native {
        body.parent.clone()
    } else {
        format!("{}.{}", body.parent, body.vlan_id)
    };

    if !body.native {
        let out = Command::new("ip")
            .args([
                "link",
                "add",
                "link",
                &body.parent,
                "name",
                &ifname,
                "type",
                "vlan",
                "id",
                &body.vlan_id.to_string(),
            ])
            .output()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.contains("File exists") {
                return Err((StatusCode::BAD_REQUEST, stderr.to_string()));
            }
        }
    }

    // Bring it up
    let _ = Command::new("ip")
        .args(["link", "set", "dev", &ifname, "up"])
        .output()
        .await;

    // Persist to /etc/network/interfaces (best-effort; FIX 2026-08-12:
    // en OpenWrt/RiverOs el archivo NO existe — crearlo en vez de 500)
    let _g = INTERFACES_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut content = std::fs::read_to_string(NETWORK_CONF)
        .unwrap_or_default();

    if !content.ends_with('\n') {
        content.push('\n');
    }

    let iface_block = format!(
        "auto {}\niface {} inet manual\n",
        ifname, ifname
    );
    // Buscar linea exacta "auto X" (no substring, para evitar "eth3" match "eth3.10")
    let search = format!("auto {}\n", ifname);
    if !content.contains(&search) {
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&iface_block);
    }

    // P1: escritura atomica (tmp+rename) — antes fs::write directo.
    // Best-effort (FIX 2026-08-12): en OpenWrt/RiverOs /etc/network/ no existe;
    // la VLAN ya esta creada en el kernel, la persistencia es solo documental.
    if let Some(parent) = std::path::Path::new(NETWORK_CONF).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = format!("{}.tmp-{}", NETWORK_CONF, std::process::id());
    if std::fs::write(&tmp, content.as_bytes()).is_ok() {
        let _ = std::fs::rename(&tmp, NETWORK_CONF);
    }

    Ok(Json(serde_json::json!({"success": true, "name": ifname})))
}

#[derive(serde::Deserialize)]
pub struct VlanDelete {
    name: String,
}

/// Request body para configurar tagged/untagged/PVID de una VLAN en un bridge
#[derive(serde::Deserialize)]
pub struct VlanConfigure {
    /// Nombre de la VLAN (ej: BridgeLan.30) o de la interfaz nativa
    name: String,
    /// "tagged" o "untagged"
    mode: String,
    /// true para marcar como PVID (solo si mode=untagged)
    #[serde(default)]
    pvid: bool,
    /// Nombre del puerto donde aplicar (ej: eth3). Si es 'self' aplica al bridge
    port: String,
}

pub async fn configure_vlan(
    body: Json<VlanConfigure>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let vlan_name = &body.name;
    let port = &body.port;
    let mode = &body.mode;

    // Extraer el VLAN ID del nombre (BridgeLan.30 -> 30, o buscar en bridge vlan show)
    let vid = if let Some(dot_pos) = vlan_name.rfind('.') {
        vlan_name[dot_pos + 1..].to_string()
    } else {
        // Es nativa (BridgeLan) — obtener su VLAN ID de bridge vlan show
        let out = Command::new("bridge")
            .args(["vlan", "show", "dev", port])
            .output()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        // Buscar la línea que tiene Egress Untagged sin PVID para el nombre
        // Si es nativa, buscamos la entrada del bridge self
        let mut found_vid = String::new();
        for line in stdout.lines() {
            let trimmed = line.trim();
            // Buscar líneas como "30 PVID Egress Untagged" o "30"
            if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit() || c == ' ' || c == '\t') {
                // Saltamos encabezados
                continue;
            }
            // Intentar parsear el primer token como número
            let first_token = trimmed.split_whitespace().next().unwrap_or("");
            if let Ok(_) = first_token.parse::<i32>() {
                found_vid = first_token.to_string();
            }
        }
        // Si no encontramos, intentar obtener VID del bridge self
        if found_vid.is_empty() {
            // Obtener el VLAN ID que la interfaz nativa tiene asignado
            // Buscar en /etc/network/interfaces o asumir que si es nativa y tiene parent
            // iguales, podría ser cualquier VLAN configurada en el bridge
            return Err((StatusCode::BAD_REQUEST, "No se pudo determinar VLAN ID para interfaz nativa. Especifique nombre como BridgeLan.30".to_string()));
        }
        found_vid
    };

    // Construir comando bridge vlan add (siempre usar add, que sobrescribe flags)
    let mut args: Vec<&str> = vec!["vlan", "add", "dev", port, "vid", &vid];

    if mode == "untagged" {
        args.push("egress");
        args.push("untagged");
        if body.pvid {
            args.push("pvid");
        }
    }
    // Si mode == "tagged", no agregamos flags extra (egress tagged por defecto)

    let out = Command::new("bridge")
        .args(&args)
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        // Si error es "doesn't exist", primero agregar la VLAN
        if stderr.contains("doesn't exist") || stderr.contains("does not exist") || stderr.contains("Vlan range start doesn't exist") {
            // Agregar VLAN primero
            let mut add_args: Vec<&str> = vec!["vlan", "add", "vid", &vid, "dev", port];
            if mode == "untagged" {
                add_args.push("untagged");
                if body.pvid {
                    add_args.push("pvid");
                }
            }
            let add_out = Command::new("bridge")
                .args(&add_args)
                .output()
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            if !add_out.status.success() {
                let add_stderr = String::from_utf8_lossy(&add_out.stderr);
                return Err((StatusCode::BAD_REQUEST, format!("Error al agregar VLAN {} al puerto {}: {}", vid, port, add_stderr)));
            }
        } else {
            return Err((StatusCode::BAD_REQUEST, stderr.to_string()));
        }
    }

    Ok(Json(serde_json::json!({"success": true, "vid": vid, "port": port, "mode": mode, "pvid": body.pvid})))
}

pub async fn delete_vlan(
    body: Json<VlanDelete>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = &body.name;
    // Bring down
    let _ = Command::new("ip")
        .args(["link", "set", "dev", name, "down"])
        .output()
        .await;

    // Delete VLAN interface (skip for native VLANs — can't delete physical interface)
    if name.contains('.') {
        let out = Command::new("ip")
            .args(["link", "delete", name])
            .output()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.contains("does not exist") {
                return Err((StatusCode::BAD_REQUEST, stderr.to_string()));
            }
        }
    }

    // Remove from /etc/network/interfaces
    let _g = INTERFACES_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(content) = std::fs::read_to_string(NETWORK_CONF) {
        let mut new_content = String::new();
        let mut in_vlan_block = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == format!("auto {}", name) {
                in_vlan_block = true;
                continue;
            }
            if in_vlan_block && trimmed == format!("iface {}", name) {
                in_vlan_block = false;
                continue;
            }
            if !in_vlan_block {
                new_content.push_str(line);
                new_content.push('\n');
            }
        }
        // P1: escritura atomica (tmp+rename)
        let tmp = format!("{}.tmp-{}", NETWORK_CONF, std::process::id());
        std::fs::write(&tmp, new_content.as_bytes()).ok();
        std::fs::rename(&tmp, NETWORK_CONF).ok();
    }

    Ok(Json(serde_json::json!({"success": true})))
}
/// Devuelve la tabla bridge vlan show parseada como JSON: matriz puerto x VLAN
pub async fn list_bridge_vlans() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let out = match Command::new("bridge").args(["vlan", "show"]).output().await {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // RiverOs: sin ip-bridge (binario `bridge`) — lista vacia
            return Ok(Json(serde_json::json!([])));
        }
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    };

    if !out.status.success() {
        // Si bridge no soporta vlan show (sin ip-bridge), lista vacia
        if String::from_utf8_lossy(&out.stderr).contains("not found")
            || String::from_utf8_lossy(&out.stderr).contains("No such file") {
            return Ok(Json(serde_json::json!([])));
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("bridge vlan show fallo: {}", stderr)));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut ports: Vec<serde_json::Value> = Vec::new();
    let mut current_port: Option<String> = None;
    let mut current_vlans: Vec<serde_json::Value> = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("port") { continue; }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() { continue; }

        // Linea de puerto: empieza con letra (nombre de interfaz)
        if parts[0].parse::<u16>().is_err() {
            // Guardar puerto anterior si existe
            if let Some(ref name) = current_port {
                ports.push(serde_json::json!({
                    "port": name,
                    "vlans": current_vlans
                }));
            }
            current_port = Some(parts[0].to_string());
            current_vlans = Vec::new();
            // Saltar la linea de puerto, la siguiente linea(s) traen los VIDs
            continue;
        }

        // Linea de VID
        let has_pvid = trimmed.contains("PVID");
        let has_untagged = trimmed.contains("Untagged");
        current_vlans.push(serde_json::json!({
            "vid": parts[0],
            "pvid": has_pvid,
            "untagged": has_untagged,
            "tagged": !has_untagged
        }));
    }

    // Guardar ultimo puerto
    if let Some(ref name) = current_port {
        ports.push(serde_json::json!({
            "port": name,
            "vlans": current_vlans
        }));
    }

    Ok(Json(serde_json::json!(ports)))
}

#[derive(serde::Deserialize)]
pub struct VlanTitle {
    name: String,
    #[serde(default)]
    title: Option<String>,
}

pub async fn set_vlan_title(
    body: Json<VlanTitle>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = &body.name;
    let title = body.title.as_deref().unwrap_or("");
    // P0: sanitizar el title — NUNCA permitir saltos de linea/caracteres de
    // control (antes inyectaban lineas `up <comando>` en el boot)
    let title: String = title.chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect();
    let title = title.trim();
    
    // Guardar/actualizar descripcion en /etc/network/interfaces
    let _g = INTERFACES_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let content = std::fs::read_to_string(NETWORK_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error leyendo {}: {}", NETWORK_CONF, e)))?;
    
    let mut new_content = String::new();
    let mut found = false;
    let mut in_block = false;
    let mut block_ended = false;
    
    // Buscar el bloque iface/auto name y agregar/actualizar description
    for line in content.lines() {
        let trimmed = line.trim();
        let search_auto = format!("auto {}", name);
        let search_iface = format!("iface {}", name);
        
        if trimmed == search_auto || trimmed.starts_with(&search_iface) {
            in_block = true;
            block_ended = false;
            new_content.push_str(line);
            new_content.push('\n');
            continue;
        }
        
        if in_block && !block_ended {
            if trimmed.starts_with("auto ") || trimmed.starts_with("iface ") {
                // Nuevo bloque — terminar el anterior
                // Si no encontramos description, agregarlo ahora
                if !found && !title.is_empty() {
                    new_content.push_str(&format!("\tdescription {}\n", title));
                }
                in_block = true;
                block_ended = false;
                found = false;
                new_content.push_str(line);
                new_content.push('\n');
                continue;
            }
            
            if trimmed.starts_with("description ") {
                if title.is_empty() {
                    // Quitar linea de descripcion
                    found = true;
                    continue;
                }
                new_content.push_str(&format!("\tdescription {}\n", title));
                found = true;
                continue;
            }
            
            // Si llegamos a una linea no-vacia que no es parte del bloque, terminamos
            if !trimmed.is_empty() && !trimmed.starts_with('\t') && !trimmed.starts_with(' ') {
                if !found && !title.is_empty() {
                    new_content.push_str(&format!("\tdescription {}\n", title));
                }
                block_ended = true;
            }
        }
        
        new_content.push_str(line);
        new_content.push('\n');
    }
    
    // Si el bloque no termino con otra auto/iface, agregar description al final
    if in_block && !block_ended && !found && !title.is_empty() {
        new_content.push_str(&format!("\tdescription {}\n", title));
    }
    
    // P1: escritura atomica (tmp+rename)
    let tmp = format!("{}.tmp-{}", NETWORK_CONF, std::process::id());
    std::fs::write(&tmp, new_content.as_bytes())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    std::fs::rename(&tmp, NETWORK_CONF)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(Json(serde_json::json!({"success": true, "name": name, "title": title})))
}


/// Configura VLANs tagged/untagged/PVID en un puerto de bridge
pub async fn configure_bridge_port(
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let port = body.get("port").and_then(|v| v.as_str()).unwrap_or("");
    if port.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "port requerido".into()));
    }

    let vlans = body.get("vlans").and_then(|v| v.as_array());
    if vlans.is_none() {
        return Err((StatusCode::BAD_REQUEST, "vlans array requerido".into()));
    }

    for vlan in vlans.unwrap() {
        let vid = vlan.get("vid").and_then(|v| v.as_u64()).unwrap_or(0);
        if vid == 0 { continue; }

        let tagged = vlan.get("tagged").and_then(|v| v.as_bool()).unwrap_or(false);
        let pvid = vlan.get("pvid").and_then(|v| v.as_bool()).unwrap_or(false);

        // P2: el PVID SIEMPRE es untagged (identifica la VLAN nativa del
        // trunk). Antes pvid+tagged juntos = `bridge vlan add` fallaba con
        // "Invalid argument" (configuracion rota).
        let tagged = if pvid { false } else { tagged };

        // P2: verificar que el puerto es miembro de un bridge — si no,
        // `bridge vlan add` falla con error oscuro. Dar error claro.
        let master = Command::new("ip").args(["-o", "link", "show", "dev", port]).output().await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let master_txt = String::from_utf8_lossy(&master.stdout);
        if !master_txt.contains(" master ") {
            return Err((StatusCode::BAD_REQUEST,
                format!("el puerto {} no pertenece a ningun bridge — las VLANs de puerto requieren bridge", port)));
        }

        let vid_str = vid.to_string();
        let mut args = vec!["vlan", "add", "dev", port, "vid", &vid_str];
        if pvid { args.push("pvid"); }
        args.push(if tagged { "tagged" } else { "untagged" });

        let out = Command::new("bridge")
            .args(&args)
            .output()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("bridge command error: {}", e)))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("bridge vlan add fallo: {}", stderr)));
        }
    }

    Ok(Json(serde_json::json!({"success": true, "port": port, "vlans_configured": vlans.unwrap().len()})))
}
