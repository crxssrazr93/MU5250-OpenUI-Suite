# MU5250-OpenUI-Suite

A custom control plane for the ZTE U60 Pro (MU5250) 5G modem: a Rust agent
on the device exposing a JSON API (`http://192.168.0.1:9090`), a React
dashboard served from the device (`http://192.168.0.1:8080`), an Android
companion app, and tooling to unlock, provision and update all three.

Fork of [dklasens/MU5250-OpenUI](https://github.com/dklasens/MU5250-OpenUI),
which is itself based on
[jesther-ai/open-u60-pro](https://github.com/jesther-ai/open-u60-pro). This tree
tracks both and adds eUICC (eSIM) profile management that exists in neither. See
**[FORK.md](FORK.md)** for lineage and the rule that keeps additions liftable
back upstream.

## Quick start

Locked firmware (HK B04+, CN B28+) — the full sequence:

```sh
python3 scripts/zunlock.py     # 1. unlock → adbd (config backup/restore route)
bash setup.sh                  # 2. build + install the agent (build-from-source)
bash scripts/zharden.sh        # 3. SSH, rc.local cleanup, dashboard :8080, FOTA off
bash deploy-dashboard.sh       # 4. build + push the web UI
```

Full instructions, requirements (backup-key suffix), updates and post-FOTA
recovery: **[docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)**.

## Repository structure

```
agent/          Rust agent (runs on the modem, port 9090)
web-app/        React dashboard (served from the modem, port 8080)
desktop/        Tauri shell around web-app — same frontend, not a copy
mobile/android/ Kotlin/Compose app
mobile/ios/     SwiftUI app: client layer tested, app layer never compiled
scripts/        unlock + hardening + recon tooling
docs/           documentation (below)
setup.sh        first-time provisioning (unlock + agent install)
deploy.sh       agent updates over SSH
deploy-dashboard.sh   dashboard build + push
zte-script-ng.js      community-vetted reference of safe ubus calls
```

## Documentation

| Doc | Contents |
|---|---|
| [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md) | unlock, install, harden, update, post-FOTA recovery |
| [docs/AGENT.md](docs/AGENT.md) | agent architecture, endpoint reference, safety constraints |
| [docs/DASHBOARD.md](docs/DASHBOARD.md) | dashboard pages, source layout, dev + local demo |
| [docs/EUICC.md](docs/EUICC.md) | eSIM support: QMI/QRTR transport, ES10, safety boundary |
| [docs/SAFETY.md](docs/SAFETY.md) | **read first** — brick-prevention rules, daemon sync barrier, recovery commands, safety audit |
| [docs/EMULATOR-TESTING.md](docs/EMULATOR-TESTING.md) | running the Android app against the real agent without a handset |
| [docs/MOBILE-API-GAP.md](docs/MOBILE-API-GAP.md) | which endpoints the apps call, which are served, and which were deliberately declined |
| [docs/FIRMWARE-SURFACE.md](docs/FIRMWARE-SURFACE.md) | inventory of the 132 ubus objects the firmware exposes, marking what the agent uses |
| [desktop/README.md](desktop/README.md) | why the desktop build routes requests through Rust, and the address rules |
| [mobile/ios/README.md](mobile/ios/README.md) | what is tested and what has never been built |
| [docs/reference/](docs/reference/) | device reference material (rpcd ACL dump, USB mode findings) |

## Testing

Four layers, each catching what the one above it cannot see. The gap they
close: a call can succeed, the screen can render, and every field can still be
blank, because the parser reads a key nothing sends. A path level check reports
that as healthy.

```sh
cargo test                                    # agent: 150 tests
cd mobile/android/OpenU60 && ./gradlew test   # parsers: 33 tests, live payloads
python3 scripts/check-mobile-methods.py       # verbs and confirmation headers
python3 scripts/check-mobile-reads.py  --password <pw>   # keys parsers read are served
python3 scripts/check-field-contract.py --password <pw>  # every key, both directions
python3 scripts/walk-app.py            --password <pw>   # opens all 26 screens
```

| Layer | Catches |
|---|---|
| `check-mobile-methods.py` | wrong HTTP verb, missing `X-Confirm` on a destructive route. Reads the route table and the destructive list out of `server.rs`, so it cannot drift from the agent |
| `check-mobile-reads.py` | a GET whose response lacks a key its parser reads. Keys are extracted from the parser source, not a hand written list |
| `check-field-contract.py` | the same in both directions across every route, with each unmatched key carrying a recorded reason. Keys that appear only in certain router states (NR fields, eUICC fields) are marked volatile |
| `walk-app.py` | drives the app on an emulator and fails on error text or a screen that comes up all placeholders |

The three checks that talk to hardware need a reachable device and the agent
password. `walk-app.py` needs an emulator running. Details, including how the
Android app reaches a real agent from an emulator, are in
[docs/EMULATOR-TESTING.md](docs/EMULATOR-TESTING.md).

## Safety in one paragraph

This device was bricked once by going beyond the sanctioned path. The rules
that keep it alive: **shell/ssh/adb only** — no boot hooks outside
`/etc/rc.local`, no system-service modifications, never disable a
`zte_topsw_daemon.conf` daemon via init.d, and stay out of partitions.
Everything else — including what the deploy path does and deliberately does not
touch — is in [docs/SAFETY.md](docs/SAFETY.md).

Exploit and injection tooling written during the unlock work is **not part of
this repository**. It is the class of tool that bricked the original device and
is kept only privately, as a record.

## Source of truth

If this README and the code ever disagree:

- `agent/src/server.rs` — HTTP routing table
- `agent/src/auth.rs` — auth and token behavior
- `web-app/src/App.tsx` — navigation groups mounted in the UI
- `web-app/src/data/api.ts` — client-side API bindings and payload shapes
