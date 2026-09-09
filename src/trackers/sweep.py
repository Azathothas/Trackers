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
import hashlib
import threading
import time
from dataclasses import dataclass, field
from typing import Any, Callable, Iterable, Mapping, Sequence

from .bep34 import Resolver
from .model import HealthState, Rung, Tracker
from .probe import (Failure, ProbeConfig, ProbeResult, health_state, probe)
from .politeness import (DEFAULT_INTERVAL_SECONDS, contactable_at,  # noqa: F401
                         run_cost)
from .profile import Budget, budget_for
from .vantage import UNKNOWN, Vantage, detect as detect_vantage

__all__ = [
    "UDP_ATTEMPT_FLOOR", "UDP_WORST_CASE_ATTEMPTS", "SweepConfig",
    "SweepResult", "Selection", "udp_attempt_timeout", "udp_budget", "select",
    "plan", "sweep", "slices_for", "slice_of",
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
    #: ⛔ Trackers in this run's slice that D7 forbade contacting, and they are
    #: a count rather than records **on purpose**: a tracker that was not
    #: probed has not been observed, and writing a record for one is the
    #: double-fold corruption in another costume -- a non-observation folded
    #: into the history as an observation, three of which say `dead`.
    held_by_ceiling: int = 0
    #: What the run actually took, which is not always what it asked for.
    selection: "Selection | None" = None

    def states(self) -> dict[str, int]:
        from collections import Counter
        c = Counter(str(r.get("health_state")) for r in self.records)
        return dict(sorted(c.items()))


def slices_for(corpus_size: int, sample_size: int) -> int:
    """How many runs a rotation takes to cover the corpus once."""
    if sample_size <= 0 or corpus_size <= sample_size:
        return 1
    return -(-corpus_size // sample_size)  # ceiling division


def slice_of(url: str, slices: int) -> int:
    """Which slice a tracker belongs to, decided by the tracker alone.

    ⛔ **Not by its position, and the difference is the whole rotation.** An
    index-based slice looks correct and degenerates the moment the corpus
    changes size: at 1327 trackers over 7 slices, adding **one** shifts every
    index by one, so the next run's slice is *exactly* the previous run's set.
    The upstreams regenerate daily. Measured on 2026-09-08 by the adversarial
    pass -- one added tracker gave a 100% overlap between consecutive runs,
    which is the rotation silently ceasing to rotate.

    Hashing the URL makes membership a property of the tracker: a corpus that
    gains or loses entries moves nobody else between slices.

    ⚠ **The price is that slices are not exactly equal.** They are 190 or so
    of 1327 with the spread a hash gives, and `sample_size` is a sampling
    figure rather than a hard ceiling -- what actually bounds a run is the
    concurrency limit, the per-host rule and the deadline.
    """
    digest = hashlib.sha256(url.encode("utf-8")).digest()
    return int.from_bytes(digest[:8], "big") % max(1, slices)


def select(trackers: Sequence[Tracker], budget: Budget,
           rotation: int = 0) -> list[Tracker]:
    """Which trackers this profile probes, deterministically.

    ⚠ **Stride, not head.** `ci` probes a sample, and taking the first N of a
    sorted corpus samples one end of it: `Tracker.sort_key` leads with the
    transport, so the head is entirely `http` and a broken UDP path would never
    show up. A stride walks the whole corpus and keeps every transport,
    network and host family represented.

    ⛔ **AND IT ROTATES, WHICH THE SCHEDULE MADE NECESSARY.** A fixed stride
    probes the **same** 190 trackers on every run: schedule that every three
    hours and those 190 are contacted eight times a day forever while the other
    1137 are never contacted again, so nothing about them can ever leave
    `unknown` -- `MIN_SAMPLES_FOR_DEATH` needs three observations and they
    would never get a second. `rotation` selects one of
    `slices_for(corpus, sample)` disjoint slices whose union is the whole
    corpus, so consecutive runs walk it, and `slice_of` decides membership from
    the tracker rather than from its position.

    ⭐ The rotation also **lowers** per-tracker load rather than raising it. At
    1327 trackers and a sample of 200 a pass takes 7 runs, so each tracker is
    probed once per 21 hours: well inside D7's three-hour ceiling rather than
    at it.

    Deterministic by construction (RULES 3.6): no randomness, no set ordering,
    and the same corpus and rotation always yield the same sample.
    """
    ordered = sorted(trackers, key=Tracker.sort_key)
    if budget.full_corpus_sweep or budget.sample_size is None:
        return ordered
    if budget.sample_size >= len(ordered):
        return ordered
    slices = slices_for(len(ordered), budget.sample_size)
    keep = rotation % slices
    return [t for t in ordered if slice_of(t.url, slices) == keep]


@dataclass(frozen=True, slots=True)
class Selection:
    """What a run will contact, and what it is holding back.

    ⛔ **Both halves, and the second one is why this is a value rather than a
    list.** A run that contacts nobody because D7 says everyone was contacted
    an hour ago is the correct outcome; a run that contacts nobody because it
    was pointed at an empty corpus is a defect. A caller handed only the first
    half cannot tell those apart, and would report one as the other.
    """

    trackers: tuple[Tracker, ...]
    #: The rotation actually used, which is not always the one asked for.
    rotation: int
    requested_rotation: int
    slices: int
    #: URLs this run may not contact yet, and the reason is always the same
    #: one: D7's interval has not elapsed since the last recorded contact.
    held_by_ceiling: tuple[str, ...] = ()
    #: True where every slice from the requested one onward was fully held.
    exhausted: bool = False
    #: ⛔ Whether a recorded history was consulted at all. `False` is not a
    #: neutral default: it says the ceiling rests on the rotation's arithmetic,
    #: which is exactly the arrangement that published 192 double contacts.
    #: A reader must be able to tell "nobody was held" from "nobody was
    #: checked", and those are the same number.
    from_history: bool = False

    @property
    def advanced(self) -> bool:
        return self.rotation != self.requested_rotation

    def as_record(self) -> dict[str, Any]:
        """The block a run report carries so a consumer can check the ceiling
        was applied rather than take it on trust."""
        return {
            "rotation": self.rotation,
            "requested_rotation": self.requested_rotation,
            "slice": self.rotation % max(1, self.slices),
            "slices": self.slices,
            "advanced_past_a_held_slice": self.advanced,
            "held_by_politeness_ceiling": len(self.held_by_ceiling),
            "interval_seconds": DEFAULT_INTERVAL_SECONDS,
            "every_slice_held": self.exhausted,
            "enforced_from_history": self.from_history,
        }


def plan(trackers: Sequence[Tracker], budget: Budget, rotation: int = 0, *,
         last_seen: Mapping[str, str] | None = None,
         now: str | None = None) -> Selection:
    """Which trackers this run may contact, with D7 enforced from the record.

    ⛔ **THE ROTATION SAMPLES; THIS ENFORCES.** `select` decides which slice of
    the corpus a run looks at, and until 2026-09-09 that arithmetic *was* the
    politeness ceiling: the claim "each tracker is probed once per 21 hours"
    held only if consecutive runs took consecutive slices. They do not.
    `rotation_for` maps an instant to the three-hour bucket containing it, so
    two runs inside one bucket take the **same** slice -- measured, and
    published: runs `34281244142` and `34289476724` both reported `slice 5 of
    7`, and 192 trackers carry observations 5878 s apart in `state.jsonl`,
    inside a 10800 s ceiling. `politeness.too_soon_after` reads what was
    recorded instead of what was assumed, and this is where the sweep asks it.
    T-087.

    ⭐ **And it advances rather than idling.** Holding a whole slice back turns
    a politeness breach into a coverage gap: the run makes no requests, the
    slot is spent, and the corpus is walked more slowly than the seven-run pass
    the schedule is sized for -- which is what `MIN_SAMPLES_FOR_DEATH` and
    T-012's subject set both wait on. So a slice with nothing due yields to the
    next one, up to a full turn of the rotation. Bounded by construction: at
    most `slices` candidates are considered and each is examined once.

    ⚠ **`last_seen` absent means no history was supplied, and then this
    behaves exactly as `select` alone did.** That is not a hole to be closed by
    defaulting: a first run has no history, and a run that refused to probe
    because it could not find a file would be a sweep that stops measuring the
    day a path changes. `scripts/probe-corpus.py` states in its output which of
    the two happened, so an unenforced run is visible rather than assumed.

    Idempotent in its own output: planning again from the rotation this
    returned yields the same rotation, because a slice with work does not
    yield. `probe-corpus.py` depends on that -- it previews once and sweeps
    once, and the two must agree (RULES 3.6).
    """
    ordered = sorted(trackers, key=Tracker.sort_key)
    slices = slices_for(len(ordered), budget.sample_size or len(ordered))
    if last_seen is None or now is None:
        chosen = select(ordered, budget, rotation)
        return Selection(trackers=tuple(chosen), rotation=rotation,
                         requested_rotation=rotation, slices=slices,
                         from_history=False)

    held_seen: list[str] = []
    for step in range(max(1, slices)):
        candidate = rotation + step
        picked = select(ordered, budget, candidate)
        due, held = contactable_at(last_seen, now, [t.url for t in picked])
        held_seen.extend(held)
        if due:
            return Selection(
                trackers=tuple(t for t in picked if t.url in set(due)),
                rotation=candidate, requested_rotation=rotation,
                # ⛔ EVERY slice this run passed over, not just the one it
                # landed on. Reporting the last slice's holds alone made an
                # advanced run say `held_by_politeness_ceiling: 0` while 192
                # trackers had been held -- which is precisely the "nobody was
                # held" / "nobody was checked" confusion this field exists to
                # prevent, reintroduced inside the fix for it. Caught by
                # `test_it_advances_rather_than_idling_through_the_slot`.
                slices=slices, held_by_ceiling=tuple(held_seen),
                from_history=True)
        # ⛔ One slice is the whole corpus under `local` and under a corpus
        # smaller than the sample, so there is nothing to advance to and the
        # loop must not pretend otherwise by wrapping onto itself.
        if slices <= 1:
            break
    return Selection(trackers=(), rotation=rotation,
                     requested_rotation=rotation, slices=slices,
                     held_by_ceiling=tuple(held_seen), exhausted=True,
                     from_history=True)


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
          rotation: int = 0,
          last_seen: Mapping[str, str] | None = None,
          monotonic: Callable[[], float] = time.monotonic,
          probe_fn: Callable[..., ProbeResult] = probe) -> SweepResult:
    """Probe a corpus and return one health record per selected tracker.

    ⭐ **One `Resolver` for the whole run.** BEP 34 is consulted per host and
    the answer is cached on the resolver, so a corpus with many URLs on one
    host asks DNS once. Building one per probe would multiply the run's DNS
    load by the number of URLs per host, for nothing.

    ⛔ **`last_seen` is D7's ceiling and it goes through `plan`**, which is the
    only door into a selection. Passing the rotation alone selects a slice and
    enforces nothing, which is what let two runs 98 minutes apart contact the
    same 192 trackers (T-087). `observed_at` is the instant the ceiling is
    measured against, because it is the instant every record in this run is
    stamped with -- a run cannot be polite against a clock other than the one
    it publishes.

    `monotonic` and `probe_fn` are injected so the deadline and the ordering
    can be tested without waiting and without a network. The production
    defaults are the real clock and the real probe.
    """
    config = config or SweepConfig()
    budget = budget or budget_for()
    vantage = vantage or detect_vantage()
    resolver = resolver or Resolver()
    cfg = config.probe_config()

    selection = plan(trackers, budget, rotation, last_seen=last_seen,
                     now=None if last_seen is None else observed_at)
    chosen = list(selection.trackers)
    out = SweepResult(corpus=len(trackers), selected=len(chosen),
                      held_by_ceiling=len(selection.held_by_ceiling),
                      selection=selection)

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
        # T-026: what this run cost the people it measured, computed from the
        # records it produced rather than from a corpus figure typed beside
        # them. `polite` is a verdict a consumer can check.
        # ⛔ T-087. `politeness` above is a **projection** from an assumed
        # cadence; this is what the run was actually permitted to do, read from
        # the recorded history. `enforced_from_history: false` means no history
        # was supplied and the ceiling rests on the rotation's arithmetic alone
        # -- the state that published 192 double contacts -- so it is stated
        # rather than implied.
        "ceiling": (result.selection.as_record() if result.selection
                    else {"enforced_from_history": False}),
        "counts": {
            "corpus": result.corpus,
            "selected": result.selected,
            "probed": result.probed,
            "refused_or_undetermined": result.refused,
            "unmeasurable": result.unmeasurable,
            "not_reached_before_deadline": result.not_reached,
            "held_by_politeness_ceiling": result.held_by_ceiling,
            "health_states": result.states(),
        },
        "trackers": result.records,
    }
