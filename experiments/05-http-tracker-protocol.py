#!/usr/bin/env python3
"""
QUESTION
    Is a bencoded response the reliable discriminator between an HTTP tracker
    and an ordinary web server -- and does a `failure reason` key really prove
    a working tracker, as the ladder in TODO/measurement.md claims?

WHY IT EXISTS
    This is the claim that decides whether HTTP health checking means anything.
    TODO/RULES.md C-32: if a bencoded `failure reason` is NOT a well-formed
    tracker response, "the key discriminator between tracker and web server is
    gone, and health checking falls back to HTTP status codes -- which is the
    naive approach the design brief Appendix A exists to prevent."

    So the experiment carries a NEGATIVE CONTROL as well as a positive one: a
    local web server that returns HTTP 200 with HTML. A probe that calls that
    endpoint a tracker has reproduced the anti-pattern, and this script fails
    with a non-zero exit when it does. That turns the research artefact into a
    regression check (TEMPLATE experiments.md).

SPEC, READ NOT REMEMBERED
    BEP 3  (https://www.bittorrent.org/beps/bep_0003.html, fetched 2026-08-29)
      "Tracker responses are bencoded dictionaries. If a tracker response has a
       key `failure reason`, then that maps to a human readable string which
       explains why the query failed, and no other keys are required."
      BEP 3's tracker GET keys are exactly: info_hash, peer_id, ip, port,
      uploaded, downloaded, left, event.
      NOTE: `numwant` is NOT among them. It is a de-facto extension, not BEP 3.

    BEP 48 (https://www.bittorrent.org/beps/bep_0048.html, fetched 2026-08-29)
      Scrape URL = the announce URL with the string `announce` in its PATH
      replaced by `scrape`.
      "scrape exchanges have no effect on a peer's participation in a swarm."
      An unsuccessful scrape returns bencoded key `failure_reason`.

    The two BEPs spell the key differently -- `failure reason` with a space in
    BEP 3, `failure_reason` with an underscore in BEP 48. A parser that accepts
    only one of them will misread half the trackers it meets. This script
    accepts both and reports which spelling each tracker used, because that
    distribution is itself a finding.

ETHICS  (RULES 4)
    - Scrape is tried FIRST. BEP 48 states, as primary source, that a scrape
      has no effect on swarm participation.
    - Where an announce is used, the info_hash is 20 random bytes generated per
      run. It corresponds to no content. `event=stopped` and `numwant=0` are
      sent so that a tracker which did somehow know the hash is told we are
      leaving and want no peers.
    - The User-Agent names the project and its URL so an operator who objects
      can find us and say so.
    - One request per endpoint per rung. No retries against real trackers.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run
"""

from __future__ import annotations

import argparse
import http.server
import os
import socket
import ssl
import sys
import threading
import urllib.error
import urllib.parse
import urllib.request
from urllib.parse import urlsplit, urlunsplit

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _consent as consent  # noqa: E402
import _conditions as C  # noqa: E402

PROJECT_URL = "https://github.com/Azathothas/Trackers"
USER_AGENT = f"trackers/0.1 (+{PROJECT_URL}; tracker health probe; contact via repository issues)"
TIMEOUT = 10.0
MAX_BYTES = 256 * 1024  # a tracker answer is small; anything larger is not one


# --- bencode ------------------------------------------------------------------
class BencodeError(ValueError):
    pass


def bdecode(data: bytes) -> tuple[object, int]:
    """Strict-enough bencode decoder. Returns (value, bytes_consumed).

    Strictness matters here: the whole point is to tell a tracker from a web
    server, and a decoder that accepts sloppy input will happily 'parse' HTML.
    """
    def _dec(i: int) -> tuple[object, int]:
        if i >= len(data):
            raise BencodeError("truncated")
        c = data[i:i + 1]
        if c == b"i":
            j = data.index(b"e", i)
            raw = data[i + 1:j]
            if raw in (b"-0",) or (len(raw) > 1 and raw.startswith(b"0")) or \
               (len(raw) > 2 and raw.startswith(b"-0")):
                raise BencodeError(f"non-canonical integer {raw!r}")
            return int(raw), j + 1
        if c == b"l":
            i += 1
            out = []
            while data[i:i + 1] != b"e":
                v, i = _dec(i)
                out.append(v)
            return out, i + 1
        if c == b"d":
            i += 1
            out = {}
            while data[i:i + 1] != b"e":
                k, i = _dec(i)
                if not isinstance(k, bytes):
                    raise BencodeError("dictionary key is not a byte string")
                v, i = _dec(i)
                out[k] = v
            return out, i + 1
        if c.isdigit():
            j = data.index(b":", i)
            n = int(data[i:j])
            if n < 0 or j + 1 + n > len(data):
                raise BencodeError("string length out of range")
            return data[j + 1:j + 1 + n], j + 1 + n
        raise BencodeError(f"unexpected byte {c!r} at offset {i}")

    try:
        return _dec(0)
    except BencodeError:
        raise
    except (ValueError, IndexError) as e:
        raise BencodeError(str(e)) from e


FAILURE_KEYS = (b"failure reason", b"failure_reason")


def classify_body(body: bytes) -> dict:
    """Decide what this response actually is. The heart of the discriminator."""
    if not body:
        return {"kind": "empty", "detail": "zero-length body"}
    try:
        value, consumed = bdecode(body)
    except BencodeError as e:
        head = body[:60]
        looks_html = head.lstrip()[:1] in (b"<",) or b"<html" in body[:512].lower()
        return {"kind": "html" if looks_html else "not_bencode",
                "detail": f"bdecode failed: {e}", "head": head.decode("utf-8", "replace")}
    if not isinstance(value, dict):
        return {"kind": "bencode_not_dict", "detail": f"top level is {type(value).__name__}"}
    trailing = len(body) - consumed
    for k in FAILURE_KEYS:
        if k in value:
            msg = value[k]
            return {"kind": "tracker_failure_response",
                    "failure_key_spelling": k.decode(),
                    "detail": (msg if isinstance(msg, bytes) else b"").decode("utf-8", "replace")[:200],
                    "trailing_bytes": trailing}
    if b"peers" in value or b"interval" in value:
        return {"kind": "tracker_announce_response",
                "detail": f"keys={sorted(x.decode('utf-8','replace') for x in value)[:8]}",
                "interval": value.get(b"interval"), "trailing_bytes": trailing}
    if b"files" in value:
        files = value[b"files"]
        return {"kind": "tracker_scrape_response",
                "detail": f"files entries={len(files) if isinstance(files, dict) else '?'}",
                "trailing_bytes": trailing}
    return {"kind": "bencode_dict_unrecognised",
            "detail": f"keys={sorted(x.decode('utf-8','replace') for x in value)[:8]}",
            "trailing_bytes": trailing}


# --- local controls -----------------------------------------------------------
class _Handler(http.server.BaseHTTPRequestHandler):
    mode = "tracker"

    def log_message(self, *a):
        pass

    def do_GET(self):
        if _Handler.mode == "html":
            # The negative control: a perfectly healthy web server that is not
            # a tracker. HTTP 200, text/html. Exactly what a parked domain,
            # an error page or a captive portal returns.
            body = b"<!doctype html><html><body><h1>It works!</h1></body></html>"
            self.send_response(200)
            self.send_header("Content-Type", "text/html")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        # The positive control: BEP 3's failure response for an unknown hash.
        body = b"d14:failure reason30:torrent not registered with mee"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class LocalServer:
    def __init__(self, mode: str):
        self.mode = mode
        self.httpd = http.server.HTTPServer(("127.0.0.1", 0), _Handler)
        self.port = self.httpd.server_address[1]
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    def __enter__(self):
        _Handler.mode = self.mode
        self.thread.start()
        return self

    def __exit__(self, *a):
        self.httpd.shutdown()
        self.httpd.server_close()
        self.thread.join(timeout=2)
        return False


# --- the probe ----------------------------------------------------------------
def scrape_url(announce_url: str) -> str | None:
    """BEP 48: replace the string 'announce' in the PATH with 'scrape'."""
    p = urlsplit(announce_url)
    if "announce" not in p.path:
        return None
    return urlunsplit((p.scheme, p.netloc, p.path.replace("announce", "scrape", 1), p.query, p.fragment))


def http_probe(url: str, params: dict[str, object], timeout: float = TIMEOUT) -> dict:
    """One HTTP(S) GET. Records the rung reached, per the ladder in TODO/measurement.md."""
    qs = urllib.parse.urlencode(params, doseq=True, quote_via=urllib.parse.quote)
    full = url + ("&" if urlsplit(url).query else "?") + qs if params else url
    req = urllib.request.Request(full, headers={"User-Agent": USER_AGENT, "Accept": "*/*"})
    rung = "dns"
    try:
        ctx = ssl.create_default_context()
        with C.Timer() as t:
            with urllib.request.urlopen(req, timeout=timeout, context=ctx) as resp:
                rung = "transport_response"
                body = resp.read(MAX_BYTES + 1)
                status = resp.status
                ctype = resp.headers.get("Content-Type", C.UNKNOWN)
        truncated = len(body) > MAX_BYTES
        cls = classify_body(body[:MAX_BYTES])
        if cls["kind"].startswith("tracker_"):
            rung = "tracker_semantic"
        elif cls["kind"] in ("bencode_dict_unrecognised", "bencode_not_dict"):
            rung = "protocol_valid"
        return {"ok": cls["kind"].startswith("tracker_"), "rung": rung, "status": status,
                "content_type": ctype, "bytes": len(body), "truncated": truncated,
                "classification": cls, "rtt_ms": round(t.ms, 3), "url": full}
    except urllib.error.HTTPError as e:
        # A tracker may answer 4xx/5xx and still be a tracker; read the body.
        try:
            body = e.read(MAX_BYTES)
        except Exception:
            body = b""
        cls = classify_body(body)
        return {"ok": cls["kind"].startswith("tracker_"),
                "rung": "tracker_semantic" if cls["kind"].startswith("tracker_") else "transport_response",
                "status": e.code, "content_type": e.headers.get("Content-Type", C.UNKNOWN) if e.headers else C.UNKNOWN,
                "bytes": len(body), "truncated": False, "classification": cls,
                "rtt_ms": None, "url": full}
    except urllib.error.URLError as e:
        reason = e.reason
        name = type(reason).__name__ if not isinstance(reason, str) else "URLError"
        if isinstance(reason, socket.gaierror):
            rung = "dns_failure"
        elif isinstance(reason, ssl.SSLError):
            rung = "tls_failure"
        else:
            rung = "transport_failure"
        return {"ok": False, "rung": rung, "status": None, "content_type": C.UNKNOWN,
                "bytes": 0, "truncated": False,
                "classification": {"kind": "no_response", "detail": f"{name}: {reason}"},
                "rtt_ms": None, "url": full}
    except Exception as e:
        return {"ok": False, "rung": rung, "status": None, "content_type": C.UNKNOWN,
                "bytes": 0, "truncated": False,
                "classification": {"kind": "no_response", "detail": f"{type(e).__name__}: {e}"},
                "rtt_ms": None, "url": full}


def probe_tracker(announce: str, synthetic_hash: bytes, do_announce: bool) -> dict:
    """Ladder: scrape (no swarm effect at all) first, announce only if asked."""
    out: dict = {"announce_url": announce, "rungs": {}}
    su = scrape_url(announce)
    if su:
        out["rungs"]["scrape"] = http_probe(su, {"info_hash": synthetic_hash})
    if do_announce:
        out["rungs"]["announce"] = http_probe(announce, {
            "info_hash": synthetic_hash,
            "peer_id": b"-TR0001-trackersrsch",   # 20 bytes, BEP 3
            "port": 6881,
            "uploaded": 0,
            "downloaded": 0,
            "left": 0,
            "event": "stopped",   # BEP 3: we are leaving
            "numwant": 0,         # de-facto extension, not BEP 3; send no-peers-wanted
            "compact": 1,
        })
    best = None
    for name in ("scrape", "announce"):
        r = out["rungs"].get(name)
        if r and r["ok"]:
            best = name
            break
    out["verdict"] = {
        "is_tracker": best is not None,
        "proved_by": best or C.UNKNOWN,
        "highest_rung": max((r["rung"] for r in out["rungs"].values()),
                            key=lambda x: ["dns_failure", "transport_failure", "tls_failure",
                                           "dns", "transport_response", "protocol_valid",
                                           "tracker_semantic"].index(x)
                            if x in ["dns_failure", "transport_failure", "tls_failure", "dns",
                                     "transport_response", "protocol_valid", "tracker_semantic"] else 0,
                            default=C.UNKNOWN),
    }
    return out


def main() -> int:
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--targets", default=os.path.join(here, "fixtures", "probe-targets.tsv"))
    ap.add_argument("--announce", action="store_true",
                    help="also send an announce with a synthetic random info_hash "
                         "(event=stopped, numwant=0). Off by default: scrape alone "
                         "answers the question for most trackers and BEP 48 states "
                         "it has no swarm effect.")
    ap.add_argument("--expect-controls", action="store_true",
                    help="exit 1 unless the bencode control is recognised as a tracker "
                         "AND the HTML control is NOT")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    synthetic = os.urandom(20)  # corresponds to no content, by construction

    # --- controls, both run twice ---------------------------------------------
    controls = {"bencode_failure_response": [], "html_web_server": []}
    with LocalServer("tracker") as srv:
        for _ in range(2):
            controls["bencode_failure_response"].append(
                http_probe(f"http://127.0.0.1:{srv.port}/announce", {"info_hash": synthetic}))
    with LocalServer("html") as srv:
        for _ in range(2):
            controls["html_web_server"].append(
                http_probe(f"http://127.0.0.1:{srv.port}/announce", {"info_hash": synthetic}))

    positive_ok = all(r["ok"] for r in controls["bencode_failure_response"])
    negative_ok = all(not r["ok"] for r in controls["html_web_server"])

    # --- subjects --------------------------------------------------------------
    targets = []
    try:
        with open(args.targets, encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                parts = line.split("\t")
                if len(parts) >= 2 and parts[0] in ("http", "https"):
                    targets.append(parts[1])
    except OSError as e:
        print(f"could not read targets: {e}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    # ⛔ BEP 34 FIRST, for every subject. This instrument reaches the same
    # action `src/trackers/probe.py` does -- contacting somebody's tracker --
    # so it consults the same record. Before 2026-09-05 it did not, and
    # `p0-ground-truth.yml` ran it against these endpoints on every push
    # touching `experiments/`.
    subjects = []
    refused = []
    for u in targets:
        permitted, why = consent.permits(u)
        if not permitted:
            # A subject we may not measure is not a subject that failed.
            refused.append({"url": u, "bep34": why})
            continue
        subjects.append(probe_tracker(u, synthetic, args.announce))

    spellings: dict[str, int] = {}
    kinds: dict[str, int] = {}
    for s in subjects:
        for r in s["rungs"].values():
            k = r["classification"]["kind"]
            kinds[k] = kinds.get(k, 0) + 1
            sp = r["classification"].get("failure_key_spelling")
            if sp:
                spellings[sp] = spellings.get(sp, 0) + 1

    results = {
        "controls": controls,
        "control_verdict": {"positive_recognised": positive_ok,
                            "negative_correctly_rejected": negative_ok},
        "subjects": subjects,
        "refused_by_bep34": refused,
        "summary": {
            "http_targets": len(targets),
            "probed": len(subjects),
            "refused_by_bep34": len(refused),
            "proved_tracker": sum(1 for s in subjects if s["verdict"]["is_tracker"]),
            "announce_sent": args.announce,
            "response_kind_histogram": kinds,
            "failure_key_spelling_histogram": spellings,
            "scrape_supported": sum(1 for s in subjects
                                    if s["rungs"].get("scrape", {}).get("ok")),
        },
    }

    conditions = C.with_network_vantage(C.collect(sample_counts={
        "http_targets": len(targets), "control_runs_each": 2,
        "requests_per_target": 1 + (1 if args.announce else 0),
    }, extra={"specs": "BEP 3 and BEP 48, fetched 2026-08-29",
              "user_agent": USER_AGENT,
              "info_hash": "20 random bytes generated per run; corresponds to no content"}))
    out = args.out or C.results_path(__file__)
    C.emit("Is a bencoded response the reliable discriminator between an HTTP tracker "
           "and an ordinary web server?", conditions, results, out)

    print("\nCONTROLS (each run twice)")
    print(f"  positive  bencoded 'failure reason' -> recognised as tracker: "
          f"{'PASS' if positive_ok else 'FAIL'}")
    print(f"  negative  HTTP 200 + HTML          -> correctly NOT a tracker: "
          f"{'PASS' if negative_ok else 'FAIL'}")
    if not negative_ok:
        print("  !! The probe called a plain web server a tracker. That is the exact")
        print("  !! anti-pattern in the design brief Appendix A. Nothing below may be quoted.")

    print(f"\nSUBJECTS  {len(targets)} HTTP/HTTPS trackers"
          f"{' (scrape + announce)' if args.announce else ' (scrape only)'}")
    for s in subjects:
        v = s["verdict"]
        print(f"  {'TRACKER' if v['is_tracker'] else 'NOT-PROVED'}  {s['announce_url']}")
        for name, r in s["rungs"].items():
            sp = r["classification"].get("failure_key_spelling")
            print(f"      {name:8s} status={r['status']} rung={r['rung']:<20s} "
                  f"kind={r['classification']['kind']}"
                  f"{f' key={sp!r}' if sp else ''}")
            if r["classification"].get("detail"):
                print(f"               {r['classification']['detail'][:110]}")

    s = results["summary"]
    print("\nSUMMARY")
    print(f"  proved to be trackers:      {s['proved_tracker']}/{s['http_targets']}")
    print(f"  scrape endpoint answered:   {s['scrape_supported']}/{s['http_targets']}")
    print(f"  response kinds:             {s['response_kind_histogram']}")
    print(f"  failure-key spellings seen: {s['failure_key_spelling_histogram'] or '{}'}")

    if args.expect_controls and not (positive_ok and negative_ok):
        print("\nEXPECTATION FAILED: --expect-controls")
        return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
