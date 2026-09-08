"""T-002: a dataset that stopped updating must be able to say so.

⛔ **The worst failure available to this project is stopping quietly.** GitHub
disables a public repository's scheduled workflows after 60 days without
activity (`C-12`, verified from the documentation). Every published file would
still serve HTTP 200 and none of them would have moved.

⭐ **The half that works is the consumer's**, and that is the point of these
tests. A reader holding the file has the instant it was generated and the
cadence it claims, so the judgement needs nothing of ours to still be running.
A watchdog on our own schedule cannot report that our schedule stopped.

The `Prove` clause is the second test: feed it a dataset older than the
threshold and it reports stale.

No network. Run:  python3 -m unittest tests.test_freshness -v
"""

from __future__ import annotations

import datetime
import json
import os
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.freshness import (PUBLISH_INTERVAL_SECONDS,  # noqa: E402
                                STALE_AFTER_INTERVALS, assess, parse_instant)
from trackers.labelled import metadata_for, render_json  # noqa: E402
from trackers.normalize import parse  # noqa: E402

NOW = datetime.datetime(2026, 9, 9, 12, 0, 0, tzinfo=datetime.timezone.utc)


def ago(seconds: float) -> str:
    return (NOW - datetime.timedelta(seconds=seconds)).strftime(
        "%Y-%m-%dT%H:%M:%SZ")


class TheWatchdog(unittest.TestCase):

    def test_a_dataset_older_than_the_threshold_reports_stale(self):
        """The `Prove` clause."""
        old = ago(PUBLISH_INTERVAL_SECONDS * STALE_AFTER_INTERVALS + 60)
        verdict = assess(old, now=NOW)
        self.assertTrue(verdict.stale, verdict.detail)
        self.assertIn("stale after", verdict.detail)

    def test_a_fresh_dataset_does_not(self):
        """⚠ The positive control. Without it, a checker that called
        everything stale would pass the test above."""
        verdict = assess(ago(60), now=NOW)
        self.assertFalse(verdict.stale, verdict.detail)

    def test_one_late_run_is_not_stale(self):
        """⚠ `C-11`: a scheduled run can be late, and one was observed **163
        minutes** late on 2026-09-08. A marker that cried stale on a single
        late run is noise, and a noisy marker is one consumers learn to
        ignore -- which costs exactly the signal this exists to send."""
        verdict = assess(ago(PUBLISH_INTERVAL_SECONDS + 3600), now=NOW)
        self.assertFalse(verdict.stale, verdict.detail)

    def test_the_boundary_is_where_it_says_it_is(self):
        threshold = PUBLISH_INTERVAL_SECONDS * STALE_AFTER_INTERVALS
        self.assertFalse(assess(ago(threshold - 1), now=NOW).stale)
        self.assertTrue(assess(ago(threshold + 1), now=NOW).stale)

    def test_a_stamp_from_the_future_is_its_own_fault_and_not_freshness(self):
        """⛔ Reporting it as fresh would hide a clock or pipeline defect
        behind the very field that exists to expose one."""
        ahead = (NOW + datetime.timedelta(hours=2)).strftime("%Y-%m-%dT%H:%M:%SZ")
        verdict = assess(ahead, now=NOW)
        self.assertTrue(verdict.from_the_future)
        self.assertFalse(verdict.stale)
        self.assertIn("future", verdict.detail)

    def test_a_small_skew_is_not_a_future_stamp(self):
        """Two machines' clocks differ by seconds routinely."""
        ahead = (NOW + datetime.timedelta(seconds=30)).strftime("%Y-%m-%dT%H:%M:%SZ")
        self.assertFalse(assess(ahead, now=NOW).from_the_future)

    def test_an_unreadable_stamp_raises_rather_than_reading_as_fresh(self):
        """⛔ A staleness check that treated an undateable dataset as current
        would report health over exactly the case it exists for."""
        for bad in ("", "yesterday", "2026-13-45T99:00:00Z"):
            with self.subTest(value=bad):
                with self.assertRaises(ValueError):
                    assess(bad, now=NOW)

    def test_it_reads_the_stamps_this_project_actually_writes(self):
        """⚠ Not a synthetic format. The renderer's own output is the input."""
        doc = json.loads(render_json(
            [parse("udp://a.example:6969/announce")], provenance={},
            generated_at="2026-09-09T11:00:00Z", code_version="0.1.0"))
        verdict = assess(doc["generated_at"], now=NOW)
        self.assertAlmostEqual(verdict.age_seconds, 3600, delta=1)
        self.assertEqual(parse_instant(doc["generated_at"]).tzinfo,
                         datetime.timezone.utc)


class TheConsumerHasWhatTheyNeed(unittest.TestCase):
    """⭐ The half that survives our own failure."""

    def setUp(self):
        self.doc = json.loads(render_json(
            [parse("udp://a.example:6969/announce")], provenance={},
            generated_at="2026-09-09T11:00:00Z", code_version="0.1.0"))
        self.meta = json.loads(metadata_for(
            {"trackers_all.csv": b"url\n"}, generated_at="2026-09-09T11:00:00Z",
            code_version="0.1.0", count=1, digest="sha256:abc"))

    def test_the_json_states_the_cadence_it_expects(self):
        """Without it a consumer has a timestamp and no way to know what age is
        abnormal, which is a number they cannot act on."""
        self.assertEqual(self.doc["publish_interval_seconds"],
                         PUBLISH_INTERVAL_SECONDS)
        self.assertEqual(self.doc["stale_after_intervals"],
                         STALE_AFTER_INTERVALS)

    def test_the_metadata_states_it_too_for_the_other_formats(self):
        """A CSV or plaintext consumer reads it from the file beside theirs."""
        self.assertEqual(self.meta["publish_interval_seconds"],
                         PUBLISH_INTERVAL_SECONDS)
        self.assertEqual(self.meta["stale_after_intervals"],
                         STALE_AFTER_INTERVALS)

    def test_a_consumer_can_decide_staleness_from_the_document_alone(self):
        """⭐ The whole property: nothing of ours has to be running.

        Everything this needs -- the stamp, the cadence, the tolerance -- is in
        the bytes the consumer already holds.
        """
        verdict = assess(self.doc["generated_at"], now=NOW,
                         interval_seconds=self.doc["publish_interval_seconds"],
                         intervals=self.doc["stale_after_intervals"])
        self.assertFalse(verdict.stale)
        long_after = NOW + datetime.timedelta(days=30)
        self.assertTrue(assess(
            self.doc["generated_at"], now=long_after,
            interval_seconds=self.doc["publish_interval_seconds"],
            intervals=self.doc["stale_after_intervals"]).stale)


class PublicationFreshnessIsNotMeasurementFreshness(unittest.TestCase):
    """⛔ The gap found on 2026-09-09 while building the issue automation.

    The publisher runs after every sweep **completion**, including a sweep that
    failed, so `generated_at` keeps moving while no new observation arrives. A
    consumer checking only that sees a fresh file full of ageing labels.
    """

    def test_the_document_says_when_it_last_learned_something(self):
        from trackers.labelled import newest_observation
        from trackers.state import TrackerHistory
        url = "udp://a.example:6969/announce"
        history = TrackerHistory.new(url, "2026-09-01T00:00:00Z").observe(
            state="live", ok=True, observed_at="2026-09-02T00:00:00Z",
            rung="tracker_semantic")
        doc = json.loads(render_json(
            [parse(url)], provenance={}, histories={url: history},
            generated_at="2026-09-09T12:00:00Z", code_version="0.1.0"))
        self.assertEqual(doc["newest_observation"], "2026-09-02T00:00:00Z")
        self.assertNotEqual(doc["newest_observation"], doc["generated_at"],
                            "the two fields answer different questions")
        self.assertEqual(newest_observation({url: history}),
                         "2026-09-02T00:00:00Z")

    def test_with_no_observations_it_is_null_rather_than_the_clock(self):
        """⛔ Never the generation time as a stand-in. That would report a
        measurement that never happened."""
        doc = json.loads(render_json(
            [parse("udp://a.example:6969/announce")], provenance={},
            generated_at="2026-09-09T12:00:00Z", code_version="0.1.0"))
        self.assertIsNone(doc["newest_observation"])

    def test_a_consumer_can_tell_a_fresh_file_from_fresh_labels(self):
        """⭐ The property: a file written minutes ago whose newest observation
        is days old is detectable, and detectable as the labels being stale
        rather than the file."""
        from trackers.state import TrackerHistory
        url = "udp://a.example:6969/announce"
        history = TrackerHistory.new(url, "2026-09-01T00:00:00Z").observe(
            state="live", ok=True, observed_at=ago(86400 * 4),
            rung="tracker_semantic")
        doc = json.loads(render_json(
            [parse(url)], provenance={}, histories={url: history},
            generated_at=ago(60), code_version="0.1.0"))
        self.assertFalse(assess(doc["generated_at"], now=NOW).stale)
        self.assertTrue(assess(doc["newest_observation"], now=NOW).stale)


if __name__ == "__main__":
    unittest.main()
