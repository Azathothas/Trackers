# BEP coverage

Sixty-six issues in the corpus mention a BEP or a protocol feature. This file
tracks what `bit-cli` speaks today and what it does not.

Implemented means there is a test. Inherited means `librqbit` provides it and
`bit-cli` has not verified it independently.

| BEP | What | Status |
| --- | --- | --- |
| 3  | The BitTorrent protocol | inherited |
| 5  | DHT | inherited, not reported (T-052) |
| 6  | Fast extension | **partial** (T-100): the allowed-fast derivation is in `fast_set.rs` and `bench swarm` reads the five messages; nothing sends one, blocked on `librqbit` |
| 7  | IPv6 tracker extension | implemented in `tracker.rs` |
| 9  | Metadata from peers (magnet) | inherited |
| 10 | Extension protocol | implemented in the bridge |
| 11 | PEX | inherited; `--no-pex` reaches nothing (T-181) |
| 12 | Multitracker metadata | implemented in `create`, `edit`, `trackers` |
| 14 | Local service discovery | inherited |
| 15 | UDP tracker protocol | implemented in `tracker.rs` |
| 16 | Superseeding | not implemented (T-082) |
| 17 | HTTP seeding, Hoffman style | implemented in `fetch.rs`, style declared not detected (T-004) |
| 19 | HTTP/FTP seeding, GetRight style | implemented, the headline feature |
| 20 | Peer id conventions | implemented |
| 21 | Extension for partial seeds | implemented in the bridge |
| 23 | Compact peer lists | implemented in `tracker.rs` |
| 27 | Private torrents | implemented in `create`, `edit` |
| 29 | uTP | **not reachable**, no flag enables it (T-101) |
| 33 | DHT scrape | not implemented (T-169) |
| 39 | Updating torrents via feed URL | implemented in `create`, `edit` |
| 44 | DHT store, mutable items | not implemented (T-170) |
| 47 | Padding files | **read only**: parsed and skipped, `create` does not emit them (T-081) |
| 48 | Tracker scrape | implemented in `tracker.rs`, BEP 48 URL convention only (T-065) |
| 51 | DHT infohash indexing | not implemented (T-169) |
| 52 | BitTorrent v2 | not implemented (T-081, T-134) |
| 53 | Magnet file selection, `so=` | implemented in `torrent/magnet.rs` |
| 54 | `lt_donthave` | not implemented (T-167) |
| 55 | Holepunch | not implemented (T-102) |
| MSE/PE | Peer encryption | not implemented (T-163) |
| WebTorrent | WebRTC peers, WSS trackers | not implemented (T-168) |

**Five rows changed on 2026-08-21 and each was wrong in the same direction:
the table described intent rather than the tree.**

- **BEP 29 said "inherited, off by default".** There is no uTP in `bit-cli` at
  all. `ListenerOptions::mode` is never set, so the session stays `TcpOnly`,
  and no flag changes that. `librqbit-utp` 0.7.0 appears in `cargo tree`
  because `librqbit` depends on it, which is not the same thing as a
  capability a user can turn on. "Off by default" reads as a switch; there is
  no switch. See [T-101](#t-101-utp-is-available-but-untested).
- **BEP 47 said "not implemented".** The read side is implemented and tested.
  `torrent/metainfo.rs:107` parses the `attr` key, `:116` `InfoFile::is_padding`
  is the predicate, `storage.rs:1048` and `:1216` never open a padding file
  because it is alignment rather than data, `cmd/files.rs:176` reports it, and
  `torrent/metainfo.rs:825` `padding_files_are_recognised` is the test. What is
  missing is the **write** side: `create` emits no padding files, which is a
  clause of [T-081](create-seed.md).
- **BEP 53 was absent.** `torrent/magnet.rs:39` and `:211` parse the `so=`
  index-range file selection out of a magnet.
- **BEP 7 was absent.** `tracker.rs:493` reads the `peers6` key at 18 bytes
  per entry beside the 6-byte `peers`, with a test at `:873`
  `ipv6_peers_come_back_bracketed`. Worth naming rather than leaving implicit,
  because [T-022](peers.md) and [T-023](peers.md) are both about IPv6 and
  neither could point at the one piece of it that works.
- **BEP 33, 44, 51 and 54, MSE/PE and WebTorrent were absent.** Six gaps the
  corpus named that no row admitted to. They have entries now rather than
  silence.

The lesson is the one [T-032](performance.md) and [T-141](webseed.md) wrote
down: a table is a claim, and a claim needs a symbol. Every row above now
either names one or names the entry that closes it.

---

### T-100 BEP 6 fast extension is not implemented

Source:      https://github.com/ikatson/rqbit/issues/584 (open)
Category:    bep
Priority:    P2
Effort:      L
Status:      **done**, 2026-08-23T04:01Z

Problem:     No `have all`, `have none`, `suggest piece`, `reject request`, or
             `allowed fast`.
Relevance:   Two parts matter here. `have all` and `have none` replace a
             bitfield with two bytes, which matters on a torrent with a million
             pieces. `reject request` is what lets a partial seed refuse a
             piece cleanly instead of timing out, which is exactly what the web
             seed bridge needs when a source turns out not to hold something it
             announced.
Approach:    The bridge is the natural place to start, because it is
             `bit-cli`'s own peer implementation: set the fast extension
             reserved bit, send `have all` when the scope covers everything and
             a bitfield otherwise, and answer an out-of-scope request with
             `reject request` rather than dropping the connection. The session
             side needs `librqbit`.
Acceptance:  The bridge negotiates BEP 6 with a session that supports it, sends
             `have all` for a complete source, and rejects an out-of-scope
             request without dropping the connection. Covered by an e2e test.

**The corpus supplies the algorithm, a conformance vector, and a warning.**

`vortex/bittorrent/src/peer_comm/peer_connection.rs:89` `generate_fast_set` is
the spec-conformant allowed-fast set: seed is
`(ip.to_bits() & 0xffffff00).to_be_bytes()`, a **/24 mask, which is what BEP 6
specifies**, concatenated with the 20-byte info hash, then `x = SHA1(x)`
repeatedly taking five big-endian `u32`s per round mod `num_pieces`,
de-duplicated, with a 300-round attempt cap. `:684-712` is the send side:
`ALLOWED_FAST_SET_SIZE = 6`, sent on the peer's first `Interested`, and a
torrent of six pieces or fewer gets the whole set rather than the algorithm.
`:758-790` is the receive side, and `:792` hard-errors on `HaveAll` or
`HaveNone` when `fast_ext` was never negotiated.

The receive side is where this goes wrong quietly.
`torrent/peerconn.go:1047-1054` carries the fix from anacrolix
[PR 1052](https://github.com/anacrolix/torrent/pull/1052): **the `AllowedFast`
case must `Add` to the peer's bitmap**, or every downstream check reads an
empty set and the feature is inert while appearing to work.
`torrent/peerconn.go:960-985` is the behaviour that makes it worth having: on
`Unchoke`, requests for allowed-fast pieces are *preserved* rather than
dropped.

**Ship this vector as a unit test.** From that same PR, reproducible against
both implementations named above:

```
ip        = 80.4.4.200
info_hash = AA AA ... AA  (20 bytes)
numPieces = 1313
k         = 7
=> [1059, 431, 808, 1217, 287, 376, 1188]
```

**Expect an aria2 peer to disagree, and do not treat that as a bug here.**
`aria2_rust/aria2-protocol/src/bittorrent/fast_set.rs:150` `mask_ip` mirrors
aria2's own C++ rather than the BEP: class A and B addresses are masked to /16
and class C to /24. So two widely deployed clients derive **different**
allowed-fast sets for the same peer. Implement the BEP as written, as vortex
and anacrolix do, and know the divergence exists before debugging it.

The receive half alone is worth having before the send half:
seedchamp [PR 7](https://github.com/j-c-m/seedchamp/pull/7) honours `Suggest`
in the picker without ever sending one, and `seedchamp/docs/design.md:152-160`
records that as a deliberate posture rather than an unfinished one.

**Partial, 2026-08-22. The Approach names the wrong half as the reachable
one.**

It says "the bridge is the natural place to start, because it is `bit-cli`'s
own peer implementation" and "the session side needs `librqbit`". The bridge
half is the one that cannot be done, and for a reason the Approach does not
consider: **the bridge's only counterparty is the session in the same
process.** It dials this run's own listen port and nothing else, so whatever it
advertises is answered by `librqbit`, and `librqbit` 9.0.0 has no BEP 6 at all.

Measured rather than assumed. `librqbit-peer-protocol` 9.0.0 `lib.rs:40-49`
declares message ids 0 through 8 and 20, and nothing in between: there is no
`HaveAll`, `HaveNone`, `SuggestPiece`, `RejectRequest` or `AllowedFast` variant
to construct, so the bridge could not send one without hand-rolling the wire
format for a peer that would fail to parse it. `Handshake::new` at `lib.rs:480`
sets `1 << 20` for the extension protocol and no other reserved bit, so the
session never offers the fast extension and never accepts an offer of it.
Zero hits for any of the five names in either crate:

```bash
grep -rniE "haveall|havenone|suggestpiece|rejectrequest|allowedfast|fast_ext"   ~/.cargo/registry/src/*/librqbit-9.0.0/src   ~/.cargo/registry/src/*/librqbit-peer-protocol-9.0.0/src
```

So this splits into three parts and only one is blocked.

**Part one, the derivation. Done.** `crates/bit-cli-core/src/fast_set.rs`
implements the allowed-fast set and **reproduces the conformance vector above
exactly**: `80.4.4.200`, twenty `0xAA` bytes, 1313 pieces, k = 7 gives
`[1059, 431, 808, 1217, 287, 376, 1188]`, which is
`the_canonical_vector_reproduces`. This is the part that is hard to get right
and impossible to check later without a reference, so it is written down while
the reference is in hand.

**The aria2 divergence is implemented rather than described.** `Mask::Bep6`
keeps three octets, `Mask::Aria2` keeps two below 192.0.0.0, and
`aria2_derives_a_different_set_below_192` asserts they disagree for the vector's
own address. A warning in prose is something to remember; a `Mask` a
measurement can name is something that reports which of the two the other end
used. `Mask::is_ambiguous` is the third answer: the two rules agree at and
above 192.0.0.0, and **loopback is not an exception** because 127.x is class A
under aria2's rule and agrees too, so a measurement taken over loopback cannot
tell them apart and says `ambiguous` rather than claiming a pass.

**Part two, the receive and measure side. Done, in `bench swarm`.** Every
synthetic peer now sets the fast extension bit, reports whether the target set
it back, counts `have all`, `have none`, `suggest` and `reject request`,
collects the offered allowed-fast set, and says which derivation it matches.
`bench swarm` is the right home rather than the bridge: it is the one part of
this tree that talks to somebody else's client, and `aria2c` 1.37.0 is
installed on this machine, so the divergence has a live counterparty to be
measured against.

It reports the blocker from the wire rather than from the source, which is the
better evidence of the two. `bench/swarm-20260822T062909627Z.json`:

| case | peers handshaked | `fast_negotiated` | `received` |
| --- | --- | --- | --- |
| `leech_1` | 1 | **0** | 8,388,608 |
| `leech_4` | 4 | **0** | 33,554,432 |
| `leech_16` | 16 | **0** | 134,217,728 |

The synthetic peers offered the bit on every one of those connections and
`bit-cli seed` declined it every time, which is `librqbit` saying it has no BEP
6 rather than this entry reading that off its source. Leeching is unchanged by
the offer: the same bytes as the run before the change, and `verdict: pass`.

`check-swarm.ps1` records `fast_negotiated` for exactly this reason and does
not judge it. Zero is what `librqbit` gives, so a script that failed on
anything else would be asserting the blocker rather than measuring it, and the
number that matters is the day it stops being zero.

The leecher acts on what it now understands, which is the difference between
reading the messages and honouring them. `have all` and `have none` stand in
for a bitfield, so a peer that negotiated the extension against a target that
sends two bytes instead of one no longer sees an empty bitfield and requests
nothing. A `reject request` clears the request from the window, which is the
stall BEP 6 exists to prevent and which anacrolix's `peerconn.go:960-985`
records the other side of.

**A defect this found, and it was in this tree.** `bench swarm` handed every
frame to `librqbit_peer_protocol::Message::deserialize`, which knows none of
the five ids, so **a target that spoke BEP 6 was reported as
`ended: "protocol"`**, a broken peer. Nothing had noticed because the only
target ever pointed at was `librqbit`, which never sends one.
`every_bep6_message_is_recognised_rather_than_called_a_protocol_error` is the
regression test.

**Part three, the send side. Blocked, upstream, and this is what keeps the
entry open.** The Acceptance says "the bridge negotiates BEP 6 with a session
that supports it", and no such session exists here. What would unblock it is
`librqbit` gaining the five message variants and the reserved bit, at
`librqbit-peer-protocol` `lib.rs:40-49` and `lib.rs:480`. The same blocker as
[T-102](#t-102-bep-55-holepunch-is-not-implemented) and
[T-167](#t-167-bep-54-lt_donthave-is-not-implemented), and named the same way.

Not blocked and not done: measuring a live `aria2c` seeder with `bench swarm`
to see which mask it uses on the wire. Everything needed is here, and the one
thing standing in the way is that a measurement over loopback is `ambiguous` by
construction, so it needs a target reachable on a class C address or aria2's
own set derived by hand from the address it sees. That is a session's work, not
a blocker.

## Part three is built, 2026-08-23, and the entry is done

The blocker above was "librqbit gaining the five message variants and the
reserved bit". The trees are vendored, so it was done here.
`patches/UPSTREAM.md` carries it under "BEP 6, the fast extension, is not
implemented at all".

**The five ids and the bit.** 13 `suggest piece`, 14 `have all`, 15 `have
none`, 16 `reject request`, 17 `allowed fast`, each with a `Message` variant.
The reserved bit is the **last** reserved byte and `0x04`, which is a different
byte from BEP 10's `0x10` at byte 5, and `Handshake::new` sets both.
`reject request` shares its three `u32` body with `request` and `cancel` and is
a third variant rather than a flag on one of them, because confusing them turns
a refusal into a demand.

**What the session does with them.** `have all` fills the peer's bitfield up to
the piece count and no further, so the spare bits past the last piece stay zero
exactly as a wire bitfield's must. `have none` sets an empty one, which is not
the same fact as sending no bitfield at all. `reject request` releases the
**whole piece** rather than the one chunk: a peer that will not serve one chunk
of a piece is not about to serve the rest, and leaving the piece assigned to it
stalls it just as long. `suggest piece` and `allowed fast` are understood,
traced and not acted on. That is **not** the posture this entry cited
`seedchamp` for: seedchamp honours a `Suggest` in its picker and never sends
one, which is the receive half acted on. This is the receive half **parsed**,
which is a weaker thing and is worth being exact about. What it buys is that a
peer sending either is no longer a protocol error, and what it does not buy is
a picker that takes advice. A suggestion is advice about which piece to ask for
and the picker here has its own order; an allowed-fast piece is one the peer
would serve while choking, and nothing here chokes. Both would be worth acting
on and neither is claimed to be.

**One thing the send side forced.** BEP 6 makes the first message mandatory: a
peer that negotiated it expects a bitfield, a have-all or a have-none before
anything else, and sending nothing is a protocol violation rather than an
omission. `should_send_bitfield` returns false when this end has nothing, and
that used to mean silence. It means `have none` now.

**The Acceptance, all three clauses.**
`crates/bit-cli-core/tests/bridge_protocol.rs`, against a session written by
hand that shares no constant with the bridge, which is the same harness
[T-166](peers.md) built and for the same reason.

| clause | test |
| --- | --- |
| negotiates BEP 6 with a session that supports it | the handshake assertion in `Session::start_with`, on the byte the bit lives in |
| sends `have all` for a complete source | `a_complete_source_announces_have_all_only_when_bep_6_is_negotiated` |
| rejects an out-of-scope request without dropping the connection | `an_out_of_scope_request_is_rejected_rather_than_ending_the_connection` |

The first of those two tests asserts **both** directions on the same fixture,
because the interesting failure is announcing `have all` to a peer that does
not know the message: that is a dropped connection rather than a smaller
greeting, and a test that only checked the negotiated case would not see it.
The second asks for a piece one past the end of the torrent, so the refusal is
deterministic and nothing has to lose a file first.

**Measured from the wire, which is where part two said the number would come
from.** `bench/swarm-20260823T040125619Z.json`, the same script and the same
workload as the run part two recorded:

| case | `fast_negotiated` before | after | `have_all` | `received` |
| --- | --- | --- | --- | --- |
| `leech_1` | 0 | **1** | **1** | 8,388,608 |
| `leech_4` | 0 | **4** | **4** | 33,554,432 |
| `leech_16` | 0 | **16** | **16** | 134,217,728 |

Every synthetic peer offered the bit, `bit-cli seed` set it back on every one,
and answered every one with `have all` rather than a bitfield. The bytes
received are identical to the run before the change, so the extension changed
what is said and not what is transferred. `check-swarm.ps1` reports `have_all`
beside `fast_negotiated` now, because "agreed to it" and "used it" are two
facts and only the second says the send side works.

**A test that had been dead for a session, found on the way.**
`test_bitfield_larger_than_max_msg_len` in `peer_binary_protocol`, which is
[T-194](peers.md)'s own regression test, carried no `#[test]` attribute: the
one it needed had landed on the test above it, which then had two. It was
compiled and never run. It is attributed now and it passes. Nothing in the
workspace gates catches this, because `cargo clippy --workspace` does not
compile the vendored crates' test targets, so the duplicate-attribute warning
only appears when the vendored tests are run.

**What is still not done, and it is not this entry's.** Measuring a live
`aria2c` seeder to see which allowed-fast mask it uses. Nothing changed about
that: it needs a target on a class C address, because the two rules agree on
loopback. `Mask::is_ambiguous` still says so rather than claiming a pass, and
nothing here sends an `allowed fast` set, so the divergence costs nothing yet.

### T-101 uTP is available but untested

Source:      corpus, `librqbit-utp`
Category:    bep
Priority:    P3
Effort:      M
Status:      open

Problem:     `ListenerOptions::mode` defaults to `TcpOnly`. `bit-cli` does not
             expose a way to enable uTP and has never tried it.
Relevance:   uTP is what keeps a seeding box from saturating its own uplink at
             the expense of everything else on the connection. On a netdisk
             that matters.
Approach:    Add `--transport tcp|utp|both`, default `tcp`, and measure. Rule
             0.10 applies: if it does not move a number, it does not ship.
Acceptance:  A download over uTP completes and verifies, and a run with a
             concurrent latency probe shows lower induced latency than the
             same run over TCP. Both numbers here.

**The title says "available" and it is not, which is a stronger statement of
the same gap.** Checked on 2026-08-21: there is no uTP anywhere in `bit-cli`.
`ListenerOptions::mode` is never set, no `--transport` flag exists, and
`grep -rn utp crates/` finds nothing. `librqbit-utp` 0.7.0 is in `cargo tree`
because `librqbit` depends on it, which is a dependency and not a capability.
The `README.md` protocol table said "available, off by default" and this file
said "inherited, off by default"; both read as a switch a user could flip.
There is no switch. Both are corrected, and the work in this entry is
unchanged: it was always "add the flag and measure", never "test the flag that
exists".

**Three implementations to read, and one argument for not writing one.**

`TorrentNG/crates/rt-utp/` is the most complete and the only one with a status
document: `TorrentNG/docs/protocol/UTP.md` separates "the packet codec works"
from "the engine can carry peer-wire traffic over it", which is exactly the
distinction this entry needs to make about itself. `congestion.rs` is LEDBAT
with `TARGET_DELAY_US = 100_000`, `:50` `on_ack` taking the base delay as a
running minimum of `timestamp_diff` and `:77` `on_timeout` halving with an MTU
floor, with three unit tests in the file. `selective_ack.rs:11` fixes
`EXTENSION_KIND = 1` and its doc states the bit numbering precisely: bit 0 of
the first byte acknowledges `ack_nr + 2`. `packet.rs`, `state.rs` and
`transport.rs` carry the header codec, the initiator-versus-acceptor
connection-ID derivation, and a shared-UDP endpoint that demultiplexes by
(remote address, receive connection id) so one socket serves many streams.

`mtorrent/mtorrent-core/src/utp/retransmitter.rs:48-50` fixes
`MAX_PACKET_SIZE = 9 KiB` (the macOS default UDP limit), `MIN_PACKET_SIZE = 1472`
(Ethernet MTU) and `INITIAL_RTO = 1 s`; `:108` `process_ack` is the
Jacobson/Karels RTT update applied **only to packets sent once**, with a fast
retransmit on the second duplicate ack. Its tests use
`tokio::test(start_paused = true)`, which is the same discipline
[T-035](performance.md) needed to make a token bucket testable.

`superseedr/src/networking/utp.rs:31-67` is the densest constants block in the
corpus if a number is wanted rather than an algorithm.

**And the argument against.** anacrolix
[Issue 1013](https://github.com/anacrolix/torrent/issues/1013) is the
maintainer of the widest-deployed Go implementation saying the pure-Go uTP is
buggy and to bind libutp instead. fx-torrent
[Issue 66](https://github.com/yoep/fx-torrent/issues/66) is one instance of
what that costs: a packet-parsing failure in the extension chain. A
hand-rolled uTP is a real and recurring maintenance cost, and this entry is P3
partly for that reason. If it is built, `librqbit-utp` already being in the
tree is the cheapest route by a wide margin.

## The flag is built and measured, 2026-08-24, and it stays open on its second half

**`--transport tcp|utp|both`, default `tcp`.** It is on `LimitArgs`, so every
command that starts a session takes it. `bit_cli_core::engine::Transport` is
the core type and `TransportMode` mirrors it in `cli.rs`, which is the split
every other enum flag here uses.

**Nothing was hand-rolled.** `librqbit-utp` is already vendored and
`ListenerMode` already existed; the work was finding that **two** settings
decide the answer and that only one of them is obvious.

### Setting the listener alone is a flag that says nothing

`SessionOptions::listen.mode` chooses which listeners are bound.
`SessionOptions::connect.enable_tcp` chooses whether the **dialer** may use
TCP, it defaults to true whatever the listener says, and
`stream_connect.rs:251` tries TCP first and only reaches uTP a second later.

The first version of this flag set the listener alone, and it produced two
wrong answers in one run:

| seeder | leecher | with the listener alone | with the dialer too |
| --- | --- | --- | --- |
| `tcp` | `utp` | **completes**, over TCP | does not connect |
| `utp` | `utp` | times out | completes |

The first row is the one that matters: a run asking for uTP reached a TCP-only
peer and reported success. `a_utp_leecher_does_not_reach_a_tcp_seeder` is that
row as a test, and it is what makes every other case in the file mean
something.

### Measured, and the flag moves a number

`scripts/check-transport.ps1`, 32 MiB over loopback, one seeder and one
leecher per case, `--peer` and nothing else. Committed at
`bench/transport-20260824T033000Z.json`:

| case | seeder | leecher | encryption | finished | rate |
| --- | --- | --- | --- | --- | --- |
| `tcp` | tcp | tcp | prefer | yes | 152.38 MiB/s |
| `utp` | utp | utp | off | yes | **76.19 MiB/s** |
| `both` | both | both | prefer | yes | 152.38 MiB/s |
| `mixed` | tcp | utp | off | **no**, and that is the control | |
| `utp-mse` | utp | utp | require | **no**, and that is [T-233](peers.md) | |
| `tcp-mse` | tcp | tcp | require | yes | 160 MiB/s |

```bash
pwsh -NoProfile -File scripts/check-transport.ps1
```

Six cases in `crates/bit-cli-core/tests/transport_e2e.rs` cover the same
ground in-process, at 512 KiB, and run in the workspace suite.

```bash
cargo test -p bit-cli-core --test transport_e2e
```

### What the measurement found that the entry did not predict

**uTP does not carry a torrent under MSE.** Every other combination of the two
works: uTP in plaintext works, TCP under MSE works, TCP in plaintext works.
The handshake completes, the extended handshake, `HaveAll` and `Unchoke` all
arrive, the leecher sends `Interested` and its first requests, and the seeder
reads none of them. That is [T-233](peers.md), it is this repository's own
code rather than upstream's, and it is carried as its own open entry with the
trace, which is what [RULES.md](RULES.md) section 5 asks of a residual.

### A claim this entry made for an hour and that is not true

**"A dual-stack UDP socket does not carry one either" was written here and is
wrong.** It was formed early, from two command line runs over the default
`[::]` bind that did not complete, and it was not retracted when the real cause
turned out to be MSE. Both of those runs were at the default
`--encryption prefer`.

Measured again on purpose, with the default bind and encryption off:

```
seeder listen_addr = [::]:58143
finished True
```

So uTP carries a torrent over the dual-stack bind, `--transport utp` **is**
usable from the command line today with `--encryption off`, and the tests in
`transport_e2e.rs` bind `127.0.0.1` for the two reasons `hostile_paths.rs`
does rather than for this one. The correction is written here rather than by
editing the claim away, which is [RULES.md](RULES.md) section 5.

### Why this stays open

The Acceptance is two clauses joined by "and". The first is met: a download
over uTP completes and verifies. The second asks for **lower induced latency
than the same run over TCP**, and nothing on this machine can show it.

That is now the **only** thing keeping it open, since the bind claim above
turned out to be nothing. LEDBAT targets a fixed one-way queueing delay and
yields when it rises. Loopback has no bottleneck link, so there is no queue to
build and no latency to induce: the 76.19 MiB/s above is a statement about this machine's loopback
and about neither congestion controller. Measuring the thing uTP is for needs a
**shaped path** with a bounded queue between the two endpoints, and a rate cap
on the sender is not one, because a sender that limits itself never fills
anybody's queue.

So what is left is one of: a shaped loopback path, or a second machine. Both
are larger than the flag was.

### T-102 BEP 55 holepunch is not implemented

Source:      https://github.com/ikatson/rqbit/issues/463 (open)
Category:    bep
Priority:    P3
Effort:      L
Status:      open

Problem:     No holepunch support, so peers behind a filtering NAT are
             unreachable.
Relevance:   It raises the reachable swarm size, which matters for a leecher
             and less for a well-connected seed. The operator's case is the
             seed, so this is low priority here.
Approach:    Needs peer protocol work in `librqbit`.
Acceptance:  Deferred. Revisit if peer reachability shows up as a measured
             limit in `bench swarm`.

**Priced on 2026-08-21, and the answer is that no NAT library helps.** The
question that prompted this was whether `iroh` should be adopted for hole
punching. It should not, and BEP 55 does not want one.

BEP 55 is three bencode messages over connections that already exist. The
extension is `ut_holepunch`; the message carries `msg_type`, `addr_type`,
`addr`, `port`, and an optional `err_code`; the types are `rendezvous`,
`connect`, and `error`; the error codes are `NoSuchPeer`, `NotConnected`,
`NoSupport`, and `NoSelf`. A dial that fails through every route asks an
**already-connected peer** to relay a `rendezvous` naming the unreachable
target; that peer checks both sides advertise the extension and sends
`connect` to each carrying the other's address; both then dial, and the two
outbound SYNs crossing in flight open both NATs. That is the whole protocol,
and it is written out here rather than cited, because the working
implementation it was read against is not a tree this repository keeps.

**The swarm is the rendezvous.** That is the design, and it is why no relay
server, no STUN, and no overlay is needed. `iroh` is a QUIC overlay with its
own node identities and its own relays, and every peer on both ends must speak
it: adopting it would make `bit-cli` reachable to other `bit-cli` instances
rather than to the swarm, which is a private network wearing a BitTorrent
costume. The same objection retires the rendezvous-server model generally.

**What blocks it is the boundary this repository already knows.** The wire
format is expressible today: `librqbit-peer-protocol` 9.0.0 carries
`ExtendedMessage::Dyn(u8, BencodeValue)`, an escape hatch for an arbitrary
extended message. What is missing is a way in: `PeerConnectionHandler`'s
`on_extended_handshake` and `update_my_extended_handshake` are what would
advertise `ut_holepunch` and route its messages, and that trait is implemented
inside `librqbit` by the torrent state rather than by anything a dependent
crate supplies. It is the same wall [T-002](webseed.md) measured and
[T-135](multi-source.md) records the decision for.

So this stays P3 and open, blocked on that boundary and not on a missing
library. Nobody should reach for a NAT crate for it again.

**The 2026-08-21 corpus supplies both an implementation and the design
argument, and the design argument is the more valuable half.**

`fx-torrent/src/peer/extension/holepunch.rs` is 678 lines of working
implementation rather than a codec: `:14` `HolepunchMessage { msg_type,
addr_type, addr, port, err_code }`, `:149` `NAME = "ut_holepunch"`, message
types `Rendezvous`, `Connect`, `Error`. It landed in
[PR 64](https://github.com/yoep/fx-torrent/pull/64). The wire format alone, in
97 lines, is `torrent/peer_protocol/ut-holepunch/ut-holepunch.go`.

`torrent/NOTES.md:15-31` is the part worth adding to this entry, because it
answers a question the protocol write-up above does not.
**Rendezvous only through relays for the same torrent.** The argument: if you
send a `rendezvous` and later receive a `connect`, you cannot tell whether that
connect answers *your* rendezvous or one some other peer sent to your relay.
Relays are not required to respond, so you cannot enforce a timeout and time
the two apart. Therefore **you do not know which info hash to put in the
handshake**. Handshaking passively always fails, because the other side may do
the same and neither initiates. Constraining rendezvous to relays for the same
torrent removes the ambiguity, and then every `connect` can be handled
actively. That is a constraint on the design, not an optimisation, and getting
it wrong produces connections that open and then hang.

The same file carries the arithmetic for whether to bother: with 30 per cent
of peers unrelayable and 50 per cent behind a bad NAT, relaying takes pairwise
connectability from 75 per cent to 92.5 per cent. That is the number this
entry's "raises the reachable swarm size" should be measured against if it is
ever built.

**The seam is gone as of 2026-08-23, and this entry is still open. The reason
it is open has changed.** What blocked it was
`PeerConnectionHandler::on_extended_handshake` and
`update_my_extended_handshake` being implemented inside `librqbit` by the
torrent state. The trees are vendored, so that is now a place this repository
can write, and [T-167](#t-167-bep-54-lt_donthave-is-not-implemented) and
[T-100](#t-100-bep-6-fast-extension-is-not-implemented) both went through it.
Nothing about BEP 55 is unreachable any more.

What stands in the way instead is this entry's own Acceptance, which says
"Deferred. Revisit if peer reachability shows up as a measured limit in `bench
swarm`", and nothing has measured one. Two things would have to exist before
building it means anything here:

1. **A measurement that says reachability is a limit.** `bench swarm` dials
   loopback, where every peer is reachable by construction, so the number this
   entry exists to move cannot be taken with the fixtures this repository has.
2. **A fixture that produces an unreachable peer**, or the acceptance is
   unfalsifiable: an implementation that never opens a hole and one that always
   does look identical against peers that never needed one.

So it stays open at P3 with the condition named rather than the blocker, which
is a different and weaker reason to leave something undone. Do not reach for a
NAT crate for it: the paragraphs above still hold and the swarm is still the
rendezvous.

### T-103 Filenames that are not valid UTF-8 are refused

Source:      https://github.com/ikatson/rqbit/issues/452 (closed, 2025-07-09)
Category:    bep
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T17:10Z

Problem:     `add_torrent` failed with "cannot decode filename bit as UTF-8" on
             a torrent with a non-UTF-8 path.
Relevance:   BEP 3 does not require UTF-8. Real torrents carry Shift-JIS and
             CP1251 names, and the `encoding` key exists to say so.
             `librqbit` 9.0.0 carries an encoding detector
             (`torrent_metainfo.rs::detect_encoding`), so the closed label is
             probably right, but `bit-cli`'s own `Metainfo` parser decodes
             paths as UTF-8 and has not been tested against anything else.
Approach:    Add a fixture with a Shift-JIS path and check that `bit-cli info`,
             `files`, and `webseed list` all handle it, including the percent
             encoding of the composed URL.
Acceptance:  A non-UTF-8 fixture parses, lists, and composes a correct web seed
             URL, and the reported path says which encoding was used.

**The practical shape of this is not Shift-JIS, it is the `.utf-8` key
variants, and that half is cheaper and more common.** intermodal
[Issue 534](https://github.com/casey/intermodal/issues/534) (CLOSED): uTorrent
writes **both** `name` and `name.utf-8`, and both `path` and `path.utf-8`, with
different encodings in each. Neither variant is in BEP 3 and both are universal
in practice. The reporter's conclusion, which is what anacrolix's
`Info.BestName()` does and what parse-torrent does, is the rule to adopt:
**if the `.utf-8` variant exists, prefer it**.
`parse-torrent/index.js:123-131` treats `info.name` **or** `info['name.utf-8']`
as satisfying the required-field check, and `path` **or** `path.utf-8`
per file; `:140` and `:181` then prefer the `.utf-8` spelling throughout.
parse-torrent [Issue 177](https://github.com/webtorrent/parse-torrent/issues/177)
adds that `comment.utf-8` exists too. `bit-cli`'s `Metainfo` parser reads
`name` and `path` only, so a uTorrent torrent carrying a mojibake `name` beside
a correct `name.utf-8` gets the mojibake.

**The creation side has its own version of this and it is a worse bug, because
it ships.** mkbrr [Issue 182](https://github.com/autobrr/mkbrr/issues/182) is
in [T-175](create-seed.md): a torrent created on macOS against an SMB mount
wrote NFD filenames, verified clean locally including with the tool's own
check, and broke for everyone else. create-torrent
[Issue 195](https://github.com/webtorrent/create-torrent/issues/195) is the
blunter form: `mkdir $'ä'` then create, and the tool cannot stat its own
input.

So this entry splits into two pieces of work that share one fixture set:
prefer the `.utf-8` variants on read (small, and the win is immediate), and
decide what a non-UTF-8 path becomes on the way to a filesystem and to a
percent-encoded URL (larger, and it interacts with the path planner
[T-071](windows.md) already built).

**Done 2026-08-23T17:10Z, and the entry's own title is what the measurement
disproved.** Nothing is refused. Every fixture below parses, exits 0, and
reports a name. The defect is different and worse than a refusal: **this tree
had two decoders and the reports used the wrong one.**

`crates/bit-cli-core/src/torrent/bencode.rs`'s `Value::as_text` decoded with
`String::from_utf8_lossy`, and `info`, `files`, `magnet` and `webseed list`
all read through it. The session that actually downloads reads through the
vendored `librqbit`, whose `detect_encoding` runs `chardetng`. So the two
disagreed on every torrent whose names are not UTF-8, and both were reachable
from one run.

**Measured before anything was written**, on a torrent with `name` `フォルダ`
and `path` `ファイル.bin` in cp932, served from `loopback-fileserver`:

| what said it | said |
| --- | --- |
| `bit-cli info`, `name` | `�t�H���_` |
| `bit-cli files`, `path` | `�t�@�C��.bin` |
| `bit-cli webseed list`, the URL | `/%EF%BF%BD%EF%BF%BD.../%EF%BF%BD....bin` |
| `bit-cli download`, `name` | `フォルダ` |
| the URL that run requested | `/%E3%83%95%E3%82%A9%E3%83%AB%E3%83%80/...` |
| what landed on disk | `フォルダ/ファイル.bin` |

**`webseed list` is the sharp one.** `man/bit-cli.json` describes it as
"Resolve every binding and print the exact URL each file maps to", with no
network. For this torrent it printed a URL of `%EF%BF%BD` runs, which is a 404
on every mirror there is, and which is not the URL the same binary requested
thirty seconds later. That is this tool's reason for existing answering
incorrectly.

**Two files collapsed onto one path.** `あ.bin` and `い.bin` in cp932 are
distinct byte strings that `from_utf8_lossy` maps to the same `��.bin`, so
`files` listed one path twice and nothing downstream could tell the two apart.

### What was built

**One decoding rule, in one place, called from both sides.** The vendored
`detect_encoding` keeps its behaviour and loses its body to
`detect_encoding_of`, a free function over an iterator of byte slices;
`bit-cli`'s `parse_info` calls the same function over the same raw `name` and
`path` bytes. Agreement is by construction rather than by care.

**The `.utf-8` keys are preferred, on both sides.** uTorrent writes `name` in
the creator's local encoding and `name.utf-8` beside it, and the same per
file; intermodal [issue 534](https://github.com/casey/intermodal/issues/534)
is the report and preferring the twin is what anacrolix's `Info.BestName()`
and `parse-torrent` already do. `Metainfo` reads `name.utf-8`, `path.utf-8`
and `comment.utf-8`, and the vendored `TorrentMetaV1Info` and
`TorrentMetaV1File` gained `name_utf8` and `path_utf8` so the download names
files the same way. A twin that is not valid UTF-8 is ignored, because a
creator who wrote the key without meaning it has said nothing.

Doing only the outside half would have re-created the defect with the sides
swapped. `patches/UPSTREAM.md` carries the section.

**The detector is fed the raw keys only.** A correctly written `.utf-8` twin
would otherwise talk `chardetng` out of the encoding the raw keys are in.

**`info` and `files` say which encoding named the files**, which is the last
line of the Acceptance. `name_encoding` carries `detected`, a WHATWG label,
and `utf8_keys`, and it is **absent** on a torrent whose names are UTF-8 and
which had nothing to choose, because a line on every report about a decision
nobody made is noise. `docs/schema.md` documents both fields, and
`schema_gen.rs` drives the new fixture so they are covered rather than
described.

### Why the `.utf-8` rule is a rule and not a better detector

`chardetng` is right often enough that one example proves nothing, so fourteen
names across six encodings were tried and **the guess was wrong for six**:

```
cp932   音楽      -> ‰¹Šy      windows-1252
cp932   ＡＢＣ    -> 俙俛俠     GBK
big5    測試      -> 덜먼       EUC-KR
cp932   release / 字.bin   -> release / Žš.bin   windows-1252
cp1251  release / я.bin    -> release / ÿ.bin    windows-1252
big5    release / 檔.bin   -> release / 읠.bin   EUC-KR
```

The last three are the common real shape and the worst case: an ASCII release
name dominates the detector's input, and every non-ASCII filename under it
comes out wrong. The `.utf-8` key is written down rather than guessed, which
is why it wins.

`音楽` with `曲.bin` is the committed fixture,
`TorrentFixture::names_that_are_not_utf8`, for exactly that reason: detection
alone gives `‰¹Šy` and `‹È.bin`, and the twins give the right answer.

### What holds it

Eight cases in `crates/bit-cli-core/src/torrent/metainfo.rs` and four across
`info`, `files` and `webseed list`. The one that matters most is
`the_two_decoders_in_this_tree_agree`: it parses the same bytes with both
implementations and compares every file path and the torrent name over four
shapes. **Run against the defect**, with the multi-file half of the vendored
patch reverted, it fails and names both sides:

```
the utf-8 keys: file paths disagree
  left: [["曲.bin"]]
 right: [["‹È.bin"]]
```

```bash
cargo test -p bit-cli-core --lib torrent::metainfo
```

```bash
cargo test -p bit-cli --lib a_url_is_composed_from_the_decoded_path
```

Upstream's own tests were run because the change is in their tree: 149
passing, unchanged.

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

### What is not closed, and it is not this entry

The second half the entry splits itself into, "what a non-UTF-8 path becomes
on the way to a filesystem", is closed for the reporting side and was never
open for the writing side: the path planner already folds and disambiguates,
and the two cp932 names that used to collide now decode to distinct strings,
so they land as two files rather than one plus a rename. What remains is that
a wrong detection produces a wrong-but-reversible name on disk, and no rule
can do better without a key the torrent does not carry. That is the case the
`.utf-8` preference exists for.

### T-167 BEP 54 lt_donthave is not implemented

Source:      `reference/RESEARCH.md` section D, 2026-08-21
Category:    bep
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-23T03:26Z

Problem:     A peer's bitfield only ever grows. BEP 3 has `Have` and no
             inverse, so once a peer has claimed a piece there is no way for
             it to withdraw the claim, and no way for `bit-cli` to hear one
             withdrawn.
Relevance:   This is the cheapest correctness win in the whole corpus for
             anything that tracks availability, and `bit-cli` tracks
             availability in two places that matter. The web seed bridge
             advertises a bitfield of exactly the pieces a source's scope
             covers in full, and [T-005](webseed.md) is the request to
             re-scope a source mid-run, which today cannot be expressed on the
             wire at all: a source that loses a file has no way to say so, and
             the session keeps asking. `lt_donthave` is that message. It is
             also what a partial seed needs when a mirror drops a file
             underneath it, which is the mirror case `bit-cli` exists for.
Approach:    `fx-torrent/src/peer/extension/donthave.rs` is the whole
             protocol, and it is small: `:19` `NAME = "lt_donthave"`, and the
             payload is a 4-byte big-endian piece index that clears one bit in
             the peer's bitfield. It is a BEP 10 extended message, so it costs
             one entry in the `m` dictionary the bridge already sends at
             `webseed/bridge.rs:708` and one handler on the receive side.
             Send it from the bridge when a scope narrows; honour it on
             receive by clearing the bit.
Acceptance:  A source re-scoped mid-run sends `lt_donthave` for every piece it
             has given up, the session stops requesting those pieces from it
             without dropping the connection, and a test asserts both. Pairs
             with [T-005](webseed.md), which is the reason to want it.

**Blocked on `librqbit` 9.0.0, and the blocker is the receive side rather than
the send side.** Read before writing any of it, which is what
[RULES.md](RULES.md) asks for and what this entry's own approach did not do.

Sending `lt_donthave` is as small as this entry says. Honouring one is not
`bit-cli`'s to do, and nothing in the session does it.

`librqbit-9.0.0/src/torrent_state/live/mod.rs:1076` dispatches
`Message::Have(h) => self.on_have(h)`, and `on_have` at `:1523` sets one bit in
`live.bitfield`. There is no inverse. Every extension message the session does
not know falls to the catch-all at `:1112`:

```rust
message => {
    warn!("received unsupported message {:?}, ignoring", message)
}
```

An `lt_donthave` arrives there as `ExtendedMessage::Dyn(id, ..)`, because
`PeerExtendedMessageIds` (`librqbit-peer-protocol-9.0.0/src/extended/mod.rs`)
carries `ut_metadata` and `ut_pex` and nothing else. So the bridge sending one
would produce a log line per retracted piece and change nothing about what the
session requests. That is worse than not sending it: a message the far end
warns about and ignores is noise that looks like a feature.

**There is no seam to do it locally either, and the near miss is worth
recording so nobody re-derives it.**
`librqbit-9.0.0/src/torrent_state/live/peers/mod.rs:114` is
`pub fn update_bitfield(&self, handle: PeerHandle, bitfield: BF)`, which is
exactly the operation needed and is declared `pub`. It is unreachable:
`lib.rs:75` declares `mod torrent_state;` with no `pub`, so the whole module
tree under it is private to the crate and `pub` inside a private module reaches
nothing. `bit-cli` holds a `ManagedTorrent` and has no path to its live peer
state.

**What would unblock it**, in the order of how much has to change upstream:

1. `librqbit` adds `lt_donthave` to `PeerExtendedMessageIds` and an
   `on_donthave` beside `on_have` that clears the bit. That is the correct fix
   and it is small: `on_have` is twenty lines and the inverse is the same
   twenty with `false` instead of `true`.
2. Failing that, `librqbit` makes `torrent_state` public, or exposes
   `update_bitfield` through `ManagedTorrent`. Then `bit-cli` could parse the
   message in the bridge and clear the bit locally, which is not the protocol
   but is the same outcome for an in-process pair.

`fx-torrent/src/peer/extension/donthave.rs:19` is still the whole protocol and
still the reference to build from: `NAME = "lt_donthave"`, a 4-byte big-endian
piece index, and `set_remote_has_piece(piece, false)`. What that tree has and
this one does not is a peer layer of its own. `bit-cli`'s peer layer is
`librqbit`'s, by decision 7.3.

**One half of this entry is not blocked, and it is deliberately not built
yet.** Any extension message the bridge **sends** needs the peer's numbering,
read out of the peer's own extended handshake, which is the second of the two
BEP 10 tables [T-166](peers.md) names. The first table, `OUR_EXTENSIONS` in
`crates/bit-cli-core/src/webseed/bridge.rs`, exists and is the receive
direction. The second does not, because nothing sends an extension message and
a table with no caller is infrastructure written against a guess. T-166 records
the seam; this entry is the first thing that will need it.

**[T-005](webseed.md) does not wait on this.** That entry's own approach,
narrow the scope and reconnect with the smaller bitfield, needs no extension at
all. What `lt_donthave` would have bought is one message instead of one
reconnect, which is an optimisation of a path that has to exist either way.
T-005 was built on the reconnect, and this entry becomes an optimisation of it
rather than a prerequisite for it. The work order that put this first was
written before the dispatch above had been read.

## The receive side is built, 2026-08-22, and this is partial rather than blocked

The blocker above is gone, and it was option 1 of the three listed: "`librqbit`
adds `lt_donthave` to `PeerExtendedMessageIds` and an `on_donthave` beside
`on_have` that clears the bit". The trees are vendored, so it was done here.

- `MY_EXTENDED_LT_DONTHAVE = 4`, and `PeerExtendedMessageIds` carries
  `lt_donthave`. That struct is the `m` dictionary of the extended handshake,
  so the field advertises the extension.
- `ExtendedMessage::LtDontHave(u32)` with its own serialize and deserialize.
  **It cannot go through the generic `Dyn` arm**: every other extension message
  in that crate has a bencoded body and this one's payload is four big-endian
  bytes and nothing else, which is the detail
  `fx-torrent/src/peer/extension/donthave.rs:19` names and the one a reader
  would get wrong.
- `PeerHandler::on_donthave` clears the bit, the inverse of `on_have` down to
  the shape. One difference, deliberate: a bitfield that was never allocated is
  left alone rather than allocated and cleared, because a peer that has claimed
  nothing cannot retract anything.

**What is proved.** `test_lt_donthave_round_trips` asserts the ten byte wire
form, the extension id, the big-endian payload and the round trip;
`test_lt_donthave_needs_the_peer_to_have_asked_for_it` asserts a peer that never
advertised the extension cannot be sent one. 142 upstream tests pass.

**What is not, and why this stays open.** Nothing here sends one, so nothing
has driven the handler end to end. That is the send half and it is now the only
thing left:

1. **The bridge has to read the session's `m`.** It drops every extension frame
   at `crates/bit-cli-core/src/webseed/bridge.rs`, against `OUR_EXTENSIONS`,
   which is right for an incoming message and is why the second BEP 10 table
   [T-166](peers.md) names does not exist yet. The extended **handshake** is id
   0 in both directions by BEP 10, so it is the one frame that can be decoded
   without agreeing a numbering first, and `m.lt_donthave` is what to keep.
2. **The `FileGone` path is where to send it.** `run` narrows `params.pieces`
   and reconnects with a smaller bitfield today, and the comment there already
   names this entry. With `lt_donthave` the bridge sends one message per
   dropped piece and stays connected. That needs the narrowing to move from
   `run` into `serve`, which holds the socket, and it needs `serve` to report
   the narrowing back so a later reconnect advertises the smaller bitfield.
3. **The acceptance is the entry's own**, and `FileGone` is already exercised,
   so it is a matter of asserting the connection survives and the session stops
   asking rather than building a new fixture.

Sending it when the session has not advertised it must stay a no-op: this
repository's own session advertises it now, and a real peer may not.

## The send half is built, 2026-08-23, and this is done

All three steps above, in that order, and one thing the entry did not predict.

**1. The bridge reads the session's `m`.** BEP 10 numbers extension messages
per receiver: the id in a message is the one the **receiver** advertised. So
sending one costs a second table, which is what [T-166](peers.md) records and
what did not exist because nothing sent an extension message. It exists now and
it holds exactly one entry, read out of the session's extended handshake.

That handshake is the one frame the bridge decodes rather than drops, and BEP
10 is what makes it safe: id 0 is the extended handshake in both directions, so
it is the only frame that can be read before a numbering is agreed. Everything
after it still goes through `OUR_EXTENSIONS`, which is still empty, because the
bridge still accepts no extension message. A session that does not advertise
`lt_donthave` leaves the id `None` and nothing is ever sent, which is the
no-op the paragraph above asks for.

**2. The `FileGone` path sends one message per dropped piece.** The narrowing
moved from `run` into `serve`, which holds the socket, and `serve` takes its
params by mutable reference so the caller's piece list shrinks with it. `run`
still has the reconnect, and it is what happens when the session does not speak
BEP 54, when nothing is left to serve, or when no announced piece touches the
lost file.

**3. The acceptance ran.**
`a_mirror_that_loses_a_file_retracts_its_pieces_without_reconnecting` in
`crates/bit-cli-core/tests/webseed_e2e.rs`, against the same partial mirror
fixture `a_mirror_that_404s_one_file_keeps_serving_the_other` uses.

| | |
| --- | --- |
| pieces retracted | **4**, every piece `b.bin` covers |
| pieces dropped | **4**, so the wire carried all of it |
| reconnects charged to `file_gone` | **0** |
| loopback ports used | **1** |

The last row is the assertion that says the connection survived, and it is the
history rather than a snapshot: a bridge takes a new port every time it dials,
so the length of that list is the number of connections it has made. Reading a
current port would have raced its own retraction, and the first version of the
test did exactly that and failed.

**What the entry did not predict, and it cost two red tests.** Every block in
flight against a lost file fails the same way, so the second failure and the
tenth are the same news as the first. Narrowing on each of them reported the
file once per failure and then retired the source for being unable to narrow,
because by the second one there was nothing left to drop. The connection
remembers which files it has already retracted, and a repeat is dropped.

The same shape one layer up: a request the session had already sent for a piece
this source has just retracted arrives after the retraction. Refusing it ends
the connection, which is the thing `lt_donthave` exists to avoid, so a request
for a retracted piece is dropped rather than refused.

**Clearing the bit was not the whole of honouring one either.** The receive
side built on 2026-08-22 cleared the peer's bitfield bit, which stops that peer
being **picked** for the piece again and does nothing about the piece already
assigned to it: it stayed in flight against a peer that had just said it cannot
serve it. `PieceTracker::release_piece_owned_by` gives it back to the queue,
and `on_donthave` calls it outside the peer lock. `on_peer_died` already
released every piece a peer owns, so this is one piece of an operation that
existed. `patches/UPSTREAM.md` carries it.

**What is proved and what is not.** The message goes out for every piece given
up, the connection survives it, and the piece goes back on the queue for
another peer, all asserted. What no test here asserts is a real peer's
behaviour on receiving one: both ends are this repository's. That is what the
BEP is for and there is nothing further to build for it.

### T-168 WebTorrent peers and WSS trackers are not supported

Source:      `reference/RESEARCH.md` section D, 2026-08-21
Category:    bep
Priority:    P3
Effort:      XL
Status:      open

Problem:     `bit-cli` speaks TCP to peers and HTTP or UDP to trackers. A
             `wss://` tracker URL in a torrent is not announced to, and a
             WebTorrent peer cannot be reached at all.
Relevance:   WebTorrent is a separate swarm sharing the same info hash. A
             torrent whose `announce-list` carries `wss://` tiers, which is
             the default for anything created by `create-torrent`, see
             `create-torrent/index.js:16-24`, where three `wss://` trackers sit
             beside the `udp://` ones each in its own BEP 12 tier, has peers
             `bit-cli` cannot see and does not report. `bit-cli trackers`
             announcing to every tracker in a torrent except the `wss://` ones
             is the visible half of that.

             Weighed honestly this is completeness rather than reach for
             `bit-cli`'s stated case. The operator's case is a seedbox and a
             netdisk, and a browser peer is neither. It is P3 for that reason
             and not because the work is large.
Approach:    Three sources, one per layer, and they are unusually complete for
             a protocol with no BEP.

             `torrust-actix/RtcTorrent.md` is 937 lines and self-contained:
             tracker announce extensions and their query parameters, the
             four-step signalling flow, the WebRTC data-channel message types
             (`MSG_PIECE_REQUEST 0x01`, `MSG_PIECE_DATA 0x02`,
             `MSG_PIECE_CHUNK 0x04`), chunked transfer, flow control, and a
             client implementation guide covering the ICE and SDP lifecycle,
             the announce loop, in-flight request management, piece
             verification and peer blacklisting. Its section 15 states the
             interop posture that makes it safe to add: RTC is purely
             additive, non-RTC clients see one extra `"rtc interval"` key they
             ignore, and mixed swarms work. Its section 14, five real defects
             with symptom, cause and fix, is worth reading before writing any
             of it.

             `torrent/webtorrent/` is the client side.
             `tracker-protocol.go` has the JSON announce shape with
             `offers[]`, `answer` and `to_peer_id`, plus `binaryToJsonString`,
             one rune per byte, which is the de-facto encoding for binary
             fields in WebTorrent JSON.
             `torrent/webtorrent/transport.go:261-303` wraps a detached data
             channel as an `io.ReadWriteCloser` and caps writes.

             `aquatic/crates/ws_protocol/` is the tracker side, and its
             comments record what the reference client actually does rather
             than what any document says:
             `aquatic/crates/ws_protocol/src/incoming/announce.rs:13` notes
             that `left` may be absent when a magnet is opened, that the
             length of `offers` **is** the peer count wanted, that the
             reference client caps it at 10, and that offers are not sent for
             `stopped` or `completed`.

             superseedr [Issue 319](https://github.com/Jagalite/superseedr/issues/319)
             scopes the whole job from a client author's side: WebRTC data
             channels, `ws://` and `wss://` announces, coexistence with TCP
             peers in one swarm, and a browser-peer test harness.
Acceptance:  Cannot be met incrementally, so it splits. First half:
             `bit-cli trackers` announces over `wss://` and reports the peers
             it is told about, which is useful on its own and needs no WebRTC.
             Second half: a WebTorrent browser peer and `bit-cli` exchange a
             verified piece. Record the first half's output here when it lands
             and leave the second open.
