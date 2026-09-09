#!/usr/bin/env python3
"""
QUESTION
    Are the corpus's `.i2p` trackers alive, measured from a vantage that
    actually has i2p?

WHY IT EXISTS
    T-039. Every record this project has ever taken calls the i2p category
    `unmeasurable`, and that is honest about our data rather than about the
    trackers (RULES 3.1). `C-37` is the structural reason: i2p needs its own
    router and no vantage here ran one. T-031 built the indirect-liveness
    mechanism and moved the IPv6-only category with it; this is the category it
    did not move.

    ⭐ **The route that opened it is a router in a container**, which
    `docs/containers.md` is the page for: an i2pd router runs in a throwaway
    container and this process speaks to its HTTP proxy. The measurement is
    then **first-hand** -- our own request, our own answer -- rather than
    second-hand through somebody else's observer, so it does not go through
    `src/trackers/secondhand.py`.

⛔ WHAT THIS DOES NOT DO
    **It never announces.** RULES 4, and here it is a constraint with teeth
    rather than a promise: only 3 of the 13 `.i2p` URLs carry a path BEP 48's
    convention can turn into a scrape endpoint. The other 8 HTTP URLs end in
    `/a`, and this instrument **does not invent `/s`**. Guessing an endpoint
    and then reporting its 404 as the tracker's defect is the fabrication
    `Tracker.scrape_url` returns `None` to prevent, and requesting `/a` itself
    would be an announce.

    So those 8 are measured to a **lower rung** instead: one request to the
    destination's root path, which establishes that the i2p destination is
    reachable and something answers. ⛔ That is `TRANSPORT_RESPONSE`, never
    `TRACKER_SEMANTIC`, and RULES 3.3 forbids reading it as liveness. It is
    strictly more than `unmeasurable` and strictly less than an answer.

    The 2 `udp://` `.i2p` URLs are **not attempted**: an HTTP proxy carries no
    datagrams, and BEP 15 over i2p needs a SAM session this does not open. They
    stay `unmeasurable` and the result says so rather than omitting them.

CONTROL -- the hierarchy in experiments/README.md, and tier 1 is the one that
matters here
    tier 0  a bencode responder this process starts on loopback, fetched with
            **no proxy** and classified by the same `classify_body` the
            subjects go through. Proves the fetch-and-classify path works. If
            tier 0 fails, no row below may be quoted.
    tier 1  a well-known **non-tracker** eepsite through the proxy. Proves the
            router has tunnels and can reach a destination.
            ⛔ **An absence is not a zero** (RULES 2): without this, "no .i2p
            tracker answered" is indistinguishable from "the router never
            built a tunnel", which is the single most relevant rule in this
            project. ⛔ And the control is deliberately **not** a tracker: a
            capability question about us must not be answered by spending
            somebody else's request.
    tier 2  the subjects.

⚠ THE ROUTER'S OWN FAILURE IS NOT THE TRACKER'S
    i2pd's proxy answers **HTTP 500 with its own HTML page** when it cannot
    resolve or reach a destination -- measured against a name nobody owns
    before any tracker was contacted. That is a fact about our route and is
    classified `router_could_not_reach`, which can never become `dead`. It is
    the same rule as `probe.py`'s `ABOUT_US` set, applied to a transport that
    file does not speak.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run
"""

from __future__ import annotations

import argparse
import http.server
import json
import os
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, "src"))
sys.path.insert(0, os.path.join(REPO, "scripts"))

import _conditions as C  # noqa: E402
from generate import load_corpus  # noqa: E402
from trackers import bep15  # noqa: E402
from trackers.bencode import TRACKER_KINDS, classify_body  # noqa: E402
from trackers.model import Network, Rung, Transport  # noqa: E402

FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")

#: A non-tracker eepsite, used only to prove the router works. ⛔ Never a
#: tracker: see the CONTROL block.
CONTROL_EEPSITE = "http://i2p-projekt.i2p/"

#: What i2pd puts in the body of its own error pages. Measured 2026-09-09
#: against `this-name-does-not-exist-t039.i2p`, a name nobody owns, so learning
#: the shape cost no tracker a request.
ROUTER_ERROR_MARKER = b"I2Pd HTTP proxy"

#: ⛔ **THE ROUTER HAS TWO FAILURES AND THEY ARE NOT THE SAME FACT.** Both
#: arrive as HTTP 500 carrying the marker above, and collapsing them loses the
#: only distinction that matters here:
#:
#:   host not found   the name is not in **our** addressbook. A fact about our
#:                    router's naming state, fixable by a jump service, and it
#:                    says nothing whatever about the destination.
#:   host is down     the destination was resolved -- always true for a b32,
#:                    which is its own hash -- and no connection could be
#:                    built. That is about the destination.
#:
#: Neither may become `dead` on one observation (RULES 3.1), and the first may
#: never become a statement about a tracker at all.
ROUTER_FAILURES: tuple[tuple[bytes, str, str], ...] = (
    (b"not found in router's addressbook", "router_name_unknown",
     "the name is not in our router's addressbook; a fact about our naming, "
     "not about the destination"),
    (b"Host is down", "destination_unreachable",
     "the destination was resolved and no connection could be built; one "
     "observation, and never `dead` on its own"),
)

#: Public i2p naming registries, fetched once per run through the proxy. ⭐
#: **A name resolved here is second-hand NAMING and not a second-hand liveness
#: signal**: the request that measures the tracker is still ours. Addressing a
#: destination by its b32 removes our addressbook from the path entirely, and
#: on 2026-09-09 that turned `opentracker.skank.i2p` from a timeout into a
#: 1677-byte answer -- the difference between measuring the tracker and
#: measuring our own router.
JUMP_SERVICES = ("http://reg.i2p/hosts.txt",
                 "http://stats.i2p/cgi-bin/newhosts.txt")

#: Read at most this much of any body. RULES 5.2: every network operation is
#: bounded, and a hostile eepsite is upstream data like any other.
MAX_BYTES = 64 * 1024


def opener_for(proxy: str) -> urllib.request.OpenerDirector:
    """An opener that sends everything through the i2p HTTP proxy.

    ⛔ **Built explicitly rather than relying on `http_proxy` in the
    environment.** An instrument whose route depends on a variable somebody
    exported is one whose committed result cannot say where it went, and
    `conditions` has to be able to name the proxy.
    """
    return urllib.request.build_opener(
        urllib.request.ProxyHandler({"http": proxy, "https": proxy}))


def fetch(opener, url: str, timeout: float) -> dict:
    """One bounded GET. Never raises; the failure is the result."""
    started = time.monotonic()
    try:
        with opener.open(url, timeout=timeout) as response:
            body = response.read(MAX_BYTES)
            return {"status": response.status, "body": body,
                    "rtt_ms": round((time.monotonic() - started) * 1000, 1)}
    except urllib.error.HTTPError as exc:
        return {"status": exc.code, "body": exc.read(MAX_BYTES),
                "rtt_ms": round((time.monotonic() - started) * 1000, 1)}
    except Exception as exc:  # noqa: BLE001 - a failed probe is a datum
        return {"status": None, "body": b"", "error": f"{type(exc).__name__}: {exc}",
                "rtt_ms": round((time.monotonic() - started) * 1000, 1)}


def router_failure(outcome: dict) -> tuple[str, str] | None:
    """Which of the router's own failures this is, or `None` if it is not one.

    ⛔ Matched on the router's message, not merely on the 500, because the two
    failures it reports mean opposite things about the destination.
    """
    if outcome.get("status") != 500:
        return None
    body = outcome.get("body", b"")
    if ROUTER_ERROR_MARKER not in body:
        return None
    for marker, verdict, detail in ROUTER_FAILURES:
        if marker in body:
            return (verdict, detail)
    return ("router_could_not_reach",
            "i2pd's own error page, with a message this instrument does not "
            "recognise; recorded verbatim rather than guessed at")


def jump_table(opener, timeout: float) -> tuple[dict[str, str], list[dict]]:
    """`name -> b32` from the public registries, plus what each one answered.

    ⛔ **Not a liveness signal and never recorded as one.** These lists say
    which destination a name points at; whether that destination answers is
    measured by our own request afterwards.
    """
    import base64
    import hashlib

    table: dict[str, str] = {}
    fetched: list[dict] = []
    for url in JUMP_SERVICES:
        outcome = fetch(opener, url, timeout)
        names = 0
        if outcome.get("status") == 200 and not router_failure(outcome):
            for line in outcome["body"].decode("utf-8", "replace").splitlines():
                if line.startswith("#") or "=" not in line:
                    continue
                name, _, dest = line.partition("=")
                name, dest = name.strip().lower(), dest.strip()
                if not name or name in table:
                    continue
                try:
                    raw = base64.b64decode(
                        dest.translate(str.maketrans("-~", "+/")))
                    digest = hashlib.sha256(raw).digest()
                    table[name] = (base64.b32encode(digest).decode()
                                   .rstrip("=").lower() + ".b32.i2p")
                    names += 1
                except Exception:  # noqa: BLE001 - a bad line is one bad line
                    continue
        fetched.append({"url": url, "http_status": outcome.get("status"),
                        "names": names, "error": outcome.get("error")})
    return table, fetched


def classify(outcome: dict, scraped: bool) -> tuple[str, str, dict]:
    """`(rung, verdict, detail)` for one subject.

    ⛔ **`scraped` decides the ceiling, and that is the whole honesty of this
    function.** A root-path request can reach `TRANSPORT_RESPONSE` and no
    further: something answered on the destination, which RULES 3.3 forbids
    reading as tracker liveness. Only a scrape endpoint can reach
    `TRACKER_SEMANTIC`.
    """
    if outcome.get("error"):
        return (Rung.NONE.value, "no_answer", {"detail": outcome["error"]})
    failure = router_failure(outcome)
    if failure is not None:
        # About our route or about the destination, but never `dead` on one
        # observation (RULES 3.1). Which of the two is the point of the split.
        return (Rung.NONE.value, failure[0], {"detail": failure[1]})
    body = outcome.get("body", b"")
    if not scraped:
        return (Rung.TRANSPORT_RESPONSE.value, "destination_answered",
                {"detail": f"HTTP {outcome.get('status')}, {len(body)} bytes "
                           f"from the destination's root path; NOT a tracker "
                           f"claim (RULES 3.3)",
                 "http_status": outcome.get("status")})
    kind = classify_body(body)
    if kind["kind"] in TRACKER_KINDS:
        return (Rung.TRACKER_SEMANTIC.value, "live",
                {"detail": f"answered as a tracker: {kind['kind']}",
                 "body_kind": kind["kind"], "http_status": outcome.get("status")})
    return (Rung.TRANSPORT_RESPONSE.value, "not_a_tracker",
            {"detail": f"answered, but as {kind['kind']}: {kind['detail']}",
             "body_kind": kind["kind"], "http_status": outcome.get("status")})


class _Responder(http.server.BaseHTTPRequestHandler):
    """A scrape response, for tier 0."""

    def do_GET(self):  # noqa: N802 - the base class names it
        body = (b"d5:filesd20:aaaaaaaaaaaaaaaaaaaad8:completei1e"
                b"10:downloadedi0e10:incompletei0eeee")
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        return


def tier0(timeout: float) -> dict:
    """Does the fetch-and-classify path work at all, with no i2p involved?"""
    try:
        server = http.server.HTTPServer(("127.0.0.1", 0), _Responder)
    except OSError as exc:
        return {"ok": False, "detail": f"could not bind loopback: {exc}"}
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        port = server.server_address[1]
        # ⛔ No proxy: this control must not depend on the thing it exists to
        # be independent of.
        direct = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        outcome = fetch(direct, f"http://127.0.0.1:{port}/scrape", timeout)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2.0)
    kind = classify_body(outcome.get("body", b""))
    ok = kind["kind"] in TRACKER_KINDS
    return {"ok": ok, "body_kind": kind["kind"],
            "detail": f"loopback scrape classified as {kind['kind']}"}


def tier1(opener, timeout: float) -> dict:
    """Does the router have tunnels? Asked of a non-tracker."""
    outcome = fetch(opener, CONTROL_EEPSITE, timeout)
    ok = (not outcome.get("error") and outcome.get("status") == 200
          and router_failure(outcome) is None
          and len(outcome.get("body", b"")) > 0)
    return {"ok": ok, "eepsite": CONTROL_EEPSITE,
            "http_status": outcome.get("status"),
            "bytes": len(outcome.get("body", b"")),
            "rtt_ms": outcome.get("rtt_ms"),
            "error": outcome.get("error"),
            "detail": ("reached a known eepsite, so the router has tunnels and "
                       "a subject that does not answer is a fact about the "
                       "subject" if ok else
                       "the router did not reach a known eepsite, so NOTHING "
                       "below may be read as a statement about any tracker")}


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--proxy", default=os.environ.get("I2P_HTTP_PROXY"),
                        help="the i2p router's HTTP proxy, e.g. "
                             "http://127.0.0.1:4444. docs/containers.md has "
                             "how to start one")
    parser.add_argument("--router-image", default=None,
                        help="the pinned image digest the router came from, "
                             "recorded in the conditions block. A measurement "
                             "whose environment moved is not reproducible")
    parser.add_argument("--timeout", type=float, default=60.0,
                        help="i2p is a high-latency overlay; a clearnet "
                             "timeout would record its own impatience")
    parser.add_argument("--fixtures", default=FIXTURES)
    parser.add_argument("--out", default=None)
    parser.add_argument("--no-jump", dest="use_jump", action="store_false",
                        help="do not consult the public naming registries. "
                             "⚠ Then every name goes through OUR "
                             "addressbook, and a miss there is recorded "
                             "against the tracker's name rather than resolved")
    parser.add_argument("--control-only", action="store_true",
                        help="run the controls and contact no tracker")
    parser.add_argument("--expect-control", action="store_true",
                        help="exit 1 if either control fails")
    args = parser.parse_args()

    if not args.proxy:
        print("no --proxy and no I2P_HTTP_PROXY: this experiment needs a "
              "router. docs/containers.md has how to start one.",
              file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    opener = opener_for(args.proxy)
    controls = {"tier0": tier0(args.timeout), "tier1": tier1(opener, args.timeout)}

    try:
        aggregate, _, _ = load_corpus(True, args.fixtures)
    except Exception as exc:  # noqa: BLE001
        print(f"could not build the corpus: {exc}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN
    subjects = sorted((t for t in aggregate.trackers if t.network is Network.I2P),
                      key=lambda t: t.url)
    if not subjects:
        print("no .i2p tracker in the corpus; refusing to report on nothing",
              file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    # ⭐ Resolve names to destinations before contacting anybody, so a request
    # that fails fails against the destination rather than against our
    # addressbook. Two fetches, both to non-trackers, once per run.
    names: dict[str, str] = {}
    registries: list[dict] = []
    if not args.control_only and controls["tier1"]["ok"] and args.use_jump:
        names, registries = jump_table(opener, args.timeout)

    rows: list[dict] = []
    if not args.control_only and controls["tier1"]["ok"]:
        for tracker in subjects:
            if tracker.transport is not Transport.HTTP:
                rows.append({
                    "url": tracker.url, "rung": Rung.NONE.value,
                    "verdict": "unmeasurable",
                    "detail": ("udp over i2p needs a SAM session; an HTTP "
                               "proxy carries no datagrams. Not attempted, "
                               "and not a statement about the tracker"),
                    "requested": None})
                continue
            # ⛔ The scrape endpoint if BEP 48's convention gives one, and the
            # ROOT path otherwise. Never the announce path, and never an
            # invented `/s`.
            if tracker.scrape_url:
                info_hash = bep15.synthetic_infohash()
                query = urllib.parse.urlencode(
                    {"info_hash": info_hash}, quote_via=urllib.parse.quote)
                target = tracker.scrape_url + (
                    "&" if urllib.parse.urlsplit(tracker.scrape_url).query
                    else "?") + query
                scraped = True
            else:
                split = urllib.parse.urlsplit(tracker.url)
                target = f"{split.scheme}://{split.netloc}/"
                scraped = False
            # ⭐ Address the destination, not the name, wherever a registry
            # gave us one. The port travels with it; the b32 is a host.
            addressed_as = "name"
            b32 = names.get(tracker.host.lower())
            if b32:
                split = urllib.parse.urlsplit(target)
                netloc = b32 + (f":{split.port}" if split.port else "")
                target = urllib.parse.urlunsplit(
                    (split.scheme, netloc, split.path, split.query,
                     split.fragment))
                addressed_as = "b32"
            outcome = fetch(opener, target, args.timeout)
            rung, verdict, detail = classify(outcome, scraped)
            rows.append({
                "url": tracker.url, "requested": target,
                "addressed_as": addressed_as,
                "reached_scrape_endpoint": scraped,
                "used_synthetic_infohash": scraped,
                "rung": rung, "verdict": verdict, "rtt_ms": outcome.get("rtt_ms"),
                **detail})

    counts: dict[str, int] = {}
    for row in rows:
        counts[row["verdict"]] = counts.get(row["verdict"], 0) + 1

    results = {
        "controls": controls,
        "proxy": args.proxy,
        "router_image": args.router_image,
        "naming_registries": registries,
        "names_resolved_to_a_destination": len(names),
        "subjects": len(subjects),
        "counts": counts,
        "rows": rows,
        "vantage": {
            "network": "i2p",
            "first_hand": True,
            "route": ("an i2pd router in a throwaway container; this process "
                      "speaks to its HTTP proxy"),
            "profile": "local",
        },
        "what_this_is_not": (
            "Not an announce, on any row: 3 of 13 URLs carry a path BEP 48 can "
            "turn into a scrape endpoint and the other 8 are measured to "
            "TRANSPORT_RESPONSE against the destination's ROOT path instead. "
            "`destination_answered` is NOT tracker liveness (RULES 3.3). "
            "`router_could_not_reach` is a fact about our route and can never "
            "become `dead` (RULES 3.1). The 2 udp:// URLs were not attempted."),
    }

    conditions = C.with_network_vantage(C.collect(sample_counts={
        "subjects": len(subjects),
        "contacted": len([r for r in rows if r.get("requested")]),
    }))
    C.emit("Are the corpus's .i2p trackers alive, from a vantage that has i2p?",
           conditions, results, args.out)

    print(f"\nCONTROL tier 0  fetch and classify: "
          f"{'PASS' if controls['tier0']['ok'] else 'FAIL'}  "
          f"{controls['tier0']['detail']}")
    print(f"CONTROL tier 1  router has tunnels: "
          f"{'PASS' if controls['tier1']['ok'] else 'FAIL'}  "
          f"{controls['tier1']['detail']}")
    print(f"\n{'VERDICT':22s} {'RUNG':20s} URL")
    for row in rows:
        print(f"  {row['verdict']:20s} {row['rung']:20s} {row['url']}")
    print(f"\ncounts: {counts}")
    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - That a `destination_answered` row is a live TRACKER. It is one")
    print("    rung below that and RULES 3.3 forbids the promotion.")
    print("  - Anything about the 2 udp:// .i2p URLs. Not attempted.")
    print("  - That i2p is reachable from CI. It is not: this needs a router,")
    print("    and the vantage block says which one.")

    if args.expect_control:
        if not controls["tier0"]["ok"]:
            print("\nEXPECTATION FAILED: tier 0 could not classify a scrape it")
            print("  served itself, so no row above may be quoted.")
            return C.EXIT_MEASURED_AND_FAILED
        if not controls["tier1"]["ok"]:
            print("\nEXPECTATION FAILED: the router did not reach a known")
            print("  eepsite, so a subject that did not answer is our route.")
            return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
