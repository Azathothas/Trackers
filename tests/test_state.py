"""T-040 and T-042: per-tracker history, and what it refuses to do.

Three properties matter more than the rest, and two of them are refusals:

  * a history **survives a full pipeline run** and is readable by a **fresh
    process** -- T-040's `Prove` clause, and the reason it names a fresh process
    is that an in-memory round trip proves the objects, not the file;
  * **bootstrap from nothing succeeds** -- the first run has no state and that
    is normal, not an incident;
  * ⛔ **corrupt state fails safely WITHOUT reinitialising.** RULES 3.9: a
    clean rebuild that discards history is data loss wearing the costume of a
    fix, and the tempting implementation catches the parse error and starts an
    empty file.

⭐ **The determinism tests are not decoration.** RULES 3.6 makes byte-identical
output a correctness property that CI asserts, and a state file is the one
artefact here that is rewritten on every run -- so a set's iteration order or a
platform's float repr leaking into it would make every run a diff.

No network. No clock: every timestamp in this file is a literal, because the
module under test takes its clock as an argument and a test that let it read
one would be testing something else.

Run:  python3 -m unittest tests.test_state -v
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.state import (DAILY_DAYS, EWMA_ALPHA, RING_SIZE,  # noqa: E402
                            STATE_FORMAT, CorruptState, TrackerHistory,
                            apply_sweep, bootstrap, parse_line, read_state,
                            render_line, write_state)

T0 = "2026-01-01T00:00:00Z"


def live(h: TrackerHistory, at: str) -> TrackerHistory:
    return h.observe(state="live", ok=True, observed_at=at,
                     rung="tracker_semantic")


def failed(h: TrackerHistory, at: str, failure: str = "timeout") -> TrackerHistory:
    return h.observe(state="unknown", ok=False, observed_at=at, rung="dns",
                     failure=failure)


class ANewTrackerIsNotAFailingOne(unittest.TestCase):
    """The first of T-041's seven shapes, and the easiest to lose."""

    def test_a_new_tracker_has_no_rate_rather_than_a_zero_rate(self):
        h = TrackerHistory.new("udp://a.example:6969/announce", T0)
        self.assertIsNone(h.ewma,
                          "a never-checked tracker with rate 0.0 is "
                          "indistinguishable from one that failed every check")
        self.assertEqual(h.lifetime_checks, 0)
        self.assertIsNone(h.last_success)

    def test_a_tracker_that_failed_every_check_has_a_rate_of_zero(self):
        h = TrackerHistory.new("udp://a.example:6969/announce", T0)
        for i in range(5):
            h = failed(h, f"2026-01-0{i + 1}T00:00:00Z")
        self.assertEqual(h.ewma, 0.0)
        self.assertEqual(h.lifetime_checks, 5)
        self.assertIsNone(h.last_success, "it never succeeded")


class TheBoundsHold(unittest.TestCase):
    """T-042: a file that grows unboundedly is the outage the bounds prevent."""

    def test_the_ring_never_exceeds_K(self):
        h = TrackerHistory.new("udp://a.example:6969/announce", T0)
        for i in range(RING_SIZE * 3):
            h = live(h, f"2026-01-01T{i % 24:02d}:{i % 60:02d}:00Z")
        self.assertEqual(len(h.ring), RING_SIZE)

    def test_the_ring_keeps_the_NEWEST_outcomes(self):
        """A ring that dropped the newest would answer every question about
        the past and none about now."""
        h = TrackerHistory.new("udp://a.example:6969/announce", T0)
        for i in range(RING_SIZE + 5):
            h = live(h, f"2026-02-01T00:00:{i:02d}Z")
        self.assertEqual(h.ring[-1].at, f"2026-02-01T00:00:{RING_SIZE + 4:02d}Z")

    def test_daily_aggregates_never_exceed_D(self):
        h = TrackerHistory.new("udp://a.example:6969/announce", T0)
        for d in range(DAILY_DAYS + 40):
            h = live(h, f"2026-{1 + d % 12:02d}-{1 + d % 28:02d}T00:00:00Z")
        self.assertLessEqual(len(h.daily), DAILY_DAYS)

    def test_lifetime_counters_do_not_roll(self):
        """The ring rolls and the lifetime does not, which is what lets a
        long-dead tracker still say how long it worked."""
        h = TrackerHistory.new("udp://a.example:6969/announce", T0)
        for i in range(RING_SIZE * 2):
            h = live(h, f"2026-01-01T{i % 24:02d}:{i % 60:02d}:00Z")
        self.assertEqual(h.lifetime_checks, RING_SIZE * 2)
        self.assertEqual(h.lifetime_successes, RING_SIZE * 2)


class ObservationsAggregateCorrectly(unittest.TestCase):

    def test_checks_on_one_day_land_in_one_aggregate(self):
        h = TrackerHistory.new("udp://a.example:6969/announce", T0)
        h = live(h, "2026-03-04T01:00:00Z")
        h = failed(h, "2026-03-04T04:00:00Z")
        h = live(h, "2026-03-04T07:00:00Z")
        self.assertEqual(len(h.daily), 1)
        self.assertEqual((h.daily[0].day, h.daily[0].checks,
                          h.daily[0].successes), ("2026-03-04", 3, 2))

    def test_an_out_of_order_observation_does_not_drop_the_newest_day(self):
        """Days are sorted before the cap is applied. Capping an unsorted list
        from the end would discard whichever day happened to arrive last."""
        h = TrackerHistory.new("udp://a.example:6969/announce", T0)
        h = live(h, "2026-03-05T00:00:00Z")
        h = live(h, "2026-03-04T00:00:00Z")
        self.assertEqual([d.day for d in h.daily],
                         ["2026-03-04", "2026-03-05"])

    def test_last_seen_never_goes_backwards(self):
        h = TrackerHistory.new("udp://a.example:6969/announce", T0)
        h = live(h, "2026-03-05T00:00:00Z")
        h = live(h, "2026-03-04T00:00:00Z")
        self.assertEqual(h.last_seen, "2026-03-05T00:00:00Z")

    def test_the_rate_recovers_rather_than_being_held_down(self):
        h = TrackerHistory.new("udp://a.example:6969/announce", T0)
        for i in range(30):
            h = failed(h, f"2026-01-01T{i % 24:02d}:00:00Z")
        floor = h.ewma
        for i in range(20):
            h = live(h, f"2026-02-01T{i % 24:02d}:00:00Z")
        self.assertGreater(h.ewma, floor)
        self.assertGreater(h.ewma, 0.5,
                           "20 consecutive successes should outweigh 30 old "
                           f"failures at alpha={EWMA_ALPHA}")


class TheFileRoundTrips(unittest.TestCase):

    def test_a_record_survives_render_and_parse(self):
        h = TrackerHistory.new("udp://a.example:6969/announce", T0)
        h = live(h, "2026-03-04T01:00:00Z")
        h = failed(h, "2026-03-04T04:00:00Z", failure="refused")
        again = parse_line(render_line(h))
        self.assertEqual(again, h)

    def test_two_writes_of_the_same_state_are_byte_identical(self):
        """RULES 3.6. A state file is rewritten every run, so any set ordering
        or float repr leaking in would make every run a diff."""
        hs = []
        for n in range(20):
            h = TrackerHistory.new(f"udp://h{n}.example:6969/announce", T0)
            h = live(h, "2026-03-04T01:00:00Z")
            hs.append(h)
        with tempfile.TemporaryDirectory() as tmp:
            a, b = os.path.join(tmp, "a"), os.path.join(tmp, "b")
            write_state(a, hs, generated_at=T0)
            write_state(b, list(reversed(hs)), generated_at=T0)
            with open(a, "rb") as fh:
                first = fh.read()
            with open(b, "rb") as fh:
                second = fh.read()
        self.assertEqual(first, second,
                         "input order changed the bytes on disk")

    def test_the_file_is_sorted_by_url(self):
        hs = [TrackerHistory.new(u, T0) for u in
              ("udp://c.example:1/announce", "udp://a.example:1/announce",
               "udp://b.example:1/announce")]
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "state.jsonl")
            write_state(path, hs, generated_at=T0)
            with open(path, encoding="utf-8") as fh:
                lines = fh.read().splitlines()
        urls = [json.loads(x)["url"] for x in lines[1:]]
        self.assertEqual(urls, sorted(urls))

    def test_newlines_are_lf_on_every_host(self):
        """RULES 15.5. The platform separator would make the same state file
        differ between Windows and a runner, and a committed artefact whose
        bytes depend on who wrote it cannot be diffed against the next run."""
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "state.jsonl")
            write_state(path, [TrackerHistory.new("udp://a.example:1/announce",
                                                  T0)], generated_at=T0)
            with open(path, "rb") as fh:
                raw = fh.read()
        self.assertNotIn(b"\r\n", raw)


class BootstrapAndCorruption(unittest.TestCase):
    """`gates.md`: bootstrap from no state succeeds; corrupt state fails safely
    **without reinitialising**."""

    def test_bootstrap_from_nothing_succeeds(self):
        with tempfile.TemporaryDirectory() as tmp:
            histories, quarantined = bootstrap(os.path.join(tmp, "absent.jsonl"))
        self.assertEqual(histories, {})
        self.assertEqual(quarantined, [])

    def test_a_wrong_format_header_raises_rather_than_starting_over(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "state.jsonl")
            with open(path, "w", encoding="utf-8", newline="\n") as fh:
                fh.write('{"format":"something.else/9"}\n')
            with self.assertRaises(CorruptState):
                bootstrap(path)
            # ⛔ And the file is still there. A reader that "recovered" would
            # have replaced it.
            self.assertTrue(os.path.exists(path))

    def test_an_empty_file_raises_rather_than_reading_as_no_history(self):
        """An absent file is the first run. A zero-byte file is a file that
        something truncated, and reading it as 'no trackers have any history'
        would publish an outage as a measurement."""
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "state.jsonl")
            open(path, "w", encoding="utf-8").close()
            with self.assertRaises(CorruptState):
                bootstrap(path)

    def test_one_bad_line_is_quarantined_and_the_rest_survive(self):
        """Losing 1326 trackers to one damaged line is the recovery-by-deletion
        RULES 3.9 forbids, at a different scale."""
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "state.jsonl")
            good = [TrackerHistory.new(f"udp://h{n}.example:1/announce", T0)
                    for n in range(3)]
            write_state(path, good, generated_at=T0)
            with open(path, "a", encoding="utf-8", newline="\n") as fh:
                fh.write("{not json at all\n")
            histories, quarantined = read_state(path)
        self.assertEqual(len(histories), 3)
        self.assertEqual(len(quarantined), 1)
        self.assertIn("line 5", quarantined[0])

    def test_impossible_counters_are_refused_not_repaired(self):
        """More successes than checks is a damaged or edited record. Guessing
        which number is wrong would publish a rate nobody measured."""
        with self.assertRaises(CorruptState):
            parse_line(json.dumps({"url": "udp://a.example:1/announce",
                                   "first_seen": T0, "last_seen": T0,
                                   "lifetime_checks": 2,
                                   "lifetime_successes": 5}))

    def test_a_duplicated_url_is_quarantined_rather_than_last_wins(self):
        """Two lines for one tracker means the file was concatenated or
        merged. Taking the last silently discards a history."""
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "state.jsonl")
            h = TrackerHistory.new("udp://a.example:1/announce", T0)
            write_state(path, [h], generated_at=T0)
            with open(path, "a", encoding="utf-8", newline="\n") as fh:
                fh.write(render_line(live(h, "2026-05-05T00:00:00Z")) + "\n")
            histories, quarantined = read_state(path)
        self.assertEqual(len(histories), 1)
        self.assertEqual(len(quarantined), 1)
        self.assertIn("twice", quarantined[0])


class FoldingASweepIn(unittest.TestCase):

    def sweep_record(self, url: str, state: str, at: str) -> dict:
        return {"url": url, "health_state": state, "observed_at": at,
                "measurement_rung": "protocol_valid", "failure": None}

    def test_only_the_probed_trackers_age(self):
        """⛔ A sweep of 200 must not age the other 1127. Not being checked is
        not the same as having failed."""
        a = TrackerHistory.new("udp://a.example:1/announce", T0)
        b = TrackerHistory.new("udp://b.example:1/announce", T0)
        out = apply_sweep({a.url: a, b.url: b},
                          [self.sweep_record(a.url, "live", "2026-04-01T00:00:00Z")])
        self.assertEqual(out[a.url].lifetime_checks, 1)
        self.assertEqual(out[b.url].lifetime_checks, 0)
        self.assertEqual(out[b.url], b, "an unprobed tracker changed")

    def test_unknown_and_unmeasurable_are_not_successes(self):
        """The two ways of knowing nothing. Counting either as a success would
        let a tracker this vantage cannot measure accumulate a perfect record."""
        for state in ("unknown", "unmeasurable", "error", "degraded"):
            with self.subTest(state=state):
                url = "udp://a.example:1/announce"
                out = apply_sweep({}, [self.sweep_record(url, state,
                                                         "2026-04-01T00:00:00Z")])
                self.assertEqual(out[url].lifetime_successes, 0)
                self.assertIsNone(out[url].last_success)

    def test_a_tracker_not_seen_before_starts_its_history_at_this_check(self):
        url = "udp://new.example:1/announce"
        out = apply_sweep({}, [self.sweep_record(url, "live",
                                                 "2026-04-01T00:00:00Z")])
        self.assertEqual(out[url].first_seen, "2026-04-01T00:00:00Z")
        self.assertEqual(out[url].lifetime_successes, 1)


class AFreshProcessCanReadIt(unittest.TestCase):
    """T-040's `Prove` clause names a fresh process, and it is right to.

    An in-memory round trip proves the objects agree with themselves. Only a
    second interpreter reading the bytes off disk proves the FILE is the
    contract -- which is what a consumer, and the next run, actually get.
    """

    def test_a_second_interpreter_reads_the_file_this_one_wrote(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "state.jsonl")
            h = TrackerHistory.new("udp://a.example:6969/announce", T0)
            h = live(h, "2026-03-04T01:00:00Z")
            h = failed(h, "2026-03-05T01:00:00Z", failure="refused")
            write_state(path, [h], generated_at=T0)

            script = (
                "import sys, json;"
                f"sys.path.insert(0, {os.path.join(REPO, 'src')!r});"
                "from trackers.state import read_state;"
                f"hs, q = read_state({path!r});"
                "h = hs['udp://a.example:6969/announce'];"
                "print(json.dumps({'q': q, 'checks': h.lifetime_checks,"
                " 'successes': h.lifetime_successes,"
                " 'last_success': h.last_success,"
                " 'last_failure': h.last_failure,"
                " 'ring': len(h.ring), 'daily': len(h.daily)}))")
            proc = subprocess.run([sys.executable, "-c", script],
                                  capture_output=True, text=True,
                                  encoding="utf-8")
        self.assertEqual(proc.returncode, 0, proc.stderr)
        got = json.loads(proc.stdout)
        self.assertEqual(got["q"], [])
        self.assertEqual(got["checks"], 2)
        self.assertEqual(got["successes"], 1)
        self.assertEqual(got["last_success"], "2026-03-04T01:00:00Z")
        self.assertEqual(got["last_failure"], "2026-03-05T01:00:00Z")
        self.assertEqual((got["ring"], got["daily"]), (2, 2))

    def test_the_header_names_the_format_and_the_bounds(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "state.jsonl")
            write_state(path, [], generated_at=T0)
            with open(path, encoding="utf-8") as fh:
                header = json.loads(fh.readline())
        self.assertEqual(header["format"], STATE_FORMAT)
        self.assertEqual(header["ring_size"], RING_SIZE)
        self.assertEqual(header["daily_days"], DAILY_DAYS)
        self.assertEqual(header["records"], 0)


if __name__ == "__main__":
    unittest.main()
