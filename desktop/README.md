# Desktop app

This is a Tauri shell around the same React dashboard the agent serves from the
router. There is no second frontend. `web-app` is the frontend. The Tauri build
builds it and embeds it. Everything the dashboard knows about the firmware stays
in one place. A fix to a screen fixes it in the browser and on the desktop at
once.

## Why it is not just a window pointed at the router

Two things change once the router does not serve the page.

**There is no address to inherit.** In a browser, the agent is on the host that
served the page. A desktop window has no such host. So the app asks for the
address once and keeps it in `localStorage`.

**The agent will not accept the window's origin.** The agent allows cross-origin
requests only from LAN addresses. A desktop origin is not one. Keep that check
exactly as it is. If you relax it to admit a desktop app, you admit other pages
too. So the desktop build sends its requests from Rust through
`tauri-plugin-http`, not from the webview. A request made outside a browser has
no origin to police. This goes around CORS. It does not weaken CORS.

`web-app/src/data/host.ts` holds both decisions. In a browser it resolves to the
page's own host and the browser's `fetch`. Under Tauri it resolves to the stored
address and the plugin's request. Nothing else in the dashboard knows the
difference.

## Which addresses it may reach

There are two layers, because neither is enough alone.

The Tauri capability (`src-tauri/capabilities/default.json`) limits the HTTP
plugin to plain HTTP on port 9090. It cannot be narrower. Tauri matches these
with `urlpattern`, and the Rust implementation does not match a wildcard across
a dot. `http://192.168.*.*:9090/*` matches nothing at all. `192.168.*` does not
match `192.168.0.1` either. This was verified against `urlpattern` 0.3.0, which
is where the browser implementation and this one differ. Only an exact host or a
bare `*` works, and the host is not known until the user types it.

So the real restriction applies to the address itself, in `isPrivateAddress`. It
allows loopback or RFC1918, the same rule the agent applies to origins. It is
checked when the address is entered, and again when it is read back from storage.

The scope also has to sit on `http:allow-fetch`, not on the `http:default` set.
`fetch` reads it as a `CommandScope`. Entries attached to the set never reach it.
Every request then comes back "url not allowed on the configured scope".

## Building

You need the Rust toolchain, Node, and on Linux `webkit2gtk-4.1` plus `librsvg`.

```sh
npm install
npm run dev      # vite dev server + the shell, hot reload
npm run build    # builds web-app, then bundles
```

The build produces a 7.8 MB binary at
`src-tauri/target/release/mu5250-openui-desktop` and a 3.5 MB `.deb`.

**The binary is not self-contained.** It links webkit2gtk-4.1, gtk-3, libsoup-3
and javascriptcoregtk-4.1 from the system. The `.deb` declares those as
`libwebkit2gtk-4.1-0, libgtk-3-0`. On a distribution that packages them, which
includes Arch and its derivatives, run the binary directly. That is the simplest
option, and it is the tested one. On any other distribution, install those
first.

### AppImage: works, deliberately off

`"appimage"` is not in `bundle.targets`. It builds and runs. It was produced,
launched, and signed in to against the live agent. But the GTK plugin bundles the
whole GTK and WebKit stack, so it comes out at **102 MB against the deb's 3.5
MB**. That is thirty times the size, to solve a dependency problem that Arch does
not have. To build it, add `"appimage"` back to `bundle.targets` and use `npm run
build:linux`.

### Why the AppImage build fails on a fresh machine

`failed to bundle project: 'failed to run linuxdeploy'` is the only thing Tauri
reports, whatever the real cause. `scripts/prepare-appimage.sh` fixes the two
causes seen here, and `npm run build:linux` runs it first. Both files live in
`~/.cache/tauri`, outside the repo, so a fresh machine meets them again.

**A truncated download, cached forever.** Tauri fetches linuxdeploy once and
reuses it without a check. The copy here was 16 KB against a real 19.8 MB. A 16
KB linuxdeploy exits 1 and prints nothing, which is why the error message is
empty. The script checks the size and re-fetches.

**The GTK plugin finding VMware's libraries.** It runs a *recursive* `find` over
the pkg-config libdir. On a machine with VMware installed it picks up
`/usr/lib/vmware/lib/…`. That bundled `libgdk_pixbuf` wants `libcroco-0.6.so.3`,
which distributions removed years ago. The plugin already passes
`--exclude-library="*vmware*"`, but that filters only transitively resolved
dependencies, not paths it passed explicitly. So the script prunes the directory
from the `find` instead. It matches the exact line and skips quietly when
upstream changes it, rather than corrupt the script.

linuxdeploy is itself an AppImage, so running it wants FUSE.
`APPIMAGE_EXTRACT_AND_RUN=1` avoids that, and `build:linux` sets it.

This is kept even though the target is off. The two failures cost an afternoon to
identify, and neither is discoverable from the error message.

## Testing it without a router on your desk

You can drive the window on a virtual display. This is how the flow above was
verified end to end:

```sh
Xvfb :97 -screen 0 1280x900x24 &
DISPLAY=:97 GDK_BACKEND=x11 WEBKIT_DISABLE_COMPOSITING_MODE=1 \
  LIBGL_ALWAYS_SOFTWARE=1 ./mu5250-openui-desktop &
DISPLAY=:97 xdotool mousemove 560 437 click 1
DISPLAY=:97 import -window root shot.png
```

Release builds have no devtools, so a failing request shows up only as the
message on screen. `client.ts` appends the underlying reason to that message on
purpose. In a browser the reason is always an opaque network error. Here it is
the Rust side talking. It is the difference between "cannot reach the agent" and
"url not allowed on the configured scope".
