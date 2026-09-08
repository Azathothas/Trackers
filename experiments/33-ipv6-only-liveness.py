#!/usr/bin/env python3
"""
QUESTION
    The corpus holds 16 tracker URLs on 15 hosts that resolve to IPv6 and
    nothing else. Every one of them is `unmeasurable` from a GitHub runner,
    which has no IPv6 egress. Are they alive, and is there any route this
    project may use that reaches them?

WHY IT EXISTS
    T-031, the leverage entry. `unmeasurable` is the honest label on our data
    and it is not permission to stop trying (RULES 10.1a). Route (e) already
    dissolved three of the fifteen by finding an IPv4 sibling; route (c), an
    oracle, turned out not to cover them. This is routes (a) and (b): reach
    them from somewhere that has IPv6.

    Two arms, and they answer different questions:

    DIRECT      from a vantage that has IPv6 egress. A contributor's machine
                usually does; `TRACKERS_PROFILE=local` is what permits the
                attempt (RULES 15.4 -- the capability is in the code and the
                `ci` profile withholds it for a measured reason, `C-04`).
                This is a first-hand measurement and carries its vantage.

    PROXIED     through the operator-approved read proxy of RULES 16, which
                has its own IPv6 egress. ⭐ This is the arm that matters for
                CI, because a runner cannot do the first one at all. It is
                SECOND-HAND by construction: the proxy is the client, so what
                comes back is evidence about the tracker recorded through
                `src/trackers/secondhand.py`, which cannot emit a health
                state.

THE CONTROL HIERARCHY  (experiments/README.md; RULES 2: an absence is not a zero)
    tier 0  a BEP 15 responder this process starts on ::1, probed through the
            production prober. If it fails, this vantage cannot speak the
            protocol over IPv6 and NO other row here may be quoted.
    tier 1  a third party that is IPv6-only and is not a tracker
            (`ipv6.google.com:443`). Separates "this network has no IPv6
            egress" from "the probe is broken", and its answer is what gets
            passed to `vantage.detect(ipv6_egress=...)` rather than a routing
            table's opinion -- `C-04` is the case where those two disagree.
    tier 2  the subjects.

ETHICS  (RULES 4)
    - BEP 34 is consulted for every subject before anything is sent, in both
      arms. A denial or an undetermined lookup skips the tracker.
    - Connect and scrape only. There is no announce path in this project.
    - One attempt per endpoint per arm. No retries against a real tracker.
    - The proxied arm makes a third party contact a tracker on our behalf, so
      it is opt-out (`--no-proxy`) and it sends one request per endpoint.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run
"""

from __future__ import annotations

import argparse
import os
import socket
import struct
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from urllib.parse import urlsplit

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, "src"))
sys.path.insert(0, os.path.join(REPO, "scripts"))

import _conditions as C  # noqa: E402
from generate import load_corpus  # noqa: E402
from trackers import bep15  # noqa: E402
from trackers.bep34 import Decision, Resolver, protocol_for_transport  # noqa: E402
from trackers.model import Transport  # noqa: E402
from trackers.probe import ProbeConfig, effective_port, probe  # noqa: E402
from trackers.secondhand import Observation, Signal  # noqa: E402
from trackers.vantage import detect as detect_vantage  # noqa: E402

FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")

#: RULES 16's read-only route for ordinary web fetches. It carries none of this
#: project's credentials and is never used for a write of any kind.
PROXY = "https://api.rv.pkgforge.dev/"

#: tier 1. IPv6-only, not a tracker, and answers a TLS handshake deterministically.
EGRESS_CONTROL = ("ipv6.google.com", 443)

#: What the proxy is, in the words that go on every record it produces.
PROXY_REACH = ("a read proxy with IPv6 egress, which a GitHub runner does not "
               "have (C-04)")
PROXY_METHOD = ("HTTP scrape issued by the proxy, not by this project. The "
                "proxy is the client and the measurement includes it (C-62)")


# --- tier 0: a BEP 15 responder on ::1 ---------------------------------------
class LoopbackUdpTracker:
    """Answers one BEP 15 connect correctly, over IPv6 loopback.

    Deliberately not `tests/fake_tracker.py`: that oracle binds IPv4, and the
    point of this control is the family. It is the same protocol code being
    exercised either way -- `bep15.py` builds the response here as well.
    """

    def __init__(self) -> None:
        self.sock = socket.socket(socket.AF_INET6, socket.SOCK_DGRAM)
        self.sock.bind(("::1", 0))
        self.port = self.sock.getsockname()[1]
        self.requests: list[bytes] = []
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._serve, daemon=True)

    def __enter__(self) -> "LoopbackUdpTracker":
        self._thread.start()
        return self

    def __exit__(self, *exc: object) -> None:
        self._stop.set()
        self.sock.close()
        self._thread.join(timeout=2.0)

    def _serve(self) -> None:
        self.sock.settimeout(0.2)
        while not self._stop.is_set():
            try:
                data, peer = self.sock.recvfrom(4096)
            except (socket.timeout, TimeoutError):
                continue
            except OSError:
                return
            self.requests.append(data)
            if len(data) != bep15.CONNECT_REQUEST_SIZE:
                continue
            _, _, txid = struct.unpack(">QII", data)
            reply = struct.pack(">IIQ", 0, txid, 0x1234_5678_9ABC_DEF0)
            try:
                self.sock.sendto(reply, peer)
            except OSError:
                return


def tier0(vantage) -> dict:
    """Can this vantage speak BEP 15 over IPv6 at all?"""
    try:
        with LoopbackUdpTracker() as fake:
            from trackers.normalize import parse
            resolver = Resolver()
            result = probe(parse(f"udp://[::1]:{fake.port}/announce"),
                           ProbeConfig(timeout=2.0, retries=0), vantage,
                           observed_at=C.utc(),
                           resolver=resolver)
            sent = len(fake.requests)
    except OSError as exc:
        return {"ok": False, "detail": f"could not bind ::1: {exc}", "sent": 0}
    return {"ok": bool(result.ok), "detail": result.detail,
            "failure": result.failure.value, "rung": result.rung.value,
            "sent": sent}


# --- tier 1: is there IPv6 egress from here, right now? ----------------------
def tier1() -> dict:
    """A TLS handshake to an IPv6-only third party that is not a tracker.

    ⛔ Measured rather than read from a routing table. `C-04` is the case where
    a host has an IPv6 stack and a route and still cannot get a packet out, and
    a run that believed the routing table there would report every subject
    below as dead.
    """
    host, port = EGRESS_CONTROL
    try:
        infos = socket.getaddrinfo(host, port, socket.AF_INET6,
                                   socket.SOCK_STREAM)
    except OSError as exc:
        return {"ok": False, "detail": f"{host} does not resolve to IPv6: {exc}"}
    address = infos[0][4]
    started = time.monotonic()
    try:
        with socket.create_connection((address[0], port), timeout=8) as sock:
            sock.settimeout(8)
            elapsed = (time.monotonic() - started) * 1000.0
    except OSError as exc:
        return {"ok": False, "address": address[0],
                "detail": f"{type(exc).__name__}: {exc}"}
    return {"ok": True, "address": address[0], "rtt_ms": round(elapsed, 3),
            "detail": f"TCP to {host} over IPv6"}


# --- the corpus's IPv6-only population ---------------------------------------
def ipv6_only_urls(trackers) -> list:
    """Every tracker whose host has IPv6 addresses and no IPv4 address.

    Resolution is `getaddrinfo` with `AF_UNSPEC`, the same call the probe makes,
    so the population here is the population the probe would meet.
    """
    out = []
    for tracker in trackers:
        host = tracker.host
        if not host:
            continue
        try:
            infos = socket.getaddrinfo(host, None, socket.AF_UNSPEC,
                                       socket.SOCK_STREAM)
        except OSError:
            continue
        families = {"ipv6" if i[0] == socket.AF_INET6 else "ipv4" for i in infos}
        if families == {"ipv6"}:
            out.append(tracker)
    return out


# --- the proxied arm ---------------------------------------------------------
def scrape_through_proxy(tracker, timeout: float = 20.0) -> tuple[Signal, str]:
    """Ask the proxy to fetch this tracker's scrape URL.

    Returns `(signal, detail)`. ⛔ Never a health state: what comes back is
    routed through `secondhand.Observation`, which cannot emit one.
    """
    target = tracker.scrape_url or tracker.url
    info_hash = bep15.synthetic_infohash()
    query = urllib.parse.urlencode({"info_hash": info_hash},
                                   quote_via=urllib.parse.quote)
    joined = target + ("&" if urlsplit(target).query else "?") + query
    request = urllib.request.Request(
        PROXY + joined,
        headers={"Accept": "*/*",
                 "User-Agent": "trackers/0.1 (+https://github.com/Azathothas/Trackers; T-031 indirect liveness)"})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            body = response.read(65536)
            status = response.status
    except urllib.error.HTTPError as exc:
        try:
            body = exc.read(65536)
        except Exception:  # noqa: BLE001
            body = b""
        status = exc.code
    except Exception as exc:  # noqa: BLE001
        return Signal.UNASSESSED, f"{type(exc).__name__}: {exc}"

    from trackers.bencode import TRACKER_KINDS, classify_body
    classified = classify_body(body)
    kind = classified["kind"]
    if kind in TRACKER_KINDS:
        return Signal.ALIVE, f"HTTP {status}, {kind}"
    # ⚠ Anything else is unassessed rather than not-alive. The proxy stands
    # between us and the tracker, so a non-tracker body may be the proxy's own
    # error page and reading it as the tracker's answer would be exactly the
    # confident wrongness this arm has to avoid.
    return Signal.UNASSESSED, f"HTTP {status}, body kind {kind}"


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", default=None)
    parser.add_argument("--fixtures", default=FIXTURES)
    parser.add_argument("--no-proxy", action="store_true",
                        help="skip the proxied arm, which asks a third party "
                             "to contact each HTTP subject once")
    parser.add_argument("--limit", type=int, default=None)
    parser.add_argument("--timeout", type=float, default=8.0)
    parser.add_argument("--expect-egress", action="store_true",
                        help="exit 1 if tier 0 or tier 1 fails, which is what "
                             "makes this a regression check rather than a "
                             "one-off: without them no subject row means "
                             "anything")
    args = parser.parse_args()

    vantage = detect_vantage()
    if "ipv6" not in vantage.ip_families:
        print("this run cannot attempt IPv6: the vantage permits "
              f"{list(vantage.ip_families)}.", file=sys.stderr)
        print("TRACKERS_PROFILE=local is what permits the attempt (RULES 15.4).",
              file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    try:
        aggregate, _, _ = load_corpus(True, args.fixtures)
    except Exception as exc:  # noqa: BLE001
        print(f"could not build the corpus: {exc}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    control0 = tier0(vantage)
    control1 = tier1()
    # The routing table's opinion is replaced by the measurement, whichever way
    # it went. `detect` takes it as an argument for exactly this.
    vantage = detect_vantage(ipv6_egress=bool(control1["ok"]))

    subjects = ipv6_only_urls(aggregate.trackers)
    subjects.sort(key=lambda t: t.url)
    if args.limit:
        subjects = subjects[:args.limit]

    resolver = Resolver()
    config = ProbeConfig(timeout=args.timeout, retries=0)
    rows: list[dict] = []
    observations: list[dict] = []

    can_probe = bool(control0["ok"] and control1["ok"])
    for tracker in subjects:
        row: dict = {"url": tracker.url, "host": tracker.host,
                     "transport": tracker.transport.value}
        verdict = resolver.consult(
            tracker.host, protocol_for_transport(tracker.transport.value),
            effective_port(tracker))
        row["bep34"] = verdict.as_record()
        if verdict.decision is not Decision.ALLOW:
            # ⛔ Not contacted, by either arm. The operator's answer governs
            # every route, not only the direct one.
            row["direct"] = {"skipped": f"BEP 34 {verdict.decision.value}"}
            rows.append(row)
            continue

        if can_probe:
            result = probe(tracker, config, vantage, resolver=resolver)
            row["direct"] = {
                "ok": result.ok, "rung": result.rung.value,
                "failure": result.failure.value, "detail": result.detail,
                "rtt_ms": result.rtt_ms, "resolved_ip": result.resolved_ip,
            }
        else:
            row["direct"] = {"skipped": "a control failed; see tiers"}

        if not args.no_proxy and tracker.transport in (Transport.HTTP,
                                                       Transport.HTTPS):
            signal, detail = scrape_through_proxy(tracker)
            observation = Observation(
                url=tracker.url, observer=PROXY, observed_at=C.utc(),
                method=PROXY_METHOD, signal=signal, reach=PROXY_REACH,
                detail=detail)
            record = observation.as_record()
            row["second_hand"] = record
            observations.append(record)
        rows.append(row)

    direct_alive = [r for r in rows if r.get("direct", {}).get("ok")]
    proxy_alive = [o for o in observations if o["signal"] == "alive"]
    results = {
        "controls": {"tier0_loopback_ipv6_bep15": control0,
                     "tier1_ipv6_egress": control1},
        "vantage": vantage.as_dict(),
        "subjects": len(subjects),
        "direct_alive": len(direct_alive),
        "second_hand_alive": len(proxy_alive),
        "rows": rows,
        "what_this_is_not": (
            "Not a health state. Nothing here writes one, the proxied arm "
            "cannot produce one, and a tracker unreachable from a runner stays "
            "`unmeasurable` there whatever this run found."),
    }

    conditions = C.with_network_vantage(C.collect(sample_counts={
        "ipv6_only_subjects": len(subjects),
        "corpus_trackers": len(aggregate.trackers),
        "proxied_requests": len(observations),
    }))
    C.emit("Are the IPv6-only trackers alive, from a vantage that has IPv6?",
           conditions, results, args.out)

    print(f"\nCONTROLS")
    print(f"  tier 0  BEP 15 over ::1        "
          f"{'PASS' if control0['ok'] else 'FAIL'}  {control0.get('detail', '')}")
    print(f"  tier 1  IPv6 egress to a third party  "
          f"{'PASS' if control1['ok'] else 'FAIL'}  {control1.get('detail', '')}")
    if not control0["ok"] or not control1["ok"]:
        print("  ⛔ A control failed, so no subject row below may be quoted.")

    print(f"\nSUBJECTS  {len(subjects)} IPv6-only tracker URLs")
    for row in rows:
        direct = row.get("direct", {})
        mark = ("alive" if direct.get("ok") else
                direct.get("skipped") or direct.get("failure", "-"))
        second = row.get("second_hand", {}).get("signal", "-")
        print(f"  {row['url'][:58]:60s} direct={mark:22s} proxied={second}")

    print(f"\nDIRECT LIVENESS: {len(direct_alive)} of {len(subjects)}")
    print(f"SECOND-HAND ALIVE: {len(proxy_alive)} of {len(observations)} asked")

    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - That a runner can reach any of these. It cannot; that is C-04,")
    print("    and it is why the proxied arm exists at all.")
    print("  - That the proxied answers are ours. The proxy is the client and")
    print("    every one is recorded second-hand with its method.")
    print("  - That a subject with no answer is dead. One observation from one")
    print("    vantage on one day, and MIN_SAMPLES_FOR_DEATH is 3.")

    if args.expect_egress and not (control0["ok"] and control1["ok"]):
        print("\nEXPECTATION FAILED: --expect-egress")
        print("  A control failed, so this run measured nothing about any")
        print("  tracker. That is a result and it is not a green one.")
        return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
