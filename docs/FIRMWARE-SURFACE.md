# Firmware surface — what this device exposes and what we use

An inventory of the 132 ubus objects on `XCBZ_HK_MU5250V1.0.0B04`, taken live
from the device (`ubus -v list`), with each entry marked by whether the agent
surfaces it.

The point is to stop rediscovering the same things. Earlier passes over this
firmware missed whole subsystems — the entire VPN tunnel suite, NFC, Samba/DLNA
and the direct-power-supply mode were all present and unexamined.

Read [SAFETY.md](SAFETY.md) before wiring any of the unimplemented ones up.

## Already surfaced by the agent

| Object | Used for |
| --- | --- |
| `zwrt_wms` | SMS list/send/delete/read |
| `zwrt_apn_object` | APN profiles |
| `zwrt_data` | data usage, WWAN interface state |
| `zwrt_wlan`, `zwrt_wlan_adapter` | Wi-Fi status and settings |
| `zwrt_bsp.battery`, `zwrt_bsp.charger` | battery, charging, charge limiting |
| `zwrt_bsp.thermal` | temperatures |
| `zwrt_bsp.usb`, `zwrt_bsp.eth` | USB mode, ethernet |
| `zwrt_bsp.powerbank` | powerbank/OTG state |
| `zwrt_zte_dm` | FOTA settings |
| `zte_nwinfo_api`, `zwrt_zte_mdm.api` | signal, network info |
| `zwrt_router.api` | LAN/DHCP, DNS |
| — (QMI over QRTR, not ubus) | eUICC / eSIM — see [EUICC.md](EUICC.md) |

## Present and not yet surfaced

Everything below was confirmed to exist and answer on this unit.

### Direct power supply mode

The stock app calls this "Power Supply": run from the AC adapter and hold the
battery at 40–60% instead of charging it to full. Two independent paths:

```
zwrt_bsp.charger    set {"direct_power_supply_mode": "enable"|"disable"}
zwrt_deviceui       zwrt_deviceui_direct_power_mode_get / _set
```

Live: `zwrt_deviceui_direct_power_mode_get` → `{"enable": "0"}` (off).

**The naming is inverted and this matters.** `direct_power_supply_mode:
"enable"` means *charging stopped*. `agent/src/charge_policy.rs` already drives
this primitive to implement charge limiting, so a user-facing "Power Supply"
toggle must be reconciled with that rather than added beside it — two features
writing the same switch with opposite intentions will fight.

### USB-C power and data role

```
zwrt_bsp.typec      list                    → power_role, data_role, cc_attch_state
zwrt_bsp.typec      set {"PR_Swap": ..., "DR_Swap": ...}
```

Live: `{"power_role": "sink", "data_role": "device", "cc_attch_state": 1}`.

Reporting the roles is free and safe, and answers two questions a user actually
has: whether the device is charging or being drained, and whether it can see USB
storage. Both belong in the existing USB and battery views.

Neither setter is worth exposing.

`PR_Swap` is redundant: `zwrt_bsp.powerbank set {"state"}` is the vendor's own
path for reverse charging and the agent already reports `otg_powerbank_state`.
Driving the Type-C controller underneath a feature that manages the same thing
invites the two to disagree — the same trap as `direct_power_supply_mode` and
charge control.

`DR_Swap` cuts the branch you are sitting on. Swapping to host while a computer
is attached tears down the ECM gadget, which is the management and deploy path
to `192.168.0.1`. You would disconnect yourself over the API with no way back
except unplugging.

More generally these roles are negotiated by the Type-C port controller from the
cable's Rp/Rd resistors and PD messaging. Manual override is an edge-case tool,
not a feature.

### NFC Wi-Fi credential sharing

```
zwrt_nfc    zwrt_nfc_wifi_get                          → result, switch, flag
zwrt_nfc    zwrt_nfc_wifi_set {"switch": int, "flag": int}
zwrt_nfc    zwrt_nfc_wifi_change / zwrt_nfc_wifi_mesh
```

Live: `{"switch": "1", "flag": "2"}` — already enabled on this unit. Tapping a
phone to the router hands over Wi-Fi credentials.

### Samba and DLNA for USB storage

```
zwrt_samba  get_settings / set_settings {"switch": "0"|"1"}
zwrt_samba  get_usb_info                  → status, size, used, free, filesystem, name
zwrt_samba  dlna_settings {"enabled": ..., "friendly_name": ..., "port": ...}
```

Live: `switch: "0"` (off), no USB storage attached.

**Worth less than it looks.** The unit has a single USB-C port. Sharing a drive
requires the port to be in host mode with the drive attached, so it cannot be
tethered to a computer at the same time — USB storage sharing and USB tethering
are mutually exclusive, not complementary. On a device whose main wired use is
tethering, that is a real limit on how often anyone would reach for this.

It is still usable for someone running the router standalone on Wi-Fi. Rank it
accordingly rather than as a headline feature.

Testing it costs the USB management path. To do it without being cut off: join
the router's own Wi-Fi first (which gives management access independent of USB,
at the cost of internet on that machine, since the router has no WAN), then free
the port.

### The full VPN tunnel suite

Eight tunnel types, each with a `.config` object carrying the settings and a
`handle {"action"}` object to bring it up and down:

| Tunnel | Config fields |
| --- | --- |
| `wireguard` | listen_port, tunnel_ip, peer_public_key, peer_endip, peer_listen_port, peer_tunnel_ip, peer_remote_ip/mask, auto_start — **plus `keygen`** |
| `openvpn` | server address/port, tunnel/proto/auth/cipher/comp types, MTU, credentials, tls_auth, push_route — **plus CA, client cert+key, TA key and pre-shared key upload/read** |
| `ipsec` | server_address, password, remote address/netmask, lifetime, key_exchange, cipher/hash/DH for phase 1 and 2 |
| `l2tp` | server_address, username, tunnel_password, password, hostname |
| `l2tpv3` | local/remote address, tunnel and session ids, encapsulation, UDP ports, VLAN |
| `pptp` | server_address, username, password |
| `gre` | server_address, level, host, remote, remote_ip/mask |
| `vxlan` | vni (×3), remote, dstport, mec_enable, vlanid (×3) |

`zwrt_tunnel.config` has the shared `get`/`list`/`up`/`down`/`reload`/`restart`.

This is the largest single unexploited surface on the device.

Config is UCI-backed at `zwrt_tunnel.<type>.*`, with `zwrt_tunnel.cur_type.type`
naming the active one (`pptp` on a stock unit). Sections exist for pptp, l2tp,
gre, ipsec and vxlan; wireguard, openvpn and l2tpv3 are created on first write.

Note `zwrt_tunnel.config get` returns stored credentials as encrypted blobs
where `list` returns them empty. Anything exposing this must mask them.

PPTP is present but cryptographically broken and should not be exposed.

#### WireGuard works, once one missing file is supplied

Verified on the device:

| Check | Result |
| --- | --- |
| `CONFIG_WIREGUARD` | `=y` — built into the kernel, not a module |
| `ip link add dev X type wireguard` | rc=0, interface created (MTU 1420, POINTOPOINT) |
| `zwrt_tunnel.wireguard.config keygen` | `{"result": "FAILED"}` on a stock unit |
| `wg` binary | **absent** |

The daemon `/usr/bin/zte-topsw-tunnel` implements the whole flow already — its
strings show `wg genkey`, `echo -n %s | wg pubkey`, and it drives
`/sbin/wireguard_conf.sh` and `/sbin/wireguard_client.sh connect|disconnect`,
both of which exist. `wireguard_client.sh` uses `ip link add`, `ip address add`
and `wg setconf`, reads its settings from `uci get zwrt_tunnel.wireguard.*`, and
reports status back via `ubus call zwrt_tunnel.config cb`.

So the only thing missing is the `wg` userspace tool. `zharden.sh` now installs
it to `/data/bin` from the OpenWrt `wireguard-tools` package. Confirmed working
there: `wg --version` runs, and `wg genkey | wg pubkey` produces a valid pair.

It cannot go anywhere on the daemon's own PATH — the rootfs is genuinely
read-only (`/dev/root ... ext4 (ro)`; `/overlay` is a separate mount, not an
overlayfs over `/`), and the daemon inherits
`PATH=/usr/sbin:/usr/bin:/sbin:/bin`. Rather than modify a vendor init script to
extend that — a boot-path change, which [SAFETY.md](SAFETY.md) rules out — the
agent should drive the vendor scripts itself with `PATH=/data/bin:$PATH`. That
reuses all the vendor routing and status logic while touching no vendor file.

Still unproven: an actual tunnel coming up, which needs a peer to connect to.

### Radio control beyond what the agent uses

`zte_nwinfo_api` has 30 methods; the agent calls 9. The rest, confirmed present:

| Method | What it offers |
| --- | --- |
| `nwinfo_manual_scan`, `nwinfo_m_netselect_status/contents/result`, `nwinfo_manual_register {m_mcc_mnc, m_rat}` | scan for operators and register on one manually |
| `nwinfo_start_detect_signal_quality`, `nwinfo_end_...`, `nwinfo_get_progress_and_quality`, `nwinfo_add/delete/modify_item_signal_quality` | a built-in signal-quality survey with stored measurements |
| `nwinfo_set_sa_bandlock {nr5g_sa_band_lock}` | SA band lock, separate from the NSA lock the agent sets |
| `nwinfo_set_sa_celllock {PCI, Arfcn, SCS}` | SA cell lock — the agent's LTE/NR cell locks do not cover SA |
| `nwinfo_lock_nr_cell2 {..., lock_nr_cell_scs}` | NR cell lock **with subcarrier spacing** — strictly better than `nwinfo_lock_nr_cell`, which the agent uses |
| `nwinfo_surge_lock_nr_cell {lock_nr_cellid, ...}` | NR cell lock by cell id |
| `nwinfo_set_lte_ext_band {lte_band}` | extended LTE band selection |
| `nwinfo_set_nr5g_sa {sa_setting}` | turn SA on and off |
| `nwinfo_set_mode {operate_mode}` | operating mode |
| `uci_setting {uci_setting_string, uci_setting_context}` | arbitrary UCI writes — **do not expose**, it is a general-purpose config write with no guard rails |

The manual operator scan and `nwinfo_lock_nr_cell2` are the most useful of
these, and both are missing today.

Not pursued: `nwinfo_set_external_ant` and `nwinfo_set_mc8650_ant`. This unit
has no external antenna connectors, so there is nothing for the switch to select
— the methods exist because the firmware image is shared with SKUs that do have
them.

### Carrier aggregation: what is and is not possible

There is **no method that forces CA**, here or on any modem — carrier
aggregation is granted by the network, not requested by the device. Anything
claiming to "force CA" is really shaping which combinations the network is able
to offer.

What this firmware actually gives you:

*Reporting* — live in `nwinfo_get_netinfo`:

| Field | Meaning |
| --- | --- |
| `lteca` | per-carrier PCI, band, EARFCN, bandwidth for each LTE component |
| `ltecasig` | per-carrier RSRP/RSRQ/SINR/RSSI, plus uplink-configured and active flags |
| `lteca_state` | whether LTE CA is up |
| `nrca` | the NR equivalent |

The dashboard already parses these into carrier components (`mapSignal`), so CA
is visible when a data session exists. They read empty on a `LIMITED_SERVICE`
card, which is expected and not a fault.

*Influence* — the band-lock masks, read live from this unit:

```
lte_band_lock        0x87e29a0e00df
gw_band_lock         0x2000006c00000
nr5g_nsa_band_lock   1,2,3,5,7,8,18,20,26,28,29,38,40,41,48,66,71,75,77,78,79
nr5g_sa_band_lock    (same list)
nr5g_nrdc_band_lock  1,2,3,5,7,8,12,13,14,18,20,25,26,28,29,30,34,38,39,40,41,46,…
```

Narrowing a band lock removes combinations; it cannot add one. The honest
framing for a UI is "restrict which bands may be used", not "force CA". Locking
to a single band **disables** CA on that leg, which is the opposite of what a
user reaching for a CA control usually wants — worth saying in the UI.

`nr5g_nrdc_band_lock` is a third, separate list for NR-DC (dual connectivity,
two NR carriers). Nothing surfaces it today.

### Other objects worth a look

| Object | Note |
| --- | --- |
| `mwan3` | multi-WAN policy routing — standard OpenWrt, unexpected here |
| `network.interface.phantap` | PhanTap, a transparent tap interface |
| `container` | container runtime hooks |
| `zwrt_zte_sleep_faw.wakelock` | wakelocks and power status |
| `zwrt_cutoff_protect.api` | data-stall detection and cutoff protection |
| `zwrt_bsp.rtc` | `ui_set_alarm` — scheduled wake |
| `zwrt_bsp.led`, `zwrt_led` | LED control, including mesh night mode |
| `zwrt_bsp.key` | physical key events |
| `zwrt_tr069.api`, `zwrt_tr098db.*` | TR-069 remote management — carrier provisioning |
| `zwrt_bsp.audio` | SLIC analogue-telephony chip (RJ11 handset). Not pursued: this SKU has no port, so there is nothing for it to drive |
| `zwrt_smart_mng.api` | unexamined |
| `zwrt_mc.device.manager` | orderly poweroff subscription |
| `zwrt_time_manager`, `zwrt_sntp` | time sync |

## SMS parameter encryption: not on this firmware, but coming

Upstream [PR #22](https://github.com/jesther-ai/open-u60-pro/pull/22) reports
that on `CN_ZTE_MU5250V1.0.0B27` (built 2025-12-25), `zte_libwms_send_sms`
rejects everything with `UBUS_STATUS_INVALID_ARGUMENT` unless `number` and
`message_body` are AES-256-GCM encrypted, the way the stock web UI does it.

**This firmware does not.** Checked directly in
`/usr/zte_web/web/js/service_rpc.js` on `XCBZ_HK_MU5250V1.0.0B04`, the stock UI
sends the fields in the clear:

```js
n = { number: e.number,
      sms_time: getCurrentTimeString(),
      message_body: escapeMessage(encodeMessage(e.message)),
      id: e.id + "", encode_type: ... }
```

The B27 version wraps both in `f()`, an AES-GCM helper. So the agent's plaintext
send is correct here and would break on a newer or CN image. If that happens,
upstream's `web_crypto.rs` in that PR is the implementation to port — the
listing path is unaffected either way.

## How this was gathered

```sh
ssh -p 2222 root@192.168.0.1 'ubus -v list' > ubus-verbose.txt
```

`ubus -v list` gives every object with its methods and each method's parameter
names and types — it is the authoritative map, and it costs one command. Any
claim that a feature is absent should be checked against it first.

## A caution about "the object exists"

An object answering `ubus call` means the daemon is running, not that the
hardware is fitted or the feature is provisioned. This firmware image is shared
across SKUs. Two things already found this way:

- eSIM strings and libraries are present on units with no eUICC;
- Wi-Fi station mode is present but unprovisioned on this SKU.

Read state before writing it, and prefer a read-only probe as the first
implementation of anything here.
