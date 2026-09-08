"""T-064: the channel semantics, asserted against what the platform did.

The entry's `Prove` clause is exact: *asserted by a test against real platform
behaviour, not against an assumption*. So one class here **reads the committed
result of `experiments/24-release-channel-behaviour.py`** and fails if this
module's design rests on something that run refuted.

⭐ That is the strongest form available without creating a release per test
run: the platform was measured once, the measurement is committed, and the
design is checked against it every time the suite runs. A test that instead
asserted "we assume `/releases/latest` works this way" would be checking the
assumption against itself.

Run:  python3 -m unittest tests.test_channels -v
"""

from __future__ import annotations

import glob
import json
import os
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.channels import (ROLLING_TAG, Channel, daily_tag,  # noqa: E402
                               due, iso_week_tag, promotable, tag_for)

RESULTS = os.path.join(REPO, "experiments", "results")


def platform_result() -> dict:
    """The most recent committed run of `experiments/24`."""
    paths = sorted(glob.glob(os.path.join(
        RESULTS, "24-release-channel-behaviour.*.json")))
    if not paths:
        raise unittest.SkipTest("no committed platform measurement")
    with open(paths[-1], encoding="utf-8") as handle:
        return json.load(handle)["results"]


class TheDesignMatchesWhatThePlatformDid(unittest.TestCase):
    """⭐ Each test names the design choice and the row that decided it."""

    def setUp(self):
        self.measured = platform_result()

    def test_the_rolling_channel_is_not_called_latest(self):
        """`C-14`: a release tagged `latest` did **not** hold
        `/releases/latest`; a newer release took it. So the name promises a
        consumer something the platform does not deliver, and D5 says rename
        rather than ship a naming coincidence."""
        c14 = self.measured["c14_two_releases"]
        self.assertFalse(c14["resolves_to_tag_named_latest"],
                         "the platform measurement no longer refutes the name, "
                         "so this module's reason for avoiding it is gone")
        self.assertTrue(c14["resolves_to_newest_non_prerelease"])
        self.assertNotEqual(ROLLING_TAG, "latest")
        self.assertEqual(tag_for(Channel.ROLLING, "2026-09-08T00:00:00Z"),
                         ROLLING_TAG)

    def test_a_prerelease_cannot_take_the_endpoint(self):
        """`C-14`'s second half, which is the one the channel design leans on:
        a prerelease published after a stable release does not become
        `/releases/latest`."""
        self.assertTrue(self.measured["c14_with_prerelease"]["prerelease_ignored"])

    def test_updating_a_channel_may_not_be_a_tag_move(self):
        """`C-17` was **refuted**: the tag moved and the release's
        `target_commitish` did not follow it. A design that moved tags would
        leave the release object pointing at an older commit, silently."""
        c17 = self.measured["c17_tag_move"]
        self.assertTrue(c17["tag_actually_moved"])
        self.assertFalse(c17["release_target_followed_the_tag"])
        self.assertEqual(c17["release_target_commitish_before"],
                         c17["release_target_commitish_after"])

    def test_publication_may_not_verify_by_reading_the_download_url(self):
        """`C-15`: the stable asset URL served the previous bytes after a
        replacement. Anything that confirms a publish by fetching that URL is
        asserting a read-after-write the platform does not offer."""
        c15 = self.measured["c15_asset_replacement"]
        before = c15["asset_before_replacement"]
        after = c15["asset_after_replacement"]
        self.assertNotEqual(before["id"], after["id"],
                            "the asset was not actually replaced, so this row "
                            "cannot support the rule it is cited for")
        # ⭐ The constraint made structural rather than stated: this module
        # cannot fetch anything, so it cannot verify a publish by reading the
        # download URL back. A test that grepped the docstring for the phrase
        # would pass on a module that did it anyway.
        import ast
        source = os.path.join(REPO, "src", "trackers", "channels.py")
        with open(source, encoding="utf-8") as handle:
            tree = ast.parse(handle.read(), filename="channels.py")
        imported = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                imported.update(alias.name.split(".")[0] for alias in node.names)
            elif isinstance(node, ast.ImportFrom) and node.module:
                imported.add(node.module.split(".")[0])
        self.assertEqual(
            imported & {"urllib", "http", "socket", "ssl", "subprocess"}, set(),
            "the channel logic can reach the network, so it can verify a "
            "publish by reading back a URL the platform does not promise")


class TheChannelTags(unittest.TestCase):

    def test_the_daily_tag_is_the_utc_day(self):
        self.assertEqual(daily_tag("2026-09-08T23:59:59Z"), "daily-2026-09-08")
        self.assertEqual(daily_tag("2026-09-09T00:00:01Z"), "daily-2026-09-09")

    def test_a_local_offset_is_converted_rather_than_trusted(self):
        """⛔ 2026-09-09T01:00:00+05:00 is 2026-09-08 in UTC. A channel that
        took the date off the front of the string would publish tomorrow's tag
        today, once, in one timezone, and nobody would find it."""
        self.assertEqual(daily_tag("2026-09-09T01:00:00+05:00"),
                         "daily-2026-09-08")

    def test_a_timestamp_without_a_zone_is_refused(self):
        """RULES 1.5: an unknown is not guessed at. `naive` could be any hour
        of any day, and the tag it produces would be unpredictable."""
        with self.assertRaises(ValueError):
            daily_tag("2026-09-08T12:00:00")

    def test_the_week_is_iso_8601_and_starts_on_monday(self):
        """D5: ISO unless evidence favours otherwise, and explicit either way.
        2026-09-07 is a Monday; the Sunday before it belongs to the week
        before."""
        self.assertEqual(iso_week_tag("2026-09-07T00:00:00Z"), "weekly-2026-W37")
        self.assertEqual(iso_week_tag("2026-09-06T23:59:59Z"), "weekly-2026-W36")
        self.assertEqual(iso_week_tag("2026-09-13T23:59:59Z"), "weekly-2026-W37")

    def test_a_week_straddling_a_year_takes_the_iso_year(self):
        """⚠ 2027-01-01 is a Friday, and ISO puts it in 2026's week 53. A tag
        built from `strftime('%Y')` would say 2027 and sort before every week
        of the year it actually belongs to."""
        self.assertEqual(iso_week_tag("2027-01-01T00:00:00Z"), "weekly-2026-W53")


class WhatMayTakeAChannel(unittest.TestCase):

    def test_a_failed_generation_never_moves_a_channel(self):
        ok, why = promotable(generation_succeeded=False,
                             volume_change_suspicious=False, tracker_count=1327)
        self.assertFalse(ok)
        self.assertIn("previous channel stands", why)

    def test_an_empty_output_never_moves_a_channel(self):
        """RULES 11: publishing an all-dead dataset when the probe breaks is
        the named anti-pattern, and zero trackers is its clearest shape."""
        ok, why = promotable(generation_succeeded=True,
                             volume_change_suspicious=False, tracker_count=0)
        self.assertFalse(ok)
        self.assertIn("all-dead", why)

    def test_a_suspicious_volume_change_never_moves_a_channel(self):
        ok, why = promotable(generation_succeeded=True,
                             volume_change_suspicious=True, tracker_count=900)
        self.assertFalse(ok)
        self.assertIn("suspicious", why)

    def test_a_good_run_may(self):
        ok, why = promotable(generation_succeeded=True,
                             volume_change_suspicious=False, tracker_count=1327)
        self.assertTrue(ok)
        self.assertIn("1327", why)


class WhenAChannelIsDue(unittest.TestCase):

    def test_the_rolling_channel_is_always_due(self):
        decision = due(Channel.ROLLING, "2026-09-08T12:00:00Z", {ROLLING_TAG})
        self.assertTrue(decision.publish)
        self.assertEqual(decision.tag, ROLLING_TAG)

    def test_a_period_already_published_is_not_republished(self):
        """⭐ A consumer pinning `daily-2026-09-08` pinned that day, not the
        last run inside it."""
        for channel, tag in ((Channel.DAILY, "daily-2026-09-08"),
                             (Channel.WEEKLY, "weekly-2026-W37")):
            with self.subTest(channel=channel):
                decision = due(channel, "2026-09-08T12:00:00Z", {tag})
                self.assertFalse(decision.publish)
                self.assertIn("already exists", decision.reason)

    def test_a_new_period_is_due(self):
        decision = due(Channel.DAILY, "2026-09-09T00:00:01Z",
                       {"daily-2026-09-08"})
        self.assertTrue(decision.publish)
        self.assertEqual(decision.tag, "daily-2026-09-09")

    def test_the_decision_carries_its_reason(self):
        record = due(Channel.WEEKLY, "2026-09-08T12:00:00Z", set()).as_record()
        self.assertEqual(record["channel"], "weekly")
        self.assertTrue(record["reason"])
        self.assertEqual(record["tag"], "weekly-2026-W37")


if __name__ == "__main__":
    unittest.main()
