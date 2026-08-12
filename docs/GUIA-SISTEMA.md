# Zpot-RS — Guía completa del sistema

> Documento de entrada: qué contiene, cómo está conectado, qué paquetes necesita,
> estructura del admin y lógica de funcionamiento del hotspot por escenarios.
> Para cada parte se indica el ARCHIVO que contiene el código (sin números de línea).

---

## 1. Qué contiene el sistema

Un solo binario Rust (axum + tokio) que corre **dos servidores HTTP en paralelo**
sobre Alpine Linux:

| Puerto | Función | URL |
|---|---|---|
| **:80** | Portal cautivo hotspot (login/status/logout) | `http://10.7.0.5/` |
| **:8081** | Admin SPA + API REST | `http://10.7.0.5:8081/zpot` |

- Código fuente: `/root/zpot-rs/`
- Binario: `/root/zpot-rs/target/release/zpot`
- Configs runtime: `/etc/zpot/*.json`
- Logs: `/tmp/zpot.log` (zlog! del hotspot), stdout del proceso en nohup
- Repo: `git@github.com:avisonofgod/Zpot.git` rama `main`

Entry point de ambos servidores: `src/main.rs`
Módulos handler registrados: `src/handlers/mod.rs`

---

## 2. Paquetes necesarios (Alpine)

Instalados vía `apk add`:

| Paquete | Uso |
|---|---|
| `iproute2` + `iproute2-tc` | comandos `ip` y `tc` (QoS HTB, rutas, reglas) |
| `nftables` | firewall hotspot, MWAN, aislamiento, bypass |
| `dnsmasq` | DHCP (rango 192.168.10.10-200), DNS |
| `ppp` + `ppp-pppoe` + `ppp-radius` | PPPoE server (pppd) y auth RADIUS PPP |
| `wireguard-tools` | túneles WireGuard (wg0) |
| `conntrack-tools` | `conntrack -D` para corte instantáneo en logout |
| `bash` | scripts de soporte (ip-up/ip-down) |
| `python3` | backend mock de interfaces (puerto 9000) y scripts de diagnóstico |
| `tcpdump` | diagnóstico de tráfico |
| `iperf3` | pruebas de velocidad |

APs WiFi (UniFi/Omada) NO usan hostapd — los controladores externos manejan la radio;
el NAS solo sirve DHCP + portal.

---

## 3. Configuraciones

Todas en `/etc/zpot/` (JSON):

| Archivo | Contenido | Código que lo lee/escribe |
|---|---|---|
| `hotspot-server.json` | iface eth3, gw 192.168.10.1, pool, RADIUS 161.97.67.63:1812, secret, dominio | `src/handlers/hotspot.rs` |
| `hotspot-profiles.json` | perfil default: login_by http-pap,mac-cookie, idle_timeout 600, rate_limit | `src/handlers/hotspot.rs` |
| `mwan.json` | WANs (eth0 wan1 mark 1, eth1 wan2 mark 2), round-robin 50/50 | `src/handlers/mwan.rs` |
| `pools.json` | pool ETH310POOL 192.168.10.101-150 | `src/handlers/pools.rs` |
| `walled-garden.json` | dominios/IPs accesibles sin autenticar (Wikipedia, controladores) | `src/handlers/hotspot.rs` |
| `adblock.json` | listas StevenBlack/OISD | `src/handlers/dns.rs` (parcial) |

Ejemplos en repo: `docs/config-examples/`

---

## 4. Frontend — estructura

| Parte | Archivo |
|---|---|
| Layout maestro (topnav + content + subnav) | `templates/base.html` |
| Router SPA + menús + init (PAGES, sw, lp) | `static/app-v4.js` |
| Estilos (variables + componentes) | `static/styles/variables.css`, `static/styles/main.css` |
| Componentes (modal, tabla, helpers) | `static/components/` |
| 45 páginas admin (una por vista) | `static/pages/*.html` |
| Portal cautivo (login, alogin, status, logout, redirect, rlogin) | `static/hotspot/` |
| JS del portal (login/status/polling) | `static/hotspot/js/main.js` |
| Versículos bíblicos del portal | `static/hotspot/js/versiculo-texto.js` |

La SPA carga cada página con `fetch('/static/pages/<page>.html')` y ejecuta sus
scripts inline; los inits de página están mapeados en `static/app-v4.js` (objeto
`pageInits` dentro de `lp()`).

---

## 5. Backend — estructura de handlers

| Handler | Rutas API | Función |
|---|---|---|
| `src/handlers/interfaces.rs` | `/api/interfaces`, `/api/vlans/*`, `/api/bridge/ports/*` | listar interfaces, VLANs, bridge VLAN table |
| `src/handlers/ip_addresses.rs` | `/api/ip-addresses` | CRUD IPs en interfaces |
| `src/handlers/routes.rs` | `/api/routes` | CRUD rutas IPv4 |
| `src/handlers/arp.rs` | `/api/arp` | tabla ARP |
| `src/handlers/pools.rs` | `/api/pools` | pools DHCP (dnsmasq) |
| `src/handlers/dhcp_leases.rs` | `/api/dhcp-leases` | leases de dnsmasq |
| `src/handlers/dns.rs` | `/api/dns` | forwarders DNS |
| `src/handlers/bridges.rs` | `/api/bridges`, `/api/bridge/ports/*` | bridges + puertos |
| `src/handlers/wireguard.rs` | `/api/wireguard/*` | interfaces y peers WG |
| `src/handlers/ppp.rs` | `/api/ppp/*` | perfiles, secrets, active, logs, QoS PPP |
| `src/handlers/mwan.rs` | `/api/mwan/*` | balanceo WANs (nft + ip rules + watchdog) |
| `src/handlers/firewall.rs` | `/api/firewall/*` | NAT, filter, mangle, sets, conntrack |
| `src/handlers/radius.rs` | `/api/radius/servers` | servidores RADIUS |
| `src/handlers/system.rs` | `/api/system` | info sistema, scripts, scheduler |
| `src/handlers/command.rs` | `/api/command` | comando raw (ip, nft, tc...) |
| `src/handlers/hotspot.rs` | `/api/hotspot/*` + portal cautivo | **el corazón del sistema** |

Registro de TODAS las rutas: `src/main.rs` (funciones `build_admin_app()` y
`build_hotspot_app()`).

---

## 6. Enlace / conexión (topología de red)

```
WiFi cliente ──► AP UniFi/Omada ──► eth3 (192.168.10.1/24, hotspot)
                                      │
                                      ├─ dnsmasq (DHCP .10-.200)
                                      ├─ nft hotspot: redirect 80 → portal, bloqueo no-auth
                                      ├─ tc HTB (eth3 bajada + ifb_eth3 subida)
                                      │
                                      ├─ MWAN: fwmark jhash(saddr) → wan1/wan2
                                      │    eth0 (192.168.2.102) WAN1 mark 1
                                      │    eth1 (192.168.3.105) WAN2 mark 2
                                      │
                                      └─ RADIUS 161.97.67.63:1812/1813 (auth/accounting)
```

- La red mgmt es 10.7.0.0/24 (wg0), la red PPPoE es eth3.881 (192.168.20.1/24).
- Reglas MWAN: `src/handlers/mwan.rs` — tabla `inet mwan`, ip rules 1401/1402.
- Reglas hotspot: `src/main.rs` (función `init_hotspot_nft()`).
- Aislamiento: hotspot→hotspot drop, hotspot→mgmt/ppp drop, ppp→ppp drop.

---

## 7. Estructura del admin (SPA)

Topnav con 10 docks generados desde el objeto `PAGES` en `static/app-v4.js`:

```
Dashboard | Interfaces | IP | WireGuard | PPP | Hotspot | RADIUS | Firewall | Bridge | System
```

Navegación: `sw(key)` abre un dock (pinta subnav), `lp(url)` carga la página
(`fetch` de `/static/pages/<page>.html`). Ambas en `static/app-v4.js`.

### Menús y sus páginas

| Dock | Submenús | Página SPA |
|---|---|---|
| Dashboard | Dashboard | `static/pages/dashboard.html` |
| Interfaces | List, MWAN, VLANs | `interfaces.html`, `routing-mwan.html`, `interfaces-vlans.html` |
| IP | Addresses, Routes, ARP, DHCP Leases, Pools, DNS | `ip-addresses.html`, `ip-routes.html`, `ip-arp.html`, `ip-dhcp-leases.html`, `ip-pools.html`, `ip-dns.html` |
| WireGuard | Interfaces, Peers | `wireguard-interfaces.html`, `wireguard-peers.html` |
| IP | Addresses, Routes, ARP, DHCP Leases, Pools, DNS, Remote | `ip-addresses.html`, `ip-routes.html`, `ip-arp.html`, `ip-dhcp-leases.html`, `ip-pools.html`, `ip-dns.html`, `ip-remote.html` |
| Hotspot | Server, Profiles, Cookies, Active, Walled Garden, IP Bindings | `hotspot-server.html`, `hotspot-server-profiles.html`, `hotspot-cookies.html`, `hotspot-active.html`, `hotspot-walled-garden.html`, `hotspot-ip-bindings.html` |
| RADIUS | Servers | `radius-servers.html` |
| Firewall | nftables, Conntrack, Limits/Log | `firewall-nftables.html`, `firewall-conntrack.html`, `firewall-limit.html` |
| Bridge | List, Ports, VLANs | `bridge-list.html`, `bridge-ports.html`, `bridge-vlans.html` |
| System | Identity, Resources, Clock, NTP, Users, Scripts, Scheduler, Logs, Files | `system-*.html` (9 páginas) |

Cada página consulta su API: `apiFetch('/api/...')` con cache TTL 3s
(definida en `static/app-v4.js`).

---

## 8. Lógica del hotspot por escenarios

Todo el flujo vive en `src/handlers/hotspot.rs`; el redirect HTTP y el arranque
de reglas nft están en `src/main.rs`.

### Escenario 1 — Primera conexión del cliente (sin cookie)

1. Cliente se asocia al AP, recibe DHCP de dnsmasq (192.168.10.x).
2. Intenta navegar → su HTTP 80 es **redirect** por nft al portal:
   reglas en `src/main.rs` (init_hotspot_nft), redirect en `src/main.rs`.
3. Cae en `/` → `handle_root()` en `src/main.rs`: no hay sesión ni cookie →
   redirect a `/hotspot/portal`.
4. `portal_root()` en `src/handlers/hotspot.rs`: no autenticado, sin cookie →
   sirve `static/hotspot/login.html` (con versículo + tabla de precios).
5. POST `/hotspot/portal/auth` → `portal_auth()` en `src/handlers/hotspot.rs`:
   - `radius_auth()` → Access-Request UDP a 161.97.67.63:1812.
   - Si Accept: crea `HotspotSession` en session_store, `apply_qos()`
     (clases tc HTB con rate/ceil del VSA), `add_bypass_nft()` (IP+MAC al set),
     `send_accounting(Start)`, `spawn_interim_task()` (re-auth + interim cada 60s).
   - Crea cookie `hs_session` (base64 de `usuario:password:mac`) + la guarda
     server-side (`save_cookie_entry()`).
   - Si Reject: sirve login con `Reply-Message`.
6. Sirve `static/hotspot/alogin.html` (autenticando...) → redirige a `/status`.

### Escenario 2 — Mismo cliente vuelve (con cookie, sesión activa o no)

1. GET `/` → `handle_root()` en `src/main.rs`:
   - Si IP está en session_store → sirve `portal_status_inline()` (status.html).
   - Si no, lee cookie `hs_session`, decodifica, **verifica MAC en ARP** y que la
     cookie exista server-side (`cookie_entry_exists()`).
   - Cookie válida → redirect a `/hotspot/portal`.
2. `portal_root()` en `src/handlers/hotspot.rs`: cookie válida →
   **RADIUS re-auth completo** (`radius_auth()`), recrea sesión, QoS, bypass nft,
   Accounting Start, interim task. Sirve alogin.
3. Cookie rechazada (eliminada del admin o expirada) → borra cookie del browser
   (`Max-Age=0`) y muestra login con "Sesión finalizada".

### Escenario 3 — Sesión activa navegando

- Tráfico del cliente pasa porque su IP.MAC está en el set `hotspot_auth`
  (regla `return` en prerouting y `accept` en forward: `src/main.rs`).
- Bajada limitada por clase tc en eth3; subida por clase en `ifb_eth3`
  (`apply_qos()` en `src/handlers/hotspot.rs`).
- El interim task (`spawn_interim_task()` en `src/handlers/hotspot.rs`) cada 60s:
  lee contadores tc (`read_tc_bytes()`), envía Accounting-Interim, verifica idle,
  re-autentica contra RADIUS para saldo vigente.

### Escenario 4 — Saldo agotado / Access-Reject en re-auth

- En `spawn_interim_task()` (`src/handlers/hotspot.rs`), la re-auth cada 60s
  devuelve `rejected=true` (Access-Reject code 3) → `session_disconnect_internal()`
  con terminate_cause=5 → se elimina sesión, nft element, clases tc, Accounting-Stop.
- Timeout de red NO desconecta (solo Access-Reject explícito).

### Escenario 5 — Idle timeout (sin tráfico)

- En `spawn_interim_task()`: si los bytes tc no cambian entre interims y
  `now - last_active >= idle_timeout` (RADIUS attr 28 → perfil → default 600) →
  `session_disconnect_internal()` con cause=4 (Idle-Timeout).

### Escenario 6 — Logout manual del cliente

- GET `/logout?username=X` → `portal_logout()` en `src/handlers/hotspot.rs`:
  borra sesión del store, elimina elemento nft, borra filtros/clases tc,
  `send_accounting(Stop)` con contadores finales, `conntrack -D` para corte
  instantáneo. Sirve `static/hotspot/logout.html`.

### Escenario 7 — Cliente se desconecta físicamente (WiFi se cae)

- Watchdog del hotspot dentro de `src/main.rs` (task tokio que corre cada 30s
  junto al watchdog MWAN): lista el set `hotspot_auth`, para cada IP verifica ARP;
  si `FAILED`/`INCOMPLETE` o sin ARP + ping fallido → elimina el elemento nft y
  termina la sesión (con `eprintln!` de log).

### Escenario 8 — Admin desconecta a un cliente

- POST `/hotspot/portal/disconnect` → `portal_disconnect()` en
  `src/handlers/hotspot.rs` → `session_disconnect_internal()` (mismo flujo que
  timeout: nft + tc + Accounting-Stop + conntrack).

### QoS rate/ceil (aplica a todos los escenarios)

- El VSA MikroTik (attr 26, OUI 14988, vendor-type 8) trae
  `rate_up/rate_down ceil_up/ceil_down` (ej `1M/4M 2M/5M`).
- Parse en `parse_radius_attrs()` de `src/handlers/hotspot.rs` → rate=tokens[0]
  (UP/DOWN), ceil=tokens[1] (UP/DOWN); formato simple sin `/` = UP/DOWN con ceil=rate.
- Aplicación: `apply_qos()` en `src/handlers/hotspot.rs` (tc HTB, QOS_LOCK global).

---

## 9. Ciclo de despliegue (flujo de trabajo)

```
VPS local (edición) ──git push──▶ GitHub ──git pull──▶ Alpine (10.7.0.5)
                                                       ├─ touch src/handlers/xxx.rs
                                                       ├─ cargo build --release
                                                       ├─ stat binario > fuente
                                                       ├─ kill PID
                                                       └─ nohup ./target/release/zpot &
```

Reglas: NUNCA compilar en el VPS local; NUNCA `pkill -f` (mata SSH); para cambios
de frontend basta git pull + cerrar pestaña + Ctrl+Shift+R (cache browser).
