use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Mutex};
use axum::{Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

const CONFIG_PATH: &str = "/etc/zpot/mwan.json";

static MWAN_STORE: OnceLock<Arc<MwanStore>> = OnceLock::new();

pub fn init_store() -> Arc<MwanStore> {
    let store = Arc::new(MwanStore::default());
    let state = read_state();
    *store.state.lock().unwrap_or_else(|e| e.into_inner()) = state;
    let _ = MWAN_STORE.set(store.clone());
    store
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WanConfig {
    pub iface: String,
    pub ip: String,
    pub gateway: String,
    pub status: String,
    pub table: u32,
    pub mark: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MwanState {
    pub wans: HashMap<String, WanConfig>,
    pub mode: String,
    pub distribution: String,
}

impl Default for MwanState {
    fn default() -> Self {
        Self {
            wans: HashMap::new(),
            mode: "failover".into(),
            distribution: "50/50".into(),
        }
    }
}

#[derive(Default)]
pub struct MwanStore {
    pub state: Mutex<MwanState>,
}

pub fn store() -> &'static Arc<MwanStore> {
    MWAN_STORE.get().expect("MwanStore no inicializado — llama init_store() en main")
}

pub fn read_state() -> MwanState {
    std::fs::read_to_string(CONFIG_PATH)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| MwanState {
            wans: HashMap::new(),
            mode: "failover".into(),
            distribution: "50/50".into(),
        })
}

fn write_state(state: &MwanState) {
    if let Some(parent) = std::path::Path::new(CONFIG_PATH).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        // P1: escritura atomica (tmp+rename)
        let tmp = format!("{}.tmp-{}", CONFIG_PATH, std::process::id());
        let _ = std::fs::write(&tmp, &json);
        let _ = std::fs::rename(&tmp, CONFIG_PATH);
    }
}

async fn detect_iface_wan(iface: &str) -> Option<(String, String)> {
    let out = Command::new("ip")
        .args(["-4", "addr", "show", "dev", iface])
        .output()
        .await
        .ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ip_line = stdout.lines().find(|l| l.trim().starts_with("inet "))?;
    let ip = ip_line.trim().split_whitespace().nth(1)?.split('/').next()?.to_string();

    let rt = Command::new("ip")
        .args(["route", "show", "table", "all", "dev", iface])
        .output()
        .await
        .ok()?;
    let rt_out = String::from_utf8_lossy(&rt.stdout);
    let gateway = rt_out
        .lines()
        .find(|l| l.starts_with("default via"))
        .and_then(|l| l.split_whitespace().nth(2))
        .map(|s| s.to_string())
        .unwrap_or_default();

    Some((ip, gateway))
}

/// Detecta estado de una interfaz WAN. Recibe la IP del store en memoria
/// (evita std::fs::read_to_string sincrono que bloqueaba el worker tokio).
async fn detect_status(iface: &str, wan_ip: &str) -> String {
    let out = Command::new("ip")
        .args(["link", "show", "dev", iface])
        .output()
        .await;
    match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            if s.contains("state UP") || s.contains("state UNKNOWN") {
                "up".into()
            } else {
                // Driver igc a veces reporta NO-CARRIER falsamente
                // Verificar con ping a 8.8.8.8 usando IP del store (en memoria, sin fs sync)
                if !wan_ip.is_empty() {
                    let ping = Command::new("ping")
                        .args(["-c", "1", "-W", "1", "-I", wan_ip, "8.8.8.8"])
                        .output()
                        .await;
                    if let Ok(p) = ping {
                        if p.status.success() {
                            return "up".into();
                        }
                    }
                }
                "down".into()
            }
        }
        Err(_) => "down".into(),
    }
}

pub async fn get_mwan_status() -> Json<serde_json::Value> {
    let state = store().state.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut result = serde_json::json!({
        "mode": state.mode,
        "distribution": state.distribution,
        "active_wan": "wan1",
        "nft_rules": [],
        "ip_rules": [],
        "tables": {},
        "handshake": format!("{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()),
    });

    let mut tables = serde_json::Map::new();
    let mut ip_rules = Vec::new();
    let mut active = "none";

    // O1: leer conntrack UNA SOLA VEZ para todas las WANs
    let (client_counts, total_unique, client_types) = count_all_client_ips().await;

    for (name, wan) in &state.wans {
        let key = name.clone();
        let iface = wan.iface.clone();

        let status = detect_status(&iface, &wan.ip).await;
        let (ip, _gateway) = detect_iface_wan(&iface).await.unwrap_or((wan.ip.clone(), String::new()));
        let gw = wan.gateway.clone();

        // FIX (2026-08-12): crear la ruta a la tabla SIEMPRE que haya gateway
        // (antes solo si status==up). `ip route replace` rechaza nexthop con
        // link down -> levantar el link administrativo primero (sin carrier
        // fisico la ruta queda "linkdown" pero lista para cuando suba).
        if !gw.is_empty() {
            let _ = Command::new("ip").args(["link", "set", &iface, "up"]).output().await;
            let has_route = Command::new("ip")
                .args(["route", "show", "table", &wan.table.to_string(), "default"])
                .output()
                .await
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.contains("default"))
                .unwrap_or(false);
            if !has_route {
                let _ = Command::new("ip")
                    .args(["route", "replace", "default", "via", &gw, "dev", &iface, "table", &wan.table.to_string()])
                    .output()
                    .await;
            }
        }

        // IPs de clientes balanceadas a este WAN (conntrack leido UNA vez arriba)
        let client_ips = client_counts.get(&wan.mark).copied().unwrap_or(0);
        // Desglose por tipo de sesion (ppp=192.168.20.x, hotspot=192.168.10.x)
        let (client_ppp, client_hotspot) = client_types.get(&wan.mark).copied().unwrap_or((0, 0));

        let wan_json = serde_json::json!({
            "iface": iface,
            "ip": ip,
            "gateway": gw,
            "status": status,
            "table": wan.table,
            "mark": wan.mark,
            "client_ips": client_ips,
            "client_ppp": client_ppp,
            "client_hotspot": client_hotspot,
        });
        result.as_object_mut().unwrap().insert(key, wan_json);

        ip_rules.push(format!("{}: from all fwmark 0x{:x} lookup {}", 1400 + wan.mark, wan.mark, name));
        tables.insert(
            wan.table.to_string(),
            if gw.is_empty() { serde_json::Value::String("none".into()) } else { serde_json::Value::String(format!("default via {} dev {}", gw, iface)) },
        );

        if status == "up" && active == "none" {
            active = name;
        }
    }

    if let Some(obj) = result.as_object_mut() {
        obj.insert("ip_rules".into(), serde_json::Value::Array(ip_rules.into_iter().map(serde_json::Value::String).collect()));
        obj.insert("tables".into(), serde_json::Value::Object(tables));
        obj.insert("active_wan".into(), serde_json::Value::String(active.into()));
        obj.insert("total_unique".into(), serde_json::json!(total_unique));
    }

    Json(result)
}

/// Cuenta IPs de clientes (src únicas que no sean IPs WAN del router)
/// saliendo por el WAN con el mark indicado, vía conntrack.
/// Lee conntrack UNA SOLA VEZ y cuenta IPs unicas por mark.
/// Optimizacion O1: evita 2 lecturas de conntrack por request.
async fn count_all_client_ips() -> (std::collections::HashMap<u32, usize>, usize, std::collections::HashMap<u32, (usize, usize)>) {
    let (_ok, text) = crate::handlers::helpers::conntrack_lines().await;
    if text.is_empty() { return (Default::default(), 0, Default::default()) };
    let mut sets: std::collections::HashMap<u32, std::collections::HashSet<String>> = Default::default();
    // Desglose por tipo de sesion por WAN: (ppp, hotspot) — clasificados por
    // subred (eth3.881 PPPoE = 192.168.20.x, hotspot eth3 = 192.168.10.x).
    let mut types: std::collections::HashMap<u32, (std::collections::HashSet<String>, std::collections::HashSet<String>)> = Default::default();
    let mut all_ips: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in text.lines() {
        let Some(mark_pos) = line.find("mark=") else { continue };
        let mark_rest = &line[mark_pos + 5..];
        let mark_str: String = mark_rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(mark) = mark_str.parse::<u32>() else { continue };
        let Some(pos) = line.find("src=") else { continue };
        let rest = &line[pos + 4..];
        let ip: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        if ip.is_empty() || ip == "0.0.0.0"
            || ip.starts_with("192.168.2.") || ip.starts_with("192.168.3.")
            || ip.starts_with("127.") || ip.starts_with("169.254.") {
            continue;
        }
        sets.entry(mark).or_default().insert(ip.clone());
        all_ips.insert(ip.clone());
        // Desglose ppp/hotspot por subred
        let te = types.entry(mark).or_default();
        if ip.starts_with("192.168.20.") {
            te.0.insert(ip);
        } else if ip.starts_with("192.168.10.") {
            te.1.insert(ip);
        }
    }
    let per_mark: std::collections::HashMap<u32, usize> = sets.into_iter().map(|(k, v)| (k, v.len())).collect();
    let per_type: std::collections::HashMap<u32, (usize, usize)> = types.into_iter()
        .map(|(k, (p, h))| (k, (p.len(), h.len())))
        .collect();
    (per_mark, all_ips.len(), per_type)
}

#[derive(Debug, Deserialize)]
pub struct WanBody {
    pub iface: Option<String>,
    pub ip: Option<String>,
    pub gateway: Option<String>,
    pub table: Option<u32>,
    pub mark: Option<u32>,
    pub weight: Option<u32>,
}

/// Sincroniza hotspot/postrouting con las WANs configuradas
/// Se llama desde apply_nft_rules y desde main.rs al iniciar
pub fn sync_hotspot_wans(state: &MwanState) {
    // Limpiar reglas existentes de hotspot/postrouting (excepto las primeras)
    exec_cmd("nft", &["flush", "chain", "inet", "hotspot", "postrouting"]);
    // Agregar masquerade para cada WAN configurada
    for (_, wan) in &state.wans {
        if !wan.iface.is_empty() {
            nft_script(&format!(
                "add rule inet hotspot postrouting oif \"{}\" masquerade", wan.iface
            ));
        }
    }
}

/// FIX-5 (BUG-1e/1f): toda la secuencia nft + ip rules en spawn_blocking.
pub async fn apply_nft_rules(state: &MwanState) {
    let state_c = state.clone();
    tokio::task::spawn_blocking(move || {
    exec_cmd("nft", &["delete", "table", "inet", "mwan"]);
    exec_cmd("nft", &["add", "table", "inet", "mwan"]);

    // Sincronizar hotspot/postrouting con todas las WANs configuradas
    sync_hotspot_wans(&state_c);

    let wans: Vec<(&String, &WanConfig)> = state_c.wans.iter().filter(|(_, w)| w.status == "up").collect();
    if wans.is_empty() { return; }

    nft_script("add chain inet mwan prerouting { type filter hook prerouting priority mangle; policy accept; }");
    nft_script("add chain inet mwan postrouting { type nat hook postrouting priority srcnat; policy accept; }");
    nft_script("add rule inet mwan prerouting iif wg0 ip daddr 10.7.0.0/24 accept");

    // Sticky: distribuye IPs entre WANs activas usando el WEIGHT del config
    // (P1: antes jhash 50/50 fijo, el distribution se ignoraba).
    // nft numgen random mod 100 con map por rangos: 0-69 -> wan1, 70-99 -> wan2
    // Si solo una WAN activa, todo a esa
    if wans.len() >= 2 {
        // Parsear distribution "70/30" -> umbral 70 (redondeado a la WAN1)
        let weight = state_c.distribution
            .split('/')
            .next()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(50)
            .min(100);
        let mut map = String::new();
        let mut lo = 0u32;
        for (i, wan) in wans.iter().enumerate() {
            let span = if i == 0 { weight } else { 100 - lo };
            let hi = (lo + span).min(100);
            if i > 0 { map.push_str(", "); }
            map.push_str(&format!("{}-{} : 0x{:x}", lo, hi.saturating_sub(1), wan.1.mark));
            lo = hi;
        }
        // Completar hasta 99 si el reparto no llego (redondeo)
        if lo < 100 {
            if !map.is_empty() { map.push_str(", "); }
            map.push_str(&format!("{}-99 : 0x{:x}", lo, wans.last().map(|w| w.1.mark).unwrap_or(1)));
        }
        nft_script(&format!(
            "add rule inet mwan prerouting ct state new meta mark set numgen random mod 100 map {{ {} }}", map
        ));
        nft_script("add rule inet mwan prerouting ct state new ct mark set meta mark");
    } else if wans.len() == 1 {
        let primary = &wans[0];
        nft_script(&format!("add rule inet mwan prerouting ct state new meta mark set {}", primary.1.mark));
    }

    // FIX (2026-07-31, VERIFICADO EN VIVO): return por iif de las WANs, NO por daddr.
    // El return viejo usaba `ip daddr { redes locales }` asumiendo que las respuestas de
    // internet llegan al hook mangle con dst = IP del cliente. FALSO: el mangle prerouting
    // (-150) corre ANTES del dstnat (-100); la respuesta entra por la WAN con dst = IP
    // publica de la WAN (ej. 192.168.3.105). El return viejo nunca matcheaba → la regla
    // 'established,related meta mark set ct mark' marcaba la respuesta → ip rule → tabla
    // wanN → re-enviada a la WAN (ROUTING LOOP: el cliente retransmite SYN, nunca recibe
    // respuesta → "site can't be reached" / "No internet connection").
    // FIX REAL: no restaurar fwmark en established que ENTRAN por las WANs (eth0/eth1) —
    // son respuestas para clientes internos (hotspot/PPPoE) que deben rutear por tabla
    // main → eth3/ppp0. Los ACKs de clientes (iif eth3/ppp*) SI se marcan → sticky WAN
    // se mantiene (cada conexion sale por la misma WAN del SYN).
    let wan_ifaces: Vec<String> = wans.iter().map(|(_, w)| format!("\"{}\"", w.iface)).collect();
    nft_script(&format!(
        "add rule inet mwan prerouting ct state established,related iif {{ {} }} return",
        wan_ifaces.join(", ")
    ));

    nft_script("add rule inet mwan prerouting ct state established,related meta mark set ct mark");

    for (_, wan) in &state_c.wans {
        if wan.status != "up" || wan.iface.is_empty() { continue; }
        nft_script(&format!(
            "add rule inet mwan postrouting oif \"{}\" meta mark {} masquerade", wan.iface, wan.mark
        ));
    }

    flush_ip_rules();

    for (_, wan) in &state_c.wans {
        if wan.status != "up" || wan.iface.is_empty() { continue; }
        let mark_hex = format!("0x{:x}", wan.mark);
        exec_cmd("ip", &["-4", "rule", "add", "fwmark", &mark_hex,
            "table", &wan.table.to_string(), "prio", &format!("{}", 1400 + wan.mark)]);
    }
    }).await.ok();
}

fn nft_script(script: &str) {
    let mut child = match std::process::Command::new("nft")
        .args(["-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return,
    };
    use std::io::Write;
    let _ = child.stdin.as_mut().unwrap().write_all(script.as_bytes());
    let _ = child.wait();
}

fn exec_cmd(cmd: &str, args: &[&str]) {
    let _ = std::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
}

fn flush_ip_rules() {
    for i in 1400..=1510_i32 {
        let _ = std::process::Command::new("ip")
            .args(["-4", "rule", "del", "prio", &i.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
    }
}

/// Aplica cambio de IP/gateway de una WAN en vivo y persistente:
/// 1. `ip addr add <ip>/<prefix> dev <iface>` (si no existe) — aplicar en vivo
/// 2. Borra cualquier otra IPv4 de la interfaz (idempotente, nunca sin IP)
/// 3. `ip route replace default via <gw> dev <iface> table <N>` — gateway en vivo
/// 4. Actualizar /etc/network/interfaces (bloque `iface <iface> inet static`)
async fn apply_wan_ip_change(iface: &str, new_ip: &str, new_gw: &str, table: u32) -> Result<(), (StatusCode, String)> {
    // Listar TODAS las IPv4 actuales de la interfaz
    let out = Command::new("ip")
        .args(["-o", "-4", "addr", "show", "dev", iface])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("ip addr show: {}", e)))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let current_ips: Vec<String> = stdout
        .lines()
        .filter_map(|l| {
            l.split_whitespace().find(|p| p.contains('/') && !p.starts_with('(')).map(|p| p.to_string())
        })
        .collect();

    if current_ips.is_empty() {
        // Interfaz sin IPv4: agregar la nueva IP directo (caso recovery/limpieza)
        let prefix = "24";
        let new_cidr = format!("{}/{}", new_ip, prefix);
        let add_out = Command::new("ip")
            .args(["addr", "add", &new_cidr, "dev", iface])
            .output()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("ip addr add: {}", e)))?;
        if !add_out.status.success() {
            let err = String::from_utf8_lossy(&add_out.stderr);
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("ip addr add fallo: {}", err.trim())));
        }
        // Levantar el link administrativo para que la ruta con nexthop sea aceptada
        let _ = Command::new("ip").args(["link", "set", iface, "up"]).output().await;
        if !new_gw.is_empty() && table > 0 {
            let _ = Command::new("ip")
                .args(["route", "replace", "default", "via", new_gw, "dev", iface, "table", &table.to_string()])
                .output()
                .await;
        }
        update_interfaces_conf(iface, new_ip, prefix, new_gw)?;
        return Ok(());
    }
    let prefix = current_ips[0].split('/').nth(1).unwrap_or("24").to_string();
    let new_cidr = format!("{}/{}", new_ip, prefix);

    // Idempotente: si ya solo existe la IP deseada, no tocar las IPs
    if !(current_ips.len() == 1 && current_ips[0] == new_cidr) {
        // PITFALL kernel: con 2 IPs del mismo prefijo, la 2da es "secondary" ligada a la
        // primary. `ip addr del` de la primary ELIMINA TAMBIÉN la secondary.
        // Solución: flush IPv4 + add (nunca deja IPs huerfanas del mismo prefijo).
        let _ = Command::new("ip")
            .args(["-4", "addr", "flush", "dev", iface])
            .output()
            .await;
        let add_out = Command::new("ip")
            .args(["addr", "add", &new_cidr, "dev", iface])
            .output()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("ip addr add: {}", e)))?;
        if !add_out.status.success() {
            let err = String::from_utf8_lossy(&add_out.stderr);
            // P1: rollback — si el add falla, RESTAURAR las IPs que el flush
            // borro (antes la iface quedaba SIN IPv4 = perdida de esa WAN)
            for old in &current_ips {
                let _ = Command::new("ip")
                    .args(["addr", "add", old, "dev", iface])
                    .output().await;
            }
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("ip addr add fallo (rollback aplicado): {}", err.trim())));
        }
    }

    if !new_gw.is_empty() && table > 0 {
        // Ruta en la tabla del mark (no en main, para no mover el tráfico WG/SSH)
        let _ = Command::new("ip")
            .args(["route", "replace", "default", "via", new_gw, "dev", iface, "table", &table.to_string()])
            .output()
            .await;
    }

    // Persistir en /etc/network/interfaces
    update_interfaces_conf(iface, new_ip, &prefix, new_gw)?;

    Ok(())
}

/// Persistencia en OpenWrt via UCI: crea/actualiza una seccion de interfaz
/// (proto static) en /etc/config/network. El cambio en vivo ya se aplico con
/// `ip addr`; aqui solo se deja la config para que sobreviva al reboot.
fn update_uci_iface(iface: &str, new_ip: &str, prefix: &str, new_gw: &str) -> Result<(), (StatusCode, String)> {
    // Nombre de seccion UCI seguro: wan_<iface> (p.ej. wan_eth1)
    let sec = format!("wan_{}", iface);
    let netmask = ipv4_prefix_to_mask(prefix)?;

    // IMPORTANTE (uci OpenWrt): primero crear la seccion con `=interface`
    // (set de opciones en seccion inexistente -> "Invalid argument")
    let mk = std::process::Command::new("uci")
        .args(["set", &format!("network.{}={}", sec, "interface")])
        .output()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("uci add: {}", e)))?;
    if !mk.status.success() {
        return Err((StatusCode::INTERNAL_SERVER_ERROR,
            format!("uci add fallo: {}", String::from_utf8_lossy(&mk.stderr).trim())));
    }

    let sets: Vec<String> = vec![
        format!("network.{}.proto=static", sec),
        format!("network.{}.device={}", sec, iface),
        format!("network.{}.ipaddr={}", sec, new_ip),
        format!("network.{}.netmask={}", sec, netmask),
    ];
    for s in &sets {
        let out = std::process::Command::new("uci").args(["set", s]).output()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("uci set: {}", e)))?;
        if !out.status.success() {
            return Err((StatusCode::INTERNAL_SERVER_ERROR,
                format!("uci set fallo: {}", String::from_utf8_lossy(&out.stderr).trim())));
        }
    }
    if !new_gw.is_empty() {
        let out = std::process::Command::new("uci")
            .args(["set", &format!("network.{}.gateway={}", sec, new_gw)])
            .output()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("uci set gateway: {}", e)))?;
        if !out.status.success() {
            return Err((StatusCode::INTERNAL_SERVER_ERROR,
                format!("uci set gateway fallo: {}", String::from_utf8_lossy(&out.stderr).trim())));
        }
    }
    let commit = std::process::Command::new("uci").arg("commit").arg("network").output()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("uci commit: {}", e)))?;
    if !commit.status.success() {
        return Err((StatusCode::INTERNAL_SERVER_ERROR,
            format!("uci commit fallo: {}", String::from_utf8_lossy(&commit.stderr).trim())));
    }
    Ok(())
}

/// Convierte prefijo /N a mascara de red (24 -> 255.255.255.0).
fn ipv4_prefix_to_mask(prefix: &str) -> Result<String, (StatusCode, String)> {
    let bits: u32 = prefix.parse().map_err(|_| (StatusCode::BAD_REQUEST, "prefijo invalido".into()))?;
    if bits > 32 {
        return Err((StatusCode::BAD_REQUEST, "prefijo >32".into()));
    }
    let mask = if bits == 0 { 0u32 } else { u32::MAX << (32 - bits) };
    Ok(format!("{}.{}.{}.{}", (mask >> 24) & 0xff, (mask >> 16) & 0xff, (mask >> 8) & 0xff, mask & 0xff))
}

/// Reemplaza `address` y `gateway` dentro del bloque `iface <iface> inet static`
/// en /etc/network/interfaces.
fn update_interfaces_conf(iface: &str, new_ip: &str, prefix: &str, new_gw: &str) -> Result<(), (StatusCode, String)> {
    if std::path::Path::new("/etc/config/network").exists() {
        return update_uci_iface(iface, new_ip, prefix, new_gw);
    }
    const NETWORK_CONF: &str = "/etc/network/interfaces";
    let content = match std::fs::read_to_string(NETWORK_CONF) {
        Ok(c) => c,
        Err(_) => return Ok(()), // sin archivo: cambio aplicado en runtime
    };
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    let mut in_block = false;
    let mut found_iface = false;
    let mut changed = false;
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim_start().to_string();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        // Entrar al bloque de esta interfaz (exacto, no eth10/eth1.5)
        if !in_block && parts.len() >= 2 && parts[0] == "iface" && parts[1] == iface && parts.get(2) == Some(&"inet") {
            in_block = true;
            found_iface = true;
            i += 1;
            continue;
        }

        if in_block {
            // Salir del bloque si empieza otro iface o auto
            if (parts.first() == Some(&"iface") && parts.get(1) != Some(&iface))
                || parts.first() == Some(&"auto")
            {
                in_block = false;
                continue;
            }
            if parts.first() == Some(&"address") {
                lines[i] = format!("    address {}/{}", new_ip, prefix);
                changed = true;
            } else if parts.first() == Some(&"gateway") && !new_gw.is_empty() {
                lines[i] = format!("    gateway {}", new_gw);
                changed = true;
            }
        }
        i += 1;
    }

    if !found_iface {
        return Err((StatusCode::BAD_REQUEST, format!("No se encontro bloque 'iface {} inet' en {}", iface, NETWORK_CONF)));
    }
    if changed {
        let new_content = lines.join("\n") + "\n";
        std::fs::write(NETWORK_CONF, new_content.as_bytes())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Error escribiendo {}: {}", NETWORK_CONF, e)))?;
    }
    Ok(())
}

pub async fn post_mwan_config(
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let payload: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("JSON invalido: {}", e)))?;

    let mode = payload.get("mode").and_then(|v| v.as_str()).map(|s| s.to_string());
    let distribution = payload.get("distribution").and_then(|v| v.as_str()).map(|s| s.to_string());
    let wans_val = payload.get("wans");

    // --- Fase 1: parsear body + aplicar cambios de IP en vivo (FUERA del lock) ---
    let mut wans_map: HashMap<String, WanBody> = HashMap::new();
    let mut clear_existing = false;

    if let Some(wans_val) = wans_val {
        match wans_val {
            serde_json::Value::Object(obj) => {
                for (name, val) in obj {
                    if let Ok(body) = serde_json::from_value::<WanBody>(val.clone()) {
                        wans_map.insert(name.clone(), body);
                    }
                }
            }
            serde_json::Value::Array(arr) => {
                clear_existing = true;
                for (i, val) in arr.iter().enumerate() {
                    if let Ok(body) = serde_json::from_value::<WanBody>(val.clone()) {
                        let name = format!("wan{}", i + 1);
                        wans_map.insert(name, body);
                    }
                }
            }
            _ => {}
        }
    }

    // Snapshot de wans actuales (para fallback de IP) sin mantener el lock
    let prev_wans: HashMap<String, WanConfig> = {
        let st = store().state.lock().unwrap_or_else(|e| e.into_inner());
        st.wans.clone()
    };

    // P1: validar entradas ANTES de tocar el sistema (antes iface con IP
    // basura = DoS; inyección de lineas en /etc/network/interfaces)
    // FIX (2026-08-12): el frontend SIEMPRE envia la fila por defecto con
    // iface vacio — NO es un error, se salta esa wan (equivalente a no tenerla).
    let wans_map: HashMap<String, WanBody> = wans_map
        .into_iter()
        .filter(|(_, w)| !w.iface.as_deref().unwrap_or("").trim().is_empty())
        .collect();
    for (name, wan_body) in &wans_map {
        if let Some(iface) = &wan_body.iface {
            if iface.is_empty() || !iface.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.') {
                return Err((StatusCode::BAD_REQUEST, format!("iface invalida en {}: {}", name, iface)));
            }
        }
        for (label, val) in [("ip", &wan_body.ip), ("gateway", &wan_body.gateway)] {
            if let Some(v) = val {
                if !v.is_empty() && v.parse::<std::net::Ipv4Addr>().is_err() {
                    return Err((StatusCode::BAD_REQUEST, format!("{} invalida en {}: {}", label, name, v)));
                }
            }
        }
    }

    // Aplicar cambios de IP/gateway en vivo + persistir interfaces (await, sin lock)
    for (name, wan_body) in &wans_map {
        let iface = wan_body.iface.clone().unwrap_or_default();
        let gateway = wan_body.gateway.clone().unwrap_or_default();
        let table = wan_body.table.unwrap_or(0);
        let ip = wan_body.ip.clone().unwrap_or_else(|| {
            prev_wans.get(name).map(|w| w.ip.clone()).unwrap_or_default()
        });

        if !iface.is_empty() && !ip.is_empty() {
            let (real_ip, real_gw) = detect_iface_wan(&iface).await.unwrap_or_default();
            if real_ip != ip || (!gateway.is_empty() && real_gw != gateway) {
                apply_wan_ip_change(&iface, &ip, &gateway, table).await?;
            }
        }
    }

    // --- Fase 2: actualizar estado en memoria (lock corto) ---
    // FIX-5: clonar y SOLTAR el lock antes del .await (MutexGuard std no
    // es Send -> cruzar await rompe el trait Handler de axum, BUG-11).
    let new_state = {
        // P1: lock RMW — serializa el read-modify del estado MWAN (dos POSTs
        // concurrentes perdían wans si ambos partian del mismo snapshot)
        static MWAN_STATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = MWAN_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = store().state.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(mode_str) = mode {
            state.mode = mode_str;
        }
        if let Some(dist_str) = distribution {
            state.distribution = dist_str;
        }

        if clear_existing {
            state.wans.clear();
        }
        for (name, wan_body) in wans_map {
            let iface = wan_body.iface.clone().unwrap_or_default();
            let gateway = wan_body.gateway.clone().unwrap_or_default();
            let table = wan_body.table.unwrap_or(0);
            let mark = wan_body.mark.unwrap_or(0);
            let weight = wan_body.weight.unwrap_or(1);
            let _ = weight;

            let ip = wan_body.ip.clone().unwrap_or_else(|| {
                state.wans.get(&name).map(|w| w.ip.clone()).unwrap_or_default()
            });

            state.wans.insert(name.clone(), WanConfig {
                iface: iface.clone(),
                ip: ip.clone(),
                gateway: gateway.clone(),
                status: "up".into(),
                table,
                mark,
            });
        }
        state.clone()
    };

    apply_nft_rules(&new_state).await;
    write_state(&new_state);
    Ok(Json(serde_json::json!({"status": "ok", "message": "configuracion guardada"})))
}
