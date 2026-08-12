use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Serialize, Deserialize, Clone)]
pub struct Script {
    name: String,
    path: String,
    owner: String,
    policy: String,
    last_run: String,
    runs: u64,
    src: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SchedulerTask {
    name: String,
    start: String,
    interval: String,
    event: String,
    policy: String,
    owner: String,
    runs: u64,
    status: String,
}

pub async fn info() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(|| {
    let uptime = Command::new("uptime")
        .arg("-p")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let hostname = Command::new("hostname")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    // FIX S1 (2026-08-04): datos REALES del sistema (los mocks del frontend
    // mostraban CPU/Mem/Disk/NTP/usuarios inventados). Todo se lee en vivo.
    let sh = |cmd: &str| {
        // P2: bash→sh (Alpine mínimo sin bash; los comandos son POSIX)
        Command::new("sh").args(["-c", cmd]).output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    let time = sh("date '+%Y-%m-%d %H:%M:%S %Z'");
    let kernel = sh("uname -r");
    let arch = sh("uname -m");
    let cpu_model = sh("grep -m1 'model name' /proc/cpuinfo | sed 's/.*: //'");
    let cpu_cores = sh("nproc");
    let load = sh("cat /proc/loadavg");
    let load_parts: Vec<&str> = load.split_whitespace().collect();
    let load1 = load_parts.first().unwrap_or(&"").to_string();
    let load5 = load_parts.get(1).unwrap_or(&"").to_string();
    let load15 = load_parts.get(2).unwrap_or(&"").to_string();
    let mem = sh("free -m | awk 'NR==2 {print $2, $3, $7}'");
    let mem_parts: Vec<&str> = mem.split_whitespace().collect();
    let mem_total_mb = mem_parts.first().unwrap_or(&"0").to_string();
    let mem_used_mb = mem_parts.get(1).unwrap_or(&"0").to_string();
    let mem_free_mb = mem_parts.get(2).unwrap_or(&"0").to_string();
    let disk = sh("df -P / | awk 'NR==2 {print $2, $3, $4}'");
    let disk_parts: Vec<&str> = disk.split_whitespace().collect();
    // df -P devuelve KB -> convertir a BYTES (frontend usa formatBytes)
    let kb2b = |s: &str| -> u64 { s.parse::<u64>().unwrap_or(0) * 1024 };
    let disk_total_bytes = disk_parts.first().map(|s| kb2b(s)).unwrap_or(0).to_string();
    let disk_used_bytes = disk_parts.get(1).map(|s| kb2b(s)).unwrap_or(0).to_string();
    let disk_free_bytes = disk_parts.get(2).map(|s| kb2b(s)).unwrap_or(0).to_string();
    let timezone = sh("cat /etc/timezone 2>/dev/null || readlink -f /etc/localtime | sed 's|.*zoneinfo/||'");
    let ntp_servers: Vec<String> = sh("grep -E '^(server|pool)' /etc/ntp.conf 2>/dev/null | awk '{print $2}'")
        .lines().map(|s| s.to_string()).filter(|s| !s.is_empty()).collect();
    let procs = sh("ps -e | wc -l");
    let users: Vec<serde_json::Value> = sh("awk -F: '{print $1, $3, $6, $7}' /etc/passwd")
        .lines().filter(|l| !l.is_empty()).take(40)
        .map(|l| {
            let p: Vec<&str> = l.splitn(4, ' ').collect();
            serde_json::json!({
                "name": p.first().unwrap_or(&""),
                "uid": p.get(1).unwrap_or(&""),
                "home": p.get(2).unwrap_or(&""),
                "shell": p.get(3).unwrap_or(&"")
            })
        }).collect();
    let files: Vec<serde_json::Value> = sh("ls -la /etc/zpot/*.json 2>/dev/null | awk '{print $5, $9}'")
        .lines().filter(|l| !l.is_empty())
        .map(|l| {
            let p: Vec<&str> = l.splitn(2, ' ').collect();
            serde_json::json!({
                "size": p.first().unwrap_or(&"0"),
                "name": p.get(1).unwrap_or(&"").rsplit('/').next().unwrap_or("")
            })
        }).collect();

    Ok(Json(serde_json::json!({
        "hostname": hostname,
        "uptime": uptime,
        "version": "Zpot-RS 0.1.0",
        "time": time,
        "kernel": kernel,
        "arch": arch,
        "cpu_model": cpu_model,
        "cpu_cores": cpu_cores,
        "load1": load1, "load5": load5, "load15": load15,
        "mem_total_mb": mem_total_mb, "mem_used_mb": mem_used_mb, "mem_free_mb": mem_free_mb,
        "disk_total_bytes": disk_total_bytes, "disk_used_bytes": disk_used_bytes, "disk_free_bytes": disk_free_bytes,
        "timezone": timezone,
        "ntp_servers": ntp_servers,
        "procs": procs,
        "users": users,
        "files": files
    })))
    }).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("spawn_blocking: {}", e)))?
}

/// GET /api/system/logs — ultimas 50 lineas del log real del sistema
pub async fn logs_list() -> Result<Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let output = tokio::process::Command::new("sh")
        .args(["-c", "tail -n 100 /var/log/messages 2>/dev/null | grep -iE 'pppd|pppoe|zpot|watchdog' || tail -n 100 /var/log/messages 2>/dev/null"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut logs = Vec::new();
    for line in text.lines().rev().take(100) {
        if line.is_empty() { continue; }
        logs.push(serde_json::json!({"line": line}));
    }
    Ok(Json(logs))
}

/// GET /api/ppp/logs/auth — eventos de AUTENTICACIÓN RADIUS/PPP:
/// accept, reject, errores, timeout, asignación de IP (revisa.md #9).
pub async fn logs_auth() -> Result<Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let output = tokio::process::Command::new("sh")
        .args(["-c", "grep -iE 'pppd|radius' /var/log/messages 2>/dev/null | grep -iE 'auth|accept|reject|failed|timeout|MSCHAP|CHAP|PAP|ip-up|ip-down|LCP|Remote message' | tail -n 60"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut logs = Vec::new();
    for line in text.lines().rev().take(60) {
        if line.is_empty() { continue; }
        logs.push(serde_json::json!({"line": line}));
    }
    Ok(Json(logs))
}

/// GET /api/system/scripts — lista scripts disponibles del watchdog y systema
pub async fn scripts_list() -> Result<Json<Vec<Script>>, (StatusCode, String)> {
    let mut scripts = Vec::new();

    // 1. Scripts del watchdog en /usr/local/bin/
    if let Ok(entries) = std::fs::read_dir("/usr/local/bin") {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".sh") && name.starts_with("ppp-") {
                    // Leer primeras lineas del script para descripcion
                    let src = std::fs::read_to_string(&path).unwrap_or_default();
                    let first_line = src.lines().next().unwrap_or("").to_string();
                    let src_preview = if src.chars().count() > 80 {
                        format!("{}...", src.chars().take(80).collect::<String>())
                    } else {
                        src.clone()
                    };

                    scripts.push(Script {
                        name: name.to_string(),
                        path: path.to_string_lossy().to_string(),
                        owner: "root".to_string(),
                        policy: "write".to_string(),
                        last_run: "-".to_string(),
                        runs: count_cron_runs(name),
                        src: first_line,
                    });
                }
            }
        }
    }

    // 2. Scripts del proyecto en /root/zpot-rs/scripts/
    // Saltar si ya existe en /usr/local/bin/ (evitar duplicados)
    let existing_names: std::collections::HashSet<String> = scripts.iter().map(|s| s.name.clone()).collect();

    if let Ok(entries) = std::fs::read_dir("/root/zpot-rs/scripts") {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".sh") {
                    // Saltar si ya esta listado desde /usr/local/bin/
                    if existing_names.contains(name) {
                        continue;
                    }
                    let src = std::fs::read_to_string(&path).unwrap_or_default();
                    let first_line = src.lines().next().unwrap_or("").to_string();
                    let src_preview = if src.chars().count() > 80 {
                        format!("{}...", src.chars().take(80).collect::<String>())
                    } else {
                        src.clone()
                    };

                    scripts.push(Script {
                        name: name.to_string(),
                        path: path.to_string_lossy().to_string(),
                        owner: "root".to_string(),
                        policy: "write".to_string(),
                        last_run: "-".to_string(),
                        runs: count_cron_runs(name),
                        src: first_line,
                    });
                }
            }
        }
    }

    Ok(Json(scripts))
}

/// GET /api/system/scheduler — lista tareas de cron desde /etc/crontabs/root
pub async fn scheduler_list() -> Result<Json<Vec<SchedulerTask>>, (StatusCode, String)> {
    let mut tasks = Vec::new();

    // Leer crontab de root
    let crontab = std::fs::read_to_string("/etc/crontabs/root").unwrap_or_default();

    for line in crontab.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parsear cron: minuto hora dia-mes mes dia-sem comando
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }

        let cron_expr = parts[..5].join(" ");
        let command = parts[5..].join(" ");
        let cmd_name = command
            .rsplit('/')
            .next()
            .unwrap_or(&command)
            .trim_end_matches(".sh")
            .to_string();

        // Convertir expresion cron a intervalo legible
        let interval = cron_to_interval(&cron_expr);

        // Status: siempore enabled en crontabs (no hay disabled en crontab nativo)
        let status = "enabled";

        tasks.push(SchedulerTask {
            name: cmd_name,
            start: cron_to_start(&cron_expr),
            interval,
            event: "none".to_string(),
            policy: "write".to_string(),
            owner: "root".to_string(),
            runs: count_cron_runs(&command),
            status: status.to_string(),
        });
    }

    Ok(Json(tasks))
}

fn cron_to_interval(expr: &str) -> String {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() < 5 {
        return expr.to_string();
    }

    let min = parts[0];
    let hour = parts[1];
    let dom = parts[2];
    let month = parts[3];
    let dow = parts[4];

    // */2 * * * * → cada 2 min
    if min.starts_with("*/") && hour == "*" && dom == "*" && month == "*" && dow == "*" {
        return format!("{}min", &min[2..]);
    }
    // */5 * * * * → cada 5 min
    if min.starts_with("*/") && hour == "*" && dom == "*" && month == "*" && dow == "*" {
        return format!("{}min", &min[2..]);
    }
    // Otras combinaciones
    if hour == "*" && min.starts_with("*/") {
        return format!("{}min", &min[2..]);
    }
    if min != "*" && hour != "*" && dom == "*" && month == "*" && dow == "*" {
        return "1d".to_string();
    }
    expr.to_string()
}

fn cron_to_start(expr: &str) -> String {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() < 2 {
        return "00:00".to_string();
    }
    let min = parts[0];
    let hour = parts[1];

    if hour == "*" && min.starts_with("*/") {
        return "00:00".to_string();
    }
    if hour == "*" {
        return "00:00".to_string();
    }
    format!("{:0>2}:{:0>2}", hour, min)
}

fn count_cron_runs(name: &str) -> u64 {
    // Contar entradas en logs del watchdog
    if name.contains("watchdog") {
        let log = std::fs::read_to_string("/var/log/messages").unwrap_or_default();
        let count = log.matches("ppp-watchdog: ZOMBIE:").count();
        return count as u64;
    }
    0
}

// ─────────────────────────────────────────────────────────────────────────
// SPEEDTEST DE WANS (2026-08-08)
// POST /api/system/speedtest {wan: "wan1"|"wan2", n: 1..10 (default 3)}
//
// Mide la capacidad REAL de cada WAN con el CLI oficial de Ookla
// (multi-stream; el speedtest-cli python usa 1 stream y con latencia alta
// subestima). Tecnica:
//   1. Lee la IP de la WAN del store MWAN (fuente de verdad, no hardcode)
//   2. Crea una regla ip rule temporal "from <wan_ip> lookup <tabla_wan>
//      pref 30000" (pref ALTO: despues de las reglas fwmark 1401/1402 del
//      balanceo — NO toca el trafico NAT de clientes; pref < main 32766
//      para que el trafico local sin fwmark use la tabla de la WAN)
//   3. Ejecuta el binario con -i <wan_ip> (bind) N veces
//   4. BORRA la regla SIEMPRE (closure + borrado posterior, aunque falle)
//   5. Media de las rondas + historial en /etc/zpot/speedtest-history.json
//      (max 50 muestras por WAN, escritura atomica tmp+rename)
// El binario: /usr/local/bin/speedtest (Ookla 1.2.0 musl) — instalado en
// Alpine; documentado en docs/SPEEDTEST-WANS.md.
// ─────────────────────────────────────────────────────────────────────────
const SPEEDTEST_BIN: &str = "/usr/local/bin/speedtest";
const SPEEDTEST_HISTORY: &str = "/etc/zpot/speedtest-history.json";
const SPEEDTEST_LIMIT: usize = 50;

fn speedtest_history_wan(wan: &str) -> (serde_json::Value, Vec<serde_json::Value>) {
    // (historia completa, arreglo de esa WAN) — lectura tolerante a fallos
    let hist: serde_json::Value = std::fs::read_to_string(SPEEDTEST_HISTORY)
        .ok().and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({"wan1": [], "wan2": []}));
    let arr = hist.get(wan).and_then(|a| a.as_array()).cloned().unwrap_or_default();
    (hist, arr)
}

fn run_speedtest_blocking(wan: String, n: usize) -> Result<serde_json::Value, String> {
    // IP/tabla/iface de la WAN desde el store MWAN (fuente de verdad)
    let (ip, iface) = {
        let st = crate::handlers::mwan::store().state.lock().unwrap_or_else(|e| e.into_inner());
        match st.wans.get(&wan) {
            Some(w) => (w.ip.clone(), w.iface.clone()),
            None => return Err(format!("WAN '{}' no configurada en MWAN", wan)),
        }
    };
    let table = wan.clone();

    // Regla temporal: SOLO el trafico local con source = ip_wan (sin fwmark)
    // cae en la tabla de la WAN; el trafico de clientes (fwmark 1401/1402)
    // no se toca. pref 30000: despues de las reglas MWAN, antes de main.
    let add = Command::new("ip")
        .args(["rule", "add", "from", &ip, "lookup", &table, "pref", "30000"])
        .output()
        .map_err(|e| format!("ip rule add: {}", e))?;
    if !add.status.success() {
        return Err(format!("ip rule add fallo: {}", String::from_utf8_lossy(&add.stderr)));
    }

    // Closure: ejecuta las N rondas y calcula media+historial. La regla se
    // borra DESPUES del closure pase lo que pase (no hay early-return).
    let result: Result<serde_json::Value, String> = (|| {
        let mut rondas: Vec<serde_json::Value> = Vec::new();
        for _ in 0..n {
            let out = Command::new(SPEEDTEST_BIN)
                .args(["-i", &ip, "--accept-license", "--accept-gdpr", "-f", "json"])
                .output()
                .map_err(|e| format!("speedtest: {}", e))?;
            if !out.status.success() {
                return Err(format!("speedtest fallo: {}", String::from_utf8_lossy(&out.stderr)));
            }
            let d: serde_json::Value = serde_json::from_slice(&out.stdout)
                .map_err(|e| format!("respuesta speedtest invalida: {}", e))?;
            let down = d["download"]["bandwidth"].as_f64().unwrap_or(0.0) * 8.0 / 1e6;
            let up = d["upload"]["bandwidth"].as_f64().unwrap_or(0.0) * 8.0 / 1e6;
            let ping = d["ping"]["latency"].as_f64().unwrap_or(0.0);
            let srv = format!("{} ({}, {})",
                d["server"]["name"].as_str().unwrap_or(""),
                d["server"]["location"].as_str().unwrap_or(""),
                d["server"]["country"].as_str().unwrap_or(""));
            rondas.push(serde_json::json!({
                "down": (down * 100.0).round() / 100.0,
                "up": (up * 100.0).round() / 100.0,
                "ping": (ping * 10.0).round() / 10.0,
                "server": srv,
            }));
        }

        // Media de las rondas
        let nf = n as f64;
        let avg = |k: &str| rondas.iter()
            .filter_map(|r| r[k].as_f64())
            .sum::<f64>() / nf;
        let media = serde_json::json!({
            "down": (avg("down") * 100.0).round() / 100.0,
            "up": (avg("up") * 100.0).round() / 100.0,
            "ping": (avg("ping") * 10.0).round() / 10.0,
        });

        // Historial: append + media historica (atomic tmp+rename)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let (mut hist, mut arr) = speedtest_history_wan(&wan);
        let media_r = media.clone();
        arr.push(serde_json::json!({
            "ts": now,
            "down": media_r["down"],
            "up": media_r["up"],
            "ping": media_r["ping"],
        }));
        if arr.len() > SPEEDTEST_LIMIT {
            let cut = arr.len() - SPEEDTEST_LIMIT;
            arr.drain(0..cut);
        }
        if let Some(a) = hist.get_mut(&wan).and_then(|a| a.as_array_mut()) {
            *a = arr.clone();
        }
        let tmp = format!("{}.tmp-{}", SPEEDTEST_HISTORY, std::process::id());
        if let Ok(s) = serde_json::to_string_pretty(&hist) {
            let _ = std::fs::write(&tmp, &s);
            let _ = std::fs::rename(&tmp, SPEEDTEST_HISTORY);
        }
        let hn = arr.len() as f64;
        let hist_avg = |k: &str| arr.iter()
            .filter_map(|r| r[k].as_f64())
            .sum::<f64>() / hn.max(1.0);
        let historial = serde_json::json!({
            "muestras": arr.len(),
            "media_down": (hist_avg("down") * 100.0).round() / 100.0,
            "media_up": (hist_avg("up") * 100.0).round() / 100.0,
        });

        Ok(serde_json::json!({
            "ok": true,
            "wan": wan,
            "iface": iface,
            "ip": ip,
            "rondas": rondas,
            "media": media,
            "historial": historial,
        }))
    })();

    // BORRAR la regla SIEMPRE (aunque el closure haya fallado)
    let _ = Command::new("ip")
        .args(["rule", "del", "from", &ip, "lookup", &table, "pref", "30000"])
        .output();

    result
}

pub async fn speedtest_run(Json(body): Json<serde_json::Value>) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static BUSY: AtomicBool = AtomicBool::new(false);

    let wan = body.get("wan").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let n = body.get("n").and_then(|v| v.as_u64()).unwrap_or(3).clamp(1, 10) as usize;
    // FIX (2026-08-08): multiwan — aceptar CUALQUIER WAN configurada en el
    // store MWAN (antes solo wan1/wan2 hardcodeado).
    {
        let st = crate::handlers::mwan::store().state.lock().unwrap_or_else(|e| e.into_inner());
        if !st.wans.contains_key(&wan) {
            let disponibles: Vec<String> = st.wans.keys().cloned().collect();
            return Err((StatusCode::BAD_REQUEST, format!(
                "wan '{}' no configurada en MWAN. Disponibles: {}", wan, disponibles.join(", "))));
        }
    }
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err((StatusCode::CONFLICT, "speedtest ya en ejecucion (espera a que termine)".into()));
    }

    let result = tokio::task::spawn_blocking(move || run_speedtest_blocking(wan, n))
        .await
        .unwrap_or_else(|e| Err(format!("task speedtest cancelado: {}", e)));

    BUSY.store(false, Ordering::SeqCst);
    match result {
        Ok(v) => Ok(Json(v)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

// ─────────────────────────────────────────────────────────────────────────
// FILES / EXPORT / IMPORT / HOTSPOT BACKUP (2026-08-08)
// GET  /api/system/files                 -> lista /etc/zpot/*.json
// GET  /api/system/export                -> JSON {components: {archivo: contenido}}
// POST /api/system/import (raw JSON)     -> restaura components con backup .bak-ts
// GET  /api/system/files/hotspot/download -> tar.gz de static/hotspot
// POST /api/system/files/hotspot/upload (raw tar.gz) -> respalda y extrae
// ─────────────────────────────────────────────────────────────────────────
const ZPOT_CFG_DIR: &str = "/etc/zpot";
const HOTSPOT_DIR: &str = "/root/zpot-rs/static/hotspot";

fn read_dir_json() -> Vec<(String, String, u64)> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(ZPOT_CFG_DIR) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") {
                if let Ok(content) = std::fs::read_to_string(format!("{}/{}", ZPOT_CFG_DIR, name)) {
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    out.push((name, content, size));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub async fn files_list() -> Json<serde_json::Value> {
    let files: Vec<serde_json::Value> = read_dir_json().into_iter()
        .map(|(name, _c, size)| serde_json::json!({"name": name, "size": size}))
        .collect();
    Json(serde_json::json!(files))
}

pub async fn export_config() -> Json<serde_json::Value> {
    let mut comps = serde_json::Map::new();
    for (name, content, _size) in read_dir_json() {
        comps.insert(name, serde_json::Value::String(content));
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    Json(serde_json::json!({
        "format": "zpot-config-1",
        "exported": ts,
        "components": comps,
    }))
}

pub async fn import_config(body: axum::body::Bytes) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let data: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("JSON invalido: {}", e)))?;
    let comps = data.get("components").and_then(|c| c.as_object())
        .ok_or((StatusCode::BAD_REQUEST, "falta components".into()))?;
    if comps.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "components vacio".into()));
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let mut restored = Vec::new();
    for (name, val) in comps {
        // Validar nombre seguro: solo [a-z0-9._-] y termina en .json
        if name.is_empty()
            || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
            || !name.ends_with(".json") {
            return Err((StatusCode::BAD_REQUEST, format!("nombre de archivo invalido: {}", name)));
        }
        let content = val.as_str().unwrap_or("");
        let path = format!("{}/{}", ZPOT_CFG_DIR, name);
        // backup del actual (si existe)
        if std::path::Path::new(&path).exists() {
            let _ = std::fs::copy(&path, format!("{}.bak-{}", path, ts));
        }
        // escritura atomica
        let tmp = format!("{}.tmp-{}", path, std::process::id());
        if std::fs::write(&tmp, content.as_bytes()).is_err() {
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("no se pudo escribir {}", name)));
        }
        if std::fs::rename(&tmp, &path).is_err() {
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("no se pudo reemplazar {}", name)));
        }
        restored.push(name.clone());
    }
    Ok(Json(serde_json::json!({"status": "ok", "restored": restored, "backup_ts": ts})))
}

pub async fn hotspot_download() -> Result<axum::response::Response, (StatusCode, String)> {
    let tmp = format!("/tmp/hotspot-export-{}.tar.gz", std::process::id());
    let out = Command::new("tar")
        .args(["czf", &tmp, "-C", "/root/zpot-rs/static", "hotspot"])
        .output()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !out.status.success() {
        return Err((StatusCode::INTERNAL_SERVER_ERROR,
            format!("tar fallo: {}", String::from_utf8_lossy(&out.stderr))));
    }
    let bytes = std::fs::read(&tmp).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let _ = std::fs::remove_file(&tmp);
    axum::response::Response::builder()
        .header("content-type", "application/gzip")
        .header("content-disposition", "attachment; filename=\"hotspot.tar.gz\"")
        .body(axum::body::Body::from(bytes))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub async fn hotspot_upload(body: axum::body::Bytes) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if body.len() < 100 {
        return Err((StatusCode::BAD_REQUEST, "el archivo parece vacio o no es un tar.gz".into()));
    }
    let tmp = format!("/tmp/hotspot-upload-{}.tar.gz", std::process::id());
    std::fs::write(&tmp, &body).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let bak = format!("/root/zpot-rs/static/hotspot.bak-{}", ts);
    // Respaldar el dir actual (rename) y recrear
    let _ = std::fs::rename(HOTSPOT_DIR, &bak);
    let _ = std::fs::create_dir_all(HOTSPOT_DIR);
    let out = Command::new("tar")
        .args(["xzf", &tmp, "-C", "/root/zpot-rs/static"])
        .output()
        .map_err(|e| {
            let _ = std::fs::rename(&bak, HOTSPOT_DIR); // rollback
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
    if !out.status.success() {
        // rollback: restaurar el dir original
        let _ = std::fs::remove_dir_all(HOTSPOT_DIR);
        let _ = std::fs::rename(&bak, HOTSPOT_DIR);
        let _ = std::fs::remove_file(&tmp);
        return Err((StatusCode::BAD_REQUEST,
            format!("tar invalido (debe contener hotspot/): {}", String::from_utf8_lossy(&out.stderr))));
    }
    let _ = std::fs::remove_file(&tmp);
    Ok(Json(serde_json::json!({"status": "ok", "backup": format!("hotspot.bak-{}", ts)})))
}
