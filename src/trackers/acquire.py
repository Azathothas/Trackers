"""Acquisition: treat every upstream as hostile, and never conflate failure with emptiness.

This module exists mostly to enforce **one invariant**, which RULES 3.10
states and which *both* pieces of prior art examined in the sweep get wrong:

    "Source failed" and "source successfully returned zero trackers" are
    distinct states, distinctly recorded, with distinct consequences.

Two independent codebases violate it, in different languages:

  * `references/pkgforge-security__Trackers/tree/.github/workflows/fetch_update_trackers.yaml`
    @ `7f2d00b` runs every step under `set +e` AND `continue-on-error: true`,
    and fetches with `curl -qfSL ... -o FILE`. `curl -o` truncates the output
    file *before* the transfer and `-f` writes nothing on an HTTP error, so a
    failed fetch leaves an **empty file** that is then concatenated into the
    published list. An entire source vanishes and nothing reports it.

  * `references/GerryFerdinandus__bittorrent-tracker-editor/tree/source/code/ngosang_trackerslist.pas:98`
    @ `c5f5b82` wraps its download in `try ... except` and calls
    `FTRackerList[...].Clear` on any exception. Same conflation, different
    language, unrelated project.

Two independent occurrences is why this is a type in the code rather than a
paragraph in a document: `Outcome` makes the two states unrepresentable as one
another, and `FetchResult.trackers` is `None` -- not `[]` -- when a fetch failed.
"""

from __future__ import annotations

import hashlib
import os
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum

from .model import Tracker
from .normalize import parse_many
from .registry import Source

USER_AGENT = (
    "trackers/0.1 "
    "(+https://github.com/Azathothas/Trackers; "
    "public tracker list aggregation; contact via repository issues)"
)

#: RULES 5.2 requires a bounded response size for every network
#: operation. 8 MiB is ~5x the largest source observed (desirefire, ~40 KB)
#: with a wide margin, and far below anything that could exhaust a runner.
MAX_RESPONSE_BYTES = 8 * 1024 * 1024

DEFAULT_TIMEOUT = 30.0


class Outcome(str, Enum):
    """The states a fetch can end in. These are NOT interchangeable.

    The distinction that matters is between `FAILED` and `EMPTY`:

      FAILED  we do not know what the source contains. Keep the last known
              good data. Do NOT let this influence the dataset.
      EMPTY   the source successfully told us it has nothing. That is
              information, and it is suspicious enough to reject on its own.
    """

    OK = "ok"
    EMPTY = "empty"
    FAILED = "failed"
    REJECTED = "rejected"       # fetched fine, failed validation (T-102)
    NOT_ATTEMPTED = "not_attempted"


@dataclass
class FetchResult:
    """What one source gave us, and what may be concluded from it."""

    source_id: str
    url: str
    outcome: Outcome
    fetched_at: str

    #: `None` means "we do not know" -- a failed fetch. An empty list means
    #: "the source told us it has nothing". Conflating these is the bug.
    trackers: list[Tracker] | None = None

    rejected: list[tuple[str, str]] = field(default_factory=list)
    http_status: int | None = None
    content_type: str | None = None
    byte_count: int | None = None
    content_sha256: str | None = None
    detail: str = ""
    looked_like_html: bool = False

    @property
    def usable(self) -> bool:
        """Whether this result may contribute trackers to the dataset."""
        return self.outcome is Outcome.OK and self.trackers is not None

    @property
    def count(self) -> int | None:
        """Tracker count, or `None` when unknown. Never silently zero."""
        return None if self.trackers is None else len(self.trackers)


def _now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def validate_body(source: Source, body: str) -> tuple[Outcome, str, bool]:
    """Sanity-check a fetched body before parsing it. RULES 5.1; T-102 owns the thresholds.

    Returns (outcome, detail, looked_like_html).
    """
    stripped = body.strip()
    looks_html = stripped[:200].lower().startswith(("<!doctype", "<html", "<?xml"))
    if looks_html:
        # A source that starts returning HTML has changed or broken. It is a
        # REJECTION, not an empty list -- newtrackon.com/list is a real example
        # of an HTML page a careless integration would read as a tracker feed.
        return (Outcome.REJECTED,
                "body is HTML/XML where a plaintext list was expected", True)
    if not stripped:
        return Outcome.EMPTY, "body is empty", False
    return Outcome.OK, "", False


def validate_counts(source: Source, n: int) -> tuple[Outcome, str]:
    """Change detection on entry count. T-102.

    Thresholds come from `Source.expected_min/max`, which are **provisional**
    and documented as such: they were set from a single observation on
    2026-08-29 and deliberately widened, because a narrow band derived from one
    sample is a future outage wearing the costume of rigour.
    """
    if n == 0:
        return (Outcome.REJECTED,
                "parsed zero trackers from a non-empty body: parser or format "
                "change (RULES 5.1)")
    if n < source.expected_min:
        return (Outcome.REJECTED,
                f"{n} entries is below the provisional floor "
                f"{source.expected_min} (observed {source.observed_20260829} "
                f"on 2026-08-29): suspicious reduction")
    if n > source.expected_max:
        return (Outcome.REJECTED,
                f"{n} entries is above the provisional ceiling "
                f"{source.expected_max} (observed {source.observed_20260829} "
                f"on 2026-08-29): suspicious increase")
    return Outcome.OK, ""


def parse_body(source: Source, body: str) -> FetchResult:
    """Validate and parse an already-fetched body. No network, so it is testable.

    Splitting this from the transport is what lets the whole pipeline run with
    no external services (RULES 2): a test can hand this function a
    fixture of any shape -- HTML, truncated, empty, garbage -- without a server.
    """
    digest = hashlib.sha256(body.encode("utf-8", "replace")).hexdigest()
    outcome, detail, html = validate_body(source, body)
    if outcome is not Outcome.OK:
        return FetchResult(
            source_id=source.id, url=source.url, outcome=outcome,
            fetched_at=_now(), trackers=None, detail=detail,
            byte_count=len(body), content_sha256=digest, looked_like_html=html,
        )

    accepted, rejected = parse_many(body.splitlines())
    outcome, detail = validate_counts(source, len(accepted))
    if outcome is not Outcome.OK:
        # Rejected: we DID see the content, so it is not `FAILED`, but it must
        # not reach the dataset. Trackers are withheld deliberately.
        return FetchResult(
            source_id=source.id, url=source.url, outcome=outcome,
            fetched_at=_now(), trackers=None, rejected=rejected,
            detail=detail, byte_count=len(body), content_sha256=digest,
        )

    return FetchResult(
        source_id=source.id, url=source.url, outcome=Outcome.OK,
        fetched_at=_now(), trackers=accepted, rejected=rejected,
        byte_count=len(body), content_sha256=digest,
    )


def fetch(source: Source, timeout: float = DEFAULT_TIMEOUT,
          opener=None) -> FetchResult:
    """Fetch one source over the network. Every failure is a `FAILED` outcome.

    `opener` is injectable so tests never touch the network.
    """
    req = urllib.request.Request(source.url, headers={"User-Agent": USER_AGENT})
    open_fn = opener or urllib.request.urlopen
    try:
        with open_fn(req, timeout=timeout) as r:
            raw = r.read(MAX_RESPONSE_BYTES + 1)
            if len(raw) > MAX_RESPONSE_BYTES:
                return FetchResult(
                    source_id=source.id, url=source.url,
                    outcome=Outcome.REJECTED, fetched_at=_now(), trackers=None,
                    detail=f"response exceeded {MAX_RESPONSE_BYTES} bytes",
                    http_status=getattr(r, "status", None),
                )
            body = raw.decode("utf-8", "replace")
            status = getattr(r, "status", None)
            ctype = r.headers.get("Content-Type") if hasattr(r, "headers") else None
    except urllib.error.HTTPError as e:
        return FetchResult(source_id=source.id, url=source.url,
                           outcome=Outcome.FAILED, fetched_at=_now(),
                           trackers=None, http_status=e.code,
                           detail=f"HTTP {e.code}")
    except Exception as e:
        # Deliberately broad. Any transport failure is FAILED -- never an
        # empty list. This except clause is the one the prior art gets wrong.
        return FetchResult(source_id=source.id, url=source.url,
                           outcome=Outcome.FAILED, fetched_at=_now(),
                           trackers=None,
                           detail=f"{type(e).__name__}: {e}")

    result = parse_body(source, body)
    result.http_status = status
    result.content_type = ctype
    return result


def read_cached(source: Source, cache_dir: str) -> FetchResult:
    """Read a source from a committed cache. No network at all.

    The cache filename is derived from the **registry's** source id, never from
    upstream content: RULES 5.1 requires that "a source-supplied string
    MUST never reach a filesystem path".
    """
    path = os.path.join(cache_dir, f"{source.id}.txt")
    if not os.path.exists(path):
        return FetchResult(source_id=source.id, url=source.url,
                           outcome=Outcome.FAILED, fetched_at=_now(),
                           trackers=None, detail=f"not cached: {path}")
    with open(path, encoding="utf-8", errors="replace") as fh:
        body = fh.read()
    return parse_body(source, body)
