#!/usr/bin/env python3
"""
QUESTION
    How fast does the `data` branch's history grow, and at what commit count
    does a full clone stop being reasonable?

WHY IT EXISTS
    T-081. The threshold for resetting the branch was inherited from the prior
    art at ~5000 commits, and the entry says it "needs deriving from the
    observed commit rate rather than inheriting". A number nobody measured is
    the thing RULES 1.5 forbids, and this is the instrument that measures it.

HOW
    Two clones of the same branch: one shallow, one full. The shallow one is
    what a snapshot costs; the difference is what the history costs, and
    dividing by the commits gives a per-commit figure.

        history cost = full(.git) - shallow(.git)
        per commit   = history cost / (commits - 1)

    ⭐ **Shallow against full rather than a guess at delta sizes.** Git packs
    and deltifies, so summing object sizes overstates by a large and unknowable
    factor. Measuring two real clones measures what a consumer would actually
    download.

⛔ WHAT THIS CANNOT TELL YOU, AND IT MATTERS MORE THAN THE NUMBER
    **The per-commit cost is not constant.** Every commit rewrites
    `state.jsonl`, and that file grows as history accumulates -- T-042 projects
    **23.4 MB** at five years against about 145 KB today. So a late-life commit
    costs far more than an early one, and any projection that multiplies
    today's figure by a commit count **understates the future**.

    That is why the threshold this derives is a **size** ceiling expressed as a
    commit count at today's rate, and why the count must be re-derived rather
    than trusted: run this again when the state file has grown.

EXIT CODES
    0  measured
    1  measured and an --expect assertion failed
    2  could not run

USAGE
    ./34-data-branch-growth.py
    ./34-data-branch-growth.py --ceiling-mib 250 --expect-headroom
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)

import _conditions as C  # noqa: E402

DEFAULT_URL = "https://github.com/Azathothas/Trackers"
#: Publishes per day: the sweep is scheduled every three hours and the
#: publisher runs after each one (D7).
PUBLISHES_PER_DAY = 8


def directory_kib(path: str) -> int:
    total = 0
    for root, _dirs, files in os.walk(path):
        for name in files:
            try:
                total += os.path.getsize(os.path.join(root, name))
            except OSError:
                pass
    return total // 1024


def clone(url: str, branch: str, into: str, *, depth: int | None) -> None:
    args = ["git", "clone", "--quiet", "--branch", branch, "--single-branch"]
    if depth:
        args += ["--depth", str(depth)]
    args += [url, into]
    subprocess.run(args, check=True, capture_output=True, timeout=600)


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default=DEFAULT_URL)
    ap.add_argument("--branch", default="data")
    ap.add_argument("--out", default=None)
    ap.add_argument("--ceiling-mib", type=int, default=250,
                    help="how large a full clone may become. The threshold is "
                         "derived from this rather than inherited.")
    ap.add_argument("--expect-headroom", action="store_true",
                    help="exit 1 if the branch is already past the derived "
                         "threshold, which would mean housekeeping is overdue")
    args = ap.parse_args()

    workspace = tempfile.mkdtemp(prefix="branch-growth-")
    try:
        full = os.path.join(workspace, "full")
        shallow = os.path.join(workspace, "shallow")
        try:
            clone(args.url, args.branch, full, depth=None)
            clone(args.url, args.branch, shallow, depth=1)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
            print(f"could not clone {args.url}#{args.branch}: {exc}",
                  file=sys.stderr)
            return C.EXIT_COULD_NOT_RUN

        # ⚠ `gc` first: a fresh clone's pack layout depends on what the server
        # sent, so measuring without it measures the transfer rather than the
        # storage.
        subprocess.run(["git", "-C", full, "gc", "--quiet"], check=False,
                       capture_output=True, timeout=600)
        commits = int(subprocess.run(
            ["git", "-C", full, "rev-list", "--count", "HEAD"],
            capture_output=True, text=True, check=True).stdout.strip())

        full_kib = directory_kib(os.path.join(full, ".git"))
        shallow_kib = directory_kib(os.path.join(shallow, ".git"))
        history_kib = max(full_kib - shallow_kib, 0)
        per_commit = history_kib / max(commits - 1, 1)

        ceiling_kib = args.ceiling_mib * 1024
        threshold = (int((ceiling_kib - shallow_kib) / per_commit)
                     if per_commit > 0 else None)
        years = (threshold / PUBLISHES_PER_DAY / 365) if threshold else None

        results = {
            "branch": args.branch,
            "commits": commits,
            "snapshot_kib": shallow_kib,
            "full_kib": full_kib,
            "history_kib": history_kib,
            "kib_per_commit": round(per_commit, 2),
            "ceiling_mib": args.ceiling_mib,
            "derived_threshold_commits": threshold,
            "years_at_current_cadence": round(years, 2) if years else None,
            "publishes_per_day": PUBLISHES_PER_DAY,
            "inherited_threshold_was": 5000,
            "the_rate_is_not_constant": (
                "every commit rewrites state.jsonl and that file grows; T-042 "
                "projects 23.4 MB at five years against ~145 KB today, so this "
                "projection understates the future and the threshold must be "
                "re-derived rather than trusted"),
        }

        conditions = C.collect(sample_counts={
            "commits": commits,
            "delta_commits_measured": max(commits - 1, 0),
        })
        C.emit("How fast does the data branch's history grow?",
               conditions, results, args.out)

        print(f"\nBRANCH  {args.branch}: {commits} commit(s)")
        print(f"  one snapshot        {shallow_kib:8d} KiB")
        print(f"  full history        {full_kib:8d} KiB")
        print(f"  history costs       {history_kib:8d} KiB "
              f"= {per_commit:.1f} KiB per commit")
        print(f"\nDERIVED THRESHOLD (a full clone under {args.ceiling_mib} MiB)")
        print(f"  {threshold} commits, about {years:.1f} years at "
              f"{PUBLISHES_PER_DAY} publishes a day")
        print(f"  the inherited number was 5000, which is "
              f"{5000 / PUBLISHES_PER_DAY / 365:.1f} years here")
        print("\n⛔ THE RATE IS NOT CONSTANT")
        print("  Every commit rewrites state.jsonl and that file grows.")
        print("  T-042 projects 23.4 MB at five years against ~145 KB today,")
        print("  so this projection UNDERSTATES the future. Re-run it rather")
        print("  than trusting the number.")

        if args.expect_headroom and threshold and commits >= threshold:
            print(f"\nEXPECTATION FAILED: {commits} commits is at or past the "
                  f"derived threshold of {threshold}; housekeeping is overdue")
            return C.EXIT_MEASURED_AND_FAILED
        return C.EXIT_MEASURED
    finally:
        shutil.rmtree(workspace, ignore_errors=True)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
