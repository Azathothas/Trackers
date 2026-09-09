"""T-043: the six scoring invariants, and which candidate models survive them.

⛔ **No model is chosen** (T-044, decision D4), so there is no scoring path to
test. The invariants are written first because they **survive a change of
model** -- that is the entry's own premise -- so what they bind is any
candidate, and running them now is what stops a model being built on before
anybody checks it against them.

⭐ **The result is a finding rather than a formality.** The plain success rate,
which is the obvious candidate and is what `state.py` already computes, **fails
I2**: a tracker seen once and answering once scores exactly as well as one seen
500 times. That refutes it before T-044 chooses, which is the cheapest moment
to learn it.

Run:  python3 -m unittest tests.test_scoring_invariants -v
"""

from __future__ import annotations

import math
import os
import sys
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.scoring import (INVARIANT_NAMES, check_invariants,  # noqa: E402
                              violations_of)


# --- candidates, and none of them is chosen ----------------------------------
#
# ⚠ These live in the test rather than in `src/` on purpose. A scorer in the
# library is a model this project ships, and shipping one is T-044's decision
# to make with evidence. These exist to be measured against the invariants.

def plain_rate(checks: int, successes: int, measurable: bool = True):
    """`successes / checks`. What `state.py`'s EWMA converges to, and the
    thing anybody would reach for first."""
    if not measurable:
        return None
    if checks <= 0:
        return None
    return successes / checks


def rate_times_count(checks: int, successes: int, measurable: bool = True):
    """A rate scaled by evidence. Fixes I2 by brute force and breaks nothing
    else, at the cost of being unbounded and hard to explain."""
    if not measurable or checks <= 0:
        return None
    return (successes / checks) * checks


def wilson_lower_bound(checks: int, successes: int, measurable: bool = True,
                       z: float = 1.96):
    """The lower bound of a Wilson score interval.

    ⭐ **The standard answer to "a rate whose confidence depends on the sample
    size"**, and it has no free parameter to tune beyond the confidence level.
    One success of one gives about 0.21; five hundred of five hundred gives
    about 0.99.
    """
    if not measurable or checks <= 0:
        return None
    p = successes / checks
    denominator = 1 + z * z / checks
    centre = p + z * z / (2 * checks)
    margin = z * math.sqrt((p * (1 - p) + z * z / (4 * checks)) / checks)
    return (centre - margin) / denominator


def scores_the_unmeasurable(checks: int, successes: int, measurable: bool = True):
    """⚠ A deliberately broken candidate. Without one, a harness that never
    reported a violation would look identical to a harness that works."""
    if checks <= 0:
        return 0.0
    return successes / checks


class TheHarnessCanFail(unittest.TestCase):
    """⛔ Mutation-proofing, as a test rather than as a manual exercise."""

    def test_a_scorer_that_scores_the_unmeasurable_fails_i5(self):
        self.assertTrue(violations_of(scores_the_unmeasurable, "I5"),
                        "I5 passed a scorer that scores an unmeasurable "
                        "tracker, so it checks nothing")

    def test_a_scorer_that_treats_no_checks_as_zero_fails_i5(self):
        """A tracker nobody has checked scoring 0.0 is the never-checked and
        failed-everything shapes collapsed into one."""
        self.assertIn("0.0", " ".join(violations_of(scores_the_unmeasurable,
                                                    "I5")))

    def test_every_invariant_is_reachable(self):
        """⚠ Seven names, seven checks. A harness advertising an invariant it does
        not run is worse than one that never claimed it."""
        self.assertEqual(len(INVARIANT_NAMES), 7)
        for name in INVARIANT_NAMES:
            with self.subTest(invariant=name):
                self.assertIsInstance(violations_of(plain_rate, name), list)


class TheInvariantsAsProperties(unittest.TestCase):
    """One test per invariant, over adversarial inputs rather than an example."""

    def test_i1_more_successes_never_lowers_the_score(self):
        for scorer in (plain_rate, wilson_lower_bound, rate_times_count):
            with self.subTest(scorer=scorer.__name__):
                self.assertEqual(violations_of(scorer, "I1"), [])

    def test_i2_one_success_must_not_outrank_hundreds(self):
        """⛔ **The plain rate fails this, and that is the point of running it
        now.** 1/1 and 500/500 both score 1.0, so a single lucky observation
        ranks level with months of evidence."""
        broken = violations_of(plain_rate, "I2")
        self.assertTrue(broken, "the plain rate passed I2, which would mean "
                                "the check is not testing what it says")
        self.assertIn("one observation ties or beats", " ".join(broken))

        # And the candidates that survive it.
        for scorer in (wilson_lower_bound, rate_times_count):
            with self.subTest(scorer=scorer.__name__):
                self.assertEqual(violations_of(scorer, "I2"), [])

    def test_i3_identical_inputs_order_identically(self):
        for scorer in (plain_rate, wilson_lower_bound, rate_times_count):
            with self.subTest(scorer=scorer.__name__):
                self.assertEqual(violations_of(scorer, "I3"), [])

    def test_i4_adding_a_failure_never_raises_the_score(self):
        for scorer in (plain_rate, wilson_lower_bound, rate_times_count):
            with self.subTest(scorer=scorer.__name__):
                self.assertEqual(violations_of(scorer, "I4"), [])

    def test_i5_an_unmeasurable_tracker_is_never_scored(self):
        for scorer in (plain_rate, wilson_lower_bound, rate_times_count):
            with self.subTest(scorer=scorer.__name__):
                self.assertEqual(violations_of(scorer, "I5"), [])

    def test_i6_the_score_depends_on_the_counts_and_nothing_else(self):
        for scorer in (plain_rate, wilson_lower_bound, rate_times_count):
            with self.subTest(scorer=scorer.__name__):
                self.assertEqual(violations_of(scorer, "I6"), [])


class WhatThisSaysAboutTheModelNotYetChosen(unittest.TestCase):
    """⭐ The evidence T-044 inherits."""

    def test_the_obvious_candidate_is_refuted(self):
        report = check_invariants(plain_rate)
        failed = sorted(k for k, v in report.items() if v)
        self.assertEqual(failed, ["I2"],
                         f"the plain rate's violations moved: {report}")

    def test_a_confidence_bound_survives_all_six(self):
        """Not a choice, an observation: the standard treatment of a rate with
        a sample size behind it passes every invariant this project wrote
        before knowing what the model would be."""
        report = check_invariants(wilson_lower_bound)
        self.assertEqual({k: v for k, v in report.items() if v}, {})

    def test_the_bound_ranks_evidence_above_luck(self):
        """The number behind I2, stated so a reader can see the size of it."""
        lucky = wilson_lower_bound(1, 1)
        established = wilson_lower_bound(500, 500)
        self.assertLess(lucky, 0.3)
        self.assertGreater(established, 0.98)
        self.assertLess(lucky, established)


def only_never_failed(checks: int, successes: int, measurable: bool = True):
    """⛔ **The candidate I7 exists to reject, and it is a plausible one.**
    "Recommend only trackers that have never failed" is a sentence somebody
    would write down approvingly. Its sensitivity to one observation is 1.0 at
    every sample size: a single failure against a thousand successes decides
    the whole ordering, which is T-045's prohibition wearing a different name.
    """
    if not measurable or checks <= 0:
        return None
    return 1.0 if successes == checks else 0.0


class NoSingleObservationDecidesTheOrder(unittest.TestCase):
    """T-045. ⛔ *"MUST NOT rank on the latest instantaneous result."*

    ⭐ **Two locks, and the first is structural.** The `Scorer` protocol takes
    `(checks, successes, measurable)` and has **no parameter that could carry
    the latest result**, so a model reaching this interface cannot rank on one
    however much its author wanted to. I7 is the second lock: it survives
    somebody widening the signature, because it tests the *consequence* --
    whether one observation keeps its power as the evidence grows -- rather
    than the input.
    """

    def test_the_rejected_candidate_is_rejected(self):
        """⛔ Without this the invariant could be vacuous, and a vacuous
        invariant is the theatre `code.md` forbids."""
        violations = violations_of(only_never_failed, "I7")
        self.assertTrue(violations, "I7 passed a scorer in which one failure "
                                    "decides everything, so it checks nothing")
        self.assertIn("1.0", " ".join(violations))

    def test_a_rate_survives_because_its_sensitivity_decays(self):
        """One observation in ten moves a rate by 0.1; one in a thousand moves
        it by 0.001. That decay **is** the property."""
        self.assertEqual(violations_of(plain_rate, "I7"), [])

    def test_the_confidence_bound_survives_it_too(self):
        self.assertEqual(violations_of(wilson_lower_bound, "I7"), [])

    def test_the_sensitivity_actually_falls_rather_than_merely_not_rising(self):
        """⚠ A constant-sensitivity scorer would pass a check written only as
        "must not increase". This asserts the number this project would care
        about: at 1000 checks one observation must matter far less than at 4.
        """
        big = abs(plain_rate(1000, 1000) - plain_rate(1000, 999))
        small = abs(plain_rate(4, 4) - plain_rate(4, 3))
        self.assertLess(big, small / 100)

    def test_the_interface_cannot_express_a_latest_result(self):
        """⭐ The structural lock, asserted rather than assumed. A signature
        that grows a `latest` parameter should fail this and make somebody
        re-read T-045."""
        import inspect
        signature = inspect.signature(plain_rate)
        self.assertEqual(list(signature.parameters),
                         ["checks", "successes", "measurable"],
                         "the Scorer signature has changed; if it can now see "
                         "an individual observation, T-045's prohibition needs "
                         "a stronger guard than I7")


class ThePublishedOrderingIsNotARanking(unittest.TestCase):
    """T-045's other half: what the dataset is actually sorted by today.

    ⭐ **Nothing ranks, and that is a finding rather than a gap.** The
    published order is `Tracker.sort_key`, which reads the URL and the
    transport and **cannot see health at all** -- so the latest instantaneous
    result cannot be ordering anything. When T-044 chooses a model this test is
    what will have to change, deliberately and visibly.
    """

    def test_the_order_does_not_move_when_health_does(self):
        from trackers.model import Tracker
        from trackers.normalize import parse
        trackers = [parse(f"udp://h{i}.example:6969/announce")
                    for i in range(20)]
        before = [t.url for t in sorted(trackers, key=Tracker.sort_key)]
        after = [t.url for t in sorted(reversed(trackers),
                                       key=Tracker.sort_key)]
        self.assertEqual(before, after)

    def test_two_runs_over_one_history_order_identically(self):
        """The `Prove` clause, over the population the invariants use: an
        ordering computed twice from the same counts, including ties."""
        population = [(c, s) for c in (1, 5, 50, 500)
                      for s in (0, 1, c // 2, c) if s <= c]
        def order(rows):
            return sorted(rows, key=lambda p: (-(wilson_lower_bound(*p) or -1),
                                               p))
        self.assertEqual(order(population), order(list(reversed(population))))
        # ⛔ And ties resolve the same way twice, which is the half a sort on
        # the score alone would get wrong.
        tied = [(10, 5), (10, 5), (10, 5)]
        self.assertEqual(order(tied), order(list(reversed(tied))))

if __name__ == "__main__":
    unittest.main()
