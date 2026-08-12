# BUGS + FIXES — RADIUS (auditoría por componente)

Fechado, creciente. Se actualiza después de cada auditoría.
Formato: texto, puntos importantes, palabras clave (sin código).

---

## 2026-08-07 — RONDA 1 (verificado contra Alpine REAL)

ESTADO REAL VERIFICADO:
- /etc/zpot/radius-servers.json: VACÍO — el módulo de servidores del
  panel NO se usa en producción (el hotspot usa su propio config).
- /etc/radiusclient/servers: 161.97.67.63 con secret EN CLARO,
  permisos del directorio estándar.
- Server RADIUS: 161.97.67.63 FreeRADIUS3 + daloRADIUS, multi-NAS
  (radacct por IP origen), ssh root DENEGADO.
- Hotspot: auth 1812, accounting SIEMPRE 1813 (por código).
- PPP: auth RADIUS via radiusclient + radattr (accounting off).

BUGS ABIERTOS / PENDIENTES:
- [ALTA] Secret en claro: hardcodeado en fallback del código y en
  hotspot-server.json (644). Fix: chmod 600 + enmascarar en GET.
- [ALTA] NO existe DELETE de servidores ni update por nombre.
- [MEDIA] post_server sin validación (name vacío/dup, ip inválida,
  puerto 0, secret vacío); save_servers ignora errores.
- [BAJA] OnceLock carga 1 vez (cambios externos no se ven); sin
  failover entre múltiples auth.

FIXES APLICADOS (rondas anteriores):
- Octets RFC 2866 corregidos (42=tx/43=rx) en acct del hotspot.
- Timeout RADIUS ≠ reject (reachable flag) en portal_auth.

PALABRAS CLAVE: 1812 auth, 1813 acct SIEMPRE, secret 0600 pendiente,
radiusclient servers, multi-NAS, radacct por IP origen.

PRÓXIMA RONDA: chmod 600, enmascarar secret, DELETE/update servers,
validación POST.

FIXES APLICADOS (2026-08-07, commit 9d7958b):
- [RESUELTO] GET enmascara secret ("***"). VERIFICADO en vivo.
- [RESUELTO] POST valida campos (name/ip/puertos/secret) y actualiza
  por nombre (antes duplicaba).
- PENDIENTE: chmod 600 de radius-servers.json (si existe), DELETE.

## 2026-08-08 — RONDA 2 (auditoría NAS RADIUS completa)

- El sistema ES un NAS RADIUS: TODA la autenticación (hotspot + PPPoE)
  va a FreeRADIUS 161.97.67.63. No hay auth local (fallback_local=false
  y auth_order=radius). Documentado en docs/NAS-RADIUS-ALPINE.md.
- El config del cliente RADIUS de pppd está en /etc/radiusclient/
  (NO radiusclient-ng): authserver :1812, acctserver :1813,
  nas_identifier zpot-nas, timeout 3, retries 2, dictionary con VSA
  Mikrotik (oui 14988, attr 8).
- radius-servers.json VACÍO en prod: el módulo funciona por fallback
  get_default_auth_server (161.97.67.63:1812). Documentado.
- accounting PPP = false (decidido): el NAS PPP NO acctea; el hotspot
  SÍ (interims a :1813).
- [RESUELTO] save_servers atómico (tmp+rename, commit 0bd0505).
- PENDIENTE: DELETE de servidores, failover multi-auth, chmod 600 del
  archivo si algún día se crea en prod.
