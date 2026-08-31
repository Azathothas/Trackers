"""Bencode, and the discriminator that tells a tracker from a web server.

This module is the production home of code that used to live inside
`experiments/05-http-tracker-protocol.py`. It moved here because T-020 requires
that an experiment and the production path be **the same code**: two copies of a
parser drift, and when they drift the experiment stops being evidence about the
thing that actually runs.

WHY STRICTNESS IS THE POINT

`classify_body` is the whole HTTP half of health checking. the ladder in TODO/measurement.md's claim,
verified as `C-32` by `experiments/05` with a negative control, is that a
bencoded response is the reliable discriminator between an HTTP tracker and an
ordinary web server. That claim only holds if the decoder is strict: a decoder
that accepts sloppy input will happily "parse" something that is not a tracker
answer, and then the probe reports a parked domain as live. That is the
anti-pattern in RULES 11 and it is the reason the negative control fails the
build rather than logging a warning.

TWO SPELLINGS, READ FROM THE SPECS RATHER THAN REMEMBERED

    BEP 3  (https://www.bittorrent.org/beps/bep_0003.html, fetched 2026-08-29)
      "If a tracker response has a key `failure reason`, then that maps to a
       human readable string which explains why the query failed, and no other
       keys are required."
    BEP 48 (https://www.bittorrent.org/beps/bep_0048.html, fetched 2026-08-29)
      An unsuccessful scrape returns the bencoded key `failure_reason`.

`failure reason` with a space, `failure_reason` with an underscore. A parser
that accepts only one of them misreads whichever half of the trackers it meets
uses the other. Both are accepted and **which one was seen is recorded**,
because that distribution is itself a finding.
"""

from __future__ import annotations

from typing import Any

__all__ = [
    "BencodeError",
    "bdecode",
    "classify_body",
    "FAILURE_KEYS",
    "TRACKER_KINDS",
]


class BencodeError(ValueError):
    """Input is not well-formed bencode. Carries where and why."""


def bdecode(data: bytes) -> tuple[Any, int]:
    """Decode one bencoded value. Returns `(value, bytes_consumed)`.

    Consumed length is returned rather than discarded so a caller can see
    trailing bytes. A tracker answer that decodes and then has 4 KiB of HTML
    after it is not a clean tracker answer, and silently ignoring the tail
    would hide that.

    Strict about the cases a lenient decoder gets wrong:

    * non-canonical integers (`i-0e`, `i007e`) are rejected -- bencode has
      exactly one encoding per value, and accepting alternatives means two
      different byte strings could decode equal, which breaks any hashing or
      comparison built on top;
    * a string length that runs past the end of the buffer is an error rather
      than a silent truncation, so a **truncated response is distinguishable
      from a short one** (this is a failure mode the oracle exercises);
    * a dictionary key that is not a byte string is an error, per BEP 3.
    """

    def _dec(i: int) -> tuple[Any, int]:
        if i >= len(data):
            raise BencodeError("truncated: input ended where a value was expected")
        c = data[i:i + 1]
        if c == b"i":
            j = data.find(b"e", i)
            if j < 0:
                raise BencodeError("unterminated integer")
            raw = data[i + 1:j]
            if not raw or raw == b"-":
                raise BencodeError("empty integer")
            body = raw[1:] if raw.startswith(b"-") else raw
            if not body.isdigit():
                raise BencodeError(f"non-numeric integer {raw!r}")
            # Canonical form: no leading zeros, and no negative zero.
            if raw == b"-0" or (len(body) > 1 and body.startswith(b"0")):
                raise BencodeError(f"non-canonical integer {raw!r}")
            return int(raw), j + 1
        if c == b"l":
            i += 1
            out: list[Any] = []
            while True:
                if i >= len(data):
                    raise BencodeError("unterminated list")
                if data[i:i + 1] == b"e":
                    return out, i + 1
                v, i = _dec(i)
                out.append(v)
        if c == b"d":
            i += 1
            out_d: dict[bytes, Any] = {}
            while True:
                if i >= len(data):
                    raise BencodeError("unterminated dictionary")
                if data[i:i + 1] == b"e":
                    return out_d, i + 1
                k, i = _dec(i)
                if not isinstance(k, bytes):
                    raise BencodeError("dictionary key is not a byte string")
                v, i = _dec(i)
                out_d[k] = v
        if c.isdigit():
            j = data.find(b":", i)
            if j < 0:
                raise BencodeError("string length has no ':' terminator")
            raw_len = data[i:j]
            if len(raw_len) > 1 and raw_len.startswith(b"0"):
                raise BencodeError(f"non-canonical string length {raw_len!r}")
            n = int(raw_len)
            if j + 1 + n > len(data):
                raise BencodeError(
                    f"string length {n} runs past end of input "
                    f"({len(data) - j - 1} bytes available) -- truncated response")
            return data[j + 1:j + 1 + n], j + 1 + n
        raise BencodeError(f"unexpected byte {c!r} at offset {i}")

    try:
        return _dec(0)
    except BencodeError:
        raise
    except (ValueError, IndexError, RecursionError) as e:
        raise BencodeError(f"{type(e).__name__}: {e}") from e


#: Both spellings, per BEP 3 and BEP 48 respectively.
FAILURE_KEYS: tuple[bytes, ...] = (b"failure reason", b"failure_reason")

#: The re-announce FLOOR a tracker asks for. BEP 3 spells it with a space;
#: the underscore form occurs in the wild and a production client reads both
#: (`C-65`). Order is significant: BEP 3's spelling wins when both appear.
MIN_INTERVAL_KEYS: tuple[bytes, ...] = (b"min interval", b"min_interval")

#: The classifications that prove the responder is a tracker rather than merely
#: a working HTTP server. A `failure reason` counts: it means the tracker
#: parsed our request and answered in-protocol, which is a *stronger* signal
#: than a 200 with peers, not a weaker one.
TRACKER_KINDS: frozenset[str] = frozenset({
    "tracker_failure_response",
    "tracker_announce_response",
    "tracker_scrape_response",
})


def classify_body(body: bytes) -> dict[str, Any]:
    """Decide what an HTTP response body actually is.

    Returns a dict with at least `kind` and `detail`. `kind` is one of:

        empty                       zero-length body
        html                        parsed as HTML, not bencode
        not_bencode                 neither bencode nor recognisably HTML
        bencode_not_dict            valid bencode whose top level is not a dict
        bencode_dict_unrecognised   a dict with none of the tracker keys
        tracker_failure_response    BEP 3 / BEP 48 failure -- a working tracker
        tracker_announce_response   has `peers` or `interval`
        tracker_scrape_response     has `files`

    Only the last three are in `TRACKER_KINDS`. The distinction between
    `bencode_dict_unrecognised` and the tracker kinds is deliberately narrow:
    something that emits valid bencode but no tracker key is *not* proven to be
    a tracker, and recording it as one would be exactly the confident wrongness
    this project exists to avoid.
    """
    if not body:
        return {"kind": "empty", "detail": "zero-length body"}
    try:
        value, consumed = bdecode(body)
    except BencodeError as e:
        head = body[:60]
        lowered = body[:512].lower()
        looks_html = head.lstrip()[:1] == b"<" or b"<html" in lowered
        return {
            "kind": "html" if looks_html else "not_bencode",
            "detail": f"bdecode failed: {e}",
            "head": head.decode("utf-8", "replace"),
        }
    if not isinstance(value, dict):
        return {"kind": "bencode_not_dict",
                "detail": f"top level is {type(value).__name__}, not a dictionary"}

    trailing = len(body) - consumed
    keys = sorted(k.decode("utf-8", "replace") for k in value)

    for k in FAILURE_KEYS:
        if k in value:
            msg = value[k]
            return {
                "kind": "tracker_failure_response",
                "failure_key_spelling": k.decode(),
                "detail": (msg if isinstance(msg, bytes) else b"").decode(
                    "utf-8", "replace")[:200],
                "trailing_bytes": trailing,
            }
    if b"peers" in value or b"interval" in value:
        interval = value.get(b"interval")
        # `min interval` is the tracker stating a FLOOR, and it binds more
        # tightly than `interval` -- it is the number an operator would judge
        # us by (`C-65`). BEP 3 spells it with a space; some trackers use an
        # underscore, and a real client reads both
        # (`references/Azathothas__bit-cli/tree/crates/bit-cli-core/src/tracker.rs:739`).
        min_interval = None
        for spelling in MIN_INTERVAL_KEYS:
            candidate = value.get(spelling)
            if isinstance(candidate, int):
                min_interval = candidate
                break
        return {
            "kind": "tracker_announce_response",
            "detail": f"keys={keys[:8]}",
            # The tracker's own stated re-check interval. D7 makes this the
            # authority on how often we may probe it, so it is carried out of
            # the classifier rather than discarded.
            "interval": interval if isinstance(interval, int) else None,
            "min_interval": min_interval,
            "trailing_bytes": trailing,
        }
    if b"files" in value:
        files = value[b"files"]
        return {
            "kind": "tracker_scrape_response",
            "detail": f"files entries={len(files) if isinstance(files, dict) else '-'}",
            "trailing_bytes": trailing,
        }
    return {"kind": "bencode_dict_unrecognised",
            "detail": f"keys={keys[:8]}",
            "trailing_bytes": trailing}
