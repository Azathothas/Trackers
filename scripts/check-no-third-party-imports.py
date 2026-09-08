#!/usr/bin/env python3
"""Gate: shipped code imports nothing outside the Python standard library.

This is the `Prove` clause of decision D1 (`HISTORY/decisions.md`).

Why it is a gate rather than a note. RULES 12 requires the project to run
unattended for years, and observes that "every dependency is a thing that can
disappear during the five-year window". A zero-dependency rule is only worth
anything if it is enforced: the failure mode is that somebody adds `requests`
for one convenient call, the rule quietly becomes false, and nobody notices
until an install breaks on a runner image three years from now.

It parses imports with `ast` rather than grepping, because a grep for `import`
matches strings, comments and docstrings -- this project's own experiment files
are full of prose about importing things.

Exit codes:
    0  every import resolves to the standard library or to project-local code
    1  a third-party import was found
    2  the check could not run (needs Python 3.10+ for sys.stdlib_module_names)
"""

from __future__ import annotations

import ast
import importlib.util
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Directories whose contents ship and must therefore stay dependency-free.
SHIPPED = ("experiments", "scripts", "src")

# Project-local modules that are not stdlib but are also not dependencies.
LOCAL = {"_conditions"}

# Directories that act as package roots. A top-level name that resolves to a
# package or module under one of these is project-local, not a dependency.
#
# This list exists because its absence was a real false positive: the gate
# failed on `scripts/generate.py: imports 'trackers'`, which is this project's
# own package living under `src/`. The original heuristic only recognised a
# sibling module in the SAME directory, so anything importing across
# directories looked third-party. A checker that cries wolf gets switched off,
# which would have been worse than not having it.
#
# `scripts` was added for the same reason and it is the same class of false
# positive: `experiments/27-value-gate.py` imports `generate.load_corpus` so
# that the value gate measures the corpus the pipeline actually publishes,
# rather than carrying a second assembly path that could disagree with it. A
# directory this checker itself walks as SHIPPED project code cannot also be a
# third-party dependency, and refusing the import would have bought a copy of
# `load_corpus` -- the copy-pasted-logic row in
# `docs/conventions/forbidden-patterns.md`.
PACKAGE_ROOTS = ("src", "scripts", ".")


def _local_names(repo: str) -> set[str]:
    names: set[str] = set()
    for root in PACKAGE_ROOTS:
        base = os.path.join(repo, root)
        if not os.path.isdir(base):
            continue
        for entry in os.listdir(base):
            full = os.path.join(base, entry)
            if os.path.isdir(full) and os.path.exists(
                    os.path.join(full, "__init__.py")):
                names.add(entry)
            elif entry.endswith(".py"):
                names.add(entry[:-3])
    return names


def _installed_elsewhere(name: str) -> bool:
    """Whether `name` resolves to something outside this repository.

    Asked with every path inside the repository removed from `sys.path`, so
    the local file cannot answer for itself. ⚠ Best-effort by construction: an
    import system that raises here is reported as "not installed", because
    failing the gate on an unanswerable question would make it unusable on a
    host with an unusual importer.
    """
    saved = list(sys.path)
    try:
        sys.path = [p for p in sys.path
                    if p and not os.path.abspath(p).startswith(REPO)]
        try:
            return importlib.util.find_spec(name) is not None
        except (ImportError, ValueError, AttributeError):
            return False
    finally:
        sys.path = saved


def top_level_imports(path: str) -> set[str]:
    with open(path, encoding="utf-8") as fh:
        try:
            tree = ast.parse(fh.read(), filename=path)
        except SyntaxError as e:
            print(f"  !! {path}: syntax error: {e}", file=sys.stderr)
            return set()
    names: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for a in node.names:
                names.add(a.name.split(".")[0])
        elif isinstance(node, ast.ImportFrom):
            # A relative import (level > 0) is project-local by definition.
            if node.level == 0 and node.module:
                names.add(node.module.split(".")[0])
    return names


def main() -> int:
    if not hasattr(sys, "stdlib_module_names"):
        print("needs Python 3.10+ for sys.stdlib_module_names", file=sys.stderr)
        return 2
    stdlib = set(sys.stdlib_module_names)
    local = LOCAL | _local_names(REPO)

    # ⛔ A LOCAL NAME THAT IS ALSO AN INSTALLED PACKAGE IS NOT A PASS.
    #
    # Found by the door sweep of 2026-09-08. `local` is built from filenames,
    # so a file under `scripts/` named after a package makes importing that
    # package pass **anywhere in the tree** -- measured, not supposed: planting
    # that pair returned exit 0 and "standard library only. D1 holds."
    #
    # ⚠ The hole predates the `scripts` root that found it; `src` and `.` have
    # had it since the roots existed. It is fixed here rather than noted,
    # because a checker that can be silenced by adding a file is the
    # "allowlist applied to the whole line" row of
    # `docs/conventions/forbidden-patterns.md` wearing a different shape.
    #
    # ⭐ The test is which one would actually be imported, and that depends on
    # `sys.path` order at runtime -- so a collision is refused rather than
    # resolved. Being ambiguous is the defect.
    collisions = sorted(n for n in local
                        if n not in stdlib and _installed_elsewhere(n))
    if collisions:
        print("FAIL  a project-local module name collides with an installed "
              "package:")
        for n in collisions:
            print(f"  - {n!r} exists both in this repository and on sys.path "
                  "outside it")
        print("\nWhich one an import resolves to depends on sys.path order at "
              "runtime, so this check cannot tell a local import from a "
              "dependency. Rename the local file.")
        return 1

    offenders: list[tuple[str, str]] = []
    checked = 0
    for d in SHIPPED:
        root = os.path.join(REPO, d)
        if not os.path.isdir(root):
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [x for x in dirnames
                           if x not in {"__pycache__", ".venv", "results",
                                        "fixtures", "tree"}]
            for fn in filenames:
                if not fn.endswith(".py"):
                    continue
                p = os.path.join(dirpath, fn)
                checked += 1
                for name in sorted(top_level_imports(p)):
                    if name in stdlib or name in local:
                        continue
                    # A sibling module in the same directory is local.
                    if os.path.exists(os.path.join(dirpath, name + ".py")):
                        continue
                    offenders.append((os.path.relpath(p, REPO), name))

    print(f"checked {checked} Python files under {', '.join(SHIPPED)}")
    if offenders:
        print("\nFAIL  third-party imports found:")
        for path, name in offenders:
            print(f"  - {path}: imports {name!r}")
        print("\nD1 records that this project is standard-library only. Adding "
              "a dependency is allowed, but it needs a recorded decision "
              "naming what it earns (RULES 9), not a silent import.")
        return 1
    print("\nOK  standard library only. D1 holds.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
