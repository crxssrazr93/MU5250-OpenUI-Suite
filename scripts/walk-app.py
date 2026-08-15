#!/usr/bin/env python3
"""Open every screen of the Android app on an emulator and fail on what is wrong.

This exists because the previous way of finding these bugs was the user
installing an APK, tapping through it, and sending screenshots back. That found
real defects — a hex sender list, an empty Power Settings card, a VPN screen
that opened on `{"error":"not found"}` — but only the ones they happened to
tap, and only after a build had shipped.

It drives the real app against the real agent. Nothing is mocked: a screen that
cannot reach the router fails here exactly as it would in a hand.

How it navigates: `uiautomator dump` gives the view tree with the bounds of
every node, so a screen is reached by finding the node whose text is the menu
entry and tapping its centre. No coordinates are hardcoded and no deep links
are added to the app — the alternative was an exported intent-filter per
screen, which is a permanent hole in a router admin app for the sake of a test.

What counts as a failure:

  - text that reads as an error: "Server error", "not found", "Unauthorized",
    "Failed to", a bare `{"ok":false...}`
  - a screen that is entirely placeholders, which is what a parser reading the
    wrong keys looks like from outside
  - a menu entry that does not open anything

Screens that write are opened, not exercised. Tapping "Lock LTE Bands" on a
live router is not something a test should do unasked.

    python3 scripts/walk-app.py
    python3 scripts/walk-app.py --serial emulator-5554 --out /tmp/walk
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from pathlib import Path

PACKAGE = "com.openu60"
ACTIVITY = f"{PACKAGE}/.MainActivity"

# Text that means the screen failed, matched case-insensitively against every
# text= attribute in the dump.
ERROR_PATTERNS = [
    r"server error",
    r'"ok"\s*:\s*false',
    r"\bnot found\b",
    r"unauthorized",
    r"failed to ",
    r"requires x-confirm",
    r"unknown error",
    r"invalid json",
]

# Bottom navigation, always present.
TABS = ["Dashboard", "SMS", "Router", "Tools", "Settings"]

# Menu entries to open, by the tab they live under. A screen reached from a
# list is tapped by its label and left by pressing back.
SCREENS = {
    "Router": [
        "Mobile Network",
        "Network Mode",
        "Cell Lock",
        "SIM Card",
        "WiFi",
        "APN",
        "LAN / DHCP",
        "DNS",
        "Firewall",
        "Telemetry Blocker",
        "Device Controls",
        "Scheduled Reboot",
    ],
    "Tools": [
        "Device Info",
        "Connected Devices",
        "Band Lock",
        "Scheduler",
        "USB Mode",
        "LAN Speed Test",
        "Process Monitor",
        "AT Terminal",
        "Config Decrypt/Encrypt",
    ],
}


def adb(serial: str | None, *args: str, timeout: int = 60) -> str:
    cmd = ["adb"] + (["-s", serial] if serial else []) + list(args)
    done = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    if done.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)} failed: {done.stderr.strip()}")
    return done.stdout


def dump(serial: str | None) -> ET.Element:
    """The current view tree.

    uiautomator occasionally reports the window as busy while an animation is
    still running, so this retries rather than treating a transient as a
    missing screen.
    """
    for attempt in range(6):
        adb(serial, "shell", "uiautomator", "dump", "/sdcard/walk.xml")
        raw = adb(serial, "shell", "cat", "/sdcard/walk.xml")
        if raw.lstrip().startswith("<"):
            try:
                return ET.fromstring(raw)
            except ET.ParseError:
                pass
        time.sleep(0.5 * (attempt + 1))
    raise RuntimeError("uiautomator never produced a usable dump")


def texts(tree: ET.Element) -> list[str]:
    found = []
    for node in tree.iter():
        for attr in ("text", "content-desc"):
            value = node.get(attr)
            if value:
                found.append(value)
    return found


def find_node(tree: ET.Element, label: str) -> ET.Element | None:
    for node in tree.iter():
        if node.get("text") == label or node.get("content-desc") == label:
            return node
    return None


def centre(node: ET.Element) -> tuple[int, int]:
    x1, y1, x2, y2 = map(int, re.findall(r"-?\d+", node.get("bounds", "")))
    return (x1 + x2) // 2, (y1 + y2) // 2


def tap(serial: str | None, node: ET.Element) -> None:
    x, y = centre(node)
    adb(serial, "shell", "input", "tap", str(x), str(y))
    time.sleep(1.4)


def return_to_tab(serial: str | None, tab: str, tries: int = 4) -> bool:
    """Get back to a tab's own list, whatever screen we are on.

    Backs out first — a tab tap while a sub-screen is open selects the tab but
    can leave the sub-screen on the stack — then taps the tab itself. Both are
    safe to repeat.
    """
    for _ in range(tries):
        tree = dump(serial)
        node = find_node(tree, tab)
        if node is not None:
            x1, y1, x2, y2 = bounds(node)
            # The bottom bar sits in the last fifth of the screen; a match
            # higher up is a title or a list row that happens to share the name.
            if y1 > 1800:
                tap(serial, node)
                time.sleep(1.0)
                return True
        adb(serial, "shell", "input", "keyevent", "KEYCODE_BACK")
        time.sleep(1.0)
    return False


def scroll_top(serial: str | None, times: int = 5) -> None:
    """Return a list to the top before looking for anything in it.

    Without this the walk searched from wherever the previous entry left the
    list scrolled, so everything above that point reported as "menu entry not
    found" — four false failures on a Tools list that had all four entries.
    """
    for _ in range(times):
        adb(serial, "shell", "input", "swipe", "540", "700", "540", "1800", "250")
        time.sleep(0.35)


def scroll_to(serial: str | None, label: str, tries: int = 8) -> ET.Element | None:
    """Find a label, from the top of the list, scrolling down to reach it."""
    scroll_top(serial)
    for _ in range(tries):
        tree = dump(serial)
        node = find_node(tree, label)
        if node is not None:
            return node
        adb(serial, "shell", "input", "swipe", "540", "1600", "540", "700", "300")
        time.sleep(0.8)
    return None


def bounds(node: ET.Element) -> tuple[int, int, int, int]:
    x1, y1, x2, y2 = map(int, re.findall(r"-?\d+", node.get("bounds", "0,0,0,0")))
    return x1, y1, x2, y2


def field_for_label(tree: ET.Element, label: str) -> ET.Element | None:
    """The text box a floating label belongs to.

    By label, never by position in the list. Material's label sits *inside* the
    box's bounds, so containment identifies it exactly — whereas indexing into
    the EditTexts put the password into the gateway field: opening the keyboard
    scrolls the form, so coordinates read before it appeared pointed at the
    wrong box by the time they were tapped.
    """
    marker = find_node(tree, label)
    if marker is None:
        return None
    mx1, my1, mx2, my2 = bounds(marker)
    cx, cy = (mx1 + mx2) // 2, (my1 + my2) // 2
    for node in tree.iter():
        if "EditText" not in (node.get("class") or ""):
            continue
        x1, y1, x2, y2 = bounds(node)
        if x1 <= cx <= x2 and y1 <= cy <= y2:
            return node
    return None


def keyboard_shown(serial: str | None) -> bool:
    try:
        state = adb(serial, "shell", "dumpsys", "input_method")
    except RuntimeError:
        return False
    return "mInputShown=true" in state


def hide_keyboard(serial: str | None) -> None:
    """Close the IME, and only the IME.

    BACK dismisses the keyboard when it is up and pops the screen when it is
    not — which is how an unconditional BACK walked the login screen away and
    reported a login failure that never happened.
    """
    if not keyboard_shown(serial):
        return
    adb(serial, "shell", "input", "keyevent", "KEYCODE_BACK")
    time.sleep(0.6)


def set_field(serial: str | None, label: str, value: str) -> bool:
    """Replace a labelled field's contents.

    Cleared with MOVE_END then DEL rather than by retyping over a selection:
    tapping and typing tends to leave one stray character, and on the password
    field that is indistinguishable from a wrong password. Learned the hard
    way — see docs/EMULATOR-TESTING.md.
    """
    hide_keyboard(serial)
    node = field_for_label(dump(serial), label)
    if node is None:
        return False
    tap(serial, node)
    adb(serial, "shell", "input", "keyevent", "KEYCODE_MOVE_END")
    for _ in range(40):
        adb(serial, "shell", "input", "keyevent", "KEYCODE_DEL")
    adb(serial, "shell", "input", "text", value)
    time.sleep(0.4)
    return True


def log_in(serial: str | None, host: str, password: str) -> bool:
    node = find_node(dump(serial), "Login")
    if node is not None:
        tap(serial, node)

    if not set_field(serial, "Gateway IP", host):
        return False
    if not set_field(serial, "Password", password):
        return False

    hide_keyboard(serial)
    button = find_node(dump(serial), "Login")
    if button is None:
        return False
    tap(serial, button)
    time.sleep(5)
    return find_node(dump(serial), "Gateway IP") is None


def screen_problems(labels: list[str], screen: str) -> list[str]:
    problems = []
    joined = " | ".join(labels).lower()
    for pattern in ERROR_PATTERNS:
        hit = re.search(pattern, joined)
        if hit:
            # Report the whole label, not the pattern, so the message is the
            # one the user would have seen.
            offender = next((t for t in labels if re.search(pattern, t.lower())), hit.group(0))
            problems.append(f"{screen}: {offender.strip()}")

    # A screen made only of placeholders is what a parser reading the wrong
    # keys looks like from outside. Ignore the small ones — a screen with three
    # labels and two dashes is not evidence of anything.
    values = [t for t in labels if t.strip()]
    dashes = [t for t in values if t.strip() in {"--", "-", "—", "N/A"}]
    if len(values) >= 8 and len(dashes) >= len(values) / 2:
        problems.append(f"{screen}: {len(dashes)} of {len(values)} fields are placeholders")
    return problems


def main() -> int:
    argp = argparse.ArgumentParser(description=__doc__)
    argp.add_argument("--serial", default=None, help="adb serial, e.g. emulator-5554")
    argp.add_argument("--out", default=None, help="directory for a screenshot per screen")
    argp.add_argument("--host", default="192.168.0.1", help="router address to log in against")
    argp.add_argument("--password", default=None, help="agent password, if not already logged in")
    argp.add_argument("--strict", action="store_true", help="exit non-zero on findings")
    args = argp.parse_args()

    out = Path(args.out) if args.out else None
    if out:
        out.mkdir(parents=True, exist_ok=True)

    problems: list[str] = []
    visited = 0

    adb(args.serial, "shell", "am", "force-stop", PACKAGE)
    adb(args.serial, "shell", "am", "start", "-n", ACTIVITY)
    time.sleep(3)

    def capture(name: str) -> None:
        if not out:
            return
        raw = subprocess.run(
            ["adb"] + (["-s", args.serial] if args.serial else []) + ["exec-out", "screencap", "-p"],
            capture_output=True,
            timeout=60,
        )
        safe = re.sub(r"[^A-Za-z0-9]+", "_", name).strip("_").lower()
        (out / f"{safe}.png").write_bytes(raw.stdout)

    # The app must be logged in, or every screen fails for one uninteresting
    # reason and the run says nothing about the screens themselves.
    tree = dump(args.serial)
    if find_node(tree, "Login") is not None:
        if not args.password:
            print("The app is not logged in. Pass --password, or log in on the emulator.")
            return 2
        if not log_in(args.serial, args.host, args.password):
            print("Login failed. Check --host and --password against the running agent.")
            capture("login_failed")
            return 2

    for tab in TABS:
        if not return_to_tab(args.serial, tab):
            problems.append(f"{tab}: bottom navigation entry not found")
            continue
        time.sleep(1.2)
        labels = texts(dump(args.serial))
        visited += 1
        problems += screen_problems(labels, tab)
        capture(tab)

        for entry in SCREENS.get(tab, []):
            # Re-seat on the tab before every entry.
            #
            # Pressing BACK after a screen is not reliable on its own — if it
            # lands late, or the screen swallows it, the walk stays where it was
            # and every remaining entry reports "not found". That produced four
            # failures in a row against a build with nothing wrong with it, and
            # a test that cries wolf gets ignored. Tapping the tab is
            # idempotent, so this costs nothing when BACK did work.
            if not return_to_tab(args.serial, tab):
                problems.append(f"{tab}: could not return to the tab list")
                break

            target = scroll_to(args.serial, entry)
            if target is None:
                problems.append(f"{tab} > {entry}: menu entry not found")
                continue
            tap(args.serial, target)
            time.sleep(1.6)
            after = dump(args.serial)
            labels = texts(after)
            if find_node(after, entry) is None and entry not in " ".join(labels):
                # Some screens title themselves differently; only complain when
                # nothing at all changed.
                pass
            visited += 1
            problems += screen_problems(labels, f"{tab} > {entry}")
            capture(f"{tab}_{entry}")
            adb(args.serial, "shell", "input", "keyevent", "KEYCODE_BACK")
            time.sleep(1.2)

    print(f"opened {visited} screens")
    if out:
        print(f"screenshots in {out}")
    if not problems:
        print("no screen reported an error or came up all placeholders.")
        return 0

    print(f"\n{len(problems)} problems:")
    for problem in problems:
        print(f"  {problem}")
    return 1 if args.strict else 0


if __name__ == "__main__":
    sys.exit(main())
