#!/usr/bin/env python3
"""Gate: every captured corpus file is actually in the clone.

`references/` is the evidence. Every load-bearing claim in this project cites a
line in a file under it, and the promise `HISTORY/` makes is RULES 3.10's:
somebody who distrusts this project can re-run each claim from a fresh clone
without asking anyone. A corpus file that exists on the capturing agent's disk
but not in the clone breaks that promise **silently** -- the capture looks
complete to whoever made it, and is short 111 files for everybody else.

That is not hypothetical. It happened here, twice over, from two independent
causes (correction 23):

  * this repository's own `.gitignore` said `out/` rather than `/out/`, and an
    unanchored directory pattern matches at any depth -- so it reached into
    `references/Aseem0xff__pacman-static/tree/experiments/out/` and dropped 20
    files of somebody else's committed instrument output;
  * the captured upstream trees carried their own `.gitignore` files, and git
    honours a `.gitignore` anywhere in the work tree regardless of who wrote
    it -- `references/Azathothas__bit-cli/tree/.gitignore` (removed) dropped
    91 bench result files, among them the announce timings.

Neither showed up in `git status`: ignored files are ignored quietly. Both were
found by counting the disk against the index, which is the first check below.

The irony worth recording: this project had already written the lesson up.
`HISTORY/references/aseem0xff-pacman-static.md` documents that project's own
recovery failing because `git add -A` honoured a `.gitignore`. Knowing a defect
class is not a control for it. This script is the control.

What this does NOT check, so nobody mistakes a pass for more than it is:

  * **That the corpus exists.** No `references/` directory exits 0 here. The
    gate that catches a deleted corpus is `check-citations.py`, which fails on
    every unresolvable path -- verified by moving the directory away and
    watching it exit 1.
  * **That a capture matches its upstream commit.** That is
    `references/PROVENANCE.md`'s job, and the trims are listed there.
  * **That the content is worth having.** Check 3 refuses every `.gitignore`
    inside a capture, including one this project might genuinely want to cite
    as evidence about an upstream's build. Nothing cites one today. If
    something ever does, the answer is a recorded exemption naming that file,
    not a loosened check.

Exit codes:
    0  the corpus is whole
    1  a corpus file is ignored, or an ignore rule could catch one
    2  the check could not run (no git, or not a work tree)
"""

from __future__ import annotations

import os
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CORPUS = "references"


def git(*args: str) -> tuple[int, str]:
    """Run git in the repository. Returns (returncode, stdout)."""
    proc = subprocess.run(
        ("git", "-C", REPO) + args,
        capture_output=True, text=True,
    )
    return proc.returncode, proc.stdout


def main() -> int:
    rc, _ = git("rev-parse", "--is-inside-work-tree")
    if rc != 0:
        print("not a git work tree; cannot check corpus integrity",
              file=sys.stderr)
        return 2

    corpus_root = os.path.join(REPO, CORPUS)
    if not os.path.isdir(corpus_root):
        print(f"no {CORPUS}/ directory; nothing to check")
        return 0

    failures: list[str] = []

    # ---------------------------------------------------------------- 1
    # Present on disk but not in the index. Catches the defect at capture
    # time, on the machine that still has the files -- the only moment at
    # which recovery is free.
    _, out = git("ls-files", "--", CORPUS)
    tracked = {line for line in out.splitlines() if line}

    on_disk: set[str] = set()
    for dirpath, dirnames, filenames in os.walk(corpus_root):
        # `.git` inside a capture would mean the capture was not stripped;
        # `references/PROVENANCE.md` requires it to be. Reported, not walked.
        if ".git" in dirnames:
            failures.append(
                f"{os.path.relpath(os.path.join(dirpath, '.git'), REPO)}: a "
                "capture still carries its .git directory -- `git -C` inside "
                "it then answers about the WRONG repository, which has "
                "already produced three wrong SHAs in this project's history "
                "(references/PROVENANCE.md)")
            dirnames.remove(".git")
        for fn in filenames:
            full = os.path.join(dirpath, fn)
            on_disk.add(os.path.relpath(full, REPO).replace(os.sep, "/"))

    missing = sorted(on_disk - tracked)
    print(f"corpus files: {len(on_disk)} on disk, {len(tracked)} tracked")
    if missing:
        failures.append(
            f"{len(missing)} corpus file(s) exist on disk and in no clone")
        for path in missing[:20]:
            rc_i, why = git("check-ignore", "-v", "--", path)
            reason = why.strip() if rc_i == 0 else "untracked, not ignored"
            failures.append(f"    {path}\n        {reason}")
        if len(missing) > 20:
            failures.append(f"    ... and {len(missing) - 20} more")

    # ---------------------------------------------------------------- 2
    # An ignore rule that *would* catch a tracked corpus file. This is the
    # check that still works in CI: a fresh clone has no untracked files, so
    # check 1 passes there trivially, but the latent rule is still in the
    # tree waiting for the next `git add`.
    #
    # `--no-index` is the whole point: without it, git reports nothing for a
    # tracked path, because tracking wins over ignoring.
    if tracked:
        proc = subprocess.run(
            ("git", "-C", REPO, "check-ignore", "-v", "--no-index", "--stdin"),
            input="\n".join(sorted(tracked)),
            capture_output=True, text=True,
        )
        caught = [line for line in proc.stdout.splitlines() if line.strip()]
        if caught:
            failures.append(
                f"{len(caught)} tracked corpus file(s) are matched by an "
                "ignore rule. They survive only because they are already "
                "tracked; the next re-capture drops them.")
            for line in caught[:20]:
                failures.append(f"    {line}")
            if len(caught) > 20:
                failures.append(f"    ... and {len(caught) - 20} more")

    # ---------------------------------------------------------------- 3
    # No ignore file inside a capture. Redundant with check 2 today and kept
    # anyway, because it names the mechanism rather than the symptom: an
    # upstream `.gitignore` is written for upstream's build, has no authority
    # over what this project keeps as evidence, and is removed on capture
    # (recorded in `references/PROVENANCE.md`).
    strays = sorted(
        p for p in on_disk
        if os.path.basename(p) in {".gitignore", ".git", ".gitmodules"}
    )
    if strays:
        failures.append(
            "an ignore/submodule file inside a capture governs what this "
            "project can commit as evidence. Remove it and record the trim "
            "in references/PROVENANCE.md:")
        for path in strays:
            failures.append(f"    {path}")

    # ---------------------------------------------------------------- 4
    # An empty directory in the corpus. Git cannot carry one, so it exists on
    # the capturing agent's disk and in no clone -- the same divergence as an
    # ignored file, arriving by a different door. Trimming a directory to
    # nothing is fine; leaving the husk behind is what misleads, because an
    # auditor listing the capture locally sees a directory that no clone has.
    husks = sorted(
        os.path.relpath(dirpath, REPO).replace(os.sep, "/")
        for dirpath, dirnames, filenames in os.walk(corpus_root)
        if not dirnames and not filenames
    )
    if husks:
        failures.append(
            "empty director(ies) under the corpus. Git does not track an "
            "empty directory, so these exist on this disk and in no clone. "
            "Delete them; if the trim that emptied one is not yet recorded, "
            "record it in references/PROVENANCE.md:")
        for path in husks:
            failures.append(f"    {path}/")

    if failures:
        print("\nFAIL  the corpus is not whole:\n")
        for line in failures:
            print(f"  {line}")
        print(
            "\nRULES 3.10: a claim is auditable only if the evidence it cites "
            "is in the clone. A corpus file that exists on one disk and "
            "nowhere else is a citation to nothing.")
        return 1

    print(f"\nOK  every file under {CORPUS}/ is committed, and no ignore rule "
          "can reach one.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
