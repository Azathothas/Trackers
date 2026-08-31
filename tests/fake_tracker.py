"""The oracle: trackers we control, including broken ones.

T-021. Without this, there is no way to distinguish *"the internet is quiet"*
from *"the probe has been broken since Tuesday"*. A silently broken probe marks
the entire dataset dead, and every number in the report stays internally
consistent while doing it. The publication volume guard is supposed to catch
that -- but only if the guard was itself tested against this case, which needs a
tracker that can be told to break.

PROMOTED FROM TWO SEEDS, BOTH ALREADY PROVEN ON RUNNERS
    `LoopbackBEP15Tracker`      `experiments/02` -- a correct BEP 15 responder
    the two control servers     `experiments/05` -- a bencoded `failure reason`
                                responder, and a plain HTML web server

**One bug was fixed on the way in.** Both seeds selected their behaviour with a
*class* attribute on the handler (`_Handler.mode = "html"`). That is process-
global: two servers alive at once silently share one mode, and the second one
started wins. The tests here run several servers concurrently, so the behaviour
is now per-instance, passed through a handler factory. The seed's version was
correct only because it never ran two at a time.

THE NEGATIVE CONTROL IS THE LOAD-BEARING HALF

A probe that calls `Behaviour.HTML` a tracker has reproduced the anti-pattern in
RULES 11 -- health checking by HTTP status code. So the oracle exists mainly to
make that failure *fail the build* rather than be noticed later by a consumer.

No test in this file touches the network. Everything binds `127.0.0.1:0`.
"""

from __future__ import annotations

import http.server
import socket
import struct
import threading
import time
from enum import Enum

import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))

from trackers.bep15 import (ACTION_CONNECT, ACTION_ERROR,  # noqa: E402
                            ACTION_SCRAPE, PROTOCOL_ID)

__all__ = ["Behaviour", "FakeUdpTracker", "FakeHttpTracker",
           "CLIENT_LIKE_UA_MARKERS", "looks_like_a_torrent_client"]


class Behaviour(str, Enum):
    """What the fake tracker does. Every value is a real failure mode.

    The list is not imagination: each one is either something a real tracker
    does (`BENCODE_FAILURE`, `HTTP_403`, `HTTP_429`), something a non-tracker
    does that a naive probe mistakes for one (`HTML`, `EMPTY_200`), or a
    transport-level fault a probe must classify rather than crash on
    (`TIMEOUT`, `TRUNCATED`, `CLOSE_MIDWAY`, `MALFORMED_BENCODE`).
    """

    # --- correct behaviour -----------------------------------------------
    CORRECT = "correct"
    """BEP 15 connect/scrape answered correctly; HTTP scrape returns a
    well-formed bencoded scrape response."""

    BENCODE_FAILURE = "bencode_failure"
    """BEP 3 `failure reason`. A **working tracker**: it parsed our request and
    answered in-protocol. Stronger evidence of life than a 200 with peers."""

    BENCODE_FAILURE_UNDERSCORE = "bencode_failure_underscore"
    """BEP 48's `failure_reason` spelling. Present because a parser that
    accepts only one spelling misreads half the trackers it meets."""

    # --- not a tracker, but a perfectly healthy server --------------------
    HTML = "html"
    """HTTP 200 with HTML. A parked domain, an error page, a captive portal.
    The negative control."""

    EMPTY_200 = "empty_200"
    """HTTP 200 with a zero-length body. Status says success, content says
    nothing; a status-code probe calls this alive."""

    # --- refusals, which are facts about us and not about liveness --------
    HTTP_403 = "http_403"
    """Forbidden. The shape a User-Agent block takes (T-012)."""

    HTTP_429 = "http_429"
    """Rate limited. Says the tracker is *very much alive* and wants us to
    slow down -- recording it dead would be exactly backwards."""

    BLOCK_UNKNOWN_UA = "block_unknown_ua"
    """403 unless the User-Agent resembles a well-known torrent client, 200
    with a valid bencoded scrape otherwise.

    This is the oracle for T-012: it lets experiment 26 prove it can *detect*
    a UA block at all before any conclusion is drawn from real trackers. An
    instrument that cannot see the effect in a case where the effect is known
    to exist has not measured its absence anywhere else."""

    # --- transport faults --------------------------------------------------
    TIMEOUT = "timeout"
    """Accepts and never answers. UDP: silence. HTTP: holds the connection."""

    TRUNCATED = "truncated"
    """A bencoded body cut mid-value. Distinguishable from `MALFORMED` because
    the decoder reports a length running past the end of input."""

    MALFORMED_BENCODE = "malformed_bencode"
    """Syntactically invalid bencode that is not HTML either."""

    CLOSE_MIDWAY = "close_midway"
    """Announces a Content-Length, sends part of it, closes. UDP: a datagram
    shorter than BEP 15's minimum."""

    WRONG_TRANSACTION_ID = "wrong_transaction_id"
    """UDP only. Answers a correct-looking connect response with somebody
    else's transaction id -- an unsolicited or spoofed datagram. A probe that
    accepts this can be told any tracker is alive by any host on the internet."""

    BEP15_ERROR = "bep15_error"
    """UDP only. Action 3 with a message. Like `BENCODE_FAILURE`: a working
    tracker declining, not a dead one."""


# --- what "looks like a real client" means, for BLOCK_UNKNOWN_UA -------------
#
# Substrings taken from the User-Agent strings mainstream BitTorrent clients
# actually send. This is the oracle's *simulation* of a blocking policy, not a
# claim about what any real tracker does -- measuring that is T-012's job, and
# this list exists so that experiment has a positive control.
CLIENT_LIKE_UA_MARKERS: tuple[str, ...] = (
    "qBittorrent", "Transmission", "libtorrent", "Deluge",
    "uTorrent", "BitTorrent", "aria2", "rtorrent", "BiglyBT", "Azureus",
)


def looks_like_a_torrent_client(ua: str) -> bool:
    """Case-insensitive substring match against `CLIENT_LIKE_UA_MARKERS`."""
    low = (ua or "").lower()
    return any(m.lower() in low for m in CLIENT_LIKE_UA_MARKERS)


# --- bodies ------------------------------------------------------------------
# Hand-built so the encodings are checkable by eye against BEP 3.
_BODY_SCRAPE_OK = (
    b"d5:filesd20:"
    + b"\x00" * 20
    + b"d8:completei3e10:downloadedi7e10:incompletei1eeee"
)
_BODY_FAILURE_SPACE = b"d14:failure reason30:torrent not registered with mee"
_BODY_FAILURE_UNDERSCORE = b"d14:failure_reason24:info hash not in databaseee"
_BODY_HTML = b"<!doctype html><html><body><h1>It works!</h1></body></html>"
# A 40-byte string header promising bytes that are not there.
_BODY_TRUNCATED = b"d14:failure reason40:torrent not"
_BODY_MALFORMED = b"d14:failure reason!!!not-bencode-at-all"


class FakeUdpTracker:
    """A BEP 15 responder on loopback, tellable to misbehave.

    Context manager. `port` is the bound ephemeral port; `seen` counts
    datagrams that carried the correct protocol magic, so a test can tell
    "the probe sent nothing" from "the probe sent something we ignored".
    """

    def __init__(self, behaviour: Behaviour = Behaviour.CORRECT,
                 connection_id: int = 0x0123456789ABCDEF):
        self.behaviour = behaviour
        self.connection_id = connection_id
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.sock.bind(("127.0.0.1", 0))
        self.port: int = self.sock.getsockname()[1]
        self.seen = 0
        self.requests: list[bytes] = []
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._serve, daemon=True)

    # -- server -------------------------------------------------------------
    def _serve(self) -> None:
        self.sock.settimeout(0.1)
        while not self._stop.is_set():
            try:
                data, addr = self.sock.recvfrom(2048)
            except socket.timeout:
                continue
            except OSError:
                break
            if len(data) < 16:
                continue
            magic, action, txid = struct.unpack(">QII", data[:16])
            # A connect carries the protocol magic; a scrape carries the
            # connection id we handed out. Both live at offset 0.
            is_connect = magic == PROTOCOL_ID
            if not (is_connect or magic == self.connection_id):
                # Wrong magic: a real tracker ignores this, and so do we.
                # Silence here is what makes the control meaningful.
                continue
            self.seen += 1
            self.requests.append(data)
            reply = self._reply_for(action, txid, data)
            if reply is not None:
                self.sock.sendto(reply, addr)

    def _reply_for(self, action: int, txid: int, data: bytes) -> bytes | None:
        b = self.behaviour
        if b is Behaviour.TIMEOUT:
            return None
        if b is Behaviour.WRONG_TRANSACTION_ID:
            # Deliberately not txid. Everything else about it is correct, which
            # is the point: only the transaction-id check catches it.
            return struct.pack(">IIQ", ACTION_CONNECT, (txid ^ 0xFFFFFFFF) & 0xFFFFFFFF,
                               self.connection_id)
        if b is Behaviour.BEP15_ERROR:
            return struct.pack(">II", ACTION_ERROR, txid) + b"go away, politely"
        if b in (Behaviour.CLOSE_MIDWAY, Behaviour.TRUNCATED):
            # Below BEP 15's 16-byte minimum for a connect response, and not a
            # valid error either: 12 bytes is a truncated datagram.
            return struct.pack(">IIQ", ACTION_CONNECT, txid, self.connection_id)[:12]
        if action == ACTION_SCRAPE:
            # 8-byte header plus one 12-byte row.
            return struct.pack(">IIIII", ACTION_SCRAPE, txid, 3, 7, 1)
        return struct.pack(">IIQ", ACTION_CONNECT, txid, self.connection_id)

    # -- lifecycle ----------------------------------------------------------
    def __enter__(self) -> "FakeUdpTracker":
        self._thread.start()
        return self

    def __exit__(self, *exc) -> bool:
        self._stop.set()
        self._thread.join(timeout=2)
        self.sock.close()
        return False


def _make_handler(server: "FakeHttpTracker"):
    """Build a handler class bound to one server instance.

    A closure rather than a class attribute. See the module docstring: the
    seeds used `_Handler.mode`, which two concurrent servers silently share.
    """

    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args) -> None:
            pass  # keep the test output readable

        def do_GET(self) -> None:  # noqa: N802 - http.server's spelling
            server.requests.append({
                "path": self.path,
                "user_agent": self.headers.get("User-Agent", ""),
                "headers": {k.lower(): v for k, v in self.headers.items()},
            })
            server._respond(self)

    return Handler


class FakeHttpTracker:
    """An HTTP tracker on loopback, tellable to misbehave.

    Context manager. `url_for("/announce")` builds an absolute URL. `requests`
    records every request seen, including the User-Agent, so a test can assert
    what the probe actually sent rather than what it meant to send -- which is
    what T-012's four arms need.
    """

    def __init__(self, behaviour: Behaviour = Behaviour.CORRECT):
        self.behaviour = behaviour
        self.requests: list[dict] = []
        self.httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0),
                                                     _make_handler(self))
        self.httpd.daemon_threads = True
        self.port: int = self.httpd.server_address[1]
        self._thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    def url_for(self, path: str = "/announce") -> str:
        return f"http://127.0.0.1:{self.port}{path}"

    # -- responses ----------------------------------------------------------
    def _respond(self, h: http.server.BaseHTTPRequestHandler) -> None:
        b = self.behaviour

        if b is Behaviour.TIMEOUT:
            # Hold the connection open, answering nothing. Bounded so a hung
            # test cannot outlive the suite.
            time.sleep(30)
            return

        if b is Behaviour.BLOCK_UNKNOWN_UA:
            ua = h.headers.get("User-Agent", "")
            if looks_like_a_torrent_client(ua):
                self._send(h, 200, "text/plain", _BODY_SCRAPE_OK)
            else:
                self._send(h, 403, "text/html",
                           b"<html><body>Forbidden: unrecognised client</body></html>")
            return

        if b is Behaviour.CLOSE_MIDWAY:
            body = _BODY_SCRAPE_OK
            h.send_response(200)
            h.send_header("Content-Type", "text/plain")
            # Promise twice what we send, then hang up.
            h.send_header("Content-Length", str(len(body) * 2))
            h.end_headers()
            h.wfile.write(body[: len(body) // 2])
            h.wfile.flush()
            h.close_connection = True
            try:
                h.connection.close()
            except OSError:
                pass
            return

        table = {
            Behaviour.CORRECT: (200, "text/plain", _BODY_SCRAPE_OK),
            Behaviour.BENCODE_FAILURE: (200, "text/plain", _BODY_FAILURE_SPACE),
            Behaviour.BENCODE_FAILURE_UNDERSCORE: (200, "text/plain",
                                                   _BODY_FAILURE_UNDERSCORE),
            Behaviour.HTML: (200, "text/html", _BODY_HTML),
            Behaviour.EMPTY_200: (200, "text/plain", b""),
            Behaviour.HTTP_403: (403, "text/html",
                                 b"<html><body>Forbidden</body></html>"),
            Behaviour.HTTP_429: (429, "text/plain", b"slow down"),
            Behaviour.TRUNCATED: (200, "text/plain", _BODY_TRUNCATED),
            Behaviour.MALFORMED_BENCODE: (200, "text/plain", _BODY_MALFORMED),
        }
        status, ctype, body = table.get(b, (200, "text/plain", _BODY_SCRAPE_OK))
        self._send(h, status, ctype, body)

    @staticmethod
    def _send(h: http.server.BaseHTTPRequestHandler, status: int,
              ctype: str, body: bytes) -> None:
        h.send_response(status)
        h.send_header("Content-Type", ctype)
        h.send_header("Content-Length", str(len(body)))
        h.end_headers()
        if body:
            h.wfile.write(body)

    # -- lifecycle ----------------------------------------------------------
    def __enter__(self) -> "FakeHttpTracker":
        self._thread.start()
        return self

    def __exit__(self, *exc) -> bool:
        self.httpd.shutdown()
        self.httpd.server_close()
        self._thread.join(timeout=2)
        return False
