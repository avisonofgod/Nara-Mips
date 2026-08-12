# PPPoE — Zpot-RS (estado 2026-08-01, post-migración RADIUS)

## Visión general

- Servidor: `pppoe-server` (rp-pppoe) en Alpine 10.7.0.5
- Autenticación: **100% RADIUS** (FreeRADIUS 161.97.67.63), `fallback_local=false`
- IPs: **fijas vía Framed-IP-Address del RADIUS** (radreply, .2-.37)
- Pool pppoe-server: SOLO provisional (.100-.200), ver PITFALL CRÍTICO abajo
- Detalle RADIUS (tablas, VSAs, cambios en servidor): ver `radius.md`

## Server Configuration

### Init.d (`/etc/init.d/pppoe`)

```bash
command="/usr/sbin/pppoe-server"
command_args="-I eth3.881 -N 100 -m 1412 -q /usr/sbin/pppd -L 192.168.20.1 -R 192.168.20.100"
```

| Option | Value | Meaning |
|---|---|---|
| `-I` | eth3.881 | Interface (VLAN 881 on eth3) |
| `-N` | 100 | Max concurrent sessions |
| `-m` | 1412 | MSS clamping |
| `-q` | /usr/sbin/pppd | pppd path |
| `-L` | 192.168.20.1 | Local (gateway) IP |
| `-R` | 192.168.20.100 | **Inicio pool PROVISIONAL** |

### pppd Options (`/etc/ppp/pppoe-server-options`, generado por backend)

```text
require-mschap-v2
lcp-echo-interval 5
lcp-echo-failure 3
ms-dns 192.168.20.1
ms-dns 8.8.8.8
plugin radius.so
plugin radattr.so
```

**CRITICAL**: Only `require-mschap-v2`. Do NOT add `require-chap` or `require-pap`.
Adding them causes pppd to negotiate `<auth chap MD5>` instead of `<auth chap MS-v2>`,
breaking MSCHAPv2 authentication for all clients.

Timers LCP (5s/3 = ~15s): cierre ordenado cuando el CPE deja de responder
(anti-zombie capa 1). Precaución: requiere CPE que responda al echo.

## PITFALL CRÍTICO — pool pppoe-server FUERA de las IPs fijas (2026-08-01)

**Síntoma:** `/ppp/active` con usuarios `-`, IPs corridas/duplicadas
(ej: `192.168.20.9` en DOS clientes), Framed-IP del RADIUS ignorada.

**Causa raíz:** con `-R 192.168.20.2` el pool (.2-.200) INCLUÍA las IPs
fijas (.2-.37). El pppoe-server asigna la IP del pool en el cmdline del pppd
(`192.168.20.1:192.168.20.X`); si el Access-Accept NO trae Framed-IP-Address
(sesiones pre-INSERT de radreply), el pppd usa ESA IP → corridas/duplicadas.

**Fix:** `-R 192.168.20.100` (pool .100-.200, fuera del rango fijo) +
`/etc/zpot/ppp-radius.json` pool_start .100. Al reconectar, el RADIUS da la
Framed-IP y el pppd la aplica (IPCP ConfNak con la fija). Verificado: 33/33
radattr con Framed-IP-Address, 0 duplicadas, 0 "-".

## Flujo de conexión (verificado en syslog)

```
PPPoE discovery (PADI/PADO/PADR)     -> pppoe-server: "Session N created for client MAC"
pppd arranca                          -> "Using interface pppN", plugin radius.so loaded
CHAP/MSCHAPv2 -> Access-Request       -> RADIUS 161.97.67.63:1812
Access-Accept (Framed-IP-Address)     -> "Peer X authenticated with CHAP"
ip-up (radattr.so escribió VSAs)      -> "user X logged in intf pppN remote IP"
   - escribe /var/run/ppp-mac-$1 (MAC peer)
   - POST /api/ppp/qos/radius (QoS por VSA)
IPCP -> remote IP = fija del RADIUS
Accounting Start + Interim 60s        -> radacct (NAS 74.244.101.17, cambia por MWAN)
```

## QoS por RADIUS (rate-limit → tc HTB por cliente) — FIX 2026-08-02

El Access-Accept del RADIUS externo trae `Mikrotik-Rate-Limit = "1M/4M 2M/5M"`
(rate_up/rate_down ceil_up/ceil_down). El pppd lo captura con radattr.so y
`/etc/ppp/ip-up` hace POST a `/api/ppp/qos/radius` → se crea árbol HTB en pppN
(DOWN) + ifb_pppN (UP) con la clase del cliente (rate=ceil aplicado: 5M down).

**Requisito CRÍTICO — dictionary de radiusclient** (`/etc/radiusclient/dictionary`):
sin la entrada Mikrotik, radattr.so OMITE la VSA y no hay QoS. Formato
radiusclient-ng (5º campo = vendor, como dictionary.microsoft):

```
VENDOR		Mikrotik	14988	Mikrotik
ATTRIBUTE	Mikrotik-Rate-Limit	8	string	Mikrotik
```

NO usar `BEGIN-VENDOR/END-VENDOR` (formato FreeRADIUS): la librería lo ignora
y `ATTRIBUTE ... 8 string` colisiona con Framed-IP-Address (attr 8 estándar)
→ radattr escribe la IP como valor de la VSA (bug visto en vivo, c0a8141c).

radattr.so escribe las VSAs CONOCIDAS por nombre (ej. `Mikrotik-Rate-Limit 1M/4M 2M/5M`),
no como `26:<hex>`. qos_radius_apply (ppp.rs) parsea por nombre + fallback `26:`.

Verificado 2026-08-02: 33/33 clientes reconectados con clase HTB 1:(1000+last*2)
rate=ceil_down + filtro `protocol ip prio 100+last` match ip dst. El RADIUS da el
MISMO plan a todos los PPP (1M/4M 2M/5M) aunque el username tenga prefijo (40H@...).

Pendientes resueltos (commits d8754b6/4730fe5/d5a7b6c):
- `ppp-mac-$1` llegaba VACÍO ($6 del ip-up) → el ip-up ahora lee la MAC del
  cmdline del pppd padre: `tr '\0' ' ' < /proc/$PPID/cmdline | sed
  's/.*remotenumber ([0-9a-f:]{17}).*/\1/'` (fallback $6).
- `disconnect_user` dependía del syslog rotado Y usaba `pkill -f '^pppd'`
  (mataba TODOS). Ahora correlaciona secrets→IP→pppN→ppp-mac→remotenumber→PID
  y mata SOLO ese pppd.
- OJO: /proc/PID/cmdline separa args con \0 — convertir con `tr '\0' ' '`
  antes de grep (aplica también a ppp-zombie-watchdog.sh).

## Active Session Detection

API: `GET /api/ppp/active`

1. Lista interfaces via `ip -json addr show type ppp`
2. Peer IP + TX/RX de `/sys/class/net/pppN/statistics/`
3. Username: syslog "logged in" (prioridad) → fallback chap-secrets por IP
4. QoS: se aplica en ip-up vía VSA del radattr

## Watchdog de zombies (`/usr/local/bin/ppp-zombie-watchdog.sh`)

Cron cada 2 min. Elimina interfaces PPP zombies (kernel circular dep cuando
pppd muere sin ip-down). 

### Correlación pppd ↔ interfaz — POR MAC (FIX 2026-08-01 v3, commits a34d048/8feaecd)

**CRÍTICO:** el cmdline de pppd contiene la IP **PROVISIONAL** del pool
(`192.168.20.1:192.168.20.196`), NO la IP final del peer (.23). NUNCA
correlacionar por IP final en cmdline.

- `ip-up` escribe `/var/run/ppp-mac-$1` con la MAC del peer (`$6` calling number)
- `pppd_alive_for()` busca `remotenumber <MAC>` en `/proc/PID/cmdline`
  (la MAC es estable y única por CPE)
- **Sin archivo ppp-mac → NO matar** (conservador: un cliente vivo es peor
  de matar que un zombie temporal)

### Criterio zombie (doble protección)

1. TX = exactamente 107 (8 paquetes LCP = solo keepalive)
2. RX NO activo en muestreo de 2s (sesión REAL = el peer envía datos AHORA)
3. pppd NO vivo (por MAC) → solo entonces `ip link delete`

### Bugs críticos corregidos (historial)

| Fecha | Bug | Fix |
|---|---|---|
| 2026-07-31 | Mataba clientes recién conectados (tx=107 sin verificar pppd) | Protección pppd vivo + edad <5 min |
| 2026-08-01 v2 | Sesión real identificada por TX acumulado (GBs históricos engañaban) | RX ACTIVO (muestreo 2s) |
| 2026-08-01 v3 | Buscaba IP FINAL en cmdline (con pool .100-.200 NUNCA coincide) → mataba clientes VIVOS en loop cada ~8 min (FidencioRivera, MelitoH) | Correlación por MAC (remotenumber) + sin ppp-mac NO matar |

### Verificación post-fix

```
OK: ppp28 peer=192.168.20.23 user=39H@MelitoH tx=107 (pppd vivo — sesion activa, conservando)
```
Sesiones estables 33/33, 0 zombies, 0 "-", 0 duplicadas.

## Cleanup de arranque (main.rs)

Al arrancar el backend elimina interfaces ppp sin pppd. **Mismo criterio MAC**
(FIX 3e34b03): lee /var/run/ppp-mac-$ppp, busca `remotenumber <MAC>` en
cmdline de pppd; sin ppp-mac → conservar. Antes usaba IP final (bug) y mataba
TODAS las interfaces vivas al reiniciar el backend.

## Scripts y archivos relacionados

| Archivo | Propósito |
|---|---|
| `/usr/local/bin/ppp-zombie-watchdog.sh` | Watchdog (cron */2) — fuente: `scripts/ppp-zombie-watchdog.sh` |
| `/etc/ppp/ip-up` | Generado por ppp_radius.rs (write_ip_up): log + ppp-mac + QoS |
| `/var/run/ppp-mac-pppN` | MAC del peer por interfaz (correlación watchdog) |
| `/var/run/radattr.pppN` | Atributos del Access-Accept (Framed-IP, VSAs) |
| `/etc/ppp/pppoe-server-options` | Opciones pppd (generado por backend) |
| `/etc/zpot/ppp-radius.json` | Config RADIUS PPP (enabled, pool, dns) |
| `/etc/radiusclient/radiusclient.conf` | authserver/acctserver (acctserver SIEMPRE, ver radius.md) |

## CHAP Protocol Details

| LCP auth type | pppd config | Works with |
|---|---|---|
| `<auth chap MS-v2>` | `require-mschap-v2` ONLY | All routers (MikroTik, OPNsense, Linux) |
| `<auth chap MD5>` | `require-chap` + `require-mschap-v2` | BROKEN — pppd uses MD5, clients send MSCHAPv2 |

## Servidor PPPoE — toggle ON/OFF (frontend) + arranque con Zpot (2026-08-02)

El pppoe-server se controla desde el frontend (dock **PPP → Server**):

| Endpoint | Método | Función |
|---|---|---|
| `/api/ppp/server/status` | GET | estado: `{"running": bool, "pids": [...]}` |
| `/api/ppp/server/start` | POST | arranca si no corre (idempotente) |
| `/api/ppp/server/stop` | POST | mata pppoe-server (impide nuevas conexiones) |

- Comando fijo: `/usr/sbin/pppoe-server -I eth3.881 -N 100 -m 1412
  -q /usr/sbin/pppd -L 192.168.20.1 -R 192.168.20.100` (mismo del init.d).
- **Solo el servicio**: NO crea VLANs ni IPs (eso lo gestiona el sistema).
- **Arranque al boot**: pegado a Zpot en `/etc/local.d/zpot-red.start`
  (pgrep guard + setsid; arranca ACTIVADO junto con Zpot). Si se apaga desde
  el frontend, al reiniciar vuelve a arrancar (comportamiento deseado).
- **Lección BusyBox**: `pgrep -x pppoe-server` NO matchea — compara contra
  argv[0] completo (`/usr/sbin/pppoe-server`), no contra comm. Usar
  `pgrep/pkill -f '[p]ppoe-server -I'` (corchete evita auto-match del propio
  comando). Mismo caso: `pidof zpot` falla (comm truncado `./target/rel`) →
  `pgrep -f '[t]arget/release/zpot'`.

## Verificación de reinicio del servicio (2026-08-02)

Prueba real con el toggle (PPP → Server):
1. `POST /api/ppp/server/stop` → pppoe-server muere, sesiones caen,
   interfaces ppp limpiadas (watchdog no toca nada indebidamente).
2. `POST /api/ppp/server/start` → server arranca (setsid, args fijos).
3. Reconexión automática de CPEs: 14/33 a los ~2 min, 33/33 a los ~4-5 min.
4. QoS por cliente verificado en sesiones nuevas: clase HTB en pppN
   (rate=ceil_down 5M, flowid por IP final) + filtros en ifb_pppN +
   radattr.pppN con Mikrotik-Rate-Limit "1M/4M 2M/5M" + Framed-IP real.

NOTA: el arranque vía API devolvió una vez running:false sin proceso (fallo
transitorio no reproducido; el comando manual idéntico funcionó y la API lo
detectó). Si vuelve a pasar: verificar `/usr/sbin/pppoe-server` a mano.

## QoS PPP — rate y ceil separados (FIX 424483e, 2026-08-02)

El VSA Mikrotik "1M/4M 2M/5M" se aplicaba ANTES con rate==ceil (5M/2M —
el ceil se usaba como rate). Ahora la clase HTB respeta el VSA completo:

| Dirección | Garantizado (rate) | Máximo (ceil) |
|---|---|---|
| Bajada (pppN egress) | 4Mbit | 5Mbit |
| Subida (ifb_pppN egress) | 1Mbit | 2Mbit |

Verificado en vivo (reconexión real): ppp0 DOWN `rate 4Mbit ceil 5Mbit`,
ifb_ppp0 UP `rate 1Mbit ceil 2Mbit`. apply_qos_ppp ahora recibe los ceils;
qos_radius_apply pasa rate y ceil por separado (antes pasaba el ceil como
rate). El hotspot ya usaba rate+ceil correctamente (no se tocó).

## /ppp/secrets — auto-registro desde la conexión (FIX 2911ba9/199e236)

- ANTES: el JSON /etc/zpot-ppp-secrets.json se llenaba MANUALMENTE.
- AHORA: ip-up llama POST /api/ppp/qos/radius (username, ip=$5=IP del RADIUS,
  iface) y qos_radius_apply hace auto_register_from_connection():
  - Si el username NO existe → se agrega {username, password:"", interface:"*",
    ip: la IP que dio el RADIUS, profile:"ClientesPPP", enabled:true} y se
    regenera chap-secrets.
  - Si existe → solo completa la IP si está vacía/"*" (no sobreescribe IPs
    estáticas asignadas).
- Password: NO disponible en la conexión (MSCHAPv2 solo transmite hash; radattr
  no lo incluye) → campo vacío; la UI NO muestra columna Password.
- Estado Sesión (Conectado/Desconectado): dinámico en la UI (cruza con
  /api/ppp/active). Acción Desconectar: ya existía.
- FIX extra: /api/ppp/qos/cleanup creado (ip-down lo llamaba → 404 silencioso);
  ahora limpia clases/filtros tc al desconectar.

## Auto-registro de /ppp/secrets — sync syslog+kernel (2026-08-02, commit f31d459)
- **NO depende del ip-up**: task periódico en el backend (cada 60s, primer
  tick inmediato) que barre sesiones activas y auto-registra clientes.
- Fuentes: `/var/log/messages` (línea `user X logged in intf pppN ... remote
  <IP>`) + kernel (`ip -json addr show type ppp` para la IP final del RADIUS).
- Idempotente: no duplica, no sobreescribe IPs estáticas (.2-.37 manuales);
  si el cliente ya existe solo completa IP vacía/"*".
- Mismo parse que /api/ppp/active (parse_syslog_users + fetch_ppp_links,
  refactorizados para reutilizarse).
- Verificado en vivo: borrados nato@Hu y Rosalba@Hu del JSON → re-agregados
  por el sync en ≤60s ([PPP-AUTOREG] + [PPP-SYNC] en el log) con su IP del
  kernel (192.168.20.22 / .37); count 33 restaurado.
- El ip-up sigue existiendo para QoS (POST /api/ppp/qos/radius), pero el
  registro ya no depende de él: si el POST falla, el sync lo captura.

## Uptime de sesiones PPP (2026-08-02, commits 4bd0c5e, d533801)

- `/api/ppp/active` ahora devuelve `uptime` real por sesión (antes "-").
- Fuente: starttime del proceso pppd (campo 22 de `/proc/<pid>/stat`,
  CLK_TCK=100) → `uptime_sesion = uptime_total - starttime/100`.
- Correlación por MAC (lección previa): la IP en el cmdline del pppd es la
  **provisional del pool** (192.168.20.100+), NO la final → se usa
  `/var/run/ppp-mac-<iface>` → patrón `remotenumber <MAC>` en el cmdline
  (con corchete en el último dígito para evitar auto-match de pgrep).
- No depende del syslog (que rota). UI PPP>Active ya mostraba la columna
  Uptime; solo el backend la llenaba con "-".

## ID por cliente en /ppp/secrets (2026-08-02, commit fd03947)

- Campo `id` (u32) en PppSecret: identificador unico por usuario.
- Asignacion: menor entero positivo libre (`next_free_id`); al ELIMINAR una
  fila (endpoint POST /api/ppp/secrets/delete) el id queda HUECO y el
  proximo cliente nuevo sin registro (auto-registro / sync) lo reutiliza.
- Migracion: clientes existentes sin id reciben 1..n en orden al cargar
  (load_secrets_json -> assign_missing_ids -> persiste).
- UI PPP>Secrets: columna ID (primera) ordenada 1..n hacia abajo
  (sort por id en el frontend, porque los re-agregados quedan al final
  del JSON) + accion Eliminar (confirm + POST delete).
- Verificado en vivo: eliminado 2412@Renau@Huayal (id=3) por el endpoint →
  id 3 libre → el sync lo re-agrego en <=60s CON id=3; tabla 1..33 ordenada.

## Fix rotacion syslog + reordenar filas (2026-08-02, commit 14df6a6)

- FIX: parse_syslog_users lee TAMBIEN /var/log/messages.0 (rotado) — antes
  solo /var/log/messages; si el syslog rotaba, los clientes conectados desde
  antes de la rotacion no tenian linea y el sync no los veia (no re-registraba
  a los eliminados). Ahora: messages.0 -> messages (las nuevas sobreescriben).
- Correlacion por IP ademas de por intf: la linea trae "remote <IP>"; la IP es
  estable por sesion (el intf pppN se recicla entre reconexiones).
- Verificado: eliminado RamonHu (conectado desde 05:52, linea rotada a
  messages.0) -> con el fix el sync lo re-agrego en <=60s con su id 23
  (el hueco) y su IP .29; active lo muestra por nombre (antes "-").
- Drag and drop en PPP>Secrets (estilo /firewall/nftables): handle por fila,
  arrastrar para reordenar; POST /api/ppp/secrets/order {order:[usernames]}
  persiste el orden en el JSON (no toca ids). La UI muestra orden por id
  (1..n) hasta el primer arrastre (ordenManual), luego respeta el guardado.
