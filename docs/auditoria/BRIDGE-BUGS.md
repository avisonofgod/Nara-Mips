# BUGS + FIXES — BRIDGE (auditoría por componente)

Fechado, creciente. Se actualiza después de cada auditoría.
Formato: texto, puntos importantes, palabras clave (sin código).

---

## 2026-08-07 — RONDA 1 (verificado contra Alpine REAL)

ESTADO REAL VERIFICADO:
- NO hay bridges reales en el sistema (sin entradas en
  /sys/class/net/*/bridge).
- El listado con salida JSON de iproute2 devuelve 78 objetos vacíos
  (artefacto de la herramienta en Alpine) → la UI no debe fiarse.

BUGS ABIERTOS / PENDIENTES:
- [ALTA] delete: solo bloquea bridgeLan/br0 — el resto (eth0, eth3,
  ppp*, wg*) se puede borrar → caída total. Validar contra bridges
  reales antes de borrar.
- [MEDIA] create: ports sin validar (enslave eth0 = outage); name no
  validado; SIN persistencia (los bridges desaparecen al reboot).
- [BAJA] list devuelve lista vacía con 200 si la salida JSON falla;
  parseo por substring de salida de texto.

FIXES APLICADOS: ninguno (componente pendiente).

PALABRAS CLAVE: bridgeLan, br0, enslave, persistencia, JSON vacío.

PRÓXIMA RONDA: validar delete contra bridges reales, validar ports,
persistencia al boot.

FIXES APLICADOS (2026-08-07, commit 9d7958b):
- [RESUELTO] delete SOLO permite bridges reales (verifica con la lista
  de bridges del sistema) — antes podía borrar eth0/eth3/wg0.
  VERIFICADO: delete eth0 rechazado.
- PENDIENTE: create sin validar ports, SIN persistencia al reboot.
