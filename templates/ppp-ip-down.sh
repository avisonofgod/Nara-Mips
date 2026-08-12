#!/bin/sh
# Limpiar QoS y eliminar interfaz zombie al desconectarse
curl -s -X POST http://localhost:8081/api/ppp/qos/cleanup \
  -H "Content-Type: application/json" \
  -d "{\"ip\":\"$5\",\"iface\":\"$1\"}"
ip link delete dev "$1" 2>/dev/null || true
