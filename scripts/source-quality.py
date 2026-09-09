#!/usr/bin/env python3
"""Per-source quality: what is measured, beside what was asserted. T-101.

⛔ **This reports and it does not act.** `Trust` in the registry is a human's
reading of each upstream; the numbers here are measurements of the same
sources. Where they disagree the report asks a question rather than answering
it, because T-101's `Decision` says both interesting findings run the opposite
way from the obvious action: a source contributing nothing unique may be the
**corroboration** that turns another source's claim into evidence, and a source
with many unique but scruffy entries wants **stricter filtering, not removal**.

⚠ **It touches no third party.** The uniqueness half comes from the corpus this
repository already builds, and the history half from `sources.jsonl` on the
`data` branch. Regenerating the report costs nobody a request, which is what
lets T-101's "not a one-time judgement" be true in practice.

```sh
python3 scripts/source-quality.py --offline
python3 scripts/source-quality.py --offline --history published/sources.jsonl
```

Exit codes:
    0  the report was produced
    1  a --expect assertion failed
    2  could not run
"""

from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))

import _scope  # noqa: E402 - reconfigures stdout on import
from generate import _NoSources, load_corpus  # noqa: E402
from trackers.provenance import CorruptProvenance, read_history  # noqa: E402
from trackers.quality import assess_all  # noqa: E402
from trackers.registry import SOURCES  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")


def main() -> int:
    _scope.printable_stdout()
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--offline", action="store_true",
                    help="build the corpus from committed fixtures")
    ap.add_argument("--fixtures", default=DEFAULT_FIXTURES)
    ap.add_argument("--history", default=None, metavar="SOURCES_JSONL",
                    help="the per-source provenance (T-103). ⚠ Without it "
                         "every history-derived column is a dash, because an "
                         "unknown rate rendered as zero reads as perfect")
    ap.add_argument("--json", action="store_true",
                    help="machine-readable, for a report that is consumed "
                         "rather than read")
    ap.add_argument("--expect-no-questions", action="store_true",
                    help="exit 1 if any source raises a question. ⚠ Off by "
                         "default and it should stay off in CI: a question is "
                         "for a human, and failing a build on one would make "
                         "somebody answer it by deleting the source")
    args = ap.parse_args()

    try:
        agg, _, _ = load_corpus(args.offline, args.fixtures)
    except _NoSources as exc:
        print(exc, file=sys.stderr)
        return 2

    histories: dict = {}
    if args.history:
        if not os.path.exists(args.history):
            print(f"--history {args.history} does not exist; the "
                  f"history-derived columns will be dashes", file=sys.stderr)
        else:
            try:
                histories, _ = read_history(args.history)
            except CorruptProvenance as exc:
                print(f"--history {args.history}: {exc}", file=sys.stderr)
                return 2

    rows = assess_all(SOURCES, provenance=agg.provenance, histories=histories)

    if args.json:
        print(json.dumps({"sources": [r.as_record() for r in rows]},
                         indent=2, sort_keys=True))
    else:
        print(f"{'source':20s} {'trust':7s} {'contrib':>8s} {'uniq':>6s} "
              f"{'corrob':>7s} {'obs':>4s} {'fail':>6s} {'bodies':>7s}")
        for row in rows:
            fail = "-" if row.failure_rate is None else f"{row.failure_rate:.0%}"
            bodies = "-" if row.distinct_bodies is None else str(row.distinct_bodies)
            print(f"{row.source_id:20s} {row.asserted_trust:7s} "
                  f"{row.contributed:8d} {row.unique:6d} {row.corroborating:7d} "
                  f"{row.observations:4d} {fail:>6s} {bodies:>7s}")
        print()
        asked = 0
        for row in rows:
            for question in row.disagreements():
                asked += 1
                print(f"  ? {row.source_id}: {question}")
        if not asked:
            print("  no source raises a question against its asserted trust")
        print("\n⛔ Questions are for a human. T-101: a source with no unique")
        print("   entries may be corroboration, and one with many scruffy")
        print("   unique entries wants filtering rather than removal.")
        if not histories:
            print("\n⚠ No history supplied, so `obs`, `fail` and `bodies` are")
            print("   dashes rather than zeroes. An unknown rendered as zero")
            print("   would read as a perfect record (RULES 1.5).")

    if args.expect_no_questions:
        questions = [q for row in rows for q in row.disagreements()]
        if questions:
            print(f"\nEXPECTATION FAILED: {len(questions)} question(s) raised.")
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
