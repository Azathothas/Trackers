#!/usr/bin/env python3
"""
QUESTION
    Where do this project's measurements and newTrackon's published uptime
    disagree -- and what does an independent observer say about the trackers
    this vantage cannot measure at all?

WHY IT EXISTS
    T-028. ⭐ **Disagreement between independent observers is the single most
    valuable thing this dataset could publish**, and no upstream in the corpus
    publishes it. Everyone else ships a list; nobody ships "these two monitors
    looked at the same tracker and came to different conclusions, and here is
    what each of them actually did."

    It is also T-031 route (c). Four categories of tracker are labelled
    `unmeasurable` from here -- IPv6-only, i2p, yggdrasil, `wss` -- and
    `unmeasurable` is the honest label on our data, never a reason to stop
    trying. An observer with different reachability having an opinion about one
    of them is evidence, and this reports it.

⛔ THE CAVEAT IS NOT A FOOTNOTE, IT IS THE HEADLINE
    **newTrackon derives uptime by ANNOUNCING** (`scraper.py:232`, `:279`,
    `thash=urandom(20)`). **This project stops at connect and scrape**, and
    RULES 4 forbids the announce path existing at all. So "uptime" there and
    "live" here are not the same measurement of the same thing, and a
    disagreement is a **methodology difference first and a finding second**
    (`C-69`). Every rate below carries that sentence with it.

    ⚠ **And it reports ONE PREFERRED PROTOCOL PER TRACKER** (newTrackon issue
    #324). So `/api/udp` is not "this tracker supports UDP" and must never be
    compared against a per-endpoint measurement. This script compares
    membership of the uptime sets only, and touches none of the
    protocol-partitioned routes.

WHAT THE ORACLE'S SETS ACTUALLY ARE, MEASURED RATHER THAN ASSUMED
    `experiments/20` established `/api/<int:percentage>` as a real uptime
    filter, monotone 261 -> 82 -> 55 -> 15, and `/api/stable` as
    `api_percentage(95, added_before=10 days)`.

    ⛔ **`stable` is NOT a subset of `live`**, measured here on the committed
    snapshots. They answer different questions -- historical uptime over a
    window against current responsiveness -- so treating the three sets as a
    ladder is a mistake, and this script asserts the nesting rather than
    assuming it. A tracker with 95% uptime that is down right now is in one and
    not the other, and that is the oracle being correct, not inconsistent.

INPUTS (pinned)
    * the committed newTrackon snapshots under `fixtures/source-cache/`, which
      is the default. RULES 15.3: prefer the check that reads our own snapshot
      over the one that opens a socket.
    * `--fetch` refreshes them from newTrackon. One request per route, no
      retries, and it touches no tracker at any point.
    * the committed health records under `results/`.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run

USAGE
    ./28-newtrackon-crosscheck.py                     # offline, from the cache
    ./28-newtrackon-crosscheck.py --expect-crosscheck # exit 1 if it cannot compare
    ./28-newtrackon-crosscheck.py --fetch             # refresh the snapshots
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import sys
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(REPO, "src"))
sys.path.insert(0, os.path.join(REPO, "scripts"))

import _conditions as C  # noqa: E402
from generate import load_corpus  # noqa: E402
from trackers.normalize import parse  # noqa: E402
from trackers.secondhand import Observation, Signal  # noqa: E402

FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")
SOURCE_CACHE = os.path.join(HERE, "fixtures", "source-cache")
RESULTS = os.path.join(HERE, "results")

#: The oracle's uptime sets. Membership is the second-hand observation; the
#: percentage-partitioned routes are deliberately not used, because a coarse
#: bucket we can defend beats a precise number we would be inventing.
ROUTES = {
    "all": "https://newtrackon.com/api/all",
    "live": "https://newtrackon.com/api/live",
    "stable": "https://newtrackon.com/api/stable",
}

#: ⛔ Travels with every rate this script emits. RULES 1.4: a number without
#: its conditions is the confident-wrongness failure.
METHODOLOGY = (
    "newTrackon ANNOUNCES to derive uptime; this project stops at connect and "
    "scrape and has no announce code path. A disagreement is a methodology "
    "difference before it is a finding (C-69). newTrackon also reports one "
    "preferred protocol per tracker (issue #324), so its protocol routes are "
    "not per-endpoint and are not read here."
)

#: The observer sits somewhere else, so its answer about a tracker we could not
#: reach is evidence about the tracker rather than about our vantage. It is
#: SECOND-HAND and is never promoted to a probe result (T-031's decision).
SECOND_HAND = (
    "second-hand: observed by newTrackon, not by this project. Recorded with "
    "its source and the snapshot date, and never merged with a direct probe."
)

USER_AGENT = "trackers/0.1 (+https://github.com/Azathothas/Trackers; oracle cross-check)"


def cache_path(url: str) -> str:
    """The committed snapshot for a route, named as `experiments/19` names its own."""
    safe = url.replace("://", "___").replace("/", "_").replace(":", "_")
    return os.path.join(SOURCE_CACHE, safe)


def fetch(url: str, timeout: float = 20.0) -> tuple[str | None, str]:
    """One request, no retries, bounded. Returns (body, detail).

    ⛔ A failed fetch returns `None`, never an empty string. RULES 3.2 is about
    sources and this is a source: "it answered with nothing" and "it did not
    answer" must not be written as one another, or a newTrackon outage would
    read as newTrackon reporting every tracker down.
    """
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            if resp.status != 200:
                return None, f"HTTP {resp.status}"
            # Bounded read: an unbounded one is a decompression bomb away from
            # consuming the runner (RULES 5.2).
            return resp.read(4 * 1024 * 1024).decode("utf-8", "replace"), "ok"
    except (urllib.error.URLError, OSError, ValueError) as exc:
        return None, f"{type(exc).__name__}: {exc}"


def parse_list(body: str) -> set[str]:
    """Through the PRODUCTION parser, so both sides name a tracker the same way."""
    out: set[str] = set()
    for line in body.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        try:
            result = parse(line)
        except Exception:  # noqa: BLE001 - not our list to validate
            continue
        url = getattr(result, "url", None)
        if url:
            out.add(url)
    return out


def load_oracle(do_fetch: bool) -> tuple[dict[str, set[str]], dict[str, str]]:
    sets: dict[str, set[str]] = {}
    detail: dict[str, str] = {}
    for name, url in ROUTES.items():
        path = cache_path(url)
        if do_fetch:
            body, why = fetch(url)
            detail[name] = f"fetched: {why}"
            if body is None:
                # The snapshot on disk is still evidence; the failure is
                # recorded rather than substituted for.
                detail[name] += " (kept the committed snapshot)"
                body = _read(path)
            else:
                os.makedirs(os.path.dirname(path), exist_ok=True)
                with open(path, "w", encoding="utf-8", newline="\n") as fh:
                    fh.write(body)
        else:
            body = _read(path)
            detail[name] = "committed snapshot" if body is not None else "missing"
        sets[name] = parse_list(body) if body is not None else set()
    return sets, detail


def _read(path: str) -> str | None:
    if not os.path.exists(path):
        return None
    with open(path, encoding="utf-8", errors="replace") as fh:
        return fh.read()


def load_records(paths: list[str]) -> dict[tuple, dict]:
    """Committed health records, grouped by vantage. Never merged (RULES 15.4)."""
    groups: dict[tuple, dict] = {}
    for path in sorted(paths):
        with open(path, encoding="utf-8") as fh:
            doc = json.load(fh)
        v = doc.get("vantage", {})
        runner = (v.get("runner") or {})
        key = (v.get("environment_class", C.UNKNOWN),
               v.get("execution_profile", C.UNKNOWN),
               str(runner.get("run_id", C.UNKNOWN)))
        g = groups.setdefault(key, {"key": key, "by_url": {},
                                    "generated_at": doc.get("generated_at",
                                                            C.UNKNOWN)})
        for rec in doc.get("trackers", []):
            g["by_url"][rec["url"]] = rec
    return groups


def crosscheck(records: dict, oracle: set[str], oracle_all: set[str]) -> dict:
    """The 2x2 over trackers BOTH sides have looked at.

    ⭐ The denominator is `oracle_all`, not the whole corpus. A tracker
    newTrackon has never heard of is not a disagreement -- it is an absence, and
    RULES 2 says an absence is not a zero. Counting it as "they say down" would
    manufacture agreement or disagreement out of a tracker nobody assessed.
    """
    both_live = we_live_they_not = they_live_we_not = neither = 0
    disagreements: list[dict] = []
    for url, rec in sorted(records.items()):
        if url not in oracle_all:
            continue
        we_live = rec["health_state"] == "live"
        they_live = url in oracle
        if we_live and they_live:
            both_live += 1
        elif we_live and not they_live:
            we_live_they_not += 1
            disagreements.append({"url": url, "we": "live", "they": "not listed",
                                  "our_rung": rec.get("measurement_rung")})
        elif they_live and not we_live:
            they_live_we_not += 1
            disagreements.append({"url": url, "we": rec["health_state"],
                                  "they": "live",
                                  "our_failure": rec.get("failure"),
                                  "our_rung": rec.get("measurement_rung")})
        else:
            neither += 1
    n = both_live + we_live_they_not + they_live_we_not + neither
    agree = both_live + neither
    return {
        "assessed_by_both": n,
        "agree_live": both_live,
        "agree_not_live": neither,
        "we_live_they_not": we_live_they_not,
        "they_live_we_not": they_live_we_not,
        "agreement_rate": (agree / n) if n else None,
        "methodology": METHODOLOGY,
        "disagreements": disagreements,
    }


def indirect_liveness(records: dict, oracle: set[str],
                      observed_at: str) -> dict:
    """T-031 route (c): what the observer says about what we could not measure.

    ⛔ SECOND-HAND, AND NEVER PROMOTED. These trackers stay `unmeasurable` or
    `unknown` in our data. What this adds is a separate, sourced observation
    beside that label -- which is the difference between "we do not know" and
    "we do not know, and somebody who can see further says it is up."
    """
    out: dict[str, list[dict]] = {"unmeasurable": [], "unknown": []}
    for url, rec in sorted(records.items()):
        state = rec["health_state"]
        if state not in out or url not in oracle:
            continue
        # ⭐ Through `secondhand.Observation` rather than a dict written here.
        # This experiment predates that module and hand-rolled the shape, which
        # is the same concept in two places: the module refuses the keys a
        # probe owns and cannot be given `direct=True`, and a dict can carry
        # anything. Found by the door sweep of 2026-09-08.
        observation = Observation(
            url=url, observer="newtrackon", observed_at=observed_at,
            method=("announces to the tracker and derives uptime; this "
                    "project scrapes and never announces (C-69)"),
            signal=Signal.ALIVE,
            reach=("an observer measuring from its own vantage, which reaches "
                   "what this one could not"))
        record = observation.as_record()
        record.update({"our_state": state, "our_failure": rec.get("failure"),
                       "network": rec.get("network"),
                       "transport": rec.get("transport")})
        out[state].append(record)
    return {
        "unmeasurable_with_a_second_hand_signal": len(out["unmeasurable"]),
        "unknown_with_a_second_hand_signal": len(out["unknown"]),
        "records": out,
        "note": SECOND_HAND,
    }


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--fetch", action="store_true",
                    help="refresh the snapshots from newTrackon. One request "
                         "per route; touches no tracker.")
    ap.add_argument("--records", nargs="*", default=None)
    ap.add_argument("--out", default=None)
    ap.add_argument("--expect-crosscheck", action="store_true",
                    help="exit 1 if the oracle or the records are unusable")
    args = ap.parse_args()

    sets, detail = load_oracle(args.fetch)
    if not sets.get("all"):
        print("no oracle snapshot: nothing to compare against.", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    paths = args.records or sorted(
        glob.glob(os.path.join(RESULTS, "health-sweep.*.json")))
    if not paths:
        print("no committed health records.", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    try:
        agg, _, _ = load_corpus(True, FIXTURES)
    except Exception as exc:  # noqa: BLE001
        print(f"could not build the corpus: {exc}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN
    ours = {t.url for t in agg.trackers}

    # The oracle's own structure, asserted rather than assumed.
    shape = {
        "sizes": {k: len(v) for k, v in sets.items()},
        "live_subset_of_all": sets["live"] <= sets["all"],
        "stable_subset_of_live": sets["stable"] <= sets["live"],
        "stable_subset_of_all": sets["stable"] <= sets["all"],
        "stable_not_live": sorted(sets["stable"] - sets["live"]),
        "in_our_corpus": {k: len(v & ours) for k, v in sets.items()},
        "reading": ("`live` is current responsiveness and `stable` is >=95% "
                    "uptime over a window with a 10-day age floor. They are "
                    "not a ladder: a tracker can be stable and down right now, "
                    "so `stable - live` is expected to be non-empty and its "
                    "size is a fact about the oracle, not a defect."),
    }

    groups = load_records(paths)
    analyses = []
    for key in sorted(groups):
        g = groups[key]
        rec = g["by_url"]
        analyses.append({
            "vantage": {"environment_class": key[0], "execution_profile": key[1],
                        "run_id": key[2], "generated_at": g["generated_at"]},
            "records": len(rec),
            # ⭐ `vs_live` is the comparison. `vs_stable` is kept because it is
            # informative, and labelled because it is NOT an agreement rate:
            # our `live` is one observation of current responsiveness and their
            # `stable` is >=95% uptime over a window with a 10-day age floor.
            # A tracker that is up today and was down last week is a true
            # `live` and a true not-`stable`, and counting that as a
            # disagreement would be comparing two different questions and
            # calling the difference an error.
            "vs_live": crosscheck(rec, sets["live"], sets["all"]),
            "vs_stable": dict(crosscheck(rec, sets["stable"], sets["all"]),
                              not_an_agreement_rate=(
                                  "our `live` is current responsiveness; their "
                                  "`stable` is >=95% historical uptime with a "
                                  "10-day age floor. Read this row as how many "
                                  "of the trackers we reached also clear a "
                                  "historical bar, never as error.")),
            # ⚠ A committed snapshot carries no capture date, so the honest
            # `observed_at` for one is a dash (RULES 1.5). `--fetch` knows when
            # it asked, and that is the closest thing to when the observer saw
            # it that exists.
            "indirect_liveness_t031": indirect_liveness(
                rec, sets["live"], C.utc() if args.fetch else "-"),
        })

    results = {
        "methodology": METHODOLOGY,
        "routes": ROUTES,
        "snapshot_detail": detail,
        "oracle_shape": shape,
        "our_corpus": len(ours),
        "analyses": analyses,
    }

    conditions = C.collect(sample_counts={
        "oracle_all": len(sets["all"]), "oracle_live": len(sets["live"]),
        "oracle_stable": len(sets["stable"]), "corpus": len(ours),
        "health_record_files": len(paths),
    })
    C.emit("Where do this project and newTrackon disagree, and what does the "
           "oracle say about what this vantage cannot measure?",
           conditions, results, args.out)

    print("\n⛔ METHODOLOGY, and it applies to every number below")
    print("  " + METHODOLOGY)

    print(f"\nORACLE  all {len(sets['all'])}, live {len(sets['live'])}, "
          f"stable {len(sets['stable'])}")
    for name in ROUTES:
        print(f"  {name:7s} {detail[name]}; {len(sets[name] & ours)} of them "
              f"are in our corpus")
    print(f"  live subset of all   : {shape['live_subset_of_all']}")
    print(f"  stable subset of live: {shape['stable_subset_of_live']}"
          f"  <- {len(shape['stable_not_live'])} stable trackers are not live")
    print("  " + shape["reading"])

    for a in analyses:
        v = a["vantage"]
        print(f"\nRUN {v['run_id']}  {v['environment_class']} / "
              f"{v['execution_profile']}  ({a['records']} records)")
        for label, x in (("vs live", a["vs_live"]), ("vs stable", a["vs_stable"])):
            if not x["assessed_by_both"]:
                print(f"  {label:10s} no tracker assessed by both")
                continue
            rate = ("agreement" if "not_an_agreement_rate" not in x
                    else "overlap (NOT agreement)")
            print(f"  {label:10s} assessed by both {x['assessed_by_both']}; "
                  f"agree-live {x['agree_live']}, agree-not {x['agree_not_live']}, "
                  f"we-live-they-not {x['we_live_they_not']}, "
                  f"they-live-we-not {x['they_live_we_not']}; "
                  f"{rate} {x['agreement_rate']:.1%}")
            if "not_an_agreement_rate" in x:
                print(f"             {x['not_an_agreement_rate']}")
        t = a["indirect_liveness_t031"]
        print(f"  T-031     second-hand liveness for "
              f"{t['unmeasurable_with_a_second_hand_signal']} `unmeasurable` and "
              f"{t['unknown_with_a_second_hand_signal']} `unknown` trackers")
        for r in (t["records"]["unmeasurable"] + t["records"]["unknown"])[:5]:
            print(f"      {r['our_state']:12s} {r['our_failure'] or '-':22s} "
                  f"{r['url']}")

    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - That either side is right. The two measure different things by")
    print("    different means, and neither is a reference standard.")
    print("  - That a tracker newTrackon has never heard of is down. It is an")
    print("    absence, and the denominator above excludes it for that reason.")
    print("  - That a second-hand signal is a measurement. It is recorded with")
    print("    its source and its date and is never merged with a probe result.")
    print("  - That the snapshots are current unless --fetch was passed. A")
    print("    committed snapshot is reproducible, not fresh.")

    if args.expect_crosscheck:
        problems = []
        if not shape["live_subset_of_all"]:
            problems.append("`live` is not a subset of `all`: the oracle's "
                            "own sets changed shape and the reading above no "
                            "longer holds")
        for a in analyses:
            if not a["vs_live"]["assessed_by_both"]:
                problems.append(f"run {a['vantage']['run_id']}: no tracker was "
                                "assessed by both sides")
        if problems:
            print("\nEXPECTATION FAILED: --expect-crosscheck")
            for p_ in problems:
                print(f"  {p_}")
            return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
