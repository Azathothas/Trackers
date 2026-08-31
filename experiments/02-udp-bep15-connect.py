#!/usr/bin/env python3
"""
QUESTION
    Does a BEP 15 connect handshake complete against known-good UDP trackers
    from this host -- and, when it does not, is that the network, the trackers,
    or this probe?

WHY IT EXISTS
    TODO/RULES.md C-01 makes BEP 15 probing the load-bearing measurement for
    every `udp://` tracker in the corpus. How large a share of the corpus that
    is, is not asserted here -- it is measured by experiment 19 (the scheme
    census), and quoting a remembered count in this header would be exactly the
    unconditioned number RULES 1.5 forbids. RULES 2 states the rule
    that makes the answer trustworthy: "an absence is not a zero. A probe that
    found nothing may have been looking in the wrong place."

    So this script never reports a bare failure count. It runs a POSITIVE
    CONTROL first: a BEP 15 responder on loopback, started by this process,
    which answers a correct connect. The control and the subjects are probed by
    the same code path, so:

        control PASS, subjects FAIL  -> the probe is correct; the failure is
                                        the network or the trackers, and
                                        experiment 01 separates those two.
        control FAIL                 -> the probe is broken. No subject result
                                        may be quoted at all.

PROTOCOL, VERIFIED AGAINST THE SPEC (not against memory)
    BEP 15, https://www.bittorrent.org/beps/bep_0015.html, fetched 2026-08-29.

      connect request   (16 bytes, and 16 bytes is the whole of it)
        offset 0   u64  protocol_id     0x41727101980   magic constant
        offset 8   u32  action          0               connect
        offset 12  u32  transaction_id  random

      connect response  (>= 16 bytes)
        offset 0   u32  action          0
        offset 4   u32  transaction_id  must equal the one we chose
        offset 8   u64  connection_id

      error response
        offset 0   u32  action          3
        offset 4   u32  transaction_id
        offset 8   string  human-readable message

    NOTE, and it is the ethically load-bearing fact of this whole project:
    the connect request has NO info_hash field. There is nowhere in those 16
    bytes to put one. A connect therefore cannot express interest in any
    content and cannot enter this host into any swarm's peer list. info_hash
    first appears in the ANNOUNCE request at offset 16. This script never
    sends an announce. (RULES 4, C-31.)

INPUTS (pinned)
    fixtures/probe-targets.tsv -- captured 2026-08-29 from newTrackon's live
    list, so every UDP subject was live by an independent oracle at capture.

POLITENESS
    One connect datagram per target per run, plus at most --retries retries.
    A connect is 16 bytes out and 16 bytes back. RULES 4 caps this
    project below one well-behaved client's load; a single connect is roughly
    a thousandth of one announce cycle's traffic.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run
"""

from __future__ import annotations

import argparse
import os
import socket
import statistics
import struct
import sys
import threading
from urllib.parse import urlsplit

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _conditions as C  # noqa: E402

PROTOCOL_ID = 0x41727101980
ACTION_CONNECT = 0
ACTION_ERROR = 3


# --- BEP 15 codec -------------------------------------------------------------
def build_connect_request(transaction_id: int) -> bytes:
    """Exactly 16 bytes: protocol_id, action, transaction_id. Nothing else fits."""
    return struct.pack(">QII", PROTOCOL_ID, ACTION_CONNECT, transaction_id)


def parse_connect_response(data: bytes, expect_txid: int) -> tuple[bool, str, int | None]:
    """Return (ok, detail, connection_id).

    Validation order is BEP 15's own: length, then transaction id, then action.
    Checking the transaction id BEFORE trusting the action matters -- an
    unsolicited or spoofed datagram must not be read as a live tracker.
    """
    if len(data) < 16:
        if len(data) >= 8:
            action, txid = struct.unpack(">II", data[:8])
            if action == ACTION_ERROR and txid == expect_txid:
                msg = data[8:].decode("utf-8", "replace")
                # An error response is still a WORKING TRACKER: it parsed our
                # datagram and answered in-protocol. That is a strictly stronger
                # signal than a socket that merely accepted bytes.
                return False, f"BEP15 error response: {msg!r}", None
        return False, f"short response: {len(data)} bytes (BEP 15 requires >= 16)", None
    action, txid, conn_id = struct.unpack(">IIQ", data[:16])
    if txid != expect_txid:
        return False, f"transaction id mismatch: got {txid}, sent {expect_txid}", None
    if action == ACTION_ERROR:
        return False, f"BEP15 error response: {data[8:].decode('utf-8', 'replace')!r}", None
    if action != ACTION_CONNECT:
        return False, f"unexpected action {action} (expected 0)", None
    return True, f"connection_id=0x{conn_id:016x}", conn_id


# --- the positive control: a BEP 15 responder we own --------------------------
class LoopbackBEP15Tracker:
    """A minimal, correct BEP 15 connect responder on loopback.

    This is an ORACLE in the sense of TEMPLATE's `docs/methodology/references.md` section 5: it produces
    ground truth independently of the thing being measured. It is the seed of
    the fake-tracker oracle T-021 requires for the real probe.
    """

    def __init__(self, connection_id: int = 0x0123456789ABCDEF):
        self.connection_id = connection_id
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.sock.bind(("127.0.0.1", 0))
        self.port = self.sock.getsockname()[1]
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._serve, daemon=True)
        self.seen = 0

    def _serve(self):
        self.sock.settimeout(0.25)
        while not self.stop.is_set():
            try:
                data, addr = self.sock.recvfrom(2048)
            except socket.timeout:
                continue
            except OSError:
                break
            if len(data) < 16:
                continue
            pid, action, txid = struct.unpack(">QII", data[:16])
            if pid != PROTOCOL_ID:
                # Wrong magic: a real tracker ignores this. So do we -- silence
                # here is what makes the control meaningful.
                continue
            self.seen += 1
            if action == ACTION_CONNECT:
                self.sock.sendto(
                    struct.pack(">IIQ", ACTION_CONNECT, txid, self.connection_id), addr
                )

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *a):
        self.stop.set()
        self.thread.join(timeout=2)
        self.sock.close()
        return False


# --- the probe ----------------------------------------------------------------
def bep15_connect(host: str, port: int, timeout: float, retries: int) -> dict:
    """One BEP 15 connect against one endpoint. Records which rung it reached.

    Rungs, per the ladder in TODO/measurement.md: dns -> datagram_sent -> response_received ->
    protocol_valid. Recording the rung is what lets a consumer tell "the name
    is gone" from "the tracker refused us".
    """
    rung = "none"
    try:
        # AF_UNSPEC, not AF_INET. Forcing AF_INET makes an IPv6-only tracker
        # raise here and be recorded as `dns_failure`, which is false: the name
        # resolved perfectly well. That misclassification is the same class of
        # lie as marking such a tracker dead (C-04, RULES 3.1), so the two
        # cases are separated below instead of being collapsed by the resolver
        # call.
        infos = socket.getaddrinfo(host, port, socket.AF_UNSPEC, socket.SOCK_DGRAM)
    except Exception as e:
        return {"ok": False, "rung": "dns_failure", "rtt_ms": None,
                "detail": f"{type(e).__name__}: {e}", "resolved_ip": C.UNKNOWN,
                "families": []}
    families = sorted({"ipv6" if i[0] == socket.AF_INET6 else "ipv4" for i in infos})
    v4 = [i for i in infos if i[0] == socket.AF_INET]
    if not v4:
        # Resolves, but to no IPv4 address. This probe is IPv4-only by
        # construction, so the honest report is "this vantage cannot reach it",
        # never "dead" and never "dns_failure".
        return {"ok": False, "rung": "no_ipv4_address", "rtt_ms": None,
                "detail": f"resolves only to {families}; this probe is IPv4-only",
                "resolved_ip": infos[0][4][0], "families": families}
    addr = v4[0][4]
    rung = "dns"
    last = "no attempt"
    for attempt in range(retries + 1):
        txid = struct.unpack(">I", os.urandom(4))[0]
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.settimeout(timeout)
        try:
            with C.Timer() as t:
                s.sendto(build_connect_request(txid), addr)
                rung = "datagram_sent"
                data, _ = s.recvfrom(2048)
            rung = "response_received"
            ok, detail, conn = parse_connect_response(data, txid)
            if ok:
                rung = "protocol_valid"
                return {"ok": True, "rung": rung, "rtt_ms": round(t.ms, 3),
                        "detail": detail, "resolved_ip": addr[0],
                        "attempts": attempt + 1, "families": families}
            last = detail
            # An in-protocol error reply still proves a live, speaking tracker.
            if detail.startswith("BEP15 error response"):
                return {"ok": False, "rung": "protocol_valid_error", "rtt_ms": round(t.ms, 3),
                        "detail": detail, "resolved_ip": addr[0],
                        "attempts": attempt + 1, "families": families}
        except socket.timeout:
            last = f"timeout after {timeout}s"
        except Exception as e:
            last = f"{type(e).__name__}: {e}"
        finally:
            s.close()
    return {"ok": False, "rung": rung, "rtt_ms": None, "detail": last,
            "resolved_ip": addr[0], "attempts": retries + 1, "families": families}


def load_targets(path: str) -> list[dict]:
    rows = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) < 4:
                continue
            scheme, url, port, oracle = parts[0], parts[1], int(parts[2]), parts[3]
            rows.append({"scheme": scheme, "url": url, "port": port, "oracle_at_capture": oracle})
    return rows


def main() -> int:
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--targets", default=os.path.join(here, "fixtures", "probe-targets.tsv"))
    ap.add_argument("--timeout", type=float, default=5.0)
    ap.add_argument("--retries", type=int, default=1,
                    help="BEP 15 says retransmit at 15*2^n seconds; that pacing is for a "
                         "downloading client. One retry is enough to separate loss from block.")
    ap.add_argument("--expect-control", action="store_true",
                    help="exit 1 unless the loopback BEP 15 control answers")
    ap.add_argument("--expect-any-live", action="store_true",
                    help="exit 1 unless at least one real UDP tracker completes a connect")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    try:
        targets = load_targets(args.targets)
    except OSError as e:
        print(f"could not read targets: {e}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    udp_targets = [t for t in targets if t["scheme"] == "udp"]
    if not udp_targets:
        print("no udp targets in fixture", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    # --- positive control, run FIRST and run TWICE ---------------------------
    # TEMPLATE experiments.md: 'a control run once is a coincidence you have
    # not noticed yet.'
    control_runs = []
    with LoopbackBEP15Tracker() as fake:
        for _ in range(2):
            control_runs.append(bep15_connect("127.0.0.1", fake.port, args.timeout, 0))
        datagrams_seen = fake.seen
    control_ok = all(r["ok"] for r in control_runs)

    # --- subjects ------------------------------------------------------------
    subject_results = []
    for t in udp_targets:
        parts = urlsplit(t["url"])
        host = parts.hostname or ""
        port = parts.port or t["port"]
        r = bep15_connect(host, port, args.timeout, args.retries)
        r.update({"url": t["url"], "host": host, "port": port,
                  "oracle_at_capture": t["oracle_at_capture"]})
        subject_results.append(r)

    live = [r for r in subject_results if r["ok"]]
    speaking = [r for r in subject_results if r["rung"].startswith("protocol_valid")]
    rtts = [r["rtt_ms"] for r in live if r["rtt_ms"] is not None]

    results = {
        "control": {
            "runs": control_runs,
            "all_ok": control_ok,
            "datagrams_received_by_control": datagrams_seen,
            "note": "loopback BEP 15 responder started by this process; two runs",
        },
        "subjects": subject_results,
        "summary": {
            "udp_targets": len(udp_targets),
            "connect_ok": len(live),
            "spoke_bep15_at_all": len(speaking),
            "oracle_said_live_at_capture": sum(
                1 for t in udp_targets if t["oracle_at_capture"] == "live"),
            "median_rtt_ms": round(statistics.median(rtts), 3) if rtts else None,
            "min_rtt_ms": round(min(rtts), 3) if rtts else None,
            "max_rtt_ms": round(max(rtts), 3) if rtts else None,
            "rung_histogram": {
                rung: sum(1 for r in subject_results if r["rung"] == rung)
                for rung in sorted({r["rung"] for r in subject_results})
            },
        },
    }

    conditions = C.with_network_vantage(C.collect(sample_counts={
        "udp_targets": len(udp_targets),
        "control_runs": len(control_runs),
        "probes_per_target": 1,
        "max_retries_per_target": args.retries,
    }, extra={
        "targets_fixture": os.path.relpath(args.targets, here),
        "timeout_s": args.timeout,
        "spec": "BEP 15, fetched 2026-08-29",
    }))
    out = args.out or C.results_path(__file__)
    C.emit(
        "Does a BEP 15 connect handshake complete against known-good UDP trackers "
        "from this host, and if not, is it the network, the trackers, or the probe?",
        conditions, results, out,
    )

    print("\nPOSITIVE CONTROL  loopback BEP 15 responder (run twice)")
    for i, r in enumerate(control_runs, 1):
        print(f"  run {i}: {'PASS' if r['ok'] else 'FAIL'}  rung={r['rung']}  {r['detail']}")
    print(f"  datagrams the control actually received: {datagrams_seen}")
    if not control_ok:
        print("  !! CONTROL FAILED -- the probe is broken. No subject row below")
        print("  !! may be quoted as evidence about any tracker.")

    print(f"\nSUBJECTS  {len(udp_targets)} UDP trackers, all 'live' by newTrackon at capture")
    for r in subject_results:
        mark = "PASS" if r["ok"] else "FAIL"
        rtt = f"{r['rtt_ms']:.1f}ms" if r["rtt_ms"] is not None else "     -"
        print(f"  {mark}  {rtt:>8s}  rung={r['rung']:<22s} {r['url']}")
        if not r["ok"]:
            print(f"                                          {r['detail']}")

    s = results["summary"]
    print("\nSUMMARY")
    print(f"  connect completed:        {s['connect_ok']}/{s['udp_targets']}")
    print(f"  spoke BEP 15 at all:      {s['spoke_bep15_at_all']}/{s['udp_targets']}")
    print(f"  median RTT:               {s['median_rtt_ms'] if s['median_rtt_ms'] is not None else C.UNKNOWN} ms")
    print(f"  rung histogram:           {s['rung_histogram']}")

    print("\nINTERPRETATION RULE")
    if not control_ok:
        print("  Control failed -> broken probe. Fix the probe; report nothing else.")
    elif live:
        print("  Control passed and real trackers answered -> BEP 15 probing WORKS from")
        print("  this host. Any target that failed is a fact about that target or about")
        print("  this vantage point, not about UDP support.")
    else:
        print("  Control passed and NO real tracker answered -> the probe is correct and")
        print("  something between this host and every tracker is not. Read experiment 01:")
        print("  if its tier-1 controls also failed, arbitrary-port UDP egress is blocked")
        print("  here and these trackers are UNMEASURABLE from this vantage, never dead.")

    if args.expect_control and not control_ok:
        print("\nEXPECTATION FAILED: --expect-control")
        return C.EXIT_MEASURED_AND_FAILED
    if args.expect_any_live and not live:
        print("\nEXPECTATION FAILED: --expect-any-live")
        return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
