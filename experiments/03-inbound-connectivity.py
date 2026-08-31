#!/usr/bin/env python3
"""
QUESTION
    Can anything on the public internet open a connection TO this host?

WHY IT EXISTS
    TODO/RULES.md C-02 records the belief that runners have no usable inbound
    connectivity. The claim is load-bearing for `D2` (HISTORY/decisions.md): a design
    where a runner listens -- for a callback, a webhook, a reflected probe, or
    a second vantage point that dials in -- is impossible if it is true, and
    several of RULES 9.1's "legitimate alternatives" quietly assume it is
    false.

    It also bounds the tracker probe itself. A BitTorrent client normally
    ADVERTISES a listening port; this project must not, and cannot. Knowing
    that inbound is closed turns "we do not listen" from a policy into a
    property of the environment, which is a stronger guarantee.

HOW IT IS MEASURED, AND WHY THIS DESIGN
    Measuring inbound honestly needs a prober OUTSIDE the host -- RULES 2,
    "measure from outside the thing you are measuring". This script has no such
    outside prober available in general, and inventing one would mean depending
    on a third-party port-scan service whose failure is indistinguishable from
    a closed port. So it measures the two things it CAN establish from inside,
    and it labels the third as not established rather than guessing:

      1  Can this host bind and listen on a port at all?          (measurable)
      2  Does the host's own public address route back to it?     (measurable,
         by dialling our own discovered public IP from this host: a hairpin.)
      3  Can an arbitrary third party reach it?                   (NOT measured
         here -- stated as a gap, not as a zero.)

    Case 2 deserves care. A hairpin connect that SUCCEEDS proves the listener
    is reachable at that address from at least one place. A hairpin connect
    that FAILS is weak evidence: many NATs simply do not hairpin. So a failure
    is reported as `inconclusive`, never as "inbound is blocked". Reporting it
    the other way would be exactly the confident wrongness RULES 3 is
    about.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run
"""

from __future__ import annotations

import argparse
import os
import socket
import sys
import threading

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _conditions as C  # noqa: E402

TIMEOUT = 6.0
BANNER = b"trackers/experiment-03 inbound probe\n"


class Listener:
    """A TCP listener that answers one banner and closes. Bound to 0.0.0.0."""

    def __init__(self, port: int = 0):
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.sock.bind(("0.0.0.0", port))
        self.sock.listen(4)
        self.port = self.sock.getsockname()[1]
        self.accepted = 0
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._serve, daemon=True)

    def _serve(self):
        self.sock.settimeout(0.25)
        while not self.stop.is_set():
            try:
                conn, _ = self.sock.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            self.accepted += 1
            try:
                conn.sendall(BANNER)
            except OSError:
                pass
            finally:
                conn.close()

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *a):
        self.stop.set()
        self.thread.join(timeout=2)
        self.sock.close()
        return False


def dial(host: str, port: int) -> dict:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(TIMEOUT)
    try:
        with C.Timer() as t:
            s.connect((host, port))
            data = s.recv(len(BANNER))
        ok = data == BANNER
        return {"ok": ok, "rtt_ms": round(t.ms, 3),
                "detail": "banner matched" if ok else f"unexpected payload {data!r}"}
    except Exception as e:
        return {"ok": False, "rtt_ms": None, "detail": f"{type(e).__name__}: {e}"}
    finally:
        s.close()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--port", type=int, default=0, help="0 picks an ephemeral port")
    ap.add_argument("--expect-no-inbound", action="store_true",
                    help="exit 1 if the public hairpin connect SUCCEEDS")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    results: dict = {}
    try:
        listener_ctx = Listener(args.port)
    except OSError as e:
        print(f"could not bind a listening socket: {e}", file=sys.stderr)
        results["bind"] = {"ok": False, "detail": f"{type(e).__name__}: {e}"}
        conditions = C.collect(sample_counts={"attempts": 0})
        C.emit("Can anything on the public internet open a connection to this host?",
               conditions, results, args.out or C.results_path(__file__))
        return C.EXIT_COULD_NOT_RUN

    with listener_ctx as lis:
        results["bind"] = {"ok": True, "port": lis.port,
                           "detail": f"listening on 0.0.0.0:{lis.port}"}

        # Control: loopback dial. Proves the listener and the dialler both work.
        results["loopback_control"] = dial("127.0.0.1", lis.port)

        # The public hairpin. Success is strong; failure is weak.
        pub_ip, pub_org = C.public_ip()
        results["public_ip"] = pub_ip
        results["public_ip_org"] = pub_org
        if pub_ip == C.UNKNOWN:
            results["public_hairpin"] = {
                "ok": False, "rtt_ms": None,
                "detail": "public IP not determinable; hairpin not attempted",
                "conclusive": False,
            }
        else:
            r = dial(pub_ip, lis.port)
            # A failure here is NOT evidence of a blocked inbound path.
            r["conclusive"] = bool(r["ok"])
            results["public_hairpin"] = r

        results["accepted_connections"] = lis.accepted

    hairpin = results["public_hairpin"]
    results["verdict"] = {
        "can_bind_and_listen": results["bind"]["ok"],
        "loopback_reachable": results["loopback_control"]["ok"],
        "reachable_at_public_address": hairpin["ok"],
        "inbound_from_arbitrary_third_party": C.UNKNOWN,
        "not_established": [
            "Whether an arbitrary internet host can reach this listener. That "
            "needs a prober outside this machine; this script does not have one "
            "and does not pretend to. A failed hairpin is consistent with both "
            "a blocked inbound path and a NAT that simply does not hairpin.",
        ],
    }

    conditions = C.collect(sample_counts={"listen_ports": 1, "dial_attempts":
                                          2 if results.get("public_ip") != C.UNKNOWN else 1})
    out = args.out or C.results_path(__file__)
    C.emit("Can anything on the public internet open a connection to this host?",
           conditions, results, out)

    print("\nRESULTS")
    print(f"  bind + listen:        {'PASS' if results['bind']['ok'] else 'FAIL'}  {results['bind']['detail']}")
    lc = results["loopback_control"]
    print(f"  loopback control:     {'PASS' if lc['ok'] else 'FAIL'}  {lc['detail']}")
    print(f"  public address:       {results.get('public_ip', C.UNKNOWN)}  ({results.get('public_ip_org', C.UNKNOWN)})")
    print(f"  public hairpin:       {'PASS' if hairpin['ok'] else 'FAIL'}  {hairpin['detail']}")
    print(f"  connections accepted: {results['accepted_connections']}")

    print("\nWHAT THIS DOES NOT ESTABLISH")
    for line in results["verdict"]["not_established"]:
        print(f"  - {line}")

    print("\nINTERPRETATION RULE")
    if not results["loopback_control"]["ok"]:
        print("  Loopback control failed -> this is a broken instrument, not a finding.")
    elif hairpin["ok"]:
        print("  The listener answered on its own public address -> this host IS")
        print("  reachable inbound from at least one vantage. C-02 is refuted as")
        print("  stated and any design forbidden by it should be reconsidered.")
    else:
        print("  The listener works locally and did not answer on its public address.")
        print("  That is CONSISTENT WITH no usable inbound connectivity and is not")
        print("  proof of it. Record C-02 as inconclusive from this instrument and")
        print("  do not build a design that depends on inbound either way.")

    if args.expect_no_inbound and hairpin["ok"]:
        print("\nEXPECTATION FAILED: --expect-no-inbound (the host WAS reachable)")
        return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
