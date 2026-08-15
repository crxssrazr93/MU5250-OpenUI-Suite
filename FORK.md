# Fork notes

This tree is a personal fork. It combines two upstreams and adds work that
neither has. It stays structured, so you can hand changes back to either
project.

## Lineage

| Remote | Upstream | Role here |
| --- | --- | --- |
| `upstream-openui` | [dklasens/MU5250-OpenUI](https://github.com/dklasens/MU5250-OpenUI) | Base of this tree: agent, dashboard, unlock and hardening tooling |
| `upstream-u60pro` | [jesther-ai/open-u60-pro](https://github.com/jesther-ai/open-u60-pro) | Android/iOS companion app, plus feature implementations to port selectively |

`dklasens/MU5250-OpenUI` is the base, because it is the safety-reviewed line. It
supports the HK B04 backup/restore unlock. It binds the agent to the LAN address
rather than every interface. It adds CORS restrictions, body limits, login rate
limiting, and confirmation gates on destructive operations. Its August 2025
audit removed the regressions behind a real brick.

`jesther-ai/open-u60-pro` is the source of the companion app and of several
useful features. Do not deploy its older agent unchanged. See "Rejected upstream
behaviour" below.

Fetch both:

```sh
git fetch upstream-openui
git fetch upstream-u60pro
```

## Design rule: keep additions liftable

Write anything added here that could help upstream so you can move it without the
fork's own conventions:

- **Depend downward only.** The eSIM stack (`agent/src/qmi/`, `agent/src/euicc/`)
  uses only `std` and `libc`. No project types, no `AppState`, and no handler
  conventions enter it.
- **Isolate the glue.** Each subsystem keeps its HTTP handlers in one small file
  (`agent/src/euicc/api.rs`). That file is the only part another agent rewrites.
- **Document the wire, not just the code.** The protocol constants and framing
  are written down in `docs/`, so a reimplementation does not re-derive them.
- **Test without hardware.** The parsers are tested against synthetic, redacted
  fixtures, so the suite runs anywhere.

## Rejected upstream behaviour

These behaviours are deliberately not carried over from `open-u60-pro`. Do not
reintroduce them without a safety review:

- killing ZTE daemons that take part in the boot sync barrier,
- an unrestricted AT terminal (this fork allows a read-only subset),
- binding the agent to `0.0.0.0:9090` without equivalent hardening,
- DoH dnsmasq rewiring that can leave DNS pointed at a dead local port,
- background scheduler and SMS-forwarding services with no maintained UI,
- Tailscale binary download and daemon supervision.

`/api/capabilities` reports these as unsupported. A client built against the
upstream API can then hide them, rather than call them and show the user an
error.

## Client compatibility

The upstream Android source references 94 API paths. Only 19 exist unchanged in
this agent. A successful APK build does **not** mean the app works against this
agent. Adapt the companion app to this API surface. Use `/api/capabilities` for
discovery, rather than assume a fixed set.

The current surface has three sources of truth:

```text
agent/src/server.rs          route table
docs/AGENT.md                endpoint reference
web-app/src/data/api.ts      dashboard bindings
```

`scripts/check-api-contract.py` fails the build when the agent, the dashboard,
and the mock agent disagree about which routes exist.

## Added here

- **eUICC / eSIM, read and write.** A dependency-free QMI-over-QRTR transport and
  a GSMA ES10 client, with lpac for profile management. Neither upstream has it.
  See [docs/EUICC.md](docs/EUICC.md).
- **`/api/capabilities`.** Capability discovery, so the client hides a removed
  feature rather than showing a 404.
