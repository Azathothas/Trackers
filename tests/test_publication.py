"""T-061 and T-063: nothing partial, nothing inconsistent, nothing on failure.

`scripts/generate.py` stages, verifies, and only then moves output into place
(RULES 3.5). These are the assertions that make that more than a comment:

  T-061  mutating one format and not the others fails publication -- and the
         mutation that matters is the one a COUNT check would pass.
  T-063  a failed verification writes nothing, and whatever was published
         before is still there afterwards.

⛔ **The set comparison, not the count comparison.** Two formats holding the
same number of different URLs is a silent corruption a consumer cannot detect.
A count check passes it, which is why one of the tests below swaps a URL rather
than removing one.

No network. Run:  python3 -m unittest tests.test_publication -v
"""

from __future__ import annotations

import json
import os
import runpy
import shutil
import subprocess
import sys
import tempfile
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))
sys.path.insert(0, os.path.join(REPO, "scripts"))

from trackers.normalize import parse  # noqa: E402
from trackers.labelled import render_csv, render_json  # noqa: E402

GENERATE = os.path.join(REPO, "scripts", "generate.py")


def generator():
    """`generate.py`'s namespace, without running its `main`."""
    return runpy.run_path(GENERATE, run_name="loaded_for_a_test")


class _Aggregate:
    """The shape `verify` reads. Small on purpose: the subject is `verify`."""

    def __init__(self, urls):
        self.trackers = [parse(u) for u in urls]
        self.provenance = {}
        self.sources_ok = ["one"]


URLS = ("udp://a.example:6969/announce",
        "http://b.example:80/announce",
        "https://c.example:443/announce")


class TheFormatsMustAgree(unittest.TestCase):
    """T-061's `Prove` clause."""

    def setUp(self):
        self.verify = generator()["verify"]
        self.agg = _Aggregate(URLS)
        self.plaintext = "".join(f"{u}\n" for u in sorted(URLS))
        self.json_text = render_json(self.agg.trackers, provenance={},
                                     generated_at="1970-01-01T00:00:00Z",
                                     code_version="0.0.0")
        self.csv_text = render_csv(self.agg.trackers, provenance={})

    def _problems(self, **over):
        args = dict(labelled_json=self.json_text, labelled_csv=self.csv_text)
        args.update(over)
        return self.verify(self.agg, self.plaintext, set(), **args)

    def test_the_untouched_set_verifies(self):
        """⚠ The positive control. Without it, every assertion below could be
        passing because `verify` refuses everything."""
        self.assertEqual(self._problems(), [])

    def test_a_row_missing_from_the_json_fails(self):
        doc = json.loads(self.json_text)
        doc["trackers"] = doc["trackers"][:-1]
        problems = self._problems(labelled_json=json.dumps(doc))
        self.assertTrue(any("JSON" in p for p in problems), problems)

    def test_a_swapped_url_fails_where_a_count_check_would_not(self):
        """⛔ THE test. The count is identical and the data is wrong."""
        doc = json.loads(self.json_text)
        before = len(doc["trackers"])
        doc["trackers"][0]["url"] = "udp://impostor.example:6969/announce"
        self.assertEqual(len(doc["trackers"]), before, "the count must match, "
                                                      "or this proves nothing")
        problems = self._problems(labelled_json=json.dumps(doc))
        self.assertTrue(any("JSON" in p for p in problems), problems)

    def test_a_duplicated_row_fails(self):
        doc = json.loads(self.json_text)
        doc["trackers"].append(dict(doc["trackers"][0]))
        problems = self._problems(labelled_json=json.dumps(doc))
        self.assertTrue(any("repeats" in p for p in problems), problems)

    def test_a_broken_json_document_fails_rather_than_raising(self):
        problems = self._problems(labelled_json="{not json")
        self.assertTrue(any("does not parse" in p for p in problems), problems)

    def test_a_row_missing_from_the_csv_fails(self):
        lines = self.csv_text.splitlines()
        problems = self._problems(labelled_csv="\n".join(lines[:-1]) + "\n")
        self.assertTrue(any("CSV" in p for p in problems), problems)

    def test_a_swapped_url_in_the_csv_fails(self):
        text = self.csv_text.replace("udp://a.example:6969/announce",
                                     "udp://impostor.example:6969/announce")
        problems = self._problems(labelled_csv=text)
        self.assertTrue(any("CSV" in p for p in problems), problems)


class AFailedGenerationPublishesNothing(unittest.TestCase):
    """T-063 and RULES 3.5, driven through the real script.

    ⭐ Not through `verify` alone: the property is that the FILES do not move,
    and only running the thing can show that.
    """

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="publication-", dir=os.path.join(REPO, ".tmp"))
        self.out = os.path.join(self.tmp, "out")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def _run(self, *extra):
        return subprocess.run(
            [sys.executable, GENERATE, "--offline", "--out", self.out, *extra],
            capture_output=True, text=True, timeout=600, cwd=REPO)

    def test_a_good_run_publishes_all_four_files(self):
        result = self._run()
        self.assertEqual(result.returncode, 0, result.stderr[-500:])
        for name in ("trackers_all.txt", "trackers_all.json",
                     "trackers_all.csv", "report.md"):
            with self.subTest(file=name):
                self.assertTrue(os.path.exists(os.path.join(self.out, name)))

    def test_the_previous_output_survives_a_failed_run(self):
        """⛔ The rule that matters: a failure leaves prior public data intact.

        The failure is induced through the fixtures rather than by patching a
        function, so what is exercised is the path a real broken source takes.
        """
        self.assertEqual(self._run().returncode, 0)
        with open(os.path.join(self.out, "trackers_all.txt"), encoding="utf-8") as fh:
            published = fh.read()
        self.assertTrue(published)

        empty = os.path.join(self.tmp, "no-fixtures")
        os.makedirs(empty, exist_ok=True)
        failed = self._run("--fixtures", empty)
        self.assertNotEqual(failed.returncode, 0,
                            "a run with no sources published something")

        with open(os.path.join(self.out, "trackers_all.txt"), encoding="utf-8") as fh:
            self.assertEqual(fh.read(), published,
                             "the failed run replaced the published data")

    def test_two_runs_over_one_input_are_byte_identical(self):
        """RULES 3.6, on the published bytes rather than on an internal."""
        self.assertEqual(self._run().returncode, 0)
        first = {}
        for name in ("trackers_all.txt", "trackers_all.json", "trackers_all.csv"):
            with open(os.path.join(self.out, name), encoding="utf-8") as fh:
                first[name] = fh.read()
        self.assertEqual(self._run().returncode, 0)
        for name, before in first.items():
            with self.subTest(file=name):
                with open(os.path.join(self.out, name), encoding="utf-8") as fh:
                    self.assertEqual(fh.read(), before)

    def test_check_only_writes_nothing_at_all(self):
        result = self._run("--check-only")
        self.assertEqual(result.returncode, 0, result.stderr[-500:])
        self.assertFalse(os.path.exists(self.out),
                         "--check-only created the output directory")


if __name__ == "__main__":
    unittest.main()
