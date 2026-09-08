#!/usr/bin/env python3
"""
QUESTION
    Does this project's dataset add measurable value over redistributing
    `ngosang/trackerslist`, or is it a bigger list of the same trackers with
    more dead ones in it?

WHY IT EXISTS
    HISTORY/gates.md carries two gates that can legitimately end this project.
    The measurement gate passed. This is the instrument for the other one, the
    value gate, and T-027 is the entry. The gate asks three questions and this
    answers all three with sample counts:

        1. trackers present here and absent there, THAT ARE ALIVE;
        2. trackers present there and dead by measurement here;
        3. health disagreements, and which side the evidence supports.

    ⛔ A NEGATIVE ANSWER IS A SUCCESSFUL OUTCOME. If the delta is negligible
    the correct response is to say so in the README and recommend shipping a
    well-documented mirror, or nothing. This script does not get to pick the
    flattering metric: the decision rule is stated in DECISION_RULE below, in
    one place, and the numbers are compared against it afterwards.

WHAT MAKES THE COMPARISON VALID, AND WHAT IT DOES NOT SURVIVE
    ⭐ Both arms were probed IN THE SAME RUN, by the same code, from the same
    vantage, within the same fifteen minutes. So while neither arm's rate
    generalises to your connection, the DIFFERENCE between them is not an
    artefact of measuring the two at different times or from different places.
    That is the whole reason a comparison is possible from one thin sweep.

    ⛔ `live` IS A FLOOR, NOT A RATE. A tracker that timed out is `unknown`,
    and some of those are alive. Every "live" figure here understates both arms
    and is stated as a floor. It is not corrected for, because a correction
    would be a guess and RULES 1.5 says write a dash instead.

    ⛔ NOTHING CAN BE `dead`. `MIN_SAMPLES_FOR_DEATH` is 3 and one sweep is one
    sample, so question 2's literal answer is structurally zero and says
    nothing about any tracker. What is reported instead is `not_live`, which is
    a different and weaker claim, labelled as such.

INPUTS (pinned, and nothing here touches a network)
    * the corpus, built by the PRODUCTION path (`scripts/generate.py`'s
      `load_corpus`) from the committed fixtures under
      `tests/fixtures/sources/`. Not a second parser: a value gate that
      measured a corpus the pipeline does not publish would answer about
      nothing.
    * `ngosang_all` from the same fixtures, through the same
      `normalize.parse`, so an unported URL and a ported one are one tracker
      on both sides.
    * the committed health records under `results/`, which are the only
      liveness evidence this project owns.
    * two independent observers' own liveness assertions, as committed
      snapshots: `ngosang/trackers_all.txt` (published as a working list) and
      `newtrackon`'s `/api/live`. Question 3 needs a second opinion and these
      are the two in the tree.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run

USAGE
    ./27-value-gate.py                     # answer the gate, print the report
    ./27-value-gate.py --expect-answered   # exit 1 if any question is unanswerable
    ./27-value-gate.py --out PATH          # write the machine-readable result
"""

from __future__ import annotations

import argparse
import glob
import json
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(REPO, "src"))
sys.path.insert(0, os.path.join(REPO, "scripts"))

import _conditions as C  # noqa: E402
from generate import load_corpus  # noqa: E402
from trackers.exclusion import mask_credential  # noqa: E402
from trackers.normalize import parse  # noqa: E402

FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")
SOURCE_CACHE = os.path.join(HERE, "fixtures", "source-cache")
RESULTS = os.path.join(HERE, "results")

#: The list a consumer would redistribute instead of using this project. The
#: gate names it, so it is not a parameter.
BASELINE = "ngosang_all"

#: The closest prior art: a concatenation of the same upstreams, published as a
#: dataset. `HISTORY/corpus-baseline.md` measures its entire independent
#: content at one URL, and says that figure belongs in this argument.
PRIOR_ART = os.path.join(
    SOURCE_CACHE,
    "https___raw.githubusercontent.com_pkgforge-security_Trackers_main_trackers_all.txt")

#: Second observers, as their own published liveness assertions. Both are
#: committed snapshots and neither is a measurement WE took.
OBSERVERS = {
    "ngosang_all": os.path.join(FIXTURES, "ngosang_all.txt"),
    "newtrackon_live": os.path.join(
        SOURCE_CACHE, "https___newtrackon.com_api_live"),
}

#: ⛔ TWO BARS, AND THE SECOND IS THE ONE THAT CAN EMBARRASS US.
#:
#: An earlier draft of this file used one bar -- our interval's LOWER bound
#: against the baseline's POINT estimate -- and it is recorded here because it
#: is the exact shape of the mistake this gate exists to prevent. That
#: comparison is asymmetric in our favour: it charges us our sampling error and
#: forgives the baseline's. Under it the verdict was "not negligible" at 67 vs
#: 52; under the fair comparison of our worst case against the baseline's best
#: (67 vs 73) the same evidence reads very differently. A rule that picks the
#: flattering end of two intervals is manufacturing a difference, which is what
#: `HISTORY/gates.md` names as the failure mode in so many words.
#:
#: So both comparisons are made at their least flattering, and the verdict is
#: three-valued rather than a pass/fail this project grades itself on.
DECISION_RULE = (
    "BAR 1 -- COUNT. Compare the live trackers we ADD, at the pessimistic end "
    "of our interval, against the baseline's WHOLE live yield at the "
    "optimistic end of its interval. Worst case against best case, never the "
    "reverse. The consumer's alternative is to take all of the baseline, so "
    "the baseline's whole list is what we must beat. "
    "BAR 2 -- DENSITY. Compare the share of each list that answered us. If "
    "ours is less live-dense than the baseline, then a consumer taking our "
    "list UNFILTERED gets a worse list, and every bit of the added value is in "
    "the LABELS rather than in the URLs. "
    "A project that clears bar 1 and fails bar 2 is justified only as a "
    "labelled dataset and is NOT justified as a longer list -- which is the "
    "prior art, and is measured here beside it."
)

Z95 = 1.959963984540054


# --------------------------------------------------------------------------
# statistics, standard library only (D1)
# --------------------------------------------------------------------------

def wilson(k: int, n: int, z: float = Z95) -> tuple[float, float]:
    """95% Wilson score interval for a proportion.

    Wilson and not normal-approximation: at 16/183 the normal interval is
    visibly wrong near the boundary and this project publishes the interval,
    not just the point estimate.
    """
    if n == 0:
        return (0.0, 1.0)
    p = k / n
    denom = 1.0 + z * z / n
    centre = (p + z * z / (2 * n)) / denom
    half = (z / denom) * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return (max(0.0, centre - half), min(1.0, centre + half))


def binom_two_sided_p(k: int, n: int, p: float) -> float:
    """Exact two-sided binomial p-value, by the method of small likelihoods.

    Used as a CONTROL, not as a finding: the sample is a stride over a sorted
    corpus (`sweep.select`), which is systematic rather than random, so
    extrapolating from it assumes membership of the baseline list is not
    correlated with position in `Tracker.sort_key` order. This is the check of
    that assumption. A small p would mean the extrapolation below is invalid.
    """
    if n == 0:
        return 1.0

    def pmf(i: int) -> float:
        return math.comb(n, i) * (p ** i) * ((1 - p) ** (n - i))

    observed = pmf(k)
    # Floating point: an equally-likely outcome must not be excluded by a
    # last-bit difference, which would understate the p-value.
    tol = observed * 1e-9
    return min(1.0, sum(pmf(i) for i in range(n + 1) if pmf(i) <= observed + tol))


# --------------------------------------------------------------------------
# inputs
# --------------------------------------------------------------------------

def read_urls(path: str) -> tuple[set[str], list[str]]:
    """Normalize a list file through the PRODUCTION parser.

    One parser, so `http://x/announce` here and `http://x:80/announce` there
    are one tracker rather than two. A second parser written for this script
    would answer a question about itself.

    ⭐ Returns the rejects too. `parse` RAISES on a line that is not a URI, and
    swallowing that would hide a fact this gate is partly about: what a
    concatenation publishes that a validating pipeline refuses. RULES 3.10 --
    a rejection is a returned value, never a discarded one.
    """
    if not os.path.exists(path):
        return set(), []
    with open(path, encoding="utf-8", errors="replace") as fh:
        body = fh.read()
    out: set[str] = set()
    rejected: list[str] = []
    for line in body.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        try:
            result = parse(line)
        except Exception as exc:  # noqa: BLE001 - the parser's own refusal
            # ⛔ MASKED BEFORE IT IS RECORDED. Two of the prior art's three
            # unparseable lines carry somebody's private-tracker `authkey`, and
            # a results file that quoted them verbatim would republish a
            # credential this project refuses to publish (T-107, C-70) --
            # in the very artefact arguing that refusing it is worth something.
            # `mask_credential` is the pipeline's own redactor, not a second
            # copy of it.
            rejected.append(f"{mask_credential(line)!r}: {exc}")
            continue
        url = getattr(result, "url", None)
        if url:
            out.add(url)
    return out, rejected


def load_records(paths: list[str]) -> dict[tuple, dict]:
    """Committed health records, grouped by the vantage that took them.

    ⛔ Never merged across groups. RULES 15.4: a `local` result and a `ci`
    result do not have equal reach, and averaging them would publish one
    profile's blindness as the other's measurement.
    """
    groups: dict[tuple, dict] = {}
    for path in sorted(paths):
        with open(path, encoding="utf-8") as fh:
            doc = json.load(fh)
        v = doc.get("vantage", {})
        runner = v.get("runner", {}) or {}
        key = (v.get("environment_class", C.UNKNOWN),
               v.get("execution_profile", C.UNKNOWN),
               str(runner.get("run_id", C.UNKNOWN)))
        g = groups.setdefault(key, {
            "environment_class": key[0], "execution_profile": key[1],
            "run_id": key[2], "generated_at": doc.get("generated_at", C.UNKNOWN),
            "reported_counts": doc.get("counts", {}),
            "files": [], "by_url": {},
        })
        g["files"].append(os.path.basename(path))
        for rec in doc.get("trackers", []):
            g["by_url"][rec["url"]] = rec
    return groups


# --------------------------------------------------------------------------
# the three questions
# --------------------------------------------------------------------------

def arm(records: dict, urls: set[str]) -> dict:
    """Health tally over one arm of the comparison."""
    rows = [records[u] for u in urls if u in records]
    states: dict[str, int] = {}
    failures: dict[str, int] = {}
    for r in rows:
        states[r["health_state"]] = states.get(r["health_state"], 0) + 1
        if r.get("failure"):
            failures[r["failure"]] = failures.get(r["failure"], 0) + 1
    live = states.get("live", 0)
    n = len(rows)
    lo, hi = wilson(live, n)
    return {
        "population": len(urls),
        "measured": n,
        "states": dict(sorted(states.items())),
        "failures": dict(sorted(failures.items(), key=lambda kv: -kv[1])),
        "live": live,
        "live_floor_rate": (live / n) if n else None,
        "live_floor_ci95": [lo, hi],
        # A `dead` count is emitted deliberately, and it is deliberately zero:
        # a reader who wants to know whether we killed anything should find the
        # field rather than infer its absence.
        "dead": states.get("dead", 0),
        "not_live": n - live,
    }


def extrapolate(a: dict) -> dict:
    """Scale an arm's measured rate to its whole population, with its interval."""
    if not a["measured"]:
        return {"point": None, "ci95": [None, None], "basis": "no records"}
    lo, hi = a["live_floor_ci95"]
    return {
        "point": a["live_floor_rate"] * a["population"],
        "ci95": [lo * a["population"], hi * a["population"]],
        "basis": f"{a['live']}/{a['measured']} scaled to {a['population']}",
    }


def disagreements(records: dict, observer_urls: set[str], ours: set[str]) -> dict:
    """Question 3: where an independent observer and this project differ.

    An observer's LIST MEMBERSHIP is its liveness assertion -- both of these
    publish refreshed working lists -- so "on their list" is "they say up".

    ⚠ THE METHODOLOGY DIFFERENCE TRAVELS WITH THE NUMBER. newTrackon derives
    uptime by ANNOUNCING; this project stops at connect and scrape (C-69,
    T-028). ngosang publishes no generator at all. So a disagreement is a
    methodology difference first and a finding second, and neither side is
    presumed right.
    """
    shared = observer_urls & ours
    both, they_only, neither = [], [], []
    for url in sorted(shared):
        rec = records.get(url)
        if rec is None:
            continue
        if rec["health_state"] == "live":
            both.append(url)
        else:
            they_only.append({"url": url, "our_state": rec["health_state"],
                              "our_failure": rec.get("failure"),
                              "our_rung": rec.get("measurement_rung")})
    # What we call live that the observer does not list at all.
    for url, rec in records.items():
        if rec["health_state"] == "live" and url not in observer_urls:
            neither.append(url)
    n = len(both) + len(they_only)
    return {
        "observer_list_size": len(observer_urls),
        "shared_with_our_corpus": len(shared),
        "shared_and_measured": n,
        "agree_live": len(both),
        "they_say_up_we_did_not_reach": len(they_only),
        "agreement_rate": (len(both) / n) if n else None,
        "agreement_ci95": wilson(len(both), n),
        "we_reached_and_they_do_not_list": len(neither),
        "detail_they_say_up_we_did_not_reach": they_only,
    }


def judge(head: dict, ours: int, baseline: int) -> dict:
    """Apply both bars of DECISION_RULE and return the three-valued verdict.

    ⛔ Every comparison here is taken at its LEAST flattering end. Where our
    figure and the baseline's are both intervals, ours is read at its lower
    bound and the baseline's at its upper. The reverse reading is available in
    the emitted JSON, and it is available so that a reader can check we did not
    take it.
    """
    add = head["q1_unique_to_us_and_alive"]["extrapolated_live"]
    base = head["baseline_whole_list"]["extrapolated_live"]
    if add["point"] is None or base["point"] is None:
        return {"delta_is_negligible": None, "conclusion": "unanswerable",
                "reason": "an arm has no records"}

    add_lo, add_hi = add["ci95"]
    base_lo, base_hi = base["ci95"]

    # BAR 1. Our worst case against their best case.
    yield_ratio_worst = 1.0 + (add_lo / base_hi) if base_hi else None
    yield_ratio_point = 1.0 + (add["point"] / base["point"]) if base["point"] else None
    yield_ratio_best = 1.0 + (add_hi / base_lo) if base_lo else None
    # "More than one" is the bar that means anything: at 1.0x a consumer gets
    # no live tracker they did not already have, and the whole dataset is a
    # mirror. It is stated as a strict inequality on the WORST case.
    clears_count = yield_ratio_worst is not None and yield_ratio_worst > 1.0

    # BAR 2. Density: what share of each list answered us.
    ours_live = (add["point"] or 0.0) + (base["point"] or 0.0)
    ours_density = ours_live / ours if ours else None
    base_density = base["point"] / baseline if baseline else None
    clears_density = (ours_density is not None and base_density is not None
                      and ours_density >= base_density)

    if not clears_count:
        conclusion = "not justified"
        detail = ("we add no live tracker a consumer of the baseline does not "
                  "already have, at the pessimistic end. Say so in the README "
                  "and ship a documented mirror, or nothing.")
    elif clears_density:
        conclusion = "justified as a list"
        detail = ("our list is both longer and no less live-dense, so a "
                  "consumer benefits from the URLs alone.")
    else:
        conclusion = "justified as a labelled dataset, not as a list"
        detail = ("we add live trackers, but our list is LESS live-dense than "
                  "the baseline, so a consumer taking it unfiltered gets a "
                  "worse list. Every bit of the value is in publishing the "
                  "measurement alongside the URLs. Publishing the plaintext "
                  "without the health labels would be the prior art, which is "
                  "measured beside this.")

    return {
        "delta_is_negligible": not clears_count,
        "conclusion": conclusion,
        "detail": detail,
        "bar_1_count": {
            "clears": clears_count,
            "live_added_ci95": [add_lo, add_hi],
            "baseline_live_ci95": [base_lo, base_hi],
            "live_yield_ratio_worst_case": yield_ratio_worst,
            "live_yield_ratio_point": yield_ratio_point,
            "live_yield_ratio_best_case": yield_ratio_best,
            "comparison": ("our lower bound against the baseline's upper "
                           "bound"),
        },
        "bar_2_density": {
            "clears": clears_density,
            "our_live_share": ours_density,
            "baseline_live_share": base_density,
            "list_length_ratio": (ours / baseline) if baseline else None,
        },
    }


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--records", nargs="*", default=None,
                    help="health record files; default is every committed "
                         "health-sweep result")
    ap.add_argument("--fixtures", default=FIXTURES)
    ap.add_argument("--out", default=None,
                    help="write the machine-readable result here")
    ap.add_argument("--expect-answered", action="store_true",
                    help="exit 1 if any of the gate's three questions cannot "
                         "be answered from committed evidence")
    args = ap.parse_args()

    paths = args.records or sorted(
        glob.glob(os.path.join(RESULTS, "health-sweep.*.json")))
    if not paths:
        print("no committed health records; the gate cannot be answered from "
              "an empty tree. Run the health sweep first.", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    try:
        agg, _, enforced = load_corpus(True, args.fixtures)
    except Exception as exc:  # noqa: BLE001 - report and refuse, never guess
        print(f"could not build the corpus: {exc}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    ours = {t.url for t in agg.trackers}
    theirs, theirs_rejected = read_urls(
        os.path.join(args.fixtures, f"{BASELINE}.txt"))
    prior_art, prior_art_rejected = read_urls(PRIOR_ART)
    groups = load_records(paths)

    # Present there, absent from what we publish. Auditable per RULES 3.10:
    # every one of these was dropped by a returned decision, not a log line.
    dropped = sorted(theirs - ours)
    drop_reasons = {}
    for url in dropped:
        why = "not accepted by the pipeline"
        for e in getattr(agg, "excluded", []) or []:
            if getattr(e, "url", None) == url:
                why = f"excluded: {getattr(e, 'reason', '-')}"
                break
        drop_reasons[url] = why

    analyses = []
    for key in sorted(groups):
        g = groups[key]
        rec = g["by_url"]
        unique_to_us = ours - theirs
        overlap = ours & theirs

        a_unique = arm(rec, unique_to_us)
        a_overlap = arm(rec, overlap)
        a_baseline_whole = arm(rec, theirs)

        # THE CONTROL. The sample is a stride, not a draw. If baseline
        # membership were correlated with sort position the extrapolation
        # below would be invalid, and this is what says whether it is.
        measured_total = len(rec)
        fraction = measured_total / len(ours) if ours else 0.0
        expected = len(overlap) * fraction
        p = binom_two_sided_p(a_overlap["measured"], len(overlap), fraction)

        analyses.append({
            "vantage": {"environment_class": g["environment_class"],
                        "execution_profile": g["execution_profile"],
                        "run_id": g["run_id"],
                        "generated_at": g["generated_at"],
                        "files": g["files"]},
            # ⛔ Derived here, never read from the record's own `counts.corpus`:
            # run 33938543488 published that field as the SAMPLE size. The
            # correction is under T-024's title (RULES 7).
            "corpus_size": len(ours),
            "records": measured_total,
            "sampling_fraction": fraction,
            "reported_counts_in_record": g["reported_counts"],
            "sample_representativeness_control": {
                "question": "is baseline membership independent of the stride?",
                "expected_baseline_members_in_sample": expected,
                "observed": a_overlap["measured"],
                "two_sided_p": p,
                "verdict": ("consistent with an unbiased sample"
                            if p >= 0.05 else
                            "BIASED: the extrapolation below is not valid"),
            },
            "q1_unique_to_us_and_alive": {
                "arm": a_unique, "extrapolated_live": extrapolate(a_unique)},
            "q2_in_baseline_and_dead_here": {
                "arm": a_overlap,
                "dead": a_overlap["dead"],
                "note": ("structurally zero: MIN_SAMPLES_FOR_DEATH is 3 and "
                         "this is one observation each. `not_live` is the "
                         "weaker claim the evidence supports."),
            },
            "baseline_whole_list": {
                "arm": a_baseline_whole,
                "extrapolated_live": extrapolate(a_baseline_whole)},
            "q3_disagreement": {
                name: disagreements(rec, read_urls(path)[0], ours)
                for name, path in OBSERVERS.items()},
        })

    # The verdict, against the rule stated once at the top of this file.
    head = max(analyses, key=lambda a: a["records"])
    verdict = judge(head, len(ours), len(theirs))
    negligible = verdict["delta_is_negligible"]

    results = {
        "decision_rule": DECISION_RULE,
        "baseline": BASELINE,
        "baseline_size": len(theirs),
        "our_dataset_size": len(ours),
        "unique_to_us": len(ours - theirs),
        "in_baseline_and_dropped_by_us": {"count": len(dropped),
                                          "reasons": drop_reasons},
        "baseline_lines_our_parser_refuses": theirs_rejected,
        "prior_art_pkgforge": {
            "size": len(prior_art),
            "outside_our_corpus": len(prior_art - ours),
            "lines_our_parser_refuses": prior_art_rejected,
            "note": ("the closest prior art is a concatenation of the same "
                     "upstreams; what it holds that we do not is the measure "
                     "of what concatenation adds"),
        },
        "analyses": analyses,
        "verdict": verdict,
        "enforced_exclusions": len(enforced),
    }

    conditions = C.collect(sample_counts={
        "health_record_files": len(paths),
        "vantage_groups": len(groups),
        "records_in_headline_group": head["records"],
        "corpus": len(ours),
        "baseline": len(theirs),
    })
    C.emit("Does this dataset add measurable value over redistributing "
           f"{BASELINE}?", conditions, results, args.out)

    # ---------------------------------------------------------------- report
    print(f"\nCORPUS  ours {len(ours)}   baseline `{BASELINE}` {len(theirs)}   "
          f"unique to us {len(ours - theirs)}")
    print(f"        in the baseline and NOT published by us: {len(dropped)}")
    for url in dropped[:5]:
        print(f"          {url}  <- {drop_reasons[url]}")
    print(f"        prior art (pkgforge concatenation): {len(prior_art)} "
          f"entries, {len(prior_art - ours)} outside our corpus")
    # A second axis of value, and it is not a liveness one: what a
    # concatenation publishes that a validating pipeline refuses to.
    print(f"        lines the production parser REFUSES: baseline "
          f"{len(theirs_rejected)}, prior art {len(prior_art_rejected)}")
    for bad in prior_art_rejected:
        print(f"          {bad}")

    for a in analyses:
        v = a["vantage"]
        print(f"\nRUN {v['run_id']}  {v['environment_class']} / "
              f"{v['execution_profile']}  {v['generated_at']}")
        print(f"  records {a['records']} of corpus {a['corpus_size']} "
              f"({a['sampling_fraction']:.1%})")
        c = a["sample_representativeness_control"]
        print(f"  CONTROL  baseline members in sample: expected "
              f"{c['expected_baseline_members_in_sample']:.1f}, observed "
              f"{c['observed']}, p={c['two_sided_p']:.3f} -> {c['verdict']}")

        for label, block in (("Q1 unique to us", a["q1_unique_to_us_and_alive"]),
                             ("   the baseline", a["baseline_whole_list"])):
            arm_, ex = block["arm"], block["extrapolated_live"]
            if not arm_["measured"]:
                print(f"  {label:16s} no records")
                continue
            lo, hi = arm_["live_floor_ci95"]
            print(f"  {label:16s} live {arm_['live']}/{arm_['measured']} = "
                  f"{arm_['live_floor_rate']:.1%} [{lo:.1%}-{hi:.1%}]  "
                  f"-> {ex['point']:.0f} live of {arm_['population']} "
                  f"[{ex['ci95'][0]:.0f}-{ex['ci95'][1]:.0f}]")

        q2 = a["q2_in_baseline_and_dead_here"]
        print(f"  Q2 baseline entries we call dead: {q2['dead']}  "
              f"(not live: {q2['arm']['not_live']}/{q2['arm']['measured']})")
        print(f"     {q2['note']}")

        print("  Q3 disagreement with an independent observer")
        for name, d in a["q3_disagreement"].items():
            if not d["shared_and_measured"]:
                print(f"     {name:16s} no shared measured trackers")
                continue
            print(f"     {name:16s} agree-live {d['agree_live']}/"
                  f"{d['shared_and_measured']} = {d['agreement_rate']:.1%}; "
                  f"they list {d['they_say_up_we_did_not_reach']} we did not "
                  f"reach; we reached {d['we_reached_and_they_do_not_list']} "
                  f"they do not list")

    b1, b2 = verdict["bar_1_count"], verdict["bar_2_density"]
    print("\nBAR 1  COUNT -- our worst case against the baseline's best")
    print(f"  live trackers we add   {b1['live_added_ci95'][0]:.0f} "
          f"- {b1['live_added_ci95'][1]:.0f}")
    print(f"  live in the baseline   {b1['baseline_live_ci95'][0]:.0f} "
          f"- {b1['baseline_live_ci95'][1]:.0f}")
    print(f"  live yield ratio       worst "
          f"{b1['live_yield_ratio_worst_case']:.2f}x, point "
          f"{b1['live_yield_ratio_point']:.2f}x, best "
          f"{b1['live_yield_ratio_best_case']:.2f}x")
    print(f"  -> {'CLEARS' if b1['clears'] else 'FAILS'}")

    print("\nBAR 2  DENSITY -- what share of each list answered us")
    print(f"  ours      {b2['our_live_share']:.1%} of {len(ours)}")
    print(f"  baseline  {b2['baseline_live_share']:.1%} of {len(theirs)}"
          f"  (our list is {b2['list_length_ratio']:.1f}x longer)")
    print(f"  -> {'CLEARS' if b2['clears'] else 'FAILS'}")

    print(f"\nVERDICT: {verdict['conclusion'].upper()}")
    print(f"  {verdict['detail']}")

    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - That any rate here is a liveness rate. One datacenter, IPv4")
    print("    only, one day, ONE observation per tracker. `live` is a floor:")
    print("    a tracker that timed out is `unknown`, and some of those are up.")
    print("  - That anything is dead. MIN_SAMPLES_FOR_DEATH is 3, so this")
    print("    sweep cannot kill a tracker and question 2's literal answer is")
    print("    zero by construction rather than by evidence.")
    print("  - That the baseline is worse maintained. The opposite is measured:")
    print("    its entries are several times likelier to answer us than ours.")
    print("    The delta is in COUNT, and it is bought with a longer list.")
    print("  - That an observer disagreeing with us is wrong. newTrackon")
    print("    ANNOUNCES and we scrape (C-69); the two answer different")
    print("    questions and a difference is methodology before it is finding.")

    if args.expect_answered:
        problems = []
        for a in analyses:
            if not a["q1_unique_to_us_and_alive"]["arm"]["measured"]:
                problems.append(f"run {a['vantage']['run_id']}: q1 has no records")
            if not a["q2_in_baseline_and_dead_here"]["arm"]["measured"]:
                problems.append(f"run {a['vantage']['run_id']}: q2 has no records")
            if not any(d["shared_and_measured"]
                       for d in a["q3_disagreement"].values()):
                problems.append(f"run {a['vantage']['run_id']}: q3 has no "
                                "shared measured trackers")
            if a["sample_representativeness_control"]["two_sided_p"] < 0.05:
                problems.append(f"run {a['vantage']['run_id']}: the sample is "
                                "biased with respect to baseline membership")
        if problems:
            print("\nEXPECTATION FAILED: --expect-answered")
            for p_ in problems:
                print(f"  {p_}")
            return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
