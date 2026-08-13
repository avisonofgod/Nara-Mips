//! Normalización de nombres de interfaz para RiverOs/NARA en MikroTik hEX.
//!
//! Las interfaces del sistema ya se renombraron a nivel kernel a la forma
//! neutra `ethX` (ver scripts/rename-ports-openwrt.sh):
//!   - `eth0` = ether1 (WAN/consola, 192.168.5.1)
//!   - `eth1` = ether2
//!   - `eth2` = ether3
//!   - `eth3` = ether4
//!   - `eth4` = ether5
//!   - `sw0`  = puerto CPU del switch (interno, NO es un puerto físico)
//!
//! Con esto el sistema YA entrega ethX: display_name es identidad y solo se
//! oculta el puerto CPU `sw0`.

/// Devuelve el nombre de presentación para una interfaz del sistema.
/// Con el rename a nivel kernel, los nombres ya son ethX — identidad.
/// Los slaves DSA se ven como "eth1@sw0" (sufijo = master); limpiar el
/// sufijo para mostrar solo ethX.
pub fn display_name(real: &str) -> String {
    real.split('@').next().unwrap_or(real).to_string()
}

/// Indica si la interfaz es el puerto CPU del switch (debe ocultarse del
/// listado). Tras el rename, el cpu port es `sw0` (antes era `eth0`).
pub fn is_cpu_port(real: &str) -> bool {
    real == "sw0"
}

/// Invierte el mapeo: nombre de presentación -> nombre real. Con el rename
/// a nivel kernel son el mismo nombre.
pub fn real_name(display: &str) -> String {
    display.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eth0_es_puerto_fisico() {
        assert_eq!(display_name("eth0"), "eth0");
        assert!(!is_cpu_port("eth0"));
    }

    #[test]
    fn sw0_es_cpu_port() {
        assert!(is_cpu_port("sw0"));
        assert_eq!(display_name("sw0"), "sw0");
    }

    #[test]
    fn nombres_no_fisicos_pasan() {
        assert_eq!(display_name("lo"), "lo");
        assert_eq!(display_name("wg0"), "wg0");
        assert_eq!(display_name("br-lan"), "br-lan");
        assert!(!is_cpu_port("lo"));
    }

    #[test]
    fn real_name_es_identidad() {
        assert_eq!(real_name("eth0"), "eth0");
        assert_eq!(real_name("eth4"), "eth4");
    }

    #[test]
    fn slaves_dsa_con_sufijo_se_limpian() {
        // Tras el rename, los puertos aparecen como eth1@sw0 (slave DSA)
        assert_eq!(display_name("eth1@sw0"), "eth1");
        assert_eq!(display_name("eth4@sw0"), "eth4");
        assert!(!is_cpu_port("eth1@sw0"));
    }
}
