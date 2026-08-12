# BUGS + FIXES — IP / RUTAS / ARP / DHCP / POOLS / DNS (auditoría por componente)

Fechado, creciente. Se actualiza después de cada auditoría.
Formato: texto, puntos importantes, palabras clave (sin código).

---

## 2026-08-07 — RONDA 1 (verificado contra Alpine REAL)

ESTADO REAL VERIFICADO:
- Rutas: default eth0; 10.7.0.0/24 wg0; subredes locales; 33 rutas
  /32 de ppp; eth2 linkdown.
- ARP: mezcla REACHABLE/STALE/FAILED (clientes hotspot reales);
  muchos FAILED = clientes que se fueron (de-bounce del ARP cleanup).
- dnsmasq: port 5353, dhcp eth3 .2-.245 12h, router/DNS .1,
  option 138 → 161.97.67.63 (controladores UniFi).
- resolv.conf: 1.1.1.1 + 8.8.8.8 (sistema).

BUGS ABIERTOS / PENDIENTES:
- [ALTA] dns.rs: SHELL INJECTION add/delete (sh -c con input del body)
  → RCE root. Reemplazar por append con validación de IP.
- [ALTA] pools.rs: delete por SUBSTRING de start (borra rangos de más);
  create escribe start/end/lease/iface verbatim en dnsmasq.conf
  (inyección de directivas).
- [MEDIA] ip_addresses.rs: add/delete sin validar IP/CIDR/iface;
  sync borra línea de OTRO bloque; RMW sin lock + write no atómico.
- [MEDIA] routes.rs: delete con destino opcional borra la default;
  add sin validar (secuestro de tráfico); race con watchdog MWAN.
- [MEDIA] dhcp_leases: hostname con espacios desplaza campos.
- [MEDIA] pools reload con error ignorado → API "success" sin aplicar.
- [BAJA] arp state por último keyword (flags nuevos lo rompen).

FIXES APLICADOS: ninguno (componente pendiente).

PALABRAS CLAVE: dnsmasq 5353, option 138, substring delete, shell
injection, RMW lock, writes atómicos.

PRÓXIMA RONDA: dns.rs sin sh -c, pools delete exacto, validación
CIDR/iface, writes atómicos + locks.

FIXES APLICADOS (2026-08-07, commit 9d7958b):
- [RESUELTO] dns.rs shell injection ELIMINADA: fs append con validación
  Ipv4Addr + atómico + dedup. VERIFICADO: payload rechazado.
- [RESUELTO] pools.rs: delete por start EXACTO (error si no hay match);
  create valida IPs/iface/lease; atómico + verifica reload con rollback.
  VERIFICADO: '1' rechazado, no-existente da error.
- PENDIENTE: ip_addresses.rs validación CIDR/iface, routes.rs delete
  opcional, dhcp_leases hostname con espacios, locks RMW.

FIXES APLICADOS 2 (2026-08-07, commit 2ee6d1b — sin wireguard):
- [RESUELTO] ip_addresses.rs: validación CIDR/iface en add/delete (lo/ppp*/
  ifb rechazados) + sync atómico. VERIFICADO: cidr inválido rechazado.
- [RESUELTO] routes.rs: dst validado + 0.0.0.0/0/::/0 prohibidos; delete
  exige dst (antes OPCIONAL borraba la default). VERIFICADO en vivo.
- [RESUELTO] dhcp_leases.rs: hostname con espacios ya no desplaza campos.
- PENDIENTE: locks RMW (interfaces, ip_addresses, pools, ppp secrets,
  mwan).

## 2026-08-08 — RONDA 2 (routes UI rotos, auditoría)

- [ALTA RESUELTO] routes: la UI mandaba {destination, iface} y el backend
  leia {dst, ifname} → CREAR ruta desde la UI SIEMPRE 400. Además
  eliminarRoute usaba DELETE /api/routes (inexistente) → borrar roto.
  Fix: backend acepta ambos nombres + metric + shape limpio; UI usa
  POST /api/routes/delete. VERIFICADO: create+delete con destination OK.
- [RESUELTO] routes list devolvía array raro [routes,{rows}] → ahora
  {"routes":[...],"rows":N}.
- [RESUELTO] secrets_list enmascara passwords ("***").
- [RESUELTO] disconnect_user verifica el kill (sin falso éxito).
- [RESUELTO] firewall create_nft_rule valida table/chain/rule.
- [RESUELTO] watchdog MWAN: replace en vez de del bucle (no borra todas).
- [RESUELTO] locks RMW completos (287caad): IPADDR/INTERFACES/POOLS/MWAN.
- PENDIENTE: list_vlans VID/native/prefix, configure_bridge_port tagged,
  bridges persistencia.
