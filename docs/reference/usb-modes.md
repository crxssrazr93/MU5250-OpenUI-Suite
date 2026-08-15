# USB Modes: ZTE U60 Pro

These are findings from probing the live device (firmware as shipped,
2026-05-28). They contradict what the dashboard's "USB Mode" section implies. So
they are worth keeping for reference.

## What the dashboard offers

`web-app/src/pages/SettingsPage.tsx` (`UsbModeSection`) presents four buttons:

- **RNDIS**. Microsoft USB networking. It is Windows-native. It needs
  unmaintained drivers on macOS.
- **ECM** (CDC-ECM). Driver-free on macOS, Linux and modern Windows.
- **NCM** (CDC-NCM). Driver-free on the same platforms. It has a much higher
  throughput ceiling than ECM.
- **DEBUG**. Inferred to be ADB or serial console, not a tethering mode.

Each button posts `{"mode": "<name>"}` to `PUT /api/usb/mode`. That forwards
verbatim to `ubus call zwrt_bsp.usb set`.

## What the firmware actually supports via the stock switch

ZTE's normal USB mode switch exposes only **ECM** and **RNDIS**. The evidence:

- `/lib/modules/ipa/` contains exactly two accelerated USB networking modules:
  ```
  ecmipam.ko
  rndisipam.ko
  ```
  No `ncmipam.ko` or equivalent GSI/IPA NCM module ships with this firmware.
- `dmesg` at boot:
  ```
  [ZTE_USB] functions ecm_gsi,mass_storage
  ```
  The composite device enumerates only ECM (plus mass storage), whatever "mode"
  the dashboard requests.
- After clicking **NCM** in the dashboard and rebooting, the device came up with
  `ecm0` as the active interface (MAC `5c:7d:ae:cb:d5:46`, IP allocator giving
  Mac `192.168.0.164`). No `ncm0` interface was created. The firmware silently
  fell back to ECM.
- Live configfs does contain a generic NCM gadget function:
  ```
  /sys/kernel/config/usb_gadget/g1/functions/ncm.0
  ```
  But the active composition links `gsi.ecm`. `/sbin/usb/compositions/usb_switch`
  has cases for `ecm`, `rndis_gsi`, and related functions, not `ncm`.

So:

| Dashboard button | What happens |
|---|---|
| RNDIS | Real — uses `rndisipam.ko`, creates an RNDIS gadget. |
| ECM | Real — uses `ecmipam.ko`, creates `ecm0`. |
| NCM | Stock path falls back to ECM. Generic `ncm.0` exists, but needs a custom experimental composition and bridge setup. |
| DEBUG | Unverified by network probe. Likely ADB/serial, not tethering. |

## Why NCM matters (and why this limitation is annoying)

NCM batches multiple Ethernet frames per USB transfer (NTB). ECM does one per
transfer. The practical ceiling differences:

| Link rate | ECM | NCM |
|---|---|---|
| < 200 Mbps | Fine | Fine |
| 200–400 Mbps | Starts bottlenecking on slower CPUs | Smooth |
| 400 Mbps – 1 Gbps | Caps / high CPU | Handles cleanly |
| > 1 Gbps (5G NR peaks) | Won't deliver | Required |

For typical 5G NR throughput (300–800 Mbps), NCM would meaningfully outperform
ECM if the generic gadget path can be bridged cleanly. This modem wastes its 5G
capability over ECM at the high end. An accelerated NCM equivalent to
`ecmipam.ko` or `rndisipam.ko` would need firmware or kernel work. But a
non-accelerated configfs NCM proof-of-concept is possible.

## Detecting the *current* mode reliably

`ubus call zwrt_bsp.usb list` returns:
```json
{ "mode": "user", "typec_cc": "cc2", "connect": 1, "usb2rj45": 0 }
```

The `mode` field reads `"user"` whatever function is active. It is a permission or
owner state, not the USB function. Do not trust it for "what mode am I in".

For reliable detection from the device side, check which interface exists under
`/sys/class/net/`:

- `ecm0` present → ECM active
- `rndis0` present → RNDIS active
- `ncm.0` linked in `/sys/kernel/config/usb_gadget/g1/configs/c.1/` plus a live
  NCM netdev (`ncm0` or `usb0`) → experimental NCM active

From the Mac side, `route -n get 192.168.0.1` shows the interface.
`networksetup -listallhardwareports` identifies it as "ZTE Mobile Broadband"
whatever the mode. So the Mac side does not distinguish them either.

## Implications for the dashboard

- The "active mode" indicator should read active configfs links and kernel-side
  interface presence, not the ubus `mode` field.
- Label NCM as experimental. Configfs support exists, but stock ZTE mode
  switching does not expose it.
- NCM persistence is agent-managed. The persisted key (`usb_default_mode`) lives
  in `/data/local/tmp/usb_config.json`, separate from the Wi-Fi boot-state
  snapshot. On first read after upgrade, the agent migrates the key out of
  `/data/local/tmp/wifi_config.json` automatically. The agent applies `ncm.0`
  after the stock USB stack has settled. So boot stays recoverable through the
  stock ECM path first.
- DEBUG warrants its own probe before you treat it as a tethering option.
- Best default for Mac USB-C users: ECM. It is the most reliable driver-free
  option here.

## Aside: don't use `usb_mode_set` as a probe

Calling `ubus call zwrt_bsp.usb set {"mode": "..."}` in a loop to detect
supported modes will switch the USB function. It drops any USB-tethered
connection, as happened during this investigation. Probe the kernel side instead.
