#!/usr/bin/env python3
"""Shared file scoping for the checks in this directory.

The defect this exists to prevent: a guard whose scope depends on who called
it. `git ls-files` is relative to the process working directory, so a check
invoked from a subdirectory silently reports on a smaller tree and calls it
clean. Every collector here resolves the repository root from this file's own
location and asks git from there.

Two scope rules, both load-bearing:

TRACKED PLUS UNTRACKED-BUT-NOT-IGNORED, not tracked alone. A file that has
never been staged is exactly when a new file is likeliest to carry a defect,
and it is what the next `git add -A` would take.

THE CAPTURED UPSTREAM TREES ARE EXEMPT FROM EVERY CONTENT RULE. They are
byte-exact at the commits `references/PROVENANCE.md` records, `.gitattributes`
marks them `-text` so nothing normalises them, and a check that asked anybody
to edit one would be asking for a corruption of the evidence, after which
`scripts/check-corpus-integrity.py` would report the tree short. The exemption
is content-only: links and cited line numbers still resolve into them, which is
`scripts/check-citations.py`'s job.

⚠ THE EXEMPTION IS PER CAPTURED REPOSITORY, NOT THE WHOLE DIRECTORY, and that
distinction was paid for. Exempting `references/` wholesale also exempted
`references/PROVENANCE.md`, which is THIS project's own writing about the
corpus rather than anybody else's source. It sat outside the prose rules until
a claim audit noticed it still carried the characters the rest of the tree had
been cleared of.

This module is project-local, not a dependency (RULES 12, decision D1).
"""

from __future__ import annotations

import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Output is UTF-8 on every host, never the platform default. On Windows the
# console default is a legacy code page, and a check whose report contains a
# status glyph or a codepoint it is complaining about prints replacement
# characters there. That is not cosmetic: a report a reader cannot read is a
# report, and the reader concludes the check is broken.
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8")
    except (AttributeError, OSError):  # a stream that is not a text file
        pass

#: A captured upstream tree: `references/<owner>__<repo>/...`. Matched by
#: shape rather than by a list, so a sweep that adds an eleventh reference
#: needs no edit here and cannot be forgotten.
CAPTURED_TREE = re.compile(r"^references/[^/]+__[^/]+/")

# Extensions asserted to be text. Anything else is out of scope by
# construction. An allowlist of "binaries that are fine" is the kind of list
# that quietly absorbs a real finding.
TEXT_EXT = (
    ".md", ".txt", ".tsv", ".csv", ".json", ".yml", ".yaml", ".toml",
    ".py", ".sh", ".ps1", ".cfg", ".ini", ".conf", ".html", ".css", ".js",
)


class CouldNotRun(Exception):
    """Raised where the check cannot run at all, which is exit 2, not exit 1."""


def _git(*args: str) -> list[str]:
    try:
        out = subprocess.run(
            ("git", "-C", REPO, *args),
            capture_output=True, text=True, encoding="utf-8", check=False,
        )
    except OSError as exc:
        raise CouldNotRun(f"git could not be run: {exc}") from exc
    if out.returncode != 0:
        raise CouldNotRun(f"git {' '.join(args)} failed: {out.stderr.strip()}")
    return [line for line in out.stdout.splitlines() if line]


def is_exempt(rel: str) -> bool:
    """Is this path exempt from content rules."""
    return bool(CAPTURED_TREE.match(rel))


def repo_files(*, text_only: bool = True, include_exempt: bool = False) -> list[str]:
    """Repository-relative paths, tracked plus untracked-but-not-ignored.

    Sorted, deduplicated, and every one exists on disk: git reports a tracked
    file that has been deleted, and opening it would be an error the check did
    not mean to report.
    """
    if not os.path.isdir(os.path.join(REPO, ".git")):
        raise CouldNotRun("not a git repository")
    names = set(_git("ls-files"))
    names |= set(_git("ls-files", "--others", "--exclude-standard"))
    out = []
    for rel in sorted(names):
        if text_only and not rel.lower().endswith(TEXT_EXT):
            continue
        if not include_exempt and is_exempt(rel):
            continue
        if not os.path.isfile(os.path.join(REPO, rel)):
            continue
        out.append(rel)
    return out


def read(rel: str) -> str:
    """Read a repository-relative file as UTF-8.

    Explicit encoding, never the platform default, which is not UTF-8 on
    Windows (RULES 15.5). `newline=""` so a CRLF file is reported as it is on
    disk rather than silently translated.
    """
    with open(os.path.join(REPO, rel), encoding="utf-8", errors="replace",
              newline="") as fh:
        return fh.read()


def emit(json_mode: bool, schema: str, problems: int, report: list[str],
         ok_line: str, tail: str = "", **extra: object) -> int:
    """Print a check's verdict and return its exit code.

    Exit 0 pass, 1 fail. Exit 2 is the caller's, raised as CouldNotRun, because
    "the check failed" and "the check could not run" mean opposite things about
    whether you can ship.
    """
    if json_mode:
        fields = "".join(f',"{k}":{v!r}'.replace("'", '"') for k, v in extra.items())
        sys.stdout.write(
            '{"schema":"%s","problems":%d%s}\n' % (schema, problems, fields))
        return 1 if problems else 0
    if problems:
        print(f"{schema.split('/')[0]} failed, {problems} problem(s):\n")
        for line in report:
            print(f"  {line}")
        if tail:
            print(f"\n{tail}")
        return 1
    print(ok_line)
    return 0
