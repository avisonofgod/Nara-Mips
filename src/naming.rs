//! Normalización de nombres de interfaz para RiverOs/NARA en MikroTik hEX.
//!
//! OpenWrt (25.12 DSA) nombra los puertos físicos del hEX RB750Gr3 como:
//!   - `wan`        -> ether1 (el único puerto con IP de consola)
//!   - `lan2..lan5` -> ether2..ether5
//!   - `eth0`       -> puerto CPU del switch (interno, NO es un puerto físico)
//!
//! NARA presenta los puertos de forma uniforme y neutra:
//!   - `eth0` = wan (ether1)   — consola, IP 192.168.5.1
//!   - `eth1` = lan2 (ether2)
//!   - `eth2` = lan3 (ether3)
//!   - `eth3` = lan4 (ether4)
//!   - `eth4` = lan5 (ether5)
//!
//! El puerto CPU `eth0` del switch se oculta del listado (no es cableable).

/// Devuelve el nombre de presentación (`ethX`) para un nombre DSA de OpenWrt.
/// Si el nombre no es un puerto físico, devuelve el mismo nombre (lo, wg*, ppp*, br*).
pub fn display_name(real: &str) -> String {
    match real {
        "wan" => "eth0".to_string(),
        "lan2" => "eth1".to_string(),
        "lan3" => "eth2".to_string(),
        "lan4" => "eth3".to_string(),
        "lan5" => "eth4".to_string(),
        other => other.to_string(),
    }
}

/// Indica si la interfaz es el puerto CPU del switch (debe ocultarse del listado).
pub fn is_cpu_port(real: &str) -> bool {
    real == "eth0"
}

/// Invierte el mapeo: nombre de presentación `ethX` -> nombre DSA real.
pub fn real_name(display: &str) -> String {
    match display {
        "eth0" => "wan".to_string(),
        "eth1" => "lan2".to_string(),
        "eth2" => "lan3".to_string(),
        "eth3" => "lan4".to_string(),
        "eth4" => "lan5".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wan_es_eth0() {
        assert_eq!(display_name("wan"), "eth0");
        assert_eq!(real_name("eth0"), "wan");
    }

    #[test]
    fn lan2_es_eth1() {
        assert_eq!(display_name("lan2"), "eth1");
        assert_eq!(real_name("eth1"), "lan2");
    }

    #[test]
    fn cpu_port_se_oculta() {
        assert!(is_cpu_port("eth0"));
        assert!(!is_cpu_port("wan"));
    }

    #[test]
    fn nombres_no_fisicos_pasan() {
        assert_eq!(display_name("wg0"), "wg0");
        assert_eq!(display_name("br-lan"), "br-lan");
        assert!(!is_cpu_port("lo"));
    }
}
