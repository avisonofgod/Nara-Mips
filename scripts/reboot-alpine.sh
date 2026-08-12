#!/bin/sh
# REBOOT ALPINE — reinicia el servidor realmente (desde /system/scripts, ▶ Ejecutar)
# ⚠️ Desconecta TODOS los clientes (PPP + hotspot) y el panel.
# El sistema vuelve solo al boot (zpot-red.start levanta interfaces + zpot + pppoe-server).
logger -t zpot "REBOOT solicitado desde el panel (system/scripts)"
echo "[zpot-reboot] $(date -Is) REINICIANDO ALPINE..." >> /var/log/zpot-reboot.log
sync
sleep 1
reboot
