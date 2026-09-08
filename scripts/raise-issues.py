#!/usr/bin/env python3
"""Tell a human what went wrong. T-080.

⛔ **DRY RUN BY DEFAULT.** This is the only script here that writes to the
repository's issue tracker, and an automation that files issues by accident is
worse than one that files none: RULES 13.1 authorises issues **on this
repository** and nowhere else, and RULES 13.2 makes every other repository
read-only under any framing. `--apply` is required to write anything.

WHAT DECIDES, AND WHAT ACTS

`src/trackers/issues.py` decides and is pure -- it takes the conditions a run
observed plus the issues already open, and returns what to open, update and
close. This script is the thin layer that carries that out. So the properties
that matter are unit-tested rather than integration-hoped:

    a repeated condition produces one issue and not many
    an issue closes when its condition genuinely clears
    no body exceeds the cap

⚠ **Deduplication is by a marker in the body**, never by the title, because a
person may edit a title and an automation matching on it files a second issue.

⛔ **Never paste a large upstream response into an issue.** The body is a
summary a maintainer reads on a phone; `C-44` says a workflow artefact expires
after 90 days, so an issue deferring to one eventually cites nothing.

Exit codes:
    0  the plan was produced, and applied if --apply was given
    1  something the plan needed could not be read
    2  could not run
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))

import _scope  # noqa: E402 - reconfigures stdout on import

from generate import _NoSources, load_corpus  # noqa: E402
from trackers.freshness import assess  # noqa: E402
from trackers.issues import (Condition, ExistingIssue, plan,  # noqa: E402
                             source_conditions, staleness_condition,
                             tracker_conditions)
from trackers.state import read_state  # noqa: E402
from trackers.vantage import detect as detect_vantage  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")


def _gh(args: list[str], *, repository: str) -> str:
    """One `gh` call. ⛔ Always `--repo`, so a misconfigured local remote cannot
    aim this at somebody else's repository (RULES 13.2)."""
    result = subprocess.run(["gh", *args, "--repo", repository],
                            capture_output=True, text=True, timeout=120)
    if result.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args)}: {result.stderr.strip()}")
    return result.stdout


def read_open_issues(repository: str) -> list[ExistingIssue]:
    raw = _gh(["issue", "list", "--state", "open", "--limit", "200",
               "--json", "number,body,state"], repository=repository)
    return [ExistingIssue(number=i["number"], body=i.get("body") or "",
                          state=i.get("state", "open").lower())
            for i in json.loads(raw or "[]")]


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repository", default="Azathothas/Trackers",
                    help="⛔ this repository only. RULES 13.2 forbids writing "
                         "to any other, under any framing.")
    ap.add_argument("--state", default=None,
                    help="a state file, for tracker and staleness conditions")
    ap.add_argument("--watched", default=None, metavar="PATH",
                    help="the maintainer's hardcoded list. Only these raise a "
                         "tracker issue: filing for every tracker that stops "
                         "answering would be a thousand issues (T-047).")
    ap.add_argument("--offline-corpus", action="store_true",
                    help="build the corpus from fixtures rather than fetching")
    ap.add_argument("--fixtures", default=DEFAULT_FIXTURES)
    ap.add_argument("--generated-at", default=None,
                    help="INJECTED clock. Defaults to now, because a staleness "
                         "check against a fixed epoch is not a check.")
    ap.add_argument("--apply", action="store_true",
                    help="actually open, update and close issues. Without it "
                         "the plan is printed and nothing is written.")
    args = ap.parse_args()

    observed_at = args.generated_at or datetime.datetime.now(
        datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

    conditions: list[Condition] = []
    try:
        agg, _, _ = load_corpus(args.offline_corpus, args.fixtures)
    except _NoSources as exc:
        print(exc, file=sys.stderr)
        return 2
    conditions += source_conditions(agg, observed_at=observed_at)

    if args.state and os.path.exists(args.state):
        histories, _ = read_state(args.state)
        watched: list[str] = []
        if args.watched and os.path.exists(args.watched):
            with open(args.watched, encoding="utf-8") as fh:
                watched = [ln.strip() for ln in fh if ln.strip()]
        conditions += tracker_conditions(
            histories, watched=watched,
            vantage=detect_vantage().as_dict())
        newest = max((h.last_seen for h in histories.values()), default=None)
        if newest:
            conditions += staleness_condition(
                assess(newest, now=datetime.datetime.now(datetime.timezone.utc)),
                generated_at=newest)

    try:
        existing = read_open_issues(args.repository)
    except (RuntimeError, OSError, ValueError) as exc:
        print(f"could not read the open issues: {exc}", file=sys.stderr)
        return 1

    decided = plan(conditions, existing)
    print(f"repository: {args.repository}")
    print(f"conditions observed: {len(conditions)}")
    print(f"open automated issues: "
          f"{sum(1 for i in existing if i.key is not None)}")
    print(json.dumps(decided.as_dict(), indent=2, sort_keys=True))

    if not args.apply:
        print("\n--apply was not given, so nothing was written.")
        return 0

    for condition in decided.open_new:
        labels = ",".join(condition.labels)
        _gh(["issue", "create", "--title", condition.title,
             "--body", condition.body(), "--label", labels],
            repository=args.repository)
        print(f"opened: {condition.key}")
    for number, condition in decided.update:
        _gh(["issue", "comment", str(number), "--body", condition.body()],
            repository=args.repository)
        print(f"updated: #{number} {condition.key}")
    for number in decided.close:
        _gh(["issue", "close", str(number), "--comment",
             "The condition this issue reports has cleared. Closed "
             "automatically; reopen if you disagree."],
            repository=args.repository)
        print(f"closed: #{number}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
