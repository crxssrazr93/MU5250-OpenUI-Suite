#!/usr/bin/env python3
"""Report mobile calls that reach a real route with the wrong method or no confirm.

`check-mobile-contract.py` asks only whether a *path* is served, and that check
stayed green while three screens were broken:

  - Telemetry Blocker POSTed to a destructive route without `X-Confirm: true`,
    so every rule add came back "destructive action requires X-Confirm".
  - Band Lock did the same, and its unlock used DELETE on a POST-only route.
  - Several screens PUT to paths the agent only serves for GET.

All three are path-level matches, so nothing reported them until someone tapped
the screen. This checks the two things the path check cannot see:

  1. the HTTP verb the app uses against the verbs the agent registers, and
  2. whether a call to a DESTRUCTIVE_PATHS route uses a confirming helper.

Exit status is 0 unless --strict.

    python3 scripts/check-mobile-methods.py
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SERVER_RS = ROOT / "agent/src/server.rs"
ANDROID = ROOT / "mobile/android/OpenU60/app/src/main"

ROUTE_RE = re.compile(r'\(&Method::(\w+),\s*"(/api/[^"]+)"\)')

# The AgentClient helpers, and what each one actually sends. `confirm` marks the
# helpers that set the X-Confirm header.
HELPERS = {
    "getJSON": ("GET", False),
    "getJSONArray": ("GET", False),
    "getSlowJSON": ("GET", False),
    "postJSON": ("POST", False),
    "postSlowJSON": ("POST", False),
    "postConfirmedJSON": ("POST", True),
    "putJSON": ("PUT", False),
    "putConfirmedJSON": ("PUT", True),
    "deleteJSON": ("DELETE", False),
    "deleteConfirmedJSON": ("DELETE", True),
}

# client.getJSON("/api/foo")  /  agentClient.postConfirmedJSON("/api/foo", ...)
# Interpolated paths are cut at the first placeholder, same as the path check.
CALL_RE = re.compile(
    r"\.(" + "|".join(HELPERS) + r")\(\s*\"(/api/[^\"?$]+)"
)


def agent_routes() -> dict[str, set[str]]:
    """path -> set of methods the agent registers for it."""
    routes: dict[str, set[str]] = {}
    for method, path in ROUTE_RE.findall(SERVER_RS.read_text()):
        routes.setdefault(path, set()).add(method.upper())
    return routes


def destructive_paths() -> set[str]:
    source = SERVER_RS.read_text()
    start = source.index("const DESTRUCTIVE_PATHS")
    end = source.index("];", start)
    return set(re.findall(r'"(/api/[^"]+)"', source[start:end]))


def calls() -> list[tuple[str, int, str, str, bool]]:
    """(file, line, method, path, confirms) for every AgentClient call."""
    found = []
    for source in sorted(ANDROID.rglob("*.kt")):
        for number, line in enumerate(source.read_text().splitlines(), 1):
            for helper, path in CALL_RE.findall(line):
                method, confirms = HELPERS[helper]
                found.append((source.name, number, method, path, confirms))
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--strict", action="store_true", help="exit non-zero on findings")
    args = parser.parse_args()

    routes = agent_routes()
    destructive = destructive_paths()

    wrong_method: list[str] = []
    missing_confirm: list[str] = []

    for name, line, method, path, confirms in calls():
        served = routes.get(path)
        if served is None:
            continue  # a missing path is check-mobile-contract.py's job
        if method not in served:
            wrong_method.append(
                f"  {name}:{line}  {method} {path}  — agent serves {'/'.join(sorted(served))}"
            )
        # A GET never mutates, so the agent does not gate it.
        if path in destructive and method != "GET" and not confirms:
            missing_confirm.append(f"  {name}:{line}  {method} {path}")

    if wrong_method:
        print(f"Wrong HTTP method ({len(wrong_method)}):")
        print("\n".join(wrong_method))
        print()
    if missing_confirm:
        print(f"Destructive route called without X-Confirm ({len(missing_confirm)}):")
        print("\n".join(missing_confirm))
        print()

    if not wrong_method and not missing_confirm:
        print("Every served path is called with a method the agent registers,")
        print("and every destructive route is called with a confirming helper.")
        return 0

    print("These reach a real route and fail at the agent, so the path check")
    print("cannot see them. Fix the caller, not the route list.")
    return 1 if args.strict else 0


if __name__ == "__main__":
    sys.exit(main())
