"""Per-source quality, measured rather than asserted. T-101, and T-100's
fields are what it reads.

⛔ **`Trust` is assigned by hand.** Somebody read each upstream and wrote
`HIGH`, `MEDIUM` or `LOW` into the registry, which satisfies *"MUST NOT treat
all sources equally"* and measures nothing. This module computes the dimensions
that **can** be measured and puts them beside the assertion, so a reader can
see where the two disagree.

⛔ **IT REPORTS AND IT DOES NOT DEMOTE.** A source whose measurements look poor
is not automatically downgraded, because T-101's `Decision` says both findings
that matter run the other way:

    no unique contribution     may still be CORROBORATION, and two sources
                               agreeing is evidence while one source is a
                               single point of failure. `ngosang_all` at 2
                               unique of 99 is kept for exactly that.
    many unique, poor quality  wants lower trust or stricter filtering, NOT
                               removal -- uniqueness is what an aggregator
                               exists to capture. `desirefire_all` is that
                               case.

An automation that acted on either reading would delete the thing the project
is for. So this returns numbers and disagreements, and a human decides.

⭐ **WHAT MAKES IT REPEATABLE IS T-103.** "Source quality is not a one-time
judgement" is the entry's own approach, and until the per-source history
existed there was nothing to recompute against: freshness, failure rate and
format stability are all properties of a series. They come from
`provenance.SourceHistory` now, so this report is regenerated from the `data`
branch on every publish rather than being re-derived by hand.

⚠ **Every dimension says how many observations it rests on.** A failure rate
over one fetch is not a failure rate, and RULES 1.4 forbids reporting it as
though it were.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Mapping, Sequence

__all__ = ["SourceQuality", "assess", "assess_all"]


@dataclass(frozen=True, slots=True)
class SourceQuality:
    """What is measured about one source, beside what was asserted about it."""

    source_id: str
    #: The hand-assigned value, carried so the comparison is visible.
    asserted_trust: str
    role: str

    #: Measured: how many of this source's URLs no other source carries.
    contributed: int
    unique: int
    #: Measured: how many of its URLs another source also carries. ⭐ The
    #: number T-101's `Decision` turns on -- a source with no unique entries
    #: may be the corroboration that makes another source's claim evidence.
    corroborating: int

    #: Measured from the history. `None` where there is none yet.
    observations: int = 0
    failure_rate: float | None = None
    latest_entries: int | None = None
    entries_min: int | None = None
    entries_max: int | None = None
    #: Distinct content digests over the retained observations: how often the
    #: body actually changes. 1 over many observations is a static list.
    distinct_bodies: int | None = None
    last_seen: str | None = None

    #: Static, from the registry (T-100). Reported so the table is a complete
    #: description of how a source is handled rather than half of one.
    expected_format: str = ""
    parser: str = ""

    @property
    def unique_share(self) -> float | None:
        return None if not self.contributed else self.unique / self.contributed

    def disagreements(self) -> tuple[str, ...]:
        """Where the measurement and the assertion do not sit comfortably.

        ⛔ Phrased as questions for a person, never as verdicts. Each one has a
        legitimate answer that keeps the source, and acting on any of them
        automatically is what the `Decision` forbids.
        """
        out: list[str] = []
        if self.observations >= 5 and (self.failure_rate or 0) > 0.2:
            out.append(
                f"{self.asserted_trust} trust with a {self.failure_rate:.0%} "
                f"failure rate over {self.observations} fetches: is the trust "
                f"still right, or is the source having a bad week?")
        if self.contributed and self.unique == 0:
            out.append(
                f"contributes {self.contributed} URLs and none uniquely: keep "
                f"it only if it corroborates ({self.corroborating} shared), "
                f"and say so rather than dropping it")
        if self.observations >= 5 and self.distinct_bodies == 1:
            out.append(
                f"the body has not changed across {self.observations} "
                f"fetches: is this list still maintained?")
        return tuple(out)

    def as_record(self) -> dict[str, Any]:
        return {
            "source_id": self.source_id,
            "asserted_trust": self.asserted_trust,
            "role": self.role,
            "contributed": self.contributed,
            "unique": self.unique,
            "corroborating": self.corroborating,
            "unique_share": (None if self.unique_share is None
                             else round(self.unique_share, 4)),
            "observations": self.observations,
            "failure_rate": self.failure_rate,
            "latest_entries": self.latest_entries,
            "entries_min": self.entries_min,
            "entries_max": self.entries_max,
            "distinct_bodies": self.distinct_bodies,
            "last_seen": self.last_seen,
            "expected_format": self.expected_format,
            "parser": self.parser,
            "questions_for_a_human": list(self.disagreements()),
        }


def assess(source: Any, *, provenance: Mapping[str, Sequence[str]],
           history: Any | None = None) -> SourceQuality:
    """Measure one source. `provenance` maps a URL to the sources carrying it.

    ⚠ **Every history-derived field is `None` without a history**, never a
    default. A source nobody has a record of has an unknown failure rate, and
    an unknown rendered as `0.0` would read as perfect (RULES 1.5).
    """
    contributed = unique = corroborating = 0
    for _url, sources in provenance.items():
        if source.id not in sources:
            continue
        contributed += 1
        if len(set(sources)) == 1:
            unique += 1
        else:
            corroborating += 1

    observations = failure_rate = latest = low = high = bodies = seen = None
    if history is not None:
        counts = history.entries_seen()
        observations = history.lifetime_fetches
        failure_rate = (round(history.lifetime_failures / observations, 4)
                        if observations else None)
        latest = counts[-1] if counts else None
        low = min(counts) if counts else None
        high = max(counts) if counts else None
        bodies = len({o.digest for o in history.ring if o.digest}) or None
        seen = history.last_seen

    return SourceQuality(
        source_id=source.id,
        asserted_trust=getattr(source.trust, "value", str(source.trust)),
        role=getattr(source.role, "value", str(source.role)),
        contributed=contributed, unique=unique, corroborating=corroborating,
        observations=observations or 0, failure_rate=failure_rate,
        latest_entries=latest, entries_min=low, entries_max=high,
        distinct_bodies=bodies, last_seen=seen,
        expected_format=source.expected_format, parser=source.parser)


def assess_all(sources: Sequence[Any], *,
               provenance: Mapping[str, Sequence[str]],
               histories: Mapping[str, Any] | None = None
               ) -> tuple[SourceQuality, ...]:
    """Every source, in registry order so two runs render identically."""
    histories = histories or {}
    return tuple(assess(source, provenance=provenance,
                        history=histories.get(source.id))
                 for source in sources)
