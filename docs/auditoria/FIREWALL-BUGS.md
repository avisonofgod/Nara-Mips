# BUGS + FIXES — FIREWALL (auditoría por componente)

Fechado, creciente. Se actualiza después de cada auditoría.
Formato: texto, puntos importantes, palabras clave (sin código).

---

## 2026-08-07 — RONDA 1 (verificado contra Alpine REAL)

ESTADO REAL VERIFICADO:
- nft tabla inet hotspot: input policy accept (lo accept; 8081 solo
  WG/LAN; 80 solo iface/lo/wg0; drop final 80/8081), forward con
  aislamiento eth3→eth3, hotspot→mgmt drop, auth accept, ppp
  isolation; prerouting drop 8081 eth3/ppp* + redirect :80;
  postrouting masquerade eth0/eth1.
- nft tabla inet mwan: prerouting marca tráfico nuevo (jhash),
  gestión wg0 exenta; postrouting masquerade con mark.
- ip rule real: 1401 fwmark 1 → wan1, 1402 fwmark 2 → wan2.

BUGS ABIERTOS / PENDIENTES:
- [ALTA] create_nft_rule sin validar table/chain/rule; position insert
  puede meter accept al inicio del forward → bypass aislamiento.
- [ALTA] delete de reglas por handle sin protección → borrar drop 8081,
  redirect 80, isolation o masquerade si se conoce el handle.
- [ALTA] move off-by-one (up sube 2, down no-op); delete+re-add con
  errores ignorados → regla perdida si el re-add falla.
- [MEDIA] Reglas largas multilínea → solo se captura el último
  fragmento → re-add con sintaxis rota.
- [BAJA] Cache muerta; conntrack_status expone tabla completa.

FIXES APLICADOS (rondas anteriores):
- init_hotspot_nft parametrizado con cfg.iface/gw (antes hardcode
  eth3/192.168.10.0/24).
- zpot-init.sh fail-closed real (drop final eth3, auth set concat).

PALABRAS CLAVE: tabla inet hotspot, handles, aislamiento, redirect 80,
masquerade, off-by-one, fail-closed.

PRÓXIMA RONDA: whitelist tables/chains/handles, fix move, validación
de reglas.

FIXES APLICADOS (2026-08-07, commit 9d7958b):
- [RESUELTO] delete_filter_rule PROTEGE reglas críticas (8081, redirect,
  masquerade, drop, accept) — VERIFICADO: handle 1 de forward negado.
- [RESUELTO] move_nft_rule verifica delete y re-add (antes errores
  ignorados = regla perdida silenciosamente).
- PENDIENTE: validar table/chain en create, cache muerta.
