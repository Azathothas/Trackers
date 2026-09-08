"""T-020, T-023, T-024, T-025: the probe's invariants, without a network.

`test_probe_oracle.py` proves the probe reads real responses correctly. This
file proves the things that must hold *regardless* of what any responder says,
which is why it is exhaustive over the enums rather than example-based: an
invariant checked on three examples is a claim about three examples.

The four that matter, each the `Prove:` clause of an entry:

  T-025  DNS resolution alone never yields `live`.
  T-025  An unmeasurable transport or network never yields `dead`, `live` or
         `degraded`.
  T-024  Every emitted record carries its vantage and a measurement rung.
  T-023  A host resolving into `0200::/7` is yggdrasil and reported
         `unmeasurable`, with the resolved address and the time recorded.

Run:  python3 -m unittest tests.test_probe -v
"""

from __future__ import annotations

import itertools
import os
import socket
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from fake_dns import (DnsBehaviour, FakeDnsServer,  # noqa: E402
                      resolver_for)
from trackers.bep34 import RESOLUTION_CLASSES  # noqa: E402
from trackers.model import (HealthState, Network, Rung,  # noqa: E402
                            Transport, classify_network)
from trackers.normalize import parse  # noqa: E402
from trackers.probe import (ABOUT_US, MIN_SAMPLES_FOR_DEATH,  # noqa: E402
                            DEFAULT_USER_AGENT, Failure, ProbeConfig,
                            classify_network_resolved, health_state, probe,
                            proves_tracker, rung_at_least)
from trackers.vantage import PROBE_VERSION, Vantage, detect  # noqa: E402

NOT_A_STATE_OF_THE_WORLD = {HealthState.DEAD, HealthState.LIVE,
                            HealthState.DEGRADED}


class StateTable(unittest.TestCase):
    """T-025. Exhaustive over the enums, because these are invariants."""

    def test_dns_alone_is_never_live(self):
        """Reaching DNS means a name resolved. It says nothing about a tracker.

        Checked over every transport, network and sample count, because a
        single counterexample anywhere is a dataset that reports resolvable
        domains as working trackers.
        """
        for transport, network, n in itertools.product(
                Transport, Network, (0, 1, 2, 3, 10)):
            with self.subTest(transport=transport, network=network, n=n):
                state = health_state(
                    rung=Rung.DNS, transport=transport, network=network,
                    sample_count=n, success_count=n, failure=Failure.NONE,
                    measurable=True)
                self.assertIsNot(state, HealthState.LIVE)

    def test_connected_alone_is_never_live(self):
        """A TCP handshake proves a socket, not a tracker. Same argument."""
        for transport, n in itertools.product(Transport, (1, 3, 10)):
            with self.subTest(transport=transport, n=n):
                self.assertIsNot(
                    health_state(rung=Rung.CONNECTED, transport=transport,
                                 network=Network.CLEARNET, sample_count=n,
                                 success_count=n),
                    HealthState.LIVE)

    def test_unmeasurable_is_never_dead_live_or_degraded(self):
        """The rule the whole two-axis model exists to enforce.

        Every combination, including ones where the probe claims success:
        `measurable=False` must dominate, because a 'success' against a network
        we cannot reach is a bug in the caller and must not be published as
        health.
        """
        for rung, transport, network, n in itertools.product(
                Rung, Transport, Network, (0, 1, 5)):
            with self.subTest(rung=rung, transport=transport, network=network):
                state = health_state(
                    rung=rung, transport=transport, network=network,
                    sample_count=n, success_count=n, measurable=False)
                self.assertNotIn(state, NOT_A_STATE_OF_THE_WORLD)
                self.assertIs(state, HealthState.UNMEASURABLE)

    def test_no_usable_address_is_unmeasurable_however_it_arrives(self):
        """Whether it comes in as the rung or as the failure, it is the same fact."""
        for transport in Transport:
            with self.subTest(transport=transport):
                self.assertIs(
                    health_state(rung=Rung.NO_USABLE_ADDRESS,
                                 transport=transport, network=Network.CLEARNET,
                                 sample_count=5, success_count=0),
                    HealthState.UNMEASURABLE)
                self.assertIs(
                    health_state(rung=Rung.DNS, transport=transport,
                                 network=Network.CLEARNET, sample_count=5,
                                 success_count=0,
                                 failure=Failure.NO_USABLE_ADDRESS),
                    HealthState.UNMEASURABLE)

    def test_failures_about_us_never_produce_dead(self):
        """`ABOUT_US` is a list of facts about our own position.

        If any of them could yield `dead`, the dataset would be publishing our
        limitations as other people's outages -- which is the single failure
        this project exists to prevent.
        """
        for failure, transport, n in itertools.product(
                sorted(ABOUT_US, key=lambda f: f.value), Transport, (1, 3, 99)):
            with self.subTest(failure=failure, transport=transport, n=n):
                self.assertIsNot(
                    health_state(rung=Rung.DNS, transport=transport,
                                 network=Network.CLEARNET, sample_count=n,
                                 success_count=0, failure=failure),
                    HealthState.DEAD)

    def test_probe_error_is_error_not_dead(self):
        self.assertIs(
            health_state(rung=Rung.NONE, transport=Transport.UDP,
                         network=Network.CLEARNET, sample_count=99,
                         success_count=0, failure=Failure.PROBE_ERROR),
            HealthState.ERROR)

    def test_deadline_is_unknown_not_dead(self):
        """T-029's decision: running out of time is a fact about us."""
        self.assertIs(
            health_state(rung=Rung.DNS, transport=Transport.UDP,
                         network=Network.CLEARNET, sample_count=99,
                         success_count=0, failure=Failure.DEADLINE_EXCEEDED),
            HealthState.UNKNOWN)

    def test_death_needs_enough_samples(self):
        """One failed probe is a moment, not a verdict."""
        for n in range(MIN_SAMPLES_FOR_DEATH):
            with self.subTest(n=n):
                self.assertIs(
                    health_state(rung=Rung.DNS, transport=Transport.UDP,
                                 network=Network.CLEARNET, sample_count=n,
                                 success_count=0, failure=Failure.TIMEOUT),
                    HealthState.UNKNOWN)
        self.assertIs(
            health_state(rung=Rung.DNS, transport=Transport.UDP,
                         network=Network.CLEARNET,
                         sample_count=MIN_SAMPLES_FOR_DEATH, success_count=0,
                         failure=Failure.TIMEOUT),
            HealthState.DEAD)

    def test_intermittent_is_degraded_and_does_not_round(self):
        self.assertIs(
            health_state(rung=Rung.PROTOCOL_VALID, transport=Transport.UDP,
                         network=Network.CLEARNET, sample_count=10,
                         success_count=4),
            HealthState.DEGRADED)

    def test_live_requires_the_proving_rung_for_the_transport(self):
        """UDP proves at connect; HTTP does not prove until tracker-semantic.

        A bencoded blob from an ordinary web server reaches `protocol_valid`
        over HTTP. If that were enough, any server emitting valid bencode would
        be published as a live tracker.
        """
        self.assertIs(
            health_state(rung=Rung.PROTOCOL_VALID, transport=Transport.UDP,
                         network=Network.CLEARNET, sample_count=3,
                         success_count=3),
            HealthState.LIVE)
        self.assertIsNot(
            health_state(rung=Rung.PROTOCOL_VALID, transport=Transport.HTTP,
                         network=Network.CLEARNET, sample_count=3,
                         success_count=3),
            HealthState.LIVE)
        self.assertIs(
            health_state(rung=Rung.TRACKER_SEMANTIC, transport=Transport.HTTP,
                         network=Network.CLEARNET, sample_count=3,
                         success_count=3),
            HealthState.LIVE)

    def test_ws_transports_prove_nothing_at_any_rung(self):
        """T-005: no handshake has been attempted, so no rung means anything yet."""
        for transport, rung in itertools.product(
                (Transport.WS, Transport.WSS), Rung):
            with self.subTest(transport=transport, rung=rung):
                self.assertFalse(proves_tracker(rung, transport))

    def test_no_usable_address_is_not_a_height_on_the_ladder(self):
        for r in Rung:
            with self.subTest(rung=r):
                self.assertFalse(rung_at_least(Rung.NO_USABLE_ADDRESS, r))


class YggdrasilByResolvedAddress(unittest.TestCase):
    """T-023. The bug that was surviving inside the fix for it."""

    #: The real entry from `ngosang/trackerslist`. An ordinary hostname.
    HOSTNAME_FORM = "http://yggtracker.i2p.rocks:80/announce"

    def test_the_url_alone_looks_like_clearnet(self):
        """This is the bug, stated as a test so the fix has something to fix.

        Note `.i2p` here is a *label inside* the name, not a suffix of it, so
        the URL classifier is right to call it clearnet -- and right to be
        insufficient.
        """
        t = parse(self.HOSTNAME_FORM)
        self.assertIs(t.network, Network.CLEARNET)
        self.assertIs(classify_network("yggtracker.i2p.rocks"), Network.CLEARNET)

    def test_a_resolved_yggdrasil_address_reclassifies(self):
        net, was = classify_network_resolved(
            Network.CLEARNET, ["203.0.113.9", "200:1234::1"])
        self.assertIs(net, Network.YGGDRASIL)
        self.assertIs(was, Network.CLEARNET,
                      "the disagreement itself must be recorded, not swallowed")

    def test_an_ordinary_ipv6_address_does_not_reclassify(self):
        net, was = classify_network_resolved(Network.CLEARNET, ["2001:db8::1"])
        self.assertIs(net, Network.CLEARNET)
        self.assertIsNone(was)

    def test_an_explicit_suffix_beats_a_resolved_address(self):
        """`.i2p` and `.onion` are stronger evidence than any address.

        Those names do not resolve in the ordinary DNS at all, so an address
        appearing for one is a resolver doing something unexpected, not a
        reclassification we should trust.
        """
        for network in (Network.I2P, Network.ONION):
            with self.subTest(network=network):
                net, was = classify_network_resolved(network, ["200:1234::1"])
                self.assertIs(net, network)
                self.assertIsNone(was)

    def test_a_yggdrasil_tracker_is_unmeasurable_never_dead(self):
        self.assertIs(
            health_state(rung=Rung.DNS, transport=Transport.HTTP,
                         network=Network.YGGDRASIL, sample_count=99,
                         success_count=0, failure=Failure.TIMEOUT,
                         measurable=False),
            HealthState.UNMEASURABLE)


class OurResolversOpinionIsNotTheNamesProperty(unittest.TestCase):
    """T-037. `getaddrinfo` failing is a fact about this vantage until a
    resolver we chose agrees with it.

    Measured cause (`C-06`, `experiments/30`, run `34210496112`): of the 239
    hosts a runner's own resolver could not answer for, public resolvers answer
    for 3 on `ubuntu-24.04` -- `openbittorrent.com` among them, 5 corpus URLs.
    Publishing those as `dns_failure` would report one of the best-known public
    trackers as gone.

    ⭐ **The probe is driven end to end**, not `second_opinion` alone: the name
    is one this host genuinely cannot resolve, the fallback speaks the real DNS
    wire protocol to a loopback resolver, and the assertions are on the record
    the sweep would commit.
    """

    #: RFC 2606 reserves `.invalid`, so no resolver may answer for it. A host
    #: whose resolver hijacks NXDOMAIN would, and that is what `setUp` checks.
    UNRESOLVABLE = "tracker.t037.invalid"

    def setUp(self):
        try:
            socket.getaddrinfo(self.UNRESOLVABLE, 80, socket.AF_UNSPEC,
                               socket.SOCK_STREAM)
        except OSError:
            return
        self.skipTest(f"this host resolves {self.UNRESOLVABLE}, so the "
                      "divergence cannot be staged here")

    def _probe(self, dns: FakeDnsServer, transport: str = "udp") -> object:
        url = (f"{transport}://{self.UNRESOLVABLE}:6969/announce")
        return probe(parse(url), ProbeConfig(timeout=1.5, retries=0), detect(),
                     resolver=resolver_for(dns))

    def test_a_name_only_the_public_resolver_finds_is_never_dead(self):
        """The `Prove` clause. A new value, in `ABOUT_US`, and no sample count
        reaches `dead`."""
        with FakeDnsServer(addresses={self.UNRESOLVABLE: ["203.0.113.7"]}) as dns:
            r = self._probe(dns)
        self.assertIs(r.failure, Failure.RESOLVER_DIVERGENCE)
        self.assertIn(Failure.RESOLVER_DIVERGENCE, ABOUT_US)
        for n in (1, 3, MIN_SAMPLES_FOR_DEATH, 99):
            with self.subTest(samples=n):
                self.assertIs(
                    health_state(rung=r.rung, transport=r.transport,
                                 network=r.network, sample_count=n,
                                 success_count=0, failure=r.failure),
                    HealthState.UNKNOWN)

    def test_the_record_carries_both_answers_and_not_the_winner(self):
        """Preferring whichever resolver answered would delete the finding."""
        with FakeDnsServer(addresses={self.UNRESOLVABLE: ["203.0.113.7"]}) as dns:
            rec = self._probe(dns).as_record(HealthState.UNKNOWN)
        dns_evidence = rec["dns"]
        self.assertEqual(dns_evidence["class"],
                         "resolves_only_for_the_public_resolver")
        self.assertFalse(dns_evidence["system"]["resolved"])
        self.assertTrue(dns_evidence["system"]["detail"],
                        "what our own resolver said is evidence and is kept")
        self.assertEqual(dns_evidence["public"]["addresses"],
                         {"ipv4": ["203.0.113.7"]})
        self.assertEqual(dns_evidence["resolvers"], ["127.0.0.1"])

    def test_nxdomain_confirmed_by_both_stays_dns_failure(self):
        """The strongest not-resolving signal available here. It is about the
        name, so it is not in `ABOUT_US` -- and three samples still decide
        `dead`, not one.

        ⚠ NXDOMAIN on the address question only. A resolver that answered it
        to the BEP 34 question as well would stop the probe at the consent
        gate, and this path would never run.
        """
        with FakeDnsServer(address_behaviour=DnsBehaviour.NXDOMAIN) as dns:
            r = self._probe(dns)
        self.assertIs(r.failure, Failure.DNS_FAILURE)
        self.assertNotIn(Failure.DNS_FAILURE, ABOUT_US)
        self.assertEqual(r.dns["class"], "gone_nxdomain_confirmed")
        self.assertIs(
            health_state(rung=r.rung, transport=r.transport, network=r.network,
                         sample_count=1, success_count=0, failure=r.failure),
            HealthState.UNKNOWN)

    def test_a_resolver_that_does_not_answer_establishes_nothing(self):
        """SERVFAIL from every resolver we asked. Neither the name nor the
        tracker was established, and that is its own value."""
        with FakeDnsServer(address_behaviour=DnsBehaviour.SERVFAIL) as dns:
            r = self._probe(dns)
        self.assertIs(r.failure, Failure.DNS_UNDETERMINED)
        self.assertIn(Failure.DNS_UNDETERMINED, ABOUT_US)
        self.assertEqual(r.dns["class"], "lookup_failed_undetermined")
        self.assertTrue(r.dns["public"]["failures"],
                        "an empty result with failures recorded is not the "
                        "same as an empty one without them")

    def test_noerror_with_no_address_is_about_the_name(self):
        """The name exists and has no address record of either type."""
        with FakeDnsServer() as dns:
            r = self._probe(dns)
        self.assertIs(r.failure, Failure.DNS_FAILURE)
        self.assertEqual(r.dns["class"], "no_address_records")

    def test_an_unspecified_address_is_not_a_rescue(self):
        """⛔ The finding that made this class exist.

        Measured 2026-09-08 from one Windows 11 host: every corpus host
        `experiments/30` had called `resolves_only_for_the_public_resolver`
        answers `0.0.0.0`, `::` or both. Reading that as our resolver's fault
        would publish eight hosts as `resolver_divergence` when both sides
        agree there is nothing to connect to.
        """
        for served in (["0.0.0.0"], ["::"], ["0.0.0.0", "::"]):
            with self.subTest(served=served):
                with FakeDnsServer(addresses={self.UNRESOLVABLE: served}) as dns:
                    r = self._probe(dns)
                self.assertEqual(r.dns["class"], "resolves_to_an_unusable_address")
                self.assertIs(r.failure, Failure.DNS_FAILURE)
                self.assertIsNot(r.failure, Failure.RESOLVER_DIVERGENCE)

    def test_one_routable_address_among_null_ones_is_still_a_rescue(self):
        """The filter is per address, not per name. A host with a real A record
        and a null AAAA is reachable and must not be written off."""
        with FakeDnsServer(
                addresses={self.UNRESOLVABLE: ["203.0.113.7", "::"]}) as dns:
            r = self._probe(dns)
        self.assertIs(r.failure, Failure.RESOLVER_DIVERGENCE)
        self.assertEqual(r.dns["class"],
                         "resolves_only_for_the_public_resolver")

    def test_nothing_claims_a_scrape_it_never_sent(self):
        """A resolution failure opens no socket, so the record must not say an
        info_hash went out. It said so until T-037 read a committed one."""
        with FakeDnsServer(addresses={self.UNRESOLVABLE: ["203.0.113.7"]}) as dns:
            r = self._probe(dns, transport="http")
        self.assertFalse(r.used_synthetic_infohash)
        self.assertFalse(r.as_record(HealthState.UNKNOWN)
                         ["used_synthetic_infohash"])

    def test_no_second_question_is_asked_when_our_resolver_answers(self):
        """The Decision's load claim, asserted rather than argued.

        520 of 759 corpus hosts resolve here, and the DNS footprint for those
        must not move. Counted at the oracle, which is the only place that can
        see a query that was not sent.
        """
        with FakeDnsServer({"localhost": []}) as dns:
            v = detect()
            if "ipv4" not in v.ip_families:
                self.skipTest("no ipv4 route from this vantage")
            probe(parse("udp://localhost:1/announce"),
                  ProbeConfig(timeout=0.5, retries=0), v,
                  resolver=resolver_for(dns))
            asked = list(dns.questions)
        self.assertEqual([t for _, t in asked], [16],
                         f"an address question was asked for a name that "
                         f"resolved: {asked}")

    def test_one_address_question_per_host_per_run(self):
        """RULES 15.2, on the second lookup as much as the first.

        ⚠ **Two queries per failing host, not one**, because the families are
        asked for separately and knowing a host is IPv6-only is the point of
        asking. The entry's Decision said one; this is the measurement. What
        the cache buys is that a second URL on the same host costs nothing.
        """
        with FakeDnsServer(addresses={self.UNRESOLVABLE: ["203.0.113.7"]}) as dns:
            resolver = resolver_for(dns)
            for port in (6969, 1337):
                probe(parse(f"udp://{self.UNRESOLVABLE}:{port}/announce"),
                      ProbeConfig(timeout=1.5, retries=0), detect(),
                      resolver=resolver)
            addressed = [q for q in dns.questions if q[1] in (1, 28)]
        self.assertEqual(addressed,
                         [(self.UNRESOLVABLE, 1), (self.UNRESOLVABLE, 28)],
                         dns.questions)

    def test_the_http_path_takes_the_same_second_opinion(self):
        """One rule, every door. A control on the UDP path alone leaves the
        HTTP prober reaching the same published field ungated."""
        with FakeDnsServer(addresses={self.UNRESOLVABLE: ["203.0.113.7"]}) as dns:
            r = self._probe(dns, transport="http")
        self.assertIs(r.failure, Failure.RESOLVER_DIVERGENCE)
        self.assertEqual(r.dns["class"], "resolves_only_for_the_public_resolver")

    def test_the_classes_are_the_experiments_own_vocabulary(self):
        """One home for the vocabulary. `experiments/30` published these
        strings and its committed results are read against them."""
        self.assertIn("resolves_only_for_the_public_resolver",
                      RESOLUTION_CLASSES)
        for kind in RESOLUTION_CLASSES:
            with self.subTest(kind=kind):
                self.assertEqual(kind, kind.lower())


class EveryRecordCarriesItsEvidence(unittest.TestCase):
    """T-024, and T-020's `Prove` clause: every result carries a `Rung`."""

    def test_an_unreachable_network_is_recorded_without_being_probed(self):
        """Asking and failing would measure our reachability and publish it as
        the tracker's health."""
        for url in ("http://tracker.i2p/announce",
                    "udp://example.onion:6969/announce"):
            with self.subTest(url=url):
                r = probe(parse(url))
                self.assertIs(r.failure, Failure.UNSUPPORTED)
                self.assertIsInstance(r.rung, Rung)
                rec = r.as_record(HealthState.UNMEASURABLE)
                self.assertEqual(rec["health_state"], "unmeasurable")
                self.assertTrue(rec["measurement_rung"])
                self.assertIn("environment_class", rec["vantage"])
                self.assertIn("probe_version", rec["vantage"])
                self.assertIn("ip_families", rec["vantage"])

    def test_wss_is_unmeasurable_and_not_probed(self):
        r = probe(parse("wss://tracker.example.invalid:443/announce"))
        self.assertIs(r.failure, Failure.UNSUPPORTED)

    def test_the_record_shape_satisfies_the_vantage_gate(self):
        """The exact keys `scripts/check-vantage-metadata.py` requires."""
        r = probe(parse("http://tracker.i2p/announce"))
        rec = r.as_record(HealthState.UNMEASURABLE)
        for key in ("url", "transport", "network", "health_state",
                    "measurement_rung", "vantage"):
            self.assertIn(key, rec)
        for key in ("environment_class", "probe_version", "ip_families"):
            self.assertIn(key, rec["vantage"])

    def test_probe_version_and_code_hash_are_both_present(self):
        """The version is a human statement of intent and can be forgotten.
        The hash cannot be, which is why both are recorded."""
        v = detect()
        d = v.as_dict()
        self.assertEqual(d["probe_version"], PROBE_VERSION)
        self.assertEqual(len(d["probe_code_sha256"]), 64)

    def test_unknown_ipv6_egress_renders_as_a_dash_not_as_false(self):
        """RULES 1.5. An unknown dressed as a measurement contaminates
        everything downstream."""
        v = detect()
        self.assertEqual(v.as_dict()["ipv6_egress"], "-")

    def test_measured_ipv6_egress_false_withholds_the_family(self):
        """`C-04`: a runner has an IPv6 stack AND a route and still cannot get
        a packet out. Believing the routing table over the measurement would
        record healthy trackers as dead."""
        v = detect(ipv6_egress=False)
        self.assertNotIn("ipv6", v.ip_families)
        self.assertFalse(v.can_attempt_ipv6)


class TrackerStatedInterval(unittest.TestCase):
    """`C-65`: the politeness anchor is what the tracker asked for.

    D7 makes the tracker's own stated interval the authority on how often we
    may probe it. `min interval` is the *floor* it asked for and binds harder
    than `interval` -- it is the number an operator would judge us by -- and BEP
    3 spells it with a space while the underscore form occurs in the wild. A
    production client reads both
    (`references/Azathothas__bit-cli/tree/crates/bit-cli-core/src/tracker.rs:739`).

    These exist because removing the underscore spelling failed nothing on
    2026-08-31. A test that does not fail when the claim stops being true is
    not evidence (RULES 1).
    """

    def _classify(self, body: bytes) -> dict:
        from trackers.bencode import classify_body
        return classify_body(body)

    def test_both_spellings_of_the_floor_are_read(self):
        for key in (b"12:min intervali900e", b"12:min_intervali900e"):
            with self.subTest(key=key):
                body = b"d8:intervali1800e" + key + b"5:peers0:e"
                got = self._classify(body)
                self.assertEqual(got["kind"], "tracker_announce_response")
                self.assertEqual(got["interval"], 1800)
                self.assertEqual(
                    got["min_interval"], 900,
                    "a tracker's stated floor must survive classification; "
                    "ignoring it means probing faster than it asked")

    def test_an_absent_floor_is_none_and_never_zero(self):
        """An absence is not a zero (RULES 2), in the inbound direction.

        A `min_interval` of 0 would mean "no floor, probe as fast as you
        like" -- the opposite of what an absent key says, which is "I did not
        state one, use `interval`".
        """
        got = self._classify(b"d8:intervali1800e5:peers0:e")
        self.assertIsNone(got["min_interval"])

    def test_a_non_integer_floor_is_rejected_rather_than_coerced(self):
        """Upstream bodies are hostile input (RULES 5.1)."""
        got = self._classify(b"d8:intervali1800e12:min interval4:soone5:peers0:e")
        self.assertIsNone(got["min_interval"])

    def test_bep_3_spelling_wins_when_a_tracker_sends_both(self):
        """Deterministic, because a tie that resolves by dict order is not
        deterministic (RULES 3.6)."""
        body = (b"d8:intervali1800e12:min intervali900e"
                b"12:min_intervali60e5:peers0:e")
        self.assertEqual(self._classify(body)["min_interval"], 900)


class ProbeConfiguration(unittest.TestCase):
    """The arms of T-012 are a config difference and nothing else."""

    def test_absent_user_agent_sends_no_header(self):
        self.assertNotIn("User-Agent", ProbeConfig(user_agent=None).headers())

    def test_a_user_agent_is_sent_verbatim(self):
        h = ProbeConfig(user_agent="qBittorrent/4.6.5").headers()
        self.assertEqual(h["User-Agent"], "qBittorrent/4.6.5")

    def test_the_four_arms_are_constructible_and_distinct(self):
        """RULES 4.1 withdrew the claim that the descriptive string is correct;
        T-012 measures it instead. That measurement is only possible if the
        identity is a parameter, so this asserts the parameter -- not the prose.

        The arms must reach the wire as four genuinely different requests. If
        two collapsed to the same headers the experiment would report no
        difference between them and be believed.
        """
        arms = {
            "absent": ProbeConfig(user_agent=None),
            "descriptive": ProbeConfig(user_agent=DEFAULT_USER_AGENT),
            "client_like": ProbeConfig(user_agent="qBittorrent/4.6.5"),
            "minimal": ProbeConfig(user_agent="Mozilla/5.0"),
        }
        sent = {name: cfg.headers().get("User-Agent") for name, cfg in arms.items()}
        self.assertIsNone(sent["absent"])
        self.assertEqual(len(set(sent.values())), len(arms),
                         f"two arms send the same User-Agent: {sent}")
        self.assertIn("trackers", DEFAULT_USER_AGENT)

    def test_no_code_path_in_src_can_send_a_udp_scrape(self):
        """T-022's decision, made enforceable rather than remembered.

        `bep15.build_scrape_request` exists, refuses a hash that is not exactly
        20 bytes, and is called by **nothing**. That is deliberate: a UDP
        connect already reaches the rung that proves a tracker
        (`_PROVING_RUNG[UDP] is PROTOCOL_VALID`), so a scrape would cost an
        operator a second round trip and a required info_hash while telling us
        nothing new. RULES 4 says prefer connect over scrape, always.

        ⭐ The capability is kept because refusing a malformed hash is worth
        keeping, and because deleting it would hide the asymmetry `C-50`
        records. This test is what stops it being wired up without the argument
        in D15 being reopened.
        """
        import ast
        src = os.path.join(os.path.dirname(os.path.dirname(
            os.path.abspath(__file__))), "src", "trackers")
        callers = []
        for name in sorted(os.listdir(src)):
            if not name.endswith(".py"):
                continue
            with open(os.path.join(src, name), encoding="utf-8") as fh:
                tree = ast.parse(fh.read(), filename=name)
            for node in ast.walk(tree):
                if not isinstance(node, ast.Call):
                    continue
                fn = node.func
                called = (fn.attr if isinstance(fn, ast.Attribute)
                          else fn.id if isinstance(fn, ast.Name) else "")
                if called == "build_scrape_request":
                    callers.append(f"{name}:{node.lineno}")
        self.assertEqual(callers, [],
                         "a UDP scrape is now reachable from src/. That "
                         "reverses D15 and needs the argument reopened, not a "
                         "test updated.")

    def test_a_synthetic_infohash_is_the_only_one_obtainable(self):
        """RULES 4: never announce with an infohash for real content. The
        enforcement is that no path in the tree can produce one."""
        from trackers.bep15 import INFOHASH_SIZE, synthetic_infohash
        first, second = synthetic_infohash(), synthetic_infohash()
        self.assertEqual(len(first), INFOHASH_SIZE)
        self.assertNotEqual(first, second, "not random per call")

    def test_the_udp_path_sends_connect_and_nothing_else(self):
        """`C-50`: a UDP scrape carries a required info_hash and is strictly
        more intrusive than a connect, which already yields liveness and RTT.

        Asserted against what `probe_udp` actually transmits, not against a
        config flag -- a flag can be set and ignored, and the datagram is the
        thing the tracker sees. This is also the enforcement of RULES 4 on the
        UDP side: the only message that leaves is a 16-byte connect.
        """
        from fake_tracker import Behaviour, FakeUdpTracker
        from trackers.bep15 import CONNECT_REQUEST_SIZE, PROTOCOL_ID
        import struct

        v = detect()
        if "ipv4" not in v.ip_families:
            self.skipTest("no ipv4 route from this vantage")
        with FakeUdpTracker(Behaviour.CORRECT) as fake:
            probe(parse(f"udp://127.0.0.1:{fake.port}/announce"),
                  ProbeConfig(timeout=1.5, retries=0), v)
            sent = list(fake.requests)
        self.assertEqual(len(sent), 1, "more than one datagram per probe")
        self.assertEqual(len(sent[0]), CONNECT_REQUEST_SIZE,
                         "a datagram that is not exactly a 16-byte connect")
        magic, action, _ = struct.unpack(">QII", sent[0])
        self.assertEqual(magic, PROTOCOL_ID)
        self.assertEqual(action, 0, "action 0 is connect; 1 would be an announce")


if __name__ == "__main__":
    unittest.main()
