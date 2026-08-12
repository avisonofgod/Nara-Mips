# BUGS + FIXES — SYSTEM / COMMAND (auditoría por componente)

Fechado, creciente. Se actualiza después de cada auditoría.
Formato: texto, puntos importantes, palabras clave (sin código).

---

## 2026-08-07 — RONDA 1 (verificado contra Alpine REAL)

ESTADO REAL VERIFICADO:
- hostname: localhost.
- /etc/local.d/zpot-red.start (boot del zpot).
- crontab: zpot-watchdog (1 min), ppp-zombie-watchdog (2 min),
  run-parts periódicos estándar.
- scripts del repo: install-ppp-qos.py, interfaces.alpine, ip-up,
  ip-down, ppp-zombie-watchdog.sh, reboot-alpine.sh,
  verify-after-reboot.sh, zpot-watchdog.sh.
- /etc/zpot/*.json: todos 644 (incluye secretos).

BUGS ABIERTOS / PENDIENTES:
- [ALTA] /api/command + TODA la API admin sin auth → RCE root desde
  WG/LAN (P0 global). Única barrera: nft input (borrable vía API).
- [ALTA] command.rs whitelist por starts_with/ends_with sin
  canonicalizar → traversal con ../.
- [MEDIA] substring "reboot" → fire-and-forget (DoS); run_script sin
  timeout (request cuelga).
- [MEDIA] cmd_* pool/dhcp sin validar → inyección dnsmasq; remove deja
  dhcp-range huérfana.
- [BAJA] system.rs usa bash (dependencia no declarada); slice por
  bytes puede panic con UTF-8; botones scripts/scheduler UI rotos.

FIXES APLICADOS: ninguno (componente pendiente).

PALABRAS CLAVE: API sin auth, whitelist traversal, substring reboot,
timeout, bash dependency, zpot-watchdog.

PRÓXIMA RONDA: auth API, canonicalizar whitelist, timeouts,
system sin bash.

FIXES APLICADOS (2026-08-07, commit 9d7958b) — command.rs:
- [RESUELTO] Whitelist de scripts CANONICALIZADA (traversal ../ negado).
- [RESUELTO] Fire-and-forget SOLO para reboot-alpine.sh (nombre exacto);
  run_script con timeout 30s.
- [RESUELTO] cmd_wireguard_add: rollback si wg set falla (sin zombie).
- PENDIENTE: auth global API :8081 (P0 mayor), system.rs bash
  dependency + UTF-8 panic, botones scripts/scheduler UI.

FIXES APLICADOS 2 (2026-08-07, commit 2ee6d1b):
- [RESUELTO] system.rs: bash→sh (Alpine sin bash); UTF-8 panic del
  src_preview (byte 80) corregido con chars().take(80).
- PENDIENTE: auth global API :8081 (único P0 mayor), botones
  scripts/scheduler UI (dependen del auth).

## 2026-08-08 — RONDA 2 (auditoría NAS RADIUS — verificado Alpine REAL)

- DNS local = **unbound** :53 (resolver) + dnsmasq :5353 (solo DHCP eth3).
  Antes se asumía dnsmasq en :53 (corregido en docs/NAS-RADIUS-ALPINE.md).
- pppoe-server activo: `-I eth3.881 -N 100 -m 1412 -R 192.168.20.100`
  (pool provisional viejo — el pool de la UI (100-200) aplica al próximo
  restart; ver PPP-BUGS).
- 33 sesiones PPP, IPs fijas .2-.37 (Framed-IP del RADIUS); el pool .100+
  es solo provisional para el handshake (ip-up cambia a la IP final).
- ip-up/ip-down reales: guardan MAC, QoS por API con --max-time 5,
  `ip link delete` anti-zombie. Sincronizados con scripts/ del repo.
- nft: SOLO 2 tablas (hotspot + mwan); iptables legacy vacío.
- Configs con secretos: hotspot-server.json, cookies, wg-peers → chmod 600.
- PENDIENTE: chmod 600 de /etc/radiusclient/servers y pap-secrets.
