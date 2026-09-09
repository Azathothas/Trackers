"""Per-source history, and the answer to "why did this tracker disappear".

T-103. `FetchResult` has carried a `content_sha256` since acquisition existed
and thrown it away at the end of every run, so nothing could answer, later:
what did this source return, when, and what changed.

⛔ **THE QUESTION THIS MODULE EXISTS FOR HAS A WRONG ANSWER THAT LOOKS RIGHT.**
A tracker vanishing from the output has two completely different causes:

    the sources stopped listing it        it is gone, and the dataset is right
    a source failed to fetch              it is NOT gone; we could not see it

RULES 3.2 is the rule that keeps those apart at acquisition time --
`FetchResult.trackers` is `None` and never `[]` for a failure -- and both
pieces of prior art here get it wrong, in two languages. `explain_absence`
is that same distinction extended through **time**, which is the only place a
consumer can ask it from: by the time somebody notices a tracker is missing,
the run that lost it is over.

⭐ **HASHES AND COUNTS, NEVER BODIES.** T-103's `Decision` forbids retaining
unlimited raw upstream data in git. A digest answers "did it change", a count
answers "by how much", and neither grows with the source. The bodies stay
where they already are: the snapshot cache, which is not history.

⛔ **BOUNDED BY CONSTRUCTION, AND THE NUMBERS ARE MEASURED.** The ring and the
daily cap are `experiments/38-source-history-size.py`'s output, not taste --
the same discipline `state.py` follows for D3, and for the same reason: a file
that grows without a bound is one somebody eventually deletes, taking the
history with it.

⛔ **THE CLOCK IS INJECTED.** Nothing here calls `datetime.now()`. Every
instant arrives as an argument (RULES 3.6).
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field, replace
from typing import Any, Iterable, Mapping, Sequence

__all__ = [
    "PROVENANCE_FORMAT", "RING_SIZE", "DAILY_DAYS", "CorruptProvenance",
    "SourceObservation", "SourceHistory", "Absence", "render_line",
    "parse_line", "write_history", "read_history", "observe_fetch",
    "explain_absence",
]

#: First on the header line. A positional format with no version mis-reads
#: silently when its shape changes -- `forbidden-patterns.md`, first row.
PROVENANCE_FORMAT = "trackers.provenance/1"

#: ⭐ Both from `experiments/38-source-history-size.py`. At D7's cadence the
#: publisher fetches every source **eight times a day**, so 240 observations is
#: a month of them, and 8 sources at that depth is a file measured in hundreds
#: of kilobytes rather than megabytes.
RING_SIZE = 240
DAILY_DAYS = 365


class CorruptProvenance(Exception):
    """Raised when a history file is not what this reader thinks it is.

    ⛔ Never recovered from by reinitialising: RULES 3.9 makes rebuilding from
    nothing data loss wearing the costume of a fix.
    """


@dataclass(frozen=True, slots=True)
class SourceObservation:
    """One fetch of one source, reduced to what answers a later question."""

    at: str
    #: The `Outcome` name as a string. ⛔ `FAILED` and `EMPTY` are different
    #: rows here for the same reason they are different states there.
    outcome: str
    #: `None` for a failure. ⛔ Never `0`: a source that could not be fetched
    #: did not return zero trackers, and writing it as zero here would
    #: reintroduce the conflation this whole module is about.
    entries: int | None = None
    digest: str | None = None
    http_status: int | None = None
    bytes_fetched: int | None = None

    #: ⛔ **Outcomes after which we KNOW what the source lists.** `EMPTY`
    #: belongs here and its absence was a bug in this module's first draft:
    #: `acquire.Outcome` says in as many words that `EMPTY` means *"the source
    #: successfully told us it has nothing"*, which is information, while
    #: `FAILED` means we do not know. Leaving `EMPTY` out made an empty source
    #: read as a blind spot -- the RULES 3.2 conflation, committed inside the
    #: module written to prevent it, and caught by
    #: `test_an_empty_source_is_a_removal_and_a_failed_one_is_not`.
    #:
    #: ⚠ `REJECTED` is deliberately **not** here. The body arrived and we
    #: refused it, so we hold no listing we are willing to act on, and a
    #: tracker absent because its only source was rejected has not been shown
    #: to be gone.
    KNOWS_THE_LISTING = ("OK", "EMPTY", "UNCHANGED")

    @property
    def ok(self) -> bool:
        """Whether this fetch left us knowing what the source lists."""
        return self.outcome in SourceObservation.KNOWS_THE_LISTING

    def as_json(self) -> list[Any]:
        """A list, not an object. The ring is the bulk of the file and a key
        per field would triple it for no reader's benefit."""
        return [self.at, self.outcome, self.entries, self.digest,
                self.http_status, self.bytes_fetched]

    @staticmethod
    def from_json(raw: Any) -> "SourceObservation":
        if not isinstance(raw, list) or len(raw) != 6:
            raise CorruptProvenance(f"an observation is six fields, got {raw!r}")
        return SourceObservation(at=str(raw[0]), outcome=str(raw[1]),
                                 entries=raw[2], digest=raw[3],
                                 http_status=raw[4], bytes_fetched=raw[5])


@dataclass(frozen=True, slots=True)
class SourceHistory:
    """Everything remembered about one source. Frozen; every change returns a
    new value, so a caller cannot corrupt one halfway through a run."""

    source_id: str
    url: str
    first_seen: str
    last_seen: str
    lifetime_fetches: int = 0
    lifetime_failures: int = 0
    ring: tuple[SourceObservation, ...] = field(default_factory=tuple)
    #: `(day, fetches, failures)`, so a month-long degradation is answerable
    #: after the ring has rolled past it.
    daily: tuple[tuple[str, int, int], ...] = field(default_factory=tuple)

    @staticmethod
    def new(source_id: str, url: str, first_seen: str) -> "SourceHistory":
        return SourceHistory(source_id=source_id, url=url,
                             first_seen=first_seen, last_seen=first_seen)

    def observe(self, observation: SourceObservation) -> "SourceHistory":
        """Record one fetch. ⛔ An observation already held at this instant is
        ignored, which is what makes folding one run twice harmless -- the same
        guard `state.py` carries, for the same reason."""
        if any(o.at == observation.at for o in self.ring):
            return self
        ring = (self.ring + (observation,))[-RING_SIZE:]
        day = observation.at[:10]
        daily = list(self.daily)
        failed = 0 if observation.ok else 1
        if daily and daily[-1][0] == day:
            _, fetches, failures = daily[-1]
            daily[-1] = (day, fetches + 1, failures + failed)
        else:
            daily.append((day, 1, failed))
            daily.sort(key=lambda d: d[0])
        daily = daily[-DAILY_DAYS:]
        return replace(
            self,
            url=self.url,
            last_seen=max(self.last_seen, observation.at),
            lifetime_fetches=self.lifetime_fetches + 1,
            lifetime_failures=self.lifetime_failures + failed,
            ring=ring,
            daily=tuple(daily),
        )

    def latest(self) -> SourceObservation | None:
        return self.ring[-1] if self.ring else None

    def entries_seen(self) -> tuple[int, ...]:
        """Every non-`None` entry count in the ring, oldest first.

        ⭐ **This is what T-102's bands are waiting for.** A threshold derived
        from one observation stays wide because one observation cannot justify
        a narrow band; this is where the distribution comes from.
        """
        return tuple(o.entries for o in self.ring if o.entries is not None)

    def as_json(self) -> dict[str, Any]:
        return {
            "source_id": self.source_id,
            "url": self.url,
            "first_seen": self.first_seen,
            "last_seen": self.last_seen,
            "lifetime_fetches": self.lifetime_fetches,
            "lifetime_failures": self.lifetime_failures,
            "ring": [o.as_json() for o in self.ring],
            "daily": [list(d) for d in self.daily],
        }

    @staticmethod
    def from_json(raw: Any) -> "SourceHistory":
        if not isinstance(raw, dict):
            raise CorruptProvenance(f"a record is an object, got {type(raw)}")
        for key in ("source_id", "url", "first_seen", "last_seen"):
            if not isinstance(raw.get(key), str):
                raise CorruptProvenance(f"record is missing {key!r}")
        return SourceHistory(
            source_id=raw["source_id"], url=raw["url"],
            first_seen=raw["first_seen"], last_seen=raw["last_seen"],
            lifetime_fetches=int(raw.get("lifetime_fetches", 0)),
            lifetime_failures=int(raw.get("lifetime_failures", 0)),
            ring=tuple(SourceObservation.from_json(o)
                       for o in raw.get("ring", [])),
            daily=tuple((str(d[0]), int(d[1]), int(d[2]))
                        for d in raw.get("daily", [])))


def render_line(history: SourceHistory) -> str:
    """One record, as one line. Sorted keys and an explicit newline: two runs
    over identical input produce identical bytes (RULES 3.6, RULES 15.5)."""
    return json.dumps(history.as_json(), sort_keys=True,
                      separators=(",", ":")) + "\n"


def parse_line(line: str) -> SourceHistory:
    try:
        return SourceHistory.from_json(json.loads(line))
    except ValueError as exc:
        raise CorruptProvenance(f"line is not JSON: {exc}") from exc


def write_history(path: str, histories: Mapping[str, SourceHistory], *,
                  generated_at: str) -> None:
    """Header first, then one line per source, sorted by id."""
    header = json.dumps({
        "format": PROVENANCE_FORMAT,
        "generated_at": generated_at,
        "records": len(histories),
        "ring_size": RING_SIZE,
        "daily_days": DAILY_DAYS,
    }, sort_keys=True, separators=(",", ":")) + "\n"
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(header)
        for source_id in sorted(histories):
            handle.write(render_line(histories[source_id]))


def read_history(path: str) -> tuple[dict[str, SourceHistory], list[str]]:
    """`(by_source_id, quarantined)`.

    ⛔ A bad **header** raises; a bad **line** is quarantined and returned. One
    damaged record must not cost the others, and neither is ever silently
    reinitialised.
    """
    out: dict[str, SourceHistory] = {}
    quarantined: list[str] = []
    with open(path, encoding="utf-8") as handle:
        first = handle.readline()
        if not first:
            raise CorruptProvenance(f"{path} is empty; a history has a header")
        try:
            header = json.loads(first)
        except ValueError as exc:
            raise CorruptProvenance(f"{path}: header is not JSON: {exc}") from exc
        if not isinstance(header, dict) or \
                header.get("format") != PROVENANCE_FORMAT:
            raise CorruptProvenance(
                f"{path}: format is {header.get('format')!r}, this reader "
                f"speaks {PROVENANCE_FORMAT!r}")
        for line in handle:
            if not line.strip():
                continue
            try:
                record = parse_line(line)
            except CorruptProvenance:
                quarantined.append(line.rstrip("\n"))
                continue
            out[record.source_id] = record
    return out, quarantined


def observe_fetch(histories: Mapping[str, SourceHistory],
                  results: Iterable[Any], *,
                  at: str) -> dict[str, SourceHistory]:
    """Fold one run's `FetchResult`s into the history. Returns a new dict.

    ⛔ **`entries` is `None` for a failure and never `0`.** `FetchResult`
    already draws that line by making `trackers` `None`; carrying it through is
    what lets `explain_absence` tell "the source dropped it" from "we could not
    read the source".

    ⛔ **`at` is the run's INJECTED instant and every observation in the run
    carries it**, rather than each result's own `fetched_at`. Two reasons, and
    the first is a rule: `fetched_at` is read from an ambient clock, so a
    history stamped with it makes two runs over identical inputs produce
    different bytes, which RULES 3.6 makes a correctness property. The second
    is that it is what gives `observe` something to be idempotent **about** --
    re-folding one run must add nothing, and it cannot recognise a repeat whose
    every timestamp is new. `state.py` stamps a sweep's records the same way,
    for the same two reasons.

    ⚠ The cost is the sub-run ordering: which source was fetched at which
    second within the run is not retained. Nothing asks that question, and the
    alternative costs determinism.
    """
    out = dict(histories)
    for result in results:
        source_id = getattr(result, "source_id", None)
        if not source_id:
            continue
        trackers = getattr(result, "trackers", None)
        outcome = getattr(getattr(result, "outcome", None), "name", "")
        observation = SourceObservation(
            at=at,
            outcome=outcome or "UNKNOWN",
            entries=None if trackers is None else len(trackers),
            digest=getattr(result, "content_sha256", None),
            http_status=getattr(result, "http_status", None),
            bytes_fetched=getattr(result, "byte_count", None),
        )
        current = out.get(source_id) or SourceHistory.new(
            source_id, getattr(result, "url", ""), first_seen=at)
        out[source_id] = current.observe(observation)
    return out


@dataclass(frozen=True, slots=True)
class Absence:
    """Why a tracker is not in the output, in a form a consumer can act on."""

    url: str
    #: The sources that used to carry it, and when each last did.
    last_carried_by: tuple[tuple[str, str], ...]
    #: Sources that carried it and whose most recent fetch FAILED.
    failing_sources: tuple[str, ...]
    #: True when every source that ever carried it fetched cleanly and none of
    #: them lists it now.
    genuinely_removed: bool
    detail: str

    def as_record(self) -> dict[str, Any]:
        return {
            "url": self.url,
            "last_carried_by": [list(p) for p in self.last_carried_by],
            "failing_sources": list(self.failing_sources),
            "genuinely_removed": self.genuinely_removed,
            "detail": self.detail,
        }


def explain_absence(url: str, *,
                    carried_by: Mapping[str, Sequence[str]],
                    histories: Mapping[str, SourceHistory],
                    present_now: bool) -> Absence:
    """Why is `url` not in the output any more?

    `carried_by` maps a source id to the instants at which that source was
    observed carrying this URL, oldest first -- which is what the per-tracker
    provenance records.

    ⛔ **The two causes are never merged.** A source whose most recent fetch
    failed is one whose listing we cannot see, so a tracker it used to carry is
    **not** shown as removed. RULES 3.2, applied to time: "the source did not
    list it" and "we could not read the source" are different sentences, and
    only the first justifies telling a consumer the tracker is gone.
    """
    last_carried = tuple(sorted(
        (source_id, max(instants))
        for source_id, instants in carried_by.items() if instants))
    failing = []
    for source_id, _ in last_carried:
        history = histories.get(source_id)
        latest = history.latest() if history else None
        if latest is not None and not latest.ok:
            failing.append(source_id)
    failing_sources = tuple(sorted(failing))

    if present_now:
        detail = "it is in the output; nothing to explain"
        removed = False
    elif not last_carried:
        detail = ("no source has ever been recorded carrying it, so its "
                  "absence is not a disappearance")
        removed = False
    elif failing_sources:
        detail = (f"{len(failing_sources)} of {len(last_carried)} source(s) "
                  f"that carried it failed their most recent fetch "
                  f"({', '.join(failing_sources)}), so this is OUR blind spot "
                  f"and not a removal (RULES 3.2)")
        removed = False
    else:
        names = ", ".join(f"{sid} last on {when}" for sid, when in last_carried)
        detail = (f"every source that carried it fetched cleanly and none "
                  f"lists it now: {names}")
        removed = True
    return Absence(url=url, last_carried_by=last_carried,
                   failing_sources=failing_sources,
                   genuinely_removed=removed, detail=detail)
