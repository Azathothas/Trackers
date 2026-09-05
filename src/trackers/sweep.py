"""Probe many trackers politely: bounded, serialised per host, under a deadline.

T-029, and T-024 with it, because a health record that carries its vantage is
useless until something actually writes one and nothing could write one until
the corpus could be probed in less than an hour.

THE THREE BOUNDS, AND WHAT EACH IS FOR

    concurrency     how many *distinct hosts* are in flight at once. Bounds
                    what the runner and our egress do. RULES 15.2.
    per host        exactly one probe per host at a time, in **both** profiles
                    and not configurable. Bounds what one operator sees.
    deadline        a wall-clock ceiling for the whole run. Bounds the job.

⛔ **The per-host rule is not only politeness.** RULES 2 requires checking
whether observing changed the answer, and a tracker that rate-limits after the
first request answers the second differently. Two concurrent probes to one host
would make the second measurement a measurement of the first one.

⛔ **A tracker not reached before the deadline is `unknown`, never `dead`.**
Running out of time is a fact about us. This is the same rule as `unmeasurable`
and it fails in the same direction if broken: our budget published as somebody
else's outage.

THE UDP BUDGET, AND WHY BEP 15's OWN IS UNUSABLE

BEP 15 says retry at `15 * 2^n` seconds for `n` in 0..8: nine attempts and up
to 62 minutes for one tracker. A diagnostic that takes an hour to say "this
tracker is down" has not answered the question, and a production client refuses
it for that reason
(`references/Azathothas__bit-cli/tree/docs/trackers.md:230`): three attempts
inside one timeout, an attempt being `max(timeout / 3, 1s)`.

⭐ **The arithmetic is adopted; the seconds are not.** A UDP exchange is *two*
round trips, connect then scrape, and either can be the one that dies, so the
worst case is **five** attempts and not three: a connect answered on its third
attempt leaves three more for the scrape. Their numbers were measured on their
hardware against their own loopback tracker, so `udp_budget` takes the timeout
and the floor as arguments rather than baking either in.

WHY THREADS AND NOT ASYNCIO

T-029 proposed `asyncio`. That would mean an async rewrite of `probe_udp` and
`probe_http`, which are synchronous socket and `urllib` code -- so the project
would carry **two implementations of the probe**, and the fix for a defect in
one would never reach the other. `code.md` forbids exactly that, and this
workload is latency-bound IO where a bounded thread pool and a bounded event
loop are the same shape. So the *production probe code path* is the one that
runs here, unmodified, and the deviation is recorded on the entry.
"""

from __future__ import annotations

import concurrent.futures
import threading
import time
from dataclasses import dataclass, field
from typing import Any, Callable, Iterable, Sequence

from .bep34 import Resolver
from .model import HealthState, Rung, Tracker
from .probe import (Failure, ProbeConfig, ProbeResult, health_state, probe)
from .profile import Budget, budget_for
from .vantage import UNKNOWN, Vantage, detect as detect_vantage

__all__ = [
    "UDP_ATTEMPT_FLOOR", "UDP_WORST_CASE_ATTEMPTS", "SweepConfig",
    "SweepResult", "udp_attempt_timeout", "udp_budget", "select", "sweep",
]

#: One attempt is never shorter than this, whatever the timeout. Below it the
#: floor decides the budget rather than the nominal timeout, which is the point:
#: a 0.5 s timeout would otherwise give a 0.17 s attempt and measure the
#: network's jitter instead of the tracker.
UDP_ATTEMPT_FLOOR = 1.0

#: Connect can burn three attempts and leave three for the scrape, and the run
#: gives up after five. Six cannot happen.
UDP_WORST_CASE_ATTEMPTS = 5


def udp_attempt_timeout(timeout: float, floor: float = UDP_ATTEMPT_FLOOR) -> float:
    """How long one UDP attempt may take."""
    return max(timeout / 3.0, floor)


def udp_budget(timeout: float, floor: float = UDP_ATTEMPT_FLOOR) -> float:
    """The worst-case wall time for one UDP tracker, in seconds."""
    return UDP_WORST_CASE_ATTEMPTS * udp_attempt_timeout(timeout, floor)


@dataclass(frozen=True, slots=True)
class SweepConfig:
    """What one sweep may spend. Every field bounds something (RULES 5.2)."""

    #: Per-probe socket timeout handed to `ProbeConfig`.
    timeout: float = 5.0
    #: Wall-clock ceiling for the whole run. `None` means no deadline, which is
    #: only correct where a caller has its own -- a workflow job timeout, say.
    deadline_seconds: float | None = None
    #: Shortest permitted UDP attempt. Exposed so a test can shrink it.
    attempt_floor: float = UDP_ATTEMPT_FLOOR

    def probe_config(self) -> ProbeConfig:
        """The per-probe settings, with the UDP attempt arithmetic applied.

        `retries` is attempts-minus-one, and the probe's own loop is the
        connect half of the exchange, so it gets two retries: three attempts,
        which is the half of `UDP_WORST_CASE_ATTEMPTS` a connect can consume.
        """
        return ProbeConfig(timeout=udp_attempt_timeout(self.timeout,
                                                       self.attempt_floor),
                           retries=2)


@dataclass
class SweepResult:
    """Every record, plus the arithmetic needed to read them honestly."""

    records: list[dict[str, Any]] = field(default_factory=list)
    #: Trackers selected but never reached before the deadline.
    not_reached: int = 0
    #: Trackers not probed because the operator refused or the lookup failed.
    refused: int = 0
    #: Trackers structurally unmeasurable from this vantage.
    unmeasurable: int = 0
    probed: int = 0
    selected: int = 0
    corpus: int = 0
    deadline_hit: bool = False

    def states(self) -> dict[str, int]:
        from collections import Counter
        c = Counter(str(r.get("health_state")) for r in self.records)
        return dict(sorted(c.items()))


def select(trackers: Sequence[Tracker], budget: Budget) -> list[Tracker]:
    """Which trackers this profile probes, deterministically.

    ⚠ **Stride, not head.** `ci` probes a sample, and taking the first N of a
    sorted corpus samples one end of it: `Tracker.sort_key` leads with the
    transport, so the head is entirely `http` and a broken UDP path would never
    show up. A stride walks the whole corpus and keeps every transport,
    network and host family represented.

    Deterministic by construction (RULES 3.6): no randomness, no set ordering,
    and the same corpus always yields the same sample.
    """
    ordered = sorted(trackers, key=Tracker.sort_key)
    if budget.full_corpus_sweep or budget.sample_size is None:
        return ordered
    if budget.sample_size >= len(ordered):
        return ordered
    stride = len(ordered) / float(budget.sample_size)
    return [ordered[int(i * stride)] for i in range(budget.sample_size)]


class _HostLocks:
    """One lock per host, created on demand.

    Not a plain dict: two threads reaching an unseen host at the same moment
    would each create a lock and each acquire their own, which is the bug this
    class exists to make unrepresentable.
    """

    def __init__(self) -> None:
        self._guard = threading.Lock()
        self._locks: dict[str, threading.Lock] = {}

    def for_host(self, host: str) -> threading.Lock:
        with self._guard:
            lock = self._locks.get(host)
            if lock is None:
                lock = threading.Lock()
                self._locks[host] = lock
            return lock


def _deadline_record(tracker: Tracker, vantage: Vantage,
                     observed_at: str) -> ProbeResult:
    """What a tracker the run never reached is recorded as.

    Not a probe that failed -- nothing was sent. `DEADLINE_EXCEEDED` is in
    `ABOUT_US` and `health_state` maps it to `unknown`, so this can never
    become `dead`.
    """
    return ProbeResult(
        url=tracker.url, transport=tracker.transport, network=tracker.network,
        rung=Rung.NONE, ok=False, failure=Failure.DEADLINE_EXCEEDED,
        detail="the run's deadline arrived before this tracker was probed",
        observed_at=observed_at, vantage=vantage.as_dict())


def _state_for(result: ProbeResult, tracker: Tracker) -> HealthState:
    """One observation's health state.

    `sample_count` is 1 for a probe that happened and 0 for one that did not,
    which is what keeps a never-probed tracker `unknown`. A single observation
    can therefore never reach `dead`: `MIN_SAMPLES_FOR_DEATH` is 3, and
    accumulating samples across runs is T-040's job, not this module's.
    """
    probed = result.failure is not Failure.DEADLINE_EXCEEDED
    return health_state(
        rung=result.rung, transport=result.transport, network=result.network,
        sample_count=1 if probed else 0,
        success_count=1 if result.ok else 0,
        failure=result.failure,
        measurable=tracker.is_measurable_here)


def sweep(trackers: Sequence[Tracker], *,
          config: SweepConfig | None = None,
          budget: Budget | None = None,
          vantage: Vantage | None = None,
          resolver: Resolver | None = None,
          observed_at: str = UNKNOWN,
          monotonic: Callable[[], float] = time.monotonic,
          probe_fn: Callable[..., ProbeResult] = probe) -> SweepResult:
    """Probe a corpus and return one health record per selected tracker.

    ⭐ **One `Resolver` for the whole run.** BEP 34 is consulted per host and
    the answer is cached on the resolver, so a corpus with many URLs on one
    host asks DNS once. Building one per probe would multiply the run's DNS
    load by the number of URLs per host, for nothing.

    `monotonic` and `probe_fn` are injected so the deadline and the ordering
    can be tested without waiting and without a network. The production
    defaults are the real clock and the real probe.
    """
    config = config or SweepConfig()
    budget = budget or budget_for()
    vantage = vantage or detect_vantage()
    resolver = resolver or Resolver()
    cfg = config.probe_config()

    chosen = select(trackers, budget)
    out = SweepResult(corpus=len(trackers), selected=len(chosen))

    started = monotonic()
    deadline = (started + config.deadline_seconds
                if config.deadline_seconds is not None else None)
    locks = _HostLocks()
    results: dict[str, ProbeResult] = {}
    guard = threading.Lock()

    def run_one(tracker: Tracker) -> None:
        # Checked before the host lock is taken, so a queue of threads waiting
        # on one slow host drains immediately once the deadline passes instead
        # of each waiting its turn to discover the same thing.
        try:
            if deadline is not None and monotonic() >= deadline:
                result = _deadline_record(tracker, vantage, observed_at)
            else:
                with locks.for_host(tracker.host):
                    if deadline is not None and monotonic() >= deadline:
                        result = _deadline_record(tracker, vantage, observed_at)
                    else:
                        result = probe_fn(tracker, cfg, vantage, observed_at,
                                          resolver)
        except Exception as e:  # noqa: BLE001
            # ⛔ RULES 3.8, applied to trackers: one failing tracker does not
            # fail the others. `probe` documents that it never raises, and a
            # promise is not a mechanism -- an exception escaping here used to
            # take the **whole sweep** down through `pool.map`, losing every
            # other tracker's measurement to one defect. Found by attacking
            # this function rather than by a test that already believed it.
            #
            # `PROBE_ERROR` is never `dead`: `health_state` maps it to `error`,
            # which is the state that exists so a broken probe cannot be
            # published as somebody else's outage.
            result = ProbeResult(
                url=tracker.url, transport=tracker.transport,
                network=tracker.network, rung=Rung.NONE, ok=False,
                failure=Failure.PROBE_ERROR,
                detail=f"the probe raised: {type(e).__name__}: {e}",
                observed_at=observed_at, vantage=vantage.as_dict())
        with guard:
            results[tracker.url] = result

    workers = max(1, budget.max_concurrency)
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
        list(pool.map(run_one, chosen))

    # Emitted in the corpus's own total order rather than completion order, so
    # two runs over one corpus produce the same file (RULES 3.6).
    for tracker in chosen:
        result = results[tracker.url]
        state = _state_for(result, tracker)
        record = result.as_record(state)
        # ⛔ The sweep is what promises RULES 3.4, so it does not delegate the
        # promise. Every prober fills this in today; a record without a vantage
        # is nevertheless made unrepresentable here rather than merely
        # unlikely, because the failure is silent -- a consumer reads `dead`
        # and cannot tell it means `dead from one datacenter, over IPv4`.
        if not record.get("vantage"):
            record["vantage"] = vantage.as_dict()
        out.records.append(record)
        if result.failure is Failure.DEADLINE_EXCEEDED:
            out.not_reached += 1
        elif result.failure in (Failure.EXCLUDED_BY_OPERATOR,
                                Failure.EXCLUSION_UNDETERMINED):
            out.refused += 1
        elif result.failure is Failure.UNSUPPORTED:
            out.unmeasurable += 1
        else:
            out.probed += 1
    out.deadline_hit = out.not_reached > 0
    return out


def render_sweep(result: SweepResult, *, generated_at: str,
                 vantage: Vantage, budget: Budget,
                 config: SweepConfig) -> dict[str, Any]:
    """The health-record document. `scripts/check-vantage-metadata.py` reads it.

    The conditions travel with the records rather than beside them, because a
    file of health states whose vantage lives somewhere else is one copy away
    from being read as universal.
    """
    return {
        "generated_at": generated_at,
        "vantage": vantage.as_dict(),
        "budget": budget.as_record(),
        "limits": {
            "timeout_seconds": config.timeout,
            "udp_attempt_seconds": udp_attempt_timeout(config.timeout,
                                                       config.attempt_floor),
            "udp_worst_case_seconds": udp_budget(config.timeout,
                                                 config.attempt_floor),
            "deadline_seconds": config.deadline_seconds,
        },
        "counts": {
            "corpus": result.corpus,
            "selected": result.selected,
            "probed": result.probed,
            "refused_or_undetermined": result.refused,
            "unmeasurable": result.unmeasurable,
            "not_reached_before_deadline": result.not_reached,
            "health_states": result.states(),
        },
        "trackers": result.records,
    }
