"""The domain model: what a tracker *is*, and what may honestly be said about it.

The shape of this module is a finding, not a preference. RULES 3.1
originally tabulated `udp`, `http`, `https`, `ws`/`wss`, `*.i2p`, "yggdrasil
hosts" and `*.onion` as seven values of one variable. Measured against
`ngosang/trackerslist` @ `1e61597` by `experiments/19-scheme-census.py`:

    trackers_all_i2p.txt        schemes present: http (11), udp (2)
    trackers_all_yggdrasil.txt  schemes present: http (1)
    trackers_all_ws.txt         schemes present: wss (3)   -- not ws

An I2P tracker is an ordinary `http://` or `udp://` URL whose *hostname* ends
in `.i2p`. So they are **two** variables:

    transport   how you speak to it          udp / http / https / ws / wss
    network     where it lives, and hence    clearnet / i2p / yggdrasil / onion
                whether we can reach it

Collapsing them is not a tidiness problem. A classifier keyed on scheme alone
sees `http://`, routes an I2P tracker to the clearnet prober, the probe fails,
and the tracker is recorded **dead** -- the exact correctness bug RULES 3.1
forbids and RULES 3 calls "confident wrongness".
"""

from __future__ import annotations

import ipaddress
from dataclasses import dataclass, field
from enum import Enum
from urllib.parse import urlsplit


class Transport(str, Enum):
    """How the probe speaks to a tracker.

    Exactly the five accepted by a real consumer's own validator,
    `references/GerryFerdinandus__bittorrent-tracker-editor/tree/source/code/
    torrent_miscellaneous.pas:393` @ `c5f5b82`, and exactly the five found by
    the census. Two independent sources agreeing is why this set is closed
    rather than open-ended.
    """

    UDP = "udp"
    HTTP = "http"
    HTTPS = "https"
    WS = "ws"
    WSS = "wss"


class Network(str, Enum):
    """Where a tracker lives, and therefore whether this vantage can reach it."""

    CLEARNET = "clearnet"
    I2P = "i2p"
    YGGDRASIL = "yggdrasil"
    ONION = "onion"


class HealthState(str, Enum):
    """RULES 3.3. Six states, and the distinctions between them are the point.

    `UNMEASURABLE` and `ERROR` exist so that the two ways of knowing nothing
    are never reported as death. RULES 2: an absence is not a zero.
    """

    LIVE = "live"
    DEAD = "dead"
    DEGRADED = "degraded"
    UNMEASURABLE = "unmeasurable"   # we cannot measure it from here, ever
    UNKNOWN = "unknown"             # never checked, or too few samples
    ERROR = "error"                 # the probe itself failed


class Rung(str, Enum):
    """the ladder in TODO/measurement.md. Which layer of the ladder a measurement actually reached.

    Recorded on every result because a latency or a liveness flag without its
    rung is unfalsifiable: "the tracker is alive" means nothing until you can
    say which of these was demonstrated.
    """

    NONE = "none"
    DNS = "dns"
    CONNECTED = "connected"                 # TCP connect / UDP datagram sent
    TLS = "tls"
    TRANSPORT_RESPONSE = "transport_response"   # bytes came back
    PROTOCOL_VALID = "protocol_valid"           # bencode parsed / BEP 15 fields
    TRACKER_SEMANTIC = "tracker_semantic"       # it answered as a *tracker*

    # Not a rung. A distinct, non-fatal outcome that must never be read as
    # death: the name resolved but to no address family this probe can use.
    NO_USABLE_ADDRESS = "no_usable_address"


# --- what this vantage can and cannot measure ---------------------------------
#
# Every entry here is backed by a measurement or a structural fact, not by
# expectation. Changing one requires new evidence and a register update.

#: Networks unreachable from a plain GitHub-hosted runner. I2P and Yggdrasil
#: need their own routers; Tor needs a Tor daemon (`C-37`). This is structural,
#: so these are `UNMEASURABLE` without needing a failed probe to "prove" it.
UNREACHABLE_NETWORKS: frozenset[Network] = frozenset(
    {Network.I2P, Network.YGGDRASIL, Network.ONION}
)

#: WebTorrent over WebSocket is a different protocol from the HTTP tracker
#: protocol (`C-36`). It is UNVERIFIED rather than shown impossible: nobody has
#: attempted a handshake. Listed here so the 13 `wss` entries in the corpus are
#: `unmeasurable` by an honest default instead of `dead` by an accident.
UNSUPPORTED_TRANSPORTS: frozenset[Transport] = frozenset(
    {Transport.WS, Transport.WSS}
)

#: Yggdrasil's address range, `0200::/7`. Kept here as the single definition so
#: it is checkable rather than folklore.
YGGDRASIL_NET = ipaddress.ip_network("0200::/7")

#: Transports whose scrape endpoint requires an info_hash. Read from BEP 15's
#: message tables: the UDP scrape request carries `info_hash` at offset
#: 16 + 20*n, while a UDP *connect* has no such field at all and BEP 48's HTTP
#: scrape takes it as a query parameter. So a UDP scrape is strictly more
#: intrusive than a UDP connect and needs a synthetic infohash (RULES 4).
SCRAPE_REQUIRES_INFOHASH: frozenset[Transport] = frozenset({Transport.UDP})


def classify_network(host: str) -> Network:
    """Which network a hostname belongs to.

    Deliberately independent of the URL scheme -- that is the whole point of
    this module.

    **Known limitation, measured.** This under-detects Yggdrasil. ngosang's
    single yggdrasil entry is `http://yggtracker.i2p.rocks:80/announce`, an
    ordinary hostname that resolves to a Yggdrasil address; only the `_ip`
    variant of that list exposes the `0200::/7` literals. Detecting the
    hostname form requires DNS resolution, which is a *time-varying inference*
    (the three dedup questions in src/trackers/dedup.py point 3) and belongs to the health checker, where the
    resolved address and its timestamp can be recorded as evidence. A pure
    URL classifier must not pretend to it.

    Note also that `.i2p` is matched as a suffix of the *whole* host, so
    `yggtracker.i2p.rocks` is correctly clearnet-by-URL and not I2P.
    """
    h = host.strip().strip("[]").lower().rstrip(".")
    if h.endswith(".i2p"):
        return Network.I2P
    if h.endswith(".onion"):
        return Network.ONION
    try:
        ip = ipaddress.ip_address(h)
    except ValueError:
        return Network.CLEARNET
    if ip.version == 6 and ip in YGGDRASIL_NET:
        return Network.YGGDRASIL
    return Network.CLEARNET


class InvalidTracker(ValueError):
    """A candidate string is not a usable tracker URL. Carries the reason.

    Rejections are auditable (RULES 3.10): a tracker that disappears from
    the dataset must be explainable, so the reason travels with the exception.
    """


@dataclass(frozen=True, slots=True)
class Tracker:
    """One tracker endpoint, canonicalised.

    Frozen because identity must not drift after construction: deduplication
    and ordering both key on it, and RULES 3.6 requires determinism.
    """

    url: str
    transport: Transport
    network: Network
    host: str
    port: int | None
    path: str
    query: str = ""

    @property
    def is_measurable_here(self) -> bool:
        """Whether this vantage can measure it *at all*.

        False does not mean dead. It means the only honest health state is
        `UNMEASURABLE` (RULES 3.1 requirement 1).
        """
        return (self.network not in UNREACHABLE_NETWORKS
                and self.transport not in UNSUPPORTED_TRANSPORTS)

    @property
    def unmeasurable_reason(self) -> str | None:
        """Why it cannot be measured, for the record. `None` if it can be."""
        if self.network in UNREACHABLE_NETWORKS:
            return f"network {self.network.value} requires a router this vantage does not run"
        if self.transport in UNSUPPORTED_TRANSPORTS:
            return f"transport {self.transport.value} speaks WebTorrent, unverified (C-36)"
        return None

    @property
    def scrape_url(self) -> str | None:
        """The scrape endpoint, per BEP 48, or `None` if the convention does not apply.

        BEP 48, re-read 2026-08-31 (`C-66`): "locating the string `announce` in
        the path section of the announce URL and replacing it with the string
        `scrape`. Performing a scrape request to URLs that are not determined by
        this method are outside of the scope of this specification."

        Note this is the *path section* and the *string* `announce` -- not "the
        final `/announce`", which is how `C-35` was originally worded. The
        difference is visible for `/announce.php`, which correctly becomes
        `/scrape.php`.

        **The match must start a path component.** A bare substring replace
        turns `/announcements/feed` into `/scrapements/feed` -- an endpoint no
        tracker serves, whose 404 would then be recorded against the tracker
        rather than against our guess. `Azathothas/bit-cli`'s
        `crates/bit-cli-core/src/tracker.rs:695` avoids the same trap by
        anchoring on the last path component; this anchors on a component
        boundary, which additionally keeps `/announce/<passkey>` working.

        Returns `None` when no path component begins with `announce`, because
        BEP 48 puts such URLs explicitly outside its scope; guessing one would
        invent an endpoint and then report its absence as a tracker defect.
        """
        marker = -1
        needle = "/announce"
        start = 0
        while True:
            found = self.path.find(needle, start)
            if found == -1:
                break
            tail = self.path[found + len(needle):]
            # `/announce` must end the component or be followed by a separator
            # or an extension. `/announcements` is a different word.
            if tail == "" or tail[0] in "/.":
                marker = found
                break
            start = found + 1
        if marker == -1:
            return None
        new_path = (self.path[:marker] + "/scrape"
                    + self.path[marker + len(needle):])
        netloc = self._netloc()
        q = f"?{self.query}" if self.query else ""
        return f"{self.transport.value}://{netloc}{new_path}{q}"

    def _netloc(self) -> str:
        host = f"[{self.host}]" if ":" in self.host else self.host
        return f"{host}:{self.port}" if self.port is not None else host

    def sort_key(self) -> tuple:
        """Deterministic, total ordering. RULES 3.6 and invariant I3/I6.

        Ordering never depends on input order, insertion order, or hashing --
        all three vary between runs and would break byte-identical output.
        """
        return (self.transport.value, self.network.value, self.host,
                self.port if self.port is not None else -1, self.path, self.query)
