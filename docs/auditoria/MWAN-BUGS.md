# BUGS + FIXES — MWAN / BALANCEO (auditoría por componente)

Fechado, creciente. Se actualiza después de cada auditoría.
Formato: texto, puntos importantes, palabras clave (sin código).

---

## 2026-08-07 — RONDA 1 (verificado contra Alpine REAL)

ESTADO REAL VERIFICADO:
- mwan.json: wan1 eth0 (ip .2.102, gw .2.1, mark 1, table 10),
  wan2 eth1 (ip .3.105, gw .3.1, mark 2, table 20), round-robin 50/50.
- ip rule: 1401 fwmark 1 → wan1; 1402 fwmark 2 → wan2.
- nft mwan: prerouting marca tráfico nuevo con jhash; gestión wg0
  exenta; postrouting masquerade por mark.
- Watchdogs en crontab: zpot-watchdog cada minuto (backend),
  ppp-zombie cada 2 min.
- Default route actual: eth0 (192.168.2.1).

BUGS ABIERTOS / PENDIENTES:
- [ALTA] Trap de boot: apply_nft_rules corre antes de init_hotspot_nft
  → WANs adicionales sin masquerade tras reboot.
- [MEDIA-ALTA] Store SIEMPRE dice "up" → 50/50 a WAN caída.
- [MEDIA-ALTA] apply_wan_ip_change: flush ANTES del add → si falla,
  iface sin IPv4; entradas sin validar.
- [MEDIA] check_wan_ping con ping -I depende de la ruta MAIN → WAN
  viva declarada caída; recovery con default equivocada.
- [MEDIA] Watchdog borra TODAS las default x5 antes de replace.
- [MEDIA] weight se ignora (jhash 50/50 fijo).
- [BAJA] get_mwan_status conntrack sin spawn_blocking.

FIXES APLICADOS (rondas anteriores):
- Orden de boot corregido una vez (apply_nft antes de init_hotspot →
  commit fa9a048) — re-verificar persistencia.
- Lección: default route recovery vía eth1 (192.168.3.1).

PALABRAS CLAVE: fwmark 1/2, tablas 10/20, jhash 50/50, watchdog 30s,
ping al gateway, trap boot.

PRÓXIMA RONDA: ping por tabla WAN, status real, aplicar weight,
validar entradas.

FIXES APLICADOS (2026-08-07, commit 9d7958b):
- [RESUELTO] ORDEN DE BOOT: init_hotspot_nft AHORA PRIMERO, luego
  apply_nft_rules (MWAN) — antes la tabla hotspot no existía y el
  masquerade WAN fallaba en silencio. Verificar en próximo reboot.
- PENDIENTE: check_wan_ping dependiente de ruta main, store dice up
  fijo, watchdog borra todas las default, weight ignorado.

FIXES APLICADOS 2 (2026-08-08, commit 1a1d9d0):
- [RESUELTO] check_wan_ping: ping al GATEWAY de la WAN (antes 8.8.8.8
  dependía de la ruta MAIN → falsos "caída").
- [RESUELTO] WEIGHT aplicado: distribution "70/30" → nft numgen random
  mod 100 con rangos (antes jhash 50/50 fijo). VERIFICADO en vivo:
  regla numgen 0-49/50-99 para 50/50.
- PENDIENTE: store dice "up" fijo (POST), watchdog borra TODAS las
  default, validar entradas del POST.

FIXES APLICADOS 3 (2026-08-08, commit 0bd0505):
- [RESUELTO] apply_wan_ip_change: ROLLBACK si el add tras el flush falla
  (antes la iface quedaba SIN IPv4 = WAN perdida).
- [RESUELTO] post_mwan_config: valida iface/ip/gateway de cada WAN antes
  de tocar el sistema. VERIFICADO: eth0;evil y 999.999.1.1 rechazados.
- [RESUELTO] write_state atómico (tmp+rename).
- PENDIENTE: store dice "up" fijo (POST), watchdog borra TODAS las
  default.
