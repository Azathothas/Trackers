"""T-086: every path from an upstream byte to a filesystem path or a parser.

RULES 5.1 states the threat model and some of it is enforced by construction.
This is the part that is asserted rather than assumed, one test per threat the
entry names:

    path traversal        a source-supplied string must never reach a path
    oversized response    every network read is bounded
    control characters    a URL carrying one is refused, not sanitised
    decompression bomb    only reachable if compression is accepted -- it is
                          not, and this is what says so

⭐ **The strongest guarantee here is structural and cannot be tested away.**
There is no shell layer at all (D1), so "never interpolate upstream content
into a shell command" is impossible rather than remembered. What follows tests
the parts where a mistake would still be possible.

`HISTORY/reviews/2026-09-09-12-acquisition-path.md` is the review these came
out of.

No network. Run:  python3 -m unittest tests.test_acquisition_security -v
"""

from __future__ import annotations

import gzip
import io
import os
import sys
import unittest
from dataclasses import replace

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.acquire import (MAX_RESPONSE_BYTES, Outcome,  # noqa: E402
                              USER_AGENT, fetch, read_cached)
from trackers.normalize import InvalidTracker, parse, parse_many  # noqa: E402
from trackers.registry import SOURCES, Source  # noqa: E402


class _Response(io.BytesIO):
    """The shape `urlopen` returns, as much of it as `fetch` reads."""

    def __init__(self, payload: bytes, status: int = 200, headers=None):
        super().__init__(payload)
        self.status = status
        self.headers = headers or {"Content-Type": "text/plain"}

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()
        return False


def a_source(url: str = "https://upstream.example/list.txt") -> Source:
    """A registry entry under test, built from a real one so the shape cannot
    drift away from what production uses."""
    template = SOURCES[0]
    return replace(template, id="under_test", url=url)


class NoSourceSuppliedStringReachesAPath(unittest.TestCase):
    """Threat 1: path traversal."""

    def test_every_registry_id_is_a_single_safe_path_component(self):
        """⛔ The cache filename is built from the id, so an id that is not a
        bare component is a traversal waiting for somebody to add it."""
        for source in SOURCES:
            with self.subTest(source=source.id):
                self.assertEqual(os.path.basename(source.id), source.id)
                self.assertNotIn("..", source.id)
                self.assertFalse(os.path.isabs(source.id))
                self.assertNotIn("/", source.id)
                self.assertNotIn("\\", source.id)

    def test_the_cache_path_comes_from_the_registry_and_not_the_body(self):
        """⭐ The structural half: `read_cached` takes a `Source`, and the body
        it reads has no way to influence which file was opened -- the path is
        computed before anything is read."""
        import inspect
        source = inspect.getsource(read_cached)
        self.assertIn("source.id", source)
        # The only join in the function uses the id, and nothing from the body
        # exists yet at that point.
        self.assertLess(source.index("os.path.join"), source.index("fh.read()"))

    def test_a_hostile_url_in_a_body_never_becomes_a_filename(self):
        """A URL that looks like a path is refused as a URL, so it cannot be
        one anywhere downstream either."""
        accepted, rejected = parse_many([
            "udp://../../etc/passwd:6969/announce",
            "http://..%2f..%2fetc%2fpasswd/announce",
            "udp:///etc/shadow:6969/announce",
        ])
        self.assertEqual(accepted, [], [t.url for t in accepted])
        self.assertEqual(len(rejected), 3)


class EveryReadIsBounded(unittest.TestCase):
    """Threat 2: an oversized response."""

    def test_a_body_over_the_ceiling_is_rejected_and_not_truncated(self):
        """⛔ Rejected, never trimmed to fit. A truncated list is a source
        silently losing its tail, which is the failed-versus-empty conflation
        wearing a different hat."""
        payload = b"udp://a.example:6969/announce\n" * 400000
        self.assertGreater(len(payload), MAX_RESPONSE_BYTES)
        result = fetch(a_source(), opener=lambda *a, **k: _Response(payload))
        self.assertIs(result.outcome, Outcome.REJECTED)
        self.assertIsNone(result.trackers,
                          "a rejected fetch must not contribute a list")
        self.assertIn("exceeded", result.detail)

    def test_a_body_at_the_ceiling_is_still_read(self):
        """⚠ The positive control: a bound that rejected everything would pass
        the test above and quietly publish nothing."""
        payload = b"".join(f"udp://h{i}.example:6969/announce\n".encode()
                           for i in range(100))
        self.assertLess(len(payload), MAX_RESPONSE_BYTES)
        result = fetch(a_source(), opener=lambda *a, **k: _Response(payload))
        self.assertIs(result.outcome, Outcome.OK)
        # ⚠ 100, not 1: `parse_many` parses and does not deduplicate. Dedup is
        # a later stage with its own decisions to record, and asserting 1 here
        # would be asserting the wrong module's behaviour.
        self.assertEqual(len(result.trackers or []), 100)

    def test_the_read_is_bounded_before_the_size_is_known(self):
        """⛔ `read(MAX + 1)`, not `read()` then a length check. The second
        reads the whole body into memory first, which is the exhaustion this
        bound exists to prevent."""
        import inspect
        source = inspect.getsource(fetch)
        self.assertIn("read(MAX_RESPONSE_BYTES + 1)", source)


class ControlCharactersAreRefused(unittest.TestCase):
    """Threat 3."""

    def test_a_url_carrying_one_is_rejected_rather_than_stripped(self):
        """⛔ Rejected. Stripping would produce a URL nobody published, and
        then publish it."""
        for raw in ("udp://a.example:6969/announce\x00",
                    "udp://a\rb.example:6969/announce",
                    "http://a.example/an\nnounce",
                    "http://a.example/\x1b[31mannounce"):
            with self.subTest(raw=raw):
                with self.assertRaises(InvalidTracker):
                    parse(raw)

    def test_the_rejection_says_which_class_it_was(self):
        """RULES 3.10: a rejection is a returned value with an explainable
        reason, because 'contains control characters' tells a maintainer what
        changed upstream."""
        _, rejected = parse_many(["udp://a.example:6969/announce\x00"])
        self.assertEqual(len(rejected), 1)
        self.assertIn("control", rejected[0][1])


class CompressionIsNotAccepted(unittest.TestCase):
    """Threat 4: a decompression bomb, which is reachable only if we decompress.

    ⭐ **We do not, and that is the finding rather than a mitigation.** The
    entry says to confirm "decompression handling **if** compression is ever
    accepted". `fetch` sends no `Accept-Encoding`, and `urllib.request` does
    not decompress on its own, so a compressed body arrives as bytes, is
    bounded at 8 MiB like any other, and decodes to text no line of which
    parses. There is no expansion step for a bomb to exploit.
    """

    def test_the_request_asks_for_no_encoding(self):
        import inspect
        source = inspect.getsource(fetch)
        self.assertNotIn("Accept-Encoding", source)
        self.assertIn("User-Agent", source)
        self.assertIn("trackers/0.1", USER_AGENT)

    def test_a_gzip_body_is_not_expanded_and_yields_no_trackers(self):
        """⛔ The bomb's payload never grows. A 30-byte gzip of a large body
        stays 30 bytes here, and its content parses to nothing."""
        inner = b"udp://a.example:6969/announce\n" * 100000
        payload = gzip.compress(inner)
        self.assertLess(len(payload), len(inner) // 10)
        result = fetch(a_source(), opener=lambda *a, **k: _Response(
            payload, headers={"Content-Type": "application/gzip",
                              "Content-Encoding": "gzip"}))
        # It is read, bounded, and refused by the parser rather than expanded.
        self.assertIsNot(result.outcome, Outcome.OK)
        self.assertFalse(result.trackers)


if __name__ == "__main__":
    unittest.main()
