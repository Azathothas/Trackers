"""T-080: tell a human, once, with evidence, and stop when it clears.

The three properties the `Prove` clause names, and they are unit-testable
because the decision is a pure function:

    a repeated condition produces one issue and not many
    an issue closes when its condition genuinely clears
    no body exceeds the stated size

⛔ **An automation that cries wolf hourly gets muted, and then it is worse than
no automation.** Most of what follows is about not doing that.

No network -- `plan` touches nothing. Run:
    python3 -m unittest tests.test_issues -v
"""

from __future__ import annotations

import os
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.freshness import assess  # noqa: E402
from trackers.issues import (MAX_BODY_BYTES, Condition,  # noqa: E402
                             ExistingIssue, clamp, key_of, marker_for, plan,
                             source_conditions, staleness_condition,
                             tracker_conditions)
from trackers.pipeline import Aggregate  # noqa: E402
from trackers.state import TrackerHistory  # noqa: E402

import datetime  # noqa: E402

NOW = datetime.datetime(2026, 9, 9, 12, 0, 0, tzinfo=datetime.timezone.utc)


def a_condition(key: str = "source-failed:x") -> Condition:
    return Condition(key=key, title="something happened",
                     evidence=(("source", "x"),), action="look at it")


def an_issue(number: int, key: str, state: str = "open") -> ExistingIssue:
    return ExistingIssue(number=number, body=a_condition(key).body(),
                         state=state)


class OneIssuePerCondition(unittest.TestCase):
    """The first `Prove` property."""

    def test_a_new_condition_opens_one_issue(self):
        result = plan([a_condition()], [])
        self.assertEqual([c.key for c in result.open_new], ["source-failed:x"])
        self.assertEqual(result.update, [])
        self.assertEqual(result.close, [])

    def test_the_same_condition_again_updates_rather_than_refiling(self):
        """⛔ The anti-spam rule. Twenty-four runs a day must not be
        twenty-four issues."""
        result = plan([a_condition()], [an_issue(7, "source-failed:x")])
        self.assertEqual(result.open_new, [])
        self.assertEqual([n for n, _ in result.update], [7])

    def test_a_hundred_runs_of_one_condition_open_nothing_after_the_first(self):
        existing = [an_issue(7, "source-failed:x")]
        for _ in range(100):
            result = plan([a_condition()], existing)
            self.assertEqual(result.open_new, [])

    def test_a_duplicate_left_by_an_older_version_is_updated_not_tripled(self):
        """⚠ Two open issues carrying one key is a state this code could have
        created before it deduplicated. The lower number wins and the other is
        left for a person, rather than a third being filed."""
        result = plan([a_condition()],
                      [an_issue(9, "source-failed:x"),
                       an_issue(4, "source-failed:x")])
        self.assertEqual(result.open_new, [])
        self.assertEqual([n for n, _ in result.update], [4])

    def test_two_different_conditions_are_two_issues(self):
        result = plan([a_condition("source-failed:x"),
                       a_condition("source-empty:y")], [])
        self.assertEqual(len(result.open_new), 2)


class ItClosesWhatHasCleared(unittest.TestCase):
    """The second `Prove` property."""

    def test_a_condition_that_cleared_closes_its_issue(self):
        result = plan([], [an_issue(7, "source-failed:x")])
        self.assertEqual(result.close, [7])
        self.assertEqual(result.open_new, [])

    def test_an_already_closed_issue_is_left_alone(self):
        result = plan([], [an_issue(7, "source-failed:x", state="closed")])
        self.assertEqual(result.close, [])

    def test_a_persons_own_issue_is_never_touched(self):
        """⛔ An automation that closed somebody's issue because the title
        looked familiar has done more damage than the condition it reported."""
        human = ExistingIssue(number=3, body="The UDP probe seems slow to me.")
        result = plan([], [human])
        self.assertEqual(result.close, [])
        self.assertIsNone(human.key)

    def test_the_plan_is_the_same_twice_over_one_input(self):
        """RULES 3.6, on a decision a workflow acts on."""
        conditions = [a_condition("b"), a_condition("a")]
        existing = [an_issue(2, "gone"), an_issue(1, "a")]
        first, second = plan(conditions, existing), plan(conditions, existing)
        self.assertEqual(first.as_dict(), second.as_dict())
        self.assertEqual([c.key for c in first.open_new], ["b"])


class NoBodyExceedsTheCap(unittest.TestCase):
    """The third `Prove` property."""

    def test_an_enormous_evidence_value_is_cut_and_says_so(self):
        huge = Condition(key="k", title="t",
                         evidence=(("body", "x" * (MAX_BODY_BYTES * 2)),))
        body = huge.body()
        self.assertLessEqual(len(body.encode("utf-8")), MAX_BODY_BYTES)
        self.assertIn("truncated", body)

    def test_an_ordinary_body_is_untouched(self):
        """⚠ The positive control: a clamp that cut everything would pass the
        test above."""
        body = a_condition().body()
        self.assertNotIn("truncated", body)
        self.assertLess(len(body.encode("utf-8")), 1000)

    def test_the_clamp_never_returns_more_than_the_limit(self):
        for size in (0, 10, MAX_BODY_BYTES - 1, MAX_BODY_BYTES,
                     MAX_BODY_BYTES + 1, MAX_BODY_BYTES * 3):
            with self.subTest(size=size):
                self.assertLessEqual(
                    len(clamp("y" * size).encode("utf-8")), MAX_BODY_BYTES)


class TheMarkerIsTheIdentity(unittest.TestCase):

    def test_a_body_round_trips_its_key(self):
        self.assertEqual(key_of(a_condition("source-failed:zz").body()),
                         "source-failed:zz")

    def test_the_key_is_not_the_title(self):
        """⭐ A title can be edited by a person and a label removed; matching on
        either is how an automation files a second issue because somebody
        clarified the first one's wording."""
        edited = a_condition("source-failed:x").body().replace(
            "something happened", "Source x is broken (see also #12)")
        self.assertEqual(key_of(edited), "source-failed:x")
        result = plan([a_condition("source-failed:x")],
                      [ExistingIssue(number=5, body=edited)])
        self.assertEqual(result.open_new, [])
        self.assertEqual([n for n, _ in result.update], [5])

    def test_the_marker_is_a_comment_and_not_visible_prose(self):
        self.assertTrue(marker_for("k").startswith("<!--"))
        self.assertTrue(marker_for("k").endswith("-->"))


class TheConditionsCarryTheEvidenceTheyOwe(unittest.TestCase):

    def test_a_failed_source_says_the_dataset_is_unaffected(self):
        """⛔ RULES 3.2 in the issue text: a failed source is not an empty one,
        and a maintainer reading the issue at 3am should not think data was
        lost."""
        agg = Aggregate(sources_failed=["ngosang_all"], sources_ok=["other"])
        conditions = source_conditions(agg, observed_at="2026-09-09T00:00:00Z")
        self.assertEqual(len(conditions), 1)
        body = conditions[0].body()
        self.assertIn("ngosang_all", body)
        self.assertIn("contributed nothing and blocked nothing", body)
        self.assertIn("2026-09-09T00:00:00Z", body)

    def test_failed_rejected_and_empty_are_three_conditions(self):
        agg = Aggregate(sources_failed=["a"], sources_rejected=["b"],
                        sources_empty=["c"])
        keys = [c.key for c in source_conditions(agg, observed_at="t")]
        self.assertEqual(keys, ["source-failed:a", "source-rejected:b",
                                "source-empty:c"])

    def test_a_watched_tracker_issue_carries_the_vantage(self):
        """⭐ The whole point: a maintainer must be able to tell `dead` from
        `dead from one datacenter`."""
        url = "udp://watched.example:6969/announce"
        history = TrackerHistory.new(url, "2026-09-01T00:00:00Z")
        for i in range(3):
            history = history.observe(state="unknown", ok=False,
                                      observed_at=f"2026-09-0{1+i}T00:00:00Z",
                                      rung="dns", failure="timeout")
        conditions = tracker_conditions(
            {url: history}, watched=[url],
            vantage={"environment_class": "github-actions-hosted",
                     "ip_families": "ipv4"})
        self.assertEqual(len(conditions), 1)
        body = conditions[0].body()
        self.assertIn("github-actions-hosted", body)
        self.assertIn("one datacenter", body)
        self.assertIn("3, none successful", body)

    def test_an_unwatched_tracker_raises_nothing(self):
        """⛔ Filing for every tracker that stops answering would be a thousand
        issues, which is the spam this entry forbids."""
        url = "udp://ordinary.example:6969/announce"
        history = TrackerHistory.new(url, "2026-09-01T00:00:00Z")
        for i in range(5):
            history = history.observe(state="unknown", ok=False,
                                      observed_at=f"2026-09-0{1+i}T00:00:00Z",
                                      rung="dns", failure="timeout")
        self.assertEqual(tracker_conditions({url: history}, watched=[],
                                            vantage={}), [])

    def test_one_failure_does_not_raise_an_issue(self):
        url = "udp://watched.example:6969/announce"
        history = TrackerHistory.new(url, "2026-09-01T00:00:00Z").observe(
            state="unknown", ok=False, observed_at="2026-09-01T00:00:00Z",
            rung="dns", failure="timeout")
        self.assertEqual(tracker_conditions({url: history}, watched=[url],
                                            vantage={}), [])

    def test_a_tracker_that_ever_succeeded_raises_nothing(self):
        url = "udp://watched.example:6969/announce"
        history = TrackerHistory.new(url, "2026-09-01T00:00:00Z")
        history = history.observe(state="live", ok=True,
                                  observed_at="2026-09-01T00:00:00Z",
                                  rung="tracker_semantic")
        for i in range(3):
            history = history.observe(state="unknown", ok=False,
                                      observed_at=f"2026-09-0{2+i}T00:00:00Z",
                                      rung="dns", failure="timeout")
        self.assertEqual(tracker_conditions({url: history}, watched=[url],
                                            vantage={}), [])

    def test_a_stale_dataset_names_the_sixty_day_rule(self):
        old = (NOW - datetime.timedelta(days=5)).strftime("%Y-%m-%dT%H:%M:%SZ")
        conditions = staleness_condition(assess(old, now=NOW), generated_at=old)
        self.assertEqual(len(conditions), 1)
        self.assertIn("60 days", conditions[0].body())

    def test_a_fresh_dataset_raises_nothing(self):
        recent = (NOW - datetime.timedelta(minutes=5)).strftime(
            "%Y-%m-%dT%H:%M:%SZ")
        self.assertEqual(
            staleness_condition(assess(recent, now=NOW), generated_at=recent),
            [])


class FortyEightHoursNotThreeFailures(unittest.TestCase):
    """T-047. ⛔ The number is the entry, and the two are not the same.

    Three failed observations at a three-hour cadence is **nine** hours, which
    is a bad afternoon. Forty-eight hours is a tracker that has gone.
    """

    URL = "udp://watched.example:6969/announce"

    def _history(self, *, days: float, observations: int = 4):
        history = TrackerHistory.new(self.URL, "2026-09-01T00:00:00Z")
        step = (days * 24) / max(observations - 1, 1)
        for index in range(observations):
            hours = int(round(index * step))
            stamp = (datetime.datetime(2026, 9, 1, tzinfo=datetime.timezone.utc)
                     + datetime.timedelta(hours=hours))
            history = history.observe(
                state="unknown", ok=False,
                observed_at=stamp.strftime("%Y-%m-%dT%H:%M:%SZ"),
                rung="dns", failure="timeout")
        return history

    def _conditions(self, history):
        return tracker_conditions({self.URL: history}, watched=[self.URL],
                                  vantage={"environment_class": "ci"})

    def test_forty_eight_hours_of_failure_raises_one_issue(self):
        conditions = self._conditions(self._history(days=2))
        self.assertEqual(len(conditions), 1)
        self.assertIn("48 hours", conditions[0].body())

    def test_a_day_of_failure_raises_nothing(self):
        """⚠ The boundary from below. Without this the threshold could be zero
        and every test above would still pass."""
        self.assertEqual(self._conditions(self._history(days=1)), [])

    def test_a_burst_of_failures_in_one_hour_raises_nothing(self):
        """⛔ Enough observations is not enough time. Six failures inside an
        hour is an outage in progress, not a tracker that has gone."""
        history = TrackerHistory.new(self.URL, "2026-09-01T00:00:00Z")
        for minute in range(0, 60, 10):
            history = history.observe(
                state="unknown", ok=False,
                observed_at=f"2026-09-01T00:{minute:02d}:00Z",
                rung="dns", failure="timeout")
        self.assertEqual(self._conditions(history), [])

    def test_a_second_run_updates_rather_than_duplicating(self):
        """The other half of T-047's `Prove` clause."""
        conditions = self._conditions(self._history(days=3))
        first = plan(conditions, [])
        self.assertEqual(len(first.open_new), 1)
        existing = [ExistingIssue(number=11, body=first.open_new[0].body())]
        second = plan(conditions, existing)
        self.assertEqual(second.open_new, [])
        self.assertEqual([n for n, _ in second.update], [11])

    def test_the_entry_is_never_deleted_for_being_unreachable(self):
        """⛔ The decision this entry exists to enforce. RULES 3.4: a tracker
        may be unreachable from one datacenter and fine everywhere else, so
        removal is the maintainer's call and not ours.

        Asserted where it could actually happen: the categories and the
        renderer, over a tracker with three days of nothing but failure.
        """
        from trackers.categories import select_all
        from trackers.normalize import parse
        from trackers.pipeline import render_plaintext

        tracker = parse(self.URL)
        histories = {self.URL: self._history(days=3)}
        cats = select_all([tracker], provenance={}, source_categories={},
                          histories=histories, hardcoded=[tracker])
        self.assertIn(self.URL,
                      [t.url for t in cats["hardcoded"].trackers],
                      "an unreachable hardcoded entry was dropped")
        self.assertIn(self.URL, render_plaintext([tracker]))

    def test_the_issue_says_how_long_and_from_where(self):
        body = self._conditions(self._history(days=5))[0].body()
        self.assertIn("unreachable for", body)
        self.assertIn("environment_class=ci", body)
        self.assertIn("one datacenter", body)


if __name__ == "__main__":
    unittest.main()
