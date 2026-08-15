# Fork notes

This tree is a personal fork that combines two upstreams and adds work that
exists in neither. It is kept structured so changes can be handed back to
either project.

## Lineage

| Remote | Upstream | Role here |
| --- | --- | --- |
| `upstream-openui` | [dklasens/MU5250-OpenUI](https://github.com/dklasens/MU5250-OpenUI) | Base of this tree: agent, dashboard, unlock and hardening tooling |
| `upstream-u60pro` | [jesther-ai/open-u60-pro](https://github.com/jesther-ai/open-u60-pro) | Android/iOS companion app, plus feature implementations to port selectively |

`dklasens/MU5250-OpenUI` is the base because it is the safety-reviewed line: it
supports the HK B04 backup/restore unlock, binds the agent to the LAN address
rather than every interface, adds CORS restrictions, body limits, login rate
limiting and confirmation gates on destructive operations, and its August 2025
audit removed the regressions behind a real brick.

`jesther-ai/open-u60-pro` is the source of the companion app and of several
features worth having. Its older agent must not be deployed unchanged — see
"Rejected upstream behaviour" below.

Fetch both:

```sh
git fetch upstream-openui
git fetch upstream-u60pro
```

## Design rule: keep additions liftable

Anything added here that could be useful upstream is written so it can be moved
without dragging this fork's conventions along:

- **Depend downward only.** The eSIM stack (`agent/src/qmi/`, `agent/src/euicc/`)
  uses only `std` and `libc`. No project types, no `AppState`, no handler
  conventions leak into it.
- **Isolate the glue.** Each subsystem's HTTP handlers live in one small file
  (`agent/src/euicc/api.rs`), which is the only part another agent would rewrite.
- **Document the wire, not just the code.** Protocol constants and framing are
  written down in `docs/` so a reimplementation does not need to re-derive them.
- **Test without hardware.** Parsers are tested against synthetic, redacted
  fixtures so the suite runs anywhere.

## Rejected upstream behaviour

Deliberately not carried over from `open-u60-pro`, and not to be reintroduced
without a safety review:

- killing ZTE daemons that participate in the boot sync barrier;
- an unrestricted AT terminal (this fork allows a read-only subset);
- binding the agent to `0.0.0.0:9090` without equivalent hardening;
- DoH dnsmasq rewiring that can leave DNS pointing at a dead local port;
- background scheduler and SMS-forwarding services with no maintained UI;
- Tailscale binary download and daemon supervision.

`/api/capabilities` reports these as explicitly unsupported so a client built
against the upstream API can hide them rather than calling them and showing the
user an error.

## Client compatibility

The upstream Android source statically references 94 API paths; only 19 exist
unchanged in this agent. A successful APK build does **not** mean the app works
against this agent. The companion app has to be adapted to this API surface,
using `/api/capabilities` for discovery rather than assuming a fixed set.

Sources of truth for the current surface:

```text
agent/src/server.rs          route table
docs/AGENT.md                endpoint reference
web-app/src/data/api.ts      dashboard bindings
```

`scripts/check-api-contract.py` fails the build if the agent, the dashboard and
the mock agent disagree about which routes exist.

## Added here

- **eUICC / eSIM, read-only** — a dependency-free QMI-over-QRTR transport and
  GSMA ES10 client. Exists in neither upstream. See
  [docs/EUICC.md](docs/EUICC.md).
- **`/api/capabilities`** — capability discovery, so removed features are hidden
  rather than surfaced as 404s.
