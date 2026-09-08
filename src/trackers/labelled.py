"""The labelled dataset: JSON and CSV, where the health data actually fits.

T-060, and T-061 with it, because two formats that can disagree are worth less
than one that cannot.

⛔ **THIS IS THE PRODUCT, AND THE PLAINTEXT IS NOT.** The value gate answered
that this dataset is justified **as a labelled dataset and not as a list**:
taken unfiltered the plaintext is 13.4x longer than the baseline and five times
less likely to answer, and filtering it by what measured live is what yields
2.1x-3.7x as many working trackers. So a consumer who takes `trackers_all.txt`
alone gets the worse product, and these two files are what make the project
worth existing (`HISTORY/gates.md`).

WHAT DECIDES WHETHER A FIELD IS HERE

`docs/schema.md` defines every field, and `tests/test_labelled.py` diffs the
emitted set against that page in both directions. T-060's rule: **a field MUST
NOT be included because it sounds useful.** A field nobody can define is a
field consumers will misread.

Four candidate fields from the entry are deliberately **absent**, and the
schema says so rather than leaving a reader to wonder:

    latency, latency statistics   the history ring keeps the outcome and the
                                  rung, not the round-trip time. Emitting a
                                  column this project does not retain would be
                                  a number nobody measured.
    reliability score, score      no scoring model is chosen (T-044, D4 open),
    version                       deliberately: four observations is not enough
                                  to fit one without fitting it to noise.
    confidence                    a confidence over an unchosen model is an
                                  adjective with a decimal point.

TWO AXES, TWO COLUMNS

`transport` and `network` are separate and always will be. Collapsing them into
one `protocol` column re-imports the defect the two-axis model exists to
prevent: `.i2p` is a hostname suffix and `udp` is a transport, and a tracker at
`udp://tracker.i2p/announce` is both.

AN UNPROBED TRACKER IS `unknown`, AND ITS RATE IS A DASH

⛔ Never 0. A tracker nobody has checked and a tracker that failed every check
are different facts, and `success_rate: 0.0` says the second about the first.
`None` renders as `null` in JSON and as an empty field in CSV.
"""

from __future__ import annotations

import csv
import io
import json
from typing import Any, Iterable, Mapping

from .model import HealthState, Tracker
from .state import TrackerHistory

__all__ = ["FIELDS", "CSV_FIELDS", "row_for", "render_json", "render_csv"]

#: Every field emitted in JSON, in the order the schema defines them. The tuple
#: is the contract: `tests/test_labelled.py` diffs it against `docs/schema.md`
#: in both directions, so a field added here without a definition fails, and a
#: definition with no field fails too.
FIELDS: tuple[str, ...] = (
    "url",
    "transport",
    "network",
    "host",
    "port",
    "sources",
    "health_state",
    "measurement_rung",
    "failure",
    "checks",
    "successes",
    "success_rate",
    "first_seen",
    "last_seen",
    "last_success",
    "last_failure",
    "observed_from",
    "unmeasurable_reason",
)

#: CSV carries the same fields with the two list-valued ones flattened, because
#: a CSV cell holding JSON is a format nobody can open in the tool they chose
#: CSV for. `sources` joins on `;` -- never `,`, which is the delimiter.
CSV_FIELDS: tuple[str, ...] = FIELDS


def _health_of(history: TrackerHistory | None,
               tracker: Tracker) -> tuple[str, str | None, str | None]:
    """`(health_state, measurement_rung, failure)` for one tracker.

    ⛔ **Derived from the history, never guessed.** A tracker with no history
    has never been observed from anywhere, which is `unknown` -- not `dead`,
    and not `live` because it came from a list somebody maintains.

    ⚠ A tracker this vantage cannot measure at all is `unmeasurable` even with
    no observation, because that is a structural fact about our position rather
    than a missing measurement (RULES 3.1).
    """
    if history is None or not history.ring:
        if not tracker.is_measurable_here:
            return HealthState.UNMEASURABLE.value, None, "unsupported"
        return HealthState.UNKNOWN.value, None, None
    last = history.ring[-1]
    return last.state, last.rung, last.failure


def row_for(tracker: Tracker, *, sources: Iterable[str] = (),
            history: TrackerHistory | None = None,
            observed_from: str | None = None) -> dict[str, Any]:
    """One tracker as the schema defines it.

    `observed_from` is the environment class the last observation came from. It
    is a single string rather than the whole vantage block because a per-row
    copy of the probe version and the IP families is the same value repeated a
    thousand times; the run's full vantage travels in the health record the
    sweep writes. ⚠ `None` where nothing has been observed, never `-`: the
    dash is for a human-readable report, and a consumer parsing JSON should get
    `null`.
    """
    state, rung, failure = _health_of(history, tracker)
    return {
        "url": tracker.url,
        "transport": tracker.transport.value,
        "network": tracker.network.value,
        "host": tracker.host,
        "port": tracker.port,
        "sources": sorted(sources),
        "health_state": state,
        "measurement_rung": rung,
        "failure": failure,
        "checks": history.lifetime_checks if history else 0,
        "successes": history.lifetime_successes if history else 0,
        # ⛔ `None`, never 0.0, for a tracker nobody has checked.
        "success_rate": (None if history is None or history.ewma is None
                         else round(history.ewma, 6)),
        "first_seen": history.first_seen if history else None,
        "last_seen": history.last_seen if history else None,
        "last_success": history.last_success if history else None,
        "last_failure": history.last_failure if history else None,
        "observed_from": observed_from if history and history.ring else None,
        "unmeasurable_reason": (tracker.unmeasurable_reason
                                if not tracker.is_measurable_here else None),
    }


def _rows(trackers: list[Tracker], provenance: Mapping[str, list[str]],
          histories: Mapping[str, TrackerHistory],
          observed_from: str | None) -> list[dict[str, Any]]:
    """Sorted by the one ordering this project has (RULES 3.6)."""
    return [row_for(t, sources=provenance.get(t.url, ()),
                    history=histories.get(t.url), observed_from=observed_from)
            for t in sorted(trackers, key=Tracker.sort_key)]


def render_json(trackers: list[Tracker], *, provenance: Mapping[str, list[str]],
                histories: Mapping[str, TrackerHistory] | None = None,
                generated_at: str, code_version: str,
                observed_from: str | None = None) -> str:
    """The labelled dataset as JSON.

    `generated_at` is injected (RULES 3.6). `sort_keys` and a fixed indent so
    two runs over one input are byte-identical, which `gate.yml` asserts.
    """
    document = {
        "generated_at": generated_at,
        "code_version": code_version,
        "count": len(trackers),
        "fields": list(FIELDS),
        # ⚠ Stated in the data rather than only in the documentation, because
        # the consumer most likely to misread this file will never open a page.
        "vantage_note": (
            "Health is measured from one cloud provider's address space and "
            "from one authoring host, never from a residential connection. A "
            "tracker recorded unknown may answer you."),
        "trackers": _rows(trackers, provenance, histories or {}, observed_from),
    }
    return json.dumps(document, indent=2, sort_keys=True) + "\n"


def render_csv(trackers: list[Tracker], *, provenance: Mapping[str, list[str]],
               histories: Mapping[str, TrackerHistory] | None = None,
               observed_from: str | None = None) -> str:
    """The same rows as CSV.

    ⚠ `lineterminator` is set explicitly. `csv` defaults to `\\r\\n`, so the
    same instrument would otherwise emit different bytes than the JSON writer
    beside it and a committed output could not be diffed (RULES 15.5).
    """
    buffer = io.StringIO()
    writer = csv.DictWriter(buffer, fieldnames=list(CSV_FIELDS),
                            lineterminator="\n", extrasaction="raise")
    writer.writeheader()
    for row in _rows(trackers, provenance, histories or {}, observed_from):
        flat = dict(row)
        flat["sources"] = ";".join(row["sources"])
        # An absent value is an empty cell, which is what every CSV reader
        # treats as missing. "None" would be a four-letter string.
        writer.writerow({k: ("" if v is None else v) for k, v in flat.items()})
    return buffer.getvalue()
