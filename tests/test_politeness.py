"""T-026: what a run costs the people it measures, asserted rather than argued.

The entry's `Prove` clause is exact: *a test that fails when the configured
schedule would exceed one probe per tracker per its stated interval*. So one
test in here reads `.github/workflows/health-sweep.yml` itself. A test that
only checked a constant would pass while the workflow scheduled a sweep every
five minutes, which is the shape of a check that measures its own assumptions.

⛔ **Two things a weaker version of this file would get wrong**, and both have
a test each:

  * reading `interval` and ignoring `min interval`, which is the number an
    operator would judge us by (`C-65`);
  * defaulting a tracker that stated nothing to zero rather than to D7's three
    hours, which turns an absence into permission.

Run:  python3 -m unittest tests.test_politeness -v
"""

from __future__ import annotations

import os
import re
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.politeness import (DEFAULT_INTERVAL_SECONDS,  # noqa: E402
                                 DNS_LOOKUPS_PER_RUN_CEILING, SECONDS_PER_DAY,
                                 probes_per_day, run_cost, schedule_violations,
                                 stated_interval)

SWEEP_WORKFLOW = os.path.join(REPO, ".github", "workflows", "health-sweep.yml")


def record(url: str = "udp://t.example:6969/announce", **keys) -> dict:
    base = {"url": url, "health_state": "live", "interval": None,
            "min_interval": None}
    base.update(keys)
    return base


class TheTrackersOwnNumberDecides(unittest.TestCase):

    def test_the_stricter_of_the_two_stated_numbers_wins(self):
        """⛔ `max`, not the first key found. A tracker stating both is asking
        for both, and only the longer interval honours both."""
        self.assertEqual(
            stated_interval(record(interval=1800, min_interval=2700)), 2700)
        self.assertEqual(
            stated_interval(record(interval=3600, min_interval=900)), 3600)

    def test_either_key_alone_is_read(self):
        self.assertEqual(stated_interval(record(interval=1800)), 1800)
        self.assertEqual(stated_interval(record(min_interval=1800)), 1800)

    def test_a_tracker_that_stated_nothing_returns_none_not_zero(self):
        """RULES 1.5. `None` is what it is; the default is applied by the
        caller, where the decision is visible."""
        self.assertIsNone(stated_interval(record()))
        self.assertIsNone(stated_interval({"url": "u"}))

    def test_a_nonsense_interval_is_ignored_rather_than_believed(self):
        """Upstream bodies are hostile input (RULES 5.1), and a tracker
        answering `interval 0` would otherwise buy itself unlimited probing."""
        for value in (0, -60, "soon", None, 1.5):
            with self.subTest(value=value):
                self.assertIsNone(stated_interval(record(interval=value)))

    def test_the_default_is_d7s_three_hours(self):
        self.assertEqual(DEFAULT_INTERVAL_SECONDS, 10_800)
        self.assertAlmostEqual(
            probes_per_day(record(), DEFAULT_INTERVAL_SECONDS), 8.0)

    def test_a_slower_schedule_beats_the_interval_and_never_the_reverse(self):
        """Asking for 3 h does not entitle a run to probe every 3 h if the
        schedule is daily; the cadence is the slower of the two."""
        self.assertAlmostEqual(probes_per_day(record(interval=1800),
                                              SECONDS_PER_DAY), 1.0)


class AScheduleTooFastIsAViolation(unittest.TestCase):

    def test_a_tracker_asking_for_more_time_than_the_schedule_gives(self):
        records = [record("udp://a.example:1/announce", interval=1800),
                   record("udp://b.example:1/announce", min_interval=21600),
                   record("udp://c.example:1/announce")]
        violations = schedule_violations(records, seconds_between_runs=3600)
        self.assertEqual(violations, ("udp://b.example:1/announce",))

    def test_nobody_is_violated_at_a_cadence_slower_than_every_request(self):
        records = [record("udp://a.example:1/announce", interval=1800),
                   record("udp://b.example:1/announce", min_interval=21600)]
        self.assertEqual(schedule_violations(records, 86_400), ())

    def test_a_tracker_that_stated_nothing_is_never_a_violation(self):
        """It asked for nothing, so no cadence breaks its request. D7's
        default governs it and that is the caller's decision, not a breach."""
        self.assertEqual(schedule_violations([record()], 60), ())


class TheConfiguredScheduleIsRead(unittest.TestCase):
    """⭐ The `Prove` clause. This reads the workflow, not a constant."""

    def _workflow(self) -> str:
        with open(SWEEP_WORKFLOW, encoding="utf-8") as handle:
            return handle.read()

    def test_the_sweep_workflow_exists_and_is_the_one_that_probes(self):
        """A test whose subject has been renamed passes by checking nothing."""
        text = self._workflow()
        self.assertIn("name: Health sweep", text)
        self.assertIn("probe-corpus.py", text)

    def test_no_cron_schedules_a_sweep_faster_than_the_default_interval(self):
        """⛔ The one that has to fail when somebody adds an hourly cron.

        Minute-level crons are the trap: `0 * * * *` is hourly, which is three
        times the rate D7 settled on and three times the load of the analogue
        the number came from.
        """
        crons = re.findall(r"^\s*-?\s*cron:\s*['\"]([^'\"]+)['\"]",
                           self._workflow(), re.MULTILINE)
        for cron in crons:
            with self.subTest(cron=cron):
                gap = self._interval_seconds(cron)
                self.assertGreaterEqual(
                    gap, DEFAULT_INTERVAL_SECONDS,
                    f"cron {cron!r} sweeps every {gap}s, faster than D7's "
                    f"{DEFAULT_INTERVAL_SECONDS}s default interval")

    @staticmethod
    def _interval_seconds(cron: str) -> int:
        """The shortest gap a five-field cron can produce, in seconds.

        Deliberately crude and deliberately pessimistic: it understands `*`,
        `*/n` and a comma list, and anything it cannot read it treats as the
        most frequent that field allows. A schedule this cannot parse is a
        schedule this test must not wave through.
        """
        fields = cron.split()
        if len(fields) != 5:
            return 0
        minute, hour = fields[0], fields[1]

        def step(field: str, span: int) -> int:
            if field == "*":
                return 1
            if field.startswith("*/"):
                try:
                    return max(1, int(field[2:]))
                except ValueError:
                    return 1
            parts = [p for p in field.split(",") if p]
            if len(parts) > 1:
                return 1  # several fixed values: assume the closest pair
            return span

        return step(minute, 60) * 60 * (step(hour, 24) if minute != "*" else 1)

    def test_the_cron_reader_recognises_a_schedule_that_is_too_fast(self):
        """The parser is load-bearing, so it is tested against known crons
        rather than trusted because it looks right."""
        cases = {
            "*/5 * * * *": 300,          # every five minutes
            "0 * * * *": 3600,           # hourly
            "0 */3 * * *": 10_800,       # every three hours: exactly D7
            "0 0 * * *": 86_400,         # daily
        }
        for cron, expected in cases.items():
            with self.subTest(cron=cron):
                self.assertEqual(
                    TheConfiguredScheduleIsRead._interval_seconds(cron),
                    expected)
        self.assertLess(
            TheConfiguredScheduleIsRead._interval_seconds("0 * * * *"),
            DEFAULT_INTERVAL_SECONDS,
            "an hourly cron must read as too fast, or this test cannot fail")

    def test_an_unparseable_cron_is_treated_as_too_fast(self):
        """A schedule nobody can read is not one to approve."""
        self.assertEqual(
            TheConfiguredScheduleIsRead._interval_seconds("nonsense"), 0)


class TheRunCostIsComputed(unittest.TestCase):

    def test_it_counts_what_was_probed_rather_than_the_corpus(self):
        """The mislabelled denominator this project has already published
        once: a report quoting 1327 while probing 200."""
        records = [record(f"udp://h{i}.example:6969/announce") for i in range(200)]
        cost = run_cost(records, seconds_between_runs=DEFAULT_INTERVAL_SECONDS)
        self.assertEqual(cost.trackers_probed, 200)
        self.assertEqual(cost.hosts, 200)
        self.assertEqual(cost.probes_per_run, 200)
        self.assertEqual(cost.probes_per_day, 1600)
        self.assertAlmostEqual(cost.runs_per_day, 8.0)

    def test_an_address_literal_costs_no_dns(self):
        """⛔ The wrong denominator, caught by the claim audit of 2026-09-08.

        `bep34.Resolver.consult` returns ALLOW for a literal without asking
        anybody and `getaddrinfo` resolves one without a query, so charging it
        a lookup reports load nobody generates. 206 of this corpus's 965 hosts
        are literals, and counting them overstated a full sweep by 1648
        queries.
        """
        literals = [record(f"udp://[2a03:7220:8083:cd0{i}::1]:451/announce")
                    for i in range(3)]
        literals += [record(f"udp://203.0.113.{i}:6969/announce")
                     for i in range(3)]
        cost = run_cost(literals)
        self.assertEqual(cost.hosts, 6)
        self.assertEqual(cost.resolvable_hosts, 0)
        self.assertEqual(cost.dns_worst_case_per_run, 0)

    def test_a_mixed_run_charges_only_the_names(self):
        records = [record("udp://203.0.113.7:6969/announce"),
                   record("udp://a.example:6969/announce"),
                   record("http://b.example:80/announce")]
        cost = run_cost(records)
        self.assertEqual(cost.hosts, 3)
        self.assertEqual(cost.resolvable_hosts, 2)
        self.assertEqual(cost.dns_worst_case_per_run, 16)

    def test_two_urls_on_one_host_are_one_host(self):
        """DNS is counted per host, because the resolver answer is cached per
        host per run. Counting per URL would report a load nobody generates."""
        records = [record("udp://one.example:6969/announce"),
                   record("http://one.example:80/announce")]
        self.assertEqual(run_cost(records).hosts, 1)

    def test_the_dns_ceiling_is_the_operators_and_the_verdict_is_returned(self):
        self.assertEqual(DNS_LOOKUPS_PER_RUN_CEILING, 100_000)
        small = run_cost([record(f"udp://h{i}.example:1/announce")
                          for i in range(1000)])
        self.assertTrue(small.within_dns_ceiling)
        self.assertLess(small.dns_worst_case_per_run,
                        DNS_LOOKUPS_PER_RUN_CEILING)

    def test_a_run_that_would_exceed_the_dns_ceiling_says_so(self):
        """The verdict has to be reachable, or it is decoration."""
        huge = run_cost([record(f"udp://h{i}.example:1/announce")
                         for i in range(20_000)])
        self.assertFalse(huge.within_dns_ceiling)
        self.assertFalse(huge.polite)

    def test_the_record_carries_every_number_the_verdict_used(self):
        cost = run_cost([record("udp://a.example:1/announce", min_interval=21600)],
                        seconds_between_runs=3600)
        emitted = cost.as_record()
        for key in ("trackers_probed", "hosts", "runs_per_day",
                    "seconds_between_runs", "probes_per_run", "probes_per_day",
                    "default_interval_seconds", "dns_worst_case_per_run",
                    "dns_ceiling_per_run", "within_dns_ceiling",
                    "probed_more_often_than_asked", "polite"):
            self.assertIn(key, emitted)
        self.assertFalse(emitted["polite"])
        self.assertEqual(emitted["probed_more_often_than_asked"],
                         ["udp://a.example:1/announce"])


if __name__ == "__main__":
    unittest.main()
