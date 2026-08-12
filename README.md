<p align="center">
  <img src="https://img.shields.io/badge/Rust-1.80+-orange?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/Axum-0.7-blueviolet?logo=rust" alt="Axum">
  <img src="https://img.shields.io/badge/SPA-HTML/CSS/JS-blue?logo=javascript" alt="SPA">
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License">
</p>

<h1 align="center">Zpot-RS — Gestor ISP</h1>
<p align="center"><em>Panel de administración para redes ISP con backend en Rust y frontend SPA vanilla</em></p>

---

## Requisitos

- Rust 1.80+ (con Cargo)
- Git

## Compilar y ejecutar

```bash
git clone https://github.com/avisonofgod/Zpot.git
cd Zpot
cargo build --release
./target/release/zpot
```

Servidor en **http://localhost:8080** (portal hotspot) y **http://localhost:8081** (admin SPA + API).

Los archivos estáticos se sirven desde disco (hot-reload): editar `static/` o `templates/` y recargar el navegador.

## Arquitectura

```
Navegador ──fetch()──▶ Backend Rust (Axum/Tokio) ──ip/nft/tc/wg/dnsmasq──▶ Sistema
   │                    Puertos 80 (portal) y 8081 (admin)
   └──▶ /static/* (JS, CSS, pages HTML)
   └──▶ /api/* (handlers Rust)
```

## 10 docks

| Dock        | Submenús                          | Página SPA principal |
|-------------|-----------------------------------|----------------------|
| Dashboard   | Dashboard                         | dashboard.html       |
| Interfaces  | List, MWAN, VLANs                 | interfaces.html      |
| IP          | Addresses, Routes, ARP, DHCP, Pools, DNS | ip-addresses.html |
| WireGuard   | Interfaces, Peers                 | wireguard-interfaces.html |
| PPP         | Secrets, Profiles, Active, Logs, Remoto | ppp-secrets.html |
| Hotspot     | Server, Profiles, Cookies, Active, Walled Garden, IP Bindings | hotspot-server.html |
| RADIUS      | Servers                           | radius-servers.html  |
| Firewall    | nftables, Conntrack, Limits/Log   | firewall-nftables.html |
| Bridge      | List, Ports, VLANs                | bridge-list.html     |
| System      | Identity, Resources, Clock, NTP, Users, Scripts, Scheduler, Logs, Files | system-identity.html |

45 páginas SPA + 16 handlers Rust.

## Estructura

```
src/main.rs              ← Entry point, router HTTP (2 servidores: :80 y :8081)
src/handlers/*.rs        ← 16 handlers (interfaces, ip, vlan, wg, ppp, hotspot, mwan, etc.)
templates/base.html      ← Layout principal SPA
static/app-v4.js         ← SPA router (PAGES, sw, lp) + cache API
static/styles/           ← variables.css + main.css
static/components/       ← helpers.js, modal.js, table.js, format-helpers.js
static/pages/*.html      ← 45 páginas, una por vista
static/hotspot/*         ← Portal cautivo (login, alogin, status, logout, redirect, rlogin)
docs/                    ← Documentación por módulo + GUIA-SISTEMA.md
STRUCTURE.md             ← Árbol completo del proyecto
CHANGELOG.md             ← Historial de cambios
```

Ver `STRUCTURE.md` para el árbol completo y **`docs/GUIA-SISTEMA.md`** para la guía
de entrada al sistema (paquetes, configs, estructura admin, lógica del hotspot por
escenarios con el archivo fuente de cada uno).

## Licencia

MIT — Hecho por avisonofgod
