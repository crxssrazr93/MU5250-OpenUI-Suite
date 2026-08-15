# Deployment: unlock, install, update

This guide has everything you need to go from a locked U60 Pro to the full stack
(agent, dashboard, and SSH), and to keep it there. Read [SAFETY.md](SAFETY.md)
first.

## The whole flow at a glance

```sh
python3 scripts/zunlock.py     # 1. unlock -> adbd        (locked firmware only)
bash setup.sh                  # 2. build + install the agent (choose build-from-source)
bash scripts/zharden.sh        # 3. SSH, rc.local cleanup, dashboard :8080, FOTA off
bash deploy-dashboard.sh       # 4. build + push the web UI
```

The end state persists across reboots:

| Service | Where | Notes |
|---|---|---|
| USB-C tethering | USB-C, ECM (stock composition) | survives reboots (no usb_op write) |
| Stock web UI | `http://192.168.0.1:80` / `:443` | untouched |
| Dashboard | `http://192.168.0.1:8080` | uhttpd `dashboard` instance -> `/data/www` |
| Agent API | `http://192.168.0.1:9090` | password = your choice at setup |
| SSH | `ssh -p 2222 root@192.168.0.1` | key-only, `/data/bin/dropbear` |
| ADB | on demand | `echo 1 > /sys/class/android_usb/android0/usb_op` over SSH, then reboot. It reverts on the next reboot |

---

## 1. Unlock (locked firmware: HK B04+, CN B28+)

Newer MU5250 firmware removed the web-accessible USB-debug toggle
(`zwrt_bsp.usb.set`). On B04 the daemon itself deletes the method, so no web
trick can re-enable ADB. CN B27 and earlier still have it. If your device is that
old, `setup.sh` can enable ADB directly, and you can skip this section.

What still works is the **config backup/restore path**. The backup is an
openssl-encrypted tar of the system config. The restore process runs as root and
extracts whatever you give it. `scripts/zunlock.py` uses that to plant one line
in `etc/rc.local`. That line re-enables the USB debug composition (adbd) at boot.

The script is self-contained (Python 3 standard library plus the `openssl` CLI).
It contains **no secrets**. The backup-key suffix is an input. The script reads
the device IMEI from the device. It discovers the USB-debug sysfs path from the
device's own stock `rc.local` inside the backup.

### Requirements

- The router's **admin password**. You set it. It is your web UI login.
- The **backup-key suffix** for this device family. See below.
- A computer on the device's network (Wi-Fi or USB), Python 3, and openssl.
- `adb` installed for afterwards (`brew install android-platform-tools`).

### The backup-key suffix

The backup encryption password is `<device IMEI><suffix>`. The IMEI is
per-device, and the script reads it itself. The suffix is a fixed string, shared
across this ZTE platform generation. It is deliberately **not** published here,
at the request of the researchers who shared it. Publishing it gets it killed in
the next firmware. To obtain it:

- ask the community,
- or extract it yourself from a rooted SDX75-era ZTE MBB unit. The web server
  binary (`zte_web`) builds the backup password in memory. The suffix is visible
  in its strings near the backup/restore code paths.

Pass it with `--suffix`, the `ZTE_BACKUP_SUFFIX` env var, or the hidden
interactive prompt. It never touches this repo.

### Usage

```sh
python3 scripts/zunlock.py --dry-run     # everything except the upload (safe)
python3 scripts/zunlock.py               # full run, asks before restoring
```

`setup.sh` runs both stages automatically when it detects a locked device (no
SSH, no ADB). It prompts for the suffix, does the dry run first, then the real
unlock behind `zunlock.py`'s own consent gate.

A full run does this:

1. It signs in to the web UI, requests a fresh config backup, and downloads it.
2. It decrypts the backup (`openssl enc -d -des-ede3-cbc -md sha256`).
3. It inserts the USB-debug line into `etc/rc.local`, right after the shebang. It
   discovers the path from the stock file and preserves the file modes and
   ownership.
4. It rebuilds the package exactly as the device does (inner tgz, then md5
   sidecar, then outer tgz, then re-encrypt). This passes the device's own
   restore-time md5 check.
5. It uploads the package (`/cgi-bin/cgi-upload`), verifies that the server's
   sha256 matches, and triggers `device_restore_proc`. The device restores and
   reboots.
6. About 60 to 90 seconds later, `adb devices` shows the unit (serial
   `0123456789ABCDEF`). You get a root shell with `adb shell`.

Your settings are preserved. The patched package is built from a backup taken
seconds earlier.

### Unlock safety notes

- The restore reboots the device and briefly interrupts connectivity (about 90
  seconds).
- The script verifies the upload hash before it triggers anything. A mismatch
  aborts before any state change.
- Never write the USB composition node by hand outside boot time. A live write
  can kill the gadget until reboot. Never experiment with A/B slot switching
  (`abctl --set_active`). A mixed-slot boot can brick the unit.

---

## 2. Agent install: `setup.sh`

```sh
bash setup.sh
```

- It prompts for the router admin password and the agent API password.
- **Choose "build from source"** (the default). The pre-built download is the
  upstream agent. It lacks this fork's endpoints and would leave parts of the
  dashboard empty.
- If the device is locked (no SSH, no ADB), it runs the unlock first (see above).
  If ADB is already up, or SSH works, it deploys straight away.
- It pushes the agent to `/data/zte-agent`, creates the startup script with your
  password, adds the rc.local line, and starts and verifies the agent.

## 3. Hardening: `scripts/zharden.sh`

```sh
bash scripts/zharden.sh
```

It is idempotent, so it is safe to re-run any time. It installs dropbear to
`/data/bin` (opkg is unusable on this firmware), generates host keys, and wires
SSH into rc.local. It **removes the usb_op payload line**, so every boot returns
to stock ECM tethering. It adds the dashboard uhttpd instance on :8080. It
disables FOTA auto-update.

## 4. Dashboard: `deploy-dashboard.sh`

```sh
bash deploy-dashboard.sh
```

It builds `web-app` (Vite) and streams `dist/` to `/data/www` over an SSH tar
pipe, because the device has no sftp or scp. It also copies `index.html` to
`mobile.html`, so ZTE's patched uhttpd serves the SPA to phone user-agents.

## Updating later

```sh
./deploy.sh              # agent (SSH; set ZTE_AGENT_PASSWORD / ZTE_AGENT_PIN)
./deploy-dashboard.sh    # dashboard
```

---

## Design rule: shell, ssh, and adb only. No boot hooks outside rc.local

`zharden.sh` installs **no** boot-time hooks outside `/etc/rc.local`, and it does
not modify system services. An earlier approach hooked boot through a `config
include` section in the firewall service. It chose that because the UCI config
dir survives FOTA. That approach is **deprecated and removed**. A hook inside a
boot-critical service is a brick risk. If it stalls, or its target moves, the
device can hang before any recovery interface (ssh, adb, or failsafe) is up.
Recovery then needs hardware access. See [SAFETY.md](SAFETY.md) for the incident
history.

There is an accepted trade-off. FOTA does **not** preserve `/etc/rc.local`, so a
firmware update wipes the service lines. The device itself still boots cleanly to
stock. Recovery after an update is just re-running the sequence above, about 15
minutes. That is the right price for never risking the boot path.

Notes:

- ADB is a bootstrap channel, not a good permanent interface on this firmware.
  Its composition drops USB networking, and it applies only at boot. SSH is the
  durable management channel.
- The rootfs is read-only except for `/etc` and `/data`. `/data` survives FOTA,
  so binaries and web assets persist. Only the rc.local lines need re-adding
  after an update.

## Post-FOTA recovery playbook

1. Verify the update landed and the device boots stock. Check ping and the web
   UI.
2. Re-run the sequence: `zunlock.py` (if ADB is gone), then `setup.sh`, then
   `zharden.sh`, then `deploy-dashboard.sh`.
3. Confirm FOTA auto-update is off again:
   `ssh -p 2222 root@192.168.0.1 'uci get zwrt_zte_dm.dm_update.dm_update_mode'`
   returns `0`.
4. If the backup-key suffix ever stops working (ZTE rotated it), extract the new
   suffix from `strings /usr/sbin/zte_web` on any rooted unit.

## Credits

Backup-crypto details and the original payload hint came from the
`amenekowo/mu5250_tweaking` community. Thanks to them. They asked that the key
material itself not be republished, and this tool honors that. The B04 daemon and
ACL analysis came from community contributors on the issue tracker.
