#!/bin/sh
# proxy.sh — Inicia nginx proxy para browser tests
# 95.111.238.114:8082 -> nginx -> TCP 10.7.0.5:8080
# Usar:  bash proxy.sh
# Parar: sudo nginx -s stop

PIDFILE=/tmp/zpot-proxy.pid

# Verificar si ya corre
if curl -s -o /dev/null -w "" http://localhost:8082/ 2>/dev/null; then
    echo "Proxy ya activo en http://95.111.238.114:8082/"
    exit 0
fi

# Iniciar nginx si no corre
sudo nginx 2>/dev/null
sudo nginx -s reload 2>/dev/null
echo "Proxy iniciado: http://95.111.238.114:8082/"
echo "Parar: sudo nginx -s stop"
echo "URL browser: http://95.111.238.114:8082/"
