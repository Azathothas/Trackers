#!/usr/bin/env python3
"""Do this repository's citations still resolve?

THE QUESTION THIS ANSWERS
    A document that cites a file, a rule, a claim, an entry or a decision is
    making a promise that the reader can follow it. This checks every one of
    those promises mechanically, because the failure mode is silent: a
    citation does not stop resolving loudly, it stops resolving the next time
    somebody renames a file, and the reader who follows it concludes the
    project is careless about everything else too.

WHAT IT CHECKS
    1. no reference survives to a document that is not in the tree
       (`IDEA.md`, `IDEA.rev1.md`, `PROMPT.md` were retired -- their content
       lives in TODO/, HISTORY/ and docs/; see HISTORY/idea-coverage.md)
    2. every relative markdown link resolves to a real path
    3. every `RULES <n>[.<n>]` names a section heading that exists
    4. every `C-nn` has a row in HISTORY/claims.md
    5. every `T-nnn` has a row in TODO/INDEX.md
    6. every `D<n>` has a row in HISTORY/decisions.md
    7. every backticked path that looks like a repo path exists
    8. every `path:NN` line citation names a line the file actually has
    9. every load-bearing citation still says what it is cited for --
       `experiments/fixtures/load-bearing-citations.tsv` pins the substring
   10. no retired corpus figure is quoted anywhere (RULES 2.1)
   11. every stated test count matches the suite. This one has drifted twice:
       the documents said 48 when there were 101, and 103 when there were 118
   12. no cited directory is empty. Git does not track an empty directory, so
       one that exists on the author's disk does not exist in a fresh clone --
       and a link to it passes locally and fails in CI. That happened here on
       2026-08-31, six times, while the commit messages said the gates were
       green

EXIT CODES
    0  every citation resolves
    1  at least one does not
    2  could not run

Standard library only (D1). Runs from any directory, on any host (RULES 15.5).
"""

from __future__ import annotations

import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

#: Trees we do not own. Their citations are theirs, not ours.
SKIP_DIRS = {".git", ".tmp", "out", "out.staging", "out.previous",
             "__pycache__", ".venv", "node_modules"}

#: `references/<owner>__<repo>/tree/` is somebody else's source, checked in as
#: evidence. `issues.json` is a verbatim API capture. Neither is ours to fix.
def _skip_path(rel: str) -> bool:
    parts = rel.split(os.sep)
    if any(p in SKIP_DIRS for p in parts):
        return True
    if rel.startswith("references" + os.sep):
        # keep PROVENANCE.md; skip captured trees and API dumps
        return "tree" in parts or rel.endswith(".json")
    if rel.startswith("experiments" + os.sep + "results"):
        return True          # committed evidence: never rewritten (RULES 2)
    if rel.startswith("experiments" + os.sep + "fixtures"):
        return True
    if rel.startswith("tests" + os.sep + "fixtures"):
        return True
    return False


TEXT_SUFFIXES = {".md", ".py", ".yml", ".yaml", ".txt", ".toml", ".cfg"}

#: The retired design documents. HISTORY/idea-coverage.md is the one file
#: allowed to name them, because its whole job is to record where they went.
RETIRED_DOCS = re.compile(r"\bIDEA(?:\.rev1)?(?:\.md|\s*section)|\bPROMPT\.md|\bPROMPT\s*section")

#: A bare `section 8.3` resolves to nothing: the document those numbers indexed is not
#: in the tree. The one legitimate use is provenance -- an entry's `Source:`
#: line saying which section of the retired brief an item came from -- and that
#: has to say so, in the form `the brief's section 8.3`, so a reader knows not to go
#: looking for a section 8.3 here.
BARE_SECTION = re.compile(r"(?<!brief's )(?<!RFC 3986 )(?<!RFC 4343 )section\d")
BARE_SECTION_ALLOWED = {
    os.path.join("HISTORY", "idea-coverage.md"),
    os.path.join("HISTORY", "corrections.md"),
    os.path.join("scripts", "check-citations.py"),
}
RETIRED_DOCS_ALLOWED = {
    os.path.join("HISTORY", "idea-coverage.md"),
    os.path.join("HISTORY", "corrections.md"),
    os.path.join("scripts", "check-citations.py"),
}

#: Figures that were in circulation without an instrument behind them
#: (RULES 2.1). Quoting one again is the regression this guards.
RETIRED_FIGURES = {
    "1510": "distinct URLs -- the census reports 1346",
    "946": "http URLs -- the census reports 723",
    "457": "udp URLs -- the census reports 362",
    "448": "udp URLs -- the census reports 362",
    "780": "http URLs -- the census reports 723",
}
#: The three files whose job is to record what was wrong. Quoting a retired
#: figure there is the point; quoting it anywhere else is the regression.
RETIRED_FIGURES_ALLOWED = {
    os.path.join("HISTORY", "corpus-baseline.md"),
    os.path.join("HISTORY", "corrections.md"),
    os.path.join("TODO", "RULES.md"),
    os.path.join("scripts", "check-citations.py"),
}

MD_LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
RULES_REF = re.compile(r"\bRULES\s+(\d+(?:\.\d+[a-z]?)?)")
CLAIM_REF = re.compile(r"\bC-(\d+[a-z]?)\b")
#: `T-244-RESEARCH` is a repository name, not an entry id, so a trailing
#: hyphen disqualifies the match.
ENTRY_REF = re.compile(r"\bT-(\d{3})\b(?!-)")
DECISION_REF = re.compile(r"\bD(\d+)\b")
BACKTICK = re.compile(r"`([^`\n]+)`")
#: The same span, matched for removal rather than for capture. Used only by the
#: markdown-link rule, which must not see inside a code span.
CODE_SPAN = re.compile(r"`[^`\n]*`")

#: A backticked token is treated as a repo path only if it looks like one:
#: it contains a slash or a known suffix, and starts with a tracked top level.
TOP_LEVEL = ("src/", "tests/", "scripts/", "experiments/", "docs/", "TODO/",
             "HISTORY/", "references/", ".github/")

#: `experiments/19` is how the whole project refers to a numbered instrument.
#: It is a real citation and it resolves to `experiments/19-<something>.py`.
NUMBERED_EXPERIMENT = re.compile(r"^experiments/(\d{2})$")

#: `views.py:131`, `torrent_miscellaneous.pas:393` -- how the sweep cites into
#: the corpus. A bare filename with a line number, resolved by basename when
#: exactly one file in the tree or the corpus carries that name. This is the
#: half of T-121 that matters: the corpus is tracked at captured commits
#: precisely so these are checkable rather than aspirational.
BARE_LINE_CITATION = re.compile(
    r"^([A-Za-z0-9_.\-]+\.(?:py|rs|pas|go|c|h|cpp|js|ts|sh|ps1|yaml|yml|md|toml)):(\d+)$")


def _basenames() -> dict[str, list[str]]:
    """basename -> every absolute path carrying it, across the whole tree."""
    index: dict[str, list[str]] = {}
    for root, dirs, files in os.walk(REPO):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for name in files:
            index.setdefault(name, []).append(os.path.join(root, name))
    return index


def _reference_paths() -> dict[str, str]:
    """Every path inside a captured reference tree, as `<top>/<rest>`.

    The sweep cites paths belonging to *other* projects
    (`.github/workflows/fetch_update_trackers.yaml`,
    `docs/methodology/references.md`). Those are real citations into the
    corpus and must not be reported as broken paths of ours.
    """
    found: dict[str, str] = {}
    refs = os.path.join(REPO, "references")
    if not os.path.isdir(refs):
        return found
    for owner in os.listdir(refs):
        tree = os.path.join(refs, owner, "tree")
        if not os.path.isdir(tree):
            continue
        for root, dirs, files in os.walk(tree):
            dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
            for name in files:
                key = os.path.relpath(os.path.join(root, name), tree)
                found.setdefault(key.replace(os.sep, "/"),
                                 os.path.join(root, name))
            for d in dirs:
                key = os.path.relpath(os.path.join(root, d), tree)
                found.setdefault(key.replace(os.sep, "/"), "")
    return found


def is_empty_directory(path: str) -> bool:
    """A directory that git cannot carry.

    Git tracks files, not directories, so an empty one exists on the machine
    that made it and nowhere else. A document linking to it therefore resolves
    for its author and 404s for everybody who clones -- the "tested tree and
    committed tree are different" failure, in its cheapest form.
    """
    return os.path.isdir(path) and not any(os.scandir(path))


def path_exists(token: str, in_references: dict[str, str]) -> bool:
    """Does this backticked token name something real?"""
    if os.path.exists(os.path.join(REPO, token)):
        return True
    m = NUMBERED_EXPERIMENT.match(token)
    if m:
        exp = os.path.join(REPO, "experiments")
        return any(f.startswith(m.group(1) + "-") for f in os.listdir(exp))
    return token in in_references


def resolve(token: str, in_references: dict[str, str]) -> str | None:
    """The absolute path a citation names, or None."""
    direct = os.path.join(REPO, token)
    if os.path.isfile(direct):
        return direct
    return in_references.get(token)


def line_count(path: str) -> int | None:
    try:
        with open(path, "rb") as handle:
            return sum(1 for _ in handle)
    except OSError:
        return None


LOAD_BEARING = os.path.join(REPO, "experiments", "fixtures",
                            "load-bearing-citations.tsv")

#: "118 tests", "**118** tests, no network" -- however a document phrases it.
TEST_COUNT = re.compile(r"\*{0,2}(\d{2,4})\*{0,2}\s+tests\b")


def actual_test_count() -> int | None:
    """How many tests the suite really has, counted by importing it.

    Counted rather than parsed, because a `test_` method inside a helper class
    or a `subTest` loop makes grep disagree with the runner, and the runner is
    what a reader will run.
    """
    import unittest
    tests_dir = os.path.join(REPO, "tests")
    src = os.path.join(REPO, "src")
    if src not in sys.path:
        sys.path.insert(0, src)
    # `top_level_dir=REPO` needs `tests/` to be an importable package and it
    # is not (no `__init__.py`, deliberately -- the suite runs as
    # `discover -s tests`). Discover the way the documented command does.
    cwd = os.getcwd()
    try:
        os.chdir(REPO)
        suite = unittest.defaultTestLoader.discover(tests_dir)
    except Exception:
        return None
    finally:
        os.chdir(cwd)

    def count(s) -> int:
        try:
            return sum(count(x) for x in s)
        except TypeError:
            return 1
    total = count(suite)
    return total or None


def check_test_counts(files: list[str]) -> list[tuple[str, int, str]]:
    """Does every document that states a test count state the right one?"""
    actual = actual_test_count()
    if actual is None:
        return []
    out: list[tuple[str, int, str]] = []
    for rel in files:
        if not rel.endswith(".md"):
            continue
        for lineno, line in enumerate(read(os.path.join(REPO, rel)).splitlines(), 1):
            # A count scoped to one module (`tests.test_profile` -- 15 tests)
            # is not a claim about the suite. Nor is one a document explicitly
            # marks as the figure at the time an entry closed: rewriting that
            # to today's number would falsify the acceptance record (RULES 7).
            if re.search(r"\btest_\w+\b", line) or "at acceptance" in line:
                continue
            for match in TEST_COUNT.finditer(line):
                stated = int(match.group(1))
                if stated != actual:
                    out.append((rel, lineno, (
                        f"states {stated} tests; the suite has {actual}. "
                        "This count has drifted twice -- cite the command or "
                        "correct the number.")))
    return out


def check_load_bearing() -> list[tuple[str, int, str]]:
    """Does each pinned line still say what it is cited for?

    Existence is not enough. Four documents once cited
    `ngosang_trackerslist.pas:93` for a `.Clear` call on line 98; line 93 is a
    comment, and the existence check passed it. This is the half that bites.
    """
    out: list[tuple[str, int, str]] = []
    rel_fixture = os.path.relpath(LOAD_BEARING, REPO)
    try:
        with open(LOAD_BEARING, encoding="utf-8") as handle:
            rows = list(enumerate(handle, 1))
    except OSError as exc:
        return [(rel_fixture, 0, f"cannot be read: {exc}")]

    for lineno, raw in rows:
        row = raw.strip()
        if not row or row.startswith("#"):
            continue
        parts = row.split("\t")
        if len(parts) != 3:
            out.append((rel_fixture, lineno,
                        "malformed row: expected path<TAB>line<TAB>substring"))
            continue
        path, number, expected = parts[0].strip(), parts[1].strip(), parts[2].strip()
        target = os.path.join(REPO, path)
        if not os.path.isfile(target):
            out.append((rel_fixture, lineno, f"cited file is gone: {path}"))
            continue
        try:
            with open(target, encoding="utf-8", errors="replace") as handle:
                lines = handle.read().splitlines()
        except OSError as exc:
            out.append((rel_fixture, lineno, f"cannot read {path}: {exc}"))
            continue
        index = int(number) - 1
        if index < 0 or index >= len(lines):
            out.append((rel_fixture, lineno,
                        f"{path}:{number} is past the end ({len(lines)} lines)"))
            continue
        if expected not in lines[index]:
            out.append((rel_fixture, lineno, (
                f"{path}:{number} no longer says what it is cited for.\n"
                f"      expected to contain: {expected!r}\n"
                f"      the line says:       {lines[index].strip()!r}")))
    return out


def read(path: str) -> str:
    with open(path, encoding="utf-8", errors="replace") as fh:
        return fh.read()


def collect_files() -> list[str]:
    out = []
    for root, dirs, files in os.walk(REPO):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for name in files:
            abs_path = os.path.join(root, name)
            rel = os.path.relpath(abs_path, REPO)
            if _skip_path(rel):
                continue
            if os.path.splitext(name)[1] in TEXT_SUFFIXES:
                out.append(rel)
    return sorted(out)


def section_ids(text: str) -> set[str]:
    """Section numbers from `## 3. Title` / `### 3.4 Title` headings."""
    ids = set()
    for m in re.finditer(r"^#{2,4}\s+(\d+(?:\.\d+[a-z]?)?)\.?\s", text, re.M):
        ids.add(m.group(1))
        if "." in m.group(1):
            ids.add(m.group(1).split(".")[0])
    return ids


def main() -> int:
    problems: list[tuple[str, int, str]] = []

    try:
        rules_text = read(os.path.join(REPO, "TODO", "RULES.md"))
        claims_text = read(os.path.join(REPO, "HISTORY", "claims.md"))
        index_text = read(os.path.join(REPO, "TODO", "INDEX.md"))
        decisions_text = read(os.path.join(REPO, "HISTORY", "decisions.md"))
    except OSError as exc:
        print(f"COULD NOT RUN (exit 2): {exc}", file=sys.stderr)
        return 2

    rules_sections = section_ids(rules_text)
    known_claims = set(re.findall(r"\bC-(\d+[a-z]?)\b", claims_text))
    known_entries = set(re.findall(r"\bT-(\d{3})\b", index_text))
    known_decisions = set(re.findall(r"^\|?\s*\*{0,2}D(\d+)\*{0,2}\s*[|:]",
                                     decisions_text, re.M))
    known_decisions |= set(re.findall(r"^#{2,4}\s+D(\d+)\b", decisions_text, re.M))

    reference_paths = _reference_paths()
    basenames = _basenames()
    files = collect_files()
    for rel in files:
        text = read(os.path.join(REPO, rel))
        lines = text.splitlines()
        rel_dir = os.path.dirname(rel)

        for lineno, line in enumerate(lines, 1):
            # 1. retired documents
            if rel not in RETIRED_DOCS_ALLOWED:
                m = RETIRED_DOCS.search(line)
                if m:
                    problems.append((rel, lineno, (
                        f"cites the retired design brief ({m.group(0).strip()!r}); "
                        "its content lives in TODO/, HISTORY/ and docs/ -- cite that")))

            # 1b. bare section references. A line that has already framed
            #     itself as provenance ("the brief's SS8.2, SS8.3, SS11") may
            #     carry the rest bare -- repeating the frame per number would
            #     be noise, and the frame is what tells the reader not to go
            #     looking for a section 8.3 in this tree.
            if rel not in BARE_SECTION_ALLOWED and "the brief's \u00a7" not in line:
                m = BARE_SECTION.search(line)
                if m:
                    problems.append((rel, lineno, (
                        f"bare section reference {line[m.start():m.start()+6].strip()!r} "
                        "resolves to nothing -- name the rule or entry that owns it, "
                        "or write it as \"the brief's \u00a7n\" if it is provenance")))

            # 8. retired figures
            if rel not in RETIRED_FIGURES_ALLOWED:
                for fig, why in RETIRED_FIGURES.items():
                    if re.search(rf"(?<![\d.]){fig}(?![\d.])", line):
                        problems.append((rel, lineno, (
                            f"quotes the retired figure {fig} ({why}); "
                            "cite HISTORY/corpus-baseline.md")))

            # 2. markdown links (a markdown construct, so only in markdown --
            #    a regex character class in Python source is not a link)
            #
            #    ⚠ CODE SPANS ARE STRIPPED FIRST, and only for this rule.
            #    Markdown does not linkify inside backticks, so `[int](2.65)`
            #    in a page about PowerShell rounding is a specimen, not a link
            #    to a file called 2.65. Reported as broken on 2026-08-31, which
            #    is a false positive, and a checker that cries wolf is a checker
            #    somebody switches off. The backticked-path rule below needs the
            #    spans intact, which is why the stripping is local to this loop.
            link_line = CODE_SPAN.sub(" ", line) if rel.endswith(".md") else line
            for target in (MD_LINK.findall(link_line) if rel.endswith(".md") else ()):
                if target.startswith(("http://", "https://", "mailto:", "#")):
                    continue
                clean = target.split("#", 1)[0]
                if not clean:
                    continue
                resolved = os.path.normpath(os.path.join(REPO, rel_dir, clean))
                if not os.path.exists(resolved):
                    problems.append((rel, lineno,
                                     f"markdown link does not resolve: {target}"))
                elif is_empty_directory(resolved):
                    problems.append((rel, lineno, (
                        f"link points at an EMPTY directory: {target}. Git does "
                        "not track empty directories, so this resolves here and "
                        "404s in a fresh clone -- put a file in it or drop the "
                        "link")))

            # 3. RULES cross-references
            for sec in RULES_REF.findall(line):
                if rel == os.path.join("TODO", "RULES.md") and sec in rules_sections:
                    continue
                if sec not in rules_sections:
                    problems.append((rel, lineno,
                                     f"RULES {sec} is not a section of TODO/RULES.md"))

            # 4/5/6. registers
            for cid in CLAIM_REF.findall(line):
                if cid not in known_claims:
                    problems.append((rel, lineno,
                                     f"C-{cid} has no row in HISTORY/claims.md"))
            for eid in ENTRY_REF.findall(line):
                if eid not in known_entries:
                    problems.append((rel, lineno,
                                     f"T-{eid} has no row in TODO/INDEX.md"))
            for did in DECISION_REF.findall(line):
                if did not in known_decisions:
                    problems.append((rel, lineno,
                                     f"D{did} has no row in HISTORY/decisions.md"))

            # 7/8. backticked repo paths and line citations
            for tok in BACKTICK.findall(line):
                tok = tok.strip()
                bare = BARE_LINE_CITATION.match(tok)
                if bare:
                    name, wanted = bare.group(1), int(bare.group(2))
                    candidates = basenames.get(name, [])
                    if len(candidates) != 1:
                        # 0 = nothing by that name; >1 = ambiguous, and a
                        # citation a reader cannot resolve uniquely is not a
                        # citation. Both are reported.
                        problems.append((rel, lineno, (
                            f"line citation names {name}, which matches "
                            f"{len(candidates)} file(s) in the tree")))
                        continue
                    total = line_count(candidates[0])
                    if total is not None and wanted > total:
                        problems.append((rel, lineno, (
                            f"line citation past end of file: {tok} but "
                            f"{os.path.relpath(candidates[0], REPO)} has "
                            f"{total} lines")))
                    continue
                if not tok.startswith(TOP_LEVEL):
                    continue
                if any(ch in tok for ch in " *?<>|{}"):
                    continue
                base, _, suffix = tok.partition(":")
                base = base.rstrip("/")
                if is_empty_directory(os.path.join(REPO, base)):
                    problems.append((rel, lineno, (
                        f"cites an EMPTY directory: {base}. Git does not track "
                        "one, so it does not exist in a fresh clone")))
                    continue
                if path_exists(base, reference_paths):
                    # 8. `views.py:131` promises line 131 exists. This is the
                    #    half of T-121 that matters: the corpus is tracked at
                    #    captured commits precisely so a line citation is
                    #    checkable rather than aspirational.
                    if suffix and suffix.split("-")[0].isdigit():
                        wanted = int(suffix.split("-")[0])
                        target = resolve(base, reference_paths)
                        total = line_count(target) if target else None
                        if total is not None and wanted > total:
                            problems.append((rel, lineno, (
                                f"line citation past end of file: {base}:{wanted} "
                                f"but the file has {total} lines")))
                    continue
                # Two kinds of non-existent path are legitimate, and each has
                # to say which it is on the same line, so that "does not
                # exist", "not built yet" and "deliberately deleted" can never
                # be confused for one another.
                #
                # `(planned)` -- work this project intends to create.
                # `(removed)` -- a path that existed and was deleted on
                #   purpose. `HISTORY/corrections.md` and
                #   `references/PROVENANCE.md` cannot do their job without it:
                #   both exist to say what is no longer here, and naming a
                #   deleted file in prose rather than in backticks would hide
                #   exactly the detail an auditor needs. Marking it also keeps
                #   the checker honest, because a `(removed)` path that comes
                #   BACK is then a contradiction somebody can grep for.
                if "(planned)" in line:
                    continue
                if "(removed)" in line:
                    continue
                problems.append((rel, lineno,
                                 f"backticked path does not exist: {base} "
                                 "-- mark it (planned) if it is unbuilt work, "
                                 "(removed) if it was deleted on purpose"))

    problems.extend(check_load_bearing())
    problems.extend(check_test_counts(files))

    print(f"checked {len(files)} tracked text files")
    print("  rules sections   :", len(rules_sections))
    print("  claim rows       :", len(known_claims))
    print("  entry rows       :", len(known_entries))
    print("  decision rows    :", len(known_decisions))
    print()

    if problems:
        print(f"FAILED  {len(problems)} citation(s) do not resolve\n")
        current = None
        for rel, lineno, msg in problems:
            if rel != current:
                print(f"  {rel}")
                current = rel
            print(f"    {lineno}: {msg}")
        print()
        print("A citation that does not resolve is worse than no citation: it")
        print("costs the reader the time to discover that, and it teaches them")
        print("to stop following the ones that do.")
        return 1

    print("OK  every citation resolves: no retired-document reference, every")
    print("    link, RULES section, claim id, entry id, decision id and")
    print("    backticked path is real, every load-bearing line still says")
    print("    what it is cited for, and no retired figure is quoted.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
