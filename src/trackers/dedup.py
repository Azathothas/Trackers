"""Deduplication, which is three different questions wearing one name.

the three dedup questions in src/trackers/dedup.py separates them, and conflating them "is how upstreams lose real
trackers":

  1. **Same URL** after safe normalization -- always a duplicate.
  2. **Same host, different scheme or port** -- *not* automatically a duplicate.
     `udp://x:6969` and `http://x:6969` are separate endpoints with separate
     failure modes, and a client benefits from having both.
  3. **Different host, same resolved IP** -- an *inference*, not a fact.
     Resolution is time-varying, and CDN-fronted trackers collapse many
     distinct hosts onto shared addresses.

This module implements (1), reports (2) without acting on it, and **refuses to
perform (3) silently**.

Why (3) is refused by default, with evidence rather than taste. ngosang's
`blacklist.txt` @ `1e61597` shows the scale at which collapsing on resolved IP
gets applied as policy: **~90 of its 346 entries** carry the reason
"duplicate of <url>" (counted, `experiments/19` cache). Some of those are
certainly genuine.

That CDN fronting makes the inference unsafe is observed but **not yet
quantified across the corpus**: of the six HTTP/HTTPS subjects in
`experiments/05`, three resolved into Cloudflare space --
`tracker.renfei.net` -> `104.21.58.176, 172.67.162.102`,
`tracker.leechshield.link` -> `104.21.28.104, 172.67.145.215`,
`1337.abcvg.info` -> `104.21.72.244, 172.67.136.175`. Three distinct trackers,
two shared address prefixes, no relationship between them.

**The corpus-wide rate is unmeasured -- a dash, not an estimate.** Measuring it
needs a resolution pass over every host in the corpus, which is experiment 23
and is not
yet written. The design does not depend on the number: one demonstrated case
of unrelated trackers sharing addresses is already enough to make silent
removal unsafe, and dropping an entry on that basis is unrecoverable for the
consumer and invisible in the output.

So: same-IP candidates are *recorded as evidence with a timestamp* and left in
the dataset. the three dedup questions in src/trackers/dedup.py: "record the resolved addresses and timestamp as
evidence rather than silently dropping entries."
"""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field

from .model import Tracker


@dataclass(frozen=True, slots=True)
class DedupDecision:
    """Why one tracker was removed, kept, or merely flagged.

    Every decision is recorded so a report can explain a disappearance
    (RULES 3.10; T-066 owns the report). A tracker that vanishes with no reason
    recorded is
    indistinguishable from a bug.
    """

    kind: str          # "exact_duplicate" | "sibling_endpoint" | "shared_address"
    kept: str          # the URL that survived
    other: str         # the URL this decision is about
    reason: str
    acted: bool        # True only when something was actually removed


@dataclass
class DedupResult:
    trackers: list[Tracker] = field(default_factory=list)
    decisions: list[DedupDecision] = field(default_factory=list)

    @property
    def removed(self) -> int:
        return sum(1 for d in self.decisions if d.acted)


def deduplicate(trackers: list[Tracker]) -> DedupResult:
    """Remove exact duplicates; flag the other two relationships without acting.

    Deterministic and order-independent (RULES 3.6, scoring invariant I6):
    input is sorted by `Tracker.sort_key` first, so the survivor of a duplicate
    pair does not depend on which one the parser happened to see first.
    """
    ordered = sorted(trackers, key=Tracker.sort_key)

    result = DedupResult()
    seen: dict[str, Tracker] = {}

    # --- question 1: identical after normalization. Always a duplicate. ------
    for t in ordered:
        prior = seen.get(t.url)
        if prior is None:
            seen[t.url] = t
            result.trackers.append(t)
            continue
        result.decisions.append(DedupDecision(
            kind="exact_duplicate",
            kept=prior.url,
            other=t.url,
            reason="byte-identical after normalization",
            acted=True,
        ))

    # --- question 2: same host, different transport or port. NOT duplicates. -
    by_host: dict[str, list[Tracker]] = defaultdict(list)
    for t in result.trackers:
        by_host[t.host].append(t)

    for host, group in sorted(by_host.items()):
        if len(group) < 2:
            continue
        canonical = min(group, key=Tracker.sort_key)
        for t in group:
            if t.url == canonical.url:
                continue
            result.decisions.append(DedupDecision(
                kind="sibling_endpoint",
                kept=canonical.url,
                other=t.url,
                reason=(
                    "same host, different transport or port. KEPT: these are "
                    "distinct endpoints with distinct failure modes "
                    "(the three dedup questions in src/trackers/dedup.py question 2)"
                ),
                acted=False,
            ))

    return result


def note_shared_addresses(
    trackers: list[Tracker],
    resolved: dict[str, list[str]],
    observed_at: str,
) -> list[DedupDecision]:
    """Record which distinct hosts share a resolved address. Removes nothing.

    `resolved` maps host -> addresses, and `observed_at` is the timestamp that
    makes the observation meaningful. Both are required arguments rather than
    optional, because a same-IP claim with no timestamp is exactly the
    time-varying inference the three dedup questions in src/trackers/dedup.py warns about, presented as a fact.

    This function deliberately has **no** removal mode. Collapsing hosts on a
    shared address is a policy decision that needs its own decision-record
    entry and its own evidence; it is not something a dedup pass should be able
    to do as a side effect.
    """
    by_addr: dict[str, list[Tracker]] = defaultdict(list)
    for t in trackers:
        for addr in resolved.get(t.host, ()):
            by_addr[addr].append(t)

    decisions: list[DedupDecision] = []
    for addr, group in sorted(by_addr.items()):
        hosts = {t.host for t in group}
        if len(hosts) < 2:
            continue
        canonical = min(group, key=Tracker.sort_key)
        for t in group:
            if t.url == canonical.url:
                continue
            decisions.append(DedupDecision(
                kind="shared_address",
                kept=canonical.url,
                other=t.url,
                reason=(
                    f"resolved to {addr} at {observed_at}, shared with "
                    f"{len(hosts)} distinct hosts. NOT removed: resolution is "
                    f"time-varying and CDN-fronted trackers legitimately share "
                    f"addresses (the three dedup questions in src/trackers/dedup.py question 3)"
                ),
                acted=False,
            ))
    return decisions
