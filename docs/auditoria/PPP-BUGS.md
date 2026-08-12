# BUGS + FIXES — PPP (auditoría por componente)

Fechado, creciente. Se actualiza después de cada auditoría.
Formato: texto, puntos importantes, palabras clave (sin código).

---

## 2026-08-07 — RONDA 1 (verificado contra Alpine REAL)

ESTADO REAL VERIFICADO:
- pppoe-server corriendo (3 procesos: server + pppd), eth3.881.
- 33 sesiones ppp activas (ppp0-ppp32), IPs fijas .2-.37 asignadas.
- /etc/ppp/pppoe-server-options: require-mschap-v2 SOLO, lcp-echo
  5/3, ms-dns 192.168.20.1 + 8.8.8.8, plugin radius.so + radattr.so.
- /etc/zpot/ppp-radius.json: enabled, nas_ip 192.168.20.1,
  fallback_local FALSE, accounting FALSE, pool .100-.200,
  dns1 192.168.20.1, dns2 8.8.8.8.
- /etc/zpot-ppp-secrets.json: 331 líneas (catálogo grande).
- ip-up REAL: guarda MAC en ppp-mac-$1 + llama a QoS por API
  (sin --max-time en el curl → pppd puede bloquearse si backend cuelga).
  CONFIRMADO: ip-up SOLO QoS (los secrets se auto-registran por sync 60s).
- ip-down REAL: llama a cleanup de QoS + elimina la interfaz del kernel
  (ip link delete) → evita zombies al desconectar.
- 33 ifb_ppp* presentes (1 por sesión).

BUGS ABIERTOS / PENDIENTES:
- [ALTA] Bug sed MAC en ppp_radius.rs (ppp-mac con cmdline entero si
  no hay remotenumber) — el ip-up real usa el mismo patrón; cascada:
  uptime "-", disconnect falso, zombies mal limpiados.
- [ALTA] Auto-registro con password vacío → fallback usa username como
  password (auth local bypass).
- [MEDIA] Race load-modify-save sin lock (sync 60s vs handlers) →
  secret borrado resucita.
- [MEDIA] ifb_pppN nunca se elimina (leak; hoy 33 = sesiones, vigilar).
- [MEDIA] ip-up curl sin --max-time (bloqueo pppd).
- [MEDIA] accounting PPP desactivado (accounting false) — revisar si
  se necesita radacct de sesiones PPP.
- [MEDIA] Pool: 3 fuentes de verdad (UI pool_start/end vs hardcode
  del arranque vs dnsmasq) — hoy el pool real es .100-.200 provisional.
- [BAJA] secrets_list devuelve passwords al SPA.

FIXES APLICADOS (rondas anteriores):
- Watchdog de zombies (v20260801): correlación MAC ppp-mac antes de
  matar; sin ppp-mac NO matar.
- Secrets como catálogo (2026-08-02): no sobreescribir IPs fijas.

PALABRAS CLAVE: require-mschap-v2, radattr, Framed-IP ignorado,
pool -R provisional, ppp-mac correlación, ip-up QoS, ip-down delete.

FIXES APLICADOS 2 (2026-08-07, commit 2ee6d1b — sin wireguard):
- [RESUELTO] ppp.rs remote_set: IP restringida a rangos locales
  (192.168.10.x hotspot / 192.168.20.x PPPoE) — antes cualquier IP
  = DNAT abierto. VERIFICADO: 8.8.8.8 rechazada.
- [RESUELTO] scripts/ip-up del REPO sincronizado con el sistema
  (guarda MAC + curl qos/radius con --max-time 5).
- [RESUELTO] ppp_radius.rs write_ip_up: curl con --max-time 5.

FIXES APLICADOS 3 (2026-08-08, commit 1a1d9d0 — globales):
- [RESUELTO] LOCK secrets (SECRETS_LOCK): serializa delete/order/
  auto_register — el sync 60s ya no resucita secretos borrados.
- [RESUELTO] pppoe_start: pool parametrizado desde ppp-radius.json
  (pool_start/pool_end de la UI) — antes -R hardcodeado. Aplica al
  próximo start del pppoe-server (no se reinició en vivo).
- [RESUELTO] qos_cleanup: elimina filtro UP + ifb_pppN (leak).
- [RESUELTO] PASSWORD LOCAL: chap-secrets NUNCA usa username como password
  (bypass) — sin password → secret aleatorio (auth local falla, auth = RADIUS).
- [RESUELTO] save_secrets_to_disk atómico (tmp+rename).
- [RESUELTO] post_config valida nas_ip/dns1/dns2/pool (IPv4). VERIFICADO.
- PENDIENTE: secrets_list devuelve passwords al SPA,
  accounting PPP off (decidir), API create/update secrets.

PRÓXIMA RONDA: fix sed MAC, lock secrets, max-time ip-up, ifb leak,
decidir accounting PPP.
