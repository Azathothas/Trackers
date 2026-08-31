#!/usr/bin/env python3
"""
QUESTION
    Which network egress does the host running this script actually have --
    which TCP ports, which UDP ports, and does IPv6 leave the machine?

WHY IT EXISTS
    TODO/RULES.md C-01 asks whether GitHub-hosted runners permit outbound UDP to
    arbitrary ports, because BEP 15 tracker probing is impossible without it.
    C-04 asks the same of IPv6, where the consequence is worse: with no IPv6
    egress, every IPv6-only tracker measures dead and the published score is a
    lie (RULES 3.4).

    Neither question can be answered by a probe that returns nothing, because
    "the port is blocked" and "the probe is broken" produce identical silence.
    So this script runs THREE tiers and reports which tier broke:

        tier 0  loopback control     -- proves the probe code itself works.
                                       A UDP responder this script starts, on
                                       this machine. If tier 0 fails, every
                                       result below it is meaningless.
        tier 1  egress controls      -- third-party services on NON-53 UDP
                                       ports that answer deterministically
                                       (STUN/RFC 5389, NTP/RFC 5905). If tier 0
                                       passes and tier 1 fails, the network
                                       blocks arbitrary-port UDP, not the code.
        tier 2  subject              -- the ports tracker traffic actually uses.

    That is the "an absence is not a zero" rule of RULES 2, made concrete.

INPUTS (pinned)
    Control endpoints are pinned in CONTROLS below, by host and port. They are
    well-known public services chosen because they answer without registration.
    A control that stops answering is itself a result and must be replaced with
    a new numbered experiment, never edited in place.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run

USAGE
    ./01-host-network-baseline.py
    ./01-host-network-baseline.py --expect-udp-arbitrary   # assert tier 1 passes
    ./01-host-network-baseline.py --expect-ipv6-egress
"""

from __future__ import annotations

import argparse
import os
import socket
import struct
import sys
import threading

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _conditions as C  # noqa: E402

# --- pinned control endpoints -------------------------------------------------
# UDP services on non-53 ports that answer a well-formed request deterministically.
CONTROLS = [
    ("stun", "stun.l.google.com", 19302),
    ("stun", "stun.cloudflare.com", 3478),
    ("ntp", "pool.ntp.org", 123),
    ("ntp", "time.cloudflare.com", 123),
]
# UDP port 53 is separated from the controls above ON PURPOSE: a network that
# allows only DNS would pass a 53-based control and tell us nothing about 6969.
DNS_COMPARISON = [("dns", "1.1.1.1", 53), ("dns", "8.8.8.8", 53)]

# TCP ports worth knowing about: 80/443 carry HTTP(S) trackers on default ports,
# the rest are ports HTTP trackers were observed on in newTrackon's live list.
TCP_TARGETS = [
    ("github.com", 443),
    ("github.com", 80),
    ("bt1.archive.org", 6969),
    ("tracker.renfei.net", 8080),
    ("bt.okmp3.ru", 2710),
    ("tracker.bt4g.com", 2095),
]

IPV6_TARGETS = [("ipv6.google.com", 443), ("ipv6.icanhazip.com", 443)]

TIMEOUT = 5.0


# --- tier 0: loopback control -------------------------------------------------
class LoopbackUDPEcho:
    """A UDP responder on 127.0.0.1 that echoes what it receives.

    This is the probe-correctness control. It is deliberately NOT a tracker: it
    proves that this process can build, send, and receive a UDP datagram. If it
    fails, nothing else in this script may be interpreted.
    """

    def __init__(self):
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.sock.bind(("127.0.0.1", 0))
        self.port = self.sock.getsockname()[1]
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._serve, daemon=True)

    def _serve(self):
        self.sock.settimeout(0.25)
        while not self.stop.is_set():
            try:
                data, addr = self.sock.recvfrom(2048)
                self.sock.sendto(b"ECHO:" + data, addr)
            except socket.timeout:
                continue
            except OSError:
                break

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *a):
        self.stop.set()
        self.thread.join(timeout=2)
        self.sock.close()
        return False


def loopback_control() -> dict:
    payload = b"tier0-probe-correctness-control"
    with LoopbackUDPEcho() as echo:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.settimeout(TIMEOUT)
        try:
            with C.Timer() as t:
                s.sendto(payload, ("127.0.0.1", echo.port))
                data, _ = s.recvfrom(2048)
            ok = data == b"ECHO:" + payload
            return {
                "ok": ok,
                "rtt_ms": round(t.ms, 3),
                "detail": "echo matched" if ok else f"echo mismatch: {data!r}",
            }
        except Exception as e:
            return {"ok": False, "rtt_ms": None, "detail": f"{type(e).__name__}: {e}"}
        finally:
            s.close()


# --- tier 1/2 probes ----------------------------------------------------------
def stun_probe(host: str, port: int) -> dict:
    """RFC 5389 Binding Request. A conforming server echoes our transaction id."""
    txid = os.urandom(12)
    pkt = struct.pack(">HHI", 0x0001, 0, 0x2112A442) + txid
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.settimeout(TIMEOUT)
    try:
        with C.Timer() as t:
            s.sendto(pkt, (host, port))
            data, _ = s.recvfrom(2048)
        if len(data) < 20:
            return {"ok": False, "rtt_ms": round(t.ms, 3), "detail": f"short reply {len(data)}B"}
        if data[8:20] != txid:
            # A reply that does not echo our transaction id is not our reply.
            return {"ok": False, "rtt_ms": round(t.ms, 3), "detail": "transaction id mismatch"}
        return {"ok": True, "rtt_ms": round(t.ms, 3), "detail": f"{len(data)}B, txid matched"}
    except Exception as e:
        return {"ok": False, "rtt_ms": None, "detail": f"{type(e).__name__}: {e}"}
    finally:
        s.close()


def ntp_probe(host: str, port: int = 123) -> dict:
    """RFC 5905 client request (LI=0, VN=3, Mode=3). A server replies 48 bytes."""
    pkt = b"\x1b" + 47 * b"\0"
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.settimeout(TIMEOUT)
    try:
        with C.Timer() as t:
            s.sendto(pkt, (host, port))
            data, _ = s.recvfrom(512)
        mode = data[0] & 0b111 if data else -1
        ok = len(data) >= 48 and mode == 4  # mode 4 == server
        return {
            "ok": ok,
            "rtt_ms": round(t.ms, 3),
            "detail": f"{len(data)}B, mode={mode}" if data else "empty",
        }
    except Exception as e:
        return {"ok": False, "rtt_ms": None, "detail": f"{type(e).__name__}: {e}"}
    finally:
        s.close()


def dns_udp_probe(host: str, port: int = 53) -> dict:
    """A raw DNS/UDP query. Separated from the controls -- see DNS_COMPARISON."""
    txid = os.urandom(2)
    q = txid + b"\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x06google\x03com\x00\x00\x01\x00\x01"
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.settimeout(TIMEOUT)
    try:
        with C.Timer() as t:
            s.sendto(q, (host, port))
            data, _ = s.recvfrom(1024)
        ok = len(data) >= 12 and data[:2] == txid
        return {"ok": ok, "rtt_ms": round(t.ms, 3), "detail": f"{len(data)}B"}
    except Exception as e:
        return {"ok": False, "rtt_ms": None, "detail": f"{type(e).__name__}: {e}"}
    finally:
        s.close()


def tcp_probe(host: str, port: int, family=socket.AF_INET) -> dict:
    try:
        infos = socket.getaddrinfo(host, port, family, socket.SOCK_STREAM)
    except Exception as e:
        return {"ok": False, "rtt_ms": None, "detail": f"resolve: {type(e).__name__}: {e}"}
    addr = infos[0][4]
    s = socket.socket(family, socket.SOCK_STREAM)
    s.settimeout(TIMEOUT)
    try:
        with C.Timer() as t:
            s.connect(addr)
        return {"ok": True, "rtt_ms": round(t.ms, 3), "detail": f"connected {addr[0]}"}
    except Exception as e:
        return {"ok": False, "rtt_ms": None, "detail": f"{type(e).__name__}: {e}"}
    finally:
        s.close()


def _is_resolution_failure(row: dict) -> bool:
    """Did this probe fail before it ever opened a socket.

    `tcp_probe` prefixes exactly that case with `resolve:`, which is the only
    marker available: everything after resolution is a real connection attempt
    and its failure is a fact about the port.
    """
    return not row.get("ok") and str(row.get("detail", "")).startswith("resolve:")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--expect-udp-arbitrary", action="store_true",
                    help="exit 1 unless at least one tier-1 control answers")
    ap.add_argument("--expect-ipv6-egress", action="store_true",
                    help="exit 1 unless at least one IPv6 target connects")
    ap.add_argument("--out", default=None, help="path for the JSON result")
    args = ap.parse_args()

    results: dict = {"tier0_loopback_control": {}, "tier1_egress_controls": [],
                     "dns_comparison": [], "tier2_tcp": [], "ipv6": {}}

    # tier 0 -- if this fails, say so loudly and stop interpreting anything else.
    results["tier0_loopback_control"] = loopback_control()

    # tier 1 -- arbitrary-port UDP, third-party, deterministic replies.
    for kind, host, port in CONTROLS:
        fn = stun_probe if kind == "stun" else ntp_probe
        r = fn(host, port)
        r.update({"kind": kind, "host": host, "port": port})
        results["tier1_egress_controls"].append(r)

    # UDP/53 for comparison only. Passing here proves nothing about port 6969.
    for kind, host, port in DNS_COMPARISON:
        r = dns_udp_probe(host, port)
        r.update({"kind": kind, "host": host, "port": port})
        results["dns_comparison"].append(r)

    # tier 2 -- TCP on the ports HTTP trackers were actually observed on.
    for host, port in TCP_TARGETS:
        r = tcp_probe(host, port)
        r.update({"host": host, "port": port})
        results["tier2_tcp"].append(r)

    # IPv6: stack presence and egress are different facts and are reported apart.
    v6 = {"stack_present": C.has_ipv6_stack(), "targets": []}
    if v6["stack_present"]:
        for host, port in IPV6_TARGETS:
            r = tcp_probe(host, port, family=socket.AF_INET6)
            r.update({"host": host, "port": port})
            v6["targets"].append(r)
    results["ipv6"] = v6

    udp_arbitrary_ok = any(r["ok"] for r in results["tier1_egress_controls"])
    udp_53_ok = any(r["ok"] for r in results["dns_comparison"])
    ipv6_egress_ok = any(r["ok"] for r in v6["targets"])
    results["verdict"] = {
        "probe_code_works": results["tier0_loopback_control"]["ok"],
        "udp_arbitrary_port_egress": udp_arbitrary_ok,
        "udp_port_53_egress": udp_53_ok,
        "ipv6_stack_present": v6["stack_present"],
        "ipv6_egress": ipv6_egress_ok,
        "tcp_ports_open": sorted({r["port"] for r in results["tier2_tcp"] if r["ok"]}),
        # A PORT is blocked only where the connection itself failed. A target
        # whose hostname does not resolve says nothing about the port, and
        # counting it as blocked is the exact defect RULES 3.1 is about: a
        # failure of ours, or of a host that has since disappeared, reported as
        # a fact about the platform.
        #
        # ⛔ THIS HAS ALREADY PRODUCED A FALSE VERDICT. On 2026-08-31 both
        # runner images reported `tcp_ports_blocked: [2710]`, which reads as
        # "GitHub blocks the classic BitTorrent tracker port". The real cause
        # was that `bt.okmp3.ru` had stopped resolving, and 2710 was never
        # attempted. Experiment 04 independently recorded the same host as
        # NXDOMAIN in the same run.
        "tcp_ports_blocked": sorted({
            r["port"] for r in results["tier2_tcp"]
            if not r["ok"] and not _is_resolution_failure(r)}),
        # Reported separately rather than folded into either list, because
        # "we could not reach the host at all" is a third answer.
        "tcp_targets_unresolvable": sorted({
            r["host"] for r in results["tier2_tcp"] if _is_resolution_failure(r)}),
    }

    conditions = C.with_network_vantage(C.collect(sample_counts={
        "tier1_controls": len(CONTROLS),
        "dns_comparison": len(DNS_COMPARISON),
        "tcp_targets": len(TCP_TARGETS),
        "ipv6_targets": len(IPV6_TARGETS) if v6["stack_present"] else 0,
        "runs_per_target": 1,
    }))
    out = args.out or C.results_path(__file__)
    C.emit(
        "Which network egress does this host actually have -- which TCP ports, "
        "which UDP ports, and does IPv6 leave the machine?",
        conditions, results, out,
    )

    print("\nTIER 0  probe-correctness control (loopback UDP echo)")
    print(f"  {'PASS' if results['tier0_loopback_control']['ok'] else 'FAIL'}  "
          f"{results['tier0_loopback_control']['detail']}")
    if not results["tier0_loopback_control"]["ok"]:
        print("  !! Tier 0 failed. Every result below is uninterpretable: this")
        print("  !! script cannot send and receive a UDP datagram to itself.")

    print("\nTIER 1  arbitrary-port UDP egress controls (NOT port 53)")
    for r in results["tier1_egress_controls"]:
        print(f"  {'PASS' if r['ok'] else 'FAIL'}  {r['kind']:5s} {r['host']}:{r['port']:<6d} {r['detail']}")

    print("\n        UDP port 53, for comparison only")
    for r in results["dns_comparison"]:
        print(f"  {'PASS' if r['ok'] else 'FAIL'}  {r['kind']:5s} {r['host']}:{r['port']:<6d} {r['detail']}")

    print("\nTIER 2  TCP connect")
    for r in results["tier2_tcp"]:
        print(f"  {'PASS' if r['ok'] else 'FAIL'}  {r['host']}:{r['port']:<6d} {r['detail']}")

    print("\nIPv6")
    print(f"  stack present: {v6['stack_present']}")
    for r in v6["targets"]:
        print(f"  {'PASS' if r['ok'] else 'FAIL'}  {r['host']}:{r['port']:<6d} {r['detail']}")

    print("\nVERDICT")
    for k, v in results["verdict"].items():
        print(f"  {k}: {v}")
    print("\nINTERPRETATION RULE")
    if not results["tier0_loopback_control"]["ok"]:
        print("  Tier 0 failed -> this is a broken probe, not a network finding.")
    elif udp_arbitrary_ok:
        print("  Tier 0 passed and tier 1 passed -> arbitrary-port UDP egress EXISTS here.")
    elif udp_53_ok:
        print("  Tier 0 passed, tier 1 failed, UDP/53 passed -> UDP egress is")
        print("  PORT-FILTERED to 53. BEP 15 probing is impossible from this host,")
        print("  and this is a property of the host, not of any tracker.")
    else:
        print("  Tier 0 passed and all outbound UDP failed -> UDP egress is blocked entirely.")

    if args.expect_udp_arbitrary and not udp_arbitrary_ok:
        print("\nEXPECTATION FAILED: --expect-udp-arbitrary")
        return C.EXIT_MEASURED_AND_FAILED
    if args.expect_ipv6_egress and not ipv6_egress_ok:
        print("\nEXPECTATION FAILED: --expect-ipv6-egress")
        return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
