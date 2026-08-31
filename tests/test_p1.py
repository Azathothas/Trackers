"""P1 tests: acquisition, normalization, deduplication, plaintext, determinism.

These are written against the lines of the definition of done (HISTORY/gates.md) that P1 can satisfy. Each
test names the requirement it discharges, so a reader can check coverage
against that list rather than trusting a count.

Run:  python3 -m unittest discover -s tests -v
No network. No external services. RULES 2.
"""

from __future__ import annotations

import importlib.util
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))

from trackers.acquire import (Outcome, FetchResult, parse_body,  # noqa: E402
                              validate_counts)
from trackers.dedup import deduplicate, note_shared_addresses  # noqa: E402
from trackers.model import (HealthState, Network, Tracker,  # noqa: E402
                            Transport, classify_network)
from trackers.normalize import InvalidTracker, parse, parse_many  # noqa: E402
from trackers.exclusion import (ExclusionClass, classify_reason,  # noqa: E402
                                parse_blacklist, summarise)
from trackers.pipeline import (aggregate, collect_exclusions,  # noqa: E402
                               enforced_exclusions, flagged_exclusions,
                               render_plaintext, render_report)
from trackers.registry import Role, Source, Trust  # noqa: E402


def src(sid="s", role=Role.PRIMARY, lo=1, hi=10_000) -> Source:
    return Source(id=sid, url=f"https://example.invalid/{sid}", role=role,
                  trust=Trust.MEDIUM, category="test", upstream="test",
                  notes="", expected_min=lo, expected_max=hi,
                  observed_20260829=1)


# --------------------------------------------------------------------------
class TestNormalizationRules(unittest.TestCase):
    """the normalization contract in src/trackers/normalize.py: every rule has a test asserting it preserves identity."""

    def test_strips_surrounding_whitespace(self):
        self.assertEqual(parse("  udp://x.example:6969/announce  ").url,
                         "udp://x.example:6969/announce")

    def test_strips_trailing_comment_only_after_whitespace(self):
        self.assertEqual(parse("http://x.example/announce # spam").url,
                         "http://x.example/announce")

    def test_hash_without_leading_space_is_not_a_comment(self):
        """A '#' with no preceding space may be part of the URL.

        Truncating there would silently shorten a real tracker URL, which is
        the failure mode this project is least willing to accept.
        """
        self.assertEqual(parse("http://x.example/announce#frag").path,
                         "/announce")
        self.assertTrue(parse("http://x.example/ann#ounce").url.startswith(
            "http://x.example/ann"))

    def test_scheme_and_host_lowercased_path_is_not(self):
        t = parse("UDP://Tracker.Example.COM/AnNounce")
        self.assertEqual(t.transport, Transport.UDP)
        self.assertEqual(t.host, "tracker.example.com")
        self.assertEqual(t.path, "/AnNounce",
                         "path case is significant; announce paths carry keys")

    def test_trailing_dot_stripped(self):
        self.assertEqual(parse("http://x.example./announce").host, "x.example")

    def test_ipv6_brackets_round_trip(self):
        t = parse("udp://[2001:db8::1]:1337/announce")
        self.assertEqual(t.host, "2001:db8::1")
        self.assertEqual(t.url, "udp://[2001:db8::1]:1337/announce")

    def test_explicit_port_is_preserved_not_defaulted(self):
        """The refusal that matters. UDP has no default-port convention."""
        a = parse("udp://x.example:80/announce")
        b = parse("udp://x.example/announce")
        self.assertNotEqual(a.url, b.url)
        self.assertEqual(a.port, 80)
        self.assertIsNone(b.port)

    def test_http_default_port_also_preserved(self):
        self.assertNotEqual(parse("http://x.example:80/announce").url,
                            parse("http://x.example/announce").url)

    def test_trailing_slash_preserved(self):
        self.assertNotEqual(parse("http://x.example/announce").url,
                            parse("http://x.example/announce/").url)

    def test_unreserved_percent_escapes_decoded_reserved_left_alone(self):
        self.assertEqual(parse("http://x.example/announce%2Dx").path,
                         "/announce-x")
        self.assertIn("%2F", parse("http://x.example/a%2Fb").path,
                      "decoding a reserved char would change the path")

    def test_normalization_is_idempotent(self):
        once = parse("  UDP://X.Example.COM.:6969/announce ")
        twice = parse(once.url)
        self.assertEqual(once.url, twice.url)


class TestRejection(unittest.TestCase):
    """the unit coverage list in T-120: adversarial inputs, not only happy paths."""

    def test_rejects_unknown_transport(self):
        with self.assertRaises(InvalidTracker):
            parse("ftp://x.example/announce")

    def test_rejects_missing_host(self):
        with self.assertRaises(InvalidTracker):
            parse("http:///announce")

    def test_rejects_control_characters(self):
        with self.assertRaises(InvalidTracker):
            parse("http://x.example/ann\x00ounce")

    def test_rejects_absurd_length(self):
        with self.assertRaises(InvalidTracker):
            parse("http://x.example/" + "a" * 5000)

    def test_rejects_path_traversal_shaped_host(self):
        """RULES 5.1: a source string must never look like a path."""
        for bad in ("http://../../etc/passwd/announce",
                    "http://x.example\\..\\..\\announce"):
            with self.assertRaises(InvalidTracker):
                parse(bad)

    def test_blank_and_comment_lines_are_not_rejections(self):
        """Formatting is not breakage; counting it as such makes sources look broken."""
        accepted, rejected = parse_many(
            ["", "   ", "# a comment", "udp://x.example:1/announce"])
        self.assertEqual(len(accepted), 1)
        self.assertEqual(rejected, [])

    def test_blank_line_separated_source_parses(self):
        """newTrackon separates entries with blank lines (measured)."""
        body = "udp://a.example:1/announce\n\nudp://b.example:2/announce\n\n"
        accepted, rejected = parse_many(body.splitlines())
        self.assertEqual(len(accepted), 2)
        self.assertEqual(rejected, [])


class TestTwoAxisModel(unittest.TestCase):
    """The census finding: transport and network are independent."""

    def test_i2p_is_a_hostname_suffix_not_a_scheme(self):
        t = parse("http://tracker.example.i2p/announce")
        self.assertEqual(t.transport, Transport.HTTP)
        self.assertEqual(t.network, Network.I2P)

    def test_i2p_lookalike_domain_is_clearnet(self):
        """`yggtracker.i2p.rocks` is a real entry and is NOT on I2P."""
        self.assertEqual(classify_network("yggtracker.i2p.rocks"),
                         Network.CLEARNET)

    def test_yggdrasil_ipv6_literal_detected(self):
        t = parse("http://[200:1e2f:e608:eb3a:2bf:1e62:87ba:e2f7]:80/announce")
        self.assertEqual(t.network, Network.YGGDRASIL)

    def test_every_census_transport_is_classifiable(self):
        """the definition of done (HISTORY/gates.md): classification covers every scheme the census found."""
        for scheme in ("udp", "http", "https", "ws", "wss"):
            with self.subTest(scheme=scheme):
                self.assertEqual(parse(f"{scheme}://x.example/announce").transport,
                                 Transport(scheme))

    def test_unmeasurable_protocols_are_flagged_not_dead(self):
        """the definition of done (HISTORY/gates.md): an unmeasurable protocol is never reported dead."""
        for url in ("http://x.i2p/announce", "wss://x.example/announce",
                    "http://[200::1]/announce"):
            with self.subTest(url=url):
                t = parse(url)
                self.assertFalse(t.is_measurable_here)
                self.assertIsNotNone(t.unmeasurable_reason)

    def test_bep48_scrape_uses_path_string_rule(self):
        self.assertEqual(parse("http://x.example/announce.php").scrape_url,
                         "http://x.example/scrape.php")
        self.assertIsNone(parse("udp://x.example:6969/").scrape_url)

    def test_bep48_derivation_never_invents_an_endpoint(self):
        """C-66: the match must start a whole path component.

        A bare substring replace turns `/announcements/feed` into
        `/scrapements/feed` -- an endpoint no tracker serves, whose 404 would
        then be recorded against the tracker instead of against our guess.
        That is the defect this asserts against; it was live until 2026-08-31.
        """
        for url in ("http://x.example/announcements/feed",
                    "http://x.example/announcements",
                    "http://x.example/nothing"):
            with self.subTest(url=url):
                self.assertIsNone(
                    parse(url).scrape_url,
                    "BEP 48 puts a path with no `announce` component outside "
                    "its scope; deriving one fabricates an endpoint")

        # And the forms that ARE in scope still derive.
        for url, expected in (
            ("udp://x.example:6969/announce", "udp://x.example:6969/scrape"),
            ("http://x.example/a/announce?key=1", "http://x.example/a/scrape?key=1"),
            ("http://x.example/announce/0b0b5b2b", "http://x.example/scrape/0b0b5b2b"),
        ):
            with self.subTest(url=url):
                self.assertEqual(parse(url).scrape_url, expected)

    def test_a_string_that_is_not_a_uri_is_rejected_with_its_reason(self):
        """RFC 3986's character set, enforced on the way in.

        Found by review 6: three of 1337 published lines carried a character no
        URI may hold -- a stray `"` leaked by somebody's HTML scraper, and two
        `authkey=...|...|...` query strings. Both characters are
        shell-significant in the `curl | client` idiom this project's README
        recommends, and the plaintext is the compatibility-critical format.

        Rejected rather than percent-encoded: encoding would change somebody's
        endpoint on our guess about what they meant. The reason travels with
        the rejection so the disappearance is explainable (RULES 3.10).
        """
        for raw in ('http://opentracker.example:6869/announce"',
                    "https://x.example/announce.php?authkey=213|10003|j46n2q",
                    "udp://x.example/announce^",
                    "udp://x.example/announce{1}"):
            with self.subTest(raw=raw):
                with self.assertRaises(InvalidTracker) as caught:
                    parse(raw)
                self.assertIn("no URI may hold", str(caught.exception))

    def test_every_reserved_uri_character_is_still_accepted(self):
        """The refusal must not become a second, stricter normalizer.

        Every gen-delim and sub-delim RFC 3986 permits stays legal; only what
        the specification excludes is refused.
        """
        for raw in ("udp://x.example:6969/announce?a=b&c=d",
                    "http://x.example/announce.php?passkey=a1b2!$'()*+,;=",
                    "http://x.example/~user/announce",
                    "http://[2001:db8::1]:80/announce",
                    "http://x.example/announce%2Fpath"):
            with self.subTest(raw=raw):
                parse(raw)   # must not raise

    def test_a_rejection_is_returned_and_not_dropped(self):
        """RULES 3.10: a tracker that disappears owes the consumer a reason."""
        accepted, rejected = parse_many([
            "udp://good.example:6969/announce",
            'http://bad.example/announce"',
        ])
        self.assertEqual(len(accepted), 1)
        self.assertEqual(len(rejected), 1)
        raw, reason = rejected[0]
        self.assertIn("bad.example", raw)
        self.assertIn("no URI may hold", reason)

    def test_an_idn_hostname_is_rejected_rather_than_mangled(self):
        """The IDN decision, made explicit because it was previously silent.

        `normalize.parse` accepts A-label (punycode) hostnames and rejects
        U-label (Unicode) ones. That is a deliberate refusal, not an oversight:
        IDNA encoding is version-dependent (IDNA2003 vs IDNA2008 disagree on
        real characters), so encoding here would mean *guessing* which tracker
        an upstream meant. A rejection is auditable and recoverable; a wrong
        A-label is a silently different host (RULES 3.10).
        """
        self.assertEqual(
            parse("udp://tracker.xn--e1afmkfd.xn--p1ai:6969/announce").host,
            "tracker.xn--e1afmkfd.xn--p1ai")
        with self.assertRaises(InvalidTracker):
            parse("udp://tracker.\u043f\u0440\u0438\u043c\u0435\u0440.\u0440\u0444:6969/announce")


class TestSourceFailureIsNotEmptiness(unittest.TestCase):
    """the definition of done (HISTORY/gates.md): 'source failed' and 'source returned zero' differ.

    This is the invariant BOTH pieces of prior art violate, in two languages.
    """

    def test_failed_fetch_yields_none_not_empty_list(self):
        r = FetchResult(source_id="s", url="u", outcome=Outcome.FAILED,
                        fetched_at="t")
        self.assertIsNone(r.trackers)
        self.assertIsNone(r.count, "count must be unknown, never 0")
        self.assertFalse(r.usable)

    def test_empty_body_is_empty_not_failed(self):
        r = parse_body(src(), "")
        self.assertIs(r.outcome, Outcome.EMPTY)
        self.assertIsNot(r.outcome, Outcome.FAILED)

    def test_html_body_is_rejected_not_empty(self):
        r = parse_body(src(), "<!DOCTYPE html><html><body>hi</body></html>")
        self.assertIs(r.outcome, Outcome.REJECTED)
        self.assertTrue(r.looked_like_html)

    def test_truncated_garbage_is_rejected_not_silently_empty(self):
        r = parse_body(src(lo=5), "udp://a.example:1/announce\nudp://trunc")
        self.assertIs(r.outcome, Outcome.REJECTED)
        self.assertIsNone(r.trackers)

    def test_suspicious_reduction_rejected(self):
        r = parse_body(src(lo=50, hi=200), "udp://a.example:1/announce\n")
        self.assertIs(r.outcome, Outcome.REJECTED)
        self.assertIn("below", r.detail)

    def test_suspicious_increase_rejected(self):
        body = "".join(f"udp://h{i}.example:1/announce\n" for i in range(300))
        r = parse_body(src(lo=1, hi=100), body)
        self.assertIs(r.outcome, Outcome.REJECTED)
        self.assertIn("above", r.detail)

    def test_failed_source_cannot_corrupt_the_dataset(self):
        """the definition of done (HISTORY/gates.md): a vanished source cannot corrupt canonical data."""
        good = parse_body(src("good"), "udp://a.example:1/announce\n")
        dead = FetchResult(source_id="bad", url="u", outcome=Outcome.FAILED,
                           fetched_at="t")
        agg = aggregate([good, dead], {"good": src("good"), "bad": src("bad")})
        self.assertEqual(len(agg.trackers), 1)
        self.assertEqual(agg.sources_failed, ["bad"])
        self.assertEqual(agg.sources_ok, ["good"])


class TestDeduplication(unittest.TestCase):
    """the three dedup questions in src/trackers/dedup.py: three different questions."""

    def test_exact_duplicates_removed(self):
        ts = [parse("udp://x.example:1/announce"),
              parse("  UDP://X.Example:1/announce  ")]
        r = deduplicate(ts)
        self.assertEqual(len(r.trackers), 1)
        self.assertEqual(r.removed, 1)

    def test_same_host_different_transport_is_kept(self):
        ts = [parse("udp://x.example:6969/announce"),
              parse("http://x.example:6969/announce")]
        r = deduplicate(ts)
        self.assertEqual(len(r.trackers), 2, "distinct endpoints must survive")
        self.assertTrue(any(d.kind == "sibling_endpoint" and not d.acted
                            for d in r.decisions))

    def test_shared_address_is_recorded_never_removed(self):
        ts = [parse("http://a.example/announce"), parse("http://b.example/announce")]
        ds = note_shared_addresses(
            ts, {"a.example": ["104.21.0.1"], "b.example": ["104.21.0.1"]},
            observed_at="2026-08-29T00:00:00Z")
        self.assertTrue(ds)
        self.assertTrue(all(not d.acted for d in ds))
        self.assertIn("2026-08-29", ds[0].reason,
                      "a same-IP claim without a timestamp is not evidence")

    def test_dedup_is_order_independent(self):
        a = [parse("udp://b.example:2/announce"), parse("udp://a.example:1/announce")]
        b = list(reversed(a))
        self.assertEqual([t.url for t in deduplicate(a).trackers],
                         [t.url for t in deduplicate(b).trackers])


class TestPlaintextAndDeterminism(unittest.TestCase):

    def test_plaintext_is_one_url_per_line_no_comments(self):
        out = render_plaintext([parse("udp://x.example:1/announce")])
        self.assertEqual(out, "udp://x.example:1/announce\n")
        self.assertNotIn("#", out)

    def test_hardcoded_preserves_order_and_self_deduplicates(self):
        """the definition of done (HISTORY/gates.md): hardcoded.txt keeps manual order and self-dedups."""
        manual = [parse("udp://z.example:1/announce"),
                  parse("udp://a.example:1/announce"),
                  parse("udp://z.example:1/announce")]
        out = render_plaintext(manual, preserve_order=True).splitlines()
        self.assertEqual(out, ["udp://z.example:1/announce",
                               "udp://a.example:1/announce"])

    def test_pipeline_is_byte_identical_across_two_runs(self):
        """the definition of done (HISTORY/gates.md) and the P1 gate. The clock is injected, so this holds."""
        body = "".join(f"udp://h{i}.example:{i+1}/announce\n" for i in range(25))
        sources = {"a": src("a"), "b": src("b")}
        first = render_plaintext(
            aggregate([parse_body(src("a"), body), parse_body(src("b"), body)],
                      sources).trackers)
        second = render_plaintext(
            aggregate([parse_body(src("b"), body), parse_body(src("a"), body)],
                      sources).trackers)
        self.assertEqual(first, second,
                         "output must not depend on source ordering (I6)")

        r1 = render_report(aggregate([parse_body(src("a"), body)], sources),
                           generated_at="FIXED", code_version="v")
        r2 = render_report(aggregate([parse_body(src("a"), body)], sources),
                           generated_at="FIXED", code_version="v")
        self.assertEqual(r1, r2)

    def test_blacklist_source_contributes_no_trackers(self):
        bl = src("bl", role=Role.BLACKLIST)
        good = src("good")
        results = [parse_body(bl, "udp://evil.example:1/announce # fake seeds\n"),
                   parse_body(good, "udp://good.example:1/announce\n")]
        agg = aggregate(results, {"bl": bl, "good": good})
        urls = {t.url for t in agg.trackers}
        self.assertEqual(urls, {"udp://good.example:1/announce"},
                         "a blacklist lists trackers an upstream REMOVED")

    def test_provenance_records_every_contributing_source(self):
        a, b = src("a"), src("b")
        body = "udp://shared.example:1/announce\n"
        agg = aggregate([parse_body(a, body), parse_body(b, body)],
                        {"a": a, "b": b})
        self.assertEqual(agg.provenance["udp://shared.example:1/announce"],
                         ["a", "b"])


if __name__ == "__main__":
    unittest.main()


class TestExclusionClassification(unittest.TestCase):
    """An upstream blacklist mixes operator requests with opinions.

    Adopting or rejecting it wholesale gets one of the two wrong.
    """

    def test_operator_requests_are_honoured(self):
        for reason in ("requested by sysadmin", "deprecated by owner",
                       "owner request", "opted out"):
            with self.subTest(reason=reason):
                self.assertIs(classify_reason(reason), ExclusionClass.HONOUR)

    def test_safety_reasons_are_enforced(self):
        for reason in ("detected by antivirus software", "detected as suspicious",
                       "malware"):
            with self.subTest(reason=reason):
                self.assertIs(classify_reason(reason), ExclusionClass.SAFETY)

    def test_measurement_opinions_are_not_enforced(self):
        """HISTORY/reference-sweep.md: an upstream's filtering decisions are opinions."""
        for reason in ("registered torrents", "duplicate of udp://x/announce",
                       "malfunction", "fake seeds", "error"):
            with self.subTest(reason=reason):
                self.assertIs(classify_reason(reason), ExclusionClass.OPINION)

    def test_unrecognised_reason_defaults_to_opinion(self):
        """The safe direction: an unknown reason must not silently delete data."""
        self.assertIs(classify_reason("something nobody has seen before"),
                      ExclusionClass.OPINION)

    def test_blacklist_parser_keeps_the_reason(self):
        ex = parse_blacklist(
            "udp://a.example:1/announce # requested by sysadmin\n"
            "udp://b.example:2/announce # registered torrents\n", "bl")
        self.assertEqual(len(ex), 2)
        self.assertEqual(summarise(ex), {"honour": 1, "safety": 0, "opinion": 1})

    def test_only_honour_and_safety_are_enforced(self):
        body = ("udp://a.example:1/announce # requested by sysadmin\n"
                "udp://b.example:2/announce # detected by antivirus software\n"
                "udp://c.example:3/announce # registered torrents\n")
        ex = collect_exclusions({"bl": body})
        self.assertEqual(enforced_exclusions(ex),
                         {"udp://a.example:1/announce",
                          "udp://b.example:2/announce"})
        self.assertIn("udp://c.example:3/announce", flagged_exclusions(ex))

    def test_operator_request_removes_the_tracker_from_other_sources(self):
        """RULES 4: the project MUST honour an exclusion request."""
        good = src("good")
        body = ("udp://asked-to-stop.example:1/announce\n"
                "udp://fine.example:2/announce\n")
        ex = collect_exclusions(
            {"bl": "udp://asked-to-stop.example:1/announce # requested by sysadmin\n"})
        agg = aggregate([parse_body(good, body)], {"good": good},
                        exclude=enforced_exclusions(ex))
        urls = {t.url for t in agg.trackers}
        self.assertEqual(urls, {"udp://fine.example:2/announce"})
        self.assertIn("udp://asked-to-stop.example:1/announce", agg.excluded,
                      "a removal must stay explainable, never silent")

    def test_upstream_opinion_does_not_remove_the_tracker(self):
        """The bt.okmp3.ru case: blacklisted upstream, live by two other observers."""
        good = src("good")
        ex = collect_exclusions(
            {"bl": "http://bt.okmp3.ru:2710/announce # fake seeds\n"})
        agg = aggregate([parse_body(good, "http://bt.okmp3.ru:2710/announce\n")],
                        {"good": good}, exclude=enforced_exclusions(ex))
        self.assertEqual([t.url for t in agg.trackers],
                         ["http://bt.okmp3.ru:2710/announce"],
                         "deleting this would destroy the disagreement that "
                         "RULES 3.4 calls the most informative output")


class TestDisplayPathSurvivesTwoDrives(unittest.TestCase):
    """`scripts/generate.py` must not die formatting its own success line.

    Found by the Windows leg of the gate on 2026-08-31: `os.path.relpath`
    raises when the two paths are on different drives, the runner's checkout
    and its scratch directory are on different drives, and generating into
    scratch therefore killed the run AFTER every check had passed.

    ⚠ The failure only ever appears where two drives exist, so this asserts the
    behaviour of the helper rather than of a real run. That is what makes it
    runnable on a POSIX host, where the raising branch cannot be reached.
    """

    def setUp(self):
        spec = importlib.util.spec_from_file_location(
            "_generate",
            os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                         "scripts", "generate.py"))
        self.generate = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.generate)

    def test_a_relative_path_is_used_where_one_exists(self):
        start = os.path.abspath(os.sep + os.path.join("repo"))
        out = os.path.join(start, "out")
        self.assertEqual(self.generate.display_path(out, start), "out")

    def test_a_path_with_no_relative_form_falls_back_to_absolute(self):
        """The cross-drive case, forced rather than waited for."""
        def raises(*_a, **_k):
            raise ValueError("path is on mount 'C:', start on mount 'D:'")

        original = os.path.relpath
        os.path.relpath = raises
        try:
            shown = self.generate.display_path("somewhere", "elsewhere")
        finally:
            os.path.relpath = original
        self.assertTrue(os.path.isabs(shown), shown)
