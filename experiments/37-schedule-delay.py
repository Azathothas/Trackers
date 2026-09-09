#!/usr/bin/env python3
"""
QUESTION
    How late does a scheduled workflow actually fire here, and how often does a
    slot produce no run at all?

WHY IT EXISTS
    T-009 and `C-11`. GitHub documents that scheduled runs can be delayed and
    that "some queued jobs may be dropped". The **rate** was never observed
    here, so every design that has to survive a late or missing run was sized
    against documentation rather than against this repository.

⛔ AND THE RATE STOPPED BEING A CURIOSITY ON 2026-09-09
    T-009 recorded, correctly, that a displaced run takes the next bucket's
    slice and therefore that slices are *skipped*. It then said they are
    "skipped, **never repeated**". ⛔ **That is refuted.** A dispatch and a
    late scheduled run inside one three-hour bucket take the **same** slice:
    runs `34281244142` and `34289476724` both took slice 5 and 192 trackers
    were contacted 5878 s apart. T-087 fixed the consequence; this instrument
    measures the cause, because the delay is what made two runs share a bucket.

⛔ THE SLOTS COME FROM THE WORKFLOW, NOT FROM A CONSTANT
    The cron is read out of `.github/workflows/health-sweep.yml`. A copy of it
    here would keep reporting against a schedule the repository had changed,
    which is the class of defect `tests/test_politeness.py` already reads that
    file to avoid.

⚠ WHAT A DROP IS AND IS NOT
    A slot with no run **inside this window** is counted as a drop. It is an
    upper bound: a run may have been dropped, or the repository may have had no
    schedule yet, or the API page may not reach back that far. The window is
    therefore clamped to start at the first scheduled run observed, and the
    count is reported as "slots with no run" rather than as "drops".

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import re
import statistics
import sys
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, "src"))

import _conditions as C  # noqa: E402

WORKFLOW = os.path.join(REPO, ".github", "workflows", "health-sweep.yml")

#: RULES 16's read-only GitHub route, for a vantage that cannot reach the API
#: directly. ⛔ Read-only, and never used for a write of any kind.
PROXY_BASE = "https://api.gh.pkgforge.dev/"


def cron_hours(path: str) -> tuple[int, ...]:
    """The hours the workflow's cron fires on, read from the workflow itself.

    Only the `0 */N * * *` and `M H,H * * *` shapes this repository uses are
    understood. ⛔ Anything else **raises** rather than being guessed at: a
    schedule this cannot read is one whose slots would be invented, and an
    invented slot produces an invented delay.
    """
    with open(path, encoding="utf-8") as handle:
        text = handle.read()
    crons = re.findall(r'-\s*cron:\s*["\']([^"\']+)["\']', text)
    if not crons:
        raise ValueError(f"no cron found in {path}")
    hours: set[int] = set()
    for cron in crons:
        fields = cron.split()
        if len(fields) != 5:
            raise ValueError(f"cannot read cron {cron!r}")
        minute, hour = fields[0], fields[1]
        if minute != "0":
            raise ValueError(f"cron {cron!r} does not fire on the hour; this "
                             f"instrument only reads the shapes in use here")
        if hour.startswith("*/"):
            step = int(hour[2:])
            hours.update(range(0, 24, step))
        elif hour == "*":
            hours.update(range(24))
        else:
            hours.update(int(h) for h in hour.split(","))
    return tuple(sorted(hours))


def fetch_runs(repo: str, workflow: str, token: str | None,
               proxied: bool) -> list[dict]:
    """Every run of one workflow, newest first. A read, and only a read."""
    route = (f"repos/{repo}/actions/workflows/{workflow}/runs?per_page=100")
    url = (PROXY_BASE + route) if proxied else ("https://api.github.com/" + route)
    request = urllib.request.Request(url, headers={
        "Accept": "application/vnd.github+json",
        "User-Agent": "trackers-experiment-37",
    })
    if token and not proxied:
        request.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response).get("workflow_runs", [])


def _instant(text: str) -> datetime.datetime:
    return datetime.datetime.fromisoformat(text.replace("Z", "+00:00"))


def slots_between(start: datetime.datetime, end: datetime.datetime,
                  hours: tuple[int, ...]) -> list[datetime.datetime]:
    """Every cron slot in `[start, end]`, oldest first."""
    out: list[datetime.datetime] = []
    day = start.replace(hour=0, minute=0, second=0, microsecond=0)
    while day <= end:
        for hour in hours:
            moment = day.replace(hour=hour)
            if start <= moment <= end:
                out.append(moment)
        day += datetime.timedelta(days=1)
    return sorted(out)


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", default="Azathothas/Trackers")
    parser.add_argument("--workflow", default="health-sweep.yml")
    parser.add_argument("--out", default=None)
    parser.add_argument("--proxied", action="store_true",
                        help="go through RULES 16's read-only GitHub route")
    parser.add_argument("--expect-samples", type=int, default=None,
                        metavar="N",
                        help="exit 1 with fewer than N scheduled runs "
                             "observed. T-009 asks for 100 before the "
                             "distribution is a distribution")
    args = parser.parse_args()

    try:
        hours = cron_hours(WORKFLOW)
    except (OSError, ValueError) as exc:
        print(f"could not read the schedule: {exc}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN
    try:
        runs = fetch_runs(args.repo, args.workflow,
                          os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN"),
                          args.proxied)
    except (urllib.error.URLError, OSError, ValueError) as exc:
        print(f"could not read the run list: {exc}", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    scheduled = sorted(
        ({"id": r["id"], "created_at": r["created_at"],
          "conclusion": r.get("conclusion")}
         for r in runs if r.get("event") == "schedule"),
        key=lambda r: r["created_at"])
    others = [r for r in runs if r.get("event") != "schedule"]

    rows: list[dict] = []
    for run in scheduled:
        fired = _instant(run["created_at"])
        # The nearest slot at or before the firing. ⛔ Not the nearest slot in
        # either direction: a run cannot fire before its own slot, and pairing
        # it with a later one would report a negative delay as an early run.
        candidates = [s for s in slots_between(
            fired - datetime.timedelta(days=2), fired, hours) if s <= fired]
        slot = candidates[-1] if candidates else None
        delay = (fired - slot).total_seconds() if slot else None
        rows.append({**run,
                     "slot": slot.strftime("%Y-%m-%dT%H:%M:%SZ") if slot else None,
                     "delay_seconds": None if delay is None else int(delay)})

    delays = [r["delay_seconds"] for r in rows if r["delay_seconds"] is not None]
    covered: list[str] = []
    unfilled: list[str] = []
    if rows:
        first, last = _instant(rows[0]["created_at"]), _instant(rows[-1]["created_at"])
        filled = {r["slot"] for r in rows}
        for slot in slots_between(first.replace(minute=0, second=0,
                                                microsecond=0), last, hours):
            stamp = slot.strftime("%Y-%m-%dT%H:%M:%SZ")
            covered.append(stamp)
            if stamp not in filled:
                unfilled.append(stamp)

    distribution = {}
    if delays:
        distribution = {
            "n": len(delays),
            "min_seconds": min(delays),
            "max_seconds": max(delays),
            "median_seconds": int(statistics.median(delays)),
            "mean_seconds": int(statistics.fmean(delays)),
            "over_one_interval": sum(1 for d in delays if d >= 10_800),
        }

    results = {
        "cron_hours": list(hours),
        "scheduled_runs": rows,
        "non_scheduled_runs": len(others),
        "delay_distribution": distribution,
        "slots_in_window": len(covered),
        "slots_with_no_run": unfilled,
        "target_samples": 100,
        "supports_a_distribution": len(delays) >= 100,
        "what_this_is_not": (
            "`slots_with_no_run` is an UPPER BOUND on drops, not a drop count: "
            "a slot may be empty because a run was dropped, because the "
            "schedule did not exist yet, or because the API page does not "
            "reach it. And a delay measured soon after the schedule was pushed "
            "carries registration lag that GitHub does not publish "
            "separately."),
    }
    conditions = C.collect(sample_counts={
        "scheduled_runs": len(rows), "slots": len(covered)})
    C.emit("How late does a scheduled workflow fire here, and how often does a "
           "slot produce no run?", conditions, results, args.out)

    print(f"\ncron fires at hours {list(hours)} UTC")
    print(f"\n{'SLOT':22s} {'FIRED':22s} DELAY")
    for row in rows:
        delay = row["delay_seconds"]
        print(f"  {str(row['slot']):20s} {row['created_at']:22s} "
              f"{'-' if delay is None else f'{delay // 60}m {delay % 60}s'}")
    if distribution:
        print(f"\nn={distribution['n']}  "
              f"min={distribution['min_seconds'] // 60}m  "
              f"median={distribution['median_seconds'] // 60}m  "
              f"max={distribution['max_seconds'] // 60}m  "
              f"over one interval: {distribution['over_one_interval']}")
    print(f"slots in the window: {len(covered)}, with no run: {len(unfilled)}")
    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - A drop RATE. An empty slot has three explanations and this")
    print("    instrument can only see one of them.")
    print(f"  - A distribution. T-009 asks for 100 runs; there are "
          f"{len(delays)}.")

    if args.expect_samples is not None and len(delays) < args.expect_samples:
        print(f"\nEXPECTATION FAILED: {len(delays)} scheduled runs observed, "
              f"{args.expect_samples} asked for.")
        return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
