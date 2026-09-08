"""T-041: the seven shapes a history must distinguish, one test each.

Every history here is **synthetic and written out check by check**, because the
entry asks for a series that exhibits each shape rather than a fixture that
asserts one. A test that built the verdict's inputs directly would be checking
that a comparison compares.

⛔ **A shape is not a health state and never becomes one.** Nothing in this
file imports `HealthState`, and `classify` cannot say `dead`.

⭐ **The pairs are the point.** Four failures scattered and four failures in a
row are the same rate; a short outage and a long decline both end in failure;
a new tracker and a tracker that has failed every check both have no
successes. Each pair is one test asserting the two do not collapse.

No network, no clock: every timestamp is a literal.

Run:  python3 -m unittest tests.test_shapes -v
"""

from __future__ import annotations

import os
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.shapes import (HEALTHY_RATE, MAX_TEMPORARY_RUN,  # noqa: E402
                             MIN_DAYS_FOR_TREND, MIN_OBSERVATIONS,
                             RECENT_WINDOW, THE_SEVEN, Shape, classify)
from trackers.state import RING_SIZE, TrackerHistory  # noqa: E402

#: Checks are three hours apart, which is D7's default cadence and what every
#: threshold in `shapes.py` is expressed in.
STEP_HOURS = 3


def at(index: int) -> str:
    """The timestamp of the nth check, three hours after the one before."""
    day = 1 + (index * STEP_HOURS) // 24
    hour = (index * STEP_HOURS) % 24
    return f"2026-01-{day:02d}T{hour:02d}:00:00Z"


def series(pattern: str, *, url: str = "udp://t.example:6969/announce",
           start: int = 0) -> TrackerHistory:
    """A history from a string of outcomes: `.` succeeded, `x` did not.

    Reading `"....xx"` as a series is the whole reason the shapes are legible
    in this file, and it is why the tests below can state a pattern rather
    than describe one.
    """
    history = TrackerHistory.new(url, at(start))
    for offset, mark in enumerate(pattern):
        moment = at(start + offset)
        if mark == ".":
            history = history.observe(state="live", ok=True,
                                      observed_at=moment,
                                      rung="tracker_semantic")
        elif mark == "x":
            history = history.observe(state="unknown", ok=False,
                                      observed_at=moment, rung="dns",
                                      failure="timeout")
        else:
            raise ValueError(f"a series is dots and crosses, not {mark!r}")
    return history


class TheSevenShapes(unittest.TestCase):
    """One test per shape, each over a series that exhibits it."""

    def test_1_a_new_tracker_is_new_rather_than_anything_else(self):
        """Two checks cannot tell a pattern from a coincidence.

        Asserted at every length below the floor **and in both directions**:
        two successes and two failures are both `NEW`, which is the
        distinction `state.new` protects by leaving `ewma` unset.
        """
        for length in range(MIN_OBSERVATIONS):
            for pattern in (".", "x"):
                with self.subTest(length=length, mark=pattern):
                    verdict = classify(series(pattern * length))
                    self.assertIs(verdict.shape, Shape.NEW)
                    self.assertIn("under the", verdict.detail)

    def test_2_a_short_outage_on_a_healthy_record_is_temporary(self):
        verdict = classify(series("............xx"))
        self.assertIs(verdict.shape, Shape.TEMPORARILY_UNAVAILABLE)
        self.assertEqual(verdict.evidence["trailing_failures"], 2)

    def test_3_scattered_failures_are_intermittent(self):
        """Mostly works, drops out, comes back. The rate sits between
        `UNRELIABLE_RATE` and `HEALTHY_RATE` and the series alternates."""
        verdict = classify(series("..x...x...x...x..."))
        self.assertIs(verdict.shape, Shape.INTERMITTENTLY_FAILING)
        self.assertGreater(verdict.evidence["alternations"], 2)

    def test_4_failing_more_than_it_works_is_consistently_unreliable(self):
        verdict = classify(series("x.xx.x.xxx.x.xx.x.x"))
        self.assertIs(verdict.shape, Shape.CONSISTENTLY_UNRELIABLE)
        self.assertLessEqual(verdict.evidence["ring_rate"], 0.5)

    def test_5_a_rate_that_falls_is_degrading(self):
        verdict = classify(series("..........x.x.xx.xxx"))
        self.assertIs(verdict.shape, Shape.DEGRADING)
        self.assertLess(verdict.evidence["newer_rate"],
                        verdict.evidence["older_rate"])

    def test_6_a_rate_that_rises_is_improving(self):
        verdict = classify(series("xxx.xx.x.x..........."))
        self.assertIs(verdict.shape, Shape.IMPROVING)
        self.assertGreater(verdict.evidence["newer_rate"],
                           verdict.evidence["older_rate"])

    def test_7_nothing_working_for_a_day_is_apparently_gone(self):
        verdict = classify(series("........" + "x" * RECENT_WINDOW))
        self.assertIs(verdict.shape, Shape.APPARENTLY_GONE)
        self.assertEqual(verdict.evidence["recent_successes"], 0)

    def test_the_residual_is_named_and_is_not_one_of_the_seven(self):
        """A tracker that simply works has no interesting shape."""
        verdict = classify(series("." * 20))
        self.assertIs(verdict.shape, Shape.STEADY)
        self.assertNotIn(Shape.STEADY, THE_SEVEN)
        self.assertEqual(len(THE_SEVEN), 7)


class ThePairsThatMustNotCollapse(unittest.TestCase):
    """Each of these is two series a weaker definition would call the same."""

    def test_a_new_tracker_and_one_that_has_never_worked(self):
        """Both have zero successes. One has three checks and one has two."""
        self.assertIs(classify(series("xx")).shape, Shape.NEW)
        self.assertIs(classify(series("xxx")).shape, Shape.APPARENTLY_GONE)

    def test_scattered_and_consecutive_failures_at_the_same_rate(self):
        """⭐ Four failures in twelve checks, twice. Same rate, same length,
        different fault: one tracker flaps and one has stopped."""
        scattered = classify(series(".x..x..x..x."))
        consecutive = classify(series("........xxxx"))
        self.assertEqual(scattered.evidence["ring_rate"],
                         consecutive.evidence["ring_rate"])
        self.assertIs(scattered.shape, Shape.INTERMITTENTLY_FAILING)
        self.assertIsNot(consecutive.shape, Shape.INTERMITTENTLY_FAILING)

    def test_a_dip_and_a_decline(self):
        """Both end in failure. One was fine nine hours ago."""
        self.assertIs(classify(series("...........xx")).shape,
                      Shape.TEMPORARILY_UNAVAILABLE)
        self.assertIs(classify(series(".........x.xx.xxxx")).shape,
                      Shape.DEGRADING)

    def test_a_long_outage_on_a_perfect_record_is_not_temporary(self):
        """⭐ The pair the run bound exists for, and both series have a
        spotless history behind them.

        Three failures is nine hours and reads as a dip; six is eighteen and
        does not. Without the bound the second is reported as temporary for as
        long as it lasts, because everything before it still looks healthy --
        which is a tracker that has been down since yesterday described as a
        blip. This test is why: removing `MAX_TEMPORARY_RUN` failed nothing
        until it existed.
        """
        dip = classify(series("." * 20 + "x" * MAX_TEMPORARY_RUN))
        self.assertIs(dip.shape, Shape.TEMPORARILY_UNAVAILABLE)
        longer = classify(series("." * 20 + "x" * (MAX_TEMPORARY_RUN + 3)))
        self.assertIs(longer.shape, Shape.DEGRADING)
        self.assertGreaterEqual(longer.evidence["recent_successes"], 1,
                                "this must not be reaching the gone rule "
                                "instead, or it proves nothing about the bound")

    def test_degrading_and_gone_are_read_in_that_order(self):
        """A decline that has reached zero for a day is gone, not degrading.

        The order in `classify` is the specification, and this is the pair it
        exists to separate: reversing the two rules would report a tracker
        that has been silent since yesterday as merely on its way down.
        """
        verdict = classify(series("........x.xx" + "x" * RECENT_WINDOW))
        self.assertIs(verdict.shape, Shape.APPARENTLY_GONE)

    def test_improving_from_gone_is_improving(self):
        """⛔ A tracker that came back must not stay classified by its worst
        week. This is the shape a ranking would otherwise hold down forever."""
        verdict = classify(series("xxxxxxxx" + "." * 10))
        self.assertIs(verdict.shape, Shape.IMPROVING)


class TheTrendComesFromTheRightSeries(unittest.TestCase):
    """A month-long decline is invisible in an eight-day ring."""

    def _month(self, older_per_day: int, newer_per_day: int) -> TrackerHistory:
        """`MIN_DAYS_FOR_TREND * 2` days, four checks each.

        Written a day at a time so the daily series is built by the production
        code rather than assembled here. ⚠ Each day's failures come **first**,
        so no day ends on one and the trailing-failure rule cannot fire: this
        test is about the trend and nothing else.
        """
        history = TrackerHistory.new("http://t.example/announce",
                                     "2026-02-01T00:00:00Z")
        days = MIN_DAYS_FOR_TREND * 2
        for index in range(days):
            day = f"2026-02-{index + 1:02d}"
            successes = older_per_day if index < days // 2 else newer_per_day
            for slot, hour in enumerate((0, 6, 12, 18)):
                ok = slot >= 4 - successes
                history = history.observe(
                    state="live" if ok else "unknown", ok=ok,
                    observed_at=f"{day}T{hour:02d}:00:00Z",
                    rung="tracker_semantic" if ok else "dns")
        return history

    def test_a_month_long_decline_is_seen_through_the_daily_aggregates(self):
        """⭐ The ring holds eight days. This decline is a month long, so the
        ring's own two halves both sit inside the bad end of it and show no
        trend at all -- the aggregates are the only place the fall is visible.
        """
        history = self._month(older_per_day=4, newer_per_day=1)
        verdict = classify(history)
        self.assertEqual(verdict.evidence["trend_source"], "daily")
        self.assertIs(verdict.shape, Shape.DEGRADING)
        self.assertEqual(verdict.evidence["older_rate"], 1.0)
        self.assertEqual(verdict.evidence["newer_rate"], 0.25)

    def test_the_ring_alone_would_have_missed_it(self):
        """The claim above, made falsifiable. If the ring could see this
        decline, preferring the aggregates would be decoration."""
        history = self._month(older_per_day=4, newer_per_day=1)
        half = len(history.ring) // 2
        older = sum(1 for o in history.ring[:half] if o.ok) / half
        newer = sum(1 for o in history.ring[half:] if o.ok) / (
            len(history.ring) - half)
        self.assertLess(abs(newer - older), 0.25,
                        "the ring can see this decline, so the daily "
                        "aggregates are not what makes it visible")

    def test_the_ring_is_used_while_there_are_too_few_days(self):
        verdict = classify(series("..........x.x.xx.xxx"))
        self.assertEqual(verdict.evidence["trend_source"], "ring")

    def test_a_ring_too_short_to_halve_reports_no_trend(self):
        """⛔ Never a trend from three checks against three. An absence is
        recorded as one rather than as a flat line (RULES 1.5)."""
        verdict = classify(series("...x"))
        self.assertEqual(verdict.evidence["trend_source"], "-")
        self.assertIsNone(verdict.evidence["older_rate"])


class TheVerdictCarriesItsEvidence(unittest.TestCase):
    """RULES 3.10: a classification a consumer cannot audit is a log line."""

    def test_every_number_the_rule_compared_is_on_the_record(self):
        record = classify(series("..........x.x.xx.xxx")).as_record()
        for key in ("lifetime_checks", "ring_length", "ring_rate",
                    "recent_window", "recent_successes", "trailing_failures",
                    "alternations", "trend_source", "older_rate",
                    "newer_rate"):
            self.assertIn(key, record["evidence"])
        self.assertTrue(record["detail"])
        self.assertIn(record["shape"], {s.value for s in Shape})

    def test_a_shape_is_stable_under_the_ring_rolling(self):
        """The ring caps at K, so a long steady history classifies the same as
        a short one. A shape that changed when an old outcome fell off the end
        would move for a reason nobody could see."""
        short = classify(series("." * 20))
        long_ = classify(series("." * (RING_SIZE + 40)))
        self.assertIs(short.shape, long_.shape)
        self.assertEqual(long_.evidence["ring_length"], RING_SIZE)

    def test_the_rate_that_decides_healthy_is_the_documented_one(self):
        """A series **at** `HEALTHY_RATE` is steady and one under it is not.

        The edge rather than a comfortable example, because a threshold nobody
        tests at its edge is a threshold that can move without a test noticing.
        Both series put their failures at the front and spread them evenly, so
        neither the trailing-failure rule nor the trend can fire and the rate
        is the only thing deciding.
        """
        at_the_line = series("x....x....x....x....")     # 16 of 20 = 0.80
        under_it = series("x...x...x...x...x...")        # 15 of 20 = 0.75
        self.assertEqual(classify(at_the_line).evidence["ring_rate"],
                         HEALTHY_RATE)
        self.assertIs(classify(at_the_line).shape, Shape.STEADY)
        self.assertLess(classify(under_it).evidence["ring_rate"], HEALTHY_RATE)
        self.assertIs(classify(under_it).shape, Shape.INTERMITTENTLY_FAILING)


if __name__ == "__main__":
    unittest.main()
