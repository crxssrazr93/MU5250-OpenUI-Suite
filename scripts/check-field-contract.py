#!/usr/bin/env python3
"""Find JSON keys the apps read that no live agent response actually contains.

`check-api-contract.py` proves the three sides agree on which *paths* exist.
`check-live-api.py` proves those paths return something. Neither catches the
failure that put "--" in every field of the Android SIM screen: the path is
served, the call succeeds, and the parser then reads a key that is not there.

Two real examples this finds:

    /api/sim/lock   returns pin_attempts_left; the app reads available_trials
    /api/cpu        returns overall;           the app reads usage_percent

The check is deliberately blunt. It takes the union of every key the app reads
and the union of every key the agent returns, then reports the difference. That
avoids having to map each parser to the endpoint that feeds it — a mapping that
would go stale silently, which is the same disease as the bug.

The cost of bluntness is false positives, and they are worth naming rather than
hiding: a key that only appears in a state the router is not currently in looks
identical to a typo from here. So this reports *suspects*, not verdicts, and
IGNORE below records the ones that have been checked by hand.

    python3 scripts/check-field-contract.py --password <agent-password>
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
SERVER_RS = ROOT / "agent/src/server.rs"

# data["key"] / json["key"] / obj["key"] as it appears in the Kotlin parsers.
#
# A trailing `=` excludes an assignment: `params["nr_pci"] = pci` is building a
# request, not reading a response, and counting it reported four cell-lock
# write parameters as keys the agent fails to send.
KEY_RE = re.compile(r'\b\w+\[\s*"([a-z0-9_][a-zA-Z0-9_]*)"\s*\](?!\s*=[^=])')

# Comments are stripped before scanning. A comment naming the key it replaced —
# `parseTraffic read data["statistics"]` — otherwise reads as a live call site,
# which turns writing down what was fixed into a new false positive.
COMMENT_RE = re.compile(r"//[^\n]*|/\*.*?\*/", re.DOTALL)
ROUTE_RE = re.compile(r'\(&Method::Get,\s*"(/api/[^"]+)"\)')

# Keys read from a nested object rather than a top-level response, or only
# present in a state this router is not in. Each one checked by hand.
IGNORE = {
    # Nested inside another payload, so they never appear as a top-level key.
    "code", "message", "data", "error", "ok", "result", "progress",
    # eUICC / lpac payloads: only present once a card operation has run.
    "iccid", "eid", "seqNumber", "notificationAddress", "isdpAid",
    "profileManagementOperation", "profileState", "profileNickname",
    "profileName", "serviceProviderName", "profileClass",
    # Only present while a scan or a transfer is actually in flight.
    "record_list", "running",
}

# Screens for features that were declined after the code was written. They are
# still in the tree but nothing routes to them, so their keys match nothing and
# would drown out the real findings. Delete an entry here if a screen comes back.
DEAD_SCREENS = (
    "SMSForwardModels.kt",
    "speedtest/",
)

# Only GETs are swept, so a key read from a POST response is unmatched here for
# a reason that is not a bug. Listed rather than silently dropped.
POST_FED = (
    "SchedulerModels.kt",
    "SchedulerViewModel.kt",
    "NetworkToolModels.kt",
)

# Keys that are unmatched for a reason, with the reason.
#
# The point of writing these down is that the check can then *fail*. A list of
# ninety suspects nobody has been through is indistinguishable from a list of
# ninety bugs, so it gets skimmed and then ignored — which is how the firewall
# screen sat there reading `firewall_switch` for months while this script
# reported it every single run.
#
# Anything not matched and not listed here is a defect until someone shows
# otherwise. Delete an entry rather than editing it if the reason stops being
# true.
EXPLAINED: dict[str, str] = {}

# Keys whose presence depends on what the router is doing right now, so they
# match on one run and not the next. They are exempt from the stale check
# below: reporting them as an entry that outlived its key just means the modem
# happened to be attached to NR when the sweep ran.
VOLATILE: set[str] = set()


def _explain(reason: str, *keys: str, volatile: bool = False) -> None:
    for key in keys:
        EXPLAINED[key] = reason
        if volatile:
            VOLATILE.add(key)


# Read from an element inside a response rather than from the response, so they
# never appear as a top-level key of any route.
_explain(
    "nested inside a message row from POST /api/sms/list",
    "content", "date", "draft_group_id", "mem_store", "messages", "number", "tag",
)
_explain(
    "nested inside a neighbour or scan record",
    "fields", "band", "earfcn", "pci", "rsrp", "rsrq", "sinr", "rat",
)
_explain("nested inside a rule or config object", "config", "type")

# eUICC reads need a card operation to have run; the four /api/euicc routes
# return nothing until then, which the sweep reports separately.
_explain(
    "eUICC profile or chip detail, only present after a card read",
    "class", "defaultDpAddress", "euiccFirmwareVer", "extCardResource",
    "freeNonVolatileMemory", "isdp_aid", "nickname", "profileVersion",
    "reboot_required", "rootDsAddress", "sasAcreditationNumber", "service_provider",
    "body_hex",
    # Volatile for two reasons: the group appears only once a card operation
    # has run, and "class" is generic enough that an unrelated route serves a
    # key by that name, which made it look like a dead entry.
    volatile=True,
)

# Reported only in a state this unit is not in. Not a typo, and not something a
# sweep of one router at one moment can tell apart from one.
_explain(
    "NR-only; present only while attached to NR, absent on LTE",
    "nr5g_action_band", "nr5g_action_channel", "nr5g_bandwidth", "nr5g_cell_id",
    "nr5g_pci", "nrcasig", "nr5g_band",
    volatile=True,
)
_explain("dual-SIM firmware only; this unit has one slot", "sim1_provision_state")
_explain(
    "operator scan results, which arrive from a POST and only while scanning",
    "m_netselect_contents", "m_netselect_result", "m_netselect_status",
    "mcc_mnc", "operator_name", "plmn",
)

# Screens that are still in the tree but that nothing routes to, because the
# firmware has nothing behind them. Each decision is recorded in
# docs/MOBILE-API-GAP.md.
_explain(
    "VPN passthrough screen, unrouted: no passthrough surface exists",
    "ipsec_passthrough", "l2tp_passthrough", "pptp_passthrough",
)
_explain("QoS screen, unrouted: no vendor QoS surface located", "qos_switch")
_explain("Smart Tower Connect, no screen: the vendor verbs do nothing", "stc_enable")
_explain(
    "guest Wi-Fi screen, unrouted: /api/wifi/guest is not served",
    "disabled_2g", "disabled_5g", "encryption", "guest_active_time", "hidden",
    "isolate", "key", "ssid", "wifi6_switch",
)
_explain(
    "endpoint deliberately not served — see docs/MOBILE-API-GAP.md",
    "fast_boot", "power_saver_mode", "operate_mode",
)

# Fed by a POST response, which this sweep does not make.
_explain("returned by POST /api/system/kill-bloat", "freed_rss_kb", "killed")
_explain("returned by POST /api/at/send", "response")
_explain(
    "webhook action fields on a scheduler job, and the eUICC relay envelope — "
    "both POST-fed",
    "headers", "method", "url",
)
_explain("scheduler job fields, nested and POST-fed", "days", "time", "remaining_seconds")

# Deliberate fallbacks for the upstream agent's spelling, kept so an older
# agent still reads. The name this agent uses is handled beside them.
_explain(
    "fallback for the upstream agent's spelling; this agent uses another name",
    "load", "memory_free", "memory_total", "network_operator",
)

# Paths that must not be swept: long-polls, or side effects on a GET.
NEVER = {
    "/api/euicc/relay/pending",
    "/api/logger/signal/download",
    "/api/logger/connection/download",
}


def app_keys() -> dict[str, set[str]]:
    """Every key the Android sources read, and the files that read it."""
    found: dict[str, set[str]] = {}
    for path in ANDROID.rglob("*.kt"):
        source = COMMENT_RE.sub("", path.read_text())
        for key in KEY_RE.findall(source):
            found.setdefault(key, set()).add(str(path.relative_to(ROOT)))
    return found


def agent_get_routes() -> list[str]:
    return sorted(set(ROUTE_RE.findall(SERVER_RS.read_text())) - NEVER)


def call(base: str, token: str, path: str) -> object | None:
    request = urllib.request.Request(f"{base}{path}", method="GET")
    request.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            payload = json.load(response)
        return payload.get("data") if payload.get("ok") else None
    except Exception:  # noqa: BLE001 - one dead route must not stop the sweep
        return None


def keys_in(value: object, depth: int = 0) -> set[str]:
    """Every key anywhere in a response, nesting included."""
    if depth > 6:
        return set()
    if isinstance(value, dict):
        found = set(value)
        for nested in value.values():
            found |= keys_in(nested, depth + 1)
        return found
    if isinstance(value, list):
        found = set()
        for item in value[:5]:
            found |= keys_in(item, depth + 1)
        return found
    return set()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agent", default="http://192.168.0.1:9090")
    parser.add_argument("--password", required=True)
    parser.add_argument("--dump", help="write every live response to this file")
    parser.add_argument("-v", "--verbose", action="store_true", help="list the explained keys too")
    args = parser.parse_args()
    base = args.agent.rstrip("/")

    login = urllib.request.Request(
        f"{base}/api/auth/login",
        data=json.dumps({"password": args.password}).encode(),
        method="POST",
    )
    login.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(login, timeout=15) as response:
        token = json.load(response)["data"]["token"]

    served: set[str] = set()
    dump: dict[str, object] = {}
    silent_routes: list[str] = []
    for path in agent_get_routes():
        data = call(base, token, path)
        if data is None:
            silent_routes.append(path)
            continue
        dump[path] = data
        served |= keys_in(data)

    if args.dump:
        # Responses carry ICCID/IMSI/EID. Writing them is opt-in, and the path
        # is the caller's problem to keep out of the repo.
        Path(args.dump).write_text(json.dumps(dump, indent=2, sort_keys=True))
        print(f"wrote live responses to {args.dump}\n")

    reads = app_keys()
    unmatched = {k: v for k, v in reads.items() if k not in served and k not in IGNORE}

    def only_in(where: set[str], markers: tuple[str, ...]) -> bool:
        return all(any(m in f for m in markers) for f in where)

    dead = {k: v for k, v in unmatched.items() if only_in(v, DEAD_SCREENS)}
    post = {k: v for k, v in unmatched.items() if k not in dead and only_in(v, POST_FED)}
    explained = {
        k: v for k, v in unmatched.items()
        if k not in dead and k not in post and k in EXPLAINED
    }
    missing = {
        k: v for k, v in unmatched.items()
        if k not in dead and k not in post and k not in explained
    }

    print(f"swept {len(dump)} routes, {len(served)} distinct keys returned")
    print(f"app reads {len(reads)} distinct keys, {len(unmatched)} unmatched")
    print(f"  {len(dead)} only in declined-feature screens, {len(post)} only fed by POSTs")
    print(f"  {len(explained)} unmatched for a recorded reason")
    print(f"  {len(missing)} left to explain\n")

    if args.verbose and explained:
        print("unmatched, with the reason:")
        by_reason: dict[str, list[str]] = {}
        for key in sorted(explained):
            by_reason.setdefault(EXPLAINED[key], []).append(key)
        for reason, keys in sorted(by_reason.items()):
            print(f"  {reason}")
            print(f"    {', '.join(keys)}")
        print()

    # An entry that no longer matches anything is a reason that has outlived
    # its key. Left in place it makes the next real finding look explained.
    stale = sorted(set(EXPLAINED) - set(unmatched) - IGNORE - VOLATILE)
    if stale:
        print(f"EXPLAINED entries that no longer match anything ({len(stale)}):")
        print(f"  {', '.join(stale)}")
        print("  The key is served now, or the read is gone. Delete these.\n")

    if silent_routes:
        print(f"routes that returned nothing ({len(silent_routes)}):")
        for path in silent_routes:
            print(f"  {path}")
        print()

    if missing:
        print("keys the app reads that no response contained:")
        for key in sorted(missing):
            where = sorted(missing[key])[0].replace("mobile/android/OpenU60/app/src/main/java/com/openu60/", "")
            extra = f" (+{len(missing[key]) - 1})" if len(missing[key]) > 1 else ""
            print(f"  {key:32} {where}{extra}")

    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main())
