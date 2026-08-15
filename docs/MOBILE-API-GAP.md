# Mobile API gap

The Android app came from the upstream `open-u60-pro` project. That project's
agent uses a different API surface almost everywhere. The app calls 108
endpoints. This fork does not serve 74 of them. That is why Signal Monitor showed
`{"error":"not found"}` instead of a signal reading.

`scripts/check-mobile-contract.py` regenerates the list. This file records what
each entry needs. So you can schedule the work instead of finding it one screen
at a time.

We chose to add the missing surface to the agent. We did not strip the app down.
The result is that the Android app, the planned Tauri desktop app, and the
dashboard all speak one API.

## 1. Renames: the agent already does this

The behaviour exists. The app asks for it at the upstream path. These are not
one-line aliases. The upstream response shapes differ from this fork's shapes.
So each one needs its handler to emit the shape the app parses. As an
alternative, you change the app's parser to match. An alias alone returns 200
with JSON the app cannot read. That is a worse failure than 404, because nothing
reports it.

| App calls | Served here as |
| --- | --- |
| `/api/network/signal` | `/api/dashboard` (batched) |
| `/api/network/wan`, `/api/network/wan6` | `/api/dashboard` |
| `/api/network/traffic`, `/api/network/speed`, `/api/network/speeds` | `/api/dashboard` |
| `/api/network/rmnet` | `/api/dashboard` |
| `/api/modem/status`, `/api/modem/data` | `/api/dashboard` |
| `/api/battery` | `/api/device/battery-info` |
| `/api/device/system`, `/api/device/thermal` | `/api/device`, `/api/device/thermal/all` |
| `/api/device/imei` | `/api/sim/imei` |
| `/api/device/usb`, `/api/device/usb/mode` | `/api/usb/status`, `/api/usb/mode` |
| `/api/device/powerbank` | `/api/usb/powerbank` |
| `/api/network/lan` | `/api/router/lan` |
| `/api/network/dns` | `/api/router/dns` |
| `/api/network/dhcp-leases` | `/api/network/clients` |
| `/api/modem/apn`, `/api/modem/apn/profile`, `/api/modem/apn/activate`, `/api/modem/apn/mode` | `/api/router/apn/*` |
| `/api/modem/bands/lock`, `/api/modem/bands/lte/lock`, `/api/modem/bands/nr/lock` | `/api/cell/band/*` |
| `/api/modem/cell-lock` | `/api/cell/lock/*` |
| `/api/modem/scan` | `/api/operator/scan` (added this session) |
| `/api/modem/register` | `/api/operator/select` |
| `/api/sms/capacity` | part of `/api/sms/list` |
| `/api/euicc/eid$full`, `/api/euicc/profiles$full` | `?full=` query, app builds the path wrong |

## 2. Backed by firmware, not yet implemented

`ubus -v list` confirms these are present on `XCBZ_HK_MU5250V1.0.0B04`.

| Area | App endpoints | Firmware surface |
| --- | --- | --- |
| SIM PIN/PUK | `/api/sim/pin/verify`, `/api/sim/pin/change`, `/api/sim/pin/toggle`, `/api/sim/puk/verify`, `/api/sim/lock`, `/api/sim/unlock` | `zwrt_zte_mdm.api` `sim_verify_pin_puk`, `sim_change_pin`, `sim_change_pin_mode`, `get_simlock_available_trials` |
| DNS / DoH | `/api/doh`, `/api/doh/status`, `/api/doh/cache` | `lan_dns_*`, `dns_mode`, `ipv4_dns_prefer`, `lan_dns_provider` |
| Firewall | `/api/firewall/config`, `/api/firewall/port-forward` | `default_firewall_policy`, `port_mapping`, `portforward_enable`, `portmapping_enable` |
| Domain filtering | `/api/firewall/domain-filter`, `/api/firewall/domain-filter/rule` | `dnsquery_action`, `dnsquery_target` |
| Airplane / radio | `/api/modem/airplane`, `/api/modem/online` | `zte_nwinfo_api` `nwinfo_set_mode` (`low_power` / `online`). The eSIM switch fix already uses it |
| Neighbour cells | `/api/modem/neighbors` | `nwinfo_get_*` cell reporting |

## 3. No firmware backing: must be built in the agent

Nothing in the firmware provides these. They are agent features in their own
right. Each has its own storage and scheduling.

| Area | App endpoints | Note |
| --- | --- | --- |
| Speed test | `/api/speedtest/start`, `/stop`, `/progress`, `/servers` | needs a client and server list on the router |
| Scheduler | `/api/scheduler/jobs`, `/api/scheduler/jobs/$id` | persistent jobs. They must survive reboot, so `/data` plus `rc.local` only. No new boot hooks (see SAFETY.md) |
| SMS forwarding | `/api/sms/forward/config`, `/rules`, `/rules/toggle`, `/log`, `/log/clear`, `/test` | **Declined. Not wanted.** It was built and then removed at the user's request. It ran a background poller that sends SMS. That costs money and cannot be recalled. So it is not something to leave in place unused. The six endpoints stay unserved deliberately. Drop the screen from the app rather than back it |
| QoS | `/api/network/qos` | vendor QoS surface not yet located |
| Signal detect | `/api/modem/signal-detect`, `/status` | **Not a duplicate of the signal logger. The firmware does not do what the app expects.** This was checked on hardware. The logger records the serving cell over time to CSV. This screen wants a sweep that returns `{band, earfcn, pci, rsrp, rsrq, sinr}` records with a progress percentage. They are different features. The firmware does expose `nwinfo_start_detect_signal_quality`, `nwinfo_end_detect_signal_quality` and `nwinfo_get_detect_quality_recorder`. But running a detection for 20s left the recorder empty. The companion methods take `{date, location, quality}`. So the vendor feature is a **manual, location-tagged site survey**, not an automatic band sweep. Backing the app's screen with it would give a UI with no progress and none of the per-band fields it renders. A real sweep would band-lock through each band in turn and measure. That is slow, drops the connection repeatedly, and overlaps the band lock screen |
| STC | `/api/modem/stc`, `/params`, `/status` | vendor "smart cell" tuning, partially visible in `nwinfo_set_stc_white_list_par` |
| Guest Wi-Fi | `/api/wifi/guest` | vendor multi-SSID surface |
| VPN passthrough | `/api/vpn/passthrough` | **Nothing.** No passthrough object exists. The `zwrt_tunnel.*` objects are outbound VPN clients. Menu entry removed (see above) |
| Schedule reboot | `/api/device/schedule-reboot` | depends on the scheduler above |
| Power | `/api/device/power-save`, `/api/device/fast-boot` | vendor power policy |
| Factory reset | `/api/device/factory-reset` | deliberately absent here. `fac_reset` exists but is irreversible and unguarded |

### USSD and STK: investigated, then removed

USSD and STK were both dropped from the agent and the apps after an exhaustive
search found no working path on this firmware (`XCBZ_HK_MU5250V1.0.0B04`):

- **No UI.** The stock web UI ships no USSD or STK page. Only login, password,
  privacy and welcome templates exist.
- **No ubus method.** Every one of the 132 ubus objects was scanned. None exposes
  a USSD or STK method. The only working web API on this build is ubus-over-HTTP
  at `/ubus/`.
- **The legacy goform is not served.** The USSD code left in the vendor
  `service_rpc.js` targets `/goform/goform_set_cmd_process`, which returns 404 on
  this build. It is dead code from a shared ZTE bundle.
- **QMI Voice refuses it.** A correct QMI Voice (0x09) `ORIGINATE_USSD` client was
  written and tested on the device. The modem accepted the request and echoed the
  code back as ASCII, then failed the origination with a supplementary-service
  error (92 sync, 94 no-wait), and no answering `USSD_IND` ever arrived. This held
  for every code, despite `domain_stat: CS_PS` (registered on both domains).
- **The AT path cannot read the reply.** `/dev/at_mdm*` are held by a
  `port-bridge` daemon, so a raw `AT+CUSD` reply URC never reaches the agent.

The one durable lesson from the earlier debugging: a single reading off a serial
port is not evidence. The port is stateful, and one observation taken alone
produced two confident and incorrect diagnoses (a "data-only device" and a
"broken serial reader") that `AT+CREG? -> 0,1` later disproved.

## Order of work

1. ~~**Group 1**~~. Done. Sixteen endpoints, verified on hardware.
2. ~~**SIM PIN, airplane, firewall, port forwarding, domain filtering, DNS/DoH**~~.
   Done. Thirteen endpoints, verified on hardware.
3. ~~**USSD**~~. Removed. No working path exists on this firmware (see above).
4. ~~**Scheduler**~~. Done, on `/data` plus the existing `rc.local` entry.
   Scheduled reboot rides on it.
5. Everything else, by whichever screen is actually wanted. SMS forwarding was
   asked for and then declined. That is the point. It is better to confirm the
   remaining forty one at a time than to build them because the app calls them.

Unserved endpoints went from 74 to 45 in the course of this work.

Group 3 is roughly forty endpoints of genuinely new agent functionality. It is
the bulk of the remaining work. Do not estimate it as if it were part of the
port.

## The third thing the inventory could not see: the request itself

A path can be served, and called, and still fail every time. There are two ways.
Both are invisible to `check-mobile-contract.py`. Both were found only when
someone tapped a screen and sent a screenshot:

- **The wrong verb.** Twenty calls used PUT or DELETE against routes the agent
  registers for POST. The app's unlock control sent `DELETE
  /api/modem/bands/lock` to a POST-only route. APN edit, delete and activate all
  used PUT. The WiFi screen read `/api/wifi/settings`, which is write-only,
  instead of `/api/wifi/status`.
- **No `X-Confirm`.** Twenty-six calls hit routes in `DESTRUCTIVE_PATHS` without
  the confirming helper. So the agent refused them. This covered band lock, cell
  lock, reboot, DoH, port forwarding, operator scan and select, SIM PIN and PUK,
  the scheduler and every domain-filter write.

`scripts/check-mobile-methods.py` reports both. It reads the verbs out of
`server.rs` and `DESTRUCTIVE_PATHS` out of the same file. So it cannot drift from
the agent.

Underneath those, several requests were the right verb carrying the wrong body:

- LTE band lock sent `lte_band_mask: "1,3,8"`. The vendor wants a **decimal
  bitmask**, band N at bit N-1. So that asked for bands 1, 2, 4, 8, 16 and 32.
  The conversion now lives in `compat::lte_band_mask` with unit tests, rather
  than being written out again in each client.
- Cell lock sent `nr_pci` or `lte_pci`. The compat handler dispatched on `pci`,
  which was therefore always absent. So it took the "no cell named" branch and
  **reset the lock**. The button labelled Lock unlocked. `compat::modem_cell_lock`
  now translates instead of forwarding.
- The firewall screen sent `firewall_switch` as `"1"`. The agent reads
  `firewall_enabled` as a bool. So every write returned "nothing to change".
- Domain filter rules were sent as `{domain}`. The route takes
  `{action, fqdn, ...}`.

## Two more vendor surfaces, settled by looking

- **VPN passthrough. Confirmed absent.** `/api/vpn/passthrough` was listed below
  as "vendor tunnel passthrough flags". That was a guess. `ubus list` has no
  passthrough object at all. What exists is `zwrt_tunnel.ipsec`, `.l2tp`, `.pptp`
  and `.openvpn`. Each has `handle {action}` and a `.config` child whose `set`
  takes `server_address`, `username`, `password`, `auto_start`. Those are
  **outbound VPN client** configurations, not passthrough switches for traffic
  crossing the router. The three switches the app drew have nothing behind them.
  So the menu entry is gone. A VPN client screen would be a new feature, and a
  real one.
- **SMS capacity. The method was there all along.** `/api/sms/capacity` was
  derived from a listing. It read `body["data"]["total"]`, a field
  `zte_libwms_get_sms_data` does not return. So the counters read zero however
  many messages were stored. `zwrt_wms zwrt_wms_get_wms_capacity` reports it
  directly and returns exactly the `sms_*` keys the apps read. One caveat is kept
  in the code. The vendor reports `sms_nvused_total` as 0 while `sms_nv_rev_total`
  counts the messages the listing returns. So the used figure is summed from the
  received, sent and draft breakdown. It reads 23 of 100 on this unit, matching
  the 23 messages listed.

The pattern is the same in both. The earlier conclusion was reached by reading
code and reasoning about it. It was wrong in opposite directions. One invented a
surface that does not exist. The other missed one that does. `ubus -v list` on
the device settles it in a minute.

## Sending an SMS

`zte_libwms_send_sms` rejects the entire call with `Invalid argument` if any part
of the argument set is wrong. It says nothing about which part. There are three
rules. Each was established by varying one field at a time against the live
daemon:

1. **`sms_time` is semicolon-separated**: `YY;MM;DD;HH;MM;SS;+Q`, offset in
   quarter hours. This is the only field that decides whether the call is
   accepted. Every combination of number and body encoding was accepted with
   semicolons and refused with commas.

   The misleading part: the listing returns stored dates comma-separated
   (`26,08,14,11,15,43,+22`). `zte_topsw_wms` contains a `%s,%s,%s,%s,%s,%s,%s`
   format string next to a "year is %s, month is %s, …" debug line. That format
   belongs to the reader, not to this argument. Acting on it broke sending. The
   fix was to put the semicolons back.

2. **`message_body` is always hex**, whatever `encode_type` says. Sending the
   literal `TEST` as `GSM7_default` was accepted and stored a single `@`. The
   letters were read as hex digits. Everything is UCS-2 encoded and sent as
   `UNICODE`. That costs 70 characters per segment instead of 160. It is the
   price of a message that arrives intact.

3. **`number` is sent plain.** The daemon UCS-2 encodes it for storage itself. So
   a pre-encoded address is stored double-encoded. `12346` came back as
   `00310032003300340036`, spelled out character by character.

This was verified end to end. `POST /api/sms/send {"to": …, "text": "Agent send
OK"}` returns 200. The message reads back from the listing with its number and
body intact. The five messages left behind by these probes were deleted. The
listing is back to the 23 the capacity counter reports. A later send to a real
handset was confirmed received. So the chain is proven to the phone and not just
to the radio.

`tag` on a stored outgoing message is the send result. The three states were each
observed rather than inferred:

| tag | meaning | how it was seen |
| --- | ------- | --------------- |
| 2 | sent | the message that arrived on a real handset |
| 3 | failed | correctly-formed sends to a non-routable number |
| 4 | draft | a malformed call that never reached the radio |

The app's `SMSTag` enum already spells these `SENT(2)`, `FAILED(3)`, `DRAFT(4)`,
which matches. This is worth stating because 3 and 4 read backwards at a glance. A
message the daemon never managed to submit is filed as a draft. One the network
refused is the failure.

## The endpoint inventory was measuring the wrong thing

Every check in this document asks whether a *path* is served. The apps were still
full of blank fields while the inventory looked healthy. The two bugs behind most
of it were not missing endpoints at all:

- `AgentClient.toAny()` retyped quoted JSON as numbers. So `as? String` returned
  null for anything numeric-looking. An ICCID ending in `F` became a Double,
  because Java's `parseDouble` treats a trailing `F` as a float suffix.
- The dashboard's digit reel laid out 70 slots inside a Box one slot tall. So
  every digit was blank while the units around it rendered.

`scripts/check-field-contract.py` was added for the first class. It compares
every key the apps read against every key a live agent returns. The second class
only shows up by running the app. That is now possible without a handset (see
`docs/EMULATOR-TESTING.md`).

## What each check actually covers

There are four checks. Each is blind to what the next one sees. Run in this order,
they take about three minutes and cover everything that has gone wrong so far.

| Check | Question | Found |
| --- | --- | --- |
| `check-mobile-contract.py` | is the path served? | the original 74 |
| `check-mobile-methods.py` | with that verb, and `X-Confirm` where the agent demands it? | 20 wrong verbs, 26 missing confirms |
| `check-mobile-reads.py` | does the response carry the keys the parser then reads? | firewall, schedule reboot, SMS capacity, IPv6 DNS |
| `walk-app.py` | and does the screen come up without an error on it? | the screenshots that started this |

`check-mobile-reads.py` extracts the keys from the parser source rather than from
a list in the script. A hand-written list is the same bug one level up. It agrees
with the parser the day it is written and then silently stops.

The Android parsers also have JVM unit tests now (`./gradlew testDebugUnitTest`),
built from payloads captured off the live agent. That is the layer where the
blank-field bugs live. It is pure Kotlin, and it had no tests at all.

## Resolved since

- **Smart Tower Connect.** Endpoints served, feature inert, no screen. I built
  this believing the UCI sections under `zte_nwinfo.stc_cell_lock_config` and
  `…_status` gave the state the missing getter did not. Toggling it proved that
  wrong. The correction matters more than the original claim:

  - `nwinfo_stc_cell_lock_enable` and `…_disable` both report success and change
    no field in `zte_nwinfo`. This was called from the device shell as well as
    through the agent. So it is not the agent mistranslating.
  - `cell_white_list_enable_flag` reads 1 before and after either verb. It is not
    the toggle it resembles. It is now reported as `whitelist_available`, which
    is the most that can be said for it.
  - After enabling, the collected counts and `collect_cell_white_list_run_time`
    sat at 0 for two minutes. That is well past the 60 s `delayed_start_timer`.
    Meanwhile the neighbour list showed ten cells available to collect from.

  So `/api/modem/stc/params`, `/status`, `PUT /api/modem/stc` and `/stc/reset`
  stay. They report the vendor's real parameters. A client discovering that the
  firmware ignores a write is a fair outcome. But nothing reports a toggle
  position, and the app has no STC entry. A switch that silently does nothing is
  worse than no switch. This is the same conclusion as signal-detect, reached the
  same way: by running it rather than reading the method list.

- **Mobile data toggle.** Works, and verified by using it. `PUT /api/modem/data
  {"enable":0}` returns the vendor's `"set commited"`. Within five seconds
  `connect_status` goes to `disconnected` with the address cleared. Setting it
  back reconnected on the same IP inside six seconds. Roaming rides on the same
  route and is deliberately **not** tested. Turning roaming on while attached to a
  foreign network is a billing event, not a reversible experiment.
- **`GET /api/modem/network-mode`.** Added. Only the setter existed, so the
  screen opened on an error. There is no `nwinfo_get_netselect`. The current value
  is read back out of `nwinfo_get_netinfo`.
- **`/api/modem/data`.** Was passing through `nwinfo_get_netinfo`, which
  describes the radio and says nothing about the data call. It is now backed by
  `zwrt_data get_wwaniface`, which is where `connect_status`, `enable` and
  `roam_enable` actually live. `PUT` was added for the mobile-data and roaming
  toggles. It forwards only those three fields. The vendor setter takes the whole
  interface description including DNS and routes.
- **NCK trials.** `available_trials` had no source and the app defaulted it to 0.
  That happened to be the true value and hid the gap. It is now read from
  `zwrt_zte_mdm.api get_simlock_available_trials`. It is reported as null, never
  0, when it cannot be read.
- **Signal Detection.** Menu entry removed. The screen remains in the tree, but
  nothing routes to it. So it can no longer open on an error.

Unserved endpoints are now 23. About 20 of them are deliberate.

## Android app parity with the dashboard

The Android app reimplements each screen by hand rather than sharing the
dashboard's React code. So it can lag the dashboard. Four features the agent
serves and the dashboard exposes had no reachable Android screen. All four are now
built to parity. They keep the dashboard's confirm gates, identifier masking and
reboot handling:

| Feature | Reached from | Backed by |
| --- | --- | --- |
| eSIM | Router settings, Cellular | the existing `ESIMViewModel` and in-app relay, which had no screen rendering them |
| WireGuard | Router settings, Connectivity | new parser, ViewModel and screen over `/api/tunnel/wireguard*` |
| TTL override | Tools | new screen over `/api/ttl*`, replacing a disabled "requires ADB" placeholder |
| Operator selection | Router settings, Cellular | new parser, ViewModel and screen over `/api/operator*` |

`walk-app.py` covers all four.

This was verified on the emulator against the live agent. All 30 Android screens
open clean, with no error text, no all-placeholder screens, and no `FATAL
EXCEPTION`.
