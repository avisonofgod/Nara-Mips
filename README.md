# NARA-MIPS — Gestor ISP (Rust / Axum) para RiverOs

Panel de administración y portal hotspot para redes ISP. Backend Rust (Axum/Tokio) con
frontend SPA vanilla servido desde disco. **Arquitectura: mipsel (MIPS32 LE, MT7621)**.

> Nota de naming: el repo original `NARA` (Alpine/x86) queda intacto; esta variante
> es el clon para **MIPS (mipsel_24kc)** — MikroTik hEX RB750Gr3 con OpenWrt 25.12.5.

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

## Nombres de interfaz (RiverOs) — ethX neutro a nivel kernel

RiverOs renombra los puertos DSA del hEX a una forma neutra `ethX` en el kernel
(no hay conceptos "wan"/"lan" como en OPNsense — los puertos son solo puertos y
cada cliente los configura a su gusto desde el frontend):

| Nombre kernel | Puerto físico | Uso |
|---|---|---|
| eth0 | ether1 | Consola / acceso físico (IP 192.168.5.1) |
| eth1 | ether2 | Libre (configurable) |
| eth2 | ether3 | Libre (configurable) |
| eth3 | ether4 | Libre (configurable) |
| eth4 | ether5 | Libre (configurable) |
| sw0 | — | Puerto CPU del switch (interno, oculto, no cableable) |

El rename se hace en el boot con `scripts/rename-ports-openwrt.sh`
(`/etc/init.d/rename-ports`, START=08, antes de network START=20).

En el JSON de `/api/interfaces`:
- `name` = ethX (presentación, sin sufijo @master)
- `real` = ethX (nombre válido para comandos ip/nft/tc; limpio de `@sw0`)

Implementación: `src/naming.rs` (display_name = identidad + limpia sufijo DSA;
`is_cpu_port` oculta `sw0`).

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
# Requiere toolchain mipsel musl + nightly (build-std)
RUSTUP_TOOLCHAIN=nightly cargo build --release --target mipsel-unknown-linux-musl \
  -Zbuild-std=std,panic_abort
# Binario: target/mipsel-unknown-linux-musl/release/zpot
```

Config de build en `.cargo/config.toml`:
- linker: gcc musl-cross (binario DINAMICO musl: interpreter `/usr/lib/libc.so.1`,
  NEEDED libgcc_s.so.1 + libc.so — replica del release que corre en el router)
- `target-cpu=1004kc` (MT7621), `-Wl,-dynamic-linker=/usr/lib/libc.so.1`

Nota: `PROJ_DIR` (ruta de `static/` y `templates/`) se define en compilación:
por defecto `/home/naram` (sobreescribible con `NARA_PROJ_DIR`). En el router
debe existir esa ruta con symlinks:

```sh
mkdir -p /home/naram
ln -sfn /etc/nara/static /home/naram/static
ln -sfn /etc/nara/templates /home/naram/templates
ln -sf /lib/ld-musl-mipsel-sf.so.1 /usr/lib/libc.so.1   # intérprete esperado por el binario
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
# rename de puertos: scp scripts/rename-ports-openwrt.sh -> /etc/init.d/rename-ports
#   chmod +x; /etc/init.d/rename-ports enable
```

Acceso SSH consola: `ssh root@192.168.5.1` (ether1). Ante un rename en caliente,
usar IP de respaldo en otro puerto (p.ej. 192.168.10.1/24) antes de tocar.

Admin: http://192.168.3.1:8081 (o la IP de consola configurada dentro de la whitelist).

## Estructura

- `src/main.rs` — arranque, rutas, firewall nft (hotspot + admin), tareas de fondo
- `src/handlers/` — API: interfaces, ip-addresses, routes, mwan, vlans, bridges,
  wireguard, ppp, ppp-radius, hotspot, radius, dns, pools, firewall, system, arp,
  helpers (compat Alpine/OpenWrt: rc-service->init.d, conntrack->/proc, getent->nslookup)
- `src/naming.rs` — nombres de interfaz ethX (oculta cpu port sw0)
- `static/` — frontend SPA (app.js, pages/, components/, hotspot/)
- `templates/` — base.html + scripts ppp
- `scripts/` — rename-ports-openwrt.sh, helpers de despliegue
- `docs/`, `MEMENTO.md`, `STRUCTURE.md`, `CHANGELOG.md` — documentación

## Base RiverOs

- Kernel 6.12.94, OpenWrt 25.12.5 (apk), device eth0 = 192.168.5.1/24 (consola)
- Bins: riveros-NETEST (52 paq) y riveros-NARA-BASE (84 paq, + nft/tc/wg/dnsmasq)
  en `/root/netinstall-openwrt/backups/`

## Compatibilidad Alpine (NARA original)

Los handlers usan `src/handlers/helpers.rs` para funcionar en ambos entornos:
- servicio: `rc-service` (Alpine) o `/etc/init.d/<svc>` (OpenWrt)
- conntrack: binario `conntrack` o `/proc/net/nf_conntrack`
- getent: binario o `/etc/hosts` + nslookup
- shell: `/bin/sh` (existe en ambos; OpenWrt no trae bash)

Esto mantiene NARA reproducible en Alpine/x86 (p.ej. N100) y RiverOs/mipsel.
