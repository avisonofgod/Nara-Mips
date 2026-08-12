# Estructura del proyecto Zpot-RS

```
zpot-rs/
├── Cargo.toml              # Dependencias Rust (axum 0.7, tokio, serde, tower-http)
├── Cargo.lock              # Lock de dependencias (autogenerado)
├── docs/                   # Documentación por módulo
│   ├── GUIA-SISTEMA.md     # ★ Guía completa de entrada (paquetes, configs, admin, hotspot por escenarios)
│   ├── README.md
│   ├── architecture.md
│   ├── backend.md
│   ├── frontend.md
│   ├── hotspot.md
│   ├── network.md
│   ├── pppoe.md
│   ├── radius.md
│   └── config-examples/    # JSONs de ejemplo (hotspot, mwan, perfiles)
│
├── README.md               # Documentación básica
├── CHANGELOG.md            # Historial de cambios
├── STRUCTURE.md            # Este archivo
├── .gitignore              # exclusiones: target/, node_modules/, *.log, .hermes/
│
├── src/
│   ├── main.rs             # Entry point: servidores HTTP (axum), router admin + hotspot
│   ├── naming.rs           # Normalización de nombres de interfaz (eth0..eth4, oculta cpu port)
│   ├── routeros_parser.rs  # Parser de salida RouterOS (texto plano) — sin uso activo
│   │
│   └── handlers/
│       ├── mod.rs          # Re-export de todos los módulos handler
│       ├── arp.rs          # GET /api/arp — tabla ARP del sistema
│       ├── bridges.rs      # GET/POST /api/bridges — bridges + puertos
│       ├── command.rs      # POST /api/command — comandos raw (ip, bridge, vlan, firewall, nft)
│       ├── dhcp_leases.rs  # GET /api/dhcp-leases — leases de dnsmasq
│       ├── dns.rs          # CRUD /api/dns — forwarders en dnsmasq.conf
│       ├── firewall.rs     # CRUD /api/firewall/* — NAT, filter, mangle, sets, conntrack
│       ├── hotspot.rs      # Hotspot completo: portal cautivo + admin + RADIUS auth/acct + idle timeout
│       ├── interfaces.rs   # GET /api/interfaces — listado de interfaces del sistema
│       ├── ip_addresses.rs # CRUD /api/ip-addresses — IPs en interfaces
│       ├── mwan.rs         # GET/POST /api/mwan/* — balanceo de WANs (nft + ip rules)
│       ├── pools.rs        # CRUD /api/pools — dhcp-range en dnsmasq.conf
│       ├── ppp.rs          # CRUD /api/ppp/* — perfiles, secrets, sesiones activas, QoS
│       ├── radius.rs       # CRUD /api/radius/servers — servidores RADIUS (auth+acct)
│       ├── routes.rs       # CRUD /api/routes — rutas IPv4 del sistema
│       ├── system.rs       # GET /api/system — info del sistema (hostname, uptime, CPU, RAM)
│       ├── vlans.rs        # CRUD /api/vlans — VLANs + bridge VLAN table
│       └── wireguard.rs    # GET/POST /api/wireguard/* — interfaces y peers
│
├── templates/
│   ├── base.html           # Layout maestro HTML: topnav, subnav, content, dock inferior
│   ├── ppp-ip-up-down.sh   # Template script ip-up/ip-down para QoS PPP (cURL a Zpot-RS)
│   └── ppp-ip-down.sh      # Template script ip-down para QoS PPP
│
├── static/
│   ├── app-v4.js           # SPA v4: navegacion (sw/lp), PAGES, cache API, live data
│   ├── app-v3.js           # SPA v3 (legacy, sin uso activo)
│   │
│   ├── styles/
│   │   ├── variables.css   # Variables CSS (colores, fuentes, espaciado)
│   │   └── main.css        # Estilos de componentes (tablas, modales, docks, forms)
│   │
│   ├── components/
│   │   ├── helpers.js      # Funciones auxiliares (esc(), formatBytes(), toast())
│   │   ├── modal.js        # Sistema de modales (zModal)
│   │   ├── table.js        # Generacion de tablas dinamicas
│   │   └── format-helpers.js # Formateo especifico (interfaces, fechas, bytes)
│   │
│   ├── hotspot/            # Portal cautivo (carga desde :80)
│   │   ├── login.html      # Pagina de login del portal
│   │   ├── rlogin.html     # Redirect para MikroTik rlogin
│   │   ├── alogin.html     # Pagina de autenticacion en proceso
│   │   ├── status.html     # Pagina de estado (usuario autenticado)
│   │   ├── logout.html     # Pagina de cierre de sesion
│   │   ├── redirect.html   # Pagina de redireccion post-login
│   │   ├── md5.js          # Cliente MD5 para CHAP-like auth
│   │   ├── css/style.css   # Estilos del portal
│   │   └── js/
│   │       ├── main.js           # JS del portal (login, status, polling)
│   │       ├── versiculo-texto.js # Texto versiculos biblicos
│   │       └── versiculos.json   # Lista de versiculos
│   │
│   └── pages/              # 45 paginas SPA (una por vista del admin)
│       ├── dashboard.html
│       ├── interfaces.html
│       ├── interfaces-bonding.html
│       ├── interfaces-bridges.html
│       ├── interfaces-vlans.html
│       ├── ip-addresses.html
│       ├── ip-arp.html
│       ├── ip-dhcp-leases.html
│       ├── ip-dns.html
│       ├── ip-pools.html
│       ├── ip-routes.html
│       ├── ppp-active.html
│       ├── ppp-logs.html
│       ├── ppp-profiles.html
│       ├── ip-remote.html
│       ├── ppp-secrets.html
│       ├── hotspot-server.html
│       ├── hotspot-server-profiles.html
│       ├── hotspot-active.html
│       ├── hotspot-walled-garden.html
│       ├── hotspot-ip-bindings.html
│       ├── radius-servers.html
│       ├── firewall-nat.html
│       ├── firewall-filter.html
│       ├── firewall-mangle.html
│       ├── firewall-address-lists.html
│       ├── firewall-limit.html
│       ├── firewall-conntrack.html
│       ├── firewall-nftables.html
│       ├── bridge-list.html
│       ├── bridge-ports.html
│       ├── bridge-vlans.html
│       ├── routing-mwan.html
│       ├── system-identity.html
│       ├── system-clock.html
│       ├── system-ntp.html
│       ├── system-resources.html
│       ├── system-logs.html
│       ├── system-users.html
│       ├── system-scripts.html
│       ├── system-scheduler.html
│       ├── system-files.html
│       ├── wireguard-interfaces.html
│       └── wireguard-peers.html
│
├── scripts/
│   └── ppp-zombie-watchdog.sh     # Watchdog PPP zombies (ip link delete)
│
├── zpot-init.sh                # Init script OpenRC para Zpot-RS (nftables + dnsmasq)
├── setup-accel.sh              # Setup accel-ppp (PPPoE alternativo)
├── start-accel.sh              # Arranque accel-ppp
├── accel-ppp.conf              # Config accel-ppp
├── pppoe-watchdog.sh           # Watchdog PPPoE
├── reinit-pppoe.sh             # Reinicio PPPoE
└── proxy.sh                    # Proxy HTTP (uso auxiliar)
```

## 10 Docks (topnav — generados desde PAGES en app-v4.js)

| # | Dock       | Submenus                                              |
|---|------------|-------------------------------------------------------|
| 1 | Dashboard  | dashboard                                             |
| 2 | Interfaces | interfaces, routing-mwan, interfaces-vlans            |
| 3 | IP         | ip-addresses, ip-routes, ip-arp, ip-dhcp-leases, ip-pools, ip-dns |
| 4 | WireGuard  | wireguard-interfaces, wireguard-peers                 |
| 5 | PPP        | ppp-secrets, ppp-active, ppp-logs |
| 6 | Hotspot    | hotspot-server, hotspot-server-profiles, hotspot-cookies, hotspot-active, hotspot-walled-garden, hotspot-ip-bindings |
| 7 | RADIUS     | radius-servers                                        |
| 8 | Firewall   | firewall-nftables, firewall-conntrack, firewall-limit |
| 9 | Bridge     | bridge-list, bridge-ports, bridge-vlans               |
| 10| System     | system-identity, -resources, -clock, -ntp, -users, -scripts, -scheduler, -logs, -files |

Total: **45 paginas SPA** + **1 layout** + **4 componentes JS** + **2 CSS** + **16 handlers Rust** + **9 paginas portal hotspot**

## Backend HTTP

### Admin API (puerto 8081)

```
GET  /zpot                              → base.html (SPA shell)
GET  /static/*path                      → archivos estaticos

GET  /api/interfaces                    → interfaces.rs
GET  /api/ip-addresses                  → ip_addresses.rs (list)
POST /api/ip-addresses                  → ip_addresses.rs (add)
DELETE /api/ip-addresses/:ifname/:addr  → ip_addresses.rs (delete)
GET  /api/vlans                         → vlans.rs (list)
POST /api/vlans                         → vlans.rs (create)
POST /api/vlans/delete                  → vlans.rs (delete)
POST /api/vlans/configure               → vlans.rs (configure)
POST /api/vlans/title                   → vlans.rs (set_title)
GET  /api/vlans/bridge-table            → vlans.rs (bridge vlan table)
GET  /api/bridge/ports                  → vlans.rs (ports = bridge vlan table)
POST /api/bridge/ports/configure        → vlans.rs (configure bridge port)
POST /api/bridge/ports/add              → bridges.rs (port add)
POST /api/bridge/ports/remove           → bridges.rs (port remove)
GET  /api/bridges                       → bridges.rs (list)
POST /api/bridges                       → bridges.rs (create)
POST /api/bridges/delete                → bridges.rs (delete)
GET  /api/routes                        → routes.rs (list)
POST /api/routes                        → routes.rs (add)
POST /api/routes/delete                 → routes.rs (delete)
GET  /api/arp                           → arp.rs (list)
GET  /api/pools                         → pools.rs (list)
POST /api/pools                         → pools.rs (create)
DELETE /api/pools                       → pools.rs (delete)
GET  /api/dhcp-leases                   → dhcp_leases.rs (list)
GET  /api/dns                           → dns.rs (list)
POST /api/dns                           → dns.rs (add)
POST /api/dns/delete                    → dns.rs (delete)
POST /api/command                       → command.rs (>30 subcomandos)

GET  /api/ppp/profiles                  → ppp.rs (list)
POST /api/ppp/profiles                  → ppp.rs (add)
POST /api/ppp/profiles/delete           → ppp.rs (delete)
POST /api/ppp/profiles/update           → ppp.rs (update)
GET  /api/ppp/secrets                   → ppp.rs (list)
POST /api/ppp/secrets                   → ppp.rs (add)
POST /api/ppp/secrets/update            → ppp.rs (update)
POST /api/ppp/secrets/delete            → ppp.rs (delete)
POST /api/ppp/secrets/toggle            → ppp.rs (toggle)
POST /api/ppp/secrets/disconnect        → ppp.rs (disconnect)
GET  /api/ppp/active                    → ppp.rs (active list)
GET  /api/ppp/logs                      → ppp.rs (logs)
POST /api/ppp/qos                       → ppp.rs (qos apply)
POST /api/ppp/qos/cleanup               → ppp.rs (qos cleanup)
GET  /api/ip/remote                     → ppp.rs (remote get)
POST /api/ip/remote                     → ppp.rs (remote set)

GET  /api/wireguard/interfaces          → wireguard.rs (list)
GET  /api/wireguard/peers/:name         → wireguard.rs (peers)
POST /api/wireguard/peers/add           → wireguard.rs (add)
POST /api/wireguard/peers/delete        → wireguard.rs (delete)

GET  /api/mwan/status                   → mwan.rs (status)
GET  /api/mwan/config                   → mwan.rs (get config)
POST /api/mwan/config                   → mwan.rs (post config)

GET  /api/firewall/nat                  → firewall.rs (list nat)
POST /api/firewall/nat                  → firewall.rs (create nat)
POST /api/firewall/nat/delete           → firewall.rs (delete nat)
GET  /api/firewall/filter               → firewall.rs (list filter)
POST /api/firewall/filter/delete        → firewall.rs (delete filter)
POST /api/firewall/rule                 → firewall.rs (create nft rule)
POST /api/firewall/rule/move            → firewall.rs (move rule)
POST /api/firewall/rule/move-to         → firewall.rs (move rule to)
GET  /api/firewall/mangle               → firewall.rs (list mangle)
GET  /api/firewall/sets                 → firewall.rs (list nft sets)
GET  /api/firewall/conntrack            → firewall.rs (conntrack status)

GET  /api/radius/servers                → radius.rs (list)
POST /api/radius/servers                → radius.rs (add/update)

GET  /api/hotspot/server                → hotspot.rs (get config)
POST /api/hotspot/server                → hotspot.rs (set config)
GET  /api/hotspot/profiles              → hotspot.rs (list profiles)
POST /api/hotspot/profiles              → hotspot.rs (add profile)
POST /api/hotspot/profiles/delete       → hotspot.rs (delete profile)
GET  /api/hotspot/active                → hotspot.rs (active sessions)
POST /api/hotspot/walled-garden         → hotspot.rs (add domain)
GET  /api/hotspot/walled-garden         → hotspot.rs (list domains)
POST /api/hotspot/walled-garden/delete  → hotspot.rs (delete domain)
GET  /api/hotspot/ip-bindings           → hotspot.rs (list bindings)
POST /api/hotspot/ip-bindings           → hotspot.rs (add binding)
POST /api/hotspot/ip-bindings/delete    → hotspot.rs (delete binding)

GET  /api/system                        → system.rs (info)
```

### Hotspot Portal (puerto 80)

```
GET  /                                 → root handler (login o status segun auth)
GET  /status                           → portal_status (JSON estado sesion)
GET  /logout                           → portal_logout (cierre de sesion)
GET  /hotspot/portal                   → portal_root
GET  /hotspot/portal/login             → portal_login (HTML formulario)
POST /hotspot/portal/auth              → portal_auth (login POST)
GET  /hotspot/portal/status            → portal_status (JSON)
GET  /hotspot/portal/logout            → portal_logout
POST /hotspot/portal/disconnect        → portal_disconnect
GET  /hotspot/portal/static/*file      → portal_static (archivos del portal)
```

## Scripts de instalacion / despliegue

| Script | Proposito |
|--------|-----------|
| `zpot-init.sh` | Init OpenRC: nftables (hotspot) + dnsmasq |
| `scripts/ppp-zombie-watchdog.sh` | Watchdog PPP zombies |
| `setup-accel.sh` | Setup accel-ppp alternativo |

> Nota: los scripts de instalación completa (setup-alpine.sh, install-ppp-qos.py,
> etc.) fueron eliminados del repo el 2026-07-21. La instalación de paquetes se
> documenta en `docs/GUIA-SISTEMA.md` sección 2.

---

## PENDIENTE: Export/Import de configuracion

Las configuraciones manuales del admin frontend (hotspot server, perfiles, walled garden, IP bindings, servidores RADIUS, etc.) se almacenan en **memoria** en el backend Rust y en archivos runtime en `/etc/zpot/` en Alpine.

Para reinstalacion limpia con migracion de config:
- [ ] Feature export: endpoint `/api/config/export` que serialice todas las configs a un JSON
- [ ] Feature import: endpoint `/api/config/import` que cargue y aplique el JSON exportado
- [ ] Incluir: servidores RADIUS, config hotspot, perfiles, walled garden, IP bindings
- [ ] Archivo de export: `zpot-export-{fecha}.json`
