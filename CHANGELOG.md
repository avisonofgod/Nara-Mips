# Changelog - Zpot-RS

## [2026-08-08] - Dashboard UP/DOWN + MWAN fix + speedtest multiwan + Files/Export/Import

- [UI] Dashboard: eliminado el subtítulo de IPs/desglose — las cards
  WAN1/WAN2 quedan solo con UP/DOWN.
- [FIX] /routing/mwan: el botón ➕ "no funcionaba" — render() re-inicializaba
  this.wans desde el config en CADA render y el push de addWan() se perdía.
  Ahora inicializa SOLO la primera vez (_initialized) y Refrescar lo resetea
  para re-sincronizar desde el backend.
- [SPEEDTEST] Multiwan: el backend acepta CUALQUIER WAN del store MWAN
  (antes wan1/wan2 hardcode) y la UI genera un botón por WAN existente
  (dinámico desde /api/mwan/status).
- [FILES] NUEVO: GET /api/system/files (lista /etc/zpot/*.json),
  GET /api/system/export (JSON {components}), POST /api/system/import
  (restaura con backup .bak-ts, valida nombres), GET
  /api/system/files/hotspot/download (tar.gz del portal),
  POST /api/system/files/hotspot/upload (respalda + extrae, rollback).
  UI: system-files.html con Exportar/Importar + Descargar/Subir hotspot.

## [2026-08-08] - Dashboard IPs por WAN + speedtest en MWAN + walled-garden fix + auditoria

- [UI] Dashboard: ELIMINADA card "Clientes". WAN1/WAN2: "Flujos" →
  "IPs" (clientes unicos de conntrack por WAN) + desglose "PPP N ·
  Hotspot M" cuando la WAN esta activa (backend: count_all_client_ips
  clasifica por subred 192.168.20.x PPP / 192.168.10.x Hotspot).
- [UI] Speedtest WAN: movido de /system/general a /routing/mwan (bajo
  la tabla de WANs). Eliminado de General.
- [HOTSPOT] walled-garden FIX: si se añade SOLO domain (ip='—'), el
  backend lo resuelve a IP con getent hosts — antes se guardaba '—' y
  apply_wg_rules NO aplicaba ninguna regla nft (no funcionaba). Ahora
  valida IPv4, usa el iface del config (antes eth3 hardcodeado) y
  rechaza dominios no resolubles con mensaje claro.
- [AUDITORIA] 39 GETs probados en Alpine (todos 200, JSON valido).
  FIX: /api/mwan/config devolvia {"status":"ok"} (stub) — ahora expone
  mode/distribution/wans reales del store.

## [2026-08-08] - /ip/pools: editar pool (lease o rango) + columna ACCIONES

- [BACKEND] NUEVO POST /api/pools/update {start, end, interface, lease}:
  reemplaza la línea dhcp-range con start exacto (misma validación que
  create: IPv4, iface segura, lease sin espacios; atómico + reload +
  rollback). La IP inicial identifica el pool y NO se cambia.
- [UI] /ip/pools: columna "ACCIONES" con ✏️ Editar (modal precargado con
  IP final, interface y lease actuales) + 🗑 Eliminar.

## [2026-08-08] - Cookies log + DHCP leases (sort/buscador) + ppp-secrets fluido

- [HOTSPOT] zlog! ahora #[macro_export] (disponible en main.rs). Eventos de
  cookie a /tmp/zpot.log: "se conecto desde cookie (via MAC)", "COOKIE
  RECHAZADA (no existe server-side)", "COOKIE IGNORADA — MAC no coincide".
  Antes eran println! (stdout perdido con nohup > /dev/null) — la
  reconexión por cookie no se podía verificar.
- [DHCP] dhcp_leases.rs: hostname limpio — el clientid (01:xx) ya no se
  pega al hostname; "*" se muestra como "—". Antes "ZTE-Blade 01:xx" y
  "* 01:xx" rompían el buscador.
- [UI] /ip/dhcp-server: buscador por IP/MAC/Hostname + click en "IP"
  ordena menor→mayor (toggle ▲▼). /ip/pools: columna LEASE visible
  (el backend ya la exponía).
- [UI] /ppp/secrets: mismo mecanismo de velocidad que /interfaces/list
  (poll único + delta de bytes del kernel + fetch directo sin cache).
  Switch "⚡ Velocidad: OFF/ON (2s)": OFF (default) = poll base 5s y
  columnas velocidad "—" SIN calcular ni pedir velocidad extra.

## [2026-08-08] - Speedtest de WANs (CLI oficial Ookla + UI)

- [NUEVO] POST /api/system/speedtest {wan, n}: mide la capacidad REAL
  de cada WAN. Usa el CLI oficial de Ookla 1.2.0 musl
  (/usr/local/bin/speedtest, multi-stream) — el speedtest-cli python
  subestima (1 stream + latencia alta). Lee la IP del store MWAN, crea
  regla ip rule temporal pref 30000 (no toca el balanceo fwmark de
  clientes), corre N rondas con bind -i, BORRA la regla SIEMPRE.
  Media de rondas + historial /etc/zpot/speedtest-history.json
  (50 muestras/WAN, atómico). Lock: 1 prueba a la vez.
- [UI] System > General: bloque "Speedtest WAN" con botones WAN1/WAN2,
  rondas + media + media histórica.
- [DOC] docs/SPEEDTEST-WANS.md. Resultados iniciales: ambas WANs =
  Starlink, muy variables (WAN1 ~147/37, WAN2 ~82/19 Mbps media de 3).
- Aclarado: Ookla mide última milla (capacidad del upstream); el VPS
  (iperf3) mide path real (puede subestimar, nunca exagera). Para
  capacidad de cada WAN: Ookla. VPS NO tocado.

## [2026-08-08] - /api/ppp/active: ELIMINADO uptime (baja de 330ms a ~15ms)

- [PERF] ppp.rs: eliminado ppp_uptime() + format_uptime() del active_list.
  ANTES ejecutaba 3 comandos sync por interfaz (pgrep+awk+awk = 7
  procesos) → ~235 procesos por llamada con 33 PPP (330ms). AHORA solo
  `ip -j -s link show type ppp` → ~15ms. El operador solo necesita
  conectado/desconectado (los contadores del dashboard solo usan length).
- [UI] ppp-active.html y ppp-secrets.html: columna Uptime ELIMINADA
  (backend ya no la expone). ppp-secrets: índices de celdas ajustados.

## [2026-08-08] - /ppp/secrets: velocidad 1s con switch ON/OFF

- [UI] ppp-secrets.html: botón "⚡ Velocidad 1s: OFF/ON" (persistido en
  localStorage). OFF (default): poll normal 5s, sin peticiones extra.
  ON: poll de /api/ppp/active cada 1s SOLO en esta página (1 petición/s)
  que actualiza las celdas Rx/Tx sin re-renderizar la tabla. El render
  completo (secrets+estado) sigue a 5s. Los timers se pausan durante
  drag&drop.
- [FIX] cache apiFetch: el poll de 1s debe usar fetch DIRECTO — apiFetch
  cachea 3s ('ttl || 3000' anula ttl=0) y devolvía siempre el mismo dato
  → delta 0 → velocidad 0.00. Además el render de 5s reusa el último
  valor del poll (speedVal) en vez de recalcular (recalcular con el
  snapshot recién escrito daba 0.00 y pisaba las celdas).

## [2026-08-08] - /ppp/secrets: columnas Rx/Tx velocidad en tiempo real (Mbps)

- [UI] ppp-secrets.html: nuevas columnas "Rx ↓ Mbps" y "Tx ↑ Mbps" para
  sesiones conectadas. Velocidad calculada en el cliente con el delta de
  bytes del kernel (stats64 rx/tx que ya expone /api/ppp/active) entre
  polls de 5s: (bytes₂-bytes₁)*8/Δt. Primera muestra "—" hasta tener 2
  puntos. Verificado con JuanCa@Hu (id 1, activo).

## [2026-08-08] - Reauth hotspot espaciado + timeout RADIUS hotspot 6s

- [RADIUS] Fix "Ignoring duplicate packet" en FreeRADIUS: causa raíz = el
  reauth del hotspot (cada 300s, cycle%5==0) reauticaba TODAS las sesiones
  en el MISMO ciclo → ráfaga sincronizada → FreeRADIUS encolaba (SQL
  authorize) → el NAS reenviaba a los 3s → duplicados (RFC 5080).
- [1] hotspot.rs: reauth ESPACIADO por sesión — `(cycle + session_idx) % 5`.
  Cada sesión sigue reauticándose cada 300s pero en ciclos distintos
  (~N/5 por ciclo en vez de N). Server NO tocado.
- [2] hotspot.rs radius_auth: timeout 3s → 6s (igual que radius_timeout del
  PPP, ya en radiusclient.conf). Aplica al reauth, login y validación de
  cookies del portal.
- Aclarado: el timeout NO es atributo RADIUS configurable desde
  daloRADIUS/ticket — es config local del NAS (código/radiusclient.conf).
  Acct-Interim-Interval SÍ es atributo por-usuario (radreply) y queda
  documentado como opción para reducir interims del PPP.

## [2026-08-08] - Fix #11 interims PPP + dock System reorganizado

- [#11] radius_timeout 3 → 6s: mitiga "Interim accounting failed" del pppd
  (el server RADIUS tarda >3s en responder; el plugin no tiene flag para
  desactivar interims). Aplicado en radius.rs (fallback) + radiusclient.conf
  real (backup). Aplica a nuevas sesiones ppp.
- [UI] Dock System reorganizado: "General" (ex Identity) con Kernel/Arch/
  CPU/cores/Load/Mem/Disk/time/timezone + Users, Scripts, Scheduler, Logs,
  Files. ELIMINADOS del subnav: Resources, Clock, NTP. Páginas huérfanas
  system-resources/clock/ntp eliminadas del repo. Mapeo pageInits limpiado.

## [2026-08-08] - revisa.md: logs auth PPP/hotspot + system info + script huérfano

- [P2] NUEVO /api/ppp/logs/auth: eventos de AUTENTICACIÓN RADIUS/PPP
  (accept/reject/failed/timeout/MSCHAP/ip-up) desde /var/log/messages.
  Botón "🔐 Auth RADIUS" en ppp-logs.html (errores en rojo).
- [P2] NUEVO /api/hotspot/logs: eventos del portal/RADIUS del hotspot
  (login/BYPASS/ACCT/INTERIM/COA/REJECT) desde /tmp/zpot.log + página
  hotspot-logs.html + enlace "Logs" en el subnav del dock Hotspot.
- [P2] system/logs: ahora filtra ppp/pppoe/zpot/watchdog (100 líneas).
- [P2] system-identity.html: añade CPU Load (5m), Total/Used/Free Memory
  y Total/Used/Free Disk (el backend ya los exponía).
- [P2] ELIMINADO scripts/install-ppp-qos.py (huérfano confirmado — el
  QoS lo aplica zpot vía API desde 08-02).

## [2026-08-08] - Boot pool parametrizado + revisa.md documentado

- [FIX] Boot scripts (local.d/zpot-red.start + init.d/pppoe): pool PPPoE
  PARAMETRIZADO desde /etc/zpot/ppp-radius.json (antes -R .100 -N 100
  hardcodeado). Aplica al próximo boot/reinicio del pppoe-server (el
  proceso actual sigue intacto). Backup .bak-20260808 en Alpine.
  Copias de referencia en scripts/zpot-red.start + scripts/init.d-pppoe.
- [FIX] main.rs L740: comentario obsoleto corregido (el orden REAL es
  init_hotspot_nft → apply_nft_rules desde 9d7958b).
- [FIX] interfaces.rs list_interfaces: excluye ifb0/ifb1/ifb_pppN del
  listado (revisa.md #2) — son internas del QoS.
- [DOC] NUEVO docs/PENDIENTES-REVISA.md: estado de TODOS los puntos del
  archivo del operador (FreeRADIUS duplicate packet investigado, watchdog
  verificado NO huérfano, install-ppp-qos.py HUÉRFANO, sistema files/logs
  pendientes).

## [2026-08-08] - Pendientes CERRADOS: atómicos, backups, tokenizer, bridges, radius DELETE

- [P1] interfaces.rs: 3 escrituras de /etc/network/interfaces ahora
  ATÓMICAS (tmp+rename): create_vlan, delete_vlan, set_vlan_title.
  (completa el patrón dns/pools/ip_addresses/mwan/radius/ppp).
- [P1] ppp_radius.rs: BACKUP (.bak-<ts>) antes de sobrescribir
  radiusclient.conf, servers y pppoe-server-options (rollback manual).
- [P2] firewall.rs create_nft_rule: TOKENIZER que respeta comillas dobles
  (antes split_whitespace rompía "comment \"mi regla\"").
- [P2] bridges.rs: PERSISTENCIA en /etc/network/interfaces (auto/iface/
  bridge_ports) en create/delete — antes el bridge se perdía al reboot.
  Nombre validado. Atomic.
- [P2] radius.rs: NUEVO DELETE /api/radius/servers (por nombre) — antes
  no existía; protege el fallback "radius-main" (no deja la lista vacía).
- [P2] bridges.rs port_add/port_remove: PERSISTEN los ports en
  /etc/network/interfaces (bridge_ports) — antes solo runtime; ahora el
  puerto agregado/quitado sobrevive al reboot.
- [OK] Verificado que apply_qos_ppp ya exige ip+iface; mwan status ya
  usa detección real (detect_status, no el store).
- [ALTA] firewall: `nft list chain` SIN `-a` NO muestra "# handle N" —
  la protección de delete (unwrap_or(true)) negaba TODOS los borrados
  (ninguna regla se podía eliminar). Fix: `nft -a list chain` en los 4
  sitios (delete_nat_rule, delete_filter_rule, move_nft_rule,
  move_nft_rule_to) + comparación en la MISMA línea (no i-1). VERIFICADO:
  regla counter de prueba creada con comment (tokenizer) y borrada OK.

## [2026-08-08] - Auditoría ronda 3: firewall + routes UI + pendientes P2

- [ALTA] conntrack_status ROTO en Alpine: `conntrack -o json` NO existe
  (v1.4.8) → dashboard SIEMPRE 0. Fix: `conntrack -L` texto (estado =
  columna 4). /proc/net/nf_conntrack no existe en este kernel.
- [P1] delete_nat_rule: protegidas reglas CRÍTICAS (masquerade/redirect/
  drop 8081) — antes se podia borrar el masquerade del hotspot y los
  clientes perdian internet en silencio.
- [P1] move_nft_rule_to: verifica delete/add (antes `let _` → regla
  PERDIDA silenciosamente si el re-add fallaba).
- [P2] routes UI: la fila "default" mostraba botón 🗑 que SIEMPRE fallaba
  (backend la prohibe) → ahora badge "MWAN" sin botón. Hint de creación
  ya no sugiere 0.0.0.0/0 (prohibida). Escape escHtml en onclick.
- [P2] list_vlans: vlan_id NUMÉRICO (antes string) + campo "prefix".
- [P2] configure_bridge_port: pvid SIEMPRE untagged (antes pvid+tagged =
  error "Invalid argument") + verifica que el puerto sea miembro de un
  bridge (error claro si no).
- [P2] NUEVO POST /api/ppp/secrets (upsert por username) — antes NO
  existia API create/update. "***" conserva el password actual.
  VERIFICADO: crear, update, enmascarado y delete OK.
- [P2] list_vlans: ip/prefix REALES (antes siempre vacíos) — el parse de
  `ip -o -4 addr` usaba splitn(5,' ') y ip -o ALINEA con multi-espacios
  → campos vacíos. Fix split_whitespace + dev clean_name (sin @parent).

## [2026-08-08] - P2 auditoría ronda 2: routes UI rotos + secrets + firewall + watchdog

- [ALTA] routes.rs: crear/borrar rutas desde la UI SIEMPRE fallaba —
  la UI mandaba {destination, iface} y el backend leia {dst, ifname}.
  Ahora acepta ambos nombres; metric aplicado; list() devuelve shape
  limpio {"routes":[...],"rows":N} (antes array raro [routes,{rows}]).
  UI ip-routes.html alineada (usa POST /api/routes/delete, no DELETE
  /api/routes inexistente). VERIFICADO en vivo (crear+borrar ruta OK).
- [P2] ppp.rs secrets_list: passwords ENMASCARADOS ("***") — antes se
  exponian al SPA (ninguna pagina los usa).
- [P1] ppp.rs disconnect_user: verifica el RESULTADO del kill — antes
  `let _` ignoraba el fallo y respondia "disconnected" falso. Ahora
  responde 400 si no pudo matar. kill_cmd reescrito en raw string.
- [P2] firewall.rs create_nft_rule: valida table (hotspot/mwan/filter),
  chain alfanumerica, rule sin caracteres de control.
- [P1] main.rs watchdog MWAN: NUNCA `ip route del default` en bucle
  (borraba TODAS las default incl. la otra WAN; si el add fallaba el
  router quedaba sin salida). Ahora SOLO `ip route replace` (atomico).

## [2026-08-08] - P1 lote 5: locks RMW completos + doc NAS RADIUS

- [P1] Locks RMW en TODOS los módulos con read-modify-write:
  IPADDR_LOCK (ip_addresses sync_interfaces), INTERFACES_LOCK (3 bloques
  de /etc/network/interfaces: create_vlan, delete_vlan, set_vlan_title),
  POOLS_LOCK (add/delete dnsmasq.conf, drop antes del reload .await),
  MWAN_STATE_LOCK (fase 2 de post_mwan_config). Completa el patrón
  SECRETS_LOCK de PPP — se acabaron las carreras entre handlers y syncs.
- [DOC] NUEVO docs/NAS-RADIUS-ALPINE.md: arquitectura REAL del sistema
  (NAS RADIUS: hotspot + PPPoE autentican contra FreeRADIUS .63; no hay
  auth local), configs reales, flujos auth/acct/CoA/QoS, redes, nft,
  puertos, servicios, checklist de verificación.
- [DOC] config-examples nuevos: ppp-radius.json, radius-servers.json,
  radiusclient.conf (NOTA: /etc/radiusclient, no -ng), pppoe-server-options.
- [DOC] Hallazgos auditados: unbound :53 (resolver) + dnsmasq :5353 (DHCP),
  pppoe-server con pool provisional -R .100 hasta próximo restart,
  radius-servers.json vacío con fallback hardcodeado, accounting PPP off.

## [2026-08-08] - P1 lote 4: password local, atómicos, validaciones MWAN/PPP

- [ALTA] ppp.rs: NUNCA más usar el username como password en chap-secrets
  (auth local bypass). Secrets sin password (auto-registro MSCHAPv2) →
  secret ALEATORIO que no coincide → la auth local falla (su auth es RADIUS).
- [P1] Escrituras atómicas tmp+rename añadidas: secrets PPP
  (save_secrets_to_disk), mwan write_state, radius save_servers.
- [P1] ppp_radius.rs post_config: valida nas_ip/dns1/dns2/pool_start/pool_end
  como IPv4 (antes cualquier basura se guardaba).
- [P1] mwan apply_wan_ip_change: si el add tras el flush falla, ROLLBACK
  restaura las IPs viejas (antes la iface quedaba SIN IPv4 = WAN perdida).
- [P1] mwan post_mwan_config: valida iface/ip/gateway de cada WAN antes de
  tocar el sistema (antes inyección de líneas en /etc/network/interfaces).

## [2026-08-08] - P1/P2 lote 3: locks, MWAN weight, pool UI, ifb leak (globales)

Cambios GLOBALES (config única para TODOS los clientes hotspot+PPP).
CoA WG real: PENDIENTE (excluido por el usuario).

- [P1] LOCK secrets PPP (SECRETS_LOCK): serializa load-modify-save en
  secrets_delete, secrets_order y auto_register_from_connection (el sync
  60s ya no puede "resucitar" secretos borrados ni perder reorden).
- [P1] MWAN check_wan_ping: ping al GATEWAY de la WAN (antes 8.8.8.8 con
  -I dependía de la ruta MAIN → WAN viva declarada caída).
- [P1] MWAN weight: el distribution "70/30" AHORA se aplica (nft numgen
  random mod 100 con rangos) — antes jhash 50/50 fijo.
- [P1] pppoe_start: pool parametrizado desde ppp-radius.json (pool_start/
  pool_end de la UI) — antes -R hardcodeado 192.168.20.100 -N 100.
  load_config() hecho pub.
- [P2] qos_cleanup: elimina filtro UP en ifb_pppN + ip link del ifb
  (leak corregido — antes el ifb quedaba para siempre).
- VERIFICAR: nft mwan con numgen, pppoe sin reinicio (el pppoe-server ya
  corre; el cambio aplica al próximo start).

## [2026-08-07] - P1/P2 seguros (sin tocar WireGuard — acceso remoto)

- [P1] ip_addresses.rs: validación CIDR/iface en add/delete (antes aceptaba
  IPs en lo/eth0/ppp*/ifb → rompía MWAN); sync_interfaces con escritura
  atómica tmp+rename.
- [P1] routes.rs: dst validado (CIDR) y PROHIBIDO 0.0.0.0/0 y ::/0 (secuestro
  de tráfico); delete exige dst (antes OPCIONAL → borraba la default);
  gateway/ifname validados.
- [P1] ppp.rs remote_set: IP restringida a rangos locales (192.168.10.x
  hotspot / 192.168.20.x PPPoE) — antes cualquier IP = DNAT abierto.
- [P2] system.rs: bash→sh (Alpine sin bash); UTF-8 panic en src_preview
  (byte 80) corregido (chars().take(80)).
- [P2] dhcp_leases.rs: hostname con ESPACIOS ya no desplaza los campos
  (antes parts[3] truncado + id mal).
- [P2] scripts/ip-up del REPO sincronizado con el sistema real (guarda MAC
  + curl qos/radius con --max-time 5) — el repo estaba desactualizado.
- [P2] ppp_radius.rs write_ip_up: curl con --max-time 5 (el sed MAC ya era
  correcto en el código).
- WireGuard NO modificado (vía de acceso remoto).

## [2026-08-07] - P0/P1: fixes de seguridad e integridad (lote pendientes)

- [P0] dns.rs: ELIMINADA shell injection (sh -c echo/sed) → fs append con
  validación Ipv4Addr + escritura atómica tmp+rename + dedup. VERIFICAR.
- [P0] command.rs: whitelist de scripts CANONICALIZADA (evita traversal ../);
  fire-and-forget SOLO para nombre exacto reboot-alpine.sh (antes substring
  "reboot"); run_script con timeout 30s; cmd_wireguard_add con rollback si
  wg set falla (antes dejaba zombie).
- [P0] bridges.rs: delete SOLO permite bridges reales (ip -j link show type
  bridge) — antes podía borrar eth0/eth3/wg0 (caída total).
- [P0] interfaces.rs: delete_vlan por bloque EXACTO (antes substring borraba
  eth3.100 al borrar eth3.10); set_vlan_title sanitiza caracteres de control
  (antes \n inyectaba comandos en el boot).
- [P0] pools.rs: delete por start EXACTO (antes substring borraba de más) +
  error si no hay match; create valida IPs/iface/lease; escritura atómica +
  VERIFICA reload dnsmasq con rollback (antes "success" sin aplicar).
- [P0] firewall.rs: delete_filter_rule PROTEGE reglas críticas (8081,
  redirect, masquerade, drop, accept); move_nft_rule verifica delete y
  re-add (antes errores ignorados = regla perdida silenciosamente).
- [P1] radius.rs: GET enmascara secret ("***"); POST valida campos y
  actualiza por nombre (antes duplicaba); ppp_radius.rs get_config usa
  el JSON enmascarado.
- [P1] main.rs: ORDEN DE BOOT corregido — init_hotspot_nft AHORA PRIMERO,
  apply_nft_rules (MWAN) después (antes la tabla hotspot no existía →
  masquerade WAN fallaba en silencio tras reboot).

## [2026-08-07] - correcciones verificación final (wg-peers stale + permisos + ip-up)

- [ALTO] wg-peers-wg0.json STALE (peer MssW del server RADIUS) → CORREGIDO
  en Alpine: JSON reescrito con el peer REAL del VPS (IIZT, allowed
  10.7.0.0/24, endpoint 95.111.238.114:51820, keepalive 25, sin preshared).
  Ahora una regeneración de wg0.conf desde el JSON sería segura.
- [ALTO] wireguard.rs: peers_add y peers_delete ahora RECHAZAN interface=wg0
  (antes solo delete() estaba protegido) → el panel NO puede regenerar
  wg0.conf con peers del JSON. VERIFICAR con build.
- [MEDIA] Permisos 644 → 600 en Alpine: hotspot-server.json,
  hotspot-cookies.json, ppp-radius.json, wg-peers-wg0.json (secretos).
  wg0.conf ya era 600.
- [MEDIA] ip-up: curl de QoS con --max-time 5 (evita bloqueo de pppd si
  el backend cuelga). Aplicado en /etc/ppp/ip-up de Alpine.
- Verificación general: túnel WG OK (handshake fresco, transfer creciendo),
  zpot PID 14408, nft input correcto, SSH OK.

## [2026-08-07] - auditoría completa por componente (verificada contra Alpine REAL)

- REVISIÓN solo-lectura del sistema Alpine por componente (configs, procesos,
  permisos, nft, rutas) — NADA modificado en el sistema.
- CREADO docs/auditoria/ — 11 changelogs de bugs+fixes POR COMPONENTE
  (fechado, creciente, sin código): HOTSPOT (3 rondas), WIREGUARD (2 rondas),
  PPP, RADIUS, FIREWALL, BRIDGE, INTERFACES, IP, MWAN, SYSTEM, DASHBOARD.
- HALLAZGO NUEVO: wg-peers-wg0.json contiene peer STALE del server RADIUS
  (MssW) — riesgo de caída de gestión si el panel regenera wg0.conf.
- VERIFICADO en vivo: ip-up EXISTE y llama QoS por API (sin --max-time);
  ip-down EXISTE (limpia QoS + elimina interfaz); radius-servers.json VACÍO
  (módulo sin uso); permisos 644 en archivos con secretos; dnsmasq port 5353;
  33 sesiones ppp + 33 ifb_ppp; mwan.json wan1 eth0/wan2 eth1.
- AUDITORIA-COMPLETA.md: secciones actualizadas con verificaciones en vivo
  (PPP, RADIUS, WireGuard) + §9 índice de changelogs por componente.
- GUIA-AUDITORIA.md: inventario de docs con docs/auditoria/*-BUGS.md.
- Memoria: flujo de auditoría guardado (revisar → doc por componente →
  changelog fechado creciente → actualizar AUDITORIA-COMPLETA+GUIA → git).

## [2026-08-07] - docs: auditoría WG actualizada + escenarios por componente

- WIREGUARD-REVISION.md reescrito (2026-08-07): sin código, solo texto/puntos/
  palabras clave. Nuevo: incidentes key rotada (1zGW→Mojz→Qa1c) y NAT rotativo
  del cliente; fixes del panel; checklist de diagnóstico y verificación;
  escenarios A-J actualizados.
- AUDITORIA-COMPLETA.md: sección 4.5 WireGuard marcada ✅ CORREGIDO (9675799)
  con estado de cada hallazgo; §4.2-4.9 (PPP, RADIUS, MWAN, Firewall, Networking,
  System, Command) completadas con PALABRAS CLAVE + ESCENARIOS DE PRUEBA +
  PUNTOS A REVISAR por componente (sin código); §7 escenarios WG 26a/26b/26c
  (key rotada, NAT rotativo, panel); §8 P0 item 3 wireguard.rs → RESUELTO.
- GUIA-AUDITORIA.md: inventario WireGuard → ✅ CORREGIDO 08-07; mapa de docs
  WIREGUARD-REVISION.md actualizado.

## [2026-08-07] - fixes panel WireGuard + rotacion key wg0 Alpine

- RECUPERACION TUNEL: la private key de wg0 en Alpine habia sido rotada
  (pub 1zGW... → Mojz...) y el VPS seguia con el peer viejo → sin
  handshake 12h. FIX: peer del VPS actualizado a la pub nueva. Luego
  rotacion COMPLETA de la key de Alpine (nueva pub Qa1c...): cualquier
  equipo con la key vieja queda invalidado. wg0.conf VPS actualizado
  (backup .bak-rotacion).
- [P0] wireguard.rs create(): reemplazado `<(echo '{}')` (process
  substitution ROTO en busybox ash — creaba interfaces con private key
  FALLABA en Alpine) por `printf '%s' ... | wg set ... private-key
  /dev/stdin`.
- [P0] wireguard.rs peers_add(): mismo fix con preshared-key (stdin
  pipe via tokio), + validacion AllowedIPs: rechaza 0.0.0.0/0, ::/0,
  10.7.0.0/24, 10.7.0.0/16 (evita full-tunnel / captura de gestion,
  incidente wg1 2026-08-05).
- [P0] wireguard.rs delete(): protegido wg0 (rechaza eliminar la
  interfaz de gestion).
- [P0] wireguard.rs peers(): Path validado (solo alfanumerico, _, -)
  antes de usarlo en sh -c (RCE via Path).
- [P1] wireguard.rs list(): private_key ya NO se expone al frontend
  (String::new()); se sigue leyendo del disco para escribir el conf.
- [P1] wireguard.rs create(): Address validado — exige /32 (IPv4) o
  /128 (IPv6); rechaza /24 (la ruta captura la VPN de gestion).
- [P1] UI: defaults corregidos — wireguard-interfaces.html Address
  "10.0.0.1/24" → "10.7.0.15/32"; wireguard-peers.html AllowedIPs
  "10.7.0.0/24" → "10.7.0.15/32".

## [2026-08-05] - incidente wg1 (caida de red de gestion) — leccion documentada

- Usuario creo wg1 desde la UI con Address 10.7.0.12/24 + AllowedIPs
  0.0.0.0/0, ::/0 + peer del server RADIUS. Consecuencias:
  1) La /24 creo ruta 10.7.0.0/24 por wg1 capturando el trafico de la
     VPN de gestion → SSH/ping a 10.7.0.5 muertos (replies salian por
     wg1 hacia un peer sin ruta de retorno).
  2) /etc/wireguard/wg0.conf habia quedado con el peer del server
     (MssW, AllowedIPs 10.7.0.0/24) en lugar del peer del VPS → el
     tunel de gestion quedaba SIN AllowedIPs del VPS (handshake OK,
     datos descartados: allowed ips: (none)).
- Recuperacion (consola local): wg-quick down wg1 + ip link del wg1 +
  rc-update del wg-quick.wg1 + ip route flush 10.7.0.0/24 + default
  restore + wg set wg0 peer IIZT allowed-ips 10.7.0.1/32 + wg0.conf
  restaurado (peer VPS 10.7.0.1/32, chmod 600) + limpieza wg1.conf.
- Sistema verificado: zpot PID 5221 (80/8081), hotspot activo, 32
  sesiones PPP, accounting 1813 OK, wg0 solo peer VPS.
- LECCION: la UI de WireGuard debe VALIDAR (pendiente P0 de
  AUDITORIA-COMPLETA): rechazar AllowedIPs 0.0.0.0/0 y ::/0, exigir
  Address /32, y NUNCA tocar wg0.conf (la interfaz de gestion).

## [2026-08-04] - commit (AUDITORIA-COMPLETA.md: 17 modulos auditados)

- Nuevo `docs/AUDITORIA-COMPLETA.md`: auditoría completa del sistema Zpot
  (hotspot fijado + 17 módulos restantes: networking, PPP/RADIUS,
  sistema/firewall/mwan/wireguard/command). Incluye inventario de
  componentes, mapa backend-frontend, configuración por componente,
  40 escenarios de prueba y pendientes priorizados P0/P1/P2.
- Hallazgos críticos NO aplicados (P0): API admin :8081 sin auth,
  shell injection en dns.rs, sh -c en wireguard.rs, whitelist traversal
  en command.rs, move firewall off-by-one, delete bridges/interfaces
  por substring. Pendientes listados en el documento.

## [2026-08-04] - commit (auditoria completa hotspot: 28 fixes)

### Auditoria 3 agentes (auth/sesion-qos/infra) — 28 hallazgos aplicados

ALTO:
- zpot-init.sh: regla autenticados `ip saddr @hotspot_auth` INVALIDA (set
  concatenado) — nft fallaba en silencio y la regla NUNCA aplicaba. Ahora
  `ip saddr . ether saddr @hotspot_auth`. + DROP FINAL eth3 (FAIL-CLOSED
  real si zpot no arranca). + masquerade eth0 (solo eth1 antes).
- Filtro UP huerfano en ifb_{iface}: disconnect y logout borraban el filtro
  src en `iface` pero apply_qos lo crea en `ifb_{iface}` — quedaba apuntando
  a una clase eliminada.
- apply_qos: HTB exige ceil >= rate — el ceil se clampaba a 100000 pero el
  rate NO; un plan >100M hacia fallar la clase, read_tc_bytes=0 y el interim
  expulsaba al cliente ACTIVO por idle. Clamp ambos.
- init_hotspot_nft parametrizado con cfg.iface/cfg.gw (antes todo hardcodeaba
  eth3/192.168.10.0/24 — cambiar la config rompia el firewall).
- Timeout RADIUS != reject: nuevo flag reachable — server caido muestra
  "Servidor RADIUS no disponible" y NO cuenta el intento en el anti-bruteforce
  (5 timeouts ya no bloquean a un cliente con clave correcta).

MEDIO:
- Octets Acct-Input/Output INVERTIDOS (RFC 2866): 42 ahora lleva tx (subida),
  43 rx (bajada) — antes los reportes de radacct salian cruzados.
- session_id con u32 (u16 colisionaba 1/65536 en el mismo segundo).
- find_password_for_username(user, mac): 2 cookies del mismo usuario con MACs
  distintas devolvian el password de la primera entrada.
- Cookie rsplit_once(':') — un username con ':' (RFC 2865) rompia el split.
- portal_logout verifica que la MAC de la cookie coincida con la ARP del peer
  (refuerzo CSRF: cookie de otro dispositivo no sirve).
- interim: saturating_sub (reloj atras no produce expulsiones masivas).
- interim: reauth RADIUS cada 5 ciclos (300s) en vez de cada 60s por sesion
  (el polling CoA ya detecta sesiones cerradas).
- interim: renovacion del bypass nft INDEPENDIENTE del trafico (sesion
  inactiva dentro del idle ya no pierde el elemento a las 24h).
- auto_reconnect: `ip neigh show dev <iface>` (antes TODA la tabla ARP — una
  MAC visible en eth0/eth1 disparaba redirect espurio) + to_lowercase.
- ARP cleanup con de-bounce: exige 2 observaciones FAILED/INCOMPLETE
  consecutivas (roaming/PSM/blip ya no expulsan).
- input chain :80 restringido a iface/lo/wg0 (antes WAN/ppp* veian el login
  del hotspot y podian brute-forcear RADIUS).
- find_session_ip: CoA con solo User-Name y 2+ sesiones del usuario -> NAK
  (antes mataba una arbitraria).
- portal_auth: username/password vacios rechazados sin roundtrip RADIUS y
  sin tocar el contador brute-force.
- FAILED_LOGINS poda >1024 IPs.

BAJO:
- User-Name >250 bytes rechazado antes de construir el paquete RADIUS.
- cookies_delete case-insensitive (UI minusculas borraba 0).
- tmp unico con pid en save_sessions/save_cookies (2 escrituras concurrentes
  ya no compiten por el mismo .tmp).
- escape alogin fallback: meta refresh normalizado (backslashes duplicados
  generaban HTML invalido) + escape_html del username.
- escHtml() escapa comillas simples en app.js (XSS en onclick del admin).
- parse_coa_attrs: comentario corregido (break es correcto — longitud invalida
  no permite saltar).
- Comentario burst tc corregido (era 1s, no 100ms).

## [2026-08-04] - commit (escenarios conexion + pendientes ronda 2)

### Revision de escenarios de conexion del cliente hotspot (IP x MAC x cookie)

- FIX (escenario C): handle_root y handle_hotspot_fallback verificaban
  "authed" SOLO por IP — una IP reasignada por DHCP a OTRO dispositivo
  heredaba la sesion (veia el status del usuario anterior, no navegaba
  y NO podia loguearse). Ahora has_active_session_for_peer() compara la
  MAC del peer con la de la sesion (misma regla que portal_root).
  get_mac_from_arp es pub para reutilizarlo desde main.rs.
- Escenarios verificados en codigo:
  A) 1a conexion (sin cookie): root -> no authed -> sin cookie -> login
     -> portal_auth (anti-bruteforce, ARP MAC, RADIUS 2 intentos) ->
     sesion + nft + tc + acct start + cookie set.
  B) Re-conexion con cookie (misma IP+MAC): auto_reconnect_from_cookie
     (cookie server-side + MAC en ARP) -> /hotspot/portal -> RADIUS
     re-auth con password server-side -> sesion nueva.
  C) IP misma, MAC diferente: YA NO hereda la sesion (fix arriba) ->
     login del nuevo dispositivo; la sesion de la IP es del que tiene
     la MAC correcta.
  D) IP diferente, MAC misma (DHCP renew, con cookie): auto-reconexion
     -> portal_root -> BUG-D cierra la sesion de la IP vieja (misma
     MAC) con accounting stop -> sesion nueva en la IP nueva.
  E) IP+MAC diferentes: sesion nueva normal.
  F) Re-login manual (misma IP+MAC, sin cookie): portal_auth hace
     Accounting-Stop de la sesion previa antes de crear la nueva.
  G) Cookie con MAC que no matchea ARP: no auto-reconecta -> login.
  H) Cookie expirada/borrada server-side: rechazada + set-cookie
     Max-Age=0 (limpia el browser).

### Pendientes de la auditoria aplicados (ronda 2)

- RADIUS AUTH CON REINTENTO: 1 paquete UDP perdido ya NO produce un
  reject falso — se reintenta 1 vez si hay timeout (un Access-Reject
  explicito se devuelve de inmediato).
- PERSISTENCIA ATOMICA: save_sessions_to_disk y save_cookies_to_disk
  escriben a .tmp + rename (un corte a mitad de fs::write dejaba el
  JSON corrupto y el boot no reconstruia sesiones).
- CoA LISTENER SIN BLOQUEO: el Disconnect-Request (nft/tc/acct — lento)
  se ejecuta en tokio::spawn y se responde ACK de inmediato (antes el
  recv loop se bloqueaba y los CoA en rafaga se perdian).
- LOGOUT LIMPIA LA COOKIE DEL BROWSER: portal_logout envia
  set-cookie hs_session=; Max-Age=0 (antes la cookie quedaba y el
  cliente se re-autenticaba solo al navegar).
- portal_status IGNORA ?username= del query: siempre muestra la sesion
  del PEER (/status?username=VICTIMA ya no muestra el estado ajeno).

## [2026-08-04] - commit (pending fixes: auth lockout, cookie segura, CSRF, secret, CoA auth)

### Pendientes de la auditoria hotspot (5 fixes)

- ANTI-BRUTE-FORCE del login: 5 intentos fallidos por IP en 60s ->
  bloqueo temporal ("Demasiados intentos"). NO es el rate-limit de
  ancho de banda (ese viene del VSA RADIUS Mikrotik-Rate-Limit o del
  fallback rate_limit del server — QoS); este limita los intentos de
  autenticacion por fuerza bruta via portal.
- COOKIE SEGURA: hs_session ya NO lleva el password (antes
  base64(user:pass:mac) viajaba en claro por HTTP — sniffer obtenia el
  password RADIUS). Ahora base64(user:mac); el password se busca
  server-side en hotspot-cookies.json al re-autenticar (portal_root,
  auto_reconnect_from_cookie).
- LOGOUT CSRF: portal_logout exige cookie hs_session valida del peer
  (un <img> cross-site no envia la cookie SameSite=Lax -> bloqueado).
- get_server ya NO expone radius_secret (devuelve "***"); post_server
  conserva el actual si llega "***" o vacio (la UI no rompe el secret).
- CoA listener autentica el paquete: Request Authenticator MD5
  (RFC 5176 §3.1) — antes bastaba el origen IP 10.7.0.0/16 (cualquier
  peer WG podia mandar Disconnect y expulsar a todos).

## [2026-08-04] - commit (hotspot audit fixes batch)

### Auditoria hotspot: fixes de conexion/desconexion (13 fixes)

- CRIT: DNAT remoto (ppp.rs) restringido a iifname { wg0, eth1 } —
  antes `iifname != "lo"` mandaba TODO el HTTPS forward de PPPoE/LAN
  al panel del AP y dejaba el 8082 abierto a hotspot no-auth.
- CRIT: ARP cleanup en task DEDICADO (main.rs) — la llamada estaba
  tras el loop infinito del watchdog MWAN (codigo muerto, nunca corria).
- CRIT: apply_qos sin rates -> clase contador 100Mbit (antes early-return
  -> read_tc_bytes=0 -> expulsaba por idle a clientes ACTIVOS).
- CRIT: polling CoA: respuesta NO-array = ciclo cancelado (antes
  expulsaba a TODOS) + periodo de gracia 90s para sesiones nuevas.
- ALTO: login sin MAC (ARP sin resolver) -> reintentos + rechazo claro
  (antes creaba sesion sin bypass = cliente atrapado en portal).
- ALTO: re-login misma IP -> Accounting-Stop de la sesion previa.
- ALTO: portal_root verifica MAC del peer (IP reasignada no hereda sesion).
- ALTO: disconnect NO-idle borra cookie server-side (admin reset efectivo).
- ALTO: interim refresca cookie server-side si sesion activa >6 dias.
- ALTO: IP bindings blocked -> insert (antes add tras auth accept = inerte).
- ALTO: established/related restringida a daddr != 192.168.10.0/24
  (port-isolation eth3->eth3 intacto + sin spoof-injection admin↔AP).
- MEDIO: session_id con sufijo aleatorio (colision 1s).
- ALTO: zpot-init.sh fail-closed (set con MAC, redirect :80, drop final).

## [2026-08-04] - commit (hotspot accounting 1813 fix)

### BUG CRITICO: accounting RADIUS iba al 1812 (auth) — clientes sin internet

- send_accounting usaba split_host_port(server, 1813) pero el config
  radius trae "161.97.67.63:1812" (puerto AUTH) -> Start/Stop/Interim
  salian al 1812, FreeRADIUS los descartaba y radacct NUNCA registraba
  la sesion.
- Consecuencia: el polling CoA veia "RADIUS activas: 0" y expulsaba a
  TODOS los clientes cada 30s (log: "MAX ... YA NO activa en RADIUS —
  cerrando") -> nadie tenia internet (store={}, set vacio, JSON vacio
  a las 15:20).
- FIX: accounting SIEMPRE al puerto 1813 (RFC 2866); del config solo
  se toma la IP. El AUTH sigue en el puerto del config (1812).

## [2026-08-04] - commit (wireguard create/delete + peer full)

### WireGuard: crear interfaces (wg1/wg2) + peers con preshared/keepalive

- POST /api/wireguard/interfaces NUEVO (antes 404): crea la interfaz
  en vivo (ip link add + wg set + ip addr multi (ipv4,ipv6) + mtu + up),
  wg genkey si private_key vacia, persiste /etc/wireguard/<name>.conf
  y crea restore al boot: /etc/init.d/wg-quick.<name> (symlink OpenRC)
  + rc-update add — se recupera igual que wg0 al reiniciar.
- DELETE /api/wireguard/interfaces: ip link del + rm conf + rm
  /etc/zpot/wg-peers-<name>.json + rc-update del + rm init.d.
- PEERS: + PresharedKey y PersistentKeepalive (form + handler).
  Persistencia por interfaz en /etc/zpot/wg-peers-<name>.json (todos
  los campos; wg dump no expone preshared/keepalive) -> se regenera el
  .conf con los bloques [Peer] para que wg-quick levante todo al boot.
- UI peers: selector de Interfaz (wg0/wg1/...) + campos Preshared Key
  y Keepalive.
- OJO colision IP: no crear wg1 con 10.7.0.5/24 (ya usada por wg0)
  — ip addr add falla "File exists".

## [2026-08-04] - commit (ip-remote disable + ppp admin block)

### Acceso Remoto: boton Desactivar + 8081 protegido para PPPoE

- /ip/remote: boton "⛔ Desactivar" — POST /api/ip/remote con ip vacio
  borra las reglas nft (8082 y 443, comment zpot-remote*) y el archivo
  /tmp/zpot-remote.txt. Cleanup extraido a cleanup_remote_rules().
- main.rs init_hotspot_nft: nueva regla prerouting
  `iifname ppp* tcp dport 8081 drop` — el admin SPA (8081) queda
  bloqueado para clientes PPPoE igual que para hotspot eth3
  (el input chain ya lo cubria; esto corta antes del DNAT/routing).
- Portal login NO se afecta: el portal usa redirect del 80 para
  clientes eth3 saliendo; el DNAT remoto usa 8082/443 dirigidos al
  servidor (443 estaba libre, no es del portal ni del admin).

## [2026-08-04] - commit (ip-remote)

### Acceso Remoto: /ppp/remote -> /ip/remote (Hotspot + PPPoE)

- Frontend: pagina movida de PPP a IP (subnav PPP sin "Remoto";
  subnav IP + "Remote" en /ip/remote). Renombrado
  static/pages/ppp-remote.html -> static/pages/ip-remote.html.
- Se ELIMINO la seccion "Clientes PPP activos" (fetch /api/ppp/active
  y botones "Usar") — la pagina ahora es generica: IP Hotspot o PPPoE.
- API renombrada: /api/ppp/remote -> /api/ip/remote (get/post);
  handlers siguen en ppp.rs (utilidad DNAT generica).
- BACKEND: el DNAT ya NO filtra por `iif eth1` — ahora aplica por
  cualquier interfaz (wg0 VPN, eth1 internet, eth3 hotspot):
  `iifname != "lo" tcp dport 8082 dnat ip to <ip>:80 comment zpot-remote`.
  Acceso: http://10.7.0.5:8082 (VPN) o http://IP_PUBLICA:8082 (WAN).
- FIX forward (main.rs init_hotspot_nft): el aislamiento
  `iif eth3 ip daddr { 10.7.0.0/24, ... } drop` mataba el SYN-ACK del AP
  hacia 10.7.0.1 (respuesta al DNAT remoto) — handshake nunca completaba.
  Nueva regla ANTES de los drops: `iif eth3 ct state established,related
  accept`. Paquetes NUEVOS siguen aislados; respuestas pasan.
- HTTPS: el panel del AP (Ubiquiti) responde 302 a https://<host>/ →
  remote_set ahora tambien DNAT 443 -> <ip>:443 (comment zpot-remote-https,
  cleanup cubierto por prefijo zpot-remote). Flujo completo:
  http://10.7.0.5:8082 -> 302 -> https://10.7.0.5/ -> 200 panel AP.
- VERIFICADO: AP 192.168.10.174 responde :80 (302) y :443 (200, title
  "Ubiquiti"); DNAT probado en vivo -> https://10.7.0.5/ carga el panel.

## [2026-08-04] - commit 5674f8a

### CoA configurable en UI + modo Polling HTTP + docs

- Frontend /hotspot/server: toggle "CoA / Desconexión remota"
  (Activado/Desactivado) + selector Modo CoA:
  - WireGuard (UDP 3799 — Disconnect entrante)
  - Polling HTTP (RADIUS API — sin UDP entrante)
  + URL Polling + IP WireGuard detectada mostrada
  (ej "Disconnect debe llegar a: 10.7.0.5:3799").
- Backend: get_server() incluye wg_ip (runtime, no se guarda);
  HotspotServer + coa_enabled/coa_mode/coa_poll_url (serde default);
  listener UDP condicional (solo modo udp); spawn_coa_polling() nuevo.
- MODO POLL (opcion C): cada 30s GET al endpoint RADIUS (sessions.php)
  que devuelve las sesiones activas (acctstoptime IS NULL, NAS 192.168.10.1).
  Las sesiones locales ausentes -> session_disconnect_internal(cause 2).
- ENDPOINT: /var/www/html/zpot-coa/sessions.php en 161.97.67.63
  (PHP + MySQL radius, secret por query param). Ver docs/coa.md.
- VERIFICADO en vivo: polling expulsó las 5 sesiones fantasma
  (RADIUS las cerró con Lost-Carrier masivo ~01:19-01:20);
  store=0, active=[], nft=0. API wg_ip=10.7.0.5.
- Docs: docs/coa.md (código PHP, instalación, configuración).

## [2026-08-04] - commit 7d644d2

### Hotspot ARP cleanup: expulsar sesion COMPLETA al irse del WiFi

- ANTES: al detectar ARP FAILED/INCOMPLETE solo borraba el elemento
  nft — la sesion quedaba en el store hasta que el interim la
  expulsara por idle (hasta 1h con idle alto tipo 7dCorridos=3600).
- AHORA: recoge las IPs FAILED/INCOMPLETE y llama
  session_disconnect_internal(cause 2 Lost-Carrier) — limpia store +
  nft + tc + manda accounting Stop. Expulsion en ~30s al irse del
  WiFi (en vez de 1h por idle).
- session_disconnect_internal ahora pub (usada por ARP cleanup).
- Validado: .245 FAILED sin sesion = no-op correcto; la funcion ya
  fue probada con el CoA/Disconnect de G4RP (limpió store+nft+tc).
- RELACION CoA: el ARP fix cubre "cliente se fue del WiFi". El CoA
  sigue cubriendo "cliente conectado pero RADIUS cierra la sesion"
  (admin/saldo). Ambos conviven; CoA ya desplegado en UDP 3799.

## [2026-08-04] - commit d76750a

### Hotspot: CoA/Disconnect-Request listener (RFC 5176) UDP 3799

- CASO G4RP: RADIUS cerro la sesion con Lost-Carrier (01:20) pero Zpot
  NO se entero — el reauth usa Access-Request (auth), no el accounting.
  Zpot seguia enviando interims a una sesion ya cerrada y el cliente
  navegaba SIN que RADIUS cuente saldo (~6h).
- Fix: spawn_coa_listener() — escucha Disconnect-Request (40) y
  CoA-Request (43) en UDP 3799.
  - Valida origen: IP del server RADIUS configurado o VPN 10.7.0.0/24
  - parse_coa_attrs: User-Name(1), Framed-IP(8), Calling-Station(31),
    Acct-Session-Id(44), Acct-Terminate-Cause(49)
  - find_session_ip: busca por sid > ip > mac > username
  - Disconnect: session_disconnect_internal (nft+tc+Stop+store),
    responde Disconnect-ACK(41)/NAK(42) con Response Authenticator MD5
    (RFC 5176 §3.2)
  - CoA: re-aplica QoS si trae VSA rate-limit, ACK(44)/NAK(45)
- PRUEBA REAL end-to-end: Disconnect-Request manual (python) desde
  wg 10.7.0.1 -> 10.7.0.5:3799 con user=G4RP ip=192.168.10.195.
  Respuesta Disconnect-ACK auth_valid=True "Session terminated".
  Verificado: store, set nft, clase tc y API active SIN G4RP.
- PENDIENTE infraestructura: el server RADIUS (161.97.67.63) no tiene
  peer wg para 10.7.0.5 (Alpine) — para que FreeRADIUS/daloradius
  mande Disconnect automatico hay que agregar el peer wg o NAT.

## [2026-08-04] - commit 186773e

### Hotspot flujo completo: fixes auditoria

- FIX-1 (puerto RADIUS): radius_auth y send_accounting ignoraban el
  puerto del config (SIEMPRE 1812/1813). Nuevo split_host_port() —
  respeta ip:puerto (default 1812 auth / 1813 acct).
- FIX-2 (NAS-IP): radius_auth tenia NAS-IP hardcoded 192.168.10.1
  mientras send_accounting leia cfg.gw — RADIUS veia 2 NAS-IP si gw
  cambiaba. Ahora ambos leen cfg.gw.
- FIX-3 (Gigawords): Acct-Input/Output-Octets 32-bit truncaban a 4GB.
  Ahora si rx/tx > 4GB se envian Acct-Input/Output-Gigawords (52/53)
  con la parte alta del contador (RFC 2869).
- FIX-4 (RADIUS spoofing): radius_auth aceptaba CUALQUIER datagrama
  como Access-Accept. Ahora verifica origen == servidor configurado
  (recv_from) + valida Response Authenticator MD5(Code+ID+Len+
  ReqAuth+attrs+secret). Verificado: Access-Reject legible en vivo.
- FIX-5 (ARP): get_mac_from_arp fallback hardcodeaba "eth3" — ahora
  usa cfg.iface.
- FIX-6 (rate_limit fallback): nuevo campo HotspotServer.rate_limit
  ("up/down ceil_up/ceil_down"). resolve_qos(): VSA de RADIUS si
  viene, sino fallback del config. Aplicado en portal_auth + cookie
  auto-login. UI /hotspot/server con campo "Rate Limit fallback".
  JSON /etc/zpot/hotspot-server.json regrabado con 1M/2M 2M/3M.
- Dead code: spawn_interim_task (~100 lineas) eliminado (reemplazada
  por spawn_interim_global).
- Dedup: handle_root y handle_hotspot_fallback compartian ~85 lineas
  de cookie/ARP/redirect — extraida auto_reconnect_from_cookie().
- Verificado: build 53.82s, auth RADIUS reject OK (authenticator
  validado), browser /hotspot/server 7 campos, 0 errores JS.

## [2026-08-04] - commit bc38153

### Hotspot portal login: auditoria -> fixes

- BUG-1 (visual, CONFIRMADO en vivo): login.html pintaba "🛜 Inicia
  sesión" en ROJO siempre. render_login reemplazaba $(if error) y
  $(endif) por separado dejando "alert" en class="info alert" sin
  error (CSS .alert fondo #ffe0e0). Fix: reemplazar el bloque
  completo $(if error)alert$(endif) primero ("" sin error, "alert"
  con error).
- BUG-2 (XSS): $(error) se inyectaba sin escapar (Reply-Message de
  RADIUS o ?error=). Fix: fn escape_html() en hotspot.rs + aplicar
  a $(error). Verificado: ?error=<script> -> &lt;script&gt;.
- BUG-3 (cookie): hs_session sin HttpOnly (base64 user:pass:MAC
  legible por JS). Fix: HttpOnly en Set-Cookie (login + 4 clears).
  Secure NO aplica (portal es HTTP).
- status.html y logout.html: css relativo "css/style.css" no
  resolvia desde /status. Fix: href absoluto
  /hotspot/portal/static/css/style.css.
- Archivos muertos eliminados: rlogin.html (template MikroTik no
  servido, $(link-redirect) sin handler) y md5.js (sin refs).
- Verificado en vivo: login sin error class="info ", con error
  class="info alert" escapado, POST auth invalido muestra
  "Usuario no encontrado" (Reply-Message RADIUS), status 302 a
  login sin sesion. Build 52.11s.

## [2026-08-04] - commit 3db3dcc

### Hotspot: fusionar profile en server — UN solo config

- HsProfile ELIMINADO. idle_timeout + shared_users ahora viven en
  HotspotServer (/etc/zpot/hotspot-server.json).
- Backend: get_active_profile(), get_profiles/post_profile/
  delete_profile, HS_PROFILES, HS_PROFILES_PATH eliminados.
  Los 4 usos (portal_auth + cookie auto-login) ahora leen
  cfg.idle_timeout / cfg.shared_users directo.
- Rutas /api/hotspot/profiles y /delete ELIMINADAS de main.rs.
- UI: /hotspot/server ahora tiene Idle Timeout + Shared Users
  (sin selector Profile). Pagina /hotspot/profiles BORRADA
  (hotspot-server-profiles.html) y subnav Hotspot sin item.
- HotspotServer final: iface, gw, html_dir, idle_timeout,
  shared_users, radius, radius_secret.
- JSON /etc/zpot/hotspot-server.json regrabado con los nuevos
  campos; hotspot-profiles.json eliminado (huérfano).
- Verificado: API 6 campos, browser form 6 labels, subnav sin
  Profiles, 0 errores JS, build 50.40s.

## [2026-08-04] - commit 13da916

### Hotspot: limpiar cosmeticos server+perfil; selector Profile

- HotspotServer ahora SOLO tiene lo que el backend lee:
  iface, gw (NAS-IP), html_dir, radius, radius_secret, profile.
  Eliminados (0 usos backend): name, pool, pool_range, dns_server,
  domain, login_by, use_radius.
- HsProfile ahora SOLO: name, idle_timeout, shared_users.
  Eliminados: rate_limit (parse_rate_limit_str era dead code, sin
  callers) y cookie_timeout (cookie siempre 604800 hardcoded).
- UI /hotspot/server: form reducido a Interface/Gateway/HTML dir/
  RADIUS server + NUEVO selector Profile (poblado desde
  /api/hotspot/profiles, selecciona cfg.profile).
  ANTES: el save() NO enviaba profile → post_server fallaba
  "missing field profile" (bug latente).
- UI /hotspot/profiles: sin Login By, sin Rate Limit, sin Cookie.
- El pool DHCP REAL sigue siendo /etc/dnsmasq.conf (Pools),
  pool/pool_range del server nunca se leyeron.
- JSON /etc/zpot/hotspot-server.json y hotspot-profiles.json
  regrabados via API (limpios).
- Verificado: API + browser (form con selector default, tabla 3
  cols, modal 3 campos); 0 errores JS; build 1m00s.

## [2026-08-04] - commit 1c9b306

### Hotspot: eliminar opcion Login By del perfil (ya es RADIUS)

- El hotspot SIEMPRE autentica via RADIUS (portal_auth llama radius_auth()
  incondicional, gate solo `rad_srv.is_empty()`). Local auth NO existe.
- El campo `login_by` de HsProfile era COSMETICO: el backend nunca lo
  leia; solo pintaba el checkbox en la UI (hotspot-server-profiles.html).
- Fix: se elimina `login_by` de HsProfile (src/handlers/hotspot.rs) y el
  bloque Login By del form + columna de la tabla en la UI.
- Datos: /etc/zpot/hotspot-profiles.json regrabado via API sin login_by
  (serde ya ignoraba el campo extra al deserializar).
- Quedan en el perfil SOLO: name, idle_timeout, shared_users, rate_limit,
  cookie_timeout.
- Verificado: API + tabla + modal sin Login By; 0 errores JS; build 1m04s.

## [2026-08-02] - commit 420a77d

### IP Bindings hotspot: whitelist IP+MAC (fix)

- apply_ib_rules() solo usaba `ip saddr` en la regla nft — la MAC se
  guardaba en el JSON y la pedía la UI pero se IGNORABA en nft.
- Fix: si la entrada tiene mac, la regla exige `ip saddr` + `ether saddr`
  (whitelist IP+MAC) — otra maquina que tome la misma IP por DHCP ya no
  hereda el bypass. Sin MAC → regla por IP sola (compat).
- Verificado: POST /api/hotspot/ip-bindings con
  {ip:192.168.10.120, mac:78:8c:b5:58:60:8e, type:bypassed} → nft
  `iif eth3 ip saddr 192.168.10.120 ether saddr 78:8c:b5:58:60:8e accept
  comment "zpot-ib" # handle 36`.
- dnsmasq: agregado `dhcp-option=eth3,138,161.97.67.63` (option 138
  CAPWAP — formato Omada segun FAQ TP-Link 1360) para que los EAPs
  Omada encuentren el controlador remoto; el option 43 UniFi se
  mantiene intacto (01:04:a1:61:43:3f).

## [2026-08-02] - commit f31d459

### /ppp/secrets auto-registro por sync syslog+kernel (sin ip-up)

- Nuevo task periódico (60s, primer tick inmediato): lee /var/log/messages
  ("user X logged in intf pppN ... remote <IP>") + kernel (ip -json addr
  type ppp) y llama auto_register_from_connection por sesión activa.
- Idempotente: no duplica ni sobreescribe IPs fijas (.2-.37 manuales).
- Refactor: parse_syslog_users() y fetch_ppp_links() extraídos y
  reutilizados por active_list (misma fuente, mismo comportamiento).
- spawn_ppp_sync_task() llamado desde main.rs al arranque.
- Verificado: borrados nato@Hu + Rosalba@Hu → re-agregados por el sync en
  ≤60s con su IP del kernel; 33/33 restaurado; WG-OK.

## [2026-08-02] - commits 4bd0c5e, d533801

### Uptime real de sesiones PPP en /api/ppp/active

- `uptime` calculado desde el starttime del proceso pppd
  (/proc/PID/stat campo 22, CLK_TCK=100) — sin depender del syslog.
- Correlación pppN→pppd por MAC (/var/run/ppp-mac-<iface> →
  `remotenumber <MAC>` en cmdline), porque la IP del cmdline del pppd
  es la provisional del pool (-R 192.168.20.100), no la final del RADIUS.
- Patrón pgrep con corchete en el último dígito (evita auto-match).
- UI PPP>Active ya tenía la columna Uptime (mostraba "-"); ahora el
  backend la llena. Verificado en vivo: 33/33 sesiones con uptime.

## [2026-08-02] - commits c918356, 1f4c290, 3388074, be5a495, ebc76b2, 1de1e97

### QoS — velocidades del RADIUS ahora se respetan (hotspot + PPP)

**Hotspot (12 clientes)**: bug de clasificación tc — `tc filter replace` sin
handle sobre hash u32 divisor 1 colapsaba a UN solo filtro (solo el último auth
quedaba limitado en bajada; los demás caían en clase default 100Mbit). Fix:
del+add con **prio único por cliente** (`100 + last_octet`) en apply_qos y en
los dels de logout/disconnect. Verificado: 3 filtros DOWN conviviendo (prios
152/340/341). Intentos descartados: replace (colapsa), tabla hash divisor 256
(el add crea su propia tabla divisor 1 e ignora la de control).

**PPP (33 clientes)**: el RADIUS externo SÍ emite Mikrotik-Rate-Limit
("1M/4M 2M/5M") pero radattr.so la OMITÍA porque el dictionary de radiusclient
en Alpine no tenía Mikrotik. Fix:
1. `/etc/radiusclient/dictionary`: `VENDOR Mikrotik 14988 Mikrotik` +
   `ATTRIBUTE Mikrotik-Rate-Limit 8 string Mikrotik` (formato radiusclient-ng,
   **5º campo = vendor**; NO BEGIN-VENDOR que colisiona con Framed-IP=8).
2. `qos_radius_apply` (ppp.rs): parsea por nombre `Mikrotik-Rate-Limit <valor>`
   (radattr escribe por nombre, no "26:hex") + fallback "26:".
Verificado end-to-end: 33/33 clientes reconectados con clase HTB rate=ceil_down
(5M) + filtro prio por IP.

Pendientes documentados en docs/pppoe.md: ppp-mac vacío ($6 ip-up), disconnect_user
pierde con rotación de /var/log/messages.

## [2026-08-02] - commits a440622, d03ba77, 7af2c5f

### Toggle Servidor PPPoE (frontend) + persistencia IPs + fixes

1. **PPP → Server** (nueva página `static/pages/ppp-server.html`): switch
   ON/OFF del pppoe-server con estado real (polling 5s). Endpoints:
   `/api/ppp/server/status|start|stop`. Solo el servicio (no VLANs ni IPs).
2. **Arranque al boot**: pegado a Zpot en `/etc/local.d/zpot-red.start`
   (pppoe-server arranca ACTIVADO junto con Zpot). Fix colateral: `pidof zpot`
   fallaba (comm truncado) → `pgrep -f '[t]arget/release/zpot'`.
3. **Persistencia de /ip/addresses**: add/delete de IP ahora sincroniza
   `/etc/network/interfaces` (bloques `iface X inet static`; runtime-only si no
   hay bloque). eth2 cambiado manualmente a 192.168.30.1/24 (runtime + file).
4. **Fix botón 🗑** (IP Addresses): el código legacy de app-v4.js llamaba
   `/api/ip-addresses/.../delete` (ruta inexistente → 404). Corregido a
   `/api/ip-addresses/:iface/:addr` (la página real ip-addresses.html ya usaba
   la ruta correcta).
5. **Lección BusyBox**: `pgrep -x pppoe-server` no matchea (argv[0] completo).
   Usar `pgrep/pkill -f '[p]ppoe-server -I'`.

## [2026-08-02] - examen exhaustivo hotspot (documentado, sin fixes)

Auditoría completa del sistema hotspot (autenticación, flujo, panel) y
actualización de docs/hotspot.md: correcciones (Idle-Timeout attr 28 NO se
parsea → idle siempre del perfil; "first sleep"; cookie no borrada en
disconnect; dnsmasq real = /etc/dnsmasq.conf; eth2=.30.1; accel-ppp y
pools.json eliminados) + nueva sección "Reconexión y hallazgos":
cookies server-side en memoria (auto-login roto tras reinicio zpot),
sesiones no reconstruidas al boot, walled-garden/ip-bindings no re-aplicados
al boot (regla nft desaparece), shared_users no validado, WG delete deja
reglas huérfanas. pendientes de fix (ver docs/hotspot.md).

## [2026-08-02] - commit 49e9fc6 — fixes reconexión hotspot

1. **Cookies a disco**: `/etc/zpot/hotspot-cookies.json` (save en cada
   mutación + load al boot). Auto-login sobrevive reinicios.
2. **Walled-garden/IP-bindings re-aplicados al boot** (init_hotspot_nft) +
   `cleanup_nft_by_comment` (reglas con comment zpot-wg/zpot-ib; borra
   huérfanas al delete). Verificado: regla Wikipedia presente tras reinicio.
3. **Idle-Timeout attr 28 parseado** (antes se ignoraba; siempre perfil local).
4. **shared_users validado** en portal_auth y re-auth por cookie
   ("Session limit reached").
Pendientes: reconstrucción de sesiones al boot (diseño), portal_status sin
username, get_mac_from_arp con neigh FAILED.

## [2026-08-02] - commits d9a7b89, 6cc1a4f — pendientes hotspot resueltos

1. **Sesiones a disco + reconstrucción al boot**: `/etc/zpot/hotspot-sessions.json`
   (save en cada mutación) + `restore_sessions_from_disk()`/`restore_and_spawn_interims()`
   → reconstruye sesiones, re-agrega bypass nft y respawnea interim tasks
   (mismo session_id → accounting continuo). FIX clave: init_hotspot_nft borra
   la tabla al boot → el restore NO valida contra el set (vacío), re-agrega
   todo. Verificado: 6 sesiones reconstruidas tras restart, active poblado,
   interim respawneado, WG intacto.
2. **portal_status por IP del peer** (ConnectInfo) — antes mostraba la 1ª
   sesión del store.
3. **get_mac_from_arp robusto**: fallback tabla ARP eth3 + reintento 400ms;
   guard en add_bypass_nft si MAC vacía.
Todos los hallazgos del examen de reconexión resueltos (ver docs/hotspot.md).

## [2026-08-02] - commit e0bbb92 + verificación reinicio PPP

1. **docs/panel-pages-map.md**: mapa completo panel → handler → archivo de
   configuración (46 subpáginas, 11 docks + portal :80).
2. **Verificación reinicio PPPoE** (toggle real): stop → 0 sesiones; start →
   reconexión automática 14/33 (~2 min) → 33/33 (~4-5 min) con QoS completo
   (clase HTB rate 5M + filtro por IP + radattr con VSA Mikrotik).

## [2026-08-02] - commit 424483e — QoS PPP rate/ceil transparente

VSA Mikrotik "1M/4M 2M/5M" antes se aplicaba rate==ceil (5M/2M). Ahora la
clase HTB usa rate (garantizado) y ceil (máximo) del VSA: DOWN 4M/5M, UP
1M/2M. apply_qos_ppp recibe los ceils; verificado en vivo (reconexión real).

## [2026-08-02] - commits 2911ba9, 199e236 — auto-registro PPP secrets

1. **/ppp/secrets auto-registro**: ip-up → qos_radius_apply agrega cliente
   nuevo (username de la conexión + IP del RADIUS + profile ClientesPPP,
   password vacío por MSCHAPv2) y regenera chap-secrets. Si existe, solo
   completa IP vacía. UI: columna Password eliminada (no disponible).
2. **/api/ppp/qos/cleanup** creado (ip-down lo llamaba → 404): limpia tc al
   desconectar.
Verificado: cliente ficticio agregado (34→33 al limpiar), chap-secrets OK.
Nota: commit 2911ba9 quedó roto (zlog! inexistente en ppp.rs) → corregido en
199e236 (eprintln!).

## [2026-08-01] - commits da47ae1

### Documentación
- **`docs/GUIA-SISTEMA.md` (nuevo)**: guía completa de entrada al sistema — paquetes
  necesarios (Alpine), configs `/etc/zpot/*.json`, estructura frontend/backend,
  topología de red, 10 docks del admin con sus páginas, y la lógica del hotspot por
  8 escenarios (primera conexión sin cookie, cookie re-auth, sesión activa, saldo
  agotado, idle timeout, logout, desconexión física, admin disconnect) indicando el
  archivo fuente de cada uno.
- **README.md**: corregido a datos reales — 2 puertos (:80 portal, :8081 admin),
  10 docks (no 11; Routing/MWAN vive en Interfaces), 45 páginas SPA + 16 handlers,
  app-v4.js, enlace a GUIA-SISTEMA.md.
- **STRUCTURE.md**: corregido — 10 docks reales, 45 páginas, app-v4.js como SPA
  activa, bloque `scripts/` reducido a lo que existe (ppp-zombie-watchdog.sh),
  docs/ con GUIA-SISTEMA.md.

## [2026-08-01] - commits 3171c4e, 49827a7

### Corregido
- **QoS rate/ceil: parse VSA corregido**. RADIUS entrega `rate_up/rate_down ceil_up/ceil_down`
  (ej `1M/4M 2M/5M`) → primer par = rate (garantía), segundo par = ceil (máximo),
  orden SUBIDA/BAJADA. Antes se interpretaba cruzado (`up_rate/up_ceil down_rate/down_ceil`)
  y un plan `1M/4M 2M/5M` se aplicaba como UP 1M→4M, DOWN 2M→5M en vez de
  UP 1M→2M, DOWN 4M→5M.
- **Fallback formato simple** sin `/` (`1M 2M` = UP/DOWN, ceil=rate) para no romper ese caso.
- **Documentacion actualizada** (docs/radius.md, docs/backend.md, docs/hotspot.md, MEMENTO.md)
  al formato real rate/ceil.
- Tokens extra del formato MikroTik completo (burst, priority, rx-min/tx-min) se ignoran.

### Verificado en vivo
- `512K/1M 1M/2M` → up=512K/ceil=1M, down=1M/ceil=2M ✓
- `1M/5M 2M/7M` (MAX) → up=1M/ceil=2M, down=5M/ceil=7M ✓
- Clases tc HTB: rate garantía, ceil máximo por usuario (eth3 bajada, ifb_eth3 subida).

## [2026-07-21] - commit 6ab2069

### Corregido
- **Warnings de compilacion eliminados (13→0)**: imports muertos (HashMap, Deserialize), structs no usados (PeerRequest,
  RemoveAddress, PoolRequest, RouteRequest), campos no usados (VlanCreate.title, Forwarder.port), funciones muertas (ip_addresses::remove,
  wireguard::add/delete), variables sin prefixar con `_`.
- **Build desde cero verificado**: `cargo clean && cargo build --release` en Alpine produce 0 errores, 0 warnings.
- **.gitignore actualizado**: excluye /target, node_modules/, frontend/, /src-tauri/target, *.log, *.new, .hermes/.
- **Estructura src/handlers/ completa**: 13 handlers registrados en mod.rs, ninguno huerfano.
- **53 pages HTML en disco = 53 pages en JS**: sincronizados, sin huerfanos ni faltantes.

### Añadido
- `STRUCTURE.md` -- arbol completo del proyecto con descripcion de cada archivo, 11 docks, lista de rutas API.
- `CHANGELOG.md` -- este archivo.

### Eliminado
- `.github/workflows/deploy.yml` (referenciaba backend-alpine/ inexistente).
- `src-tauri/` completo (UI nativa Tauri pospuesta).
- `backend-alpine/` completo (version Go obsoleta).
- `scripts/` completo.
- `.hermes/plans/` completo.
- Pages huerfanos: ip-adblock.html, ip-dhcp-client.html, ip-dhcp-server.html, ip-vlans.html.
- Archivos sueltos huerfanos: `dist/`, `_pages_backup/`, `app.js`, `app-v2.js`, `style.css`, `interfaces-vlans.js`.

### Notas
- `base.html` requiere **DOS** CSS en orden: `variables.css` primero, `main.css` segundo.
- `app-v3.js` carga pages via `fetch('/static/pages/' + name + '.html')` -- 53 pages servidos desde disco.
- Backend usa puerto 8080, no requiere sudo para rutas de solo lectura.

---

## [2026-07-19] - commit 3963684

### Corregido
- **CSS faltante**: se agregó `<link rel="stylesheet" href="/static/styles/variables.css">` en base.html.
- **Variables CSS**: ahora `--clr-bg`, `--font-family`, `--nav-bg` etc. se cargan correctamente.

### Añadido
- `ip-dhcp-leases.html` -- page faltante que JS esperaba.

### Eliminado
- Misma limpieza que 2026-07-21 (commit anterior).

---

## [2026-07-10] - Migracion Zig → Rust

### Cambio mayor
- Proyecto migrado de Zig a Rust (axum 0.7 + tokio).
- Repositorio Zig movido a `/root/__obsoleto_Zpot_zig/`.
- Backend Go `mwan-agent` obsoleto, reemplazado por handler Rust en puerto 8080.

## [2026-08-02] - commits fd03947, 6498a29

### /ppp/secrets — ID unico por cliente + accion Eliminar

- Campo `id` en PppSecret; asignacion por el MENOR entero libre
  (next_free_id); al eliminar una fila el id queda hueco y el proximo
  cliente sin registro (sync/auto-registro) lo reutiliza.
- Migracion automatica: clientes sin id reciben 1..n en orden al cargar.
- Endpoint POST /api/ppp/secrets/delete (elimina del JSON + regenera
  chap-secrets). UI: columna ID (primera, ordenada 1..n) + boton Eliminar.
- Verificado: delete 2412@Renau@Huayal (id=3) -> hueco -> sync lo
  re-agrego con id=3 en <=60s; tabla 33 filas 1..33 ordenada.

## [2026-08-02] - commits 14df6a6, e20ad7b

### Fix rotacion syslog + drag-drop reordenar PPP>Secrets

- parse_syslog_users ahora lee /var/log/messages.0 (rotado) + messages y
  correlaciona por IP (remote <IP>) ademas de por intf. El sync vuelve a
  ver 33/33 sesiones aunque el syslog haya rotado (antes solo 8).
- Caso real: RamonHu eliminado del secrets (su linea habia rotado) no
  reaparecia; con el fix el sync lo re-registro con su id 23 (hueco).
- Drag-drop en PPP>Secrets (estilo nftables): handle por fila, endpoint
  POST /api/ppp/secrets/order persiste el orden (no toca ids). UI ordena
  por id hasta el primer arrastre (ordenManual=true despues).
- Verificado en browser: JuanCa@Hu arrastrado 0->2, persistido y restaurado.

## [2026-08-02] - commit 5be8c8c

### /hotspot/active live (polling 5s)

- La pagina cargaba UNA vez + boton Refresh manual; el uptime se congelaba.
- Ahora setInterval(cargarHotspotActive, 5000) igual que PPP>Secrets;
  la tabla ya no parpadea "Cargando..." (no se borra al recargar).
- Verificado en browser: recarga automatica cada 5s y uptime 3h59m -> 4h0m.

## [2026-08-02] - commit 0e6dac7

### Cache-Control no-cache en estaticos del admin

- Los archivos de /static se servian sin Cache-Control (solo Last-Modified);
  el browser cacheaba paginas/JS y tras un deploy seguia con la version vieja
  ("no se actualizan"). Ahora SetResponseHeaderLayer agrega
  cache-control: no-cache a TODO el admin -> el browser revalida siempre.
- cache-control: no-cache. Tras un deploy, Ctrl+Shift+R (o cerrar pestana) carga lo nuevo.

## [2026-08-02] - commits d284778, 9dc1268

### Script de reinicio en /system/scripts + fix ejecucion de scripts

- Nuevo scripts/reboot-alpine.sh (sh + sync + reboot REAL); aparece en
  /system/scripts (backend escanea scripts/ dinamicamente, sin rebuild).
- UI: confirmacion FUERTE (doble) para cualquier script con "reboot" en
  el nombre (advertencia: se caen todos los clientes; vuelve solo al boot).
- FIX: /api/command era solo routeros_parser (RouterOS) → "Comando
  invalido: sh ..." en texto plano rompia r.json() del frontend
  ("Unexpected token 'C'"). Ahora acepta "sh <path>" SOLO si el path
  esta en whitelist (scripts/ del proyecto o /usr/local/bin/ppp-*.sh)
  y devuelve JSON {ok, output, stderr, exit}; los de reinicio se lanzan
  SIN esperar (el sistema muere antes de responder).
- FIX: Cargo.lock estaba en version 4 (cargo moderno) y el cargo 1.75
  del VPS no lo parsea → bajado a version 3 (ambos compilan).
- Verificado: watchdog ejecutado via /api/command JSON; no-permitido
  403; /ip/address/print intacto.
