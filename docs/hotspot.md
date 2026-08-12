# Hotspot

## Complete Session Lifecycle

### 1. User connects to WiFi

```
User connects WiFi → DHCP → gets IP (192.168.10.x)
     ↓
nftables: iif eth3 ip saddr . ether saddr @hotspot_auth
     ↓
NO match → HTTP 80 redirect to portal
     ↓
Browser shows login page
```

### 2. Authentication

```
User enters username/password → POST /api/hotspot/portal/auth
     ↓
radius_auth(RADIUS, secret, username, password)
     ↓
Access-Accept:
  Mikrotik-Rate-Limit="1M/4M 2M/5M"  (o "512K/1M 1M/2M" segun voucher)
  Reply-Message, Framed-IP, etc.
  NOTA: Idle-Timeout (attr 28) NO se parsea en el codigo (2026-08-02)
  → idle_timeout siempre viene del perfil local (default 600s)
     ↓
1. HotspotSession created (session_store, keyed by IP)
2. nft: add element { IP . MAC timeout 24h }
   → cleanup stale elements with same IP first
3. tc: QoS applied (HTB classes + filters)
4. Accounting-Start sent to RADIUS
5. spawn_interim_task(60s loop)
6. Cookie saved (base64(username:password:mac))
     ↓
User redirected to alogin.html → internet access
```

### 3. Active Session (60s loop)

```rust
loop {
    sleep(60s);  // primer interim a los 60s reales

    // 1. Read TC counters
    let rx = read_tc_bytes(iface, down_minor);
    let tx = read_tc_bytes(&format!("ifb_{}", iface), up_minor);

    // 2. Send Accounting-Request (Interim-Update)
    send_accounting(rad_srv, rad_sec, username, ip, 3, session_id, time, rx, tx, 0);

    // 3. Idle timeout check (if idle_timeout > 0)
    if now - last_active >= idle_timeout {
        session_disconnect_internal(terminate_cause=4);  // Idle-Timeout
        break;
    }

    // 4. Re-auth with RADIUS
    match find_password_for_username(&username) {
        Some(password) => {
            let reauth = radius_auth(&rad_srv, &rad_sec, &username, &password).await;
            if reauth.rejected {
                session_disconnect_internal(terminate_cause=5);  // Session-Timeout (saldo agotado)
                break;
            }
        }
        None => {
            session_disconnect_internal(terminate_cause=6);  // Cookie expirada
            break;
        }
    }
}
```

### 4. Disconnection

```rust
session_disconnect_internal(ip, rad_srv, rad_sec, iface, terminate_cause):
  1. nft: delete element { IP . MAC }        ← remove bypass
  2. tc: delete filters + classes             ← remove QoS
  3. send_accounting(Stop, terminate_cause)   ← final accounting
  4. conntrack -D -s IP                       ← flush connections
  5. session_store.remove(ip)                 ← remove from HashMap
  (la cookie server-side NO se borra en disconnect interno — 2026-08-02)
```

## Scenarios

### Scenario A: Normal user, active browsing

```
T=0:   Login → session created, nft bypass, QoS, acct start
T=60s: interim: acct + re-auth → Access-Accept (saldo OK)
T=120s: traffic detected → last_active updated
  ... (continues every 60s)
T=23h: cookie expires (7d window, not reached in 1 session)
T=saldo agota: re-auth → Access-Reject → [REAUTH] desconectando
```

### Scenario B: User leaves (idle timeout)

```
T=0:   Login
T=30m: User closes WiFi, last_active = T+30m
T=60m: no traffic → now - last_active = 30m
T=90m: no traffic → 60m idle
  ...
T=idle_timeout (3600s=1h):
  [IDLE-TIMEOUT] desconectando (terminate_cause=4)
  Cookie preserved
```

### Scenario C: User returns after idle

```
T=0:   Idle timeout disconnected user (cookie alive)
T=2h:  User reconnects WiFi → new IP (or same)
       HTTP 80 → nft: no match → REDIRECT portal
       Browser has cookie hs_session
       Cookie re-auth → RADIUS Access-Accept
       New session created (same user, new IP)
       User navigates directly (no manual login)
```

### Scenario D: IP reassigned to different user

```
T=0:   User A (IP=100, MAC=AA) active
T=1h:  User A leaves, session alive
T=2h:  User B connects, DHCP gives IP=100 (MAC=BB)
       nft: ip saddr 100 . ether saddr BB → NO match → portal
       User B logs in (or cookie):
         1. nft cleanup: delete 100 . AA (stale)
         2. nft add: 100 . BB timeout 24h
         3. session_store: User B overwrites User A
T=4h:  User A returns, DHCP gives different IP → portal → cookie login
```

### Scenario E: RADIUS timeout (no false disconnect)

```
T=0:   User active
T=60s: re-auth → RADIUS timeout (3s, no response)
       radius_auth() returns rejected=false
       → NO disconnect, session continues
T=120s: re-auth → Access-Accept (RADIUS recovered)
```

### Scenario F: Cookie expires (7 days)

```
Day 1:  User logs in → cookie saved (expires in 7d)
Day 2-5: OK
Day 7:  Cookie expires → find_password_for_username() returns None
        [REAUTH] user no tiene cookie valida, expulsando
        Session terminated (terminate_cause=6)
```

## Walled-Garden (Controladores UniFi/Omada)

Las APs necesitan acceso a sus controladores **sin autenticar** para poder
enlazarse y actualizarse. Se configuran reglas en la forward chain antes del
`drop` de no-autenticados.

### Set de Controladores

```nft
set controladores {
    type ipv4_addr
    flags timeout
    timeout 1d
    elements = {
        161.97.67.63,       # Omada Controller + RADIUS
        44.193.125.236,     # Omada Cloud
        18.213.142.156,     # Omada Cloud
        34.238.17.94,       # Omada Cloud
        54.243.197.97       # Omada Cloud
    }
}
```

### Puertos Abiertos (sin autenticar)

| Puerto | Protocolo | Servicio |
|---|---|---|
| 29810-29814 | TCP | Omada Management |
| 29810-29814 | UDP | Omada Discovery |
| 8088, 8043, 8843 | TCP | Omada Portal |
| 27001 | UDP | Omada Discovery broadcast |
| 3478 | UDP | UniFi STUN |
| 10001 | UDP | UniFi Discovery |
| 123 | UDP | NTP (sincronizacion hora APs) |

### DHCP Option 43 — Omada Discovery

Las APs Omada necesitan la IP del controlador via DHCP Option 43 para saber
a donde conectarse. Configurado en `/etc/dnsmasq.conf`:

```text
dhcp-option=eth3,43,01:04:a1:61:43:3f
```

### Verificacion

```bash
# Ver reglas aplicadas
nft list chain inet hotspot forward | grep -E 'controladores|29810|27001|3478|10001|123'

# Ver APs Omada conectadas
cat /var/lib/misc/dnsmasq.leases | grep -iE 'eap|tp-link|omada'

# Ver trafico hacia controlador
timeout 10 tcpdump -i any -n 'host 161.97.67.63'
```

## nftables Rules

### Set

```nft
table inet hotspot {
    set hotspot_auth {
        type ipv4_addr . ether_addr
        flags timeout
        timeout 24h
    }
}
```

### Prerouting (NAT)

```nft
chain prerouting {
    type nat hook prerouting priority dstnat; policy accept;
    iif "eth3" tcp dport 8081 drop
    iif "eth3" ip saddr . ether saddr @hotspot_auth return
    iif "eth3" tcp dport 80 redirect
}
```

### Forward (Filter)

```nft
chain forward {
    type filter hook forward priority filter; policy accept;
    iif "eth3" udp dport { 67, 68 } accept          # DHCP
    iif "eth3" udp dport 53 accept                   # DNS
    iif "eth3" tcp dport 53 accept                   # DNS
    iif "eth3" tcp dport 80 accept                   # Portal
    iif "eth3" oif "eth3" drop                       # Isolation
    iif "eth3" ip daddr { 10.7.0.0/24, 192.168.20.0/24 } drop
    iif "eth3" ip saddr . ether saddr @hotspot_auth accept
    iif "eth3" drop                                  # No-auth drop
}
```

### Postrouting (NAT)

```nft
chain postrouting {
    type nat hook postrouting priority srcnat; policy accept;
    oif "eth0" masquerade
    oif "eth1" masquerade
}
```

## QoS (tc HTB)

- Root: `1:0` on hotspot interface (eth3)
- One HTB class per IP: `1:{1000 + last_octet * 2}` (down) and `1:{1001 + last_octet * 2}` (up)
- IFB interface `ifb_eth3` for upload shaping
- Rate = garantía, Ceil = máximo (ambos de la VSA: `rate_up/rate_down ceil_up/ceil_down`)
- Filtros: `protocol ip prio {100+last_octet}` del+add por IP (FIX 2026-08-02 —
  el replace colapsaba a 1 filtro; ver references/qos-radius-2026-08-02.md)

## Configuración actual (2026-08-02) — referencia para import/export

### `/etc/zpot/hotspot-server.json`
```json
{
  "iface": "eth3", "name": "Hotspot", "gw": "192.168.10.1",
  "html_dir": "/root/zpot-rs/static/hotspot", "pool": "default",
  "pool_range": "192.168.10.10-192.168.10.200",
  "dns_server": "192.168.10.1", "domain": "wifi1.info",
  "login_by": "http-pap,mac-cookie", "use_radius": true,
  "radius": "161.97.67.63:1812", "radius_secret": "85River@B",
  "profile": "default"
}
```

### `/etc/zpot/hotspot-profiles.json`
```json
[{ "name": "default", "login_by": "http-pap,mac-cookie",
   "idle_timeout": 600, "shared_users": 1,
   "rate_limit": "1M/2M 2M/3M", "cookie_timeout": "7d" }]
```

### `/etc/zpot/walled-garden.json`
```json
[{"comment":"Wikipedia","domain":"wikipedia.org","ip":"208.80.154.224","port":"80,443","protocol":"tcp"}]
```
NOTA (2026-08-02): `pools.json` ELIMINADO (era legacy, nadie lo leia; el pool real
se configura en /ip/pools y se guarda en /etc/dnsmasq.conf). La regla nft del
walled-garden SOLO se aplica al guardar desde la API — NO se re-aplica al boot
del backend (si se reinicia, la regla desaparece hasta re-guardar).

### nftables real — `nft list table inet hotspot` (resumen)
- set `hotspot_auth` (ipv4_addr . ether_addr, timeout 1d) — clientes autenticados
- set `controladores` (ipv4_addr, timeout 1d) — UniFi/Omada: 18.213.142.156,
  34.238.17.94, 44.193.125.236, 54.243.197.97, 161.97.67.63
- prerouting: `iif eth3 tcp dport 8081 drop`; `@hotspot_auth return`; `tcp dport 80 redirect`
- forward: DHCP 67/68, DNS 53, HTTP 80, `@controladores`, puertos Omada
  (29810-29814, 8043/8088/8843, 3478, 10001, 27001, 123), isolation `oif eth3 drop`,
  drop 10.7.0.0/24+192.168.20.0/24, drop ppp*↔ppp*; no-auth drop
- postrouting: `oif eth0 masquerade`, `oif eth1 masquerade`

### dnsmasq (DHCP) — `/etc/dnsmasq.conf` (generado por Zpot)
port=5353 (DNS lo sirve unbound/adblock), dhcp-authoritative,
`interface=eth3` con `dhcp-range 192.168.10.2-245 12h` + option 43 (Omada)
+ router/dns 192.168.10.1. El pool se configura en /ip/pools (test-pool).
NOTA: `/etc/dnsmasq.d/zz-zpot-pools.conf` solo tiene comentarios residuales
de pools viejos (eth3.10/eth3/bridgeLan) — inofensivo, no genera rangos.

### Interfaces y servicios
eth0=192.168.2.102/24 (WAN1), eth1=192.168.3.105/24 (WAN2), eth3=192.168.10.1/24
(hotspot), eth3.881=192.168.20.1/24 (PPPoE), eth2=192.168.30.1/24 DOWN, wg0=10.7.0.5.
Servicios: dnsmasq+unbound (started), pppoe-server (toggle en PPP→Server; arranca
con Zpot via /etc/local.d/zpot-red.start — ver pppoe.md), zpot (:80 portal,
:8081 admin).
Paquetes: nftables, dnsmasq, unbound, freeradius-utils/radiusclient (dictionary
Mikrotik — ver pppoe.md), iproute2, ppp+pppoe. accel-ppp ELIMINADO 2026-08-02.

### Portal (frontend) — `static/hotspot/`
alogin.html, login.html, logout.html, redirect.html, rlogin.html, status.html,
md5.js + css/ + js/. Admin: 6 páginas en app-v4.js (Server, Profiles, Cookies,
Active, Walled Garden, IP Bindings) + 45 páginas totales.

### RADIUS externo (161.97.67.63:1812, secret 85River@B)
Panel DaloRADIUS (MySQL radius/85River@B). El hotspot autentica por HTTP-PAP
(http-pap) contra este servidor; los vouchers hotspot (2YJD, 88TD...) reciben
Mikrotik-Rate-Limit en Access-Accept. PPP y hotspot comparten servidor.

## Reconexión y hallazgos (2026-08-02) — estado en memoria

### Flujo de reconexión
1. **Auto-login por cookie** (escenario C): el browser trae `hs_session`
   (base64 user:pass:mac) → portal_root valida contra las cookies
   server-side → RADIUS re-auth → sesión nueva (mismo flujo que login).
2. **Sin cookie / cookie inválida** → login.html con error.
3. **Tras reinicio de zpot** (FIX 2026-08-02): sesiones + cookies ahora se
   PERSISTEN en /etc/zpot/hotspot-sessions.json y hotspot-cookies.json y se
   reconstruyen al boot (incluido el re-agregado del bypass nft y el respawn
   de interim tasks). `/api/hotspot/active` vuelve a mostrar los clientes sin
   que re-logineen y el accounting/interim continúa con el mismo session_id.
   Las sesiones fantasma las expulsa el interim en el 1er ciclo.

### Hallazgos — TODOS RESUELTOS (49e9fc6, d9a7b89, 6cc1a4f)
1. ✅ **Cookie server-side en memoria** → persisten en
   `/etc/zpot/hotspot-cookies.json` (save en cada mutación + `load_cookies_from_disk()`
   al boot). Auto-login por cookie sobrevive reinicios de zpot.
2. ✅ **Sesiones no reconstruidas al boot** → persisten en
   `/etc/zpot/hotspot-sessions.json`; `restore_sessions_from_disk()` + `restore_and_spawn_interims()`
   las reconstruyen al boot y **respawnan los interim tasks**. NOTA: como
   `init_hotspot_nft()` borra/recrea la tabla (set vacío), el restore NO valida
   contra el set — re-agrega el bypass nft para cada sesión (add_bypass_nft).
   Las clases/filtros tc persisten en el kernel. Sesiones fantasma (cliente que
   se fue sin logout) las expulsa el interim en el 1er ciclo. Verificado:
   tras restart, 6 sesiones reconstruidas + active poblado + interim respawneado.
3. ✅ **Walled-garden e IP-bindings no se re-aplicaban al boot** → AHORA
   `init_hotspot_nft()` llama `apply_wg_rules(load_wg())` + `apply_ib_rules(load_ib())`
   al arrancar. Además `cleanup_nft_by_comment()` borra reglas previas
   (`comment "zpot-wg"` / `"zpot-ib"`) antes de re-insertar → elimina las
   reglas huérfanas al borrar entradas. Verificado: regla Wikipedia presente
   tras reinicio.
4. ✅ **Idle-Timeout (attr 28) no se parseaba** → AHORA `parse_radius_attrs`
   tiene case 28; si RADIUS envía Idle-Timeout se usa (sino perfil local 600s).
   Verificado: clientes con idle 3600 (1h) y HTXD con 180 (voucher distinto).
5. ✅ **shared_users no se validaba** → AHORA `shared_users_reached()` se
   evalúa en portal_auth (login form) y en el re-auth por cookie de portal_root;
   si se alcanza el límite del perfil se sirve login con "Session limit reached"
   (traducción ya existente en login.html).
6. ✅ **portal_status sin username** mostraba la primera sesión del store →
   AHORA recibe `ConnectInfo(peer)` y busca la sesión del cliente que consulta.
7. ✅ **Walled-garden delete dejaba reglas huérfanas** → resuelto con
   cleanup_nft_by_comment (mismo fix que #3).
8. ✅ **get_mac_from_arp con neigh FAILED/INCOMPLETE** → AHORA busca en toda
   la tabla ARP de eth3 y reintenta tras 400ms; `add_bypass_nft` tiene guard
   si la MAC queda vacía (no agrega elemento inválido).
