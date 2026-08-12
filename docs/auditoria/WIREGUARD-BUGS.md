# BUGS + FIXES — WIREGUARD (auditoría por componente)

Fechado, creciente. Se actualiza después de cada auditoría.
Formato: texto, puntos importantes, palabras clave (sin código).

---

## 2026-08-07 — RONDA 2 (verificado contra Alpine REAL)

ESTADO REAL VERIFICADO:
- wg0 Alpine: 10.7.0.5/32, puerto 51820, MTU 1420, peer VPS IIZT
  AllowedIPs 10.7.0.0/24, keepalive 25. Pub actual Qa1c (rotada 2x).
- wg0 VPS: 10.7.0.1/24, peer Alpine Qa1c AllowedIPs 10.7.0.0/24.
- clientepc 10.7.0.6/32 intacto.
- /etc/wireguard/wg0.conf: peer correcto (VPS), chmod 600.

BUGS NUEVOS ENCONTRADOS (08-07):
- [ALTO] /etc/zpot/wg-peers-wg0.json contenía el peer STALE del server
  RADIUS (pub MssW, endpoint 161.97.67.63:51820, preshared) — residuo
  del incidente wg1. CORREGIDO 08-07: JSON reescrito con el peer REAL
  del VPS (IIZT, allowed 10.7.0.0/24, endpoint 95.111.238.114:51820,
  keepalive 25, sin preshared) + chmod 600. Una regeneración de
  wg0.conf desde el JSON ahora es segura.
- [ALTO] peers_add/peers_delete con interface=wg0 podían regenerar
  wg0.conf con peers del JSON. CORREGIDO 08-07 (commit b19d605):
  ambas rechazan wg0 (antes solo delete()). VERIFICADO en vivo.
- [MEDIA] write_conf genera confs 0644 (wg0.conf manual es 600).
  chmod 600 automático pendiente.
- [MEDIA] Permisos 644 → 600 aplicado en Alpine (hotspot-server.json,
  hotspot-cookies.json, ppp-radius.json, wg-peers-wg0.json).

FIXES APLICADOS (ronda 1, 2026-08-07, commit 9675799):
- Create con private key: process substitution roto en busybox →
  stdin pipe. VERIFICADO en vivo.
- Peer con preshared: idem. VERIFICADO.
- AllowedIPs: rechaza 0.0.0.0/0, ::/0, 10.7.0.0/24, /16. VERIFICADO.
- Address: exige /32 o /128. VERIFICADO (/24 rechazado).
- Delete wg0: protegido. VERIFICADO.
- Path peers validado (anti inyección).
- private_key ya NO se expone en la API. VERIFICADO.
- UI defaults corregidos (Address 10.7.0.15/32, AllowedIPs /32).

INCIDENTES DOCUMENTADOS (lecciones):
- Key rotada sin avisar (1zGW→Mojz→Qa1c): síntoma handshake viejo +
  transfer estancado; cotejar pubs AMBOS lados.
- NAT del cliente con puertos rotativos: timeouts intermitentes con
  handshake fresco; PersistentKeepalive 25 AMBOS lados estabiliza.

PALABRAS CLAVE: wg0 intocable, /32 siempre, /0 nunca, keepalive NAT,
stdin pipe, pub coincidente, wg-peers stale.

PRÓXIMA RONDA: limpiar wg-peers-wg0.json, chmod 600 write_conf.
