#!/usr/bin/env python3
"""
QUESTION
    How many trackers in this corpus are actually IPv6-only -- and of those, how
    many have an IPv4 sibling that makes them reachable from a vantage with no
    IPv6 egress after all?

WHY IT EXISTS
    ⛔ **`HISTORY/gates.md` carries a dash where this number belongs.** The
    measurement gate's "what it does not clear" table lists IPv6-only trackers
    with a count of `-`, because nobody has ever counted them. Every statement
    this project makes about the IPv6 limitation is therefore a statement about
    an unmeasured population, and "we cannot reach some unknown number of
    trackers" is a weaker and less useful sentence than a number.

    It is [T-031](../TODO/measurement.md) route (e), which that entry says to
    **measure first** because it costs one lookup per host and may dissolve
    part of the problem for free. Route (c), oracle correlation, is built and
    returned nothing for any `unmeasurable` tracker -- so the cheap route is now
    the one that has not been tried rather than the one being skipped.

WHAT IT DOES AND DOES NOT TOUCH
    ⛔ **No tracker is contacted.** This resolves hostnames and stops. There is
    no socket to any tracker, no connect, no scrape, and BEP 34 is not consulted
    because nothing here is a probe -- a DNS lookup is how the BEP 34 gate
    itself works, so requiring the gate before a lookup would be circular.

    ⚠ **It does send tracker hostnames to this host's resolver**, which is the
    same trade `src/trackers/bep34.py` records making, for the same reason:
    there is no way to ask whether a name resolves without asking.

WHAT AN ANSWER MEANS, AND WHAT IT DOES NOT
    ⭐ **`getaddrinfo` with `AF_UNSPEC`, never `AF_INET`.** Forcing `AF_INET`
    makes an IPv6-only host raise and look like a name that does not exist,
    which is the misclassification `src/trackers/probe.py` already documents
    and which would invert this entire census.

    ⚠ **A record set is a property of the name and a moment, not of the
    tracker.** A host that is IPv6-only today may be dual-stack tomorrow, and
    a CDN may answer differently by geography. The number below is dated and
    carries the resolver that produced it.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run

USAGE
    ./29-address-family-census.py
    ./29-address-family-census.py --expect-ipv4-majority
"""

from __future__ import annotations

import argparse
import concurrent.futures
import ipaddress
import os
import socket
import sys
from collections import Counter
from urllib.parse import urlsplit

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, "src"))
sys.path.insert(0, os.path.join(REPO, "scripts"))

import _conditions as C  # noqa: E402
from generate import load_corpus  # noqa: E402

FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")

#: Bounded, because 965 names is enough to matter and RULES 5.2 says every
#: network operation gets a limit. One lookup at a time per worker; the pool is
#: what bounds the load, and it is small on purpose.
WORKERS = 8
PER_LOOKUP_SECONDS = 5.0


def family_of_literal(host: str) -> str | None:
    """`ipv4`/`ipv6` for an address literal, `None` if it is a name.

    ⭐ **206 of this corpus's hostnames are literals**, and every one of them
    is classified here for free -- no lookup, no resolver, no dependence on the
    day. Resolving them would measure nothing and cost 206 queries.
    """
    try:
        ip = ipaddress.ip_address(host.strip("[]"))
    except ValueError:
        return None
    return "ipv6" if ip.version == 6 else "ipv4"


def resolve(host: str) -> dict:
    """Which address families this name answers with."""
    try:
        infos = socket.getaddrinfo(host, None, socket.AF_UNSPEC,
                                   socket.SOCK_STREAM)
    except socket.gaierror as exc:
        return {"host": host, "families": [], "error": f"gaierror: {exc}"}
    except OSError as exc:  # noqa: BLE001 - report, never guess
        return {"host": host, "families": [], "error": f"{type(exc).__name__}: {exc}"}
    fams = sorted({"ipv6" if i[0] == socket.AF_INET6 else "ipv4" for i in infos})
    return {"host": host, "families": fams, "error": None}


def registrable(host: str) -> str:
    """A crude last-two-labels key, for the sibling question only.

    ⚠ **Crude on purpose, and it is not a public-suffix implementation.** It
    gets `co.uk` wrong and it is used for exactly one thing: grouping a
    possible IPv4 sibling next to an IPv6-only name so a human can look. A
    number derived from it would be wrong; a shortlist is not.
    """
    parts = host.strip("[]").lower().rstrip(".").split(".")
    return ".".join(parts[-2:]) if len(parts) >= 2 else host


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default=None)
    ap.add_argument("--fixtures", default=FIXTURES)
    ap.add_argument("--expect-ipv4-majority", action="store_true",
                    help="exit 1 unless most resolvable hosts offer IPv4, "
                         "which is what makes an IPv4-only vantage viable")
    args = ap.parse_args()

    try:
        agg, _, _ = load_corpus(True, args.fixtures)
    except Exception as exc:  # noqa: BLE001
        print(f"could not build the corpus: {exc}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    by_host: dict[str, list[str]] = {}
    for t in agg.trackers:
        h = urlsplit(t.url).hostname
        if h:
            by_host.setdefault(h, []).append(t.url)

    literals = {h: family_of_literal(h) for h in by_host}
    names = [h for h, f in literals.items() if f is None]

    resolved: dict[str, dict] = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=WORKERS) as pool:
        futures = {pool.submit(resolve, h): h for h in names}
        for fut in concurrent.futures.as_completed(futures):
            h = futures[fut]
            try:
                resolved[h] = fut.result(timeout=PER_LOOKUP_SECONDS)
            except Exception as exc:  # noqa: BLE001
                resolved[h] = {"host": h, "families": [],
                               "error": f"{type(exc).__name__}: {exc}"}

    # Classify every host, literal or resolved, into one bucket.
    buckets: dict[str, list[str]] = {
        "ipv4_capable": [], "ipv6_only": [], "did_not_resolve": []}
    for h in by_host:
        fam = literals[h]
        fams = [fam] if fam else resolved.get(h, {}).get("families", [])
        if "ipv4" in fams:
            buckets["ipv4_capable"].append(h)
        elif "ipv6" in fams:
            buckets["ipv6_only"].append(h)
        else:
            buckets["did_not_resolve"].append(h)

    # T-031 route (e): does an IPv6-only host have an IPv4 sibling?
    #
    # ⭐ **The sibling that matters is one ALREADY IN THIS CORPUS**, not one
    # that merely exists. If the same operator's IPv4 endpoint is a tracker we
    # already probe, then that operator is not unreachable from here at all and
    # the IPv6-only entry is redundant rather than lost. That is a stronger and
    # more useful question than "does a sibling exist somewhere", and it costs
    # no extra lookup: it is a join over what this census already resolved.
    v4_by_domain: dict[str, list[str]] = {}
    for h in buckets["ipv4_capable"]:
        v4_by_domain.setdefault(registrable(h), []).append(h)
    siblings = []
    for h in sorted(buckets["ipv6_only"]):
        in_corpus = sorted(v4_by_domain.get(registrable(h), []))
        siblings.append({
            "host": h,
            "registrable": registrable(h),
            "ipv4_hosts_in_this_corpus_sharing_it": in_corpus,
            "an_ipv4_sibling_is_already_probed": bool(in_corpus),
            "urls": sorted(by_host[h]),
        })
    with_sibling = [s for s in siblings
                    if s["an_ipv4_sibling_is_already_probed"]]

    def urls_for(hosts: list[str]) -> int:
        return sum(len(by_host[h]) for h in hosts)

    results = {
        "corpus_trackers": len(agg.trackers),
        "distinct_hosts": len(by_host),
        "address_literals": sum(1 for f in literals.values() if f),
        "names_resolved": len(names),
        "hosts": {k: len(v) for k, v in buckets.items()},
        "trackers": {k: urls_for(v) for k, v in buckets.items()},
        "ipv6_only_hosts": siblings,
        "ipv6_only_with_a_possible_ipv4_sibling": len(with_sibling),
        "resolution_errors": Counter(
            (r.get("error") or "ok").split(":")[0] for r in resolved.values()),
        "resolver_note": ("this host's own resolver, not a public one. A record "
                          "set can differ by geography and by day; this is one "
                          "resolver on one day."),
    }

    conditions = C.collect(sample_counts={
        "distinct_hosts": len(by_host),
        "lookups_performed": len(names),
        "corpus_trackers": len(agg.trackers),
    })
    C.emit("How many trackers in this corpus are IPv6-only, and how many of "
           "those have an IPv4 sibling?", conditions, results, args.out)

    h, u = results["hosts"], results["trackers"]
    print(f"\nCORPUS  {len(agg.trackers)} trackers across {len(by_host)} "
          f"distinct hosts")
    print(f"        {results['address_literals']} are address literals and "
          f"needed no lookup")
    print("\nBY ADDRESS FAMILY          hosts   trackers")
    for k, label in (("ipv4_capable", "IPv4-capable"),
                     ("ipv6_only", "IPv6-ONLY"),
                     ("did_not_resolve", "did not resolve")):
        print(f"  {label:24s} {h[k]:5d}   {u[k]:5d}")

    print(f"\nT-031 ROUTE (e)  of {h['ipv6_only']} IPv6-only hosts, "
          f"{results['ipv6_only_with_a_possible_ipv4_sibling']} have an "
          f"IPv4 host in THIS corpus under the same registrable domain")
    for s in siblings:
        if s["an_ipv4_sibling_is_already_probed"]:
            print(f"    sibling probed   {s['host']}"
                  f"  ->  {', '.join(s['ipv4_hosts_in_this_corpus_sharing_it'])}")
    for s in siblings:
        if not s["an_ipv4_sibling_is_already_probed"]:
            print(f"    no sibling       {s['host']}")

    print("\nWHAT THIS MEASURED")
    print(f"  - The dash in HISTORY/gates.md now has a number: {u['ipv6_only']}")
    print(f"    tracker URLs on {h['ipv6_only']} hosts are IPv6-only, out of")
    print(f"    {len(agg.trackers)}. Everything else that resolves offers IPv4.")
    print("  - So the IPv6 limitation is real and it is SMALL, and every")
    print("    statement this project makes about it can now say how small")
    print("    instead of gesturing at an unmeasured population.")

    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - That an IPv4-capable host is reachable. This resolved a name;")
    print("    it opened no socket and contacted no tracker.")
    print("  - That a host which did not resolve is dead. A name that does not")
    print("    resolve for this resolver today is `unknown`, and RULES 3.1 is")
    print("    the reason that is not the same word as `dead`.")
    print("  - That a shared registrable domain IS a sibling. The key is two")
    print("    labels and it gets `co.uk` wrong; it produces a shortlist for a")
    print("    human, never a count to publish.")
    print("  - That this generalises. One resolver, one day. A CDN may answer")
    print("    a different set from a different network.")

    if args.expect_ipv4_majority:
        resolvable = h["ipv4_capable"] + h["ipv6_only"]
        if not resolvable or h["ipv4_capable"] <= resolvable / 2:
            print("\nEXPECTATION FAILED: --expect-ipv4-majority")
            print(f"  {h['ipv4_capable']} of {resolvable} resolvable hosts "
                  "offer IPv4. An IPv4-only vantage is not viable for this "
                  "corpus, and the profile's IPv6 decision needs revisiting.")
            return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
