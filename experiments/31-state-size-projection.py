#!/usr/bin/env python3
"""
QUESTION
    How large does this project's history file get over five years, and what
    do `K` and `D` have to be for that number to be one somebody would accept?

WHY IT EXISTS
    T-042, and its source says "compute the size", not estimate it. ⛔ **A
    state file that grows unboundedly is the same outage as a git history that
    does** -- and this project already knows that failure, because RULES 3.7
    exists to keep history out of git history precisely so the data branch can
    be reset without losing data.

    T-040 says `K` and `D` are chosen **from this arithmetic rather than from
    taste**, so this runs first and its output is an input to the design.

    ⭐ It also decides something less obvious: whether a tracker that
    disappears can be kept forever. RULES 11 forbids deleting a tracker on
    first failure -- "destroys the historical record that makes the dataset
    valuable" -- so the population this file must hold is not today's corpus,
    it is **every tracker ever seen**.

WHAT IS MEASURED AND WHAT IS ASSUMED
    ⭐ Measured, from the tree: the accepted corpus, and the exact serialised
    size of a real record built by `src/trackers/state.py` from real health
    records. The bytes are not estimated from a guess about JSON overhead --
    a record is built and `len()` is taken.

    ⚠ Assumed, and every one is a named parameter rather than a number buried
    in a formula: the probe cadence (D7's 3 h default), the growth rate of the
    corpus, and five years. Change one on the command line and the projection
    changes with it; that is the point of an instrument over an estimate.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run

USAGE
    ./31-state-size-projection.py
    ./31-state-size-projection.py --expect-under-mb 64
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, "src"))
sys.path.insert(0, os.path.join(REPO, "scripts"))

import _conditions as C  # noqa: E402
from generate import load_corpus  # noqa: E402
from trackers.state import (DAILY_DAYS, RING_SIZE, STATE_FORMAT,  # noqa: E402
                            TrackerHistory, render_line)

FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")
RESULTS = os.path.join(HERE, "results")

#: D7: probe each tracker on its own stated interval, defaulting to 3 h.
DEFAULT_INTERVAL_HOURS = 3.0

#: ⚠ AN ASSUMPTION, NOT A MEASUREMENT. The corpus has been measured once, so
#: there is no growth rate in this tree to read. 20%/year is stated here so a
#: reader can disagree with a number rather than with a feeling.
DEFAULT_GROWTH_PER_YEAR = 0.20


def worst_case_record(url: str, clock: str) -> TrackerHistory:
    """A history with every bounded field full.

    ⭐ **The projection is of a FULL record, never an average one.** A file
    sized on the average is a file that overflows once the trackers have been
    around long enough, which is exactly when nobody is watching any more.
    """
    h = TrackerHistory.new(url, first_seen=clock)
    # Fill the ring and the daily aggregates past their bounds, so what is
    # measured is the size after they have been capped rather than before.
    for i in range(RING_SIZE + 10):
        stamp = f"2026-{1 + i % 12:02d}-{1 + i % 28:02d}T{i % 24:02d}:00:00Z"
        h = h.observe(state="degraded", ok=(i % 2 == 0), observed_at=stamp,
                      rung="tracker_semantic", failure="timeout")
    for d in range(DAILY_DAYS + 10):
        stamp = f"20{26 + d // 365:02d}-{1 + d % 12:02d}-{1 + d % 28:02d}T00:00:00Z"
        h = h.observe(state="live", ok=True, observed_at=stamp,
                      rung="tracker_semantic", failure=None)
    return h


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default=None)
    ap.add_argument("--years", type=float, default=5.0)
    ap.add_argument("--growth", type=float, default=DEFAULT_GROWTH_PER_YEAR,
                    help="assumed fractional corpus growth per year")
    ap.add_argument("--interval-hours", type=float,
                    default=DEFAULT_INTERVAL_HOURS)
    ap.add_argument("--expect-under-mb", type=float, default=None,
                    help="exit 1 if the five-year projection exceeds this")
    args = ap.parse_args()

    try:
        agg, _, _ = load_corpus(True, FIXTURES)
    except Exception as exc:  # noqa: BLE001
        print(f"could not build the corpus: {exc}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    today = len(agg.trackers)

    # ⭐ MEASURED, not assumed: build a real full record and take its length.
    longest = max((t.url for t in agg.trackers), key=len)
    typical = sorted(t.url for t in agg.trackers)[today // 2]
    sizes = {
        "worst_case_url": len(render_line(worst_case_record(longest, "2026-01-01T00:00:00Z"))),
        "typical_url": len(render_line(worst_case_record(typical, "2026-01-01T00:00:00Z"))),
        "empty_record": len(render_line(TrackerHistory.new(typical, "2026-01-01T00:00:00Z"))),
    }
    per_record = sizes["worst_case_url"]

    # Every tracker ever seen, because a tracker that disappears is kept
    # (RULES 11). Growth compounds; the file does not shrink.
    ever_seen = today
    per_year = []
    for year in range(1, int(args.years) + 1):
        ever_seen = int(round(ever_seen * (1 + args.growth)))
        per_year.append({"year": year, "trackers_ever_seen": ever_seen,
                         "bytes": ever_seen * per_record,
                         "megabytes": ever_seen * per_record / 1_048_576})

    final = per_year[-1]
    checks_per_tracker_per_day = 24.0 / args.interval_hours
    ring_days = RING_SIZE / checks_per_tracker_per_day

    # ⭐ THE TRADE-OFF, SWEPT RATHER THAN ASSERTED. T-040 says K and D come
    # from this arithmetic, so the arithmetic has to show what the alternatives
    # cost. Each row is measured the same way -- a full record built and its
    # length taken -- with only the bound changed.
    import trackers.state as _state
    sweep = []
    saved = (_state.RING_SIZE, _state.DAILY_DAYS)
    try:
        for k in (32, 64, 128):
            for d in (90, 180, 365):
                _state.RING_SIZE, _state.DAILY_DAYS = k, d
                size = len(render_line(
                    worst_case_record(longest, "2026-01-01T00:00:00Z")))
                sweep.append({
                    "K": k, "D": d, "record_bytes": size,
                    "five_year_megabytes": final["trackers_ever_seen"] * size
                    / 1_048_576,
                    "ring_covers_days": k / checks_per_tracker_per_day,
                })
    finally:
        _state.RING_SIZE, _state.DAILY_DAYS = saved

    results = {
        "format": STATE_FORMAT,
        "ring_size_K": RING_SIZE,
        "daily_days_D": DAILY_DAYS,
        "measured_record_bytes": sizes,
        "corpus_today": today,
        "assumptions": {
            "years": args.years,
            "growth_per_year": args.growth,
            "probe_interval_hours": args.interval_hours,
            "note": ("growth is ASSUMED -- the corpus has been measured once, "
                     "so there is no rate in this tree to read. The other two "
                     "are D7's cadence and the projection window."),
        },
        "derived": {
            "checks_per_tracker_per_day": checks_per_tracker_per_day,
            "ring_covers_days": ring_days,
            "daily_covers_days": DAILY_DAYS,
        },
        "projection": per_year,
        "five_year_megabytes": final["megabytes"],
        "k_and_d_trade_off": sweep,
    }

    conditions = C.collect(sample_counts={"corpus_today": today,
                                          "ring_size": RING_SIZE,
                                          "daily_days": DAILY_DAYS})
    C.emit("How large does the history file get over five years, and what do "
           "K and D have to be?", conditions, results, args.out)

    print(f"\nRECORD SIZE, measured by building one and taking its length")
    for k, v in sizes.items():
        print(f"  {k:20s} {v:6d} bytes")
    print(f"\nSHAPE  K = {RING_SIZE} outcomes, D = {DAILY_DAYS} daily aggregates")
    print(f"  at D7's {args.interval_hours:g} h cadence that is "
          f"{checks_per_tracker_per_day:.1f} checks/day, so the ring covers "
          f"{ring_days:.1f} days")
    print(f"  and the daily aggregates cover {DAILY_DAYS} days "
          f"({DAILY_DAYS / 365:.1f} years)")

    print(f"\nPROJECTION  {today} trackers today, growing {args.growth:.0%}/year, "
          f"nothing ever deleted")
    for row in per_year:
        print(f"  year {row['year']}  {row['trackers_ever_seen']:6d} trackers  "
              f"{row['megabytes']:7.1f} MB")

    print("\nTHE TRADE-OFF  five-year size at other bounds, measured the same way")
    print("     K    D   record   5yr MB   ring covers")
    for row in sweep:
        mark = "  <- chosen" if (row["K"], row["D"]) == (RING_SIZE, DAILY_DAYS) else ""
        print(f"  {row['K']:4d} {row['D']:4d}  {row['record_bytes']:6d}  "
              f"{row['five_year_megabytes']:7.1f}   "
              f"{row['ring_covers_days']:4.1f} days{mark}")

    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - The growth rate. It is assumed and stated, not measured: this")
    print("    corpus has one census. Re-run with --growth to disagree.")
    print("  - That the file is the only cost. A file rewritten hourly also")
    print("    costs whatever stores its versions, which is why RULES 3.7 puts")
    print("    history in files on a branch that is reset rather than in git")
    print("    history that is not.")
    print("  - That a full record is typical. It is deliberately the worst")
    print("    case: sizing on the average overflows exactly when the trackers")
    print("    have been around long enough for nobody to be watching.")

    if args.expect_under_mb is not None and final["megabytes"] > args.expect_under_mb:
        print(f"\nEXPECTATION FAILED: --expect-under-mb {args.expect_under_mb}")
        print(f"  the five-year projection is {final['megabytes']:.1f} MB. "
              "Reduce K or D, and record why in T-042.")
        return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
