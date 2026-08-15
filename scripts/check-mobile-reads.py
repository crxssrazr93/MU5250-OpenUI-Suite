#!/usr/bin/env python3
"""Call every read the Android app makes and check the keys its parsers need.

This is the check the other three do not make.

    check-api-contract.py     do the three clients agree on which paths exist?
    check-mobile-contract.py  does the agent serve every path the app calls?
    check-mobile-methods.py   with the verb it uses, and X-Confirm where needed?
    this                      and does the response contain what it then reads?

The last one is where the screens actually broke. Firewall read
`firewall_switch` from a response containing `firewall_enabled`, so every
switch showed off no matter how the router was configured; the path existed,
the verb was right, the call returned 200, and nothing reported it. Same
disease as the SIM screen reading `available_trials` and the CPU tile reading
`usage_percent`.

The keys are **read out of the parser source**, not listed here. A hand-written
list is the same bug one level up: it agrees with the parser on the day it is
written and silently stops. Only the route-to-parser mapping is declared below,
and that mapping is checked — a parser that no longer exists fails the run.

`check-field-contract.py` casts a wider net: every key the app reads anywhere
against every key any route returned. That makes it good at finding suspects
and bad at being a gate, because a key that only appears in a state the router
is not currently in looks identical to a typo. This one is narrow on purpose —
one route, one parser — so a miss is a defect rather than a suspect.

Writes are deliberately not exercised. Locking a band, adding a firewall rule
or sending a USSD code all change the router, and a test that has to be undone
is not a test anyone runs. Those bodies are covered by unit tests in the agent,
where the translation into vendor field names lives.

    python3 scripts/check-mobile-reads.py --password <agent-password>
    python3 scripts/check-mobile-reads.py --password ... --strict
"""
from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ANDROID = ROOT / "mobile/android/OpenU60/app/src/main/java/com/openu60"

# route -> the parser function whose top-level key reads must be satisfied.
#
# `body` is sent when the vendor's listing method needs one; a POST here is
# still a read. Keys marked optional are ones the firmware only reports in a
# state this unit is not in — each is named, not waved away.
CASES: list[dict] = [
    {"path": "/api/firewall/config", "parser": "FirewallParser.parseConfig"},
    {"path": "/api/firewall/port-forward", "parser": "FirewallParser.parsePortForwardRules"},
    {"path": "/api/firewall/domain-filter", "parser": "TelemetryParser.parseDomainFilter"},
    {"path": "/api/device/schedule-reboot", "parser": "ScheduleRebootParser.parse"},
    {
        "path": "/api/wifi/status",
        "parser": "WiFiParser.parse",
        # Reported only by firmware that has Wi-Fi 6; this unit reports
        # wifi6_supported false and omits the switch.
        "optional": ["wifi6_switch"],
    },
    {"path": "/api/network/lan", "parser": "LANParser.parse"},
    {"path": "/api/network/dns", "parser": "DNSParser.parse"},
    {
        "path": "/api/sim/info",
        "parser": "SIMParser.parseSIMInfo",
        # Present on dual-SIM firmware; this unit has one slot and omits it.
        "optional": ["sim1_provision_state"],
    },
    {"path": "/api/sim/lock", "parser": "SIMParser.parseSIMLock"},
    {"path": "/api/sms/list", "parser": "SMSParser.parseMessages", "body": {}},
    {"path": "/api/sms/capacity", "parser": "SMSParser.parseCapacity"},
    # Read directly in a view model rather than through a named parser, so the
    # keys are given here. Kept short deliberately: anything longer belongs in
    # a parser where it can be unit-tested.
    {
        "path": "/api/network/signal",
        "parser": None,
        "who": "BandLockViewModel.refresh, MobileNetworkViewModel.registerNetwork",
        "keys": ["lte_band_lock", "nr5g_sa_band_lock", "network_provider"],
    },
    {
        "path": "/api/cpu",
        "parser": None,
        "who": "DashboardViewModel.fetchSystem",
        "keys": ["overall", "cores"],
    },
    {
        "path": "/api/modem/data",
        "parser": None,
        "who": "DashboardViewModel.fetchMobileDataStatus",
        "keys": ["connect_status"],
    },
    # Reachability only: these feed models with no single parser entry point.
    {"path": "/api/modem/apn", "parser": None, "who": "APN list", "keys": []},
    {"path": "/api/modem/apn/mode", "parser": None, "who": "APN mode", "keys": []},
    {"path": "/api/modem/network-mode", "parser": None, "who": "Network mode", "keys": []},
    {"path": "/api/modem/neighbors", "parser": None, "who": "Cell lock neighbours", "keys": []},
    {"path": "/api/device", "parser": None, "who": "Device info", "keys": []},
    {"path": "/api/device/battery-info", "parser": None, "who": "Battery tile", "keys": []},
    {"path": "/api/network/clients", "parser": None, "who": "Clients list", "keys": []},
    {"path": "/api/usb/status", "parser": None, "who": "USB mode", "keys": []},
]


class ParserNotFound(Exception):
    pass


def _block(text: str, open_at: int) -> str:
    """The brace-matched block starting at the first `{` at or after open_at."""
    start = text.index("{", open_at)
    depth, i = 0, start
    while i < len(text):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                break
        i += 1
    return text[start : i + 1]


def parser_keys(qualified: str) -> tuple[list[str], list[list[str]]]:
    """Keys a Kotlin parser reads from its own Map argument.

    Returns (required, alternatives). An alternative group is a
    `data["a"] ?: data["b"]` fallback — the response has to carry one of them,
    not both, and demanding both would report a working parser as broken.

    Two things this deliberately does not count:

    - reads through any name other than the function's own parameter.
      `rule["fqdn"]` inside a loop reads an element of the response, not the
      response, so requiring it at the top level asks for the wrong shape.
    - functions of the same name in a neighbouring object. Several parsers in
      RouterSettingsModels.kt are called `parse`, and searching the whole file
      found whichever came first — which is how `net_select` came to be
      demanded of the Wi-Fi and LAN routes.
    """
    obj_name, fun_name = qualified.split(".")
    for source in ANDROID.rglob("*.kt"):
        text = source.read_text()
        marker = text.find(f"object {obj_name}")
        if marker < 0:
            continue
        scope = _block(text, marker)

        signature = re.search(
            rf"fun\s+{re.escape(fun_name)}\s*\(\s*(\w+)\s*:\s*Map<String,\s*Any\?>\s*\)",
            scope,
        )
        if not signature:
            continue
        param = signature.group(1)
        body = _block(scope, signature.end())

        pair = re.compile(rf'{param}\["([^"]+)"\]\s*\?:\s*{param}\["([^"]+)"\]')
        alternatives = [list(m) for m in pair.findall(body)]
        covered = {k for group in alternatives for k in group}
        required = sorted(
            set(re.findall(rf'{param}\["([^"]+)"\]', body)) - covered
        )
        return required, alternatives
    raise ParserNotFound(qualified)


def call(base: str, token: str, path: str, body: dict | None) -> tuple[bool, object]:
    request = urllib.request.Request(base + path)
    request.add_header("Authorization", f"Bearer {token}")
    if body is not None:
        request.add_header("Content-Type", "application/json")
        request.data = json.dumps(body).encode()
        request.method = "POST"
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            payload = json.load(response)
    except urllib.error.HTTPError as e:
        try:
            payload = json.load(e)
        except Exception:
            return False, f"HTTP {e.code}"
    except Exception as e:  # noqa: BLE001 — the reason is what we want to print
        return False, str(e)
    if not payload.get("ok"):
        return False, payload.get("error", "not ok")
    return True, payload.get("data")


def login(base: str, password: str) -> str:
    request = urllib.request.Request(base + "/api/auth/login", method="POST")
    request.add_header("Content-Type", "application/json")
    request.data = json.dumps({"password": password}).encode()
    with urllib.request.urlopen(request, timeout=20) as response:
        return json.load(response)["data"]["token"]


def main() -> int:
    argp = argparse.ArgumentParser(description=__doc__)
    argp.add_argument("--host", default="192.168.0.1")
    argp.add_argument("--port", type=int, default=9090)
    argp.add_argument("--password", required=True)
    argp.add_argument("--strict", action="store_true", help="exit non-zero on findings")
    args = argp.parse_args()

    base = f"http://{args.host}:{args.port}"
    try:
        token = login(base, args.password)
    except Exception as e:  # noqa: BLE001
        print(f"could not log in to {base}: {e}", file=sys.stderr)
        return 2

    failures: list[str] = []
    checked = key_count = 0

    for case in CASES:
        path = case["path"]
        qualified = case.get("parser")
        who = qualified or case.get("who", "?")
        optional = set(case.get("optional", []))

        alternatives: list[list[str]] = []
        if qualified:
            try:
                keys, alternatives = parser_keys(qualified)
            except ParserNotFound:
                failures.append(f"  {path:<30} {qualified} no longer exists — update this table")
                continue
        else:
            keys = list(case.get("keys", []))

        keys = [k for k in keys if k not in optional]
        key_count += len(keys) + len(alternatives)

        ok, data = call(base, token, path, case.get("body"))
        checked += 1
        if not ok:
            failures.append(f"  {path:<30} unreachable — {data}   [{who}]")
            continue
        if not isinstance(data, dict):
            if keys:
                failures.append(
                    f"  {path:<30} returned {type(data).__name__}, not an object   [{who}]"
                )
            continue
        missing = [k for k in keys if k not in data]
        for group in alternatives:
            if not any(k in data for k in group):
                missing.append(" or ".join(group))
        if missing:
            failures.append(f"  {path:<30} missing {', '.join(missing)}   [{who}]")

    print(f"checked {checked} reads and {key_count} parser keys against {base}")
    if not failures:
        print("every key a parser reads is present in the response that feeds it.")
        return 0

    print(f"\n{len(failures)} reads do not carry what the app reads from them:")
    print("\n".join(failures))
    print("\nThe path is served and the call succeeds; the screen renders blank.")
    print("Fix the parser to match the response, or the handler to emit the key.")
    return 1 if args.strict else 0


if __name__ == "__main__":
    sys.exit(main())
