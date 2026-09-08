"""Per-tracker history: what has happened to each tracker, over time.

T-040, and decision **D3**. Scoring needs history and none was stored, so
nothing could tell a new tracker from one that has failed twice from one that
has been degrading for a month. Those are the distinctions T-041 requires, and
none of them is expressible in a last-result field.

⛔ **HISTORY LIVES IN FILES, NEVER IN GIT HISTORY.** RULES 3.7, and it is not
a preference: the data branch is reset by design, so anything inferred from
commits is destroyed by routine housekeeping. Nothing in this module reads a
commit, a timestamp from git, or a file's mtime.

WHAT IS STORED, AND WHY EACH FIELD EARNS ITS PLACE

    ewma            a success rate that weights recent checks more, so a
                    tracker that recovered is not held down by last month
    ring            the last K outcomes WITH their timestamps -- the shapes in
                    T-041 are patterns over time and a count cannot hold one
    daily           D days of aggregates, so "degrading for a month" is
                    answerable after the ring has rolled past it
    lifetime        totals that never roll, so a long-dead tracker still says
                    how long it worked
    first_seen      distinguishes a NEW tracker from a failing one, which is
                    the first of T-041's seven shapes and the easiest to lose
    last_success    how long since it last worked, which "consistently
                    unreliable" and "apparently gone" are told apart by

⭐ **K AND D COME FROM ARITHMETIC, NOT FROM TASTE.** T-040 says so and
`experiments/31-state-size-projection.py` is the arithmetic: it builds a full
record, measures its serialised length, and projects the file over five years
against D7's cadence. The values below are that experiment's output, and
changing one without re-running it is how a bounded file stops being bounded.

THE FORMAT IS JSON LINES, AND THE SHAPE IS THE POINT

One object per line, sorted by URL, with a version header. Four properties
come out of that and each was a requirement rather than a convenience:

  * **deterministic bytes.** Sorted lines, sorted keys, explicit `\\n`. Two runs
    over identical inputs produce identical files, which RULES 3.6 makes a
    correctness property rather than a nicety.
  * **a diff that means something.** A changed tracker is one changed line, so
    a reviewer can see what a run actually did.
  * **per-line corruption.** A damaged line loses one tracker, not the file.
  * **streaming.** Nothing has to hold every tracker in memory to read or
    write, so the five-year projection is not also a memory projection.

⛔ **A CORRUPT STATE FILE IS NEVER SILENTLY REINITIALISED.** `gates.md`'s
definition of done requires bootstrap-from-nothing to succeed *and* corrupt
state to fail safely without reinitialising, because RULES 3.9 says recovering
by deleting valid data is data loss wearing the costume of a fix. Loading a
file whose header is wrong or missing raises; a single unreadable line is
reported and quarantined rather than dropped.

⛔ **THE CLOCK IS INJECTED.** Nothing here calls `datetime.now()`. Every
timestamp arrives as an argument, because RULES 3.6 makes determinism a
property CI asserts on every push.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field, replace
from typing import Any, Iterable, Iterator

__all__ = [
    "STATE_FORMAT", "RING_SIZE", "DAILY_DAYS", "EWMA_ALPHA",
    "CorruptState", "Outcome", "DayAggregate", "TrackerHistory",
    "render_line", "parse_line", "write_state", "read_state",
    "bootstrap", "apply_sweep",
]

#: The version, and it is first on the header line. A positional or implicit
#: format with no version mis-reads silently when its shape changes, which is
#: the first row of `docs/conventions/forbidden-patterns.md`.
STATE_FORMAT = "trackers.state/1"

#: ⭐ **Both numbers are `experiments/31-state-size-projection.py`'s output**,
#: which measures a full record by building one, and projects the file over
#: five years against D7's cadence and a stated growth assumption. The sweep it
#: prints is the argument:
#:
#:     K    D   record   5yr MB   ring covers
#:    32  180    5593     17.6     4.0 days
#:    64  180    7449     23.4     8.0 days   <- chosen
#:    64  365   10964     34.5     8.0 days
#:   128  365   14676     46.2    16.0 days
#:
#: **K = 64** is 8 days at D7's 3 h default -- long enough to see
#: "intermittently failing" inside a week, where 32 gives four days and can
#: miss a tracker that fails once a weekend. **D = 180** is six months, so
#: "degrading over a month" has five months of background behind it; 365
#: buys a second year of background for 11 MB and the shapes in T-041 do not
#: ask for one.
#:
#: ⛔ **Changing either without re-running that experiment is how a bounded
#: file stops being bounded**, and 46 MB is already inside the range where a
#: platform starts refusing files.
RING_SIZE = 64
DAILY_DAYS = 180

#: How much one observation moves the rate. 0.15 gives a half-life of about
#: 4.3 checks, so a tracker that comes back is believed within a day at D7's
#: cadence and one that fails once is not written off.
EWMA_ALPHA = 0.15


class CorruptState(ValueError):
    """The state file is not readable as state.

    ⛔ Raised rather than recovered from. A caller that catches this and starts
    a fresh file has deleted a tracker's entire history to make an error go
    away, which RULES 3.9 names exactly.
    """


@dataclass(frozen=True, slots=True)
class Outcome:
    """One check, as it will be read back.

    `state` is the `HealthState` value as a string rather than the enum: this
    module stores what was recorded, and a state vocabulary that grows must not
    make an old file unreadable.
    """

    at: str          # ISO 8601 UTC, injected
    state: str
    ok: bool
    rung: str
    failure: str | None = None

    def as_json(self) -> list:
        # A list, not an object: 64 of these per tracker, and the key names
        # would be 60% of the bytes. The order is fixed by this method and
        # `from_json` is its only reader.
        return [self.at, self.state, 1 if self.ok else 0, self.rung,
                self.failure]

    @staticmethod
    def from_json(raw: Any) -> "Outcome":
        if not isinstance(raw, list) or len(raw) != 5:
            raise CorruptState(f"an outcome must be 5 fields, got {raw!r}")
        at, state, ok, rung, failure = raw
        if not isinstance(at, str) or not isinstance(state, str):
            raise CorruptState(f"outcome has non-string fields: {raw!r}")
        return Outcome(at=at, state=state, ok=bool(ok), rung=str(rung),
                       failure=failure if failure is None else str(failure))


@dataclass(frozen=True, slots=True)
class DayAggregate:
    """One day, once the ring has rolled past it."""

    day: str         # YYYY-MM-DD
    checks: int
    successes: int

    def as_json(self) -> list:
        return [self.day, self.checks, self.successes]

    @staticmethod
    def from_json(raw: Any) -> "DayAggregate":
        if not isinstance(raw, list) or len(raw) != 3:
            raise CorruptState(f"a day aggregate must be 3 fields, got {raw!r}")
        day, checks, successes = raw
        if not isinstance(day, str):
            raise CorruptState(f"day is not a string: {raw!r}")
        if not isinstance(checks, int) or not isinstance(successes, int):
            raise CorruptState(f"day counts are not integers: {raw!r}")
        if successes > checks or checks < 0 or successes < 0:
            # ⛔ Not repaired. A day claiming more successes than checks is a
            # file that has been edited or damaged, and guessing which number
            # is wrong would publish a rate nobody measured.
            raise CorruptState(f"day {day}: {successes} successes of {checks}")
        return DayAggregate(day=day, checks=checks, successes=successes)


@dataclass(frozen=True, slots=True)
class TrackerHistory:
    """Everything remembered about one tracker.

    Frozen, and every mutation returns a new value. A history that can be
    edited in place is one a caller can corrupt halfway through a run and then
    write out.
    """

    url: str
    first_seen: str
    last_seen: str
    last_success: str | None = None
    last_failure: str | None = None
    ewma: float | None = None
    lifetime_checks: int = 0
    lifetime_successes: int = 0
    ring: tuple[Outcome, ...] = field(default_factory=tuple)
    daily: tuple[DayAggregate, ...] = field(default_factory=tuple)

    @staticmethod
    def new(url: str, first_seen: str) -> "TrackerHistory":
        """A tracker seen for the first time.

        ⭐ **`ewma` is `None`, not 0.0.** A new tracker and a tracker that has
        failed every check are the first and fourth of T-041's seven shapes,
        and starting the rate at zero makes them the same number. RULES 1.5:
        where a value is unknown, write a dash.
        """
        return TrackerHistory(url=url, first_seen=first_seen,
                              last_seen=first_seen)

    def observe(self, *, state: str, ok: bool, observed_at: str, rung: str,
                failure: str | None = None) -> "TrackerHistory":
        """Record one check. Returns a new history; never mutates.

        ⚠ **`observed_at` is the injected clock**, and it is the only source of
        time in this module.
        """
        outcome = Outcome(at=observed_at, state=state, ok=ok, rung=rung,
                          failure=failure)
        ring = (self.ring + (outcome,))[-RING_SIZE:]

        day = observed_at[:10]
        daily = list(self.daily)
        if daily and daily[-1].day == day:
            last = daily[-1]
            daily[-1] = DayAggregate(day, last.checks + 1,
                                     last.successes + (1 if ok else 0))
        else:
            daily.append(DayAggregate(day, 1, 1 if ok else 0))
            # ⚠ Sorted, then capped from the end. A run that replays an older
            # observation must not silently drop the newest day because it
            # arrived out of order.
            daily.sort(key=lambda d: d.day)
        daily = daily[-DAILY_DAYS:]

        rate = 1.0 if ok else 0.0
        ewma = rate if self.ewma is None else (
            EWMA_ALPHA * rate + (1 - EWMA_ALPHA) * self.ewma)

        return replace(
            self,
            last_seen=max(self.last_seen, observed_at),
            last_success=observed_at if ok else self.last_success,
            last_failure=self.last_failure if ok else observed_at,
            ewma=ewma,
            lifetime_checks=self.lifetime_checks + 1,
            lifetime_successes=self.lifetime_successes + (1 if ok else 0),
            ring=ring,
            daily=tuple(daily),
        )

    def as_json(self) -> dict[str, Any]:
        return {
            "url": self.url,
            "first_seen": self.first_seen,
            "last_seen": self.last_seen,
            "last_success": self.last_success,
            "last_failure": self.last_failure,
            # Rounded on the way out so the bytes cannot depend on a platform's
            # float repr. RULES 3.6: two runs, identical bytes.
            "ewma": None if self.ewma is None else round(self.ewma, 6),
            "lifetime_checks": self.lifetime_checks,
            "lifetime_successes": self.lifetime_successes,
            "ring": [o.as_json() for o in self.ring],
            "daily": [d.as_json() for d in self.daily],
        }

    @staticmethod
    def from_json(raw: Any) -> "TrackerHistory":
        if not isinstance(raw, dict):
            raise CorruptState(f"a record must be an object, got {type(raw)}")
        for key in ("url", "first_seen", "last_seen"):
            if not isinstance(raw.get(key), str):
                raise CorruptState(f"record is missing {key!r}")
        ewma = raw.get("ewma")
        if ewma is not None and not isinstance(ewma, (int, float)):
            raise CorruptState(f"ewma is not a number: {ewma!r}")
        checks = raw.get("lifetime_checks", 0)
        successes = raw.get("lifetime_successes", 0)
        if not isinstance(checks, int) or not isinstance(successes, int):
            raise CorruptState("lifetime counters are not integers")
        if successes > checks:
            raise CorruptState(
                f"{raw['url']}: {successes} successes of {checks} checks")
        return TrackerHistory(
            url=raw["url"], first_seen=raw["first_seen"],
            last_seen=raw["last_seen"], last_success=raw.get("last_success"),
            last_failure=raw.get("last_failure"),
            ewma=None if ewma is None else float(ewma),
            lifetime_checks=checks, lifetime_successes=successes,
            ring=tuple(Outcome.from_json(o) for o in raw.get("ring", ())),
            daily=tuple(DayAggregate.from_json(d) for d in raw.get("daily", ())),
        )


def render_line(history: TrackerHistory) -> str:
    """One record as one line. `sort_keys` so the bytes are total."""
    return json.dumps(history.as_json(), sort_keys=True,
                      separators=(",", ":"), ensure_ascii=False)


def parse_line(line: str) -> TrackerHistory:
    try:
        raw = json.loads(line)
    except ValueError as exc:
        raise CorruptState(f"line is not JSON: {exc}") from exc
    return TrackerHistory.from_json(raw)


def write_state(path: str, histories: Iterable[TrackerHistory],
                *, generated_at: str) -> int:
    """Write the whole file, sorted by URL. Returns the number of records.

    ⚠ **Whole-file, not append.** A ring and a set of daily aggregates are
    rewritten by every observation, so an append-only file would be a log this
    module then had to replay -- which is a second format, and the replay is
    where the corruption would live.
    """
    records = sorted(histories, key=lambda h: h.url)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(json.dumps({"format": STATE_FORMAT,
                             "generated_at": generated_at,
                             "records": len(records),
                             "ring_size": RING_SIZE,
                             "daily_days": DAILY_DAYS},
                            sort_keys=True, separators=(",", ":")) + "\n")
        for h in records:
            fh.write(render_line(h) + "\n")
    return len(records)


def read_state(path: str) -> tuple[dict[str, TrackerHistory], list[str]]:
    """Read a state file. Returns `(by_url, quarantined)`.

    ⛔ **A bad HEADER raises**; a bad LINE is quarantined and reported. The two
    are different failures: a wrong header means the whole file is not what
    this reader thinks it is, and continuing would mis-read every record. One
    unreadable line means one tracker's history is damaged, and losing the
    other 1326 to it would be the recovery-by-deletion RULES 3.9 forbids.

    ⛔ **Neither is ever silently reinitialised.** `quarantined` is returned so
    a caller must decide what to do about it; a caller that ignores it has made
    that choice visibly.
    """
    with open(path, encoding="utf-8") as fh:
        first = fh.readline()
        if not first:
            raise CorruptState(f"{path} is empty; a state file has a header")
        try:
            header = json.loads(first)
        except ValueError as exc:
            raise CorruptState(f"{path}: header is not JSON: {exc}") from exc
        if not isinstance(header, dict) or header.get("format") != STATE_FORMAT:
            raise CorruptState(
                f"{path}: format is {header.get('format')!r}, "
                f"this reader speaks {STATE_FORMAT!r}")

        by_url: dict[str, TrackerHistory] = {}
        quarantined: list[str] = []
        for number, line in enumerate(fh, start=2):
            line = line.strip()
            if not line:
                continue
            try:
                h = parse_line(line)
            except CorruptState as exc:
                quarantined.append(f"line {number}: {exc}")
                continue
            if h.url in by_url:
                quarantined.append(f"line {number}: {h.url} appears twice")
                continue
            by_url[h.url] = h
    return by_url, quarantined


def bootstrap(path: str) -> tuple[dict[str, TrackerHistory], list[str]]:
    """Read the state, or start empty if there is none.

    ⭐ **Absent and corrupt are not the same file.** A missing state file is
    the first run and is normal; a damaged one is an incident. This function
    is the only place that treats absence as acceptable, and it does not catch
    `CorruptState`.
    """
    if not os.path.exists(path):
        return {}, []
    return read_state(path)


def apply_sweep(histories: dict[str, TrackerHistory],
                records: Iterable[dict[str, Any]]) -> dict[str, TrackerHistory]:
    """Fold one sweep's health records into the history. Returns a new dict.

    ⭐ **A tracker absent from this sweep keeps its history untouched.** It was
    not checked, which is not the same as having failed, and a sweep that
    sampled 200 of 1327 must not age the other 1127 (RULES 3.2, applied to
    time instead of to sources).
    """
    out = dict(histories)
    for rec in records:
        url = rec["url"]
        at = rec["observed_at"]
        state = str(rec["health_state"])
        current = out.get(url) or TrackerHistory.new(url, first_seen=at)
        out[url] = current.observe(
            state=state,
            # ⛔ `ok` is liveness, not "the probe returned". `unknown` and
            # `unmeasurable` are the two ways of knowing nothing and neither is
            # a success -- counting them as one would let a tracker we cannot
            # measure accumulate a perfect record.
            ok=state == "live",
            observed_at=at,
            rung=str(rec.get("measurement_rung", "none")),
            failure=rec.get("failure"))
    return out

