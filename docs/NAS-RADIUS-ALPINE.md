# Zpot-RS — NAS RADIUS Alpine (estado REAL del sistema)

> Documento de referencia 2026-08-08. Verificado contra Alpine 10.7.0.5 en
> producción. **El sistema ES un NAS RADIUS**: Zpot-RS actúa como Network
> Access Server que autentica TODOS los accesos contra FreeRADIUS remoto
> (161.97.67.63). No hay autenticación local implementada ni deseada.

## Arquitectura (visión general)

```
                    FreeRADIUS 3 + daloRADIUS (161.97.67.63)
                    auth :1812  acct :1813  |  CoA poll HTTP :80
                          ^            ^             ^
                          |            |             |
  +-----------------------|------------|-------------|-------------------+
  |  ALPINE 10.7.0.5 = NAS RADIUS (Zpot-RS)                            |
  |                                                                     |
  |  HOTSPOT WiFi (eth3 .10.0/24)         PPPoE (eth3.881 .20.0/24)     |
  |  zpot = cliente RADIUS                pppd + radius.so + radattr.so |
  |  portal captivo :80                   conf /etc/radiusclient/       |
  |  Auth: Access-Request -> :1812        Auth: pppd -> :1812           |
  |  Acct: Interim -> :1813               VSAs -> QoS tc (ip-up)        |
  |  CoA: poll sessions.php               IPs fijas por Framed-IP       |
  +---------------------------------------------------------------------+
  |  DNS local: unbound :53   DHCP: dnsmasq :5353 (solo eth3)           |
  |  Gestión: wg0 10.7.0.5/32 (VPN) + API admin :8081 (WG/LAN)          |
  |  WANs: eth0 (.2.102) + eth1 (.3.105) con MWAN 50/50 (numgen)        |
  +---------------------------------------------------------------------+
```

## Componentes reales (verificados 2026-08-08)

| Componente | PID/puerto | Rol |
|---|---|---|
| zpot (Rust) | 26225 / :80 :8081 | Portal hotspot, API admin, RADIUS client, CoA poll, MWAN |
| pppoe-server | 3370 / eth3.881 | Terminador PPPoE, `-R 192.168.20.100 -N 100` (pool provisional) |
| pppd (x33) | por sesión | MSCHAPv2 + plugin radius.so + radattr.so |
| unbound | 3308 / :53 | Resolver DNS local (hotspot + LAN) |
| dnsmasq | 3249 / :5353 | DHCP solo eth3 (rango .10.2-.245, 12h), option 138 → RADIUS |
| wg0 | kernel / 51820 | Túnel de gestión 10.7.0.5/32 |
| nft | kernel | tabla `hotspot` + tabla `mwan` |

## Configs REALES (/etc/zpot — comparar con docs/config-examples/)

### hotspot-server.json
```json
{
  "iface": "eth3",
  "gw": "192.168.10.1",
  "html_dir": "/root/zpot-rs/static/hotspot",
  "idle_timeout": 600,
  "shared_users": 1,
  "rate_limit": "1M/2M 2M/3M",
  "radius": "161.97.67.63:1812",
  "radius_secret": "***",
  "coa_enabled": true,
  "coa_mode": "poll",
  "coa_poll_url": "http://161.97.67.63/zpot-coa/sessions.php?secret=***"
}
```
Notas: `radius_secret` y `coa_poll_url` se muestran enmascarados ("***") vía
GET /api/hotspot/config. El `rate_limit` es el fallback; las sesiones activas
usan la VSA de FreeRADIUS (ej. 5M/7M). Config UNO — no hay profiles.

### ppp-radius.json
```json
{
  "enabled": true,
  "server_name": "radius-main",
  "nas_identifier": "zpot-nas",
  "nas_ip": "192.168.20.1",
  "fallback_local": false,
  "accounting": false,
  "pool_start": "192.168.20.100",
  "pool_end": "192.168.20.200",
  "dns1": "192.168.20.1",
  "dns2": "8.8.8.8"
}
```
Notas: `fallback_local=false` → pppd SOLO RADIUS (requerido: MSCHAPv2).
`accounting=false` → el NAS NO manda accounting PPP (el de hotspot sí va a
:1813). `nas_ip` = IP de la subred PPPoE (Atributo NAS-IP-Address).

### radius-servers.json
VACÍO en producción. El módulo radius.rs usa `get_default_auth_server()`
fallback hardcodeado (161.97.67.63:1812) cuando no hay servidores
configurados. La UI PPP lo resuelve como "radius-main".

### mwan.json
```json
{
  "wans": {
    "wan1": {"iface": "eth0", "ip": "192.168.2.102", "gateway": "192.168.2.1", "status": "up", "table": 10, "mark": 1},
    "wan2": {"iface": "eth1", "ip": "192.168.3.105", "gateway": "192.168.3.1", "status": "up", "table": 20, "mark": 2}
  },
  "mode": "round-robin",
  "distribution": "50/50"
}
```
`status` en el JSON es informativo (la detección real es por ping al gateway
de cada WAN). El reparto real: `numgen random mod 100 map {0-49: mark2, 50-99: mark1}`.

## Configs reales de RADIUS del NAS (pppd)

### /etc/radiusclient/radiusclient.conf (NOTA: NO es radiusclient-ng)
```
auth_order     radius
authserver     161.97.67.63:1812
acctserver     161.97.67.63:1813
servers        /etc/radiusclient/servers
dictionary     /etc/radiusclient/dictionary
nas_identifier zpot-nas
radius_timeout 3
radius_retries 2
```
`auth_order radius` (sin local) confirma el modo NAS RADIUS puro.
El dictionary define la VSA Mikrotik (oui 14988, attr 8, string) usada para
Mikrotik-Rate-Limit → QoS.

### /etc/ppp/pppoe-server-options
```
require-mschap-v2
lcp-echo-interval 5
lcp-echo-failure 3
ms-dns 192.168.20.1
ms-dns 8.8.8.8
plugin radius.so
plugin radattr.so
```

### Flujo QoS (PPP)
1. pppd autentica contra FreeRADIUS; radattr.so escribe /var/run/radattr.pppN
2. ip-up (real) guarda MAC del peer y POST /api/ppp/qos/radius (curl --max-time 5)
3. zpot lee la VSA `Mikrotik-Rate-Limit` (formato `up/down ceil_up/ceil_down`,
   ej. `1M/4M 2M/5M`) y aplica tc: DOWN en pppN (clase 1:1048 rate 4M/ceil 5M),
   UP en ifb_pppN (1:1049 rate 1M/ceil 2M) + filtro u32 por IP
4. ip-down: POST /api/ppp/qos/cleanup + `ip link delete dev pppN` (anti-zombie)

## Flujo auth/acct

### Hotspot (zpot = cliente RADIUS)
- Cliente WiFi → DHCP (dnsmasq) → portal captivo (redirect :80) → login.html
- POST login → Access-Request a 161.97.67.63:1812 (secret compartido)
- Accept → inserta IP+MAC en set `hotspot_auth` (nft, timeout 1d) → navega
- Accounting interim a :1813 (radacct por IP origen)
- Desconexión: idle_timeout 600s controla; CoA en modo poll consulta
  sessions.php?secret=*** cada N segundos y expulsa sesiones que ya no están

### PPPoE (pppd + plugin radius.so)
- CPE → pppoe-server (eth3.881) → pppd → MSCHAPv2 → Access-Request :1812
- Accept con Framed-IP-Address → ip-up reemplaza la IP provisional del pool
  por la fija (.2-.37) y aplica QoS
- Accounting PPP: OFF (decidido — el portal hotspot sí acctea)

## Redes (verificadas)
- eth0 192.168.2.102/24 (wan1, default via .2.1 gestionada por MWAN)
- eth1 192.168.3.105/24 (wan2, gateway .3.1 en interfaces)
- eth2 192.168.30.1/24 (DOWN, sin cable)
- eth3 192.168.10.1/24 (hotspot; dnsmasq DHCP .10.2-.245; unbound :53)
- eth3.881 192.168.20.1/24 (PPPoE; 33 sesiones .2-.37 fijas + pool .100+ provisional)
- wg0 10.7.0.5/32 (gestión VPN, peer VPS 10.7.0.0/24)
- ip rules: 1401 fwmark 0x1 → tabla 10 (wan1), 1402 fwmark 0x2 → tabla 20 (wan2)

## nft — reglas clave

### tabla inet hotspot
- prerouting: eth3 dport 8081 drop; eth3 no-auth → tcp 80 redirect (portal);
  eth3 auth (IP+MAC en set) → return
- postrouting: oif eth1/eth0 masquerade (salida)
- forward: aislamiento eth3↔eth3 drop; eth3→{10.7.0.0/24, .20.0/24} drop;
  ppp*↔ppp* drop; DNS 53, DHCP 67/68, controladores UniFi/TPlink/Omada
  (18.213.142.156, 34.238.17.94, 44.193.125.236, 54.243.197.97) + RADIUS
  161.97.67.63 en el walled garden; 29810-29814, 8043/8088/8843, 3478,
  10001, 27001, NTP 123 permitidos
- input: 8081 solo {10.7.0.0/24, 192.168.2.0/23}; 80 solo {lo, wg0, eth3}

### tabla inet mwan
- prerouting: wg0→10.7.0.0/24 accept; ct new → mark numgen 50/50; ct established
  → marca desde ct mark
- postrouting: oif eth1 mark 0x2 masquerade; oif eth0 mark 0x1 masquerade

## Puertos
- 80 portal hotspot (restringido por nft)
- 8081 API admin (restringido WG/LAN)
- 5353 dnsmasq (DHCP)
- 53 unbound (DNS)
- 51820 WireGuard
- 1812/1813 salientes hacia FreeRADIUS (UDP, del NAS)
- 8082/443 DNAT remoto (APs UniFi/Omada desde fuera)

## Servicios críticos (Alpine)
```
rc-service zpot        # binario /root/zpot-rs/target/release/zpot
rc-service pppoe-server # -I eth3.881
rc-service dnsmasq     # DHCP eth3
rc-service unbound     # DNS local
rc-service nftables    # reglas hotspot + mwan
rc-service wg-quick    # wg0 (gestión)
```

## Verificación rápida (checklist NAS RADIUS)
1. `ss -tlnp | grep -E ":80 |:8081 "` → zpot activo
2. `nslookup google.com 127.0.0.1` → unbound OK
3. `cat /etc/radiusclient/radiusclient.conf` → auth_order radius, server .63:1812
4. `grep -c peer /proc/net/dev` o `ip -br addr | grep -c ppp` → sesiones PPPoE
5. `nft list table inet hotspot | grep -c hotspot_auth` → clientes auth
6. `nft list table inet mwan` → reglas MWAN presentes
7. `wg show wg0 | grep handshake` → túnel gestión fresco
