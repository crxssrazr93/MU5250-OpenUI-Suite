#!/usr/bin/env python3
"""Call every route the dashboard uses against a real agent and report failures.

`check-api-contract.py` proves the three sides agree on which *paths* exist.
It cannot tell you that a route returns something the firmware accepts — the
SMS listing agreed perfectly on paths while failing every call, because the
body it sent was not the shape the daemon wanted.

This closes that gap by actually calling them. Read-only by default: GETs, plus
the handful of POSTs that only read (SMS listing, process list). Anything that
changes state is listed and skipped unless you pass --include-writes, and even
then the genuinely destructive ones stay out.

    python3 scripts/check-live-api.py --password <agent-password>
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
SERVER_RS = ROOT / "agent/src/server.rs"
ROUTE_RE = re.compile(r'\(&Method::(\w+),\s*"(/api/[^"]+)"\)')

# POSTs that only read. Everything else that mutates is skipped by default.
READ_ONLY_POSTS = {
    "/api/sms/list": {},
    "/api/system/kill-bloat": None,  # never call: it stops daemons
}

# Bodies for routes that need one to mean anything.
BODIES = {
    "/api/sms/list": {"mem_store": 1, "page": 0, "per_page": 5},
}

# Routes whose failure is the correct answer in a clean state, with the reason.
# Treating these as failures trains you to ignore the output.
EXPECTED_FAILURES = {
    "/api/logger/signal/download": "404 until a signal log has been recorded",
    "/api/logger/connection/download": "404 until a connection log has been recorded",
}

# Never called, at any flag level: irreversible, disruptive, or both.
NEVER = {
    "/api/device/reboot",
    "/api/device/factory-reset",
    "/api/system/kill-bloat",
    "/api/euicc/download",
    "/api/euicc/enable",
    "/api/euicc/disable",
    "/api/euicc/delete",
    "/api/euicc/notifications/process",
    "/api/euicc/notifications/remove",
    "/api/euicc/relay/pending",   # long-polls; would just stall the sweep
    "/api/euicc/relay/response",
    "/api/auth/login",            # already used to get the token
    "/api/auth/logout",           # would invalidate the sweep's own token
}


def routes() -> list[tuple[str, str]]:
    return sorted(set(ROUTE_RE.findall(SERVER_RS.read_text())))


def call(base: str, token: str, method: str, path: str, body=None, timeout=30):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(f"{base}{path}", data=data, method=method)
    request.add_header("Authorization", f"Bearer {token}")
    if data is not None:
        request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            payload = json.load(response)
            if payload.get("ok"):
                return "ok", summarize(payload.get("data"))
            return "error", str(payload.get("error"))[:150]
    except urllib.error.HTTPError as e:
        detail = e.read().decode(errors="replace")[:150]
        return f"HTTP {e.code}", detail
    except Exception as e:  # noqa: BLE001 - one bad route must not stop the sweep
        return "unreachable", str(e)[:150]


def summarize(data) -> str:
    """Say enough to spot an empty answer without printing anyone's messages."""
    if isinstance(data, dict):
        if not data:
            return "empty object"
        return f"{len(data)} fields: {', '.join(list(data)[:6])}"
    if isinstance(data, list):
        return f"list of {len(data)}"
    return type(data).__name__


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agent", default="http://192.168.0.1:9090")
    parser.add_argument("--password", required=True)
    parser.add_argument(
        "--include-writes",
        action="store_true",
        help="also call mutating routes that are not in the NEVER list",
    )
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

    failures, skipped, checked = [], [], 0
    for method, path in routes():
        if path in NEVER:
            skipped.append((method, path, "never called"))
            continue
        if method != "Get" and path not in READ_ONLY_POSTS and not args.include_writes:
            skipped.append((method, path, "mutating"))
            continue

        status, detail = call(base, token, method.upper(), path, BODIES.get(path))
        checked += 1
        if status == "ok":
            mark, note = "ok  ", detail
        elif path in EXPECTED_FAILURES:
            mark, note = "note", f"expected: {EXPECTED_FAILURES[path]}"
        else:
            mark, note = "FAIL", detail
            failures.append((method, path, status, detail))
        print(f"  {mark} {method.upper():6} {path:44} {note}")

    print(f"\nchecked {checked}, skipped {len(skipped)}, failed {len(failures)}")
    if failures:
        print("\nFailing routes:")
        for method, path, status, detail in failures:
            print(f"  {method.upper()} {path} -> {status}: {detail}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
