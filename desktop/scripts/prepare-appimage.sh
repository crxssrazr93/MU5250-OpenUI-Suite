#!/usr/bin/env bash
#
# Make `tauri build` able to produce an AppImage on this machine.
#
# Tauri downloads linuxdeploy and its GTK plugin into ~/.cache/tauri the first
# time it bundles an AppImage, then reuses them forever. Two things go wrong
# there, and both present as the same unhelpful line:
#
#     failed to bundle project: `failed to run linuxdeploy`
#
# 1. A truncated download is cached and never retried. The copies here were
#    16 KB against a real 19.8 MB, and a 16 KB linuxdeploy exits 1 with no
#    output at all, which is why the error says nothing. Tauri does not check
#    the size, so it fails identically forever.
#
# 2. The GTK plugin runs a *recursive* find over the pkg-config libdir, so on
#    a machine with VMware installed it picks up /usr/lib/vmware/lib/…, whose
#    bundled libgdk_pixbuf needs libcroco-0.6.so.3 — a library that has not
#    shipped in years. It then fails on the missing dependency. The plugin
#    already passes --exclude-library="*vmware*", but that only filters
#    transitively-resolved dependencies, not paths it passed explicitly.
#
# Both are fixed here rather than in a comment, because the cache is outside
# the repository and a fresh machine will hit them again.
#
# Idempotent: safe to re-run, and re-running after Tauri refreshes the cache is
# the point.

set -euo pipefail

CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/tauri"
mkdir -p "$CACHE"

LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
PLUGIN_URL="https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage"

# A real one is tens of megabytes. Anything near a few hundred KB is a
# truncated download or an error page, whatever its name says.
MIN_BYTES=1000000

ensure_tool() {
    local path="$1" url="$2" name="$3"
    local size=0
    [ -f "$path" ] && size=$(wc -c < "$path")

    if [ "$size" -ge "$MIN_BYTES" ]; then
        echo "  $name: ok ($((size / 1000000)) MB)"
        return
    fi

    if [ "$size" -gt 0 ]; then
        echo "  $name: cached copy is only $size bytes — truncated, replacing"
    else
        echo "  $name: missing, downloading"
    fi
    curl -fsSL -o "$path.tmp" "$url"
    local got
    got=$(wc -c < "$path.tmp")
    if [ "$got" -lt "$MIN_BYTES" ]; then
        rm -f "$path.tmp"
        echo "  $name: download was only $got bytes, giving up" >&2
        exit 1
    fi
    mv "$path.tmp" "$path"
    chmod +x "$path"
    echo "  $name: downloaded ($((got / 1000000)) MB)"
}

echo "Checking AppImage tooling in $CACHE"
ensure_tool "$CACHE/linuxdeploy-x86_64.AppImage" "$LINUXDEPLOY_URL" "linuxdeploy"
ensure_tool "$CACHE/linuxdeploy-plugin-appimage.AppImage" "$PLUGIN_URL" "appimage plugin"

# The GTK plugin arrives with the rest and is a shell script, so it is patched
# in place. Matching on the exact find line rather than a line number: if
# upstream changes it, this stops applying instead of corrupting the script.
GTK_PLUGIN="$CACHE/linuxdeploy-plugin-gtk.sh"
if [ -f "$GTK_PLUGIN" ]; then
    if grep -q "path '\*/vmware/\*' -prune" "$GTK_PLUGIN"; then
        echo "  gtk plugin: already pruning vendor library trees"
    elif grep -qF 'done < <(find "$directory" \( -type l -o -type f \) -name "$library" -print0)' "$GTK_PLUGIN"; then
        cp "$GTK_PLUGIN" "$GTK_PLUGIN.orig"
        python3 - "$GTK_PLUGIN" <<'PATCH'
import sys
path = sys.argv[1]
source = open(path).read()
old = 'done < <(find "$directory" \\( -type l -o -type f \\) -name "$library" -print0)'
new = ("done < <(find \"$directory\" -path '*/vmware/*' -prune -o "
       "\\( -type l -o -type f \\) -name \"$library\" -print0)")
open(path, "w").write(source.replace(old, new))
PATCH
        echo "  gtk plugin: patched to skip /usr/lib/vmware (original kept as .orig)"
    else
        echo "  gtk plugin: find line not recognised — upstream changed it, patch skipped" >&2
        echo "    if the build fails on a vendored libgdk_pixbuf, that is why" >&2
    fi
else
    echo "  gtk plugin: not cached yet; run a build once, then re-run this"
fi

# linuxdeploy and the plugin are themselves AppImages, so running them needs
# FUSE unless told to extract instead. Containers and sandboxes rarely have it.
echo
echo "Now run:  APPIMAGE_EXTRACT_AND_RUN=1 npm run build"
