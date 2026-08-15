# Agent: `zte-agent`

This is a Rust HTTP backend that runs on the modem (`agent/`). It talks to ubus,
the AT ports, sysfs and procfs, and device services. `agent/src/server.rs` is the
canonical routing table. This document summarizes it.

- It binds `192.168.0.1:9090`. Override this with `ZTE_AGENT_BIND` and
  `ZTE_AGENT_THREADS`.
- Auth is `POST /api/auth/login`. The password comes from `ZTE_AGENT_PASSWORD`,
  or from an optional 6-digit mobile PIN. It issues bearer tokens with a sliding
  1 h expiry, so a dashboard left open stays signed in. Login is rate-limited.
  CORS is LAN-only.
- The JSON envelope is `{ "ok": true, "data": … }` or
  `{ "ok": false, "error": … }`.
- Destructive actions need the `X-Confirm: true` header. Examples are
  `/api/device/reboot` and `/api/device/shutdown`.

## Endpoint reference

`server.rs` is the source of truth. It has about 137 distinct paths and about 160
method-and-path pairs. `scripts/check-api-contract.py` keeps the agent, the
dashboard, and the mock agent in agreement. The table below summarizes the
families.

| Family | What it covers |
|---|---|
| Auth | `POST /api/auth/login`. Bearer token, sliding 1 h expiry |
| Batch | `GET /api/dashboard`. Device, battery, cpu, memory, speed, data usage, signal, wan, wan6, and thermal in one request. This is the app's heartbeat. Home, Signal, and Modem/Data all read it instead of polling their own endpoints |
| Status | `GET /api/device`, `/api/cpu`, `/api/memory`, `/api/system/top` |
| Network | `GET /api/network/clients`, `/api/network/signal`, `/api/network/wan`, `/api/network/lan`, and related reads |
| Device | battery, thermal, charger reads, plus `POST /api/device/reboot` and `/api/device/shutdown` |
| System | `POST /api/system/restart-agent`, `/api/system/kill-bloat` |
| Wi-Fi | `GET /api/wifi/status`, `PUT /api/wifi/settings` |
| Modem | network mode, data, APN, cell and band lock, neighbours, and airplane mode |
| Operator | `GET /api/operator/scan`, `POST /api/operator/scan/start`, `POST /api/operator/select` |
| Router | DNS, LAN, and APN profile management |
| DoH | `GET /api/doh/status`, `/api/doh/cache`, and the proxy controls |
| Firewall | config, port forwarding, and the telemetry domain filter |
| SMS | `POST /api/sms/list`, `/api/sms/send`, `/api/sms/delete`, `/api/sms/read`. Delete falls back to direct SQLite for SIM-stored rows the firmware refuses |
| SIM | `GET /api/sim/info`, `/api/sim/imei`, plus the PIN and PUK flows |
| eUICC (eSIM) | status, EID, profiles, chip info, download, enable, disable, delete, nickname, notifications, and the relay. See [EUICC.md](EUICC.md) |
| WireGuard | `GET+POST /api/tunnel/wireguard`, connect, disconnect, keygen, and the profile library |
| USB | `GET /api/usb/status`, `PUT /api/usb/mode`, `/api/usb/default`, `/api/usb/powerbank` |
| Power | `GET+PUT /api/device/charge-control`. Manual stop and resume, plus a limit enforcer with hysteresis, driven by `BSP_CHARGER_EVENT` |
| Scheduler | `GET /api/scheduler/jobs`, plus job add and delete |
| Extras | TTL override (`/api/ttl/*`), AT console (`/api/at/*`), and the signal and connection CSV loggers (`/api/logger/*`) |

## Architecture notes

- **Transport.** A `tiny_http` thread pool, with no async runtime, keeps the
  binary and the footprint small. The listener is supervised. tiny_http's accept
  thread exits for good on its first `accept()` error, so `server::start` watches
  for that, drains the workers, and rebuilds. It does not sit alive serving
  nothing.
- **Dependencies.** `serde`, `serde_json`, `tiny_http`, `sha2`, and `libc`. There
  is no TLS stack and no HTTP client in the agent. Features that need outbound
  HTTPS, such as the DoH proxy, drive an external binary or the device's own
  `curl` instead.
- **Subprocess cost.** Every `ubus` or `uci` read is a fork and exec, which
  dominates the agent's CPU. `cache.rs` gives each dashboard source its own TTL
  (signal 2.5 s, thermal 10 s, wan and wan6 30 s, data usage 30 s, cycle dates
  300 s). So the client's poll rate is decoupled from the refresh rate, and
  concurrent clients collapse onto one refresh. `wifi_status` dumps whole configs
  with `ubus::uci_show` instead of one `uci get` per key.
- **Event bus.** One `ubus listen` process dispatches to subscribers over bounded
  channels. `BSP_CHARGER_EVENT` reaches the charge enforcer this way.
- **State files.** All under `/data/local/tmp/`: `charge_limit.json`,
  `usb_config.json`, and the signal and connection CSV logs.
- **Boot behavior.** `main.rs` runs a one-shot migration. It undoes an old
  in-process DoH proxy's dnsmasq rewiring. Without this, a device that had that
  DoH enabled would come back up forwarding DNS to a dead port. It also applies
  `start_ttl.sh` when present, and re-applies persisted NCM only when explicitly
  enabled. See [SAFETY.md](SAFETY.md) §2 for why the last two are acceptable.
- **Logging.** stdout and stderr go to syslog through `logger -t zte-agent`. Read
  it with `logread -e zte-agent`. It is not a file on tmpfs.

## Safety constraints built into the agent

- **The AT console is allowlisted** (`server.rs`). It permits read-only commands
  only. It blocks `AT+CFUN`, `AT^…`, `AT+CMGD`, `AT$QCRMCALL`, `AT+CLCK`,
  `AT+CGDCONT=`, and `AT+CGACT=`.
- **kill-bloat kills only daemons that are safe to kill.** It never touches the
  `zte_topsw_daemon.conf` sync-barrier set. See SAFETY.md.
- **Destructive endpoints need `X-Confirm: true`.** These are `/api/device/reboot`
  and `/api/device/shutdown`, among others.
- **Login is rate-limited.** 5 failures per client IP arm a 30 s lockout.
- **LAN-only bind and LAN-origin CORS** by default.
- **ubus inputs from HTTP are validated** for size and depth (`validate.rs`)
  before the agent forwards them.

## USB modes

See [reference/usb-modes.md](reference/usb-modes.md) for the live-device
findings. The stock switch exposes only ECM and RNDIS. NCM exists in configfs and
the agent manages it. That is experimental and gated behind
`confirm_experimental`. The ubus `mode` field is not a reliable detector of the
active composition.

## Building

```sh
cargo build --release --target aarch64-unknown-linux-musl -p zte-agent
```

The cross-linker config is in `.cargo/config.toml` (`aarch64-linux-musl-gcc`).
`cargo test` runs the unit tests. These cover auth lockout and token expiry,
dashboard payload shapes, the TTL cache, UCI value unquoting, USB boot guards,
the WiFi sanitizers, the SMS send shape, band and cell lock translation, and the
eUICC parsers.

`python3 scripts/check-api-contract.py` asserts that the agent route table, the
dashboard's calls, and the mock agent's fixtures all agree. Run it after you
touch any of the three.
