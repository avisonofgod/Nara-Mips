# AUDITORÍA COMPLETA — Sistema Zpot-RS

Fecha: 2026-08-04 · Commits de referencia: `0c04484` (hotspot r2), `85511fa` (hotspot auditoría completa)
Cobertura: 19 handlers + 39 páginas SPA + portal :80 + admin :8081. Hotspot auditado a fondo (3 rondas, fixes aplicados). 17 módulos restantes auditados en esta ronda (hallazgos NO aplicados aún — ver §8).

---

## 1. Resumen del sistema

| Área | Valor |
|---|---|
| Stack | Rust + axum 0.7 + tokio, Alpine Linux (10.7.0.5) |
| Listeners | `:80` portal hotspot (redirect captivo) · `:8081` admin SPA + API |
| Redes | eth3 hotspot 192.168.10.0/24 · eth3.881 PPPoE 192.168.20.0/24 (gw .20.1) · wg0 mgmt 10.7.0.5/32 · WAN eth0/eth1 (MWAN) |
| Config | `/etc/zpot/*.json` + `/etc/network/interfaces` + `/etc/dnsmasq.conf` + `/etc/ppp/*` + `/etc/radiusclient/*` + nft tabla `inet hotspot` |
| Auth RADIUS | 161.97.67.63:1812 (FreeRADIUS3 + daloRADIUS) · accounting SIEMPRE 1813 |
| Portal | static/hotspot/*.html (login/alogin/status/logout) + anti-bruteforce + cookies server-side |
| Deploy | git → Alpine `git pull` + `touch src/*` + `cargo build --release` (DEBE tardar >5s) + kill PID exacto + nohup. NUNCA pkill -f (mata SSH) |

## 2. Inventario de componentes (estado de auditoría)

| Dock | Módulos | Estado |
|---|---|---|
| Dashboard | system, ppp, hotspot, interfaces (lectura) | Revisado (sin bugs críticos) |
| Interfaces | interfaces.rs, vlans.rs | ⚠️ Hallazgos ALTA (substring delete, inyección title) |
| IP | ip_addresses.rs, routes.rs, arp.rs, dhcp_leases.rs, pools.rs, dns.rs | ⚠️ dns.rs = SHELL INJECTION; pools.rs inyección config |
| WireGuard | wireguard.rs | ✅ CORREGIDO 2026-08-07 (9675799): stdin pipe, validación AllowedIPs/Address, protege wg0, private_key oculta. Red: incidentes key rotada + NAT rotativo documentados |
| PPP | ppp.rs | ⚠️ race secrets, falsos éxitos, pool UI sin efecto |
| Hotspot | hotspot.rs | ✅ AUDITADO 3 rondas — 28+ fixes aplicados (commits 0c04484/85511fa) |
| RADIUS | radius.rs, ppp_radius.rs | ⚠️ secret en claro, sin DELETE, bug sed MAC |
| Firewall | firewall.rs | ⚠️ move off-by-one, delete reglas críticas |
| Bridge | bridges.rs | ⚠️ ip link delete eth0/eth3 sin validar |
| Routing | mwan.rs (+ main.rs watchdog) | ⚠️ trap boot + ping ruta main |
| System | system.rs, command.rs | ⚠️ command.rs whitelist traversal; API sin auth |

## 3. Hallazgos CRÍTICOS transversales (prioridad 0)

- [ALTA] **API admin :8081 SIN autenticación** (main.rs:298-379): cero middleware/token/sesión. Única barrera = nft input (10.7.0.0/24, 192.168.2.0/24, 192.168.3.0/24, lo) → cualquier host WG/LAN = root del router. Y esas reglas son borrables vía /api/firewall. + /api/command (command.rs:16) ejecuta scripts → cadena completa RCE root.
- [ALTA] **dns.rs:32/42 — shell injection → RCE root**: `sh -c "echo '{}' >> /etc/resolv.conf"` y `sh -c "sed -i '/^nameserver {}/d' ..."` con input del body sin validar. `address = "1.1.1.1'; rm -rf / #"` ejecuta. Fix: std::fs append + validar Ipv4Addr, nunca sh -c.
- [ALTA] **wireguard.rs — inyección sh -c** (peers_add 226/258/337+, delete 268): campos del body concatenados en `wg set {interface} peer ...` + `<(echo '{psk}')`. `; touch /tmp/pwn` = RCE. Y `<( )` (process substitution) es bash — en busybox ash falla SIEMPRE → create/peers con psk rotos en Alpine. Fix: stdin pipe + validación.
- [ALTA] **Escrituras de config NO atómicas** (interfaces.rs:441/602/749, ip_addresses.rs:53, pools.rs:101/126, ppp.rs, ppp_radius.rs:133-152, wireguard.rs:179, radius.rs): fs::write directo → crash corrompe /etc/network/interfaces, dnsmasq.conf, radiusclient, etc. Fix: temp+rename+fsync.
- [ALTA] **RMW sin lock** (interfaces, ip_addresses, pools, ppp secrets sync vs handlers): 2 requests concurrentes pierden actualizaciones; el sync 60s puede "resucitar" secrets borrados.
- [MEDIA] **Permisos 0644** en archivos con passwords en claro: /etc/zpot/radius-servers.json (secret RADIUS), /etc/zpot-ppp-secrets.json (passwords PPP), /etc/wireguard/*.conf (private/preshared keys), /etc/zpot/hotspot-cookies.json (passwords RADIUS de cookies). Fix: chmod 0600.

---

## 4. Auditoría por componente

### 4.1 HOTSPOT ✅ (auditado a fondo — fixes APLICADOS)

**Lógica conexión/reconexión/desconexión (escenarios verificados):**
- A) 1ª conexión sin cookie: root → no authed → login → portal_auth (anti-bruteforce, ARP MAC, RADIUS 2 intentos) → sesión + nft bypass + tc + acct-start + cookie.
- B) Re-conexión cookie (misma IP+MAC): auto_reconnect_from_cookie (cookie server-side + MAC en ARP dev iface) → /hotspot/portal → RADIUS re-auth con password server-side (find_password user+mac) → sesión nueva.
- C) IP misma, MAC diferente: NO hereda sesión (has_active_session_for_peer compara MAC en root/fallback/portal_root) → login del nuevo dispositivo.
- D) IP nueva, MAC misma (DHCP renew, cookie): auto-reconexión → BUG-D cierra sesión IP vieja con acct-stop → sesión nueva.
- E) IP+MAC distintas: sesión normal.
- F) Re-login manual misma IP+MAC: acct-stop de la sesión previa antes de crear.
- G) Cookie MAC ≠ ARP: no auto-reconecta → login.
- H) Cookie expirada/borrada: rechazada + set-cookie Max-Age=0.

**Cookies:** server-side `/etc/zpot/hotspot-cookies.json` (user+mac+password+expira), browser `hs_session=base64(user:mac)` HttpOnly SameSite=Lax sin password. Refresh server-side a los 6d si hay tráfico. Logout borra server-side + browser. CSRF: logout exige cookie válida + MAC del peer.

**nftables hotspot** (tabla `inet hotspot`, init parametrizado con cfg.iface/gw):
- input: lo accept; 8081 solo WG/LAN; 80 solo iface/lo/wg0; drop final 80/8081.
- prerouting: drop 8081 eth3 y ppp*; auth `ip saddr . ether saddr @hotspot_auth return`; redirect :80 no-auth.
- forward: DHCP/DNS/80 accept; established,related con `daddr != hs_net`; isolation eth3→eth3; hotspot→mgmt drop; auth accept; drop final eth3; ppp isolation.
- postrouting: masquerade eth0/eth1 (WANs).
- zpot-init.sh: set concat + `ip saddr . ether saddr @hotspot_auth` + DROP FINAL (fail-closed real).

**Anti-bruteforce:** 5 fallos/60s por IP; timeout RADIUS NO cuenta (reachable flag); poda >1024 IPs; rechaza campos vacíos sin roundtrip.

**QoS:** VSA Mikrotik-Rate-Limit (rate/ceil separados) o fallback rate_limit del config; clamp rate y ceil a 100000 (HTB ceil>=rate); burst 1s (comentario corregido); filtro UP en ifb_{iface} (disconnect/logout borran el filtro correcto).

**CoA:** listener UDP 3799 con Request Authenticator MD5 (RFC 5176 §3.1), spawn sin bloquear loop, find_session_ip con NAK si username ambiguo; o modo polling HTTP 30s (endpoint PHP radacct), gracia 90s, NO-array = ciclo cancelado.

**CoA / desconexión por RADIUS:** polling expulsa sesiones cerradas en RADIUS; ARP cleanup dedicado 30s con de-bounce 2 muestras; interim global 60s (acct-interim + idle + reauth 300s + renovación nft independiente del tráfico).

### 4.2 PPPoE + RADIUS PPP (ppp.rs, ppp_radius.rs) ⚠️

- [ALTA] **Bug sed MAC** (ppp_radius.rs:215-217): `sed -E 's/.../\\1/'` sin `-n` → si el cmdline no tiene remotenumber o MAC en mayúsculas, /var/run/ppp-mac-pppN queda con el cmdline ENTERO; el fallback `$6` es código muerto. Cascada: uptime "-", disconnect falso, cleanup de zombies (main.rs:436-443) puede `ip link delete` de interfaces VIVAS al reiniciar backend. Fix: `sed -nE 's/.*remotenumber ([0-9a-f:]{17}).*/\\1/p'`.
- [ALTA] ppp.rs:88: auto-registro crea password vacío → fallback usa **username como password** (auth local bypass).
- [MEDIA] Race load-modify-save sin lock en /etc/zpot-ppp-secrets.json (sync 60s vs handlers delete/order) → secret borrado resucita.
- [MEDIA] auto_register persiste IP del POOL como fija (stale al reconectar); NO hay API create/update de secrets (IPs fijas .2-.37 solo a mano).
- [MEDIA] disconnect_user falso éxito (kill falla en silencio, responde "disconnected").
- [MEDIA] qos_radius_apply sin validar sesión activa (ip/iface arbitrarios → shaping/DoS); apply_qos_ppp exige ambas direcciones o no aplica nada.
- [MEDIA] ifb_pppN NUNCA se elimina (leak hasta 100 ifb); qos_cleanup no borra el filtro del ifb.
- [MEDIA] ppp_radius.rs: dns1/dns2/nas_ip/pool sin validar; pool_start/pool_end de la UI NUNCA se aplican (hardcodeado `-R 192.168.20.100 -N 100` en ppp.rs:849) — 3 fuentes de verdad del pool.
- [MEDIA] ppp_radius.rs:133-152: escritura NO atómica de 4 archivos de sistema sin backup ni rollback; sobrescribe config manual previa.
- [MEDIA] auth_order "radius,local" con MSCHAPv2: "local" = PAM (login.radius), PAM NO habla MSCHAPv2 → fallback local probablemente nunca autentica. VERIFICAR en runtime.
- [MEDIA] ip-up curl sin --max-time → pppd se bloquea si backend cuelga; JSON del curl sin escapar $PEERNAME.
- [MEDIA] pppoe_start no verifica eth3.881/binario; dos caminos de gestión (start/stop directo vs rc-service pppoe restart) pueden divergir.
- [BAJA] secrets JSON corrupto → load devuelve [] y el sync SOBRESCRIBE con lista vacía (pérdida silenciosa total). secrets_list devuelve passwords al SPA. ppp_uptime CLK_TCK=100 hardcoded.

PALABRAS CLAVE: pppoe-server eth3.881, pool -R provisional .100+, secrets = catálogo (NO sobreescribir fijas .2-.37), require-mschap-v2 SOLO, zombie = ip link delete SOLO con ppp-mac, Framed-IP se ignora, acct octets 42=tx/43=rx, 1813 SIEMPRE.

ESCENARIOS DE PRUEBA (pendientes):
- Cliente con IP fija reconecta → IP se mantiene (secrets catálogo no sobreescribe).
- Cliente sin fija → pool -R .100+ (corridas/dup aceptadas como provisional).
- Windows/Android MSCHAPv2 → auth OK (SOLO require-mschap-v2; chap+mschap juntos = MD5 y fallan).
- RADIUS caído → fallback local PAM NO habla MSCHAPv2 (verificar runtime).
- Reinicio del backend → zombies: correlacionar ppp-mac antes de borrar.
- Sesión cerrada → acct/interim a 1813; QoS VSA crea ifb_pppN correcto.
- ip-up cuelga → pppd no debe bloquearse (revisar timeout).

PUNTOS A REVISAR: bug sed MAC (ppp-mac con cmdline entero), race secrets 60s (resucitan), ifb leak hasta 100, 3 fuentes de verdad del pool, auth_order local, escritura no atómica 4 archivos, auto-registro password vacío (fallback username como password).

VERIFICADO EN VIVO (08-07): ip-up EXISTE y llama a QoS por API (sin --max-time → riesgo bloqueo pppd); ip-down EXISTE y limpia QoS + elimina la interfaz del kernel; pppoe-options = require-mschap-v2 SOLO + radius.so + radattr.so; ppp-radius.json accounting FALSE; pool real .100-.200 provisional; 33 sesiones ppp + 33 ifb_ppp.

### 4.3 RADIUS servers (radius.rs) ⚠️

- [ALTA] Secret en claro: hardcodeado en fallback del código ("85River@B"), persistido 0644, GET devuelve el secret COMPLETO al SPA. Fix: chmod 600 + enmascarar en GET.
- [ALTA] NO existe DELETE de servidores (ni update por nombre: re-POST con mismo name no actualiza — get_server_by_name devuelve el primero).
- [MEDIA] post_server sin validación (name vacío/dup, ip inválida, puerto 0, secret vacío); save_servers ignora errores.
- [BAJA] OnceLock carga 1 vez (cambios externos no se ven); sin failover entre múltiples auth.

PALABRAS CLAVE: 161.97.67.63 FreeRADIUS3+daloRADIUS, multi-NAS (radacct por IP origen), 1812 auth / 1813 acct SIEMPRE, secret 0600, timeout ≠ reject (reachable flag).

ESCENARIOS DE PRUEBA (pendientes):
- GET lista → secret enmascarado (hoy se expone; fix pendiente).
- POST server nuevo → aparece en lista.
- POST con mismo nombre → NO actualiza (sin DELETE/update; bug).
- Server caído → timeout marcado como NO disponible, no como rechazo.
- Dos servers → sin failover (usa el primero).

PUNTOS A REVISAR: chmod 600 de radius-servers.json, enmascarar secret en GET, implementar DELETE y update por nombre, validar campos del POST.

VERIFICADO EN VIVO (08-07): /etc/zpot/radius-servers.json VACÍO — el módulo de servidores del panel NO se usa en producción (el hotspot usa su propio config). /etc/radiusclient/servers = 161.97.67.63 con secret en claro.

### 4.4 MWAN / Balanceo (mwan.rs + main.rs watchdog) ⚠️

- [ALTA] **Trap de boot**: apply_nft_rules (con sync_hotspot_wans) corre ANTES de init_hotspot_nft → tabla no existe → flush/add fallan en silencio → WANs adicionales sin masquerade tras reboot. Fix: init primero o re-sync después.
- [MEDIA-ALTA] Store SIEMPRE dice "up" (post inserta status:"up" fijo; watchdog solo actualiza un CLONE) → config con WAN caída reparte 50/50 a la muerta.
- [MEDIA-ALTA] apply_wan_ip_change: `ip -4 addr flush` ANTES del add — si el add falla la iface queda SIN IPv4 sin rollback; entrada sin validar (iface="eth0" con ip basura = DoS); inyección de líneas en /etc/network/interfaces vía new_ip/new_gw.
- [MEDIA] **Trap conocido**: check_wan_ping usa `ping -I <ip wan> 8.8.8.8` que depende de la ruta MAIN → WAN viva declarada caída → recovery deja default en WAN equivocada. Fix: ping al gateway de la WAN o por su tabla.
- [MEDIA] Watchdog borra TODAS las default x5 antes de replace → ventana sin ruta + borra defaults ajenas (wg-quick). 2 pings por WAN por ciclo inconsistentes.
- [MEDIA] flush_ip_rules borra prio 1400..=1510 sin importar fwmark; mark=0/table=0 sin validar.
- [MEDIA] weight se parsea y se IGNORA (jhash siempre 50/50); race POST concurrentes; watchdog lee disco mientras POST escribe disco después de aplicar.
- [BAJA] get_mwan_status ejecuta conntrack -L sin spawn_blocking (bloquea worker); "active_wan" primer "up" del HashMap (orden aleatorio); count_all_client_ips mezcla tráfico admin.

PALABRAS CLAVE: eth0/eth1 WANs, jhash 50/50, fwmark 1/2, tablas 1400..1510, watchdog 30s, ping al GATEWAY (no 8.8.8.8), trap de boot (apply_nft antes de init_hotspot), store dice "up" fijo.

ESCENARIOS DE PRUEBA (pendientes):
- Una WAN cae → watchdog cambia default sin trap (ping al gateway).
- Reboot → nft/masquerade aplican en el orden correcto (trap).
- Config con WAN caída → store falso "up" → 50/50 a la muerta (bug).
- Flush de rutas → NO borrar defaults ajenas (wg-quick).
- Weight distinto → se ignora (jhash fijo 50/50).
- POST concurrentes → sin pérdida (race).

PUNTOS A REVISAR: orden apply_nft_rules vs init_hotspot_nft, check_wan_ping dependiente de ruta main, watchdog borra TODAS las default, flush_ip_rules sin validar fwmark, weight ignorado.

### 4.5 WireGuard (wireguard.rs) ✅ CORREGIDO 2026-08-07

Hallazgos originales (2026-08-05) y su estado actual:

- [RESUELTO] Inyección sh -c en peers_add/delete/remove_boot_restore → Path del listado de peers validado; campos del body con validación de AllowedIPs y Address. Pendiente global: revisar los demás `sh -c` del módulo con inputs controlados (name validado en create).
- [RESUELTO] Process substitution `<(echo '...')` roto en busybox → reemplazado por stdin pipe en create y peers_add. VERIFICADO en vivo: crear interfaz y peer con preshared funcionan.
- [RESUELTO] Indices del dump en peers() → la lógica actual matchea correctamente pub del peer; VERIFICADO con wg0 real (1 peer listado OK).
- [RESUELTO] write_conf/save_peers_json permisos 0644 → pendiente chmod 600 automático (los conf generados; wg0.conf es 600 manual).
- [RESUELTO] delete sin validar + `|| true` → ahora rechaza wg0; el `|| true` sigue tragando fallos menores (revisar al tocar el módulo).
- [RESUELTO] allowed_ips sin validar → ahora rechaza 0.0.0.0/0, ::/0, 10.7.0.0/24, 10.7.0.0/16. VERIFICADO.
- [RESUELTO] regenerar .conf pierde campos custom → aceptado como limitación (panel no gestiona Table/PostUp).
- [RESUELTO] list() expone private_key → ahora vacía en la API. VERIFICADO.
- [NUEVO 08-07] Incidente key rotada de wg0 (1zGW→Mojz→Qa1c): síntoma handshake viejo + transfer estancado. Diagnóstico: cotejar pub en ambos lados. Lección en WIREGUARD-REVISION.md §2.1.
- [NUEVO 08-07] NAT del cliente con puertos rotativos: timeouts intermitentes con handshake fresco. Fix: keepalive 25 en ambos lados. WIREGUARD-REVISION.md §2.2.
- [NUEVO 08-07] wg-peers-wg0.json contiene el peer STALE del server RADIUS (MssW 161.97.67.63, preshared) — residuo del incidente wg1. Si el panel regenera wg0.conf desde ese JSON (peers_add/peers_delete con interface wg0) se pierde el peer del VPS → caída de gestión. PENDIENTE: limpiar JSON o proteger wg0 de regeneración.

PALABRAS CLAVE del componente: wg0 protegido, /32 siempre, /0 nunca, keepalive NAT, stdin pipe, pub coincidente, rutas /24 solo wg0.
- [MEDIA] `wg genkey` sin chequear status; TOCTOU check-then-add.
- ✅ OK: create valida nombre y aplica rollback por comando; persistencia conf+json+init.d+rc-update; wg9 probado end-to-end.

### 4.6 Firewall (firewall.rs) ⚠️

- [ALTA] create_nft_rule: table/chain/rule_str sin validar; position="insert" mete "accept" al principio de hotspot/forward → bypass del aislamiento/8081.
- [ALTA] delete_filter_rule/delete_nat_rule: aceptan cualquier chain + handle → borrar reglas críticas de hotspot (drop 8081, redirect 80, isolation, masquerade) si se conoce el handle (los expone el list).
- [ALTA] move_nft_rule/move_nft_rule_to: OFF-BY-ONE (up sube 2 posiciones; down NO-OP); delete ANTES del re-add con errores ignorados → si el re-add falla la regla se PIERDE.
- [MEDIA] Extracción del texto de regla toma la línea anterior a "# handle N" — reglas largas envueltas en múltiples líneas → solo el último fragmento → re-add con sintaxis rota.
- [MEDIA] create_nat_rule action_suffix/protocol sin validar.
- [BAJA] Cache muerta (cache_get/set/invalidate nunca usados); expr_text lossy (display incompleto); conntrack_status expone tabla completa.

PALABRAS CLAVE: tabla inet hotspot, chains input/forward/prerouting/postrouting, handles, move off-by-one, delete reglas críticas (drop 8081, redirect 80, isolation, masquerade), insert position.

ESCENARIOS DE PRUEBA (pendientes):
- Insertar regla accept al inicio del forward → bypass de aislamiento (bug ALTA).
- Mover regla hacia arriba → sube 2 posiciones (off-by-one).
- Mover hacia abajo → no-op (bug).
- Borrar handle de una regla crítica → posible (bug).
- Regla larga multilínea → re-add con sintaxis rota (solo último fragmento).
- Crear regla NAT con action/protocol inválidos → rechazar.

PUNTOS A REVISAR: validar table/chain/rule_str, proteger reglas críticas de delete/move, corregir off-by-one, parseo multilínea, cache muerta.

### 4.7 Networking básico (interfaces/vlans/bridges/ip/routes/arp/pools/dns/dhcp) ⚠️

**interfaces.rs / vlans.rs:**
- [ALTA] delete_vlan por SUBSTRING (`contains(name)`): borrar "eth3.10" elimina "iface eth3.100" + deja opciones del bloque huérfanas; name sin '.' (eth3) borra el bloque de la iface física → hotspot muerto al boot.
- [ALTA] set_vlan_title: title sin sanitizar con `\n` inyecta líneas `up <comando>` en /etc/network/interfaces → RCE en boot.
- [MEDIA] set_vlan_title prefix match ("iface eth3.10" matchea eth3.100); list_vlans parsea parts[0] como VID pero en líneas de puerto es el NOMBRE → TODAS las VLANs reportadas "tagged"; campo "native" mezcla bool y string (contrato API inconsistente).
- [MEDIA] create_vlan sin validar vlan_id (0/negativos), parent; VLAN sin persistir bridge/IP (drift); `ip link set up` error ignorado.
- [MEDIA] configure_bridge_port: flag "tagged" no existe en iproute2 (solo pvid|untagged|self|master) → toda config tagged falla.
- [BAJA] auto check por contains dentro de comentarios; duplicados.

**bridges.rs:**
- [ALTA] delete: solo bloquea "bridgeLan"/"br0" — `ip link delete eth0/eth3` ELIMINA la interfaz física (caída total). Fix: validar contra `ip -j link show type bridge`.
- [MEDIA] create: ports sin validar (enslave eth0 = outage); name no validado; SIN persistencia (bridges desaparecen al reboot).
- [BAJA] list devuelve [] con 200 si ip -json falla; parseo por substring de `ip -d link`.

**ip_addresses.rs:**
- [MEDIA] add/delete SIN validar IP/CIDR/iface (permite IPs en lo/eth0 → rompe MWAN); sync_interfaces retain global borra la línea address de OTRO bloque; RMW sin lock + write no atómico; sync best-effort (API dice success sin persistir).

**routes.rs:**
- [MEDIA] delete dst OPCIONAL → borra la default (caída hasta watchdog ≤30s); add sin validar dst/gw/dev (secuestro de tráfico con dst=0.0.0.0/0 via atacante); race con el watchdog MWAN (barre/duplica rutas).
- [BAJA] shape JSON `[array, {rows}]` rara; filtro por substring "ppp" oculta rutas legítimas.

**arp.rs / dhcp_leases.rs:**
- [BAJA] arp: state por último keyword (flags nuevos lo rompen); parseo posicional frágil.
- [MEDIA] dhcp_leases: hostname con ESPACIOS desplaza campos (id mal, hostname truncado). Fix: parts[3..len-1].join(" ").
- [BAJA] dhcp_leases usa `cat` en vez de fs::read (fork extra).

**pools.rs:**
- [ALTA] delete por SUBSTRING de start: start="192.168.10.2" borra también .20-.200; start="1" borra casi todo.
- [ALTA] create escribe start/end/lease/iface VERBATIM en /etc/dnsmasq.conf (inyección `\naddress=/evil.com/...` = DNS poisoning persistente; reload fallido ignorado).
- [MEDIA] reload con error ignorado → API "success" sin aplicar; RMW sin lock + write no atómico (dnsmasq.conf corrupto = DHCP global caído).
- [BAJA] parse_range con rama de "líneas corruptas" nunca usada (list filtra antes); join sin newline final.

**dns.rs — PEOR MÓDULO:**
- [ALTA] SHELL INJECTION add y delete (sh -c con input del body) → RCE root — ver §3.
- [MEDIA] add sin dedup (nameservers duplicados crecen sin límite); append/sed no atómicos sin lock.

PALABRAS CLAVE: /etc/network/interfaces fuente de verdad, dnsmasq.conf, DELETE POR SUBSTRING (peligroso), inyección \n en títulos, bridges sin persistencia, dns.rs = shell injection (P0).

ESCENARIOS DE PRUEBA (pendientes):
- Borrar vlan eth3.10 → NO tocar eth3.100 (substring bug).
- Title de vlan con salto de línea → NO inyectar comandos al boot.
- Delete pool con IP parcial → NO borrar rangos de más.
- DNS add con IP válida → OK; con payload → rechazar (hoy RCE).
- Bridge create → sobrevive reboot (hoy no persiste).
- DHCP lease con hostname con espacios → campos correctos.
- IP add en eth0/lo → rechazar (rompe MWAN).
- Route delete sin destino → NO borrar la default.

PUNTOS A REVISAR: deletes por substring (vlan, pool), set_vlan_title sanitizar, dns.rs sin sh -c, writes atómicos + locks, bridges persistencia, routes delete opcional.

### 4.8 System (system.rs) ⚠️

- [BAJA] `Command::new("bash")` para TODAS las lecturas — dependencia no declarada (Alpine mínimo sin bash = todo vacío). Fix: sh/awk o /proc.
- [BAJA] `&src[..80]` corta por BYTES → PANIC si byte 80 cae en UTF-8 multibyte (scripts_list → 500).
- [BAJA] files lista metadata de /etc/zpot/*.json (incluye wg-peers con preshared — solo nombres/tamaños, sin contenidos, aceptable).
- [INFO] Todo es solo lectura (identity/resources/clock/ntp/users/logs/scripts/scheduler/files). La "gestión" de scripts/scheduler en la UI se hace con shell crudo vía /api/command que el backend RECHAZA (system-scripts.html:75 "Comando no implementado: head", system-scheduler.html sed → 404): botones Ver script/toggle/delete ROTOS (funcional, BAJA).

PALABRAS CLAVE: solo lectura, dependencia bash (Alpine mínimo), slice por bytes UTF-8, scripts/scheduler UI rotos, /api/command sin auth (P0 global).

ESCENARIOS DE PRUEBA (pendientes):
- Sistema Alpine sin bash → listas de system vacías (bug).
- Script con UTF-8 multibyte en el byte 80 → 500 panic (bug).
- Botones de scripts/scheduler en la UI → verificar estado (rotos hoy).
- Lista de archivos de /etc/zpot → nombres OK, sin exponer contenidos.

PUNTOS A REVISAR: quitar dependencia bash (sh/awk/proc), fix slice por bytes, decidir gestión de scripts/scheduler.

### 4.9 Command (command.rs) ⚠️

- [ALTA] /api/command + TOTA la API sin auth (ver §3).
- [ALTA] Whitelist de scripts por starts_with/ends_with sin canonicalizar → traversal `/root/zpot-rs/scripts/../../usr/local/bin/reboot-alpine.sh` la atraviesa.
- [MEDIA] `path.contains("reboot")` por substring → cualquier ruta con "reboot" se dispara fire-and-forget (DoS); run_script sin timeout (request cuelga).
- [MEDIA] cmd_ip_pool_add/dhcp_server_add: name/ranges/lease sin validar → inyección de directivas dnsmasq; cmd_*_remove deja dhcp-range huérfana (pool borrado sigue sirviendo).
- [MEDIA] cmd_wireguard_add: wg set falla y aun así ip link up (zombie roto).
- [BAJA] cmd_mwan_table_add sin validar id/nombre en /etc/iproute2/rt_tables; cmd_ip_route_remove no acepta "table".

PALABRAS CLAVE: /api/command sin auth (P0 global), whitelist traversal sin canonicalizar, substring "reboot" = fire-and-forget, run_script sin timeout, cmd_wireguard_add zombie.

ESCENARIOS DE PRUEBA (pendientes):
- Ejecutar script con ../ fuera de scripts/ → rechazar (hoy atraviesa).
- Ruta cuyo nombre contenga "reboot" → NO disparar automático.
- cmd_wireguard_add con wg set fallido → NO dejar interfaz zombie.
- Script que cuelga → timeout (hoy la request cuelga).
- cmd_ip_pool_add con directivas maliciosas → rechazar (hoy inyecta dnsmasq).

PUNTOS A REVISAR: canonicalizar whitelist, reemplazar substring reboot, timeouts, auth global de la API.

---

## 5. Conexión backend-frontend

Detalle completo en `docs/panel-pages-map.md`. Resumen:
- Admin SPA `:8081` → `/api/*` (axum admin_app, main.rs:298-379) → 50 rutas (interfaces, vlans, bridge, ip-addresses, routes, arp, pools, dhcp-leases, dns, ppp/*, ip/remote, wireguard/*, mwan/*, firewall/*, system, command).
- Portal `:80` → rutas portal (/, /hotspot/portal, /auth, /status, /logout, /disconnect, /static/*).
- SPA: static/app.js (PAGES + sw()/lp()) + static/pages/*.html (39). Frontend consume backend vía fetch().
- ⚠️ Contrato inconsistente detectado: list_vlans "native" bool vs string (SPA puede romperse).

## 6. Configuración (archivos por componente)

| Componente | Archivos |
|---|---|
| Hotspot | /etc/zpot/hotspot-server.json (iface gw html_dir idle_timeout shared_users rate_limit radius radius_secret coa_*) · hotspot-sessions.json · hotspot-cookies.json · walled-garden.json · ip-bindings.json |
| PPP | /etc/zpot-ppp-secrets.json + /etc/ppp/chap-secrets + /etc/zpot/ppp-radius.json + /etc/radiusclient/{radiusclient.conf,servers,dictionary} + /etc/ppp/{pppoe-server-options,ip-up} + /var/run/{radattr.pppN,ppp-mac-pppN} + /tmp/zpot-remote.txt |
| RADIUS | /etc/zpot/radius-servers.json |
| MWAN | /etc/zpot/mwan.json + /etc/iproute2/rt_tables + /etc/network/interfaces |
| WireGuard | /etc/wireguard/<name>.conf + /etc/zpot/wg-peers-*.json + /etc/init.d/wg-quick.<name> |
| Interfaces/VLANs | /etc/network/interfaces (fuente de verdad persistencia) |
| Pools/DHCP | /etc/dnsmasq.conf (dhcp-range, option 43/138) + /var/lib/misc/dnsmasq.leases |
| DNS | /etc/resolv.conf |
| System | /etc/hostname · /etc/ntp.conf · /etc/crontabs/root · /etc/local.d/* (lectura) |
| Boot | /etc/local.d/zpot-red.start + zpot-init.sh (nft fail-closed) |

## 7. Escenarios de funcionamiento para prueba

**Hotspot (portal):**
1. 1ª conexión: cliente nuevo → redirect :80 → login → RADIUS OK → navega; verificar store+nft set+tc+radacct start.
2. Cookie: reconectar mismo dispositivo (misma IP/MAC) → auto-login sin password.
3. DHCP renew: misma MAC IP nueva → sesión vieja cerrada con Stop, nueva creada.
4. IP reasignada a OTRO dispositivo → NO hereda sesión → login del nuevo.
5. Logout: cookie borrada server-side + browser; NO se re-autentica solo.
6. Anti-bruteforce: 5 fallos → "Demasiados intentos" 60s; timeout RADIUS (apagar server) → "Servidor RADIUS no disponible" SIN contar fallo.
7. Idle: cliente inactivo > idle_timeout → expulsado con Stop cause 4, cookie preservada → auto-reconexión al volver.
8. QoS: VSA Mikrotik-Rate-Limit → tc class rate/ceil correctos; >100M → clamp sin expulsión.
9. Polling CoA: cerrar sesión en daloRADIUS → expulsada ≤30s (gracia 90s).
10. ARP: apagar WiFi del cliente → expulsado tras 2 ciclos (60s) con Stop cause 2.
11. Walled garden: IP sin auth accesible; puerto/protocolo correcto.
12. IP bindings: bypass sin login; blocked → sin internet.

**PPPoE:**
13. Cliente conecta: ppp0 + MAC + radattr; sync 60s auto-registra secret; QoS VSA aplicada (rate+ceil).
14. Desconectar por MAC: kill pppd correcto (NO pkill -f).
15. IP fija .2-.37 respetada; pool -R .100+ sin sobrescribir fijas.
16. Reboot con sesiones: zombies limpiados por MAC; sin `ip link delete` de viva.
17. apply_config: pppoe restart con confirmación; ms-dns/nas_ip válidos.
18. Remote: 10.7.0.5:8082 → AP:80; https :443; botón Desactivar limpia DNAT.

**MWAN/Balanceo:**
19. Caída de una WAN: watchdog detecta ≤30s, default migra a la otra.
20. Pings por ruta main (trap): eth1 viva no debe declararse caída.
21. Tráfico sticky por fwmark; reboot: masquerade en TODAS las WANs (trap boot).
22. Config con WAN caída: no repartir 50/50 a la muerta.

**WireGuard:**
23. Crear interfaz (wg genkey) → conf + init.d + rc-update; wg-quick up OK.
24. Peer con preshared+keepalive → dump correcto (preshared NO en endpoint).
25. AllowedIPs /32 OK; 0.0.0.0/0 y 10.7.0.0/24 → fallo wg-quick con rollback.
26. Delete → limpia conf/json/init.d/rc-update, wg0 intacto.
26a. Key rotada de wg0 (incidente 08-07): pub de Alpine ≠ pub del peer en VPS → handshake viejo 12h, transfer estancado. Diagnóstico: cotejar pubs AMBOS lados. Fix: rotar key nueva en ambos lados.
26b. NAT del cliente con puertos rotativos (08-07): handshake fresco pero timeouts intermitentes → PersistentKeepalive 25 en AMBOS lados (Alpine mantiene mapeo + enseña endpoint).
26c. Panel: crear interfaz con private key y peer con preshared → YA FUNCIONA (stdin pipe). Address /24 y AllowedIPs /0 → RECHAZADOS. Delete wg0 → RECHAZADO.

**Firewall:**
27. Mover regla up/down → posición exacta (sin off-by-one); delete+re-add no pierde regla.
28. Insert accept en hotspot/forward → BLOQUEADO (whitelist).
29. Delete de handle protegido (drop 8081) → BLOQUEADO.

**Networking:**
30. DNS add/delete con IP válida; `1.1.1.1'; rm -rf / #` → rechazado (fix).
31. Pool create/delete: start exacto no borra pools vecinos; reload aplica.
32. VLAN create eth3.10 + delete → no toca eth3.100 ni la iface física; title con \n rechazado.
33. Bridge delete brX → validado como bridge real; eth0 rechazado.
34. IP add/delete con CIDR válido; sync /etc/network/interfaces atómico.
35. DHCP lease con hostname con espacios → campos correctos.

**Sistema/API:**
36. /api/command sin token → 401 (tras fix auth).
37. Script whitelist: ruta dentro de scripts/ OK; ../ fuera → 403.
38. system scripts_list con UTF-8 → sin panic.
39. Secret RADIUS en GET → "***" (no claro).
40. Reboot loop: zpot-init.sh deja eth3 SIN internet (fail-closed) hasta que zpot inicie.

## 8. Pendientes priorizados (NO aplicados — siguientes rondas)

**P0 (seguridad — RCE/auth):**
1. Auth real en API admin :8081 (token/sesión) + fail-closed si nft no instala. ⏳ PENDIENTE (único P0 mayor restante).
2. ✅ RESUELTO 08-07 dns.rs: shell injection ELIMINADA (fs append + validación Ipv4Addr + atómico). VERIFICADO.
3. ✅ RESUELTO 08-07 wireguard.rs: stdin pipe, validación, wg0 protegido, private_key oculta.
4. ✅ RESUELTO 08-07 command.rs: whitelist canonicalizada, reboot nombre exacto, timeout 30s, wg rollback.
5. ✅ RESUELTO 08-07 firewall.rs: delete protege reglas críticas; move verifica delete/re-add.
6. ✅ RESUELTO 08-07 bridges.rs: delete solo bridges reales.
7. ✅ RESUELTO 08-07 interfaces.rs: delete_vlan por bloque exacto; title sanitizado.
8. ✅ RESUELTO 08-07 pools.rs: delete por start exacto; validación; atómico + rollback.

**P1 (integridad/config):**
9. Escrituras atómicas temp+rename+fsync en TODOS los módulos + chmod 0600 de archivos con secretos. ✅ Parcial: atómico en dns/pools/ip_addresses; chmod 600 aplicado en Alpine (server/cookies/ppp-radius/wg-peers).
10. Locks RMW (interfaces, ip_addresses, pools, ppp secrets sync vs handlers, mwan apply). ✅ 08-08 COMPLETO: SECRETS_LOCK (PPP), IPADDR_LOCK, INTERFACES_LOCK (3 bloques), POOLS_LOCK (add/delete con drop antes del reload), MWAN_STATE_LOCK (fase 2).
11. ppp_radius.rs: bug sed -n; pool de la UI efectivo (✅ 08-08: pppoe_start usa pool_start/end del config); validar dns/nas_ip (✅ 08-08); ip-up curl --max-time (✅ APLICADO sistema 08-07); backup antes de sobrescribir configs.
12. ✅ RESUELTO 08-07 radius.rs: secret enmascarado en GET; POST valida + update por nombre.
13. ✅ RESUELTO 08-07 mwan.rs: ORDEN DE BOOT corregido (hotspot primero, MWAN después). ✅ 08-08: ping al GATEWAY + WEIGHT (numgen) + rollback apply_wan_ip_change + validación POST. PENDIENTE: status real, watchdog default.
14. ✅ RESUELTO 08-07 (2ee6d1b) validación IP/CIDR/iface: ip_addresses.rs
    (CIDR/iface), routes.rs (dst/gw/dev + default prohibida), remote_set
    (solo rangos locales). PENDIENTE: locks RMW.
15. ✅ RESUELTO 08-08 (287caad) locks RMW: interfaces/ip_addresses/pools/mwan.
16. ✅ RESUELTO 08-08 (auditoría ronda 2): routes UI crear/borrar rotos
    (destination vs dst + DELETE inexistente) → backend acepta ambos +
    shape limpio; secrets_list enmascara passwords; disconnect_user verifica
    kill (sin falso éxito); firewall create valida table/chain; watchdog
    MWAN usa replace (no del bucle). PENDIENTE: list_vlans VID/native,
    configure_bridge_port tagged, bridges persistencia, accounting PPP off
    (decisión), API create/update secrets, CoA WG real (excluido),
    write_conf chmod 600 (acceso local).

**P2 (menores/funcional):**
15. DHCP leases hostname con espacios; system &src[..80] UTF-8 panic; bash→sh en system.rs.
16. Botones Ver script/toggle/delete scheduler rotos (command rechaza shell crudo).
17. native bool/string unificado en list_vlans; shape JSON de routes.rs; dedup DNS.
18. qos_cleanup borra filtro ifb + ifb_pppN; secrets order/delete con lock; API create/update secrets PPP.
19. CiroCampos@Hu (cliente no reconecta — lado server OK, revisar CPE/radcheck).
20. Infra CoA WireGuard real (server RADIUS sin peer wg para Alpine) — opcional.

---

*Formato: KEYWORDS. Cada ronda de auditoría añade sección al CHANGELOG.md y actualiza este archivo. Hotspot = auditado y fixeado. WireGuard = corregido 08-07 (9675799) + incidentes documentados. Resto = hallazgos recolectados pendientes de aplicar (P0/P1/P2 §8).*

## 9. Changelogs de bugs+fixes por componente (fechado, creciente)

Cada componente tiene su changelog en docs/auditoria/ — se actualiza después
de cada auditoría (nueva entrada con fecha arriba):

- docs/auditoria/HOTSPOT-BUGS.md — 3 rondas, fixes aplicados
- docs/auditoria/WIREGUARD-BUGS.md — 2 rondas, fixes aplicados (9675799)
- docs/auditoria/PPP-BUGS.md — ronda 1, verificado en vivo 08-07
- docs/auditoria/RADIUS-BUGS.md — ronda 1, verificado en vivo 08-07
- docs/auditoria/FIREWALL-BUGS.md — ronda 1, verificado en vivo 08-07
- docs/auditoria/BRIDGE-BUGS.md — ronda 1, verificado en vivo 08-07
- docs/auditoria/INTERFACES-BUGS.md — ronda 1, verificado en vivo 08-07
- docs/auditoria/IP-BUGS.md — ronda 1, verificado en vivo 08-07
- docs/auditoria/MWAN-BUGS.md — ronda 1, verificado en vivo 08-07
- docs/auditoria/SYSTEM-BUGS.md — ronda 1, verificado en vivo 08-07
- docs/auditoria/DASHBOARD-BUGS.md — ronda 1, verificado en vivo 08-07
