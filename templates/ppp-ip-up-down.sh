#!/bin/sh
# Copiar a /etc/ppp/ip-up
# Zpot-RS — NOTA: usar DOBLE comillas para expansion de variables shell
logger -t ppp "user $PEERNAME logged in intf $1 local $4 remote $5"

# Notificar a Zpot-RS para aplicar QoS segun perfil
curl -s -X POST http://localhost:8081/api/ppp/qos \
  -H "Content-Type: application/json" \
  -d "{\"username\":\"$PEERNAME\",\"ip\":\"$5\",\"iface\":\"$1\"}"
