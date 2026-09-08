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
import hashlib
import io
import json
from typing import Any, Iterable, Mapping

from . import NORMALIZATION_VERSION, SCHEMA_VERSION, SCORING_VERSION
from .freshness import PUBLISH_INTERVAL_SECONDS, STALE_AFTER_INTERVALS
from .model import HealthState, Rung, Tracker
from .probe import Failure, health_state
from .state import TrackerHistory

__all__ = ["FIELDS", "CSV_FIELDS", "row_for", "render_json", "render_csv",
           "digest_of", "metadata_for", "newest_observation"]

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


def digest_of(rows) -> str:
    """A stable sha256 over the rows, as `sha256:<hex>`.

    ⭐ **What lets a consumer tell a re-publication from a change.** Every run
    writes a new `generated_at`, so a reader comparing timestamps sees movement
    on every publish whether or not anything moved. Two documents with one
    digest carry the same data.

    Hashed over the canonical JSON of the rows alone, so the digest does not
    change when the document's own metadata does -- including when it changes
    because the digest was added.
    """
    canonical = json.dumps(rows, sort_keys=True, separators=(",", ":"))
    return "sha256:" + hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def metadata_for(paths_and_bytes, *, generated_at: str, code_version: str,
                 count: int, digest: str,
                 newest_observation_at: str | None = None) -> str:
    """The release's own description, as `metadata.json`.

    ⛔ **This exists because CSV cannot carry document metadata** and T-062
    requires that a consumer of any published artefact can tell what they
    received. A version column repeated on 1334 identical rows is not a header,
    and a comment line breaks the format for the readers people choose CSV for.
    So the versions and a digest **per file** live here, and a CSV consumer
    checks the file they hold against this one. RULES 9: the requirement is not
    dropped, it is met by the strongest form the format allows.
    """
    files = {}
    for name, payload in sorted(paths_and_bytes.items()):
        files[name] = {
            "bytes": len(payload),
            "digest": "sha256:" + hashlib.sha256(payload).hexdigest(),
        }
    return json.dumps({
        "generated_at": generated_at,
        "code_version": code_version,
        "schema_version": SCHEMA_VERSION,
        "normalization_version": NORMALIZATION_VERSION,
        "scoring_version": SCORING_VERSION,
        "count": count,
        "dataset_digest": digest,
        "publish_interval_seconds": PUBLISH_INTERVAL_SECONDS,
        "stale_after_intervals": STALE_AFTER_INTERVALS,
        "newest_observation": newest_observation_at,
        "files": files,
        "schema": "schema.md, published beside this file",
    }, indent=2, sort_keys=True) + "\n"


def _health_of(history: TrackerHistory | None,
               tracker: Tracker) -> tuple[str, str | None, str | None]:
    """`(health_state, measurement_rung, failure)` for one tracker.

    ⛔ **Derived from the history, never guessed.** A tracker with no history
    has never been observed from anywhere, which is `unknown` -- not `dead`,
    and not `live` because it came from a list somebody maintains.

    ⚠ A tracker this vantage cannot measure at all is `unmeasurable` even with
    no observation, because that is a structural fact about our position rather
    than a missing measurement (RULES 3.1).

    ⛔ **THE STATE IS ASKED FOR, NOT ECHOED, AND THAT WAS A REAL DEFECT.** This
    returned `history.ring[-1].state` -- the state the sweep recorded for **one
    observation**, where the sample count is 1 by construction. So no
    accumulation could ever change it, and on 2026-09-09 the published dataset
    carried **28 trackers with three observations and no successes, every one
    of them `unknown`**. `MIN_SAMPLES_FOR_DEATH` was decorative: the state
    machine that decides `dead` existed, was correct, and was never asked.

    Found by looking at the published data on the first day the history was
    deep enough for the question to have an answer.
    """
    if history is None or not history.ring:
        if not tracker.is_measurable_here:
            return HealthState.UNMEASURABLE.value, None, "unsupported"
        return HealthState.UNKNOWN.value, None, None
    last = history.ring[-1]
    failure = Failure(last.failure) if last.failure else Failure.NONE
    rung = Rung(last.rung) if last.rung else Rung.NONE
    state = health_state(
        rung=rung, transport=tracker.transport, network=tracker.network,
        # ⭐ The LIFETIME counts, which is the whole point: three failures
        # across three runs is what `MIN_SAMPLES_FOR_DEATH` is counting.
        sample_count=history.lifetime_checks,
        success_count=history.lifetime_successes,
        failure=failure, measurable=tracker.is_measurable_here)
    return state.value, last.rung, last.failure


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


def newest_observation(histories: Mapping[str, TrackerHistory] | None) -> str | None:
    """When the most recent observation in the history was taken, or `None`.

    ⛔ **Publication freshness is not measurement freshness, and conflating them
    hides the failure that matters.** The publisher runs after every sweep
    *completion* -- including a sweep that failed -- so `generated_at` keeps
    moving while no new observation arrives. A consumer checking only that
    would see a fresh file full of ageing labels.

    Found on 2026-09-09 while building the issue automation on top of the
    staleness contract: the automation asks the state file when it last learned
    something, and the published data could not answer the same question.
    """
    stamps = [h.last_seen for h in (histories or {}).values() if h.last_seen]
    return max(stamps) if stamps else None


def render_json(trackers: list[Tracker], *, provenance: Mapping[str, list[str]],
                histories: Mapping[str, TrackerHistory] | None = None,
                generated_at: str, code_version: str,
                observed_from: str | None = None) -> str:
    """The labelled dataset as JSON.

    `generated_at` is injected (RULES 3.6). `sort_keys` and a fixed indent so
    two runs over one input are byte-identical, which `gate.yml` asserts.
    """
    rows = _rows(trackers, provenance, histories or {}, observed_from)
    document = {
        "generated_at": generated_at,
        "code_version": code_version,
        # T-062. Four questions a consumer must be able to answer about what
        # they received: which dataset, when, under what rules, in what shape.
        # ⛔ Versioned **independently**, because they change for different
        # reasons: a field can be added without any normalization rule moving.
        "schema_version": SCHEMA_VERSION,
        "normalization_version": NORMALIZATION_VERSION,
        # ⛔ `null` is the honest value while no model is chosen (T-044). A `1`
        # would tell a consumer a methodology exists and is stable.
        "scoring_version": SCORING_VERSION,
        #: The rows, hashed. Two documents with one digest are the same data,
        #: whatever their `generated_at` says, which is what lets a consumer
        #: tell a re-publication from a change.
        "digest": digest_of(rows),
        # T-002. ⛔ The cadence travels with the data because the consumer is
        # the only party outside our own failure. If this project stops, every
        # file still serves 200 and nothing but `generated_at` against this
        # number says so.
        "publish_interval_seconds": PUBLISH_INTERVAL_SECONDS,
        "stale_after_intervals": STALE_AFTER_INTERVALS,
        # ⛔ Not the same question as `generated_at`. See `newest_observation`.
        "newest_observation": newest_observation(histories),
        "count": len(trackers),
        "fields": list(FIELDS),
        # ⚠ Stated in the data rather than only in the documentation, because
        # the consumer most likely to misread this file will never open a page.
        "vantage_note": (
            "Health is measured from one cloud provider's address space and "
            "from one authoring host, never from a residential connection. A "
            "tracker recorded unknown may answer you."),
        "trackers": rows,
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
