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
from trackers.sweep import plan, select, slices_for  # noqa: E402


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

    def test_two_runs_in_a_row_share_no_slice(self):
        """Two consecutive rotation INTEGERS select disjoint sets.

        ⚠ **Renamed 2026-09-09, because the old name claimed more than this
        checks**, and the gap was load-bearing. It read `..._share_no_tracker`
        and was taken for "two consecutive runs contact different trackers",
        which is a property of runs; this is a property of integers, and
        nothing asserted that two consecutive runs *get* different integers.
        They do not. `TheRotationIsNotTheCeiling` below carries the run-level
        property and the measurement that forced it. T-087.
        """
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


class TheRotationIsNotTheCeiling(unittest.TestCase):
    """T-087. ⛔ **The class above asserts a property of rotation INTEGERS, and
    the schedule needed a property of RUNS.**

    `test_two_runs_in_a_row_share_no_slice` compares rotation 0 with rotation 1
    and finds them disjoint, which is true and was read as "two consecutive
    runs contact different trackers". Nothing anywhere asserted that two
    consecutive runs *get* different rotations, and they do not:
    `rotation_for` maps an instant to the three-hour bucket containing it, so
    every run started inside one bucket takes one slice.

    ⛔ **Measured, and published.** Runs `34281244142` (2026-09-08T21:33:23Z)
    and `34289476724` (23:11:21Z) both reported `slice 5 of 7 (rotation
    165639)`; `state.jsonl` on the `data` branch carries 192 trackers with
    consecutive observations 5878 s apart, inside a 10800 s ceiling. The
    ceiling is enforced from the recorded history now, so no arithmetic over a
    wall clock can breach it again.
    """

    FIRST = "2026-09-08T21:33:23Z"
    SECOND = "2026-09-08T23:11:21Z"

    @staticmethod
    def _rotation_for():
        import runpy
        return runpy.run_path(
            os.path.join(REPO, "scripts", "probe-corpus.py"),
            run_name="loaded_for_a_test")["rotation_for"]

    def test_two_runs_in_one_bucket_really_do_get_the_same_rotation(self):
        """⛔ The defect, asserted as a property rather than described in a
        comment. It is kept green on purpose: `rotation_for` is a **sampler**,
        and this is what a bucketed sampler does. Deleting this test because it
        looks like it asserts a bug is how the ceiling would quietly go back to
        resting on it."""
        rotation_for = self._rotation_for()
        self.assertEqual(rotation_for(self.FIRST), 165639)
        self.assertEqual(rotation_for(self.SECOND), 165639)

    def test_a_scheduled_run_two_hours_late_lands_in_the_earlier_bucket(self):
        """⚠ Why it is reachable rather than a curiosity: T-009 measured this
        repository's own scheduled sweeps firing **163** and **131** minutes
        late. A slot delayed past the next one's nominal instant puts two runs
        in one bucket."""
        rotation_for = self._rotation_for()
        on_time = rotation_for("2026-09-08T21:00:00Z")
        two_hours_late = rotation_for("2026-09-08T23:00:00Z")
        self.assertEqual(on_time, two_hours_late)

    def _history(self, trackers, at):
        return {t.url: at for t in trackers}

    def test_the_repeated_slice_contacts_nobody_it_just_contacted(self):
        """⭐ The property that actually had to hold, stated over runs."""
        trackers = corpus(1327)
        budget = budget_for("ci")
        first = plan(trackers, budget, 165639)
        self.assertEqual(first.rotation, 165639)
        seen = self._history(first.trackers, self.FIRST)

        second = plan(trackers, budget, 165639, last_seen=seen,
                      now=self.SECOND)
        contacted_twice = {t.url for t in second.trackers} & {
            t.url for t in first.trackers}
        self.assertEqual(contacted_twice, set(),
                         "the second run re-contacted trackers the first one "
                         "measured 98 minutes earlier")

    def test_it_advances_rather_than_idling_through_the_slot(self):
        """⭐ Holding the slice back would turn a politeness breach into a
        coverage gap: no requests, the slot spent, and the corpus walked slower
        than the seven-run pass the schedule is sized for. So a slice with
        nothing due yields to the next."""
        trackers = corpus(1327)
        budget = budget_for("ci")
        first = plan(trackers, budget, 165639)
        second = plan(trackers, budget, 165639,
                      last_seen=self._history(first.trackers, self.FIRST),
                      now=self.SECOND)
        self.assertTrue(second.advanced)
        self.assertEqual(second.rotation, 165640)
        self.assertGreater(len(second.trackers), 0)
        self.assertEqual(len(second.held_by_ceiling), len(first.trackers))

    def test_planning_again_from_the_rotation_it_returned_is_stable(self):
        """⛔ `probe-corpus.py` previews once and sweeps once, and the two must
        agree (RULES 3.6). A preview of a different 190 trackers is not a
        preview -- run 34252497106 showed slice 0 and probed slice 3."""
        trackers = corpus(1327)
        budget = budget_for("ci")
        seen = self._history(plan(trackers, budget, 165639).trackers,
                             self.FIRST)
        once = plan(trackers, budget, 165639, last_seen=seen, now=self.SECOND)
        twice = plan(trackers, budget, once.rotation, last_seen=seen,
                     now=self.SECOND)
        self.assertEqual(once.rotation, twice.rotation)
        self.assertEqual([t.url for t in once.trackers],
                         [t.url for t in twice.trackers])

    def test_a_corpus_contacted_end_to_end_contacts_nobody_and_says_so(self):
        """⛔ Exit 0 having probed nothing is normally the forbidden pattern.
        Here it is the only correct answer -- every tracker was contacted
        inside the interval -- so it is reported as its own state rather than
        being indistinguishable from an empty corpus."""
        trackers = corpus(1327)
        budget = budget_for("ci")
        everyone = {t.url: self.FIRST for t in trackers}
        held = plan(trackers, budget, 165639, last_seen=everyone,
                    now=self.SECOND)
        self.assertEqual(held.trackers, ())
        self.assertTrue(held.exhausted)
        self.assertEqual(len(held.held_by_ceiling), len(trackers))
        self.assertTrue(held.as_record()["every_slice_held"])

    def test_without_a_history_it_selects_exactly_what_select_selects(self):
        """⚠ The change is additive: a caller that supplies no history gets
        the behaviour that existed before it, and the record says so rather
        than implying a ceiling that was never applied."""
        trackers = corpus(1327)
        budget = budget_for("ci")
        for rotation in (0, 3, 165639):
            with self.subTest(rotation=rotation):
                unplanned = plan(trackers, budget, rotation)
                self.assertEqual([t.url for t in unplanned.trackers],
                                 [t.url for t in select(trackers, budget,
                                                        rotation)])
                self.assertFalse(unplanned.from_history)
                self.assertFalse(unplanned.as_record()["enforced_from_history"])

    def test_nobody_held_and_nobody_checked_are_told_apart(self):
        """⛔ Both are zero held. A reader cannot tell a run that applied the
        ceiling and found everyone due from one that never opened the history,
        and the second is the arrangement that published 192 double contacts."""
        trackers = corpus(1327)
        budget = budget_for("ci")
        long_ago = {t.url: "2026-01-01T00:00:00Z" for t in trackers}
        checked = plan(trackers, budget, 0, last_seen=long_ago,
                       now=self.SECOND)
        unchecked = plan(trackers, budget, 0)
        self.assertEqual(len(checked.held_by_ceiling),
                         len(unchecked.held_by_ceiling))
        self.assertNotEqual(checked.as_record()["enforced_from_history"],
                            unchecked.as_record()["enforced_from_history"])

    def test_the_local_profile_has_one_slice_and_cannot_advance(self):
        """⚠ `local` takes the whole corpus, so there is nowhere to advance to
        and the loop must not wrap onto the slice it just held."""
        trackers = corpus(50)
        budget = budget_for("local")
        held = plan(trackers, budget, 0,
                    last_seen={t.url: self.FIRST for t in trackers},
                    now=self.SECOND)
        self.assertEqual(held.trackers, ())
        self.assertEqual(held.rotation, 0)
        self.assertTrue(held.exhausted)


if __name__ == "__main__":
    unittest.main()
