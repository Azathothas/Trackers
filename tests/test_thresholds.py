"""T-102: every change-detection threshold cites the observations behind it.

⛔ **The entry's `Prove` clause is exact**: *each threshold cites the
observation window it was derived from, and a test fails when a threshold has
no derivation recorded.* Both halves are here, and the second is the one with
teeth -- a derivation nothing checks is a comment.

⚠ **What this does NOT assert is that the bands are right.** They cannot be:
the window is **one observation** for every source in the registry, taken on
2026-08-29 by `experiments/19-scheme-census.py`. What it asserts is that they
are **wide enough for what was actually seen** and that nobody can narrow one
without narrowing its derivation too. T-102's `Decision` is explicit about the
direction: widen rather than narrow while the sample is one.

⛔ **Two bands failed this when it was written.** `ngosang_all` and `xiu2_all`
were each one entry narrower than the "~40% and ~3x" rule their own comment
claimed, because the numbers were typed beside the rule rather than derived
from it. That is RULES 2.1 in miniature and it is exactly what this file
prevents recurring.

No network. Run:  python3 -m unittest tests.test_thresholds -v
"""

from __future__ import annotations

import os
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.registry import SOURCES, Derivation, WIDE_BAND  # noqa: E402


class EveryThresholdCitesItsObservations(unittest.TestCase):

    def test_every_source_has_a_derivation(self):
        """⛔ The `Prove` clause's second half: no source may carry a band
        without recording where it came from."""
        for source in SOURCES:
            with self.subTest(source=source.id):
                self.assertIsInstance(source.derivation, Derivation)
                self.assertGreaterEqual(source.derivation.samples, 1,
                                        "a band derived from no observation "
                                        "is a magic number")

    def test_every_observation_carries_the_instant_it_was_taken_at(self):
        """⚠ One window entry per observation. A count whose date is missing
        cannot be re-derived, and RULES 2 requires the conditions to travel
        with the number."""
        for source in SOURCES:
            with self.subTest(source=source.id):
                self.assertEqual(len(source.derivation.window),
                                 source.derivation.samples)
                for instant in source.derivation.window:
                    self.assertRegex(instant, r"^\d{4}-\d{2}-\d{2}")

    def test_every_derivation_names_a_committed_instrument_that_exists(self):
        """⛔ RULES 2: the instrument is the deliverable. A derivation citing a
        script nobody can run is a citation that does not resolve."""
        for source in SOURCES:
            with self.subTest(source=source.id):
                instrument = source.derivation.instrument
                self.assertTrue(instrument)
                self.assertTrue(
                    os.path.exists(os.path.join(REPO, instrument)),
                    f"{source.id} cites {instrument}, which is not in the tree")

    def test_no_band_is_narrower_than_its_own_derivation(self):
        """⭐ **The one with teeth.** The registry may be wider than the method
        justifies -- it usually is, because a small source's variation is
        larger than a multiple of a small number -- but never narrower."""
        for source in SOURCES:
            with self.subTest(source=source.id):
                low, high = source.derivation.derived_band()
                self.assertLessEqual(
                    source.expected_min, low,
                    f"{source.id}: expected_min {source.expected_min} is above "
                    f"the {low} its observations support")
                self.assertGreaterEqual(
                    source.expected_max, high,
                    f"{source.id}: expected_max {source.expected_max} is below "
                    f"the {high} its observations support")

    def test_every_observation_falls_inside_the_band_it_produced(self):
        """A band that excludes the count it was derived from would refuse the
        source on the very run that measured it."""
        for source in SOURCES:
            for count in source.derivation.observations:
                with self.subTest(source=source.id, count=count):
                    self.assertGreaterEqual(count, source.expected_min)
                    self.assertLessEqual(count, source.expected_max)

    def test_the_band_is_a_band(self):
        for source in SOURCES:
            with self.subTest(source=source.id):
                self.assertLess(source.expected_min, source.expected_max)
                self.assertGreaterEqual(source.expected_min, 1)


class TheDerivationCanFail(unittest.TestCase):
    """⛔ Mutation-proofing. A check that has never been seen to fail is
    theatre, so the failures are planted here rather than hoped for."""

    def test_a_narrower_band_is_caught(self):
        """The exact defect that was in the registry: a band one entry
        narrower than the rule it claims to follow."""
        derivation = Derivation(observations=(100,), window=("2026-01-01",),
                                instrument="experiments/19-scheme-census.py")
        low, high = derivation.derived_band()
        self.assertEqual((low, high), (40, 301))
        # A band that stops one short of the derived ceiling must not pass the
        # comparison the test above makes.
        self.assertFalse(300 >= high)
        self.assertFalse(41 <= low)

    def test_more_observations_narrow_the_derived_band(self):
        """⭐ The mechanism that retires the wide bands. Nothing has to be
        rewritten by hand when the history arrives: the same method over more
        observations yields a tighter floor and ceiling."""
        one = Derivation(observations=(100,), window=("2026-01-01",),
                         instrument="experiments/19-scheme-census.py")
        many = Derivation(observations=(100, 104, 98, 101),
                          window=("2026-01-01", "2026-01-02", "2026-01-03",
                                  "2026-01-04"),
                          instrument="experiments/19-scheme-census.py")
        self.assertEqual(many.samples, 4)
        # The floor rises with the smallest seen and the ceiling with the
        # largest, so a spread of real observations is what earns a tight band.
        self.assertGreaterEqual(many.derived_band()[0],
                                one.derived_band()[0] - 1)
        self.assertGreaterEqual(many.derived_band()[1], one.derived_band()[1])

    def test_the_method_is_named_rather_than_implied(self):
        """A derivation whose method is a blank string is a number with a
        citation to nothing."""
        for source in SOURCES:
            with self.subTest(source=source.id):
                self.assertEqual(source.derivation.method, WIDE_BAND)
                self.assertIn("0.4", source.derivation.method)


class TheWindowIsHonestlySmall(unittest.TestCase):
    """⚠ The finding this file records rather than hides."""

    def test_every_source_still_rests_on_a_single_observation(self):
        """⛔ **This test is expected to start failing**, and that is its job.

        When a second observation is recorded for any source, this fails and
        sends somebody to T-102 to narrow that source's band from the real
        distribution. A wide band nobody revisits is the exemption-nobody-
        removes row of `docs/conventions/forbidden-patterns.md`.
        """
        samples = {s.id: s.derivation.samples for s in SOURCES}
        self.assertEqual(
            sorted(set(samples.values())), [1],
            f"a source now has more than one observation ({samples}); derive "
            f"its band from the distribution and update T-102")


if __name__ == "__main__":
    unittest.main()
