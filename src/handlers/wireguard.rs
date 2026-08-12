use axum::{Json, http::StatusCode};
use tokio::process::Command;
use serde::{Serialize, Deserialize};
use axum::extract::Path;

#[derive(Serialize)]
pub struct WgInterface {
    pub name: String,
    pub public_key: String,
    pub private_key: String,
    pub listen_port: u16,
    pub address: String,
    pub dns: String,
    pub mtu: u16,
    pub peers_count: usize,
    pub status: String,
}

pub async fn list() -> Result<Json<Vec<WgInterface>>, (StatusCode, String)> {
    let output = Command::new("sh")
        .args(["-c", "wg show interfaces 2>/dev/null || true"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let names = String::from_utf8_lossy(&output.stdout);
    let names: Vec<&str> = names.split_whitespace().filter(|s| !s.is_empty()).collect();
    let mut interfaces = Vec::new();

    for name in names {
        let info = Command::new("sh")
            .args(["-c", &format!("wg show {} 2>/dev/null || true", name)])
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        let mut public_key = String::new();
        let mut listen_port = 0u16;
        let mut peers = 0usize;

        for line in info.lines() {
            let t = line.trim();
            if t.starts_with("public key:") {
                public_key = t.trim_start_matches("public key:").trim().to_string();
            } else if t.starts_with("listening port:") {
                listen_port = t.trim_start_matches("listening port:").trim().parse().unwrap_or(0);
            } else if t.starts_with("peer:") {
                peers += 1;
            }
        }

        // Private key via wg show
        let private_key = Command::new("sh")
            .args(["-c", &format!("wg show {} private-key 2>/dev/null || grep -m1 '^PrivateKey' /etc/wireguard/{}.conf 2>/dev/null | awk '{{print $3}}' || echo ''", name, name)])
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        // IP via ip addr
        let addr_out = Command::new("sh")
            .args(["-c", &format!("ip -o -4 addr show {} 2>/dev/null | awk '{{print $4}}' | cut -d/ -f1 | head -1", name)])
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        let status = Command::new("sh")
            .args(["-c", &format!("ip link show {} 2>/dev/null | grep -qE 'state UP|state UNKNOWN' && echo 'up' || echo 'down'", name)])
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "unknown".into());

        interfaces.push(WgInterface {
            name: name.to_string(),
            public_key,
            // NUNCA exponer la private key al frontend (seguridad)
            private_key: String::new(),
            listen_port,
            address: addr_out,
            dns: String::new(),
            mtu: 1420,
            peers_count: peers,
            status,
        });
    }

    Ok(Json(interfaces))
}

// ─── CREAR / ELIMINAR INTERFAZ ────────────────────────────────
// Persistencia: /etc/wireguard/<name>.conf (wg-quick) + restore via
// /etc/init.d/wg-quick.<name> (symlink OpenRC) + rc-update.
// Peers persistidos en /etc/zpot/wg-peers-<name>.json (todos los campos,
// incl. preshared/keepalive que wg dump no expone) -> se regenera el .conf.

#[derive(Deserialize)]
pub struct InterfaceCreate {
    pub name: String,
    pub address: String,
    pub listen_port: u16,
    pub private_key: String,
    pub dns: String,
    pub mtu: u16,
}

#[derive(Deserialize)]
pub struct InterfaceDelete {
    pub name: String,
}

fn peers_json_path(name: &str) -> String {
    format!("/etc/zpot/wg-peers-{}.json", name)
}

fn load_peers_json(name: &str) -> Vec<serde_json::Value> {
    std::fs::read_to_string(peers_json_path(name))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_peers_json(name: &str, peers: &[serde_json::Value]) {
    if let Some(parent) = std::path::Path::new(&peers_json_path(name)).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(peers) {
        let _ = std::fs::write(peers_json_path(name), &json);
    }
}

/// Regenera /etc/wireguard/<name>.conf con Interface + todos los peers
/// (de wg-peers-<name>.json). Lo usa wg-quick al boot.
fn write_conf(name: &str, address: &str, dns: &str, listen_port: u16, private_key: &str, mtu: u16) {
    let mut conf = String::from("[Interface]\n");
    if !address.is_empty() {
        conf.push_str(&format!("Address = {}\n", address));
    }
    if !dns.is_empty() {
        conf.push_str(&format!("DNS = {}\n", dns));
    }
    if listen_port > 0 {
        conf.push_str(&format!("ListenPort = {}\n", listen_port));
    }
    if !private_key.is_empty() {
        conf.push_str(&format!("PrivateKey = {}\n", private_key));
    }
    if mtu > 0 {
        conf.push_str(&format!("MTU = {}\n", mtu));
    }
    for p in load_peers_json(name) {
        conf.push('\n');
        conf.push_str("[Peer]\n");
        if let Some(v) = p.get("public_key").and_then(|v| v.as_str()) {
            conf.push_str(&format!("PublicKey = {}\n", v));
        }
        if let Some(v) = p.get("preshared_key").and_then(|v| v.as_str()) {
            if !v.is_empty() {
                conf.push_str(&format!("PresharedKey = {}\n", v));
            }
        }
        if let Some(v) = p.get("endpoint").and_then(|v| v.as_str()) {
            if !v.is_empty() {
                conf.push_str(&format!("Endpoint = {}\n", v));
            }
        }
        if let Some(v) = p.get("allowed_ips").and_then(|v| v.as_str()) {
            if !v.is_empty() {
                conf.push_str(&format!("AllowedIPs = {}\n", v));
            }
        }
        if let Some(v) = p.get("persistent_keepalive").and_then(|v| v.as_u64()) {
            if v > 0 {
                conf.push_str(&format!("PersistentKeepalive = {}\n", v));
            }
        }
    }
    let _ = std::fs::write(format!("/etc/wireguard/{}.conf", name), &conf);
}

fn ensure_boot_restore(name: &str) {
    // init.d symlink (OpenRC): wg-quick up <name> al boot
    let _ = Command::new("sh")
        .args(["-c", &format!("ln -sf /etc/init.d/wg-quick /etc/init.d/wg-quick.{} 2>/dev/null; rc-update add wg-quick.{} default 2>/dev/null || true", name, name)])
        .output();
}

fn remove_boot_restore(name: &str) {
    let _ = Command::new("sh")
        .args(["-c", &format!("rc-update del wg-quick.{} default 2>/dev/null || true; rm -f /etc/init.d/wg-quick.{}", name, name)])
        .output();
}

/// POST /api/wireguard/interfaces — crea la interfaz (wg1/wg2...).
/// Aplica en vivo (ip link + wg set + ip addr) y persiste el .conf +
/// restore al boot. Address puede traer "ip/mask, ip6/64" (separado por coma).
pub async fn create(Json(body): Json<InterfaceCreate>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = body.name.trim().to_string();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err((StatusCode::BAD_REQUEST, "nombre invalido (solo alfanumerico, _ y -)".into()));
    }
    let address = body.address.trim().to_string();
    if address.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "address required".into()));
    }
    // Validar Address: exigir /32 (IPv4) o /128 (IPv6) — un /24 en la
    // interfaz crea una ruta que captura la VPN de gestion (incidente wg1)
    for addr in address.split(',') {
        let a = addr.trim();
        if a.is_empty() {
            continue;
        }
        if !(a.ends_with("/32") || a.ends_with("/128")) {
            return Err((StatusCode::BAD_REQUEST,
                format!("Address '{}' debe ser /32 (IPv4) o /128 (IPv6) — ej. 10.7.0.15/32. NUNCA /24 (rompe gestion)", a)));
        }
    }
    // OJO: no permitir IP duplicada con otra interfaz (ip addr add fallaria)
    let exists = Command::new("sh").args(["-c", &format!("ip link show {} 2>/dev/null | grep -q .", name)]).output().await
        .map(|o| o.status.success()).unwrap_or(false);
    if exists {
        return Err((StatusCode::BAD_REQUEST, format!("interfaz {} ya existe", name)));
    }

    // Private key: usar la dada o generar
    let private_key = if body.private_key.trim().is_empty() {
        let out = Command::new("sh").args(["-c", "wg genkey"]).output().await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    } else {
        body.private_key.trim().to_string()
    };

    // ip link add + wg set (private-key via stdin — <(echo) no existe en busybox ash)
    let mut cmds = vec![
        format!("ip link add dev {} type wireguard", name),
        format!("printf '%s' '{}' | wg set {} private-key /dev/stdin", private_key, name),
    ];
    if body.listen_port > 0 {
        cmds.push(format!("wg set {} listen-port {}", name, body.listen_port));
    }
    // Address puede ser "ip/24, ipv6/64"
    for addr in address.split(',') {
        let a = addr.trim();
        if !a.is_empty() {
            cmds.push(format!("ip addr add {} dev {}", a, name));
        }
    }
    if body.mtu > 0 {
        cmds.push(format!("ip link set mtu {} dev {}", body.mtu, name));
    }
    cmds.push(format!("ip link set {} up", name));

    for c in &cmds {
        let out = Command::new("sh").args(["-c", c]).output().await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).to_string();
            let _ = Command::new("sh").args(["-c", &format!("ip link del dev {} 2>/dev/null || true", name)]).output().await;
            return Err((StatusCode::BAD_REQUEST, format!("{} -> {}", c, err)));
        }
    }

    // Persistencia
    save_peers_json(&name, &[]);
    write_conf(&name, &address, &body.dns, body.listen_port, &private_key, body.mtu);
    ensure_boot_restore(&name);

    let pub_out = Command::new("sh").args(["-c", &format!("echo '{}' | wg pubkey", private_key)]).output().await
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    Ok(Json(serde_json::json!({"ok": true, "name": name, "public_key": pub_out})))
}

/// DELETE /api/wireguard/interfaces — elimina la interfaz (ip link del +
/// conf + peers json + init.d/rc-update). No toca los peers del otro lado.
pub async fn delete(Json(body): Json<InterfaceDelete>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let name = body.name.trim().to_string();
    // Proteger la interfaz de gestion (wg0 = VPN de management, NUNCA borrar)
    if name == "wg0" {
        return Err((StatusCode::BAD_REQUEST, "no se puede eliminar wg0 (interfaz de gestion)".into()));
    }
    let out = Command::new("sh").args(["-c", &format!("ip link del dev {} 2>&1 || true", name)]).output().await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let _ = std::fs::remove_file(format!("/etc/wireguard/{}.conf", name));
    let _ = std::fs::remove_file(peers_json_path(&name));
    remove_boot_restore(&name);
    Ok(Json(serde_json::json!({"ok": true, "name": name, "detail": String::from_utf8_lossy(&out.stderr).trim()})))
}

pub async fn peers(Path(name): Path<String>) -> Result<Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let name = name.trim().to_string();
    // Validar nombre antes de usarlo en sh -c (evitar RCE via Path)
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err((StatusCode::BAD_REQUEST, "nombre invalido".into()));
    }
    let output = Command::new("sh")
        .args(["-c", &format!("wg show {} peers 2>/dev/null || true", name)])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let peer_keys = String::from_utf8_lossy(&output.stdout);
    let peer_keys: Vec<&str> = peer_keys.split_whitespace().filter(|s| !s.is_empty()).collect();
    let mut peers_vec = Vec::new();

    for pk in peer_keys {
        let dump = Command::new("sh")
            .args(["-c", &format!("wg show {} dump 2>/dev/null || true", name)])
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        for line in dump.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 8 && parts[0] == pk {
                let mut peer = serde_json::Map::new();
                peer.insert("public_key".to_string(), serde_json::Value::String(pk.to_string()));
                peer.insert("endpoint".to_string(), serde_json::Value::String(if parts[2].is_empty() { String::new() } else { parts[2].to_string() }));
                peer.insert("allowed_ips".to_string(), serde_json::Value::String(parts[3].to_string()));
                peer.insert("handshake".to_string(), serde_json::Value::String(parts[4].to_string()));
                peer.insert("rx".to_string(), serde_json::Value::String(format!("{} bytes", parts[5])));
                peer.insert("tx".to_string(), serde_json::Value::String(format!("{} bytes", parts[6])));
                // preshared/keepalive desde el JSON persistido (dump no los da)
                if let Some(saved) = load_peers_json(&name).iter().find(|p| p.get("public_key").and_then(|v| v.as_str()) == Some(pk)) {
                    if let Some(v) = saved.get("preshared_key") { peer.insert("preshared_key".to_string(), v.clone()); }
                    if let Some(v) = saved.get("persistent_keepalive") { peer.insert("persistent_keepalive".to_string(), v.clone()); }
                }
                peers_vec.push(serde_json::Value::Object(peer));
                break;
            }
        }
    }

    Ok(Json(peers_vec))
}

#[derive(Deserialize)]
pub struct PeerAdd {
    pub interface: String,
    pub public_key: String,
    pub allowed_ips: String,
    pub endpoint: Option<String>,
    pub preshared_key: Option<String>,
    pub persistent_keepalive: Option<u16>,
}

#[derive(Deserialize)]
pub struct PeerDelete {
    pub interface: String,
    pub public_key: String,
}

pub async fn peers_add(Json(body): Json<PeerAdd>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Proteger wg0: NO tocar peers de la interfaz de gestion (regenerar
    // wg0.conf desde el JSON podria perder el peer del VPS → caida)
    if body.interface.trim() == "wg0" {
        return Err((StatusCode::BAD_REQUEST, "no se pueden modificar peers de wg0 (interfaz de gestion)".into()));
    }
    // Validar AllowedIPs: prohibir full-tunnel (rompe gestion/MWAN, incidente wg1)
    let allowed = body.allowed_ips.trim();
    if allowed.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "allowed_ips required".into()));
    }
    for part in allowed.split(',') {
        let p = part.trim();
        if p == "0.0.0.0/0" || p == "::/0" || p == "10.7.0.0/24" || p == "10.7.0.0/16" {
            return Err((StatusCode::BAD_REQUEST,
                format!("AllowedIPs '{}' NO permitido (rompe rutas/gestion en boot)", p)));
        }
    }

    let mut cmd = format!(
        "wg set {} peer {} allowed-ips {}",
        body.interface, body.public_key, allowed
    );
    if let Some(ep) = &body.endpoint {
        if !ep.is_empty() {
            cmd.push_str(&format!(" endpoint {}", ep));
        }
    }
    let mut stdin_data = String::new();
    if let Some(psk) = &body.preshared_key {
        if !psk.is_empty() {
            cmd.push_str(" preshared-key /dev/stdin");
            stdin_data.push_str(psk);
        }
    }
    if let Some(k) = body.persistent_keepalive {
        if k > 0 {
            cmd.push_str(&format!(" persistent-keepalive {}", k));
        }
    }
    let mut sh = Command::new("sh");
    sh.arg("-c").arg(&cmd);
    if !stdin_data.is_empty() {
        sh.stdin(std::process::Stdio::piped());
    }
    let mut child = sh
        .spawn()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !stdin_data.is_empty() {
        use tokio::io::AsyncWriteExt;
        if let Some(mut si) = child.stdin.take() {
            let _ = si.write_all(stdin_data.as_bytes()).await;
        }
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        return Err((StatusCode::BAD_REQUEST, err));
    }

    // Persistir en el JSON + regenerar .conf (para wg-quick al boot)
    let mut peers = load_peers_json(&body.interface);
    peers.retain(|p| p.get("public_key").and_then(|v| v.as_str()) != Some(body.public_key.as_str()));
    peers.push(serde_json::json!({
        "public_key": body.public_key,
        "allowed_ips": body.allowed_ips,
        "endpoint": body.endpoint.unwrap_or_default(),
        "preshared_key": body.preshared_key.unwrap_or_default(),
        "persistent_keepalive": body.persistent_keepalive.unwrap_or(0),
    }));
    save_peers_json(&body.interface, &peers);
    // Reconstruir el conf: Interface actual + peers del JSON
    let conf_path = format!("/etc/wireguard/{}.conf", body.interface);
    if let Ok(conf) = std::fs::read_to_string(&conf_path) {
        let iface_block: Vec<&str> = conf.lines().take_while(|l| !l.trim().starts_with("[Peer]")).collect();
        let mut new_conf = iface_block.join("\n");
        // private_key/dns/address/mtu/listen del bloque Interface actual
        let addr = iface_block.iter().find(|l| l.starts_with("Address")).map(|l| l.splitn(2, '=').nth(1).unwrap_or("").trim()).unwrap_or("");
        let dns = iface_block.iter().find(|l| l.starts_with("DNS")).map(|l| l.splitn(2, '=').nth(1).unwrap_or("").trim()).unwrap_or("");
        let lp = iface_block.iter().find(|l| l.starts_with("ListenPort")).map(|l| l.splitn(2, '=').nth(1).unwrap_or("").trim().parse().unwrap_or(0)).unwrap_or(0);
        let pk = iface_block.iter().find(|l| l.starts_with("PrivateKey")).map(|l| l.splitn(2, '=').nth(1).unwrap_or("").trim()).unwrap_or("");
        let mtu = iface_block.iter().find(|l| l.starts_with("MTU")).map(|l| l.splitn(2, '=').nth(1).unwrap_or("").trim().parse().unwrap_or(0)).unwrap_or(0);
        write_conf(&body.interface, addr, dns, lp, pk, mtu);
        let _ = new_conf;
    }

    Ok(Json(serde_json::json!({"ok": true, "interface": body.interface, "public_key": body.public_key})))
}

pub async fn peers_delete(Json(body): Json<PeerDelete>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Proteger wg0: NO tocar peers de la interfaz de gestion
    if body.interface.trim() == "wg0" {
        return Err((StatusCode::BAD_REQUEST, "no se pueden modificar peers de wg0 (interfaz de gestion)".into()));
    }
    let output = Command::new("sh")
        .args(["-c", &format!("wg set {} peer {} remove", body.interface, body.public_key)])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        return Err((StatusCode::BAD_REQUEST, err));
    }

    // Quitar del JSON + regenerar .conf
    let mut peers = load_peers_json(&body.interface);
    peers.retain(|p| p.get("public_key").and_then(|v| v.as_str()) != Some(body.public_key.as_str()));
    save_peers_json(&body.interface, &peers);
    let conf_path = format!("/etc/wireguard/{}.conf", body.interface);
    if let Ok(conf) = std::fs::read_to_string(&conf_path) {
        let iface_block: Vec<&str> = conf.lines().take_while(|l| !l.trim().starts_with("[Peer]")).collect();
        let addr = iface_block.iter().find(|l| l.starts_with("Address")).map(|l| l.splitn(2, '=').nth(1).unwrap_or("").trim()).unwrap_or("");
        let dns = iface_block.iter().find(|l| l.starts_with("DNS")).map(|l| l.splitn(2, '=').nth(1).unwrap_or("").trim()).unwrap_or("");
        let lp = iface_block.iter().find(|l| l.starts_with("ListenPort")).map(|l| l.splitn(2, '=').nth(1).unwrap_or("").trim().parse().unwrap_or(0)).unwrap_or(0);
        let pk = iface_block.iter().find(|l| l.starts_with("PrivateKey")).map(|l| l.splitn(2, '=').nth(1).unwrap_or("").trim()).unwrap_or("");
        let mtu = iface_block.iter().find(|l| l.starts_with("MTU")).map(|l| l.splitn(2, '=').nth(1).unwrap_or("").trim().parse().unwrap_or(0)).unwrap_or(0);
        write_conf(&body.interface, addr, dns, lp, pk, mtu);
    }

    Ok(Json(serde_json::json!({"ok": true})))
}
