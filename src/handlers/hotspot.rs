use axum::{Json, http::StatusCode, extract::Path, extract::ConnectInfo, response::{Response, IntoResponse}, body::Body};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Mutex;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use std::process::Command;
use std::io::Write;
use std::fs::{self, OpenOptions};

const HS_CONFIG_PATH: &str = "/etc/zpot/hotspot-server.json";
const HS_COOKIES_PATH: &str = "/etc/zpot/hotspot-cookies.json";
const HS_SESSIONS_PATH: &str = "/etc/zpot/hotspot-sessions.json";

/// Macro de logging: escribe a stderr y a /tmp/zpot.log
/// #[macro_export] (2026-08-08): disponible en todo el crate (main.rs usa
/// zlog! para los eventos de cookie que antes iban a stdout y se perdian
/// con nohup > /dev/null).
#[macro_export]
macro_rules! zlog {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        eprintln!("{}", msg);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true).append(true).open("/tmp/zpot.log")
        {
            use std::io::Write;
            let _ = writeln!(f, "{}", msg);
        }
    }};
}
// (zlog! macro definida arriba — disponible en todo hotspot.rs)

static HS_CONFIG: std::sync::Mutex<Option<HotspotServer>> = std::sync::Mutex::new(None);
static SESSION_STORE: std::sync::Mutex<Option<HashMap<String, HotspotSession>>> = std::sync::Mutex::new(None);
pub fn session_store() -> &'static Mutex<Option<HashMap<String, HotspotSession>>> {
    &SESSION_STORE
}

// ── Persistencia de sesiones a disco (FIX 2026-08-02) ─────────────
// Las sesiones se guardan en /etc/zpot/hotspot-sessions.json en cada
// mutacion y se reconstruyen al boot (junto con las cookies) para que
// el accounting/interim/reauth sobreviva reinicios de zpot.

/// Serializa el session_store a disco (best-effort)
/// FIX (2026-08-04): escritura ATOMICA (tmp + rename) — un corte a mitad
/// de fs::write dejaba el JSON corrupto y el boot no reconstruia sesiones.
fn save_sessions_to_disk() {
    let store = SESSION_STORE.lock().unwrap_or_else(|e| e.into_inner());
    let map = store.as_ref();
    let data = match map {
        Some(m) => serde_json::to_string_pretty(m).unwrap_or_default(),
        None => String::new(),
    };
    drop(store);
    if let Some(parent) = std::path::Path::new(HS_SESSIONS_PATH).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = format!("{}.{}.tmp", HS_SESSIONS_PATH, std::process::id());
    if fs::write(&tmp, &data).is_ok() {
        let _ = fs::rename(&tmp, HS_SESSIONS_PATH);
    }
}

/// Reconstruye sesiones desde disco y re-agrega su bypass nft.
/// NOTA (FIX 2026-08-02): init_hotspot_nft() borra y recrea la tabla, asi que
/// el set hotspot_auth queda VACIO al boot — NO se puede validar contra el set.
/// En su lugar: se restauran TODAS las sesiones del JSON (las que estaban
/// activas en el ultimo save) y se re-agrega el bypass nft para cada una.
/// Las clases/filtros tc del kernel persisten entre restarts (no se tocan).
/// Las sesiones fantasma (cliente que ya se fue sin logout) las expulsa el
/// interim task (idle/reauth) en el primer ciclo.
/// Devuelve los datos necesarios para respawnear el interim task de cada una.
pub fn restore_sessions_from_disk() -> Vec<(String, String, String, u32)> {
    let mut restored = Vec::new();
    let data = match fs::read_to_string(HS_SESSIONS_PATH) {
        Ok(d) => d,
        Err(_) => return restored,
    };
    let sessions: HashMap<String, HotspotSession> = match serde_json::from_str(&data) {
        Ok(s) => s,
        Err(_) => return restored,
    };
    let mut store = SESSION_STORE.lock().unwrap_or_else(|e| e.into_inner());
    store.get_or_insert_with(HashMap::new);
    let cfg = get_hs_config();
    for (ip, session) in &sessions {
        store.as_mut().unwrap().insert(ip.clone(), session.clone());
        // Re-agregar bypass nft (el set se recreo vacio al boot)
        add_bypass_nft(ip, &session.client_mac);
        // FIX-H7 (2026-08-04): re-aplicar QoS tc al boot — el kernel BORRA
        // los qdisc/clases en un REBOOT del sistema (antes el comentario
        // decia que persistian, pero eso solo vale para restarts de zpot).
        // Sin esto, los clientes hotspot restaurados navegan SIN limite.
        apply_qos(ip, &cfg.iface, &session.speed_up, &session.speed_down,
                  &session.up_ceil_str, &session.down_ceil_str);
        restored.push((
            session.username.clone(),
            ip.clone(),
            session.session_id.clone(),
            session.idle_timeout,
        ));
        zlog!("[SESSION-RESTORE] {} ip={} mac={} sid={} idle={}",
            session.username, ip, session.client_mac, session.session_id, session.idle_timeout);
    }
    drop(store);
    if !restored.is_empty() {
        zlog!("[SESSION-RESTORE] {} sesiones reconstruidas desde {}", restored.len(), HS_SESSIONS_PATH);
    }
    restored
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotSession {
    pub username: String,
    pub client_ip: String,
    pub client_mac: String,
    pub session_id: String,
    pub start: u64,
    pub speed_up: String,   // ej: "512K"
    pub speed_down: String, // ej: "1M"
    pub rx_bytes: u64,      // para Interim-Update
    pub tx_bytes: u64,      // para Interim-Update
    pub idle_timeout: u32,    // segundos sin trafico antes de desconectar, 0=desactivado
    pub last_active: u64,     // timestamp Unix de ultimo trafico detectado
    #[serde(default)]
    pub nft_expire: u64,      // timestamp del ultimo add del elemento nft (BUG-E)
    #[serde(default)]
    pub up_ceil_str: String,  // ceil UP del VSA ("2M/5M" -> "2M") para re-aplicar QoS al boot
    #[serde(default)]
    pub down_ceil_str: String, // ceil DOWN del VSA -> "5M"
}

fn hs_config() -> &'static Mutex<Option<HotspotServer>> {
    &HS_CONFIG
}
fn get_hs_config() -> HotspotServer {
    let mut cfg = HS_CONFIG.lock().unwrap_or_else(|e| e.into_inner());
    if cfg.is_none() {
        // Intentar cargar desde disco
        if let Ok(data) = fs::read_to_string(HS_CONFIG_PATH) {
            if let Ok(parsed) = serde_json::from_str::<HotspotServer>(&data) {
                *cfg = Some(parsed);
                zlog!("[CONFIG] Cargada desde {}", HS_CONFIG_PATH);
            }
        }
        // Si aun no hay config, usar hardcoded defaults
        if cfg.is_none() {
            *cfg = Some(HotspotServer {
                iface: "eth4".into(),
                gw: "192.168.10.1".into(),
                html_dir: format!("{}/static/hotspot", crate::PROJ_DIR).into(),
                idle_timeout: 600,
                shared_users: 1,
                rate_limit: String::new(),
                radius: "161.97.67.63:1812".into(),
                radius_secret: "85River@B".into(),
                coa_enabled: false,
                coa_mode: "udp".into(),
                coa_poll_url: String::new(),
            });
        }
    }
    cfg.as_ref().unwrap().clone()
}

fn save_hs_config(cfg: &HotspotServer) {
    if let Some(parent) = std::path::Path::new(HS_CONFIG_PATH).parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(cfg) {
        Ok(json) => match fs::write(HS_CONFIG_PATH, &json) {
            Ok(_) => zlog!("[CONFIG] Guardada en {}", HS_CONFIG_PATH),
            Err(e) => zlog!("[CONFIG] Error guardando {}: {}", HS_CONFIG_PATH, e),
        },
        Err(e) => zlog!("[CONFIG] Error serializando: {}", e),
    }
}

/// Cookie entry almacenada server-side para auto-login por MAC cookie
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CookieEntry {
    pub username: String,
    pub password: String,
    pub mac: String,
    pub created_at: u64,
    pub expires_at: u64,
}

/// Lista server-side de cookies activas (persiste hasta 7 dias)
/// Permite auto-login cuando el usuario vuelve al portal con la cookie del browser.
/// Eliminar una entrada aqui = denegar auto-login (como en MikroTik).
/// Limpieza de expiradas en cada iteracion del interim update.
/// PERSISTENCIA (2026-08-02): se guardan en HS_COOKIES_PATH para que el
/// auto-login sobreviva reinicios de zpot (antes: solo memoria → tras reinicio
/// el browser tenia la cookie pero el servidor la olvidaba).
static HOTSPOT_COOKIES: std::sync::Mutex<Vec<CookieEntry>> = std::sync::Mutex::new(Vec::new());

/// Carga cookies desde disco al arranque (llamada desde main.rs)
pub fn load_cookies_from_disk() {
    if let Ok(data) = fs::read_to_string(HS_COOKIES_PATH) {
        if let Ok(parsed) = serde_json::from_str::<Vec<CookieEntry>>(&data) {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            let mut cookies = HOTSPOT_COOKIES.lock().unwrap_or_else(|e| e.into_inner());
            cookies.retain(|_| false); // vaciar (evitar duplicados si se llama 2x)
            cookies.extend(parsed.into_iter().filter(|c| c.expires_at > now));
            drop(cookies);
            zlog!("[COOKIE] Cargadas {} cookies validas desde {}", HOTSPOT_COOKIES.lock().unwrap_or_else(|e| e.into_inner()).len(), HS_COOKIES_PATH);
        }
    }
}

/// Persiste cookies a disco (best-effort) — escritura ATOMICA (tmp+rename)
fn save_cookies_to_disk() {
    let cookies = HOTSPOT_COOKIES.lock().unwrap_or_else(|e| e.into_inner());
    let data = serde_json::to_string_pretty(&*cookies).unwrap_or_default();
    drop(cookies);
    if let Some(parent) = std::path::Path::new(HS_COOKIES_PATH).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = format!("{}.{}.tmp", HS_COOKIES_PATH, std::process::id());
    if fs::write(&tmp, &data).is_ok() {
        let _ = fs::rename(&tmp, HS_COOKIES_PATH);
    }
}

/// Verifica si existe una cookie valida server-side para username+mac
pub fn cookie_entry_exists(username: &str, mac: &str) -> bool {
    let cookies = HOTSPOT_COOKIES.lock().unwrap_or_else(|e| e.into_inner());
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    cookies.iter().any(|c|
        c.username == username &&
        c.mac.to_lowercase() == mac.to_lowercase() &&
        c.expires_at > now
    )
}

/// Guarda una cookie server-side
pub fn save_cookie_entry(username: &str, password: &str, mac: &str, ttl_secs: u64) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let mut cookies = HOTSPOT_COOKIES.lock().unwrap_or_else(|e| e.into_inner());
    // Reemplazar entry existente (mismo user+mac)
    cookies.retain(|c| !(c.username == username && c.mac.to_lowercase() == mac.to_lowercase()));
    cookies.push(CookieEntry {
        username: username.to_string(),
        password: password.to_string(),
        mac: mac.to_string(),
        created_at: now,
        expires_at: now + ttl_secs,
    });
    drop(cookies);
    save_cookies_to_disk();
    zlog!("[COOKIE] Guardada: {} mac={} expira en +{}s", username, mac, ttl_secs);
}

/// Elimina una cookie server-side por username+mac
pub fn delete_cookie_entry(username: &str, mac: &str) {
    let mut cookies = HOTSPOT_COOKIES.lock().unwrap_or_else(|e| e.into_inner());
    let before = cookies.len();
    cookies.retain(|c| !(c.username == username && c.mac.to_lowercase() == mac.to_lowercase()));
    let after = cookies.len();
    drop(cookies);
    if before != after {
        save_cookies_to_disk();
        zlog!("[COOKIE] Eliminada: {} mac={}", username, mac);
    }
}

/// Limpia cookies expiradas del store
fn cleanup_expired_cookies() {
    let mut cookies = HOTSPOT_COOKIES.lock().unwrap_or_else(|e| e.into_inner());
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let before = cookies.len();
    cookies.retain(|c| c.expires_at > now);
    let after = cookies.len();
    drop(cookies);
    if before != after {
        save_cookies_to_disk();
        zlog!("[COOKIE] Limpieza: {} expiradas eliminadas", before - after);
    }
}

/// Retorna copia de todas las cookies (solo no expiradas)
fn get_cookie_entries() -> Vec<CookieEntry> {
    cleanup_expired_cookies();
    let cookies = HOTSPOT_COOKIES.lock().unwrap_or_else(|e| e.into_inner());
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    cookies.iter().filter(|c| c.expires_at > now).cloned().collect()
}

/// Busca password de un username+MAC en las cookies server-side
/// FIX (2026-08-04): filtra por MAC tambien — con 2 cookies del mismo
/// usuario (2 dispositivos, MACs distintas) ANTES devolvia el password
/// de la PRIMERA entrada (el dispositivo A se re-autenticaba con el
/// password nuevo de B y fallaba).
fn find_password_for_username(username: &str, mac: &str) -> Option<String> {
    cleanup_expired_cookies();
    let cookies = HOTSPOT_COOKIES.lock().unwrap_or_else(|e| e.into_inner());
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    cookies.iter()
        .find(|c| c.username == username && c.mac.to_lowercase() == mac.to_lowercase() && c.expires_at > now)
        .map(|c| c.password.clone())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotServer {
    pub iface: String,
    pub gw: String,
    pub html_dir: String,
    pub idle_timeout: u32,
    pub shared_users: u32,
    pub rate_limit: String,
    pub radius: String,
    pub radius_secret: String,
    // CoA / desconexion remota (FIX 2026-08-04, caso G4RP)
    #[serde(default)]
    pub coa_enabled: bool,              // toggle global
    #[serde(default = "default_coa_mode")]
    pub coa_mode: String,               // "udp" (listener WG/VPN) | "poll" (HTTP al RADIUS)
    #[serde(default)]
    pub coa_poll_url: String,           // URL del endpoint RADIUS (modo poll)
}

fn default_coa_mode() -> String { "udp".into() }

/// Separa "ip:puerto" -> (ip, puerto) con default si no hay puerto (FIX 2026-08-04).
/// Antes radius_auth/send_accounting ignoraban el puerto del config y SIEMPRE
/// usaban 1812/1813 — si RADIUS corria en otro puerto, auth/accounting fallaban.
fn split_host_port(server: &str, default_port: u16) -> (String, u16) {
    if let Some((host, port)) = server.rsplit_once(':') {
        // IPv6 tiene ':' internos — no confundir con separador de puerto
        if !host.contains(':') {
            if let Ok(p) = port.parse::<u16>() {
                return (host.to_string(), p);
            }
        }
    }
    (server.to_string(), default_port)
}

/// Parse rate_limit del config: "rate_up/rate_down ceil_up/ceil_down"
/// Ej: "1M/2M 2M/3M" → (up="1M", down="2M", up_ceil="2M", down_ceil="3M")
/// Fallback sin '/': "1M 2M" → (up="1M", down="2M", ceils vacios)
fn parse_rate_limit_str(s: &str) -> (String, String, String, String) {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    let mut up = String::new();
    let mut down = String::new();
    let mut up_ceil = String::new();
    let mut down_ceil = String::new();
    if !tokens.is_empty() {
        let rate_parts: Vec<&str> = tokens[0].split('/').collect();
        up = rate_parts[0].to_string();
        if rate_parts.len() >= 2 { down = rate_parts[1].to_string(); }
    }
    if tokens.len() >= 2 {
        let ceil_parts: Vec<&str> = tokens[1].split('/').collect();
        up_ceil = ceil_parts[0].to_string();
        if ceil_parts.len() >= 2 { down_ceil = ceil_parts[1].to_string(); }
    }
    (up, down, up_ceil, down_ceil)
}

/// QoS final de una sesion: VSA de RADIUS si viene, sino fallback del config
/// (rate_limit). Devuelve (up, down, up_ceil, down_ceil).
fn resolve_qos(cfg: &HotspotServer, rad: &RadiusResult) -> (String, String, String, String) {
    if rad.speed_up.is_empty() || rad.speed_down.is_empty() {
        parse_rate_limit_str(&cfg.rate_limit)
    } else {
        (rad.speed_up.clone(), rad.speed_down.clone(), rad.up_ceil_str.clone(), rad.down_ceil_str.clone())
    }
}

// --- API Handlers ---

pub async fn get_server() -> Json<serde_json::Value> {
    // FIX 2026-08-04: incluir wg_ip detectada (solo runtime, NO se guarda)
    // para que la UI muestre el destino del Disconnect en modo WireGuard.
    let cfg = get_hs_config();
    let mut v = serde_json::to_value(&cfg).unwrap_or(serde_json::json!({}));
    if let Some(obj) = v.as_object_mut() {
        obj.insert("wg_ip".into(), serde_json::json!(detect_wg_ip()));
        // FIX (2026-08-04): no exponer el secret RADIUS al browser.
        // post_server conserva el actual si llega "***" o vacio.
        if obj.contains_key("radius_secret") {
            obj.insert("radius_secret".into(), serde_json::json!("***"));
        }
    }
    Json(v)
}

/// Detecta la IP de la VPN WireGuard del servidor (interfaz wg*).
fn detect_wg_ip() -> String {
    if let Ok(out) = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        for line in s.lines() {
            if line.to_lowercase().contains(" wg") {
                // Formato: "3: wg0    inet 10.7.0.5/24 scope global wg0 ..."
                if let Some(pos) = line.find("inet ") {
                    if let Some(ip) = line[pos + 5..].split('/').next() {
                        let ip = ip.trim();
                        if !ip.is_empty() {
                            return ip.to_string();
                        }
                    }
                }
            }
        }
    }
    String::new()
}

pub async fn post_server(body: axum::body::Bytes) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut cfg: HotspotServer = serde_json::from_slice(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("JSON: {}\nNota: campos requeridos: iface, gw, html_dir, idle_timeout, shared_users, rate_limit, radius, radius_secret", e)))?;
    // FIX (2026-08-04): si el form envio "***" (placeholder de la UI) o vacio,
    // conservar el radius_secret actual — la UI ya no recibe el secret real.
    if cfg.radius_secret.is_empty() || cfg.radius_secret == "***" {
        let current = get_hs_config();
        cfg.radius_secret = current.radius_secret.clone();
    }
    save_hs_config(&cfg);
    *HS_CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(cfg.clone());
    // FIX (2026-08-12): reaplicar el firewall hotspot al cambiar la config
    // (iface/gw). ANTES solo se aplicaba en el arranque — cambiar iface a
    // eth4 dejaba el portal :80 apuntando a la iface vieja (eth3).
    let _ = crate::init_hotspot_nft();
    Ok(Json(serde_json::json!({"status": "ok"})))
}

// --- Portal Pages (sirven templates MikroTik reales) ---

/// Wrapper para query params del portal
#[derive(Deserialize, Default)]
pub struct PortalQuery {
    pub error: Option<String>,
    pub username: Option<String>,
}

/// GET /api/hotspot/active — sesiones activas desde nft set + session store
pub async fn active_sessions() -> Json<Vec<serde_json::Value>> {
    let mut sessions = Vec::new();
    // FIX-2a (BUG-1k): nft list en spawn_blocking — no bloquear workers.
    // Misma logica de parseo, solo cambia el hilo de ejecucion.
    let authed_ips = tokio::task::spawn_blocking(fetch_authed_ips).await.unwrap_or_default();
    let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
    let store_map = store.as_ref();
    for ip in &authed_ips {
        let username = store_map.and_then(|m| m.get(ip)).map(|s| s.username.clone()).unwrap_or_default();
        let mac = store_map.and_then(|m| m.get(ip)).map(|s| s.client_mac.clone()).unwrap_or_default();
        let uptime = store_map.and_then(|m| m.get(ip)).map(|s| {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            let secs = now - s.start;
            if secs < 60 { format!("{}s", secs) }
            else if secs < 3600 { format!("{}m {}s", secs / 60, secs % 60) }
            else { format!("{}h {}m", secs / 3600, (secs % 3600) / 60) }
        }).unwrap_or_default();
        sessions.push(serde_json::json!({"username": username, "ip": ip, "mac": mac, "uptime": uptime}));
    }
    Json(sessions)
}

/// FIX-2a: lee IPs autenticadas del set nft hotspot_auth (solo lectura).
/// Extraido para ejecutarse en spawn_blocking (misma logica que antes).
fn fetch_authed_ips() -> Vec<String> {
    let output = std::process::Command::new("nft")
        .args(["-j", "list", "set", "inet", "hotspot", "hotspot_auth"])
        .output().ok();
    let mut authed_ips: Vec<String> = Vec::new();
    if let Some(o) = output {
        if let Ok(data) = serde_json::from_slice::<serde_json::Value>(&o.stdout) {
            if let Some(arr) = data.get("nftables").and_then(|a| a.as_array()) {
                if let Some(meta) = arr.get(1) {
                    if let Some(set_obj) = meta.get("set") {
                        if let Some(elems) = set_obj.get("elem").and_then(|e| e.as_array()) {
                            for e in elems {
                                // Hotspot_auth es type ipv4_addr . ether_addr (concatenado)
                                // Formato JSON: { "elem": { "val": { "concat": ["IP", "MAC"] }, "expires": N } }
                                let ip = e.as_str().map(|s| s.to_string()).or_else(|| {
                                    e.as_object()
                                        .and_then(|obj| obj.get("elem"))
                                        .and_then(|inner| {
                                            // Concatenated: inner puede ser array ["IP", "MAC"] o {"val": {"concat": ["IP","MAC"]}}
                                            if let Some(arr) = inner.as_array() {
                                                arr.get(0).and_then(|v| v.as_str().map(|s| s.to_string()))
                                            } else if let Some(obj) = inner.as_object() {
                                                obj.get("val")
                                                    .and_then(|v| v.get("concat"))
                                                    .and_then(|c| c.as_array())
                                                    .and_then(|arr| arr.get(0))
                                                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                                            } else {
                                                None
                                            }
                                        })
                                });
                                if let Some(ip) = ip {
                                    authed_ips.push(ip);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    authed_ips
}

/// Servir archivos estaticos: /hotspot/portal/static/css/*, /hotspot/portal/static/js/*
pub async fn portal_static(axum::extract::Path(file): axum::extract::Path<String>) -> Result<(StatusCode, [(String, String); 1], Vec<u8>), (StatusCode, String)> {
    let cfg = get_hs_config();
    let html_dir = if cfg.html_dir.is_empty() { "static/hotspot".into() } else { cfg.html_dir.clone() };
    let file_path = format!("{}/{}", html_dir, file);

    if file_path.contains("..") {
        return Err((StatusCode::FORBIDDEN, "Path traversal".into()));
    }

    match tokio::fs::read(&file_path).await {
        Ok(data) => {
            let ext = file.rsplit('.').next().unwrap_or("");
            let mime = match ext {
                "css" => "text/css; charset=utf-8",
                "js" => "application/javascript; charset=utf-8",
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "svg" => "image/svg+xml",
                "ico" => "image/x-icon",
                _ => "text/plain; charset=utf-8",
            };
            Ok((StatusCode::OK, [("Content-Type".into(), mime.into())], data))
        }
        Err(_) => Err((StatusCode::NOT_FOUND, "File not found".into()))
    }
}

/// GET /hotspot/portal — pagina de login
/// Version inline de portal_root para usar desde handle_root sin ConnectInfo
pub fn portal_root_inline() -> String {
    let cfg = get_hs_config();
    let html_dir = if cfg.html_dir.is_empty() { "static/hotspot".into() } else { cfg.html_dir.clone() };
    let login_path = format!("{}/login.html", html_dir);

    match std::fs::read_to_string(&login_path) {
        Ok(html) => render_login(html, "", ""),
        Err(_) => fallback_login_page(),
    }
}

pub async fn portal_root(
    headers: axum::http::HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let client_ip = peer.ip().to_string();
    let cfg = get_hs_config();
    let html_dir = if cfg.html_dir.is_empty() { "static/hotspot".into() } else { cfg.html_dir.clone() };
    let alogin_path = format!("{}/alogin.html", html_dir);

    let (has_session, session_mac) = {
        let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
        match store.as_ref().and_then(|s| s.get(&client_ip)) {
            None => (false, String::new()),
            Some(sess) => (true, sess.client_mac.clone()),
        }
    };
    // FIX (2026-08-04): si la sesion tiene MAC, verificar que el peer actual
    // tiene la MISMA MAC (una IP reasignada por DHCP a otro dispositivo no
    // debe heredar la sesion de la IP anterior). El .await va FUERA del lock.
    let already_authed = if !has_session {
        false
    } else if session_mac.is_empty() {
        true
    } else {
        let mac = get_mac_from_arp(&client_ip).await;
        !mac.is_empty() && mac.to_lowercase() == session_mac.to_lowercase()
    };
    if already_authed {
        let body = match tokio::fs::read_to_string(&alogin_path).await {
            Ok(html) => render_alogin(html),
            Err(_) => format!("<!DOCTYPE html><html><head><meta http-equiv=\\\"refresh\\\" content=\\\"0;url=/\\\"></head><body><p>Ya autenticado</p></body></html>"),
        };
        return Ok(Response::builder().status(200).header("content-type", "text/html; charset=utf-8").body(Body::from(body)).unwrap());
    }

    // Buscar cookie hs_session
    if let Some(cookie_header) = headers.get("cookie") {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for part in cookie_str.split(';') {
                let part = part.trim();
                if let Some(b64) = part.strip_prefix("hs_session=") {
                    if let Ok(decoded) = base64::decode(b64) {
                        if let Ok(cookie_val) = String::from_utf8(decoded) {
                            // FIX (2026-08-04): rsplit_once — la MAC es el ULTIMO
                            // segmento; un username con ':' (valido en RFC 2865)
                            // rompia el split(':') tradicional.
                            if let Some((username, cookie_mac)) = cookie_val.rsplit_once(':') {
                                let client_mac = get_mac_from_arp(&client_ip).await;
                                if !client_mac.is_empty() && client_mac.to_lowercase() == cookie_mac.to_lowercase() {
                                    // Verificar que la cookie existe server-side (no fue eliminada del admin)
                                    if !cookie_entry_exists(username, &cookie_mac) {
                                        zlog!("[HOTSPOT-COOKIE] RECHAZADO (no existe server-side): {} mac={}", username, client_mac);
                                        let login_path = format!("{}/login.html", html_dir);
                                        let body2 = match tokio::fs::read_to_string(&login_path).await {
                                            Ok(html) => render_login(html, "Sesion finalizada", "Cookie no valida. Inicie sesion nuevamente."),
                                            Err(_) => fallback_login_page(),
                                        };
                                        return Ok(Response::builder().status(200)
                                            .header("content-type", "text/html; charset=utf-8")
                                            .header("set-cookie", "hs_session=; Path=/; Max-Age=0; HttpOnly")
                                            .body(Body::from(body2)).unwrap());
                                    }
                                    // FIX (2026-08-04): la cookie ya NO trae el password
                                    // (base64(user:mac)) — buscarlo server-side.
                                    let Some(cookie_password) = find_password_for_username(username, &client_mac) else {
                                        zlog!("[HOTSPOT-COOKIE] RECHAZADO (sin password server-side): {} mac={}", username, client_mac);
                                        let login_path = format!("{}/login.html", html_dir);
                                        let body2 = match tokio::fs::read_to_string(&login_path).await {
                                            Ok(html) => render_login(html, "Sesion finalizada", "Cookie no valida. Inicie sesion nuevamente."),
                                            Err(_) => fallback_login_page(),
                                        };
                                        return Ok(Response::builder().status(200)
                                            .header("content-type", "text/html; charset=utf-8")
                                            .header("set-cookie", "hs_session=; Path=/; Max-Age=0; HttpOnly")
                                            .body(Body::from(body2)).unwrap());
                                    };
                                    // Cookie valida — hacer RADIUS re-auth completo
                                    let rad_result = radius_auth(&cfg.radius, &cfg.radius_secret, username, &cookie_password).await;
                                    if rad_result.success {
                                        // BUG-D: mismo dispositivo (misma MAC) con IP nueva
                                        // (DHCP renew): cerrar sesion anterior con accounting stop.
                                        if !client_mac.is_empty() {
                                            let mac_lower = client_mac.to_lowercase();
                                            let stale_ips: Vec<String> = {
                                                let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
                                                store.as_ref().map(|m| m.iter()
                                                    .filter(|(ip, s)| *ip != &client_ip && s.client_mac.to_lowercase() == mac_lower)
                                                    .map(|(ip, _)| ip.clone()).collect()).unwrap_or_default()
                                            };
                                            for old_ip in stale_ips {
                                                zlog!("[HOTSPOT-COOKIE] misma MAC {} con IP vieja {} — cerrando sesion anterior", client_mac, old_ip);
                                                session_disconnect_internal(&old_ip, &cfg.radius, &cfg.radius_secret, &cfg.iface, 1).await;
                                            }
                                        }
                                        // shared_users: limite de sesiones concurrentes
                                        // (FIX-H6: el login nuevo GANA — reemplaza la mas antigua).
                                        let shared_limit = cfg.shared_users;
                                        make_room_for_session(username, shared_limit, &cfg.radius, &cfg.radius_secret, &cfg.iface).await;
        let session_id = format!("zpot-{}-{}-{}", username,
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            rand::random::<u32>());
                                        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                                        // idle_timeout: RADIUS attr 28 si >0, sino config local, sino default 600
                                        let profile_idle_timeout = cfg.idle_timeout;
                                        let idle_timeout = if rad_result.idle_timeout > 0 { rad_result.idle_timeout } else { profile_idle_timeout };
                                        // FIX-6 (2026-08-04): QoS — VSA de RADIUS si viene, sino
                                        // fallback cfg.rate_limit del server config.
                                        let (speed_up, speed_down, up_ceil_str, down_ceil_str) = resolve_qos(&cfg, &rad_result);
                                        let session = HotspotSession {
                                            username: username.to_string(),
                                            client_ip: client_ip.clone(),
                                            client_mac: client_mac.clone(),
                                            session_id: session_id.clone(),
                                            start: now,
                                            speed_up: speed_up.clone(),
                                            speed_down: speed_down.clone(),
                                            rx_bytes: 0,
                                            tx_bytes: 0,
                                            idle_timeout,
                                            last_active: now,
                                            nft_expire: now,
                                            up_ceil_str: up_ceil_str.clone(),
                                            down_ceil_str: down_ceil_str.clone(),
                                        };
                                        session_store().lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(client_ip.clone(), session);
                                        save_sessions_to_disk();
                                        let iface = &cfg.iface;
                                        // FIX-3c (BUG-1i): apply_qos (QOS_LOCK interno) en spawn_blocking.
                                        {
                                            let c_ip = client_ip.clone();
                                            let c_iface = iface.clone();
                                            let c_su = speed_up.clone();
                                            let c_sd = speed_down.clone();
                                            let c_uc = up_ceil_str.clone();
                                            let c_dc = down_ceil_str.clone();
                                            tokio::task::spawn_blocking(move || {
                                                apply_qos(&c_ip, &c_iface, &c_su, &c_sd, &c_uc, &c_dc);
                                            }).await.ok();
                                        }
                                        // FIX-4 (BUG-14): add_bypass_nft (3 nft cmds) en spawn_blocking.
                                        let c_ip = client_ip.clone();
                                        let c_mac = client_mac.clone();
                                        tokio::task::spawn_blocking(move || add_bypass_nft(&c_ip, &c_mac)).await.ok();
                                        send_accounting(&cfg.radius, &cfg.radius_secret, username, &client_ip, 1, &session_id, 0, 0, 0, 0);
                                        // FIX-8 (BUG-6): interim lo cubre el task GLOBAL (spawn_interim_global).
                                        zlog!("[HOTSPOT-COOKIE] RADIUS re-auth OK: {} ip={} mac={} up={} down={}",
                                            username, client_ip, client_mac, rad_result.speed_up, rad_result.speed_down);
                                        let body = match tokio::fs::read_to_string(&alogin_path).await {
                                            Ok(html) => render_alogin(html),
                                            Err(_) => format!("<!DOCTYPE html><html><head><meta http-equiv=\\\"refresh\\\" content=\\\"0;url=/\\\"></head><body><p>Autenticado como {}. Redirigiendo...</p></body></html>", username),
                                        };
                                        return Ok(Response::builder().status(200).header("content-type", "text/html; charset=utf-8").body(Body::from(body)).unwrap());
                                    } else {
                                        // RADIUS rechazo — mostrar Reply-Message
                                        let err_msg = if rad_result.reply_message.is_empty() { "Acceso denegado" } else { &rad_result.reply_message };
                                        zlog!("[HOTSPOT-COOKIE] RADIUS re-auth FAIL: {} ip={} msg={}", username, client_ip, err_msg);
                                        let login_path = format!("{}/login.html", html_dir);
                                        let body2 = match tokio::fs::read_to_string(&login_path).await {
                                            Ok(html) => render_login(html, err_msg, err_msg),
                                            Err(_) => fallback_login_page(),
                                        };
                                        return Ok(Response::builder().status(200)
                                            .header("content-type", "text/html; charset=utf-8")
                                            .header("set-cookie", "hs_session=; Path=/; Max-Age=0; HttpOnly")
                                            .body(Body::from(body2)).unwrap());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Sin sesion ni cookie valida → servir login
    let login_path = format!("{}/login.html", html_dir);
    match tokio::fs::read_to_string(&login_path).await {
        Ok(html) => Ok(Response::builder().status(200).header("content-type", "text/html; charset=utf-8").body(Body::from(render_login(html, "", ""))).unwrap()),
        Err(_) => Ok(Response::builder().status(200).header("content-type", "text/html; charset=utf-8").body(Body::from(fallback_login_page())).unwrap()),
    }
}

/// GET /hotspot/portal/login?error=... — login con error
pub async fn portal_login(
    axum::extract::Query(query): axum::extract::Query<PortalQuery>,
) -> Result<axum::response::Html<String>, (StatusCode, String)> {
    let err = query.error.unwrap_or_default();
    let cfg = get_hs_config();
    let html_dir = if cfg.html_dir.is_empty() { "static/hotspot".into() } else { cfg.html_dir.clone() };
    let login_path = format!("{}/login.html", html_dir);

    match tokio::fs::read_to_string(&login_path).await {
        Ok(html) => Ok(axum::response::Html(render_login(html, &err, &err))),
        Err(_) => Ok(axum::response::Html(fallback_login_page()))
    }
}

/// POST /hotspot/portal/auth — autenticar
#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
}

/// FIX-H6 (2026-08-04): el login nuevo GANA. Si el usuario alcanzo el limite
/// de shared_users, se cierra(n) la(s) sesion(es) mas antigua(s) del mismo
/// usuario (accounting stop + limpieza nft/tc) hasta dejar espacio.
/// Con limite=1: el dispositivo B desconecta a A y toma su lugar
/// (antes se rechazaba con "Session limit reached").
async fn make_room_for_session(username: &str, limit: u32, rad_srv: &str, rad_sec: &str, iface: &str) {
    if limit == 0 { return; }
    loop {
        let (count, oldest_ip) = {
            let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
            let count = store.as_ref()
                .map(|m| m.values().filter(|s| s.username.eq_ignore_ascii_case(username)).count())
                .unwrap_or(0);
            let oldest = store.as_ref().and_then(|m| m.iter()
                .filter(|(_, s)| s.username.eq_ignore_ascii_case(username))
                .min_by_key(|(_, s)| s.start)
                .map(|(k, _)| k.clone()));
            (count, oldest)
        };
        if count < limit as usize { break; }
        if let Some(oldest_ip) = oldest_ip {
            zlog!("[HOTSPOT-SHARED] {} limite={} con {} sesiones — reemplazando la mas antigua ({})",
                username, limit, count, oldest_ip);
            session_disconnect_internal(&oldest_ip, rad_srv, rad_sec, iface, 1).await;
        } else {
            break;
        }
    }
}

// ─── Anti-brute-force del login (FIX 2026-08-04) ─────────────
// NO es el rate-limit de ANCHO DE BANDA (ese viene del VSA RADIUS
// Mikrotik-Rate-Limit o del fallback rate_limit del server — QoS).
// Este limita los INTENTOS de autenticacion por IP: 5 fallos en 60s
// -> bloqueo temporal (fuerza bruta de passwords via portal).
static FAILED_LOGINS: std::sync::Mutex<Option<std::collections::HashMap<String, (u32, u64)>>> = std::sync::Mutex::new(None);
const MAX_FAILED_LOGINS: u32 = 5;
const FAIL_WINDOW_SECS: u64 = 60;

pub async fn portal_auth(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    axum::extract::Form(mut form): axum::extract::Form<LoginForm>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    // FreeRADIUS exige username en MAYÚSCULAS (case-sensitive, Reject si mixto).
    // Normalizar en el punto de entrada para que "RamonHu" funcione igual que "RAMONHU"
    // en el Access-Request, la sesión, la cookie y el accounting.
    form.username = form.username.trim().to_uppercase();

    let cfg = get_hs_config();
    let rad_srv = cfg.radius.clone();
    let rad_sec = cfg.radius_secret.clone();
    let html_dir = if cfg.html_dir.is_empty() { "static/hotspot".into() } else { cfg.html_dir.clone() };

    if rad_srv.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "RADIUS not configured".into()));
    }

    // FIX (2026-08-04): rechazar vacios ANTES de consumir un roundtrip RADIUS
    // y sin tocar el contador anti-brute-force.
    if form.username.is_empty() || form.password.is_empty() {
        let login_path = format!("{}/login.html", html_dir);
        let body = match tokio::fs::read_to_string(&login_path).await {
            Ok(html) => render_login(html, "Campos requeridos", "Ingrese usuario y contraseña"),
            Err(_) => fallback_login_page()
        };
        return Ok((StatusCode::OK, [("content-type", "text/html; charset=utf-8")], body).into_response());
    }

    // FIX (2026-08-04): anti-brute-force — bloquear IPs con >=5 fallos en 60s.
    // La decision se toma sin mantener el lock a traves del .await (MutexGuard
    // no es Send -> rompia el Handler de axum).
    let ip_blocked = {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let mut guard = FAILED_LOGINS.lock().unwrap_or_else(|e| e.into_inner());
        let fails = guard.get_or_insert_with(std::collections::HashMap::new);
        // FIX (2026-08-04): poda — un brute-force distribuido no debe crecer
        // el HashMap indefinidamente; si supera 1024 IPs se barren las expiradas.
        if fails.len() > 1024 {
            fails.retain(|_, (_, start)| now - *start < FAIL_WINDOW_SECS);
        }
        let mut block = false;
        if let Some((count, start)) = fails.get(&peer.ip().to_string()) {
            if *count >= MAX_FAILED_LOGINS && now - *start < FAIL_WINDOW_SECS {
                block = true;
            }
            if now - *start >= FAIL_WINDOW_SECS {
                fails.remove(&peer.ip().to_string());
            }
        }
        block
    };
    if ip_blocked {
        zlog!("[AUTH] bloqueado por fuerza bruta ip={}", peer.ip());
        let login_path = format!("{}/login.html", html_dir);
        let body = match tokio::fs::read_to_string(&login_path).await {
            Ok(html) => render_login(html, "Demasiados intentos", "Demasiados intentos fallidos. Espere 60 segundos."),
            Err(_) => fallback_login_page()
        };
        return Ok((StatusCode::OK, [("content-type", "text/html; charset=utf-8")], body).into_response());
    }

    let rad_result = radius_auth(&rad_srv, &rad_sec, &form.username, &form.password).await;

    if rad_result.success {
        let client_ip = peer.ip().to_string();
        // Login OK — limpiar contador de fuerza bruta
        FAILED_LOGINS.lock().unwrap_or_else(|e| e.into_inner()).as_mut().map(|m| m.remove(&client_ip));
        // FIX (2026-08-04): sin MAC no hay bypass nft -> cliente "autenticado"
        // sin internet. Reintentar ARP (puede estar transicionando) y si sigue
        // sin resolver, RECHAZAR el login con mensaje claro (antes creaba la
        // sesion con client_mac="" y el cliente quedaba atrapado en el portal).
        let mut client_mac = get_mac_from_arp(&client_ip).await;
        if client_mac.is_empty() {
            for _ in 0..3 {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                client_mac = get_mac_from_arp(&client_ip).await;
                if !client_mac.is_empty() { break; }
            }
        }
        if client_mac.is_empty() {
            zlog!("[HOTSPOT] login {} ip={} SIN MAC (ARP no resuelto) — login rechazado", form.username, client_ip);
            let login_path = format!("{}/login.html", html_dir);
            let body = match tokio::fs::read_to_string(&login_path).await {
                Ok(html) => render_login(html, "Error de red", "No se pudo resolver su direccion MAC. Reintente en unos segundos."),
                Err(_) => fallback_login_page()
            };
            return Ok((StatusCode::OK, [("content-type", "text/html; charset=utf-8")], body).into_response());
        }

        // BUG-D: mismo dispositivo (misma MAC) con IP nueva (DHCP renew /
        // reconexion): cerrar la sesion anterior con accounting stop — evita
        // que shared_users bloquee al cliente legitimo y deja limpio el store.
        if !client_mac.is_empty() {
            let mac_lower = client_mac.to_lowercase();
            let stale_ips: Vec<String> = {
                let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
                store.as_ref().map(|m| m.iter()
                    .filter(|(ip, s)| *ip != &client_ip && s.client_mac.to_lowercase() == mac_lower)
                    .map(|(ip, _)| ip.clone()).collect()).unwrap_or_default()
            };
            for old_ip in stale_ips {
                zlog!("[HOTSPOT] misma MAC {} con IP vieja {} — cerrando sesion anterior", client_mac, old_ip);
                session_disconnect_internal(&old_ip, &rad_srv, &rad_sec, &cfg.iface, 1).await;
            }
        }

        // FIX (2026-08-04): re-login sobre sesion existente de la MISMA IP —
        // cerrar la anterior con Accounting-Stop (antes el insert pisaba la
        // sesion y el Stop nunca salia -> radacct facturaba para siempre).
        {
            let exists = session_store().lock().unwrap_or_else(|e| e.into_inner())
                .as_ref().map(|m| m.contains_key(&client_ip)).unwrap_or(false);
            if exists {
                zlog!("[HOTSPOT] re-login {} ip={} — cerrando sesion previa con stop", form.username, client_ip);
                session_disconnect_internal(&client_ip, &rad_srv, &rad_sec, &cfg.iface, 1).await;
            }
        }

        // shared_users: limite de sesiones concurrentes (FIX-H6: el login
        // nuevo GANA — con limite=1 el dispositivo B reemplaza a A).
        let shared_limit = cfg.shared_users;
        make_room_for_session(&form.username, shared_limit, &rad_srv, &rad_sec, &cfg.iface).await;

        let session_id = format!("zpot-{}-{}-{}", form.username,
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            rand::random::<u32>());
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        // idle_timeout: RADIUS attr 28 si >0, sino config local, sino default 600
        let profile_idle_timeout = cfg.idle_timeout;
        let idle_timeout = if rad_result.idle_timeout > 0 { rad_result.idle_timeout } else { profile_idle_timeout };
        // FIX-6 (2026-08-04): QoS — VSA de RADIUS si viene, sino fallback
        // cfg.rate_limit del server config.
        let (speed_up, speed_down, up_ceil_str, down_ceil_str) = resolve_qos(&cfg, &rad_result);

        let session = HotspotSession {
            username: form.username.clone(),
            client_ip: client_ip.clone(),
            client_mac: client_mac.clone(),
            session_id: session_id.clone(),
            start: now,
            speed_up: speed_up.clone(),
            speed_down: speed_down.clone(),
            rx_bytes: 0,
            tx_bytes: 0,
            idle_timeout,
            last_active: now,
            nft_expire: now,
            up_ceil_str: up_ceil_str.clone(),
            down_ceil_str: down_ceil_str.clone(),
        };
        session_store().lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(client_ip.clone(), session);
        save_sessions_to_disk();

        let iface = &cfg.iface;
        // FIX-3c (BUG-1i): apply_qos (QOS_LOCK interno) en spawn_blocking.
        {
            let c_ip = client_ip.clone();
            let c_iface = iface.clone();
            let c_su = speed_up.clone();
            let c_sd = speed_down.clone();
            let c_uc = up_ceil_str.clone();
            let c_dc = down_ceil_str.clone();
            tokio::task::spawn_blocking(move || {
                apply_qos(&c_ip, &c_iface, &c_su, &c_sd, &c_uc, &c_dc);
            }).await.ok();
        }
        // FIX-4 (BUG-14): add_bypass_nft (3 nft cmds) en spawn_blocking.
        let c_ip = client_ip.clone();
        let c_mac = client_mac.clone();
        tokio::task::spawn_blocking(move || add_bypass_nft(&c_ip, &c_mac)).await.ok();
        send_accounting(&rad_srv, &rad_sec, &form.username, &client_ip, 1, &session_id, 0, 0, 0, 0);
        // FIX-8 (BUG-6): interim lo cubre el task GLOBAL (spawn_interim_global).

        zlog!("[HOTSPOT] INICIO SESION: {} ip={} mac={} up={} down={}",
            form.username, client_ip, client_mac, rad_result.speed_up, rad_result.speed_down);

        // Crear cookie para auto-reconexion.
        // FIX (2026-08-04): cookie SIN password en claro — antes
        // base64(user:pass:mac) viajaba por HTTP y un sniffer decodificaba
        // el password RADIUS del cliente. Ahora solo user:mac; el password
        // se busca server-side en hotspot-cookies.json al re-autenticar.
        let cookie_val = format!("{}:{}", form.username, client_mac);
        let cookie_b64 = base64::encode(&cookie_val);
        let cookie = format!("hs_session={}; Path=/; Max-Age=604800; SameSite=Lax; HttpOnly", cookie_b64);
        // Guardar cookie server-side (MikroTik-style: server conoce las cookies activas)
        save_cookie_entry(&form.username, &form.password, &client_mac, 604800);

        // Servir alogin.html con cookie
        let alogin_path = format!("{}/alogin.html", html_dir);
        let body = match tokio::fs::read_to_string(&alogin_path).await {
            Ok(html) => render_alogin(html),
            Err(_) => format!("<!DOCTYPE html><html><head><meta http-equiv='refresh' content='0;url=/'></head><body><p>Autenticado como {}. Redirigiendo...</p></body></html>",
                escape_html(&form.username))
        };
        let resp = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/html; charset=utf-8")
            .header("set-cookie", &cookie)
            .body(axum::body::Body::from(body))
            .unwrap();
        Ok(resp)
    } else {
        // FIX (2026-08-04): timeout de RADIUS ≠ reject — el server no
        // respondio (reachable=false). NO contar como fallo de password
        // (el anti-bruteforce solo bloquea rejects reales) y mostrar un
        // mensaje de servicio, no "Invalid username or password".
        let timed_out = !rad_result.reachable;
        if timed_out {
            zlog!("[AUTH] RADIUS no respondio para {} ip={} — no se cuenta como fallo", form.username, peer.ip());
        } else {
            // Registrar intento fallido (anti-brute-force)
            {
                let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                let mut guard = FAILED_LOGINS.lock().unwrap_or_else(|e| e.into_inner());
                let fails = guard.get_or_insert_with(std::collections::HashMap::new);
                let entry = fails.entry(peer.ip().to_string()).or_insert((0, now));
                if now - entry.1 >= FAIL_WINDOW_SECS { *entry = (0, now); }
                entry.0 += 1;
                zlog!("[AUTH] intento fallido {} ip={} ({} en 60s)", form.username, peer.ip(), entry.0);
            }
        }
        // Servir login con error — usar Reply-Message de RADIUS si existe
        let err_msg = if timed_out {
            "Servidor RADIUS no disponible, reintente"
        } else if rad_result.reply_message.is_empty() { "Invalid username or password" } else { &rad_result.reply_message };
        let login_path = format!("{}/login.html", html_dir);
        let body = match tokio::fs::read_to_string(&login_path).await {
            Ok(html) => render_login(html, err_msg, err_msg),
            Err(_) => fallback_login_page()
        };
        Ok((StatusCode::OK, [("content-type", "text/html; charset=utf-8")], body).into_response())
    }
}

// --- Interim task compartido (portal_auth + cookie auto-login) ---

/// FIX-8 (BUG-6): UN SOLO task global que cada 60s barre TODAS las sesiones
/// hotspot (accounting Interim, idle-timeout, re-auth RADIUS).
/// Reemplaza spawn_interim_task (1 task por sesion — escalaba mal:
/// N sesiones = N tasks x tc/radius por minuto).
pub fn spawn_interim_global() {
    tokio::spawn(async {
        // FIX (2026-08-04): contador de ciclos para espaciar el reauth RADIUS.
        let mut cycle: u64 = 0;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            cycle += 1;
            let cfg = get_hs_config();
            let rad_srv = cfg.radius.clone();
            let rad_sec = cfg.radius_secret.clone();
            let iface = cfg.iface.clone();
            // Snapshot de sesiones (sin mantener el lock)
            let sessions: Vec<(String, String, String, u32, String)> = {
                let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
                store.as_ref().map(|m| m.iter().map(|(ip, s)| (
                    ip.clone(), s.username.clone(), s.session_id.clone(), s.idle_timeout, s.client_mac.clone()
                )).collect()).unwrap_or_default()
            };
            for (session_idx, (ip, username, session_id, idle_timeout, client_mac)) in sessions.iter().enumerate() {
                let (session_time, session_idle_cur) = {
                    let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
                    let session = store.as_ref().and_then(|m| m.get(ip));
                    match session {
                        None => continue,
                        Some(s) => {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                            // FIX (2026-08-04): saturating_sub — un reloj hacia
                            // atras (NTP/ajuste) no debe producir session_time
                            // gigante ni expulsiones por idle masivas.
                            (now.saturating_sub(s.start), s.idle_timeout)
                        }
                    }
                };

                // Leer contadores reales desde TC
                let (down_minor, up_minor) = ip_to_minors(ip);
                let rx = read_tc_bytes(&iface, down_minor).await;
                let tx = read_tc_bytes(&format!("ifb_{}", iface), up_minor).await;
                zlog!("[INTERIM] {} time={} rx={} tx={}", username, session_time, rx, tx);
                send_accounting(&rad_srv, &rad_sec, username, ip, 3, session_id, session_time, rx, tx, 0);

                // Detectar trafico: actualizar contadores/last_active (idle) y
                // marcar renovacion del bypass nft (BUG-E: el set expira a las
                // 24h — mientras haya trafico se renueva, sin depender del idle).
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                let mut idle_expired = false;
                let mut renovar_bypass = false;
                {
                    let mut store = session_store().lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(map) = store.as_mut() {
                        if let Some(session) = map.get_mut(ip) {
                            let prev_rx = session.rx_bytes;
                            let prev_tx = session.tx_bytes;
                            let last_active = session.last_active;
                            if rx != prev_rx || tx != prev_tx {
                                // Hay trafico — actualizar contadores y last_active
                                session.rx_bytes = rx;
                                session.tx_bytes = tx;
                                session.last_active = now;
                                // FIX (2026-08-04): sesion ACTIVA con trafico
                                // >6 dias — refrescar cookie server-side (antes
                                // expiraba a los 7 dias y el interim expulsaba
                                // por "cookie no valida" aunque el cliente
                                // siguiera navegando).
                                if session_time >= 6 * 86400 {
                                    if let Some(pw) = find_password_for_username(username, client_mac) {
                                        save_cookie_entry(username, &pw, &client_mac.clone(), 604800);
                                        zlog!("[COOKIE] refrescada server-side para {} (sesion activa >6d)", username);
                                    }
                                }
                            } else if *idle_timeout > 0
                                && now.saturating_sub(last_active) >= *idle_timeout as u64 {
                                idle_expired = true;
                            }
                            // FIX (2026-08-04): renovacion del bypass nft
                            // INDEPENDIENTE del trafico — una sesion inactiva
                            // (dentro del idle) no debe perder el elemento a las
                            // 24h y quedar atrapada en el portal al volver a
                            // navegar. ANTES solo se renovaba con trafico.
                            if !client_mac.is_empty()
                                && (session.nft_expire == 0 || now.saturating_sub(session.nft_expire) >= 22 * 3600) {
                                session.nft_expire = now;
                                renovar_bypass = true;
                            }
                        }
                    }
                }
                // BUG-E: renovar el elemento hotspot_auth (timeout 24h) —
                // delete+add en el MISMO bloque para minimizar la ventana.
                // Ocurre ~1 vez por sesion cada 22-24h (solo con trafico).
                if renovar_bypass {
                    let c_ip = ip.clone();
                    let c_mac = client_mac.clone();
                    tokio::task::spawn_blocking(move || {
                        let _ = Command::new("nft")
                            .args(["delete", "element", "inet", "hotspot", "hotspot_auth", "{", &c_ip, ".", &c_mac, "}"])
                            .output();
                        let _ = Command::new("nft")
                            .args(["add", "element", "inet", "hotspot", "hotspot_auth", "{", &c_ip, ".", &c_mac, "timeout", "24h", "}"])
                            .output();
                    }).await.ok();
                }
                if idle_expired {
                    zlog!("[IDLE-TIMEOUT] {} ip={} inactivo >= {}s, desconectando",
                        username, ip, idle_timeout);
                    session_disconnect_internal(ip, &rad_srv, &rad_sec, &iface, 4).await; // Idle-Timeout
                    continue;
                }
                // Re-autenticar con RADIUS — FIX (2026-08-04): cada 5 ciclos
                // (300s), antes cada 60s por sesion: con N sesiones el loop se
                // degradaba (interims atrasados) y saturaba el server. El
                // polling CoA ya detecta sesiones cerradas en RADIUS.
                // FIX (2026-08-08): ESPACIAR por sesion — antes TODAS se
                // reauticaban en el MISMO ciclo (rafaga sincronizada cada
                // 300s → FreeRADIUS encolaba y el NAS reenviaba = "duplicate
                // packet" en el server). Ahora cada sesion se reautica cada
                // 5 ciclos pero en ciclos distintos (~N/5 por ciclo).
                if (cycle + session_idx as u64) % 5 == 0 {
                    match find_password_for_username(username, client_mac) {
                        Some(password) => {
                            let reauth = radius_auth(&rad_srv, &rad_sec, username, &password).await;
                            if reauth.rejected {
                                zlog!("[REAUTH] {} saldo agotado en RADIUS, desconectando", username);
                                session_disconnect_internal(ip, &rad_srv, &rad_sec, &iface, 5).await;
                            }
                        }
                        None => {
                            zlog!("[REAUTH] {} no tiene cookie valida (expirada/eliminada), expulsando", username);
                            session_disconnect_internal(ip, &rad_srv, &rad_sec, &iface, 6).await;
                        }
                    }
                }
            }
        }
    });
}

/// Spawn interim-update task: cada 60s verifica session-timeout, idle-timeout,
/// envia RADIUS Accounting-Interim, y desconecta si es necesario.
/// Usado tanto por portal_auth (login form) como por portal_root (cookie auto-login).
/// Version sync de portal_status para handle_root (sin ConnectInfo, sin async)
pub fn portal_status_inline(client_ip: String) -> (StatusCode, [(&'static str, String); 1], String) {
    let cfg = get_hs_config();
    let html_dir = if cfg.html_dir.is_empty() { "static/hotspot".into() } else { cfg.html_dir.clone() };
    let status_path = format!("{}/status.html", html_dir);

    // Buscar sesion por IP
    let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
    let logout_username = store.as_ref()
        .and_then(|s| s.get(&client_ip))
        .map(|s| s.username.clone())
        .unwrap_or_default();
    drop(store); // liberar lock

    if logout_username.is_empty() {
        return (
            StatusCode::FOUND,
            [("location", "/".to_string())],
            String::new(),
        );
    }

    let logout_link = format!("/logout?username={}", logout_username);

    let html = match std::fs::read_to_string(&status_path) {
        Ok(html) => html,
        Err(_) => {
            return (
                StatusCode::OK,
                [("content-type", "text/html; charset=utf-8".to_string())],
                format!(r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><title>Estado</title></head>
<body><div class="container"><h1>Conexión activa</h1>
<p>Estás autenticado como {}</p>
<a href="{}" class="button">Cerrar sesión</a></div></body></html>"#, logout_username, logout_link)
            );
        }
    };

    let html = html.replace("$(link-logout)", &logout_link)
        .replace("$(link-login)", "/");

    (StatusCode::OK, [("content-type", "text/html; charset=utf-8".to_string())], html)
}

/// GET /hotspot/portal/status — estado de la sesion activa
pub async fn portal_status(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    _raw_query: axum::extract::RawQuery,
) -> impl axum::response::IntoResponse {
    let cfg = get_hs_config();
    let html_dir = if cfg.html_dir.is_empty() { "static/hotspot".into() } else { cfg.html_dir.clone() };
    let status_path = format!("{}/status.html", html_dir);

    // FIX (2026-08-04): SIEMPRE la sesion del PEER — el ?username= del query
    // era editable (/status?username=VICTIMA mostraba el estado de otro y el
    // link de logout apuntaba a la victima). El estado solo es del que pide.
    let logout_username = {
        let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
        store.as_ref().and_then(|s| s.get(&peer.ip().to_string())).map(|s| s.username.clone()).unwrap_or_default()
    };

    // Si no hay sesion activa ni username, redirigir al login
    if logout_username.is_empty() {
        return (
            StatusCode::FOUND,
            [("location", "/")],
            String::new(),
        );
    }

    let logout_link = format!("/logout?username={}", logout_username);

    let html = match tokio::fs::read_to_string(&status_path).await {
        Ok(html) => html,
        Err(_) => {
            r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><title>Estado</title></head>
<body><div class="container"><h1>Conexión activa</h1>
<p>Tu sesión está en curso.</p>
<a href="$(link-logout)" class="button">Cerrar sesión</a></div></body></html>"#.to_string()
        }
    };

    let html = html.replace("$(link-logout)", &logout_link)
        .replace("$(link-login)", "/");

    (StatusCode::OK, [("content-type", "text/html; charset=utf-8")], html)
}
/// GET /hotspot/portal/logout?username=X — cerrar sesion
/// FIX (2026-08-04): CSRF — exige cookie hs_session valida del peer. Un
/// <img> cross-site NO envia la cookie (SameSite=Lax) -> el logout remoto
/// (DoS por img en cualquier pagina) queda bloqueado.
pub async fn portal_logout(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    _query: axum::extract::RawQuery,
) -> (StatusCode, [(&'static str, String); 2], String) {
    let cfg = get_hs_config();
    let rad_srv = cfg.radius.clone();
    let rad_sec = cfg.radius_secret.clone();
    let iface = cfg.iface.clone();
    let html_dir = if cfg.html_dir.is_empty() { "static/hotspot".into() } else { cfg.html_dir.clone() };

    let peer_ip = peer.ip().to_string();

    // CSRF guard: la cookie hs_session (user:mac) debe existir server-side
    let cookie_valid = {
        let mut ok = false;
        if let Some(ch) = headers.get("cookie").and_then(|v| v.to_str().ok()) {
            for part in ch.split(';') {
                let part = part.trim();
                if let Some(b64) = part.strip_prefix("hs_session=") {
                    if let Ok(decoded) = base64::decode(b64) {
                        if let Ok(val) = String::from_utf8(decoded) {
                            // FIX (2026-08-04): rsplit_once (username con ':' OK)
                            // + verificar que la MAC de la cookie coincida con la
                            // MAC ARP del peer (refuerzo CSRF — una cookie de
                            // otro dispositivo no sirve para cerrar esta sesion).
                            if let Some((u, m)) = val.rsplit_once(':') {
                                let peer_mac = get_mac_from_arp(&peer_ip).await;
                                if !peer_mac.is_empty() && peer_mac.to_lowercase() == m.to_lowercase()
                                    && cookie_entry_exists(u, m) {
                                    ok = true;
                                }
                            }
                        }
                    }
                }
            }
        }
        ok
    };
    if !cookie_valid {
        zlog!("[LOGOUT] rechazado (sin cookie valida) de {}", peer_ip);
        return (
            StatusCode::FOUND,
            [
                ("location", "/".to_string()),
                ("set-cookie", "hs_session=; Path=/; Max-Age=0; HttpOnly".to_string()),
            ],
            String::new(),
        );
    }

    // BUG-B: SOLO la sesion del PEER — un cliente hotspot no puede cerrar
    // la sesion de otro usuario (antes se buscaba por username en TODO el
    // store y cualquiera podia hacer /logout?username=VICTIMA).
    let removed: Option<(String, HotspotSession)> = {
        let mut store = session_store().lock().unwrap_or_else(|e| e.into_inner());
        let store_map = store.get_or_insert_with(HashMap::new);
        store_map.remove(&peer_ip).map(|s| (peer_ip.clone(), s))
    };

    if let Some((ip, session)) = &removed {
        // BUG-A: el logout debe eliminar la cookie server-side — si no, el
        // cliente se re-autentica solo al navegar (portal_root con cookie)
        // y NO puede cerrar su cuenta (el saldo seguiria consumiendose).
        delete_cookie_entry(&session.username, &session.client_mac);
        let client_mac = &session.client_mac;
        let (down_min, up_min) = ip_to_minors(ip);
        // FIX-3d (BUG-10): leer contadores ANTES de borrar las clases tc
        // (tras class del devuelven 0) y TX desde ifb_{iface} (no iface).
        let rx = read_tc_bytes(&iface, down_min).await;
        let tx = read_tc_bytes(&format!("ifb_{}", iface), up_min).await;
        // FIX-3b (BUG-1j): borrado nft/tc en spawn_blocking — no bloquear workers.
        let ip_c = ip.clone();
        let mac_c = client_mac.clone();
        let iface_c = iface.clone();
        let prio = format!("{}", 100 + (down_min - 1000) / 2);
        let down_cid = format!("1:{down_min}");
        let up_cid = format!("1:{up_min}");
        let ifb_c = format!("ifb_{}", iface);
        tokio::task::spawn_blocking(move || {
            let _ = std::process::Command::new("nft")
                .args(["delete", "element", "inet", "hotspot", "hotspot_auth", "{", &ip_c, ".", &mac_c, "}"])
                .output();
            let _ = std::process::Command::new("tc")
                .args(["filter", "del", "dev", &iface_c, "parent", "1:0", "protocol", "ip", "prio", &prio, "u32", "match", "ip", "dst", &ip_c, "flowid", &down_cid])
                .output();
            // FIX (2026-08-04): el filtro UP se crea en ifb_{iface} (apply_qos),
            // NO en iface — ANTES quedaba huerfano apuntando a una clase
            // ya eliminada (mismo bug que session_disconnect_internal).
            let _ = std::process::Command::new("tc")
                .args(["filter", "del", "dev", &ifb_c, "parent", "1:0", "protocol", "ip", "prio", &prio, "u32", "match", "ip", "src", &ip_c, "flowid", &up_cid])
                .output();
            let _ = std::process::Command::new("tc")
                .args(["class", "del", "dev", &iface_c, "classid", &down_cid])
                .output();
            let _ = std::process::Command::new("tc")
                .args(["class", "del", "dev", &ifb_c, "classid", &up_cid])
                .output();
        }).await.ok();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let session_time = now - session.start;
        send_accounting(&rad_srv, &rad_sec, &session.username, ip, 2, &session.session_id, session_time, rx, tx, 1); // User-Request
        // Flush conntrack para corte instantaneo de internet
        crate::handlers::helpers::conntrack_flush(ip).await;
        zlog!("[HOTSPOT] LOGOUT: {} ({})", session.username, ip);
    }
    save_sessions_to_disk();

    // Servir logout.html
    let logout_path = format!("{}/logout.html", html_dir);
    let html = match std::fs::read_to_string(&logout_path) {
        Ok(html) => html.replace("$(link-login)", "/"),
        Err(_) => {
            r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><title>Sesión Finalizada</title></head>
<body><div class="container"><h1>Sesión finalizada</h1>
<p>Gracias por usar nuestro servicio.</p>
<a href="/" class="button">Volver a conectar</a></div></body></html>"#.to_string()
        }
    };

    // FIX (2026-08-04): el logout tambien borra la cookie del browser —
    // antes quedaba y el cliente se re-autenticaba solo al navegar.
    let headers = [
        ("content-type", "text/html; charset=utf-8".to_string()),
        ("set-cookie", "hs_session=; Path=/; Max-Age=0; HttpOnly".to_string()),
    ];
    (StatusCode::OK, headers, html)
}

/// Limpia una sesion del hotspot: nft, tc, accounting Stop, conntrack, store
/// Se usa desde portal_disconnect (admin), interim-update (timeout automatico),
/// ARP cleanup (FIX-7: cliente se fue del WiFi) y CoA/Disconnect-Request.
pub async fn session_disconnect_internal(ip: &str, rad_srv: &str, rad_sec: &str, iface: &str, terminate_cause: u32) {
    let (username, client_mac, session_id, start, rx, tx) = {
        let mut store = session_store().lock().unwrap_or_else(|e| e.into_inner());
        let store_map = store.as_mut();
        if let Some(map) = store_map {
            if let Some(session) = map.remove(ip) {
                let username = session.username.clone();
                let client_mac = session.client_mac.clone();
                let session_id = session.session_id.clone();
                let start = session.start;
                let rx = session.rx_bytes;
                let tx = session.tx_bytes;
                drop(store); // liberar lock antes de comandos externos
                (username, client_mac, session_id, start, rx, tx)
            } else {
                return; // sesion no encontrada
            }
        } else {
            return;
        }
    };
    save_sessions_to_disk();

    // FIX (2026-08-04): las expulsiones NO-idle (admin reset, lost-carrier,
    // polling, ARP) borran la cookie server-side para que el cliente no se
    // auto-reconecte al instante (antes el "desconectar" del admin era
    // inefectivo: la cookie re-autenticaba sola). El idle (4) NO borra —
    // el cliente que vuelve debe reconectar con su cookie.
    if terminate_cause != 4 {
        delete_cookie_entry(&username, &client_mac);
    }

    // Leer TC counters para accounting stop
    let (down_min, up_min) = ip_to_minors(ip);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let session_time = now.saturating_sub(start);
    let final_rx = read_tc_bytes(iface, down_min).await;
    let final_tx = read_tc_bytes(&format!("ifb_{}", iface), up_min).await;
    // Usar el valor mas reciente disponible
    let stop_rx = if final_rx > 0 { final_rx } else { rx };
    let stop_tx = if final_tx > 0 { final_tx } else { tx };

    // FIX-3b (BUG-1j): borrado nft/tc + conntrack en spawn_blocking —
    // no bloquear workers del runtime. Mismos comandos, mismo orden.
    let ip_c = ip.to_string();
    let mac_c = client_mac.clone();
    let iface_c = iface.to_string();
    let prio = format!("{}", 100 + (down_min - 1000) / 2);
    let down_cid = format!("1:{down_min}");
    let up_cid = format!("1:{up_min}");
    let ifb_c = format!("ifb_{}", iface);
    let rad_srv_c = rad_srv.to_string();
    let rad_sec_c = rad_sec.to_string();
    let username_c = username.clone();
    let session_id_c = session_id.clone();
    tokio::task::spawn_blocking(move || {
        let _ = Command::new("nft")
            .args(["delete", "element", "inet", "hotspot", "hotspot_auth", "{", &ip_c, ".", &mac_c, "}"])
            .output();
        let _ = Command::new("tc")
            .args(["filter", "del", "dev", &iface_c, "parent", "1:0", "protocol", "ip", "prio", &prio, "u32", "match", "ip", "dst", &ip_c, "flowid", &down_cid])
            .output();
        // FIX (2026-08-04): el filtro UP se crea en ifb_{iface} (apply_qos
        // L2094-2097), NO en iface — ANTES quedaba huerfano apuntando a una
        // clase ya eliminada.
        let _ = Command::new("tc")
            .args(["filter", "del", "dev", &ifb_c, "parent", "1:0", "protocol", "ip", "prio", &prio, "u32", "match", "ip", "src", &ip_c, "flowid", &up_cid])
            .output();
        let _ = Command::new("tc")
            .args(["class", "del", "dev", &iface_c, "classid", &down_cid])
            .output();
        let _ = Command::new("tc")
            .args(["class", "del", "dev", &ifb_c, "classid", &up_cid])
            .output();
        crate::handlers::helpers::conntrack_flush_sync(&ip_c);
    }).await.ok();
    send_accounting(&rad_srv_c, &rad_sec_c, &username_c, ip, 2, &session_id_c, session_time, stop_rx, stop_tx, terminate_cause);
    zlog!("[DISCONNECT] {} ({}) session_time={}s rx={} tx={}",
        username, ip, session_time, stop_rx, stop_tx);
}

/// POST /hotspot/portal/disconnect — API de desconexion (PORTAL :80)
/// SOLO accesible desde orígenes admin (WG/LAN/localhost): los clientes
/// hotspot (eth3) no pueden desconectar a otros (BUG-B/C).
pub async fn portal_disconnect(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    axum::extract::Form(form): axum::extract::Form<HashMap<String, String>>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let peer_ip = peer.ip().to_string();
    let admin_ok = peer_ip == "127.0.0.1"
        || peer_ip.starts_with("10.7.0.")
        || peer_ip.starts_with("192.168.2.")
        || peer_ip.starts_with("192.168.3.");
    if !admin_ok {
        return Ok(Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("Forbidden")).unwrap());
    }
    let cfg = get_hs_config();
    let rad_srv = cfg.radius.clone();
    let rad_sec = cfg.radius_secret.clone();
    let iface = cfg.iface.clone();

    let username = form.get("username").cloned().unwrap_or_default();
    let session_id = form.get("session_id").cloned().unwrap_or_default();

    let target_ip = {
        let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
        let store_map = store.as_ref();
        store_map.and_then(|map| {
            if !username.is_empty() {
                map.iter().find(|(_, s)| s.username == username).map(|(k, _)| k.clone())
            } else if !session_id.is_empty() {
                // BUG-C: session_id NO es la IP — buscar la sesion cuyo id coincida
                map.iter().find(|(_, s)| s.session_id == session_id).map(|(k, _)| k.clone())
            } else {
                None
            }
        })
    };

    if let Some(ip) = target_ip {
        session_disconnect_internal(&ip, &rad_srv, &rad_sec, &iface, 6).await; // Admin-Reset
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .header("set-cookie", "hs_session=; Path=/; Max-Age=0; HttpOnly")
        .body(Body::from(r#"<!DOCTYPE html><html><body><h2>✅ Desconectado</h2><p>Puede cerrar esta ventana</p></body></html>"#))
        .unwrap())
}

/// POST /hotspot/portal/disconnect — variante ADMIN (:8081)
/// Sin ConnectInfo (el router admin no provee connect info). Protegido
/// por el firewall input FIX-9 (solo WG/LAN llegan al 8081).
pub async fn portal_disconnect_admin(
    axum::extract::Form(form): axum::extract::Form<HashMap<String, String>>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let cfg = get_hs_config();
    let rad_srv = cfg.radius.clone();
    let rad_sec = cfg.radius_secret.clone();
    let iface = cfg.iface.clone();

    let username = form.get("username").cloned().unwrap_or_default();
    let session_id = form.get("session_id").cloned().unwrap_or_default();

    let target_ip = {
        let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
        let store_map = store.as_ref();
        store_map.and_then(|map| {
            if !username.is_empty() {
                map.iter().find(|(_, s)| s.username == username).map(|(k, _)| k.clone())
            } else if !session_id.is_empty() {
                // BUG-C: session_id NO es la IP — buscar la sesion cuyo id coincida
                map.iter().find(|(_, s)| s.session_id == session_id).map(|(k, _)| k.clone())
            } else {
                None
            }
        })
    };

    if let Some(ip) = target_ip {
        session_disconnect_internal(&ip, &rad_srv, &rad_sec, &iface, 6).await; // Admin-Reset
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .header("set-cookie", "hs_session=; Path=/; Max-Age=0; HttpOnly")
        .body(Body::from(r#"<!DOCTYPE html><html><body><h2>✅ Desconectado</h2><p>Puede cerrar esta ventana</p></body></html>"#))
        .unwrap())
}

const WG_PATH: &str = "/etc/zpot/walled-garden.json";

pub fn load_wg() -> Vec<serde_json::Value> {
    std::fs::read_to_string(WG_PATH)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_wg(entries: &[serde_json::Value]) {
    if let Some(parent) = std::path::Path::new(WG_PATH).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = std::fs::write(WG_PATH, &json);
    }
}

/// Borra reglas nft previas marcadas con un comment (evita reglas huerfanas
/// al re-aplicar walled-garden/ip-bindings — FIX 2026-08-02)
fn cleanup_nft_by_comment(chain: &str, comment: &str) {
    if let Ok(out) = Command::new("nft").args(["-a", "list", "chain", "inet", "hotspot", chain]).output() {
        let s = String::from_utf8_lossy(&out.stdout);
        for line in s.lines() {
            if line.contains(comment) {
                // Formato: "... comment \"zpot-wg\" # handle N"
                for part in line.split('#') {
                    if let Some(h) = part.trim().strip_prefix("handle ") {
                        if let Ok(handle) = h.parse::<u64>() {
                            let _ = Command::new("nft")
                                .args(["delete", "rule", "inet", "hotspot", chain, "handle", &handle.to_string()])
                                .output();
                        }
                    }
                }
            }
        }
    }
}

pub fn apply_wg_rules(entries: &[serde_json::Value]) {
    // Limpiar reglas previas (todas con comment zpot-wg) y re-insertar actuales
    // FIX (2026-08-08): usar el iface del config (antes eth3 hardcodeado) +
    // validar IPv4 antes de inyectar en nft.
    let hs_iface = get_hs_config().iface;
    let iface = if hs_iface.is_empty() { "eth4".to_string() } else { hs_iface };
    cleanup_nft_by_comment("forward", "zpot-wg");
    for entry in entries {
        if let Some(ip) = entry.get("ip").and_then(|v| v.as_str()) {
            if !ip.is_empty() && ip != "—" && ip.parse::<std::net::Ipv4Addr>().is_ok() {
                let _ = Command::new("nft")
                    .args(["insert", "rule", "inet", "hotspot", "forward",
                        "iif", &iface, "ip", "daddr", ip, "accept", "comment", "zpot-wg"])
                    .output();
            }
        }
    }
}

pub async fn walled_garden_list() -> Json<Vec<serde_json::Value>> {
    Json(load_wg())
}

pub async fn walled_garden_add(Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut entries = load_wg();
    let mut entry = body;
    // FIX (2026-08-08): si vienen SOLO con domain (ip='—'), resolver dominio a
    // IP con getent hosts — antes se guardaba '—' y apply_wg_rules NO aplicaba
    // ninguna regla nft (el walled garden "no funcionaba").
    let ip = entry.get("ip").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let domain = entry.get("domain").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if (ip.is_empty() || ip == "—") && !domain.is_empty() {
        // getent hosts devuelve IPv6 primero — usar ahostsv4 (solo IPv4)
        let first_ip = crate::handlers::helpers::resolve_ipv4(&domain).await.unwrap_or_default();
        if first_ip.parse::<std::net::Ipv4Addr>().is_err() {
            return Err((StatusCode::BAD_REQUEST, format!(
                "no se pudo resolver '{}' a una IP (getent ahostsv4). Pon la IP manualmente.", domain)));
        }
        entry["ip"] = serde_json::Value::String(first_ip.clone());
        zlog!("[WG] {} resuelto a {} (walled garden)", domain, first_ip);
    }
    let ip_final = entry.get("ip").and_then(|v| v.as_str()).unwrap_or("");
    if ip_final.is_empty() || ip_final == "—" {
        return Err((StatusCode::BAD_REQUEST, "se requiere ip (o domain resoluble)".into()));
    }
    if ip_final.parse::<std::net::Ipv4Addr>().is_err() {
        return Err((StatusCode::BAD_REQUEST, format!("ip invalida: {}", ip_final)));
    }
    entries.push(entry);
    save_wg(&entries);
    // FIX-4 (BUG-17): apply_wg_rules (nft loop) en spawn_blocking.
    let entries_c = entries.clone();
    tokio::task::spawn_blocking(move || apply_wg_rules(&entries_c)).await.ok();
    Ok(Json(serde_json::json!({"status": "ok"})))
}

pub async fn walled_garden_delete(Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let ip = body.get("ip").and_then(|v| v.as_str()).unwrap_or("");
    let mut entries = load_wg();
    entries.retain(|e| e.get("ip").and_then(|v| v.as_str()).unwrap_or("") != ip);
    save_wg(&entries);
    // Re-aplicar todas (quitando la eliminada)
    // FIX-4 (BUG-17): apply_wg_rules (nft loop) en spawn_blocking.
    let entries_c = entries.clone();
    tokio::task::spawn_blocking(move || apply_wg_rules(&entries_c)).await.ok();
    Ok(Json(serde_json::json!({"status": "ok"})))
}


fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_login(html: String, error: &str, error_msg: &str) -> String {
    let link_login_only = "/hotspot/portal/auth";
    let link_login = "/hotspot/portal";
    let link_orig = "http://192.168.10.1/";
    let link_orig_esc = "http%3A%2F%2F192.168.10.1%2F";

    let mut h = html;
    // BUG-1 (2026-08-04): el bloque $(if error)alert$(endif) se reemplazaba
    // por partes ($(if error) y $(endif) por separado) dejando "alert" en el
    // class SIEMPRE -> login pintaba "🛜 Inicia sesión" en rojo sin error.
    // Fix: reemplazar el bloque COMPLETO primero (antes de los condicionales).
    h = h.replace("$(if error)alert$(endif)", if error.is_empty() { "" } else { "alert" });
    h = h.replace("$(link-login-only)", link_login_only);
    h = h.replace("$(link-login)", link_login);
    h = h.replace("$(link-orig)", link_orig);
    h = h.replace("$(link-orig-esc)", link_orig_esc);
    // BUG-2 (2026-08-04): escapar HTML del error — Reply-Message de RADIUS o
    // ?error= podia inyectar <script> sin escapar.
    h = h.replace("$(error)", &escape_html(error_msg));
    h = h.replace("$(if error == \"\")", "");
    h = h.replace("$(endif)", "");
    // Si hay error, ocultar el texto "🛜 Inicia sesión" y remover $(if error)
    if !error.is_empty() {
        h = h.replace("🛜 Inicia sesión", "");
        h = h.replace("$(if error)", "");
    } else {
        // Si no hay error, limpiar el bloque $(if error)
        h = h.replace("$(if error)", "");
        h = h.replace("⚠️ ", "");
    }
    h
}

fn render_alogin(html: String) -> String {
    let link_status = "http://192.168.10.1/";
    html.replace("$(link-status)", link_status)
}

fn fallback_login_page() -> String {
    r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Hotspot Login</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;background:#0f1923;color:#eee;display:flex;align-items:center;justify-content:center;min-height:100vh}
.login-box{background:#1a2332;border:1px solid #2a3a4a;border-radius:12px;padding:40px;width:360px;text-align:center}
.logo{font-size:48px;margin-bottom:16px}
h2{color:#fff;margin-bottom:24px;font-weight:500}
input{width:100%;padding:12px 16px;margin:8px 0;background:#0f1923;border:1px solid #2a3a4a;border-radius:6px;color:#fff;font-size:14px}
input:focus{outline:none;border-color:#e94560}
button{width:100%;padding:12px;background:#e94560;color:#fff;border:none;border-radius:6px;font-size:15px;cursor:pointer;margin-top:16px}
button:hover{background:#d63850}
.err{color:#e94560;margin-top:12px;font-size:13px}
</style></head><body>
<div class="login-box">
<div class="logo">🌐</div>
<h2>Hotspot Login</h2>
<form method="POST" action="/hotspot/portal/auth">
<input name="username" placeholder="Usuario" autocomplete="username" required>
<input name="password" type="password" placeholder="Contraseña" autocomplete="current-password" required>
<button type="submit">Ingresar</button>
</form>
</div></body></html>"#.into()
}

/// Reconstruye sesiones desde disco (los interim los cubre el task GLOBAL).
pub fn restore_and_spawn_interims() -> usize {
    let restored = restore_sessions_from_disk();
    restored.len()
}

/// Devuelve la config actual del hotspot (usada desde main.rs)
pub fn get_hs_config_pub() -> HotspotServer {
    get_hs_config()
}

// ─── RADIUS Access-Request (UDP) ───

struct RadiusResult {
    success: bool,
    rejected: bool,         // true solo si RADIUS respondio con Access-Reject (code 3)
    // FIX (2026-08-04): reachable=false = el server NO respondio (timeout,
    // origen invalido, authenticator invalido). El login debe mostrar
    // \"servidor no disponible\" y NO contar el intento como fallo de
    // password en el anti-brute-force.
    reachable: bool,
    speed_up: String,
    speed_down: String,
    up_ceil_str: String,    // primer valor del segundo par "2M/5M" → ceil UP=2M
    down_ceil_str: String,  // segundo valor del segundo par "2M/5M" → ceil DOWN=5M
    idle_timeout: u32,      // Idle-Timeout (attr 28, RFC 2865 §5.28), 0=no enviado
    reply_message: String,  // Reply-Message (attr 18) desde Access-Reject
}

async fn radius_auth(server: &str, secret: &str, username: &str, password: &str) -> RadiusResult {
    use std::time::Duration;
    use md5::{Md5, Digest};

    let mut buf = Vec::new();
    let id: u8 = rand::random();
    let authenticator: [u8; 16] = rand::random();

    buf.push(1); buf.push(id);
    buf.extend_from_slice(&[0u8; 2]);
    buf.extend_from_slice(&authenticator);

    let uname = username.trim().to_uppercase();
    let uname = uname.as_bytes();
    // FIX (2026-08-04): User-Name > 250 bytes no cabe en el attr (len u8) —
    // rechazar antes de construir un paquete malformado.
    if uname.len() > 250 { return RadiusResult::reject(); }
    buf.push(1); buf.push((uname.len() + 2) as u8); buf.extend_from_slice(uname);

    let enc_pw = encode_password(password, secret, &authenticator);
    buf.push(2); buf.push((enc_pw.len() + 2) as u8); buf.extend_from_slice(&enc_pw);

    // FIX-2 (2026-08-04): NAS-IP-Address desde cfg.gw (antes hardcoded 192.168.10.1)
    let gw = get_hs_config().gw;
    let nas_ip: Vec<u8> = gw.split('.').filter_map(|s| s.parse().ok()).collect();
    if nas_ip.len() == 4 {
        buf.push(4); buf.push(6); buf.extend_from_slice(&[nas_ip[0], nas_ip[1], nas_ip[2], nas_ip[3]]);
    }
    buf.push(6); buf.push(6); buf.extend_from_slice(&(2u32).to_be_bytes());
    buf.push(61); buf.push(6); buf.extend_from_slice(&(15u32).to_be_bytes());
    let nid = b"Zpot-Hotspot";
    buf.push(32); buf.push((nid.len()+2) as u8); buf.extend_from_slice(nid);

    let len = buf.len() as u16;
    buf[2..4].copy_from_slice(&len.to_be_bytes());

    // FIX-1 (2026-08-04): respetar el puerto del config (antes SIEMPRE :1812)
    let (rad_addr, rad_port) = split_host_port(server, 1812);
    let Ok(sock) = tokio::net::UdpSocket::bind("0.0.0.0:0").await else { return RadiusResult::reject(); };
    let Ok(_) = sock.send_to(&buf, format!("{rad_addr}:{rad_port}")).await else { return RadiusResult::reject(); };

    let mut resp = [0u8; 4096];
    // FIX-4 (2026-08-04): recv_from para verificar el origen del datagrama
    // (antes recv aceptaba cualquier paquete como respuesta valida).
    // FIX (2026-08-04): reintento — 1 paquete UDP perdido no debe rechazar
    // a un cliente valido. Solo se reintenta si hubo TIMEOUT (sin respuesta);
    // un Access-Reject explicito se devuelve de inmediato.
    let mut got = None;
    for attempt in 0..2 {
        let Ok(_) = sock.send_to(&buf, format!("{rad_addr}:{rad_port}")).await else { return RadiusResult::reject(); };
        // FIX (2026-08-08): timeout 3s → 6s — igual que radius_timeout del
        // PPP. Con reauth espaciado (1 sesion/ciclo) el server ya no encola;
        // 3s era corto para la consulta SQL authorize → el reenvio del NAS
        // generaba "duplicate packet" en FreeRADIUS.
        match tokio::time::timeout(Duration::from_secs(6), sock.recv_from(&mut resp)).await {
            Ok(Ok((n, src))) => { got = Some((n, src)); break; }
            _ => {
                if attempt == 0 {
                    zlog!("[RADIUS] timeout intento 1 para {} — reintentando", username);
                } else {
                    zlog!("[RADIUS] timeout final para {} (2 intentos)", username);
                }
            }
        }
    }
    let Some((n, src)) = got else { return RadiusResult::reject(); };
    // Solo aceptar respuestas del servidor RADIUS configurado
    if src.ip().to_string() != rad_addr {
        zlog!("[RADIUS] respuesta de origen {} != servidor {} — descartando", src.ip(), rad_addr);
        return RadiusResult::reject();
    }
    if n < 20 { return RadiusResult::reject(); }
    // FIX-4: validar Response Authenticator:
    // MD5(Code + ID + Length + RequestAuthenticator + Attributes + Secret)
    let mut hasher = Md5::new();
    hasher.update(&resp[0..4]);      // Code, ID, Length
    hasher.update(&authenticator);   // Request Authenticator
    hasher.update(&resp[20..n]);     // Attributes
    hasher.update(secret.as_bytes());
    let hash = hasher.finalize();
    if hash.as_slice() != &resp[4..20] {
        zlog!("[RADIUS] Response Authenticator INVALIDO — descartando (spoof?)");
        return RadiusResult::reject();
    }
    if resp[0] == 2 {
        // Access-Accept
        parse_radius_attrs(&resp[20..n], secret)
    } else if resp[0] == 3 {
        // Access-Reject — parsear attrs igual para extraer Reply-Message (attr 18)
        let mut r = parse_radius_attrs(&resp[20..n], secret);
        r.success = false; // Forzar reject aunque parse_radius_attrs inicialice success=true
        r.rejected = true; // RADIUS respondio explicitamente Access-Reject
        r
    } else {
        // Respuesta del server con code desconocido (no 2/3): el server
        // respondio pero no fue util — NO es un timeout.
        let mut r = RadiusResult::reject();
        r.reachable = true;
        r
    }
}

fn parse_radius_attrs(data: &[u8], _secret: &str) -> RadiusResult {
    // FIX (2026-08-04): quien llama esto ya recibio una respuesta VALIDA del
    // server (Access-Accept/Reject con authenticator OK) -> reachable=true.
    let mut r = RadiusResult { success: true, rejected: false, reachable: true, speed_up: String::new(), speed_down: String::new(), up_ceil_str: String::new(), down_ceil_str: String::new(), idle_timeout: 0, reply_message: String::new() };
    let mut i = 0;
    while i + 1 < data.len() {
        let t = data[i];
        let l = data[i+1] as usize;
        if l < 2 || i + l > data.len() { break; }
        let v = &data[i+2..i+l];
        match t {
            18 => {
                // Reply-Message (RFC 2865 §5.18)
                if let Ok(msg) = std::str::from_utf8(v) {
                    r.reply_message = msg.to_string();
                    zlog!("[RADIUS-ATTR] type=18 (Reply-Message) value={}", msg);
                }
            }
            28 => {
                // Idle-Timeout (RFC 2865 §5.28) — segundos de inactividad
                if v.len() >= 4 {
                    r.idle_timeout = u32::from_be_bytes([v[0], v[1], v[2], v[3]]);
                    zlog!("[RADIUS-ATTR] type=28 (Idle-Timeout) value={}", r.idle_timeout);
                }
            }
            26 => {
                if v.len() >= 8 {
                    let oui = u32::from_be_bytes([v[0], v[1], v[2], v[3]]);
                    if oui == 14988 {
                        let vt = v[4];
                        let vl = v[5] as usize;
                        if vl >= 2 && vl <= v.len() - 4 {
                            if vt == 8 || vt == 53 {
                                let raw = std::str::from_utf8(&v[6..4+vl]).unwrap_or("");
                                zlog!("[RADIUS-VSA] raw={raw:?} oui={oui} vt={vt} vl={vl}");
                                // Formato Zpot rate-limit: "rate_up/rate_down ceil_up/ceil_down"
                                // Ej: "1M/4M 2M/5M" → rate=1M/4M (UP=1M, DOWN=4M), ceil=2M/5M (UP=2M, DOWN=5M)
                                // Fallback sin '/': "1M 2M" → rate UP=1M, rate DOWN=2M, ceil=rate
                                let tokens: Vec<&str> = raw.split_whitespace().collect();
                                if !tokens.is_empty() {
                                    if tokens[0].contains('/') {
                                        let rate_parts: Vec<&str> = tokens[0].split('/').collect();
                                        r.speed_up = rate_parts[0].to_string();
                                        if rate_parts.len() >= 2 {
                                            r.speed_down = rate_parts[1].to_string();
                                        }
                                        if tokens.len() >= 2 {
                                            let ceil_parts: Vec<&str> = tokens[1].split('/').collect();
                                            r.up_ceil_str = ceil_parts[0].to_string();
                                            if ceil_parts.len() >= 2 {
                                                r.down_ceil_str = ceil_parts[1].to_string();
                                            }
                                        }
                                    } else {
                                        // Formato simple "up down" sin ceil
                                        r.speed_up = tokens[0].to_string();
                                        if tokens.len() >= 2 {
                                            r.speed_down = tokens[1].to_string();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                // Debug: log ALL attribute types for diagnosis
                if t == 1 {
                    if let Ok(val) = std::str::from_utf8(v) {
                        zlog!("[RADIUS-ATTR] type=1 (User-Name) value={}", val);
                    }
                } else if t == 2 {
                    zlog!("[RADIUS-ATTR] type=2 (User-Password) len={}", v.len());
                } else if t == 4 {
                    if v.len() >= 4 {
                        let ip = format!("{}.{}.{}.{}", v[0], v[1], v[2], v[3]);
                        zlog!("[RADIUS-ATTR] type=4 (NAS-IP-Address) value={}", ip);
                    }
                } else if t == 6 {
                    if v.len() >= 4 {
                        let val = u32::from_be_bytes([v[0], v[1], v[2], v[3]]);
                        zlog!("[RADIUS-ATTR] type=6 (Service-Type) value={}", val);
                    }
                } else if t == 7 {
                    if v.len() >= 4 {
                        let val = u32::from_be_bytes([v[0], v[1], v[2], v[3]]);
                        zlog!("[RADIUS-ATTR] type=7 (Framed-Protocol) value={}", val);
                    }
                } else if t == 8 {
                    if v.len() >= 4 {
                        let ip = format!("{}.{}.{}.{}", v[0], v[1], v[2], v[3]);
                        zlog!("[RADIUS-ATTR] type=8 (Framed-IP-Address) value={}", ip);
                    }
                } else if t == 9 {
                    let val = format!("{}.{}.{}.{}", v[0], v[1], v[2], v[3]);
                    zlog!("[RADIUS-ATTR] type=9 (Framed-IP-Netmask) value={}", val);
                } else if t == 12 {
                    let val = String::from_utf8_lossy(v);
                    zlog!("[RADIUS-ATTR] type=12 (Framed-MTU) value={}", val);
                } else if t == 25 {
                    if let Ok(val) = std::str::from_utf8(v) {
                        zlog!("[RADIUS-ATTR] type=25 (Class) value={}", val);
                    }
                } else if t == 29 {
                    if v.len() >= 4 {
                        let val = u32::from_be_bytes([v[0], v[1], v[2], v[3]]);
                        zlog!("[RADIUS-ATTR] type=29 (Termination-Action) value={}", val);
                    }
                } else if t == 31 {
                    if let Ok(val) = std::str::from_utf8(v) {
                        zlog!("[RADIUS-ATTR] type=31 (Calling-Station-Id) value={}", val);
                    }
                } else if t == 32 {
                    if let Ok(val) = std::str::from_utf8(v) {
                        zlog!("[RADIUS-ATTR] type=32 (NAS-Identifier) value={}", val);
                    }
                } else if t == 61 {
                    if v.len() >= 4 {
                        let val = u32::from_be_bytes([v[0], v[1], v[2], v[3]]);
                        zlog!("[RADIUS-ATTR] type=61 (NAS-Port-Type) value={}", val);
                    }
                } else {
                    zlog!("[RADIUS-ATTR] type={} len={} hex={:02x?}", t, v.len(), &v[..v.len().min(32)]);
                }
            }
        }
        i += l;
    }
    r
}

impl RadiusResult {
    fn reject() -> Self {
        RadiusResult { success: false, rejected: false, reachable: false, speed_up: String::new(), speed_down: String::new(), up_ceil_str: String::new(), down_ceil_str: String::new(), idle_timeout: 0, reply_message: String::new() }
    }
}

fn to_kbps(s: &str) -> u64 {
    let s = s.trim().to_uppercase();
    if s.ends_with("M") {
        let n: f64 = s.trim_end_matches("M").parse().unwrap_or(0.0);
        (n * 1000.0) as u64
    } else if s.ends_with("K") {
        let n: f64 = s.trim_end_matches("K").parse().unwrap_or(0.0);
        n as u64
    } else if s.ends_with("B") {
        let s2 = s.trim_end_matches("B");
        if s2.ends_with("M") { let n: f64 = s2.trim_end_matches("M").parse().unwrap_or(0.0); (n * 1000.0 * 8.0) as u64 }
        else if s2.ends_with("K") { let n: f64 = s2.trim_end_matches("K").parse().unwrap_or(0.0); (n * 8.0) as u64 }
        else { 0 }
    } else {
        s.parse().unwrap_or(0)
    }
}

fn ip_to_minors(ip: &str) -> (u16, u16) {
    let last: u16 = ip.rsplit('.').next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    (1000 + last * 2, 1000 + last * 2 + 1)
}

/// Lee contadores de trafico desde TC class stats
/// FIX-3a (BUG-1h): tc -s class show en spawn_blocking — no bloquear workers.
/// Misma logica de parseo, solo cambia el hilo de ejecucion.
async fn read_tc_bytes(iface: &str, minor: u16) -> u64 {
    let iface = iface.to_string();
    tokio::task::spawn_blocking(move || {
        let out = std::process::Command::new("tc")
            .args(["-s", "class", "show", "dev", &iface, "classid", &format!("1:{minor}")])
            .output().ok();
        if let Some(o) = out {
            let s = String::from_utf8_lossy(&o.stdout);
            // Busca "Sent NNNNN bytes" — el primer token u64 DESPUES de "Sent"
            let words: Vec<&str> = s.split_whitespace().collect();
            for i in 0..words.len().saturating_sub(1) {
                if words[i] == "Sent" {
                    if let Ok(n) = words[i+1].parse::<u64>() {
                        return n;
                    }
                }
            }
        }
        0
    }).await.unwrap_or(0)
}

/// Ejecuta tc y loguea stderr si falla. Retorna true si exit 0.
fn tc_run(args: &[&str]) -> bool {
    let out = Command::new("tc").args(args).output();
    match out {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            let err_trim = err.trim();
            if !err_trim.is_empty() {
                zlog!("[TC-ERR] tc {} -> {}", args.join(" "), err_trim);
            }
            false
        }
        Err(e) => {
            zlog!("[TC-ERR] tc {} -> {}", args.join(" "), e);
            false
        }
    }
}

/// Serializa TODAS las operaciones tc de QoS. El kernel serializa cada comando tc
/// individual (lock RTNL) pero NO la secuencia multi-comando: dos logins en paralelo
/// intercalaban del/class/filter y se pisaban filtros entre clientes (carrera
/// detectada 2026-07-31). Con este lock la secuencia completa es atomica.
static QOS_LOCK: Mutex<()> = Mutex::new(());

fn apply_qos(client_ip: &str, iface: &str, speed_up: &str, speed_down: &str, up_ceil_str: &str, down_ceil_str: &str) {
    // FIX (2026-08-04): sin rates configurados (config vacio + RADIUS sin
    // VSA) crear clase con el rate del padre (100Mbit = contador sin limite
    // real). ANTES: early-return -> no habia clases tc -> read_tc_bytes=0
    // -> el interim expulsaba por idle a clientes ACTIVOS cada 10 min.
    let (up_kbps, down_kbps) = if speed_up.is_empty() || speed_down.is_empty() {
        (100000, 100000)
    } else {
        (to_kbps(speed_up), to_kbps(speed_down))
    };
    // Si no hay ceil explicito, usar rate (comportamiento anterior)
    let up_ceil_kbps = if up_ceil_str.is_empty() { up_kbps } else { to_kbps(up_ceil_str) };
    let down_ceil_kbps = if down_ceil_str.is_empty() { down_kbps } else { to_kbps(down_ceil_str) };
    if up_kbps == 0 && down_kbps == 0 { return; }
    // FIX (2026-08-04): HTB exige ceil >= rate — ANTES el ceil se clampaba a
    // 100000 pero el rate NO: con un plan >100M el `tc class change/add`
    // fallaba, no habia clase, read_tc_bytes=0 y el interim expulsaba al
    // cliente ACTIVO por idle cada idle_timeout. Clampar ambos.
    let up_kbps = std::cmp::min(up_kbps, 100000);
    let down_kbps = std::cmp::min(down_kbps, 100000);
    let up_ceil_kbps = std::cmp::min(up_ceil_kbps, 100000);
    let down_ceil_kbps = std::cmp::min(down_ceil_kbps, 100000);

    zlog!("[QOS] {client_ip}@{iface}: up={speed_up}({up_kbps}kbps/ceil={up_ceil_kbps}kbps) down={speed_down}({down_kbps}kbps/ceil={down_ceil_kbps}kbps)");

    // SERIALIZAR toda la secuencia tc (el guard se mantiene hasta el final de la fn).
    // tc filter replace es idempotente (nunca duplica) → el del agresivo ya no existe:
    // borraba filtros de OTROS clientes cuando el match no coincidia (causa de que el
    // estado final de tc quedara en caos).
    let _qos_guard = QOS_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let (down_min, up_min) = ip_to_minors(client_ip);
    let down_cid = format!("1:{down_min}");
    let up_cid = format!("1:{up_min}");
    let ifb_name = format!("ifb_{iface}");
    // PRIO UNICO POR CLIENTE (100+last_octet): cada cliente su propia cadena
    // de filtros. Con prio 1 compartido el del operaba sobre el primer filtro
    // del bucket, que podia ser de OTRO cliente (bug 2026-08-02: solo el
    // ultimo auth quedaba clasificado). Con prio unico el del solo ve los
    // filtros de ESTA IP.
    let prio = format!("{}", 100 + (down_min - 1000) / 2);

    // Garantizar arbol HTB root con default class (evita estrangular control PPP)
    let out = Command::new("tc").args(["class", "show", "dev", iface, "classid", "1:1"]).output();
    let has_class1 = out.map(|o| String::from_utf8_lossy(&o.stdout).contains("htb")).unwrap_or(false);
    if !has_class1 {
        tc_run(&["qdisc", "add", "dev", iface, "root", "handle", "1:", "htb", "default", "2"]);
        tc_run(&["class", "add", "dev", iface, "parent", "1:", "classid", "1:1", "htb", "rate", "100mbit"]);
        tc_run(&["class", "add", "dev", iface, "parent", "1:1", "classid", "1:2", "htb", "rate", "100mbit", "ceil", "100mbit", "burst", "64k", "cburst", "64k"]);
    }

    // Preparar ifb_{iface} para UP (crear si no existe, levantar)
    let _ = Command::new("ip").args(["link", "add", &ifb_name, "type", "ifb"]).output();
    let _ = Command::new("ip").args(["link", "set", &ifb_name, "up"]).output();
    let _ = Command::new("sysctl").args(["-w", &format!("net.ipv4.conf.{}.rp_filter=2", &ifb_name)]).output();

    // Garantizar arbol HTB root en ifb_{iface} para UP
    let out2 = Command::new("tc").args(["class", "show", "dev", &ifb_name, "classid", "1:1"]).output();
    let has_ifb_htb = out2.map(|o| String::from_utf8_lossy(&o.stdout).contains("htb")).unwrap_or(false);
    if !has_ifb_htb {
        tc_run(&["qdisc", "add", "dev", &ifb_name, "root", "handle", "1:", "htb", "default", "2"]);
        tc_run(&["class", "add", "dev", &ifb_name, "parent", "1:", "classid", "1:1", "htb", "rate", "100mbit"]);
        tc_run(&["class", "add", "dev", &ifb_name, "parent", "1:1", "classid", "1:2", "htb", "rate", "100mbit", "ceil", "100mbit", "burst", "64k", "cburst", "64k"]);

        // Ingress redirect en {iface} hacia ifb_{iface} (solo 1 vez)
        tc_run(&["qdisc", "add", "dev", iface, "ingress"]);
        tc_run(&["filter", "add", "dev", iface, "parent", "ffff:", "protocol", "all",
            "u32", "match", "u32", "0", "0", "action", "mirred", "egress", "redirect", "dev", &ifb_name]);
    }

    // Burst proporcional: kbps*125 = 1 segundo completo de datos (cap 128KB) —
    // suficiente headroom TCP; el comentario antiguo decia "100ms" pero la
    // formula era 1s (se mantiene la formula, se corrige el comentario).
    let down_burst = format!("{}b", std::cmp::min(std::cmp::max(down_kbps * 125, 16000), 131072));
    let up_burst = format!("{}b", std::cmp::min(std::cmp::max(up_kbps * 125, 16000), 131072));
    let down_ceil = std::cmp::min(down_ceil_kbps, 100000);
    let up_ceil = std::cmp::min(up_ceil_kbps, 100000);

    // Crear o actualizar clase DOWN — `tc class change` modifica tasa en-place
    if down_kbps > 0 {
        let changed = Command::new("tc").args(["class", "change", "dev", iface, "classid", &down_cid, "htb",
            "rate", &format!("{}kbit", down_kbps), "ceil", &format!("{}kbit", down_ceil),
            "burst", &down_burst, "cburst", &down_burst]).output()
            .map(|o| o.status.success()).unwrap_or(false);
        if !changed {
            // add: si falla AQUI es un error real (la clase no existe y no se pudo crear)
            tc_run(&["class", "add", "dev", iface, "parent", "1:1", "classid", &down_cid, "htb",
                "rate", &format!("{}kbit", down_kbps), "ceil", &format!("{}kbit", down_ceil),
                "burst", &down_burst, "cburst", &down_burst]);
        }
        // del exacto + add con prio unico (declarado arriba)
        let _ = Command::new("tc").args(["filter", "del", "dev", iface, "parent", "1:0", "protocol", "ip", "prio", &prio,
            "u32", "match", "ip", "dst", client_ip, "flowid", &down_cid]).output();
        tc_run(&["filter", "add", "dev", iface, "parent", "1:0", "protocol", "ip", "prio", &prio,
            "u32", "match", "ip", "dst", client_ip, "flowid", &down_cid]);
    }
    // Crear o actualizar clase UP en ifb_{iface}
    if up_kbps > 0 {
        let changed = Command::new("tc").args(["class", "change", "dev", &ifb_name, "classid", &up_cid, "htb",
            "rate", &format!("{}kbit", up_kbps), "ceil", &format!("{}kbit", up_ceil),
            "burst", &up_burst, "cburst", &up_burst]).output()
            .map(|o| o.status.success()).unwrap_or(false);
        if !changed {
            // add: si falla AQUI es un error real (la clase no existe y no se pudo crear)
            tc_run(&["class", "add", "dev", &ifb_name, "parent", "1:1", "classid", &up_cid, "htb",
                "rate", &format!("{}kbit", up_kbps), "ceil", &format!("{}kbit", up_ceil),
                "burst", &up_burst, "cburst", &up_burst]);
        }
        // del exacto + add con prio unico (ver comentario DOWN)
        let _ = Command::new("tc").args(["filter", "del", "dev", &ifb_name, "parent", "1:0", "protocol", "ip", "prio", &prio,
            "u32", "match", "ip", "src", client_ip, "flowid", &up_cid]).output();
        tc_run(&["filter", "add", "dev", &ifb_name, "parent", "1:0", "protocol", "ip", "prio", &prio,
            "u32", "match", "ip", "src", client_ip, "flowid", &up_cid]);
    }
}

fn send_accounting(
    server: &str, secret: &str, username: &str,
    client_ip: &str, status: u32,
    session_id: &str, session_time: u64,
    rx_bytes: u64, tx_bytes: u64,
    terminate_cause: u32,
) {
    // Acct-Status-Type values per RFC 2866 §5.7
    const ACCT_START: u32  = 1;  // Start
    const ACCT_STOP: u32   = 2;  // Stop
    const ACCT_INTERIM: u32 = 3; // Interim-Update
    // Acct-Terminate-Cause values per RFC 2866 §5.13
    const TERM_USER_REQUEST: u32  = 1;  // User-Request (logout)
    const TERM_IDLE_TIMEOUT: u32  = 4;  // Idle-Timeout
    const TERM_SESSION_TO: u32    = 5;  // Session-Timeout
    const TERM_ADMIN_RESET: u32   = 6;  // Admin-Reset

    use std::net::UdpSocket;
    use md5::{Md5, Digest};
    let mut buf = Vec::new();
    let id: u8 = rand::random();

    // Code=4 (Accounting-Request), ID, length placeholder
    buf.push(4);
    buf.push(id);
    buf.extend_from_slice(&[0u8; 2]);      // length placeholder
    buf.extend_from_slice(&[0u8; 16]);     // Authenticator: 16 ZEROS (luego MD5)

    // User-Name (1)
    let uname = username.as_bytes();
    buf.push(1); buf.push((uname.len()+2) as u8); buf.extend_from_slice(uname);

    // NAS-IP-Address (4) — desde gw config de la interfaz hotspot
    let gw = get_hs_config().gw;
    let nas_ip: Vec<u8> = gw.split('.').filter_map(|s| s.parse().ok()).collect();
    // FIX-H5 (BUG hotspot): gw sin 4 octetos validos no debe panic
    if nas_ip.len() == 4 {
        let nas_ip_arr = [nas_ip[0], nas_ip[1], nas_ip[2], nas_ip[3]];
        buf.push(4); buf.push(6); buf.extend_from_slice(&nas_ip_arr);
    }

    // Service-Type (6) = Framed (2)
    buf.push(6); buf.push(6); buf.extend_from_slice(&(2u32).to_be_bytes());

    // NAS-Port-Type (61) = Ethernet (15)
    buf.push(61); buf.push(6); buf.extend_from_slice(&(15u32).to_be_bytes());

    // Acct-Status-Type (40) = Start(1)/Stop(2)/Interim(3)
    buf.push(40); buf.push(6); buf.extend_from_slice(&status.to_be_bytes());

    // Acct-Session-Id (44) — mismo del Start para correlacion
    let sid_b = session_id.as_bytes();
    buf.push(44); buf.push((sid_b.len()+2) as u8); buf.extend_from_slice(sid_b);

    // Framed-IP-Address (8)
    let ip: Vec<u8> = client_ip.split('.').filter_map(|s| s.parse().ok()).collect();
    if ip.len() == 4 {
        buf.push(8); buf.push(6); buf.extend_from_slice(&ip);
    }

    // NAS-Identifier (32)
    let nid = b"Zpot-Hotspot";
    buf.push(32); buf.push((nid.len()+2) as u8); buf.extend_from_slice(nid);

    // Acct-Session-Time (46) — solo para Stop e Interim, NO para Start
    if status != ACCT_START {
        buf.push(46); buf.push(6); buf.extend_from_slice(&(session_time as u32).to_be_bytes());
    }

    // Acct-Input-Octets (42) / Acct-Output-Octets (43) — solo para Stop e Interim
    // 32-bit per RFC 2866 §5.11 (4GB max). FIX-3 (2026-08-04): si >4GB se envia
    // tambien Acct-Input-Gigawords (52) / Acct-Output-Gigawords (53) — parte alta
    // del contador de 64-bit (RFC 2869 §5.4). Sin esto, una sesion >4GB reportaba
    // contadores truncados (facturacion incorrecta).
    if status != ACCT_START {
        // FIX (2026-08-04): octets INPUT/OUTPUT INVERTIDOS (RFC 2866 §5.11):
        // Acct-Input-Octets(42) = lo que el cliente SUBE (tx, clase UP en ifb,
        // src=cliente); Acct-Output-Octets(43) = lo que el cliente BAJA (rx,
        // clase DOWN en iface, dst=cliente). ANTES iban cruzados y los
        // reportes de FreeRADIUS/radacct salian invertidos.
        buf.push(42); buf.push(6); buf.extend_from_slice(&(tx_bytes as u32).to_be_bytes());
        buf.push(43); buf.push(6); buf.extend_from_slice(&(rx_bytes as u32).to_be_bytes());
        if tx_bytes > u32::MAX as u64 {
            buf.push(52); buf.push(6); buf.extend_from_slice(&((tx_bytes >> 32) as u32).to_be_bytes());
        }
        if rx_bytes > u32::MAX as u64 {
            buf.push(53); buf.push(6); buf.extend_from_slice(&((rx_bytes >> 32) as u32).to_be_bytes());
        }
    }

    // Acct-Terminate-Cause (49) — solo para Stop
    if status == ACCT_STOP {
        // RFC 2866 §5.13: 1=User-Request, 4=Idle-Timeout, 5=Session-Timeout, 6=Admin-Reset
        let cause = if terminate_cause > 0 { terminate_cause } else { TERM_USER_REQUEST };
        buf.push(49); buf.push(6); buf.extend_from_slice(&cause.to_be_bytes());
    }

    let len = buf.len() as u16;
    buf[2..4].copy_from_slice(&len.to_be_bytes());

    // MD5 authenticator: RFC 2866 §3 = MD5(Code + ID + Length + 16 zeros + Attributes + Secret)
    let mut hasher = Md5::new();
    hasher.update(&buf);      // packet con authenticator en ceros
    hasher.update(secret.as_bytes());
    let hash = hasher.finalize();
    buf[4..20].copy_from_slice(&hash);

    // BUG-FIX (2026-08-04): el accounting SIEMPRE al puerto 1813 (RFC 2866).
    // El campo radius del config (ej "161.97.67.63:1812") es el puerto de
    // AUTH — respetarlo aqui enviaba Start/Stop/Interim al 1812, FreeRADIUS
    // los descartaba y radacct NUNCA registraba la sesion. Consecuencia:
    // el polling CoA veia "0 sesiones en RADIUS" y expulsaba a todos los
    // clientes cada 30s (MAX/RWX2/F6H8 sin internet). Tomamos SOLO la IP.
    let (rad_addr, _) = split_host_port(server, 1812);
    let rad_port: u16 = 1813;
    let label = match status { 1 => "start", 2 => "stop", 3 => "interim", _ => "?" };
    zlog!("[ACCT] acc-{} sent to {}:{} (len={}, id=0x{:02x}, time={}, rx={}, tx={})",
        label, rad_addr, rad_port, buf.len(), id, session_time, rx_bytes, tx_bytes);
    match UdpSocket::bind("0.0.0.0:0") {
        Ok(sock) => {
            let target = format!("{}:{}", rad_addr, rad_port);
            match sock.send_to(&buf, &target) {
                Ok(n) => zlog!("[ACCT] sent {} bytes", n),
                Err(e) => zlog!("[ACCT] send_to FAILED: {}", e),
            }
        }
        Err(e) => zlog!("[ACCT] UdpSocket::bind FAILED: {}", e),
    }
}

fn encode_password(password: &str, secret: &str, authenticator: &[u8; 16]) -> Vec<u8> {
    use md5::{Md5, Digest};
    let mut pw = password.as_bytes().to_vec();
    while pw.len() % 16 != 0 {
        pw.push(0);
    }

    let mut result = Vec::with_capacity(pw.len());
    let mut prev = authenticator.to_vec();

    for chunk in pw.chunks(16) {
        let mut hasher = Md5::new();
        hasher.update(secret.as_bytes());
        hasher.update(&prev);
        let hash = hasher.finalize();

        let mut enc = Vec::with_capacity(16);
        for (a, b) in chunk.iter().zip(hash.iter()) {
            enc.push(a ^ b);
        }
        result.extend_from_slice(&enc);
        prev = enc;
    }

    result
}

/// Busca la MAC de una IP en la tabla ARP.
/// FIX 2026-08-02: reintenta si el neigh esta FAILED/INCOMPLETE y busca en la
/// tabla completa de eth3 (el neigh puntual puede no estar resuelto aun).
/// FIX-4 (BUG-15): lookups en spawn_blocking — no bloquear workers.
/// Resuelve la MAC de una IP via tabla ARP (spawn_blocking para no bloquear)
pub async fn get_mac_from_arp(ip: &str) -> String {
    let ip_owned = ip.to_string();
    let ip_first = ip_owned.clone();
    let mut mac = tokio::task::spawn_blocking(move || lookup_arp(&ip_first)).await.unwrap_or_default();
    if mac.is_empty() {
        // Fallback 1: buscar en toda la tabla ARP de la iface del hotspot
        // FIX-5 (2026-08-04): antes hardcodeaba "eth3" — si cfg.iface cambia
        // el fallback buscaba en la interfaz equivocada.
        let ip2 = ip_owned.clone();
        let hs_iface = get_hs_config().iface;
        mac = tokio::task::spawn_blocking(move || {
            if let Ok(out) = Command::new("ip").args(["neigh", "show", "dev", &hs_iface]).output() {
                let s = String::from_utf8_lossy(&out.stdout);
                for line in s.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 5 && parts[0] == ip2 {
                        let state = parts.get(5).unwrap_or(&"");
                        if *state != "FAILED" && *state != "INCOMPLETE" && *state != "" {
                            let m = parts[4];
                            if m.len() == 17 && m.contains(':') {
                                return m.to_string();
                            }
                        }
                    }
                }
            }
            String::new()
        }).await.unwrap_or_default();
    }
    if mac.is_empty() {
        // Fallback 2: el ARP se resuelve tras el DHCP / primer paquete — reintento
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let ip3 = ip_owned.clone();
        mac = tokio::task::spawn_blocking(move || lookup_arp(&ip3)).await.unwrap_or_default();
    }
    mac
}

fn lookup_arp(ip: &str) -> String {
    if let Ok(out) = Command::new("ip").args(["neigh", "show", ip]).output() {
        let s = String::from_utf8_lossy(&out.stdout);
        let parts: Vec<&str> = s.split_whitespace().collect();
        // Formato: "192.168.10.218 dev eth3 lladdr 4a:47:c5:08:22:c6 STALE"
        if parts.len() >= 5 {
            let state = parts.get(5).unwrap_or(&"");
            if *state != "FAILED" && *state != "INCOMPLETE" && *state != "" {
                let mac = parts[4];
                if mac.len() == 17 && mac.contains(':') {
                    return mac.to_string();
                }
            }
        }
    }
    String::new()
}

fn add_bypass_nft(client_ip: &str, client_mac: &str) {
    // Guard: sin MAC no se puede agregar el elemento IP.MAC (FIX 2026-08-02)
    if client_mac.is_empty() {
        zlog!("[BYPASS] WARN: MAC vacia para {} — bypass nft NO agregado", client_ip);
        return;
    }
    // Limpiar elementos viejos con misma IP pero diferente MAC
    // para que al reasignarse la IP no quede un bypass huerfano
    if let Ok(out) = Command::new("nft").args(["-j", "list", "set", "inet", "hotspot", "hotspot_auth"]).output() {
        if let Ok(data) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
            if let Some(elems) = data.get("nftables")
                .and_then(|a| a.as_array())
                .and_then(|arr| arr.get(1))
                .and_then(|m| m.get("set"))
                .and_then(|s| s.get("elem"))
                .and_then(|e| e.as_array())
            {
                for e in elems {
                    let concat = e.get("elem")
                        .and_then(|inner| inner.get("val"))
                        .and_then(|v| v.get("concat"))
                        .and_then(|c| c.as_array())
                        .and_then(|arr| {
                            if arr.len() >= 2 {
                                Some((arr[0].as_str().unwrap_or(""), arr[1].as_str().unwrap_or("")))
                            } else { None }
                        });
                    if let Some((ip, mac)) = concat {
                        if ip == client_ip && mac != client_mac {
                            let _ = Command::new("nft")
                                .args(["delete", "element", "inet", "hotspot", "hotspot_auth", "{", ip, ".", mac, "}"])
                                .output();
                            zlog!("[BYPASS-CLEANUP] Elemento stale eliminado: {} . {}", ip, mac);
                        }
                    }
                }
            }
        }
    }
    let _ = Command::new("nft")
        .args(["add", "element", "inet", "hotspot", "hotspot_auth", "{", client_ip, ".", client_mac, "timeout", "24h", "}"])
        .output();
    zlog!("[BYPASS] IP+MAC {} . {} agregada al set hotspot_auth (timeout 24h)", client_ip, client_mac);
}

const IB_PATH: &str = "/etc/zpot/ip-bindings.json";

pub fn load_ib() -> Vec<serde_json::Value> {
    std::fs::read_to_string(IB_PATH)
        .ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}
fn save_ib(entries: &[serde_json::Value]) {
    if let Some(parent) = std::path::Path::new(IB_PATH).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = std::fs::write(IB_PATH, &json);
    }
}
pub fn apply_ib_rules(entries: &[serde_json::Value]) {
    // Limpiar reglas previas (comment zpot-ib) y re-insertar actuales
    cleanup_nft_by_comment("forward", "zpot-ib");
    // FIX (2026-08-12): usar el iface del config (antes "eth3" hardcodeado).
    let ib_iface = get_hs_config().iface;
    let ib_iface = if ib_iface.is_empty() { "eth4".to_string() } else { ib_iface };
    for entry in entries {
        let ip = entry.get("ip").and_then(|v| v.as_str()).unwrap_or("");
        let mac = entry.get("mac").and_then(|v| v.as_str()).unwrap_or("");
        let typ = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if ip.is_empty() || ip == "\u{2014}" { continue; }
        if typ == "bypassed" {
            // FIX 2026-08-02: whitelist IP+MAC — si hay MAC, la regla exige
            // ambas; asi otra maquina que tome la misma IP por DHCP no
            // hereda el bypass (antes solo ip saddr).
            if !mac.is_empty() {
                let _ = Command::new("nft").args(["insert", "rule", "inet", "hotspot", "forward",
                    "iif", &ib_iface, "ip", "saddr", ip, "ether", "saddr", mac, "accept", "comment", "zpot-ib"]).output();
                zlog!("[IB] bypass IP+MAC {} . {} insertado", ip, mac);
            } else {
                let _ = Command::new("nft").args(["insert", "rule", "inet", "hotspot", "forward",
                    "iif", &ib_iface, "ip", "saddr", ip, "accept", "comment", "zpot-ib"]).output();
                zlog!("[IB] bypass IP {} insertado (sin MAC)", ip);
            }
        } else if typ == "blocked" {
            // FIX (2026-08-04): insert (principio de la cadena) — con add el
            // block quedaba al FINAL, DESPUES del @hotspot_auth accept -> un
            // cliente autenticado blockeado seguia navegando (block inerte).
            if !mac.is_empty() {
                let _ = Command::new("nft").args(["insert", "rule", "inet", "hotspot", "forward",
                    "iif", &ib_iface, "ip", "saddr", ip, "ether", "saddr", mac, "drop", "comment", "zpot-ib"]).output();
                zlog!("[IB] block IP+MAC {} . {} insertado", ip, mac);
            } else {
                let _ = Command::new("nft").args(["insert", "rule", "inet", "hotspot", "forward",
                    "iif", &ib_iface, "ip", "saddr", ip, "drop", "comment", "zpot-ib"]).output();
                zlog!("[IB] block IP {} insertado (sin MAC)", ip);
            }
        }
    }
}

// ── Cookies server-side API (MikroTik-style) ─────────────────────────────────

/// GET /api/hotspot/cookies — lista todas las cookies activas
pub async fn cookies_list() -> Json<Vec<serde_json::Value>> {
    let list = get_cookie_entries();
    let result: Vec<serde_json::Value> = list.iter().map(|c| {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let expires_in = if c.expires_at > now { c.expires_at - now } else { 0 };
        serde_json::json!({
            "username": c.username,
            "mac": c.mac,
            "expires_in": expires_in,
            "created_at": c.created_at,
            "expires_at": c.expires_at,
            "remaining": format_remaining(expires_in)
        })
    }).collect();
    Json(result)
}

/// POST /api/hotspot/cookies/delete — elimina una cookie
pub async fn cookies_delete(Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let username = body.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mac = body.get("mac").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if username.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "username requerido".into()));
    }
    let mut cookies = HOTSPOT_COOKIES.lock().unwrap_or_else(|e| e.into_inner());
    let before = cookies.len();
    // Normalizar MAC para comparacion
    let mac_norm = mac.to_lowercase();
    // FIX (2026-08-04): comparacion case-insensitive del username (el store
    // guarda MAYUSCULAS; la UI podia mandar minusculas y borrar 0).
    let uname_upper = username.to_uppercase();
    cookies.retain(|c| !(c.username.eq_ignore_ascii_case(&uname_upper) && c.mac.to_lowercase() == mac_norm));
    let deleted = before - cookies.len();
    drop(cookies);
    if deleted > 0 {
        save_cookies_to_disk();
    }
    zlog!("[HOTSPOT-COOKIE] DELETE: username={} mac={} deleted={}", username, mac, deleted);
    Ok(Json(serde_json::json!({"ok": true, "deleted": deleted})))
}

/// Formatea segundos restantes como texto legible
fn format_remaining(secs: u64) -> String {
    if secs < 60 { return format!("{}s", secs); }
    if secs < 3600 { return format!("{}m {}s", secs / 60, secs % 60); }
    if secs < 86400 { return format!("{}h {}m", secs / 3600, (secs % 3600) / 60); }
    format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
}

pub async fn ip_bindings_list() -> Json<Vec<serde_json::Value>> {
    Json(load_ib())
}
pub async fn ip_bindings_add(Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut entries = load_ib();
    entries.push(body);
    save_ib(&entries);
    // FIX-4 (BUG-16): apply_ib_rules (nft loop) en spawn_blocking.
    let entries_c = entries.clone();
    tokio::task::spawn_blocking(move || apply_ib_rules(&entries_c)).await.ok();
    Ok(Json(serde_json::json!({"status": "ok"})))
}
pub async fn ip_bindings_delete(Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let ip = body.get("ip").and_then(|v| v.as_str()).unwrap_or("");
    let mut entries = load_ib();
    entries.retain(|e| e.get("ip").and_then(|v| v.as_str()).unwrap_or("") != ip);
    save_ib(&entries);
    // FIX-4 (BUG-16): apply_ib_rules (nft loop) en spawn_blocking.
    let entries_c = entries.clone();
    tokio::task::spawn_blocking(move || apply_ib_rules(&entries_c)).await.ok();
    Ok(Json(serde_json::json!({"status": "ok"})))
}

/// GET /api/hotspot/logs — eventos del PORTAL/RADIUS del hotspot:
/// login, accept/reject, BYPASS, accounting, CoA (revisa.md #10).
/// Fuente: /tmp/zpot.log (escrito por la macro de logging de zpot).
pub async fn hotspot_logs() -> Result<Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let output = tokio::process::Command::new("sh")
        .args(["-c", "grep -iE 'AUTH|LOGIN|BYPASS|ACCT|INTERIM|COA|REJECT|FAIL|portal|RADIUS' /tmp/zpot.log 2>/dev/null | tail -n 80"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut logs = Vec::new();
    for line in text.lines().rev().take(80) {
        if line.is_empty() { continue; }
        logs.push(serde_json::json!({"line": line}));
    }
    Ok(Json(logs))
}



// ── RADIUS CoA / Disconnect-Request (RFC 3576/5176) — puerto UDP 3799 ──────
// FIX 2026-08-04 (caso G4RP): el server RADIUS cerro una sesion con
// Lost-Carrier (01:20) pero Zpot NO se entero — el reauth usa Access-Request
// (auth), no el accounting. Zpot seguia enviando interims a una sesion ya
// cerrada en RADIUS y el cliente navegaba SIN que se cuente saldo.
// Solucion: escuchar Disconnect-Request (40) y CoA-Request (43) del server.
// Cuando RADIUS cierra una sesion, manda Disconnect al NAS -> Zpot mata la
// sesion local (nft + tc + accounting Stop + store).
// RFC 5176 codes: Disconnect-Request=40 ACK=41 NAK=42 | CoA-Request=43 ACK=44 NAK=45

/// Arranca el listener UDP 3799 (CoA/Disconnect). Llamado desde main.rs.
/// Solo activo si coa_enabled=true y coa_mode="udp".
pub fn spawn_coa_listener() {
    tokio::spawn(async {
        let cfg0 = get_hs_config();
        if !cfg0.coa_enabled || cfg0.coa_mode != "udp" {
            zlog!("[COA] listener UDP desactivado (coa_enabled={}, mode={})", cfg0.coa_enabled, cfg0.coa_mode);
            return;
        }
        let socket = match tokio::net::UdpSocket::bind("0.0.0.0:3799").await {
            Ok(s) => s,
            Err(e) => {
                zlog!("[COA] bind UDP 3799 FAILED: {}", e);
                return;
            }
        };
        zlog!("[COA] escuchando Disconnect/CoA en UDP 3799");
        let mut buf = [0u8; 4096];
        loop {
            let Ok((n, src)) = socket.recv_from(&mut buf).await else { continue; };
            let cfg = get_hs_config();
            // Solo aceptar paquetes del servidor RADIUS configurado o de la
            // VPN privada (10.7.0.0/24 — el server RADIUS es peer wg 10.7.0.1).
            let (rad_addr, _) = split_host_port(&cfg.radius, 1812);
            let src_ip = src.ip().to_string();
            let is_vpn = src_ip.starts_with("10.7.");
            if src_ip != rad_addr && !is_vpn {
                zlog!("[COA] origen {} != servidor {} — ignorando", src_ip, rad_addr);
                continue;
            }
            if n < 20 { continue; }
            let code = buf[0];
            let id = buf[1];
            // El Request Authenticator del paquete (16 bytes) se usa para
            // construir el Response Authenticator de la respuesta.
            let req_auth: [u8; 16] = match buf[4..20].try_into() {
                Ok(a) => a,
                Err(_) => continue,
            };
            let attrs = &buf[20..n];
            // FIX (2026-08-04): autenticar el paquete CoA — RFC 5176 §3.1:
            // Request Authenticator = MD5(Code+ID+Length+16 zeros+Attrs+Secret).
            // ANTES solo se validaba el origen IP (10.7.0.0/16 = cualquier peer
            // WG podia mandar Disconnect y expulsar a todos los clientes).
            {
                use md5::{Md5, Digest};
                let mut ver = Vec::with_capacity(n + cfg.radius_secret.len());
                ver.push(code);
                ver.push(id);
                ver.extend_from_slice(&(n as u16).to_be_bytes());
                ver.extend_from_slice(&[0u8; 16]);
                ver.extend_from_slice(attrs);
                ver.extend_from_slice(cfg.radius_secret.as_bytes());
                let mut hasher = Md5::new();
                hasher.update(&ver);
                let hash = hasher.finalize();
                if hash.as_slice() != &req_auth[..] {
                    zlog!("[COA] Request Authenticator INVALIDO de {} — ignorando (posible spoof)", src.ip());
                    continue;
                }
            }
            let (username, ip, session_id, mac, cause) = parse_coa_attrs(attrs);
            zlog!("[COA] code={} id={} from {} (user={} ip={} sid={} mac={})",
                code, id, src.ip(), username, ip, session_id, mac);

            let resp: Vec<u8> = match code {
                40 => {
                    // Disconnect-Request: matar la sesion local
                    let target = find_session_ip(&username, &ip, &session_id, &mac);
                    match target {
                        Some(target_ip) => {
                            let cause_final = if cause > 0 { cause } else { 6 }; // Admin-Reset
                            // FIX (2026-08-04): el disconnect (nft/tc/acct-stop —
                            // lento) NO debe bloquear el listener UDP; si llegan
                            // varios CoA en rafaga se perderian. Responder ACK ya.
                            let c_ip = target_ip.clone();
                            let c_rad = cfg.radius.clone();
                            let c_sec = cfg.radius_secret.clone();
                            let c_iface = cfg.iface.clone();
                            let c_user = username.clone();
                            tokio::spawn(async move {
                                session_disconnect_internal(&c_ip, &c_rad, &c_sec, &c_iface, cause_final).await;
                                zlog!("[COA] Disconnect OK: {} ip={} cause={}", c_user, c_ip, cause_final);
                            });
                            build_coa_response(41, id, &req_auth, &cfg.radius_secret, "Session terminated")
                        }
                        None => {
                            zlog!("[COA] Disconnect NAK: sesion no encontrada (user={} ip={} sid={} mac={})",
                                username, ip, session_id, mac);
                            build_coa_response(42, id, &req_auth, &cfg.radius_secret, "Session not found")
                        }
                    }
                }
                43 => {
                    // CoA-Request: re-aplicar QoS si trae VSA rate-limit
                    let rad = parse_radius_attrs(attrs, &cfg.radius_secret);
                    let target = find_session_ip(&username, &ip, &session_id, &mac);
                    match target {
                        Some(target_ip) => {
                            if !rad.speed_up.is_empty() || !rad.speed_down.is_empty() {
                                let c_ip = target_ip.clone();
                                let c_iface = cfg.iface.clone();
                                let c_su = rad.speed_up.clone();
                                let c_sd = rad.speed_down.clone();
                                let c_uc = rad.up_ceil_str.clone();
                                let c_dc = rad.down_ceil_str.clone();
                                tokio::task::spawn_blocking(move || {
                                    apply_qos(&c_ip, &c_iface, &c_su, &c_sd, &c_uc, &c_dc);
                                }).await.ok();
                                // Actualizar store con la nueva QoS
                                let mut store = session_store().lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(s) = store.as_mut().and_then(|m| m.get_mut(&target_ip)) {
                                    s.speed_up = rad.speed_up.clone();
                                    s.speed_down = rad.speed_down.clone();
                                    s.up_ceil_str = rad.up_ceil_str.clone();
                                    s.down_ceil_str = rad.down_ceil_str.clone();
                                }
                                drop(store);
                                save_sessions_to_disk();
                                zlog!("[COA] QoS re-aplicado: {} up={} down={}", target_ip, rad.speed_up, rad.speed_down);
                            }
                            build_coa_response(44, id, &req_auth, &cfg.radius_secret, "CoA OK")
                        }
                        None => {
                            zlog!("[COA] CoA NAK: sesion no encontrada");
                            build_coa_response(45, id, &req_auth, &cfg.radius_secret, "Session not found")
                        }
                    }
                }
                _ => {
                    zlog!("[COA] code {} no soportado — ignorando", code);
                    continue;
                }
            };
            let _ = socket.send_to(&resp, src).await;
        }
    });
}

/// Busca la IP de la sesion por session_id > ip > mac > username.
fn find_session_ip(username: &str, ip: &str, session_id: &str, mac: &str) -> Option<String> {
    let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
    let map = store.as_ref()?;
    if !session_id.is_empty() {
        map.iter().find(|(_, s)| s.session_id == session_id).map(|(ip, _)| ip.clone())
    } else if !ip.is_empty() {
        map.get(ip).map(|_| ip.to_string())
    } else if !mac.is_empty() {
        map.iter()
            .find(|(_, s)| s.client_mac.to_lowercase() == mac.to_lowercase())
            .map(|(ip, _)| ip.clone())
    } else if !username.is_empty() {
        // FIX (2026-08-04): si hay 2+ sesiones del mismo usuario
        // (shared_users>1) y el CoA solo trae User-Name, matar una
        // ARBITRARIA es peligroso — NAK (ambiguo) y que el server
        // especifique IP/MAC/SID.
        let matches: Vec<String> = map.iter()
            .filter(|(_, s)| s.username == username)
            .map(|(ip, _)| ip.clone()).collect();
        if matches.len() == 1 { matches.into_iter().next() } else { None }
    } else {
        None
    }
}

/// Parsea atributos de un Disconnect/CoA-Request:
/// 1=User-Name, 8=Framed-IP-Address, 31=Calling-Station-Id (MAC),
/// 44=Acct-Session-Id, 49=Acct-Terminate-Cause.
fn parse_coa_attrs(data: &[u8]) -> (String, String, String, String, u32) {
    let mut username = String::new();
    let mut ip = String::new();
    let mut session_id = String::new();
    let mut mac = String::new();
    let mut cause = 0u32;
    let mut i = 0;
    while i + 1 < data.len() {
        let t = data[i];
        let l = data[i + 1] as usize;
        // FIX (2026-08-04): atributo malformado (longitud invalida) = paquete
        // corrupto — break (no se puede saltar al siguiente sin longitud valida;
        // un continue con i+=l invalido provocaria loop infinito).
        if l < 2 || i + l > data.len() { break; }
        let v = &data[i + 2..i + l];
        match t {
            1 => username = String::from_utf8_lossy(v).to_string(),
            8 if v.len() >= 4 => ip = format!("{}.{}.{}.{}", v[0], v[1], v[2], v[3]),
            31 => mac = String::from_utf8_lossy(v).to_string(),
            44 => session_id = String::from_utf8_lossy(v).to_string(),
            49 if v.len() >= 4 => cause = u32::from_be_bytes([v[0], v[1], v[2], v[3]]),
            _ => {}
        }
        i += l;
    }
    (username, ip, session_id, mac, cause)
}

/// Construye Disconnect-ACK/NAK o CoA-ACK/NAK (RFC 5176 §3.2):
/// Response Authenticator = MD5(Code + ID + Length + RequestAuthenticator + Attributes + Secret)
fn build_coa_response(code: u8, id: u8, req_auth: &[u8; 16], secret: &str, msg: &str) -> Vec<u8> {
    use md5::{Md5, Digest};
    let mut buf = Vec::new();
    buf.push(code);
    buf.push(id);
    buf.extend_from_slice(&[0u8; 2]);       // length placeholder
    buf.extend_from_slice(&[0u8; 16]);      // authenticator placeholder
    let mb = msg.as_bytes();
    buf.push(18);                           // Reply-Message (18)
    buf.push((mb.len() + 2) as u8);
    buf.extend_from_slice(mb);
    let len = buf.len() as u16;
    buf[2..4].copy_from_slice(&len.to_be_bytes());
    let mut hasher = Md5::new();
    hasher.update(&buf[0..4]);
    hasher.update(req_auth);
    hasher.update(&buf[20..]);
    hasher.update(secret.as_bytes());
    let hash = hasher.finalize();
    buf[4..20].copy_from_slice(&hash);
    buf
}

// ── CoA por POLLING HTTP (opcion C, 2026-08-04) ─────────────────────────
// En vez de recibir UDP entrante (requiere reachability server->NAS), Zpot
// CONSULTA al server RADIUS cada 30s un endpoint que devuelve las sesiones
// que RADIUS considera ACTIVAS (acctstoptime IS NULL). Las sesiones locales
// que ya NO estan en esa lista fueron cerradas por RADIUS (Lost-Carrier,
// saldo, admin) -> Zpot las expulsa (store + nft + tc + Stop cause 2).
// Usa tokio TcpStream (sin dependencias HTTP nuevas).

/// GET HTTP/1.1 simple (sin TLS) — devuelve el body. Timeout global.
async fn http_get(url: &str, timeout_secs: u64) -> Option<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rest = url.strip_prefix("http://")?;
    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()?),
        None => (host_port, 80),
    };
    let addr = format!("{}:{}", host, port);
    let Ok(mut stream) = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        tokio::net::TcpStream::connect(&addr),
    ).await.ok()? else { return None; };
    let req = format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path, host);
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        stream.write_all(req.as_bytes()),
    ).await;
    let mut resp = Vec::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        stream.read_to_end(&mut resp),
    ).await;
    let text = String::from_utf8_lossy(&resp).to_string();
    Some(text.split("\r\n\r\n").nth(1).unwrap_or("").to_string())
}

/// Task de polling: cada 30s consulta el endpoint RADIUS y expulsa las
/// sesiones locales que RADIUS ya cerro. Solo activo si coa_enabled y
/// coa_mode="poll".
pub fn spawn_coa_polling() {
    tokio::spawn(async {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let cfg = get_hs_config();
            if !cfg.coa_enabled || cfg.coa_mode != "poll" {
                continue; // desactivado — no hacer nada
            }
            if cfg.coa_poll_url.is_empty() {
                zlog!("[COA-POLL] coa_poll_url vacio — polling inactivo");
                continue;
            }
            let Some(body) = http_get(&cfg.coa_poll_url, 6).await else {
                zlog!("[COA-POLL] error consultando {}", cfg.coa_poll_url);
                continue;
            };
            // Respuesta esperada: JSON array de {username, framedipaddress, acctsessionid}
            let remote: serde_json::Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(_) => {
                    zlog!("[COA-POLL] JSON invalido: {}", &body[..body.len().min(160)]);
                    continue;
                }
            };
            let mut remote_sids: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut remote_ips: std::collections::HashSet<String> = std::collections::HashSet::new();
            // FIX (2026-08-04): si la respuesta NO es un array (null, {},
            // {"error":...} — p.ej. fallo de DB del endpoint) NO expulsar
            // nada este ciclo (antes se interpretaba como "0 sesiones" y se
            // expulsaba a TODOS los clientes cada 30s).
            let Some(arr) = remote.as_array() else {
                zlog!("[COA-POLL] respuesta NO es array ({}): ciclo cancelado, sin expulsiones", &body[..body.len().min(160)]);
                continue;
            };
            for e in arr {
                if let Some(sid) = e.get("acctsessionid").and_then(|v| v.as_str()) {
                    remote_sids.insert(sid.to_string());
                }
                if let Some(ip) = e.get("framedipaddress").and_then(|v| v.as_str()) {
                    remote_ips.insert(ip.to_string());
                }
            }
            zlog!("[COA-POLL] RADIUS activas: {} sids, {} ips", remote_sids.len(), remote_ips.len());
            // Comparar con el store local
            let local: Vec<(String, String, String)> = {
                let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
                store.as_ref().map(|m| m.iter()
                    .map(|(ip, s)| (ip.clone(), s.username.clone(), s.session_id.clone()))
                    .collect()).unwrap_or_default()
            };
            for (ip, username, sid) in local {
                let in_radius = remote_sids.contains(&sid) || remote_ips.contains(&ip);
                if !in_radius {
                    // FIX (2026-08-04): periodo de gracia 90s — una sesion
                    // recien creada cuyo Accounting-Start esta en vuelo (o el
                    // endpoint aun no la refleja) NO debe morir en el primer
                    // ciclo del polling.
                    let young = {
                        let store = session_store().lock().unwrap_or_else(|e| e.into_inner());
                        store.as_ref().and_then(|m| m.get(&ip)).map(|s| {
                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                            now.saturating_sub(s.start) < 90
                        }).unwrap_or(false)
                    };
                    if young { continue; }
                    zlog!("[COA-POLL] {} ip={} sid={} YA NO activa en RADIUS — cerrando", username, ip, sid);
                    session_disconnect_internal(&ip, &cfg.radius, &cfg.radius_secret, &cfg.iface, 2).await;
                }
            }
        }
    });
}
