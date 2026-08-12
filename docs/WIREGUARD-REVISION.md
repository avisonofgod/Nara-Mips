# REVISIÓN COMPLETA — WireGuard + Panel (2026-08-07, actualizada)

Estado: REVISADO + FIXES APLICADOS. Documento de auditoría por componente.
Modo: solo texto, puntos importantes y palabras clave (sin código).

---

## 1. ESTADO ACTUAL (verificado en vivo 2026-08-07)

- wg0 Alpine: 10.7.0.5/32, puerto 51820, MTU 1420.
- wg0 VPS: 10.7.0.1/24, puerto 51820.
- Peer Alpine en VPS: pub Qa1c (rotada 2 veces el 2026-08-07), AllowedIPs 10.7.0.0/24, keepalive 25.
- Peer VPS en Alpine: pub IIZT, AllowedIPs 10.7.0.0/24, keepalive 25.
- clientepc: 10.7.0.6/32, pub R5sSn, intacto.
- wg1: eliminada (incidente 2026-08-05). NO recrear con Address /24 ni AllowedIPs /0.

PALABRAS CLAVE: wg0 = gestión, NUNCA tocar; AllowedIPs /0 rompe todo; Address SIEMPRE /32.

## 2. INCIDENTES Y LECCIONES (frescos)

### 2.1 Key rotada sin avisar (2026-08-07)
- Síntoma: handshake viejo (12h), transfer sin crecer, ping/SSH muertos, túnel "casi vivo".
- Causa: la private key de wg0 en Alpine cambió (pub 1zGW → Mojz) sin actualizar el peer del VPS → el VPS descartaba las respuestas.
- Diagnóstico: comparar la pub que muestra wg en Alpine vs la que tiene el peer en el VPS.
- Fix aplicado: rotación completa a key NUEVA (pub Qa1c) en ambos lados → cualquier equipo con key vieja queda invalidado.
- Lección: si el túnel no handshakea y nadie tocó config, SOSPECHAR key rotada; verificar pub por ambos lados ANTES de tocar rutas.

### 2.2 NAT del cliente con puertos rotativos (2026-08-07)
- Síntoma: timeouts intermitentes (a veces SSH OK, a veces timeout), handshake fresco pero tráfico TCP que no fluye en ráfagas.
- Causa: el router del cliente rota el puerto de origen del UDP (54719 → 1766 → 49932 → ...); el VPS apunta a un puerto stale hasta que llega un keepalive de Alpine.
- Fix aplicado: PersistentKeepalive 25 en AMBOS lados (Alpine mantiene el mapeo NAT y le enseña al VPS el endpoint actual).
- Lección: con NAT rotativo, NUNCA quitar el keepalive; los timeouts cortos son esperables durante la ventana de rotación.

### 2.3 Incidente wg1 (2026-08-05, documentado)
- Address 10.7.0.12/24 creó ruta 10.7.0.0/24 por wg1 → capturó la VPN de gestión → caída total.
- AllowedIPs 0.0.0.0/0 = full tunnel → rompió salida.
- wg0.conf sobreescrito perdiendo el peer del VPS (allowed ips: none) → handshake OK pero datos descartados.
- Recuperación por consola local. Lección: /32 siempre, /0 nunca, wg0.conf intocable.

## 3. PANEL — BUGS ENCONTRADOS Y CORREGIDOS (commit 9675799, deployado)

- Crear interfaz con private key FALLABA en Alpine (process substitution de bash no existe en busybox). Corregido a stdin pipe. VERIFICADO: crea y elimina correctamente.
- Peer con preshared key FALLABA igual. Corregido a stdin pipe.
- AllowedIPs sin validar: ahora RECHAZA 0.0.0.0/0, ::/0, 10.7.0.0/24, 10.7.0.0/16 (mensaje claro). VERIFICADO.
- Eliminar wg0: ahora PROTEGIDO (rechaza). VERIFICADO.
- Nombre de interfaz en la ruta de peers sin validar → ahora validado (anti inyección).
- La API listaba la private key → ahora vacía (no se expone). VERIFICADO.
- Address sin validar → ahora exige /32 (IPv4) o /128 (IPv6); /24 rechazado con mensaje. VERIFICADO.
- Defaults peligrosos de la UI corregidos (Address 10.0.0.1/24 → 10.7.0.15/32; AllowedIPs 10.7.0.0/24 → 10.7.0.15/32).

PENDIENTE (no crítico): chmod 600 automático de los .conf generados; no perder campos custom al regenerar; mostrar errores del backend en la UI (hoy solo consola).

## 4. PUNTOS IMPORTANTES A REVISAR (checklist por componente)

### 4.1 Interfaz wg0 (gestión)
- Address /32, puerto libre, MTU correcto, peer del VPS presente SIEMPRE.
- NUNCA editar por el panel; proteger contra delete.
- Verificar: pub coincide con el peer del VPS.

### 4.2 Interfaces nuevas (wg1, wg2...)
- Address SIEMPRE /32 o /128. Puerto distinto por interfaz (51821, 51822...).
- Private key vacía = que genere. NUNCA reusar la de wg0.
- Persistencia: conf + init.d + rc-update (sobrevive reboot).
- Verificar tras crear: handshake con el peer, ruta /32 presente.

### 4.3 Peers
- AllowedIPs: IP del peer /32 (o la subred si el peer es un server). NUNCA /0 ni 10.7.0.0/24.
- Preshared key: usar la misma en ambos lados (server y peer).
- Keepalive 25 cuando el peer está detrás de NAT (siempre en este escenario).
- Verificar: handshake < 1 min, RX/TX creciendo.

### 4.4 Diagnóstico de túnel caído (orden)
1. Handshake viejo → key rotada o peer mal → comparar pubs ambos lados.
2. Handshake fresco pero TCP no fluye → NAT rotativo (esperar/keepalive) o firewall.
3. Transfer estancado → nadie habla; revisar endpoint y NAT.
4. Ping ICMP puede fallar aunque SSH/HTTP pasen (normal en Alpine).

### 4.5 Seguridad
- API admin sin auth (P0 transversal, pendiente global).
- Claves: wg0.conf 600; confs generados revisar permisos.
- No exponer private keys por API (corregido).

## 5. ESCENARIOS (2026-08-07)

A. Túnel caído 12h sin tocar nada → key rotada. Fix: rotar y alinear ambos lados.
B. Conexión intermitente con handshake fresco → NAT rotativo. Fix: keepalive 25 ambos lados.
C. Crear interfaz con private key desde panel → YA FUNCIONA (stdin pipe). Verificar handshake después.
D. Crear interfaz con Address /24 → RECHAZADO por el panel. NUNCA forzar por consola.
E. Peer con AllowedIPs /0 → RECHAZADO por el panel. NUNCA forzar por consola.
F. Intentar borrar wg0 desde el panel → RECHAZADO. Por consola tampoco (gestión).
G. Dos interfaces con la misma IP /32 → la segunda falla al agregar (IP duplicada).
H. Reinicio de Alpine → wg0 se recupera solo (conf + init.d). Verificar handshake y rutas.
I. Server remoto en 10.7.0.x con IP conflictiva → usar IP distinta (ej. 10.7.0.10/32) en el server, AllowedIPs /32 del server en Alpine.
J. Cliente detrás de NAT → keepalive 25 obligatorio para que el server aprenda el endpoint.

## 6. VERIFICACIÓN TRAS CAMBIOS (lista de control)

- Handshake reciente (menos de 1 minuto).
- Ping/SSH/HTTP al panel desde el VPS (los 3).
- Transfer creciendo (keepalives + datos reales).
- Rutas: /32 de la interfaz, /24 de gestión por wg0.
- Public key del peer coincide en AMBOS lados.
- Reiniciar y confirmar recuperación automática.

## 7. ESTADO DEL COMPONENTE EN LA AUDITORÍA GENERAL

- WireGuard PANEL: ✅ CORREGIDO (9675799) — pendientes menores no críticos.
- WireGuard RED (wg0): ✅ OPERATIVO — lecciones documentadas (key rotada, NAT rotativo).
- Siguiente componente a auditar/documentar: ver AUDITORIA-COMPLETA.md (Interfaces, IP, PPP, RADIUS, Firewall, MWAN, Command, DNS...).
