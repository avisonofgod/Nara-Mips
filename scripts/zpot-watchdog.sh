#!/bin/sh
# zpot-watchdog.sh — Relanza zpot si el proceso muere (crash/panic).
# Zpot-RS — se instala en /usr/local/bin y se ejecuta via cron cada minuto.
#
# Cubre "que no se caiga": si zpot crashea, se relanza en <=60s con el
# MISMO comando que zpot-red.start (boot). El estado se restaura solo
# (sesiones/cookies desde /etc/zpot/*.json, nft via init_hotspot_nft).
#
# NOTA: NO detecta hang (proceso vivo colgado) — ese problema se resolvio
# de raiz (spawn_blocking/tokio async, FIX-1..8). Si un dia reaparece,
# anadir un check HTTP: curl -sf localhost:8081 con timeout y relanzar.

# pgrep -f con [c]orchete: no matchea el cmdline de ESTE script (el patron
# regex "[t]arget" no coincide con la cadena literal "[t]arget").
pgrep -f "[t]arget/release/zpot" >/dev/null || {
  logger -t zpot-watchdog "zpot caido — relanzando"
  cd /root/zpot-rs || exit 1
  nohup ./target/release/zpot > /dev/null 2>&1 &
  sleep 2
  pgrep -f "[t]arget/release/zpot" >/dev/null && logger -t zpot-watchdog "zpot relanzado OK" || logger -t zpot-watchdog "FALLO al relanzar zpot"
}

exit 0
