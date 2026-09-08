"""What a tracker's history looks like over time. T-041.

The brief names seven states that matter, and every one of them is a **shape
over time rather than an instantaneous value**: new, temporarily unavailable,
intermittently failing, consistently unreliable, degrading, improving,
apparently gone. `state.py` stores a series that can express them and nothing
read it. This does.

⛔ **A SHAPE IS NOT A HEALTH STATE.** `probe.health_state` decides `live`,
`dead` and the rest from one observation plus a sample count, and it is the
only thing that may say `dead`. This says what a *series* looks like, and
`APPARENTLY_GONE` is a description of a pattern rather than a verdict about a
tracker. The two are computed from different inputs and neither is derived
from the other.

⛔ **AND IT IS NOT A SCORE.** `D4` is open on purpose: history exists but no
tracker has more than four observations, and fitting a model to that is
fitting it to noise. A shape is a label with a definition somebody can check
by hand, which is what makes it safe to build before the model.

THERE IS NO CLOCK IN HERE, AND THAT IS A DECISION

Nothing takes `now`. A shape describes the series that was recorded, so the
same state file classifies identically on any day and on any machine (RULES
3.6). Staleness is a real question and it is a different one: a tracker whose
last eight checks succeeded a year ago is a fact about **our sweeping** rather
than about the tracker, and `last_seen` is on the record for a consumer who
needs it.

THE THRESHOLDS ARE DURATIONS, NOT TASTE

Every constant below is a count of observations, and at D7's default cadence
of one check per three hours each has a meaning in hours. That is the whole
justification for the numbers, and it is why they are written as arithmetic:

    3 observations   ~9 hours    under a day. Shorter than this says nothing.
    8 observations   ~24 hours   a day, which is the recent window.
    K = 64           ~8 days     the ring, from `experiments/31`.

⚠ **Change the cadence and these change with it.** They are not tuned against
this corpus, because this corpus does not yet have a tracker with more than
four observations to tune against.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any

from .state import TrackerHistory

__all__ = [
    "Shape", "ShapeVerdict", "classify", "MIN_OBSERVATIONS", "RECENT_WINDOW",
    "MAX_TEMPORARY_RUN", "HEALTHY_RATE", "UNRELIABLE_RATE", "TREND_DELTA",
    "MIN_DAYS_FOR_TREND",
]

#: Below this, no shape but `NEW` is claimable. Three checks is about nine
#: hours at D7's cadence, and it is the same floor `MIN_SAMPLES_FOR_DEATH`
#: uses for the same reason: two observations cannot separate a pattern from
#: a coincidence.
MIN_OBSERVATIONS = 3

#: One day of checks. `APPARENTLY_GONE` means nothing worked across this
#: window, so it is a day of continuous failure rather than a bad afternoon.
RECENT_WINDOW = 8

#: The longest run of trailing failures still called temporary. Three checks
#: is under nine hours; a fourth means it has been failing since yesterday and
#: the series is going down rather than dipping.
MAX_TEMPORARY_RUN = 3

#: What "was fine before this" means, as a rate.
HEALTHY_RATE = 0.8

#: At or below this, a tracker is failing more than it works. Above it and
#: below `HEALTHY_RATE`, it mostly works and drops out, which is a different
#: complaint with a different fix.
UNRELIABLE_RATE = 0.5

#: How much the rate must move between the two halves of the series before the
#: movement is a direction rather than noise. A quarter of the range.
TREND_DELTA = 0.25

#: Days of aggregates before the trend is taken from them rather than from the
#: ring. The ring covers 8 days; a month-long decline is invisible inside it,
#: which is what the daily aggregates in `state.py` exist for.
MIN_DAYS_FOR_TREND = 14


class Shape(str, Enum):
    """The seven the brief names, and the residual that is not one of them.

    ⭐ `STEADY` is deliberately last and deliberately outside the seven: a
    tracker that simply works has no interesting shape, and inventing one for
    it would make the vocabulary describe everything and distinguish nothing.
    """

    NEW = "new"
    TEMPORARILY_UNAVAILABLE = "temporarily_unavailable"
    INTERMITTENTLY_FAILING = "intermittently_failing"
    CONSISTENTLY_UNRELIABLE = "consistently_unreliable"
    DEGRADING = "degrading"
    IMPROVING = "improving"
    APPARENTLY_GONE = "apparently_gone"
    STEADY = "steady"


#: The seven the brief asks for, as a set, so a test can assert the vocabulary
#: has not quietly lost one.
THE_SEVEN: frozenset[Shape] = frozenset(Shape) - {Shape.STEADY}


@dataclass(frozen=True, slots=True)
class ShapeVerdict:
    """The shape, and the numbers that decided it.

    RULES 3.10: a classification a consumer cannot audit is a log line with
    extra steps. Every field here is what the rule above it compared.
    """

    shape: Shape
    detail: str
    evidence: dict[str, Any] = field(default_factory=dict)

    def as_record(self) -> dict[str, Any]:
        return {"shape": self.shape.value, "detail": self.detail,
                "evidence": dict(self.evidence)}


def _rate(outcomes: tuple) -> float | None:
    """Success rate, or `None` over nothing. Never 0.0 over nothing -- that is
    the new-versus-failing conflation `state.new` avoids in `ewma`."""
    if not outcomes:
        return None
    return sum(1 for o in outcomes if o.ok) / len(outcomes)


def _trailing_failures(outcomes: tuple) -> int:
    """How many failures the series ends on."""
    run = 0
    for outcome in reversed(outcomes):
        if outcome.ok:
            break
        run += 1
    return run


def _alternations(outcomes: tuple) -> int:
    """How many times the series changes between working and not.

    This is what separates `INTERMITTENTLY_FAILING` from a single outage of
    the same length: four failures scattered through a week and four failures
    in a row are the same rate and are not the same fault.
    """
    return sum(1 for a, b in zip(outcomes, outcomes[1:]) if a.ok != b.ok)


def _trend(history: TrackerHistory) -> tuple[float | None, float | None, str]:
    """The rate over the older half and the newer half, and where they came from.

    ⭐ **The daily aggregates are preferred once there are enough of them.**
    The ring is K = 64 outcomes, about eight days, so "degrading over a month"
    cannot be seen in it at all -- which is exactly why `state.py` keeps D days
    of aggregates alongside it.
    """
    daily = history.daily
    if len(daily) >= MIN_DAYS_FOR_TREND:
        half = len(daily) // 2
        older, newer = daily[:half], daily[half:]
        older_checks = sum(d.checks for d in older)
        newer_checks = sum(d.checks for d in newer)
        if older_checks and newer_checks:
            return (sum(d.successes for d in older) / older_checks,
                    sum(d.successes for d in newer) / newer_checks,
                    "daily")
    ring = history.ring
    if len(ring) < 2 * MIN_OBSERVATIONS:
        # Not enough on either side for a difference to mean anything.
        return None, None, "-"
    half = len(ring) // 2
    return _rate(ring[:half]), _rate(ring[half:]), "ring"


def classify(history: TrackerHistory) -> ShapeVerdict:
    """Which shape a tracker's recorded series has. Ordered; first match wins.

    The order is the specification, and it is written to be read top to bottom:

    1. **Too little history** -> `NEW`. Nothing else is claimable, and calling
       a tracker with two checks unreliable is the failure that makes a
       dataset untrustworthy from its first day.
    2. **Nothing worked across a day of checks** -> `APPARENTLY_GONE`.
    3. **A short run of failures on an otherwise healthy record** ->
       `TEMPORARILY_UNAVAILABLE`. Under nine hours, and it was fine before.
    4. **The rate moved down between the halves** -> `DEGRADING`, taken from
       the daily aggregates where there are enough of them.
    5. **The rate moved up** -> `IMPROVING`.
    6. **It fails more often than it works** -> `CONSISTENTLY_UNRELIABLE`.
    7. **It mostly works and drops out** -> `INTERMITTENTLY_FAILING`.
    8. **None of the above** -> `STEADY`, which is not one of the seven.
    """
    ring = history.ring
    checks = history.lifetime_checks
    rate = _rate(ring)
    recent = ring[-RECENT_WINDOW:]
    trailing = _trailing_failures(ring)
    older, newer, trend_source = _trend(history)
    evidence: dict[str, Any] = {
        "lifetime_checks": checks,
        "ring_length": len(ring),
        "ring_rate": None if rate is None else round(rate, 6),
        "recent_window": len(recent),
        "recent_successes": sum(1 for o in recent if o.ok),
        "trailing_failures": trailing,
        "alternations": _alternations(ring),
        "trend_source": trend_source,
        "older_rate": None if older is None else round(older, 6),
        "newer_rate": None if newer is None else round(newer, 6),
    }

    def verdict(shape: Shape, detail: str) -> ShapeVerdict:
        return ShapeVerdict(shape=shape, detail=detail, evidence=evidence)

    if checks < MIN_OBSERVATIONS or len(ring) < MIN_OBSERVATIONS:
        return verdict(
            Shape.NEW,
            f"{checks} check(s), under the {MIN_OBSERVATIONS} a shape needs")

    if len(recent) >= MIN_OBSERVATIONS and not any(o.ok for o in recent):
        return verdict(
            Shape.APPARENTLY_GONE,
            f"no success in the last {len(recent)} checks")

    before = ring[:-trailing] if trailing else ring
    before_rate = _rate(before)
    if (0 < trailing <= MAX_TEMPORARY_RUN
            and len(before) >= MIN_OBSERVATIONS
            and before_rate is not None and before_rate >= HEALTHY_RATE):
        return verdict(
            Shape.TEMPORARILY_UNAVAILABLE,
            f"{trailing} failure(s) after a run at {before_rate:.2f}")

    if older is not None and newer is not None:
        if newer <= older - TREND_DELTA:
            return verdict(
                Shape.DEGRADING,
                f"{trend_source} rate fell {older:.2f} -> {newer:.2f}")
        if newer >= older + TREND_DELTA:
            return verdict(
                Shape.IMPROVING,
                f"{trend_source} rate rose {older:.2f} -> {newer:.2f}")

    if rate is not None and rate <= UNRELIABLE_RATE:
        return verdict(
            Shape.CONSISTENTLY_UNRELIABLE,
            f"succeeds {rate:.2f} of the time with no direction")

    if rate is not None and rate < HEALTHY_RATE:
        return verdict(
            Shape.INTERMITTENTLY_FAILING,
            f"succeeds {rate:.2f} of the time, "
            f"{evidence['alternations']} change(s) of state")

    return verdict(
        Shape.STEADY,
        f"succeeds {rate:.2f} of the time with no direction"
        if rate is not None else "no outcomes")
