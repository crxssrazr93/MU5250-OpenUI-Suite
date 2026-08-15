# Using the suite

This guide gives step-by-step instructions for every feature, in all three
front ends.

There are three ways to control the router. They share one agent, so an action
in one front end shows up in the others.

- **Dashboard.** The router serves it at `http://192.168.0.1:8080`. Open it in a
  browser on the router's network.
- **Desktop app.** This is the same dashboard in a native window (Windows,
  macOS, Linux). It sends its requests through a small Rust layer. That layer
  lets it do a few things a browser cannot.
- **Android app.** This is a separate native app. It talks to the same agent.

Each feature below names where it lives in the dashboard and in the Android app.
The desktop app matches the dashboard exactly.

## Contents

- [Sign in](#sign-in)
- [Home and signal](#home-and-signal)
- [Operator selection](#operator-selection)
- [Band lock and cell lock](#band-lock-and-cell-lock)
- [SIM and PIN](#sim-and-pin)
- [eSIM: download and switch profiles](#esim)
- [Wi-Fi](#wi-fi)
- [Connected devices](#connected-devices)
- [LAN, DHCP and DNS](#lan-dhcp-and-dns)
- [WireGuard VPN](#wireguard-vpn)
- [APN](#apn)
- [Mobile data](#mobile-data)
- [TTL override](#ttl-override)
- [SMS](#sms)
- [Firewall and telemetry blocker](#firewall-and-telemetry-blocker)
- [USB mode](#usb-mode)
- [Device controls and scheduled reboot](#device-controls)
- [Tools](#tools)

---

## Sign in

The agent needs a password. It answers only on the router's own network.

1. Connect your phone or computer to the router. Use Wi-Fi or a USB tether.
2. Open the dashboard, the desktop app, or the Android app.
3. Enter the agent password. The app keeps the session until it expires or the
   agent restarts. Then the app signs in again on its own.

The agent locks out repeated wrong passwords. If it shows "too many attempts",
wait the number of seconds it names. An earlier retry only extends the lockout.

---

## Home and signal

**Dashboard:** the Home tab, and Signal, Overview.
**Android:** the Dashboard tab, and the Signal screen.

The home view is read-only. It shows the current network, the signal strength,
the data connection state, the client count, the battery, and the temperature.
The signal overview adds the detailed radio figures. These are RSRP, RSRQ, SINR,
band, and cell. The view refreshes on a timer.

To change the refresh rate, use the control in Settings. At a one-second rate the
numbers update live. The pull-to-refresh animation appears only when you pull
down by hand. It does not appear on an automatic refresh.

---

## Operator selection

Select the mobile network by hand. The modem does not choose for itself.

**Dashboard:** Signal, Operator.
**Android:** Router settings, Cellular, Operator Selection.

1. Press **Scan** and confirm. The modem examines every band for about 40
   seconds. Mobile data stops during the scan. Do not scan while you need the
   connection.
2. Wait for the list. Each network shows a name, a code, a radio type, and a
   status. The status is available, current, or forbidden.
3. Press **Use** on a network and confirm. The modem then stays on that network,
   even when the signal is weak.
4. To reverse this, press **Automatic**. The modem chooses for itself again.

A forbidden network is barred for your SIM. Registration will most likely fail.
The router can lose service until you return to automatic.

---

## Band lock and cell lock

These features restrict which radio bands, or which single cell, the modem uses.
This helps when the modem keeps leaving a band or a cell that you prefer.

**Band lock. Dashboard:** Signal, Mode & Locking. **Android:** Tools, Band Lock.

1. The screen shows the current lock. Select the LTE or NR (5G) bands that you
   want.
2. Press the LTE lock button or the NR lock button. The app sends the band list.
   The agent converts LTE bands into the vendor bitmask for you. You select plain
   band numbers, not a mask.
3. To remove a lock, use Unlock all.

NR locking on this firmware is SA (standalone) only.

**Cell lock. Dashboard:** Signal, Mode & Locking. **Android:** Router settings,
Cellular, Cell Lock.

1. Enter the PCI. You can also enter the EARFCN and the band, for the LTE or NR
   cell.
2. Press **Lock**. The agent selects the radio from the PCI that you filled in.
3. Press **Unlock** to clear the lock. You can also scan neighbour cells first,
   to see what is in range.

---

## SIM and PIN

**Dashboard:** the signal and modem views show this.
**Android:** Router settings, Cellular, SIM Card.

The SIM screen reads the ICCID, IMSI, operator, MSISDN, and slot. It also manages
the PIN.

- **Verify PIN.** Enter the PIN when the SIM asks for it.
- **Change PIN.** Enter the current PIN and a new PIN.
- **Enter PUK.** After too many wrong PINs, enter the PUK and set a new PIN.
- **Unlock NCK.** Enter the network unlock code for a carrier-locked modem.

The modem limits PUK and NCK attempts. The screen shows how many remain. A used-up
PUK disables the SIM permanently.

---

## eSIM

Download and switch eSIM profiles on the router's built-in eUICC.

**Dashboard:** Modem, eSIM. **Desktop app:** the same. **Android:** Router
settings, Cellular, eSIM.

### The one rule that catches everyone: it needs the internet

A profile download is a live conversation with your operator's server (the
SM-DP+). A profile switch also reports back to the operator. Both need a route to
the internet. There are two ways to provide one.

- **The router already has a working data connection.** You do nothing extra.
  The agent reaches the operator directly. This is the normal case. You already
  have one working profile, and you add or switch to another.
- **The router has no internet yet.** This happens with a new eUICC, or when the
  only profile is disabled. The profile would provide the cellular link, but the
  download needs that same link. A **relay** breaks this loop. A relay is another
  device on the router's network that has its own internet and carries the
  traffic.

### Carry the relay

- **Android app.** The phone carries the relay for you. On the eSIM screen, set
  **Router has no internet** to on. Keep the phone's mobile data on. Keep the
  phone connected to the router's Wi-Fi. The phone can then reach both sides.
- **Dashboard or desktop app.** A browser cannot carry this traffic. Run the
  relay client on a computer or phone that can reach both the router and the
  internet:

  ```sh
  python3 scripts/relay-client.py --agent http://192.168.0.1:9090 --password <pw>
  ```

  Keep it running. Then start the download from the dashboard with the relay
  option set to on.

### Download a profile

1. Open the eSIM screen. Press **Add** (dashboard), or open Add a profile
   (Android).
2. Provide the profile in one of three ways:
   - **Activation code.** Paste the `LPA:1$...` string from your operator.
   - **Scan QR.** On the dashboard, select the QR image. The browser decodes it
     and does not upload it. On Android, select the QR image. The phone decodes
     it.
   - **Enter manually.** Type the SM-DP+ address and the matching ID separately.
     This suits operators who send those as text. Add a confirmation code or an
     IMEI only when the operator issued one.
3. If the router has no internet, start the relay first. See above.
4. Press **Download profile**. It talks to your operator. It can take a minute.
   Do not close the screen. Progress lines appear as it works.
5. The new profile arrives **disabled**, next to any existing profile.

### Switch which profile is active

1. In the profile list, press **Enable** on the profile that you want. Or press
   **Disable** on the active profile.
2. A notice says that a **reboot is required**. This is normal on this modem. The
   card has switched, but the modem still reads the old profile until it
   restarts. This is not a failure.
3. Press **Reboot now**. Every connected device drops for about a minute. After
   the reboot the router is on the new profile.

If you disable the only enabled profile, the router has no mobile service until
you enable another one. The app warns you first.

### Rename and delete

- **Rename** gives a profile a nickname. Leave it blank to clear the nickname.
- **Delete** erases a profile from the card and tells the operator. Some
  operators then release the activation code for reuse. Many operators issue
  single-use codes, so treat a delete as permanent. You must disable a profile
  before you delete it.

### Undelivered notifications

A switch or a delete reports to the operator. If that report cannot reach the
operator, for example when the router had no internet, the app keeps it. It lists
the report as an undelivered notification. Connect a relay and press **Retry**.
Or press **Discard** when the operator's server is gone.

---

## Wi-Fi

**Dashboard:** Network, Wi-Fi. **Android:** Router settings, Connectivity, WiFi.

1. Edit the 2.4 GHz and 5 GHz network names, passwords, channels, and encryption.
2. Save. The radios restart for a moment, so Wi-Fi clients reconnect.

The stock router web UI manages guest Wi-Fi on this firmware. This app does not
duplicate it.

---

## Connected devices

**Dashboard:** Network, Clients. **Android:** Tools, Connected Devices.

This is a read-only list of connected clients and DHCP leases. It shows the
hostname, the IP, the MAC, the medium, and the signal for wireless clients.

---

## LAN, DHCP and DNS

**Dashboard:** Network, Router. **Android:** Router settings, Connectivity,
LAN / DHCP and DNS.

- **LAN / DHCP.** This sets the router's address, the DHCP range, and the lease
  time.
- **DNS.** This sets the resolvers for clients. This build can run a
  DNS-over-HTTPS proxy. When the proxy is on, open the DoH cache inspector to see
  what it resolved.

A change to the LAN address moves the whole dashboard to the new address.

---

## WireGuard VPN

Route the router's traffic through a WireGuard tunnel. This uses the firmware's
own tunnel stack.

**Dashboard:** Network, WireGuard. **Android:** Router settings, Connectivity,
WireGuard VPN.

The firmware holds one active tunnel. The app keeps a library of saved tunnels.
It copies one into the active slot when you activate it.

### Import a provider's tunnel

1. Press **Import**.
2. Give it a name. Then paste the provider's `.conf` exactly as given, or select
   the file. A retyped file is how a wrong character enters a key, so paste the
   file or load it.
3. Save. The app ignores DNS, MTU, and keepalive lines in the file. This firmware
   has nowhere to put them.

### Activate and connect

1. Press **Use** on a saved tunnel. This copies it into the active config.
2. To configure a peer by hand instead, press **Generate** for a key pair. The
   private half stays on the router. Give the shown public key to the peer. Then
   fill in the peer and addressing fields and press **Save**.
3. Set the **Connection** switch to on. This becomes available once the required
   fields are present. The required fields are the peer public key, the router
   address, and the peer endpoint.

A new key overwrites the old one. Every peer that uses the old public key then
stops accepting the router, until you give them the new key. A delete removes the
tunnel's private key. Most providers issue that key only once.

If the screen says the `wg` tool is not installed, run `scripts/zharden.sh`. It
installs the tool.

---

## APN

**Dashboard:** Modem, APN. **Android:** Router settings, Connectivity, APN.

This screen manages the cellular access point names. You can view the profiles,
add or edit one, and set the active profile. An APN change drops the data
connection for a moment while the modem re-attaches.

---

## Mobile data

**Dashboard:** Modem, Data. **Android:** the Dashboard and mobile-network
screens.

This turns the mobile data connection on or off. It shows usage. It also shows
roaming, which is guarded. A data-off action drops the connection within a few
seconds. A data-on action reconnects on the same address. Roaming is a billing
event, so use it with care.

---

## TTL override

Rewrite the TTL (hop limit) on traffic that leaves the router. A carrier then
cannot find tethered devices by their reduced hop count.

**Dashboard:** Modem, TTL. **Android:** Tools, TTL Override.

1. Enter a TTL value from 1 to 255. The usual choice is 65.
2. Press **Enable**, or **Update** when a value is already set. It applies at
   once, to both IPv4 and IPv6. It stays across reboots.
3. Press **Disable** to remove the override.

---

## SMS

**Dashboard:** Modem, SMS. **Android:** the SMS tab.

- **Read.** The list shows messages newest first. When you open a message, the
  app marks it read.
- **Send.** Enter the destination number and the text, then send. The app handles
  non-ASCII text for you. A message costs whatever your operator charges.
- **Delete.** Remove one or more messages.

The number field takes a normal phone number, with an optional leading `+`.
Delivery depends on the network, the same as from a phone.

---

## Firewall and telemetry blocker

**Dashboard:** part of the Router and system views. **Android:** Router settings,
Security, Firewall and Telemetry Blocker.

- **Firewall.** These are the switches the agent serves: firewall on or off, NAT,
  port forwarding, port mapping, remote admin, and WAN ping. Each switch shows
  the router's real state. You can list and edit the port-forward rules.
- **Telemetry blocker.** This is a domain filter. It blocks the vendor's
  telemetry hosts. You can set each rule on or off.

This app does not offer VPN passthrough or QoS. The firmware has no surface for
them, so a switch would write nowhere. See MOBILE-API-GAP.md.

---

## USB mode

**Dashboard:** the system tools. **Android:** Tools, USB Mode.

This changes what the router presents over USB. Examples are a network gadget or
mass storage. Set it through the app. The agent refuses to write the composition
node live, because a live write can wedge the port.

---

## Device controls

**Dashboard:** System, Settings. **Android:** Router settings, System, Device
Controls and Scheduled Reboot.

- **Reboot.** This restarts the router. Every connected device drops for about a
  minute. The eSIM switch also points you to this button.
- **Shutdown.** This powers the router off.
- **Charge control.** This caps the battery charge level to protect the battery.
  The limiter starts charging again when you unplug and replug, or when you
  disable it.
- **Scheduled reboot.** This sets a time and days for the router to restart on
  its own.

The reboot, shutdown, and charge actions are guarded. The app asks before it
sends them.

---

## Tools

**Android:** the Tools tab. **Dashboard:** System, Tools and Metrics.

- **Device Info.** Hardware, firmware, and identifiers.
- **Metrics.** CPU, memory, thermal, and battery over time.
- **Process Monitor.** The running processes. You can stop known bloat daemons.
- **Speed test and LAN speed test.** These measure the internet link, or the
  Wi-Fi link to the router.
- **AT Terminal.** This sends raw AT commands to the modem. Use it only when you
  know the commands.
- **Config Decrypt/Encrypt.** An offline tool for the router's backup config
  files.
- **Enable ADB.** This starts USB debugging.
- **Scheduler.** This automates tasks on a timetable.

Some entries carry warnings, or the app guards them. They can interrupt service
or change how the device starts. Read [SAFETY.md](SAFETY.md) before you use the
lower-level tools.
