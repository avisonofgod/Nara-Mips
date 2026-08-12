# PENDIENTES / REVISIONES — /root/revisa.md (2026-08-08)

Estado de cada punto del archivo de revisión del operador. Fechado, creciente.

---

## 1. FreeRADIUS: "Ignoring duplicate packet ... unfinished request ... module sql" — INVESTIGADO

- Warning REAL y ACTIVO en /var/log/freeradius/radius.log (FreeRADIUS 3.0.21).
- Causa: el server TARDA en procesar el authorize (consulta SQL lenta) y el
  NAS (Alpine, radiusclient timeout 3 / retries 2) REENVÍA el Access-Request
  desde otro puerto origen; FreeRADIUS descarta el duplicado porque la
  request original sigue abierta.
- NO es error de autenticación: los paquetes se ignoran y la auth original
  se procesa. Indica LATENCIA del server SQL (daloRADIUS/MySQL cargado).
- Acción sugerida (no aplicada): revisar la carga del server RADIUS /
  optimizar radcheck queries. Sin impacto funcional.

## 2. ifb0 / ifb1 en /interfaces/list — RESUELTO (commit pendiente)

- ifb0/ifb1 existen en el kernel (módulo ifb del QoS hotspot, state DOWN,
  qdisc noop — NO usadas) y aparecían en /api/interfaces.
- FIX: list_interfaces() las excluye (ifb0, ifb1, ifb_pppN) — son internas
  del QoS, no configurables.

## 3. /system/general: agregar Kernel/CPU/Memoria/Disco/Tiempo + quitar
   identity/Resources/Clock/Ntp — PENDIENTE (requiere rediseño UI)

- Agregar: Kernel, Architecture, cpu, cpu-cores, CPU Load (5m),
  Total/Used/Free Memory, Total/Used/Free Disk /, time, timezone.
- Eliminar docks: identity, Resources, Clock, Ntp. Dejar: User, Script,
  Scheduler, Logs, Files.
- El backend system.rs ya expone datos del sistema (bash→sh, UTF-8 fix);
  falta la página (system-identity.html) y el mapeo de docks.

## 4. zpot-watchdog.sh — INVESTIGADO: NO es huérfano, es el supervisor

- Se ejecuta via cron CADA MINUTO (`* * * * * /usr/local/bin/zpot-watchdog.sh`).
- Función: si zpot crashea, lo relanza en <=60s con el mismo comando del boot.
- IDÉNTICO al repo (diff OK). NO es redundante: zpot NO tiene supervisión
  nativa (no es rc-service), el cron es el mecanismo de respaldo.
- ppp-zombie-watchdog.sh (cron cada 2 min, /usr/local/bin) también activo e
  idéntico al repo — anti-zombies PPP (v20260801-watchdog-mac).
- CONCLUSIÓN: ambos watchdogs son necesarios; se quedan.

## 5. /system/logs — REVISAR (pendiente de verificación UI)

- El endpoint /api/system/logs existe; la página system-logs.html lo muestra.
- Pendiente: verificar en vivo que muestre log del sistema + servicios +
  errores (logread). (Anotado para próxima ronda.)

## 6. /system/files: hotspot subir/descargar + respaldo — PENDIENTE (grande)

- Mostrar /root/zpot-rs/static/hotspot en system-files.
- Subir la carpeta hotspot COMPLETA (con respaldo previo de la actual).
- Botón descargar carpeta completa (tar.gz).
- Reemplazar al subir. El hotspot actual está TERMINADO y funcional —
  respaldar ANTES de cualquier prueba (tar czf hotspot-YYYYMMDD.tgz).

## 7. /system/files: export/import config por componente (JSON) — PENDIENTE

- Exportar/importar la configuración actual del sistema por sección o
  componente en JSON (hotspot-server.json, ppp-radius.json, mwan.json,
  radius-servers.json, pools/dnsmasq, secrets...).
- Diseño sugerido: GET /api/backup/<componente> → JSON; POST para importar
  con validación y rollback.

## 8. /system/scripts: huérfanos — INVESTIGADO

- INSTALL-PPP-QOS.PY = HUÉRFANO (no referenciado en cron/local.d/init.d/
  usr/local/bin). El QoS lo aplica zpot vía API (/api/ppp/qos/radius) —
  el script python es del sistema viejo. CANDIDATO a eliminar del repo.
- reboot-alpine.sh / verify-after-reboot.sh: herramientas MANUALES de
  operación (no referenciadas en cron) — se mantienen como utilidades.
- ip-up/ip-down: USADOS (sincronizados con el sistema real).
- zpot-watchdog.sh / ppp-zombie-watchdog.sh: USADOS (cron) — ver punto 4.

## 9. /ppp/logs: auth RADIUS (accept/rechazada/error/IP) — RESUELTO

- NUEVO /api/ppp/logs/auth: filtra /var/log/messages por pppd/radius +
  auth/accept/reject/failed/timeout/MSCHAP/ip-up. Botón "🔐 Auth RADIUS"
  en ppp-logs.html (errores en rojo).

## 10. /hotspot/logs: auth RADIUS del portal — RESUELTO

- NUEVO /api/hotspot/logs: lee /tmp/zpot.log (login/BYPASS/ACCT/INTERIM/
  COA/REJECT) + página hotspot-logs.html + enlace Logs en el subnav del
  dock Hotspot.

## 11. HALLAZGO (2026-08-08): "Interim accounting failed" en pppd — MITIGADO

- El pppd (plugin radius.so) manda INTERIMS de accounting SIEMPRE (usa el
  acctserver del radiusclient.conf) aunque ppp-radius.json tenga
  accounting=false. El plugin 2.5.2 NO expone flag local para desactivarlo
  (strings: solo radius_timeout/radius_retries).
- El "failed" es por LATENCIA del server (la respuesta tarda > timeout 3s —
  consistente con el "duplicate packet" del FreeRADIUS, punto 1).
- MITIGACIÓN (08-08): radius_timeout 3 → 6 en radius.rs (fallback) y en
  /etc/radiusclient/radiusclient.conf (backup .bak-20260808). Aplica a las
  NUEVAS sesiones ppp (los pppd actuales conservan el timeout viejo hasta
  reconectar).
- SOLUCIÓN COMPLETA (si se decide NO acctear PPP): quitar
  Acct-Interim-Interval=60 del Access-Accept en FreeRADIUS (el plugin solo
  manda interims si el server lo indica). Acción del operador en el server
  161.97.67.63 — NO tocar acctserver del conf (rompe la carga del plugin).
- EXTENSIÓN (08-08): causa raíz del "duplicate packet" (punto 1) =
  reauth del hotspot reauticando TODAS las sesiones en el mismo ciclo
  (300s). APLICADO: reauth espaciado por sesión `(cycle + session_idx) % 5`
  + timeout del cliente RADIUS del hotspot 3 → 6s (hotspot.rs). Cada sesión
  se reautica cada 300s pero en ciclos distintos. Server NO tocado
  (Access-Accept GLOBAL intacto).
