//! Helpers de compatibilidad Alpine/OpenWrt (RiverOs).
//!
//! NARA se origino en Alpine (rc-service, getent, conntrack binario).
//! RiverOs es OpenWrt 25.12.5 (busybox+apk, /etc/init.d, sin getent,
//! /proc/net/nf_conntrack disponible). Estos helpers detectan el entorno
//! y usan el mecanismo correcto.

use std::process::Stdio;

/// Ejecuta un comando solo si el binario existe (evita errores en entornos
/// donde el paquete no esta instalado).
pub fn has_binary(name: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {} >/dev/null 2>&1", name)])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Lee el estado de conntrack: usa el binario `conntrack -L` si existe,
/// si no lee /proc/net/nf_conntrack (mismo formato de columnas).
/// Devuelve (ok, texto).
pub async fn conntrack_lines() -> (bool, String) {
    if has_binary("conntrack") {
        if let Ok(out) = tokio::process::Command::new("conntrack")
            .args(["-L"])
            .output()
            .await
        {
            if out.status.success() {
                return (true, String::from_utf8_lossy(&out.stdout).to_string());
            }
        }
    }
    // Fallback: /proc/net/nf_conntrack (kernel, sin conntrack-tools)
    match tokio::fs::read_to_string("/proc/net/nf_conntrack").await {
        Ok(text) => (true, text),
        Err(_) => (false, String::new()),
    }
}

/// Flush de conntrack para una IP. No-op si conntrack-tools no existe
/// (el corte real lo hace nft; el flush es solo optimizacion).
pub async fn conntrack_flush(ip: &str) {
    if has_binary("conntrack") {
        let _ = tokio::process::Command::new("conntrack")
            .args(["-D", "-s", ip])
            .output()
            .await;
    }
}

/// Resuelve un dominio a IPv4. Usa getent (Alpine) si existe; si no,
/// nslookup de busybox (OpenWrt); ultimo recurso: /etc/hosts.
pub async fn resolve_ipv4(domain: &str) -> Option<String> {
    // 1. /etc/hosts directo (sin DNS)
    if let Ok(hosts) = tokio::fs::read_to_string("/etc/hosts").await {
        for line in hosts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[1] == domain {
                if parts[0].parse::<std::net::Ipv4Addr>().is_ok() {
                    return Some(parts[0].to_string());
                }
            }
        }
    }
    // 2. getent (Alpine)
    if has_binary("getent") {
        if let Ok(out) = tokio::process::Command::new("getent")
            .args(["ahostsv4", domain])
            .output()
            .await
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(ip) = text.split_whitespace().next() {
                if ip.parse::<std::net::Ipv4Addr>().is_ok() {
                    return Some(ip.to_string());
                }
            }
        }
    }
    // 3. nslookup (busybox OpenWrt)
    if has_binary("nslookup") {
        if let Ok(out) = tokio::process::Command::new("nslookup")
            .arg(domain)
            .output()
            .await
        {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                // busybox nslookup: "Address: 1.2.3.4"
                if let Some(pos) = line.find("Address:") {
                    let rest = line[pos + 8..].trim();
                    if let Some(ip) = rest.split_whitespace().next() {
                        if ip.parse::<std::net::Ipv4Addr>().is_ok() {
                            return Some(ip.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// Accion de servicio: rc-service (Alpine) o /etc/init.d/<svc> <accion>
/// (OpenWrt). Devuelve ok.
pub async fn service_action(svc: &str, action: &str) -> bool {
    let res = if has_binary("rc-service") {
        tokio::process::Command::new("rc-service")
            .args([svc, action])
            .output()
            .await
    } else {
        tokio::process::Command::new(format!("/etc/init.d/{}", svc))
            .arg(action)
            .output()
            .await
    };
    match res {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

/// Igual que service_action pero devuelve un std::process::Output sintetico
/// (compatible con el codigo que esperaba `Command::new("rc-service").output()`).
pub async fn service_action_output(svc: &str, action: &str) -> std::io::Result<std::process::Output> {
    let ok = service_action(svc, action).await;
    Ok(synthetic_output(ok))
}

fn synthetic_output(ok: bool) -> std::process::Output {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(if ok { 0 } else { 1 }),
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }
    #[cfg(not(unix))]
    {
        std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }
}

/// Version sincrona del flush de conntrack (para spawn_blocking).
pub fn conntrack_flush_sync(ip: &str) {
    if has_binary("conntrack") {
        let _ = std::process::Command::new("conntrack")
            .args(["-D", "-s", ip])
            .output();
    }
}

/// Version sincrona (para usar dentro de spawn_blocking).
pub fn service_action_sync(svc: &str, action: &str) -> bool {
    let res = if has_binary("rc-service") {
        std::process::Command::new("rc-service")
            .args([svc, action])
            .output()
    } else {
        std::process::Command::new(format!("/etc/init.d/{}", svc))
            .arg(action)
            .output()
    };
    match res {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}
