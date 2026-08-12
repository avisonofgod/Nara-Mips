# Speedtest de WANs — Zpot-RS

Fecha: 2026-08-08
Estado: IMPLEMENTADO (endpoint + UI)

## Herramienta

CLI OFICIAL de Ookla 1.2.0 (multi-stream) en `/usr/local/bin/speedtest`
(versión musl para Alpine; NO usar el speedtest-cli python: usa 1 stream
y con latencia alta subestima — midió 47 Mbps a un servidor de California
a 125ms; el oficial midió 198 Mbps con el mismo enlace).

Instalación (una vez):
```
cd /tmp && wget https://install.speedtest.net/app/cli/ookla-speedtest-1.2.0-linux-x86_64.tgz
tar xzf ookla-speedtest-1.2.0-linux-x86_64.tgz
cp speedtest /usr/local/bin/ && chmod 755 /usr/local/bin/speedtest
```

## Endpoint

POST /api/system/speedtest  {"wan": "wan1"|"wan2", "n": 1..10 (default 3)}

Respuesta: {ok, wan, iface, ip, rondas:[{down,up,ping,server}], media,
historial:{muestras, media_down, media_up}}

- Lee la IP de la WAN del store MWAN (fuente de verdad)
- Crea regla temporal `ip rule add from <wan_ip> lookup <wan> pref 30000`
  (pref alto: NO toca el balanceo fwmark 1401/1402 de clientes)
- Ejecuta el binario con `-i <wan_ip>` (bind) N veces
- BORRA la regla SIEMPRE (closure + borrado posterior, aunque falle)
- Historial: /etc/zpot/speedtest-history.json (max 50 muestras/WAN,
  escritura atomica tmp+rename)
- Lock global: 1 prueba a la vez (409 Conflict si hay otra corriendo)

## UI

System > General > bloque "Speedtest WAN": botones WAN1/WAN2, muestra
rondas + media + media historica.

## Resultados iniciales (2026-08-08)

Ambas WANs = Starlink (ISP detectado por Ookla). Muy variables:
- WAN1 (eth0): 198 / 122 / 121 Mbps down → media ~147 Mbps, up ~37 Mbps
- WAN2 (eth1): 20 / 183 / 41 Mbps down → media ~82 Mbps, up ~19 Mbps

La varianza es ALTISIMA (satelital): repetir 3+ rondas y varias veces
al dia. La media historica (50 muestras) da la capacidad real usable.

## Preguntas frecuentes

- ¿VPS vs Ookla? Ookla mide la ULTIMA MILLA (capacidad que entrega el
  upstream). El VPS (iperf3) mide el path real a ese servidor (puede
  subestimar por peering/latencia, nunca exagera). Para "capacidad de
  cada WAN" usar Ookla.
- ¿Toca el MWAN? NO. La regla temporal es de trafico LOCAL (sin fwmark)
  con source = ip_wan; el trafico NAT de clientes sigue el balanceo.
- ¿Costo? 3 rondas ~1-2 min; durante la prueba esa WAN queda saturada
  (los clientes que el balanceo envie por ella se ralentizan).
