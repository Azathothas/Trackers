#!/usr/bin/env python3
"""Gate: no literal control byte in any tracked text file.

The defect this exists to catch makes a file invisible to both review tools at
once. `grep` decides the file is binary and skips it, saying so on a line
nobody reads, and `git diff` prints "Binary files differ", so a code review
shows no diff at all. The runtime value is identical either way, which is
exactly why it survives unnoticed: only reviewability is ever at stake.

Write the ESCAPE, not the byte. `\x1b` in a source file is the same character
at runtime and stays reviewable.

Scope is every text file this project owns, not markdown alone. Binaries are
out of scope by construction rather than by an allowlist: an allowlist of
"binaries that are fine" is the kind of list that quietly absorbs a real
finding. `references/` is exempt for the reason `scripts/_scope.py` gives.

NUL is tested separately from the other C0 bytes because it is the commonest
offender by a distance: it is what somebody reaches for as a key separator.

Usage:
    python3 scripts/check-control-bytes.py [--json]

Exit codes:
    0  no literal control byte
    1  at least one
    2  the check could not run
"""

from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _scope  # noqa: E402

# C0 controls except the three that are legitimately in text: tab (0x09),
# newline (0x0a) and carriage return (0x0d). NUL is in the set and is reported
# under its own name because it is the one people write on purpose.
CONTROL = {c for c in range(0x00, 0x20)} - {0x09, 0x0a, 0x0d}


def main(argv: list[str]) -> int:
    json_mode = "--json" in argv
    files = _scope.repo_files()
    if not files:
        raise _scope.CouldNotRun("no text files in scope")

    report: list[str] = []
    for rel in files:
        with open(os.path.join(_scope.REPO, rel), "rb") as fh:
            raw = fh.read()
        if b"\x00" in raw:
            report.append(f"{rel} a NUL byte")
            continue
        for n, line in enumerate(raw.split(b"\n"), start=1):
            hit = next((b for b in line if b in CONTROL), None)
            if hit is not None:
                report.append(f"{rel}:{n} a C0 control byte, 0x{hit:02X}")
                break

    return _scope.emit(
        json_mode, "check-control-bytes/1", len(report), report,
        f"no literal control bytes in {len(files)} text files "
        f"(tracked plus untracked-not-ignored)",
        tail=("Write the ESCAPE, not the byte. The escape is the same "
              "character at runtime,\nand the byte is what makes the file "
              "invisible to grep and unreviewable\nin git diff. "
              "docs/conventions/shell.md section 6."),
        files=len(files))


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except _scope.CouldNotRun as exc:
        print(f"check-control-bytes: {exc}", file=sys.stderr)
        sys.exit(2)
