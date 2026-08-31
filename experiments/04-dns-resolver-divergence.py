#!/usr/bin/env python3
"""
QUESTION
    Does this host's own resolver answer tracker hostnames the same way public
    resolvers do -- and where it does not, is the difference filtering, split
    horizon, or ordinary DNS churn?

WHY IT EXISTS
    TODO/RULES.md C-06 asks whether a runner's resolver filters or behaves
    differently from a consumer's. The consequence is precise and easy to get
    wrong: if the local resolver returns NXDOMAIN for a tracker that public
    resolvers resolve fine, then `dns_failure` in this project's health data
    means "our resolver has an opinion", not "the tracker is gone" -- and
    RULES 3.3 forbids reporting the second when we measured the first.

METHOD
    For each hostname, ask:
      - the host's own resolver, via getaddrinfo (what the probe would really use)
      - each pinned public resolver, by raw DNS/UDP query written here in stdlib

    Then classify each hostname:
      agree            both resolved, address sets intersect
      disjoint         both resolved, address sets do NOT intersect  (CDN or
                       geo-DNS; usually benign, and it is NOT filtering)
      local_only       local resolved, public did not
      public_only      public resolved, local did not   <-- the dangerous case
      both_failed      neither resolved
      no_public_answer public resolvers unreachable; comparison not possible

    `public_only` is the case that corrupts health data. It is counted and
    called out separately rather than folded into a "mismatch" percentage.

WHAT THIS CANNOT ESTABLISH
    That a public resolver is "right". DNS legitimately differs by vantage:
    geo-DNS, anycast and CDN fronting all produce disjoint address sets for a
    healthy name. Only the local_only / public_only asymmetries carry a signal,
    and even those can be a name in mid-propagation. Re-run before concluding.

INPUTS (pinned)
    Hostnames are derived from fixtures/probe-targets.tsv (captured 2026-08-29).
    Public resolvers are pinned by address below.

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
from urllib.parse import urlsplit

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _conditions as C  # noqa: E402

PUBLIC_RESOLVERS = [
    ("cloudflare", "1.1.1.1"),
    ("google", "8.8.8.8"),
    ("quad9", "9.9.9.9"),
]
TIMEOUT = 5.0

# DNS RCODEs worth naming: the difference between "no such name" and "the
# server refused to tell you" is the difference between a dead tracker and a
# filtering resolver.
RCODES = {0: "NOERROR", 1: "FORMERR", 2: "SERVFAIL", 3: "NXDOMAIN",
          4: "NOTIMP", 5: "REFUSED"}


def encode_qname(name: str) -> bytes:
    out = b""
    for label in name.rstrip(".").split("."):
        b = label.encode("idna") if any(ord(c) > 127 for c in label) else label.encode("ascii")
        out += bytes([len(b)]) + b
    return out + b"\x00"


def dns_query_a(resolver: str, name: str, timeout: float = TIMEOUT) -> dict:
    """A minimal DNS/UDP A query. stdlib only -- no dependency to rot."""
    txid = os.urandom(2)
    header = txid + struct.pack(">HHHHH", 0x0100, 1, 0, 0, 0)  # RD=1
    packet = header + encode_qname(name) + struct.pack(">HH", 1, 1)  # QTYPE=A QCLASS=IN
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.settimeout(timeout)
    try:
        with C.Timer() as t:
            s.sendto(packet, (resolver, 53))
            data, _ = s.recvfrom(4096)
    except Exception as e:
        return {"ok": False, "rcode": C.UNKNOWN, "addresses": [],
                "detail": f"{type(e).__name__}: {e}", "rtt_ms": None}
    finally:
        s.close()

    if len(data) < 12 or data[:2] != txid:
        return {"ok": False, "rcode": C.UNKNOWN, "addresses": [],
                "detail": "transaction id mismatch or short reply", "rtt_ms": round(t.ms, 3)}
    flags, qd, an = struct.unpack(">HHH", data[2:8])
    rcode = flags & 0x000F
    # Skip the question section.
    i = 12
    for _ in range(qd):
        while i < len(data) and data[i] != 0:
            if data[i] & 0xC0 == 0xC0:
                i += 2
                break
            i += data[i] + 1
        else:
            i += 1
        i += 4
    addresses = []
    for _ in range(an):
        if i + 12 > len(data):
            break
        if data[i] & 0xC0 == 0xC0:
            i += 2
        else:
            while i < len(data) and data[i] != 0:
                i += data[i] + 1
            i += 1
        rtype, _rclass, _ttl, rdlen = struct.unpack(">HHIH", data[i:i + 10])
        i += 10
        if rtype == 1 and rdlen == 4:
            addresses.append(socket.inet_ntoa(data[i:i + 4]))
        i += rdlen
    return {"ok": rcode == 0 and bool(addresses), "rcode": RCODES.get(rcode, str(rcode)),
            "addresses": sorted(addresses), "detail": f"{len(addresses)} A record(s)",
            "rtt_ms": round(t.ms, 3)}


def local_resolve(name: str) -> dict:
    """What the probe would actually use: the host's configured resolver."""
    try:
        with C.Timer() as t:
            infos = socket.getaddrinfo(name, None, socket.AF_INET, socket.SOCK_STREAM)
        addrs = sorted({i[4][0] for i in infos})
        return {"ok": bool(addrs), "addresses": addrs, "rcode": "NOERROR",
                "detail": f"{len(addrs)} address(es)", "rtt_ms": round(t.ms, 3)}
    except Exception as e:
        return {"ok": False, "addresses": [], "rcode": C.UNKNOWN,
                "detail": f"{type(e).__name__}: {e}", "rtt_ms": None}


def classify(local: dict, publics: list[dict]) -> str:
    answered = [p for p in publics if p["addresses"]]
    if not any(p["rcode"] != C.UNKNOWN for p in publics):
        return "no_public_answer"
    pub_addrs = set()
    for p in answered:
        pub_addrs |= set(p["addresses"])
    loc_addrs = set(local["addresses"])
    if loc_addrs and pub_addrs:
        return "agree" if loc_addrs & pub_addrs else "disjoint"
    if loc_addrs and not pub_addrs:
        return "local_only"
    if pub_addrs and not loc_addrs:
        return "public_only"
    return "both_failed"


def main() -> int:
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--targets", default=os.path.join(here, "fixtures", "probe-targets.tsv"))
    ap.add_argument("--expect-no-public-only", action="store_true",
                    help="exit 1 if any hostname resolves publicly but not locally")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    try:
        hostnames = []
        with open(args.targets, encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                parts = line.split("\t")
                if len(parts) < 2:
                    continue
                h = urlsplit(parts[1]).hostname
                if h and h not in hostnames:
                    hostnames.append(h)
    except OSError as e:
        print(f"could not read targets: {e}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    rows = []
    for name in hostnames:
        local = local_resolve(name)
        publics = []
        for label, addr in PUBLIC_RESOLVERS:
            r = dns_query_a(addr, name)
            r["resolver"] = f"{label} ({addr})"
            publics.append(r)
        rows.append({"hostname": name, "local": local, "public": publics,
                     "classification": classify(local, publics)})

    hist: dict[str, int] = {}
    for r in rows:
        hist[r["classification"]] = hist.get(r["classification"], 0) + 1
    public_only = [r["hostname"] for r in rows if r["classification"] == "public_only"]
    results = {"rows": rows, "classification_histogram": hist,
               "public_only_hostnames": public_only,
               "public_resolvers_reachable": any(
                   p["rcode"] != C.UNKNOWN for r in rows for p in r["public"])}

    conditions = C.collect(sample_counts={"hostnames": len(hostnames),
                                          "public_resolvers": len(PUBLIC_RESOLVERS),
                                          "queries_per_hostname": len(PUBLIC_RESOLVERS) + 1})
    out = args.out or C.results_path(__file__)
    C.emit("Does this host's resolver answer tracker hostnames the same way public "
           "resolvers do?", conditions, results, out)

    print(f"\nRESULTS  {len(hostnames)} hostnames x {len(PUBLIC_RESOLVERS)} public resolvers + local")
    for r in rows:
        loc = ",".join(r["local"]["addresses"]) or r["local"]["detail"]
        pub = ",".join(sorted({a for p in r["public"] for a in p["addresses"]})) or "-"
        print(f"  {r['classification']:<16s} {r['hostname']}")
        print(f"    local : {loc}")
        print(f"    public: {pub}")

    print("\nHISTOGRAM")
    for k, v in sorted(hist.items()):
        print(f"  {k}: {v}")
    if not results["public_resolvers_reachable"]:
        print("\n  !! No public resolver answered. UDP/53 egress is unavailable here, so")
        print("  !! this comparison could not run and NOTHING about C-06 follows from it.")
    if public_only:
        print("\n  !! public_only hostnames -- these resolve for the internet and NOT here.")
        print("  !! Any dns_failure this project records for them is a fact about our")
        print("  !! resolver, and they must not be reported dead:")
        for h in public_only:
            print(f"  !!   {h}")

    if args.expect_no_public_only and public_only:
        print("\nEXPECTATION FAILED: --expect-no-public-only")
        return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
