# NARA-MIPS — Gestor ISP (Rust / Axum) para RiverOs

Panel de administración y portal hotspot para redes ISP. Backend Rust (Axum/Tokio) con
frontend SPA vanilla servido desde disco. **Arquitectura: mipsel (MIPS32 LE, MT7621)**.

> Nota de naming: el repo original `NARA` (Alpine/x86) queda intacto; esta variante
> es el clon para **MIPS (mipsel_24kc)** — MikroTik hEX RB750Gr3 con OpenWrt 25.12.5.

> ⚠️ CAPAS SEPARADAS: la capa SO (kernel mejorado, ImageBuilder, scripts de red,
> rename de puertos) vive en el repo **RiverOs** — ver `avisonofgod/RiverOs` (o
> `/root/proyectos/riveros/repo` local). NARA-MIPS es SOLO el gestor ISP que corre
> encima. El rename de puertos a ethX lo hace RiverOs en el boot; NARA lo consume.

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

El rename se hace en el boot por la capa SO RiverOs (`scripts/rename-ports` en el
repo RiverOs, instalado como `/etc/init.d/rename-ports`, START=08, antes de network START=20).
NARA no renombra nada: consume los nombres ethX del sistema.

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
# rename de puertos (capa SO RiverOs): /etc/init.d/rename-ports (ver repo RiverOs)
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
- `scripts/` — init.d-nara-openwrt, helpers de despliegue (rename de puertos en repo RiverOs)
- `docs/`, `MEMENTO.md`, `STRUCTURE.md`, `CHANGELOG.md` — documentación

## Base RiverOs (capa SO — repo separado)

- Kernel 6.12.94, OpenWrt 25.12.5 (apk), device eth0 = 192.168.5.1/24 (consola)
- Repo: avisonofgod/RiverOs (local: /root/proyectos/riveros/repo) — kernel mejorado,
  ImageBuilder (riveros-NETEST 52 paq / NARA-BASE 84 paq), scripts de red y rename ethX
- Bins: riveros-NETEST-25.12.5.bin (md5 fdb90695) y riveros-NARA-BASE-25.12.5.bin
  (md5 d4bf6c76) en `/root/netinstall-openwrt/backups/`
- NARA-BASE v2 = netest + dnsmasq + nftables + tc-tiny + wireguard-tools + kmod-wireguard

## RADIUS y PPPoE (estado 2026-08-12)

- **Hotspot → RADIUS**: nativo en zpot (radius_auth construye Access-Request UDP,
  sin dependencias externas). Config: hotspot server radius=161.97.67.63:1812,
  secret 85River@B. El portal rechaza si no hay servidor configurado.
- **PPPoE → RADIUS**: operativo tras bin NARA-RADIUS (e1665bc8/d453ee33): pppd,
  pppoe-server (rp-pppoe-server), radius.so, dictionary en /etc/ppp/radius/.
  ppp_radius.rs aplica radiusclient.conf + servers + options (require-mschap-v2,
  plugin radius.so; radattr.so solo si existe). Config: POST /api/ppp/radius
  + /api/ppp/radius/apply.
  RECUPERACION tras sysupgrade: el flasheo borra /etc/init.d/rename-ports y
  /etc/nara (zpot/static/configs) — NO estan en sysupgrade.conf. Restaurar desde
  initramfs: mount /dev/mtdblock9, copiar zpot+static a /etc/nara, ln -s libc.so.1,
  reinstalar rename-ports (S08) + S99nara.

## Compatibilidad Alpine (NARA original)

Los handlers usan `src/handlers/helpers.rs` para funcionar en ambos entornos:
- servicio: `rc-service` (Alpine) o `/etc/init.d/<svc>` (OpenWrt)
- conntrack: binario `conntrack` o `/proc/net/nf_conntrack`
- getent: binario o `/etc/hosts` + nslookup
- shell: `/bin/sh` (existe en ambos; OpenWrt no trae bash)

Esto mantiene NARA reproducible en Alpine/x86 (p.ej. N100) y RiverOs/mipsel.
