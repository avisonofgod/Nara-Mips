#!/sbin/openrc-run
# ZPOT-RS — Hotspot ISP Manager
description="ZPOT-RS Hotspot Manager"

name="zpot"
command="/root/zpot-rs/target/release/zpot"
command_background=true
pidfile="/run/${RC_SVCNAME}.pid"
command_user="root"

depend() {
    need net
    after firewall
}

start_pre() {
    ebegin "Configuring nftables hotspot..."
    # Crear tabla hotspot si no existe
    nft add table inet hotspot 2>/dev/null || true
    # FIX 2026-08-04: set CONCATENADO (ipv4_addr . ether_addr) — el viejo
    # (type ipv4_addr a secas) era incompatible con el set que crea zpot.
    nft add set inet hotspot hotspot_auth { type ipv4_addr . ether_addr\; flags timeout\; timeout 24h\; } 2>/dev/null || true
    
    # Crear chains si no existen
    nft add chain inet hotspot prerouting { type nat hook prerouting priority dstnat\; policy accept\; } 2>/dev/null || true
    nft add chain inet hotspot forward { type filter hook forward priority filter\; policy accept\; } 2>/dev/null || true
    nft add chain inet hotspot postrouting { type nat hook postrouting priority srcnat\; policy accept\; } 2>/dev/null || true
    
    # Reglas (idempotentes)
    nft add rule inet hotspot prerouting iif "eth3" ip saddr . ether saddr @hotspot_auth return 2>/dev/null || true
    # FIX 2026-08-04: redirect a :80 (el portal real) — antes :8080 (muerto)
    nft add rule inet hotspot prerouting iif "eth3" tcp dport 80 redirect to :80 2>/dev/null || true
    # FIX 2026-08-04: FAIL-CLOSED — si zpot no arranca, el hotspot queda SIN
    # internet (antes policy accept sin drop final = internet ABIERTO sin auth).
    
    # FIX (2026-08-04): la regla de autenticados usaba `ip saddr @hotspot_auth`
    # que es INVALIDO contra el set concatenado (ipv4_addr.ether_addr) — nft
    # fallaba silenciosamente (|| true) y la regla NUNCA se aplicaba. La forma
    # valida es `ip saddr . ether saddr @hotspot_auth` (como la del prerouting).
    nft add rule inet hotspot forward iif "eth3" ip saddr . ether saddr @hotspot_auth accept 2>/dev/null || true
    nft add rule inet hotspot forward iif "eth3" udp dport { 67, 68 } accept 2>/dev/null || true
    nft add rule inet hotspot forward iif "eth3" udp dport 53 accept 2>/dev/null || true
    nft add rule inet hotspot forward iif "eth3" tcp dport 53 accept 2>/dev/null || true
    nft add rule inet hotspot forward iif "eth3" tcp dport 80 accept 2>/dev/null || true
    # FIX (2026-08-04): DROP FINAL — si zpot no arranca (crash loop) o en la
    # ventana entre este script y el init de zpot, los clientes eth3 NO
    # autenticados quedan SIN internet (FAIL-CLOSED real). ANTES la chain
    # forward tenia policy accept sin drop final = internet ABIERTO sin auth.
    nft add rule inet hotspot forward iif "eth3" drop 2>/dev/null || true

    # Masquerade para TODAS las WANs (eth0 + eth1)
    nft add rule inet hotspot postrouting oif "eth0" masquerade 2>/dev/null || true
    nft add rule inet hotspot postrouting oif "eth1" masquerade 2>/dev/null || true
    
    eend $? "nftables hotspot configurado"
    
    ebegin "Starting dnsmasq..."
    start-stop-daemon --start --exec /usr/sbin/dnsmasq -- --no-daemon --interface=eth3 --dhcp-range=192.168.10.100,192.168.10.200,255.255.255.0,12h --dhcp-option=3,192.168.10.1 --dhcp-option=6,8.8.8.8 --port=0 2>/dev/null || true
    eend $? "dnsmasq iniciado"
}

stop_post() {
    ebegin "Stopping dnsmasq..."
    pkill dnsmasq 2>/dev/null || true
    eend $?
}
