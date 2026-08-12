# BUGS + FIXES — INTERFACES / VLANs (auditoría por componente)

Fechado, creciente. Se actualiza después de cada auditoría.
Formato: texto, puntos importantes, palabras clave (sin código).

---

## 2026-08-07 — RONDA 1 (verificado contra Alpine REAL)

ESTADO REAL VERIFICADO:
- eth0 UP 192.168.2.102/24 (WAN principal, default via .2.1).
- eth1 UP 192.168.3.105/24 (WAN secundaria MWAN).
- eth2 DOWN 192.168.30.1/24 (linkdown, sin cable).
- eth3 UP 192.168.10.1/24 (hotspot).
- eth3.881 UP 192.168.20.1/24 (PPPoE).
- wg0 UP 10.7.0.5/32.
- /etc/network/interfaces = fuente de verdad de persistencia.

BUGS ABIERTOS / PENDIENTES:
- [ALTA] delete_vlan por SUBSTRING (contains) → borrar eth3.10 elimina
  eth3.100 y deja opciones huérfanas; eth3 sin punto borra la física.
- [ALTA] set_vlan_title sin sanitizar → salto de línea inyecta comandos
  en el boot (RCE).
- [MEDIA] set_vlan_title prefix match (eth3.10 matchea eth3.100);
  list_vlans parsea VID mal → TODAS "tagged"; native bool vs string.
- [MEDIA] create_vlan sin validar id/parent; sin persistir bridge/IP;
  errores de up ignorados.
- [MEDIA] configure_bridge_port: flag "tagged" no existe en iproute2
  → config tagged falla siempre.

FIXES APLICADOS: ninguno (componente pendiente).

PALABRAS CLAVE: eth3.881, substring delete, inyección title,
persistencia interfaces, native bool/string.

PRÓXIMA RONDA: delete por bloque exacto, sanitizar title, validar VID,
persistir bridge/IP.

FIXES APLICADOS (2026-08-07, commit 9d7958b):
- [RESUELTO] delete_vlan por bloque EXACTO (antes substring borraba
  eth3.100 al borrar eth3.10).
- [RESUELTO] set_vlan_title sanitiza caracteres de control (antes \n
  inyectaba comandos en el boot).
- PENDIENTE: list_vlans VID mal (todas tagged), native bool/string,
  configure_bridge_port tagged roto.
