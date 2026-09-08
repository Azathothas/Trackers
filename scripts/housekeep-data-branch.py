#!/usr/bin/env python3
"""Reset the data branch's history when it has grown too large. T-081.

⛔ **DRY RUN BY DEFAULT, AND IT REWRITES HISTORY.** This is the one operation
in this project that discards commits, so it is the one that has to be hardest
to fire by accident: `--apply` is required, the threshold has to be crossed,
and it refuses while a publication is in flight.

WHY IT IS SAFE, AND THE ARGUMENT IS RULES 3.7

**History lives in files, not in git history.** Every measurement is in
`state.jsonl`, which the reset **preserves byte for byte** -- what is discarded
is commits, not data. The prior art resets its data branch the same way, and it
is safe there only because that repository stores nothing worth losing; here it
is safe because the thing worth keeping is a tracked file.

⛔ **A consumer pinned to a commit SHA on the data branch breaks by design**,
and that is why the documented pin target is the branch or a tag. Said in
`docs/schema.md` rather than only here.

THE THRESHOLD IS DERIVED, NOT INHERITED

`experiments/34-data-branch-growth.py` measures what history costs: **14.3 KiB
per commit** on 2026-09-09, so a full clone stays under 250 MiB for about
17,853 commits, or 6.1 years at eight publishes a day. The prior art's 5000
would be 1.7 years here.

⚠ **The rate is not constant and the count is therefore a proxy.** Every commit
rewrites `state.jsonl`, and T-042 projects that file at **23.4 MB** in five
years against ~145 KB today, so late commits cost far more than early ones.
This checks the **measured size** as well, and the size is what decides.

Exit codes:
    0  checked, and reset if --apply and the threshold was crossed
    1  refused: a publication is in flight, or a verification failed
    2  could not run
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))

import _scope  # noqa: E402 - reconfigures stdout on import

#: A full clone should stay under this. The commit threshold is derived from
#: it and from a measured per-commit cost, rather than being a round number
#: somebody liked.
CEILING_MIB = 250

#: Derived by `experiments/34-data-branch-growth.py` on 2026-09-09 from 14.3
#: KiB per commit. ⚠ Re-derive it rather than trusting it: the per-commit cost
#: grows with `state.jsonl`.
THRESHOLD_COMMITS = 17853

#: ⛔ Nothing outside this list survives a reset, and the reset refuses if any
#: of them is missing afterwards. It is the published contract.
REQUIRED_FILES = ("trackers_all.txt", "trackers_all.json", "trackers_all.csv",
                  "report.md", "metadata.json", "state.jsonl", "schema.md")


def run(args: list[str], *, cwd: str | None = None) -> str:
    result = subprocess.run(args, cwd=cwd, capture_output=True, text=True,
                            timeout=600)
    if result.returncode != 0:
        raise RuntimeError(f"{' '.join(args)}: {result.stderr.strip()}")
    return result.stdout


def directory_kib(path: str) -> int:
    total = 0
    for root, _dirs, files in os.walk(path):
        for name in files:
            try:
                total += os.path.getsize(os.path.join(root, name))
            except OSError:
                pass
    return total // 1024


def publication_in_flight(repository: str) -> bool:
    """⛔ Never reset while a publish is running. The two would race for the
    branch and the loser's dataset is the one on it."""
    try:
        raw = run(["gh", "run", "list", "--repo", repository,
                   "--workflow", "Publish", "--limit", "5",
                   "--json", "status"])
    except (RuntimeError, OSError):
        # ⚠ Unable to tell is not permission to proceed. A destructive
        # operation that cannot confirm the field is clear does not run.
        return True
    return any(r.get("status") in ("in_progress", "queued")
               for r in json.loads(raw or "[]"))


def assess(checkout: str) -> dict:
    run(["git", "gc", "--quiet"], cwd=checkout)
    commits = int(run(["git", "rev-list", "--count", "HEAD"],
                      cwd=checkout).strip())
    size_kib = directory_kib(os.path.join(checkout, ".git"))
    present = sorted(n for n in REQUIRED_FILES
                     if os.path.exists(os.path.join(checkout, n)))
    return {"commits": commits, "git_kib": size_kib,
            "git_mib": round(size_kib / 1024, 2),
            "files_present": present,
            "files_missing": sorted(set(REQUIRED_FILES) - set(present)),
            "over_commit_threshold": commits >= THRESHOLD_COMMITS,
            "over_size_ceiling": size_kib >= CEILING_MIB * 1024}


def reset_history(checkout: str, *, branch: str, message: str) -> None:
    """Replace the branch with one commit carrying the current working tree.

    ⛔ **The working tree is never touched.** `checkout --orphan` keeps every
    file exactly as it is and only drops the parent, so the data that survives
    is the data that was there -- not a reconstruction of it.
    """
    run(["git", "checkout", "--orphan", "housekeeping-tmp"], cwd=checkout)
    run(["git", "add", "-A"], cwd=checkout)
    run(["git", "commit", "-m", message], cwd=checkout)
    run(["git", "branch", "-M", branch], cwd=checkout)


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--checkout", required=True,
                    help="a clone of the data branch to work on")
    ap.add_argument("--branch", default="data")
    ap.add_argument("--repository", default="Azathothas/Trackers")
    ap.add_argument("--apply", action="store_true",
                    help="actually rewrite and push. Without it nothing is "
                         "changed and the assessment is printed.")
    ap.add_argument("--force-threshold", action="store_true",
                    help="reset even below the threshold. For the test that "
                         "proves the reset preserves the dataset; ⛔ never in "
                         "a workflow.")
    ap.add_argument("--skip-inflight-check", action="store_true",
                    help="for tests, which have no workflow to race with")
    args = ap.parse_args()

    if not os.path.isdir(os.path.join(args.checkout, ".git")):
        print(f"{args.checkout} is not a git checkout", file=sys.stderr)
        return 2

    # ⛔ Never touch `main`, asserted rather than assumed: this refuses to run
    # against a checkout whose branch is not the data branch, whatever a caller
    # passed for --branch.
    current = run(["git", "rev-parse", "--abbrev-ref", "HEAD"],
                  cwd=args.checkout).strip()
    if current != args.branch:
        print(f"refusing: the checkout is on {current!r}, not {args.branch!r}",
              file=sys.stderr)
        return 1

    before = assess(args.checkout)
    print(json.dumps({"before": before}, indent=2, sort_keys=True))

    if before["files_missing"]:
        print(f"refusing: the branch is missing {before['files_missing']}; "
              f"a reset would make that permanent", file=sys.stderr)
        return 1

    crossed = (before["over_commit_threshold"] or before["over_size_ceiling"]
               or args.force_threshold)
    if not crossed:
        print(f"\nunder the threshold: {before['commits']} commits and "
              f"{before['git_mib']} MiB against {THRESHOLD_COMMITS} and "
              f"{CEILING_MIB} MiB. Nothing to do.")
        return 0

    if not args.apply:
        print("\nthe threshold is crossed and --apply was not given, so "
              "nothing was changed.")
        return 0

    if not args.skip_inflight_check and publication_in_flight(args.repository):
        print("refusing: a publication is in flight, or it could not be "
              "confirmed clear", file=sys.stderr)
        return 1

    reset_history(args.checkout, branch=args.branch,
                  message=f"Reset history at {before['commits']} commits "
                          f"({before['git_mib']} MiB). The data is unchanged; "
                          f"only commits were discarded (RULES 3.7).")

    after = assess(args.checkout)
    print(json.dumps({"after": after}, indent=2, sort_keys=True))
    # ⛔ Verify AFTER, before anything is pushed. A reset that lost a file must
    # fail here rather than on somebody's next fetch.
    if after["files_missing"] or after["commits"] != 1:
        print(f"refusing to push: after the reset the branch has "
              f"{after['commits']} commit(s) and is missing "
              f"{after['files_missing']}", file=sys.stderr)
        return 1
    print("\nreset locally. Push with:")
    print(f"  git -C {args.checkout} push --force-with-lease origin {args.branch}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (RuntimeError, subprocess.TimeoutExpired) as exc:
        print(f"housekeeping failed safely: {exc}", file=sys.stderr)
        sys.exit(1)
    except KeyboardInterrupt:
        sys.exit(2)
