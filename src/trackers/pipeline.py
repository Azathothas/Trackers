"""Deterministic aggregation, and the plaintext renderer.

RULES 3.6 restates determinism achievably:

    output = f(accepted_source_snapshots, prior_state_file, configuration,
               code_version, scoring_version, injected_clock)

"Anything else influencing output is a defect." Two consequences are enforced
here rather than hoped for:

  * **The clock is injected.** Nothing in this module calls `datetime.now()`.
    A generated-at timestamp read ambiently would make two runs over identical
    inputs differ, which is exactly what the P1 gate forbids.
  * **Every ordering is total and explicit.** Sorting is by
    `Tracker.sort_key`, never by insertion order, never by a set's iteration
    order, and never by hash -- Python's string hashing is randomised per
    process, so a set-ordered output would differ between runs on the same
    machine.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from .acquire import FetchResult, Outcome
from .dedup import DedupDecision, deduplicate
from .exclusion import Exclusion, parse_blacklist
from .model import HealthState, Tracker
from .registry import PUBLISHABLE_ROLES, Role, Source


@dataclass
class Aggregate:
    """The accepted dataset plus everything needed to explain it."""

    trackers: list[Tracker] = field(default_factory=list)
    provenance: dict[str, list[str]] = field(default_factory=dict)
    decisions: list[DedupDecision] = field(default_factory=list)
    rejected: list[tuple[str, str, str]] = field(default_factory=list)

    sources_ok: list[str] = field(default_factory=list)
    sources_failed: list[str] = field(default_factory=list)
    sources_rejected: list[str] = field(default_factory=list)
    sources_empty: list[str] = field(default_factory=list)

    #: URL -> sources that offered it, for entries removed by an enforced
    #: exclusion. Kept so a disappearance is always explainable (T-066).
    excluded: dict[str, list[str]] = field(default_factory=dict)

    @property
    def any_source_failed(self) -> bool:
        return bool(self.sources_failed or self.sources_rejected)


def aggregate(results: list[FetchResult],
              sources: dict[str, Source],
              exclude: set[str] | None = None) -> Aggregate:
    """Combine source results into one accepted dataset.

    A `FAILED` or `REJECTED` source contributes **nothing and blocks nothing**
    (RULES 3.10: one failing source must not fail the others; and a broken
    source must never corrupt canonical data). Its identity is recorded so a
    report and the issue automation can act on it.
    """
    agg = Aggregate()
    all_trackers: list[Tracker] = []
    provenance: dict[str, set[str]] = {}

    # Sort by source id so the order sources are merged in cannot affect
    # anything downstream (scoring invariant I6).
    for res in sorted(results, key=lambda r: r.source_id):
        src = sources.get(res.source_id)
        if src is None:
            continue

        if res.outcome is Outcome.FAILED:
            agg.sources_failed.append(res.source_id)
            continue
        if res.outcome is Outcome.REJECTED:
            agg.sources_rejected.append(res.source_id)
            continue
        if res.outcome is Outcome.EMPTY:
            # Distinct from FAILED and recorded separately. The source told us
            # it has nothing; that is information, and it is suspicious.
            agg.sources_empty.append(res.source_id)
            continue
        if not res.usable:
            agg.sources_failed.append(res.source_id)
            continue

        agg.sources_ok.append(res.source_id)
        for raw, reason in res.rejected:
            agg.rejected.append((res.source_id, raw, reason))

        # A blacklist's entries are trackers an upstream deliberately REMOVED.
        # Counting them as available would invert their meaning.
        if src.role not in PUBLISHABLE_ROLES:
            continue

        for t in res.trackers or ():
            if exclude and t.url in exclude:
                # Operator request or safety. Recorded, not silently dropped.
                agg.excluded.setdefault(t.url, []).append(res.source_id)
                continue
            all_trackers.append(t)
            provenance.setdefault(t.url, set()).add(res.source_id)

    dedup = deduplicate(all_trackers)
    agg.trackers = dedup.trackers
    agg.decisions = dedup.decisions
    agg.provenance = {url: sorted(ids) for url, ids in sorted(provenance.items())}
    return agg


def collect_exclusions(bodies: dict[str, str]) -> list[Exclusion]:
    """Parse every BLACKLIST source body, keeping each reason.

    Takes raw bodies rather than `FetchResult`s because the ordinary parser
    strips the trailing ` # reason`, and for a blacklist that comment is the
    entire signal (see `exclusion.py`).
    """
    out: list[Exclusion] = []
    for source_id, body in sorted(bodies.items()):
        out.extend(parse_blacklist(body, source_id))
    return out


def enforced_exclusions(exclusions: list[Exclusion]) -> set[str]:
    """URLs this project actually removes: operator requests and safety only.

    An upstream's measurement opinions are NOT enforced. HISTORY/reference-sweep.md warns
    that consuming an upstream's output inherits its filtering decisions, and
    RULES 3.4 calls disagreement between observers the most informative thing this
    dataset can publish. Deleting an entry because somebody else measured it
    unfavourably destroys exactly that.
    """
    return {e.url for e in exclusions if e.excluded}


def flagged_exclusions(exclusions: list[Exclusion]) -> dict[str, list[str]]:
    """URL -> the opinions held about it, kept in the dataset and published."""
    out: dict[str, list[str]] = {}
    for e in exclusions:
        if e.excluded:
            continue
        out.setdefault(e.url, []).append(f"{e.source_id}: {e.reason or '-'}")
    return {k: sorted(v) for k, v in sorted(out.items())}


def render_plaintext(trackers: list[Tracker], *, preserve_order: bool = False) -> str:
    """The compatibility-critical format. T-001.

    Deliberately boring, and every choice here is a decision not to be clever:

    * **One URL per line, `\\n`, trailing newline.** No blank-line separation,
      even though newTrackon uses it (measured: `/api/live` returned 156 lines,
      78 non-blank and 78 blank). Single-`\\n` is the strict subset every
      observed consumer handles.
    * **No comments.** `C-41` is unverified. One real client's parser was read
      (`torrent_miscellaneous.pas:174`) and it tolerates them -- it truncates at
      the first space and then rejects a bare `#` as an invalid URL -- but one
      client is not "clients", and its tolerance is partly incidental. Until a
      client survey exists, the conservative format is the correct one.
    * **No ranking numbers, no prose, no metadata.** They belong in JSON.

    **MUST NOT optimize plaintext for human readability at the cost of consumer
    compatibility.** This is the one format where being boring is the feature.

    `preserve_order` exists for `hardcoded.txt`, which T-046 and RULES 3.6
    require to keep the maintainer's manual order and not be sorted.
    """
    ordered = trackers if preserve_order else sorted(trackers, key=Tracker.sort_key)
    seen: set[str] = set()
    lines: list[str] = []
    for t in ordered:
        # Self-deduplicate while preserving order -- required for hardcoded.txt
        # (T-046: "deduplicate against itself; preserve manual order").
        if t.url in seen:
            continue
        seen.add(t.url)
        lines.append(t.url)
    return "".join(f"{u}\n" for u in lines)


def render_report(agg: Aggregate, *, generated_at: str, code_version: str) -> str:
    """A human-readable run report. T-066.

    `generated_at` is injected, never read from the clock here, so that the
    determinism test can hold it fixed and diff everything else byte for byte.
    """
    from collections import Counter

    transports = Counter(t.transport.value for t in agg.trackers)
    networks = Counter(t.network.value for t in agg.trackers)
    unmeasurable = [t for t in agg.trackers if not t.is_measurable_here]

    lines = [
        "# Run report",
        "",
        f"generated_at: {generated_at}",
        f"code_version: {code_version}",
        "",
        "## Sources",
        "",
        f"- ok:       {len(agg.sources_ok)} {sorted(agg.sources_ok)}",
        f"- failed:   {len(agg.sources_failed)} {sorted(agg.sources_failed)}",
        f"- rejected: {len(agg.sources_rejected)} {sorted(agg.sources_rejected)}",
        f"- empty:    {len(agg.sources_empty)} {sorted(agg.sources_empty)}",
        "",
        "`failed` and `empty` are different states and are counted separately.",
        "A failed source contributed nothing and blocked nothing; the previous",
        "accepted data for it stands (RULES 3.10).",
        "",
        "## Dataset",
        "",
        f"- accepted trackers: {len(agg.trackers)}",
        f"- rejected lines:    {len(agg.rejected)}",
        f"- dedup decisions:   {len(agg.decisions)} "
        f"({sum(1 for d in agg.decisions if d.acted)} removed)",
        "",
        "### Transport",
        "",
    ]
    lines += [f"- {k}: {v}" for k, v in sorted(transports.items())]
    lines += ["", "### Network", ""]
    lines += [f"- {k}: {v}" for k, v in sorted(networks.items())]
    lines += [
        "",
        "### Measurability",
        "",
        f"- measurable from this vantage: {len(agg.trackers) - len(unmeasurable)}",
        f"- unmeasurable:                 {len(unmeasurable)}",
        "",
        "An unmeasurable tracker is one this vantage cannot reach at all",
        "(no IPv6 egress; i2p/yggdrasil/onion need routers; ws/wss unverified).",
        "It is never reported dead -- that would measure the probe, not the",
        "tracker (RULES 3.1 requirement 1).",
        "",
    ]
    return "\n".join(lines)
