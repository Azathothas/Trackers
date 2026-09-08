"""Every script can print on the platform the contributor guide names.

⛔ **Found by running `--help`, which nothing had ever done.** On 2026-09-09
`scripts/update-state.py --help` and `scripts/probe-corpus.py --help` both died
with `UnicodeEncodeError` on Windows: argparse prints the module docstring,
those docstrings carry the project's own markers, and a Windows console is a
legacy code page. Neither script imported `scripts/_scope.py`, which
reconfigures both streams to UTF-8 on import.

⚠ **CI could never have caught it.** The runners are UTF-8, so the failure is
invisible there and lands only on a contributor following a documented command.
`docs/conventions/shell.md` section 6 records the hazard, and the two scripts
that most needed it were the two that did not have it.

⭐ So the assertion is **structural, not behavioural**: every script that can
print reconfigures its streams. That holds on any platform, including the one
where the defect is invisible.

No network. Run:  python3 -m unittest tests.test_scripts_are_runnable -v
"""

from __future__ import annotations

import ast
import os
import subprocess
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPTS = os.path.join(REPO, "scripts")

#: `_scope.py` is where the reconfigure lives, so it cannot import itself.
#:
#: ⛔ `check-no-third-party-imports.py` is exempt for a **measured** reason, not
#: a convenient one: adding the scripts directory to `sys.path` there makes its
#: own guard fire -- *"a project-local module name collides with an installed
#: package"* -- because sys.path resolution is precisely what that check
#: reasons about, and it cannot reason about a path it just altered. The
#: exemption is safe because its output is ASCII, and the test below asserts
#: that rather than assuming it, so the exemption cannot outlive its reason.
EXEMPT = {"_scope.py", "check-no-third-party-imports.py"}


def script_files() -> list[str]:
    return sorted(n for n in os.listdir(SCRIPTS)
                  if n.endswith(".py") and n not in EXEMPT)


class EveryScriptCanPrint(unittest.TestCase):

    def test_a_script_carrying_a_marker_can_print_it(self):
        """⛔ The structural form of the defect.

        ⚠ **Scoped to scripts that contain a non-ASCII character**, which is
        the precise condition. Requiring the import everywhere would fail
        `check-todo.py`, which is pure ASCII and can never hit this -- and a
        rule that fires where the defect cannot occur is one somebody
        eventually exempts their way around.
        """
        missing = []
        for name in script_files():
            path = os.path.join(SCRIPTS, name)
            with open(path, encoding="utf-8") as fh:
                source = fh.read()
            if source.isascii():
                continue
            tree = ast.parse(source, filename=name)
            imports_scope = any(
                (isinstance(node, ast.Import)
                 and any(a.name == "_scope" for a in node.names))
                or (isinstance(node, ast.ImportFrom) and node.module == "_scope")
                for node in ast.walk(tree))
            if not (imports_scope or "reconfigure" in source):
                missing.append(name)
        self.assertEqual(
            missing, [],
            f"these scripts contain a character a Windows console cannot "
            f"encode and never make stdout UTF-8, so printing it kills them: "
            f"{missing}. docs/conventions/shell.md section 6.")

    def test_the_exempt_script_prints_only_ascii(self):
        """⛔ The exemption's own condition, asserted rather than trusted.

        `check-no-third-party-imports.py` cannot take the reconfigure without
        breaking its own guard, so it is exempt only for as long as nothing it
        prints needs one. The moment somebody puts a marker in its report this
        fails, which is what stops an exemption from outliving its reason.
        """
        result = subprocess.run(
            [sys.executable,
             os.path.join(SCRIPTS, "check-no-third-party-imports.py")],
            capture_output=True, timeout=300)
        for stream, text in (("stdout", result.stdout), ("stderr", result.stderr)):
            with self.subTest(stream=stream):
                self.assertTrue(
                    text.decode("utf-8", "replace").isascii(),
                    "this script is exempt from the reconfigure because its "
                    "output is ASCII, and it no longer is")

    def test_no_test_assumes_the_ignored_scratch_directory_exists(self):
        """⛔ Green here, red in every clone.

        `.tmp/` is gitignored, so it exists on the machine that wrote a test
        and in **no checkout**. `tests/test_publication.py` took a `mkdtemp`
        inside it and four tests errored in CI while passing locally -- run
        `34277752016`, the third time this project has shipped a red gate on a
        green local one.

        The rule is mechanical: a test that names the scratch directory
        creates it.

        ⚠ **Scoped to the function**, not the file. The first version asked
        whether the file contained a `makedirs` anywhere, and a mutation that
        deleted the one that mattered still passed -- an unrelated `makedirs`
        three methods away satisfied it. That is the "a test whose name claims
        more than it checks" pattern, caught by planting the defect.
        """
        offenders = []
        tests_dir = os.path.dirname(os.path.abspath(__file__))
        # ⚠ This file names the literal in order to search for it, so the
        # check would otherwise report itself. Skipping the checker is the
        # narrowest exemption available and it cannot hide a real offender:
        # nothing else in this file touches the scratch directory.
        myself = os.path.basename(__file__)
        for name in sorted(os.listdir(tests_dir)):
            if not name.startswith("test_") or not name.endswith(".py"):
                continue
            if name == myself:
                continue
            with open(os.path.join(tests_dir, name), encoding="utf-8") as fh:
                tree = ast.parse(fh.read(), filename=name)
            for node in ast.walk(tree):
                if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    continue
                names = [n for n in ast.walk(node)
                         if isinstance(n, ast.Constant) and n.value == ".tmp"]
                if not names:
                    continue
                creates = any(
                    isinstance(c, ast.Call)
                    and getattr(c.func, "attr", getattr(c.func, "id", "")) == "makedirs"
                    for c in ast.walk(node))
                if not creates:
                    offenders.append(f"{name}:{node.name}")
        self.assertEqual(
            offenders, [],
            f"these use the gitignored scratch directory without creating it "
            f"in the same function, so they pass here and error in a fresh "
            f"clone: {offenders}")

    def test_the_scope_helper_says_it_is_deliberate(self):
        """An import for a side effect is one a tidy-up removes. `_scope`
        exports a named no-op so the dependency is stated."""
        sys.path.insert(0, SCRIPTS)
        import _scope
        self.assertTrue(callable(_scope.printable_stdout))

    def test_the_scripts_with_a_parser_print_their_help(self):
        """The behavioural half, which is what actually failed. ⚠ It passes
        vacuously on a UTF-8 host, which is why the structural test above is
        the one that carries the rule."""
        for name in ("update-state.py", "probe-corpus.py", "generate.py"):
            with self.subTest(script=name):
                result = subprocess.run(
                    [sys.executable, os.path.join(SCRIPTS, name), "--help"],
                    capture_output=True, timeout=120)
                self.assertEqual(
                    result.returncode, 0,
                    f"{name} --help failed: "
                    f"{result.stderr.decode('utf-8', 'replace')[-400:]}")


if __name__ == "__main__":
    unittest.main()
