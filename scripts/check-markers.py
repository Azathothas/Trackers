#!/usr/bin/env python3
"""Gate: only the five defined characters, and not too many of them.

Two rules, one subject, one home (`docs/conventions/prose.md`).

The defect this exists to catch is a document set that reads as machine
output. An em dash costs nothing to type and nothing to read past, so a tree
accumulates them until the prose has a texture no human wrote: 1655 characters
outside the five were in this repository's own files on 2026-08-31, across 55
files, 840 of them em dashes. Nothing else in the gate could see any of it.

⚠ This check reported those as 1213 offending LINES, because it reports the
first offender per line. A line count and a character count are different
numbers and neither is a substitute for the other.

The second rule is the mirror of the first. An agent kept strictly to the
allowed characters and then spammed them until the documents were unreadable,
because the rule said which characters and never said how many. The ceiling is
30 markers per 100 non-blank lines, measured rather than chosen.

The exemptions, each of which would otherwise break something:

  - `references/` is out of scope entirely. `scripts/_scope.py` says why.
  - A LEADING byte-order mark is exempt, and only a leading one. A mark a
    merge left in the middle of a file is still a finding.
  - In markdown, a fenced block is skipped whole and an inline code span is
    cut out, so a page can name the character it bans. This file could not
    describe the check otherwise. Outside markdown there is no exemption.

Markers are counted BEFORE anything is stripped, because the density rule is
about what a reader sees on the page.

Usage:
    python3 scripts/check-markers.py [--json] [--ceiling N]

Exit codes:
    0  every file is inside the allowlist and under the ceiling
    1  a character outside the five, or a file over the ceiling
    2  the check could not run
"""

from __future__ import annotations

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _scope  # noqa: E402

# U+26D4 stop, U+2B50 star, U+26A0 warning, U+2705 pass, U+274C fail.
MARKERS = ("\u26d4", "\u2b50", "\u26a0", "\u2705", "\u274c")
BOM = "\ufeff"
DEFAULT_CEILING = 30

FENCE = re.compile(r"^[ \t]*```")
CODE_SPAN = re.compile(r"`[^`]*`")
# U+FE0F is the variation selector some editors append to an emoji. It carries
# no meaning of its own and is stripped with the marker it decorates, rather
# than reported as a sixth character nobody typed.
VARIATION = "\ufe0f"


def scan(rel: str, text: str, ceiling: int) -> tuple[list[str], int, int]:
    problems: list[str] = []
    is_md = rel.lower().endswith(".md")
    nmark = 0
    nonblank = 0
    fenced = False
    for n, line in enumerate(text.splitlines(), start=1):
        if n == 1 and line.startswith(BOM):
            line = line[len(BOM):]
        if line.strip():
            nonblank += 1
        for m in MARKERS:
            nmark += line.count(m)
        stripped = line
        for m in MARKERS:
            stripped = stripped.replace(m + VARIATION, "").replace(m, "")
        if is_md:
            if FENCE.match(line):
                fenced = not fenced
                continue
            if fenced:
                continue
            stripped = CODE_SPAN.sub("", stripped)
        for ch in stripped:
            if ord(ch) < 128:
                continue
            problems.append(
                f"{rel}:{n} U+{ord(ch):04X} is outside the five. "
                f"docs/conventions/prose.md")
            break
    density = (nmark * 100) // max(nonblank, 1)
    if density > ceiling:
        problems.append(
            f"{rel} {nmark} markers in {nonblank} non-blank lines, {density} "
            f"per 100. The ceiling is {ceiling}. docs/conventions/prose.md")
    return problems, nmark, density


def main(argv: list[str]) -> int:
    json_mode = "--json" in argv
    ceiling = DEFAULT_CEILING
    if "--ceiling" in argv:
        ceiling = int(argv[argv.index("--ceiling") + 1])

    files = _scope.repo_files()
    if not files:
        raise _scope.CouldNotRun("no text files in scope")

    report: list[str] = []
    total = 0
    worst = 0
    worst_file = "-"
    for rel in files:
        found, nmark, density = scan(rel, _scope.read(rel), ceiling)
        report.extend(found)
        total += nmark
        if density > worst:
            worst, worst_file = density, rel

    return _scope.emit(
        json_mode, "check-markers/1", len(report), report,
        f"markers ok: {len(files)} files, {total} markers, densest {worst} per "
        f"100 non-blank lines ({worst_file}), ceiling {ceiling}",
        tail=("The five are the three prose markers and the two status "
              "glyphs.\nEverything else is ASCII. docs/conventions/prose.md "
              "is the rule."),
        files=len(files), markers=total, ceiling=ceiling, worst_density=worst)


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except _scope.CouldNotRun as exc:
        print(f"check-markers: {exc}", file=sys.stderr)
        sys.exit(2)
