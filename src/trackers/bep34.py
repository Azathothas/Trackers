"""BEP 34: the operator's refusal, read before anything is contacted.

T-032. RULES 4 makes one thing absolute -- an operator must be able to stop
this project without knowing it exists -- and names two routes. Asking is
implemented in `exclusion.py`. This is the other one, and it is the one that
matters, because it needs no contact with us at all: the operator publishes a
DNS TXT record and we never open a socket.

RULES 4 also states the consequence of it not existing: **until it does, no
corpus-wide probe runs.** That is why this module is P0 and why it gates the
probe rather than filtering its output.

WHAT THE SPECIFICATION ACTUALLY SAYS

Read from `https://www.bittorrent.org/beps/bep_0034.html` on 2026-09-05, at the
version that page publishes as its own: `9c5c1dd1b372`, Last-Modified
2016-07-21. Quoted rather than recalled, because every word of it is
load-bearing here:

    The contents of the TXT records are case-sensitive and consist of one or
    more words separated by spaces.

    BITTORRENT   should always be the first word. The presence of a record
                 beginning with this word indicates that the host is only
                 running trackers on the ports explicitly indicated in the
                 following words.
    UDP:X        there is a tracker running on UDP port X.
    TCP:X        there is a tracker running on TCP port X.

    Unrecognized words are silently ignored.

    "BITTORRENT"            The host is not running any trackers.
    "BITTORRENT DENY ALL"   Handled exactly like the previous example. The
                            words "DENY" and "ALL" are ignored.

So the record is **not** a deny-list. It is an exhaustive allow-list, and the
empty allow-list is the denial. That distinction is the whole mechanism: a
record naming `UDP:1337` denies every other port on that host by saying
nothing about them, and an implementation that looks for the word "DENY"
honours only the readable spelling of the denial and misses the normative one.

THREE DECISIONS THAT ARE NOT THE SPECIFICATION'S

**1. We do not follow the record to a working port.** The spec tells a
*client* that finds a dead URL to retry on an advertised port. This project is
not a client; it measures the endpoint a list published. Retrying elsewhere
would report the health of an endpoint nobody listed, so an unadvertised
endpoint is skipped and recorded, never redirected. `model.Tracker.scrape_url`
refuses to invent an endpoint for the same reason.

**2. A DNS failure is not consent.** An unresolvable or unparseable lookup
yields `UNDETERMINED`, and an undetermined tracker is **skipped**, not probed.
Being wrong in that direction costs a row of data; being wrong in the other
costs somebody who explicitly asked not to be contacted. `code.md`'s
worst-case rule decides it, and newTrackon issue #316 is the recorded case of
it failing the other way -- opt-outs silently not honoured because a resolver
misbehaved, which is the worst way for a refusal to fail.

**3. Public resolvers, not the host's.** That same issue is why: the
production instance's internal resolvers did not follow CNAMEs, so records
that existed were never seen. A public recursive resolver does the CNAME
chasing and hands back the TXT it found. ⚠ The cost is real and is recorded
rather than hidden: the tracker hostnames we look up are visible to whichever
public resolver answers.

THERE IS NO SWITCH THAT TURNS THIS OFF

`Bep34Config` can point the resolver at a different address -- that is how the
loopback oracle in `tests/fake_dns.py` exercises this wire code with no network
-- but nothing anywhere disables the consultation. A flag that skipped it would
be a documented route to contacting a host that refused us, and RULES 4.1's one
immovable line is that no identity and no option is ever used to evade an
exclusion already given.

WHY THERE IS A DNS CLIENT IN HERE

D1 and RULES 12: standard library only. `socket.getaddrinfo` resolves names to
addresses and cannot ask for a TXT record, so the wire format is implemented
here, the same way `bencode.py` and `bep15.py` implement theirs. Every response
is hostile input (RULES 5.1): sizes are bounded, compression pointers cannot
loop, the transaction id and the echoed question are checked before a single
byte of an answer is believed, and nothing is padded or guessed.
"""

from __future__ import annotations

import ipaddress
import os
import socket
import struct
from dataclasses import dataclass
from enum import Enum

__all__ = [
    "Decision", "Bep34Record", "Bep34Result", "Bep34Config", "Resolver",
    "PUBLIC_RESOLVERS", "MARKER", "parse_record", "protocol_for_transport",
]

#: The first word. Case-sensitive, because the specification says the contents
#: are: a record spelled `bittorrent` is not a BEP 34 record and reading it as
#: one would invent a refusal the operator did not publish.
MARKER = "BITTORRENT"

#: Three recursive resolvers run by three different operators. Diversity of
#: operator is the point rather than redundancy: the failure being guarded
#: against is one operator's resolver answering wrongly, and two addresses
#: belonging to the same operator do not guard against it.
PUBLIC_RESOLVERS: tuple[str, ...] = ("1.1.1.1", "8.8.8.8", "9.9.9.9")

_TYPE_TXT = 16
_CLASS_IN = 1

#: DNS response codes we distinguish. `NOERROR` and `NXDOMAIN` are both
#: *answers*: the first may carry a record, the second establishes that the
#: name does not exist and therefore carries none. Everything else is a
#: failure to determine, which is never consent.
_RCODE_NOERROR = 0
_RCODE_NXDOMAIN = 3

#: Bounds. Every one of them exists because the response is somebody else's
#: bytes (RULES 5.2), and each is ours rather than the specification's.
MAX_UDP_RESPONSE = 4096
MAX_TCP_RESPONSE = 16384
MAX_POINTER_JUMPS = 64
MAX_LABELS = 128

#: How many words of a record we will read, and how many endpoints we will
#: keep. The specification bounds neither. A record exceeding these is not
#: truncated into a shorter allow-list -- truncating one would deny endpoints
#: the operator advertised and call it a measurement -- it is `UNDETERMINED`,
#: which skips the tracker and says why.
MAX_WORDS = 64
MAX_ENDPOINTS = 16


class Decision(str, Enum):
    """What the DNS said about contacting one endpoint.

    Three values, not two, because "no record exists" and "we could not find
    out" are different facts with different consequences, and collapsing them
    is how an opt-out fails silently.
    """

    #: No BEP 34 record, or one that advertises this exact endpoint.
    ALLOW = "allow"
    #: A record exists and this endpoint is not in it. Do not contact.
    DENY = "deny"
    #: The lookup did not answer, or answered something we will not guess at.
    #: Skips the tracker. **Never** read as permission.
    UNDETERMINED = "undetermined"


def protocol_for_transport(transport: str) -> str:
    """The BEP 34 word a transport is advertised under.

    The record speaks of transport-layer protocols, not URL schemes: `http`,
    `https`, `ws` and `wss` all ride TCP and are all advertised by `TCP:X`.
    Mapping them one-to-one onto scheme names would look for `HTTPS:443`,
    find nothing, and manufacture a denial out of a spelling.
    """
    return "udp" if transport == "udp" else "tcp"


@dataclass(frozen=True, slots=True)
class Bep34Record:
    """A parsed `BITTORRENT` record. Keeps the raw text; the parse is evidence."""

    raw: str
    #: `(protocol, port)` in the order the operator wrote them, which the
    #: specification says is order of preference. Empty means the host runs no
    #: trackers at all, which is the normative spelling of a denial.
    endpoints: tuple[tuple[str, int], ...]

    @property
    def denies_everything(self) -> bool:
        return not self.endpoints

    def advertises(self, protocol: str, port: int) -> bool:
        return (protocol, port) in self.endpoints


@dataclass(frozen=True, slots=True)
class Bep34Result:
    """The decision, and enough of its provenance to audit it afterwards.

    RULES 3.10: a tracker that vanishes from the output owes the consumer who
    noticed a reason, so the reason is a returned value and not a log line.
    """

    decision: Decision
    detail: str
    host: str
    protocol: str
    port: int
    record: Bep34Record | None = None
    #: Which resolver answered, or `-` when none did.
    resolver: str = "-"

    def as_record(self) -> dict[str, object]:
        """The shape carried into a health record."""
        return {
            "decision": self.decision.value,
            "detail": self.detail,
            "record": self.record.raw if self.record else None,
            "advertised": ([f"{p}:{n}" for p, n in self.record.endpoints]
                           if self.record else None),
            "resolver": self.resolver,
        }


def parse_record(text: str) -> Bep34Record | None:
    """Parse one TXT string. `None` if it is not a BEP 34 record at all.

    Everything about this is case-sensitive and whitespace-separated, per the
    specification. Unrecognised words are ignored, which is what lets
    `BITTORRENT DENY ALL` mean exactly what a bare `BITTORRENT` means: neither
    `DENY` nor `ALL` is a `UDP:`/`TCP:` word, so both records parse to an empty
    allow-list and both deny everything.

    Raises `ValueError` when the record is a BEP 34 record but longer than
    `MAX_WORDS`, because the alternative is a silently shortened allow-list.
    """
    words = text.split()
    if not words or words[0] != MARKER:
        return None
    if len(words) > MAX_WORDS:
        raise ValueError(f"record has {len(words)} words, over the {MAX_WORDS} bound")

    endpoints: list[tuple[str, int]] = []
    for word in words[1:]:
        for prefix, proto in (("UDP:", "udp"), ("TCP:", "tcp")):
            if not word.startswith(prefix):
                continue
            digits = word[len(prefix):]
            # `isdigit` rather than `int()` in a `try`: `int(" 80")`,
            # `int("+80")` and `int("8_0")` all succeed and none of them is a
            # port the operator wrote.
            if not digits.isdigit():
                break
            port = int(digits)
            if not 0 < port < 65536:
                break
            pair = (proto, port)
            if pair not in endpoints:      # a repeat is not a second endpoint
                endpoints.append(pair)
            break
    if len(endpoints) > MAX_ENDPOINTS:
        raise ValueError(
            f"record advertises {len(endpoints)} endpoints, over the "
            f"{MAX_ENDPOINTS} bound")
    return Bep34Record(raw=text, endpoints=tuple(endpoints))


# --- the DNS wire format ------------------------------------------------------
#
# RFC 1035 sections 4.1.1 to 4.1.4 for the message, and 3.3.14 for TXT rdata.
# Only what a TXT query needs is implemented; anything unimplemented raises
# rather than being approximated.

class _Malformed(ValueError):
    """The response is not a DNS message we will act on. Never guessed at."""


def _encode_name(host: str) -> bytes:
    """A hostname as DNS labels.

    The length limits are the protocol's (RFC 1035 section 2.3.4) and a name
    that breaks them cannot have a record, so this refuses rather than sending
    something a resolver would reject anyway.
    """
    name = host.strip().rstrip(".")
    if not name:
        raise _Malformed("empty hostname")
    out = bytearray()
    for label in name.split("."):
        raw = label.encode("idna") if any(ord(c) > 127 for c in label) else label.encode("ascii")
        if not 0 < len(raw) < 64:
            raise _Malformed(f"label {label!r} is {len(raw)} bytes")
        out.append(len(raw))
        out += raw
    out.append(0)
    if len(out) > 255:
        raise _Malformed(f"name is {len(out)} bytes")
    return bytes(out)


def _read_name(buf: bytes, offset: int) -> tuple[bytes, int]:
    """Read a possibly-compressed name. Returns the name and the next offset.

    Two bounds, and both are load-bearing against a hostile response: a
    compression pointer may only point *backwards*, and the number of jumps is
    capped. Without them a response can point a name at itself and this loops
    until the process is killed -- a denial of service that costs the sender
    one packet.
    """
    labels: list[bytes] = []
    jumps = 0
    here = offset
    after: int | None = None
    while True:
        if here >= len(buf):
            raise _Malformed("name runs past end of message")
        length = buf[here]
        if length == 0:
            here += 1
            break
        if length & 0xC0 == 0xC0:
            if here + 1 >= len(buf):
                raise _Malformed("truncated compression pointer")
            target = ((length & 0x3F) << 8) | buf[here + 1]
            if target >= here:
                raise _Malformed("compression pointer does not point backwards")
            jumps += 1
            if jumps > MAX_POINTER_JUMPS:
                raise _Malformed("too many compression pointers")
            if after is None:
                after = here + 2
            here = target
            continue
        if length & 0xC0:
            raise _Malformed(f"reserved label type {length:#x}")
        end = here + 1 + length
        if end > len(buf):
            raise _Malformed("label runs past end of message")
        labels.append(buf[here + 1:end])
        if len(labels) > MAX_LABELS:
            raise _Malformed("too many labels")
        here = end
    return b".".join(labels).lower(), (after if after is not None else here)


def _build_query(qid: int, qname: bytes) -> bytes:
    """A single recursion-desired TXT question. No EDNS0.

    Leaving EDNS0 out keeps the response under 512 bytes or sets the truncation
    bit, and truncation is handled by retrying over TCP. The alternative --
    advertising a large buffer and hoping -- is what turns a long TXT record
    into a silently missing one.
    """
    flags = 0x0100  # RD
    header = struct.pack(">HHHHHH", qid, flags, 1, 0, 0, 0)
    return header + qname + struct.pack(">HH", _TYPE_TXT, _CLASS_IN)


def _parse_response(buf: bytes, qid: int, qname: bytes) -> tuple[int, bool, list[str]]:
    """Return `(rcode, truncated, txt_strings)`.

    Nothing in the answer section is believed until the header's transaction id
    and the echoed question both match what was sent. An off-path forger has to
    guess 16 bits of id plus the source port to be read at all; without the
    check it would need neither.
    """
    if len(buf) < 12:
        raise _Malformed(f"response is {len(buf)} bytes, under a 12-byte header")
    rid, flags, qdcount, ancount, _, _ = struct.unpack(">HHHHHH", buf[:12])
    if rid != qid:
        raise _Malformed("transaction id does not match the query")
    if not flags & 0x8000:
        raise _Malformed("response has the query bit set")
    truncated = bool(flags & 0x0200)
    rcode = flags & 0x000F
    if qdcount != 1:
        raise _Malformed(f"qdcount is {qdcount}, expected 1")

    offset = 12
    echoed, offset = _read_name(buf, offset)
    if offset + 4 > len(buf):
        raise _Malformed("question runs past end of message")
    qtype, qclass = struct.unpack(">HH", buf[offset:offset + 4])
    offset += 4
    expected, _ = _read_name(qname, 0)
    if echoed != expected or qtype != _TYPE_TXT or qclass != _CLASS_IN:
        raise _Malformed("response does not echo the question that was asked")

    texts: list[str] = []
    for _ in range(ancount):
        if offset >= len(buf):
            raise _Malformed("answer count exceeds the message")
        _, offset = _read_name(buf, offset)
        if offset + 10 > len(buf):
            raise _Malformed("resource record header runs past end of message")
        rtype, rclass, _, rdlength = struct.unpack(">HHIH", buf[offset:offset + 10])
        offset += 10
        end = offset + rdlength
        if end > len(buf):
            raise _Malformed("rdata runs past end of message")
        if rtype == _TYPE_TXT and rclass == _CLASS_IN:
            texts.append(_read_txt_rdata(buf, offset, end))
        # Any other type -- a CNAME the resolver chased, a signature -- is
        # simply not this question's answer. Skipping it is not a guess.
        offset = end
    return rcode, truncated, texts


def _read_txt_rdata(buf: bytes, start: int, end: int) -> str:
    """Concatenate a TXT record's character-strings (RFC 1035 section 3.3.14).

    A TXT record is a *sequence* of length-prefixed strings, and a value longer
    than 255 bytes is necessarily split across several. Reading only the first
    is a real defect rather than a theoretical one: it silently shortens long
    records, and a shortened `BITTORRENT ...` record parses to a different
    allow-list than the operator published.
    """
    parts: list[bytes] = []
    here = start
    while here < end:
        length = buf[here]
        here += 1
        if here + length > end:
            raise _Malformed("character-string runs past its rdata")
        parts.append(buf[here:here + length])
        here += length
    # BEP 34 records are ASCII words. `surrogateescape` keeps a non-ASCII TXT
    # record readable in the detail line instead of raising, and it can never
    # produce the `BITTORRENT` marker by accident.
    return b"".join(parts).decode("ascii", errors="surrogateescape")


@dataclass(frozen=True, slots=True)
class Bep34Config:
    """Where to ask, and how long to wait. Bounded by construction (RULES 5.2).

    `resolvers` is an argument so the loopback oracle can exercise this exact
    wire code with no network. It is **not** a way to skip the consultation:
    every path through `Resolver.consult` ends in a `Decision`, and the only
    one that permits contact is `ALLOW`.
    """

    resolvers: tuple[str, ...] = PUBLIC_RESOLVERS
    port: int = 53
    timeout: float = 3.0


@dataclass(frozen=True, slots=True)
class _HostAnswer:
    """What DNS established about a host, before any port is considered.

    A separate type rather than a tuple carrying a `Decision`, because one
    host's record answers differently for two of its own ports: `UDP:1337`
    allows `udp://host:1337` and denies `udp://host:6969`. A per-host value
    that already called itself `DENY` or `ALLOW` would be read as the verdict
    it is not.
    """

    #: `None` means DNS answered definitively that no BEP 34 record exists.
    record: Bep34Record | None
    #: True when the lookup did not establish anything. Never consent.
    undetermined: bool
    detail: str
    resolver: str = "-"


class Resolver:
    """Asks the question once per host per run, and remembers the answer.

    The cache is per instance and lives for the run. RULES 15.2 bounds the
    noise this project makes; a corpus with many URLs on one host would
    otherwise ask the same question once per URL, which is load generated for
    nothing.

    ⚠ It is deliberately **not** a TTL cache. Within one run the record is
    treated as fixed; across runs nothing is remembered, so an operator who
    publishes a denial is honoured on the next run rather than whenever a
    cached entry happened to expire.
    """

    def __init__(self, config: Bep34Config | None = None) -> None:
        self.config = config or Bep34Config()
        self._cache: dict[str, _HostAnswer] = {}

    # -- the lookup ------------------------------------------------------------
    def _query_one(self, resolver: str, qname: bytes, qid: int) -> tuple[int, list[str]]:
        """One resolver, UDP then TCP if the answer was truncated."""
        query = _build_query(qid, qname)
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.settimeout(self.config.timeout)
        try:
            # `connect` so the kernel drops datagrams from anyone else. A
            # `recvfrom` plus an address comparison is the same check written
            # where it can be forgotten.
            s.connect((resolver, self.config.port))
            s.send(query)
            data = s.recv(MAX_UDP_RESPONSE)
        finally:
            s.close()
        rcode, truncated, texts = _parse_response(data, qid, qname)
        if not truncated:
            return rcode, texts
        return self._query_tcp(resolver, qname, qid)

    def _query_tcp(self, resolver: str, qname: bytes, qid: int) -> tuple[int, list[str]]:
        """RFC 1035 section 4.2.2: the same message behind a 2-byte length.

        Reached only when a response set the truncation bit. Without it, a TXT
        record too long for a datagram would arrive cut in half and be parsed
        into an allow-list the operator never wrote.
        """
        query = _build_query(qid, qname)
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.settimeout(self.config.timeout)
        try:
            s.connect((resolver, self.config.port))
            s.sendall(struct.pack(">H", len(query)) + query)
            header = _recv_exactly(s, 2)
            (length,) = struct.unpack(">H", header)
            if length > MAX_TCP_RESPONSE:
                raise _Malformed(f"TCP response declares {length} bytes")
            data = _recv_exactly(s, length)
        finally:
            s.close()
        rcode, still_truncated, texts = _parse_response(data, qid, qname)
        if still_truncated:
            raise _Malformed("TCP response is still truncated")
        return rcode, texts

    def lookup(self, host: str) -> _HostAnswer:
        """Establish what record, if any, governs a host. Asks no port question.

        Resolvers are tried in order and the **first definitive answer wins**,
        rather than querying all three and honouring any denial among them.
        The trade is recorded because it is a real one: querying all three
        would catch a divergent resolver, and it would also triple the DNS load
        this project generates against the whole corpus (RULES 15.2), to catch
        a divergence `experiments/04` measured at 0 of 17 names. If T-007 ever
        measures meaningful divergence, this is the decision it reopens.
        """
        try:
            qname = _encode_name(host)
        except _Malformed as e:
            return _HostAnswer(None, True, f"hostname not queryable: {e}")

        failures: list[str] = []
        for resolver in self.config.resolvers:
            # 16 fresh random bits per query. Reusing an id across resolvers
            # would let one answer be replayed as another's.
            qid = struct.unpack(">H", os.urandom(2))[0]
            try:
                rcode, texts = self._query_one(resolver, qname, qid)
            except (OSError, _Malformed, struct.error) as e:
                failures.append(f"{resolver}: {type(e).__name__}: {e}")
                continue

            if rcode == _RCODE_NXDOMAIN:
                # The name does not exist, so no record exists. A definitive
                # answer. Resolution for the probe itself will fail next and be
                # recorded as the DNS failure it is.
                return _HostAnswer(None, False,
                                   f"NXDOMAIN from {resolver}; no record", resolver)
            if rcode != _RCODE_NOERROR:
                failures.append(f"{resolver}: rcode {rcode}")
                continue

            records: list[Bep34Record] = []
            for text in texts:
                try:
                    parsed = parse_record(text)
                except ValueError as e:
                    return _HostAnswer(
                        None, True,
                        f"unparseable BEP 34 record from {resolver}: {e}", resolver)
                if parsed is not None:
                    records.append(parsed)

            if not records:
                return _HostAnswer(
                    None, False,
                    f"no BEP 34 record among {len(texts)} TXT record(s) from "
                    f"{resolver}", resolver)

            if len({r.endpoints for r in records}) > 1:
                # Two records disagreeing is an ambiguous preference, and DNS
                # does not order an answer set. Picking one would make the
                # outcome depend on what the resolver happened to send first,
                # which RULES 3.6 forbids, and picking wrongly against an
                # operator is the expensive direction.
                return _HostAnswer(
                    None, True,
                    f"{len(records)} conflicting BEP 34 records from {resolver}",
                    resolver)
            return _HostAnswer(records[0], False, f"record from {resolver}", resolver)

        detail = ("no resolver answered: " + "; ".join(failures) if failures
                  else "no resolvers configured")
        return _HostAnswer(None, True, detail)

    def consult(self, host: str, protocol: str, port: int) -> Bep34Result:
        """Whether this endpoint may be contacted. The only entry point.

        `DENY` and `UNDETERMINED` both mean *do not open a socket*; they are
        distinct because one is the operator's decision and the other is our
        own failure, and a run that skipped a thousand trackers needs to say
        which of those happened.
        """
        try:
            ipaddress.ip_address(host.strip().strip("[]"))
        except ValueError:
            pass
        else:
            # BEP 34 is a record on a *hostname*. A URL written as an address
            # has no name to ask about, so there is no record and no query to
            # send. ⚠ This is a real gap and is recorded rather than papered
            # over: an operator who denies `tracker.example` is not protected
            # on a corpus entry that names the same machine by its address.
            # Closing it needs a denial to propagate to siblings sharing a
            # resolved address, which the probe only learns *after* resolving
            # -- noted on T-032 rather than half-built here.
            return Bep34Result(
                decision=Decision.ALLOW, host=host, protocol=protocol,
                port=port, record=None,
                detail="host is an IP literal; BEP 34 is keyed on a hostname")

        if host not in self._cache:
            self._cache[host] = self.lookup(host)
        answer = self._cache[host]

        base = dict(host=host, protocol=protocol, port=port,
                    resolver=answer.resolver)
        if answer.undetermined:
            return Bep34Result(decision=Decision.UNDETERMINED,
                               detail=answer.detail, record=None, **base)
        record = answer.record
        if record is None:
            return Bep34Result(decision=Decision.ALLOW, detail=answer.detail,
                               record=None, **base)

        detail = answer.detail
        if record.denies_everything:
            return Bep34Result(
                decision=Decision.DENY, record=record,
                detail=(f"{detail}: {record.raw!r} advertises no tracker, so the "
                        f"host runs none"), **base)
        if record.advertises(protocol, port):
            return Bep34Result(
                decision=Decision.ALLOW, record=record,
                detail=f"{detail}: advertises {protocol.upper()}:{port}", **base)
        return Bep34Result(
            decision=Decision.DENY, record=record,
            detail=(f"{detail}: {protocol.upper()}:{port} is not advertised; "
                    f"the record names "
                    f"{', '.join(f'{p.upper()}:{n}' for p, n in record.endpoints)}"),
            **base)


def _recv_exactly(sock: socket.socket, count: int) -> bytes:
    """Read exactly `count` bytes or raise.

    A short read is a failure, never a shorter message: trusting whatever
    arrived is the "trusting a declared length instead of counting what
    arrived" row in `forbidden-patterns.md`.
    """
    chunks: list[bytes] = []
    remaining = count
    while remaining > 0:
        chunk = sock.recv(remaining)
        if not chunk:
            raise _Malformed(f"connection closed with {remaining} bytes outstanding")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)
