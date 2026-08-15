# Safety: how to not brick the U60 Pro

These are hard-won rules for this device. Read them before you run anything
against the modem with ADB or SSH access. A step beyond the sanctioned path
bricked the device once (July 2026). Everything below is the reason it cannot
happen again.

## Golden rules

1. **Use shell, ssh, and adb only.** Add no boot hooks outside `/etc/rc.local`.
   Add no firewall includes or hooks. Make no init.d service changes. Make no uci
   changes to system services. A boot-time hook that stalls, or whose target
   moves, can wedge the device before any recovery interface exists. The
   `firewall.zte_recovery` bootstrap, withdrawn in commit `a41f89c`, is the
   canonical example of what NOT to reintroduce.
2. **Never disable a daemon listed in `zte_topsw_daemon.conf`** through
   `/etc/init.d/<name> disable`. See the sync barrier below.
3. **Stay out of the partitions.** Use no `dd`, `mtd`, `fw_setenv`, `/dev/block`,
   QFPROM or fuse writes, or `abctl --set_active`. A mixed-slot boot can brick the
   device. There is no recovery from these without a flasher.
4. **Config backup/restore is the only sanctioned privileged path**
   (`scripts/zunlock.py`, `scripts/zbackup.py`). Always run `--dry-run` first.
   The restore runs as root and extracts whatever it validates.
5. **Exploit and injection tooling is kept out of this repository.** It was
   quarantined under `scripts/research/` during the unlock work. It is retained
   only privately. It is not published, because it is the class of tool that
   bricked the original device.
6. **FOTA auto-update stays off** (`zharden.sh` step 4). A surprise firmware
   update wipes rc.local hooks, and can change the rules under the agent.
7. **Never write the USB composition node (`usb_op`) live.** Set it through
   rc.local and a reboot. A live write (`0` or `2` after `1`) kills the gadget
   until the next reboot.

## CRITICAL: ZTE daemon sync barrier

`zte_topsw_daemon` is the master daemon. It reads
`/etc/config/zte_topsw_daemon.conf`. It **waits for ALL listed daemons to
register** before it releases the boot sequence.

If you disable a daemon.conf daemon through init.d, the device does this:

- it sticks on the ZTE boot logo (the UI never renders),
- it never connects the WAN (the mobile data call never starts),
- it leaves the touchscreen dead (the mtdev2tuio bridge never starts).

All other daemons appear to run fine, which makes the root cause very hard to
diagnose.

**These daemons are in daemon.conf. NEVER disable them through init.d. NEVER kill
them casually:**

```
zte_topsw_mc, zte_router, zte_topsw_data, zte_topsw_nwinfo,
zte_topsw_mdm, zte_topsw_sleep_faw, zte_topsw_apn, zte_topsw_wms,
zte_topsw_key, zte_topsw_led, zte_topsw_tr098db, zte_dm,
zte_topsw_fota_result, zte_topsw_devui, zte_topsw_wlan, zte_smart_manage
```

**These are safe to disable through init.d. They are NOT in daemon.conf:**

```
zte_topsw_diag, zte_topsw_samba, zte_topsw_nfc, zte_topsw_get_brand,
zte_topsw_jwxk_query, zte_topsw_tr069_sub, zte_mqtt_sdk_st,
zte_topsw_dua, zte-topsw-tunnel
```

The agent's `kill-bloat` endpoint (`agent/src/system.rs`, `BLOAT_PROCESSES`)
contains only the safe list. Do not add daemon.conf names to it. Killing
`zte_topsw_wms` breaks SMS. Killing `zte_topsw_sleep_faw` breaks wakelocks. procd
respawns them anyway.

If a daemon.conf daemon truly must be disabled, comment it out in
`/etc/config/zte_topsw_daemon.conf` with a `#` prefix. Never do it through
init.d.

### Overlay whiteouts

`/etc/init.d/<name> disable` creates **whiteout character devices** in
`/zteoverlay/etc-upper_a/rc.d/`. They silently delete the ROM symlinks. They are
invisible in a normal `ls /etc/rc.d/`, and they persist across reboots.

- Check: `ls -la /zteoverlay/etc-upper_a/rc.d/ | grep '^c'`
- Fix: `rm /zteoverlay/etc-upper_a/rc.d/<whiteout_file>`

### Verify sync status after boot

```sh
ubus call zwrt_topsw_daemon.sync get_sync_info '{}'
# Should return: "noSyncModuleName": "sync success"
```

## Recovery commands

**Display stuck on the logo:**

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

- **Airplane mode bug.** `nwinfo_set_mode ONLINE` does NOT recover the modem from
  low-power mode. The only fix is a reboot.
- **Charge policy inversion.** `zwrt_bsp.charger set
  {"direct_power_supply_mode":"enable"}` STOPS charging. `"disable"` STARTS it.
  The agent's charge-control code (`agent/src/charge_policy.rs`) already accounts
  for this. Do not "fix" the inversion.
- **procd respawn.** `kill -9` on a procd service may respawn it. Use
  `/etc/init.d/<name> stop` instead.
- **rc.local discipline.** The stock rc.local contains a flash-protect block that
  READS `usb_op`. Never delete that block. `sed '/usb_op/d'` breaks the script's
  syntax and kills ALL rc.local actions on the next boot. Always run `sh -n
  /etc/rc.local` after any edit.
- **IMEI is QFPROM-fused.** It is hardware-locked, not modifiable. Do not try.
- **eSIM.** The device has no soldered eUICC. A removable eUICC card in the SIM
  slot does work, and this suite manages it. See [EUICC.md](EUICC.md).

## What the deploy path does, and only this

`setup.sh`, `deploy.sh`, `deploy-dashboard.sh`, and `scripts/zharden.sh` do this:

- they push `/data/zte-agent` and `/data/local/tmp/start_zte_agent.sh`,
- they push the dashboard static files to `/data/www`,
- they append `sh /data/local/tmp/start_*.sh` lines to `/etc/rc.local`
  (idempotent, grep-guarded, and syntax-checked with `sh -n`),
- they install dropbear to `/data/bin` (zharden) with key auth on port 2222,
- they add a second uhttpd instance on :8080 for the dashboard
  (uci `uhttpd.dashboard`),
- they disable FOTA auto-update (`zwrt_zte_dm set_update_mode`).

Anything beyond this list is a red flag during review.

## Known-good ubus surface

`zte-script-ng.js` (repo root) is the community-vetted reference of ubus calls
that are safe on this firmware. It covers `zte_nwinfo_api` (netinfo, netselect,
band lock, cell lock), `zwrt_wlan set` (txpower, country, maxassoc), `uci get`,
and read-only status objects. New agent features should prefer these objects and
methods. Anything outside that surface deserves extra scrutiny.

---

# Safety audit: 2026-08-09

Scope: every commit (`git log --all`), the working tree, and the deploy path.
Goal: confirm that nothing in this repo can brick the device when ADB is regained
and this is deployed again.

**Verdict: the deploy path is clean.** The findings and dispositions are below.

## 1. Deploy path

| Surface | What it does | Verdict |
|---|---|---|
| `setup.sh` | unlock (through zunlock.py), agent push, startup script, rc.local line | idempotent, grep-guarded rc.local edits; safe |
| `deploy.sh` | ssh-only binary push and restart | safe |
| `deploy-dashboard.sh` | builds `web-app`, tars to `/data/www` | safe (data partition only) |
| `scripts/zharden.sh` | dropbear to `/data`, rc.local cleanup, uhttpd :8080, FOTA off | v2 removed the firewall-include bootstrap, the last boot-critical hook; safe now |
| `scripts/zunlock.py` / `zbackup.py` | config backup patch and restore (the unlock itself) | highest-risk by nature, but gated: `--dry-run`, explicit confirm, sha256 upload verification, and payload auto-discovered from the device's own rc.local |

## 2. Agent boot-time behavior: acceptable, documented

- `main.rs` runs `/data/local/tmp/start_ttl.sh` (iptables mangle TTL and HL
  rules). This is a runtime firewall only. It has no persistence beyond that
  script. Safe.
- `usb::enforce_usb_mode_on_boot()` rebuilds the USB configfs gadget **only if
  NCM was explicitly persisted** (`/data/local/tmp/usb_config.json`). It waits up
  to 75 s for the stock USB stack to finish (a bridge-membership check). It skips
  in power-off-charging states. configfs is runtime sysfs, so a failure cannot
  persist across a reboot. The worst case is that tethering needs a reboot.
  Acceptable.
- All agent state files live under `/data/local/tmp/` (the writable data
  partition). The agent never writes `/etc`, `/zteoverlay`, or raw partitions,
  except through `uci commit wireless`, `dhcp`, or `uhttpd`, and the documented
  rc.local lines.

## 3. Findings acted on (2026-08-09)

1. **Exploit tooling quarantined.** `zacs.py` (a rogue TR-069 ACS with
   SetParameterValues), `zrce.py`, `zinj.py`, `zdns.py`, `zstrings.py`, `zgap.py`
   (fac_reset, fac_reboot, and FOTA probes), `zadb.py`, `zhidden.py`, and
   `zwrite.py` were moved to `scripts/research/` with a warning README. They never
   ran as part of deploy, but they are the class of tool that bricked the device
   and should not sit next to sanctioned tools. They were later removed from the
   published repository entirely and are kept only privately.
2. **Safety docs consolidated into this file** (device rules and audit).
3. **v2.1 upstream regressions rejected** during the feature port (below).
4. **setup.sh unlock modernized.** The dead `zwrt_bsp.usb set {mode:debug}` path
   was replaced with the backup/restore route. The broken in-setup dropbear
   install (a 404 URL and unusable opkg) was removed in favor of `zharden.sh`.

## 4. v2.1 features ported, and what was deliberately NOT ported

Ported (safe, gated): charge control (`zwrt_bsp.charger`, with the inverted
semantics handled), USB powerbank (`zwrt_bsp.powerbank set`), the SMS
sqlite-delete fallback for SIM-stored messages, `/api/at/port`, and the WiFi dual
UCI namespace (`zte_mbb.wifi.*` and `wireless.zte_mbb.*`) with a guest-time read.

**Rejected from v2.1 (safety regressions):**

| v2.1 change | Why rejected |
|---|---|
| kill-bloat list adds `zte_topsw_mc`, `zte_dm`, `zte_topsw_wms`, `zte_topsw_sleep_faw`, `zte_topsw_tr098db` | these are daemon.conf daemons, a sync-barrier risk (see above); the current list is kept |
| unrestricted AT terminal (any `AT…` accepted) | the current allowlist is kept (`AT+CFUN`, `AT^…`, `AT+CMGD`, `AT$QCRMCALL`, and others blocked) |
| bind `0.0.0.0:9090`, no CORS pinning, no body limit, no `X-Confirm` on destructive ops | the current hardening is kept (LAN bind `192.168.0.1`, LAN-only CORS, 1 MiB body cap, `X-Confirm: true` required) |
| Tailscale module | skipped by owner decision (it installs and supervises a daemon, and downloads binaries) |

The scheduler, the DoH proxy, and the SMS forwarder were removed at this time,
because they had no dashboard surface. `main.rs` runs a one-shot migration that
undoes the old DoH proxy's dnsmasq rewiring, so a device that had it enabled does
not come back up forwarding DNS to a dead port. (DoH later returned as a separate
feature that drives an external proxy binary. See AGENT.md.)

## 5. Secrets and sensitive data

- `back_parameter` (the encrypted config backup), `adb-lock-investigation.md`
  (which contains the backup-key suffix, the IMEI, and sticker credentials),
  `logs/`, and `loopdebug-capture/` are **gitignored and never committed**. This
  was verified across all history. Keep it that way.
- No IMEI, passwords, session tokens, or backup-key material are in any committed
  file (scanned 2026-08-09).
- The `scripts/research/` tools are env-parameterized (no embedded credentials).

## 6. Residual risks (accepted, documented)

- `zunlock.py` restore path: inherent to the unlock, with mitigations in place.
- NCM gadget rebuild: runtime only, recoverable by a reboot.
- `zwrt_bsp.charger set` can stop charging. The charge-limit enforcer re-enables
  charging when you unplug the charger, and when you disable it. The API exposes a
  manual override.
- The `scripts/research/` tools remain runnable if deliberately invoked. That is
  the point of the quarantine README. They are kept out of the published
  repository.
