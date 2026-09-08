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

DNS IS INSIDE THE BUDGET

Operator ruling, 2026-09-08: **100,000 lookups per run on a GitHub runner**,
local runs unbounded. A sweep spends them in three places -- BEP 34 consent per
host, `getaddrinfo` per probe, and the second opinion T-037 asks for whenever
the first fails -- and none of them was counted before this module existed.
"""

from __future__ import annotations

import ipaddress
from dataclasses import dataclass
from typing import Any, Iterable, Mapping
from urllib.parse import urlsplit

__all__ = [
    "DEFAULT_INTERVAL_SECONDS", "DNS_LOOKUPS_PER_RUN_CEILING",
    "SECONDS_PER_DAY", "RunCost", "stated_interval", "probes_per_day",
    "run_cost", "schedule_violations",
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
