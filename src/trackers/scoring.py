"""The seven scoring invariants, as executable properties. T-043 and T-045.

⛔ **There is no scoring model here, deliberately.** Choosing one is T-044 and
decision D4, and it stays open because no tracker has enough history to fit a
model against without fitting it to noise. What this module holds is the part
that **survives a change of model**, which is why the entry says the invariants
are written before it.

⭐ **So this is a filter on candidates rather than a scorer.** Any function of
the form

    scorer(checks: int, successes: int, measurable: bool) -> float | None

can be put through `check_invariants` and told which of the seven it breaks,
before anybody builds on it. `tests/test_scoring_invariants.py` runs the
obvious candidates through it, and the result is a finding rather than a
formality: the rate this project already computes **fails I2**.

THE SEVEN

    I1  more successes at the same rate never lowers the score
    I2  one success must not outrank hundreds at an equal-or-better rate
    I3  identical inputs produce an identical ordering, including ties
    I4  adding a failure never raises the score
    I5  an unmeasurable tracker is never scored as if measured
    I6  score is invariant to input ordering and to source ordering
    I7  no single observation decides the order, and its power must
        FALL as the evidence grows. ⭐ **T-045's prohibition,
        made checkable**: "MUST NOT rank on the latest instantaneous
        result" is a rule about inputs, and this is the consequence a
        function handed only aggregates can be tested against

⚠ **I5 and I6 are checked here on the scoring path only.** `Tracker.sort_key`
is already total and `aggregate()` already sorts by source id; re-testing those
would be two checks enforcing one rule, which the entry's `Decision` says not
to do.

⛔ **The `Scorer` signature is itself a guard, and it is the
stronger half of T-045.** It takes `(checks, successes, measurable)` and
has no parameter that could carry the latest observation, so a model
reaching this interface **cannot** rank on one. I7 is what survives
somebody widening the signature.

⛔ **A scorer that returns `None` for "cannot say" is the honest shape**, and
the invariants treat it as such rather than as a zero. A tracker nobody has
checked and a tracker that failed every check are the first and fourth of
T-041's seven shapes, and a scorer that maps both to 0.0 has destroyed the
distinction the dataset exists to carry.
"""

from __future__ import annotations

from typing import Callable, Protocol

__all__ = ["Scorer", "INVARIANT_NAMES", "check_invariants", "violations_of"]


class Scorer(Protocol):
    """What a scoring model must look like to be checkable.

    `None` means "cannot say", which is not the same as zero and must not be
    rendered as one.
    """

    def __call__(self, checks: int, successes: int,
                 measurable: bool = True) -> float | None: ...


def _rank(scorer: Scorer, checks: int, successes: int,
          measurable: bool = True) -> float:
    """A total order over scores, with `None` below everything.

    ⚠ Ranking `None` as lowest is a **comparison convenience and not a
    claim**: an unscored tracker is not worse than a bad one, it is unknown.
    Nothing here publishes this value; it exists so the invariants can compare
    two outputs at all.
    """
    value = scorer(checks, successes, measurable)
    return float("-inf") if value is None else value


def _i1_more_successes_never_lowers(scorer: Scorer) -> list[str]:
    out = []
    for checks in (2, 10, 100, 1000):
        for half in (checks // 2,):
            better = _rank(scorer, checks, half + 1)
            worse = _rank(scorer, checks, half)
            if better < worse:
                out.append(f"{checks} checks: {half + 1} successes scores "
                           f"{better} below {half} successes at {worse}")
    return out


def _i2_one_success_never_outranks_hundreds(scorer: Scorer) -> list[str]:
    """⛔ The invariant with teeth, and the one a plain rate fails.

    A tracker seen once and answering once has a rate of 1.0. So does one seen
    500 times. Ranking them equal publishes a single lucky observation as
    equivalent to months of evidence.
    """
    out = []
    for many in (100, 500, 1000):
        lucky = _rank(scorer, 1, 1)
        established = _rank(scorer, many, many)
        if lucky >= established:
            out.append(f"1/1 scores {lucky} against {many}/{many} at "
                       f"{established}: one observation ties or beats {many}")
        # And at a slightly worse rate, which is the realistic case.
        nearly = _rank(scorer, many, many - 1)
        if lucky >= nearly:
            out.append(f"1/1 scores {lucky} against {many - 1}/{many} at "
                       f"{nearly}")
    return out


def _i3_identical_inputs_order_identically(scorer: Scorer) -> list[str]:
    population = [(c, s) for c in (1, 5, 50, 500) for s in (0, 1, c // 2, c)
                  if s <= c]
    first = sorted(population, key=lambda p: (-_rank(scorer, *p), p))
    second = sorted(list(reversed(population)),
                    key=lambda p: (-_rank(scorer, *p), p))
    return ([] if first == second else
            [f"ordering depends on input order: {first[:3]} vs {second[:3]}"])


def _i4_a_failure_never_raises(scorer: Scorer) -> list[str]:
    out = []
    for checks, successes in ((1, 1), (10, 7), (100, 99), (500, 250)):
        before = _rank(scorer, checks, successes)
        after = _rank(scorer, checks + 1, successes)   # one more check, no more successes
        if after > before:
            out.append(f"{successes}/{checks} scores {before}, and adding a "
                       f"failure raises it to {after}")
    return out


def _i5_unmeasurable_is_never_scored_as_measured(scorer: Scorer) -> list[str]:
    out = []
    for checks, successes in ((0, 0), (3, 0), (10, 10)):
        value = scorer(checks, successes, False)
        if value is not None:
            out.append(f"unmeasurable {successes}/{checks} scored {value} "
                       f"rather than None")
    return out


def _i6_ordering_is_invariant_to_presentation(scorer: Scorer) -> list[str]:
    """The scoring path's own half of I6: a score depends on the counts and on
    nothing else it was handed alongside them."""
    out = []
    for checks, successes in ((10, 5), (100, 60)):
        repeated = {scorer(checks, successes) for _ in range(5)}
        if len(repeated) != 1:
            out.append(f"{successes}/{checks} scored {repeated} across "
                       f"identical calls")
    return out


def _i7_no_single_observation_decides(scorer: Scorer) -> list[str]:
    """⛔ **T-045's prohibition, made checkable.**

    *"MUST NOT rank on the latest instantaneous result"* is a rule about
    *inputs*, and a rule about inputs cannot be enforced against a function
    that is handed aggregates. What **can** be enforced is the property that
    makes the prohibition matter: as the evidence grows, no one observation may
    keep the same power over the score.

    So the test is on the **sensitivity** `|score(c, s) - score(c, s - 1)|`,
    and the requirement is that it does not grow with `c`. A model that reads
    the last result has a sensitivity of 1 at every sample size; a model over
    the history has one that decays like `1/c`. The difference is visible
    without ever asking the scorer where its numbers came from.

    ⚠ **This rejects a scorer somebody would plausibly write.** "Only trackers
    that have never failed" -- `1.0 if s == c else 0.0` -- has a sensitivity of
    1 forever: a single failure on a tracker with a thousand successes decides
    the whole ordering, which is the pathology under a different name.

    ⭐ **And it is one of two locks, not the only one.** The `Scorer` protocol
    takes `(checks, successes, measurable)` and has **no way to express** "the
    latest result", so a model reaching this interface cannot rank on one even
    if its author wanted to. That is the structural half; this is the half that
    survives somebody widening the signature.
    """
    out = []
    sensitivities: list[tuple[int, float]] = []
    for checks in (4, 10, 100, 1000):
        with_all = _rank(scorer, checks, checks)
        one_worse = _rank(scorer, checks, checks - 1)
        for value in (with_all, one_worse):
            if value in (float("-inf"), float("inf")):
                # `None` scores are I5's business, not this one.
                break
        else:
            sensitivities.append((checks, abs(with_all - one_worse)))
    for (small, low), (large, high) in zip(sensitivities, sensitivities[1:]):
        # ⚠ A tolerance, because a model may legitimately plateau; what is
        # forbidden is one observation mattering MORE as evidence accumulates.
        if high > low + 1e-9:
            out.append(
                f"one observation moves the score by {high} at {large} checks "
                f"and only {low} at {small}: a single result gains power as "
                f"the history grows, which is T-045's prohibition")
    if sensitivities and len(sensitivities) > 1:
        first, last = sensitivities[0][1], sensitivities[-1][1]
        if first > 0 and last >= first:
            out.append(
                f"one observation moves the score by {last} at "
                f"{sensitivities[-1][0]} checks, no less than the {first} it "
                f"moves it at {sensitivities[0][0]}: the evidence never "
                f"outweighs the newest result")
    return out


_CHECKS: dict[str, Callable[[Scorer], list[str]]] = {
    "I1": _i1_more_successes_never_lowers,
    "I2": _i2_one_success_never_outranks_hundreds,
    "I3": _i3_identical_inputs_order_identically,
    "I4": _i4_a_failure_never_raises,
    "I5": _i5_unmeasurable_is_never_scored_as_measured,
    "I6": _i6_ordering_is_invariant_to_presentation,
    "I7": _i7_no_single_observation_decides,
}

INVARIANT_NAMES: tuple[str, ...] = tuple(sorted(_CHECKS))


def violations_of(scorer: Scorer, invariant: str) -> list[str]:
    """Every way `scorer` breaks one invariant. Empty means it holds."""
    return _CHECKS[invariant](scorer)


def check_invariants(scorer: Scorer) -> dict[str, list[str]]:
    """All seven, as `{invariant: violations}`. Empty lists throughout is a pass.

    ⛔ **Returns the violations rather than a boolean.** A model that fails is
    not simply rejected: which invariant it fails is the argument for the next
    candidate, and a boolean would throw that away.
    """
    return {name: _CHECKS[name](scorer) for name in INVARIANT_NAMES}
