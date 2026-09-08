"""T-084: a scheduled sweep must walk the corpus, not re-probe one slice of it.

⛔ **Two defects this exists to prevent were each one commit from shipping.**
The operator settled the cadence at D7's three hours on 2026-09-08, and the
selector took a fixed stride from index 0: scheduling that means the **same**
190 trackers are contacted eight times a day forever while the other 1137 are
never contacted again, so `MIN_SAMPLES_FOR_DEATH` can never be reached for them
and nothing about them ever leaves `unknown`.

⛔ **And the first fix for it was worse than it looked.** Slicing by position
degenerates the moment the corpus changes size, which it does daily: adding
**one** tracker shifts every index by one, so the next run's slice is exactly
the previous run's set. Membership is the tracker's own hash now.

⭐ The rotation is also the politer arrangement, which is the unusual part: a
pass over 1327 trackers at 200 a run takes 7 runs, so each tracker is probed
once per **21 hours** rather than once per three.

No network. Run:  python3 -m unittest tests.test_rotation -v
"""

from __future__ import annotations

import os
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.normalize import parse  # noqa: E402
from trackers.profile import budget_for  # noqa: E402
from trackers.sweep import select, slices_for  # noqa: E402


def corpus(size: int):
    """A corpus of distinct trackers, built through the production parser."""
    return [parse(f"udp://h{i:05d}.example:6969/announce") for i in range(size)]


class TheRotationCoversTheCorpus(unittest.TestCase):

    def test_consecutive_runs_cover_every_tracker_exactly_once(self):
        """⭐ The property the schedule rests on. Anything less and a tracker
        is either never probed or probed twice as often as the arithmetic in
        the politeness budget assumes."""
        trackers = corpus(1327)
        budget = budget_for("ci")
        slices = slices_for(len(trackers), budget.sample_size)
        self.assertEqual(slices, 7)

        seen: list[str] = []
        for rotation in range(slices):
            seen.extend(t.url for t in select(trackers, budget, rotation))
        self.assertEqual(len(seen), len(trackers), "a pass did not cover the "
                                                   "corpus exactly once")
        self.assertEqual(len(set(seen)), len(trackers))

    def test_no_slice_is_wildly_larger_or_smaller_than_the_others(self):
        """⚠ Hash-assigned slices are not exactly equal, and `sample_size` is a
        sampling figure rather than a hard ceiling -- a run is bounded by the
        concurrency limit, the per-host rule and the deadline. What would be a
        defect is a lopsided split, so the assertion is a band around the mean
        rather than an exact size."""
        trackers = corpus(1327)
        budget = budget_for("ci")
        slices = slices_for(len(trackers), budget.sample_size)
        sizes = [len(select(trackers, budget, r)) for r in range(slices)]
        mean = len(trackers) / slices
        self.assertEqual(sum(sizes), len(trackers))
        for size in sizes:
            with self.subTest(size=size):
                self.assertGreater(size, mean * 0.7, f"sizes {sizes}")
                self.assertLess(size, mean * 1.3, f"sizes {sizes}")

    def test_two_runs_in_a_row_share_no_tracker(self):
        """⛔ The failure mode stated directly: consecutive runs three hours
        apart must not contact the same operator twice."""
        trackers = corpus(1327)
        budget = budget_for("ci")
        first = {t.url for t in select(trackers, budget, 0)}
        second = {t.url for t in select(trackers, budget, 1)}
        self.assertEqual(first & second, set())

    def test_the_rotation_wraps(self):
        """Run 7 is run 0 again, which is what makes a pass a cycle."""
        trackers = corpus(1327)
        budget = budget_for("ci")
        slices = slices_for(len(trackers), budget.sample_size)
        self.assertEqual([t.url for t in select(trackers, budget, 0)],
                         [t.url for t in select(trackers, budget, slices)])

    def test_it_is_deterministic(self):
        """RULES 3.6. The same corpus and rotation give the same bytes."""
        trackers = corpus(500)
        budget = budget_for("ci")
        self.assertEqual([t.url for t in select(trackers, budget, 3)],
                         [t.url for t in select(trackers, budget, 3)])

    def test_a_slice_still_spans_the_whole_ordering(self):
        """⚠ The reason the selector was a stride and not a head: the sort key
        leads with the transport, so a contiguous block is one transport and a
        broken UDP path would never appear in it."""
        mixed = ([parse(f"udp://u{i}.example:6969/announce") for i in range(300)]
                 + [parse(f"http://h{i}.example:80/announce") for i in range(300)])
        budget = budget_for("ci")
        chosen = select(mixed, budget, 0)
        schemes = {t.url.split(":")[0] for t in chosen}
        self.assertEqual(schemes, {"udp", "http"},
                         "a slice contains one transport, so a fault in the "
                         "other could not show up in it")

    def test_the_local_profile_takes_the_whole_corpus_whatever_the_rotation(self):
        """`local` does not sample, so rotating it must change nothing."""
        trackers = corpus(1327)
        budget = budget_for("local")
        for rotation in (0, 1, 6, 99):
            with self.subTest(rotation=rotation):
                self.assertEqual(len(select(trackers, budget, rotation)),
                                 len(trackers))

    def test_a_clock_it_cannot_read_stops_the_run(self):
        """⛔ The adversarial pass's third attack. Returning 0 for a malformed
        clock pins every scheduled run to slice 0, so the same 190 trackers are
        probed eight times a day forever and the other 1137 never -- silently,
        and indistinguishable from working."""
        import runpy
        rotation_for = runpy.run_path(
            os.path.join(REPO, "scripts", "probe-corpus.py"),
            run_name="loaded_for_a_test")["rotation_for"]
        self.assertEqual(rotation_for("2026-09-08T12:00:00Z"), 165636)
        for bad in ("not-a-date", "", "yesterday"):
            with self.subTest(value=bad):
                with self.assertRaises(ValueError):
                    rotation_for(bad)

    def test_a_corpus_that_changes_size_does_not_reshuffle_the_slices(self):
        """⛔ The defect the adversarial pass of 2026-09-08 found, and it is
        the one that mattered most.

        The upstreams regenerate daily, so the corpus one scheduled run sees is
        not the corpus the last one saw. With slices assigned by **position**,
        adding a single tracker shifted every index by one and the next run's
        slice became **exactly the previous run's set** -- a 100% overlap, the
        rotation silently ceasing to rotate, and the original defect back with
        a rotation bolted on top of it.

        Assigning by the tracker's own hash makes membership independent of
        everybody else.
        """
        budget = budget_for("ci")
        before = corpus(1327)
        for added in (1, 5, 20):
            with self.subTest(added=added):
                after = before + [
                    parse(f"udp://appeared{j}.example:6969/announce")
                    for j in range(added)]
                first = {t.url for t in select(before, budget, 0)}
                second = {t.url for t in select(after, budget, 1)}
                self.assertEqual(first & second, set())

    def test_a_corpus_that_crosses_a_slice_boundary_is_bounded_not_zero(self):
        """⚠ Growing past a multiple of the sample size changes how many
        slices there are, and then membership does move. It is stated rather
        than hidden: the overlap is a minority of a slice, and a tracker in it
        is probed twice three hours apart -- at D7's ceiling, not over it.
        """
        budget = budget_for("ci")
        before = corpus(1327)
        after = before + [parse(f"udp://appeared{j}.example:6969/announce")
                          for j in range(100)]
        self.assertNotEqual(slices_for(len(before), budget.sample_size),
                            slices_for(len(after), budget.sample_size),
                            "this test has stopped crossing a boundary")
        first = {t.url for t in select(before, budget, 0)}
        second = {t.url for t in select(after, budget, 1)}
        self.assertLess(len(first & second), len(first) * 0.5)

    def test_a_corpus_smaller_than_the_sample_is_one_slice(self):
        trackers = corpus(50)
        budget = budget_for("ci")
        self.assertEqual(slices_for(len(trackers), budget.sample_size), 1)
        self.assertEqual(len(select(trackers, budget, 4)), len(trackers))


if __name__ == "__main__":
    unittest.main()
