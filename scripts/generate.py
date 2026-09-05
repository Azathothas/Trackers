#!/usr/bin/env python3
"""Generate the published dataset. The P1 deliverable.

Runs the whole acquire -> validate -> normalize -> deduplicate -> render
pipeline and writes plaintext plus a run report.

Two properties this script exists to guarantee, both required by the P1 gate:

  * **It runs with no network.** `--offline` reads committed fixtures only, so
    the pipeline is testable without a live third party (RULES 2) and
    CI can exercise it without touching anyone's server.
  * **It is byte-identical across two runs over identical inputs.** The clock
    is injected via `--generated-at`; nothing in the pipeline reads it
    ambiently (RULES 3.6).

Atomicity (RULES 3.5): generate -> validate -> stage -> verify ->
publish. Output is written to a staging directory and only moved into place
once every check passes, so a failed generation can never leave partial public
data. `--check-only` stops before publishing.

Exit codes:
    0  generated and published
    1  generated and a verification check failed; previous output untouched
    2  could not run
"""

from __future__ import annotations

import argparse
import os
import shutil
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))

from trackers import NORMALIZATION_VERSION, __version__          # noqa: E402
from trackers.acquire import Outcome, fetch, read_cached          # noqa: E402
from trackers.exclusion import (carries_private_credential,       # noqa: E402
                                summarise)
from trackers.pipeline import (aggregate, collect_exclusions,      # noqa: E402
                               enforced_exclusions, flagged_exclusions,
                               render_plaintext, render_report)
from trackers.registry import SOURCES, Role, enabled_sources      # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")
DEFAULT_OUT = os.path.join(REPO, "out")


def verify(agg, plaintext: str, blacklist: set[str]) -> list[str]:
    """Pre-publication checks. RULES 3.5.

    Returns a list of problems; empty means publishable. These run against the
    STAGED output, before anything becomes public.
    """
    problems: list[str] = []

    if not agg.trackers:
        problems.append("dataset is empty; refusing to publish an empty list")

    lines = [ln for ln in plaintext.splitlines()]
    if len(lines) != len(agg.trackers):
        problems.append(f"plaintext has {len(lines)} lines but the dataset has "
                        f"{len(agg.trackers)} trackers (cross-format mismatch)")

    for ln in lines:
        if not ln or ln.startswith("#") or " " in ln or "://" not in ln:
            problems.append(f"plaintext contains a non-URL line: {ln[:80]!r}")
            break

    if len(set(lines)) != len(lines):
        problems.append("plaintext contains duplicate lines")

    leaked = sorted(set(lines) & blacklist)
    if leaked:
        problems.append(f"{len(leaked)} blacklisted URL(s) reached the output, "
                        f"e.g. {leaked[0]}")

    # T-107. The pipeline refuses these already; this is the guard that makes
    # a regression in that refusal a **failed publication** rather than a
    # published credential. RULES 3.5 makes the failure safe: nothing is moved
    # into place, so the previous output stands.
    #
    # ⚠ The count is named and the URL is not. A verification message that
    # printed the offending line would leak the credential into a build log,
    # which is the same mistake one file further along.
    creds = [ln for ln in lines if carries_private_credential(ln)]
    if creds:
        problems.append(
            f"{len(creds)} URL(s) carrying a private-tracker credential "
            f"reached the output; refusing to publish (T-107). The URLs are "
            f"deliberately not printed here.")

    # If EVERY source failed we have no evidence at all. Publishing then would
    # replace good data with the consequences of an outage (T-083:
    # "the correct behaviour under total measurement failure is to publish the
    # PREVIOUS data with a stale marker, not an empty or all-dead dataset").
    if not agg.sources_ok:
        problems.append("every source failed; refusing to publish")

    return problems


def load_corpus(offline: bool, fixtures: str):
    """Fetch every enabled source and aggregate it. The one assembly path.

    Returned rather than printed so both `generate.py` and
    `probe-corpus.py` build the corpus the same way. A second copy of this
    would acquire different defects, and the one nobody looks at would be the
    one publishing.
    """
    sources = {s.id: s for s in enabled_sources()}
    if not sources:
        raise _NoSources("no enabled sources")

    results = []
    bodies: dict[str, str] = {}
    for s in sorted(sources.values(), key=lambda x: x.id):
        if offline:
            path = os.path.join(fixtures, f"{s.id}.txt")
            if s.role is Role.BLACKLIST and os.path.exists(path):
                with open(path, encoding="utf-8", errors="replace") as fh:
                    bodies[s.id] = fh.read()
            results.append(read_cached(s, fixtures))
        else:
            results.append(fetch(s))

    exclusions = collect_exclusions(bodies)
    enforced = enforced_exclusions(exclusions)
    agg = aggregate(results, sources, exclude=enforced)
    return agg, exclusions, enforced


class _NoSources(RuntimeError):
    """The registry is empty. Exit 2: could not run."""


def display_path(path: str, start: str) -> str:
    """A path to print: relative to `start` where one exists, absolute where
    one does not.

    ⚠ `os.path.relpath` RAISES on Windows when the two paths are on different
    drives, because there is no relative path between them to compute. That is
    not hypothetical: on a GitHub Windows runner the checkout is on one drive
    and the scratch directory is on another, so generating with `--out` into
    scratch killed the run with `ValueError: path is on mount 'C:', start on
    mount 'D:'` after every check in the gate had already passed. Measured
    2026-08-31 on `windows-2025`.

    The last line a successful run prints is not worth failing a build for.
    `tests.test_p1.TestDisplayPathSurvivesTwoDrives` is the regression.
    """
    try:
        return os.path.relpath(path, start)
    except ValueError:
        return os.path.abspath(path)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--offline", action="store_true",
                    help="read committed fixtures only; touch no network")
    ap.add_argument("--fixtures", default=DEFAULT_FIXTURES)
    ap.add_argument("--out", default=DEFAULT_OUT)
    ap.add_argument("--generated-at", default="1970-01-01T00:00:00Z",
                    help="INJECTED clock. Deterministic by default so two runs "
                         "over identical inputs are byte-identical.")
    ap.add_argument("--check-only", action="store_true",
                    help="stage and verify, but do not publish")
    args = ap.parse_args()

    try:
        agg, exclusions, enforced = load_corpus(args.offline, args.fixtures)
    except _NoSources as exc:
        print(exc, file=sys.stderr)
        return 2
    flagged = flagged_exclusions(exclusions)

    plaintext = render_plaintext(agg.trackers)
    report = render_report(agg, generated_at=args.generated_at,
                           code_version=f"{__version__}+norm{NORMALIZATION_VERSION}")

    problems = verify(agg, plaintext, enforced)

    print(f"sources ok={len(agg.sources_ok)} failed={len(agg.sources_failed)} "
          f"rejected={len(agg.sources_rejected)} empty={len(agg.sources_empty)}")
    print(f"accepted trackers: {len(agg.trackers)}  "
          f"rejected lines: {len(agg.rejected)}  "
          f"dedup removed: {sum(1 for d in agg.decisions if d.acted)}")
    counts = summarise(exclusions)
    print(f"upstream exclusions: {counts} "
          f"-> enforced {len(enforced)} (operator request + safety), "
          f"kept-and-flagged {len(flagged)} (someone else's measurement)")
    print(f"entries refused and recorded with a reason: {len(agg.excluded)}")

    if problems:
        print("\nVERIFICATION FAILED -- nothing was published:")
        for p in problems:
            print(f"  - {p}")
        print("\nPrevious output, if any, is untouched (RULES 3.5).")
        return 1

    if args.check_only:
        print("\nverified; --check-only so nothing was written")
        return 0

    # stage -> verify -> publish. The staging directory is a sibling so the
    # final move is atomic on the same filesystem.
    staging = args.out + ".staging"
    if os.path.exists(staging):
        shutil.rmtree(staging)
    os.makedirs(staging, exist_ok=True)
    with open(os.path.join(staging, "trackers_all.txt"), "w",
              encoding="utf-8", newline="\n") as fh:
        fh.write(plaintext)
    with open(os.path.join(staging, "report.md"), "w",
              encoding="utf-8", newline="\n") as fh:
        fh.write(report)

    previous = args.out + ".previous"
    if os.path.exists(args.out):
        if os.path.exists(previous):
            shutil.rmtree(previous)
        os.replace(args.out, previous)
    os.replace(staging, args.out)

    print(f"\npublished -> {display_path(args.out, REPO)}/")
    return 0


if __name__ == "__main__":
    sys.exit(main())
