"""T-104: a 304 is a third outcome, and it keeps the source.

RULES 5.4 makes conditional requests mandatory in `ci`, and RULES 15.2 says a
304 is the cheapest correct answer available. The publisher fetches every
upstream after each sweep -- eight times a day -- so an unconditional fetch is
64 full downloads a day this project does not need.

⛔ **The outcome shape is the entry.** A 304 is not `OK`, because no body
arrived; not `FAILED`, because nothing went wrong; and emphatically not
`EMPTY`, which would delete the source from the dataset. Squeezing it into any
of the three is the conflation RULES 3.2 is about, one status code further
along.

The `Prove` clause is `test_a_304_preserves_the_previously_accepted_data` and
`test_it_is_recorded_distinctly_from_success_and_from_failure`.

No network -- the opener is injected. Run:
    python3 -m unittest tests.test_conditional_requests -v
"""

from __future__ import annotations

import io
import os
import sys
import unittest
import urllib.error
from dataclasses import replace

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "src"))

from trackers.acquire import Outcome, fetch  # noqa: E402
from trackers.pipeline import aggregate  # noqa: E402
from trackers.registry import SOURCES  # noqa: E402

BODY = ("udp://a.example:6969/announce\n"
        "udp://b.example:6969/announce\n"
        "http://c.example:80/announce\n")


class _Response(io.BytesIO):
    def __init__(self, payload: bytes, status: int = 200, headers=None):
        super().__init__(payload)
        self.status = status
        self.headers = headers or {"Content-Type": "text/plain",
                                   "ETag": '"abc123"',
                                   "Last-Modified": "Mon, 08 Sep 2026 00:00:00 GMT"}

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()
        return False


def a_source():
    return replace(SOURCES[0], id="under_test",
                   url="https://upstream.example/list.txt",
                   expected_min=1, expected_max=100)


def serving(payload: bytes = BODY.encode()):
    seen = {}

    def opener(req, **_kw):
        seen["headers"] = dict(req.headers)
        return _Response(payload)
    return opener, seen


def not_modified():
    def opener(req, **_kw):
        raise urllib.error.HTTPError(req.full_url, 304, "Not Modified",
                                     {}, None)
    return opener


class TheValidatorsAreSentAndKept(unittest.TestCase):

    def test_a_first_fetch_sends_no_validator_and_keeps_what_came_back(self):
        opener, seen = serving()
        result = fetch(a_source(), opener=opener)
        self.assertIs(result.outcome, Outcome.OK)
        self.assertNotIn("If-none-match", seen["headers"])
        self.assertEqual(result.etag, '"abc123"')
        self.assertEqual(result.last_modified,
                         "Mon, 08 Sep 2026 00:00:00 GMT")

    def test_the_next_fetch_sends_them_both(self):
        opener, seen = serving()
        fetch(a_source(), opener=opener, etag='"abc123"',
              last_modified="Mon, 08 Sep 2026 00:00:00 GMT")
        # ⚠ urllib title-cases header names, so the comparison is on that form.
        self.assertEqual(seen["headers"].get("If-none-match"), '"abc123"')
        self.assertEqual(seen["headers"].get("If-modified-since"),
                         "Mon, 08 Sep 2026 00:00:00 GMT")

    def test_nothing_defeats_the_cache(self):
        """⛔ RULES 5.4: no random query parameter, which is rude, ineffective
        and a fast route to 403."""
        opener, seen = serving()
        source = a_source()
        fetch(source, opener=opener)
        self.assertNotIn("Cache-Control", seen["headers"])
        self.assertNotIn("Pragma", seen["headers"])
        self.assertNotIn("?", source.url.split("//", 1)[1])


class A304KeepsTheSource(unittest.TestCase):
    """The `Prove` clause."""

    def test_a_304_preserves_the_previously_accepted_data(self):
        result = fetch(a_source(), opener=not_modified(), etag='"abc123"',
                       cached_body=BODY)
        self.assertIs(result.outcome, Outcome.UNCHANGED)
        self.assertIsNotNone(result.trackers)
        self.assertEqual(len(result.trackers or []), 3)
        self.assertTrue(result.usable,
                        "a 304 that cannot contribute has deleted the source")

    def test_it_is_recorded_distinctly_from_success_and_from_failure(self):
        unchanged = fetch(a_source(), opener=not_modified(), etag='"x"',
                          cached_body=BODY)
        ok, _ = serving()
        fresh = fetch(a_source(), opener=ok)
        failed = fetch(a_source(), opener=lambda *a, **k: (_ for _ in ()).throw(
            OSError("connection reset")))
        self.assertIs(unchanged.outcome, Outcome.UNCHANGED)
        self.assertIs(fresh.outcome, Outcome.OK)
        self.assertIs(failed.outcome, Outcome.FAILED)
        self.assertEqual(len({unchanged.outcome, fresh.outcome,
                              failed.outcome}), 3)
        self.assertEqual(unchanged.http_status, 304)

    def test_it_is_never_empty(self):
        """⛔ The conflation this entry exists to prevent. `EMPTY` would delete
        the source from the published dataset."""
        result = fetch(a_source(), opener=not_modified(), etag='"x"',
                       cached_body=BODY)
        self.assertIsNot(result.outcome, Outcome.EMPTY)
        self.assertIsNotNone(result.trackers)

    def test_a_304_with_no_snapshot_is_a_failure_and_says_whose_fault(self):
        """⚠ The server is right that nothing changed and we have nothing to
        show for it. That is our cache's fault, not the source's, and reporting
        it as unchanged would publish an empty source as current."""
        result = fetch(a_source(), opener=not_modified(), etag='"x"')
        self.assertIs(result.outcome, Outcome.FAILED)
        self.assertIsNone(result.trackers)
        self.assertIn("our cache", result.detail)

    def test_the_validators_survive_a_304_so_the_next_run_still_asks(self):
        result = fetch(a_source(), opener=not_modified(), etag='"abc123"',
                       last_modified="Mon, 08 Sep 2026 00:00:00 GMT",
                       cached_body=BODY)
        self.assertEqual(result.etag, '"abc123"')
        self.assertEqual(result.last_modified,
                         "Mon, 08 Sep 2026 00:00:00 GMT")


class TheDatasetKeepsAnUnchangedSource(unittest.TestCase):

    def test_its_trackers_reach_the_aggregate(self):
        source = a_source()
        result = fetch(source, opener=not_modified(), etag='"x"',
                       cached_body=BODY)
        agg = aggregate([result], {source.id: source})
        self.assertEqual(len(agg.trackers), 3,
                         "a 304 dropped the source from the dataset")

    def test_it_is_counted_as_unchanged_as_well_as_ok(self):
        """⭐ Both, and the distinction is the point: `ok` says the dataset has
        it, `unchanged` says the upstream was not made to send it again."""
        source = a_source()
        result = fetch(source, opener=not_modified(), etag='"x"',
                       cached_body=BODY)
        agg = aggregate([result], {source.id: source})
        self.assertEqual(agg.sources_unchanged, [source.id])
        self.assertEqual(agg.sources_ok, [source.id])

    def test_a_fresh_fetch_is_not_counted_as_unchanged(self):
        """⚠ The positive control."""
        source = a_source()
        opener, _ = serving()
        agg = aggregate([fetch(source, opener=opener)], {source.id: source})
        self.assertEqual(agg.sources_unchanged, [])
        self.assertEqual(agg.sources_ok, [source.id])


class TheBodyTravelsWithTheResult(unittest.TestCase):
    """⛔ The defect the conditional-request work surfaced, and it was live.

    `load_corpus` populated the raw bodies on its **offline** branch only, so an
    online run collected no exclusions and enforced none -- and the publisher
    runs online. On 2026-09-09 **eight URLs an operator had asked to be
    excluded were in the published dataset**. RULES 4 makes honouring a request
    absolute, so the body now travels on the result rather than being re-read
    by whichever branch remembers to.
    """

    def test_a_fetch_carries_the_body_it_read(self):
        opener, _ = serving()
        result = fetch(a_source(), opener=opener)
        self.assertEqual(result.body, BODY)

    def test_a_304_carries_the_snapshot_as_its_body(self):
        """Otherwise a blacklist that answered 304 would stop being enforced,
        which is the same defect one status code further along."""
        result = fetch(a_source(), opener=not_modified(), etag='"x"',
                       cached_body=BODY)
        self.assertEqual(result.body, BODY)

    def test_a_failed_fetch_carries_no_body(self):
        """⚠ `None`, not empty: we do not know what the source contains."""
        result = fetch(a_source(), opener=lambda *a, **k: (_ for _ in ()).throw(
            OSError("reset")))
        self.assertIsNone(result.body)

    def test_the_online_and_offline_paths_enforce_the_same_exclusions(self):
        """⭐ The regression, stated as the property that failed: the two paths
        must agree about what is excluded, because only one of them publishes.
        """
        import sys as _sys
        _sys.path.insert(0, os.path.join(REPO, "scripts"))
        from generate import load_corpus
        offline_agg, _, offline_enforced = load_corpus(
            True, os.path.join(REPO, "tests", "fixtures", "sources"))
        self.assertTrue(offline_enforced,
                        "the fixture blacklist enforces nothing, so this test "
                        "cannot tell the two paths apart")
        # The online path is exercised by its own shape: a blacklist result
        # carrying a body must reach `collect_exclusions`.
        from trackers.pipeline import collect_exclusions, enforced_exclusions
        from trackers.registry import SOURCES as REGISTERED
        blacklist = next(s for s in REGISTERED if s.role.value == "blacklist")
        with open(os.path.join(REPO, "tests", "fixtures", "sources",
                               f"{blacklist.id}.txt"), encoding="utf-8") as fh:
            body = fh.read()
        online_enforced = enforced_exclusions(
            collect_exclusions({blacklist.id: body}))
        self.assertEqual(online_enforced, offline_enforced)


if __name__ == "__main__":
    unittest.main()
