"""T-084: a scheduled sweep must walk the corpus, not re-probe one slice of it.

⛔ **The defect this exists to prevent was one commit from shipping.** The
operator settled the cadence at D7's three hours on 2026-09-08, and the
selector took a fixed stride from index 0. Scheduling that means the **same**
200 trackers are contacted eight times a day forever while the other 1127 are
never contacted again -- so `MIN_SAMPLES_FOR_DEATH` can never be reached for
them and nothing about them ever leaves `unknown`.

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

    def test_no_run_exceeds_the_sample_size(self):
        """The budget is a ceiling, and a slice that overshot it would spend
        requests the profile did not authorise."""
        trackers = corpus(1327)
        budget = budget_for("ci")
        for rotation in range(slices_for(len(trackers), budget.sample_size)):
            with self.subTest(rotation=rotation):
                self.assertLessEqual(len(select(trackers, budget, rotation)),
                                     budget.sample_size)

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

    def test_a_corpus_smaller_than_the_sample_is_one_slice(self):
        trackers = corpus(50)
        budget = budget_for("ci")
        self.assertEqual(slices_for(len(trackers), budget.sample_size), 1)
        self.assertEqual(len(select(trackers, budget, 4)), len(trackers))


if __name__ == "__main__":
    unittest.main()
