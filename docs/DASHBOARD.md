# Dashboard

This is a React 19, Vite, and Tailwind single-page app. The device serves it
itself (uhttpd, port 8080, files in `/data/www`). It talks to the agent on port
9090.

## Layout

There are five navigation groups. On phones they are a bottom tab bar. On desktop
they are a sidebar. The theme is light or dark, set automatically or by hand.
Each group loads on demand.

| Group | Contents |
|---|---|
| **Home** | signal, modem mode, throughput, battery, connection, device, data usage. One batched `/api/dashboard` poll backs it |
| **Signal** | per-carrier LTE/NR detail (PCI, ARFCN, RSRP/RSRQ/SINR), plus network mode, band lock, one-tap cell lock from live cells, and operator selection |
| **Network** | clients by Wi-Fi, USB-C, or Ethernet with link details, per-band Wi-Fi configuration, LAN/DHCP, DNS, and WireGuard |
| **Modem** | APN profiles (with carrier presets), data usage with reset day, TTL override, SMS (inbox and sent, compose, delete), and eSIM profile management |
| **System** | thermals, battery health, charge control, signal and connection loggers, AT console, on-demand process list, device and SIM info, USB mode and powerbank, power actions |

The agent exposes what these screens use. It reports removed features through
`/api/capabilities`, so a client hides them rather than call them and show an
error. See [AGENT.md](AGENT.md) for the route table. See
`scripts/check-api-contract.py`, which fails when the agent and the dashboard
drift apart.

## Source layout

```
web-app/src/
  App.tsx            auth gate + group switching (lazy-loaded)
  app/               shell (sidebar/bottom tabs), login, theme, home poll context
  data/
    client.ts        token handling, envelope unwrapping, timeouts
    api.ts           endpoint bindings + firmware response mappers
    poll.ts          visibility-aware, non-overlapping poller with SWR cache
  ui/                design-system primitives (cards, controls, toast, confirm)
  icons.tsx          inline SVG icon set (no icon dependency)
  features/
    home/            Overview - single batched /api/dashboard poll
    signal/          Overview + Mode & Locking + Operator
    network/         Clients + Wi-Fi + Router + WireGuard
    modem/           APN + Data + TTL + SMS + eSIM
    system/          Metrics + Tools + Settings
```

## Conventions that keep the device happy

- Home is one batched request (`/api/dashboard`) every 3 s, not nine calls.
- Pollers never overlap. The next poll starts after the previous one finishes.
  They pause while the browser tab is hidden.
- Expensive endpoints (`/api/network/clients`, `/api/system/top`) poll slowly,
  every 15 s, or load on demand.
- Last-good data is cached in memory. So a tab switch renders at once, then
  refreshes in the background.

## Develop

```sh
cd web-app
npm install
npm run dev       # local dev server (expects agent at <hostname>:9090)
npm run build     # tsc + vite build -> dist/
npm run lint
```

To deploy to the device, run `./deploy-dashboard.sh` from the repo root.

### Local demo without the device

```sh
cd web-app
bash tools/demo.sh        # dashboard on :8080 + mock agent on :9090
bash tools/demo.sh stop
```

The mock agent (`tools/mock_agent.py`, stdlib only) serves realistic U60 Pro
data. It provides Telstra ENDC with an LTE anchor and n78 NR, live-jittering
throughput, battery, clients, and thermals. So you can review every screen
without hardware. Sign in with any password.
