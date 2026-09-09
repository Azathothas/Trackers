"""T-103: does the retained provenance answer "why did this tracker disappear"?

⛔ **The `Prove` clause is a scenario, not a property**, and it is written that
way because the wrong answer to this question is the plausible one. A tracker
missing from the output has two causes that look identical from the output
alone:

    the sources stopped listing it     it is gone, and we should say so
    a source failed to fetch           it is NOT gone; we could not see it

RULES 3.2 keeps those apart **inside one run** -- `FetchResult.trackers` is
`None` and never `[]` for a failure. This file is the same distinction across
**time**, which is the only place a consumer can ask it from: by the time
anybody notices a tracker is missing, the run that lost it is long over.

⚠ Both pieces of prior art in this space get the one-run version wrong, in two
languages. Getting the across-time version wrong would publish "this tracker is
gone" every time an upstream had a bad afternoon.

No network. Run:  python3 -m unittest tests.test_provenance -v
"""

from __future__ import annotations

import os
import sys
import tempfile
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.provenance import (DAILY_DAYS, PROVENANCE_FORMAT,  # noqa: E402
                                 RING_SIZE, CorruptProvenance, SourceHistory,
                                 SourceObservation, explain_absence,
                                 observe_fetch, parse_line, read_history,
                                 render_line, write_history)


def obs(at: str, outcome: str = "OK", entries: int | None = 100,
        digest: str = "a" * 64) -> SourceObservation:
    return SourceObservation(at=at, outcome=outcome, entries=entries,
                             digest=digest, http_status=200,
                             bytes_fetched=4096)


def history(source_id: str = "s", *observations: SourceObservation
            ) -> SourceHistory:
    out = SourceHistory.new(source_id, f"https://example.invalid/{source_id}",
                            first_seen=observations[0].at if observations
                            else "2026-01-01T00:00:00Z")
    for observation in observations:
        out = out.observe(observation)
    return out


class WhyDidThisTrackerDisappear(unittest.TestCase):
    """⛔ The `Prove` clause, as the scenario it asks for."""

    URL = "udp://gone.example:6969/announce"

    def test_a_clean_run_that_dropped_it_is_a_removal(self):
        """Every source that carried it fetched cleanly and none lists it: the
        tracker is genuinely gone and a consumer may be told so."""
        histories = {"a": history("a", obs("2026-09-01T00:00:00Z"),
                                  obs("2026-09-02T00:00:00Z"))}
        absence = explain_absence(
            self.URL, carried_by={"a": ["2026-09-01T00:00:00Z"]},
            histories=histories, present_now=False)
        self.assertTrue(absence.genuinely_removed)
        self.assertEqual(absence.failing_sources, ())
        self.assertIn("none lists it now", absence.detail)

    def test_a_failed_source_is_never_reported_as_a_removal(self):
        """⛔ **THE test.** The source that carried it could not be read, so
        its listing is unknown -- and an unknown listing is not an absent one.
        RULES 3.2, extended through time."""
        histories = {"a": history("a", obs("2026-09-01T00:00:00Z"),
                                  obs("2026-09-02T00:00:00Z", "FAILED", None))}
        absence = explain_absence(
            self.URL, carried_by={"a": ["2026-09-01T00:00:00Z"]},
            histories=histories, present_now=False)
        self.assertFalse(absence.genuinely_removed)
        self.assertEqual(absence.failing_sources, ("a",))
        self.assertIn("blind spot", absence.detail)

    def test_one_failing_source_of_several_is_still_not_a_removal(self):
        """⚠ The pessimistic reading, and it is the correct one: if any source
        that carried it is unreadable, we cannot say nobody lists it."""
        histories = {
            "a": history("a", obs("2026-09-02T00:00:00Z")),
            "b": history("b", obs("2026-09-02T00:00:00Z", "FAILED", None)),
        }
        absence = explain_absence(
            self.URL,
            carried_by={"a": ["2026-09-01T00:00:00Z"],
                        "b": ["2026-09-01T00:00:00Z"]},
            histories=histories, present_now=False)
        self.assertFalse(absence.genuinely_removed)
        self.assertEqual(absence.failing_sources, ("b",))

    def test_an_empty_source_is_a_removal_and_a_failed_one_is_not(self):
        """⭐ The distinction that has no second chance. `EMPTY` means the
        source said it has nothing, which is evidence; `FAILED` means it said
        nothing, which is not."""
        empty = {"a": history("a", obs("2026-09-02T00:00:00Z", "EMPTY", 0))}
        failed = {"a": history("a", obs("2026-09-02T00:00:00Z", "FAILED", None))}
        carried = {"a": ["2026-09-01T00:00:00Z"]}
        self.assertTrue(explain_absence(self.URL, carried_by=carried,
                                        histories=empty,
                                        present_now=False).genuinely_removed)
        self.assertFalse(explain_absence(self.URL, carried_by=carried,
                                         histories=failed,
                                         present_now=False).genuinely_removed)

    def test_a_rejected_source_is_a_blind_spot_and_not_a_removal(self):
        """⚠ The body arrived and we refused it, so we hold no listing we are
        willing to act on. Being wrong here costs a tracker we could have
        published; being wrong the other way tells a consumer something is gone
        when nobody established that."""
        histories = {"a": history("a", obs("2026-09-02T00:00:00Z",
                                           "REJECTED", None))}
        absence = explain_absence(
            self.URL, carried_by={"a": ["2026-09-01T00:00:00Z"]},
            histories=histories, present_now=False)
        self.assertFalse(absence.genuinely_removed)
        self.assertEqual(absence.failing_sources, ("a",))

    def test_a_tracker_still_present_is_not_a_disappearance(self):
        absence = explain_absence(
            self.URL, carried_by={"a": ["2026-09-01T00:00:00Z"]},
            histories={"a": history("a", obs("2026-09-02T00:00:00Z"))},
            present_now=True)
        self.assertFalse(absence.genuinely_removed)
        self.assertIn("nothing to explain", absence.detail)

    def test_a_tracker_no_source_ever_carried_is_not_a_disappearance(self):
        """⚠ An absence with no history behind it is a question about the
        corpus, not about a removal, and saying otherwise would invent one."""
        absence = explain_absence(self.URL, carried_by={}, histories={},
                                  present_now=False)
        self.assertFalse(absence.genuinely_removed)
        self.assertIn("has ever been recorded", absence.detail)

    def test_the_answer_names_when_each_source_last_carried_it(self):
        """A verdict with no dates is one nobody can check."""
        absence = explain_absence(
            self.URL,
            carried_by={"a": ["2026-09-01T00:00:00Z", "2026-09-03T00:00:00Z"]},
            histories={"a": history("a", obs("2026-09-04T00:00:00Z"))},
            present_now=False)
        self.assertEqual(absence.last_carried_by,
                         (("a", "2026-09-03T00:00:00Z"),))
        self.assertIn("2026-09-03", absence.detail)


class AFailedFetchIsNeverRecordedAsZero(unittest.TestCase):
    """⛔ The conflation, guarded at the point it would enter the history."""

    def test_observe_fetch_writes_none_for_a_failure(self):
        class _Outcome:
            name = "FAILED"

        class _Result:
            source_id = "a"
            url = "https://example.invalid/a"
            fetched_at = "2026-09-01T00:00:00Z"
            outcome = _Outcome()
            trackers = None
            content_sha256 = None
            http_status = 500
            byte_count = 0

        out = observe_fetch({}, [_Result()], at="2026-09-01T00:00:00Z")
        self.assertIsNone(out["a"].latest().entries,
                          "a failed fetch recorded zero entries, which is the "
                          "one thing this module exists to prevent")
        self.assertFalse(out["a"].latest().ok)
        self.assertEqual(out["a"].lifetime_failures, 1)

    def test_an_empty_source_is_recorded_as_zero_and_ok(self):
        class _Outcome:
            name = "EMPTY"

        class _Result:
            source_id = "a"
            url = "https://example.invalid/a"
            fetched_at = "2026-09-01T00:00:00Z"
            outcome = _Outcome()
            trackers: list = []
            content_sha256 = "b" * 64
            http_status = 200
            byte_count = 0

        out = observe_fetch({}, [_Result()], at="2026-09-01T00:00:00Z")
        self.assertEqual(out["a"].latest().entries, 0)


class TheHistoryIsBoundedAndDeterministic(unittest.TestCase):

    def test_the_ring_never_exceeds_its_cap(self):
        record = history("a", *[obs(f"2026-09-01T{i:02d}:00:00Z")
                                for i in range(24)])
        for day in range(2, 40):
            record = record.observe(obs(f"2026-09-{day:02d}T00:00:00Z"))
        self.assertLessEqual(len(record.ring), RING_SIZE)
        self.assertLessEqual(len(record.daily), DAILY_DAYS)

    def test_an_observation_at_a_known_instant_is_ignored(self):
        """Folding one run twice must not record two observations, which is the
        defect that took `state.py` to MIN_SAMPLES_FOR_DEATH on one probe."""
        once = history("a", obs("2026-09-01T00:00:00Z"))
        twice = once.observe(obs("2026-09-01T00:00:00Z"))
        self.assertEqual(len(twice.ring), 1)
        self.assertEqual(twice.lifetime_fetches, once.lifetime_fetches)

    def test_two_writes_of_one_history_are_byte_identical(self):
        record = history("a", obs("2026-09-01T00:00:00Z"),
                         obs("2026-09-02T00:00:00Z"))
        self.assertEqual(render_line(record), render_line(record))
        self.assertEqual(parse_line(render_line(record)), record)

    def test_a_round_trip_through_a_file_preserves_everything(self):
        records = {"a": history("a", obs("2026-09-01T00:00:00Z")),
                   "b": history("b", obs("2026-09-01T00:00:00Z", "FAILED",
                                         None))}
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "sources.jsonl")
            write_history(path, records, generated_at="2026-09-02T00:00:00Z")
            back, quarantined = read_history(path)
        self.assertEqual(quarantined, [])
        self.assertEqual(back, records)

    def test_a_wrong_header_raises_rather_than_reinitialising(self):
        """⛔ RULES 3.9: a clean rebuild that discards history is data loss
        wearing the costume of a fix."""
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "sources.jsonl")
            with open(path, "w", encoding="utf-8", newline="\n") as handle:
                handle.write('{"format":"something.else/9"}\n')
            with self.assertRaises(CorruptProvenance):
                read_history(path)

    def test_one_bad_line_is_quarantined_and_the_rest_survive(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "sources.jsonl")
            write_history(path, {"a": history("a", obs("2026-09-01T00:00:00Z"))},
                          generated_at="2026-09-02T00:00:00Z")
            with open(path, "a", encoding="utf-8", newline="\n") as handle:
                handle.write("{not json at all\n")
            back, quarantined = read_history(path)
        self.assertEqual(len(back), 1)
        self.assertEqual(len(quarantined), 1)

    def test_two_folds_of_one_run_at_one_instant_add_one_observation(self):
        """⛔ **The determinism property, and it was broken when written.**
        The first draft stamped each observation with the result's own
        `fetched_at`, read from an ambient clock -- so two runs over identical
        inputs produced different bytes (RULES 3.6) and the idempotence guard
        could never recognise a repeat, because every timestamp was new."""
        class _Outcome:
            name = "OK"

        class _Result:
            source_id = "a"
            url = "https://example.invalid/a"
            fetched_at = "ignored, and that is the point"
            outcome = _Outcome()
            trackers = [1, 2, 3]
            content_sha256 = "c" * 64
            http_status = 200
            byte_count = 30

        once = observe_fetch({}, [_Result()], at="2026-09-01T00:00:00Z")
        twice = observe_fetch(once, [_Result()], at="2026-09-01T00:00:00Z")
        self.assertEqual(len(twice["a"].ring), 1)
        self.assertEqual(render_line(once["a"]), render_line(twice["a"]))
        self.assertEqual(once["a"].latest().at, "2026-09-01T00:00:00Z")
        later = observe_fetch(once, [_Result()], at="2026-09-01T03:00:00Z")
        self.assertEqual(len(later["a"].ring), 2)

    def test_the_format_is_versioned(self):
        self.assertTrue(PROVENANCE_FORMAT.endswith("/1"))


class TheDistributionT102IsWaitingFor(unittest.TestCase):
    """⭐ The link between this entry and T-102, asserted so it is not lost."""

    def test_entries_seen_returns_the_counts_a_band_would_be_derived_from(self):
        record = history("a", obs("2026-09-01T00:00:00Z", entries=100),
                         obs("2026-09-02T00:00:00Z", "FAILED", None),
                         obs("2026-09-03T00:00:00Z", entries=104))
        self.assertEqual(record.entries_seen(), (100, 104),
                         "a failed fetch contributed a count to the "
                         "distribution a threshold would be derived from")


if __name__ == "__main__":
    unittest.main()
