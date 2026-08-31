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
from .model import (Network, Rung, Tracker, Transport, YGGDRASIL_NET,
                    HealthState)
from .vantage import UNKNOWN, Vantage, detect as detect_vantage

__all__ = [
    "Failure", "ProbeConfig", "ProbeResult", "MIN_SAMPLES_FOR_DEATH",
    "health_state", "classify_network_resolved", "probe", "probe_udp",
    "probe_http", "DEFAULT_USER_AGENT",
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
    DNS_FAILURE = "dns_failure"
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


#: Failures that are statements about our own position and must never be read
#: as evidence that a tracker is gone.
ABOUT_US: frozenset[Failure] = frozenset({
    Failure.NO_USABLE_ADDRESS, Failure.BLOCKED_BY_POLICY,
    Failure.DEADLINE_EXCEEDED, Failure.PROBE_ERROR, Failure.UNSUPPORTED,
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
    2. **Resolved, but to no address family we can use** -> `unmeasurable`. We
       did not fail to reach it; we never asked (`C-04`).
    3. **The probe itself broke** -> `error`. Never `dead`: a broken probe that
       marks everything dead is the failure T-021's oracle exists to catch.
    4. **Never observed** (sample_count 0, or the deadline arrived first) ->
       `unknown`. Running out of time is a fact about us (T-029).
    5. **Refused or rate-limited** -> never `dead`. A 429 means very much
       alive; a 403 may be about our User-Agent rather than about them (T-012).
    6. **Every observation proved a tracker** -> `live`.
    7. **Some did** -> `degraded`.
    8. **None did, and there are enough observations** -> `dead`.
    9. **None did, and there are not** -> `unknown`. Too few samples is not
       death.
    """
    if not measurable or failure is Failure.UNSUPPORTED:
        return HealthState.UNMEASURABLE
    if rung is Rung.NO_USABLE_ADDRESS or failure is Failure.NO_USABLE_ADDRESS:
        return HealthState.UNMEASURABLE
    if failure is Failure.PROBE_ERROR:
        return HealthState.ERROR
    if sample_count <= 0 or failure is Failure.DEADLINE_EXCEEDED:
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

    @property
    def first(self) -> str:
        return self.addresses[0] if self.addresses else UNKNOWN


def _resolve(host: str, port: int, sock_type: int, vantage: Vantage) -> Resolution:
    """Resolve with `AF_UNSPEC` and report which families came back.

    `AF_UNSPEC`, never `AF_INET`. Forcing `AF_INET` makes an IPv6-only tracker
    raise here and be recorded `dns_failure`, which is false -- the name
    resolved perfectly well. That misclassification is the same class of lie as
    marking such a tracker dead, and it is a bug this project has already found
    and fixed once, in `experiments/02`.
    """
    infos = socket.getaddrinfo(host, port, socket.AF_UNSPEC, sock_type)
    families = tuple(sorted({
        "ipv6" if i[0] == socket.AF_INET6 else "ipv4" for i in infos}))
    usable = tuple(
        i for i in infos
        if ("ipv6" if i[0] == socket.AF_INET6 else "ipv4") in vantage.ip_families)
    return Resolution(infos=tuple(infos),
                      addresses=tuple(i[4][0] for i in infos),
                      families=families, usable=usable)


# --- UDP ----------------------------------------------------------------------
def probe_udp(tracker: Tracker, cfg: ProbeConfig, vantage: Vantage,
              observed_at: str = UNKNOWN) -> ProbeResult:
    """One BEP 15 exchange. Connect always; scrape only if `cfg.udp_scrape`."""
    port = tracker.port or 80
    base = dict(url=tracker.url, transport=tracker.transport,
                observed_at=observed_at, vantage=vantage.as_dict(),
                sent_user_agent=None)  # BEP 15 is binary; there is no UA field

    try:
        res = _resolve(tracker.host, port, socket.SOCK_DGRAM, vantage)
    except OSError as e:
        return ProbeResult(network=tracker.network, rung=Rung.NONE, ok=False,
                           failure=Failure.DNS_FAILURE,
                           detail=f"{type(e).__name__}: {e}", **base)

    network, from_net = classify_network_resolved(tracker.network,
                                                  list(res.addresses))
    families = res.families

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
               observed_at: str = UNKNOWN) -> ProbeResult:
    """One HTTP(S) scrape. BEP 48: a scrape has no effect on swarm participation.

    Where the announce URL has no `announce` in its path, BEP 48's convention
    does not apply and no scrape URL is invented -- guessing one would fabricate
    an endpoint and then report its absence as the tracker's defect.
    """
    target = tracker.scrape_url or tracker.url
    info_hash = bep15.synthetic_infohash()
    qs = urllib.parse.urlencode({"info_hash": info_hash},
                                quote_via=urllib.parse.quote)
    full = target + ("&" if urllib.parse.urlsplit(target).query else "?") + qs

    port = tracker.port or (443 if tracker.transport is Transport.HTTPS else 80)
    base = dict(url=tracker.url, transport=tracker.transport,
                observed_at=observed_at, vantage=vantage.as_dict(),
                sent_user_agent=cfg.user_agent, used_synthetic_infohash=True)

    try:
        res = _resolve(tracker.host, port, socket.SOCK_STREAM, vantage)
    except OSError as e:
        return ProbeResult(network=tracker.network, rung=Rung.NONE, ok=False,
                           failure=Failure.DNS_FAILURE,
                           detail=f"{type(e).__name__}: {e}", **base)

    network, from_net = classify_network_resolved(tracker.network,
                                                  list(res.addresses))
    families = res.families
    addresses = list(res.addresses)

    if not addresses:
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=Rung.NONE, ok=False,
                           failure=Failure.DNS_FAILURE,
                           detail=f"no address for {tracker.host}", **base)
    if not res.usable:
        return ProbeResult(
            network=network, network_reclassified_from=from_net,
            rung=Rung.NO_USABLE_ADDRESS, ok=False,
            failure=Failure.NO_USABLE_ADDRESS,
            detail=(f"resolves only to {list(families)}; this vantage can use "
                    f"{list(vantage.ip_families)}"),
            resolved_ip=res.first, families=families, **base)

    req = urllib.request.Request(full, headers=cfg.headers())
    t0 = time.monotonic()
    try:
        ctx = ssl.create_default_context()
        with urllib.request.urlopen(req, timeout=cfg.timeout, context=ctx) as resp:
            body = resp.read(MAX_BYTES + 1)
            status = resp.status
        rtt = (time.monotonic() - t0) * 1000.0
        return _classify_http(body, status, rtt, base, network, from_net,
                              addresses[0], families)
    except urllib.error.HTTPError as e:
        # A tracker may answer 4xx and still be a tracker; read the body before
        # deciding. A 403 with a bencoded failure inside is a live tracker.
        try:
            body = e.read(MAX_BYTES)
        except Exception:
            body = b""
        rtt = (time.monotonic() - t0) * 1000.0
        return _classify_http(body, e.code, rtt, base, network, from_net,
                              addresses[0], families)
    except urllib.error.URLError as e:
        reason = e.reason
        if isinstance(reason, ssl.SSLError):
            failure, rung = Failure.TLS_FAILURE, Rung.CONNECTED
        elif isinstance(reason, socket.gaierror):
            failure, rung = Failure.DNS_FAILURE, Rung.NONE
        elif isinstance(reason, socket.timeout) or "timed out" in str(reason):
            failure, rung = Failure.TIMEOUT, Rung.DNS
        elif isinstance(reason, ConnectionResetError):
            failure, rung = Failure.RESET, Rung.CONNECTED
        elif isinstance(reason, ConnectionRefusedError):
            failure, rung = Failure.REFUSED, Rung.DNS
        else:
            failure, rung = Failure.REFUSED, Rung.DNS
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=rung, ok=False, failure=failure,
                           detail=f"{type(reason).__name__}: {reason}",
                           resolved_ip=addresses[0], families=families, **base)
    except (socket.timeout, TimeoutError):
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=Rung.DNS, ok=False, failure=Failure.TIMEOUT,
                           detail=f"timeout after {cfg.timeout}s",
                           resolved_ip=addresses[0], families=families, **base)
    except (ConnectionResetError, BrokenPipeError,
            http.client.IncompleteRead, http.client.HTTPException) as e:
        # `CLOSE_MIDWAY`: headers promised more than arrived. That is a
        # transport fault, not a protocol one, and it is not death -- the
        # tracker was there and answering right up to the moment it was not.
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=Rung.TRANSPORT_RESPONSE, ok=False,
                           failure=Failure.RESET,
                           detail=f"{type(e).__name__}: {e}",
                           resolved_ip=addresses[0], families=families, **base)
    except Exception as e:  # the probe itself broke
        return ProbeResult(network=network, network_reclassified_from=from_net,
                           rung=Rung.NONE, ok=False, failure=Failure.PROBE_ERROR,
                           detail=f"{type(e).__name__}: {e}",
                           resolved_ip=addresses[0], families=families, **base)


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
          observed_at: str = UNKNOWN) -> ProbeResult:
    """Probe one tracker. Never raises; every failure becomes a recorded result.

    A tracker this vantage cannot measure is **not probed at all**. Asking and
    failing would produce a timeout that looks exactly like a dead tracker, and
    the record would then say something false about the world rather than
    something true about us.
    """
    cfg = cfg or ProbeConfig()
    vantage = vantage or detect_vantage()

    if not tracker.is_measurable_here:
        return ProbeResult(
            url=tracker.url, transport=tracker.transport,
            network=tracker.network, rung=Rung.NONE, ok=False,
            failure=Failure.UNSUPPORTED,
            detail=tracker.unmeasurable_reason or "not measurable from this vantage",
            observed_at=observed_at, vantage=vantage.as_dict())

    if tracker.transport is Transport.UDP:
        return probe_udp(tracker, cfg, vantage, observed_at)
    if tracker.transport in (Transport.HTTP, Transport.HTTPS):
        return probe_http(tracker, cfg, vantage, observed_at)

    return ProbeResult(
        url=tracker.url, transport=tracker.transport, network=tracker.network,
        rung=Rung.NONE, ok=False, failure=Failure.UNSUPPORTED,
        detail=f"transport {tracker.transport.value} has no prober",
        observed_at=observed_at, vantage=vantage.as_dict())
