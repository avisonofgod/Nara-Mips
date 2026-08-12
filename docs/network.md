# Network

## Interfaces

| Interface | IP | Purpose | VLAN |
|---|---|---|---|
| eth0 | 192.168.2.102 | WAN1 | — |
| eth1 | 192.168.3.105/24 | WAN2 | — |
| eth2 | 192.168.4.110/24 | Libre | — |
| eth3 | 192.168.10.1/24 | Hotspot network | — |
| eth3.881 | 192.168.20.1/24 | PPPoE VLAN | 881 |
| wg0 | 10.7.0.5/32 | WireGuard management | — |
| ppp0-pppN | 192.168.20.x/32 | PPPoE clients (per-interface) | — |
| ifb_eth3 | — | IFB for upload shaping | — |

## Multi-WAN (MWAN)

### Configuration (`/etc/zpot/mwan.json`)

```json
{
  "wans": [
    { "iface": "eth0", "ip": "192.168.2.102", "gateway": "192.168.2.1", "mark": 1, "table": 10 },
    { "iface": "eth1", "ip": "192.168.3.105", "gateway": "192.168.3.1", "mark": 2, "table": 20 }
  ]
}
```

### nftables Rules

```nft
table inet mwan {
    chain prerouting {
        type filter hook prerouting priority mangle; policy accept;
        iif wg0 ip daddr 10.7.0.0/24 accept
        meta mark set ct mark
        ip daddr { 192.168.10.0/24, 192.168.20.0/24, 10.7.0.0/24 } return
        ct state new meta mark set jhash ip saddr mod 2 map { 0 : 0x00000001, 1 : 0x00000002 }
        ct state new ct mark set meta mark
        ct state established,related meta mark set ct mark
    }

    chain postrouting {
        type nat hook postrouting priority srcnat; policy accept;
        oif eth0 meta mark 0x00000001 masquerade
        oif eth1 meta mark 0x00000002 masquerade
    }
}
```

### Distribution

- **Method**: jhash mod 2 (round-robin by source IP hash)
- **Expected**: ~50/50 split across WANs
- **Fallback**: If one WAN has carrier=0, uses fixed mark on the other

### Cambio de IP de WAN desde el panel (v20260731-mwan)

Al cambiar la IP/gateway de una WAN en `/routing/mwan` (POST `/api/mwan/config`),
el backend aplica el cambio en TODAS las capas (commit `1419f03`):

```
POST /api/mwan/config  {wans:[{iface, ip, gateway, weight, table, mark}]}
  → Fase 1 (fuera del lock — MutexGuard no es Send):
      detect_iface_wan(iface) compara IP/gateway reales vs body
      si difieren → apply_wan_ip_change():
        1. ip -4 addr flush dev <iface>        ← BORRA TODAS las IPv4
        2. ip addr add <new>/<prefix> dev <iface>
        3. ip route replace default via <gw> dev <iface> table <N>  (tabla del mark)
        4. update_interfaces_conf(): reescribe address/gateway en /etc/network/interfaces
  → Fase 2 (lock corto): estado en memoria + apply_nft_rules() + write_state(mwan.json)
```

**PITFALL CRITICO — `ip addr del` de primary borra secondary del mismo prefijo:**
Con 2 IPs del mismo /24 (ej. .105 primary + .106 secondary), `ip addr del .105/24`
elimina TAMBIÉN la .106. Usar SIEMPRE `ip -4 addr flush dev <iface>` + `ip addr add`.

**Idempotente:** si la interfaz ya tiene SOLO la IP deseada, no toca nada.
**Recovery:** si la interfaz está sin IPv4, agrega la IP directo (prefix default /24).

### Archivos involucrados

| Archivo | Rol |
|---|---|
| `src/handlers/mwan.rs` | `apply_wan_ip_change()`, `update_interfaces_conf()`, POST refactorizado |
| `/etc/zpot/mwan.json` | Config MWAN (IP, gateway, mark, table) |
| `/etc/network/interfaces` | IP estática persistente (fuente de verdad al boot) |

## dnsmasq (DHCP only)

Runs on port 5353 (not 53 — unbound handles DNS).

```text
port=5353
dhcp-authoritative
interface=eth3
dhcp-range=192.168.10.2,192.168.10.245,12h
dhcp-option=option:router,192.168.10.1
dhcp-option=option:dns-server,192.168.10.1
dhcp-option=eth3,43,01:04:a1:61:43:3f
```

### DHCP Option 43 — Omada Controller Discovery

Las APs Omada (EAP225, EAP245, etc.) necesitan saber la IP del controlador para
conectarse. Se configura via DHCP Option 43:

```text
dhcp-option=eth3,43,01:04:a1:61:43:3f
```

Donde `01:04:a1:61:43:3f` es:
- `01` = sub-option type 1 (controller IP)
- `04` = longitud 4 bytes
- `a1:61:43:3f` = 161.97.67.63 en hex

Las APs reciben esta opcion al renovar su lease DHCP y conectan automaticamente
al controlador.

## Known Issues

### igc Driver False NO-CARRIER

- **Affects**: eth0 (I226-V)
- **Symptom**: Kernel reports NO-CARRIER but link works (ping responds)
- **Fix**: `sysctl -w net.ipv4.conf.eth0.ignore_routes_with_linkdown=1`
- **Impact**: MWAN jhash sends traffic through eth0, kernel rejects due to
  linkdown → SYN packets lost → user has no internet on that WAN

### MWAN init order bug (FIXED in fa9a048)

`sync_hotspot_wans()` was running BEFORE `init_hotspot_nft()`:
- Postrouting chain didn't exist yet → masquerade rules lost
- Hotspot clients couldn't NAT

Fix: Add masquerade rules unconditionally in `init_hotspot_nft()`.
