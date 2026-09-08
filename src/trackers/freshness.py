"""Is this dataset still being updated? T-002.

⛔ **The worst failure available to this project is stopping quietly.** GitHub
disables a public repository's scheduled workflows after 60 days without
repository activity (`C-12`, verified). The dataset would simply stop moving,
every file would still serve HTTP 200, and nothing in it would say so.

TWO HALVES, AND ONLY ONE OF THEM CAN BE TRUSTED

    (a) keep the schedule alive     the publisher commits to the `data` branch
                                    on every run, which is repository activity
    (b) be detectable when it dies  the published data says when it was
                                    generated and how often it expects to be

⛔ **(a) is exactly the thing that breaks.** If publication is what keeps the
schedule alive, then publication failing takes the schedule with it, and the
failure is silent by construction. A watchdog that runs on the same schedule
cannot report that the schedule stopped -- it stopped too.

⭐ **So the load-bearing half is (b), and it works because the consumer is
outside the failure.** A reader who has the file has everything needed to decide
it is stale: the instant it was generated and the cadence it claims. That
judgement does not depend on any part of this project still running, which is
the only property worth having here.

⚠ **This module decides staleness; it does not fetch anything and it does not
read a clock.** `now` is passed in, for the same reason nothing else here reads
one (RULES 3.6).
"""

from __future__ import annotations

import datetime
from dataclasses import dataclass

__all__ = ["PUBLISH_INTERVAL_SECONDS", "STALE_AFTER_INTERVALS", "Freshness",
           "parse_instant", "assess"]

#: How often the dataset expects to be regenerated. The publisher runs after
#: each health sweep and the sweep is scheduled every three hours (D7).
PUBLISH_INTERVAL_SECONDS = 10800

#: How many missed publications before a consumer should call the data stale.
#: ⚠ **Three, not one.** `C-11` says a scheduled run can be delayed or dropped,
#: and one was observed **163 minutes** late on 2026-09-08 -- more than half an
#: interval. A marker that cried stale on a single late run would be noise, and
#: a noisy marker is one consumers learn to ignore, which costs exactly the
#: signal this exists to send.
STALE_AFTER_INTERVALS = 3


@dataclass(frozen=True, slots=True)
class Freshness:
    """What a consumer can conclude about a dataset's age."""

    #: Seconds between `generated_at` and `now`. Negative where the dataset is
    #: stamped in the future, which is a defect rather than freshness.
    age_seconds: float
    stale: bool
    #: True when the stamp is ahead of `now` by more than a clock skew. ⛔ Not
    #: folded into `stale`: a future stamp is a different fault with a different
    #: cause, and reporting it as freshness would hide it entirely.
    from_the_future: bool
    detail: str

    def as_dict(self) -> dict[str, object]:
        return {"age_seconds": round(self.age_seconds, 3), "stale": self.stale,
                "from_the_future": self.from_the_future, "detail": self.detail}


def parse_instant(text: str) -> datetime.datetime:
    """An ISO 8601 UTC instant, as this project writes them.

    ⚠ Raises `ValueError` on anything it cannot read rather than returning a
    default. A staleness check that treated an unreadable timestamp as fresh
    would report health over a dataset nobody can date, which is the failure
    this module exists to make impossible.
    """
    raw = text.strip()
    if raw.endswith("Z"):
        raw = raw[:-1] + "+00:00"
    moment = datetime.datetime.fromisoformat(raw)
    if moment.tzinfo is None:
        moment = moment.replace(tzinfo=datetime.timezone.utc)
    return moment.astimezone(datetime.timezone.utc)


def assess(generated_at: str, *, now: datetime.datetime,
           interval_seconds: int = PUBLISH_INTERVAL_SECONDS,
           intervals: int = STALE_AFTER_INTERVALS,
           skew_seconds: int = 300) -> Freshness:
    """Decide whether a dataset stamped `generated_at` is still being updated.

    ⭐ **The whole test a consumer needs**, and it needs nothing of ours to run
    it: the stamp is in the file they hold and the cadence is beside it.
    """
    stamped = parse_instant(generated_at)
    age = (now - stamped).total_seconds()
    threshold = interval_seconds * intervals
    if age < -skew_seconds:
        return Freshness(
            age_seconds=age, stale=False, from_the_future=True,
            detail=(f"generated_at is {abs(age):.0f}s in the future, which is "
                    f"a clock or a pipeline defect rather than freshness"))
    stale = age > threshold
    missed = age / interval_seconds if interval_seconds else 0.0
    return Freshness(
        age_seconds=age, stale=stale, from_the_future=False,
        detail=(f"{age:.0f}s old, about {missed:.1f} publication interval(s); "
                f"stale after {intervals}") if stale else
               f"{age:.0f}s old, within {intervals} publication interval(s)")
