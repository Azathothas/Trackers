"""What a run costs the people it measures, computed rather than asserted.

T-026 and decision **D7**. RULES 4 states a ceiling and nothing computed it:
`trackers x probes-per-tracker x runs-per-day` was a sentence, the per-tracker
rate was never published, and no test read the configured schedule. ⛔ **A
ceiling nobody measures is a preference**, and this project's whole argument is
that a number without an instrument behind it is not a number.

THE ANCHOR IS THE TRACKER'S OWN NUMBER

D7: probe each tracker on its stated interval, defaulting to three hours where
it has not stated one. The default is not taste. It is newTrackon's floor --
`references/CorralPeltzer__newTrackon/tree/newtrackon/tracker.py` clamps its
recheck interval at 10800 seconds -- and its maintainer's statement in issue
#334 that "the current checking frequency (every ~3 hours) is reasonable for
the server load". ⭐ That monitor **announces**; this project stops at connect
and scrape, so it does strictly less work per check than the analogue the
number comes from.

⛔ **BOTH KEYS, AND THE STRICTER OF THE TWO.** A tracker may state `interval`
and `min interval`, BEP 3 spells the second with a space, and the underscore
form occurs in the wild (`C-65`). Taking `interval` alone ignores the floor an
operator would actually judge us by, so `stated_interval` takes the **maximum**
of whatever was stated: never sooner than either number.

⚠ **The number has to survive the record to be usable.** `classify_body` read
both keys from the moment `C-65` landed and `ProbeResult.as_record` dropped
them, so nothing downstream could have honoured a tracker's request even in
principle. They are on the record now, which is what makes the assertions below
possible.

⛔ **PROJECTING THE CEILING IS NOT ENFORCING IT, AND FOR A DAY THIS MODULE ONLY
PROJECTED.** `run_cost` and `schedule_violations` answer what a cadence *would*
cost, from `seconds_between_runs` -- a number the caller states. The sweep's
actual spacing came from its rotation instead, and the rotation is a wall-clock
bucket: two runs inside one bucket take the same slice and contact the same
trackers twice. That happened on 2026-09-08 and is published. `too_soon_after`
is the enforcement that was missing -- it reads what was *recorded* rather than
what was *assumed*, so no arithmetic anywhere else can breach D7 again. T-087.

DNS IS INSIDE THE BUDGET

Operator ruling, 2026-09-08: **100,000 lookups per run on a GitHub runner**,
local runs unbounded. A sweep spends them in three places -- BEP 34 consent per
host, `getaddrinfo` per probe, and the second opinion T-037 asks for whenever
the first fails -- and none of them was counted before this module existed.
"""

from __future__ import annotations

import datetime
import ipaddress
from dataclasses import dataclass
from typing import Any, Iterable, Mapping
from urllib.parse import urlsplit

__all__ = [
    "DEFAULT_INTERVAL_SECONDS", "DNS_LOOKUPS_PER_RUN_CEILING",
    "SECONDS_PER_DAY", "RunCost", "stated_interval", "probes_per_day",
    "run_cost", "schedule_violations", "seconds_between", "contactable_at",
    "too_soon_after",
]

#: D7's default where a tracker has stated nothing. Three hours, in seconds.
DEFAULT_INTERVAL_SECONDS = 10_800

#: Operator ruling, 2026-09-08. A ceiling on the whole run, not per host.
DNS_LOOKUPS_PER_RUN_CEILING = 100_000

SECONDS_PER_DAY = 86_400

#: How many DNS questions one host costs at worst. BEP 34 consent is one TXT
#: lookup, cached per host per run; `getaddrinfo` is one; and a failure buys a
#: second opinion of one query per address family against up to three
#: resolvers (T-037). ⚠ Measured as a worst case, not an average: the point of
#: a budget is what happens when everything misses.
DNS_QUERIES_PER_HOST_WORST_CASE = 1 + 1 + (2 * 3)


@dataclass(frozen=True, slots=True)
class RunCost:
    """What one run spends, and what a day of them spends.

    Every field is a count somebody else pays. `within_dns_ceiling` is a
    verdict rather than a suggestion: a run that exceeds it has spent a
    resource the operator bounded.
    """

    trackers_probed: int
    hosts: int
    #: Hosts that cost a DNS query. ⛔ Not the same as `hosts`: an address
    #: literal is never looked up -- `bep34.Resolver.consult` returns ALLOW
    #: without asking and `getaddrinfo` on a literal sends nothing -- and 206
    #: of this corpus's 965 hosts are literals. Charging them a lookup
    #: overstated a full sweep by 1648 queries.
    resolvable_hosts: int
    runs_per_day: float
    #: One request per tracker per run. There is no retry against a real
    #: tracker and no second endpoint, so this is the count, not an estimate.
    probes_per_run: int
    probes_per_day: int
    dns_worst_case_per_run: int
    within_dns_ceiling: bool
    seconds_between_runs: float
    #: Trackers whose stated interval is longer than the gap between runs. A
    #: non-empty list is a schedule that probes somebody faster than they asked.
    too_often_for: tuple[str, ...] = ()

    @property
    def polite(self) -> bool:
        return self.within_dns_ceiling and not self.too_often_for

    def as_record(self) -> dict[str, Any]:
        """The block that goes into a run report, so a consumer can check it."""
        return {
            "trackers_probed": self.trackers_probed,
            "hosts": self.hosts,
            "resolvable_hosts": self.resolvable_hosts,
            "runs_per_day": self.runs_per_day,
            "seconds_between_runs": self.seconds_between_runs,
            "probes_per_run": self.probes_per_run,
            "probes_per_day": self.probes_per_day,
            "default_interval_seconds": DEFAULT_INTERVAL_SECONDS,
            "dns_worst_case_per_run": self.dns_worst_case_per_run,
            "dns_ceiling_per_run": DNS_LOOKUPS_PER_RUN_CEILING,
            "within_dns_ceiling": self.within_dns_ceiling,
            "probed_more_often_than_asked": list(self.too_often_for),
            "polite": self.polite,
            # ⛔ RULES 1.4: the per-day figures are what this run WOULD cost at
            # the stated cadence, not a count of runs that happened. A reader
            # who takes `probes_per_day` for an observation has read a
            # projection as a measurement, which is the whole class of error
            # this project exists to avoid.
            "note": (
                f"per-day figures assume one run every "
                f"{int(self.seconds_between_runs)}s and are a projection, not "
                f"an observation. `probed_more_often_than_asked` is empty for "
                f"records that carry no stated interval, which is different "
                f"from every tracker being content."),
        }


def _instant(text: str | None) -> datetime.datetime | None:
    """One ISO 8601 UTC instant, or `None` where the text is not one.

    ⚠ `fromisoformat` did not accept a trailing `Z` before Python 3.11, and
    every instant this project writes ends in one. The supported floor is 3.11
    (RULES 12) so it would parse either way; the substitution stays because it
    is what makes that independent of the interpreter rather than a version
    floor nobody re-checks.
    """
    if not isinstance(text, str) or not text.strip():
        return None
    raw = text.strip()
    if raw.endswith("Z"):
        raw = raw[:-1] + "+00:00"
    try:
        moment = datetime.datetime.fromisoformat(raw)
    except ValueError:
        return None
    if moment.tzinfo is None:
        moment = moment.replace(tzinfo=datetime.timezone.utc)
    return moment


def _is_literal(host: str) -> bool:
    """Whether this host is an address rather than a name.

    ⛔ A literal costs no DNS. `bep34.Resolver.consult` returns ALLOW for one
    without asking anybody, and `getaddrinfo` resolves it without a query, so
    counting it into a lookup budget reports load nobody generates.
    """
    try:
        ipaddress.ip_address(host.strip("[]"))
        return True
    except ValueError:
        return False


def stated_interval(record: Mapping[str, Any]) -> int | None:
    """What this tracker asked for, in seconds, or `None` if it asked nothing.

    ⛔ **The maximum of what was stated, never the first key found.** A tracker
    sending `interval 1800` and `min interval 2700` is asking for both, and the
    only reading that honours both is the longer one.

    Reads the health record, which is what a scheduler has. A record from
    before the interval was carried simply has neither key, and `None` is the
    honest answer rather than the default silently taking its place -- the
    caller decides that, in `probes_per_day`.
    """
    stated = [value for key in ("min_interval", "interval")
              if isinstance(value := record.get(key), int) and value > 0]
    return max(stated) if stated else None


def seconds_between(earlier: str | None, later: str | None) -> float | None:
    """How long separates two ISO 8601 instants, or `None` if either is
    unreadable.

    ⛔ **Unreadable is `None`, never zero and never the epoch.** Both of those
    are answers -- one says "no time has passed" and the other says "an age
    has" -- and the caller's decision turns on which. A parser that guesses
    hands a probe a permission nobody granted.
    """
    a, b = _instant(earlier), _instant(later)
    if a is None or b is None:
        return None
    return (b - a).total_seconds()


def too_soon_after(last_seen: str | None, now: str, *,
                   interval_seconds: int = DEFAULT_INTERVAL_SECONDS) -> bool:
    """Whether contacting this tracker again at `now` would breach D7.

    ⛔ **THIS IS THE CEILING ITSELF, NOT AN ESTIMATE OF IT.** Everything else
    in this module projects what a schedule *would* cost; this reads what was
    actually recorded and answers the only question RULES 4 asks before a
    socket opens. Until it existed the ceiling was a property of the sweep's
    rotation arithmetic, and arithmetic over a wall clock is not a guarantee:
    `rotation_for` maps an instant to the three-hour bucket it falls in, so two
    runs inside one bucket take the identical slice. Measured 2026-09-08 --
    runs `34281244142` (21:33:23Z) and `34289476724` (23:11:21Z) both reported
    `slice 5 of 7`, and **192 trackers were contacted 5878 s apart**, inside
    the 10800 s ceiling, with both observations published to the `data` branch.
    T-087.

    ⛔ **`interval_seconds` defaults to D7's default rather than to each
    tracker's own number, and that is precise rather than lazy.** D7 is "the
    tracker's stated interval, defaulting to three hours where none has been
    observed". None has been observed: `stated_interval` reads two keys that
    `classify_body` has populated since `C-65`, and **0 of 299 committed sweep
    records carry either** -- the two committed sweeps predate `T-026` carrying
    them onto the record at all. A caller holding a record that states one
    passes it; inventing a field the history does not keep would be a ceiling
    derived from nothing.

    Three readings are pessimistic on purpose, because being wrong in one
    direction costs a probe we could have made and in the other costs somebody
    else a request they refused (`docs/conventions/code.md`):

      * a `last_seen` that cannot be read is **too soon**. Corrupt state must
        not buy a probe;
      * a `last_seen` in the future -- clock skew, or a record from a run whose
        injected instant ran ahead -- is **too soon**;
      * `None` is **not** too soon, and that is the one permissive case: a
        tracker with no history has never been contacted, and refusing it would
        mean the sweep could never take a first measurement of anything.

    A `now` that cannot be read **raises**, for the reason `rotation_for`
    raises on one: a run that cannot tell when it is has not been told, and
    guessing here silently disables the ceiling for the whole run.
    """
    if _instant(now) is None:
        raise ValueError(
            f"now={now!r} is not an ISO 8601 instant, so this run cannot tell "
            f"which trackers it is still permitted to contact")
    if last_seen is None:
        return False
    gap = seconds_between(last_seen, now)
    if gap is None:
        return True
    return gap < interval_seconds


def contactable_at(last_seen: Mapping[str, str], now: str, urls: Iterable[str],
                   *, interval_seconds: int = DEFAULT_INTERVAL_SECONDS
                   ) -> tuple[tuple[str, ...], tuple[str, ...]]:
    """Split `urls` into the ones D7 permits contacting at `now`, and the rest.

    Both halves are returned because the second is a count a run owes its
    reader: a sweep that probed nobody because everybody was probed an hour ago
    is the polite outcome, and one that probed nobody because it was pointed at
    an empty corpus is a defect. A caller that only got the permitted half
    could not tell them apart.
    """
    due: list[str] = []
    held: list[str] = []
    for url in urls:
        target = held if too_soon_after(last_seen.get(url), now,
                                        interval_seconds=interval_seconds) \
            else due
        target.append(url)
    return tuple(due), tuple(held)


def probes_per_day(record: Mapping[str, Any],
                   seconds_between_runs: float) -> float:
    """How often this tracker would be probed, against how often it asked.

    Returns probes per day at the given cadence. The interval is the tracker's
    own where it stated one and D7's default where it did not.
    """
    interval = stated_interval(record) or DEFAULT_INTERVAL_SECONDS
    effective = max(float(interval), float(seconds_between_runs))
    return SECONDS_PER_DAY / effective if effective > 0 else 0.0


def schedule_violations(records: Iterable[Mapping[str, Any]],
                        seconds_between_runs: float) -> tuple[str, ...]:
    """Which trackers a schedule at this cadence would probe too often.

    ⛔ The comparison is against each tracker's **own** number, which is the
    whole of D7. A global interval that satisfies the average is a schedule
    that is rude to everybody who asked for more.
    """
    out = []
    for record in records:
        interval = stated_interval(record)
        if interval is not None and seconds_between_runs < interval:
            out.append(str(record.get("url", "-")))
    return tuple(sorted(out))


def run_cost(records: Iterable[Mapping[str, Any]], *,
             seconds_between_runs: float = DEFAULT_INTERVAL_SECONDS,
             hosts: int | None = None) -> RunCost:
    """Compute what a run costs from the records it produced.

    Derived from the run rather than from a corpus figure typed in beside it:
    the number that matters is what was actually probed, and a report quoting a
    corpus size while probing a sample is the mislabelled denominator this
    project has already published once.
    """
    materialised = list(records)
    probes = len(materialised)
    # ⚠ `hostname`, not the authority: `one.example:6969` and `one.example:80`
    # are two endpoints on one host, and the resolver answer is cached per
    # host. Counting the port in would report DNS load nobody generates.
    seen = {host for r in materialised
            if (host := urlsplit(str(r.get("url", ""))).hostname)}
    distinct = hosts if hosts is not None else len(seen)
    resolvable = len({h for h in seen if not _is_literal(h)})
    runs = (SECONDS_PER_DAY / seconds_between_runs
            if seconds_between_runs > 0 else 0.0)
    dns = resolvable * DNS_QUERIES_PER_HOST_WORST_CASE
    return RunCost(
        trackers_probed=probes,
        hosts=distinct,
        resolvable_hosts=resolvable,
        runs_per_day=round(runs, 4),
        probes_per_run=probes,
        probes_per_day=int(round(probes * runs)),
        dns_worst_case_per_run=dns,
        within_dns_ceiling=dns <= DNS_LOOKUPS_PER_RUN_CEILING,
        seconds_between_runs=float(seconds_between_runs),
        too_often_for=schedule_violations(materialised, seconds_between_runs),
    )
