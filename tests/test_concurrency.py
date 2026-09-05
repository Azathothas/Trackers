"""T-029 and T-024: the bounds a sweep runs under, and the records it emits.

Three of these are the entry's `Prove` clause, and each is a way the sweep
could be wrong in a direction that costs somebody else rather than us:

  * a tracker not reached before the deadline must be `unknown`, **never**
    `dead` -- running out of time is a fact about us;
  * no two probes may be in flight against one host, in either profile;
  * the per-tracker UDP budget is `5 * max(timeout / 3, floor)`.

⭐ **The concurrency tests observe the sweep rather than trusting it.** A test
that asserted `max_concurrency` was passed to a pool would pass over a pool
that ignored it. These count what is actually in flight, from inside the probe
the sweep calls.

The end-to-end case probes real trackers this project controls, on loopback,
and runs the real vantage gate over the records that come out. Nothing here
touches the network.

Run:  python3 -m unittest tests.test_concurrency -v
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import threading
import time
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from fake_tracker import Behaviour, FakeUdpTracker  # noqa: E402
from trackers.model import HealthState, Rung, Transport  # noqa: E402
from trackers.normalize import parse  # noqa: E402
from trackers.probe import Failure, ProbeResult  # noqa: E402
from trackers.profile import budget_for  # noqa: E402
from trackers.sweep import (SweepConfig, UDP_ATTEMPT_FLOOR,  # noqa: E402
                            UDP_WORST_CASE_ATTEMPTS, render_sweep, select,
                            sweep, udp_attempt_timeout, udp_budget)
from trackers.vantage import Vantage, detect  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def loopback_vantage() -> Vantage:
    v = detect()
    if "ipv4" in v.ip_families:
        return v
    return Vantage(
        environment_class=v.environment_class, probe_version=v.probe_version,
        probe_code_sha256=v.probe_code_sha256, ip_families=("ipv4",),
        ip_families_method="forced ipv4 for loopback tests",
        ipv6_stack_present=v.ipv6_stack_present,
        ipv6_route_present=v.ipv6_route_present)


def corpus(n: int, *, hosts: int | None = None):
    """`n` trackers spread over `hosts` distinct hostnames."""
    hosts = n if hosts is None else hosts
    return [parse(f"udp://h{i % hosts}.example:{6969 + i}/announce")
            for i in range(n)]


def ok_result(tracker, *_args, **_kw) -> ProbeResult:
    return ProbeResult(url=tracker.url, transport=tracker.transport,
                       network=tracker.network, rung=Rung.PROTOCOL_VALID,
                       ok=True, detail="stub")


class UdpBudget(unittest.TestCase):
    """`Prove` case 3. The arithmetic, not the seconds."""

    def test_the_budget_is_five_attempts_of_a_third_of_the_timeout(self):
        for timeout in (3.0, 6.0, 30.0, 90.0):
            with self.subTest(timeout=timeout):
                expected = UDP_WORST_CASE_ATTEMPTS * max(timeout / 3.0,
                                                         UDP_ATTEMPT_FLOOR)
                self.assertAlmostEqual(udp_budget(timeout), expected)

    def test_the_floor_decides_below_three_seconds(self):
        """`--tracker-timeout 1s` and `3s` cost the same, and that is the
        floor doing its job rather than a bug."""
        self.assertEqual(udp_attempt_timeout(1.0), UDP_ATTEMPT_FLOOR)
        self.assertEqual(udp_attempt_timeout(3.0), UDP_ATTEMPT_FLOOR)
        self.assertEqual(udp_budget(1.0), udp_budget(3.0))

    def test_above_the_floor_it_scales_with_the_timeout(self):
        self.assertAlmostEqual(udp_attempt_timeout(30.0), 10.0)
        self.assertAlmostEqual(udp_budget(30.0), 50.0)

    def test_bep15s_own_schedule_is_not_what_is_implemented(self):
        """BEP 15 says up to 62 minutes for one tracker. A diagnostic that
        takes an hour to say a tracker is down has not answered the question."""
        self.assertLess(udp_budget(30.0), 60.0)

    def test_the_probe_config_carries_three_attempts_for_the_connect_half(self):
        cfg = SweepConfig(timeout=30.0).probe_config()
        self.assertEqual(cfg.retries, 2, "retries is attempts minus one")
        self.assertAlmostEqual(cfg.timeout, 10.0)


class PerHostSerialisation(unittest.TestCase):
    """`Prove` case 2, observed from inside the probe."""

    def test_one_host_never_sees_two_probes_at_once(self):
        live: dict[str, int] = {}
        overlaps: list[str] = []
        guard = threading.Lock()

        def watching(tracker, *_args, **_kw) -> ProbeResult:
            with guard:
                live[tracker.host] = live.get(tracker.host, 0) + 1
                if live[tracker.host] > 1:
                    overlaps.append(tracker.host)
            time.sleep(0.02)
            with guard:
                live[tracker.host] -= 1
            return ok_result(tracker)

        # Many URLs, few hosts: without the per-host lock this collides.
        sweep(corpus(40, hosts=4), budget=budget_for("local"),
              vantage=loopback_vantage(), probe_fn=watching)
        self.assertEqual(overlaps, [], "two probes hit one host at once")

    def test_distinct_hosts_do_run_concurrently(self):
        """The positive control. A sweep that serialised *everything* would
        pass the test above and take an hour on the real corpus."""
        peak = 0
        live = 0
        guard = threading.Lock()

        def watching(tracker, *_args, **_kw) -> ProbeResult:
            nonlocal peak, live
            with guard:
                live += 1
                peak = max(peak, live)
            time.sleep(0.05)
            with guard:
                live -= 1
            return ok_result(tracker)

        sweep(corpus(16, hosts=16), budget=budget_for("local"),
              vantage=loopback_vantage(), probe_fn=watching)
        self.assertGreater(peak, 1, "the sweep ran entirely serially")

    def test_concurrency_never_exceeds_the_profiles_bound(self):
        peak = 0
        live = 0
        guard = threading.Lock()
        bound = budget_for("ci").max_concurrency

        def watching(tracker, *_args, **_kw) -> ProbeResult:
            nonlocal peak, live
            with guard:
                live += 1
                peak = max(peak, live)
            time.sleep(0.02)
            with guard:
                live -= 1
            return ok_result(tracker)

        sweep(corpus(60, hosts=60), budget=budget_for("ci"),
              vantage=loopback_vantage(), probe_fn=watching)
        self.assertLessEqual(peak, bound, f"{peak} in flight, bound is {bound}")


class Deadline(unittest.TestCase):
    """`Prove` case 1. Running out of time is a fact about us."""

    def test_what_the_deadline_cut_off_is_unknown_and_never_dead(self):
        clock = [0.0]

        def creeping(tracker, *_args, **_kw) -> ProbeResult:
            clock[0] += 1.0        # every probe consumes a second
            return ok_result(tracker)

        result = sweep(corpus(20, hosts=20),
                       config=SweepConfig(deadline_seconds=5.0),
                       budget=budget_for("ci"), vantage=loopback_vantage(),
                       monotonic=lambda: clock[0], probe_fn=creeping)

        self.assertTrue(result.deadline_hit)
        self.assertGreater(result.not_reached, 0)
        states = {r["url"]: r["health_state"] for r in result.records}
        missed = [r for r in result.records
                  if r["failure"] == Failure.DEADLINE_EXCEEDED.value]
        self.assertGreater(len(missed), 0)
        for record in missed:
            with self.subTest(url=record["url"]):
                self.assertEqual(record["health_state"],
                                 HealthState.UNKNOWN.value)
        self.assertNotIn(HealthState.DEAD.value, set(states.values()))

    def test_a_record_exists_for_every_selected_tracker(self):
        """A tracker that vanishes because the run ran out of time is worse
        than one recorded `unknown`: the consumer cannot tell it was skipped."""
        clock = [0.0]

        def creeping(tracker, *_args, **_kw) -> ProbeResult:
            clock[0] += 1.0
            return ok_result(tracker)

        trackers = corpus(20, hosts=20)
        result = sweep(trackers, config=SweepConfig(deadline_seconds=3.0),
                       budget=budget_for("ci"), vantage=loopback_vantage(),
                       monotonic=lambda: clock[0], probe_fn=creeping)
        self.assertEqual(len(result.records), len(trackers))

    def test_no_deadline_means_everything_is_probed(self):
        trackers = corpus(12, hosts=12)
        result = sweep(trackers, budget=budget_for("local"),
                       vantage=loopback_vantage(), probe_fn=ok_result)
        self.assertEqual(result.not_reached, 0)
        self.assertEqual(result.probed, len(trackers))


class Selection(unittest.TestCase):
    """What a profile probes, and why it is not the first N."""

    def setUp(self):
        self.mixed = (
            [parse(f"udp://u{i}.example:6969/announce") for i in range(50)]
            + [parse(f"http://h{i}.example/announce") for i in range(50)]
            + [parse(f"https://s{i}.example/announce") for i in range(50)])

    def test_a_sample_keeps_every_transport(self):
        """`Tracker.sort_key` leads with the transport, so taking the head of
        a sorted corpus samples one transport and a broken UDP path would
        never appear in a `ci` run."""
        budget = budget_for("ci")
        chosen = select(self.mixed, budget)
        self.assertEqual(len(chosen), min(budget.sample_size, len(self.mixed)))
        transports = {t.transport for t in chosen}
        self.assertEqual(transports,
                         {Transport.UDP, Transport.HTTP, Transport.HTTPS})

    def test_local_takes_the_whole_corpus(self):
        self.assertEqual(len(select(self.mixed, budget_for("local"))),
                         len(self.mixed))

    def test_selection_is_deterministic(self):
        budget = budget_for("ci")
        first = [t.url for t in select(self.mixed, budget)]
        shuffled = list(reversed(self.mixed))
        second = [t.url for t in select(shuffled, budget)]
        self.assertEqual(first, second,
                         "selection depends on input order (RULES 3.6)")

    def test_a_corpus_smaller_than_the_sample_is_taken_whole(self):
        small = self.mixed[:5]
        self.assertEqual(len(select(small, budget_for("ci"))), 5)


class RecordsSatisfyTheVantageGate(unittest.TestCase):
    """T-024, end to end, against trackers this project controls."""

    def test_the_emitted_document_passes_check_vantage_metadata(self):
        v = loopback_vantage()
        with FakeUdpTracker(Behaviour.CORRECT) as good, \
                FakeUdpTracker(Behaviour.TIMEOUT) as quiet:
            trackers = [
                parse(f"udp://127.0.0.1:{good.port}/announce"),
                parse(f"udp://127.0.0.1:{quiet.port}/announce"),
                parse("http://tracker.i2p/announce"),
                parse("wss://tracker.example.invalid:443/announce"),
            ]
            result = sweep(trackers, config=SweepConfig(timeout=1.0),
                           budget=budget_for("local"), vantage=v,
                           observed_at="1970-01-01T00:00:00Z")

        doc = render_sweep(result, generated_at="1970-01-01T00:00:00Z",
                           vantage=v, budget=budget_for("local"),
                           config=SweepConfig(timeout=1.0))
        self.assertEqual(len(doc["trackers"]), 4)

        with tempfile.TemporaryDirectory(prefix="trackers-sweep-") as out:
            path = os.path.join(out, "health.json")
            with open(path, "w", encoding="utf-8", newline="\n") as fh:
                json.dump(doc, fh, indent=2, sort_keys=True)
            proc = subprocess.run(
                [sys.executable, os.path.join(REPO, "scripts",
                                              "check-vantage-metadata.py"),
                 "--path", out],
                capture_output=True, text=True, encoding="utf-8",
                errors="replace")
        self.assertEqual(proc.returncode, 0,
                         f"the gate rejected our own records:\n"
                         f"{proc.stdout}\n{proc.returncode}")

    def test_the_unreachable_ones_are_unmeasurable_and_were_never_probed(self):
        v = loopback_vantage()
        trackers = [parse("http://tracker.i2p/announce"),
                    parse("wss://tracker.example.invalid:443/announce")]
        result = sweep(trackers, budget=budget_for("local"), vantage=v)
        self.assertEqual(result.unmeasurable, 2)
        self.assertEqual(result.probed, 0)
        for record in result.records:
            with self.subTest(url=record["url"]):
                self.assertEqual(record["health_state"],
                                 HealthState.UNMEASURABLE.value)

    def test_records_come_out_in_corpus_order_not_completion_order(self):
        """Two runs over one corpus must be byte-identical (RULES 3.6), and
        completion order is whatever the thread pool happened to do."""
        trackers = corpus(30, hosts=30)

        def jittery(tracker, *_args, **_kw) -> ProbeResult:
            time.sleep((hash(tracker.host) % 7) / 1000.0)
            return ok_result(tracker)

        first = sweep(trackers, budget=budget_for("local"),
                      vantage=loopback_vantage(), probe_fn=jittery)
        second = sweep(trackers, budget=budget_for("local"),
                       vantage=loopback_vantage(), probe_fn=jittery)
        self.assertEqual([r["url"] for r in first.records],
                         [r["url"] for r in second.records])

    def test_every_record_carries_its_vantage_and_a_rung(self):
        result = sweep(corpus(5, hosts=5), budget=budget_for("local"),
                       vantage=loopback_vantage(), probe_fn=ok_result)
        for record in result.records:
            with self.subTest(url=record["url"]):
                self.assertIn("environment_class", record["vantage"])
                self.assertIn("probe_version", record["vantage"])
                self.assertIn("ip_families", record["vantage"])
                self.assertTrue(record["measurement_rung"])

    def test_one_observation_can_never_reach_dead(self):
        """`MIN_SAMPLES_FOR_DEATH` is 3, and accumulating samples across runs
        is T-040's job. A single sweep must not be able to kill anything."""
        def failing(tracker, *_args, **_kw) -> ProbeResult:
            return ProbeResult(url=tracker.url, transport=tracker.transport,
                               network=tracker.network, rung=Rung.DNS,
                               ok=False, failure=Failure.TIMEOUT,
                               detail="stub timeout")

        result = sweep(corpus(10, hosts=10), budget=budget_for("local"),
                       vantage=loopback_vantage(), probe_fn=failing)
        states = {r["health_state"] for r in result.records}
        self.assertNotIn(HealthState.DEAD.value, states)
        self.assertEqual(states, {HealthState.UNKNOWN.value})


if __name__ == "__main__":
    unittest.main()
