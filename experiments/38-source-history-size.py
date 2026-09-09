#!/usr/bin/env python3
"""
QUESTION
    How large does a per-source provenance history get over five years, and
    what must the ring size be to keep it bounded?

WHY IT EXISTS
    T-103's `Decision` is explicit: *"MUST NOT retain unlimited raw upstream
    data in git. Use hashes, compact artefacts, rolling history... **Compute
    the growth rate before choosing** -- same arithmetic discipline as T-042."*
    So this is the same shape as `experiments/31-state-size-projection.py`: it
    builds a **real record** through the production writer, measures its
    serialised length, and projects the file rather than estimating it.

⛔ THE RECORD IS BUILT, NOT GUESSED
    A projection from an assumed row width is a projection of the assumption.
    This calls `provenance.render_line` on a fully-populated observation --
    every optional field present, the longest realistic source id, a full
    64-character digest -- so the width is the writer's, not a guess about it.

⚠ WHAT DRIVES IT IS THE PUBLISH CADENCE, NOT THE SWEEP CADENCE
    Every publish fetches every source, and the publisher runs after every
    sweep: eight times a day at D7's interval. That is the rate a source
    observation arrives at, and it is eight times what a naive "once a day"
    reading would give.

EXIT CODES
    0  the projection ran
    1  the projection ran and an --expect assertion failed
    2  could not run
"""

from __future__ import annotations

import argparse
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, "src"))

import _conditions as C  # noqa: E402
from trackers.politeness import SECONDS_PER_DAY, DEFAULT_INTERVAL_SECONDS  # noqa: E402
from trackers.provenance import (DAILY_DAYS, RING_SIZE,  # noqa: E402
                                 SourceHistory, SourceObservation,
                                 render_line)
from trackers.registry import SOURCES  # noqa: E402


def widest_record(ring: int, daily: int) -> tuple[int, str]:
    """A full record at the configured caps, through the production writer."""
    observations = tuple(
        SourceObservation(
            at=f"2026-09-{(i % 28) + 1:02d}T{(i % 24):02d}:00:00Z",
            outcome="OK",
            entries=1091,
            digest="f" * 64,
            http_status=200,
            bytes_fetched=98_765,
        ) for i in range(ring))
    history = SourceHistory(
        source_id="ngosang_yggdrasil_and_a_long_identifier",
        url="https://raw.githubusercontent.com/example/repository/master/"
            "trackers_all_some_long_name.txt",
        first_seen="2026-08-29T00:00:00Z",
        last_seen="2031-08-29T00:00:00Z",
        lifetime_fetches=14_600,
        lifetime_failures=42,
        ring=observations,
        daily=tuple((f"2026-{(i % 12) + 1:02d}-{(i % 28) + 1:02d}", 8, 1)
                    for i in range(daily)),
    )
    line = render_line(history)
    return len(line.encode("utf-8")), line


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--years", type=float, default=5.0)
    parser.add_argument("--out", default=None)
    parser.add_argument("--expect-under-mb", type=float, default=None,
                        help="exit 1 if the five-year projection exceeds this")
    args = parser.parse_args()

    per_day = SECONDS_PER_DAY / DEFAULT_INTERVAL_SECONDS
    sources = len(SOURCES)
    width, sample = widest_record(RING_SIZE, DAILY_DAYS)

    # ⛔ The file is bounded by the CAPS, not by the run count: a ring drops its
    # oldest entry and the daily aggregates roll. So the steady-state size is
    # what a full record costs times the number of sources, and the projection
    # is a statement about when steady state is reached rather than about
    # unbounded growth.
    steady_bytes = width * sources
    observations_per_year = per_day * 365 * sources
    days_to_fill_ring = RING_SIZE / per_day

    unbounded = width * per_day * 365 * args.years * sources

    results = {
        "sources": sources,
        "publishes_per_day": per_day,
        "ring_size": RING_SIZE,
        "daily_days": DAILY_DAYS,
        "widest_record_bytes": width,
        "steady_state_bytes": steady_bytes,
        "steady_state_mb": round(steady_bytes / 1_048_576, 4),
        "days_to_fill_the_ring": round(days_to_fill_ring, 2),
        "observations_per_year": int(observations_per_year),
        "unbounded_five_year_mb": round(unbounded / 1_048_576, 2),
        "sample_line_head": sample[:180],
        "what_this_is_not": (
            "Not a measurement of a real file: no source history exists yet. "
            "It is the production writer's own output at the configured caps, "
            "which is what bounds the file once it is running."),
    }
    C.emit("How large does the per-source provenance history get?",
           C.collect(sample_counts={"sources": sources,
                                    "ring": RING_SIZE, "daily": DAILY_DAYS}),
           results, args.out)

    print(f"\nsources                 {sources}")
    print(f"publishes per day       {per_day:.0f}  (D7's interval)")
    print(f"ring / daily caps       {RING_SIZE} observations, {DAILY_DAYS} days")
    print(f"widest record           {width} bytes, through the real writer")
    print(f"steady-state file       {steady_bytes} bytes "
          f"({results['steady_state_mb']} MB)")
    print(f"days to fill the ring   {days_to_fill_ring:.1f}")
    print(f"\nWITHOUT the caps, five years would be "
          f"{results['unbounded_five_year_mb']} MB")
    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - That any source history exists. None does yet; this measures")
    print("    what the writer produces at the caps it is configured with.")

    if args.expect_under_mb is not None:
        if results["steady_state_mb"] > args.expect_under_mb:
            print(f"\nEXPECTATION FAILED: {results['steady_state_mb']} MB "
                  f"exceeds {args.expect_under_mb} MB")
            return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
