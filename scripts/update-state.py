#!/usr/bin/env python3
"""Fold a sweep's health records into the tracker history. T-040.

This is the stage between `probe-corpus.py`, which measures, and scoring, which
does not exist yet. It reads the state, applies one or more sweeps, and writes
the state back.

⛔ **IT NEVER DELETES A HISTORY.** A tracker that has left the corpus keeps its
record: RULES 11 says deleting a tracker on first failure destroys the
historical record that makes the dataset valuable, and "it is not in today's
list" is a weaker reason than that. Retention is what makes "apparently gone"
answerable at all.

⛔ **A CORRUPT STATE FILE STOPS THIS.** It does not start a new one. RULES 3.9:
preserving state and retrying beats reconstructing from nothing, and a clean
rebuild that discards history is data loss wearing the costume of a fix. Exit
1, say what is wrong, and leave the file alone for a human.

⚠ **The clock is injected** (`--generated-at`), because RULES 3.6 makes
determinism a property CI asserts. Every timestamp written comes from the
health records themselves, which carry the instant they were observed.

```sh
python3 scripts/update-state.py --state out/history.jsonl \\
    experiments/results/health-sweep.*.json
```

Exit codes:
    0  the state was read, updated and written
    1  the state is corrupt, or a record was unusable; nothing was written
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

from generate import display_path  # noqa: E402
from trackers.state import (CorruptState, apply_sweep, bootstrap,  # noqa: E402
                            write_state)

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("sweeps", nargs="+",
                    help="health.json files from scripts/probe-corpus.py")
    ap.add_argument("--state", required=True,
                    help="the history file; created if absent")
    ap.add_argument("--generated-at", default="1970-01-01T00:00:00Z",
                    help="INJECTED clock, written into the header (RULES 3.6)")
    ap.add_argument("--dry-run", action="store_true",
                    help="report what would change and write nothing")
    args = ap.parse_args()

    try:
        histories, quarantined = bootstrap(args.state)
    except CorruptState as exc:
        # ⛔ The one branch that matters. Starting over here would be the
        # single most destructive thing this program could do.
        print(f"refusing to touch a corrupt state file: {exc}", file=sys.stderr)
        print("nothing was written. The file is left exactly as it was.",
              file=sys.stderr)
        return 1
    except OSError as exc:
        print(f"could not read {args.state}: {exc}", file=sys.stderr)
        return 2

    if quarantined:
        # A damaged line is not a reason to discard the other records, and it
        # is also not something to pass over in silence.
        print(f"⚠ {len(quarantined)} record(s) quarantined and NOT carried "
              f"forward:", file=sys.stderr)
        for q in quarantined:
            print(f"    {q}", file=sys.stderr)

    before = len(histories)
    observed = 0
    for path in sorted(args.sweeps):
        try:
            with open(path, encoding="utf-8") as fh:
                doc = json.load(fh)
        except (OSError, ValueError) as exc:
            print(f"could not read {path}: {exc}", file=sys.stderr)
            return 2
        records = doc.get("trackers") or []
        if not records:
            # An empty sweep is not the same as a sweep that found nothing
            # alive; either way it teaches the history nothing, and pretending
            # otherwise would age every tracker on no evidence.
            print(f"⚠ {display_path(path, REPO)} carries no records; skipped",
                  file=sys.stderr)
            continue
        histories = apply_sweep(histories, records)
        observed += len(records)

    if args.dry_run:
        print(f"--dry-run: {before} record(s) in, {len(histories)} out, "
              f"{observed} observation(s) applied. Nothing was written.")
        return 0

    directory = os.path.dirname(os.path.abspath(args.state))
    os.makedirs(directory, exist_ok=True)
    written = write_state(args.state, histories.values(),
                          generated_at=args.generated_at)

    print(f"state:        {display_path(args.state, REPO)}")
    print(f"records:      {before} -> {written}")
    print(f"observations: {observed} applied from {len(args.sweeps)} sweep(s)")
    print(f"quarantined:  {len(quarantined)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
