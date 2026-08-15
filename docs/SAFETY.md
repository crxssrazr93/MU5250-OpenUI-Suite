# Safety — how to not brick the U60 Pro

Hard-won rules for this device. Read before running anything against the
modem with ADB/SSH access. The device was bricked once already (July 2026)
by going beyond the sanctioned path; everything below is the reason it
cannot happen again.

## Golden rules

1. **Shell/ssh/adb only.** No boot hooks outside `/etc/rc.local`. No
   firewall includes/hooks, no init.d service changes, no uci changes to
   system services. A boot-time hook that stalls or whose target moves can
   wedge the device before any recovery interface exists. (The
   `firewall.zte_recovery` bootstrap, withdrawn in commit `a41f89c`, is the
   canonical example of what NOT to reintroduce.)
2. **Never disable a daemon listed in `zte_topsw_daemon.conf` via
   `/etc/init.d/<name> disable`** — see the sync barrier below.
3. **Stay out of partitions.** No `dd`, `mtd`, `fw_setenv`, `/dev/block`,
   QFPROM/fuse writes, `abctl --set_active` (mixed-slot boots can brick).
   There is no recovery from these without a flasher.
4. **Config backup/restore is the only sanctioned privileged path**
   (`scripts/zunlock.py`, `scripts/zbackup.py`). Always `--dry-run` first;
   restore runs as root and extracts whatever it validates.
5. **Exploit/injection tooling is kept out of this repository.** It was
   quarantined under `scripts/research/` during the unlock work and is retained
   only privately; it is not published, because it is the class of tool that
   bricked the original device.
6. **FOTA auto-update stays off** (`zharden.sh` step 4). A surprise firmware
   update wipes rc.local hooks and can change the rules under the agent.
7. **Never write the USB composition node (`usb_op`) live** — set it via
   rc.local + reboot. Live writes (`0` or `2` after `1`) kill the gadget
   until the next reboot.

## CRITICAL: ZTE daemon sync barrier

`zte_topsw_daemon` is the master daemon. It reads
`/etc/config/zte_topsw_daemon.conf` and **waits for ALL listed daemons to
register** before releasing the boot sequence.

If you disable a daemon.conf daemon via init.d, the device will:

- stick on the ZTE boot logo (UI never renders)
- never connect WAN (mobile data call never initiated)
- leave the touchscreen dead (mtdev2tuio bridge never starts)

…while all other daemons appear to run fine, which makes the root cause
very hard to diagnose.

**Daemons in daemon.conf (NEVER disable via init.d, NEVER kill casually):**

```
zte_topsw_mc, zte_router, zte_topsw_data, zte_topsw_nwinfo,
zte_topsw_mdm, zte_topsw_sleep_faw, zte_topsw_apn, zte_topsw_wms,
zte_topsw_key, zte_topsw_led, zte_topsw_tr098db, zte_dm,
zte_topsw_fota_result, zte_topsw_devui, zte_topsw_wlan, zte_smart_manage
```

**Safe to disable via init.d (NOT in daemon.conf):**

```
zte_topsw_diag, zte_topsw_samba, zte_topsw_nfc, zte_topsw_get_brand,
zte_topsw_jwxk_query, zte_topsw_tr069_sub, zte_mqtt_sdk_st,
zte_topsw_dua, zte-topsw-tunnel
```

The agent's `kill-bloat` endpoint (`agent/src/system.rs` `BLOAT_PROCESSES`)
deliberately contains only the safe list. Do not add daemon.conf names to it
(killing `zte_topsw_wms` breaks SMS, `zte_topsw_sleep_faw` breaks wakelocks,
and procd will respawn them anyway).

If a daemon.conf daemon truly must be disabled, comment it out in
`/etc/config/zte_topsw_daemon.conf` (prefix `#`) — never via init.d.

### Overlay whiteouts

`/etc/init.d/<name> disable` creates **whiteout character devices** in
`/zteoverlay/etc-upper_a/rc.d/` that silently delete the ROM symlinks.
Invisible in normal `ls /etc/rc.d/`, persistent across reboots.

- Check: `ls -la /zteoverlay/etc-upper_a/rc.d/ | grep '^c'`
- Fix: `rm /zteoverlay/etc-upper_a/rc.d/<whiteout_file>`

### Verify sync status after boot

```sh
ubus call zwrt_topsw_daemon.sync get_sync_info '{}'
# Should return: "noSyncModuleName": "sync success"
```

## Recovery commands

**Display stuck on logo:**

```sh
sh /usr/bin/mtdev2tuio.sh                                      # touchscreen bridge
kill -9 $(pidof zte_topsw_devui); /usr/bin/zte_topsw_devui &   # restart UI
```

**WAN not connecting:**

```sh
ubus call zwrt_qcmap_cli set_qcliiface '{"source_module":"zte_topsw_data","type":1,"enable":1,"sub_id":1}'
ubus call zwrt_qcmap_cli set_qcliiface '{"source_module":"zte_topsw_data","type":2,"enable":1,"sub_id":1}'
```

**Check data call status:**

```sh
ubus call zwrt_data get_wwaniface '{"source_module":"zte_topsw_data","cid":1}'
# Look for: "enable": 1, "connect_status": "connected"
```

## Firmware behavior gotchas

- **Airplane mode bug**: `nwinfo_set_mode ONLINE` does NOT recover the modem
  from low-power mode. Only fix: reboot.
- **Charge policy inversion**: `zwrt_bsp.charger set
  {"direct_power_supply_mode":"enable"}` STOPS charging; `"disable"` STARTS
  it. The agent's charge-control code (`agent/src/charge_policy.rs`) already
  accounts for this — don't "fix" the inversion.
- **procd respawn**: `kill -9` on a procd service may respawn it. Use
  `/etc/init.d/<name> stop` instead.
- **rc.local discipline**: stock rc.local contains a flash-protect block that
  READS `usb_op` — never delete that block (`sed '/usb_op/d'` breaks the
  script's syntax and kills ALL rc.local actions next boot). Always `sh -n
  /etc/rc.local` after any edit.
- **IMEI is QFPROM-fused** — hardware-locked, not modifiable. Don't try.
- **eSIM**: no eUICC chip; not feasible.

## What the deploy path does (and only this)

`setup.sh` / `deploy.sh` / `deploy-dashboard.sh` / `scripts/zharden.sh`:

- push `/data/zte-agent` + `/data/local/tmp/start_zte_agent.sh`
- push dashboard static files to `/data/www`
- append `sh /data/local/tmp/start_*.sh` lines to `/etc/rc.local`
  (idempotent, grep-guarded, syntax-checked with `sh -n`)
- install dropbear to `/data/bin` (zharden) with key auth on port 2222
- add a second uhttpd instance on :8080 for the dashboard (uci `uhttpd.dashboard`)
- disable FOTA auto-update (`zwrt_zte_dm set_update_mode`)

Anything beyond this list is a red flag during review.

## Known-good ubus surface

`zte-script-ng.js` (repo root) is the community-vetted reference of ubus
calls that are safe on this firmware: `zte_nwinfo_api` (netinfo/netselect/
bandlock/cell lock), `zwrt_wlan set` (txpower/country/maxassoc), `uci get`,
and read-only status objects. New agent features should prefer these objects
and methods; anything outside that surface deserves extra scrutiny.

---

# Safety audit — 2026-08-09

Scope: every commit (`git log --all`), the working tree, and the deploy
path. Goal: confirm nothing in this repo can brick the device when ADB is
regained and this is deployed again.

**Verdict: the deploy path is clean.** Findings and dispositions below.

## 1. Deploy path

| Surface | What it does | Verdict |
|---|---|---|
| `setup.sh` | unlock (via zunlock.py) + agent push, startup script, rc.local line | idempotent, grep-guarded rc.local edits; safe |
| `deploy.sh` | ssh-only binary push + restart | safe |
| `deploy-dashboard.sh` | builds `web-app`, tars to `/data/www` | safe (data partition only) |
| `scripts/zharden.sh` | dropbear to `/data`, rc.local cleanup, uhttpd :8080, FOTA off | v2 removed the firewall-include bootstrap — the last boot-critical hook; safe now |
| `scripts/zunlock.py` / `zbackup.py` | config backup patch + restore (the unlock itself) | highest-risk by nature, but gated: `--dry-run`, explicit confirm, sha256 upload verification, payload auto-discovered from the device's own rc.local |

## 2. Agent boot-time behavior — acceptable, documented

- `main.rs` runs `/data/local/tmp/start_ttl.sh` (iptables mangle TTL/HL
  rules). Runtime firewall only; no persistence beyond that script. Safe.
- `usb::enforce_usb_mode_on_boot()` rebuilds the USB configfs gadget **only
  if NCM was explicitly persisted** (`/data/local/tmp/usb_config.json`),
  waits up to 75 s for the stock USB stack to finish (bridge-membership
  check), and skips in power-off-charging states. configfs is runtime sysfs —
  a failure cannot persist across reboot. Worst case: tethering needs a
  reboot. Acceptable.
- All agent state files live under `/data/local/tmp/` (writable data
  partition). The agent never writes `/etc`, `/zteoverlay`, or raw
  partitions, except via `uci commit wireless`/`dhcp`/`uhttpd` and the
  documented rc.local lines.

## 3. Findings acted on (2026-08-09)

1. **Exploit tooling quarantined.** `zacs.py` (rogue TR-069 ACS with
   SetParameterValues), `zrce.py`, `zinj.py`, `zdns.py`, `zstrings.py`,
   `zgap.py` (fac_reset/fac_reboot/FOTA probes), `zadb.py`, `zhidden.py`,
   `zwrite.py` moved to `scripts/research/` with a warning README. They
   never ran as part of deploy, but they are the class of tool that bricked
   the device and should not sit next to sanctioned tools.
2. **Safety docs consolidated into this file** (device rules + audit).
3. **v2.1 upstream regressions rejected** during the feature port (below).
4. **setup.sh unlock modernized**: the dead `zwrt_bsp.usb set {mode:debug}`
   path replaced with the backup/restore route; the broken in-setup dropbear
   install (404 URL + unusable opkg) removed in favor of `zharden.sh`.

## 4. v2.1 features ported, and what was deliberately NOT ported

Ported (safe, gated): charge control (`zwrt_bsp.charger`, inverted semantics
handled), USB powerbank (`zwrt_bsp.powerbank set`), SMS sqlite-delete
fallback for SIM-stored messages, `/api/at/port`, WiFi dual UCI namespace
(`zte_mbb.wifi.*` + `wireless.zte_mbb.*`) with guest-time read.

**Rejected from v2.1 (safety regressions):**

| v2.1 change | Why rejected |
|---|---|
| kill-bloat list adds `zte_topsw_mc`, `zte_dm`, `zte_topsw_wms`, `zte_topsw_sleep_faw`, `zte_topsw_tr098db` | daemon.conf daemons — sync-barrier risk (see above); current list kept |
| unrestricted AT terminal (any `AT…` accepted) | current allowlist kept (`AT+CFUN`, `AT^…`, `AT+CMGD`, `AT$QCRMCALL` etc. blocked) |
| bind `0.0.0.0:9090`, no CORS pinning, no body limit, no `X-Confirm` on destructive ops | current hardening kept (LAN bind `192.168.0.1`, LAN-only CORS, 1 MiB body cap, `X-Confirm: true` required) |

The scheduler, DoH proxy and SMS forwarder have since been removed entirely —
they had no dashboard surface, and `main.rs` runs a one-shot migration that
undoes DoH's dnsmasq rewiring so a device that had it enabled does not come
back up forwarding DNS to a dead port.
| Tailscale module | skipped by owner decision (installs/supervises a daemon, downloads binaries) |

## 5. Secrets / sensitive data

- `back_parameter` (encrypted config backup), `adb-lock-investigation.md`
  (contains the backup-key suffix, IMEI and sticker credentials), `logs/`,
  `loopdebug-capture/` are **gitignored and never committed** — verified
  across all history. Keep it that way.
- No IMEI, passwords, session tokens or backup-key material in any committed
  file (scanned 2026-08-09).
- `scripts/research/` tools are env-parameterized (no embedded credentials).

## 6. Residual risks (accepted, documented)

- `zunlock.py` restore path: inherent to the unlock; mitigations in place.
- NCM gadget rebuild: runtime-only, recoverable by reboot.
- `zwrt_bsp.charger set` can stop charging; the charge-limit enforcer
  re-enables on charger unplug and on disable. Manual override is exposed
  via the API.
- `scripts/research/` remains runnable if deliberately invoked — that is the
  point of the quarantine README.
