# Zpot-RS Documentation

> Gestor ISP written in Rust (axum + tokio) for Alpine Linux.

## Contents

| Document | Description |
|---|---|
| [Architecture](architecture.md) | System overview, ports, processes, data flow |
| [Backend](backend.md) | Rust modules, handlers, RADIUS integration, config files |
| [Frontend](frontend.md) | SPA structure, dock navigation, pages, components |
| [Hotspot](hotspot.md) | Complete hotspot flow, session lifecycle, scenarios |
| [PPPoE](pppoe.md) | PPPoE server, RADIUS auth, pool, watchdog zombie, flujo conexión |
| [RADIUS](radius.md) | Migración PPP a FreeRADIUS, atributos, VSAs, anti-zombies, pitfalls |
| [Network](network.md) | nftables rules, MWAN, QoS tc HTB, interfaces |
| Config examples | [hotspot-server.json](config-examples/hotspot-server.json), [hotspot-profiles.json](config-examples/hotspot-profiles.json), [mwan.json](config-examples/mwan.json) |

## Quick Reference

- **Admin UI**: `http://10.7.0.5:8081/`
- **Hotspot portal**: `http://10.7.0.5:80/`
- **RADIUS server**: `161.97.67.63:1812` (auth), `:1813` (acct)
- **Secret**: `85River@B`
- **Hotspot gateway**: `192.168.10.1` (eth3)
- **PPPoE gateway**: `192.168.20.1` (eth3.881)
- **Management**: `10.7.0.0/24` (WireGuard wg0)

## Architecture Overview

```
┌─────────────────────────────────────────────────────┐
│                  Alpine Linux (10.7.0.5)             │
│                                                     │
│  ┌──────────┐  ┌──────────┐  ┌──────────────────┐  │
│  │  zpot-rs  │  │  pppoe-  │  │  nftables / tc   │  │
│  │  (Rust)   │  │  server  │  │  (kernel)        │  │
│  │  :80      │  │  eth3.881│  │                  │  │
│  │  :8081    │  │          │  │  hotspot table    │  │
│  └────┬─────┘  └────┬─────┘  └──────────────────┘  │
│       │             │                               │
│       │   RADIUS    │                               │
│       └─────┬───────┘                               │
│             │                                       │
│   161.97.67.63:1812/1813                            │
└─────────────────────────────────────────────────────┘
```

## Runtime Config Files (`/etc/zpot/`)

| File | Purpose |
|---|---|
| `hotspot-server.json` | Hotspot interface, RADIUS credentials, pool |
| `hotspot-profiles.json` | QoS profiles (idle timeout, rate limit) |
| `mwan.json` | Multi-WAN interfaces and routing tables |
| `pools.json` | IP pools for DHCP |
| `walled-garden.json` | Captive portal bypass domains |
| `adblock.json` | Ad-blocking domains via nftables |
