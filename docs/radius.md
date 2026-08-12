# RADIUS Integration — Zpot-RS (PPP + Hotspot)

## Server

| Field | Value |
|---|---|
| IP | 161.97.67.63 |
| Auth port | 1812 (UDP) |
| Acct port | 1813 (UDP) |
| Secret | 85River@B |
| NAS IP | 192.168.10.1 (hotspot) / 192.168.20.1 (PPP) |
| SSH read/write | root / CIEL0AZUL (vmi2598302) — SOLO con autorización explícita |

## Attributes Sent (Access-Request)

| Attr | Type | Value |
|---|---|---|
| User-Name | 1 | From login form / pppd |
| User-Password | 2 | Encrypted with RADIUS secret (PAP) |
| NAS-IP-Address | 4 | 192.168.10.1 / 192.168.20.1 |
| Service-Type | 6 | 2 (Framed) |
| NAS-Port-Type | 61 | 15 (Ethernet) / 5 (Virtual PPP) |
| NAS-Identifier | 32 | "Zpot-Hotspot" / "zpot-nas" |

## Attributes Received (Access-Accept) — PPP

Ejemplo Rosalba@Hu:
```
Framed-IP-Address = 192.168.20.37
Framed-Protocol = PPP
Mikrotik-Rate-Limit = "1M/4M 2M/5M"
WISPr-Session-Terminate-Time = "2027-08-01T23:45:00"
Acct-Interim-Interval = 60
```

## Attr 26 — MikroTik VSA (OUI 14988)

Formato: `rate_up/rate_down ceil_up/ceil_down` — primer par = **rate** (garantía),
segundo par = **ceil** (máximo). Orden dentro de cada par: SUBIDA/BAJADA.
Tokens extra del formato MikroTik completo (burst, priority, rx-min/tx-min) se ignoran.
Sin `/` (formato simple `1M 2M`): rate UP=1M, rate DOWN=2M, ceil=rate.

## Normalización de username (MAYÚSCULAS) — REGLA RELAJADA 2026-08-01

**FreeRADIUS (161.97.67.63) — regla MAYÚSCULAS COMENTADA (2026-08-01):** el bloque
de rechazo (Reply-Message "Solo se permite acceso con nombre en MAYÚSCULAS") fue
comentado en el servidor. Verificado en vivo con radclient (NAS → 161.97.67.63:1812):

| User-Name | Password | Resultado |
|---|---|---|
| RAMONHU | RAMONHU | Access-Accept |
| ramonhu | RAMONHU | Access-Accept (antes Reject) |
| RamonHu | RAMONHU | Access-Accept (antes Reject) |
| RAMONHU | wrongpass | Access-Reject "Checa tu Usuario y Password" (genérico) |

Implicación: la collation de radcheck es case-insensitive (CI) — el lookup con
`ramonhu`/`RamonHu` matchea la fila `RAMONHU`. El riesgo de collation utf8_bin NO
se materializó. Ahora no existe duplicidad posible: todo entra como el mismo user.

**Backend normaliza a MAYÚSCULAS (se mantiene como defensa):**
- `hotspot.rs portal_auth()` — username del form → `trim().to_uppercase()` en el punto de
  entrada (sesión, cookie, nft, accounting usan el normalizado).
- `hotspot.rs radius_auth()` — defensa extra: User-Name del Access-Request siempre en
  MAYÚSCULAS (cubre re-auth de cookies viejas).

**Nota PPP:** el Access-Request de PPP lo genera `pppd + radius.so` (radiusclient),
NO el backend. Con MSCHAPv2 NO reescribir el User-Name (rompe el hash del cliente).
Como la regla MAYÚSCULAS ya no existe, el router del cliente puede mandar el nombre
en cualquier caja y matchea igual (collation CI).

**PITFALL radiusclient.conf (verificado 2026-08-01):** el plugin radius.so de pppd
2.5.2 (Alpine) EXIGE `acctserver` en /etc/radiusclient/radiusclient.conf para que
rc_read_config() cargue. Si falta -> "RADIUS: Can't read config file" y la auth
RADIUS falla (CHAP Failure). Fix aplicado (commit 3f01d35): ppp_radius.rs escribe
`acctserver` SIEMPRE (antes solo si cfg.accounting=true). El plugin hace accounting
Start/Stop por defecto una vez que acctserver existe.

---

# MIGRACIÓN PPP A RADIUS (2026-08-01) — SOLO RADIUS

Estado final: PPP autentica 100% contra FreeRADIUS (como el hotspot).
`fallback_local=false` (auth_order=radius), 33/33 clientes verificados con
MSCHAPv2 real, accounting (interim 60s) en radacct, IPs fijas vía Framed-IP-Address.

## Archivos modificados en el servidor FreeRADIUS (161.97.67.63)

| Archivo | Cambio |
|---|---|
| `/etc/freeradius/3.0/policy.d/filter` | Regla "Realm does not have at least one dot separator" (líneas 64-69) COMENTADA — bloqueaba todos los usuarios con @ en el username (30/33 clientes PPP). Backup: `/tmp/filter.bak.20260801` |
| `/etc/freeradius/3.0/sites-enabled/default` | Ver sección "TODOS los cambios en sites-enabled/default" abajo |
| MySQL `radreply` | 33 filas `Framed-IP-Address := 192.168.20.X` (IPs fijas de los clientes PPP). Backup: `/tmp/radreply.bak.20260801.sql` |
| MySQL `radreply` | `UPDATE DanielGomez Mikrotik-Rate-Limit` — la fila existía vacía; se completó `1M/4M 2M/5M` |
| MySQL `radgroupcheck` | `INSERT ('PPPoE','Simultaneous-Use',':=','1')` — evita 2 sesiones del mismo usuario |
| `/usr/local/bin/radius-close-stale.sh` + cron `*/2 * * * *` | Cierra sesiones radacct huérfanas (sin interim > 4 min) → libera Simultaneous-Use rápido. 267 sesiones históricas cerradas en la 1ª ejecución |
| NAS `/etc/init.d/pppoe` + `/etc/zpot/ppp-radius.json` (Alpine) | Pool pppoe-server MOVIDO a `.100-.200` (`-R 192.168.20.100`) — ver sección PITFALL CRÍTICO abajo. Backups: `.bak.poolfix` |

## TODOS los cambios en /etc/freeradius/3.0/sites-enabled/default

mtime: 2026-08-01 01:52 (última edición). Nada más se tocó después.

1. **AUTHORIZE — `suffix` COMENTADO (línea 374, operador 2026-08-01 01:52)**:
   con comentario `##Cambio paraque no parta @ clientes pppoe comentar`.
   Evita el realm processing en la AUTENTICACIÓN → los nombres con @
   (Rosalba@Hu) se buscan COMPLETOS en radcheck/radreply.
   NOTA: este cambio solo no resolvía el problema real (era el
   filter_username); pero es correcto para el flujo actual.
   `ntdomain` sigue comentado (default).

2. **AUTHORIZE — configuración de contadores PRE-EXISTENTE (setup junio
   daloRADIUS), NO tocada**: el authorize activo es:
   ```
   authorize {
       filter_username
       preprocess
       chap
       mschap
       digest
       eap { ok = return }
       files
       sql {
           notfound = 1
           reject = 2
       }
       if(notfound)   { Reply-Message := "Usuario no encontrado"; reject }
       if(reject)     { Reply-Message := "Checa usuario y contrasena"; reject }
       -ldap
       expiration
       logintime
       access_period
       noresetcounter { reject = 1 }
       if(reject)     { Reply-Message := "Tu tiempo se Acabo"; reject }
       quotalimit
   }
   ```
   Estos son los módulos de TIEMPO PAUSADO (Max-All-Session/noresetcounter),
   TIEMPO CORRIDO (Access-Period/expire_on_login + WISPr deadline) y CUOTAS
   (quotalimit), verificados en las sesiones de junio.

3. **PREACCT — `suffix` ACTIVO (línea 643), NO modificado**: procesa el
   realm en los paquetes de ACCOUNTING. Con él activo, radacct registra
   los usernames con @ COMPLETOS (verificado: 'Rosalba@Hu' en radacct).
   No afecta la autenticación.

## Ejemplo de usuario agregado — Rosalba@Hu (cliente PPP real)

radcheck (autenticación):
```sql
INSERT INTO radcheck (username, attribute, op, value) VALUES
('Rosalba@Hu','Cleartext-Password',':=','Rosalba@Hu');
```

radreply (lo que devuelve el Access-Accept):
```sql
INSERT INTO radreply (username, attribute, op, value) VALUES
('Rosalba@Hu','Framed-IP-Address',':=','192.168.20.37'),
('Rosalba@Hu','Mikrotik-Rate-Limit',':=','1M/4M 2M/5M'),
('Rosalba@Hu','WISPr-Session-Terminate-Time',':=','2027-08-01T23:45:00');
```

radusergroup (perfil):
```sql
INSERT INTO radusergroup (username, groupname, priority) VALUES ('Rosalba@Hu','PPPoE',0);
```

Grupo PPPoE (aplica a todos los PPP):
```sql
INSERT INTO radgroupcheck (groupname, attribute, op, value) VALUES
('PPPoE','Simultaneous-Use',':=','1');
INSERT INTO radgroupreply (groupname, attribute, op, value) VALUES
('PPPoE','Acct-Interim-Interval',':=','60'),
('PPPoE','Framed-Protocol','=','PPP');
```

## Atributos necesarios por usuario PPP (MSCHAPv2)

| Tabla | Attribute | Op | Valor | Propósito |
|---|---|---|---|---|
| radcheck | Cleartext-Password | := | (password exacta, case-sensitive) | permite mschap validar MSCHAPv2 |
| radreply | Framed-IP-Address | := | 192.168.20.X | IP FIJA (única por usuario) |
| radreply | Mikrotik-Rate-Limit | := | "rate_up/rate_down ceil_up/ceil_down" | QoS tc (subida/bajada) |
| radreply | WISPr-Session-Terminate-Time | := | "AAAA-MM-DDTHH:MM:SS" | deadline tiempo corrido (reemplaza Session-Timeout) |
| radgroupreply | Acct-Interim-Interval | := | 60 | interims cada 60s (contadores + detección de sesión muerta) |
| radgroupcheck | Simultaneous-Use | := | 1 | rechaza 2ª sesión del mismo usuario |

OJO: NO usar User-Password como check (solo sirve PAP/hotspot; MSCHAPv2 no lo lee).
OJO: NO usar Idle-Timeout/Session-Timeout (política WISPr).

## Anti-zombies: dos capas complementarias (2026-08-01)

1. **Cron RADIUS** (`radius-close-stale.sh`, cada 2 min): cierra en radacct las
   sesiones sin interim reciente (>4 min) → Simultaneous-Use deja de rechazar la
   reconexión del cliente sin esperar al watchdog del NAS. Cubre pppd crash/CPE mudo.
2. **Timers LCP** en pppoe-server-options (generado por ppp_radius.rs):
   `lcp-echo-interval 5` + `lcp-echo-failure 3` → pppd detecta el enlace muerto
   (~15s) y cierra ORDENADAMENTE (interfaz limpia + Accounting-Stop) → evita que
   el zombie se cree. Precaución: requiere que los CPE respondan al echo.
3. El watchdog local (`scripts/ppp-zombie-watchdog.sh`, cron 2 min) se mantiene
   como última red para interfaces pppN huérfanas en el kernel (problema del
   kernel/pppd, no de auth).

## Refactor del backend (commit 3ff663a) — código PPP antiguo ELIMINADO

Eliminado (522 líneas):
- `/api/ppp/profiles*` (perfiles locales) + struct PppProfile + /etc/zpot-ppp-profiles.json
- `/api/ppp/secrets` POST/update/delete/toggle (gestión local chap-secrets) + sync_secrets/write_chap/write_pap
- `/api/ppp/qos` + `/api/ppp/qos/cleanup` (QoS por perfil local) + get_ppp_speeds
- Rama "Sin RADIUS" del ip-up (write_ip_up)
- Frontend: static/pages/ppp-profiles.html (git rm), menú sin Profiles, ppp-secrets.html solo lectura + Desconectar

Mantenido:
- `/api/ppp/secrets` GET (lectura — active list correlaciona IP→usuario), `/api/ppp/secrets/disconnect`
- `/api/ppp/active`, `/api/ppp/logs`, `/api/ppp/qos/radius` (QoS por VSA), `/api/ip/remote*`
- `ppp_radius.rs` completo (config + apply + status)
- Limpieza de zombies al arranque (main.rs) + watchdog

Timers LCP en el generador (commit pendiente): ppp_radius.rs agrega
`lcp-echo-interval 5` + `lcp-echo-failure 3` a pppoe-server-options.

## PITFALL CRÍTICO — pool pppoe-server DEBE estar FUERA de las IPs fijas (2026-08-01)

**Síntoma en producción:** `/ppp/active` mostraba usuarios con `-` en la columna
USER y IPs corridas/duplicadas (p.ej. `192.168.20.9` en DOS clientes a la vez).

**Causa raíz (verificada):** el pppoe-server corría con `-R 192.168.20.2`
(pool `.2-.200`) que INCLUYE las IPs fijas `.2-.37` del radreply. Las sesiones
que se autenticaron ANTES del INSERT de Framed-IP-Address (~02:40) o que
reconectaron sin que el RADIUS devolviera el atributo quedaron con IP del pool.
El pppoe-server asigna la IP del pool en el cmdline del pppd
(`192.168.20.1:192.168.20.X`) y si el Access-Accept NO trae Framed-IP-Address,
el pppd usa ESA IP (la del pool) → IPs corridas, duplicadas y usuarios `-`.

**Evidencia:** solo 9/33 `/var/run/radattr.ppp*` tenían Framed-IP-Address
(sesiones reconectadas DESPUÉS del INSERT); las 24 viejas no (radattr sin el
atributo → IP del pool). Las reconexiones post-INSERT (03:48+) SÍ recibieron
su IP fija (radattr.ppp28 = .7 para MoisesKaren).

**Fix aplicado (2026-08-01):**
1. `/etc/init.d/pppoe`: `-R 192.168.20.2` → `-R 192.168.20.100` (pool `.100-.200`,
   FUERA del rango fijo `.2-.37`). Backup: `/etc/init.d/pppoe.bak.poolfix`.
2. `/etc/zpot/ppp-radius.json`: `pool_start` → `192.168.20.100` (pool_end .200).
   Backup: `/etc/zpot/ppp-radius.json.bak.poolfix`.
3. `ip link delete` de las interfaces viejas → los CPE reconectan solos
   (backoff progresivo: 7 a los ~2min, 30 a los ~5min). Cada reconexión ahora
   recibe Framed-IP-Address del RADIUS → IP fija.

**Resultado:** 30/30 radattr con Framed-IP-Address, IPs remotas = fijas
(.2-.37, sin duplicadas), `-` desaparece de /ppp/active.

**Mecánica del pppd (por qué el pool importa):** el plugin radius.so aplica la
Framed-IP-Address del Access-Accept SOLO si viene en el paquete. El cmdline del
pppde-server (`-R`) define la IP provisional del pool; si el RADIUS no da IP,
esa provisional es la final. Con el pool dentro del rango fijo, dos usuarios
distintos podían recibir la misma IP (uno por pool, otro por RADIUS).

## PITFALL CRÍTICO — watchdog/cleanup correlacionan pppd por MAC, NO por IP (2026-08-01)

**Síntoma:** clientes PPPoE con routers de fibra SIEMPRE encendidos caían y
reconectaban en loop cada ~8 min (FidencioRivera, MelitoH). El watchdog los
declaraba "ZOMBIE: sin pppd" y eliminaba la interfaz, pero el pppd estaba VIVO.

**Causa raíz (verificada):** el cmdline de pppd contiene la IP PROVISIONAL del
pool (`192.168.20.1:192.168.20.196`), NO la IP final del peer (.23). El
watchdog (`pppd_alive_for`) y el cleanup de arranque de main.rs buscaban la IP
FINAL en el cmdline → nunca coincidía → sesión viva declarada zombie.
Con el pool viejo (.2-.37 ≈ fijas) a veces coincidía por casualidad; con el
pool nuevo (.100-.200) NUNCA coincide → bug determinista.

**Fix (commits a34d048, 8feaecd, 3e34b03):**
1. `ip-up` ahora escribe `/var/run/ppp-mac-$1` con la MAC del peer ($6)
2. `pppd_alive_for` busca `remotenumber <MAC>` en /proc/PID/cmdline (MAC es
   estable y única por CPE). Sin archivo MAC → conservar (NO matar)
3. Cleanup de arranque de main.rs usa el mismo criterio MAC

**Verificación:** watchdog ahora loguea "pppd vivo — sesion activa,
conservando" para tx=107; 33/33 sesiones estables, 0 zombies, 0 "-".

## NAS Attribute Processing

| Attr | Action |
|---|---|
| `Idle-Timeout` (28) | NO usar (política WISPr) |
| `Session-Timeout` (27) | IGNORADO — redundante con WISPr deadline |
| `Acct-Interim-Interval` (85) | 60s — interim frecuente (anti-zombie) |
| `Mikrotik-Rate-Limit` (26/14988) | QoS tc HTB |
| `Reply-Message` (18) | Mostrado al usuario en Reject |
| `WISPr-Session-Terminate-Time` | Deadline absoluto (tiempo corrido) |
