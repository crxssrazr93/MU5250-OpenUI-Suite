# Deployment — unlock, install, update

Everything needed to go from a locked U60 Pro to the full stack (agent +
dashboard + SSH), and to keep it there. Read [SAFETY.md](SAFETY.md) first.

## The whole flow at a glance

```sh
python3 scripts/zunlock.py     # 1. unlock → adbd        (locked firmware only)
bash setup.sh                  # 2. build + install the agent (choose build-from-source)
bash scripts/zharden.sh        # 3. SSH, rc.local cleanup, dashboard :8080, FOTA off
bash deploy-dashboard.sh       # 4. build + push the web UI
```

End state (persists across reboots):

| Service | Where | Notes |
|---|---|---|
| USB-C tethering | USB-C, ECM (stock composition) | survives reboots (no usb_op write) |
| Stock web UI | `http://192.168.0.1:80` / `:443` | untouched |
| Dashboard | `http://192.168.0.1:8080` | uhttpd `dashboard` instance → `/data/www` |
| Agent API | `http://192.168.0.1:9090` | password = your choice at setup |
| SSH | `ssh -p 2222 root@192.168.0.1` | key-only, `/data/bin/dropbear` |
| ADB | on demand | `echo 1 > /sys/class/android_usb/android0/usb_op` via SSH + reboot; reverts next reboot |

---

## 1. Unlock (locked firmware: HK B04+, CN B28+)

Newer MU5250 firmware removed the web-accessible USB-debug toggle
(`zwrt_bsp.usb.set`) — on B04 the method is deleted from the daemon itself,
so no web trick can re-enable ADB. (CN B27 and earlier still have it — if
your device is that old, `setup.sh` can enable ADB directly and you can skip
this section.)

What still works is the **config backup/restore path**: the backup is an
openssl-encrypted tar of the system config, and the restore process runs as
root and extracts whatever you give it. `scripts/zunlock.py` uses that to
plant one line in `etc/rc.local` that re-enables the USB debug composition
(adbd) at boot.

The script is fully self-contained (Python 3 stdlib + `openssl` CLI) and
contains **no secrets**: the backup-key suffix is an input, the device IMEI
is read from the device, and the USB-debug sysfs path is discovered from the
device's own stock `rc.local` inside the backup.

### Requirements

- The router's **admin password** (you set it — it's your web UI login)
- The **backup-key suffix** for this device family (see below)
- A computer on the device's network (WiFi or USB), Python 3, openssl
- `adb` installed for afterwards (`brew install android-platform-tools`)

### The backup-key suffix

The backup encryption password is `<device IMEI><suffix>` — the IMEI is
per-device (the script reads it itself), the suffix is a fixed string shared
across this ZTE platform generation. It is deliberately **not** published
here, at the request of the researchers who shared it (publishing it gets it
killed in the next firmware). To obtain it:

- ask the community
- extract it yourself from a rooted SDX75-era ZTE MBB unit: the web server
  binary (`zte_web`) builds the backup password in memory; the suffix is
  visible in its strings near the backup/restore code paths

Pass it via `--suffix`, the `ZTE_BACKUP_SUFFIX` env var, or the interactive
(hidden) prompt. It never touches this repo.

### Usage

```sh
python3 scripts/zunlock.py --dry-run     # everything except the upload (safe)
python3 scripts/zunlock.py               # full run, asks before restoring
```

`setup.sh` runs both stages automatically when it detects a locked device
(no SSH, no ADB) — it prompts for the suffix, does the dry run first, then
the real unlock with `zunlock.py`'s own consent gate.

What happens on a full run:

1. logs into the web UI, requests a fresh config backup, downloads it
2. decrypts it (`openssl enc -d -des-ede3-cbc -md sha256`)
3. inserts the USB-debug line into `etc/rc.local` (right after the shebang,
   path discovered from the stock file), preserving file modes/ownership
4. rebuilds the package exactly as the device does (inner tgz → md5 sidecar →
   outer tgz → re-encrypt) — passes the device's own restore-time md5 check
5. uploads it (`/cgi-bin/cgi-upload`, verifies the server's sha256 matches),
   triggers `device_restore_proc` — the device restores and reboots
6. ~60–90 s later: `adb devices` shows the unit (serial `0123456789ABCDEF`),
   root shell via `adb shell`

Your settings are preserved — the patched package is built from a backup
taken seconds earlier.

### Unlock safety notes

- The restore reboots the device and briefly interrupts connectivity (~90 s).
- The script verifies the upload hash before triggering anything; a mismatch
  aborts before any state change.
- Never write the USB composition node manually outside boot time (live
  writes can kill the gadget until reboot), and never experiment with A/B
  slot switching (`abctl --set_active`) — mixed-slot boots can brick the unit.

---

## 2. Agent install — `setup.sh`

```sh
bash setup.sh
```

- Prompts for the router admin password and the agent API password.
- **Choose "build from source"** (the default). The pre-built download is
  the upstream agent, which lacks this fork's endpoints and would leave
  parts of the dashboard empty.
- If the device is locked (no SSH, no ADB) it runs the unlock first (see
  above). If ADB is already up or SSH works, it deploys straight away.
- Pushes the agent to `/data/zte-agent`, creates the startup script with
  your password, adds the rc.local line, starts and verifies it.

## 3. Hardening — `scripts/zharden.sh`

```sh
bash scripts/zharden.sh
```

Idempotent — safe to re-run anytime. Installs dropbear to `/data/bin`
(opkg is unusable on this firmware), generates host keys, wires SSH into
rc.local, **removes the usb_op payload line** (so every boot returns to
stock ECM tethering), adds the dashboard uhttpd instance on :8080, and
disables FOTA auto-update.

## 4. Dashboard — `deploy-dashboard.sh`

```sh
bash deploy-dashboard.sh
```

Builds `web-app` (Vite) and streams `dist/` to `/data/www` over an SSH tar
pipe (the device has no sftp/scp). Also copies `index.html` → `mobile.html`
so ZTE's patched uhttpd serves the SPA to phone user-agents.

## Updating later

```sh
./deploy.sh              # agent (SSH; set ZTE_AGENT_PASSWORD / ZTE_AGENT_PIN)
./deploy-dashboard.sh    # dashboard
```

---

## Design rule: shell/ssh/adb only — no boot hooks outside rc.local

`zharden.sh` deliberately installs **no** boot-time hooks outside
`/etc/rc.local` and does not modify system services. An earlier approach
hooked boot through a `config include` section in the firewall service
(chosen because the UCI config dir survives FOTA) — that approach is
**deprecated and removed**: a hook inside a boot-critical service is a brick
risk. If it stalls or its target moves, the device can hang before any
recovery interface (ssh/adb/failsafe) is up, and recovery then requires
hardware access. See [SAFETY.md](SAFETY.md) for the incident history.

Trade-off accepted: `/etc/rc.local` is **not** preserved by FOTA, so a
firmware update wipes the service lines (the device itself still boots
cleanly to stock). Recovery after an update is simply re-running the
sequence above (~15 minutes). That is the right price for never risking the
boot path.

Notes:

- ADB is a bootstrap channel, not a good permanent interface on this
  firmware (its composition drops USB networking, and it only applies at
  boot) — SSH is the durable management channel.
- The rootfs is read-only except `/etc` and `/data`; `/data` survives FOTA,
  so binaries and web assets persist — only the rc.local lines need
  re-adding after an update.

## Post-FOTA recovery playbook

1. Verify the update landed and the device boots stock: ping, web UI up.
2. Re-run the sequence: `zunlock.py` (if ADB is gone) → `setup.sh` →
   `zharden.sh` → `deploy-dashboard.sh`.
3. Confirm FOTA auto-update is off again:
   `ssh -p 2222 root@192.168.0.1 'uci get zwrt_zte_dm.dm_update.dm_update_mode'` → `0`.
4. If the backup-key suffix ever stops working (ZTE rotated it): extract the
   new suffix from `strings /usr/sbin/zte_web` on any rooted unit.

## Credits

Backup-crypto details and the original payload hint: the
`amenekowo/mu5250_tweaking` community (with thanks — they asked that the key
material itself not be republished, and this tool honors that). B04
daemon/ACL analysis: community contributors on the issue tracker.
