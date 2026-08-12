# Mapa Panel → Código → Configuración (2026-08-02)

Panel admin SPA en `:8081` (axum `build_admin_app` en `src/main.rs`).
Portal hotspot en `:80` (`build_hotspot_app`). Frontend: `static/`
(app-v4.js menú `PAGES` + `static/pages/*.html`).

## DASHBOARD (📊)
| Subpágina | URL | Página | Handler | Config |
|---|---|---|---|---|
| Dashboard | `/` | dashboard.html | app-v4.js (cargarDashboard) + system.rs, ppp.rs, hotspot.rs, interfaces.rs | lectura (sin archivo propio) |

## INTERFACES (🔌)
| Subpágina | URL | Página | Handler | Config |
|---|---|---|---|---|
| List | `/interfaces/list` | interfaces.html | interfaces.rs (list_interfaces) | `/etc/network/interfaces` + `/etc/zpot/mwan.json` (WANs) |
| MWAN | `/routing/mwan` | routing-mwan.html | mwan.rs | `/etc/zpot/mwan.json` + `/etc/network/interfaces` |
| VLANs | `/interfaces/vlans` | interfaces-vlans.html | vlans.rs | kernel/nft (ip link add vlan) |

## IP (🌐)
| Subpágina | URL | Página | Handler | Config |
|---|---|---|---|---|
| Addresses | `/ip/addresses` | ip-addresses.html | ip_addresses.rs (list/add/delete) | `/etc/network/interfaces` (persistencia add/delete) |
| Routes | `/ip/routes` | ip-routes.html | routes.rs | kernel (ip route) + `/etc/iproute2/rt_tables` |
| ARP | `/ip/arp` | ip-arp.html | arp.rs | tabla ARP kernel (lectura) |
| DHCP Leases | `/ip/dhcp-server` | ip-dhcp-leases.html | dhcp_leases.rs | `/var/lib/misc/dnsmasq.leases` (lectura) |
| Pools | `/ip/pools` | ip-pools.html | pools.rs (list/create/delete) + command.rs | `/etc/dnsmasq.conf` (dhcp-range; fuente de verdad de pools) |
| DNS | `/ip/dns` | ip-dns.html | dns.rs | `/etc/resolv.conf` |

## WIREGUARD (🔒)
| Subpágina | URL | Página | Handler | Config |
|---|---|---|---|---|
| Interfaces | `/wireguard/interfaces` | wireguard-interfaces.html | wireguard.rs (list) | `wg show` / `/etc/wireguard/*.conf` |
| Peers | `/wireguard/peers` | wireguard-peers.html | wireguard.rs (peers) | `/etc/wireguard/*.conf` |

## PPP (📡)
| Subpágina | URL | Página | Handler | Config |
|---|---|---|---|---|
| Server | `/ppp/server` | ppp-server.html | ppp.rs (pppoe_status/start/stop) | proceso pppoe-server (args fijos) |
| Secrets | `/ppp/secrets` | ppp-secrets.html | ppp.rs (secrets_list/disconnect) | `/etc/zpot-ppp-secrets.json` + `/etc/ppp/chap-secrets` |
| Active | `/ppp/active` | ppp-active.html | ppp.rs (active_list) | lectura: `ip addr type ppp`, `/var/run/radattr.pppN`, `/var/run/ppp-mac-pppN` |
| Logs | `/ppp/logs` | ppp-logs.html | ppp.rs (logs_list) | `/var/log/messages` (syslog pppd) |
| Remote | `/ip/remote` | ip-remote.html | ppp.rs (remote_get/set) | `/tmp/zpot-remote.txt` + nft (DNAT comment zpot-remote) |
| RADIUS Auth | `/ppp/radius` | ppp-radius.html | ppp_radius.rs | `/etc/zpot/ppp-radius.json` + `/etc/radiusclient/radiusclient.conf` + `/etc/radiusclient/servers` + `/etc/radiusclient/dictionary` + `/etc/ppp/pppoe-server-options` + `/etc/ppp/ip-up` |

## HOTSPOT (🔥)
| Subpágina | URL | Página | Handler | Config |
|---|---|---|---|---|
| Server | `/hotspot/server` | hotspot-server.html | hotspot.rs (get/post_server) | `/etc/zpot/hotspot-server.json` |
| Profiles | `/hotspot/profiles` | hotspot-server-profiles.html | hotspot.rs (get/post/delete_profile) | `/etc/zpot/hotspot-profiles.json` |
| Cookies | `/hotspot/cookies` | hotspot-cookies.html | hotspot.rs (cookies_list/delete) | `/etc/zpot/hotspot-cookies.json` |
| Active | `/hotspot/active` | hotspot-active.html | hotspot.rs (active_sessions) | lectura: nft set hotspot_auth + SESSION_STORE (+ `/etc/zpot/hotspot-sessions.json`) |
| Walled Garden | `/hotspot/walled-garden` | hotspot-walled-garden.html | hotspot.rs (walled_garden_*) | `/etc/zpot/walled-garden.json` + nft (comment zpot-wg) |
| IP Bindings | `/hotspot/ip-bindings` | hotspot-ip-bindings.html | hotspot.rs (ip_bindings_*) | `/etc/zpot/ip-bindings.json` + nft (comment zpot-ib) |

## RADIUS (🔐)
| Subpágina | URL | Página | Handler | Config |
|---|---|---|---|---|
| Servers | `/radius/servers` | radius-servers.html | radius.rs | `/etc/zpot/radius-servers.json` |

## FIREWALL (🛡️)
| Subpágina | URL | Página | Handler | Config |
|---|---|---|---|---|
| nftables | `/firewall/nftables` | firewall-nftables.html | firewall.rs | nft ruleset (kernel) |
| Conntrack | `/firewall/conntrack` | firewall-conntrack.html | firewall.rs (conntrack_status) | conntrack (kernel) |
| Limits/Log | `/firewall/limits` | firewall-limit.html | firewall.rs | nft (limit/log) |

## BRIDGE (🔗)
| Subpágina | URL | Página | Handler | Config |
|---|---|---|---|---|
| List | `/bridge/list` | bridge-list.html | bridges.rs | kernel (ip link type bridge) |
| Ports | `/bridge/ports` | bridge-ports.html | vlans.rs (bridge_vlans/configure_bridge_port) | kernel (ip link set master) |
| VLANs | `/bridge/vlans` | bridge-vlans.html | vlans.rs (bridge_vlans) | nft/ip link |

## SYSTEM (⚙️)
| Subpágina | URL | Página | Handler | Config |
|---|---|---|---|---|
| Identity | `/system/identity` | system-identity.html | system.rs | `/etc/hostname` |
| Resources | `/system/resources` | system-resources.html | system.rs | proc (lectura) |
| Clock | `/system/clock` | system-clock.html | system.rs | date/hwclock |
| NTP | `/system/ntp` | system-ntp.html | system.rs | chronyd/ntpd |
| Users | `/system/users` | system-users.html | system.rs | `/etc/passwd`, `/etc/shadow` |
| Scripts | `/system/scripts` | system-scripts.html | system.rs | `/etc/local.d/*` |
| Scheduler | `/system/scheduler` | system-scheduler.html | system.rs | `/etc/crontabs/root` |
| Logs | `/system/logs` | system-logs.html | system.rs | `/var/log/messages` |
| Files | `/system/files` | system-files.html | system.rs | filesystem |

## Portal Hotspot (:80 — build_hotspot_app)
| Ruta | Handler | Config |
|---|---|---|
| `/` | handle_root (portal_root_inline) | hotspot-server.json (html_dir) |
| `/hotspot/portal` | portal_root (login/alogin/cookie re-auth) | hotspot-server.json + hotspot-cookies.json + hotspot-profiles.json |
| `/hotspot/portal/auth` | portal_auth (POST login) | hotspot-server.json (radius) |
| `/hotspot/portal/status` + `/status` | portal_status | hotspot-sessions.json (por IP peer) |
| `/hotspot/portal/logout` + `/logout` | portal_logout | hotspot-sessions.json |
| `/hotspot/portal/disconnect` | portal_disconnect (admin) | hotspot-sessions.json |
| `/hotspot/portal/static/*` | portal_static | html_dir (`/root/zpot-rs/static/hotspot/`) |

## Archivos de configuración del sistema (no /etc/zpot)
| Archivo | Escrito por | Usado por |
|---|---|---|
| `/etc/network/interfaces` | ip_addresses.rs, interfaces.rs, mwan.rs | ifup al boot |
| `/etc/dnsmasq.conf` | pools.rs, command.rs (generado) | dnsmasq (DHCP + option 43) |
| `/etc/resolv.conf` | dns.rs | resolución |
| `/etc/radiusclient/radiusclient.conf`, `servers`, `dictionary` | ppp_radius.rs (conf), manual (dictionary Mikrotik) | pppd plugin radius.so |
| `/etc/ppp/pppoe-server-options`, `/etc/ppp/ip-up` | ppp_radius.rs (write_ip_up) | pppoe-server / pppd |
| `/etc/ppp/chap-secrets` | ppp.rs (secrets) | pppd auth |
| `/etc/crontabs/root` | system.rs | cron (watchdog PPP) |
| `/etc/local.d/zpot-red.start` | manual | boot: interfaces + pppoe-server + zpot |
| `/etc/zpot/*.json` | handlers respectivos | backend |
