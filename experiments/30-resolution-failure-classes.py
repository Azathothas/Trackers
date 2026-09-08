#!/usr/bin/env python3
"""
QUESTION
    351 corpus URLs did not resolve. Which kind of not-resolving is each one --
    a name that is gone, a nameserver that is broken, or a resolver of ours
    that could not answer?

WHY IT EXISTS
    T-036. `experiments/29` found that **26% of the corpus does not resolve**,
    fifteen times the IPv6-only population this project has spent far more
    words on, and put all of it in one bucket. The kinds have different
    consequences and only one of them is about the tracker:

        NXDOMAIN            the name does not exist. Evidence about the tracker.
        NOERROR, no answer  the name exists and has no address of that type.
        SERVFAIL / refused  somebody's nameserver is broken. About the PATH.
        no answer at all    our query did not get through. About US.

    ⛔ **The last two must never become `dead`.** RULES 3.1 is the rule and
    this is the same failure wearing a different hat: a resolver problem
    published as an operator's outage. `MIN_SAMPLES_FOR_DEATH` is 3 and nothing
    here writes a health state at all.

THE CONTROL, AND IT IS THE POINT OF THE EXPERIMENT
    ⭐ **Two resolvers, and they are not the same kind of thing.**

        the host's own      `socket.getaddrinfo`, which is what
                            `src/trackers/probe.py` uses and therefore what
                            decided the 351 in the first place.
        public recursive    `src/trackers/bep34.py`'s own DNS client against
                            1.1.1.1 / 8.8.8.8 / 9.9.9.9, generalised from TXT
                            to A and AAAA for this experiment.

    A name that fails for the first and answers for the second is **not a dead
    tracker**; it is our resolver, and that is exactly the failure newTrackon
    hit in production (issue #316) where opt-outs were silently missed because
    an internal resolver did not follow CNAMEs. Separating those two is the
    whole reason this experiment exists rather than a bigger version of 29.

    ⚠ It also feeds `T-007`: `experiments/04` measured resolver divergence at
    **0 of 17 names**, which is thin. This asks the same question over 244.

WHAT IT DOES NOT TOUCH
    ⛔ No tracker. It resolves names and stops -- no connect, no scrape, and no
    health state is written by anything here.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run

USAGE
    ./30-resolution-failure-classes.py
    ./30-resolution-failure-classes.py --limit 40      # a cheaper first look
    ./30-resolution-failure-classes.py --expect-no-mass-divergence
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
from trackers.bep34 import Resolver  # noqa: E402

FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")
WORKERS = 6

#: What a host is classified as. ⛔ None of these is `dead`, and the vocabulary
#: is deliberately about the LOOKUP rather than about the tracker.
CLASSES = (
    "resolves_for_both",
    "resolves_only_for_the_public_resolver",
    "resolves_only_for_this_host",
    "gone_nxdomain_confirmed",
    "no_address_records",
    "lookup_failed_undetermined",
)


def system_resolve(host: str) -> dict:
    """What `getaddrinfo` says -- the resolver that produced the 351."""
    try:
        infos = socket.getaddrinfo(host, None, socket.AF_UNSPEC,
                                   socket.SOCK_STREAM)
    except socket.gaierror as exc:
        return {"families": [], "error": f"gaierror {exc.errno}: {exc.strerror}"}
    except OSError as exc:  # noqa: BLE001
        return {"families": [], "error": f"{type(exc).__name__}: {exc}"}
    return {"families": sorted({"ipv6" if i[0] == socket.AF_INET6 else "ipv4"
                                for i in infos}), "error": None}


def classify(system: dict, public: dict) -> str:
    sys_ok = bool(system["families"])
    pub_ok = bool(public["families"])
    if sys_ok and pub_ok:
        return "resolves_for_both"
    if pub_ok:
        return "resolves_only_for_the_public_resolver"
    if sys_ok:
        return "resolves_only_for_this_host"
    if public["nxdomain"]:
        return "gone_nxdomain_confirmed"
    if not public["failures"]:
        # The public resolver answered NOERROR and offered no address.
        return "no_address_records"
    return "lookup_failed_undetermined"


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default=None)
    ap.add_argument("--fixtures", default=FIXTURES)
    ap.add_argument("--limit", type=int, default=None,
                    help="classify at most this many hosts, in corpus order")
    ap.add_argument("--expect-no-mass-divergence", action="store_true",
                    help="exit 1 if a large share of hosts answer for one "
                         "resolver and not the other, which would mean the "
                         "corpus's health records measured a resolver")
    args = ap.parse_args()

    try:
        agg, _, _ = load_corpus(True, args.fixtures)
    except Exception as exc:  # noqa: BLE001
        print(f"could not build the corpus: {exc}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    by_host: dict[str, list[str]] = {}
    for t in agg.trackers:
        h = urlsplit(t.url).hostname
        if not h:
            continue
        try:
            ipaddress.ip_address(h.strip("[]"))
            continue  # a literal needs no resolver and cannot diverge
        except ValueError:
            pass
        by_host.setdefault(h, []).append(t.url)

    # ⭐ Only the hosts the SYSTEM resolver could not answer for. Re-asking the
    # public resolver about the 700 that already resolve would triple this
    # project's DNS load to confirm something already known (RULES 15.2).
    names = sorted(by_host)
    if args.limit:
        names = names[:args.limit]

    shared = Resolver()

    def one(host: str) -> dict:
        system = system_resolve(host)
        if system["families"]:
            # Answered here; no second opinion needed and none is asked for.
            return {"host": host, "system": system,
                    "public": {"families": [], "addresses": {},
                               "failures": [], "nxdomain": False,
                               "asked": False},
                    "class": "resolves_for_both" if system["families"] else ""}
        public = shared.addresses(host)
        public["asked"] = True
        return {"host": host, "system": system, "public": public,
                "class": classify(system, public)}

    rows: list[dict] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=WORKERS) as pool:
        for row in pool.map(one, names):
            if row["class"] == "":
                row["class"] = "resolves_for_both"
            rows.append(row)

    counts = Counter(r["class"] for r in rows)
    urls = Counter()
    for r in rows:
        urls[r["class"]] += len(by_host[r["host"]])

    rescued = [r for r in rows
               if r["class"] == "resolves_only_for_the_public_resolver"]
    asked = [r for r in rows if r["public"].get("asked")]

    results = {
        "hosts_considered": len(names),
        "hosts_asked_a_second_resolver": len(asked),
        "classes_by_host": {k: counts.get(k, 0) for k in CLASSES},
        "classes_by_url": {k: urls.get(k, 0) for k in CLASSES},
        "rescued_by_the_public_resolver": [
            {"host": r["host"], "families": r["public"]["families"],
             "urls": sorted(by_host[r["host"]]),
             "system_said": r["system"]["error"]} for r in rescued],
        "system_errors": Counter(
            (r["system"]["error"] or "ok").split(":")[0] for r in rows),
        "resolvers": {
            "system": "socket.getaddrinfo on this host",
            "public": "trackers.bep34.Resolver -> 1.1.1.1, 8.8.8.8, 9.9.9.9"},
        "no_health_state_was_written": True,
    }

    conditions = C.collect(sample_counts={
        "hosts_considered": len(names),
        "second_opinions_asked": len(asked),
        "corpus_trackers": len(agg.trackers),
    })
    C.emit("Which kind of not-resolving is each unresolvable corpus host?",
           conditions, results, args.out)

    print(f"\nHOSTS  {len(names)} name-addressed hosts considered; "
          f"{len(asked)} needed a second opinion")
    print("\nCLASS                                     hosts   urls")
    for k in CLASSES:
        print(f"  {k:38s} {counts.get(k, 0):5d}  {urls.get(k, 0):5d}")

    print(f"\n⭐ RESCUED BY THE PUBLIC RESOLVER: {len(rescued)}")
    for r in rescued[:15]:
        print(f"    {r['host']:42s} {','.join(r['public']['families'])}")
    if rescued:
        print("    ⛔ These resolve perfectly well. This host's resolver could")
        print("       not answer for them, and every one would have been")
        print("       recorded `dns_failure` by a sweep from here.")

    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - That any of these trackers is dead. Nothing here wrote a health")
    print("    state and nothing contacted a tracker. `gone_nxdomain_confirmed`")
    print("    is the strongest class and it is still two resolvers on one day.")
    print("  - That the public resolvers are right and this host is wrong.")
    print("    They are a second opinion, not a reference standard.")
    print("  - That a name resolving means the tracker answers. That is a")
    print("    probe, and this is not one.")

    if args.expect_no_mass_divergence:
        if asked and len(rescued) > len(asked) / 2:
            print("\nEXPECTATION FAILED: --expect-no-mass-divergence")
            print(f"  {len(rescued)} of {len(asked)} hosts answered for the "
                  "public resolver and not for this one. The corpus's health "
                  "records measured a resolver as much as they measured "
                  "trackers, and T-007 is the entry that reopens.")
            return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
