"""Which failures a human has to be told about, and how not to spam them. T-080.

⛔ **Nothing currently surfaces an exception to a person**, so every failure
mode this project designs against would fail silently -- which is the failure
mode it exists to avoid. The report names sustained failures and nobody reads a
report.

WHY THE DECISION IS A PURE FUNCTION

`plan` takes the conditions a run observed and the issues that already exist,
and returns what to open, update and close. It touches no network, so the three
properties that matter are **unit-testable rather than hoped for**:

    a repeated condition produces one issue and not many
    an issue closes when its condition genuinely clears
    no body exceeds MAX_BODY_BYTES

⭐ **Deduplication is by key, and the key is in the issue body.** A title can be
edited by a person and a label can be removed; the marker line cannot be
mistaken for prose. Matching on the title is how automations end up filing a
second issue because somebody clarified the first one's wording.

⛔ **An automation that cries wolf hourly gets muted, and then it is worse than
no automation.** Two rules follow, and both are in `plan`: a condition already
open is **updated, never re-filed**, and a condition that has cleared is closed
rather than left for somebody to tidy.

⚠ **`C-44`: workflow artefacts expire after 90 days**, so an issue citing one
will eventually cite nothing. Evidence is summarised **into** the body, and a
run reference is a supplement rather than the record.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Iterable, Mapping, Sequence

from .freshness import parse_instant

__all__ = ["MAX_BODY_BYTES", "MARKER_PREFIX",
           "UNREACHABLE_HOURS_BEFORE_ISSUE", "Condition", "ExistingIssue",
           "Plan", "marker_for", "key_of", "clamp", "plan",
           "source_conditions", "tracker_conditions", "staleness_condition"]

#: A cap on what goes in an issue. ⛔ **Never paste an enormous upstream
#: response**: the body is a summary a maintainer reads on a phone, and the
#: large evidence belongs in a workflow artefact that the body **summarises**
#: rather than defers to (`C-44`).
MAX_BODY_BYTES = 8000

#: The line that identifies an automated issue's condition. Deliberately ugly
#: and deliberately not the title.
MARKER_PREFIX = "<!-- trackers-condition: "


@dataclass(frozen=True, slots=True)
class Condition:
    """One thing a human should know about, with the evidence it owes.

    `key` is the identity. Two runs observing the same problem produce the same
    key, which is what makes deduplication possible at all.
    """

    key: str
    title: str
    #: `(label, value)` pairs, rendered as a list. Ordered, because the body is
    #: part of a diff a person reads (RULES 3.6 applied to prose).
    evidence: tuple[tuple[str, str], ...] = ()
    labels: tuple[str, ...] = ("automated",)
    #: What the reader should do, in one line. An issue with no action is a
    #: notification, and notifications get muted.
    action: str = ""

    def body(self, *, run_reference: str = "") -> str:
        lines = [marker_for(self.key), "", f"**{self.title}**", ""]
        for label, value in self.evidence:
            lines.append(f"- **{label}**: {value}")
        if self.action:
            lines += ["", f"**What to do**: {self.action}"]
        if run_reference:
            lines += ["", f"Run: {run_reference}",
                      "",
                      "⚠ A workflow artefact expires after 90 days, so the "
                      "evidence above is summarised here rather than left in "
                      "one."]
        return clamp("\n".join(lines) + "\n")


@dataclass(frozen=True, slots=True)
class ExistingIssue:
    """An issue already on the repository, as much of it as `plan` needs."""

    number: int
    body: str
    state: str = "open"

    @property
    def key(self) -> str | None:
        return key_of(self.body)


@dataclass
class Plan:
    """What to do about this run's conditions. Every list is ordered.

    ⛔ **Returned rather than executed**, so the decision can be tested and
    reviewed without a token in the room, and so `--dry-run` is the same code
    path as the real thing rather than a second one.
    """

    open_new: list[Condition] = field(default_factory=list)
    update: list[tuple[int, Condition]] = field(default_factory=list)
    close: list[int] = field(default_factory=list)

    @property
    def is_empty(self) -> bool:
        return not (self.open_new or self.update or self.close)

    def as_dict(self) -> dict[str, object]:
        return {"open": [c.key for c in self.open_new],
                "update": [{"number": n, "key": c.key} for n, c in self.update],
                "close": list(self.close)}


def marker_for(key: str) -> str:
    return f"{MARKER_PREFIX}{key} -->"


def key_of(body: str) -> str | None:
    """The condition key an issue body carries, or `None` for a human's issue.

    ⛔ **A body with no marker is somebody's own issue and is never touched.**
    An automation that closed a person's issue because the title looked
    familiar has done more damage than the condition it was reporting.
    """
    for line in body.splitlines():
        if line.startswith(MARKER_PREFIX) and line.rstrip().endswith("-->"):
            return line[len(MARKER_PREFIX):].rstrip()[:-3].strip()
    return None


def clamp(body: str, limit: int = MAX_BODY_BYTES) -> str:
    """Cut a body to the cap, and **say** that it was cut.

    ⚠ Truncating silently would leave a maintainer reading half the evidence
    with no way to know there was more, which is worse than a shorter summary.
    """
    raw = body.encode("utf-8")
    if len(raw) <= limit:
        return body
    notice = "\n\n[truncated: the evidence exceeded the issue body cap]\n"
    room = limit - len(notice.encode("utf-8"))
    return raw[:room].decode("utf-8", "ignore") + notice


def plan(conditions: Iterable[Condition],
         existing: Iterable[ExistingIssue]) -> Plan:
    """Decide what to open, update and close. Pure.

    ⭐ **A condition that persists is an update, never a second issue**, and a
    condition that has cleared is closed rather than left behind. Those two
    lines are the whole anti-spam design.
    """
    wanted = {c.key: c for c in conditions}
    # ⚠ Sorted so two runs over one input produce the same plan (RULES 3.6).
    open_issues: dict[str, ExistingIssue] = {}
    # ⛔ **Lowest number wins**, which is the oldest issue and therefore the one
    # people are subscribed to. Iteration order would have picked whichever the
    # API happened to return first, and a comment claiming otherwise was how
    # this was found: the code took first-seen and said lowest.
    for issue in sorted((i for i in existing
                         if i.state == "open" and i.key is not None),
                        key=lambda i: i.number):
        open_issues.setdefault(issue.key, issue)

    result = Plan()
    for key in sorted(wanted):
        condition = wanted[key]
        found = open_issues.get(key)
        if found is None:
            result.open_new.append(condition)
        else:
            result.update.append((found.number, condition))
    for key in sorted(open_issues):
        if key not in wanted:
            result.close.append(open_issues[key].number)
    return result


# --- the conditions this project can observe today ---------------------------

def _hours_between(first: str | None, last: str | None) -> float | None:
    """Elapsed hours between two ISO 8601 instants, or `None` if unreadable.

    ⚠ `None` rather than 0 on an unreadable stamp: zero would read as "no time
    has passed", which would suppress the issue rather than admit it could not
    be decided.
    """
    if not first or not last:
        return None
    try:
        start = parse_instant(first)
        end = parse_instant(last)
    except ValueError:
        return None
    return (end - start).total_seconds() / 3600.0



def source_conditions(agg, *, observed_at: str) -> list[Condition]:
    """A source that failed, was rejected, or returned nothing.

    The evidence a source issue owes is listed in T-080: id, timestamp, and
    what state it ended in. ⚠ The URL is included and the **response body is
    not** -- a garbage response is exactly the case where pasting it would
    breach the cap.
    """
    out = []
    for source_id in sorted(agg.sources_failed):
        out.append(Condition(
            key=f"source-failed:{source_id}",
            title=f"Source {source_id} failed to fetch",
            evidence=(("source", source_id), ("observed at", observed_at),
                      ("outcome", "failed: the fetch did not complete"),
                      ("effect", "it contributed nothing and blocked nothing; "
                                 "the last accepted data for it stands")),
            labels=("automated", "source"),
            action="check whether the upstream moved or is down. A failed "
                   "source is not an empty one, so the dataset is unaffected."))
    for source_id in sorted(agg.sources_rejected):
        out.append(Condition(
            key=f"source-rejected:{source_id}",
            title=f"Source {source_id} was fetched and refused",
            evidence=(("source", source_id), ("observed at", observed_at),
                      ("outcome", "rejected: it fetched and failed validation")),
            labels=("automated", "source"),
            action="read the rejection reason in the run report. This is the "
                   "shape a format change upstream takes."))
    for source_id in sorted(agg.sources_empty):
        out.append(Condition(
            key=f"source-empty:{source_id}",
            title=f"Source {source_id} returned no trackers",
            evidence=(("source", source_id), ("observed at", observed_at),
                      ("outcome", "empty: it answered and had nothing")),
            labels=("automated", "source"),
            action="an empty source is information and is suspicious on its "
                   "own. Confirm upstream still publishes a list."))
    return out


#: How long a watched tracker must have been failing before a human is told.
#: T-047 sets it at 48 hours, and the number matters: three failed observations
#: at a three-hour cadence is **nine** hours, which is a bad afternoon rather
#: than a tracker that has gone.
UNREACHABLE_HOURS_BEFORE_ISSUE = 48


def tracker_conditions(histories: Mapping[str, object], *,
                       watched: Sequence[str], vantage: Mapping[str, object],
                       min_observations: int = 3,
                       hours: int = UNREACHABLE_HOURS_BEFORE_ISSUE
                       ) -> list[Condition]:
    """A watched tracker unreachable for more than `hours`. T-047.

    ⛔ **Both conditions, not either.** Enough observations *and* enough elapsed
    time: a burst of failures inside one hour is an outage in progress, and a
    single failure 48 hours ago with nothing since is not evidence of anything.
    Requiring both is what makes the issue mean "this has been gone for two
    days" rather than "something went wrong recently".

    ⛔ **`watched` is the maintainer's own list, not the corpus.** Filing an
    issue for every tracker that stops answering would be a thousand issues and
    is the spam this entry forbids; the maintainer's hardcoded entries are the
    ones they asked to be told about.

    ⚠ **The vantage travels with it**, because the whole point is letting a
    maintainer tell "dead" from "dead from CI".
    """
    out = []
    for url in sorted(set(watched)):
        history = histories.get(url)
        if history is None:
            continue
        checks = getattr(history, "lifetime_checks", 0)
        successes = getattr(history, "lifetime_successes", 0)
        if checks < min_observations or successes:
            continue
        ring = getattr(history, "ring", ())
        last = ring[-1] if ring else None
        span = _hours_between(getattr(history, "first_seen", None),
                              getattr(history, "last_seen", None))
        if span is None or span < hours:
            continue
        out.append(Condition(
            key=f"tracker-sustained:{url}",
            title=f"Hardcoded tracker has failed every check: {url}",
            evidence=(
                ("tracker", url),
                ("first failure", getattr(history, "first_seen", "-")),
                ("last failure", getattr(history, "last_failure", "-") or "-"),
                ("observations", f"{checks}, none successful"),
                ("unreachable for", f"{span:.0f} hours"),
                ("last rung reached", getattr(last, "rung", "-") or "-"),
                ("last failure class", getattr(last, "failure", "-") or "-"),
                ("vantage", ", ".join(f"{k}={v}" for k, v in
                                      sorted(vantage.items()))),
            ),
            labels=("automated", "tracker"),
            action="⛔ Read the vantage before concluding the tracker is gone: "
                   "every measurement here comes from one datacenter, and a "
                   "tracker that refuses us may answer everybody else."))
    return out


def staleness_condition(freshness, *, generated_at: str,
                        run_reference: str = "") -> list[Condition]:
    """The dataset has stopped being updated (T-002).

    ⚠ Raised from a `Freshness` verdict rather than recomputed here, so the
    threshold lives in one place.
    """
    if not getattr(freshness, "stale", False):
        return []
    return [Condition(
        key="dataset-stale",
        title="The published dataset has stopped being updated",
        evidence=(("generated at", generated_at),
                  ("age", f"{getattr(freshness, 'age_seconds', 0):.0f}s"),
                  ("detail", getattr(freshness, "detail", "-"))),
        labels=("automated", "publication"),
        action="⛔ A public repository's scheduled workflows are disabled after "
               "60 days without activity (C-12). Check whether the schedule is "
               "still enabled before looking anywhere else.")]
