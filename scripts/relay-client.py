#!/usr/bin/env python3
"""Carry the router's eSIM download traffic on its behalf.

A router being provisioned for the first time has no WAN: the cellular link it
would use is the one the profile provides. This client breaks that cycle by
performing the ES9+ HTTPS requests itself and handing the responses back to the
agent.

Run it on anything that can reach both the agent and the internet — a laptop on
another network, or a phone. It is also the reference implementation of the
relay protocol for the Android app, which does exactly this natively.

A browser cannot do this job: the requests are cross-origin and SM-DP+ servers
do not answer CORS preflights, so `fetch` is refused before it is sent.

Usage:
    python3 scripts/relay-client.py --agent http://192.168.0.1:9090 --password <pw>

Then start a download with "relay": true, e.g. from the dashboard, or:
    curl -X POST -H "Authorization: Bearer $TOKEN" -H "X-Confirm: true" \
         -H "Content-Type: application/json" \
         -d '{"activation_code":"LPA:1$...","relay":true}' \
         http://192.168.0.1:9090/api/euicc/download
"""
from __future__ import annotations

import argparse
import json
import os
import http.client
import ssl
import sys
import time
import urllib.error
import urllib.request

RETRY_DELAY = 3
POLL_WAIT = 25
REQUEST_TIMEOUT = 60

# ES9+ TLS is not anchored on the public web PKI. Some SM-DP+ servers present a
# WebPKI certificate; others present one issued by a GSMA Certificate Issuer,
# which no OS trust store carries. Both must be accepted, so the GSMA roots are
# loaded *in addition to* the system bundle rather than replacing it.
DEFAULT_CA_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "certs")


def build_ssl_context(extra_ca_dir: str) -> ssl.SSLContext:
    context = ssl.create_default_context()
    if os.path.isdir(extra_ca_dir):
        for name in sorted(os.listdir(extra_ca_dir)):
            if name.endswith(".pem"):
                context.load_verify_locations(cafile=os.path.join(extra_ca_dir, name))
    return context


def agent_call(agent: str, path: str, token: str, body=None, timeout=POLL_WAIT + 10):
    """Call the agent's JSON API."""
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(
        f"{agent}{path}",
        data=data,
        method="POST" if data is not None else "GET",
    )
    request.add_header("Authorization", f"Bearer {token}")
    if data is not None:
        request.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.load(response)


def login(agent: str, password: str) -> str:
    result = agent_call(agent, "/api/auth/login", "", {"password": password}, timeout=15)
    if not result.get("ok"):
        raise SystemExit(f"login failed: {result.get('error')}")
    return result["data"]["token"]


def perform(url: str, headers: list[str], body: bytes, context: ssl.SSLContext) -> tuple[int, bytes]:
    """Do the actual HTTPS request the router cannot."""
    request = urllib.request.Request(url, data=body or None, method="POST" if body else "GET")
    for header in headers:
        name, _, value = header.partition(":")
        if name and value:
            request.add_header(name.strip(), value.strip())
    try:
        with urllib.request.urlopen(request, timeout=REQUEST_TIMEOUT, context=context) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as e:
        # An HTTP error is a real answer and must be relayed as-is: the SM-DP+
        # signals protocol failures with 4xx/5xx bodies that lpac parses.
        return e.code, e.read()
    except Exception as e:  # noqa: BLE001 - network failures must not kill the loop
        print(f"  ! request failed: {e}", file=sys.stderr)
        return 0, b""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agent", default="http://192.168.0.1:9090")
    parser.add_argument("--password", required=True, help="agent API password")
    parser.add_argument("--once", action="store_true", help="exit after one request")
    parser.add_argument(
        "--ca-dir",
        default=DEFAULT_CA_DIR,
        help="directory of extra trust anchors (GSMA CI roots), added to the system bundle",
    )
    args = parser.parse_args()

    agent = args.agent.rstrip("/")
    token = login(agent, args.password)
    context = build_ssl_context(args.ca_dir)
    print(f"[relay] connected to {agent}; waiting for requests (Ctrl-C to stop)")

    while True:
        try:
            result = agent_call(agent, f"/api/euicc/relay/pending?wait={POLL_WAIT}", token)
        except urllib.error.HTTPError as e:
            # The agent restarting invalidates the token, and this client is
            # meant to sit running for hours across exactly that. Treating 401
            # as a network blip left it polling forever with a dead token,
            # silently failing every notification the agent tried to send.
            if e.code in (401, 403):
                print("[relay] session expired; signing in again", file=sys.stderr)
                try:
                    token = login(agent, args.password)
                except (urllib.error.URLError, TimeoutError) as retry_error:
                    print(f"[relay] re-login failed: {retry_error}", file=sys.stderr)
                    time.sleep(RETRY_DELAY)
                continue
            print(f"[relay] agent error: {e}; retrying", file=sys.stderr)
            time.sleep(RETRY_DELAY)
            continue
        except (urllib.error.URLError, http.client.HTTPException, OSError) as e:
            # Broad on purpose. This died on http.client.RemoteDisconnected,
            # which urllib does not wrap in URLError, so the agent closing a
            # long-poll connection killed the client outright — and every
            # notification the agent then tried to send waited out its timeout
            # against a relay that was no longer there. Backoff too: without one
            # an agent that is down turns this into a hot loop.
            print(f"[relay] lost the agent: {e}; retrying", file=sys.stderr)
            time.sleep(RETRY_DELAY)
            continue
        except TimeoutError:
            continue

        request = (result.get("data") or {}).get("request")
        if not request:
            continue

        body = bytes.fromhex(request["body_hex"]) if request["body_hex"] else b""
        print(f"[relay] -> {request['url']} ({len(body)} bytes)")
        status, response_body = perform(request["url"], request.get("headers", []), body, context)
        print(f"[relay] <- {status} ({len(response_body)} bytes)")

        try:
            agent_call(
                agent,
                "/api/euicc/relay/response",
                token,
                {
                    "id": request["id"],
                    "status": status,
                    "body_hex": response_body.hex().upper(),
                },
                timeout=30,
            )
        except urllib.error.HTTPError as e:
            print(f"[relay] agent rejected response: {e.read().decode()}", file=sys.stderr)

        if args.once:
            return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\n[relay] stopped")
