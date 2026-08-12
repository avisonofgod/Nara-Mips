# Frontend

## SPA Architecture

The admin interface is a single-page application at `http://10.7.0.5:8081/`.

### Entry Point

`static/index.html` — loads `static/app-v4.js` which contains all SPA logic.

### Navigation

Two-level navigation:

1. **Dock** (bottom icons): 11 docks accessible via `sw(menu_id)`
2. **Subnav** (top tabs): Sub-pages within each dock via `lp(path)`

### Docks

| Dock | ID | Pages |
|---|---|---|
| Dashboard | `nav-dashboard` | Status overview, charts |
| Interfaces | `nav-interfaces` | List, Bridges, VLANs |
| IP | `nav-ip` | Addresses, Routes, ARP, DHCP Leases, DNS |
| WireGuard | `nav-wireguard` | Interfaces, Peers |
| PPP | `nav-ppp` | Profiles, Secrets, Active sessions, PPPoE config |
| Hotspot | `nav-hotspot` | Server config, Profiles, Active, Cookies, Bindings, Walled Garden, AdBlock |
| RADIUS | `nav-radius` | Server status |
| Firewall | `nav-firewall` | Rules, NAT |
| Bridge | `nav-bridge` | Bridge config |
| Routing | `nav-routing` | Routing tables, rules |
| System | `nav-system` | Logs, Services, Reboot |

### Navigation Functions

```javascript
sw('nav-hotspot')      // Switch to Hotspot dock (shows subnav)
lp('/hotspot/active')  // Load Active Sessions page
```

### Cache Busting

All JS/CSS includes `?t=TIMESTAMP` to force browser refresh. Meta tags in `index.html`:
```html
<meta http-equiv="Cache-Control" content="no-cache, no-store, must-revalidate">
<meta http-equiv="Pragma" content="no-cache">
```

### Hotspot Server Profiles Page

Located at `static/pages/hotspot-server-profiles.html`.

Form fields:
| Field | ID | Type | Default |
|---|---|---|---|
| Name | `hsp-name` | text | — |
| Login By | `hsp-lb-*` | checkboxes | radius, mac-cookie, http-pap |
| Idle Timeout | `hsp-it` | number | 180 |
| Shared Users | `hsp-su` | number | 1 |
| Rate Limit | `hsp-rl` | text | — |
| Cookie Timeout | `hsp-ct` | text | 3d |

### Portal Templates (`static/hotspot/`)

| File | Purpose |
|---|---|
| `login.html` | Login page with verse box + username/password form |
| `alogin.html` | "Already authenticated" page |
| `status.html` | Session status with counters |
| `logout.html` | Logout confirmation |
| `redirect.html` | Captive portal redirect |
| `rlogin.html` | RADIUS login |
| `js/versiculo-texto.js` | Verse display logic |
| `js/versiculos.json` | Verses database |
