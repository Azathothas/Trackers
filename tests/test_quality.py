"""T-100 and T-101: the registry's fields, and what is measured about a source.

⛔ **T-101's finding is that the obvious action is the wrong one, twice.** A
source contributing nothing unique looks like dead weight and may be the
**corroboration** that turns another source's claim into evidence; a source
with many unique but scruffy entries looks like a liability and is the thing an
aggregator exists to capture. So the report asks questions and never acts, and
these tests assert that it never acts.

⚠ **T-100's `Premise` decides which fields belong in the registry at all**:
last successful fetch, last failure, failure count and health state are
per-run **state**, so they live in `provenance.SourceHistory` and are asserted
here to be absent from the registry. A value in two places with no check that
they agree is drift.

No network. Run:  python3 -m unittest tests.test_quality -v
"""

from __future__ import annotations

import os
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.provenance import SourceHistory, SourceObservation  # noqa: E402
from trackers.quality import assess, assess_all  # noqa: E402
from trackers.registry import SOURCES  # noqa: E402


REQUIRED_STATIC_FIELDS = ("id", "url", "role", "trust", "category", "upstream",
                          "notes", "expected_min", "expected_max",
                          "derivation", "expected_format", "parser",
                          "fetch_strategy", "cache_strategy", "normalization",
                          "validation_rules")

#: ⛔ These are **state**, not configuration, and T-100's `Premise` says so.
#: They live in `provenance.SourceHistory`. A registry that carried them too
#: would have two copies of one fact and no check that they agree.
FIELDS_THAT_BELONG_IN_STATE = ("last_successful_fetch", "last_failure",
                               "failure_count", "health_state")


class EveryRegistryEntryIsComplete(unittest.TestCase):
    """T-100's `Prove` clause: adding a source without a field must fail."""

    def test_every_source_populates_every_required_field(self):
        for source in SOURCES:
            for field in REQUIRED_STATIC_FIELDS:
                with self.subTest(source=source.id, field=field):
                    self.assertTrue(hasattr(source, field),
                                    f"{source.id} has no {field}")
                    value = getattr(source, field)
                    self.assertNotIn(value, (None, "", ()),
                                     f"{source.id}.{field} is empty, so the "
                                     f"field describes nothing")

    def test_the_state_fields_are_not_duplicated_into_the_registry(self):
        """⛔ T-100's `Premise`, asserted so a later session does not helpfully
        add them back beside the ones in `provenance.SourceHistory`."""
        for source in SOURCES:
            for field in FIELDS_THAT_BELONG_IN_STATE:
                with self.subTest(source=source.id, field=field):
                    self.assertFalse(
                        hasattr(source, field),
                        f"{field} is per-run state and lives in "
                        f"provenance.SourceHistory; two copies is drift")

    def test_a_source_missing_a_field_cannot_be_constructed(self):
        """⚠ The dataclass is what enforces it, and this is the proof: the
        fields with no default cannot be omitted."""
        from trackers.registry import Source
        with self.assertRaises(TypeError):
            Source(id="x")  # type: ignore[call-arg]

    def test_the_one_source_whose_format_differs_says_so(self):
        """⭐ A field with the same value everywhere is boilerplate nobody
        reads. The blacklist's format is genuinely different -- its exclusion
        reasons live in comment lines the common parser strips -- and that
        difference is what published eight excluded URLs when a caller assumed
        every source was interchangeable."""
        formats = {s.id: s.expected_format for s in SOURCES}
        blacklist = formats["ngosang_blacklist"]
        self.assertIn("REASON", blacklist)
        self.assertGreater(len(set(formats.values())), 1,
                           "every source claims an identical format, so the "
                           "field carries no information")


class WhatIsMeasuredSitsBesideWhatWasAsserted(unittest.TestCase):
    """T-101."""

    def setUp(self):
        self.provenance = {
            "udp://only-a.example:1/announce": ["a"],
            "udp://both.example:1/announce": ["a", "b"],
            "udp://only-b.example:1/announce": ["b"],
        }

    def _source(self, sid: str):
        return next(s for s in SOURCES if s.id == sid)

    def test_unique_and_corroborating_are_counted_separately(self):
        """⭐ The distinction T-101's `Decision` turns on. Collapsing them into
        "contributed" is what would make a corroborating source look useless."""
        source = self._source("ngosang_all")
        provenance = {"u1": [source.id], "u2": [source.id, "other"],
                      "u3": ["other"]}
        quality = assess(source, provenance=provenance)
        self.assertEqual((quality.contributed, quality.unique,
                          quality.corroborating), (2, 1, 1))

    def test_a_source_with_no_history_reports_dashes_and_not_zeroes(self):
        """⛔ RULES 1.5. An unknown failure rate rendered as 0.0 reads as a
        perfect record, which is the opposite of what is known."""
        quality = assess(self._source("xiu2_all"), provenance={})
        self.assertIsNone(quality.failure_rate)
        self.assertIsNone(quality.latest_entries)
        self.assertIsNone(quality.distinct_bodies)
        self.assertEqual(quality.observations, 0)

    def test_a_history_supplies_the_measured_columns(self):
        source = self._source("xiu2_all")
        history = SourceHistory.new(source.id, source.url,
                                    "2026-09-01T00:00:00Z")
        for hour, outcome, entries, digest in (
                (0, "OK", 150, "a" * 64), (3, "FAILED", None, None),
                (6, "OK", 152, "b" * 64)):
            history = history.observe(SourceObservation(
                at=f"2026-09-01T{hour:02d}:00:00Z", outcome=outcome,
                entries=entries, digest=digest))
        quality = assess(source, provenance={}, history=history)
        self.assertEqual(quality.observations, 3)
        self.assertAlmostEqual(quality.failure_rate, 1 / 3, places=3)
        self.assertEqual((quality.entries_min, quality.entries_max), (150, 152))
        self.assertEqual(quality.distinct_bodies, 2)

    def test_a_question_is_never_raised_on_a_thin_sample(self):
        """⚠ A failure rate over one fetch is not a failure rate, and RULES 1.4
        forbids reporting it as one."""
        source = self._source("xiu2_all")
        history = SourceHistory.new(source.id, source.url,
                                    "2026-09-01T00:00:00Z").observe(
            SourceObservation(at="2026-09-01T00:00:00Z", outcome="FAILED"))
        quality = assess(source, provenance={}, history=history)
        self.assertEqual(quality.failure_rate, 1.0)
        self.assertEqual(quality.disagreements(), (),
                         "a question was raised on one observation")

    def test_a_sustained_failure_rate_does_raise_a_question(self):
        """⛔ And the harness must be able to fire, or it proves nothing."""
        source = self._source("xiu2_all")
        history = SourceHistory.new(source.id, source.url,
                                    "2026-09-01T00:00:00Z")
        for hour in range(8):
            history = history.observe(SourceObservation(
                at=f"2026-09-01T{hour:02d}:00:00Z",
                outcome="FAILED" if hour % 2 else "OK",
                entries=None if hour % 2 else 150,
                digest=None if hour % 2 else f"{hour:064d}"))
        quality = assess(source, provenance={}, history=history)
        self.assertTrue(quality.disagreements())
        self.assertIn("failure rate", " ".join(quality.disagreements()))

    def test_a_question_is_a_question_and_never_a_demotion(self):
        """⛔ T-101's `Decision`: neither interesting finding means "drop it".
        Nothing in the returned value changes the asserted trust."""
        source = self._source("desirefire_all")
        provenance = {f"u{i}": [source.id, "other"] for i in range(20)}
        quality = assess(source, provenance=provenance)
        self.assertEqual(quality.asserted_trust, "low")
        self.assertEqual(quality.unique, 0)
        questions = " ".join(quality.disagreements())
        self.assertIn("corroborat", questions)
        self.assertNotIn("drop", questions.replace("dropping it", ""))

    def test_the_report_is_deterministic_and_in_registry_order(self):
        """RULES 3.6: two runs render identically."""
        first = assess_all(SOURCES, provenance=self.provenance)
        second = assess_all(SOURCES, provenance=self.provenance)
        self.assertEqual([r.as_record() for r in first],
                         [r.as_record() for r in second])
        self.assertEqual([r.source_id for r in first], [s.id for s in SOURCES])

    def test_the_record_carries_the_static_fields_too(self):
        """A quality table that omitted how a source is parsed would be half a
        description of how it is handled."""
        record = assess(self._source("ngosang_blacklist"),
                        provenance={}).as_record()
        self.assertIn("REASON", record["expected_format"])
        self.assertEqual(record["parser"], "normalize.parse_many")
        self.assertIn("questions_for_a_human", record)


if __name__ == "__main__":
    unittest.main()
