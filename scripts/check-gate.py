#!/usr/bin/env python3
"""Run the whole local gate with one command.

THE DEFECT THIS EXISTS TO CATCH
    Part (a) of `docs/methodology/gate.md` is a LIST, and a list run by hand is
    run in the order somebody recalls it. The check that gets forgotten is the
    one added last, which is also the one nobody has seen fail yet.

WHAT IT HOLDS, AND WHAT IT DELEGATES
    It holds no rules of its own. Every verdict below comes from the check that
    owns it, and the only thing this file adds is that all of them ran.

    An exit code is read from the process that produced it, with no pipe. A
    guard on the left of a pipe reports the pipeline's status, so one that
    failed reads as green.

    A SKIPPED CHECK IS REPORTED AS A SKIP, NEVER AS A PASS. A check that could
    not run means nothing about its subject was verified, which is the
    opposite of a pass. `--strict` turns a skip into a failure, which is what
    CI should pass, because there the environment is built on purpose and a
    skip means the build broke.

    ZERO PASSES IS RED WHATEVER THE SKIPS SAY. A runner that found nothing to
    run and printed a green verdict over nothing at all is the forbidden
    pattern about a step that exits 0 having done nothing.

WHAT IS DELIBERATELY NOT HERE
    `scripts/check-vantage-metadata.py` exits 2 by design until health records
    exist, which is a correct answer rather than a broken check. It runs here
    and is reported as a skip with its reason, and `--strict` does NOT fail on
    it, because a check that is right to be unable to run is not a broken
    environment. It is listed with `expect_skip`, and when P2 lands and it
    starts exiting 0, that flag comes off.

Usage:
    python3 scripts/check-gate.py [--strict] [--fast] [--json]

    --fast   drop the two slowest members: the test suite and the offline
             end-to-end generation. For an edit-and-recheck loop, never for a
             verdict on whether the tree is green.

Exit codes:
    0  every check that ran passed, and at least one ran
    1  a check failed, or --strict and something skipped
    2  the gate itself could not run
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _scope  # noqa: E402

PY = sys.executable or "python3"

# name, argv, slow, expect_skip
CHECKS = (
    ("check-todo", [PY, "scripts/check-todo.py"], False, False),
    ("check-citations", [PY, "scripts/check-citations.py"], False, False),
    ("check-corpus-integrity", [PY, "scripts/check-corpus-integrity.py"], False, False),
    ("check-decision-record", [PY, "scripts/check-decision-record.py"], False, False),
    ("check-no-third-party-imports",
     [PY, "scripts/check-no-third-party-imports.py"], False, False),
    ("check-docs", [PY, "scripts/check-docs.py"], False, False),
    ("check-markers", [PY, "scripts/check-markers.py"], False, False),
    ("check-control-bytes", [PY, "scripts/check-control-bytes.py"], False, False),
    ("check-one-home", [PY, "scripts/check-one-home.py"], False, False),
    ("check-no-secrets", [PY, "scripts/check-no-secrets.py", "--public"], False, False),
    ("check-vendor-pin", [PY, "scripts/check-vendor-pin.py"], False, False),
    ("check-vantage-metadata",
     [PY, "scripts/check-vantage-metadata.py"], False, True),
    ("tests", [PY, "-m", "unittest", "discover", "-s", "tests"], True, False),
    # --out into scratch. Without it the census writes a timestamped result
    # into `experiments/results/` on every gate run, so running the gate
    # dirties the tree and RULES 10.3 step 6 can never be satisfied. A result
    # is committed when a session decides to keep it, not as a side effect of
    # a check.
    ("offline-census",
     [PY, "experiments/19-scheme-census.py", "--offline",
      "--expect-known-schemes", "--out", "{OUT}/census.json"], True, False),
)

PASS, FAIL, SKIP = "✅", "❌", "-"


def run(argv: list[str], out_dir: str) -> tuple[int, str]:
    """Run one check from the repository root and return (code, last line)."""
    argv = [a.replace("{OUT}", out_dir) for a in argv]
    proc = subprocess.run(argv, cwd=_scope.REPO, capture_output=True,
                          text=True, encoding="utf-8", errors="replace")
    body = (proc.stdout or "") + (proc.stderr or "")
    lines = [ln.strip() for ln in body.splitlines() if ln.strip()]
    return proc.returncode, (lines[-1] if lines else "")


def main(argv: list[str]) -> int:
    strict = "--strict" in argv
    fast = "--fast" in argv
    json_mode = "--json" in argv

    rows = []
    with tempfile.TemporaryDirectory(prefix="trackers-gate-") as out_dir:
        checks = list(CHECKS)
        if not fast:
            # The end-to-end run writes, so it gets a scratch directory rather
            # than the tree. RULES 15.5: no /tmp, no absolute path.
            checks.append(("offline-generate",
                           [PY, "scripts/generate.py", "--offline", "--out",
                            "{OUT}"], True, False))
        for name, cmd, slow, expect_skip in checks:
            if fast and slow:
                rows.append((name, "skipped", SKIP, "--fast"))
                continue
            code, last = run(cmd, out_dir)
            if code == 0:
                rows.append((name, "pass", PASS, last))
            elif code == 2:
                rows.append((name, "expected-skip" if expect_skip else "skip",
                             SKIP, last))
            else:
                rows.append((name, "fail", FAIL, last))

    passes = sum(1 for r in rows if r[1] == "pass")
    fails = sum(1 for r in rows if r[1] == "fail")
    skips = sum(1 for r in rows if r[1] in ("skip", "skipped"))
    expected = sum(1 for r in rows if r[1] == "expected-skip")

    if json_mode:
        print('{"schema":"check-gate/1","pass":%d,"fail":%d,"skip":%d,'
              '"expected_skip":%d,"strict":%s}'
              % (passes, fails, skips, expected,
                 "true" if strict else "false"))
    else:
        width = max(len(r[0]) for r in rows)
        for name, state, glyph, detail in rows:
            print("%s %-*s  %s" % (glyph, width, name, detail[:96]))
        print("\n%d passed, %d failed, %d skipped, %d skipped as expected%s"
              % (passes, fails, skips, expected,
                 ", strict" if strict else ""))

    if passes == 0:
        print("\nzero checks passed. A green verdict over nothing is not a "
              "verdict.", file=sys.stderr)
        return 1
    if fails:
        return 1
    if strict and any(r[1] == "skip" for r in rows):
        print("\n--strict: a skip is a failure here. Nothing about that "
              "check's subject was verified.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except _scope.CouldNotRun as exc:
        print("check-gate: %s" % exc, file=sys.stderr)
        sys.exit(2)
