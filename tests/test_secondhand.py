"""T-031: a signal somebody else observed is never reported as our measurement.

The entry's `Prove` clause names this test specifically, and the reason is that
the failure it guards against is invisible: a second-hand signal that acquires
a `health_state` on its way through a file is indistinguishable from a probe
result by the time a consumer reads it, and the dataset is then claiming to
have measured something it never touched.

⛔ **The tests here are refusals.** Each one plants the mistake and requires it
to be impossible rather than merely absent.

Run:  python3 -m unittest tests.test_secondhand -v
"""

from __future__ import annotations

import json
import os
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.secondhand import (PROBE_ONLY_KEYS, PROVENANCE,  # noqa: E402
                                 Observation, Signal, annotate)


def observation(**overrides) -> Observation:
    fields = dict(
        url="udp://ipv6.example:6969/announce",
        observer="newtrackon",
        observed_at="2026-09-08T12:00:00Z",
        method="announces to the tracker and derives uptime",
        signal=Signal.ALIVE,
        reach="an observer with IPv6 egress, which this vantage does not have",
    )
    fields.update(overrides)
    return Observation(**fields)


class ItCannotLookLikeAMeasurement(unittest.TestCase):

    def test_the_record_carries_no_key_a_probe_owns(self):
        record = observation().as_record()
        for key in PROBE_ONLY_KEYS:
            with self.subTest(key=key):
                self.assertNotIn(key, record)

    def test_the_record_says_it_is_not_direct_and_there_is_no_way_to_say_it_is(self):
        """`direct` is not a parameter. A caller cannot set it to `True`
        because nothing accepts it."""
        self.assertIs(observation().as_record()["direct"], False)
        with self.assertRaises(TypeError):
            Observation(url="u", observer="o", observed_at="t", method="m",
                        signal=Signal.ALIVE, reach="r", direct=True)

    def test_annotating_a_health_record_leaves_its_verdict_alone(self):
        """⛔ The one that matters. An `unmeasurable` tracker with three
        observers reporting it alive is still `unmeasurable` to us."""
        health = {"url": "udp://ipv6.example:6969/announce",
                  "health_state": "unmeasurable",
                  "measurement_rung": "none", "failure": "unsupported"}
        annotated = annotate(health, [observation(), observation(
            observer="a proxy with IPv6 egress",
            method="HTTP scrape through an approved read proxy")])
        self.assertEqual(annotated["health_state"], "unmeasurable")
        self.assertEqual(annotated["measurement_rung"], "none")
        self.assertEqual(len(annotated["second_hand"]), 2)
        self.assertTrue(all(not o["direct"] for o in annotated["second_hand"]))

    def test_annotating_does_not_mutate_what_it_was_given(self):
        health = {"url": "u", "health_state": "unmeasurable"}
        annotate(health, [observation()])
        self.assertNotIn("second_hand", health)

    def test_it_survives_a_round_trip_through_a_file(self):
        """The failure this guards is one that happens *after* serialisation,
        so the assertion is made on the far side of one."""
        record = json.loads(json.dumps(observation().as_record()))
        self.assertIs(record["direct"], False)
        self.assertIn("second-hand", record["provenance"])
        self.assertFalse(PROBE_ONLY_KEYS & set(record))


class AnObservationWithoutProvenanceIsUnusable(unittest.TestCase):

    def test_every_provenance_field_is_required(self):
        for missing in ("url", "observer", "observed_at", "method", "reach"):
            with self.subTest(missing=missing):
                with self.assertRaises(ValueError) as caught:
                    observation(**{missing: ""})
                self.assertIn(missing, str(caught.exception))

    def test_an_unassessed_observer_is_not_a_negative_one(self):
        """RULES 2: an absence is not a zero. An observer that has never looked
        at a tracker has not reported it down."""
        self.assertEqual(len(Signal), 3)
        self.assertIsNot(Signal.UNASSESSED, Signal.NOT_ALIVE)
        record = observation(signal=Signal.UNASSESSED).as_record()
        self.assertEqual(record["signal"], "unassessed")

    def test_the_provenance_string_says_what_it_is(self):
        self.assertIn("second-hand", PROVENANCE)
        self.assertIn("never merged with a direct probe", PROVENANCE)


if __name__ == "__main__":
    unittest.main()
