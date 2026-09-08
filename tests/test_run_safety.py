"""T-084: a run that fires twice, late, or not at all must not corrupt state.

The entry's `Prove` clause, and it is not a hypothetical: GitHub's own
documentation says a scheduled workflow may be **delayed, dropped, or run more
than once** (`C-11`), and re-running `scripts/update-state.py` over a directory
it has already read is one keystroke.

⛔ **Measured before the guard existed, on 2026-09-08**: folding one sweep in
twice gave a tracker two observations from one measurement, and three folds
would have reached `MIN_SAMPLES_FOR_DEATH` -- every non-live tracker in that
sweep published as `dead` on the strength of a single probe.

Two layers are tested here because they fail in different places:

  * **per observation**, inside `state.observe`, keyed on the instant. Its
    window is the ring, so it covers anything inside the last `RING_SIZE`
    checks;
  * **per sweep**, in the state file's header, so a run already folded is
    refused even after the evidence rolls off the ring.

Run:  python3 -m unittest tests.test_run_safety -v
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
sys.path.insert(0, os.path.join(REPO, "scripts"))

from trackers.state import (APPLIED_RUNS_KEPT, RING_SIZE,  # noqa: E402
                            TrackerHistory, apply_sweep, read_applied_runs,
                            read_state, write_state)

UPDATE_STATE = os.path.join(REPO, "scripts", "update-state.py")


def health(url: str, at: str, state: str = "live") -> dict:
    return {"url": url, "observed_at": at, "health_state": state,
            "measurement_rung": "protocol_valid" if state == "live" else "dns",
            "failure": "none" if state == "live" else "timeout"}


def sweep_doc(records: list[dict], at: str) -> dict:
    return {"generated_at": at, "trackers": records,
            "selection": {"mode": "whole corpus"}}


class ADuplicatedRunChangesNothing(unittest.TestCase):

    def test_folding_one_sweep_twice_records_one_observation(self):
        records = [health("udp://a.example:6969/announce", "2026-09-08T12:00:00Z")]
        once = apply_sweep({}, records)
        twice = apply_sweep(once, records)
        first = once["udp://a.example:6969/announce"]
        second = twice["udp://a.example:6969/announce"]
        self.assertEqual(first.lifetime_checks, 1)
        self.assertEqual(second.lifetime_checks, 1)
        self.assertEqual(len(second.ring), 1)
        self.assertEqual(second.daily[0].checks, 1)

    def test_three_folds_do_not_reach_the_death_threshold(self):
        """⛔ The consequence, stated as the test. Before the guard, three
        folds of one failing sweep put a tracker at three observations, which
        is `MIN_SAMPLES_FOR_DEATH`."""
        from trackers.probe import MIN_SAMPLES_FOR_DEATH
        records = [health("udp://a.example:6969/announce",
                          "2026-09-08T12:00:00Z", state="unknown")]
        histories = {}
        for _ in range(MIN_SAMPLES_FOR_DEATH + 2):
            histories = apply_sweep(histories, records)
        history = histories["udp://a.example:6969/announce"]
        self.assertEqual(history.lifetime_checks, 1)
        self.assertLess(history.lifetime_checks, MIN_SAMPLES_FOR_DEATH)

    def test_the_state_file_is_byte_identical_after_a_duplicate_fold(self):
        """The entry says *the same state*, so the assertion is on the bytes.
        Two runs over identical inputs produce identical files (RULES 3.6)."""
        records = [health(f"udp://h{i}.example:6969/announce",
                          "2026-09-08T12:00:00Z") for i in range(5)]
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "history.jsonl")
            first = apply_sweep({}, records)
            write_state(path, first.values(), generated_at="2026-09-08T13:00:00Z")
            with open(path, "rb") as fh:
                before = fh.read()
            second = apply_sweep(first, records)
            write_state(path, second.values(), generated_at="2026-09-08T13:00:00Z")
            with open(path, "rb") as fh:
                after = fh.read()
        self.assertEqual(before, after)

    def test_a_genuinely_new_observation_is_still_recorded(self):
        """⛔ The guard must not swallow real checks. A second sweep an hour
        later is a second measurement, not a duplicate."""
        url = "udp://a.example:6969/announce"
        histories = apply_sweep({}, [health(url, "2026-09-08T12:00:00Z")])
        histories = apply_sweep(histories, [health(url, "2026-09-08T15:00:00Z")])
        self.assertEqual(histories[url].lifetime_checks, 2)


class ASkippedIntervalIsNotCorruption(unittest.TestCase):

    def test_a_gap_in_observations_leaves_the_history_consistent(self):
        """A dropped run means fewer observations, not wrong ones. The
        aggregates are keyed by day, so a missing day is simply absent."""
        url = "http://a.example/announce"
        history = TrackerHistory.new(url, "2026-01-01T00:00:00Z")
        for day in (1, 2, 9, 10):        # days 3 to 8 never ran
            history = history.observe(state="live", ok=True, rung="tracker_semantic",
                                      observed_at=f"2026-01-{day:02d}T00:00:00Z")
        self.assertEqual(history.lifetime_checks, 4)
        self.assertEqual(history.lifetime_successes, 4)
        self.assertEqual([d.day for d in history.daily],
                         ["2026-01-01", "2026-01-02", "2026-01-09", "2026-01-10"])
        self.assertEqual(sum(d.checks for d in history.daily), 4)

    def test_an_out_of_order_run_does_not_lose_the_newest_day(self):
        """A delayed run arriving after a later one is `C-11`'s other shape."""
        url = "http://a.example/announce"
        history = TrackerHistory.new(url, "2026-01-01T00:00:00Z")
        history = history.observe(state="live", ok=True, rung="tracker_semantic",
                                  observed_at="2026-01-05T00:00:00Z")
        history = history.observe(state="live", ok=True, rung="tracker_semantic",
                                  observed_at="2026-01-02T00:00:00Z")
        self.assertEqual([d.day for d in history.daily],
                         ["2026-01-02", "2026-01-05"])
        self.assertEqual(history.last_seen, "2026-01-05T00:00:00Z",
                         "an older observation must not move `last_seen` back")


class TheStateFileRemembersWhatItFolded(unittest.TestCase):

    def test_the_header_carries_the_runs_and_is_bounded(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "history.jsonl")
            runs = [f"2026-09-{d:02d}T00:00:00Z|whole corpus|-|10"
                    for d in range(1, 30)]
            write_state(path, [], generated_at="2026-09-08T00:00:00Z",
                        applied_runs=runs * 4)
            kept = read_applied_runs(path)
        self.assertLessEqual(len(kept), APPLIED_RUNS_KEPT)
        self.assertEqual(len(kept), len(set(kept)), "duplicates were stored")

    def test_a_file_without_the_field_reports_nothing_rather_than_guessing(self):
        """RULES 2. A state file that predates the field has not told us it
        folded nothing; it has told us nothing."""
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "history.jsonl")
            write_state(path, [], generated_at="2026-09-08T00:00:00Z")
            self.assertEqual(read_applied_runs(path), [])
            self.assertEqual(read_applied_runs(os.path.join(tmp, "absent")), [])

    def test_the_updater_refuses_a_sweep_it_has_already_folded(self):
        """⭐ Driven through the real command, twice, because the defect lives
        in the pipeline rather than in the library: it is what a retry does."""
        with tempfile.TemporaryDirectory() as tmp:
            sweep = os.path.join(tmp, "health.json")
            state = os.path.join(tmp, "history.jsonl")
            with open(sweep, "w", encoding="utf-8", newline="\n") as fh:
                json.dump(sweep_doc(
                    [health("udp://a.example:6969/announce",
                            "2026-09-08T12:00:00Z", state="unknown")],
                    "2026-09-08T12:00:00Z"), fh)
            for _ in range(3):
                done = subprocess.run(
                    [sys.executable, UPDATE_STATE, "--state", state, sweep,
                     "--generated-at", "2026-09-08T13:00:00Z"],
                    capture_output=True, text=True, check=False)
                self.assertEqual(done.returncode, 0, done.stderr)
            histories, quarantined = read_state(state)
        self.assertEqual(quarantined, [])
        history = histories["udp://a.example:6969/announce"]
        self.assertEqual(history.lifetime_checks, 1,
                         "three runs of the updater over one sweep recorded "
                         f"{history.lifetime_checks} observations")

    def test_it_refuses_a_sweep_whose_observations_have_rolled_off_the_ring(self):
        """⭐ The case only this layer can catch, and the one that proves it
        is not decoration.

        The per-observation guard reads the ring, so it covers `RING_SIZE`
        checks and no more. Beyond that the evidence is gone and an ancient
        sweep replayed against a busy history looks new. Here the ring is
        filled past capacity with later observations before the original sweep
        is offered again.
        """
        url = "udp://a.example:6969/announce"
        original = health(url, "2026-01-01T00:00:00Z", state="unknown")
        doc = sweep_doc([original], "2026-01-01T00:00:00Z")

        history = TrackerHistory.new(url, "2026-01-01T00:00:00Z")
        history = history.observe(state="unknown", ok=False, rung="dns",
                                  observed_at="2026-01-01T00:00:00Z")
        for i in range(RING_SIZE + 5):   # push the original off the end
            history = history.observe(
                state="unknown", ok=False, rung="dns",
                observed_at=f"2026-02-{1 + i // 24:02d}T{i % 24:02d}:30:00Z")
        self.assertNotIn("2026-01-01T00:00:00Z", [o.at for o in history.ring],
                         "the setup failed: the original is still in the ring")
        before = history.lifetime_checks

        with tempfile.TemporaryDirectory() as tmp:
            sweep = os.path.join(tmp, "health.json")
            state = os.path.join(tmp, "history.jsonl")
            with open(sweep, "w", encoding="utf-8", newline="\n") as fh:
                json.dump(doc, fh)
            import runpy
            identity = runpy.run_path(
                UPDATE_STATE, run_name="loaded_for_a_test")["sweep_identity"]
            write_state(state, [history], generated_at="2026-02-05T00:00:00Z",
                        applied_runs=[identity(doc)])
            done = subprocess.run(
                [sys.executable, UPDATE_STATE, "--state", state, sweep,
                 "--generated-at", "2026-02-06T00:00:00Z"],
                capture_output=True, text=True, check=False)
            self.assertEqual(done.returncode, 0, done.stderr)
            histories, _ = read_state(state)

        self.assertEqual(histories[url].lifetime_checks, before,
                         "a sweep already folded was folded again once its "
                         "observations had rolled off the ring")

    def test_two_slices_at_one_instant_are_two_sweeps(self):
        """⛔ The collision the adversarial pass of 2026-09-08 found.

        Two rotations of the same corpus at the same injected instant carry the
        same clock, the same mode and the same record count. Without the slice
        in the identity the second is refused as already folded, and **190 real
        observations are dropped** -- a guard against double-counting that
        discards distinct data.
        """
        import runpy
        identity = runpy.run_path(
            UPDATE_STATE, run_name="loaded_for_a_test")["sweep_identity"]
        base = {"generated_at": "2026-09-08T12:00:00Z",
                "trackers": [health(f"udp://a{i}.example:1/announce",
                                    "2026-09-08T12:00:00Z") for i in range(190)]}
        first = dict(base, selection={"mode": "whole corpus", "slice": 0})
        second = dict(base, selection={"mode": "whole corpus", "slice": 1})
        self.assertNotEqual(identity(first), identity(second))

    def test_the_identity_is_the_sweep_and_not_its_filename(self):
        """The same measurement under a second name is the same measurement."""
        # ⚠ `update-state.py` has a hyphen and cannot be imported as a module,
        # so it is loaded the way the script itself runs. `run_name` is
        # anything but `__main__`, or reading it would execute it.
        import runpy
        namespace = runpy.run_path(UPDATE_STATE, run_name="loaded_for_a_test")
        identity = namespace["sweep_identity"]
        doc = sweep_doc([health("udp://a.example:1/announce",
                                "2026-09-08T12:00:00Z")],
                        "2026-09-08T12:00:00Z")
        self.assertEqual(identity(doc), identity(dict(doc)))
        other = dict(doc, generated_at="2026-09-08T15:00:00Z")
        self.assertNotEqual(identity(doc), identity(other))


if __name__ == "__main__":
    unittest.main()
