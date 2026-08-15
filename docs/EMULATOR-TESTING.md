# Running the Android app without a handset

The app was being tested by installing an APK on a phone and reading the screen.
That found bugs, but slowly, and only the ones on screens someone thought to
open. Two of the worst — every digit on the dashboard blank, and half the SIM
screen reading `--` — survived several rounds of endpoint work because the
contract checks all ask whether a *path* is served, and both paths were.

An emulator on the build machine can reach the router over the LAN, so the app
can be driven against the real agent, on real firmware, with no handset.

## One-time setup

The SDK needs the emulator package and a system image. KVM must be usable by
your user (`ls -l /dev/kvm`).

```sh
export ANDROID_HOME=~/Android/Sdk
cd $ANDROID_HOME/cmdline-tools/latest/bin
yes | ./sdkmanager --licenses
./sdkmanager "emulator" "system-images;android-35;google_apis;x86_64"   # ~1.7 GB
echo no | ./avdmanager create avd -n mu5250 \
    -k "system-images;android-35;google_apis;x86_64" -d pixel_6
```

## Each session

```sh
$ANDROID_HOME/emulator/emulator -avd mu5250 \
    -no-window -no-audio -no-snapshot -accel on \
    -gpu swiftshader_indirect -no-boot-anim &

until [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]
do sleep 5; done
```

Boots in about 70 seconds headless.

```sh
cd mobile/android/OpenU60
ANDROID_HOME=~/Android/Sdk ./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
adb shell monkey -p com.openu60 -c android.intent.category.LAUNCHER 1
```

## Pointing it at the router

The emulator NATs through the host, so `192.168.0.1` is reachable — confirm with
`adb shell ping -c2 192.168.0.1`. The app's gateway field defaults to `10.0.2.2`
(the emulator's own host alias), so it has to be changed to the router's address
on first run.

Two things make driving it by `adb input` less painful:

- `input text` lands in whatever field has focus, and focus after a `tap` is not
  always where you think. Screenshot before typing.
- To replace a field's contents, `KEYCODE_MOVE_END` then a run of `KEYCODE_DEL`.
  Clearing by tapping and retyping tends to leave a stray character, which on a
  password field looks exactly like a wrong password.

## Driving and looking

```sh
adb shell input tap <x> <y>
adb shell input swipe <x1> <y1> <x2> <y2> 300
adb shell input keyevent 4                  # back
adb shell screencap -p /sdcard/s.png && adb pull /sdcard/s.png .
```

Screencaps come out at the panel resolution (1080x2400 for a Pixel 6), so
coordinates read off a scaled screenshot need scaling back up.

`adb logcat -c` before an action and `adb logcat -d` after keeps the output to
what that action produced.

## What this is and is not for

It exercises the app against the real agent, so it catches parsing, layout and
missing-endpoint faults — everything that made screens read `--`. It is not a
substitute for a handset on anything touching the phone itself: no cellular
radio, no real battery, and the relay's long-poll behaviour under Android's
background limits is not represented.

Pair it with `scripts/check-field-contract.py`, which answers the other half:
whether the keys the app reads are keys the agent actually returns.

## Driving it automatically

Everything above is the manual version. `scripts/walk-app.py` does it in one
command: logs in, opens all 26 screens, and fails on any that shows an error or
comes up as nothing but placeholders.

```sh
python3 scripts/walk-app.py --password <agent-password> --out /tmp/walk
```

It navigates from `uiautomator dump`, finding the node whose text is the menu
entry and tapping its centre, so no coordinates are hardcoded and the app needs
no deep links — the alternative was an exported intent-filter per screen, a
permanent hole in a router admin app for the sake of a test.

Three things it has to get right, each learned by getting it wrong:

- **BACK is two different keys.** With the keyboard up it closes the keyboard;
  without it, it leaves the screen. An unconditional BACK during login walked
  the login screen away and reported a login failure that had not happened.
  `dumpsys input_method | grep mInputShown` decides which situation it is in.
- **Fields are found by label, never by index.** Material's floating label sits
  inside the box's bounds, so containment identifies it. Indexing into the
  EditTexts put the password into the gateway field, because opening the
  keyboard scrolls the form and coordinates read beforehand point somewhere
  else by the time they are tapped.
- **It has to re-seat itself.** If one BACK lands late, every later entry
  reports "menu entry not found" — four consecutive false failures against a
  build with nothing wrong with it. Tapping the tab before each entry is
  idempotent and costs nothing when BACK did work.

Screens that write are opened, not exercised. Tapping "Lock LTE Bands" against
a live router is not something a test should do unasked.

## The release build

Release had never been built: `isMinifyEnabled` was true while
`proguard-rules.pro` did not exist, and there was no signing config. Both are
in place now.

```sh
./gradlew assembleRelease          # app/build/outputs/apk/release/
```

No Play Store account is involved — a release APK needs a signing key and
nothing else. The key lives in `keystore.properties` next to the `.jks`, both
gitignored; without them the build falls back to the debug key so a fresh
checkout still builds.

R8 failures are runtime failures and a build that only has to compile cannot
see them, so verify a release build by installing it and running the walk
against it:

```sh
adb uninstall com.openu60          # release and debug keys differ
adb install app/build/outputs/apk/release/app-x86_64-release.apk
python3 scripts/walk-app.py --password <agent-password>
```

arm64-v8a comes out at 8.6 MB against the debug build's 29.6 MB.
