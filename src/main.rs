use axum::extract::ConnectInfo;
use std::net::SocketAddr;
use std::collections::HashMap;
use std::path::PathBuf;
use axum::{
    response::{Html, Redirect, IntoResponse, Response},
    routing::{get, delete, post},
    http::StatusCode,
    Router,
};
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use axum::{http::HeaderValue, Json};
use base64::Engine;
use handlers::mwan::WanConfig;

mod handlers;
mod naming;
mod routeros_parser;

const PROJ_DIR: &str = match option_env!("NARA_PROJ_DIR") {
    Some(v) => v,
    None => "/home/naram",
};

fn render_page(content: &str) -> String {
    let base = std::fs::read_to_string(
        format!("{}/templates/base.html", PROJ_DIR),
    )
    .unwrap_or_else(|_| "<html><body>base.html not found</body></html>".into());
    base.replace("__CONTENT_PLACEHOLDER__", content)
}

/// Verifica si una WAN esta UP mediante ping al GATEWAY de la WAN
/// (workaround driver igc que reporta carrier=0 falsamente).
/// P1: ANTES ping a 8.8.8.8 con -I <wan.ip> → dependia de la ruta MAIN
/// (una WAN viva podia declararse caida si la default apuntaba a la otra).
/// El gateway esta en la MISMA subred → ruta local por la iface.
fn check_wan_ping(wan: &WanConfig) -> bool {
    if wan.ip.is_empty() || wan.gateway.is_empty() {
        return false;
    }
    std::process::Command::new("ping")
        .args(["-c", "1", "-W", "2", "-I", &wan.ip, &wan.gateway])
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Deteccion de clientes hotspot desconectados por ARP (cada 30s).
/// FIX-1 2026-08-03: ejecutado en spawn_blocking para NO bloquear los
/// workers del runtime (misma logica, mismo orden, solo cambia el hilo).
/// FIX-7 2026-08-04: antes solo borraba el elemento nft cuando el neigh
/// era FAILED/INCOMPLETE — la sesion quedaba en el store hasta que el
/// interim la expulsara por idle (hasta 1h). Ahora recoge las IPs y llama
/// session_disconnect_internal (store + nft + tc + accounting Stop cause 2
/// Lost-Carrier) — expulso rapido cuando el cliente se va del WiFi.
/// NOTA (BUG-3): el set hotspot_auth es concatenado (ipv4_addr.ether_addr)
/// y elem.as_str() devuelve None -> el loop NO limpia clientes. Pendiente
/// corregir parseo a {"val":{"concat":[...]}} en iteracion siguiente.
// FIX (2026-08-04): de-bounce ARP — una sola observacion FAILED/INCOMPLETE
// (roaming, PSM de moviles, blip de radio) NO debe expulsar; exigir 2 ciclos
// consecutivos (60s) antes de session_disconnect_internal.
static ARP_FAIL_COUNT: std::sync::Mutex<Option<std::collections::HashMap<String, u32>>> = std::sync::Mutex::new(None);

async fn hotspot_arp_cleanup() {
    // Recoger IPs cuyo ARP dice FAILED/INCOMPLETE (la MAC no responde)
    let raw_failed: Vec<(String, String)> = tokio::task::spawn_blocking(|| {
        let mut found = Vec::new();
        let hs_out = std::process::Command::new("nft")
            .args(["-j", "list", "set", "inet", "hotspot", "hotspot_auth"])
            .output().ok();
        if let Some(o) = hs_out {
            if let Ok(data) = serde_json::from_slice::<serde_json::Value>(&o.stdout) {
                if let Some(arr) = data.get("nftables").and_then(|a| a.as_array()) {
                    if let Some(meta) = arr.get(1) {
                        if let Some(set_obj) = meta.get("set") {
                            if let Some(elems) = set_obj.get("elem").and_then(|e| e.as_array()) {
                                for elem in elems {
                                    // FIX-6 (BUG-3): el set hotspot_auth es CONCATENADO
                                    // (ipv4_addr.ether_addr). Formato JSON:
                                    //   {"elem": {"val": {"concat": ["IP","MAC"]}, "expires": N}}
                                    //   o {"elem": ["IP","MAC"]}
                                    // ANTES: elem.as_str() devolvia None -> loop INERTE.
                                    let pair: Option<(String, String)> = elem.as_object()
                                        .and_then(|obj| obj.get("elem"))
                                        .and_then(|inner| {
                                            if let Some(arr) = inner.as_array() {
                                                Some((arr[0].as_str().unwrap_or("").to_string(), arr[1].as_str().unwrap_or("").to_string()))
                                            } else if let Some(o2) = inner.as_object() {
                                                o2.get("val")
                                                    .and_then(|v| v.get("concat"))
                                                    .and_then(|c| c.as_array())
                                                    .map(|arr| (arr[0].as_str().unwrap_or("").to_string(), arr[1].as_str().unwrap_or("").to_string()))
                                            } else {
                                                None
                                            }
                                        });
                                    if let Some((ip, mac)) = pair {
                                        if ip.is_empty() || mac.is_empty() { continue; }
                                        let arp = std::process::Command::new("ip")
                                            .args(["neigh", "show", &ip])
                                            .output().ok();
                                        if let Some(a) = arp {
                                            let txt = String::from_utf8_lossy(&a.stdout);
                                            // FIX-6: SOLO FAILED/INCOMPLETE (ARP dice que la MAC
                                            // no responde). El neigh VACIO NO se borra (puede ser
                                            // transicion; el interim task lo expulsa por idle).
                                            // Se elimino el ping -W2 (2s x cliente bloqueando).
                                            if txt.contains("FAILED") || txt.contains("INCOMPLETE") {
                                                found.push((ip, mac));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        found
    }).await.unwrap_or_default();

    // FIX-7: limpiar la sesion COMPLETA (store + nft + tc + Stop cause 2)
    // FIX (2026-08-04): de-bounce — exigir 2 observaciones consecutivas.
    let mut to_disconnect: Vec<(String, String)> = Vec::new();
    {
        let mut counts = ARP_FAIL_COUNT.lock().unwrap_or_else(|e| e.into_inner());
        let map = counts.get_or_insert_with(std::collections::HashMap::new);
        for (ip, mac) in &raw_failed {
            let c = map.entry(ip.clone()).or_insert(0);
            *c += 1;
            if *c >= 2 {
                to_disconnect.push((ip.clone(), mac.clone()));
                map.remove(ip);
            }
        }
        // Resetear IPs que ya no estan fallando este ciclo
        map.retain(|ip, _| raw_failed.iter().any(|(i, _)| i == ip));
    }
    if !to_disconnect.is_empty() {
        let cfg = handlers::hotspot::get_hs_config_pub();
        for (ip, mac) in &to_disconnect {
            eprintln!("  [HOTSPOT] Cliente {} ({}) desconectado (ARP FAILED/INCOMPLETE), limpiando sesion", ip, mac);
            handlers::hotspot::session_disconnect_internal(
                ip, &cfg.radius, &cfg.radius_secret, &cfg.iface, 2,
            ).await; // 2 = Lost-Carrier
        }
    }
}

/// Intenta auto-reconexion via MAC cookie (server-side). Retorna Some(response)
/// si la cookie es valida y hay que redirigir a /hotspot/portal (re-auth completo).
/// Compartida por handle_root y handle_hotspot_fallback (dedup 2026-08-04).
async fn auto_reconnect_from_cookie(headers: &axum::http::HeaderMap, client_ip: &str, tag: &str) -> Option<Response> {
    if let Some(cookie_str) = headers.get("cookie").and_then(|v| v.to_str().ok()) {
        for part in cookie_str.split(';') {
            let part = part.trim();
            if part.starts_with("hs_session=") {
                if let Ok(decoded) = base64::Engine::decode(
                    &base64::engine::general_purpose::STANDARD,
                    &part[11..])
                {
                    if let Ok(token) = String::from_utf8(decoded) {
                        let parts: Vec<&str> = token.split(':').collect();
                        // FIX (2026-08-04): cookie sin password — formato "user:mac"
                        if parts.len() >= 2 {
                            let username = parts[0];
                            let saved_mac = parts[1];
                            // Verificar que la MAC existe en ARP
                            // FIX-4 (BUG-12): ip neigh show en spawn_blocking — no bloquear workers.
    // FIX (2026-08-04): filtrar por dev cfg.iface — ANTES buscaba la MAC en
    // TODA la tabla ARP (eth0/eth1 WAN, bridges) y una MAC visible en otra
    // iface disparaba un redirect espurio al portal.
                            let hs_cfg_arp = handlers::hotspot::get_hs_config_pub();
                            let arp_iface = if hs_cfg_arp.iface.is_empty() { "eth4".to_string() } else { hs_cfg_arp.iface.clone() };
                            let arp_text = tokio::task::spawn_blocking(move || {
                                std::process::Command::new("ip")
                                    .args(["neigh", "show", "dev", &arp_iface])
                                    .output()
                                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                                    .unwrap_or_default()
                            }).await.unwrap_or_default();
                            if !arp_text.is_empty() {
                                for line in arp_text.lines() {
                                    let line_parts: Vec<&str> = line.split_whitespace().collect();
                                    if line_parts.len() >= 5 {
                                        let mut mac = "";
                                        for i in 0..line_parts.len() {
                                            if line_parts[i] == "lladdr" && i + 1 < line_parts.len() { mac = line_parts[i + 1]; }
                                        }
                                        // FIX (2026-08-04): case-insensitive (igual que cookie_entry_exists)
                                        if mac.to_lowercase() == saved_mac.to_lowercase() {
                                            // MAC coincide! Verificar que la cookie existe server-side
                                            if handlers::hotspot::cookie_entry_exists(username, saved_mac) {
                                                // Cookie valida -> redirect a portal_root para sesion completa + interim task
                                                zlog!("[HOTSPOT][{}] {} se conecto desde cookie (via MAC, ip={}, mac={})", tag, username, client_ip, mac);
                                                return Some((StatusCode::FOUND, [("location", "/hotspot/portal")], ()).into_response());
                                            } else {
                                                // Cookie eliminada del server (por admin o expirada) -> no auto-reconectar
                                                zlog!("[HOTSPOT][{}] COOKIE RECHAZADA (no existe server-side) — {} pide login: mac={}", tag, username, saved_mac);
                                            }
                                        } else {
                                            // MAC del ARP no coincide con la de la cookie -> pide login
                                            zlog!("[HOTSPOT][{}] COOKIE IGNORADA — MAC no coincide (arp={}, cookie={}) — {} pide login", tag, mac, saved_mac, username);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                break;
            }
        }
    }
    None
}

/// FIX (2026-08-04): sesion activa SOLO si la MAC del peer coincide con la
/// de la sesion — una IP reasignada por DHCP a OTRO dispositivo no debe
/// heredar la sesion (antes handle_root/fallback miraban solo la IP y el
/// dispositivo B veia "autenticado" sin poder navegar ni loguearse).
async fn has_active_session_for_peer(client_ip: &str) -> bool {
    let (has_session, session_mac) = {
        let store = handlers::hotspot::session_store().lock().unwrap_or_else(|e| e.into_inner());
        match store.as_ref().and_then(|s| s.get(client_ip)) {
            None => (false, String::new()),
            Some(s) => (true, s.client_mac.clone()),
        }
    };
    if !has_session { return false; }
    if session_mac.is_empty() { return true; }
    let mac = handlers::hotspot::get_mac_from_arp(client_ip).await;
    !mac.is_empty() && mac.to_lowercase() == session_mac.to_lowercase()
}

/// Root / — Hotspot portal (login o redirect a status segun autenticacion)
async fn handle_root(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
) -> Response {
    let client_ip = addr.ip().to_string();

    // Verificar sesión activa en session_store (misma fuente que portal_root).
    // ANTES usaba `nft get element { IP }` pero el set es ipv4_addr.ether_addr
    // (concatenado) → con IP sola daba "invalid data type" → authed siempre false
    // → loop infinito: handle_root → portal → alogin → handle_root ...
    // FIX (2026-08-04): ademas verificar la MAC del peer (escenario IP
    // reasignada a otro dispositivo — no hereda la sesion).
    let authed = has_active_session_for_peer(&client_ip).await;

    if authed {
        return handlers::hotspot::portal_status_inline(client_ip).into_response();
    }

    // Intentar auto-reconexion via MAC cookie (dedup: auto_reconnect_from_cookie)
    if let Some(resp) = auto_reconnect_from_cookie(&headers, &client_ip, "ROOT").await {
        return resp;
    }

    // No autenticado ni cookie valida -> redirect a portal (para que cookie se envie)
    (StatusCode::FOUND, [("location", "/hotspot/portal")], ()).into_response()
}

/// SPA admin en /zpot
async fn handle_zpot() -> Html<String> {
    Html(render_page(""))
}

/// SPA admin catch-all para /zpot/* (solo puerto admin)
async fn handle_zpot_spa() -> Response {
    Html(render_page("")).into_response()
}

/// Fallback para puerto hotspot — sirve login para cualquier ruta desconocida
/// (captura /generate_204, /hotspot-detect.html, etc.)
async fn handle_hotspot_fallback(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
) -> Response {
    let ip = addr.ip().to_string();
    // FIX-4 (BUG-13): el set hotspot_auth es concatenado (ipv4_addr.ether_addr)
    // y `nft get element {IP}` con IP sola da "invalid data type" -> SIEMPRE
    // fallaba (authed=false). Usar session_store (misma fuente que handle_root).
    // FIX (2026-08-04): ademas verificar la MAC del peer (misma regla que
    // handle_root y portal_root — IP reasignada no hereda la sesion).
    let authed = has_active_session_for_peer(&ip).await;

    if authed {
        return handlers::hotspot::portal_status_inline(ip).into_response();
    }

    // Intentar auto-reconexion via MAC cookie (dedup: auto_reconnect_from_cookie)
    if let Some(resp) = auto_reconnect_from_cookie(&headers, &ip, "FALLBACK").await {
        return resp;
    }

    (StatusCode::FOUND, [("location", "/hotspot/portal")], ()).into_response()
}

async fn handle_mwan_get() -> Json<serde_json::Value> {
    // FIX (2026-08-08): antes devolvia {"status":"ok"} (stub) — el endpoint
    // debe exponer la config real (modo, distribucion, wans) del store.
    let st = handlers::mwan::store().state.lock().unwrap_or_else(|e| e.into_inner());
    Json(serde_json::json!({
        "status": "ok",
        "mode": st.mode,
        "distribution": st.distribution,
        "wans": st.wans,
    }))
}

fn build_admin_app(static_dir: PathBuf) -> Router {
    Router::new()
        // API — comandos reales del sistema
        .route("/api/interfaces", get(handlers::interfaces::list_interfaces))
        .route("/api/vlans", get(handlers::vlans::list).post(handlers::vlans::create))
        .route("/api/vlans/delete", post(handlers::vlans::delete))
        .route("/api/vlans/configure", post(handlers::vlans::configure))
        .route("/api/vlans/title", post(handlers::vlans::set_title))
        .route("/api/vlans/bridge-table", get(handlers::vlans::bridge_vlans))
        .route("/api/bridge/ports", get(handlers::vlans::bridge_vlans))
        .route("/api/bridge/ports/configure", post(handlers::vlans::configure_bridge_port))
        .route("/api/bridge/ports/add", post(handlers::bridges::port_add))
        .route("/api/bridge/ports/remove", post(handlers::bridges::port_remove))
        .route("/api/ip-addresses", get(handlers::ip_addresses::list).post(handlers::ip_addresses::add))
        .route("/api/ip-addresses/:ifname/:addr", delete(handlers::ip_addresses::delete))
        .route("/api/routes", get(handlers::routes::list).post(handlers::routes::add))
        .route("/api/routes/delete", post(handlers::routes::delete))
        .route("/api/arp", get(handlers::arp::list))
        .route("/api/pools", get(handlers::pools::list).post(handlers::pools::create).delete(handlers::pools::delete))
        .route("/api/pools/update", post(handlers::pools::update))
        .route("/api/dhcp-leases", get(handlers::dhcp_leases::list))
        .route("/api/dns", get(handlers::dns::list).post(handlers::dns::add))
        .route("/api/dns/delete", post(handlers::dns::delete))
        .route("/api/bridges", get(handlers::bridges::list).post(handlers::bridges::create))
        .route("/api/bridges/delete", post(handlers::bridges::delete))
        .route("/api/ppp/secrets", get(handlers::ppp::secrets_list).post(handlers::ppp::secrets_upsert))
        .route("/api/ppp/secrets/disconnect", post(handlers::ppp::secrets_disconnect))
        .route("/api/ppp/secrets/delete", post(handlers::ppp::secrets_delete))
        .route("/api/ppp/secrets/order", post(handlers::ppp::secrets_order))
        .route("/api/ppp/active", get(handlers::ppp::active_list))
        .route("/api/ppp/logs", get(handlers::ppp::logs_list))
        .route("/api/ppp/logs/auth", get(handlers::system::logs_auth))
        .route("/api/ppp/qos/radius", post(handlers::ppp::qos_radius_apply))
        .route("/api/ppp/qos/cleanup", post(handlers::ppp::qos_cleanup))
        .route("/api/ip/remote", get(handlers::ppp::remote_get))
        .route("/api/ip/remote", post(handlers::ppp::remote_set))
        .route("/api/ppp/server/status", get(handlers::ppp::pppoe_status))
        .route("/api/ppp/server/start", post(handlers::ppp::pppoe_start))
        .route("/api/ppp/server/stop", post(handlers::ppp::pppoe_stop))
        .route("/api/ppp/radius", get(handlers::ppp_radius::get_config).post(handlers::ppp_radius::post_config))
        .route("/api/ppp/radius/apply", post(handlers::ppp_radius::apply_config))
        .route("/api/ppp/radius/status", get(handlers::ppp_radius::get_status))
        .route("/api/wireguard/interfaces", get(handlers::wireguard::list).post(handlers::wireguard::create).delete(handlers::wireguard::delete))
        .route("/api/wireguard/peers/:name", get(handlers::wireguard::peers))
        .route("/api/wireguard/peers/add", post(handlers::wireguard::peers_add))
        .route("/api/wireguard/peers/delete", post(handlers::wireguard::peers_delete))
        .route("/api/command", post(handlers::command::run))
        .route("/api/mwan/status", get(handlers::mwan::get_mwan_status))
        .route("/api/mwan/config", get(handle_mwan_get).post(handlers::mwan::post_mwan_config))
        .route("/api/firewall/nat", get(handlers::firewall::list_nat_rules).post(handlers::firewall::create_nat_rule))
        .route("/api/firewall/nat/delete", post(handlers::firewall::delete_nat_rule))
        .route("/api/firewall/filter", get(handlers::firewall::list_filter_rules))
        .route("/api/firewall/filter/delete", post(handlers::firewall::delete_filter_rule))
        .route("/api/firewall/rule", post(handlers::firewall::create_nft_rule))
        .route("/api/firewall/rule/move", post(handlers::firewall::move_nft_rule))
        .route("/api/firewall/rule/move-to", post(handlers::firewall::move_nft_rule_to))
        .route("/api/firewall/mangle", get(handlers::firewall::list_mangle_rules))
        .route("/api/firewall/sets", get(handlers::firewall::list_nft_sets))
        .route("/api/firewall/conntrack", get(handlers::firewall::conntrack_status))
        .route("/api/radius/servers", get(handlers::radius::get_servers).post(handlers::radius::post_server).delete(handlers::radius::delete_server))
        .route("/api/hotspot/server", get(handlers::hotspot::get_server).post(handlers::hotspot::post_server))
        .route("/api/hotspot/active", get(handlers::hotspot::active_sessions))
        .route("/api/hotspot/walled-garden", get(handlers::hotspot::walled_garden_list).post(handlers::hotspot::walled_garden_add))
        .route("/api/hotspot/walled-garden/delete", post(handlers::hotspot::walled_garden_delete))
        .route("/api/hotspot/ip-bindings", get(handlers::hotspot::ip_bindings_list).post(handlers::hotspot::ip_bindings_add))
        .route("/api/hotspot/ip-bindings/delete", post(handlers::hotspot::ip_bindings_delete))
        .route("/api/hotspot/logs", get(handlers::hotspot::hotspot_logs))
        .route("/api/hotspot/cookies", get(handlers::hotspot::cookies_list))
        .route("/api/hotspot/cookies/delete", post(handlers::hotspot::cookies_delete))
        .route("/hotspot/portal/disconnect", post(handlers::hotspot::portal_disconnect_admin))
        .route("/api/system", get(handlers::system::info))
        .route("/api/system/logs", get(handlers::system::logs_list))
        .route("/api/system/scripts", get(handlers::system::scripts_list))
        .route("/api/system/scheduler", get(handlers::system::scheduler_list))
        .route("/api/system/speedtest", post(handlers::system::speedtest_run))
        .route("/api/system/files", get(handlers::system::files_list))
        .route("/api/system/export", get(handlers::system::export_config))
        .route("/api/system/import", post(handlers::system::import_config))
        .route("/api/system/files/hotspot/download", get(handlers::system::hotspot_download))
        .route("/api/system/files/hotspot/upload", post(handlers::system::hotspot_upload))
        // SPA admin
        .route("/zpot", get(handle_zpot))
        // Estaticos con revalidacion SIEMPRE (no-cache): el browser no debe
        // congelar paginas/JS tras un deploy (problema recurrente).
        .nest_service("/static", ServeDir::new(&static_dir))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-cache"),
        ))
        .fallback(handle_zpot_spa)
}

fn build_hotspot_app() -> Router {
    Router::new()
        .route("/", get(handle_root))
        .route("/hotspot/portal", get(handlers::hotspot::portal_root))
        .route("/hotspot/portal/login", get(handlers::hotspot::portal_login))
        .route("/hotspot/portal/auth", post(handlers::hotspot::portal_auth))
        .route("/hotspot/portal/status", get(handlers::hotspot::portal_status))
        .route("/hotspot/portal/logout", get(handlers::hotspot::portal_logout))
        .route("/hotspot/portal/disconnect", post(handlers::hotspot::portal_disconnect))
        .route("/hotspot/portal/static/*file", get(handlers::hotspot::portal_static))
        .route("/status", get(handlers::hotspot::portal_status))
        .route("/logout", get(handlers::hotspot::portal_logout))
        .fallback(handle_hotspot_fallback)
}

#[tokio::main]
async fn main() {
    let static_dir = PathBuf::from(PROJ_DIR).join("static");

    // Inicializar hotspot nftables PRIMERO (crea la tabla inet hotspot —
    // apply_nft_rules del MWAN depende de que exista para el masquerade WAN)
    if let Err(e) = init_hotspot_nft() {
        println!("  ⚠️ hotspot nft: {}", e);
    } else {
        println!("  ✅ hotspot nft: redirect 80 + masquerade eth1");
    }

    // Inicializar MWAN store (static, sin State de axum)
    handlers::mwan::init_store();
    // Aplicar reglas MWAN al arranque (nft + ip rules) — DESPUES del hotspot
    let state = handlers::mwan::read_state();
    if !state.wans.is_empty() {
        handlers::mwan::apply_nft_rules(&state).await;
        println!("  MWAN: {} WANs configuradas, nft+ip rules aplicadas", state.wans.len());
    }

    // Reconstruir sesiones hotspot desde disco (FIX 2026-08-02)
    restore_hotspot_sessions();
    // FIX-8 (BUG-6): UN SOLO task interim global (barre todas las sesiones c/60s)
    handlers::hotspot::spawn_interim_global();
    // FIX 2026-08-04 (caso G4RP): CoA/Disconnect — listener UDP 3799 (modo
    // udp) o polling HTTP al RADIUS (modo poll). Cada uno se auto-desactiva
    // segun coa_enabled/coa_mode de la config.
    handlers::hotspot::spawn_coa_listener();
    handlers::hotspot::spawn_coa_polling();

    // Limpiar interfaces PPP zombies (sin pppd) al arranque
    // FIX 2026-08-01 (bug critico): antes buscaba la IP final del peer en el
    // cmdline de pppd, pero el cmdline tiene la IP PROVISIONAL del pool
    // (-R .100-.200), no la final -> mataba interfaces VIVAS al reiniciar
    // el backend. Ahora: pppd vivo se correlaciona por MAC (remotenumber)
    // usando /var/run/ppp-mac-<iface> (escrito por ip-up, $6). Sin archivo
    // MAC -> no matar (conservador).
    println!("  🔄 Limpiando PPP zombies...");
    let cleanup_out = std::process::Command::new("sh")
        .arg("-c")
        .arg(r#"for ppp in $(ip -br addr show type ppp 2>/dev/null | awk '{print $1}'); do
  mac=$(cat /var/run/ppp-mac-$ppp 2>/dev/null | tr -d ' \n');
  [ -z "$mac" ] && continue;
  found=0;
  for pid in $(pgrep -x pppd); do
    if cat /proc/$pid/cmdline 2>/dev/null | tr '\0' ' ' | grep -q "remotenumber $mac"; then found=1; break; fi;
  done;
  if [ "$found" -eq 0 ]; then ip link delete dev $ppp 2>/dev/null && echo "  $ppp (mac $mac sin pppd)"; fi;
done"#)
        .output();
    match cleanup_out {
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stdout);
            let cleaned: Vec<&str> = out.lines().filter(|l| !l.is_empty()).collect();
            if cleaned.is_empty() {
                println!("  ✅ PPP zombies: ninguno");
            } else {
                for line in &cleaned {
                    println!("  🗑️ {}", line);
                }
                println!("  ✅ PPP zombies: {} eliminados", cleaned.len());
            }
        }
        Err(e) => println!("  ⚠️ PPP cleanup: {e}"),
    }

    // Sync periodico de clientes PPP: syslog + kernel -> /ppp/secrets
    // (auto-registro sin depender del ip-up; cada 60s, primero inmediato)
    handlers::ppp::spawn_ppp_sync_task();
    println!("  🔄 PPP sync task: auto-registro cada 60s (syslog+kernel)");

    // Hotspot: detectar clientes desconectados por ARP cada 30s (FIX-7).
    // FIX (2026-08-04): task DEDICADO — antes la llamada estaba DESPUES del
    // loop infinito del watchdog MWAN (codigo muerto, nunca se ejecutaba).
    tokio::spawn(async {
        use tokio::time::{interval, Duration};
        let mut tick = interval(Duration::from_secs(30));
        loop {
            tick.tick().await;
            hotspot_arp_cleanup().await;
        }
    });

    // Watchdog MWAN interno — verifica WAN via ping cada 30s
    tokio::spawn(async {
        use tokio::time::{interval, Duration};
        let mut tick = interval(Duration::from_secs(30));
        let mut prev_carrier: HashMap<String, i32> = HashMap::new();
        let mut first_run = true;
        loop {
            tick.tick().await;
            let state = handlers::mwan::read_state();
            if state.wans.is_empty() { continue; }
            let mut changed = false;

            // Verificar WAN actual via ping (workaround driver igc carrier=0)
            for (name, wan) in &state.wans {
                let wan_c = wan.clone();
                let up = tokio::task::spawn_blocking(move || check_wan_ping(&wan_c)).await.unwrap_or(false);
                let current = if up { 1 } else { 0 };
                let prev = prev_carrier.get(name).copied().unwrap_or(-1);
                if current != prev || first_run {
                    prev_carrier.insert(name.clone(), current);
                    if !up {
                        println!("  ⚠️ MWAN watchdog: {} ({}) caida (ping fail)", name, wan.iface);
                    } else {
                        println!("  ✅ MWAN watchdog: {} ({}) UP", name, wan.iface);
                    }
                    changed = true;
                }
            }
            first_run = false;

            if changed {
                println!("  🔄 MWAN watchdog: cambio detectado, re-aplicando reglas...");
                // Actualizar status antes de apply
                let mut new_state = state.clone();
                for (name, _) in &state.wans {
                    let carrier = *prev_carrier.get(name).unwrap_or(&0);
                    if let Some(wan) = new_state.wans.get_mut(name) {
                        wan.status = if carrier == 1 { "up".into() } else { "down".into() };
                    }
                }
                handlers::mwan::apply_nft_rules(&new_state).await;
            }

            // Ruta default main: priorizar WAN activa, migrar WG si es necesario
            let mut up_wans: Vec<&WanConfig> = Vec::new();
            for w in state.wans.values() {
                let wan_c = w.clone();
                if tokio::task::spawn_blocking(move || check_wan_ping(&wan_c)).await.unwrap_or(false) {
                    up_wans.push(w);
                }
            }
            // Priorizar wan1 (eth0) como default si esta up, sino wan2 (eth1)
            let default_wan = up_wans.iter().find(|w| w.mark == 1)
                .or_else(|| up_wans.first());
            if let Some(wan) = default_wan {
                // P1: NUNCA `ip route del default` en bucle — borra TODAS las
                // default (incluida la de la otra WAN) y si el add posterior
                // falla, el router queda SIN salida. `replace` es atomico:
                // reemplaza la default existente o la crea.
                let gw = wan.gateway.clone();
                let ifc = wan.iface.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = std::process::Command::new("ip")
                        .args(["-4", "route", "replace", "default", "via", &gw, "dev", &ifc])
                        .output();
                }).await.ok();
                println!("  🚦 MWAN watchdog: ruta default main -> {} via {}", wan.iface, wan.gateway);
            }
        }
    });

    // Construir ambas apps
    let hotspot_app = build_hotspot_app();
    let admin_app = build_admin_app(static_dir.clone());

    let hs_addr = SocketAddr::from(([0, 0, 0, 0], 80));
    let admin_addr = SocketAddr::from(([0, 0, 0, 0], 8081));

    println!("═══════════════════════════════════════════");
    println!("  Zpot-RS — dos puertos separados");
    println!("  🔥 Hotspot (login/status):  http://0.0.0.0:80");
    println!("  🖥️  Admin (SPA + API):      http://0.0.0.0:8081");
    println!("═══════════════════════════════════════════");

    let hs_listener = tokio::net::TcpListener::bind(hs_addr).await.unwrap();
    let admin_listener = tokio::net::TcpListener::bind(admin_addr).await.unwrap();

    // Correr ambos servidores concurrentemente
    tokio::join!(
        axum::serve(
            hs_listener,
            hotspot_app.into_make_service_with_connect_info::<SocketAddr>(),
        ),
        axum::serve(
            admin_listener,
            admin_app.into_make_service(),
        ),
    );
    // Uno de los dos siempre corre — si ambos terminan, error
}

fn init_hotspot_nft() -> Result<(), Box<dyn std::error::Error>> {
    use std::process::Command;

    // FIX (2026-08-04): parametrizar con cfg.iface/gw — ANTES todo hardcodeaba
    // eth3/192.168.10.0/24; cambiar la config dejaba el firewall apuntando a
    // la iface vieja (roto o abierto).
    let cfg = handlers::hotspot::get_hs_config_pub();
    let iface = if cfg.iface.is_empty() { "eth4".to_string() } else { cfg.iface.clone() };
    let gw = if cfg.gw.is_empty() { "192.168.10.1".to_string() } else { cfg.gw.clone() };
    let gw_prefix = gw.rsplit_once('.').map(|(p, _)| p.to_string()).unwrap_or_else(|| "192.168.10".to_string());
    let hs_net = format!("{}.0/24", gw_prefix);

    // Esperar que nft esté disponible
    for attempt in 1..=5 {
        let check = Command::new("nft").arg("list").arg("tables").output();
        if check.is_ok() { break; }
        if attempt < 5 { std::thread::sleep(std::time::Duration::from_secs(2)); }
    }

    // Flush+crear en lugar de add repetido (evita duplicados post-reboot)
    let _ = Command::new("nft").args(["delete", "table", "inet", "hotspot"]).output();
    let _ = Command::new("nft").args(["add", "table", "inet", "hotspot"]).output();

    // Chain prerouting NAT: redirect HTTP 80 al portal, saltar si autenticado
    let _ = Command::new("nft").args(["add", "chain", "inet", "hotspot", "prerouting",
        "{ type nat hook prerouting priority dstnat; policy accept; }"]).output();

    // Chain postrouting: masquerade salida a WAN
    let _ = Command::new("nft").args(["add", "chain", "inet", "hotspot", "postrouting",
        "{ type nat hook postrouting priority srcnat; policy accept; }"]).output();

    // Chain forward: bloquear trafico no autenticado desde eth3 (policy accept)
    // NOTA: policy accept — las reglas restrictivas se evaluan antes del default accept
    // Se borra y recrea para asegurar la politica correcta
    let _ = Command::new("nft").args(["delete", "chain", "inet", "hotspot", "forward"]).output();
    let _ = Command::new("nft").args(["add", "chain", "inet", "hotspot", "forward",
        "{ type filter hook forward priority filter; policy accept; }"]).output();

    // FIX-9 (BUG-19): proteger admin 8081 — SOLO WG y LAN pueden acceder.
    // policy accept (no toca SSH/DNS/otros); solo 8081 se restringe.
    let _ = Command::new("nft").args(["add", "chain", "inet", "hotspot", "input",
        "{ type filter hook input priority filter; policy accept; }"]).output();
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "input",
        "iif", "lo", "accept"]).output();
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "input",
        "tcp", "dport", "8081", "ip", "saddr", "{ 10.7.0.0/24, 192.168.2.0/24, 192.168.4.0/24, 192.168.5.0/24 }", "accept"]).output();
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "input",
        "tcp", "dport", "8081", "drop"]).output();
    // FIX (2026-08-04): el portal :80 SOLO para el hotspot (iface), lo y wg0 —
    // ANTES cualquier host de WAN/ppp* que tocara la IP del router:80 veia el
    // login del hotspot (y podia brute-forcear RADIUS distribuido). El admin
    // usa :8081 (restringido arriba).
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "input",
        "iifname", "{", &iface, ",", "lo", ",", "wg0", "}", "tcp", "dport", "80", "accept"]).output();
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "input",
        "tcp", "dport", "80", "drop"]).output();

    // Set de IPs autenticadas (bypass redirect + bypass filter)
    let _ = Command::new("nft").args(["add", "set", "inet", "hotspot", "hotspot_auth",
        "{ type ipv4_addr . ether_addr; flags timeout; timeout 24h; }"]).output();

    // Set de IPs de controladores UniFi/Omada (accesibles sin autenticar)
    let _ = Command::new("nft").args(["add", "set", "inet", "hotspot", "controladores",
        "{ type ipv4_addr; flags timeout; timeout 1d; }"]).output();
    // Poblar con IPs de controladores conocidos
    for ctrl_ip in &["161.97.67.63", "44.193.125.236", "18.213.142.156", "34.238.17.94", "54.243.197.97"] {
        let _ = Command::new("nft").args(["add", "element", "inet", "hotspot", "controladores",
            "{", ctrl_ip, "timeout", "1d", "}"]).output();
    }

    // === REGLAS PREROUTING (NAT) ===
    // BLOQUEAR puerto admin (8081) para TODOS los clientes hotspot (incluso auth)
    let _ = Command::new("nft").args(["insert", "rule", "inet", "hotspot", "prerouting",
        "iif", &iface, "tcp", "dport", "8081", "drop"]).output();
    // BLOQUEAR puerto admin (8081) para clientes PPPoE (ppp*) — simetria con eth3.
    // El input chain ya dropea 8081 (solo acepta 10.7.0.0/24 + LANs), esto corta
    // ANTES del DNAT/routing por si el 8081 se usa como destino de un forward.
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "prerouting",
        "iifname", "ppp*", "tcp", "dport", "8081", "drop"]).output();
    // PRIMERO: autenticados saltan todo (sin redirect)
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "prerouting",
        "iif", &iface, "ip", "saddr", ".", "ether", "saddr", "@hotspot_auth", "return"]).output();
    // SOLO no-autenticados: HTTP 80 redirect al portal
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "prerouting",
        "iif", &iface, "tcp", "dport", "80", "redirect"]).output();

    // === REGLAS FORWARD (FILTRO) ===
    // Permitir DHCP (cliente→servidor udp 67/68)
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "udp", "dport", "{ 67, 68 }", "accept"]).output();
    // Permitir DNS (udp/tcp 53)
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "udp", "dport", "53", "accept"]).output();
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "tcp", "dport", "53", "accept"]).output();
    // Permitir HTTP 80 (captive portal)
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "tcp", "dport", "80", "accept"]).output();
    // === WALLED-GARDEN: controladores UniFi/Omada (accesibles sin autenticar) ===
    // Cualquier protocolo/puerto hacia IPs del set controladores
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "ip", "daddr", "@controladores", "accept"]).output();
    // Omada Management TCP
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "tcp", "dport", "{ 29810-29814 }", "accept"]).output();
    // Omada Portal HTTP/HTTPS/Guest Portal
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "tcp", "dport", "{ 8088, 8043, 8843 }", "accept"]).output();
    // Omada Discovery UDP
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "udp", "dport", "{ 29810-29814 }", "accept"]).output();
    // UniFi STUN UDP 3478
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "udp", "dport", "3478", "accept"]).output();
    // UniFi Discovery UDP 10001
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "udp", "dport", "10001", "accept"]).output();
    // Omada Discovery broadcast UDP 27001
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "udp", "dport", "27001", "accept"]).output();
    // NTP (UDP 123) — necesario para que APs sincronicen hora antes de conectar al controlador
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "udp", "dport", "123", "accept"]).output();
    // RESPUESTAS de flujos YA establecidos (FIX 2026-08-04): el DNAT remoto
    // (8082 -> AP:80) o la ruta VPN (10.7.0.x -> 192.168.10.x) hacen que el AP
    // responda por eth3 hacia 10.7.0.1 — el aislamiento "hotspot -> mgmt" de
    // abajo lo dropeaba y el admin nunca recibia el SYN-ACK. Aceptar
    // established/related ANTES de los drops, PERO SOLO respuestas que NO van
    // a la propia subred hotspot (daddr != 192.168.10.0/24) — asi el
    // port-isolation eth3->eth3 se mantiene y un cliente no puede inyectar en
    // las sesiones del admin↔AP suplantando la IP del AP dentro de eth3.
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "ct", "state", "established,related", "ip", "daddr", "!=", &hs_net, "accept"]).output();
    // AISLAMIENTO: clientes hotspot no se comunican entre si (port isolation)
    // Solo gateway (192.168.10.1) e internet
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "oif", &iface, "drop"]).output();
    // AISLAMIENTO: bloquear hotspot -> mgmt y ppp (incluso auth)
    // eth0/eth1 son WANs (balanceo) — no se bloquean
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "ip", "daddr", "{ 10.7.0.0/24, 192.168.20.0/24 }", "drop"]).output();
    // Si la IP esta autenticada, permitir todo lo demas (internet)
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "ip", "saddr", ".", "ether", "saddr", "@hotspot_auth", "accept"]).output();
    // DROP: todo trafico no-autenticado desde eth3
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iif", &iface, "drop"]).output();
    // AISLAMIENTO: clientes PPP no se comunican entre si (private VLAN)
    // Solo gateway (192.168.20.1) e internet
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iifname", "ppp*", "oifname", "ppp*", "drop"]).output();
    // AISLAMIENTO: bloquear PPPoE -> mgmt y hotspot (evita que PPP acceda a redes internas)
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "forward",
        "iifname", "ppp*", "ip", "daddr", "{", "10.7.0.0/24", ",", &hs_net, "}", "drop"]).output();

    // === POSTROUTING — masquerade a todas las WANs ===
    // NOTA (2026-08-08): el ORDEN REAL de boot es init_hotspot_nft() PRIMERO
    // (L403) y apply_nft_rules() DESPUÉS (L413) — el comentario anterior
    // ("apply_nft_rules corre ANTES que init_hotspot_nft") quedó obsoleto
    // tras el fix 9d7958b. Por eso el masquerade se agrega AQUI siempre,
    // independiente de MWAN: cubre el caso de que la tabla hotspot exista.
    // Si MWAN esta activo, sync_hotspot_wans() tambien agregara reglas
    // cuando se re-configure — pero al arranque estas reglas son necesarias.
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "postrouting",
        "oif", "eth0", "masquerade"]).output();
    let _ = Command::new("nft").args(["add", "rule", "inet", "hotspot", "postrouting",
        "oif", "eth1", "masquerade"]).output();

    // === RE-APLICAR estado persistido (FIX 2026-08-02) ===
    // Cookies server-side: recargar desde disco (sobreviven reinicios)
    handlers::hotspot::load_cookies_from_disk();
    // Walled-garden e IP-bindings: re-insertar reglas nft desde disco
    // (antes se perdian al reiniciar el backend — solo se aplicaban al POST API)
    let wg_entries = handlers::hotspot::load_wg();
    handlers::hotspot::apply_wg_rules(&wg_entries);
    let ib_entries = handlers::hotspot::load_ib();
    handlers::hotspot::apply_ib_rules(&ib_entries);

    println!("  🔥 hotspot nft ok — bloqueo no-autenticados activo");
    Ok(())
}

/// Reconstruye sesiones hotspot desde disco y respawnea los interim tasks.
/// Se llama DESPUES de init_hotspot_nft (necesita el set nft para validar).
fn restore_hotspot_sessions() {
    let n = handlers::hotspot::restore_and_spawn_interims();
    if n > 0 {
        println!("  🔄 {} sesiones hotspot reconstruidas (interim respawneado)", n);
    }
}
