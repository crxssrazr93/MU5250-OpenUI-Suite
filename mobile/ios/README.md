# iOS app

Source only. **No binary has ever been produced from this, and no part of the
SwiftUI layer has been compiled**, because building for iOS needs Xcode and
there is no Mac in this project's toolchain. Treat the app target as a starting
point that will need fixing on first build, not as working software.

What *is* verified is the half that matters most, and it is verified properly.

## What is tested and what is not

`OpenU60Kit` is a SwiftPM package with no dependency on SwiftUI or UIKit, so it
builds and its tests run on Linux. It holds the agent client, the error
taxonomy, and every parser. It is checked two ways:

- **19 unit tests** against payload shapes that are checked in, so they stay
  fixed.
- **2 live tests**, skipped by default, that sign in to a real agent and read
  the routes the app opens with. These catch what frozen fixtures cannot: a key
  renamed by a firmware update, or a value that changes type.

Both suites pass, including against the router this was written on.

```sh
cd OpenU60Kit
swift test                                              # unit tests only
AGENT_HOST=192.168.0.1 AGENT_PASSWORD=… swift test      # adds the live ones
```

On Linux a toolchain from swift.org works. On Arch it needs `libncurses.so.6`,
which ships as `libncursesw.so.6`; symlink it and point `LD_LIBRARY_PATH` at it.

The app target — `OpenU60/` — is the untested part: five SwiftUI views and an
`AppModel`. It covers the overview and the SIM screen. It is deliberately far
short of the Android app, which has thirteen feature areas. Adding to it is
mostly a matter of reading a route through `AgentObject` and rendering it; the
client will not need changing.

## Why the parsers look the way they do

The firmware is not consistent about JSON types. `lte_rsrp` is a number,
`mdm_mcc` is the string `"413"`, `pin_status` is `"0"`, and the same key can
change type between firmware builds.

The Android client got this wrong in the other direction, and it cost most of a
day: it tried `intOrNull` before falling back to the string, so quoted values
were retyped as numbers and every `as? String` on them returned nil. Half the
SIM screen read `--`. The memorable one was an ICCID ending in `F`, because
Java's `parseDouble` reads a trailing `F` as a float suffix and turned a
twenty-digit identifier into `8.99e18`.

So `AgentValue` keeps the JSON type it was given and converts only when a caller
asks. Its string parsing is deliberately stricter than `Double(_:)`, which also
accepts `"1F"`, `"0x1p3"`, `"infinity"` and `"nan"`. There is a test for each.

## Building

```sh
brew install xcodegen
cd mobile/ios
xcodegen generate
open OpenU60.xcodeproj
```

The `.xcodeproj` is generated, not committed: it is large, it conflicts on every
merge, and nobody can review a diff of it. `project.yml` is the real definition.

Signing is off in `project.yml` so `xcodebuild build` works with no account. To
run on a device, set your team in Xcode.

## The two settings an iOS build fails without

Both are in `OpenU60/Info.plist`, and both are easy to lose when regenerating.

**`NSAllowsLocalNetworking`.** The agent is plain HTTP on the LAN, which App
Transport Security blocks. This key permits cleartext to private and link-local
addresses only, leaving the public internet HTTPS-only. Do not reach for
`NSAllowsArbitraryLoads` instead — it disables the rule everywhere, and it is
grounds for App Store rejection.

**`NSLocalNetworkUsageDescription`.** Required since iOS 14. Without it the
first request to a local address fails and no permission prompt appears at all,
which presents exactly like the router being unreachable. Worth knowing before
spending an afternoon on it.

The permission prompt appears on the first request, not when the address is
entered, so a refusal shows up as a failed sign-in rather than at the address
screen.

## Things that will need deciding

- **The eSIM relay.** The Android app carries the router's SM-DP+ traffic by
  long-polling the agent while the eSIM screen is open. iOS suspends background
  work more aggressively, and doing it honestly needs the screen to stay
  frontmost or a real background mode. Nothing here attempts it.
- **Reachability.** There is no check for being on the right Wi-Fi. A request
  to a private address from a phone on cellular simply fails, and the message
  says the agent is unreachable rather than why.
- **The password.** `AppModel` keeps it only long enough to sign in and never
  stores it. If it is ever persisted, that belongs in the Keychain, not
  `UserDefaults` — which is where the router address is kept, correctly, since
  it is not a secret.
