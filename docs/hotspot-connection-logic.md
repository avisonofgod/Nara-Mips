# Lógica de Conexión Hotspot — Zpot-RS

> Documento de referencia (2026-08-04). Describe el flujo real del código
> (src/handlers/hotspot.rs) tras FIX-8, H1-H6.

## FLUJO BASE (aplica a todos los escenarios)

1. Cliente WiFi -> DHCP -> IP 192.168.10.x (pool .10-.200)
2. Navega HTTP :80 -> nft prerouting eth3:
   - IP.MAC en @hotspot_auth -> return (sigue sin portal)
   - si no -> redirect al portal (192.168.10.1)
3. Login OK (portal_auth) = 4 acciones:
   a. radius_auth: Access-Request UDP RADIUS:1812
      (timeout 3s async, username UPPERCASE)
   b. apply_qos: clases tc (down eth3, up ifb_eth3)
      rate/ceil del VSA (ej "1M/4M 2M/5M")
   c. add_bypass_nft: IP.MAC -> set hotspot_auth timeout 24h
   d. send_accounting Start -> RADIUS:1813
4. Cada 60s (interim global FIX-8):
   lee tc rx/tx -> send_accounting Interim -> idle check ->
   reauth RADIUS -> renovar set nft si falta <2h (H4)
5. Desconexion (session_disconnect_internal): accounting Stop
   (cause 1 user / 4 idle / 5 reauth-reject / 6 admin),
   borra nft element + tc classes, flush conntrack, del store

## ESCENARIO 1 — PRIMER CLIENTE SIN COOKIE

- Redirect -> portal_root: sin sesion, sin cookie -> login.html
- POST /hotspot/portal/auth:
  - RADIUS valida (Access-Accept trae QoS + idle_timeout)
  - H3: misma MAC con IP vieja? no -> skip
  - H6: shared_users alcanzado? si -> reemplaza la mas antigua
  - crea HotspotSession + save a disco
  - QoS tc + bypass nft + accounting Start
  - crea cookie hs_session = base64(user:pass:MAC)
    -> Set-Cookie browser + save_cookie_entry (7 dias)
  - sirve alogin.html -> navega libre

## ESCENARIO 2 — CON COOKIE

Caso A: ya tiene sesion activa -> alogin.html (sin RADIUS)
Caso B: cookie pero sin sesion (cerro browser / reinicio):
  - base64 -> user:pass:MAC_cookie
  - get_mac_from_arp(IP) == MAC_cookie? si difiere -> login
  - cookie_entry_exists (server-side)? si NO -> login
    "Cookie no valida" + limpia cookie browser
  - radius_auth re-auth completo:
    Reject -> login + limpia cookie
    Accept -> H3 -> H6 -> NUEVA sesion (QoS+bypass+Start)
  - alogin.html -> entra SOLO sin credenciales

## ESCENARIO 3 — SE CONECTA, SE VA, VUELVE (mismo dispositivo)

- "Se va" sin logout: sesion queda + set nft (24h);
  el interim la expulsa por idle (default 10 min)
  con accounting Stop cause 4. Cookie server-side SOBREVIVE.
- "Vuelve" en < 10 min: sesion activa -> alogin (sigue)
- "Vuelve" despues del idle: -> ESCENARIO 2B (reauth auto)
- "Vuelve" con IP nueva (DHCP renew): H3 cierra la vieja
  (misma MAC) con Stop cause 1 -> crea la nueva
- "Vuelve" tras LOGOUT explicito: H2 elimino la cookie ->
  login MANUAL (no hay auto-login)

## ESCENARIO 4 — DIFERENTE DISPOSITIVO (mismo usuario)

- B (MAC_B != MAC_A) hace login:
  - radius_auth OK
  - H3: MAC distinta -> no toca sesion de A
  - H6 shared_users:
    limite=1 -> cuenta 1 sesion (A) >= 1 -> CIERRA A (Stop)
      -> B TOMA SU LUGAR (el login nuevo GANA)
    limite=2 -> cierra solo si hay >= 2 (la mas antigua)
  - B crea su sesion + cookie propia (MAC_B)
- Interim con 2 dispositivos: reauth usa la cookie del user
  (mismo password) -> ambas sesiones viven si RADIUS acepta
- Logout de B (H2): elimina SOLO cookie MAC_B + sesion de B
  -> A sigue intacto (cookie MAC_A)

## NOTAS CLAVE

- Cookie = auto-login del MISMO dispositivo (MAC en la cookie)
- La MAC es el ancla de identidad del dispositivo
- H6 shared_users: el login nuevo GANA (reemplazo FIFO de la
  sesion mas antigua del mismo usuario; nunca rechaza).
  limite=0 = sin limite (multi-dispositivo total)
- Set nft expira a 24h; H4 lo renueva (delete+add) cuando
  falta <2h y hay trafico — 1 vez por sesion cada 22-24h
- Sesion fantasma (se fue sin logout): la expulsa idle (10 min)
  o reauth (cookie expirada/eliminada -> cause 6)
- RADIUS recibe: Start (login), Interim (60s), Stop (corte)
  con cause exacto (1/4/5/6) — facturacion y saldo en server
- Seguridad (H1/H2): logout y disconnect solo afectan la sesion
  del PEER; disconnect admin restringido a WG/LAN/localhost (403
  para clientes hotspot)
