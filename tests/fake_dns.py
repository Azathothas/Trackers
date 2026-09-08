"""The DNS oracle: resolvers we control, including broken ones.

T-032. `fake_tracker.py` is the same idea for trackers, and the reason is the
same one RULES 2 states: an absence is not a zero. A BEP 34 lookup that returns
nothing may mean the operator published no record, or it may mean the DNS
client has been broken since Tuesday, and those are indistinguishable without a
responder that is known to answer.

⭐ **The seam this exercises is the address, not the logic.** A test points
`Bep34Config.resolvers` at loopback and the *production* wire code -- the same
encoder, the same parser, the same bounds -- sends a real query and reads a
real response. Replacing the resolver with a stub object would leave the code
that actually ships untested, which is `code.md`'s rule about the production
default of an injectable seam.

WHAT IT CAN BE TOLD TO DO

The hostile cases are the point, because each is a way an opt-out could fail
silently in production:

    NXDOMAIN        a definitive "no record exists"
    SERVFAIL        a failure to answer, which is never consent
    SILENT          a resolver that does not reply at all
    TRUNCATE_UDP    sets the truncation bit over UDP and answers in full over
                    TCP, which is the case that decides whether a long record
                    is read or silently halved
    WRONG_ID        an answer to a question we did not ask
    GARBAGE         bytes that are not a DNS message
    POINTER_LOOP    a compression pointer aimed at itself

⚠ UDP and TCP are bound to the **same port number** deliberately: the client's
truncation fallback reconnects on the port it queried, so a server that moved
would test a path production does not have.

No test in this file touches the network: everything binds loopback, and
`_bind_pair` explains why the port is not always an ephemeral one.
"""

from __future__ import annotations

import ipaddress
import random
import socket
import struct
import threading
from enum import Enum

__all__ = ["DnsBehaviour", "FakeDnsServer", "encode_response", "resolver_for"]

_TYPE_A = 1
_TYPE_TXT = 16
_TYPE_AAAA = 28
_CLASS_IN = 1

#: Which record types this oracle serves. TXT is BEP 34's question and the
#: addresses are T-037's: the probe asks for A and AAAA when this host's
#: resolver has failed, and a resolver that answered TXT to that question would
#: leave the divergence path untested.
_SERVED = (_TYPE_A, _TYPE_TXT, _TYPE_AAAA)


class DnsBehaviour(str, Enum):
    """How the fake resolver misbehaves. `ANSWER` is the honest one."""

    ANSWER = "answer"
    NXDOMAIN = "nxdomain"
    SERVFAIL = "servfail"
    SILENT = "silent"
    TRUNCATE_UDP = "truncate_udp"
    WRONG_ID = "wrong_id"
    GARBAGE = "garbage"
    POINTER_LOOP = "pointer_loop"


def _encode_name(name: str) -> bytes:
    out = bytearray()
    for label in name.strip(".").split("."):
        raw = label.encode("ascii")
        out.append(len(raw))
        out += raw
    out.append(0)
    return bytes(out)


def _encode_txt_rdata(text: str) -> bytes:
    """One TXT record as a sequence of character-strings.

    Anything over 255 bytes is necessarily split, which is exactly the shape
    that catches a reader that keeps only the first string.
    """
    raw = text.encode("utf-8")
    parts = [raw[i:i + 255] for i in range(0, len(raw), 255)] or [b""]
    return b"".join(bytes([len(p)]) + p for p in parts)


def _encode_rdata(qtype: int, value: str) -> bytes:
    """One record's rdata. An address is packed; anything else is TXT."""
    if qtype in (_TYPE_A, _TYPE_AAAA):
        return ipaddress.ip_address(value).packed
    return _encode_txt_rdata(value)


def encode_response(qid: int, question: bytes, values: list[str],
                    *, qtype: int = _TYPE_TXT, rcode: int = 0,
                    truncated: bool = False, compress: bool = True) -> bytes:
    """Build a response to a question of `qtype`.

    ⭐ **One encoder, several record types**, mirroring the decoder it is
    tested against: `bep34.py` implements one DNS client and parameterises the
    type, and a second encoder here would be the copy-pasted-codec defect T-033
    spent a session removing.

    `compress` points each answer's name back at the question's, which is what
    a real resolver does and what makes the client's pointer handling load
    bearing rather than decorative.
    """
    flags = 0x8180 | (rcode & 0x0F)          # QR + RD + RA
    if truncated:
        flags |= 0x0200
    answers = b"" if truncated else b"".join(
        (b"\xc0\x0c" if compress else question)
        + struct.pack(">HHIH", qtype, _CLASS_IN, 300,
                      len(_encode_rdata(qtype, v)))
        + _encode_rdata(qtype, v)
        for v in values)
    count = 0 if truncated else len(values)
    header = struct.pack(">HHHHHH", qid, flags, 1, count, 0, 0)
    return header + question + struct.pack(">HH", qtype, _CLASS_IN) + answers


class FakeDnsServer:
    """A resolver on loopback, serving UDP and TCP on one port number.

    `records` maps a lowercase hostname to the TXT strings it serves. A name
    that is absent answers NOERROR with no records, which is the ordinary
    "this host published nothing" case and must never be read as a refusal.

    `addresses` maps a lowercase hostname to the addresses it serves, IPv4 and
    IPv6 in one list, sorted onto A and AAAA by what each parses as. That is
    T-037's oracle: a name this host's resolver cannot answer for and a
    resolver we chose can.
    """

    def __init__(self, records: dict[str, list[str]] | None = None,
                 behaviour: DnsBehaviour = DnsBehaviour.ANSWER,
                 addresses: dict[str, list[str]] | None = None,
                 address_behaviour: DnsBehaviour | None = None) -> None:
        self.records = {k.lower(): v for k, v in (records or {}).items()}
        self.addresses = {k.lower(): v for k, v in (addresses or {}).items()}
        self.behaviour = behaviour
        #: How the resolver misbehaves on A and AAAA alone, leaving TXT to
        #: `behaviour`. The two are separable because a probe asks both
        #: questions and the answers are independent: consent is established
        #: first, and only then is an address asked for. A single switch would
        #: stop every probe at the BEP 34 gate and leave the resolution path
        #: untested (T-037).
        self.address_behaviour = address_behaviour
        #: Every question received, as `(name, type)`, so a test can assert a
        #: lookup was cached rather than repeated and can tell a consent query
        #: from an address one. Counting requests is the only way to prove the
        #: per-run cache exists.
        self.questions: list[tuple[str, int]] = []
        self._udp, self._tcp, self.port = self._bind_pair()
        self._tcp.listen(8)
        self._stop = threading.Event()
        self._threads: list[threading.Thread] = []

    @property
    def queries(self) -> list[str]:
        """The names asked about, derived from `questions` rather than kept
        beside it, so the two cannot disagree."""
        return [name for name, _ in self.questions]

    @staticmethod
    def _try_port(port: int) -> tuple[socket.socket, socket.socket, int] | None:
        """Bind both protocols to one port, or clean up and return `None`."""
        udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        tcp = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            udp.bind(("127.0.0.1", port))
            tcp.bind(("127.0.0.1", udp.getsockname()[1]))
        except OSError:
            udp.close()
            tcp.close()
            return None
        return udp, tcp, udp.getsockname()[1]

    @classmethod
    def _bind_pair(cls, attempts: int = 60) -> tuple[socket.socket, socket.socket, int]:
        """Bind UDP and TCP to the *same* port number.

        ⚠ **An ephemeral port free for UDP is not necessarily free for TCP.**
        Assuming it was made this oracle fail on roughly one run in three, which
        reads as a broken gate rather than a broken fixture. Measured on the
        Windows 11 host this was written on, 2026-09-05:

            netsh int ipv4 show excludedportrange protocol=tcp   25 ranges
            netsh int ipv4 show excludedportrange protocol=udp   23 ranges

        Both sit inside the 49152-65535 dynamic range, they are **different
        sets**, and each excluded block is about 100 ports wide with some pairs
        adjacent. So neither protocol's allocator can be trusted to pick a port
        the other will accept.

        ⭐ **Retrying `bind(0)` does not fix it, and that is the part worth
        knowing**: Windows hands out ephemeral ports roughly sequentially, so
        consecutive attempts walk *through* one excluded block rather than away
        from it, and twenty tries in a row all failed with `WinError 10013`.
        The retries have to be decorrelated, which is why the fallback picks a
        random port instead of asking for another ephemeral one.

        `SO_REUSEADDR` is deliberately not set: on Windows it permits binding a
        port another socket already holds, which would hide the collision this
        exists to resolve.
        """
        bound = cls._try_port(0)
        if bound is not None:
            return bound
        for _ in range(attempts):
            bound = cls._try_port(random.randint(49152, 65535))
            if bound is not None:
                return bound
        raise OSError(
            f"could not bind UDP and TCP to one loopback port in {attempts} "
            f"attempts; see this method's note on excluded port ranges")

    # -- lifecycle -------------------------------------------------------------
    def __enter__(self) -> "FakeDnsServer":
        self.start()
        return self

    def __exit__(self, *exc: object) -> None:
        self.stop()

    def start(self) -> None:
        for target in (self._serve_udp, self._serve_tcp):
            t = threading.Thread(target=target, daemon=True)
            t.start()
            self._threads.append(t)

    def stop(self) -> None:
        self._stop.set()
        for sock in (self._udp, self._tcp):
            try:
                sock.close()
            except OSError:
                pass
        for t in self._threads:
            t.join(timeout=2.0)

    # -- serving ---------------------------------------------------------------
    def _serve_udp(self) -> None:
        self._udp.settimeout(0.2)
        while not self._stop.is_set():
            try:
                data, peer = self._udp.recvfrom(4096)
            except (socket.timeout, TimeoutError):
                continue
            except OSError:
                return
            reply = self._respond(data, over_tcp=False)
            if reply is not None:
                try:
                    self._udp.sendto(reply, peer)
                except OSError:
                    return

    def _serve_tcp(self) -> None:
        self._tcp.settimeout(0.2)
        while not self._stop.is_set():
            try:
                conn, _ = self._tcp.accept()
            except (socket.timeout, TimeoutError):
                continue
            except OSError:
                return
            with conn:
                try:
                    conn.settimeout(2.0)
                    header = conn.recv(2)
                    if len(header) < 2:
                        continue
                    (length,) = struct.unpack(">H", header)
                    data = conn.recv(length)
                    reply = self._respond(data, over_tcp=True)
                    if reply is not None:
                        conn.sendall(struct.pack(">H", len(reply)) + reply)
                except OSError:
                    continue

    def _respond(self, data: bytes, *, over_tcp: bool) -> bytes | None:
        if len(data) < 12:
            return None
        (qid,) = struct.unpack(">H", data[:2])
        question = data[12:]
        name, qtype = self._question(question)
        self.questions.append((name, qtype))

        behaviour = self.behaviour
        if qtype in (_TYPE_A, _TYPE_AAAA) and self.address_behaviour is not None:
            behaviour = self.address_behaviour
        if behaviour is DnsBehaviour.SILENT:
            return None
        if behaviour is DnsBehaviour.GARBAGE:
            return b"this is not a dns message"
        if behaviour is DnsBehaviour.POINTER_LOOP:
            # A pointer at offset 12 aimed at offset 12: reading the name jumps
            # to itself forever unless the client bounds it.
            header = struct.pack(">HHHHHH", qid, 0x8180, 1, 1, 0, 0)
            return header + b"\xc0\x0c" + struct.pack(">HH", qtype, _CLASS_IN)
        if behaviour is DnsBehaviour.WRONG_ID:
            qid = (qid + 1) & 0xFFFF

        question_wire = _encode_name(name) + struct.pack(">HH", qtype, _CLASS_IN)
        stripped = question_wire[:-4]
        if behaviour is DnsBehaviour.NXDOMAIN:
            return encode_response(qid, stripped, [], qtype=qtype, rcode=3)
        if behaviour is DnsBehaviour.SERVFAIL:
            return encode_response(qid, stripped, [], qtype=qtype, rcode=2)
        values = self._values_for(name, qtype)
        if behaviour is DnsBehaviour.TRUNCATE_UDP and not over_tcp:
            return encode_response(qid, stripped, values, qtype=qtype,
                                   truncated=True)
        return encode_response(qid, stripped, values, qtype=qtype)

    def _values_for(self, name: str, qtype: int) -> list[str]:
        """What this name has of the type asked for.

        ⛔ **An A question is never answered with an AAAA record.** A resolver
        that answered whatever it held would hide the type check `bep34.py`
        makes before believing an answer, and that check is what stops an
        unrelated record being read as the operator's refusal.
        """
        if qtype == _TYPE_TXT:
            return self.records.get(name, [])
        if qtype not in _SERVED:
            return []
        want = 4 if qtype == _TYPE_A else 6
        return [a for a in self.addresses.get(name, [])
                if ipaddress.ip_address(a).version == want]

    @staticmethod
    def _question(question: bytes) -> tuple[str, int]:
        """The name asked about and the type asked for.

        A question with no readable type is treated as TXT, which is the only
        type this oracle served before T-037 and keeps a malformed question
        answering the way it always did.
        """
        labels: list[str] = []
        i = 0
        while i < len(question):
            length = question[i]
            if length == 0:
                break
            labels.append(question[i + 1:i + 1 + length].decode("ascii", "replace"))
            i += 1 + length
        qtype = _TYPE_TXT
        if i + 3 <= len(question):
            (qtype,) = struct.unpack(">H", question[i + 1:i + 3])
        return ".".join(labels).lower(), qtype


def resolver_for(dns: FakeDnsServer, timeout: float = 1.5) -> object:
    """A `Resolver` that asks this oracle and nothing else.

    Here rather than in a test file because two suites need it and a second
    copy would be a second place for the port or the timeout to be wrong. The
    import is inside the function so this module stays importable without
    `src/` on the path.
    """
    from trackers.bep34 import Bep34Config, Resolver
    return Resolver(Bep34Config(resolvers=("127.0.0.1",), port=dns.port,
                                timeout=timeout))
