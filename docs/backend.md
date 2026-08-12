# Backend

## Rust Modules

### `src/main.rs`

Entry point. Initializes:
- Axum router with all handlers
- nftables hotspot rules (`init_hotspot_nft()`)
- MWAN rules (`apply_nft_rules()`)
- MWAN watchdog (checks carrier every 10s)
- PPP zombie watchdog (every 120s)
- Serves static files via `ServeDir` at `/zpot/`

### `src/handlers/hotspot.rs`

Main hotspot logic (~1700 lines). Contains:

| Component | Lines | Description |
|---|---|---|
| `HotspotSession` struct | 35-48 | In-memory session data |
| `HotspotServer` struct | 193-211 | Server config (iface, RADIUS, pool) |
| `HsProfile` struct | 214-222 | Perfil (idle_timeout, rate_limit) |
| `CookieEntry` struct | 98-105 | Server-side cookie for auto-login |
| `RadiusResult` struct | 1148-1157 | Parsed RADIUS response |
| Session store | 28-32 | `Mutex<Option<HashMap<String, HotspotSession>>>` |
| Cookie store | 111 | `Mutex<Vec<CookieEntry>>` |
| `portal_root()` | 409-537 | GET / — redirect/main page |
| `portal_login()` | 538-557 | GET /login — login form |
| `portal_auth()` | 560-657 | POST /auth — authenticate user |
| `portal_logout()` | 861-942 | GET /logout — end session |
| `portal_disconnect()` | 1009-1078 | POST /disconnect — admin disconnect |
| `active_sessions()` | 331-377 | GET /active — list sessions |
| `spawn_interim_task()` | 687-767 | Background loop (60s): acct + re-auth + idle |
| `session_disconnect_internal()` | 944-1015 | Cleanup: nft, tc, acct stop, conntrack |
| `radius_auth()` | 1174-1215 | Raw UDP RADIUS Access-Request |
| `parse_radius_attrs()` | 1218-1338 | Parse RADIUS response attributes |
| `add_bypass_nft()` | 1638-1675 | Add IP+MAC to hotspot_auth set |
| `get_mac_from_arp()` | 1632-1649 | Get MAC from ARP table |

## RADIUS Integration

### Auth Flow

```rust
radius_auth(server, secret, username, password)
  -> RadiusResult {
       success: bool,
       rejected: bool,     // true = Access-Reject (code 3)
       speed_up: String,   // rate UP (tokens[0] primer valor)
       speed_down: String, // rate DOWN (tokens[0] segundo valor)
       up_ceil_str: String,   // ceil UP (tokens[1] primer valor)
       down_ceil_str: String, // ceil DOWN (tokens[1] segundo valor)
       idle_timeout: u32,  // attr 28
       reply_message: String,
     }
```

### Timeout vs Reject

| Scenario | `success` | `rejected` | NAS action |
|---|---|---|---|
| Access-Accept (code 2) | true | false | Continue session |
| Access-Reject (code 3) | false | true | Disconnect (terminate_cause=5) |
| Network timeout / error | false | false | No action (try again in 60s) |

## Configuration Files

### `/etc/zpot/hotspot-server.json`

```json
{
  "iface": "eth3",
  "name": "Hotspot",
  "gw": "192.168.10.1",
  "html_dir": "/root/zpot-rs/static/hotspot",
  "pool": "default",
  "pool_range": "192.168.10.10-192.168.10.200",
  "dns_server": "192.168.10.1",
  "domain": "wifi1.info",
  "login_by": "http-pap,mac-cookie",
  "use_radius": true,
  "radius": "161.97.67.63:1812",
  "radius_secret": "85River@B",
  "profile": "default"
}
```

### `/etc/zpot/hotspot-profiles.json`

```json
[
  {
    "name": "default",
    "login_by": "http-pap,mac-cookie",
    "idle_timeout": 600,
    "shared_users": 1,
    "rate_limit": "1M/2M 2M/3M",
    "cookie_timeout": "7d"
  }
]
```

### `/etc/zpot/mwan.json`

```json
{
  "wans": [
    { "iface": "eth0", "ip": "192.168.2.102", "gateway": "192.168.2.1", "mark": 1, "table": 10 },
    { "iface": "eth1", "ip": "192.168.3.105", "gateway": "192.168.3.1", "mark": 2, "table": 20 }
  ]
}
```

### POST `/api/mwan/config` — cambio de IP/gateway aplica en TODO

Desde v20260731-mwan (commit `1419f03`), al cambiar `ip`/`gateway` de una WAN
el backend ejecuta `apply_wan_ip_change()`:

1. `ip -4 addr flush dev <iface>` + `ip addr add <new>/24` — aplicar en vivo
2. `ip route replace default via <gw> dev <iface> table <N>` — ruta del mark
3. Reescribe `address`/`gateway` en `/etc/network/interfaces` — persistente
4. Actualiza mwan.json + reglas nft + ip rules

**PITFALL:** `ip addr del` de una primary borra la secondary del mismo prefijo —
usar siempre `flush -4` + `add`. **MutexGuard no es Send:** los awaits de
`apply_wan_ip_change()` corren FUERA del lock (`std::sync::Mutex`), o axum
rechaza el handler (`trait Handler` no implementado).

## Idle-Timeout Resolution

```rust
let profile_idle = get_active_profile().map(|p| p.idle_timeout).unwrap_or(600);
let idle_timeout = if rad_result.idle_timeout > 0 {
    rad_result.idle_timeout   // RADIUS wins
} else {
    profile_idle              // Profile fallback (600s)
};
```
