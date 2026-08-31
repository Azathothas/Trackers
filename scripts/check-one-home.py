#!/usr/bin/env python3
"""Gate: no sentence of 12 or more words appears in two documents.

One fact, one home (`docs/conventions/prose.md`, RULES 17.2). A value in two
documents with no check between them drifts, and the copy a reader trusts is
the wrong one. The rule was written down long before anything checked it, and
an unchecked rule drifts the way every unchecked rule does.

It compares SENTENCES, so a fact restated in different words passes here and
fails a review. That is the same split every prose rule has: the mechanical
half is mechanical so the reading is spent on the half that needs it.

Three exemptions, each narrow:

  - `HISTORY/` is exempt entirely. A superseded page states things the live
    pages now state differently, which is the point of that directory
    (`HISTORY/README.md`). It is exempt from THIS rule and no other.
  - `docs/AGENTS.md` and `README.md` are exempt FROM EACH OTHER only. Each is
    an entry point that may be the one document a reader is handed. A sentence
    shared between one of them and any other file is still refused, so the
    exemption cannot be used to seed a copy into the tree.
  - `references/` is out of scope, per `scripts/_scope.py`.

Headings, table rows, fenced blocks and code spans are excluded. A shared
heading is not a duplicated fact, and a shared table row usually is not either.

The scope is asserted before the verdict: a run over fewer than two files is
exit 2, not a clean pass. The first version of this instrument elsewhere
reported zero over a scope it had never opened.

Usage:
    python3 scripts/check-one-home.py [--json] [--min-words N]

Exit codes:
    0  no sentence of MIN+ words in two documents
    1  at least one
    2  the check could not run
"""

from __future__ import annotations

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _scope  # noqa: E402

MIN_WORDS = 12
ROUTERS = frozenset({"docs/AGENTS.md", "README.md"})
EXEMPT_PREFIX = ("HISTORY/",)

FENCE = re.compile(r"^[ \t]*```")
CODE_SPAN = re.compile(r"`[^`]*`")
LINK_TARGET = re.compile(r"\]\([^)]*\)")
SENTENCE_SPLIT = re.compile(r"[.:!?]+[ \t]+")
NON_WORD = re.compile(r"[^a-z0-9 ]+")


def sentences(text: str) -> list[str]:
    buf: list[str] = []
    fenced = False
    for line in text.splitlines():
        if FENCE.match(line):
            fenced = not fenced
            continue
        if fenced:
            continue
        stripped = line.lstrip()
        if stripped.startswith("|") or stripped.startswith("#"):
            continue
        line = CODE_SPAN.sub(" ", line)
        line = LINK_TARGET.sub(" ", line).replace("[", " ")
        buf.append(line)
    out = []
    for part in SENTENCE_SPLIT.split(" ".join(buf)):
        s = " ".join(NON_WORD.sub(" ", part.lower()).split())
        if s and len(s.split()) >= MIN_WORDS:
            out.append(s)
    return out


def main(argv: list[str]) -> int:
    json_mode = "--json" in argv
    global MIN_WORDS
    if "--min-words" in argv:
        MIN_WORDS = int(argv[argv.index("--min-words") + 1])

    files = [f for f in _scope.repo_files()
             if f.endswith(".md") and not f.startswith(EXEMPT_PREFIX)]
    if len(files) < 2:
        raise _scope.CouldNotRun(
            f"only {len(files)} document(s) in scope; nothing to compare")

    seen: dict[str, set[str]] = {}
    for rel in files:
        for s in sentences(_scope.read(rel)):
            seen.setdefault(s, set()).add(rel)

    report: list[str] = []
    for s in sorted(seen):
        where = seen[s]
        if len(where) < 2 or where <= ROUTERS:
            continue
        report.append(f'"{s[:88]}"')
        report.extend(f"    {w}" for w in sorted(where))
        report.append("")

    problems = sum(1 for line in report if line.startswith('"'))
    return _scope.emit(
        json_mode, "check-one-home/1", problems, report,
        f"one fact one home: {len(files)} documents, no sentence of "
        f"{MIN_WORDS}+ words in two of them",
        tail=('Keep the fact in the document that owns it and make the other '
              'a pointer.\ndocs/conventions/prose.md, "one fact, one home".'),
        files=len(files), min_words=MIN_WORDS)


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except _scope.CouldNotRun as exc:
        print(f"check-one-home: {exc}", file=sys.stderr)
        sys.exit(2)
