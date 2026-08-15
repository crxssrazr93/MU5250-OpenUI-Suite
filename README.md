# MU5250-OpenUI-Suite

A control plane for the ZTE U60 Pro (MU5250) 5G modem. It has four parts:

- a Rust agent on the device, which serves a JSON API at `http://192.168.0.1:9090`,
- a React dashboard, which the device serves at `http://192.168.0.1:8080`,
- an Android app,
- tools to unlock, provision, and update all three.

This is a fork of [dklasens/MU5250-OpenUI](https://github.com/dklasens/MU5250-OpenUI).
That project is itself based on
[jesther-ai/open-u60-pro](https://github.com/jesther-ai/open-u60-pro). This tree
tracks both. It adds eUICC (eSIM) profile management, which neither has. See
**[FORK.md](FORK.md)** for the lineage and the rule that keeps additions easy to
lift back upstream.

## Quick start

The full sequence for locked firmware (HK B04+, CN B28+):

```sh
python3 scripts/zunlock.py     # 1. unlock -> adbd (config backup/restore route)
bash setup.sh                  # 2. build + install the agent (build-from-source)
bash scripts/zharden.sh        # 3. SSH, rc.local cleanup, dashboard :8080, FOTA off
bash deploy-dashboard.sh       # 4. build + push the web UI
```

For full instructions, requirements (the backup-key suffix), updates, and
post-FOTA recovery, see **[docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)**.

To use the features once installed, see **[docs/USAGE.md](docs/USAGE.md)**.

## Repository structure

```
agent/          Rust agent (runs on the modem, port 9090)
web-app/        React dashboard (served from the modem, port 8080)
desktop/        Tauri shell around web-app (same frontend, not a copy)
mobile/android/ Kotlin/Compose app
mobile/ios/     SwiftUI app (client layer tested, app layer never compiled)
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
| [docs/USAGE.md](docs/USAGE.md) | step-by-step guide to every feature, in all three front ends |
| [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md) | unlock, install, harden, update, post-FOTA recovery |
| [docs/AGENT.md](docs/AGENT.md) | agent architecture, endpoint reference, safety constraints |
| [docs/DASHBOARD.md](docs/DASHBOARD.md) | dashboard pages, source layout, dev + local demo |
| [docs/EUICC.md](docs/EUICC.md) | eSIM support: QMI/QRTR transport, ES10, safety boundary |
| [docs/SAFETY.md](docs/SAFETY.md) | **read first.** brick-prevention rules, daemon sync barrier, recovery commands, safety audit |
| [docs/EMULATOR-TESTING.md](docs/EMULATOR-TESTING.md) | run the Android app against the real agent without a handset |
| [docs/MOBILE-API-GAP.md](docs/MOBILE-API-GAP.md) | which endpoints the apps call, which are served, and which were declined |
| [docs/FIRMWARE-SURFACE.md](docs/FIRMWARE-SURFACE.md) | inventory of the 132 ubus objects the firmware exposes, marking what the agent uses |
| [desktop/README.md](desktop/README.md) | why the desktop build routes requests through Rust, and the address rules |
| [mobile/ios/README.md](mobile/ios/README.md) | what is tested and what has never been built |
| [docs/reference/](docs/reference/) | device reference material (rpcd ACL dump, USB mode findings) |

## Testing

There are four test layers. Each one catches what the layer above it cannot see.
They close one gap: a call can succeed, the screen can render, and every field
can still be blank, because the parser reads a key that nothing sends. A
path-level check reports that state as healthy.

```sh
cargo test                                    # agent: 150 tests
cd mobile/android/OpenU60 && ./gradlew test   # parsers: unit tests on live payloads
python3 scripts/check-mobile-methods.py       # verbs and confirmation headers
python3 scripts/check-mobile-reads.py  --password <pw>   # keys parsers read are served
python3 scripts/check-field-contract.py --password <pw>  # every key, both directions
python3 scripts/walk-app.py            --password <pw>   # opens every screen
```

| Layer | Catches |
|---|---|
| `check-mobile-methods.py` | a wrong HTTP verb, or a missing `X-Confirm` on a destructive route. It reads the route table and the destructive list out of `server.rs`, so it cannot drift from the agent |
| `check-mobile-reads.py` | a GET whose response lacks a key that its parser reads. It extracts the keys from the parser source, not from a hand-written list |
| `check-field-contract.py` | the same check in both directions across every route. Each unmatched key carries a recorded reason. Keys that appear only in certain router states (NR fields, eUICC fields) are marked volatile |
| `walk-app.py` | drives the app on an emulator. It fails on error text, or on a screen that comes up all placeholders |

The three checks that talk to hardware need a reachable device and the agent
password. `walk-app.py` needs a running emulator. For details, and for how the
Android app reaches a real agent from an emulator, see
[docs/EMULATOR-TESTING.md](docs/EMULATOR-TESTING.md).

## Safety in one paragraph

A step beyond the sanctioned path bricked this device once. These rules keep it
alive. Use **shell, ssh, and adb only**. Add no boot hooks outside
`/etc/rc.local`. Modify no system services. Never disable a
`zte_topsw_daemon.conf` daemon through init.d. Stay out of the partitions. For
everything else, including what the deploy path does and does not touch, see
[docs/SAFETY.md](docs/SAFETY.md).

Exploit and injection tools were written during the unlock work. They are **not
part of this repository**. They are the class of tool that bricked the original
device. They are kept only privately, as a record.

## Source of truth

If this README and the code ever disagree, trust the code:

- `agent/src/server.rs` for the HTTP routing table,
- `agent/src/auth.rs` for auth and token behavior,
- `web-app/src/App.tsx` for the navigation groups mounted in the UI,
- `web-app/src/data/api.ts` for the client-side API bindings and payload shapes.
