"""T-032: the operator's refusal is read, and it is read before anything is sent.

RULES 4 is absolute and its cost is borne by somebody who explicitly asked not
to be contacted, so these tests are about *conduct*, not about parsing. The
question every one of them asks is the same: did a packet leave.

⭐ **The load-bearing test is `test_a_denial_sends_nothing`**, and it is the
only shape that can prove the rule. A test that checks the returned `Failure`
proves what the probe *said*; a real tracker on loopback that recorded no
request proves what the probe *did*. The two come apart exactly when the gate
is placed after the socket, which is the defect this entry exists to prevent.

⚠ **`localhost` is used deliberately**, not `127.0.0.1`. BEP 34 is keyed on a
hostname, so an address literal is short-circuited before any lookup -- correct
behaviour, and it would make every test here pass without the gate ever
running. `localhost` resolves to the loopback address on every supported host,
which is what lets one tracker be both nameable in DNS and reachable.

The DNS side is the oracle in `fake_dns.py`: the production wire code sends a
real query to a real responder on loopback and parses a real answer. Nothing
here touches the network.

Run:  python3 -m unittest tests.test_bep34 -v
"""

from __future__ import annotations

import os
import socket
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from fake_dns import DnsBehaviour, FakeDnsServer  # noqa: E402
from fake_tracker import Behaviour, FakeHttpTracker, FakeUdpTracker  # noqa: E402
from trackers.bep34 import (MARKER, Bep34Config, Decision,  # noqa: E402
                            MAX_WORDS, Resolver, parse_record,
                            protocol_for_transport)
from trackers.model import HealthState, Rung, Transport  # noqa: E402
from trackers.normalize import parse  # noqa: E402
from trackers.probe import (ABOUT_US, Failure, ProbeConfig,  # noqa: E402
                            effective_port, health_state, probe, probe_http,
                            probe_udp)
from trackers.vantage import Vantage, detect  # noqa: E402

FAST = ProbeConfig(timeout=1.5, retries=0)

#: A denial, spelled both ways the specification allows.
DENY_BARE = MARKER
DENY_READABLE = f"{MARKER} DENY ALL"


def loopback_vantage() -> Vantage:
    """A vantage that permits IPv4, so a loopback probe is actually attempted."""
    v = detect()
    if "ipv4" in v.ip_families:
        return v
    return Vantage(
        environment_class=v.environment_class, probe_version=v.probe_version,
        probe_code_sha256=v.probe_code_sha256, ip_families=("ipv4",),
        ip_families_method="forced ipv4 for loopback tests",
        ipv6_stack_present=v.ipv6_stack_present,
        ipv6_route_present=v.ipv6_route_present)


def resolver_for(dns: FakeDnsServer, timeout: float = 1.5) -> Resolver:
    return Resolver(Bep34Config(resolvers=("127.0.0.1",), port=dns.port,
                                timeout=timeout))


def localhost_resolves() -> bool:
    try:
        socket.getaddrinfo("localhost", 80, socket.AF_INET, socket.SOCK_STREAM)
        return True
    except OSError:
        return False


class RecordParsing(unittest.TestCase):
    """Every example the specification gives, and the ones it does not.

    Quoted from `bep_0034.html` version `9c5c1dd1b372`.
    """

    def test_the_three_worked_examples_from_the_specification(self):
        bare = parse_record(DENY_BARE)
        self.assertEqual(bare.endpoints, ())
        self.assertTrue(bare.denies_everything)

        readable = parse_record(DENY_READABLE)
        self.assertEqual(readable.endpoints, (),
                         "DENY and ALL are unrecognised words and are ignored")
        self.assertTrue(readable.denies_everything)

        both = parse_record(f"{MARKER} UDP:1337 TCP:80")
        self.assertEqual(both.endpoints, (("udp", 1337), ("tcp", 80)))
        self.assertFalse(both.denies_everything)

    def test_preference_order_is_the_operators_order(self):
        """The specification says preferred trackers come first, so the order
        is the operator's statement and is not sorted away."""
        self.assertEqual(parse_record(f"{MARKER} TCP:80 UDP:1337").endpoints,
                         (("tcp", 80), ("udp", 1337)))

    def test_a_record_is_case_sensitive(self):
        """The contents are case-sensitive. Reading `bittorrent` as the marker
        would invent a refusal the operator did not publish."""
        for text in ("bittorrent UDP:80", "BitTorrent UDP:80",
                     f"{MARKER} udp:80"):
            with self.subTest(text=text):
                record = parse_record(text)
                if record is None:
                    continue
                self.assertEqual(record.endpoints, (),
                                 "a lowercase protocol word is not a UDP: word")

    def test_the_marker_must_be_the_first_word(self):
        self.assertIsNone(parse_record(f"v=spf1 {MARKER} UDP:80"))
        self.assertIsNone(parse_record("not a bittorrent record"))
        self.assertIsNone(parse_record(""))

    def test_malformed_ports_are_ignored_never_guessed(self):
        record = parse_record(
            f"{MARKER} UDP:abc TCP: UDP:0 TCP:65536 UDP:+80 TCP: 80 UDP:1337")
        self.assertEqual(record.endpoints, (("udp", 1337),))

    def test_a_repeated_endpoint_is_one_endpoint(self):
        self.assertEqual(
            parse_record(f"{MARKER} UDP:80 UDP:80 UDP:80").endpoints,
            (("udp", 80),))

    def test_an_over_long_record_refuses_rather_than_truncating(self):
        """Truncating would deny endpoints the operator advertised and call it
        a measurement. Refusing makes it `UNDETERMINED`, which skips."""
        with self.assertRaises(ValueError):
            parse_record(MARKER + " UDP:1" * (MAX_WORDS + 1))

    def test_transports_map_onto_transport_layer_protocols(self):
        """`https` is advertised by `TCP:443`; there is no `HTTPS:` word."""
        self.assertEqual(protocol_for_transport("udp"), "udp")
        for scheme in ("http", "https", "ws", "wss"):
            self.assertEqual(protocol_for_transport(scheme), "tcp")


class DnsClient(unittest.TestCase):
    """The wire code, against a responder we control (`fake_dns.py`)."""

    def test_a_record_is_read_over_the_wire(self):
        with FakeDnsServer({"tracker.example": [f"{MARKER} UDP:1337"]}) as dns:
            r = resolver_for(dns).consult("tracker.example", "udp", 1337)
        self.assertIs(r.decision, Decision.ALLOW)
        self.assertEqual(r.record.endpoints, (("udp", 1337),))

    def test_an_unadvertised_port_on_an_advertising_host_is_denied(self):
        """The record is an allow-list, not a deny-list: a port it does not
        name is denied by the record saying nothing about it."""
        with FakeDnsServer({"tracker.example": [f"{MARKER} UDP:1337"]}) as dns:
            resolver = resolver_for(dns)
            self.assertIs(resolver.consult("tracker.example", "udp", 6969).decision,
                          Decision.DENY)
            self.assertIs(resolver.consult("tracker.example", "tcp", 1337).decision,
                          Decision.DENY, "UDP:1337 says nothing about TCP:1337")

    def test_both_spellings_of_a_denial_deny(self):
        for text in (DENY_BARE, DENY_READABLE):
            with self.subTest(record=text):
                with FakeDnsServer({"tracker.example": [text]}) as dns:
                    r = resolver_for(dns).consult("tracker.example", "udp", 6969)
                self.assertIs(r.decision, Decision.DENY)
                self.assertTrue(r.record.denies_everything)

    def test_a_host_with_no_record_is_allowed(self):
        with FakeDnsServer({"tracker.example": ["v=spf1 -all"]}) as dns:
            r = resolver_for(dns).consult("tracker.example", "udp", 6969)
        self.assertIs(r.decision, Decision.ALLOW)
        self.assertIsNone(r.record)

    def test_a_multi_string_txt_record_is_concatenated(self):
        """A TXT value over 255 bytes is split across character-strings. A
        reader that keeps only the first parses a different allow-list than the
        one published."""
        padding = "X" * 300           # forces a split, and is an ignored word
        text = f"{MARKER} {padding} UDP:1337"
        with FakeDnsServer({"tracker.example": [text]}) as dns:
            r = resolver_for(dns).consult("tracker.example", "udp", 1337)
        self.assertIs(r.decision, Decision.ALLOW)
        self.assertEqual(r.record.endpoints, (("udp", 1337),))

    def test_a_truncated_answer_is_retried_over_tcp(self):
        """The case where an opt-out fails silently: a denial too long for a
        datagram must not arrive as half a record."""
        with FakeDnsServer({"tracker.example": [DENY_BARE]},
                           behaviour=DnsBehaviour.TRUNCATE_UDP) as dns:
            r = resolver_for(dns).consult("tracker.example", "udp", 6969)
        self.assertIs(r.decision, Decision.DENY)

    def test_nxdomain_is_a_definitive_absence_of_a_record(self):
        with FakeDnsServer(behaviour=DnsBehaviour.NXDOMAIN) as dns:
            r = resolver_for(dns).consult("nothing.example", "udp", 6969)
        self.assertIs(r.decision, Decision.ALLOW)

    def test_every_way_of_not_answering_is_undetermined_not_consent(self):
        """`code.md`'s worst-case rule. Being wrong here costs a row of data;
        being wrong the other way costs somebody who refused us."""
        for behaviour in (DnsBehaviour.SERVFAIL, DnsBehaviour.SILENT,
                          DnsBehaviour.GARBAGE, DnsBehaviour.WRONG_ID,
                          DnsBehaviour.POINTER_LOOP):
            with self.subTest(behaviour=behaviour):
                with FakeDnsServer({"tracker.example": [f"{MARKER} UDP:6969"]},
                                   behaviour=behaviour) as dns:
                    r = resolver_for(dns, timeout=0.4).consult(
                        "tracker.example", "udp", 6969)
                self.assertIs(r.decision, Decision.UNDETERMINED,
                              f"{behaviour.value} was read as permission")

    def test_no_resolver_reachable_is_undetermined(self):
        """A closed port stands in for a resolver that is simply not there."""
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
        s.close()
        resolver = Resolver(Bep34Config(resolvers=("127.0.0.1",), port=port,
                                        timeout=0.4))
        self.assertIs(resolver.consult("tracker.example", "udp", 6969).decision,
                      Decision.UNDETERMINED)

    def test_conflicting_records_are_undetermined_not_first_wins(self):
        """DNS does not order an answer set, so believing the first record
        would make the verdict depend on send order (RULES 3.6)."""
        with FakeDnsServer({"tracker.example": [DENY_BARE, f"{MARKER} UDP:80"]}) as dns:
            r = resolver_for(dns).consult("tracker.example", "udp", 80)
        self.assertIs(r.decision, Decision.UNDETERMINED)

    def test_identical_duplicate_records_are_not_a_conflict(self):
        with FakeDnsServer({"tracker.example": [DENY_BARE, DENY_BARE]}) as dns:
            r = resolver_for(dns).consult("tracker.example", "udp", 80)
        self.assertIs(r.decision, Decision.DENY)

    def test_one_question_per_host_per_run(self):
        """RULES 15.2. Without the cache a host with forty URLs is asked forty
        times, which is load generated for nothing."""
        with FakeDnsServer({"tracker.example": [f"{MARKER} UDP:1337"]}) as dns:
            resolver = resolver_for(dns)
            for port in (1337, 6969, 80, 443):
                resolver.consult("tracker.example", "udp", port)
            asked = list(dns.queries)
        self.assertEqual(asked.count("tracker.example"), 1, asked)

    def test_an_address_literal_asks_nothing(self):
        """BEP 34 is keyed on a hostname. There is no name to ask about, so no
        query is sent -- which is also what keeps this suite offline."""
        with FakeDnsServer() as dns:
            r = resolver_for(dns).consult("127.0.0.1", "udp", 6969)
            self.assertEqual(dns.queries, [])
        self.assertIs(r.decision, Decision.ALLOW)
        self.assertIn("IP literal", r.detail)


class TheGateStopsThePacket(unittest.TestCase):
    """T-032's `Prove` clause: a denial skips the tracker without a socket."""

    def setUp(self):
        if not localhost_resolves():
            self.skipTest("localhost does not resolve to an IPv4 address here")
        self.v = loopback_vantage()

    def test_a_denial_sends_nothing(self):
        """The test that matters. A real tracker on loopback records every
        datagram it receives; after a denial it must have recorded none."""
        with FakeDnsServer({"localhost": [DENY_READABLE]}) as dns, \
                FakeUdpTracker(Behaviour.CORRECT) as fake:
            tracker = parse(f"udp://localhost:{fake.port}/announce")
            r = probe_udp(tracker, FAST, self.v, resolver=resolver_for(dns))
            received = list(fake.requests)
        self.assertIs(r.failure, Failure.EXCLUDED_BY_OPERATOR)
        self.assertIs(r.rung, Rung.NONE)
        self.assertEqual(received, [], "a datagram was sent to a host that refused")

    def test_the_same_tracker_is_probed_when_the_record_permits_it(self):
        """The positive control. Without it, a gate that refused everything --
        or a fake tracker that never records anything -- would pass the test
        above and prove nothing."""
        with FakeUdpTracker(Behaviour.CORRECT) as fake:
            with FakeDnsServer({"localhost": [f"{MARKER} UDP:{fake.port}"]}) as dns:
                tracker = parse(f"udp://localhost:{fake.port}/announce")
                r = probe_udp(tracker, FAST, self.v, resolver=resolver_for(dns))
                received = list(fake.requests)
        self.assertTrue(r.ok, r.detail)
        self.assertEqual(len(received), 1, "the permitted probe never happened")

    def test_http_is_gated_by_the_same_check(self):
        """`probe_http` is a public entry point that opens its own socket. A
        control on one path into an action and not its sibling is the most
        recurring hole there is."""
        with FakeDnsServer({"localhost": [DENY_BARE]}) as dns, \
                FakeHttpTracker(Behaviour.CORRECT) as fake:
            tracker = parse(f"http://localhost:{fake.port}/announce")
            r = probe_http(tracker, FAST, self.v, resolver=resolver_for(dns))
            received = list(fake.requests)
        self.assertIs(r.failure, Failure.EXCLUDED_BY_OPERATOR)
        self.assertEqual(received, [], "an HTTP request reached a host that refused")
        self.assertFalse(r.used_synthetic_infohash,
                         "nothing was sent, so no infohash was sent either")

    def test_probe_dispatches_through_the_gate_too(self):
        with FakeDnsServer({"localhost": [DENY_BARE]}) as dns, \
                FakeUdpTracker(Behaviour.CORRECT) as fake:
            tracker = parse(f"udp://localhost:{fake.port}/announce")
            r = probe(tracker, FAST, self.v, resolver=resolver_for(dns))
            received = list(fake.requests)
        self.assertIs(r.failure, Failure.EXCLUDED_BY_OPERATOR)
        self.assertEqual(received, [])

    def test_an_undetermined_lookup_also_sends_nothing(self):
        """A DNS failure is not consent."""
        with FakeDnsServer(behaviour=DnsBehaviour.SERVFAIL) as dns, \
                FakeUdpTracker(Behaviour.CORRECT) as fake:
            tracker = parse(f"udp://localhost:{fake.port}/announce")
            r = probe_udp(tracker, FAST, self.v, resolver=resolver_for(dns, 0.4))
            received = list(fake.requests)
        self.assertIs(r.failure, Failure.EXCLUSION_UNDETERMINED)
        self.assertEqual(received, [])

    def test_the_gate_checks_the_port_the_probe_would_contact(self):
        """A gate that checks a different port than the prober opens is
        decorative. The record advertises a port the tracker is not on."""
        with FakeUdpTracker(Behaviour.CORRECT) as fake:
            other = fake.port + 1 if fake.port < 65535 else fake.port - 1
            with FakeDnsServer({"localhost": [f"{MARKER} UDP:{other}"]}) as dns:
                tracker = parse(f"udp://localhost:{fake.port}/announce")
                r = probe_udp(tracker, FAST, self.v, resolver=resolver_for(dns))
                received = list(fake.requests)
        self.assertIs(r.failure, Failure.EXCLUDED_BY_OPERATOR)
        self.assertEqual(received, [])


class WhatTheRecordSays(unittest.TestCase):
    """The refusal is a returned value carrying its reason (RULES 3.10)."""

    def setUp(self):
        if not localhost_resolves():
            self.skipTest("localhost does not resolve to an IPv4 address here")
        self.v = loopback_vantage()

    def test_a_refusal_is_never_death_and_never_a_measurement(self):
        for failure in (Failure.EXCLUDED_BY_OPERATOR,
                        Failure.EXCLUSION_UNDETERMINED):
            with self.subTest(failure=failure):
                self.assertIn(failure, ABOUT_US)
                for n in (1, 3, 99):
                    self.assertIsNot(
                        health_state(rung=Rung.NONE, transport=Transport.UDP,
                                     network=parse("udp://a.example:1/announce").network,
                                     sample_count=n, success_count=0,
                                     failure=failure),
                        HealthState.DEAD)

    def test_an_operator_refusal_is_unmeasurable_and_ours_is_unknown(self):
        """Both mean nothing was learned. They differ in whose decision it was,
        which is what the `failure` field carries."""
        common = dict(rung=Rung.NONE, transport=Transport.UDP,
                      network=parse("udp://a.example:1/announce").network,
                      sample_count=99, success_count=0)
        self.assertIs(health_state(failure=Failure.EXCLUDED_BY_OPERATOR, **common),
                      HealthState.UNMEASURABLE)
        self.assertIs(health_state(failure=Failure.EXCLUSION_UNDETERMINED, **common),
                      HealthState.UNKNOWN)

    def test_the_record_carries_what_was_asked_and_who_answered(self):
        with FakeDnsServer({"localhost": [f"{MARKER} UDP:1"]}) as dns, \
                FakeUdpTracker(Behaviour.CORRECT) as fake:
            tracker = parse(f"udp://localhost:{fake.port}/announce")
            r = probe_udp(tracker, FAST, self.v, resolver=resolver_for(dns))
        record = r.as_record(HealthState.UNMEASURABLE)
        self.assertEqual(record["bep34"]["decision"], "deny")
        self.assertEqual(record["bep34"]["record"], f"{MARKER} UDP:1")
        self.assertEqual(record["bep34"]["advertised"], ["udp:1"])
        self.assertEqual(record["bep34"]["resolver"], "127.0.0.1")

    def test_an_allowed_probe_records_that_it_asked(self):
        """"We asked and were permitted" is the evidence the gate ran at all."""
        with FakeUdpTracker(Behaviour.CORRECT) as fake:
            with FakeDnsServer({"localhost": [f"{MARKER} UDP:{fake.port}"]}) as dns:
                tracker = parse(f"udp://localhost:{fake.port}/announce")
                r = probe_udp(tracker, FAST, self.v, resolver=resolver_for(dns))
        self.assertEqual(r.bep34["decision"], "allow")
        self.assertEqual(r.as_record(HealthState.LIVE)["bep34"]["decision"], "allow")


class EffectivePort(unittest.TestCase):
    """One definition of the port, read by the gate and by both probers."""

    def test_an_explicit_port_is_kept(self):
        self.assertEqual(effective_port(parse("udp://a.example:6969/announce")), 6969)
        self.assertEqual(effective_port(parse("https://a.example:8443/announce")), 8443)

    def test_the_defaults_are_the_ones_the_probers_already_used(self):
        self.assertEqual(effective_port(parse("http://a.example/announce")), 80)
        self.assertEqual(effective_port(parse("https://a.example/announce")), 443)
        self.assertEqual(effective_port(parse("udp://a.example/announce")), 80)


if __name__ == "__main__":
    unittest.main()
