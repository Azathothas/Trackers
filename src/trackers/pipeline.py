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
from .exclusion import (Exclusion, carries_private_credential,
                        mask_credential, parse_blacklist)
from .model import HealthState, Tracker
from .registry import PUBLISHABLE_ROLES, Role, Source


@dataclass(frozen=True, slots=True)
class Excluded:
    """One entry that was offered and refused, and why.

    RULES 3.10: a rejection is a returned value, never a log line, because a
    tracker that disappears from the output owes the consumer who noticed a
    reason. Before this carried `reason`, the record said only *that* something
    was removed.

    ⚠ `url` is **safe to print**: a private-credential refusal stores the
    masked form, so the audit names the host without repeating the token.
    """

    url: str
    reason: str
    sources: tuple[str, ...] = ()


@dataclass
class Aggregate:
    """The accepted dataset plus everything needed to explain it."""

    trackers: list[Tracker] = field(default_factory=list)
    provenance: dict[str, list[str]] = field(default_factory=dict)
    decisions: list[DedupDecision] = field(default_factory=list)
    rejected: list[tuple[str, str, str]] = field(default_factory=list)

    sources_ok: list[str] = field(default_factory=list)
    #: T-104. Sources that answered 304: the snapshot we hold is current, and
    #: they contributed their trackers from it. ⛔ Counted separately from `ok`
    #: because "we downloaded it" and "the upstream told us not to bother" are
    #: different facts, and only one of them cost the upstream a transfer.
    sources_unchanged: list[str] = field(default_factory=list)
    sources_failed: list[str] = field(default_factory=list)
    sources_rejected: list[str] = field(default_factory=list)
    sources_empty: list[str] = field(default_factory=list)

    #: URL -> the refusal, for entries removed by an enforced exclusion. Kept
    #: so a disappearance is always explainable (T-066). ⚠ Keyed by the **raw**
    #: URL so the count is right; every value's `url` is the safe-to-print
    #: form, and rendering iterates values rather than keys.
    excluded: dict[str, Excluded] = field(default_factory=dict)

    @property
    def any_source_failed(self) -> bool:
        return bool(self.sources_failed or self.sources_rejected)


def _refuse(agg: "Aggregate", key: str, display: str, reason: str,
            source_id: str) -> None:
    """Record one refusal, merging the sources that offered the same URL.

    ⚠ **Keyed by the raw URL, displayed as `display`.** Keying by the masked
    form instead loses count: two different people's passkeys on one host mask
    to the same string, so the refusal total silently under-reported by one --
    seven URLs refused, six rows written. The raw URL never leaves this dict as
    a key; every rendered line reads `Excluded.url`, which is the masked form.

    Sources accumulate in sorted order rather than arrival order, because the
    report is part of the deterministic output (RULES 3.6).
    """
    prior = agg.excluded.get(key)
    sources = tuple(sorted(set(prior.sources if prior else ()) | {source_id}))
    agg.excluded[key] = Excluded(url=display, reason=reason, sources=sources)


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

        if res.outcome is Outcome.UNCHANGED:
            agg.sources_unchanged.append(res.source_id)
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
                _refuse(agg, t.url, t.url,
                        "upstream exclusion: operator request or safety",
                        res.source_id)
                continue
            if carries_private_credential(t.url):
                # T-107. Refused here rather than in the parser, so the
                # decision is auditable (RULES 3.10) instead of a row silently
                # vanishing; and refused rather than redacted, because a URL
                # with its token stripped is a different endpoint and
                # publishing it as the tracker invents one.
                #
                # RULES 6: no private-tracker data. It costs a consumer nothing
                # -- a passkey URL authenticates one person and is unusable by
                # anybody else -- and it is the clearest instance of this
                # project doing something a concatenation cannot.
                _refuse(agg, t.url, mask_credential(t.url),
                        "carries a private-tracker credential (T-107)",
                        res.source_id)
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


#: Every question a run report must answer (T-066). ⛔ The tuple is the
#: contract: `tests/test_report.py` asserts each label appears, so a section
#: cannot silently disappear from the report and take the answer with it.
REPORT_FIELDS: tuple[str, ...] = (
    "sources fetched", "ok:", "failed:", "rejected:", "empty:",
    "accepted trackers", "rejected lines", "duplicates removed",
    "health observations", "health states", "measurement rungs",
    "observation depth", "sustained failures", "stale sources",
    "what this report cannot answer yet",
)


def _health_section(agg: "Aggregate", histories) -> list[str]:
    """What the measurements say, over the trackers this run accepted.

    ⛔ **Derived from the histories, never from a count somebody kept.** A
    report that carried its own tally would be a second place for the numbers
    to be wrong.
    """
    from collections import Counter

    urls = {t.url for t in agg.trackers}
    mine = {u: h for u, h in histories.items() if u in urls}
    states: Counter = Counter()
    rungs: Counter = Counter()
    checks = 0
    sustained = []
    for url, history in sorted(mine.items()):
        checks += history.lifetime_checks
        if history.ring:
            states[history.ring[-1].state] += 1
            rungs[history.ring[-1].rung or "-"] += 1
        else:
            states["unknown"] += 1
        # ⚠ "Sustained" means every observation failed AND there are enough of
        # them to mean something. One failure is a moment (RULES 11).
        if (history.lifetime_checks >= 3 and history.lifetime_successes == 0):
            sustained.append(url)
    never = len(urls) - len(mine)
    depths = sorted(h.lifetime_checks for h in mine.values())
    deepest = depths[-1] if depths else 0
    median = depths[len(depths) // 2] if depths else 0

    lines = [
        "## Health",
        "",
        f"- health observations: {checks} across {len(mine)} tracker(s)",
        f"- never observed:      {never}",
        f"- health states:       {dict(sorted(states.items()))}",
        f"- measurement rungs:   {dict(sorted(rungs.items()))}",
        f"- observation depth:   median {median}, deepest {deepest}",
        f"- sustained failures:  {len(sustained)} "
        f"(3+ observations, none successful)",
        "",
        "⛔ A tracker that did not answer is `unknown`, never `dead`: saying",
        "dead needs 3 observations of one tracker and the state machine is",
        "the only place that decision is made.",
        "",
    ]
    for url in sustained[:20]:
        lines.append(f"- sustained: `{url}`")
    if sustained:
        lines.append("")
    return lines


def _unanswerable_section() -> list[str]:
    """⛔ What this report cannot answer, and why -- rather than a plausible
    number in its place.

    RULES 9.1: a requirement that cannot be met is retained, its limitation is
    stated, and the result is labelled honestly. Three of the questions T-066
    asks are unanswerable today for reasons that are properties of the data
    rather than of the effort spent here.
    """
    return [
        "## What this report cannot answer yet",
        "",
        "- **stale sources**: whether an upstream has stopped being updated.",
        "  Answering it needs each fetch's `Last-Modified` or `ETag` retained",
        "  across runs, and provenance snapshots are not kept yet (T-103). ⚠ A",
        "  source that contributed nothing **this** run is reported above as",
        "  failed or empty, which is a different question and is answered.",
        "- **latency distribution**: the history keeps each observation's",
        "  outcome and rung, not its round-trip time. A distribution here",
        "  would be a number this project does not retain.",
        "- **ranking changes**: nothing is ranked. No scoring model is chosen",
        "  (T-044), deliberately, because the history is too short to fit one",
        "  against without fitting it to noise.",
        "- **reliability distribution**: the same reason. The invariants a",
        "  model must satisfy exist and are tested (T-043); the model does not.",
        "- **whether publication succeeded**: this report is written *before*",
        "  publication, by the step whose output is being published. It cannot",
        "  report on an event that has not happened. The workflow's own summary",
        "  answers it.",
        "",
    ]


def render_report(agg: Aggregate, *, generated_at: str, code_version: str,
                  categories=None, histories=None) -> str:
    """A human-readable run report. T-066.

    `generated_at` is injected, never read from the clock here, so that the
    determinism test can hold it fixed and diff everything else byte for byte.

    ⛔ **`categories` carries each category's rule and why it holds what it
    holds** (T-046). Three of the five are legitimately empty, and an empty
    file with no stated reason looks like a defect -- so the reason is
    published rather than printed to a log nobody keeps.
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
        f"- sources fetched: {len(agg.sources_ok) + len(agg.sources_failed) + len(agg.sources_rejected) + len(agg.sources_empty)}",
        "",
        f"- ok:       {len(agg.sources_ok)} {sorted(agg.sources_ok)}",
        f"- unchanged: {len(agg.sources_unchanged)} {sorted(agg.sources_unchanged)} (304; the held snapshot is current)",
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
        f"- duplicates removed: {len(agg.decisions)} "
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
        "## Refused entries",
        "",
        f"- refused: {len(agg.excluded)}",
        "",
        "Every entry offered by a source and not published, with the reason.",
        "A tracker that vanishes owes the consumer who noticed an explanation",
        "(RULES 3.10), so this is a returned value and not a log line.",
        "",
        "⚠ A URL refused for carrying a private-tracker credential is listed",
        "with the credential removed. The token is what got it refused;",
        "printing it here would republish in the report what the dataset",
        "declined to republish (T-107).",
        "",
        "So two of these lines can read identically: two people's credentials",
        "on one endpoint differ only in the part that is not shown. They are",
        "counted separately above, which is the number that matters.",
        "",
    ]
    # Ordered by what is printed, not by the key. Sorting on the raw URL would
    # make the order of these lines a function of somebody's passkey; ordering
    # on the rendered text keeps the output total, explicit (RULES 3.6) and
    # derived only from what a reader can see.
    for e in sorted(agg.excluded.values(),
                    key=lambda x: (x.url, x.reason, x.sources)):
        lines.append(f"- `{e.url}` -- {e.reason} [{', '.join(e.sources)}]")
    lines.append("")

    if histories is not None:
        lines += _health_section(agg, histories)
    lines += _unanswerable_section()

    if categories:
        lines += [
            "## Categories",
            "",
            "Each file's membership rule, and why it holds what it holds.",
            "⛔ An empty file is not a defect: it says below whether the rule",
            "matched nothing or the evidence it needs does not exist yet.",
            "",
        ]
        for name, selection in sorted(categories.items()):
            marker = "" if selection.evidence_available else " (evidence absent)"
            lines.append(f"### {name}.txt -- {selection.count}{marker}")
            lines.append("")
            lines.append(f"- rule: {selection.rule}")
            lines.append(f"- why:  {selection.reason}")
            lines.append("")
    return "\n".join(lines)
