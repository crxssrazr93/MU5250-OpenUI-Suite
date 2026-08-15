# Mobile API gap

The Android app was vendored from the upstream `open-u60-pro` project, whose
agent has a different API surface almost everywhere. Of the 108 endpoints the
app calls, 74 are not served here. That is why Signal Monitor showed
`{"error":"not found"}` rather than a signal reading.

`scripts/check-mobile-contract.py` regenerates the list. This file records what
each entry actually needs, so the work can be scheduled instead of discovered
one screen at a time.

The decision taken was to implement the missing surface in the agent rather than
strip the app down, so the Android app, the planned Tauri desktop app and the
dashboard all speak one API.

## 1. Renames — the agent already does this

The behaviour exists; the app asks for it at the upstream path. These are not
one-line aliases: the upstream response shapes differ from this fork's, so each
needs its handler to emit the shape the app parses, or the app's parser changed
to match. An alias alone returns 200 with JSON the app cannot read, which is a
worse failure than 404 because nothing reports it.

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

Confirmed present on `XCBZ_HK_MU5250V1.0.0B04` by `ubus -v list`.

| Area | App endpoints | Firmware surface |
| --- | --- | --- |
| SIM PIN/PUK | `/api/sim/pin/verify`, `/api/sim/pin/change`, `/api/sim/pin/toggle`, `/api/sim/puk/verify`, `/api/sim/lock`, `/api/sim/unlock` | `zwrt_zte_mdm.api` `sim_verify_pin_puk`, `sim_change_pin`, `sim_change_pin_mode`, `get_simlock_available_trials` |
| DNS / DoH | `/api/doh`, `/api/doh/status`, `/api/doh/cache` | `lan_dns_*`, `dns_mode`, `ipv4_dns_prefer`, `lan_dns_provider` |
| Firewall | `/api/firewall/config`, `/api/firewall/port-forward` | `default_firewall_policy`, `port_mapping`, `portforward_enable`, `portmapping_enable` |
| Domain filtering | `/api/firewall/domain-filter`, `/api/firewall/domain-filter/rule` | `dnsquery_action`, `dnsquery_target` |
| Airplane / radio | `/api/modem/airplane`, `/api/modem/online` | `zte_nwinfo_api` `nwinfo_set_mode` (`low_power` / `online`) — already used by the eSIM switch fix |
| Neighbour cells | `/api/modem/neighbors` | `nwinfo_get_*` cell reporting |
| USSD | `/api/ussd/send`, `/api/ussd/respond`, `/api/ussd/cancel` | **implemented and correct; the network does not answer.** The modem supports USSD (`AT+CUSD=?` returns `+CUSD: (0-2)`), is registered on the circuit-switched domain (`+CREG: 0,1`), and accepts the request with `OK`. No `+CUSD:` reply then arrives — checked on `at_mdm0`, `at_mdm1`, `at_mdm2` and `at_usb0` for 25s. `no_reply` is therefore the honest answer, not a bug. Most likely USSD is barred for this SIM or plan; retry on another SIM before concluding anything about the code |
| STK menus | `/api/stk/menu`, `/api/stk/select` | **not available.** No STK or USSD ubus methods exist at all (`ubus -v list` matches zero). Menu browsing would need `+CUSATP`/`+STKPRO`, which are vendor-specific and unverified here. This is the same missing CAT path that makes eSIM REFRESH fail, so a profile switch needs a reboot |

## 3. No firmware backing — must be built in the agent

Nothing in the firmware provides these. They are agent features in their own
right, with their own storage and scheduling.

| Area | App endpoints | Note |
| --- | --- | --- |
| Speed test | `/api/speedtest/start`, `/stop`, `/progress`, `/servers` | needs a client and server list on the router |
| Scheduler | `/api/scheduler/jobs`, `/api/scheduler/jobs/$id` | persistent jobs; must survive reboot, so `/data` plus `rc.local` only — no new boot hooks (see SAFETY.md) |
| SMS forwarding | `/api/sms/forward/config`, `/rules`, `/rules/toggle`, `/log`, `/log/clear`, `/test` | **declined — not wanted.** Was built and then removed at the user's request. It ran a background poller that sends SMS, which costs money and cannot be recalled, so it is not something to leave in place unused. The six endpoints stay unserved deliberately; the screen should be dropped from the app rather than backed |
| QoS | `/api/network/qos` | vendor QoS surface not yet located |
| Signal detect | `/api/modem/signal-detect`, `/status` | **not a duplicate of the signal logger, but the firmware does not do what the app expects.** Checked on hardware. The logger records the serving cell over time to CSV; this screen wants a sweep returning `{band, earfcn, pci, rsrp, rsrq, sinr}` records with a progress percentage. Different features. The firmware does expose `nwinfo_start_detect_signal_quality`, `nwinfo_end_detect_signal_quality` and `nwinfo_get_detect_quality_recorder`, but running a detection for 20s left the recorder empty, and the companion methods take `{date, location, quality}` — so the vendor feature is a **manual, location-tagged site survey**, not an automatic band sweep. Backing the app's screen with it would give a UI with no progress and none of the per-band fields it renders. A real sweep would mean band-locking through each band in turn and measuring, which is slow, drops the connection repeatedly, and overlaps the band lock screen |
| STC | `/api/modem/stc`, `/params`, `/status` | vendor "smart cell" tuning, partially visible in `nwinfo_set_stc_white_list_par` |
| Guest Wi-Fi | `/api/wifi/guest` | vendor multi-SSID surface |
| VPN passthrough | `/api/vpn/passthrough` | **nothing.** No passthrough object exists; the `zwrt_tunnel.*` objects are outbound VPN clients. Menu entry removed — see above |
| Schedule reboot | `/api/device/schedule-reboot` | depends on the scheduler above |
| Power | `/api/device/power-save`, `/api/device/fast-boot` | vendor power policy |
| Factory reset | `/api/device/factory-reset` | deliberately absent here — `fac_reset` exists but is irreversible and unguarded |

### Corrections

This entry was wrong twice before it was right, so the reasoning is kept rather
than tidied away.

1. USSD was first grouped with STK as unavailable, because the firmware exposes
   no usable CAT path. That holds for STK menus only — the two reach the modem
   by different routes, and the AT port answers for USSD.
2. A single `+CME ERROR: no network service` read off `/dev/at_mdm0` was then
   taken as proof of a data-only device with no CS attach, and separately as
   proof of a defect in the agent's serial read path. Both were wrong. `AT+CREG?`
   reports `0,1`, so the device *is* CS-registered, and the agent's reader
   captures `OK`, `+CREG:` and `+CGREG:` correctly. That one error came from a
   port left in a different state by an earlier session, and comparing it
   against an agent call was not like-for-like.

The lesson worth keeping: a single reading off a serial port is not evidence.
The port is stateful, and one observation taken alone produced two confident
and incorrect diagnoses.

## Order of work

1. ~~**Group 1**~~ — done. Sixteen endpoints, verified on hardware.
2. ~~**SIM PIN, airplane, firewall, port forwarding, domain filtering, DNS/DoH**~~
   — done. Thirteen endpoints, verified on hardware.
3. **USSD**, now that the AT path is confirmed. Needs an unsolicited-response
   reader, which nothing in the agent has yet.
4. ~~**Scheduler**~~ — done, on `/data` plus the existing `rc.local` entry.
   Scheduled reboot rides on it.
5. Everything else, by whichever screen is actually wanted. SMS forwarding was
   asked for and then declined, which is the point: the remaining forty are
   worth confirming one at a time rather than built because the app calls them.

Unserved endpoints went from 74 to 45 in the course of this work.

Group 3 is roughly forty endpoints of genuinely new agent functionality. It is
the bulk of the remaining work and should not be estimated as if it were part of
the port.

## The third thing the inventory could not see: the request itself

A path can be served, and called, and still fail every time. Two ways, both
invisible to `check-mobile-contract.py`, and both found only when someone
tapped a screen and sent a screenshot:

- **The wrong verb.** Twenty calls used PUT or DELETE against routes the agent
  registers for POST. The app's unlock control sent `DELETE
  /api/modem/bands/lock` to a POST-only route; APN edit, delete and activate
  all used PUT; the WiFi screen read `/api/wifi/settings`, which is write-only,
  instead of `/api/wifi/status`.
- **No `X-Confirm`.** Twenty-six calls hit routes in `DESTRUCTIVE_PATHS`
  without the confirming helper, so the agent refused them. Band lock, cell
  lock, reboot, DoH, port forwarding, operator scan and select, SIM PIN and
  PUK, USSD, the scheduler and every domain-filter write.

`scripts/check-mobile-methods.py` reports both. It reads the verbs out of
`server.rs` and `DESTRUCTIVE_PATHS` out of the same file, so it cannot drift
from the agent.

Underneath those, several requests were the right verb carrying the wrong body:

- LTE band lock sent `lte_band_mask: "1,3,8"`. The vendor wants a **decimal
  bitmask**, band N at bit N-1, so that asked for bands 1, 2, 4, 8, 16 and 32.
  The conversion now lives in `compat::lte_band_mask` with unit tests, rather
  than being written out again in each client.
- Cell lock sent `nr_pci`/`lte_pci`, and the compat handler dispatched on
  `pci`, which was therefore always absent — so it took the "no cell named"
  branch and **reset the lock**. The button labelled Lock unlocked.
  `compat::modem_cell_lock` now translates instead of forwarding.
- The firewall screen sent `firewall_switch` as `"1"`. The agent reads
  `firewall_enabled` as a bool, so every write returned "nothing to change".
- Domain filter rules were sent as `{domain}`; the route takes
  `{action, fqdn, ...}`.

## Two more vendor surfaces, settled by looking

- **VPN passthrough — confirmed absent.** `/api/vpn/passthrough` was listed
  below as "vendor tunnel passthrough flags", which was a guess. `ubus list`
  has no passthrough object at all. What exists is `zwrt_tunnel.ipsec`,
  `.l2tp`, `.pptp` and `.openvpn`, each with `handle {action}` and a `.config`
  child whose `set` takes `server_address`, `username`, `password`,
  `auto_start`. Those are **outbound VPN client** configurations, not
  passthrough switches for traffic crossing the router. The three switches the
  app drew have nothing behind them, so the menu entry is gone. A VPN client
  screen would be a new feature, and a real one.
- **SMS capacity — the method was there all along.** `/api/sms/capacity` was
  derived from a listing, reading `body["data"]["total"]`, a field
  `zte_libwms_get_sms_data` does not return. So the counters read zero however
  many messages were stored. `zwrt_wms zwrt_wms_get_wms_capacity` reports it
  directly and returns exactly the `sms_*` keys the apps read. One caveat kept
  in the code: the vendor reports `sms_nvused_total` as 0 while
  `sms_nv_rev_total` counts the messages the listing returns, so the used
  figure is summed from the received/sent/draft breakdown. Reads 23 of 100 on
  this unit, matching the 23 messages listed.

The pattern in both: the earlier conclusion was reached by reading code and
reasoning about it, and it was wrong in opposite directions — one invented a
surface that does not exist, the other missed one that does. `ubus -v list` on
the device settles it in a minute.

## Sending an SMS

`zte_libwms_send_sms` rejects the entire call with `Invalid argument` if any
part of the argument set is wrong, and says nothing about which part. Three
rules, each established by varying one field at a time against the live daemon:

1. **`sms_time` is semicolon-separated**: `YY;MM;DD;HH;MM;SS;+Q`, offset in
   quarter hours. This is the only field that decides whether the call is
   accepted — every combination of number and body encoding was accepted with
   semicolons and refused with commas.

   The misleading part: the listing returns stored dates comma-separated
   (`26,08,14,11,15,43,+22`) and `zte_topsw_wms` contains a
   `%s,%s,%s,%s,%s,%s,%s` format string next to a "year is %s, month is %s, …"
   debug line. That format belongs to the reader, not to this argument. Acting
   on it broke sending, and the fix was to put the semicolons back.

2. **`message_body` is always hex**, whatever `encode_type` says. Sending the
   literal `TEST` as `GSM7_default` was accepted and stored a single `@`: the
   letters were read as hex digits. Everything is UCS-2 encoded and sent as
   `UNICODE`, which costs 70 characters per segment instead of 160 and is the
   price of a message that arrives intact.

3. **`number` is sent plain.** The daemon UCS-2 encodes it for storage itself,
   so a pre-encoded address is stored double-encoded — `12346` came back as
   `00310032003300340036` spelled out character by character.

Verified end to end: `POST /api/sms/send {"to": …, "text": "Agent send OK"}`
returns 200 and the message reads back from the listing with its number and
body intact. The five messages left behind by these probes were deleted, and
the listing is back to the 23 the capacity counter reports. A later send to a
real handset was confirmed received, so the chain is proven to the phone and
not just to the radio.

`tag` on a stored outgoing message is the send result, and the three states were
each observed rather than inferred:

| tag | meaning | how it was seen |
| --- | ------- | --------------- |
| 2 | sent | the message that arrived on a real handset |
| 3 | failed | correctly-formed sends to a non-routable number |
| 4 | draft | a malformed call that never reached the radio |

The app's `SMSTag` enum already spells these `SENT(2)`, `FAILED(3)`, `DRAFT(4)`,
which matches. Worth stating because 3 and 4 read backwards at a glance: a
message the daemon never managed to submit is filed as a draft, while one the
network refused is the failure.

## The endpoint inventory was measuring the wrong thing

Every check in this document asks whether a *path* is served. The apps were
still full of blank fields with the inventory looking healthy, because the two
bugs behind most of it were not missing endpoints at all:

- `AgentClient.toAny()` retyped quoted JSON as numbers, so `as? String` returned
  null for anything numeric-looking. An ICCID ending in `F` became a Double,
  because Java's `parseDouble` treats a trailing `F` as a float suffix.
- The dashboard's digit reel laid out 70 slots inside a Box one slot tall, so
  every digit was blank while the units around it rendered.

`scripts/check-field-contract.py` was added for the first class: it compares
every key the apps read against every key a live agent returns. The second class
only shows up by running the app, which is now possible without a handset — see
`docs/EMULATOR-TESTING.md`.

## What each check actually covers

Four checks, each blind to what the next one sees. Run in this order, they take
about three minutes and cover everything that has gone wrong so far.

| Check | Question | Found |
| --- | --- | --- |
| `check-mobile-contract.py` | is the path served? | the original 74 |
| `check-mobile-methods.py` | with that verb, and `X-Confirm` where the agent demands it? | 20 wrong verbs, 26 missing confirms |
| `check-mobile-reads.py` | does the response carry the keys the parser then reads? | firewall, schedule reboot, SMS capacity, IPv6 DNS |
| `walk-app.py` | and does the screen come up without an error on it? | the screenshots that started this |

`check-mobile-reads.py` extracts the keys from the parser source rather than
from a list in the script. A hand-written list is the same bug one level up: it
agrees with the parser the day it is written and then silently stops.

The Android parsers also have JVM unit tests now — `./gradlew testDebugUnitTest`
— built from payloads captured off the live agent. That is the layer where the
blank-field bugs live, it is pure Kotlin, and it had no tests at all.

## Resolved since

- **Smart Tower Connect** — endpoints served, feature inert, no screen. I built
  this believing the UCI sections under `zte_nwinfo.stc_cell_lock_config` and
  `…_status` gave the state the missing getter did not. Toggling it proved that
  wrong, and the correction matters more than the original claim:

  - `nwinfo_stc_cell_lock_enable` and `…_disable` both report success and change
    no field in `zte_nwinfo`. Called from the device shell as well as through
    the agent, so it is not the agent mistranslating.
  - `cell_white_list_enable_flag` reads 1 before and after either verb. It is
    not the toggle it resembles, and is now reported as `whitelist_available`,
    which is the most that can be said for it.
  - After enabling, the collected counts and `collect_cell_white_list_run_time`
    sat at 0 for two minutes — well past the 60 s `delayed_start_timer` — while
    the neighbour list showed ten cells available to collect from.

  So `/api/modem/stc/params`, `/status`, `PUT /api/modem/stc` and `/stc/reset`
  stay: they report the vendor's real parameters, and a client discovering that
  the firmware ignores a write is a fair outcome. But nothing reports a toggle
  position, and the app has no STC entry — a switch that silently does nothing
  is worse than no switch. Same conclusion as signal-detect, reached the same
  way: by running it rather than reading the method list.

- **Mobile data toggle** — works, and verified by using it. `PUT
  /api/modem/data {"enable":0}` returns the vendor's `"set commited"`, and
  within five seconds `connect_status` goes to `disconnected` with the address
  cleared. Setting it back reconnected on the same IP inside six seconds.
  Roaming rides on the same route and is deliberately **not** tested: turning
  roaming on while attached to a foreign network is a billing event, not a
  reversible experiment.
- **`GET /api/modem/network-mode`** — added. Only the setter existed, so the
  screen opened on an error. There is no `nwinfo_get_netselect`; the current
  value is read back out of `nwinfo_get_netinfo`.
- **`/api/modem/data`** — was passing through `nwinfo_get_netinfo`, which
  describes the radio and says nothing about the data call. Now backed by
  `zwrt_data get_wwaniface`, which is where `connect_status`, `enable` and
  `roam_enable` actually live. `PUT` added for the mobile-data and roaming
  toggles, forwarding only those three fields — the vendor setter takes the
  whole interface description including DNS and routes.
- **NCK trials** — `available_trials` had no source and the app defaulted it to
  0, which happened to be the true value and hid the gap. Now read from
  `zwrt_zte_mdm.api get_simlock_available_trials` and reported as null, never 0,
  when it cannot be read.
- **Signal Detection** — menu entry removed. The screen remains in the tree, but
  nothing routes to it, so it can no longer open on an error.

Unserved endpoints are now 23, of which about 20 are deliberate.
