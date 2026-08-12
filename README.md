# NARA — Gestor ISP (Rust / Axum) para RiverOs

Panel de administración y portal hotspot para redes ISP. Backend Rust (Axum/Tokio) con
frontend SPA vanilla servido desde disco. Orientado a MikroTik hEX RB750Gr3 (MT7621,
mipsel_24kc) corriendo OpenWrt 25.12.5 reducido (RiverOs).

## Arquitectura

```
Navegador ──fetch()──▶ NARA (Rust/Axum) ──ip/nft/tc/wg/dnsmasq──▶ Sistema
   │                   puertos 80 (hotspot) y 8081 (admin/API)
   └──▶ /static/* (JS, CSS, pages HTML)
   └──▶ /api/* (handlers Rust)
```

- Portal hotspot (login/status): http://IP:80
- Admin (SPA + API): http://IP:8081
- Los archivos estáticos se sirven desde disco (`static/`, `templates/`).

## Nombres de interfaz (RiverOs)

OpenWrt 25.12 DSA nombra los puertos físicos del hEX como `wan` y `lan2..lan5`,
y expone el puerto CPU del switch como `eth0`. NARA normaliza esto en la capa de
presentación:

| Presentación NARA | Nombre DSA OpenWrt | Puerto físico |
|---|---|---|
| eth0 | wan | ether1 (consola, IP 192.168.5.1) |
| eth1 | lan2 | ether2 |
| eth2 | lan3 | ether3 |
| eth3 | lan4 | ether4 |
| eth4 | lan5 | ether5 |

El puerto CPU del switch (`eth0` DSA) se oculta del listado (no es cableable).
En el JSON de `/api/interfaces`:
- `name` = nombre de presentación (`eth0`..`eth4`)
- `real` = nombre real del kernel (`wan`, `lan2`..`lan5`)

Los selects del frontend usan `real` como valor de opción (para los comandos
`ip`/`nft`/`tc`) y `name` como etiqueta visible. Implementación: `src/naming.rs`.

## Seguridad admin :8081

El admin se protege con nft (chain `input` de la tabla `hotspot`): solo se acepta
TCP 8081 desde las subredes de la whitelist (ver `src/main.rs`, función de setup):

- 10.7.0.0/24 (WireGuard interno)
- 192.168.2.0/24
- 192.168.3.0/24
- 192.168.5.0/24 (consola RiverOs ether1)

El portal :80 solo se sirve a la interfaz hotspot, `lo` y `wg0`.

## Compilar (cross mipsel)

```bash
# Requiere toolchain mipsel musl (zig-mipsel-linker.sh apunta a /home/toolchains/...)
cargo build --release --target mipsel-unknown-linux-musl
# Binario: target/mipsel-unknown-linux-musl/release/zpot
```

Config de build en `.cargo/config.toml`:
- linker: `zig-mipsel-linker.sh` (fuerza `-static`, quita `-pie/-Bdynamic/-lgcc_s`)
- `relocation-model=static`, `target-cpu=1004kc` (MT7621)

Nota: `PROJ_DIR` (ruta de `static/` y `templates/`) se embebe en compilación
(`env!("CARGO_MANIFEST_DIR")`). En el router debe existir esa ruta con symlinks:

```sh
mkdir -p /home/naram
ln -sfn /etc/nara/static /home/naram/static
ln -sfn /etc/nara/templates /home/naram/templates
ln -sf /lib/libc.so /usr/lib/libc.so.1   # intérprete esperado por el binario
```

## Despliegue en RiverOs (OpenWrt 25.12.5)

```sh
# Paquetes base requeridos (bin NARA-BASE):
#   nftables tc wireguard-tools dnsmasq ip-full  (+ kmods nft/wireguard)
mkdir -p /etc/nara
cp zpot /etc/nara/zpot && chmod +x /etc/nara/zpot
cp -r static /etc/nara/static
cp -r templates /etc/nara/templates
# init script: /etc/init.d/nara (START=99, procd respawn)
```

Acceso SSH consola: `ssh root@192.168.5.1` (password en shadow del router).
Admin: http://192.168.3.1:8081 (o la IP de consola configurada dentro de la whitelist).

## Estructura

- `src/main.rs` — arranque, rutas, firewall nft (hotspot + admin), tareas de fondo
- `src/handlers/` — API: interfaces, ip-addresses, routes, mwan, vlans, bridges,
  wireguard, ppp, ppp-radius, hotspot, radius, dns, pools, firewall, system, arp
- `src/naming.rs` — normalización de nombres de interfaz (eth0..eth4)
- `static/` — frontend SPA (app.js, pages/, components/, hotspot/)
- `templates/` — base.html + scripts ppp
- `scripts/`, `*.sh` — helpers de despliegue (accel-ppp, proxy, zpot-init)
- `docs/`, `MEMENTO.md`, `STRUCTURE.md`, `CHANGELOG.md` — documentación histórica

## Base RiverOs

- Kernel 6.12.94, OpenWrt 25.12.5 (apk), device 'wan' = 192.168.5.1/24
- Bins: riveros-NETEST (52 paq) y riveros-NARA-BASE (84 paq, + nft/tc/wg/dnsmasq)
  en `/root/netinstall-openwrt/backups/`
