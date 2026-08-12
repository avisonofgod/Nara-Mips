# BUGS + FIXES — DASHBOARD (auditoría por componente)

Fechado, creciente. Se actualiza después de cada auditoría.
Formato: texto, puntos importantes, palabras clave (sin código).

---

## 2026-08-07 — RONDA 1 (verificado contra Alpine REAL)

ESTADO REAL VERIFICADO:
- Dashboard = solo lectura (system, ppp, hotspot, interfaces).
- Sin bugs críticos encontrados en la revisión de lectura.
- Datos provienen de las APIs de cada módulo (misma fuente que el resto).

BUGS ABIERTOS / PENDIENTES:
- [BAJA] "active_wan" del Dashboard MWAN = primer "up" del HashMap
  (orden aleatorio).
- [BAJA] count_all_client_ips mezcla tráfico admin con clientes.

FIXES APLICADOS: ninguno (componente pendiente).

PALABRAS CLAVE: solo lectura, conntrack, total_unique, WAN flujos.

PRÓXIMA RONDA: orden determinista de WANs, separar tráfico admin.
