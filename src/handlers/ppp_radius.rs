use axum::{Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::fs;

const PPP_RADIUS_PATH: &str = "/etc/zpot/ppp-radius.json";
const RADIUSCLIENT_CONF: &str = "/etc/radiusclient/radiusclient.conf";
const RADIUSCLIENT_SERVERS: &str = "/etc/radiusclient/servers";
const PPPOE_OPTIONS: &str = "/etc/ppp/pppoe-server-options";
const IP_UP: &str = "/etc/ppp/ip-up";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PppRadiusConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_server_name")]
    pub server_name: String,
    #[serde(default)]
    pub nas_identifier: String,
    #[serde(default = "default_nas_ip")]
    pub nas_ip: String,
    #[serde(default)]
    pub fallback_local: bool,
    #[serde(default)]
    pub accounting: bool,
    #[serde(default = "default_pool_start")]
    pub pool_start: String,
    #[serde(default = "default_pool_end")]
    pub pool_end: String,
    #[serde(default = "default_dns1")]
    pub dns1: String,
    #[serde(default = "default_dns2")]
    pub dns2: String,
}

fn default_server_name() -> String { "radius-main".to_string() }
fn default_nas_ip() -> String { "192.168.20.1".to_string() }
fn default_pool_start() -> String { "192.168.20.2".to_string() }
fn default_pool_end() -> String { "192.168.20.200".to_string() }
fn default_dns1() -> String { "192.168.20.1".to_string() }
fn default_dns2() -> String { "8.8.8.8".to_string() }

impl Default for PppRadiusConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server_name: default_server_name(),
            nas_identifier: String::new(),
            nas_ip: default_nas_ip(),
            fallback_local: false,
            accounting: false,
            pool_start: default_pool_start(),
            pool_end: default_pool_end(),
            dns1: default_dns1(),
            dns2: default_dns2(),
        }
    }
}

pub fn load_config() -> PppRadiusConfig {
    fs::read_to_string(PPP_RADIUS_PATH)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(cfg: &PppRadiusConfig) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(PPP_RADIUS_PATH).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(PPP_RADIUS_PATH, &json).map_err(|e| e.to_string())
}

/// GET /api/ppp/radius — config actual + servidores disponibles
pub async fn get_config() -> Json<serde_json::Value> {
    let cfg = load_config();
    // get_servers() ya devuelve JSON con secret enmascarado ("***")
    let servers: Vec<serde_json::Value> = super::radius::get_servers().await.0;
    Json(serde_json::json!({
        "config": cfg,
        "servers": servers,
    }))
}

// P1: backup con fecha antes de sobrescribir un config (rollback manual)
fn backup_file(path: &str) {
    if !std::path::Path::new(path).exists() { return; }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = fs::copy(path, format!("{}.bak-{}", path, ts));
}

/// POST /api/ppp/radius — guarda config (sin aplicar)
pub async fn post_config(Json(cfg): Json<PppRadiusConfig>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // P1: validar IPs del config antes de guardar (antes cualquier basura)
    for (label, val) in [("nas_ip", &cfg.nas_ip), ("dns1", &cfg.dns1), ("dns2", &cfg.dns2)] {
        if !val.is_empty() && val.parse::<std::net::Ipv4Addr>().is_err() {
            return Err((StatusCode::BAD_REQUEST, format!("{} invalida: {}", label, val)));
        }
    }
    if !cfg.pool_start.is_empty() && cfg.pool_start.parse::<std::net::Ipv4Addr>().is_err() {
        return Err((StatusCode::BAD_REQUEST, format!("pool_start invalida: {}", cfg.pool_start)));
    }
    if !cfg.pool_end.is_empty() && cfg.pool_end.parse::<std::net::Ipv4Addr>().is_err() {
        return Err((StatusCode::BAD_REQUEST, format!("pool_end invalida: {}", cfg.pool_end)));
    }
    save_config(&cfg).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({"ok": true, "message": "Config guardada. Usa /api/ppp/radius/apply para aplicarla."})))
}

/// POST /api/ppp/radius/apply — regenera archivos NAS + opcional restart pppoe
pub async fn apply_config(
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let cfg = load_config();
    let restart = body.get("restart").and_then(|v| v.as_bool()).unwrap_or(false);

    // Resolver servidor RADIUS
    let server = super::radius::get_server_by_name(&cfg.server_name)
        .or_else(super::radius::get_default_auth_server)
        .ok_or((StatusCode::BAD_REQUEST, format!("Servidor RADIUS '{}' no encontrado", cfg.server_name)))?;

    // 1. radiusclient.conf
    let auth_order = if cfg.fallback_local { "radius,local" } else { "radius" };
    let mut conf = String::new();
    conf.push_str("# Generado por Zpot-RS (PPP NAS RADIUS)\n");
    conf.push_str(&format!("auth_order\t{}\n", auth_order));
    conf.push_str(&format!("authserver\t{}:{}\n", server.ip, server.auth_port));
    // acctserver SIEMPRE: el plugin radius.so de pppd 2.5.2 exige acctserver en el
    // config para que rc_read_config() cargue (si falta -> "Can't read config file"
    // y la auth RADIUS falla). El plugin hace accounting Start/Stop por defecto.
    conf.push_str(&format!("acctserver\t{}:{}\n", server.ip, server.acct_port));
    conf.push_str("servers\t\t/etc/radiusclient/servers\n");
    conf.push_str("dictionary\t/etc/radiusclient/dictionary\n");
    conf.push_str("login_radius\t/usr/sbin/login.radius\n");
    conf.push_str("seqfile\t\t/var/run/radius.seq\n");
    conf.push_str("mapfile\t\t/etc/radiusclient/port-id-map\n");
    conf.push_str(&format!("radius_timeout\t{}\n", server.timeout.max(1)));
    conf.push_str(&format!("radius_retries\t{}\n", server.retries.max(1)));
    if !cfg.nas_identifier.is_empty() {
        conf.push_str(&format!("nas_identifier\t{}\n", cfg.nas_identifier));
    }
    // P1: backup antes de sobrescribir (rollback manual)
    backup_file(RADIUSCLIENT_CONF);
    fs::write(RADIUSCLIENT_CONF, &conf).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 2. servers (secret)
    let servers_file = format!("# Generado por Zpot-RS\n{}\t{}\n", server.ip, server.secret);
    backup_file(RADIUSCLIENT_SERVERS);
    fs::write(RADIUSCLIENT_SERVERS, &servers_file).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 3. pppoe-server-options (mantener require-mschap-v2 + ms-dns; añadir plugin radius.so si enabled)
    // Timers LCP echo: deteccion de enlace muerto (~15s) -> cierre ordenado -> menos zombies PPP
    let mut opts = String::new();
    opts.push_str("# Generado por Zpot-RS\n");
    opts.push_str("require-mschap-v2\n");
    opts.push_str("lcp-echo-interval 5\n");
    opts.push_str("lcp-echo-failure 3\n");
    opts.push_str(&format!("ms-dns {}\n", cfg.dns1));
    opts.push_str(&format!("ms-dns {}\n", cfg.dns2));
    if cfg.enabled {
        opts.push_str("plugin radius.so\n");
        opts.push_str("plugin radattr.so\n");
    }
    backup_file(PPPOE_OPTIONS);
    fs::write(PPPOE_OPTIONS, &opts).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 4. ip-up — leer radattr (VSAs rate-limit) cuando RADIUS está activo
    write_ip_up(&cfg).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // 5. Restart opcional del servidor PPPoE (solo si el usuario lo pide explicitamente)
    let mut restarted = false;
    let mut active_before = 0;
    if restart {
        let result = tokio::task::spawn_blocking(|| {
            let before = count_ppp_interfaces();
            let out = std::process::Command::new("rc-service")
                .args(["pppoe", "restart"])
                .output()
                .map_err(|e| format!("rc-service: {}", e))?;
            let ok = out.status.success();
            if !ok {
                eprintln!("[PPP-RADIUS] rc-service pppoe restart failed: {}",
                    String::from_utf8_lossy(&out.stderr));
            }
            Ok::<(u32, bool), String>((before, ok))
        }).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("spawn_blocking: {}", e)))?;
        match result {
            Ok((before, ok)) => { active_before = before; restarted = ok; }
            Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, e)),
        }
    }

    Ok(Json(serde_json::json!({
        "ok": true,
        "applied": true,
        "enabled": cfg.enabled,
        "server": server.ip,
        "restarted": restarted,
        "active_before": active_before,
    })))
}

/// GET /api/ppp/radius/status — estado real de los archivos en disco
pub async fn get_status() -> Json<serde_json::Value> {
    let cfg = load_config();
    let conf = fs::read_to_string(RADIUSCLIENT_CONF).unwrap_or_default();
    let servers = fs::read_to_string(RADIUSCLIENT_SERVERS).unwrap_or_default();
    let opts = fs::read_to_string(PPPOE_OPTIONS).unwrap_or_default();

    Json(serde_json::json!({
        "config": cfg,
        "radiusclient_conf": conf,
        "radiusclient_servers": servers,
        "pppoe_options": opts,
        "plugin_radius_in_options": opts.contains("plugin radius.so"),
    }))
}

fn write_ip_up(_cfg: &PppRadiusConfig) -> Result<(), String> {
    let script = r#"#!/bin/sh
logger -t ppp "user $PEERNAME logged in intf $1 local $4 remote $5"

# Guardar MAC del peer (calling number) para correlacion pppd<->interfaz
# (el cmdline de pppd tiene la IP PROVISIONAL del pool, no la final;
#  la MAC es estable y unica por CPE)
# $6 suele venir VACIO con pppoe-server -> leer la MAC del cmdline del
# pppd padre (/proc/$PPID/cmdline, campo "remotenumber <MAC>")
MAC=$(tr '\0' ' ' < /proc/$PPID/cmdline 2>/dev/null | sed -E 's/.*remotenumber ([0-9a-f:]{17}).*/\1/')
[ -z "$MAC" ] && MAC="$6"
echo "$MAC" > /var/run/ppp-mac-$1 2>/dev/null

# RADIUS activo: radattr.so escribio /var/run/radattr.$1 con las VSAs
# --max-time 5: evita bloquear pppd si el backend cuelga
curl -s --max-time 5 -X POST http://localhost:8081/api/ppp/qos/radius \
  -H "Content-Type: application/json" \
  -d "{\"username\":\"$PEERNAME\",\"ip\":\"$5\",\"iface\":\"$1\"}"
"#;
    fs::write(IP_UP, script).map_err(|e| e.to_string())?;
    let _ = std::process::Command::new("chmod").args(["+x", IP_UP]).output();
    Ok(())
}

fn count_ppp_interfaces() -> u32 {
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg("ip -br link show type ppp 2>/dev/null | wc -l")
        .output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0))
        .unwrap_or(0)
}
