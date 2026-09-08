"""Evidence about a tracker that this project did not observe itself. T-031.

Four categories of tracker sit at `unmeasurable` from a GitHub runner:
IPv6-only, i2p, yggdrasil and `ws`/`wss`. ⛔ **`unmeasurable` is the honest
label on our data and is never a reason to stop trying** (RULES 10.1a), and the
routes that remain all share one shape: somebody or something with reach we do
not have looks at the tracker, and we record what they saw.

⭐ **One mechanism, not four special cases.** An oracle's uptime list, a probe
run through a proxy with IPv6 egress, a public gateway into a network we cannot
route to, and a run from a second vantage are the same kind of evidence with
different observers. They differ in the `observer` and `method` fields and in
nothing else.

⛔ **A SECOND-HAND SIGNAL IS NEVER PROMOTED TO A MEASUREMENT.** That is T-031's
decision and it is enforced here rather than remembered:

  * `as_record` cannot emit `health_state`, `measurement_rung`, `ok` or
    `rtt_ms`. Those are a probe's vocabulary, and a record carrying them would
    be indistinguishable from one after a copy through a JSON file.
  * `annotate` attaches observations to a health record and **returns the
    health state unchanged**. There is no argument that lets it decide one.
  * every record says `direct: false` and carries who observed it, when, and
    by what method, because a signal without its provenance is exactly the
    confident wrongness this project exists to avoid.

WHAT IT IS FOR AND WHAT IT IS NOT

It is for saying "we could not reach this tracker, and here is who could".
It is not a way to reach a tracker that refused us: RULES 4.1's immovable line
is that no route is used to evade an exclusion already given, and a BEP 34
denial stops a probe before any of this is reachable.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Any, Iterable

__all__ = [
    "Signal", "Observation", "annotate", "PROVENANCE", "PROBE_ONLY_KEYS",
]

#: Travels on every record. RULES 1.4: a capability with no class is a promise
#: nobody can hold you to, and this whole module is the `externally dependent`
#: class.
PROVENANCE = (
    "second-hand: observed by somebody else, not by this project. Recorded "
    "with its observer, its method and the moment it was taken, and never "
    "merged with a direct probe result."
)

#: ⛔ The keys a direct probe owns. A second-hand record may not carry one,
#: because a file is the only thing that travels between a measurement and a
#: consumer and these are what a consumer keys on.
PROBE_ONLY_KEYS: frozenset[str] = frozenset({
    "health_state", "measurement_rung", "ok", "rtt_ms", "resolved_ip",
})


class Signal(str, Enum):
    """What the observer saw. Three values, and the third is not padding.

    An observer that has never assessed a tracker has told us nothing, and
    recording that as "not alive" would be an absence read as a zero (RULES 2).
    """

    ALIVE = "alive"
    NOT_ALIVE = "not_alive"
    UNASSESSED = "unassessed"


@dataclass(frozen=True, slots=True)
class Observation:
    """One observer's answer about one tracker URL.

    Every field is required except `detail`, and that is the point: an
    observation missing its observer or its date is not weaker evidence, it is
    unusable evidence, and a dataclass is where that gets refused rather than
    reviewed.
    """

    url: str
    observer: str
    #: When the observer saw it, ISO 8601 UTC. Not when we read it: a snapshot
    #: read today can be a week old and the difference is the whole value of
    #: the field.
    observed_at: str
    #: How they saw it, in enough detail to judge the answer. "announces to the
    #: tracker" and "scrapes it through an IPv6 proxy" support very different
    #: conclusions.
    method: str
    signal: Signal
    #: What the observer could do that this vantage could not. The reason the
    #: observation exists at all.
    reach: str
    detail: str = ""

    def __post_init__(self) -> None:
        for field_name in ("url", "observer", "observed_at", "method", "reach"):
            if not getattr(self, field_name):
                raise ValueError(
                    f"a second-hand observation needs {field_name}: an "
                    f"observation without its provenance is unusable")

    def as_record(self) -> dict[str, Any]:
        """The shape that goes into a result file.

        ⛔ `direct` is first and is always `False`. It is not a flag a caller
        sets: there is no code path here that can produce `True`.
        """
        record = {
            "direct": False,
            "url": self.url,
            "observer": self.observer,
            "observed_at": self.observed_at,
            "method": self.method,
            "signal": self.signal.value,
            "reach": self.reach,
            "detail": self.detail,
            "provenance": PROVENANCE,
        }
        # Belt and braces, and it has a reason: this dict is edited by future
        # sessions and the invariant is one field away from being lost.
        leaked = PROBE_ONLY_KEYS & set(record)
        if leaked:
            raise AssertionError(
                f"a second-hand record carries probe-only key(s) {sorted(leaked)}")
        return record


def annotate(health_record: dict[str, Any],
             observations: Iterable[Observation]) -> dict[str, Any]:
    """Attach observations to a health record without touching its verdict.

    Returns a new dict. ⛔ **`health_state` comes out exactly as it went in**,
    whatever the observers said. A tracker this vantage cannot reach stays
    `unmeasurable`; what changes is that the record now says who could reach it
    and what they found, which is the difference between a dead end and a lead.
    """
    annotated = dict(health_record)
    annotated["second_hand"] = [o.as_record() for o in observations]
    if "health_state" in health_record:
        annotated["health_state"] = health_record["health_state"]
    return annotated
