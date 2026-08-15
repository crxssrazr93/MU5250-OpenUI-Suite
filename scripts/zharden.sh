#!/bin/bash
# zharden.sh — post-unlock hardening for the MU5250 (U60 Pro) on B04+.
#
# Run AFTER scripts/zunlock.py (adb up) and setup.sh (agent installed).
# Idempotent: safe to re-run anytime; each step no-ops when already done.
#
# What it does (details in docs/DEPLOYMENT.md):
#   1. installs dropbear SSH (port 2222, key auth) into /data
#   2. cleans rc.local: keeps stock + agent/dropbear lines, removes usb_op
#      write lines (so every boot = stock ECM tethering; adb on demand)
#   3. adds the dashboard uhttpd instance on :8080
#   4. disables FOTA auto-update
#   5. offers a final reboot into the clean state
#
# DESIGN RULE (2026-07-21): shell/ssh/adb only. This script deliberately
# installs NO boot hooks outside /etc/rc.local and does NOT modify system
# services (no firewall includes/hooks). A boot-time hook that stalls or
# fails can wedge the device before any recovery interface exists. rc.local
# is not FOTA-preserved, so after a firmware update simply re-run:
# zunlock.py -> setup.sh -> zharden.sh (~15 min, see docs/DEPLOYMENT.md).
#
# Usage: bash scripts/zharden.sh [--gw 192.168.0.1]
set -euo pipefail

GW="${1:-192.168.0.1}"; GW="${GW#--gw }"; GW="${GW#--gw=}"
SSH_PORT=2222
SSH="ssh -p $SSH_PORT -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=$HOME/.ssh/known_hosts.d/zte -o ConnectTimeout=5 root@$GW"
DROPBEAR_URL="https://downloads.openwrt.org/releases/23.05.4/targets/armsr/armv8/packages/dropbear_2022.82-6_aarch64_generic.ipk"
WGTOOLS_URL="https://downloads.openwrt.org/releases/23.05.4/packages/aarch64_generic/base/wireguard-tools_1.0.20210914-2_aarch64_generic.ipk"

info() { echo -e "\033[0;36m[*]\033[0m $1"; }
ok()   { echo -e "\033[0;32m[+]\033[0m $1"; }
warn() { echo -e "\033[1;33m[!]\033[0m $1"; }

# ── Channel: prefer adb (always present right after zunlock) ─────────────
if adb devices 2>/dev/null | grep -q 'device$'; then
  CH=adb; info "channel: adb"
elif $SSH 'true' 2>/dev/null; then
  CH=ssh; info "channel: ssh"
else
  echo "No channel: run scripts/zunlock.py first (adb), or have dropbear up (ssh)." >&2
  exit 1
fi
rcmd() { if [ "$CH" = adb ]; then adb shell "$@"; else $SSH "$@"; fi; }

# This device's adbd does not propagate exit codes — `adb shell 'exit 7'` still
# returns 0. So `if rcmd '<some test>'` is ALWAYS true over adb, which silently
# skipped the dropbear install and then reported it as already present. Test by
# having the remote shell print a sentinel and looking for it in stdout.
rtest() { rcmd "$1 && echo __RTEST_OK__" 2>/dev/null | tr -d '\r' | grep -q '^__RTEST_OK__$'; }

# ── 1. dropbear into /data ───────────────────────────────────────────────
if rtest 'test -x /data/bin/dropbear'; then
  ok "dropbear already installed"
else
  info "installing dropbear to /data/bin ..."
  TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
  curl -sfL "$DROPBEAR_URL" -o "$TMP/dropbear.ipk"
  # Extract the whole ipk rather than naming the member: the archive stores it
  # as "./data.tar.gz", and newer GNU tar will not match a bare "data.tar.gz"
  # against that, so asking for the member by name fails on current distros.
  (cd "$TMP" && tar xzf dropbear.ipk)
  [ -f "$TMP/data.tar.gz" ] || { echo "ipk did not contain data.tar.gz" >&2; exit 1; }
  if [ "$CH" = adb ]; then
    adb push "$TMP/data.tar.gz" /tmp/data.tar.gz >/dev/null
    rcmd 'cd /tmp && tar xzf data.tar.gz ./usr/sbin/dropbear ./usr/bin/dbclient ./usr/bin/dropbearkey && mkdir -p /data/bin && cp usr/sbin/dropbear usr/bin/dbclient usr/bin/dropbearkey /data/bin/ && chmod +x /data/bin/* && rm -rf /tmp/usr /tmp/data.tar.gz'
  else
    cat "$TMP/data.tar.gz" | $SSH 'cat > /tmp/data.tar.gz; cd /tmp && tar xzf data.tar.gz ./usr/sbin/dropbear ./usr/bin/dbclient ./usr/bin/dropbearkey && mkdir -p /data/bin && cp usr/sbin/dropbear usr/bin/dbclient usr/bin/dropbearkey /data/bin/ && chmod +x /data/bin/* && rm -rf /tmp/usr /tmp/data.tar.gz'
  fi
  rtest 'test -x /data/bin/dropbear' \
    || { echo "dropbear did not land in /data/bin — check the ipk extract above" >&2; exit 1; }
  ok "dropbear installed (manual ipk extract — opkg is unusable on this firmware)"
fi

# ── 1b. wireguard-tools into /data ───────────────────────────────────────
# The kernel here is built with CONFIG_WIREGUARD=y and the vendor's tunnel
# daemon already implements WireGuard — it shells out to `wg genkey`,
# `wg pubkey` and `wg setconf` via /sbin/wireguard_client.sh. The only missing
# piece is the `wg` userspace tool, which is why the daemon's own keygen
# returns FAILED on a stock unit.
#
# The rootfs is genuinely read-only (ext4 ro; /overlay is a separate mount, not
# an overlayfs on /), so `wg` cannot go anywhere on the daemon's PATH. It lives
# in /data/bin and the agent invokes the vendor scripts with PATH extended.
# That keeps every vendor file and the boot path untouched.
if rtest 'test -x /data/bin/wg'; then
  ok "wireguard-tools already installed"
else
  info "installing wireguard-tools to /data/bin ..."
  TMPW=$(mktemp -d); trap 'rm -rf "$TMPW"' EXIT
  if curl -sfL "$WGTOOLS_URL" -o "$TMPW/wgt.ipk"; then
    (cd "$TMPW" && tar xzf wgt.ipk && tar xzf data.tar.gz ./usr/bin/wg 2>/dev/null || tar xzf data.tar.gz)
    if [ -f "$TMPW/usr/bin/wg" ]; then
      if [ "$CH" = adb ]; then
        adb push "$TMPW/usr/bin/wg" /data/bin/wg >/dev/null
        rcmd 'chmod +x /data/bin/wg'
      else
        cat "$TMPW/usr/bin/wg" | $SSH 'mkdir -p /data/bin && cat > /data/bin/wg && chmod +x /data/bin/wg'
      fi
      rtest '/data/bin/wg --version >/dev/null 2>&1' \
        && ok "wireguard-tools installed (kernel already has CONFIG_WIREGUARD=y)" \
        || warn "wg installed but will not run — WireGuard features stay unavailable"
    else
      warn "wireguard-tools ipk had no usr/bin/wg — skipping"
    fi
  else
    warn "could not fetch wireguard-tools — WireGuard features stay unavailable"
  fi
fi

# ssh key + host keys + authorized_keys
[ -f "$HOME/.ssh/id_ed25519" ] || ssh-keygen -t ed25519 -f "$HOME/.ssh/id_ed25519" -N "" >/dev/null
rcmd 'mkdir -p /etc/dropbear /data/dropbear && chmod 700 /etc/dropbear'
PUB=$(cat "$HOME/.ssh/id_ed25519.pub")
rcmd "grep -qF '$PUB' /etc/dropbear/authorized_keys 2>/dev/null || echo '$PUB' >> /etc/dropbear/authorized_keys; chmod 600 /etc/dropbear/authorized_keys"
rcmd 'for k in ed25519 rsa; do f=/etc/dropbear/dropbear_${k}_host_key; [ -s "$f" ] || /data/bin/dropbearkey -t $k -f $f >/dev/null 2>&1; done'
rcmd 'cp /etc/dropbear/authorized_keys /etc/dropbear/dropbear_*_host_key /data/dropbear/ 2>/dev/null; chmod 600 /data/dropbear/*'
rcmd 'printf "#!/bin/sh\n/data/bin/dropbear -p 2222 -r /etc/dropbear/dropbear_ed25519_host_key -r /etc/dropbear/dropbear_rsa_host_key\n" > /data/local/tmp/start_dropbear.sh && chmod +x /data/local/tmp/start_dropbear.sh'
ok "ssh keys, host keys, startup script in place"

# ── 2. rc.local: service lines present, usb_op writes removed ────────────
rcmd '
grep -qF "start_zte_agent.sh" /etc/rc.local || sed -i "/^exit 0/i sh /data/local/tmp/start_zte_agent.sh" /etc/rc.local
grep -qF "start_dropbear.sh" /etc/rc.local || sed -i "/^exit 0/i sh /data/local/tmp/start_dropbear.sh" /etc/rc.local
# remove only usb_op WRITE lines (echo 1 > ...usb_op); the stock flash-protect
# block READS usb_op and must stay (deleting it breaks rc.local syntax)
sed -i "/^echo [0-9] > .*usb_op/d" /etc/rc.local
sh -n /etc/rc.local'
ok "rc.local: agent+dropbear lines present, usb_op writes gone, syntax OK"

# ── 3. dashboard uhttpd instance ─────────────────────────────────────────
rcmd 'uci -q get uhttpd.dashboard >/dev/null 2>&1 || {
  uci set uhttpd.dashboard=uhttpd
  uci set uhttpd.dashboard.listen_http="0.0.0.0:8080"
  uci set uhttpd.dashboard.home="/data/www"
  uci set uhttpd.dashboard.no_dirlists="1"
  uci commit uhttpd
}; /etc/init.d/uhttpd restart 2>/dev/null; true'
ok "dashboard instance on :8080"

# ── 4. auto-update OFF ───────────────────────────────────────────────────
rcmd 'ubus call zwrt_zte_dm set_update_mode "{\"dm_update_mode\":\"0\"}" >/dev/null 2>&1; uci get zwrt_zte_dm.dm_update.dm_update_mode' | grep -q 0 \
  && ok "FOTA auto-update disabled" || warn "could not confirm dm_update_mode=0 — check manually"

# ── 5. start dropbear now + verify ssh ───────────────────────────────────
rcmd 'pidof dropbear >/dev/null 2>&1 || sh /data/local/tmp/start_dropbear.sh'
sleep 2
if $SSH 'echo ok' >/dev/null 2>&1; then
  ok "SSH verified: ssh -p 2222 root@$GW"
else
  warn "SSH not yet reachable (firewall may need a reload, or reboot once)"
fi

echo ""
ok "Hardening complete. Every boot = stock ECM tethering + agent :9090 + ssh :2222."
echo "    Dashboard: http://$GW:8080   (deploy with: bash deploy-dashboard.sh)"
echo "    ADB on demand: ssh -p 2222 root@$GW 'echo 1 > /sys/class/android_usb/android0/usb_op' + reboot"
if [ "$CH" = adb ]; then
  echo ""
  echo "Reboot now to drop the ADB composition and return USB tethering? [y/N]"
  read -r ANS
  if [ "$ANS" = y ] || [ "$ANS" = Y ]; then
    adb reboot
    echo "Rebooting — ~90s. Verify afterwards: ping $GW, then ssh -p 2222 root@$GW"
  fi
fi
