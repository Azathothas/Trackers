"""T-081: a history reset preserves the data and never touches `main`.

⛔ **This is the one operation in the project that discards commits**, so the
tests are about what it must never do:

    the current dataset survives byte for byte
    `main` is never touched
    it refuses below the threshold
    it refuses when a required file is already missing
    it verifies after the rewrite, before anything could be pushed

⭐ **Driven against a real synthetic repository**, not a mock. The subject is
git's behaviour under `checkout --orphan`, and a stub would prove nothing about
that.

Why it is safe at all is RULES 3.7: history lives in files, so the reset
discards **commits, not data**, and `state.jsonl` is a tracked file that the
working tree carries through untouched.

No network. Run:  python3 -m unittest tests.test_housekeeping -v
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO, "scripts", "housekeep-data-branch.py")
REQUIRED = ("trackers_all.txt", "trackers_all.json", "trackers_all.csv",
            "report.md", "metadata.json", "state.jsonl", "schema.md")


def git(args, cwd):
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True,
                          text=True, check=True, timeout=300).stdout


class AResetPreservesTheData(unittest.TestCase):

    def setUp(self):
        scratch = os.path.join(REPO, ".tmp")
        os.makedirs(scratch, exist_ok=True)
        self.tmp = tempfile.mkdtemp(prefix="housekeeping-", dir=scratch)
        self.checkout = os.path.join(self.tmp, "data")
        os.makedirs(self.checkout)
        git(["init", "--quiet", "--initial-branch", "data"], self.checkout)
        git(["config", "user.name", "Test"], self.checkout)
        # ⚠ Not an address. `check-no-secrets.py --public` refuses an email
        # literal anywhere in the tree and is right to; git accepts any string
        # here, so a fixture does not need one.
        git(["config", "user.email", "housekeeping-fixture"], self.checkout)
        # Three commits, so a reset has something to discard.
        for round_number in range(3):
            for name in REQUIRED:
                with open(os.path.join(self.checkout, name), "w",
                          encoding="utf-8", newline="\n") as fh:
                    fh.write(f"{name} contents round {round_number}\n")
            git(["add", "-A"], self.checkout)
            git(["commit", "--quiet", "-m", f"round {round_number}"],
                self.checkout)

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def _run(self, *extra):
        return subprocess.run(
            [sys.executable, SCRIPT, "--checkout", self.checkout,
             "--skip-inflight-check", *extra],
            capture_output=True, text=True, timeout=600, cwd=REPO)

    def _contents(self):
        out = {}
        for name in REQUIRED:
            with open(os.path.join(self.checkout, name), encoding="utf-8") as fh:
                out[name] = fh.read()
        return out

    def test_it_does_nothing_below_the_threshold(self):
        """⛔ Three commits is not seventeen thousand. A destructive operation
        that fires whenever it is invoked is one somebody invokes by accident."""
        before = self._contents()
        result = self._run()
        self.assertEqual(result.returncode, 0, result.stderr[-400:])
        self.assertIn("under the threshold", result.stdout)
        self.assertEqual(git(["rev-list", "--count", "HEAD"],
                             self.checkout).strip(), "3")
        self.assertEqual(self._contents(), before)

    def test_crossing_the_threshold_without_apply_still_changes_nothing(self):
        result = self._run("--force-threshold")
        self.assertEqual(result.returncode, 0, result.stderr[-400:])
        self.assertIn("nothing was changed", result.stdout)
        self.assertEqual(git(["rev-list", "--count", "HEAD"],
                             self.checkout).strip(), "3")

    def test_the_reset_keeps_every_file_byte_for_byte(self):
        """⭐ The `Prove` clause. What is discarded is commits, not data."""
        before = self._contents()
        result = self._run("--force-threshold", "--apply")
        self.assertEqual(result.returncode, 0, result.stderr[-400:])
        self.assertEqual(git(["rev-list", "--count", "HEAD"],
                             self.checkout).strip(), "1")
        self.assertEqual(self._contents(), before,
                         "the reset changed the published data")

    def test_the_state_file_survives_which_is_the_whole_argument(self):
        """RULES 3.7: history lives in files. If `state.jsonl` did not survive,
        a reset would be data loss rather than housekeeping."""
        with open(os.path.join(self.checkout, "state.jsonl"),
                  encoding="utf-8") as fh:
            before = fh.read()
        self.assertEqual(self._run("--force-threshold", "--apply").returncode, 0)
        with open(os.path.join(self.checkout, "state.jsonl"),
                  encoding="utf-8") as fh:
            self.assertEqual(fh.read(), before)

    def test_the_branch_keeps_its_name(self):
        self._run("--force-threshold", "--apply")
        self.assertEqual(git(["rev-parse", "--abbrev-ref", "HEAD"],
                             self.checkout).strip(), "data")

    def test_it_refuses_a_checkout_that_is_not_the_data_branch(self):
        """⛔ Never touch `main`. Asserted against the branch it is actually on
        rather than against what a caller claimed."""
        git(["checkout", "--quiet", "-b", "main"], self.checkout)
        result = self._run("--force-threshold", "--apply")
        self.assertEqual(result.returncode, 1)
        self.assertIn("refusing", result.stderr)
        self.assertEqual(git(["rev-list", "--count", "HEAD"],
                             self.checkout).strip(), "3",
                         "it rewrote a branch that was not the data branch")

    def test_it_refuses_when_a_required_file_is_already_missing(self):
        """⛔ A reset would make the absence permanent, so a branch that is
        already short is not housekeeping's problem to bury."""
        os.remove(os.path.join(self.checkout, "state.jsonl"))
        git(["add", "-A"], self.checkout)
        git(["commit", "--quiet", "-m", "lose the state"], self.checkout)
        result = self._run("--force-threshold", "--apply")
        self.assertEqual(result.returncode, 1)
        self.assertIn("state.jsonl", result.stderr)

    def test_it_never_pushes_by_itself(self):
        """The push is printed for a human to run. An automation that force
        pushed as part of a check would be one keystroke from a mistake."""
        result = self._run("--force-threshold", "--apply")
        self.assertIn("push with", result.stdout.lower())
        self.assertNotIn("Everything up-to-date", result.stdout)


if __name__ == "__main__":
    unittest.main()
