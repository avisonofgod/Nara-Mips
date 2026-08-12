#!/bin/sh
while true; do
  if ! pgrep pppoe-server > /dev/null; then
    logger "pppoe-server caido, reiniciando..."
    pppoe-server -I eth3 -L 192.168.20.1 -R 192.168.20.2 -N 100 -C prueba
  fi
  sleep 30
done