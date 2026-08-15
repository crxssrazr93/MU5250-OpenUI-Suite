#!/usr/bin/env python3
"""Report which endpoints the mobile apps call that this agent does not serve.

The dashboard contract check covers the dashboard only, so the Android app was
free to drift: it was vendored from the upstream project, whose agent has a
different API surface almost everywhere. That was found by installing an APK
and tapping a screen until something said "not found", which does not scale to
110 endpoints.

Exit status is 0 by design. This reports a known gap rather than gating a
build; use --strict to fail instead.

    python3 scripts/check-mobile-contract.py
"""
from __future__ import annotations

import argparse
import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SERVER_RS = ROOT / "agent/src/server.rs"
ROUTE_RE = re.compile(r'\(&Method::(\w+),\s*"(/api/[^"]+)"\)')
# Kotlin builds some paths by interpolation; strip at the first placeholder so
# "/api/euicc/eid$full" is compared as "/api/euicc/eid".
CALL_RE = re.compile(r'"(/api/[^"?$]+)')

APPS = {"android": ROOT / "mobile/android/OpenU60/app/src/main"}


def agent_paths() -> set[str]:
    return {path for _, path in ROUTE_RE.findall(SERVER_RS.read_text())}


def called_paths(root: Path) -> dict[str, set[str]]:
    found: dict[str, set[str]] = defaultdict(set)
    for source in root.rglob("*.kt"):
        for path in CALL_RE.findall(source.read_text()):
            found[path.rstrip("/")].add(source.name)
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--strict", action="store_true", help="exit non-zero when anything is missing")
    args = parser.parse_args()

    served = agent_paths()
    total_missing = 0

    for name, root in APPS.items():
        if not root.is_dir():
            print(f"{name}: not vendored, skipping")
            continue
        called = called_paths(root)
        missing = {path: files for path, files in called.items() if path not in served}
        total_missing += len(missing)

        print(f"\n{name}: calls {len(called)} endpoints, {len(missing)} of which this agent does not serve")
        if missing:
            for path, files in sorted(missing.items()):
                print(f"  {path:42} {', '.join(sorted(files))}")

    if total_missing:
        print(f"\n{total_missing} endpoints are missing. Each is either a path this fork renamed")
        print("or a feature it never implemented; the app shows 'not found' for both.")
    else:
        print("\nOK: every endpoint the mobile apps call is served.")

    return 1 if (args.strict and total_missing) else 0


if __name__ == "__main__":
    sys.exit(main())
