"""T-066: a run report must answer the questions observability requires.

The `Prove` clause: **a field silently disappearing fails.** `REPORT_FIELDS`
is the contract and every label in it must appear in a rendered report, so
deleting a section is a red suite rather than a question nobody can answer any
more.

⛔ **Three of the questions T-066 asks are unanswerable today**, and the report
says so in its own words rather than printing a plausible number. RULES 9.1: a
requirement that cannot be met is retained, its limitation stated, and the
result labelled honestly. A latency distribution over data that keeps no
latency would be exactly the fabricated number RULES 1.5 forbids.

No network. Run:  python3 -m unittest tests.test_report -v
"""

from __future__ import annotations

import os
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.categories import select_all  # noqa: E402
from trackers.normalize import parse  # noqa: E402
from trackers.pipeline import (REPORT_FIELDS, Aggregate,  # noqa: E402
                               render_report)
from trackers.state import TrackerHistory  # noqa: E402


def a_history(url: str, *, checks: int, successes: int) -> TrackerHistory:
    history = TrackerHistory.new(url, "2026-09-01T00:00:00Z")
    for index in range(checks):
        ok = index < successes
        history = history.observe(
            state="live" if ok else "unknown", ok=ok,
            observed_at=f"2026-09-0{1 + index}T00:00:00Z",
            rung="tracker_semantic" if ok else "dns",
            failure=None if ok else "timeout")
    return history


def a_run():
    trackers = [parse(f"udp://h{i}.example:6969/announce") for i in range(4)]
    agg = Aggregate(trackers=trackers,
                    provenance={t.url: ["general_source"] for t in trackers},
                    sources_ok=["general_source"], sources_failed=["broken"])
    histories = {
        trackers[0].url: a_history(trackers[0].url, checks=4, successes=4),
        trackers[1].url: a_history(trackers[1].url, checks=3, successes=0),
        trackers[2].url: a_history(trackers[2].url, checks=1, successes=1),
    }
    categories = select_all(trackers, provenance=agg.provenance,
                            source_categories={"general_source": "general"},
                            histories=histories)
    return agg, histories, categories


def rendered() -> str:
    agg, histories, categories = a_run()
    return render_report(agg, generated_at="2026-09-09T00:00:00Z",
                         code_version="0.1.0+norm1", categories=categories,
                         histories=histories)


class EveryRequiredFieldIsPresent(unittest.TestCase):
    """T-066's `Prove` clause."""

    def test_the_report_answers_every_field_in_the_contract(self):
        text = rendered().lower()
        missing = [f for f in REPORT_FIELDS if f.lower() not in text]
        self.assertEqual(
            missing, [],
            f"the report no longer answers: {missing}. A section was removed "
            f"and took the answer with it.")

    def test_the_contract_is_not_empty(self):
        """⛔ Never a clean verdict over a scope that was never opened. An
        empty `REPORT_FIELDS` would make the test above vacuous."""
        self.assertGreaterEqual(len(REPORT_FIELDS), 10)


class TheHealthSectionIsDerived(unittest.TestCase):

    def test_it_counts_states_rungs_and_depth_from_the_histories(self):
        text = rendered()
        self.assertIn("health observations: 8 across 3 tracker(s)", text)
        self.assertIn("never observed:      1", text)
        self.assertIn("'live': 2", text)
        self.assertIn("observation depth:   median 3, deepest 4", text)

    def test_a_tracker_failing_every_check_is_a_sustained_failure(self):
        """⚠ Three observations, none successful. One failure is a moment and
        must not appear here (RULES 11)."""
        text = rendered()
        self.assertIn("sustained failures:  1", text)
        self.assertIn("- sustained: `udp://h1.example:6969/announce`", text)

    def test_one_failure_alone_is_not_sustained(self):
        trackers = [parse("udp://only.example:6969/announce")]
        agg = Aggregate(trackers=trackers, sources_ok=["s"])
        histories = {trackers[0].url: a_history(trackers[0].url, checks=1,
                                                successes=0)}
        text = render_report(agg, generated_at="2026-09-09T00:00:00Z",
                             code_version="0.1.0", histories=histories)
        self.assertIn("sustained failures:  0", text)

    def test_nothing_in_the_report_calls_a_tracker_dead(self):
        """⛔ The state machine is the only place that decision is made, and
        nothing has three observations that all failed **and** the samples to
        support it."""
        text = rendered()
        self.assertNotIn("'dead'", text)


class WhatItCannotAnswerIsStatedRatherThanFaked(unittest.TestCase):
    """⛔ RULES 9.1 and 1.5, in the one place a plausible number would be
    easiest to invent."""

    def test_the_absent_questions_are_named_with_their_reasons(self):
        text = rendered()
        for phrase in ("latency distribution", "ranking changes",
                       "reliability distribution",
                       "whether publication succeeded"):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, text)

    def test_no_latency_number_is_printed_anywhere(self):
        """The history keeps outcomes and rungs, not round-trip times, so any
        latency figure here would be invented."""
        text = rendered().lower()
        for invented in ("median latency", "p95", "percentile", "ms)"):
            with self.subTest(term=invented):
                self.assertNotIn(invented, text)

    def test_it_says_why_publication_success_is_not_reportable_here(self):
        """The report is written by the step whose output is being published,
        so it cannot report on an event that has not happened."""
        self.assertIn("written *before*", rendered())


class TheReportStaysDeterministic(unittest.TestCase):

    def test_two_renders_of_one_run_are_identical(self):
        """RULES 3.6. The report is part of the published output."""
        self.assertEqual(rendered(), rendered())


if __name__ == "__main__":
    unittest.main()
