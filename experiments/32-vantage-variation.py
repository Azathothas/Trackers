#!/usr/bin/env python3
"""
QUESTION
    How much of what this project calls unreachable is a fact about the
    tracker, and how much is a fact about the address we asked from?

WHY IT EXISTS
    T-004, and it is the honesty problem the measurement gate could not clear:
    every number here comes from one cloud provider's address space, trackers
    commonly treat datacenter ranges differently from residential ones, and the
    dataset cannot tell "dead" from "dead from AS8075".

    ⛔ **D2 stands: this project does not operate a second measurement
    environment.** That closes one route and not the question (RULES 10.1a).
    Two routes that need no infrastructure are measured here:

      (d) **within-AS8075 variation.** This project has already measured from
          FIVE distinct public addresses without arranging to. If the same
          tracker answers one and refuses another, some of the bias is
          address-specific rather than range-specific -- and that is measurable
          today, from results already committed, for nothing.

      (a) **oracle correlation at scale.** newTrackon observes from a different
          vantage and publishes what it sees. Systematic disagreement across
          the corpus is the signal.

⛔ WHAT THIS CANNOT DO, AND SAYING SO IS THE POINT
    **It cannot measure residential reachability.** Nothing here has a
    residential address and no amount of arithmetic over datacenter addresses
    produces one. What it can do is put a floor under the question: if results
    already vary *within* one autonomous system, the variation *between* that
    system and a home connection is at least that large, and probably larger.

    ⚠ **A NULL RESULT HERE IS NOT REASSURANCE.** Five addresses in one provider
    agreeing tells you those five addresses are treated alike. It says nothing
    about whether all five are blocked together, which is the failure mode
    D2 accepted and this entry exists to keep visible.

INPUTS (pinned, and nothing here touches a network)
    Committed results only: every `02.*` and `05.*` run, each carrying the
    public address it went out from, and every `health-sweep.*` record. The
    newTrackon snapshots under `fixtures/source-cache/` are the oracle.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run

USAGE
    ./32-vantage-variation.py
    ./32-vantage-variation.py --expect-reported
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import sys
from collections import defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, "src"))
sys.path.insert(0, os.path.join(REPO, "scripts"))

import _conditions as C  # noqa: E402
from trackers.normalize import parse  # noqa: E402

RESULTS = os.path.join(HERE, "results")
SOURCE_CACHE = os.path.join(HERE, "fixtures", "source-cache")

#: ⛔ Travels with every disagreement figure below. RULES 1.4: a rate without
#: its conditions is the confident-wrongness failure this project exists to
#: avoid, and here the condition changes what the number means.
METHODOLOGY = (
    "newTrackon derives uptime by ANNOUNCING; this project stops at BEP 15 "
    "connect and HTTP scrape and has no announce code path at all (RULES 4). "
    "So a disagreement is a methodology difference before it is a vantage "
    "finding, and this instrument cannot separate the two (C-69)."
)


def load_probe_results() -> list[dict]:
    """Every committed run of the two instruments that probe pinned subjects."""
    out = []
    for path in sorted(glob.glob(os.path.join(RESULTS, "0[25].*.json"))):
        with open(path, encoding="utf-8") as fh:
            doc = json.load(fh)
        cond, res = doc.get("conditions", {}), doc.get("results", {})
        image = os.path.basename(path).split(".")[1]
        for s in res.get("subjects", []):
            url = s.get("url") or s.get("announce_url")
            if not url:
                continue
            verdict = s.get("verdict")
            if isinstance(verdict, dict):          # experiment 05's shape
                ok = bool(verdict.get("is_tracker"))
                rung = verdict.get("highest_rung", "-")
            else:                                   # experiment 02's shape
                ok = bool(s.get("ok"))
                rung = s.get("rung", "-")
            out.append({
                "url": url, "ok": ok, "rung": rung,
                "public_ipv4": cond.get("public_ipv4", C.UNKNOWN),
                "org": cond.get("public_ipv4_org", C.UNKNOWN),
                "image": image, "date": (cond.get("utc") or "")[:10],
                "file": os.path.basename(path),
            })
    return out


def read_urls(path: str) -> set[str]:
    if not os.path.exists(path):
        return set()
    out = set()
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                r = parse(line)
            except Exception:  # noqa: BLE001 - not our list to validate
                continue
            if getattr(r, "url", None):
                out.add(r.url)
    return out


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default=None)
    ap.add_argument("--expect-reported", action="store_true",
                    help="exit 1 if either route has no evidence to report")
    args = ap.parse_args()

    rows = load_probe_results()
    if not rows:
        print("no committed probe results to read", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    addresses = sorted({r["public_ipv4"] for r in rows} - {C.UNKNOWN})
    orgs = sorted({r["org"] for r in rows} - {C.UNKNOWN})

    # --- route (d): does the answer depend on which address asked? ----------
    #
    # ⭐ Compared WITHIN a date. Two runs a week apart differing is the tracker
    # changing, which is a different fact and is reported separately below;
    # only same-day disagreement isolates the address.
    by_url_day: dict[tuple, dict[str, set[bool]]] = defaultdict(lambda: defaultdict(set))
    for r in rows:
        by_url_day[(r["url"], r["date"])][r["public_ipv4"]].add(r["ok"])

    same_day_multi = []
    address_disagreements = []
    confounded_by_instability = []
    for (url, day), by_ip in sorted(by_url_day.items()):
        if len(by_ip) < 2:
            continue
        same_day_multi.append({"url": url, "date": day,
                               "addresses": sorted(by_ip)})
        verdicts = {ip: sorted(v) for ip, v in by_ip.items()}
        distinct = {tuple(v) for v in verdicts.values()}
        if len(distinct) == 1:
            continue
        # ⛔ THE CONTROL, AND WITHOUT IT THIS INSTRUMENT REPORTS AN ARTEFACT.
        # An address that gave BOTH answers on the same day did not disagree
        # with another address; it disagreed with itself. Counting that as a
        # vantage effect would be naming a culprit without a control that
        # isolates it (RULES 2), and it is the only "disagreement" in this
        # data -- so the distinction is the entire result rather than a detail.
        if any(len(v) > 1 for v in verdicts.values()):
            confounded_by_instability.append(
                {"url": url, "date": day, "by_address": verdicts,
                 "why": "at least one address returned both answers itself, so "
                        "the difference is instability at that address rather "
                        "than a difference between addresses"})
            continue
        address_disagreements.append(
            {"url": url, "date": day, "by_address": verdicts})

    # --- across time, same address, so the address is held constant ---------
    by_url_ip: dict[tuple, set[bool]] = defaultdict(set)
    for r in rows:
        by_url_ip[(r["url"], r["public_ipv4"])].add(r["ok"])
    unstable_over_time = sorted(
        {url for (url, _ip), v in by_url_ip.items() if len(v) > 1})

    # --- route (a): oracle correlation over everything we have measured -----
    oracle_live = read_urls(os.path.join(SOURCE_CACHE,
                                         "https___newtrackon.com_api_live"))
    oracle_all = read_urls(os.path.join(SOURCE_CACHE,
                                        "https___newtrackon.com_api_all"))
    agree_live = agree_not = we_live_they_not = they_live_we_not = 0
    for path in sorted(glob.glob(os.path.join(RESULTS, "health-sweep.*.json"))):
        with open(path, encoding="utf-8") as fh:
            doc = json.load(fh)
        for rec in doc.get("trackers", []):
            url = rec["url"]
            if url not in oracle_all:
                # ⛔ An absence is not a zero (RULES 2). A tracker newTrackon
                # has never assessed cannot agree or disagree with us.
                continue
            we = rec["health_state"] == "live"
            they = url in oracle_live
            if we and they:
                agree_live += 1
            elif we:
                we_live_they_not += 1
            elif they:
                they_live_we_not += 1
            else:
                agree_not += 1
    assessed = agree_live + agree_not + we_live_they_not + they_live_we_not
    disagreements = we_live_they_not + they_live_we_not

    results = {
        "methodology": METHODOLOGY,
        "route_d_within_as8075": {
            "distinct_public_addresses_measured_from": addresses,
            "autonomous_systems": orgs,
            "observations": len(rows),
            "subject_days_with_two_or_more_addresses": len(same_day_multi),
            "same_day_address_disagreements": address_disagreements,
            "confounded_by_single_address_instability": confounded_by_instability,
            "reading": ("a disagreement here is one address in AS8075 being "
                        "answered where another was not, on the same day. Zero "
                        "of them does NOT mean the range is unblocked -- it "
                        "means these addresses are treated alike, which is "
                        "exactly what a range-level block looks like."),
        },
        "stability_over_time": {
            "subjects_whose_answer_changed_at_one_address": unstable_over_time,
            "reading": ("the address is held constant, so this is the tracker "
                        "or the path changing, not the vantage"),
        },
        "route_a_oracle_correlation": {
            "assessed_by_both": assessed,
            "agree_live": agree_live,
            "agree_not_live": agree_not,
            "we_live_they_not": we_live_they_not,
            "they_live_we_not": they_live_we_not,
            "disagreement_rate": (disagreements / assessed) if assessed else None,
            "methodology": METHODOLOGY,
        },
        "what_this_cannot_measure": (
            "residential reachability. Nothing here has a residential address, "
            "and no arithmetic over datacenter addresses produces one."),
    }

    conditions = C.collect(sample_counts={
        "probe_observations": len(rows),
        "distinct_addresses": len(addresses),
        "oracle_assessed_by_both": assessed,
    })
    C.emit("How much of what this project calls unreachable is a fact about "
           "the address we asked from?", conditions, results, args.out)

    print("\n⛔ METHODOLOGY, and it applies to every disagreement figure below")
    print("  " + METHODOLOGY)

    print(f"\nROUTE (d)  WITHIN-AS8075 VARIATION")
    print(f"  measured from {len(addresses)} distinct public addresses, "
          f"{len(rows)} observations of pinned subjects")
    for a in addresses:
        print(f"    {a}")
    print(f"  autonomous systems: {', '.join(orgs) or '-'}")
    print(f"  subject-days seen from two or more addresses: {len(same_day_multi)}")
    print(f"  of those, answers that DIFFERED BY ADDRESS: "
          f"{len(address_disagreements)}")
    print(f"  discarded as instability at ONE address: "
          f"{len(confounded_by_instability)}")
    for d in confounded_by_instability[:5]:
        print(f"    {d['date']}  {d['url']}")
        for ip, v in sorted(d["by_address"].items()):
            print(f"        {ip:18s} ok={v}")
        print(f"        -> {d['why']}")
    for d in address_disagreements[:10]:
        print(f"    {d['date']}  {d['url']}")
        for ip, v in sorted(d["by_address"].items()):
            print(f"        {ip:18s} ok={v}")
    print(f"  ⚠ {results['route_d_within_as8075']['reading']}")

    print(f"\nSTABILITY OVER TIME, address held constant")
    print(f"  subjects whose answer changed at one address: "
          f"{len(unstable_over_time)}")
    for u in unstable_over_time[:8]:
        print(f"    {u}")

    print(f"\nROUTE (a)  ORACLE CORRELATION, over every tracker both sides assessed")
    if assessed:
        print(f"  assessed by both   {assessed}")
        print(f"  agree live         {agree_live}")
        print(f"  agree not-live     {agree_not}")
        print(f"  we live, they not  {we_live_they_not}")
        print(f"  they live, we not  {they_live_we_not}")
        print(f"  DISAGREEMENT RATE  {disagreements}/{assessed} = "
              f"{disagreements / assessed:.1%}")
    else:
        print("  no tracker was assessed by both sides")

    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - Residential reachability. Nothing here has a residential")
    print("    address, and this is the limitation D2 accepted rather than one")
    print("    it removed.")
    print("  - That agreement between our addresses means we are not blocked.")
    print("    Five addresses in one provider agreeing is what a RANGE-level")
    print("    block looks like from inside the range.")
    print("  - That an oracle disagreement is a vantage effect. newTrackon")
    print("    announces and we scrape, so the methodology difference and the")
    print("    vantage difference are confounded and this cannot separate them.")

    if args.expect_reported:
        problems = []
        if len(addresses) < 2:
            problems.append("fewer than two distinct addresses: route (d) has "
                            "nothing to compare")
        if not assessed:
            problems.append("no tracker assessed by both sides: route (a) has "
                            "no evidence")
        if problems:
            print("\nEXPECTATION FAILED: --expect-reported")
            for p_ in problems:
                print(f"  {p_}")
            return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
