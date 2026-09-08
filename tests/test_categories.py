"""T-046: the five categories, and the rule behind each one.

⛔ **A hand-curated list presented as derived is a lie about methodology**, so
the tests that matter here are the ones about *why* a tracker is in a file:

  stable      measured history only, above a stated threshold. Empty on day
              one, and empty **honestly** -- never reputation-seeded.
  foss        derived from FOSS provenance plus a seed labelled as curated.
              Neither half has content, and the file says which.
  hardcoded   the maintainer's order, preserved and self-deduplicated.
  common      the others merged, plus what measured live.
  anime       provenance from a source the registry classifies `anime`.

⭐ **An empty category must say whether the rule matched nothing or the
evidence it needs does not exist**, which is the same distinction as `FAILED`
against `EMPTY` in acquisition. A file that is empty for an unstated reason
looks like a defect and gets "fixed" by somebody filling it in.

No network. Run:  python3 -m unittest tests.test_categories -v
"""

from __future__ import annotations

import os
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.categories import (CATEGORIES,  # noqa: E402
                                 MIN_OBSERVATIONS_FOR_STABLE,
                                 STABLE_SUCCESS_RATE, select_all, select_for)
from trackers.normalize import parse  # noqa: E402
from trackers.state import TrackerHistory  # noqa: E402

SOURCE_CATEGORIES = {"anime_source": "anime", "general_source": "general",
                     "foss_source": "foss"}


def a_history(url: str, *, checks: int, successes: int,
              live: bool = True) -> TrackerHistory:
    """A history with `checks` observations, `successes` of them successful."""
    history = TrackerHistory.new(url, "2026-09-01T00:00:00Z")
    for index in range(checks):
        ok = index < successes
        history = history.observe(
            state="live" if ok else "unknown", ok=ok,
            observed_at=f"2026-09-01T{index:02d}:00:00Z",
            rung="tracker_semantic" if ok else "dns",
            failure=None if ok else "timeout")
    if live and successes:
        return history
    return history


class Stable(unittest.TestCase):
    """⛔ Measured evidence only."""

    def setUp(self):
        self.trackers = [parse(f"udp://h{i}.example:6969/announce")
                         for i in range(3)]
        self.args = dict(provenance={},
                         source_categories={"general_source": "general"})

    def test_it_is_empty_with_no_history_and_says_why(self):
        """The bootstrap problem, made visible rather than hidden."""
        selection = select_for("stable", self.trackers, **self.args)
        self.assertEqual(selection.count, 0)
        self.assertFalse(selection.evidence_available)
        self.assertIn("honest", selection.reason)
        self.assertIn(str(MIN_OBSERVATIONS_FOR_STABLE), selection.rule)

    def test_a_tracker_below_the_threshold_does_not_qualify(self):
        url = self.trackers[0].url
        histories = {url: a_history(url, checks=MIN_OBSERVATIONS_FOR_STABLE - 1,
                                    successes=MIN_OBSERVATIONS_FOR_STABLE - 1)}
        selection = select_for("stable", self.trackers, histories=histories,
                               **self.args)
        self.assertEqual(selection.count, 0, selection.reason)

    def test_a_tracker_at_the_threshold_qualifies(self):
        """⚠ The positive control. A rule that admitted nothing would pass
        every test above it."""
        url = self.trackers[0].url
        histories = {url: a_history(url, checks=MIN_OBSERVATIONS_FOR_STABLE,
                                    successes=MIN_OBSERVATIONS_FOR_STABLE)}
        selection = select_for("stable", self.trackers, histories=histories,
                               **self.args)
        self.assertEqual([t.url for t in selection.trackers], [url])
        self.assertTrue(selection.evidence_available)

    def test_enough_observations_at_a_poor_rate_does_not_qualify(self):
        """⛔ Long history is not the same as reliability, and the entry names
        both."""
        url = self.trackers[0].url
        histories = {url: a_history(url, checks=20, successes=10)}
        selection = select_for("stable", self.trackers, histories=histories,
                               **self.args)
        self.assertEqual(selection.count, 0)
        self.assertIn(str(STABLE_SUCCESS_RATE), selection.rule)

    def test_the_threshold_exceeds_the_bar_for_calling_a_tracker_dead(self):
        """⭐ The evidence needed to recommend a tracker should exceed the
        evidence needed to stop claiming it is alive."""
        from trackers.probe import MIN_SAMPLES_FOR_DEATH
        self.assertGreater(MIN_OBSERVATIONS_FOR_STABLE, MIN_SAMPLES_FOR_DEATH)


class Foss(unittest.TestCase):
    """D9: derived plus a labelled seed, and the halves stay distinguishable."""

    def test_with_no_foss_source_and_no_seed_it_is_empty_and_says_so(self):
        selection = select_for("foss", [parse("udp://a.example:1/announce")],
                               provenance={},
                               source_categories={"general_source": "general"})
        self.assertEqual(selection.count, 0)
        self.assertFalse(selection.evidence_available)
        self.assertIn("methodology lie", selection.reason)

    def test_it_derives_from_provenance_when_a_foss_source_exists(self):
        tracker = parse("udp://a.example:1/announce")
        selection = select_for(
            "foss", [tracker], provenance={tracker.url: ["foss_source"]},
            source_categories=SOURCE_CATEGORIES)
        self.assertEqual([t.url for t in selection.trackers], [tracker.url])
        self.assertIn("derived", selection.reason)

    def test_the_seed_is_counted_separately_from_the_derived_half(self):
        """⭐ The point of D9: a consumer must be able to tell curated from
        measured."""
        derived = parse("udp://a.example:1/announce")
        seeded = parse("udp://b.example:1/announce")
        selection = select_for(
            "foss", [derived], provenance={derived.url: ["foss_source"]},
            source_categories=SOURCE_CATEGORIES, foss_seed=[seeded])
        self.assertEqual(selection.count, 2)
        self.assertIn("1 derived", selection.reason)
        self.assertIn("1 from the curated seed", selection.reason)


class Hardcoded(unittest.TestCase):
    """The maintainer's file, not ours."""

    def test_it_preserves_their_order_and_does_not_sort(self):
        manual = [parse(u) for u in ("udp://zzz.example:1/announce",
                                     "udp://aaa.example:1/announce",
                                     "http://mmm.example:80/announce")]
        selection = select_for("hardcoded", [], provenance={},
                               source_categories={}, hardcoded=manual)
        self.assertEqual([t.url for t in selection.trackers],
                         [t.url for t in manual],
                         "the maintainer's order was rewritten")

    def test_it_deduplicates_against_itself(self):
        manual = [parse("udp://a.example:1/announce"),
                  parse("udp://a.example:1/announce"),
                  parse("udp://b.example:1/announce")]
        selection = select_for("hardcoded", [], provenance={},
                               source_categories={}, hardcoded=manual)
        self.assertEqual(len(selection.trackers), 2)
        self.assertEqual(selection.trackers[0].url, "udp://a.example:1/announce")

    def test_with_no_input_file_it_is_empty_and_names_the_entry(self):
        selection = select_for("hardcoded", [], provenance={},
                               source_categories={})
        self.assertEqual(selection.count, 0)
        self.assertIn("T-106", selection.reason)


class Anime(unittest.TestCase):
    """Provenance, which is auditable, rather than a judgement about content."""

    def test_membership_comes_from_the_sources_registry_category(self):
        tracker = parse("udp://a.example:1/announce")
        other = parse("udp://b.example:1/announce")
        selection = select_for(
            "anime", [tracker, other],
            provenance={tracker.url: ["anime_source"],
                        other.url: ["general_source"]},
            source_categories=SOURCE_CATEGORIES)
        self.assertEqual([t.url for t in selection.trackers], [tracker.url])
        self.assertIn("anime_source", selection.reason)


class Common(unittest.TestCase):
    """Merged, deduplicated, and drawing on the evidence this project has."""

    def test_it_carries_what_measured_live(self):
        live = parse("udp://live.example:1/announce")
        dark = parse("udp://dark.example:1/announce")
        histories = {live.url: a_history(live.url, checks=1, successes=1)}
        selection = select_for("common", [live, dark], provenance={},
                               source_categories={}, histories=histories)
        self.assertEqual([t.url for t in selection.trackers], [live.url])
        self.assertIn("measured live", selection.reason)

    def test_it_deduplicates_across_the_categories_it_merges(self):
        shared = parse("udp://shared.example:1/announce")
        histories = {shared.url: a_history(shared.url,
                                           checks=MIN_OBSERVATIONS_FOR_STABLE,
                                           successes=MIN_OBSERVATIONS_FOR_STABLE)}
        cats = select_all([shared], provenance={shared.url: ["anime_source"]},
                          source_categories=SOURCE_CATEGORIES,
                          histories=histories, hardcoded=[shared])
        urls = [t.url for t in cats["common"].trackers]
        self.assertEqual(urls.count(shared.url), 1,
                         "a tracker in three categories appears three times")


class EveryCategoryStatesItsRule(unittest.TestCase):

    def test_all_five_exist_and_each_carries_a_rule_and_a_reason(self):
        cats = select_all([parse("udp://a.example:1/announce")], provenance={},
                          source_categories={})
        self.assertEqual(sorted(cats), sorted(CATEGORIES))
        self.assertEqual(len(CATEGORIES), 5)
        for name, selection in cats.items():
            with self.subTest(category=name):
                self.assertTrue(selection.rule, f"{name} states no rule")
                self.assertTrue(selection.reason, f"{name} states no reason")
                self.assertEqual(selection.category, name)

    def test_an_unknown_category_is_refused(self):
        with self.assertRaises(KeyError):
            select_for("reputation", [], provenance={}, source_categories={})


if __name__ == "__main__":
    unittest.main()
