"""The five published categories, each with a rule a consumer can audit. T-046.

⛔ **A hand-curated list presented as derived is a lie about methodology**, and
that sentence is the entry's, not a paraphrase. So every category here states
its rule, and a category with no evidence behind it today publishes an **empty
file with the reason in the report** rather than being quietly filled from
reputation.

THE FIVE, AND WHAT DECIDES MEMBERSHIP

    stable      measured history only: at least MIN_OBSERVATIONS_FOR_STABLE
                observations and a success rate at or above
                STABLE_SUCCESS_RATE. ⛔ Never reputation.
    foss        two halves that stay distinguishable (operator ruling, D9):
                derived from FOSS-ecosystem source provenance, plus a small
                seed labelled as curated. Neither half has content yet.
    hardcoded   the maintainer's manual list. Preserves their order, is not
                sorted, is not ranked, deduplicates against itself.
    common      merged and deduplicated from the other categories plus the
                trackers that measured live, which is the justified evidence
                this project actually has.
    anime       provenance from a source the registry classifies `anime`.

⭐ **Two of the five are derivable today and three are honestly empty**, and
which is which is a property of the evidence rather than of the effort spent:
the registry carries `category` on every source, and nothing in it is `foss`.

THE BOOTSTRAP PROBLEM IS VISIBLE ON PURPOSE

⛔ On day one there is no history, so `stable.txt` is empty. The entry's
decision: **an empty `stable.txt` is honest and a reputation-seeded one
pretending to be measured is not.** `reason_for` is what makes the emptiness
legible instead of looking like a bug.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable, Mapping, Sequence

from .model import HealthState, Tracker
from .state import TrackerHistory

__all__ = ["CATEGORIES", "MIN_OBSERVATIONS_FOR_STABLE", "STABLE_SUCCESS_RATE",
           "Selection", "select_for", "select_all"]

#: How many observations before a tracker may be called stable. ⚠ **A judgement
#: recorded as one, not a measurement.** It is deliberately above
#: `MIN_SAMPLES_FOR_DEATH`, which is 3: the evidence needed to recommend a
#: tracker should exceed the evidence needed to stop claiming it is alive.
MIN_OBSERVATIONS_FOR_STABLE = 5

#: And how reliable over those observations. Also a judgement. 0.95 admits one
#: failure in twenty, which is a tracker having a bad hour rather than a bad
#: month.
STABLE_SUCCESS_RATE = 0.95


@dataclass(frozen=True, slots=True)
class Selection:
    """The members of one category, and why they are the members.

    ⛔ **`reason` is not decoration.** A category that comes out empty has to
    say whether that is because the rule found nothing or because the evidence
    it needs does not exist yet, and those are different states with different
    consequences -- the same distinction as `FAILED` against `EMPTY` in
    acquisition (RULES 3.2).
    """

    category: str
    trackers: tuple[Tracker, ...]
    rule: str
    reason: str
    #: True when the rule is satisfiable today and simply matched nothing.
    #: False when the evidence the rule needs is absent from the tree.
    evidence_available: bool = True

    @property
    def count(self) -> int:
        return len(self.trackers)


def _histories(histories: Mapping[str, TrackerHistory] | None
               ) -> Mapping[str, TrackerHistory]:
    return histories or {}


def _live(tracker: Tracker, histories: Mapping[str, TrackerHistory]) -> bool:
    history = histories.get(tracker.url)
    if history is None or not history.ring:
        return False
    return history.ring[-1].state == HealthState.LIVE.value


def _stable(trackers: Sequence[Tracker],
            histories: Mapping[str, TrackerHistory]) -> Selection:
    rule = (f"at least {MIN_OBSERVATIONS_FOR_STABLE} observations and a "
            f"success rate of {STABLE_SUCCESS_RATE} or better, measured")
    members = []
    for tracker in trackers:
        history = histories.get(tracker.url)
        if history is None or history.lifetime_checks < MIN_OBSERVATIONS_FOR_STABLE:
            continue
        rate = history.lifetime_successes / history.lifetime_checks
        if rate >= STABLE_SUCCESS_RATE:
            members.append(tracker)
    deepest = max((h.lifetime_checks for h in histories.values()), default=0)
    if not members:
        reason = (f"no tracker has {MIN_OBSERVATIONS_FOR_STABLE} observations "
                  f"yet; the deepest history is {deepest}. An empty file is "
                  f"the honest answer on day one and a reputation-seeded one "
                  f"would be a lie about methodology")
    else:
        reason = f"{len(members)} qualified; deepest history is {deepest}"
    return Selection("stable", tuple(members), rule, reason,
                     evidence_available=deepest >= MIN_OBSERVATIONS_FOR_STABLE)


def _by_source_category(trackers: Sequence[Tracker],
                        provenance: Mapping[str, Sequence[str]],
                        source_categories: Mapping[str, str],
                        wanted: str) -> list[Tracker]:
    ids = {sid for sid, category in source_categories.items()
           if category == wanted}
    if not ids:
        return []
    return [t for t in trackers
            if ids.intersection(provenance.get(t.url, ()))]


def _foss(trackers: Sequence[Tracker],
          provenance: Mapping[str, Sequence[str]],
          source_categories: Mapping[str, str],
          seed: Sequence[Tracker]) -> Selection:
    rule = ("provenance from a source the registry classifies `foss`, plus a "
            "seed labelled as curated rather than measured (D9)")
    derived = _by_source_category(trackers, provenance, source_categories, "foss")
    has_source = any(c == "foss" for c in source_categories.values())
    members = list(derived) + [t for t in seed if t not in derived]
    if members:
        reason = (f"{len(derived)} derived from FOSS provenance, "
                  f"{len(members) - len(derived)} from the curated seed")
    elif not has_source:
        reason = ("no source in the registry is classified `foss` and the "
                  "curated seed is empty. The seed stays empty until the "
                  "operator supplies entries: a guessed one is the "
                  "methodology lie D9 exists to prevent")
    else:
        reason = "a FOSS source is registered and contributed nothing"
    return Selection("foss", tuple(members), rule, reason,
                     evidence_available=has_source or bool(seed))


def _hardcoded(hardcoded: Sequence[Tracker]) -> Selection:
    rule = ("the maintainer's manual list, in their order, deduplicated "
            "against itself, never sorted and never ranked")
    seen: set[str] = set()
    members = []
    for tracker in hardcoded:
        if tracker.url in seen:
            continue
        seen.add(tracker.url)
        members.append(tracker)
    reason = (f"{len(members)} entries in the maintainer's order"
              if members else
              "no input file exists yet (T-106); the renderer that preserves "
              "manual order is implemented and tested")
    return Selection("hardcoded", tuple(members), rule, reason,
                     evidence_available=bool(hardcoded))


def _anime(trackers: Sequence[Tracker],
           provenance: Mapping[str, Sequence[str]],
           source_categories: Mapping[str, str]) -> Selection:
    rule = "provenance from a source the registry classifies `anime`"
    members = _by_source_category(trackers, provenance, source_categories,
                                  "anime")
    sources = sorted(sid for sid, c in source_categories.items() if c == "anime")
    reason = (f"{len(members)} contributed by {', '.join(sources)}"
              if members else
              "no source in the registry is classified `anime`")
    return Selection("anime", tuple(members), rule, reason,
                     evidence_available=bool(sources))


def _common(parts: Iterable[Selection], trackers: Sequence[Tracker],
            histories: Mapping[str, TrackerHistory]) -> Selection:
    rule = ("the other categories merged and deduplicated, plus every tracker "
            "whose most recent observation was `live`")
    members: list[Tracker] = []
    seen: set[str] = set()
    for part in parts:
        for tracker in part.trackers:
            if tracker.url not in seen:
                seen.add(tracker.url)
                members.append(tracker)
    live = [t for t in trackers if _live(t, histories) and t.url not in seen]
    members.extend(live)
    ordered = sorted(members, key=Tracker.sort_key)
    reason = (f"{len(ordered) - len(live)} from the other categories and "
              f"{len(live)} more that measured live")
    return Selection("common", tuple(ordered), rule, reason,
                     evidence_available=bool(ordered) or bool(trackers))


CATEGORIES: tuple[str, ...] = ("anime", "common", "foss", "hardcoded", "stable")


def select_all(trackers: Sequence[Tracker], *,
               provenance: Mapping[str, Sequence[str]],
               source_categories: Mapping[str, str],
               histories: Mapping[str, TrackerHistory] | None = None,
               hardcoded: Sequence[Tracker] = (),
               foss_seed: Sequence[Tracker] = ()) -> dict[str, Selection]:
    """Every category, keyed by name.

    ⚠ `common` is computed last because it merges the others, and it is the one
    place the order of evaluation matters.
    """
    hist = _histories(histories)
    stable = _stable(trackers, hist)
    foss = _foss(trackers, provenance, source_categories, foss_seed)
    hard = _hardcoded(hardcoded)
    anime = _anime(trackers, provenance, source_categories)
    common = _common((stable, foss, hard), trackers, hist)
    return {"anime": anime, "common": common, "foss": foss,
            "hardcoded": hard, "stable": stable}


def select_for(category: str, trackers: Sequence[Tracker], **kwargs) -> Selection:
    """One category. A convenience over `select_all`, which computes them all."""
    if category not in CATEGORIES:
        raise KeyError(f"unknown category {category!r}; known: {CATEGORIES}")
    return select_all(trackers, **kwargs)[category]
