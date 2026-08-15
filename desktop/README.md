# Desktop app

A Tauri shell around the same React dashboard the agent serves from the router.
There is no second frontend: `web-app` is the frontend, built by the Tauri build
and embedded. Everything the dashboard knows about the firmware stays in one
place, and a fix to a screen fixes it in the browser and on the desktop at once.

## Why it is not just a window pointed at the router

Two things differ once the page is not served by the router.

**There is no address to inherit.** In a browser the agent is on the host that
served the page. A desktop window has no such host, so the address is asked for
once and kept in `localStorage`.

**The agent will not accept the window's origin.** The agent only allows
cross-origin requests from LAN addresses, and a desktop origin is not one. That
check is worth keeping exactly as it is — relaxing it to admit a desktop app
would admit other pages too. So the desktop build issues its requests from Rust
through `tauri-plugin-http` instead of from the webview. A request made outside
a browser has no origin to police, which goes around CORS rather than weakening
it.

`web-app/src/data/host.ts` holds both decisions. In a browser it resolves to the
page's own host and the browser's `fetch`; under Tauri, to the stored address
and the plugin's. Nothing else in the dashboard knows the difference.

## Which addresses it may reach

Two layers, because neither is sufficient alone.

The Tauri capability (`src-tauri/capabilities/default.json`) limits the HTTP
plugin to plain HTTP on port 9090. It cannot be narrower: Tauri matches these
with `urlpattern`, and the Rust implementation will not match a wildcard across
a dot. `http://192.168.*.*:9090/*` matches nothing at all, and `192.168.*` does
not match `192.168.0.1` either — verified against `urlpattern` 0.3.0, which is
where the browser implementation and this one part company. Only an exact host
or a bare `*` works, and the host is not known until the user types it.

So the real restriction is applied to the address itself, in `isPrivateAddress`:
loopback or RFC1918, the same rule the agent applies to origins. It is checked
when the address is entered and again when it is read back from storage.

Note the scope also has to sit on `http:allow-fetch` rather than on the
`http:default` set. `fetch` reads it as a `CommandScope`, and entries attached
to the set never reach it — every request comes back "url not allowed on the
configured scope".

## Building

Needs the Rust toolchain, Node, and on Linux `webkit2gtk-4.1` plus `librsvg`.

```sh
npm install
npm run dev      # vite dev server + the shell, hot reload
npm run build    # builds web-app, then bundles
```

The build produces a 7.8 MB binary at
`src-tauri/target/release/mu5250-openui-desktop` and a 3.5 MB `.deb`.

**The binary is not self-contained.** It links webkit2gtk-4.1, gtk-3, libsoup-3
and javascriptcoregtk-4.1 from the system, which the `.deb` declares as
`libwebkit2gtk-4.1-0, libgtk-3-0`. On a distribution that packages those — Arch
and its derivatives included — running the binary directly is the simplest
option, and it is what was tested here. On anything else, install those first.

### AppImage: works, deliberately off

`"appimage"` is not in `bundle.targets`. It builds and runs — it was produced,
launched and signed in to against the live agent — but the GTK plugin bundles
the whole GTK and WebKit stack, so it comes out at **102 MB against the deb's
3.5 MB**, thirty times the size to solve a dependency problem that Arch does
not have. Add `"appimage"` back to `bundle.targets` and use `npm run
build:linux` if you want it.

### Why the AppImage build fails on a fresh machine

`failed to bundle project: 'failed to run linuxdeploy'` is the only thing Tauri
says, whatever the actual cause. `scripts/prepare-appimage.sh` fixes the two
causes seen here, and `npm run build:linux` runs it first. Both live in
`~/.cache/tauri`, outside the repo, so a fresh machine hits them again.

**A truncated download, cached forever.** Tauri fetches linuxdeploy once and
reuses it without checking it. The copy here was 16 KB against a real 19.8 MB,
and a 16 KB linuxdeploy exits 1 printing nothing — which is exactly why the
error message is empty. The script size-checks and re-fetches.

**The GTK plugin finding VMware's libraries.** It runs a *recursive* `find` over
the pkg-config libdir, so on a machine with VMware installed it picks up
`/usr/lib/vmware/lib/…`, whose bundled `libgdk_pixbuf` wants
`libcroco-0.6.so.3` — gone from distributions years ago. The plugin already
passes `--exclude-library="*vmware*"`, but that only filters transitively
resolved dependencies, not paths it passed explicitly, so the script prunes the
directory from the `find` instead. It matches on the exact line and skips
quietly if upstream changes it, rather than corrupting the script.

linuxdeploy is itself an AppImage, so running it wants FUSE;
`APPIMAGE_EXTRACT_AND_RUN=1` avoids that and `build:linux` sets it.

Kept even though the target is off, because the two failures cost an afternoon
to identify and neither is discoverable from the error message.

## Testing it without a router on your desk

The window can be driven on a virtual display, which is how the flow above was
verified end to end:

```sh
Xvfb :97 -screen 0 1280x900x24 &
DISPLAY=:97 GDK_BACKEND=x11 WEBKIT_DISABLE_COMPOSITING_MODE=1 \
  LIBGL_ALWAYS_SOFTWARE=1 ./mu5250-openui-desktop &
DISPLAY=:97 xdotool mousemove 560 437 click 1
DISPLAY=:97 import -window root shot.png
```

Release builds have no devtools, so a failing request shows up only as the
message on screen. `client.ts` deliberately appends the underlying reason to it:
in a browser that is always an opaque network error, but here it is the Rust
side talking, and it is the difference between "cannot reach the agent" and
"url not allowed on the configured scope".
