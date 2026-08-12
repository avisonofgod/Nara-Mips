# Revisiones Post-Reboot — Zpot-RS

> Documento ACUMULATIVO: cada vez que Alpine se reinicia, se ejecuta
> /usr/local/bin/verify-after-reboot.sh y se registra el resultado aqui.
> Script de verificacion: scripts/verify-after-reboot.sh (47 checks).
> Criterio: 47 OK / 0 FAIL = sistema completo. Cualquier FAIL se investiga
> y se anota en la entrada con su resolucion.

---

## REVISION 2026-08-04 01:18 — reboot 01:15:51 (panel reboot-alpine.sh)

MOTIVO: test de supervivencia a reinicio (solicitado por el operador).

RESULTADO: 45 OK / 2 FAIL (primera pasada) -> FIX-H7 -> 47 OK / 0 FAIL.

HALLAZGOS Y RESOLUCIONES:
- FAIL QoS tc eth3 (0 clases htb) + ifb_eth3 sin qdisc:
  BUG REAL (FIX-H7): el kernel BORRA los qdisc tc al REBOOT del sistema.
  El comentario viejo en restore_sessions_from_disk decia que persistian,
  pero eso solo es cierto para restarts de zpot (kernel vivo). Las sesiones
  hotspot se restauraban (nft + store) pero los clientes navegaban SIN
  limite de velocidad. Fix: guardar up_ceil_str/down_ceil_str en
  HotspotSession (serde default) y re-aplicar apply_qos en
  restore_sessions_from_disk. Commit 4c159b7.
  VERIFICADO: 0 clases tras reboot -> 10 clases tras restore.

- Observacion PPP: al momento de la revision (2 min post-boot) solo 14/33
  clientes PPP reconectados; el resto reconecta solo (sus routers PPPoE).
  No es bug — tiempo de reconexion de los CPE.

ESTADO POST-FIX (verify-after-reboot.sh):
- 47 OK / 0 FAIL — SISTEMA COMPLETO
- zpot pid 6095, admin 200, portal 302, cola 0/128, watchdog cron
- WG ping 165ms, wg0 up, ruta + proteccion nft
- eth0/eth1/eth3/eth3.881/wg0 IPs OK
- tablas nft hotspot + mwan, chain input (FIX-9), set auth 8 elems
- hotspot: 8 sesiones, 37 cookies, QoS re-aplicado
- pppoe-server up, secrets 33, chap-secrets 34, /dev/ppp
- balanceo WAN: 2 up, active_wan=wan1, PPP wan1=1052 / wan2=1006 (51/49)
- dnsmasq, ntpd, crond, mwan-agent up
- zpot-reboot.log entrada 01:15:51, sin panics

---

<!-- PLANTILLA PARA PROXIMAS REVISIONES:
## REVISION YYYY-MM-DD HH:MM — reboot HH:MM (motivo)
MOTIVO:
RESULTADO: XX OK / XX FAIL
HALLAZGOS Y RESOLUCIONES:
-
ESTADO POST-FIX:
-
-->
