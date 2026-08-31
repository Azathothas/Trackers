"""T-021: the probe, against every failure mode the oracle can produce.

The point of these tests is not that the probe works. It is that **a broken
probe fails here rather than in production**, where a broken probe marks the
whole dataset dead and every number in the report stays internally consistent
while it does so.

The negative controls are the load-bearing half. `test_html_200_is_not_a_tracker`
and its siblings are the ones that must fail the build: a probe that calls an
ordinary web server a tracker has reproduced the anti-pattern RULES 11 names,
and the resulting dataset is confidently wrong rather than merely incomplete.

No network. Everything binds 127.0.0.1:0.

Run:  python3 -m unittest tests.test_probe_oracle -v
"""

from __future__ import annotations

import os
import socket
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from fake_tracker import (Behaviour, FakeHttpTracker,  # noqa: E402
                          FakeUdpTracker, looks_like_a_torrent_client)
from trackers.model import (HealthState, Network, Rung,  # noqa: E402
                            Tracker, Transport)
from trackers.normalize import parse  # noqa: E402
from trackers.probe import (Failure, ProbeConfig, health_state,  # noqa: E402
                            probe, probe_http, probe_udp, proves_tracker)
from trackers.vantage import Vantage, detect  # noqa: E402

FAST = ProbeConfig(timeout=1.5, retries=0)


def loopback_vantage() -> Vantage:
    """A vantage that permits IPv4, so loopback probes are actually attempted.

    `detect()` on a host with no IPv4 default route would return no usable
    families and every test below would pass vacuously by returning
    `unmeasurable`. Constructing it explicitly makes the tests test the probe.
    """
    v = detect()
    if "ipv4" in v.ip_families:
        return v
    return Vantage(
        environment_class=v.environment_class, probe_version=v.probe_version,
        probe_code_sha256=v.probe_code_sha256, ip_families=("ipv4",),
        ip_families_method="forced ipv4 for loopback tests",
        ipv6_stack_present=v.ipv6_stack_present,
        ipv6_route_present=v.ipv6_route_present)


def udp_tracker(port: int) -> Tracker:
    return parse(f"udp://127.0.0.1:{port}/announce")


def http_tracker(port: int) -> Tracker:
    return parse(f"http://127.0.0.1:{port}/announce")


class UdpOracle(unittest.TestCase):
    """BEP 15, against a responder we control."""

    def setUp(self):
        self.v = loopback_vantage()

    def test_correct_connect_is_recognised(self):
        with FakeUdpTracker(Behaviour.CORRECT) as fake:
            r = probe_udp(udp_tracker(fake.port), FAST, self.v)
        self.assertTrue(r.ok, r.detail)
        self.assertIs(r.rung, Rung.PROTOCOL_VALID)
        self.assertIsNotNone(r.rtt_ms)

    def test_positive_control_actually_received_our_datagram(self):
        """Distinguishes 'the probe sent nothing' from 'we ignored what it sent'.

        Without this, a probe that never transmits and a probe whose datagram
        is malformed both look like a plain failure.
        """
        with FakeUdpTracker(Behaviour.CORRECT) as fake:
            probe_udp(udp_tracker(fake.port), FAST, self.v)
            seen = fake.seen
        self.assertEqual(seen, 1, "the oracle saw no correctly-magicked datagram")

    def test_spoofed_transaction_id_is_rejected(self):
        """The security property, not a style preference.

        UDP is unauthenticated. A probe that skips the transaction-id check can
        be told any tracker is alive by any host on the internet that answers
        first.
        """
        with FakeUdpTracker(Behaviour.WRONG_TRANSACTION_ID) as fake:
            r = probe_udp(udp_tracker(fake.port), FAST, self.v)
        self.assertFalse(r.ok)
        self.assertIn("transaction id mismatch", r.detail)

    def test_bep15_error_response_is_a_live_tracker(self):
        """An in-protocol refusal is *stronger* evidence of life than silence."""
        with FakeUdpTracker(Behaviour.BEP15_ERROR) as fake:
            r = probe_udp(udp_tracker(fake.port), FAST, self.v)
        self.assertTrue(r.ok, r.detail)
        self.assertIn("BEP15 error response", r.detail)

    def test_truncated_datagram_is_not_a_tracker(self):
        with FakeUdpTracker(Behaviour.TRUNCATED) as fake:
            r = probe_udp(udp_tracker(fake.port), FAST, self.v)
        self.assertFalse(r.ok)
        self.assertIn("short response", r.detail)

    def test_silence_is_a_timeout_and_not_death(self):
        with FakeUdpTracker(Behaviour.TIMEOUT) as fake:
            r = probe_udp(udp_tracker(fake.port), FAST, self.v)
        self.assertFalse(r.ok)
        self.assertIs(r.failure, Failure.TIMEOUT)
        state = health_state(rung=r.rung, transport=Transport.UDP,
                             network=Network.CLEARNET, sample_count=1,
                             success_count=0, failure=r.failure)
        self.assertIs(state, HealthState.UNKNOWN,
                      "one timeout is not enough to say dead")

    def test_closed_port_is_not_recorded_as_dns_failure(self):
        """A refusal and a name that does not exist are different facts."""
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
        s.close()
        r = probe_udp(udp_tracker(port), ProbeConfig(timeout=0.5, retries=0), self.v)
        self.assertFalse(r.ok)
        self.assertNotEqual(r.failure, Failure.DNS_FAILURE)


class HttpOracle(unittest.TestCase):
    """The discriminator between a tracker and a web server."""

    def setUp(self):
        self.v = loopback_vantage()

    def test_bencoded_scrape_is_a_tracker(self):
        with FakeHttpTracker(Behaviour.CORRECT) as fake:
            r = probe_http(http_tracker(fake.port), FAST, self.v)
        self.assertTrue(r.ok, r.detail)
        self.assertIs(r.rung, Rung.TRACKER_SEMANTIC)
        self.assertEqual(r.classification["kind"], "tracker_scrape_response")

    def test_both_failure_key_spellings_are_accepted(self):
        """BEP 3 spells it `failure reason`; BEP 48 spells it `failure_reason`.

        A parser that accepts only one misreads whichever half of the trackers
        uses the other.
        """
        for behaviour, spelling in (
            (Behaviour.BENCODE_FAILURE, "failure reason"),
            (Behaviour.BENCODE_FAILURE_UNDERSCORE, "failure_reason"),
        ):
            with self.subTest(spelling=spelling):
                with FakeHttpTracker(behaviour) as fake:
                    r = probe_http(http_tracker(fake.port), FAST, self.v)
                self.assertTrue(r.ok, r.detail)
                self.assertEqual(r.classification["failure_key_spelling"], spelling)

    # --- the negative controls -------------------------------------------
    def test_html_200_is_not_a_tracker(self):
        """THE load-bearing test. A green build with this failing is worthless."""
        with FakeHttpTracker(Behaviour.HTML) as fake:
            r = probe_http(http_tracker(fake.port), FAST, self.v)
        self.assertFalse(r.ok, "an HTTP 200 serving HTML was called a tracker")
        self.assertIs(r.failure, Failure.NOT_A_TRACKER)
        self.assertEqual(r.classification["kind"], "html")

    def test_empty_200_is_not_a_tracker(self):
        with FakeHttpTracker(Behaviour.EMPTY_200) as fake:
            r = probe_http(http_tracker(fake.port), FAST, self.v)
        self.assertFalse(r.ok)
        self.assertEqual(r.classification["kind"], "empty")

    def test_malformed_bencode_is_not_a_tracker(self):
        with FakeHttpTracker(Behaviour.MALFORMED_BENCODE) as fake:
            r = probe_http(http_tracker(fake.port), FAST, self.v)
        self.assertFalse(r.ok)
        self.assertIn(r.classification["kind"], ("not_bencode", "html"))

    def test_truncated_bencode_is_reported_as_truncated(self):
        """Distinguishable from malformed: the decoder says the declared string
        length runs past the end of the input."""
        with FakeHttpTracker(Behaviour.TRUNCATED) as fake:
            r = probe_http(http_tracker(fake.port), FAST, self.v)
        self.assertFalse(r.ok)
        self.assertIn("runs past end of input", r.classification["detail"])

    def test_close_midway_is_truncation_and_never_death(self):
        """A cut-off answer is a transport fault, not a missing tracker.

        `NOT_A_TRACKER` would be wrong here and consequentially so: a web
        server on the tracker's URL is evidence the tracker is gone, whereas
        an answer that stopped mid-value is evidence that something *was*
        answering. Publishing the second as the first turns a network fault
        into a dead tracker.
        """
        with FakeHttpTracker(Behaviour.CLOSE_MIDWAY) as fake:
            r = probe_http(http_tracker(fake.port), FAST, self.v)
        self.assertFalse(r.ok)
        self.assertIs(r.failure, Failure.TRUNCATED_RESPONSE)
        self.assertIn("runs past end of input", r.detail)
        state = health_state(rung=r.rung, transport=Transport.HTTP,
                             network=Network.CLEARNET, sample_count=99,
                             success_count=0, failure=r.failure)
        self.assertIs(state, HealthState.DEGRADED)

    # --- refusals are facts about us -------------------------------------
    def test_403_never_contributes_to_dead(self):
        """T-012's whole premise. A refusal may be about our User-Agent."""
        with FakeHttpTracker(Behaviour.HTTP_403) as fake:
            r = probe_http(http_tracker(fake.port), FAST, self.v)
        self.assertIs(r.failure, Failure.BLOCKED_BY_POLICY)
        state = health_state(rung=r.rung, transport=Transport.HTTP,
                             network=Network.CLEARNET, sample_count=99,
                             success_count=0, failure=r.failure)
        self.assertIsNot(state, HealthState.DEAD)
        self.assertIs(state, HealthState.UNKNOWN)

    def test_429_means_very_much_alive(self):
        with FakeHttpTracker(Behaviour.HTTP_429) as fake:
            r = probe_http(http_tracker(fake.port), FAST, self.v)
        self.assertIs(r.failure, Failure.RATE_LIMITED)
        state = health_state(rung=r.rung, transport=Transport.HTTP,
                             network=Network.CLEARNET, sample_count=99,
                             success_count=0, failure=r.failure)
        self.assertIs(state, HealthState.DEGRADED)

    def test_timeout_does_not_hang_the_suite(self):
        with FakeHttpTracker(Behaviour.TIMEOUT) as fake:
            r = probe_http(http_tracker(fake.port),
                           ProbeConfig(timeout=1.0, retries=0), self.v)
        self.assertFalse(r.ok)
        self.assertIn(r.failure, (Failure.TIMEOUT, Failure.RESET))


class UserAgentArms(unittest.TestCase):
    """T-012's instrument needs a positive control before it measures anything.

    An instrument that cannot detect a User-Agent block in a case where one is
    known to exist has not measured its absence anywhere else.
    """

    def setUp(self):
        self.v = loopback_vantage()

    def test_the_oracle_can_block_on_user_agent(self):
        with FakeHttpTracker(Behaviour.BLOCK_UNKNOWN_UA) as fake:
            descriptive = probe_http(
                http_tracker(fake.port),
                ProbeConfig(timeout=1.5, retries=0,
                            user_agent="trackers/0.1 (+https://example.invalid)"),
                self.v)
            client_like = probe_http(
                http_tracker(fake.port),
                ProbeConfig(timeout=1.5, retries=0,
                            user_agent="qBittorrent/4.6.5"),
                self.v)
        self.assertIs(descriptive.failure, Failure.BLOCKED_BY_POLICY,
                      "the block arm was not blocked; the control is broken")
        self.assertTrue(client_like.ok,
                        "the client-like arm was blocked; the control is broken")

    def test_absent_user_agent_is_a_distinct_arm(self):
        """`user_agent=None` must send no header at all, not the string 'None'."""
        with FakeHttpTracker(Behaviour.CORRECT) as fake:
            probe_http(http_tracker(fake.port),
                       ProbeConfig(timeout=1.5, retries=0, user_agent=None),
                       self.v)
            seen = [r["user_agent"] for r in fake.requests]
        self.assertEqual(len(seen), 1)
        # urllib supplies its own default when we send none; what must never
        # happen is our own string, or the literal "None", going out.
        self.assertNotIn("trackers", seen[0])
        self.assertNotEqual(seen[0], "None")

    def test_what_was_sent_is_recorded_on_the_result(self):
        """An arm is only reconstructable if the result says what it sent."""
        ua = "Transmission/4.0.5"
        with FakeHttpTracker(Behaviour.CORRECT) as fake:
            r = probe_http(http_tracker(fake.port),
                           ProbeConfig(timeout=1.5, retries=0, user_agent=ua),
                           self.v)
            seen = fake.requests[0]["user_agent"]
        self.assertEqual(r.sent_user_agent, ua)
        self.assertEqual(seen, ua)

    def test_the_marker_list_matches_itself(self):
        self.assertTrue(looks_like_a_torrent_client("qBittorrent/4.6.5"))
        self.assertTrue(looks_like_a_torrent_client("Deluge 2.1.1"))
        self.assertFalse(looks_like_a_torrent_client("curl/8.5.0"))
        self.assertFalse(looks_like_a_torrent_client(""))


class ConcurrentServers(unittest.TestCase):
    """The bug fixed on the way in from the seeds.

    Both seeds selected behaviour with a *class* attribute, which two live
    servers silently share. This test fails against the seed implementation and
    passes against the promoted one.
    """

    def test_two_servers_keep_their_own_behaviour(self):
        v = loopback_vantage()
        with FakeHttpTracker(Behaviour.CORRECT) as good, \
             FakeHttpTracker(Behaviour.HTML) as bad:
            r_good = probe_http(http_tracker(good.port), FAST, v)
            r_bad = probe_http(http_tracker(bad.port), FAST, v)
        self.assertTrue(r_good.ok, "the good server was contaminated by the bad one")
        self.assertFalse(r_bad.ok, "the bad server was contaminated by the good one")


class ScrapeEthics(unittest.TestCase):
    """T-022 and RULES 4, asserted rather than remembered."""

    def test_no_announce_builder_exists(self):
        """RULES 4 is a property of the code: there is nothing here to call."""
        from trackers import bep15
        builders = [n for n in dir(bep15) if n.startswith("build_")]
        self.assertEqual(sorted(builders),
                         ["build_connect_request", "build_scrape_request"],
                         "a message builder was added to bep15; if it builds an "
                         "announce, RULES 4 stops being structural")

    def test_udp_scrape_refuses_a_wrong_length_infohash(self):
        from trackers.bep15 import Bep15Error, build_scrape_request
        with self.assertRaises(Bep15Error):
            build_scrape_request(1, 2, [b"too-short"])
        with self.assertRaises(Bep15Error):
            build_scrape_request(1, 2, [])

    def test_synthetic_infohash_is_random_and_correct_length(self):
        from trackers.bep15 import INFOHASH_SIZE, synthetic_infohash
        a, b = synthetic_infohash(), synthetic_infohash()
        self.assertEqual(len(a), INFOHASH_SIZE)
        self.assertNotEqual(a, b, "a per-run synthetic hash must not be constant")

    def test_http_probe_records_that_the_infohash_was_synthetic(self):
        with FakeHttpTracker(Behaviour.CORRECT) as fake:
            r = probe_http(http_tracker(fake.port), FAST, loopback_vantage())
        self.assertTrue(r.used_synthetic_infohash)
        rec = r.as_record(HealthState.LIVE)
        self.assertTrue(rec["used_synthetic_infohash"])


if __name__ == "__main__":
    unittest.main()
