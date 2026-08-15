# eUICC (eSIM) support

Status: **profile management works end to end.** The agent detects an eUICC,
reads its EID, enumerates profiles, and — via `lpac` — downloads, enables,
disables, deletes and renames them, and handles RSP notifications.

A profile has been downloaded and installed on real hardware through this stack.

Verified on ZTE U60 Pro / MU5250, firmware `XCBZ_HK_MU5250V1.0.0B04`, with a
physical removable eUICC in slot 1.

## Why QRTR

The modem on this platform (SDX75) is PCIe/MHI-attached, and the stock kernel is
built with `CONFIG_QRTR=y`, `CONFIG_QRTR_SMD=y` and `CONFIG_QRTR_MHI=y`. The
`QIPCRTR` protocol is registered and an `IPCR` MHI channel is present, so QMI
services are reachable from an ordinary `AF_QIPCRTR` datagram socket.

Three transports were considered.

1. **`AF_QIPCRTR` socket (chosen).** No dependencies beyond `libc`, so it works
   in the agent's static musl binary. Full control over timeouts and channel
   cleanup.
2. **`libqmi_cci.so` on the device.** Ruled out: a statically linked musl binary
   cannot `dlopen` the device's shared libraries.
3. **Driving `/usr/bin/qmi_simple_ril_test`.** Ruled out for production: it is an
   interactive test tool, so APDU bytes have to be scraped from a human-readable
   log, and there is no reliable way to bound or clean up a logical channel left
   open by a killed child process.

There is no `qmicli` on this firmware, so the open libqmi CLI was not an option.

## Layering

Each layer depends only on the one below it, plus `std` and `libc`. No third
party crates are involved, so the whole stack can be lifted into another agent.

| Module | Responsibility |
| --- | --- |
| `agent/src/qmi/qrtr.rs` | `AF_QIPCRTR` socket, name-server lookup, QMI request/response framing |
| `agent/src/qmi/tlv.rs` | QMI TLV codec, result decoding |
| `agent/src/qmi/uim.rs` | UIM service: card status, logical channel, APDU |
| `agent/src/euicc/bertlv.rs` | BER-TLV reader for ES10 responses |
| `agent/src/euicc/es10.rs` | ES10 command construction, EID and profile parsing |
| `agent/src/euicc/mod.rs` | Session orchestration, locking, masking |
| `agent/src/euicc/api.rs` | HTTP handlers — the only project-specific layer |

### Wire format

QRTR gives every socket an implicit port, so there is no QMUX control-service
client-ID allocation. Requests go straight to the service's `{node, port}`:

```
QMI header (7 bytes, little-endian)
  u8  message type   0x00 request, 0x02 response, 0x04 indication
  u16 transaction id
  u16 message id
  u16 payload length
TLVs
  u8  tag
  u16 length
  ... value
```

Service and message IDs used:

| Name | ID |
| --- | --- |
| UIM service | `0x0B` |
| `UIM_GET_CARD_STATUS` | `0x002F` |
| `UIM_SEND_APDU` | `0x003B` |
| `UIM_CLOSE_LOGICAL_CHANNEL` | `0x003F` |
| `UIM_OPEN_LOGICAL_CHANNEL` | `0x0042` |

`UIM_SWITCH_SLOT` (`0x0046`) is deliberately **not** implemented. The agent
never remaps slots, so an eUICC probe cannot disturb the active SIM.

### ES10

Commands are wrapped in the ISD-R STORE DATA APDU `80 E2 91 00 <Lc> <data> 00`
and sent on a logical channel opened against the GSMA ISD-R AID
`A0000005591010FFFFFFFF8900000100`.

| Operation | Command |
| --- | --- |
| `GetEID` | `BF3E 03 5C 01 5A` |
| `GetProfilesInfo` | `BF2D 00` |

The card answers with `61 XX`, and the payload is collected with GET RESPONSE
(`80 C0 00 00 XX`) until `90 00`. `6C XX` retries the command with the length
the card asked for. The chain is bounded at 32 reads.

## Safety boundary

Enforced in code, not by convention:

- **Read-only.** Only `GetEID` and `GetProfilesInfo` are ever constructed. There
  is no code path that can enable, disable, delete, download, rename, reset or
  re-provision a profile, or change PIN state.
- **Channels always close.** `ChannelGuard` closes the logical channel in `Drop`,
  so every early return, error and panic still releases it. The card has a small
  fixed number of channels and a leaked one is only recovered by a modem reset.
- **Serialized.** All card access is behind one process-wide mutex. Concurrent
  dashboard and Android polls would otherwise exhaust the channel pool.
- **Bounded.** A 10 second QMI timeout and a 32-read continuation cap mean a
  wedged card cannot hold an HTTP worker thread.
- **Masked.** EID and ICCID are masked to first four and last four digits.
  `?full=true` returns the whole value; the route is still authenticated and
  LAN-only.

Read `docs/SAFETY.md` before changing anything here.

## API

All routes require the usual bearer token.

### `GET /api/euicc/status`

```json
{"ok": true, "data": {
  "card_present": true,
  "euicc_available": true,
  "detail": "ISD-R selected successfully"
}}
```

`card_present` means a card is in the slot. `euicc_available` is the stronger
claim that the ISD-R selected — the only reliable proof the card is an eSIM,
since this vendor firmware carries eSIM strings and libraries on every SKU
regardless of what hardware is fitted.

### `GET /api/euicc/eid[?full=true]`

```json
{"ok": true, "data": {"eid": "8904************************8436", "masked": true}}
```

### `GET /api/euicc/profiles[?full=true]`

```json
{"ok": true, "data": {
  "profiles": [{
    "iccid": "8994************9299",
    "isdp_aid": "A0000005591010FFFFFFFF8900001100",
    "state": "enabled",
    "enabled": true,
    "class": "operational",
    "nickname": null,
    "service_provider": "MLS",
    "name": "Mobitel"
  }],
  "count": 1,
  "masked": true
}}
```

`state` is `enabled`, `disabled` or `unknown`. `class` is `operational`,
`provisioning`, `test` or `unknown`.

### `GET /api/capabilities`

Reports what this build serves, plus the features it deliberately does not, so
clients can hide them instead of calling them and surfacing a 404 to the user.

## Verifying on-device

The agent has a probe mode that runs the read-only path once and exits without
starting the server or touching boot state. Use it to confirm the transport
before deploying anything:

```sh
adb push target/aarch64-unknown-linux-musl/release/zte-agent /data/local/tmp/zte-agent-probe
adb shell "chmod +x /data/local/tmp/zte-agent-probe && /data/local/tmp/zte-agent-probe euicc-probe"
```

Expected output on a working eUICC (identifiers are masked in probe output too,
since this is the text most likely to be pasted into an issue):

```text
== eUICC read-only probe ==
card present   : true
eUICC (ISD-R)  : true
detail         : ISD-R selected successfully
EID            : 8904************************8436
profiles       : 1
  [0] 8994************9299 / enabled / operational / MLS
```

Run it several times: a channel leak shows up as the second run failing to open
the ISD-R.

## Profile management

Implemented by driving [`lpac`](https://github.com/estkme-group/lpac) rather
than reimplementing GSMA RSP. The agent is lpac's *backend*: lpac runs with
`LPAC_APDU=stdio` and `LPAC_HTTP=stdio`, the agent answers its APDU requests
using the QMI/QRTR transport above, and answers its HTTP requests either with
the device's `curl` or through the relay below.

Because both drivers are stdio, lpac links neither libcurl nor libqmi/glib. The
whole install is ~288 KiB. Build it with:

```sh
cmake -B build -G Ninja \
  -DCMAKE_TOOLCHAIN_FILE=<aarch64-musl toolchain> \
  -DLPAC_WITH_APDU_PCSC=OFF -DLPAC_WITH_HTTP_CURL=OFF \
  -DSTANDALONE_MODE=ON -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
  -DCMAKE_INSTALL_RPATH='$ORIGIN;$ORIGIN/lib'
```

Lay it out as `lpac`, `lib/*.so`, `driver/*.so` — the driver directory is found
relative to the binary's RPATH.

### Endpoints

| Route | Notes |
| --- | --- |
| `POST /api/euicc/download` | activation code, or SM-DP+ / matching ID / confirmation code / IMEI |
| `POST /api/euicc/enable` | see REFRESH below |
| `POST /api/euicc/disable` | refuses the last enabled profile unless `force` |
| `POST /api/euicc/delete` | refuses an enabled profile |
| `POST /api/euicc/nickname` | empty nickname clears it |
| `GET /api/euicc/notifications` | pending RSP notifications |
| `POST /api/euicc/notifications/process` | send, then remove |
| `POST /api/euicc/notifications/remove` | drop without sending |

All state-changing routes need `X-Confirm: true`.

Values that reach lpac's argv are rejected if empty, control-bearing or
`-`-prefixed, so nothing a client sends can be read as a flag. ICCIDs must be
18-22 digits.

### This modem cannot REFRESH

`ES10c EnableProfile` **fails** on this device when the refresh flag is set, and
succeeds without it. That is consistent with the rest of the firmware, which
exposes no usable STK/CAT path, so the card has no way to deliver a REFRESH
proactive command to the modem.

The practical effect: after enable or disable, the card really has switched, but
the modem keeps reading the old profile **until the router reboots**. Verified
directly — `lpac profile list` showed the new profile enabled while
`get_sim_info` still reported the old ICCID and IMSI, and a reboot resolved it.

So `refresh` defaults to off, and a successful switch returns:

```json
{"ok": true, "data": {"reboot_required": true, "notice": "..."}}
```

Clients must surface that. Silently switching and showing no change reads as a
failed operation.

## The relay: downloading without a WAN

A router being provisioned for the first time cannot reach the SM-DP+, because
its only WAN is the cellular link the profile would provide. There is no offline
alternative — RSP requires the server to mint the bound profile package.

The relay resolves it by letting something else on the LAN carry the traffic. A
phone joined to the router's Wi-Fi keeps mobile data active precisely because
that Wi-Fi has no internet, so it can reach both.

```text
  lpac --stdio--> agent --park--> GET  /api/euicc/relay/pending  (long poll)
  lpac <--------- agent <-------- POST /api/euicc/relay/response
```

Pass `"relay": true` to `/api/euicc/download` or
`/api/euicc/notifications/process`, and run a client:

```sh
python3 scripts/relay-client.py --agent http://192.168.0.1:9090 --password <pw>
```

**The client must be native.** A browser cannot do this: the requests are
cross-origin and SM-DP+ servers do not answer CORS preflights, so `fetch` is
refused before it is sent. `scripts/relay-client.py` is the reference
implementation and the model for the Android app.

Only one request is ever outstanding, because all card access is serialized.

### ES9+ TLS is not web PKI

Some SM-DP+ servers present a WebPKI certificate; others present one issued by a
GSMA Certificate Issuer, which no OS trust store carries. Truphone/1GLOBAL is
the latter, so a stock client fails with `unable to get local issuer
certificate` before the download begins.

`certs/` carries the GSMA RSP2 Root CI1, loaded *in addition to* the system
bundle. See `certs/README.md` for why that certificate can be trusted — briefly,
the eUICC itself names the same CA key id in its
`euiccCiPKIdListForVerification`, and that root verifies the live server leaf.

## Verified on hardware

Against the real card on `XCBZ_HK_MU5250V1.0.0B04`:

- read-only probe: ISD-R selects, EID reads, profiles enumerate, repeatable with
  no channel leak and no change to SIM or network state;
- `chip info`, read live and in full:

  | Field | Value |
  | --- | --- |
  | `euiccFirmwareVer` | 36.17.4 |
  | `profileVersion` | 2.3.1 (SGP.22) |
  | `globalplatformVersion` | 2.3.0 |
  | `sasAcreditationNumber` | KN-DN-UP-0924 |
  | `extCardResource.freeNonVolatileMemory` | 1 455 008 bytes (~1.39 MB) |
  | `rspCapability` | `additionalProfile`, `testProfileSupport`, `deviceInfoExtensibilitySupport` |
  | `forbiddenProfilePolicyRules` | `ppr1` |

  `extCardResource` is an **object** of byte counts, not the raw BER-TLV string
  — clients must read `freeNonVolatileMemory` from it. That is the number that
  decides whether another profile will fit.
- **download**: a profile installed through the relay with the full RSP flow —
  challenge, `initiateAuthentication`, `authenticateServer`,
  `authenticateClient`, metadata parse, `prepareDownload`,
  `getBoundProfilePackage`, `loadBoundProfilePackage` — landing as a second,
  disabled profile alongside the existing one;
- notification delivered to the SM-DP+ and removed from the card;
- enable/disable switch the card correctly, and require a reboot to take effect.

### Notifications can outlive their server

This card carried two undeliverable notifications whose address,
`mls.prod.ondemandconnectivity.com`, is NXDOMAIN — the operator retired that
SM-DP+ hostname. The address is written into the profile at download time and
cannot be changed; a current one only arrives with a re-issued profile. Expect
this and let the user drop such notifications with
`/api/euicc/notifications/remove`.

### The card names its own trust anchor and discovery servers

`chip info` also returns three things worth acting on:

```
euiccCiPKIdListForVerification  81370f5125d0b1d408d4c3b232e6d25e795bebfb
EuiccConfiguredAddresses.defaultDpAddress  smdp-plus-0.eu.cd.rsp.kigen.com
EuiccConfiguredAddresses.rootDsAddress     lpa.ds.gsma.com
```

The first is the subject key identifier of the GSMA RSP2 Root CI1 in `certs/`.
The card naming the same key it will verify against is the independent
confirmation that the certificate there is the right one — see
`certs/README.md`.

`rootDsAddress` is the GSMA root discovery server, so **SM-DS discovery is
reachable on this card**: a profile can be found without an activation code by
asking `lpa.ds.gsma.com` what is waiting for this EID.

## Still to do

- never log activation codes, EID, ICCID, IMSI or bound profile packages —
  currently upheld by review, not by a lint;
- Android UI for profile management;
- SM-DS discovery, now that the card has been confirmed to carry a root DS
  address.

## References

- GSMA SGP.22 (RSP Technical Specification) — ES10 command definitions
- [`estkme-group/lpac`](https://github.com/estkme-group/lpac) — reference ES10 parsing
- [`damonto/euicc-go`](https://github.com/damonto/euicc-go) and
  [`damonto/wwan-go`](https://github.com/damonto/wwan-go) — QMI/QRTR UIM driver
  used to confirm the wire constants
