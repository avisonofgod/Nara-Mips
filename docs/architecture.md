# Architecture

## Processes

| Process | PID file | Ports | Purpose |
|---|---|---|---|
| `zpot` | none | 80, 8081 | Main daemon: hotspot portal + admin SPA + API |
| `pppoe-server` | none | eth3.881 (L2) | PPPoE discovery and session management |
| `pppd` (xN) | none | pppN | One per active PPPoE client |

## Ports

| Port | Service | Protocol | Description |
|---|---|---|---|
| 80 | Hotspot portal | HTTP | Login, status, redirect for captive portal |
| 8081 | Admin SPA + API | HTTP | Management UI and REST API |
| 1812 | RADIUS auth | UDP | Authentication requests to RADIUS server |
| 1813 | RADIUS acct | UDP | Accounting (start, interim, stop) |
| 51820 | WireGuard | UDP | Management VPN (10.7.0.0/24) |

## Data Flow

```
User Browser                    Alpine                        RADIUS Server
    │                             │                              │
    │── HTTP GET (port 80) ──────►│                              │
    │                             │── Access-Request ───────────►│
    │                             │◄── Access-Accept/Reject ─────│
    │◄── login page / alogin ─────│                              │
    │                             │                              │
    │── HTTPS / HTTP ────────────►│                              │
    │                             │  nftables bypass check       │
    │                             │  (ip saddr . ether saddr)    │
    │◄── internet ────────────────│                              │
    │                             │                              │
    │                             │  Each 60s:                   │
    │                             │── Accounting-Request ───────►│
    │                             │── Access-Request (re-auth) ─►│
    │                             │◄── Access-Accept / Reject ───│
```

## Session Store

In-memory `HashMap<String, HotspotSession>` keyed by client IP:

```rust
HashMap {
    "192.168.10.100" => HotspotSession {
        username: "MAX",
        password: "xxx",       // stored in cookie entry
        client_ip: "192.168.10.100",
        client_mac: "aa:bb:cc:dd:ee:ff",
        session_id: "zpot-MAX-1234567890",
        start: 1785390000,
        speed_up: "1M",
        speed_down: "4M",
        rx_bytes: 512712,
        tx_bytes: 17150251,
        idle_timeout: 3600,
        last_active: 1785390060,
    },
    ...
}
```
