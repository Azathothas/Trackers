#!/usr/bin/env python3
"""Gate: the documents are written the way this repository writes documents.

WHAT THIS OWNS, AND WHAT IT DELIBERATELY DOES NOT
    Two checks enforcing one rule is two places for it to be wrong. So:

      - link and path resolution belong to `scripts/check-citations.py`,
        which resolves markdown links, backticked paths, `path:NN` line
        citations, rule ids, claim ids, entry ids and decision ids, and which
        also refuses a cited EMPTY directory. This file does not look at links
        except to work out which pages nothing points at.
      - the five-character allowlist and the marker density ceiling belong to
        `scripts/check-markers.py`, over every tracked text file rather than
        markdown alone.
      - literal control bytes belong to `scripts/check-control-bytes.py`, over
        every tracked text file for the same reason.

    What is left is this file's, and it is three rules:

    1. EVERY FENCED SHELL BLOCK PARSES. A block that does not parse is a block
       nobody can copy and paste, and the reader finds out by running it.
    2. NO ANGLE-BRACKET PLACEHOLDER INSIDE A SHELL BLOCK. A human reads
       `<deployment-id>` as "fill this in" and a shell reads it as a redirect,
       so the reader gets a cryptic syntax error instead of an obvious
       instruction. Use an upper-case name or a quoted variable.
    3. NONE OF THE BANNED VOCABULARY. Words that assert quality instead of
       demonstrating it survive review because they feel like description.
       `docs/conventions/prose.md` carries the list and the reason.

    A fourth rule has no natural home anywhere else: A PAGE NOTHING LINKS TO
    IS A FINDING. Unlinked means unread, which means uncorrected, which is the
    state every stale document passes through on the way to being wrong. Files
    at the repository root and any `README.md` are exempt: those are entry
    points a reader or a raw URL arrives at directly.

WHAT THE PARSE CHECK CAN AND CANNOT SEE
    It is `shlex` in POSIX mode plus a heredoc scan, not a shell. It catches an
    unterminated quote, an unterminated heredoc and unbalanced brackets, which
    is every way a block in this repository's documents has actually been
    wrong. It does not catch a grammatical error a shell would refuse. That is
    an honest scope rather than a claim to be a parser, and it is the scope
    that runs identically on Windows, where `sh` is not guaranteed
    (RULES 15.5).

Usage:
    python3 scripts/check-docs.py [--json]

Exit codes:
    0  every document passes
    1  at least one problem
    2  the check could not run
"""

from __future__ import annotations

import os
import re
import shlex
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _scope  # noqa: E402

SHELL_FENCE = re.compile(r"^[ \t]*```(bash|sh|shell)[ \t]*$")
ANY_FENCE = re.compile(r"^[ \t]*```")
PLACEHOLDER = re.compile(r"<[a-z][a-z0-9-]*>")
HEREDOC = re.compile(r"""<<-?\s*['"]?([A-Za-z_][A-Za-z0-9_]*)['"]?""")
LINK = re.compile(r"\]\(([^)\s]+)\)")
CODE_SPAN = re.compile(r"`[^`]*`")

# Words that assert quality instead of demonstrating it.
# `docs/conventions/prose.md` is the rule and the reason.
# "leverage" is deliberately NOT here. It was in a first draft of this list and
# came out: this project's own highest-value entry is named "the leverage
# entry", meaning mechanical advantage rather than marketing, and a rule that
# refuses a term of art is a rule somebody switches off.
BANNED = (
    "seamless", "seamlessly", "blazing", "blazingly", "effortless",
    "effortlessly", "robust", "powerful", "cutting-edge", "state-of-the-art",
    "world-class", "elegant", "elegantly", "simply", "obviously",
    "of course", "revolutionary", "game-changing", "rock-solid",
    "bulletproof", "lightning-fast",
)
BANNED_RE = re.compile(r"\b(" + "|".join(BANNED) + r")\b", re.IGNORECASE)

# "just" is matched only where it is doing the damaging job, which is telling a
# reader who is stuck that the thing they cannot do is easy. "just as", "just
# over", "just under", "just before" and "just after" are comparisons.
JUST_RE = re.compile(r"\bjust\b(?!\s+(?:as|over|under|before|after|about))",
                     re.IGNORECASE)


def shell_blocks(text):
    """Every fenced shell block, as (line number of the opening fence, body)."""
    out = []
    start = 0
    body = []
    inside = False
    for n, line in enumerate(text.splitlines(), start=1):
        if inside:
            if ANY_FENCE.match(line):
                out.append((start, "\n".join(body)))
                inside = False
                body = []
            else:
                body.append(line)
        elif SHELL_FENCE.match(line):
            inside, start, body = True, n, []
    if inside:
        out.append((start, "\n".join(body)))
    return out


def parse_problem(block):
    """Why this shell block would not survive being pasted, or None."""
    text = block.replace("\r", "")
    open_docs = []
    for line in text.splitlines():
        if open_docs:
            if line.strip() == open_docs[0]:
                open_docs.pop(0)
            continue
        for m in HEREDOC.finditer(line):
            open_docs.append(m.group(1))
    if open_docs:
        return "unterminated heredoc, expected " + open_docs[0]
    try:
        shlex.split(text, comments=True, posix=True)
    except ValueError as exc:
        return str(exc)
    depth = {"(": 0, "{": 0}
    pairs = {")": "(", "}": "{"}
    for ch in re.sub(r"'[^']*'|\"[^\"]*\"", "", text):
        if ch in depth:
            depth[ch] += 1
        elif ch in pairs:
            depth[pairs[ch]] -= 1
    for opener, n in depth.items():
        if n < 0:
            return "unbalanced " + opener
    return None


def prose_lines(text):
    """Lines outside fenced blocks, with code spans removed."""
    out = []
    fenced = False
    for n, line in enumerate(text.splitlines(), start=1):
        if ANY_FENCE.match(line):
            fenced = not fenced
            continue
        if fenced:
            continue
        out.append((n, CODE_SPAN.sub(" ", line)))
    return out


def linked_targets(files):
    """Every repository-relative path any document links to."""
    seen = set()
    for rel in files:
        base = os.path.dirname(rel)
        for target in LINK.findall(_scope.read(rel)):
            if target.startswith(("http://", "https://", "mailto:", "#")):
                continue
            target = target.split("#")[0]
            if not target:
                continue
            joined = os.path.normpath(os.path.join(base, target))
            seen.add(joined.replace(os.sep, "/"))
    return seen


def main(argv):
    json_mode = "--json" in argv
    files = [f for f in _scope.repo_files() if f.endswith(".md")]
    if not files:
        raise _scope.CouldNotRun("no markdown files in scope")

    report = []
    nblocks = 0
    for rel in files:
        text = _scope.read(rel)
        for start, block in shell_blocks(text):
            nblocks += 1
            why = parse_problem(block)
            if why:
                report.append(
                    "%s:%d shell block does not parse: %s" % (rel, start, why))
            if PLACEHOLDER.search(block):
                report.append(
                    "%s:%d shell-unsafe placeholder. A shell reads it as a "
                    "redirect; use UPPER_SNAKE or a quoted variable"
                    % (rel, start))
        for n, line in prose_lines(text):
            for m in list(BANNED_RE.finditer(line)) + list(JUST_RE.finditer(line)):
                report.append(
                    "%s:%d banned vocabulary: %r. docs/conventions/prose.md"
                    % (rel, n, m.group(0)))

    linked = linked_targets(files)
    for rel in files:
        if "/" not in rel or rel.endswith("README.md"):
            continue
        if rel not in linked:
            report.append(
                "%s is linked from nowhere. An unlinked page is not read, so "
                "it is not corrected." % rel)

    return _scope.emit(
        json_mode, "check-docs/1", len(report), report,
        "docs ok: %d documents, %d shell blocks, vocabulary clean, "
        "every page linked" % (len(files), nblocks),
        files=len(files), shell_blocks=nblocks)


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except _scope.CouldNotRun as exc:
        print("check-docs: %s" % exc, file=sys.stderr)
        sys.exit(2)
