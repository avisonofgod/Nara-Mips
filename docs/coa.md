# CoA / Desconexión Remota — Zpot-RS

> Documento 2026-08-04. Mecanismo para que el server RADIUS cierre sesiones
> locales del hotspot (caso G4RP: RADIUS cerró con Lost-Carrier pero Zpot
> seguía con la sesión activa y el cliente navegaba sin que se cuente).

## CÓMO FUNCIONA

Zpot expulsa una sesión local por 4 vías: idle-timeout (interim), ARP
FAILED/INCOMPLETE, reauth Reject, y CoA/Desconexión remota. Este doc cubre
la 4ª: el server RADIUS avisa a Zpot que una sesión se cerró.

## MODO 1 — WIREGUARD (UDP 3799, Disconnect-Request RFC 5176)

- Zpot escucha en UDP 3799 (listener CoA/Disconnect).
- Acepta paquetes SOLO de: servidor RADIUS configurado (cfg.radius) o
  VPN 10.7.0.0/24 (peer wg).
- El server manda Disconnect-Request (code 40) con User-Name / Framed-IP /
  Acct-Session-Id → Zpot mata la sesión (store + nft + tc + Stop cause 6)
  y responde Disconnect-ACK (41) con Response Authenticator MD5.
- DESTINO: la IP WireGuard del NAS (se muestra en la UI /hotspot/server,
  p.ej. 10.7.0.5:3799).
- REQUISITO: el server RADIUS debe poder alcanzar esa IP (peer wg o NAT).

## MODO 2 — POLLING HTTP (opción C, sin UDP entrante)

- Zpot consulta cada 30s un endpoint del server RADIUS que devuelve las
  sesiones ACTIVAS (acctstoptime IS NULL) del NAS.
- Las sesiones locales que YA NO están en esa lista fueron cerradas por
  RADIUS (Lost-Carrier/saldo/admin) → Zpot las expulsa (cause 2).
- VENTAJA: NO necesita reachability entrante (Zpot ya sale a internet
  hacia el server). Es la opción recomendada si no hay VPN.

### CÓDIGO DEL ENDPOINT (server RADIUS / FreeRADIUS + MySQL)

Archivo: `/var/www/html/zpot-coa/sessions.php` (en el server 161.97.67.63)

```php
<?php
// Zpot CoA polling endpoint — devuelve las sesiones ACTIVAS del NAS Zpot
// (radacct con acctstoptime IS NULL). Zpot consulta este endpoint cada 30s
// y expulsa sus sesiones locales que RADIUS ya cerro (Lost-Carrier, saldo...).

$SECRET = "ZPOTCOA2026a1b2c3";   // cámbialo y ajusta la URL en Zpot

if (!isset($_GET['secret']) || $_GET['secret'] !== $SECRET) {
    http_response_code(403);
    exit('forbidden');
}

$mysqli = @new mysqli("127.0.0.1", "radius", "85River@B", "radius");
if ($mysqli->connect_error) {
    http_response_code(500);
    exit('db error');
}

// Solo sesiones del NAS de Zpot (NAS-IP-Address = 192.168.10.1) activas
$r = $mysqli->query(
    "SELECT username, framedipaddress, acctsessionid
     FROM radacct
     WHERE acctstoptime IS NULL
       AND nasipaddress = '192.168.10.1'
     ORDER BY acctstarttime"
);

$out = array();
if ($r) {
    while ($row = $r->fetch_assoc()) {
        $out[] = array(
            "username"        => $row["username"],
            "framedipaddress" => $row["framedipaddress"],
            "acctsessionid"   => $row["acctsessionid"],
        );
    }
}
$mysqli->close();

header('Content-Type: application/json');
echo json_encode($out);
```

### INSTALACIÓN DEL ENDPOINT

```
ssh root@161.97.67.63
mkdir -p /var/www/html/zpot-coa
# subir sessions.php (scp) y luego:
chown www-data:www-data /var/www/html/zpot-coa/sessions.php
chmod 644 /var/www/html/zpot-coa/sessions.php
# prueba:
curl 'localhost/zpot-coa/sessions.php?secret=ZPOTCOA2026a1b2c3'   # -> []
curl -o /dev/null -w '%{http_code}\n' localhost/zpot-coa/sessions.php  # -> 403
```

### CONFIGURACIÓN EN ZPOT (UI /hotspot/server)

- CoA / Desconexión remota: Activado
- Modo CoA: WireGuard (UDP 3799) o Polling HTTP
- URL Polling (modo poll): http://161.97.67.63/zpot-coa/sessions.php?secret=ZPOTCOA2026a1b2c3
- En modo WireGuard la UI muestra la IP wg destino (ej 10.7.0.5:3799)

## NOTAS

- El listener UDP (modo WireGuard) y el polling (modo poll) son mutuamente
  excluyentes: solo corre el del modo seleccionado.
- cause usado: 2 (Lost-Carrier) en polling/ARP, 6 (Admin-Reset) en
  Disconnect-Request.
- Si el server RADIUS no tiene peer wg para el NAS, usar modo Polling.
