# BUGS + FIXES — HOTSPOT (auditoría por componente)

Fechado, creciente. Se actualiza después de cada auditoría.
Formato: texto, puntos importantes, palabras clave (sin código).

---

## 2026-08-07 — RONDA 3 (verificado contra Alpine REAL)

ESTADO REAL VERIFICADO:
- Config /etc/zpot/hotspot-server.json: iface eth3, gw 192.168.10.1,
  idle 600, shared_users 1, rate_limit 1M/2M 2M/3M, radius
  161.97.67.63:1812, secret EN CLARO, coa_enabled true, coa_mode poll.
- PERMISOS: hotspot-server.json, hotspot-cookies.json, ppp-radius.json
  = 644 (rw-r--r--) con secretos/passwords → chmod 600 pendiente.
- nft tabla inet hotspot VERIFICADA (input policy accept; 8081 solo
  WG/LAN; 80 solo iface/lo/wg0; forward con aislamiento eth3→eth3,
  hotspot→mgmt drop, auth accept; zpot-init.sh fail-closed).
- Sesiones activas: 10 (set hotspot_auth) + cookies server-side.
- CoA: polling HTTP 30s activo (endpoint PHP radacct).

BUGS ABIERTOS / PENDIENTES:
- [MEDIA] Permisos 0644 en archivos con secretos (server/cookies).
- [MEDIA] Secret del CoA viaja en la URL del poll (visible en config).
- [BAJA] coa_poll_url con secret en claro en el JSON de config.

FIXES APLICADOS (rondas anteriores — resumen):
- 28+ fixes (commits 0c04484, 85511fa): anti-bruteforce, octets RFC
  2866 corregidos, cookie sin password, ARP cleanup dedicado con
  de-bounce, polling NO-array cancelado, QoS clamp, init parametrizado,
  zpot-init.sh fail-closed, CoA autenticado MD5.

PALABRAS CLAVE: idle-timeout, shared-users, rate-limit fallback,
coa poll 30s, anti-bruteforce 5/60s, cookie base64 user+mac, ARP
de-bounce 2 muestras, acct 1813 SIEMPRE, VSA QoS.

PRÓXIMA RONDA: chmod 600, secret CoA fuera de URL, revisar cookies
huérfanas tras CoA.
