"""The health checker: walk the ladder, record the rung, never claim more.

T-020, and with it T-022 (synthetic infohash), T-023 (yggdrasil by resolved
address), T-024 (vantage on every record) and T-025 (the rung -> state table).
They land together because they are all properties of one walk, and splitting
them would mean building the same fixture four times.

THE LADDER (RULES 3.3; each layer recorded separately, because each fails for
a different reason and a consumer troubleshooting a tracker needs to know
which one broke)

    DNS resolution
      +- TCP connect / UDP datagram sent
           +- TLS handshake (https only)
                +- transport response received
                     +- protocol-valid response
                          +- tracker-semantic response

THE OPERATOR IS ASKED FIRST (T-032)

Before either prober opens a socket it consults `bep34.py`, which reads the
tracker hostname's DNS TXT record. Only `ALLOW` reaches a socket; a denial and
an undetermined lookup both stop there, recorded with their reason. RULES 4
makes this absolute and states the consequence of its absence -- until it
exists, no corpus-wide probe may run -- which is why it gates the ladder rather
than filtering results afterwards.

⛔ **The check is in `probe_udp` and `probe_http`, not only in `probe`.** Both
are public entry points that open sockets, and a control enforced on one path
into an action while its sibling reaches the same action ungated is the most
recurring hole there is (`forbidden-patterns.md`).

A RESOLUTION FAILURE IS OUR RESOLVER'S OPINION UNTIL A SECOND ONE AGREES (T-037)

`_resolve` uses `socket.getaddrinfo`, which is the resolver a consumer on this
machine would get, so what it says is kept. Where it fails, `second_opinion`
asks this project's own resolvers through `bep34.py` before anything is
recorded, and the failure vocabulary splits the answers: a name only the public
resolvers can find is `RESOLVER_DIVERGENCE` and is in `ABOUT_US`, while
NXDOMAIN confirmed by both is the strongest not-resolving signal available
here. Measured cause: on both runner images this vantage cannot resolve
`openbittorrent.com`, which public resolvers answer for (`C-06`).

TWO THINGS THIS MODULE WILL NOT DO

**It cannot announce.** `bep15.py` has no function that builds an announce, and
the HTTP path sends only a scrape URL derived by BEP 48's rule. RULES 4's
prohibition is therefore a property of the code, not a policy someone has to
remember. Making this module announce would require adding a message builder to
`bep15.py`, which is a reviewable change to a file whose docstring says why it
is absent.

**It does not decide that anything is dead from one failed probe.** `dead` needs
`MIN_SAMPLES_FOR_DEATH` observations. A single timeout is a fact about one
moment, and ranking on the latest instantaneous result is the failure mode RULES
forbids by name.

WHAT THE STATE TABLE IS FOR

`health_state` is the *only* place a `HealthState` is produced. It is an
explicit, ordered table rather than scattered conditionals, because the
distinctions are the entire value of the dataset: `unknown` (never checked, or
too few samples) and `error` (the probe itself broke) must never collapse into
`dead`, and nothing this vantage cannot reach may be anything but
`unmeasurable`.
"""

from __future__ import annotations

import http.client
import ipaddress
import os
import socket
import ssl
import struct
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from enum import Enum
from typing import Any

from . import bep15
from .bencode import TRACKER_KINDS, classify_body
from .bep34 import (Decision, Resolver, classify_resolution,
                    protocol_for_transport)
from .model import (Network, Rung, Tracker, Transport,
                    UNREACHABLE_NETWORKS, YGGDRASIL_NET, HealthState)
from .vantage import UNKNOWN, Vantage, detect as detect_vantage

__all__ = [
    "Failure", "ProbeConfig", "ProbeResult", "MIN_SAMPLES_FOR_DEATH",
    "health_state", "classify_network_resolved", "probe", "probe_udp",
    "probe_http", "DEFAULT_USER_AGENT", "effective_port", "second_opinion",
]

#: How many observations before `dead` is sayable. Three is a judgement, not a
#: measurement, and it is recorded as one: with one sample a timeout is noise,
#: and the cost of calling a live tracker dead is higher than the cost of
#: saying `unknown` for another two cycles. Revisit once T-040 gives history.
MIN_SAMPLES_FOR_DEATH = 3

#: A tracker answer is small. Anything larger is not one, and reading it would
#: let a hostile or broken endpoint spend our memory.
MAX_BYTES = 256 * 1024

#: **Open empirical question, not a recommendation.** RULES 4.1 withdrew the
#: claim that a self-identifying User-Agent is the right thing to send: it was
#: asserted from six targets on one day, it never applied to UDP at all, and
#: trackers are reported to refuse clients that do not look like clients. T-012
#: measures it. Until that lands this is the string the project has historically
#: sent, kept as the default *so the measurement has a baseline arm*, not
#: because it is known to be correct.
DEFAULT_USER_AGENT = (
    "trackers/0.1 "
    "(+https://github.com/Azathothas/Trackers; tracker health probe)"
)


class Failure(str, Enum):
    """Why a rung was not reached. Distinct values because they are distinct facts.

    The split that matters most is between failures that are about the
    **tracker** (`refused`, `not_a_tracker`) and failures that are about
    **us** (`no_usable_address`, `blocked_by_policy`, `deadline_exceeded`,
    `probe_error`). Only the first kind is ever evidence of death.
    """

    NONE = "none"
    #: Neither this host's resolver nor the resolvers this project chose found
    #: an address, and the public answer was definitive: NXDOMAIN, or NOERROR
    #: carrying no address record. The strongest not-resolving signal available
    #: here, and still not `dead` on its own -- `MIN_SAMPLES_FOR_DEATH` decides
    #: that. `dns` on the result carries which of the two it was.
    DNS_FAILURE = "dns_failure"
    #: T-037: this host's resolver could not answer and a public one could.
    #: A fact about our vantage, measured at 3 hosts of 239 on `ubuntu-24.04`
    #: (`C-06`, `experiments/30`), so it is in `ABOUT_US` and can never produce
    #: `dead`. Publishing it as `dns_failure` would report one of the
    #: best-known public trackers as gone.
    RESOLVER_DIVERGENCE = "resolver_divergence"
    #: T-037: neither resolver answered and no answer was definitive -- a
    #: timeout, a SERVFAIL, a refusal. Nothing was established about the name,
    #: which is a different fact from establishing that it does not resolve.
    DNS_UNDETERMINED = "dns_undetermined"
    NO_USABLE_ADDRESS = "no_usable_address"
    TIMEOUT = "timeout"
    REFUSED = "refused"
    RESET = "reset"
    TLS_FAILURE = "tls_failure"
    NOT_A_TRACKER = "not_a_tracker"
    #: The body began as bencode and stopped mid-value. Distinct from
    #: `NOT_A_TRACKER` because the facts differ: a web server on the tracker's
    #: URL is evidence the tracker is gone, whereas a cut-off answer is
    #: evidence that *something answered* and the transport failed. Collapsing
    #: the two would let a network fault be published as a dead tracker -- the
    #: same shape of conflation RULES 3.2 is about.
    TRUNCATED_RESPONSE = "truncated_response"
    PROTOCOL_ERROR = "protocol_error"
    #: HTTP 401/403. Says somebody decided not to serve *us*. Under T-012 this
    #: may be our User-Agent rather than anything about the tracker, so it can
    #: never contribute to `dead`.
    BLOCKED_BY_POLICY = "blocked_by_policy"
    #: HTTP 429. The tracker is emphatically alive and asking us to slow down.
    RATE_LIMITED = "rate_limited"
    #: Not reached before the run's deadline (T-029). A fact about us.
    DEADLINE_EXCEEDED = "deadline_exceeded"
    #: The probe itself broke. Never `dead`; always `error`.
    PROBE_ERROR = "probe_error"
    #: This vantage cannot speak this transport or reach this network at all.
    UNSUPPORTED = "unsupported"
    #: T-032: the operator published a BEP 34 record that does not advertise
    #: this endpoint. Nothing was sent. It says nothing whatever about whether
    #: a tracker is running, and it is the one failure here that is somebody
    #: else's decision rather than an outcome.
    EXCLUDED_BY_OPERATOR = "excluded_by_operator"
    #: T-032: the BEP 34 lookup did not answer, so consent was never
    #: established. Distinct from `EXCLUDED_BY_OPERATOR` because a run that
    #: skipped a thousand trackers on a broken resolver must not read as a
    #: thousand operators refusing us.
    EXCLUSION_UNDETERMINED = "exclusion_undetermined"


#: Failures that are statements about us -- our position, or our own conduct --
#: and must never be read as evidence that a tracker is gone.
#:
#: `EXCLUDED_BY_OPERATOR` belongs here for the second reason: we chose not to
#: contact the host. Nothing was measured, so nothing about the tracker was
#: learned, and a record derived from it that said `dead` would be publishing
#: our own politeness as somebody else's outage.
ABOUT_US: frozenset[Failure] = frozenset({
    Failure.NO_USABLE_ADDRESS, Failure.BLOCKED_BY_POLICY,
    Failure.DEADLINE_EXCEEDED, Failure.PROBE_ERROR, Failure.UNSUPPORTED,
    Failure.EXCLUDED_BY_OPERATOR, Failure.EXCLUSION_UNDETERMINED,
    Failure.RESOLVER_DIVERGENCE, Failure.DNS_UNDETERMINED,
})


@dataclass(frozen=True, slots=True)
class ProbeConfig:
    """Everything that changes what a probe sends. Recorded with the result.

    `user_agent` is a field rather than a constant precisely so T-012 can run
    four arms through **this same code path**, differing in nothing else. A
    `None` value sends no User-Agent header at all, which is one of the arms.
    """

    timeout: float = 5.0
    retries: int = 1
    user_agent: str | None = DEFAULT_USER_AGENT
    #: Extra headers, for arms that vary more than the UA.
    extra_headers: tuple[tuple[str, str], ...] = ()

    # There is deliberately no `udp_scrape` switch here yet. `bep15.py` can
    # build a scrape request and refuses a non-20-byte hash, but `probe_udp`
    # sends **connect only** -- so a flag would advertise behaviour that does
    # not exist, which is worse than its absence. Wiring it is T-022, and the
    # bar is high on purpose: a UDP scrape carries a required info_hash and is
    # strictly more intrusive than a connect (`C-50`), while connect already
    # yields both liveness and RTT.

    def headers(self) -> dict[str, str]:
        h: dict[str, str] = {"Accept": "*/*"}
        if self.user_agent is not None:
            h["User-Agent"] = self.user_agent
        h.update(dict(self.extra_headers))
        return h


@dataclass(frozen=True, slots=True)
class ProbeResult:
    """One observation of one endpoint. Carries its own evidence.

    Every field that could be mistaken for a general truth is qualified by one
    that says where it came from: `rung` qualifies `ok`, `vantage` qualifies
    everything, and `resolved_ip` plus `observed_at` qualify `network`.
    """

    url: str
    transport: Transport
    #: The network as measured. May differ from the URL-derived classification
    #: when resolution reveals a Yggdrasil address (T-023).
    network: Network
    rung: Rung
    ok: bool
    failure: Failure = Failure.NONE
    detail: str = ""
    rtt_ms: float | None = None
    resolved_ip: str = UNKNOWN
    families: tuple[str, ...] = ()
    http_status: int | None = None
    #: Set when the URL-derived network and the resolved network disagree.
    #: The disagreement is the finding, so it is recorded, not silently
    #: resolved in favour of one side.
    network_reclassified_from: Network | None = None
    #: T-022: recorded whenever a scrape was sent, so the health record states
    #: that the info_hash corresponded to no content.
    used_synthetic_infohash: bool = False
    #: Exactly what was sent, so an arm of T-012 is reconstructable.
    sent_user_agent: str | None = None
    observed_at: str = UNKNOWN
    vantage: dict[str, Any] = field(default_factory=dict)
    classification: dict[str, Any] = field(default_factory=dict)
    #: T-032: what the operator's DNS said, and which resolver said it. Present
    #: on every result, including the ones that were allowed, because "we asked
    #: and were permitted" is the evidence that the gate ran at all.
    bep34: dict[str, Any] = field(default_factory=dict)
    #: T-037: both resolvers' answers, and the class they classify to. Present
    #: only where this host's resolver failed, which is the only case that asks
    #: a second question. ⛔ Both answers are kept rather than the winning one:
    #: the disagreement is the finding.
    dns: dict[str, Any] = field(default_factory=dict)

    def as_record(self, health: HealthState) -> dict[str, Any]:
        """The health-record shape `scripts/check-vantage-metadata.py` reads."""
        return {
            "url": self.url,
            "transport": self.transport.value,
            "network": self.network.value,
            "health_state": health.value,
            "measurement_rung": self.rung.value,
            "failure": self.failure.value,
            "detail": self.detail,
            "rtt_ms": self.rtt_ms,
            "resolved_ip": self.resolved_ip,
            "ip_families_seen": list(self.families),
            "http_status": self.http_status,
            "ipv6_only": bool(self.families) and "ipv4" not in self.families,
            "network_reclassified_from": (
                self.network_reclassified_from.value
                if self.network_reclassified_from else None),
            "used_synthetic_infohash": self.used_synthetic_infohash,
            "sent_user_agent": self.sent_user_agent if self.sent_user_agent else UNKNOWN,
            "observed_at": self.observed_at,
            "vantage": dict(self.vantage),
            "bep34": dict(self.bep34),
            "dns": dict(self.dns),
        }


# --- T-025: the rung -> state table -------------------------------------------
#
# Which rung proves "this is a tracker" is transport-specific, and that is the
# only place transport enters the decision.
#
#   UDP    a valid BEP 15 connect response IS tracker-specific. The magic
#          constant 0x41727101980 is a tracker protocol constant; nothing else
#          answers it with our transaction id echoed back. So PROTOCOL_VALID
#          proves a tracker here.
#   HTTP   PROTOCOL_VALID means only "bencode parsed". A bencoded blob that is
#          not a tracker answer proves nothing, so the bar is TRACKER_SEMANTIC.
_PROVING_RUNG: dict[Transport, Rung] = {
    Transport.UDP: Rung.PROTOCOL_VALID,
    Transport.HTTP: Rung.TRACKER_SEMANTIC,
    Transport.HTTPS: Rung.TRACKER_SEMANTIC,
}

_RUNG_ORDER: tuple[Rung, ...] = (
    Rung.NONE, Rung.DNS, Rung.CONNECTED, Rung.TLS,
    Rung.TRANSPORT_RESPONSE, Rung.PROTOCOL_VALID, Rung.TRACKER_SEMANTIC,
)


def rung_at_least(reached: Rung, needed: Rung) -> bool:
    """Ladder comparison. `NO_USABLE_ADDRESS` is not on the ladder and is never
    'at least' anything -- it is an outcome, not a height."""
    if reached not in _RUNG_ORDER or needed not in _RUNG_ORDER:
        return False
    return _RUNG_ORDER.index(reached) >= _RUNG_ORDER.index(needed)


def proves_tracker(rung: Rung, transport: Transport) -> bool:
    """Whether reaching `rung` on `transport` proves the responder is a tracker.

    The single consumer of `_PROVING_RUNG`, used by both the probe (to set
    `ok`) and `health_state` (to decide liveness), so the table cannot become
    decorative while the real rule lives somewhere else.

    A transport with no entry proves nothing at any rung. That is the correct
    default for `ws`/`wss`: no handshake has been attempted, so no rung of this
    ladder means anything there yet (T-005).
    """
    needed = _PROVING_RUNG.get(transport)
    return needed is not None and rung_at_least(rung, needed)


def health_state(*, rung: Rung, transport: Transport, network: Network,
                 sample_count: int, success_count: int,
                 failure: Failure = Failure.NONE,
                 measurable: bool = True) -> HealthState:
    """The single place a `HealthState` is decided. Ordered; first match wins.

    The order is the specification. Read it top to bottom:

    1. **Cannot be measured here at all** -> `unmeasurable`. Structural, and it
       does not need a failed probe to "prove" it. Asking and failing would be
       measuring our own reachability and reporting it as the tracker's health.
       **An operator's BEP 34 refusal lands here too** (T-032): "we may not
       measure it" and "we cannot measure it" differ in their reason, which the
       `failure` field carries, and not in what is known about the tracker,
       which is nothing in both cases.
    2. **Resolved, but to no address family we can use** -> `unmeasurable`. We
       did not fail to reach it; we never asked (`C-04`).
    3. **The probe itself broke** -> `error`. Never `dead`: a broken probe that
       marks everything dead is the failure T-021's oracle exists to catch.
    4. **Never observed** (sample_count 0, or the deadline arrived first) ->
       `unknown`. Running out of time is a fact about us (T-029). **A name our
       resolver could not answer for lands here too** (T-037): whether another
       resolver answered or nobody did, no socket was opened and nothing about
       the tracker was learned.
    5. **Refused or rate-limited** -> never `dead`. A 429 means very much
       alive; a 403 may be about our User-Agent rather than about them (T-012).
    6. **Every observation proved a tracker** -> `live`.
    7. **Some did** -> `degraded`.
    8. **None did, and there are enough observations** -> `dead`.
    9. **None did, and there are not** -> `unknown`. Too few samples is not
       death.
    """
    if (not measurable or failure is Failure.UNSUPPORTED
            or failure is Failure.EXCLUDED_BY_OPERATOR):
        return HealthState.UNMEASURABLE
    if rung is Rung.NO_USABLE_ADDRESS or failure is Failure.NO_USABLE_ADDRESS:
        return HealthState.UNMEASURABLE
    if failure is Failure.PROBE_ERROR:
        return HealthState.ERROR
    if (sample_count <= 0 or failure is Failure.DEADLINE_EXCEEDED
            or failure is Failure.EXCLUSION_UNDETERMINED
            or failure is Failure.RESOLVER_DIVERGENCE
            or failure is Failure.DNS_UNDETERMINED):
        # Each is stated explicitly rather than left to `sample_count == 0`, so
        # it stays `unknown` even where a caller carries observations forward
        # from an earlier run. The two DNS values are here for the reason
        # T-037 exists: our resolver's opinion is not the name's property, and
        # letting either accumulate toward `dead` would publish one of the
        # best-known public trackers as gone.
        return HealthState.UNKNOWN
    if failure in (Failure.RATE_LIMITED, Failure.BLOCKED_BY_POLICY,
                   Failure.TRUNCATED_RESPONSE, Failure.RESET):
        # Something answered. Whatever this is, it is not absence.
        #   429 / truncated / reset -> answering, but not serving us correctly,
        #                              which is what `degraded` is for.
        #   401 / 403               -> `unknown`, because the refusal may be
        #                              about our identity rather than about the
        #                              tracker at all (T-012). Calling that
        #                              `degraded` would assert a fault we have
        #                              no evidence for.
        return (HealthState.UNKNOWN if failure is Failure.BLOCKED_BY_POLICY
                else HealthState.DEGRADED)

    # Liveness needs BOTH: observations that succeeded, AND a rung that proves
    # a tracker on this transport. Requiring the rung is what stops a bencoded
    # blob from an ordinary web server, or a TCP connect that reached nothing,
    # from being counted as a live tracker.
    proven = proves_tracker(rung, transport)
    if success_count > 0 and success_count >= sample_count and proven:
        return HealthState.LIVE
    if success_count > 0:
        # Some observations proved a tracker and the most recent did not, or
        # vice versa. Intermittent is its own state and must not round to
        # either neighbour.
        return HealthState.DEGRADED
    if sample_count >= MIN_SAMPLES_FOR_DEATH:
        return HealthState.DEAD
    return HealthState.UNKNOWN


# --- T-023: network from the resolved address ---------------------------------
def reclassified_out_of_reach(network: Network, from_net: Network | None,
                              tracker: Tracker, base: dict,
                              resolved_ip: str,
                              families: tuple[str, ...]) -> ProbeResult | None:
    """A result when resolution moved a tracker into a network we cannot reach.

    ⛔ **This is T-023's bug one layer down, and it was live.** `probe` checks
    `tracker.is_measurable_here` before resolving, off the URL alone, so
    `http://yggtracker.i2p.rocks:80/announce` passes: the name looks like
    clearnet. Resolution then returns `200:1e2f:...`, inside `0200::/7`, and
    `classify_network_resolved` correctly says yggdrasil -- and nothing read
    that answer, so the probe opened a socket to a network this vantage cannot
    route to and recorded `timeout`. Three of those and the state table would
    have said `dead`, which is RULES 11's named anti-pattern and RULES 3.1's
    absolute rule, reached through the fix that exists to prevent it.

    Measured on 2026-09-08 by `experiments/33`, which is what a real probe of
    that host from a vantage with IPv6 turned up.
    """
    if from_net is None or network not in UNREACHABLE_NETWORKS:
        return None
    return ProbeResult(
        network=network, network_reclassified_from=from_net, rung=Rung.NONE,
        ok=False, failure=Failure.UNSUPPORTED,
        detail=(f"{tracker.host} resolves into {network.value} "
                f"({resolved_ip}), which needs a router this vantage does not "
                f"run. Nothing was sent."),
        resolved_ip=resolved_ip, families=families, **base)


def classify_network_resolved(url_network: Network,
                              addresses: list[str]) -> tuple[Network, Network | None]:
    """Refine the URL-derived network using addresses DNS actually returned.

    Returns `(network, reclassified_from)`. `reclassified_from` is `None` when
    nothing changed.

    This is the fix for the bug RULES 3.1 exists to prevent, surviving inside
    the fix for it: ngosang's single yggdrasil entry is
    `http://yggtracker.i2p.rocks:80/announce` -- an ordinary hostname that
    resolves into `0200::/7`. A URL-only classifier calls it clearnet, routes
    it to the clearnet prober, and records it **dead**.

    **This needs a DNS answer, not Yggdrasil connectivity**, so it is fully
    solvable from this vantage. Reaching the tracker afterwards is T-031's
    problem, not this function's.

    The result is a *time-varying inference*, never a permanent property: it is
    recorded with the address and the observation time, for the same reason
    dedup refuses to collapse two hosts that merely share an address today.
    """
    if url_network is not Network.CLEARNET:
        # An explicit `.i2p` or `.onion` suffix is stronger evidence than a
        # resolved address, and those names do not resolve in the ordinary DNS
        # anyway.
        return url_network, None
    for a in addresses:
        try:
            ip = ipaddress.ip_address(a)
        except ValueError:
            continue
        if ip.version == 6 and ip in YGGDRASIL_NET:
            return Network.YGGDRASIL, url_network
    return url_network, None


# --- resolution ---------------------------------------------------------------
def _asks_for_loopback(host: str) -> bool:
    """Whether this URL means the local host, rather than being sent there.

    Two ways to mean it, and both are the caller saying what it wants:

    * an **address literal**, `127.0.0.1` or `[::1]`, which names exactly one
      destination;
    * **`localhost`** or a name under it, which RFC 6761 section 6.3 reserves
      for the loopback address and requires resolvers to answer that way.

    Everything else that resolves to loopback is DNS pointing the probe at
    this machine, which is a different fact and not a destination. The oracles
    in `tests/` use both permitted forms, so the rule below does not have to
    carve out a test path -- which is the shape that makes a control decorative.
    """
    bare = host.strip("[]").lower().rstrip(".")
    if bare == "localhost" or bare.endswith(".localhost"):
        return True
    try:
        ipaddress.ip_address(bare)
        return True
    except ValueError:
        return False


def _undialable(address: str, *, from_a_name: bool) -> bool:
    """Whether this address is one no probe may open a socket to.

    ⛔ **On Linux a connect to the unspecified address reaches the local
    host**, so probing a name that resolves to one would open a socket to the
    runner itself and record whatever answered as the tracker. Windows refuses
    it outright with `WinError 10049`. Both halves are measured by
    `tests/test_probe.py`
    `ANullAddressIsNeverDialled.test_what_the_null_address_actually_does_on_this_platform`,
    which runs on both platforms in the gate: the same endpoint produces two
    different failures on two vantages, and neither is a fact about a tracker.

    ⚠ **The corpus contains these.** 11 hosts and 14 URLs answer `0.0.0.0`,
    `::` or both, and on a runner they resolve for `getaddrinfo` and reach the
    prober (`experiments/30`, run `34235047982` and the authoring-host run of
    the same day).

    ⛔ **And one resolves to `::1`.** `ipv6.tracker.harry.lu` answered loopback
    on 2026-09-08 and the probe connected to this machine, recording the reset
    as the tracker's (`experiments/33`). An earlier revision of this function
    excluded only the unspecified addresses and said loopback was deliberately
    left out because it had not been measured in this corpus. It has been now,
    so it is excluded too -- but only when a **name** resolved to it, because
    the oracle probes `127.0.0.1` on purpose and a URL that names an address
    means the address it names.
    """
    try:
        ip = ipaddress.ip_address(address)
    except ValueError:
        return False
    return ip.is_unspecified or (from_a_name and ip.is_loopback)


@dataclass(frozen=True, slots=True)
class Resolution:
    """What DNS said, kept whole so nothing has to ask twice.

    Resolving a second time inside one probe is not merely wasteful: DNS can
    answer differently between two calls, and then the address we classified
    the network from is not the address we connected to.
    """

    infos: tuple[tuple, ...]
    addresses: tuple[str, ...]
    families: tuple[str, ...]
    usable: tuple[tuple, ...]
    #: Addresses dropped because they are unspecified. Kept rather than
    #: discarded: a name answering `0.0.0.0` is evidence about that name, and
    #: the record has to be able to say so.
    unspecified: tuple[str, ...] = ()

    @property
    def first(self) -> str:
        return self.addresses[0] if self.addresses else UNKNOWN

    @property
    def first_usable(self) -> str:
        """The address a probe would open, or the first one DNS returned.

        ⚠ **On the HTTP path this is what we would have contacted, not
        provably what was contacted.** `urllib` resolves the hostname again
        inside `urlopen` and chooses for itself, so the record cannot claim
        more than this. The UDP path does choose, and there the two agree.
        """
        return self.usable[0][4][0] if self.usable else self.first

    @property
    def only_unspecified(self) -> bool:
        """Every address the resolver returned is a null one."""
        return bool(self.unspecified) and len(self.unspecified) == len(self.addresses)


def _resolve(host: str, port: int, sock_type: int, vantage: Vantage) -> Resolution:
    """Resolve with `AF_UNSPEC` and report which families came back.

    `AF_UNSPEC`, never `AF_INET`. Forcing `AF_INET` makes an IPv6-only tracker
    raise here and be recorded `dns_failure`, which is false -- the name
    resolved perfectly well. That misclassification is the same class of lie as
    marking such a tracker dead, and it is a bug this project has already found
    and fixed once, in `experiments/02`.

    ⚠ **This is the host's resolver and it has an opinion of its own.** Every
    caller of this function sends a failure through `second_opinion` rather
    than recording it (T-037).
    """
    infos = socket.getaddrinfo(host, port, socket.AF_UNSPEC, sock_type)
    named = not _asks_for_loopback(host)
    families = tuple(sorted({
        "ipv6" if i[0] == socket.AF_INET6 else "ipv4" for i in infos}))
    unspecified = tuple(i[4][0] for i in infos
                        if _undialable(i[4][0], from_a_name=named))
    usable = tuple(
        i for i in infos
        if ("ipv6" if i[0] == socket.AF_INET6 else "ipv4") in vantage.ip_families
        and not _undialable(i[4][0], from_a_name=named))
    return Resolution(infos=tuple(infos),
                      addresses=tuple(i[4][0] for i in infos),
                      families=families, usable=usable,
                      unspecified=unspecified)


# --- T-037: a second opinion, only where the first failed ---------------------
#
# What each resolution class means for a health record. The classes are
# `bep34.RESOLUTION_CLASSES` and they are about the lookup; these are the
# consequences, and the split is the one `ABOUT_US` draws:
#
#   the public resolvers answered        -> our vantage. Never `dead`.
#   they answered definitively that      -> about the name, and the strongest
#   there is no address                     signal available here.
#   they did not answer at all           -> nothing was established.
#
# `resolves_for_both` and `resolves_only_for_this_host` are absent on purpose:
# both mean `getaddrinfo` answered, and no second query is made in that case.
_RESOLUTION_FAILURE: dict[str, Failure] = {
    "resolves_only_for_the_public_resolver": Failure.RESOLVER_DIVERGENCE,
    "gone_nxdomain_confirmed": Failure.DNS_FAILURE,
    "no_address_records": Failure.DNS_FAILURE,
    # Both sides agree there is nothing to connect to: this host reported no
    # data and the public resolvers answered `0.0.0.0`. That is about the name.
    "resolves_to_an_unusable_address": Failure.DNS_FAILURE,
    "lookup_failed_undetermined": Failure.DNS_UNDETERMINED,
}


def null_address_record(res: "Resolution") -> dict[str, Any]:
    """The `dns` evidence for a name our own resolver answered with nothing
    routable. No second opinion is asked: this resolver did answer, and a
    public one measured the same null addresses for the same hosts.
    """
    return {
        "class": "resolves_to_an_unusable_address",
        "system": {"resolved": True, "addresses": list(res.addresses),
                   "detail": "every address returned is unspecified"},
        "public": {"asked": False},
    }


def second_opinion(host: str, resolver: Resolver,
                   detail: str) -> tuple[Failure, str, dict[str, Any]]:
    """Ask this project's own resolvers about a name `getaddrinfo` refused.

    Returns `(failure, detail, record)`.

    ⛔ **Not a replacement for `getaddrinfo`.** It is the resolver a consumer
    on this machine would use, so its answer is a real fact about this vantage
    and is kept. This asks a second question **only where the first failed**,
    which is 239 of 759 corpus hosts at worst and leaves the other 520
    untouched (T-037, RULES 15.2).

    The alternative -- preferring whichever resolver answers -- would delete the
    fact that they disagreed, and the disagreement is the finding: on both
    runner images `openbittorrent.com` fails here and answers there (`C-06`).
    """
    try:
        public = resolver.addresses(host)
    except Exception as e:  # a broken second lookup is our defect, not a fact
        return (Failure.PROBE_ERROR,
                f"{detail}; second opinion raised {type(e).__name__}: {e}", {})

    # `system_resolved` is False at every call site: this is reached only from
    # a resolution that failed. The parameter exists because `experiments/30`
    # classifies hosts where it is True.
    kind = classify_resolution(system_resolved=False, public=public)
    record = {
        "class": kind,
        "system": {"resolved": False, "detail": detail},
        "public": public,
        "resolvers": list(resolver.config.resolvers),
    }
    return (_RESOLUTION_FAILURE.get(kind, Failure.DNS_UNDETERMINED),
            f"{detail}; public resolvers: {kind}", record)


# --- T-032: the operator's refusal, before any socket -------------------------
#
# The port a URL is contacted on is decided here and nowhere else. Both probers
# and the BEP 34 gate read it from this function, because a gate that checks
# port 80 while the prober contacts 6969 is decorative -- it would report an
# endpoint permitted that was never the endpoint we opened.

def effective_port(tracker: Tracker) -> int:
    """The port this probe will actually contact.

    `udp` has no default-port convention (`normalize.keep_explicit_port`), so a
    `udp://` URL without one keeps the 80 the probe has always used rather than
    acquiring a new default in a change about something else.
    """
    if tracker.port is not None:
        return tracker.port
    return 443 if tracker.transport in (Transport.HTTPS, Transport.WSS) else 80


def _consult_operator(tracker: Tracker, resolver: Resolver,
                      base: dict) -> tuple[dict[str, Any], ProbeResult | None]:
    """Ask DNS whether this endpoint may be contacted at all.

    Returns `(record, refusal)`. A non-`None` refusal is returned to the caller
    **unchanged and immediately**: no socket, no resolution, nothing sent.

    There is deliberately no argument that skips this. `resolver` selects who is
    asked -- which is how the loopback oracle exercises it -- and every value of
    it still ends in a decision. It is required rather than defaulted, so that
    the resolver consulted here is the one a resolution failure asks for a
    second opinion (T-037), and both are cached together.
    """
    port = effective_port(tracker)
    verdict = resolver.consult(tracker.host,
                               protocol_for_transport(tracker.transport.value),
                               port)
    record = verdict.as_record()
    if verdict.decision is Decision.ALLOW:
        return record, None

    failure = (Failure.EXCLUDED_BY_OPERATOR
               if verdict.decision is Decision.DENY
               else Failure.EXCLUSION_UNDETERMINED)
    return record, ProbeResult(
        network=tracker.network, rung=Rung.NONE, ok=False, failure=failure,
        detail=f"BEP 34 {verdict.decision.value}: {verdict.detail}",
        bep34=record, **base)


# --- UDP ----------------------------------------------------------------------
def probe_udp(tracker: Tracker, cfg: ProbeConfig, vantage: Vantage,
              observed_at: str = UNKNOWN,
              resolver: Resolver | None = None) -> ProbeResult:
    """One BEP 15 connect exchange, if BEP 34 permits it.

    ⚠ Connect only. `bep15.py` can build a scrape and refuses a non-20-byte
    hash, but nothing here sends one; wiring that is T-022. An earlier version
    of this line advertised a `cfg.udp_scrape` switch that has never existed.
    """
    resolver = resolver or Resolver()
    base = dict(url=tracker.url, transport=tracker.transport,
                observed_at=observed_at, vantage=vantage.as_dict(),
                sent_user_agent=None)  # BEP 15 is binary; there is no UA field

    consulted, refusal = _consult_operator(tracker, resolver, base)
    if refusal is not None:
        return refusal
    base["bep34"] = consulted
    port = effective_port(tracker)

    try:
        res = _resolve(tracker.host, port, socket.SOCK_DGRAM, vantage)
    except OSError as e:
        failure, detail, dns = second_opinion(
            tracker.host, resolver, f"{type(e).__name__}: {e}")
        return ProbeResult(network=tracker.network, rung=Rung.NONE, ok=False,
                           failure=failure, detail=detail, dns=dns, **base)

    network, from_net = classify_network_resolved(tracker.network,
                                                  list(res.addresses))
    families = res.families
    out_of_reach = reclassified_out_of_reach(network, from_net, tracker, base,
                                             res.first, families)
    if out_of_reach is not None:
        return out_of_reach

    if res.only_unspecified:
        return ProbeResult(
            network=network, network_reclassified_from=from_net,
            rung=Rung.NONE, ok=False, failure=Failure.DNS_FAILURE,
            detail=(f"resolves only to {list(res.unspecified)}, which is an "
                    f"answer and not a destination"),
            dns=null_address_record(res), resolved_ip=res.first,
            families=families, **base)
    if not res.usable:
        return ProbeResult(
            network=network, network_reclassified_from=from_net,
            rung=Rung.NO_USABLE_ADDRESS, ok=False,
            failure=Failure.NO_USABLE_ADDRESS,
            detail=(f"resolves only to {list(families)}; this vantage can use "
                    f"{list(vantage.ip_families)}"),
            resolved_ip=res.first, families=families, **base)

    family, _, _, _, addr = res.usable[0]
    rung = Rung.DNS
    detail = "no attempt"
    for attempt in range(cfg.retries + 1):
        # 32 fresh random bits per attempt. This is the anti-spoofing value an
        # off-path attacker would have to guess to forge liveness, so it is
        # drawn from urandom and never reused across attempts.
        txid = struct.unpack(">I", os.urandom(4))[0]
        s = socket.socket(family, socket.SOCK_DGRAM)
        s.settimeout(cfg.timeout)
        t0 = time.monotonic()
        try:
            s.sendto(bep15.build_connect_request(txid), addr)
            rung = Rung.CONNECTED
            data, _ = s.recvfrom(4096)
            rtt = (time.monotonic() - t0) * 1000.0
            rung = Rung.TRANSPORT_RESPONSE
            ok, why, conn_id = bep15.parse_connect_response(data, txid)
            if ok:
                # A correct connect response is tracker-specific: nothing but a
                # BEP 15 tracker answers the magic constant with our own
                # transaction id echoed back.
                return ProbeResult(
                    network=network, network_reclassified_from=from_net,
                    rung=Rung.PROTOCOL_VALID, ok=True, detail=why,
                    rtt_ms=round(rtt, 3), resolved_ip=addr[0],
                    families=families, **base)
            if why.startswith("BEP15 error response"):
                # A tracker declining is a tracker. Strictly stronger evidence
                # of life than silence.
                return ProbeResult(
                    network=network, network_reclassified_from=from_net,
                    rung=Rung.PROTOCOL_VALID, ok=True,
                    failure=Failure.NONE, detail=why, rtt_ms=round(rtt, 3),
                    resolved_ip=addr[0], families=families, **base)
            detail = why
            rung = Rung.TRANSPORT_RESPONSE
        except socket.timeout:
            detail = f"timeout after {cfg.timeout}s"
        except OSError as e:
            detail = f"{type(e).__name__}: {e}"
        finally:
            s.close()

    failure = (Failure.TIMEOUT if detail.startswith("timeout")
               else Failure.PROTOCOL_ERROR if rung is Rung.TRANSPORT_RESPONSE
               else Failure.REFUSED)
    return ProbeResult(network=network, network_reclassified_from=from_net,
                       rung=rung, ok=False, failure=failure, detail=detail,
                       resolved_ip=addr[0], families=families, **base)


# --- HTTP ---------------------------------------------------------------------
def probe_http(tracker: Tracker, cfg: ProbeConfig, vantage: Vantage,
               observed_at: str = UNKNOWN,
               resolver: Resolver | None = None) -> ProbeResult:
    """One HTTP(S) scrape, if BEP 34 permits it. BEP 48: a scrape has no effect
    on swarm participation.

    Where the announce URL has no `announce` in its path, BEP 48's convention
    does not apply and no scrape URL is invented -- guessing one would fabricate
    an endpoint and then report its absence as the tracker's defect.
    """
    resolver = resolver or Resolver()
    # ⛔ False until a request is actually issued. It travels in `base` into
    # every result this function builds, and the ones built before the socket
    # -- an operator's refusal, a resolution failure, no usable address -- sent
    # no info_hash at all. A record claiming one is the "hardcoded status"
    # forbidden pattern, and it reached a committed sweep record before T-037
    # read one closely.
    base = dict(url=tracker.url, transport=tracker.transport,
                observed_at=observed_at, vantage=vantage.as_dict(),
                sent_user_agent=cfg.user_agent, used_synthetic_infohash=False)

    consulted, refusal = _consult_operator(tracker, resolver, base)
    if refusal is not None:
        return refusal
    base["bep34"] = consulted

    target = tracker.scrape_url or tracker.url
    info_hash = bep15.synthetic_infohash()
    qs = urllib.parse.urlencode({"info_hash": info_hash},
                                quote_via=urllib.parse.quote)
    full = target + ("&" if urllib.parse.urlsplit(target).query else "?") + qs
    port = effective_port(tracker)

    try:
        res = _resolve(tracker.host, port, socket.SOCK_STREAM, vantage)
    except OSError as e:
        failure, detail, dns = second_opinion(
            tracker.host, resolver, f"{type(e).__name__}: {e}")
        return ProbeResult(network=tracker.network, rung=Rung.NONE, ok=False,
                           failure=failure, detail=detail, dns=dns, **base)

    network, from_net = classify_network_resolved(tracker.network,
                                                  list(res.addresses))
    families = res.families
    out_of_reach = reclassified_out_of_reach(network, from_net, tracker, base,
                                             res.first, families)
    if out_of_reach is not None:
        return out_of_reach
    addresses = list(res.addresses)
    # ⛔ Never `addresses[0]`. That is whatever DNS listed first, which
    # may be a null address this probe refuses to open (T-037).
    contacted = res.first_usable

    if not addresses:
        failure, detail, dns = second_opinion(
            tracker.host, resolver, f"no address for {tracker.host}")
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=Rung.NONE, ok=False, failure=failure,
                           detail=detail, dns=dns, **base)
    if res.only_unspecified:
        return ProbeResult(
            network=network, network_reclassified_from=from_net,
            rung=Rung.NONE, ok=False, failure=Failure.DNS_FAILURE,
            detail=(f"resolves only to {list(res.unspecified)}, which is an "
                    f"answer and not a destination"),
            dns=null_address_record(res), resolved_ip=res.first,
            families=families, **base)
    if not res.usable:
        return ProbeResult(
            network=network, network_reclassified_from=from_net,
            rung=Rung.NO_USABLE_ADDRESS, ok=False,
            failure=Failure.NO_USABLE_ADDRESS,
            detail=(f"resolves only to {list(families)}; this vantage can use "
                    f"{list(vantage.ip_families)}"),
            resolved_ip=res.first, families=families, **base)

    # From here a scrape carrying a synthetic info_hash is on its way out, so
    # every result built below records that it was sent.
    base["used_synthetic_infohash"] = True
    req = urllib.request.Request(full, headers=cfg.headers())
    t0 = time.monotonic()
    try:
        ctx = ssl.create_default_context()
        with urllib.request.urlopen(req, timeout=cfg.timeout, context=ctx) as resp:
            body = resp.read(MAX_BYTES + 1)
            status = resp.status
        rtt = (time.monotonic() - t0) * 1000.0
        return _classify_http(body, status, rtt, base, network, from_net,
                              contacted, families)
    except urllib.error.HTTPError as e:
        # A tracker may answer 4xx and still be a tracker; read the body before
        # deciding. A 403 with a bencoded failure inside is a live tracker.
        try:
            body = e.read(MAX_BYTES)
        except Exception:
            body = b""
        rtt = (time.monotonic() - t0) * 1000.0
        return _classify_http(body, e.code, rtt, base, network, from_net,
                              contacted, families)
    except urllib.error.URLError as e:
        reason = e.reason
        dns: dict[str, Any] = {}
        detail = f"{type(reason).__name__}: {reason}"
        if isinstance(reason, ssl.SSLError):
            failure, rung = Failure.TLS_FAILURE, Rung.CONNECTED
        elif isinstance(reason, socket.gaierror):
            # `urlopen` resolves again, and this one failed where the probe's
            # own lookup had just succeeded. That is our resolver contradicting
            # itself inside one probe, so it takes the same second opinion as
            # any other resolution failure rather than being published as a
            # property of the name (T-037).
            rung = Rung.NONE
            failure, detail, dns = second_opinion(tracker.host, resolver, detail)
        elif isinstance(reason, socket.timeout) or "timed out" in str(reason):
            failure, rung = Failure.TIMEOUT, Rung.DNS
        elif isinstance(reason, ConnectionResetError):
            failure, rung = Failure.RESET, Rung.CONNECTED
        elif isinstance(reason, ConnectionRefusedError):
            failure, rung = Failure.REFUSED, Rung.DNS
        else:
            failure, rung = Failure.REFUSED, Rung.DNS
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=rung, ok=False, failure=failure, detail=detail,
                           dns=dns, resolved_ip=contacted,
                           families=families, **base)
    except (socket.timeout, TimeoutError):
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=Rung.DNS, ok=False, failure=Failure.TIMEOUT,
                           detail=f"timeout after {cfg.timeout}s",
                           resolved_ip=contacted, families=families, **base)
    except (ConnectionResetError, BrokenPipeError,
            http.client.IncompleteRead, http.client.HTTPException) as e:
        # `CLOSE_MIDWAY`: headers promised more than arrived. That is a
        # transport fault, not a protocol one, and it is not death -- the
        # tracker was there and answering right up to the moment it was not.
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=Rung.TRANSPORT_RESPONSE, ok=False,
                           failure=Failure.RESET,
                           detail=f"{type(e).__name__}: {e}",
                           resolved_ip=contacted, families=families, **base)
    except Exception as e:  # the probe itself broke
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=Rung.NONE, ok=False, failure=Failure.PROBE_ERROR,
                           detail=f"{type(e).__name__}: {e}",
                           resolved_ip=contacted, families=families, **base)


def _classify_http(body: bytes, status: int, rtt: float, base: dict,
                   network: Network, from_net: Network | None,
                   resolved_ip: str, families: tuple[str, ...]) -> ProbeResult:
    """Decide what came back. The discriminator, and the refusal cases.

    Order matters: the **body is read first**, because a tracker that answers
    403 with a bencoded failure is a live tracker and its status code is the
    less informative half of the response.
    """
    cls = classify_body(body[:MAX_BYTES])
    kind = cls["kind"]

    if kind in TRACKER_KINDS:
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=Rung.TRACKER_SEMANTIC, ok=True,
                           detail=cls.get("detail", ""), rtt_ms=round(rtt, 3),
                           resolved_ip=resolved_ip, families=families,
                           http_status=status, classification=cls, **base)

    if status in (401, 403):
        # A refusal aimed at us. Under T-012 this may be our User-Agent, so it
        # can never contribute to `dead`.
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=Rung.TRANSPORT_RESPONSE, ok=False,
                           failure=Failure.BLOCKED_BY_POLICY,
                           detail=f"HTTP {status}; body kind={kind}",
                           rtt_ms=round(rtt, 3), resolved_ip=resolved_ip,
                           families=families, http_status=status,
                           classification=cls, **base)
    if status == 429:
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=Rung.TRANSPORT_RESPONSE, ok=False,
                           failure=Failure.RATE_LIMITED,
                           detail=f"HTTP {status}; body kind={kind}",
                           rtt_ms=round(rtt, 3), resolved_ip=resolved_ip,
                           families=families, http_status=status,
                           classification=cls, **base)

    rung = (Rung.PROTOCOL_VALID
            if kind in ("bencode_dict_unrecognised", "bencode_not_dict")
            else Rung.TRANSPORT_RESPONSE)
    # A body that began as bencode and ran out is a transport fault, not a web
    # server. The decoder's own message is the discriminator, so this cannot
    # drift from what the parser actually decided.
    failure = (Failure.TRUNCATED_RESPONSE
               if "runs past end of input" in str(cls.get("detail", ""))
               else Failure.NOT_A_TRACKER)
    return ProbeResult(network=network, network_reclassified_from=from_net,
                       rung=rung, ok=False, failure=failure,
                       detail=f"HTTP {status}; {cls.get('detail', '')}",
                       rtt_ms=round(rtt, 3), resolved_ip=resolved_ip,
                       families=families, http_status=status,
                       classification=cls, **base)


# --- entry point --------------------------------------------------------------
def probe(tracker: Tracker, cfg: ProbeConfig | None = None,
          vantage: Vantage | None = None,
          observed_at: str = UNKNOWN,
          resolver: Resolver | None = None) -> ProbeResult:
    """Probe one tracker. Never raises; every failure becomes a recorded result.

    A tracker this vantage cannot measure is **not probed at all**. Asking and
    failing would produce a timeout that looks exactly like a dead tracker, and
    the record would then say something false about the world rather than
    something true about us.

    ⭐ **Pass one `Resolver` for a whole run.** It caches the BEP 34 answer per
    host, so a corpus with many URLs on one host asks once instead of once per
    URL (RULES 15.2). Left `None`, each call builds its own and the caching is
    lost, which is correct but noisy.
    """
    cfg = cfg or ProbeConfig()
    vantage = vantage or detect_vantage()

    if not tracker.is_measurable_here:
        # Ahead of the BEP 34 gate on purpose: these are never contacted under
        # any answer, and `.i2p` and `.onion` names do not resolve in the
        # ordinary DNS, so a lookup here would be a query that cannot succeed
        # asked about a host we were never going to reach.
        return ProbeResult(
            url=tracker.url, transport=tracker.transport,
            network=tracker.network, rung=Rung.NONE, ok=False,
            failure=Failure.UNSUPPORTED,
            detail=tracker.unmeasurable_reason or "not measurable from this vantage",
            observed_at=observed_at, vantage=vantage.as_dict())

    if tracker.transport is Transport.UDP:
        return probe_udp(tracker, cfg, vantage, observed_at, resolver)
    if tracker.transport in (Transport.HTTP, Transport.HTTPS):
        return probe_http(tracker, cfg, vantage, observed_at, resolver)

    return ProbeResult(
        url=tracker.url, transport=tracker.transport, network=tracker.network,
        rung=Rung.NONE, ok=False, failure=Failure.UNSUPPORTED,
        detail=f"transport {tracker.transport.value} has no prober",
        observed_at=observed_at, vantage=vantage.as_dict())
