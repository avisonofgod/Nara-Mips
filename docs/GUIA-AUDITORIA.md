# GUÍA DE AUDITORÍA — Zpot-RS

Versión: 1.0 (2026-08-05) · Modo: plantilla/checklist para auditar TODO el sistema por componentes.
Esta guía NO corrige nada: recopila, organiza y estandariza cómo se revisa cada componente.

---

## 1. OBJETIVO

Auditar componente por componente (backend + frontend + conexión + configuración) usando
escenarios de prueba. Al final de cada componente: reporte de hallazgos [SEVERIDAD] + estado.

## 2. METODOLOGÍA (7 pasos por componente)

1. **Inventario**: identificar handler(s) `src/handlers/*.rs`, página `static/pages/*.html`, rutas en `src/main.rs`
2. **Leer backend completo**: lógica, validación, comandos, escrituras, locks, errores
3. **Leer frontend**: contrato con la API, escapes, defaults, acciones destructivas
4. **Verificar conexión**: método/path de cada endpoint, status codes, timeouts, auth
5. **Revisar configuración**: archivos leídos/escritos, permisos, persistencia al reboot
6. **Probar escenarios** (sección 4): feliz, inválido, concurrencia, reboot, dependencia caída, seguridad
7. **Reportar**: hallazgos con línea exacta + actualizar este documento y AUDITORIA-COMPLETA.md

## 3. PLANTILLA POR COMPONENTE

Para CADA componente (Dock → páginas → handlers) revisar:

### 3.1 BACKEND (handler)
- [ ] Lógica del flujo principal (¿hace lo que la UI promete?)
- [ ] Validación de entrada: IPs/CIDR/nombres con parser (no substring); ¿rechaza \n, ;, espacios?
- [ ] Shell injection: ¿usa `sh -c` con input del body? → REEMPLAZAR por argv (Command::args) o validar
- [ ] Path traversal: ¿rutas derivadas de input del usuario?
- [ ] Comandos de sistema: ¿status verificado? ¿errores ignorados (`let _`)? ¿falsos éxitos?
- [ ] Escrituras de config: ¿atómicas (tmp+rename+fsync)? ¿backup? ¿permisos 0600?
- [ ] Races: ¿read-modify-write sin lock? ¿sync 60s vs handlers? ¿doble submit?
- [ ] Secretos: ¿expuestos en GET/JSON? ¿modo 0644? ¿hardcodeados?
- [ ] Auth: ¿el endpoint requiere sesión/token? (hoy TODO :8081 sin auth — P0)
- [ ] Idempotencia: ¿crear 2 veces? ¿borrar 2 veces? ¿restart a mitad?

### 3.2 FRONTEND (página SPA)
- [ ] Contrato API: campos/tipos que la página espera vs lo que devuelve el backend (bool/string, null)
- [ ] XSS: ¿escapa con escHtml/escAttr (incluye comillas simples)? ¿onclick con strings del backend?
- [ ] Errores: ¿muestra el error del backend o solo console.error?
- [ ] Defaults del form: ¿seguros? (ej. WG: 10.0.0.1/24, 51820, 10.7.0.0/24 = peligrosos)
- [ ] Destructivos: ¿confirm() en delete? ¿protección de objetos críticos (wg0, eth0)?
- [ ] Recarga: ¿cargar() al entrar? ¿refresco de datos?

### 3.3 CONEXIÓN backend↔frontend
- [ ] Ruta/método coincide (GET/POST/DELETE) con el handler
- [ ] Status codes: 400/404/500 con mensaje útil
- [ ] Timeouts: ¿curl/scripts con --max-time? ¿requests que cuelgan?
- [ ] Auth de red: ¿qué IPs pueden llegar? (nft input :8081 → 10.7.0.0/24 + LANs; :80 → iface/lo/wg0)

### 3.4 CONFIGURACIÓN
- [ ] Archivos fuente de verdad (tabla §7): /etc/zpot/*.json, /etc/network/interfaces, dnsmasq, ppp, radiusclient, nft
- [ ] Persistencia al reboot: ¿se re-aplica? (init.d, rc-update, zpot-init.sh, restore)
- [ ] ¿Tres fuentes de verdad para lo mismo? (ej. pool PPP: UI vs -R/-N vs dnsmasq)
- [ ] Permisos de archivos con secretos (0600)

### 3.5 ESCENARIOS POR COMPONENTE
Ver §4 + los específicos del componente (hotspot A-H, WG mismo/distinto segmento, etc.)

## 4. ESCENARIOS GENÉRICOS DE PRUEBA (aplicar a cada componente)

| # | Escenario | Qué debe pasar | Dónde aplica |
|---|---|---|---|
| E1 | CRUD feliz | crear→listar→modificar→borrar | todos |
| E2 | Input vacío/nulo | 400 sin tocar el sistema | todos |
| E3 | Input malicioso (`; rm`, `\n`, `../`, comillas) | rechazado, SIN shell/inyección | todos |
| E4 | Input fuera de rango (IP inválida, CIDR, puerto 0/65536, vlan 0/4095) | 400 claro | interfaces, ip, routes, pools, wg |
| E5 | Doble submit / 2 requests concurrentes | sin duplicados, sin perder actualización | todos (RMW) |
| E6 | Reboot / restart del backend | config persistida; sesiones/reglas re-aplicadas | todos |
| E7 | Dependencia caída (RADIUS, nft, dnsmasq, wg-quick) | error visible, no falso éxito | hotspot, ppp, wg, pools |
| E8 | Usuario sin auth | 401 (tras fix P0); hoy 200 (documentar) | API :8081 |
| E9 | Objeto crítico (wg0, eth0, default route, regla 8081) | protegido de delete/edit | wg, bridges, routes, firewall |
| E10 | Multi-dispositivo/multi-VPS (mismo segmento IP) | sin colisión de rutas/IPs | wg, hotspot (IP/MAC) |
| E11 | Espacios/UTF-8 en inputs (hostname, username, title) | sin panic ni campos desplazados | dhcp, system, vlans |
| E12 | Escritura a disco llena/fallo | error propagado, backup intacto | todos los writes |

## 5. INVENTARIO CON ESTADO DE AUDITORÍA (acomodado 2026-08-05)

| Dock | Handler(s) | Página(s) | Estado | Documento |
|---|---|---|---|---|
| Dashboard | system, ppp, hotspot, interfaces | dashboard.html | ✅ Revisado (lectura) | AUDITORIA-COMPLETA §4.8 |
| Interfaces | interfaces.rs, vlans.rs | interfaces, interfaces-vlans | ✅ Fixes 08-07 (delete exacto, title sanitizado) | AUDITORIA-COMPLETA §4.7 |
| IP | ip_addresses, routes, arp, dhcp_leases, pools, dns | ip-addresses, ip-routes, ip-arp, ip-dhcp-leases, ip-pools, ip-dns | ✅ dns.rs sin shell injection; pools exacto | AUDITORIA-COMPLETA §4.7 |
| WireGuard | wireguard.rs | wireguard-interfaces, wireguard-peers | ✅ CORREGIDO 08-07 (9675799): stdin, validación, wg0 protegido | WIREGUARD-REVISION.md |
| PPP | ppp.rs | ppp-server, ppp-secrets, ppp-active, ppp-logs, ip-remote | ⚠️ race secrets, sed MAC | AUDITORIA-COMPLETA §4.2 |
| Hotspot | hotspot.rs | hotspot-server, active, cookies, walled-garden, ip-bindings | ✅ AUDITADO + FIXEADO (3 rondas) | AUDITORIA-COMPLETA §4.1 |
| RADIUS | radius.rs, ppp_radius.rs | radius-servers, ppp-radius | ✅ secret enmascarado + update por nombre (08-07) | AUDITORIA-COMPLETA §4.2/4.3 |
| Firewall | firewall.rs | firewall-nftables, conntrack, limit | ✅ críticas protegidas + move verificado (08-07) | AUDITORIA-COMPLETA §4.6 |
| Bridge | bridges.rs | bridges | ✅ delete solo bridges reales (08-07) | AUDITORIA-COMPLETA §4.7 |
| Routing | mwan.rs (+ main.rs watchdog) | mwan | ✅ orden boot corregido (08-07) | AUDITORIA-COMPLETA §4.4 |
| System | system.rs, command.rs | system-* | ✅ command.rs whitelist/timeout (08-07); auth ⏳ | AUDITORIA-COMPLETA §4.8/4.9 |
| Portal :80 | hotspot.rs | static/hotspot/* | ✅ Auditado + fixeado | AUDITORIA-COMPLETA §4.1 |

## 6. DOCUMENTACIÓN EXISTENTE (mapa)

| Doc | Contenido | Para qué |
|---|---|---|
| AUDITORIA-COMPLETA.md | Hallazgos 17 módulos + hotspot fijado + escenarios de prueba (40) + pendientes P0/P1/P2 | Registro de hallazgos |
| NAS-RADIUS-ALPINE.md | **ARQUITECTURA REAL del sistema (2026-08-08): NAS RADIUS completo, configs reales, flujos auth/acct/QoS, redes, nft, checklist** | **Fuente de verdad del sistema** |
| WIREGUARD-REVISION.md | Revisión WG completa actualizada 08-07 (incidentes key rotada + NAT rotativo, fixes panel, escenarios, checklist) | WG |
| docs/auditoria/*-BUGS.md | Changelog bugs+fixes POR COMPONENTE (fechado, creciente; 11 archivos) | Registro por componente |
| panel-pages-map.md | Mapa panel→código→config por dock + rutas portal | Conexión backend-frontend |
| architecture.md / backend.md / frontend.md | Arquitectura, backend, frontend | Contexto |
| network.md / pppoe.md / radius.md / coa.md / hotspot.md / hotspot-connection-logic.md | Redes, PPPoE, RADIUS, CoA, hotspot | Contexto por dominio |
| GUIA-SISTEMA.md | Guía de operación del sistema | Operación |
| config-examples/*.json | Ejemplos de config (hotspot-server, ppp-radius, radius-servers, mwan, radiusclient.conf, pppoe-server-options) | Config |
| CHANGELOG.md | Historial de cambios/fixes | Trazabilidad |
| bugs-2026-08-03-hang.md / post-reboot-reviews.md | Incidentes previos | Lecciones |

## 7. CONFIGURACIÓN — FUENTES DE VERDAD (por componente)

| Componente | Archivos |
|---|---|
| Hotspot | /etc/zpot/hotspot-server.json, hotspot-sessions.json, hotspot-cookies.json, walled-garden.json, ip-bindings.json |
| PPP | /etc/zpot-ppp-secrets.json, /etc/ppp/chap-secrets, /etc/zpot/ppp-radius.json, /etc/radiusclient/*, /etc/ppp/pppoe-server-options, ip-up, /var/run/radattr.pppN, ppp-mac-pppN, /tmp/zpot-remote.txt |
| RADIUS | /etc/zpot/radius-servers.json |
| MWAN | /etc/zpot/mwan.json, /etc/iproute2/rt_tables, /etc/network/interfaces |
| WireGuard | /etc/wireguard/<name>.conf (600), /etc/zpot/wg-peers-<name>.json, /etc/init.d/wg-quick.<name>, rc-update |
| Interfaces/VLANs | /etc/network/interfaces |
| Pools/DHCP | /etc/dnsmasq.conf, /var/lib/misc/dnsmasq.leases |
| DNS | /etc/resolv.conf, **unbound :53** (resolver), **dnsmasq :5353** (solo DHCP eth3) |
| System | /etc/hostname, /etc/ntp.conf, /etc/crontabs/root, /etc/local.d/* (lectura) |
| Boot/firewall | /etc/local.d/zpot-red.start, zpot-init.sh (nft fail-closed), nft tabla inet hotspot |

## 8. FORMATO DE REGISTRO DE HALLAZGOS

```
[SEVERIDAD] modulo:linea - bug - fix sugerido
```
Severidad: ALTA (RCE/seguridad/fallo total) · MEDIA (integridad/falso éxito) · BAJA (cosmético/UX).
Cada hallazgo nuevo se agrega a AUDITORIA-COMPLETA.md (sección del componente) + CHANGELOG.

## 9. PENDIENTES GLOBALES (de AUDITORIA-COMPLETA — NO corregidos aún)

- P0: auth API :8081 · dns.rs sh -c (RCE) · wireguard.rs sh -c + <( ) + parser peers + proteger wg0 · command.rs traversal · firewall move/delete · bridges/interfaces delete substring
- P1: escrituras atómicas + chmod 0600 · locks RMW · ppp sed -n · radius DELETE · mwan boot/ping · validación IP/CIDR
- P2: leases espacios · system UTF-8 · scripts/scheduler UI · native bool/string · dedup DNS · ifb leak · CiroCampos · CoA WG real

## 10. CÓMO USAR ESTA GUÍA (flujo de trabajo)

1. Elegir componente del inventario §5 con estado ⚠️
2. Aplicar plantilla §3 (backend/frontend/conexión/config)
3. Probar escenarios §4 (E1-E12) + escenarios específicos del componente
4. Reportar hallazgos (§8) → actualizar AUDITORIA-COMPLETA.md
5. Cuando el usuario diga "aplica" → corregir con commit + CHANGELOG + deploy (flujo git→Alpine)
6. Marcar estado ✅ en §5
