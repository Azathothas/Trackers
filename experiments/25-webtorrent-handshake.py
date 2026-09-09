#!/usr/bin/env python3
"""
QUESTION
    Do the corpus's `wss` trackers answer a WebSocket handshake, and are any of
    them demonstrably trackers rather than merely WebSocket endpoints?

WHY IT EXISTS
    T-005 and `C-36`. Ten `wss` URLs are recorded `unmeasurable` **because no
    handshake had been attempted**, not because one was shown impossible.
    T-005 says it plainly: that label here is inertia rather than a
    constraint, which is the failure RULES 10.1a describes. A WebSocket
    handshake is ordinary TCP plus TLS plus an HTTP Upgrade, and this vantage
    has all three.

⛔ THE CEILING IS A SCRAPE, AND THE HANDSHAKE ALONE IS NOT THE ANSWER
    Completing a handshake proves there is a **WebSocket endpoint** there. It
    does not prove a tracker, and recording it as liveness would be RULES 3.3's
    "never claim liveness from a weaker signal" -- the `wss` restatement of
    treating HTTP 200 as a live tracker, which is the first row of RULES 11.

    So this goes one step further, and exactly one: the WebTorrent tracker
    protocol carries a **scrape** action over the socket, and RULES 4 permits a
    scrape with a synthetic info_hash. ⛔ **It never announces.** An announce
    would join a swarm; the code to build one does not exist here, and the
    ladder stops where BEP 48's does.

CONTROL
    tier 0a  a WebSocket tracker this process starts on loopback: it completes
             the handshake and answers a scrape. Proves the handshake, the
             **frame masking** RFC 6455 requires of a client, and the response
             classification. If it fails, no row below may be quoted.
    tier 0b  ⛔ **the negative control, and it is the one that matters.** A
             plain HTTP server on loopback that answers 200 and never upgrades.
             The prober must **not** call it a WebSocket. `experiments/05` has
             the same control for the same reason: without it, a probe that
             says yes to everything looks identical to a probe that works.
    tier 1   a TLS handshake to a host known to answer, so "no subject
             answered" is separable from "this vantage cannot reach 443".

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import http.server
import json
import os
import socket
import ssl
import struct
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, "src"))
sys.path.insert(0, os.path.join(REPO, "scripts"))

import _conditions as C  # noqa: E402
from generate import load_corpus  # noqa: E402
from trackers import bep15  # noqa: E402
from trackers.model import Rung, Transport  # noqa: E402

FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")

#: RFC 6455 section 1.3. The server proves it understood the handshake by
#: echoing sha1(key + this), base64-encoded -- which is the whole difference
#: between "a server returned 101" and "a WebSocket endpoint answered".
WS_GUID = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

#: Bounded, like every other network operation here (RULES 5.2).
MAX_FRAME = 64 * 1024

#: Where tier 1 proves this vantage can reach 443 at all. ⚠ It is a third
#: party and it is named rather than implied: a control that quietly contacts
#: somebody is still contacting somebody.
TLS_CONTROL_HOST = ("github.com", 443)


def _mask(payload: bytes) -> bytes:
    """One masked client frame. ⛔ RFC 6455 section 5.3 requires a client to
    mask, and a server that follows the RFC closes the connection on an
    unmasked frame -- which would read as the tracker refusing us."""
    key = os.urandom(4)
    masked = bytes(b ^ key[i % 4] for i, b in enumerate(payload))
    header = b"\x81"  # FIN + text frame
    length = len(payload)
    if length < 126:
        header += bytes([0x80 | length])
    elif length < (1 << 16):
        header += bytes([0x80 | 126]) + struct.pack("!H", length)
    else:
        header += bytes([0x80 | 127]) + struct.pack("!Q", length)
    return header + key + masked


def _read_exactly(sock: socket.socket, count: int) -> bytes:
    out = b""
    while len(out) < count:
        chunk = sock.recv(count - len(out))
        if not chunk:
            raise ConnectionError("closed while reading a frame")
        out += chunk
    return out


def _read_frame(sock: socket.socket) -> bytes:
    """One server frame. Servers do not mask (RFC 6455 section 5.1)."""
    header = _read_exactly(sock, 2)
    length = header[1] & 0x7F
    if length == 126:
        length = struct.unpack("!H", _read_exactly(sock, 2))[0]
    elif length == 127:
        length = struct.unpack("!Q", _read_exactly(sock, 8))[0]
    if header[1] & 0x80:  # a masked server frame is a protocol error
        _read_exactly(sock, 4)
    if length > MAX_FRAME:
        raise ValueError(f"frame of {length} bytes exceeds the {MAX_FRAME} cap")
    return _read_exactly(sock, length)


def handshake(host: str, port: int, path: str, timeout: float,
              tls: bool = True) -> dict:
    """Open a socket and attempt the Upgrade. Returns what each rung reached."""
    out: dict = {"rung": Rung.NONE.value, "host": host, "port": port}
    started = time.monotonic()
    sock = None
    try:
        sock = socket.create_connection((host, port), timeout=timeout)
        out["rung"] = Rung.CONNECTED.value
        if tls:
            context = ssl.create_default_context()
            sock = context.wrap_socket(sock, server_hostname=host)
            out["rung"] = Rung.TLS.value
            out["tls_version"] = sock.version()
        key = base64.b64encode(os.urandom(16))
        request = (
            f"GET {path or '/'} HTTP/1.1\r\n"
            f"Host: {host}\r\n"
            f"Upgrade: websocket\r\n"
            f"Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key.decode()}\r\n"
            f"Sec-WebSocket-Version: 13\r\n"
            f"\r\n").encode()
        sock.sendall(request)
        sock.settimeout(timeout)
        raw = b""
        while b"\r\n\r\n" not in raw and len(raw) < MAX_FRAME:
            chunk = sock.recv(4096)
            if not chunk:
                break
            raw += chunk
        if not raw:
            out["detail"] = "connected and the peer said nothing"
            return out
        out["rung"] = Rung.TRANSPORT_RESPONSE.value
        head, _, _ = raw.partition(b"\r\n\r\n")
        lines = head.decode("latin-1").split("\r\n")
        out["status_line"] = lines[0][:120]
        headers = {}
        for line in lines[1:]:
            name, _, value = line.partition(":")
            headers[name.strip().lower()] = value.strip()
        expected = base64.b64encode(hashlib.sha1(key + WS_GUID).digest()).decode()
        accept = headers.get("sec-websocket-accept", "")
        # ⛔ BOTH, and the accept token is the half that cannot be faked by a
        # server that merely likes the number 101.
        upgraded = ("101" in lines[0] and accept == expected)
        out["accept_matched"] = accept == expected
        if not upgraded:
            out["detail"] = (f"no upgrade: {lines[0][:80]!r}"
                             + ("" if accept else "; no accept header")
                             + ("" if accept == expected or not accept
                                else "; accept token did not match"))
            return out
        out["rung"] = Rung.PROTOCOL_VALID.value
        out["socket"] = sock
        sock = None  # handed to the caller; do not close it here
        return out
    except Exception as exc:  # noqa: BLE001 - a failed probe is a datum
        out["detail"] = f"{type(exc).__name__}: {exc}"
        return out
    finally:
        out["rtt_ms"] = round((time.monotonic() - started) * 1000, 1)
        if sock is not None:
            try:
                sock.close()
            except OSError:
                pass


def scrape_over(sock: socket.socket, timeout: float) -> dict:
    """One WebTorrent scrape, with a synthetic info_hash. Never an announce.

    ⚠ The info_hash travels as a **binary string**: WebTorrent's JavaScript
    clients put each byte in one code unit and `JSON.stringify` then UTF-8
    encodes it, so `latin-1` in and `ensure_ascii=False` out is what reproduces
    a real client's bytes rather than a plausible-looking approximation.
    """
    info_hash = bep15.synthetic_infohash()
    message = {"action": "scrape",
               "info_hash": [info_hash.decode("latin-1")]}
    payload = json.dumps(message, ensure_ascii=False).encode("utf-8")
    try:
        sock.settimeout(timeout)
        sock.sendall(_mask(payload))
        body = _read_frame(sock)
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "detail": f"{type(exc).__name__}: {exc}"}
    try:
        answer = json.loads(body.decode("utf-8", "replace"))
    except ValueError as exc:
        return {"ok": False, "detail": f"answer was not JSON: {exc}",
                "bytes": len(body)}
    if not isinstance(answer, dict):
        return {"ok": False, "detail": "answer was JSON but not an object"}
    # ⛔ `files` is the scrape response's own key. A JSON object that carries
    # none of the protocol's keys is not proven to be a tracker, which is the
    # same bar `bencode.classify_body` sets for the HTTP path.
    if "files" in answer:
        return {"ok": True, "detail": "answered a scrape with `files`",
                "keys": sorted(answer)[:8]}
    if "failure reason" in answer or "failure_reason" in answer:
        return {"ok": True, "detail": "answered a scrape with a failure "
                                      "reason, which is a working tracker",
                "keys": sorted(answer)[:8]}
    return {"ok": False, "detail": "JSON object with no tracker key",
            "keys": sorted(answer)[:8]}


class _WSTracker(http.server.BaseHTTPRequestHandler):
    """tier 0a: completes the handshake, then answers one scrape."""

    def do_GET(self):  # noqa: N802
        key = self.headers.get("Sec-WebSocket-Key", "").encode()
        accept = base64.b64encode(hashlib.sha1(key + WS_GUID).digest()).decode()
        self.send_response(101)
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.end_headers()
        try:
            _read_frame(self.connection)
            body = json.dumps({"action": "scrape", "files": {}}).encode()
            self.connection.sendall(b"\x81" + bytes([len(body)]) + body)
        except Exception:  # noqa: BLE001 - the control's own failure is the datum
            pass

    def log_message(self, *args):
        return


class _PlainServer(http.server.BaseHTTPRequestHandler):
    """tier 0b: answers 200 and never upgrades."""

    def do_GET(self):  # noqa: N802
        body = b"<html><body>not a websocket</body></html>"
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        return


def _loopback(handler, timeout: float, path: str = "/announce") -> dict:
    server = http.server.HTTPServer(("127.0.0.1", 0), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        port = server.server_address[1]
        result = handshake("127.0.0.1", port, path, timeout, tls=False)
        sock = result.pop("socket", None)
        if sock is not None:
            result["scrape"] = scrape_over(sock, timeout)
            try:
                sock.close()
            except OSError:
                pass
        return result
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2.0)


def tls_control(timeout: float) -> dict:
    """tier 1: can this vantage complete TLS on 443 at all?"""
    host, port = TLS_CONTROL_HOST
    try:
        with socket.create_connection((host, port), timeout=timeout) as raw:
            context = ssl.create_default_context()
            with context.wrap_socket(raw, server_hostname=host) as tls:
                return {"ok": True, "host": host, "tls_version": tls.version(),
                        "detail": "443 leaves this vantage"}
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "host": host,
                "detail": f"{type(exc).__name__}: {exc}"}


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--timeout", type=float, default=15.0)
    parser.add_argument("--fixtures", default=FIXTURES)
    parser.add_argument("--out", default=None)
    parser.add_argument("--control-only", action="store_true",
                        help="run the controls and contact no tracker")
    parser.add_argument("--no-scrape", action="store_true",
                        help="stop at the handshake. ⚠ Then nothing can reach "
                             "TRACKER_SEMANTIC and every row is capped at "
                             "PROTOCOL_VALID, which is a WebSocket endpoint "
                             "and not a tracker")
    parser.add_argument("--expect-controls", action="store_true",
                        help="exit 1 if the positive control fails, or if the "
                             "NEGATIVE control is called a WebSocket")
    args = parser.parse_args()

    positive = _loopback(_WSTracker, args.timeout)
    negative = _loopback(_PlainServer, args.timeout)
    controls = {
        "tier0a_websocket_tracker": {
            "ok": (positive.get("rung") == Rung.PROTOCOL_VALID.value
                   and (positive.get("scrape") or {}).get("ok") is True),
            "rung": positive.get("rung"),
            "scrape": positive.get("scrape"),
            "detail": "the handshake, the client masking and the scrape "
                      "classification all work"},
        "tier0b_plain_server_is_not_a_websocket": {
            # ⛔ The assertion is that it did NOT upgrade. A control that can
            # only pass proves nothing.
            "ok": negative.get("rung") != Rung.PROTOCOL_VALID.value,
            "rung": negative.get("rung"),
            "status_line": negative.get("status_line"),
            "detail": "a plain 200 must not be recorded as a WebSocket "
                      "(RULES 11, first row, in its wss form)"},
        "tier1_tls_egress": tls_control(args.timeout),
    }

    try:
        aggregate, _, _ = load_corpus(True, args.fixtures)
    except Exception as exc:  # noqa: BLE001
        print(f"could not build the corpus: {exc}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN
    subjects = sorted((t for t in aggregate.trackers
                       if t.transport in (Transport.WS, Transport.WSS)),
                      key=lambda t: t.url)
    if not subjects:
        print("no ws/wss tracker in the corpus; refusing to report on nothing",
              file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    rows: list[dict] = []
    if not args.control_only:
        for tracker in subjects:
            port = tracker.port or (443 if tracker.transport is Transport.WSS
                                    else 80)
            path = tracker.url.split(tracker.host, 1)[-1]
            path = path[path.find("/"):] if "/" in path else "/"
            result = handshake(tracker.host, port, path, args.timeout,
                               tls=tracker.transport is Transport.WSS)
            sock = result.pop("socket", None)
            row = {"url": tracker.url, **result}
            if sock is not None:
                if args.no_scrape:
                    row["scrape"] = {"ok": False,
                                     "detail": "--no-scrape: not attempted"}
                else:
                    row["scrape"] = scrape_over(sock, args.timeout)
                    if row["scrape"].get("ok"):
                        row["rung"] = Rung.TRACKER_SEMANTIC.value
                try:
                    sock.close()
                except OSError:
                    pass
            rows.append(row)

    by_rung: dict[str, int] = {}
    for row in rows:
        by_rung[row["rung"]] = by_rung.get(row["rung"], 0) + 1

    results = {
        "controls": controls,
        "subjects": len(subjects),
        "rungs": by_rung,
        "rows": rows,
        "what_this_is_not": (
            "A handshake that completes proves a WebSocket ENDPOINT, not a "
            "tracker (RULES 3.3). Only a row at TRACKER_SEMANTIC answered a "
            "scrape. Nothing here announces: the WebTorrent announce message "
            "is not built anywhere in this repository."),
    }
    conditions = C.with_network_vantage(C.collect(sample_counts={
        "subjects": len(subjects), "contacted": len(rows)}))
    C.emit("Do the corpus's wss trackers answer a WebSocket handshake?",
           conditions, results, args.out)

    for name, control in controls.items():
        print(f"CONTROL {name}: {'PASS' if control.get('ok') else 'FAIL'}  "
              f"{control.get('detail', '')}")
    print(f"\n{'RUNG':20s} {'SCRAPE':7s} URL")
    for row in rows:
        scraped = (row.get("scrape") or {}).get("ok")
        print(f"  {row['rung']:18s} {'yes' if scraped else '-':7s} {row['url']}")
    print(f"\nrungs: {by_rung}")
    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - That a PROTOCOL_VALID row is a tracker. It is a WebSocket.")
    print("  - That a failure is permanent. One vantage, one day.")

    if args.expect_controls:
        if not controls["tier0a_websocket_tracker"]["ok"]:
            print("\nEXPECTATION FAILED: the positive control did not complete,")
            print("  so no row above may be quoted.")
            return C.EXIT_MEASURED_AND_FAILED
        if not controls["tier0b_plain_server_is_not_a_websocket"]["ok"]:
            print("\nEXPECTATION FAILED: a plain HTTP server was recorded as a")
            print("  WebSocket. The prober says yes to everything.")
            return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
