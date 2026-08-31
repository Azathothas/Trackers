"""BEP 15, the UDP tracker protocol: message codec and nothing else.

Production home of the codec that used to live inside
`experiments/02-udp-bep15-connect.py`. Same reason as `bencode.py`: an
experiment that is also the production path cannot drift from it.

SPEC, READ RATHER THAN REMEMBERED
    BEP 15, https://www.bittorrent.org/beps/bep_0015.html, fetched 2026-08-29.

      connect request   (16 bytes, and 16 bytes is the whole of it)
        offset 0   u64  protocol_id     0x41727101980   magic constant
        offset 8   u32  action          0               connect
        offset 12  u32  transaction_id  random

      connect response  (>= 16 bytes)
        offset 0   u32  action          0
        offset 4   u32  transaction_id  must equal the one we chose
        offset 8   u64  connection_id

      scrape request    (16 + 20n bytes)
        offset 0   u64  connection_id
        offset 8   u32  action          2               scrape
        offset 12  u32  transaction_id
        offset 16  20b  info_hash       repeated n times

      scrape response   (8 + 12n bytes)
        offset 0   u32  action          2
        offset 4   u32  transaction_id
        offset 8   u32  seeders    \\
        offset 12  u32  completed   |  repeated n times
        offset 16  u32  leechers   /

      error response
        offset 0   u32  action          3
        offset 4   u32  transaction_id
        offset 8   str  human-readable message

THE ETHICALLY LOAD-BEARING FACT OF THIS PROJECT
    The connect request has **no info_hash field**. There is nowhere in those
    16 bytes to put one. A connect therefore cannot express interest in any
    content and cannot enter this host into any swarm's peer list. `info_hash`
    first appears in the ANNOUNCE request at offset 16.

    **This module cannot build an announce.** There is no function here that
    produces one, which is what makes RULES 4's prohibition a property of the
    code rather than a policy somebody has to remember. Adding one would be a
    reviewable change to this file.

    Scrape is different from connect and the difference is measured, not
    assumed: BEP 15's scrape request carries `info_hash` as a **required**
    field, so a UDP scrape is strictly more intrusive than a UDP connect
    (`C-50`, T-022). `build_scrape_request` therefore refuses any info_hash it
    was not told is synthetic -- see its docstring.
"""

from __future__ import annotations

import os
import struct

__all__ = [
    "PROTOCOL_ID",
    "ACTION_CONNECT",
    "ACTION_ANNOUNCE",
    "ACTION_SCRAPE",
    "ACTION_ERROR",
    "CONNECT_REQUEST_SIZE",
    "INFOHASH_SIZE",
    "Bep15Error",
    "build_connect_request",
    "parse_connect_response",
    "build_scrape_request",
    "parse_scrape_response",
    "synthetic_infohash",
]

PROTOCOL_ID = 0x41727101980
ACTION_CONNECT = 0
ACTION_ANNOUNCE = 1  # named for parsing only; nothing here builds one
ACTION_SCRAPE = 2
ACTION_ERROR = 3

CONNECT_REQUEST_SIZE = 16
INFOHASH_SIZE = 20


class Bep15Error(ValueError):
    """A datagram is not a well-formed BEP 15 message."""


def build_connect_request(transaction_id: int) -> bytes:
    """Exactly 16 bytes: protocol_id, action, transaction_id. Nothing else fits."""
    return struct.pack(">QII", PROTOCOL_ID, ACTION_CONNECT, transaction_id)


def parse_connect_response(data: bytes, expect_txid: int) -> tuple[bool, str, int | None]:
    """Return `(ok, detail, connection_id)`.

    Validation order is BEP 15's own: length, then transaction id, then action.

    **Checking the transaction id before trusting the action is the security
    property**, not a style choice. UDP is unauthenticated and trivially
    spoofable; without this check an unsolicited datagram from anywhere on the
    internet would be recorded as "this tracker is alive". The transaction id
    is 32 random bits chosen per attempt, so an off-path attacker has to guess
    it to forge liveness.

    An in-protocol **error** reply returns `ok=False` and a detail beginning
    `BEP15 error response`, which callers treat as a *stronger* liveness signal
    than silence: the tracker parsed our datagram and answered.
    """
    if len(data) < 16:
        if len(data) >= 8:
            action, txid = struct.unpack(">II", data[:8])
            if action == ACTION_ERROR and txid == expect_txid:
                msg = data[8:].decode("utf-8", "replace")
                return False, f"BEP15 error response: {msg!r}", None
        return False, f"short response: {len(data)} bytes (BEP 15 requires >= 16)", None

    action, txid, conn_id = struct.unpack(">IIQ", data[:16])
    if txid != expect_txid:
        return False, f"transaction id mismatch: got {txid}, sent {expect_txid}", None
    if action == ACTION_ERROR:
        return False, f"BEP15 error response: {data[8:].decode('utf-8', 'replace')!r}", None
    if action != ACTION_CONNECT:
        return False, f"unexpected action {action} (expected {ACTION_CONNECT})", None
    return True, f"connection_id=0x{conn_id:016x}", conn_id


def synthetic_infohash() -> bytes:
    """20 random bytes, corresponding to no content, generated per call.

    RULES 4 permits a scrape against a synthetic hash and requires the fact to
    be recorded in the health record. This function is the only supported way
    to obtain an info_hash in this codebase; there is no path that reads one
    from a real torrent.
    """
    return os.urandom(INFOHASH_SIZE)


def build_scrape_request(connection_id: int, transaction_id: int,
                         info_hashes: list[bytes]) -> bytes:
    """A BEP 15 scrape request: 16 header bytes plus 20 bytes per info_hash.

    Refuses an empty list -- a scrape with no hashes is not a defined message --
    and refuses any hash that is not exactly 20 bytes, so a truncated or
    over-long hash cannot be sent by accident.

    The caller is expected to pass `synthetic_infohash()`. T-022's `Prove`
    clause is a test that this path never sends a non-random hash; the
    enforcement is that nothing in this codebase can produce a real one.
    """
    if not info_hashes:
        raise Bep15Error("a scrape request with zero info_hashes is not a BEP 15 message")
    for h in info_hashes:
        if len(h) != INFOHASH_SIZE:
            raise Bep15Error(
                f"info_hash must be exactly {INFOHASH_SIZE} bytes, got {len(h)}")
    head = struct.pack(">QII", connection_id, ACTION_SCRAPE, transaction_id)
    return head + b"".join(info_hashes)


def parse_scrape_response(data: bytes, expect_txid: int,
                          n_hashes: int) -> tuple[bool, str, list[dict] | None]:
    """Return `(ok, detail, rows)` where each row is seeders/completed/leechers.

    Same validation order and the same spoofing argument as
    `parse_connect_response`. A response carrying fewer rows than hashes we
    asked about is reported as a mismatch rather than silently zero-filled --
    an absence is not a zero (RULES 1).
    """
    if len(data) < 8:
        return False, f"short response: {len(data)} bytes (BEP 15 requires >= 8)", None
    action, txid = struct.unpack(">II", data[:8])
    if txid != expect_txid:
        return False, f"transaction id mismatch: got {txid}, sent {expect_txid}", None
    if action == ACTION_ERROR:
        return False, f"BEP15 error response: {data[8:].decode('utf-8', 'replace')!r}", None
    if action != ACTION_SCRAPE:
        return False, f"unexpected action {action} (expected {ACTION_SCRAPE})", None

    body = data[8:]
    available = len(body) // 12
    if available < n_hashes:
        return False, (f"scrape response carries {available} row(s) for "
                       f"{n_hashes} info_hash(es)"), None
    rows = []
    for i in range(n_hashes):
        seeders, completed, leechers = struct.unpack(">III", body[i * 12:(i + 1) * 12])
        rows.append({"seeders": seeders, "completed": completed, "leechers": leechers})
    return True, f"{n_hashes} row(s)", rows
