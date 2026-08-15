#!/bin/bash
set -e

# ── Colors ───────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
info()  { echo -e "${CYAN}[*]${NC} $1"; }
ok()    { echo -e "${GREEN}[+]${NC} $1"; }
warn()  { echo -e "${YELLOW}[!]${NC} $1"; }
fail()  { echo -e "${RED}[-]${NC} $1" >&2; exit 1; }

# ── Password input ──────────────────────────────────────────────────
if [ $# -ge 2 ]; then
    ROUTER_PASSWORD="$1"
    AGENT_PASSWORD="$2"
elif [ $# -eq 0 ]; then
    echo -e "${CYAN}Router admin password:${NC} "
    read -rs ROUTER_PASSWORD; echo
    echo -e "${CYAN}Agent API password:${NC} "
    read -rs AGENT_PASSWORD; echo
    [ -z "$ROUTER_PASSWORD" ] && fail "Router password cannot be empty."
    [ -z "$AGENT_PASSWORD" ] && fail "Agent password cannot be empty."
else
    echo "Usage: ./setup.sh [router-password agent-password]"
    echo "       ./setup.sh    (interactive — prompts for passwords)"
    exit 1
fi

GATEWAY=192.168.0.1
AGENT_PORT=9090
SSH_PORT=2222
TARGET=aarch64-unknown-linux-musl
BINARY=target/$TARGET/release/zte-agent
REMOTE_BIN=/data/zte-agent
STARTUP_SCRIPT=/data/local/tmp/start_zte_agent.sh
BINARY_CHANGED=false
DOWNLOAD_URL="https://github.com/dklasens/MU5250-OpenUI/releases/latest/download/zte-agent"

# ── Binary source menu ──────────────────────────────────────────────
# Build-from-source (2) is the default: this fork's agent has diverged from
# upstream (batched /api/dashboard, charge control, powerbank, …) and the
# dashboard in this repo expects those. Option 1 downloads the UPSTREAM
# agent, which will leave parts of the new dashboard empty.
echo ""
echo -e "${CYAN}How would you like to get the zte-agent binary?${NC}"
echo "  1) Download pre-built upstream from GitHub (no dev tools needed, but"
echo "     may lack this fork's endpoints and leave parts of the dashboard empty)"
echo "  2) Build from source (recommended — matches this fork's dashboard;"
echo "     requires Rust toolchain)"
echo ""
echo -n "Choice [2]: "
read -r BUILD_CHOICE
BUILD_CHOICE="${BUILD_CHOICE:-2}"

if [ "$BUILD_CHOICE" != "1" ] && [ "$BUILD_CHOICE" != "2" ]; then
    fail "Invalid choice. Enter 1 or 2."
fi

# ── Step 0: Prerequisites ───────────────────────────────────────────
info "Checking prerequisites..."

OS="$(uname -s)"
HAS_BREW=false
HAS_APT=false
command -v brew >/dev/null 2>&1 && HAS_BREW=true
command -v apt-get >/dev/null 2>&1 && HAS_APT=true

# The linker to check for is whatever agent/.cargo/config.toml names for the
# musl target — that file is what cargo actually obeys. Checking for anything
# else passes the prerequisite step and then fails at link time, which is what
# the old hardcoded "aarch64-linux-gnu-gcc" did on Linux: the build links with
# musl on every platform, so the gnu cross-compiler was never the right thing
# to look for.
CROSS_CC="$(sed -n 's/^ *linker *= *"\(.*\)"/\1/p' agent/.cargo/config.toml 2>/dev/null | head -1)"
CROSS_CC="${CROSS_CC:-aarch64-linux-musl-gcc}"

# Dependency table: cmd | brew_pkg | apt_pkg | custom_install
# Build path needs all deps; download path only needs curl, python3, adb
if [ "$BUILD_CHOICE" = "1" ]; then
    DEPS=(
        "curl|curl|curl|"
        "python3|python3|python3|"
        "adb|android-platform-tools|android-tools-adb|"
        "openssl|openssl|openssl|"
    )
else
    DEPS=(
        "curl|curl|curl|"
        "python3|python3|python3|"
        "adb|android-platform-tools|android-tools-adb|"
        "openssl|openssl|openssl|"
        "cargo|||curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
        "$CROSS_CC|filosottile/musl-cross/musl-cross|gcc-aarch64-linux-gnu musl-tools|"
    )
fi

MISSING_CMDS=()
MISSING_INSTALLS=()
ALL_OK=true

# Returns 0 if every missing tool has a resolved install command
all_have_installers() {
    for i in "${!MISSING_CMDS[@]}"; do
        [ -z "${MISSING_INSTALLS[$i]}" ] && return 1
    done
    return 0
}

for entry in "${DEPS[@]}"; do
    IFS='|' read -r cmd brew_pkg apt_pkg custom <<< "$entry"
    if command -v "$cmd" >/dev/null 2>&1; then
        echo -e "  ${GREEN}✓${NC} $cmd"
    else
        echo -e "  ${RED}✗${NC} $cmd"
        ALL_OK=false
        MISSING_CMDS+=("$cmd")

        # Determine install command
        install_cmd=""
        if [ -n "$custom" ]; then
            install_cmd="$custom"
        elif [ "$OS" = "Darwin" ] && [ "$HAS_BREW" = true ] && [ -n "$brew_pkg" ]; then
            install_cmd="brew install $brew_pkg"
        elif [ "$OS" != "Darwin" ] && [ "$HAS_APT" = true ] && [ -n "$apt_pkg" ]; then
            install_cmd="sudo apt-get install -y $apt_pkg"
        fi
        MISSING_INSTALLS+=("$install_cmd")
    fi
done

# For download path, ADB is only needed if SSH isn't set up — defer check
# (we'll check SSH reachability later and skip ADB requirement if SSH works)

if [ "$ALL_OK" = false ]; then
    # On macOS without brew, offer to install it first
    if [ "$OS" = "Darwin" ] && [ "$HAS_BREW" = false ]; then
        echo ""
        warn "Homebrew not found. It's needed to install dependencies on macOS."
        echo -e "${CYAN}Install Homebrew now? (y/N)${NC}"
        read -r INSTALL_BREW
        if [ "$INSTALL_BREW" = "y" ] || [ "$INSTALL_BREW" = "Y" ]; then
            /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
            # Source brew shellenv for current session
            if [ -x /opt/homebrew/bin/brew ]; then
                eval "$(/opt/homebrew/bin/brew shellenv)"
            elif [ -x /usr/local/bin/brew ]; then
                eval "$(/usr/local/bin/brew shellenv)"
            fi
            HAS_BREW=true
            # Re-evaluate install commands now that brew is available
            for i in "${!MISSING_CMDS[@]}"; do
                if [ -z "${MISSING_INSTALLS[$i]}" ]; then
                    for entry in "${DEPS[@]}"; do
                        IFS='|' read -r cmd brew_pkg _ _ <<< "$entry"
                        if [ "$cmd" = "${MISSING_CMDS[$i]}" ] && [ -n "$brew_pkg" ]; then
                            MISSING_INSTALLS[$i]="brew install $brew_pkg"
                            break
                        fi
                    done
                fi
            done
        else
            fail "Homebrew is required on macOS. Install it from https://brew.sh"
        fi
    fi

    if ! all_have_installers; then
        echo ""
        echo "Cannot auto-install all dependencies. Please install manually:"
        for i in "${!MISSING_CMDS[@]}"; do
            if [ -z "${MISSING_INSTALLS[$i]}" ]; then
                echo "  ${MISSING_CMDS[$i]} — no auto-install available for this platform"
            fi
        done
        exit 1
    fi

    echo ""
    echo "The following will be installed:"
    for i in "${!MISSING_CMDS[@]}"; do
        echo "  ${MISSING_CMDS[$i]}  →  ${MISSING_INSTALLS[$i]}"
    done
    echo ""
    echo -e "${CYAN}Install all missing dependencies now? (y/N)${NC}"
    read -r DO_INSTALL
    if [ "$DO_INSTALL" != "y" ] && [ "$DO_INSTALL" != "Y" ]; then
        fail "Missing dependencies: ${MISSING_CMDS[*]}. Install them and re-run."
    fi

    for i in "${!MISSING_CMDS[@]}"; do
        info "Installing ${MISSING_CMDS[$i]}..."
        if ! eval "${MISSING_INSTALLS[$i]}"; then
            fail "Failed to install ${MISSING_CMDS[$i]}."
        fi
        # Source cargo env if we just installed rustup
        if [ "${MISSING_CMDS[$i]}" = "cargo" ] && [ -f "$HOME/.cargo/env" ]; then
            # shellcheck disable=SC1091
            source "$HOME/.cargo/env"
        fi
        if command -v "${MISSING_CMDS[$i]}" >/dev/null 2>&1; then
            ok "${MISSING_CMDS[$i]} installed."
        else
            fail "Failed to install ${MISSING_CMDS[$i]}."
        fi
    done
fi

# Ensure Rust cross-compilation target is installed (build path only)
if [ "$BUILD_CHOICE" = "2" ]; then
    if ! rustup target list --installed 2>/dev/null | grep -q aarch64-unknown-linux-musl; then
        info "Adding Rust cross-compilation target..."
        rustup target add aarch64-unknown-linux-musl
    fi
fi
ok "All prerequisites found."

# ── Helper: SHA-256 (file) ───────────────────────────────────────────
sha256_file() {
    shasum -a 256 "$1" 2>/dev/null | awk '{print $1}' \
        || sha256sum "$1" | awk '{print $1}'
}

# ── Helper: timeout (macOS-compatible) ───────────────────────────────
# macOS doesn't have GNU timeout; use a portable fallback
wait_with_timeout() {
    local secs="$1"; shift
    if command -v timeout >/dev/null 2>&1; then
        timeout "$secs" "$@"
    else
        # Portable fallback: run in background with a timer
        "$@" &
        local pid=$!
        local i=0
        while [ "$i" -lt "$secs" ]; do
            if ! kill -0 "$pid" 2>/dev/null; then
                wait "$pid"
                return $?
            fi
            sleep 1
            i=$((i + 1))
        done
        kill "$pid" 2>/dev/null
        return 124
    fi
}

# ── Transport detection ──────────────────────────────────────────────
SSH_CMD="ssh -p $SSH_PORT -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=$HOME/.ssh/known_hosts.d/zte -o ConnectTimeout=3 root@$GATEWAY"
USE_SSH=false

if $SSH_CMD "echo ok" >/dev/null 2>&1; then
    USE_SSH=true
    ok "SSH reachable — using wireless deploy."
else
    warn "SSH not reachable — falling back to ADB."

# ── Steps 1-2: Enable ADB + connect ──────────────────────────────────
if adb devices 2>/dev/null | grep -qw device; then
    ok "ADB already connected, skipping unlock."
else
    # ── Unlock: config backup/restore (B04+ locked firmware) ─────────
    # The old `zwrt_bsp.usb set {mode:debug}` primitive was removed in
    # HK B04 / CN B28, so ADB can no longer be switched on over the web.
    # The way back in is the device's own config backup/restore path:
    # zunlock.py downloads a fresh encrypted backup, injects a boot-time
    # usb_op payload into its rc.local, repacks it md5-valid, re-encrypts
    # and restores it. The device reboots and comes back with adbd up.
    # Backup password = <IMEI><suffix>; the IMEI is read from the device,
    # only the platform suffix (passphrase) is supplied by you. No secrets
    # are hardcoded here — see docs/DEPLOYMENT.md.
    info "No ADB and no SSH — device looks locked (B04+)."
    info "Unlock method: config backup/restore via scripts/zunlock.py."
    echo ""
    warn "This restores a patched config backup: current settings are"
    warn "overwritten and the device reboots (~90 s). On a fresh device"
    warn "there is nothing to lose."
    echo ""

    SUFFIX="${ZTE_BACKUP_SUFFIX:-}"
    if [ -z "$SUFFIX" ]; then
        echo -e "${CYAN}Backup-key suffix / passphrase:${NC} "
        read -rs SUFFIX; echo
        [ -z "$SUFFIX" ] && fail "Backup-key suffix cannot be empty."
    fi

    # Stage 1 — dry run: login, fetch backup, decrypt, patch, repack, all
    # WITHOUT uploading. Catches a wrong suffix before touching the device.
    info "Stage 1/2 — dry run (makes no changes to the device)..."
    if ! ROUTER_PW="$ROUTER_PASSWORD" ZTE_BACKUP_SUFFIX="$SUFFIX" \
        python3 scripts/zunlock.py --gw "$GATEWAY" --dry-run; then
        fail "Unlock dry-run failed — check the output above (wrong suffix?)."
    fi
    ok "Dry-run passed — backup decrypts and patches cleanly."

    # Stage 2 — real unlock. zunlock.py keeps its own "Proceed? [y/N]"
    # consent gate immediately before the upload + restore + reboot.
    info "Stage 2/2 — unlock (upload + restore + reboot)..."
    if ! ROUTER_PW="$ROUTER_PASSWORD" ZTE_BACKUP_SUFFIX="$SUFFIX" \
        python3 scripts/zunlock.py --gw "$GATEWAY"; then
        fail "Unlock failed — check the output above."
    fi

    info "Waiting for the device to reboot and ADB to come up (~90 s)..."
    if ! wait_with_timeout 180 adb wait-for-device 2>/dev/null; then
        fail "ADB device not found after unlock. Check the USB cable, then re-run."
    fi
    ok "ADB device connected — unlock complete."
fi
fi

# ── Helper: remote command / push ────────────────────────────────────
rcmd() {
    if [ "$USE_SSH" = true ]; then
        $SSH_CMD "$@"
    else
        adb shell "$@"
    fi
}

# rcmd_check: like rcmd but reliably returns the remote exit code.
# adb shell does not propagate exit codes on macOS, so we embed a sentinel.
rcmd_check() {
    if [ "$USE_SSH" = true ]; then
        $SSH_CMD "$@"
    else
        adb shell "($*) && echo __ADB_OK__" 2>/dev/null | grep -q "__ADB_OK__"
    fi
}

rpush() {
    local src="$1" dst="$2"
    if [ "$USE_SSH" = true ]; then
        cat "$src" | $SSH_CMD "cat > $dst && chmod +x $dst"
    else
        adb push "$src" "$dst" && adb shell "chmod +x $dst"
    fi
}

# ── Step 3: Get zte-agent binary ─────────────────────────────────────
if [ "$BUILD_CHOICE" = "1" ]; then
    info "Downloading zte-agent from GitHub releases..."
    mkdir -p "$(dirname "$BINARY")"
    if ! curl -sfL "$DOWNLOAD_URL" -o "$BINARY"; then
        fail "Download failed. Check your internet connection or try: $DOWNLOAD_URL"
    fi
    # Verify download
    FILE_SIZE=$(wc -c < "$BINARY" | tr -d ' ')
    if [ "$FILE_SIZE" -lt 1000 ]; then
        rm -f "$BINARY"
        fail "Downloaded file is too small ($FILE_SIZE bytes) — likely not a valid binary. Check the release page."
    fi
    chmod +x "$BINARY"
    ok "Downloaded zte-agent ($FILE_SIZE bytes)."
else
    info "Building zte-agent (this may take a few minutes on first run)..."
    cargo build --release --target "$TARGET" -p zte-agent
    ok "Build complete."
fi

# ── Step 4: Push binary ─────────────────────────────────────────────
info "Checking zte-agent binary..."
LOCAL_SHA=$(sha256_file "$BINARY")
REMOTE_SHA=$(rcmd "sha256sum $REMOTE_BIN 2>/dev/null" | awk '{print $1}')
if [ "$LOCAL_SHA" = "$REMOTE_SHA" ]; then
    ok "Binary unchanged, skipping push."
else
    info "Stopping running agent before push..."
    rcmd "killall zte-agent 2>/dev/null; sleep 1"
    info "Pushing zte-agent to device..."
    rpush "$BINARY" "$REMOTE_BIN"
    BINARY_CHANGED=true
    ok "Binary deployed to $REMOTE_BIN."
fi

# ── Step 5: Create startup script ───────────────────────────────────
# Escape single quotes for safe embedding in sh single-quoted string
SAFE_PASSWORD=$(printf '%s' "$AGENT_PASSWORD" | sed "s/'/'\\\\''/g")

rcmd "mkdir -p $(dirname $STARTUP_SCRIPT)"
# Also require the syslog line, so a script written by an older setup.sh (which
# redirected into /tmp and grew in RAM forever) gets rewritten rather than kept.
if rcmd_check "grep -qF '${SAFE_PASSWORD}' $STARTUP_SCRIPT 2>/dev/null && grep -qF 'logger -t zte-agent' $STARTUP_SCRIPT 2>/dev/null"; then
    ok "Startup script already up to date."
else
    info "Creating startup script..."
    cat > /tmp/start_zte_agent.sh <<BOOT
#!/bin/sh
export ZTE_AGENT_PASSWORD='${SAFE_PASSWORD}'
# Log via syslog (logd's fixed-size ring buffer) rather than a file on /tmp:
# /tmp is tmpfs, so a plain redirect grows in RAM forever with no rotation.
# Read it back with: logread -e zte-agent
nohup sh -c '/data/zte-agent 2>&1 | logger -t zte-agent' >/dev/null 2>&1 </dev/null &
BOOT
    rpush /tmp/start_zte_agent.sh "$STARTUP_SCRIPT"
    rm /tmp/start_zte_agent.sh
    ok "Startup script created at $STARTUP_SCRIPT."
fi

# ── Step 6: Update rc.local for boot persistence ────────────────────
info "Configuring auto-start on boot..."
RC_LINE="sh $STARTUP_SCRIPT"
if rcmd_check "grep -qF '$RC_LINE' /etc/rc.local 2>/dev/null"; then
    ok "rc.local already configured."
else
    rcmd "grep -q '^exit 0' /etc/rc.local \
        && sed -i '/^exit 0/i $RC_LINE' /etc/rc.local \
        || echo '$RC_LINE' >> /etc/rc.local"
    ok "Added zte-agent to /etc/rc.local."
fi

# ── Step 7: Start agent ─────────────────────────────────────────────
info "Checking agent status..."
AGENT_RUNNING=false
curl -sf "http://$GATEWAY:$AGENT_PORT/api/auth/login" \
    -H 'Content-Type: application/json' \
    -d "{\"password\":\"$AGENT_PASSWORD\"}" >/dev/null 2>&1 && AGENT_RUNNING=true

if [ "$BINARY_CHANGED" = true ] || [ "$AGENT_RUNNING" = false ]; then
    info "Starting zte-agent..."
    rcmd "killall zte-agent 2>/dev/null; true"
    sleep 1
    if [ "$USE_SSH" = true ]; then
        rcmd "sh $STARTUP_SCRIPT"
    else
        adb shell "sh $STARTUP_SCRIPT; sleep 1; pidof zte-agent" | grep -q '[0-9]' \
            || fail "Agent process did not start. Check 'logread -e zte-agent' on device."
    fi
    ok "Agent (re)started."
else
    ok "Agent already running with current binary, skipping restart."
fi

# ── Step 8: Verify ──────────────────────────────────────────────────
info "Verifying agent is running..."
sleep 2

# Always verify over the LAN address. `adb forward` cannot be used here: it
# forwards to *localhost on the device*, and the agent deliberately binds to
# the LAN address only, so nothing is listening there. Using the forward made
# this step fail on every ADB-only install.
VERIFY_URL="http://$GATEWAY:$AGENT_PORT/api/auth/login"

TOKEN=$(curl -sf -m 10 "$VERIFY_URL" \
    -H 'Content-Type: application/json' \
    -d "{\"password\":\"$AGENT_PASSWORD\"}" \
    2>/dev/null | python3 -c 'import sys,json; print(json.load(sys.stdin)["data"]["token"])' 2>/dev/null || true)

if [ -n "$TOKEN" ] && [ "$TOKEN" != "null" ]; then
    ok "Agent is running and authenticated."
elif ping -c 1 -W 2 "$GATEWAY" >/dev/null 2>&1; then
    fail "Agent started but login verification failed at $VERIFY_URL."
else
    # Installing over USB-ADB alone is legitimate, but then this machine has
    # no route to the agent and cannot check it. Say so instead of failing a
    # deployment that actually worked.
    warn "Installed, but $GATEWAY is not reachable from here, so the agent"
    warn "could not be verified. Join the router's network (WiFi, or bring up"
    warn "its USB ethernet interface) and check:"
    warn "  curl -X POST -d '{\"password\":\"...\"}' $VERIFY_URL"
fi

# ── Step 9: SSH for wireless deploys ────────────────────────────────
# Dropbear install deliberately lives in scripts/zharden.sh, not here:
# opkg is unusable on this firmware and the old in-setup install pinned a
# stale package URL and a wrong dropbear path. zharden.sh extracts dropbear
# to /data, generates host keys and wires up rc.local correctly (and also
# adds the dashboard uhttpd instance + turns FOTA auto-update off).
if [ "$USE_SSH" = true ]; then
    ok "SSH already configured."
else
    echo ""
    info "Next: run 'bash scripts/zharden.sh' to install dropbear SSH,"
    info "clean up rc.local, serve the dashboard on :8080 and disable"
    info "FOTA auto-update. Then 'bash deploy-dashboard.sh' to deploy the UI."
fi

# ── Done ─────────────────────────────────────────────────────────────
echo ""
echo -e "${GREEN}Setup complete!${NC}"
echo ""
echo "  Agent API:  http://$GATEWAY:$AGENT_PORT"
echo "  Re-deploy:  ZTE_AGENT_PASSWORD=<your-password> ZTE_AGENT_PIN=<six-digit-pin> ./deploy.sh"
echo ""
echo "  Next steps:"
echo "    1) bash scripts/zharden.sh      # dropbear SSH, rc.local cleanup,"
echo "                                    # dashboard on :8080, FOTA auto-update off"
echo "    2) bash deploy-dashboard.sh     # build + push the web UI to /data/www"
echo ""
echo "  Point the iOS/Android companion app at http://$GATEWAY:$AGENT_PORT"
