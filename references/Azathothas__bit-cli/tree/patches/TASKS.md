# The work the fork exists to do

Ordered. Every item names the `TODO/` entry it unblocks and the seam it has to
reach, because a vendored change with no entry behind it cannot be reviewed
against anything.

This is not `TODO/INDEX.md` and does not replace it. An item here is a
**vendored change**; the entry it unblocks stays where it is and closes there,
with its own acceptance run. `TODO/` remains the authoritative record.

Written 2026-08-22, the session that vendored the trees and changed nothing in
them. Rewritten 2026-08-22T16:41Z by the session that worked sections 4 and 5,
after that session found the table below still describing a state two sessions
old, and kept current through that session as each item closed. `scripts/check-todo.ps1` compares every row here against the entry it
names now, and `scripts/gates.ps1` runs it, so this file cannot say `open`
about an entry that says `done` and reach a commit.

## What owning the fork is worth, counted

**17 entries: 16 done, 0 partial, 0 blocked, 1 open.** Every one of them was
held up by a seam somebody else's crate does not expose. **No open P0 is left
in the record, and nothing in the record is blocked.**

**Five of the eight vendored trees are not `librqbit`'s**, and that is new as
of 2026-08-29. [T-244](../TODO/cli-surface.md) needed a TLS `ClientHello` and
an HTTP/2 SETTINGS frame that a published `rustls` and a published `h2` will
not produce, so `rustls`, `h2`, `impit`, `reqwest` and `hyper-util` are
vendored beside them. The section below is what each one is for.

| entry | priority | status | what it is waiting for |
| --- | --- | --- | --- |
| [T-194](../TODO/peers.md) | **P0** | **done** | a bitfield that does not fit one message buffer |
| [T-020](../TODO/peers.md) | **P0** | **done** | a `tokio::select!` arm in upstream's accept loop |
| [T-040](../TODO/memory.md) | **P0** | **done** | a peer row nothing ever reclaimed |
| [T-022](../TODO/peers.md) | P1 | **done** | an HTTP tracker announce per address family |
| [T-132](../TODO/multi-source.md) | P1 | **done** | a download limit that skips one peer |
| [T-016](../TODO/disk-io.md) | P2 | **done** | a resume cache without session persistence |
| [T-100](../TODO/bep-coverage.md) | P2 | **done** | five message ids and a reserved bit |
| [T-163](../TODO/peers.md) | P2 | **done** | MSE, a wire-level handshake |
| [T-167](../TODO/bep-coverage.md) | P2 | **done** | **has** an inverse of `on_have` now |
| [T-195](../TODO/peers.md) | P2 | **done** | the read side of T-194, at 262,104 pieces |
| [T-210](../TODO/peers.md) | P1 | **done** | an incoming peer filed under our own peer id |
| [T-102](../TODO/bep-coverage.md) | P3 | open | `PeerConnectionHandler`, for BEP 55 |
| [T-025](../TODO/peers.md) | P3 | **done** | one `pub use`, and the filter had no name |
| [T-256](../TODO/trackers.md) | P1 | **done** | a private announce loop that recomputed its event |
| [T-244](../TODO/cli-surface.md) | P2 | **done** | a `ClientHello`, a SETTINGS frame and a pseudo-header order |
| [T-262](../TODO/cli-surface.md) | P3 | **done** | a PRIORITY block on the HEADERS frame a client opens with |
| [T-263](../TODO/cli-surface.md) | P3 | **done** | GREASE at both ends of the extension list, at chosen codepoints |

One is not done: [T-102](../TODO/bep-coverage.md), BEP 55, and it is no longer
waiting on a seam. The seam that blocked it is the one
[T-100](../TODO/bep-coverage.md) and [T-167](../TODO/bep-coverage.md) both went
through. What it waits on now is its own acceptance, which asks for a measured
reachability limit that loopback fixtures cannot produce. The entry says so.

**All three P0 items are closed.** [T-040](../TODO/memory.md) was the last, and
it closed on a measurement rather than a change: six hours of `soak.ps1` on the
`steady` workload, **+0.909 MiB/h while the peer records accumulate and flat
once the 1,024 row bound engages**, with the break at the instant the map
fills. `CLOSE_WAIT` was zero at all 687 samples, which is
[T-020](../TODO/peers.md)'s fix holding under load rather than for the length
of an acceptance script.

**Two entries were added to this table by the work rather than found before
it.** [T-210](../TODO/peers.md) came out of building
[T-132](../TODO/multi-source.md): a rate limit keyed on peer identity did not
limit, because every incoming peer was filed under this session's own peer id.
[T-025](../TODO/peers.md) is one `pub use` that had been sitting open as an
upstream API gap. Neither could have been fixed without the fork and neither
was on anyone's list.

**Before reconciling anything**, read `README.md` under "Upstream is not
automatically right". A new release is a proposal, not an authority, and a hunk
that touches something we have already changed needs three questions answered
before it is taken.

## 0. DONE. Is 9.0.1 broken for us, and it was

**[rqbit#637](https://github.com/ikatson/rqbit/issues/637)**, "[regression]
rqbit faill to add torrent larger than 2MB", was found by
`scripts/upstream-scan.ps1` half an hour after it was filed, which is the
clearest argument for running that scan there is. This asked whether `bit-cli`
was exposed. **It was**, and the answer is [T-194](../TODO/peers.md), P0, done.

- **The size of the `.torrent` is not the variable, the piece count is.** Every
  peer message was serialized into one fixed buffer, `MAX_MSG_LEN` = 16,500
  bytes, sized for a `ut_metadata` chunk. A bitfield is one bit per piece, so
  past **131,960 pieces** it did not fit and the connection was dropped before
  anything was served, in either role.
- **Measured to one piece**: 131,960 works, 131,961 does not, and both are
  2.64 MB torrents. That pair is what rules the file size out.
- **Whether this is #637 cannot be settled**, because the issue body is empty.
  The entry says so rather than claiming the scalp.
- The question this section asked last, whether `bit-cli create` could build
  such a fixture quickly enough to test with, is answered: **0.195 s** from
  160 MiB of payload.
- The residual this left, [T-195](../TODO/peers.md), the read side at 262,104
  pieces, is **done** too. `ReadBuf` grows now, bounded by what the connection
  says the torrent could need rather than by what the peer claims to be
  sending, and 1,048,576 pieces resolve. `scripts/check-bitfield.ps1` is the
  acceptance for both halves and its two default cases are the two counts a
  client here has actually died on.

## 1. DONE. T-020, and it was one match arm

`TODO/peers.md` T-020 had already done the work of finding this. The change was
exactly what the paragraph below said it would be, and the entry carries the
before and after.

Defect two is `task_listener` in `vendor/rqbit/crates/librqbit/src/session.rs`.
Its second `tokio::select!` arm is
`Some(Ok((live, checked))) = futs.next(), if !futs.is_empty()`. A pending
handshake check that resolves to `Err` **fails that pattern**, so `select!`
disables the arm for the rest of the call and waits on `l.accept()`, which on
an idle seeder is forever. Nothing in `futs` is polled until the next
connection arrives, so the queue drains at one entry per accepted connection
and sockets pile up in `CLOSE_WAIT`.

The entry measured it one for one: twenty poisoned connections, then single
peers one at a time, and **the twentieth got a handshake while the nineteen
before it got nothing**. `bench/listener-20260822T045550230Z.json`, case
`recovery`.

The arm binds the whole result and handles it inside now, so no outcome can
disable it. The acceptance the entry names **had never passed** and does:

```bash
pwsh -NoProfile -File scripts/check-close-wait.ps1 -Ceiling 100
```

| | before | after |
| --- | --- | --- |
| `no-handshake` CLOSE_WAIT, during and after a settle | 986 and 986 | **0 and 0** |
| handles | 188 to 1210 | **188 to 194** |
| connections to clear a 20 connection backlog | 20 | **1** |

**The worse half was never a socket count.** A backed up queue stopped the
seeder handshaking for **any** info hash, including one it was serving, while
it went on reporting itself as seeding.

**Closing it broke three acceptance cases, which is the right way round.**
Three of `scripts/check-listener.ps1`'s four asserted the defect and are
inverted rather than deleted; `scripts/check-swarm.ps1`'s `listener_poisoned`
carried `judged: false` and is judged. A fourth, `sources_ignored`, was resting
on the target being unable to answer its peers and had to be rebuilt: its
window went from 6 samples to 1 the moment the loop drained. **An acceptance
that needs the system under test to be slow is measuring the defect.**

**Not sent upstream, and it will not be**, [RULES.md](../TODO/RULES.md) section
6. It is [rqbit#311](https://github.com/ikatson/rqbit/issues/311), open, and the
change is one match arm, so a release may close it independently. That is what a
reconciliation checks for.

## 2. BOUNDED. T-040, and the nzbd patches were not needed

`TODO/memory.md` T-040 was attributed and bounded, not fixed: a peer row is
kept for every completed handshake and never reclaimed, and twenty-four
handshake-and-close connections leave twenty-four rows at `live 0` and `dead 0`
forever.

**There is a bound now.** `MAX_PEER_RECORDS`, 1,024 per torrent, reclaiming
`NotNeeded` and `Dead` rows before an insert and never a `Live`, `Connecting`
or `Queued` one. 2,000 connections leave **exactly 1,024** rows where they left
2,000, and one row per handshake below the bound is still asserted separately,
because a bound that reclaimed a live peer would also make the count flat.

Two things worth carrying forward:

- **RSS at that scale did not move, and that is the expected result.** Freeing
  a row returns it to the allocator, not to the operating system. What the
  bound changes is that demand stops growing.
- **A `Dead` row can be in the dial queue when it is reclaimed**, and that path
  answered a missing row with `Error::BugPeerNotFound`. A bound that logs "bug"
  for its own correct behaviour is worse than no bound, so it returns quietly
  now. Found by reading the callers before running anything.

**The nzbd series was read and not used.** `0010-bound-known-peer-records` is
the same idea against 8.1.1, and forward porting it would have cost more than
writing the bound against the tree in front of us, which is four functions in
two files. Nothing was copied, so nothing is owed in `THIRD_PARTY.md`. The
other three memory patches listed in section 4 are still unread and still worth
reading before the next bound is written.

**The measurement is in.** Six hours of `scripts/soak.ps1` on `steady`:
**+0.909 MiB/h while the records accumulate, flat afterwards**, the break at
4.65 hours where the map fills, handles flat and `CLOSE_WAIT` zero at all 687
samples. The entry has the fits either side of the break and why the whole-run
slope of 0.815 MiB/h describes neither regime. Start a soak early in a session:
it outlasts most of one, and it survives a `gates.ps1` run now because
`gates.ps1` leaves a process under `.tmp/` alone.

**Not sent upstream, and it will not be**, [RULES.md](../TODO/RULES.md) section
6. It is [rqbit#525](https://github.com/ikatson/rqbit/issues/525), open, and
reported as exactly this, so a release may close it independently. That is what
a reconciliation checks for.

## 3. DONE. MSE, and this tree did not take upstream's shape

[rqbit#633](https://github.com/ikatson/rqbit/pull/633), "feat(mse): Message
Stream Encryption (MSE) support", is open upstream and was the reason this
section asked a question before it asked for code.
[T-163](../TODO/peers.md) is **done** as of 2026-08-23.

**The decision this section asked for, taken.** Our own shape, through a seam,
and the argument is where the tests run and what a reconciliation has to read.
Upstream's pull request puts `crates/librqbit/src/mse/` inside the library:
taking it means carrying somebody else's crypto through every future merge, and
`cargo test --workspace` does not run the vendored crates' tests, so none of it
would be in the gates. What went into the vendored tree instead is one trait,
`StreamTransform`, called once per connection in each direction before the
BitTorrent handshake crosses it. The implementation is
`crates/bit-cli-core/src/mse/`, this repository's own code, where the gates
reach it.

If #633 lands, this seam is a change across seven files to weigh against it
rather than a competing implementation, and `README.md`'s three questions can
be answered then with the code in hand.

**Nothing was copied.** `reference/FluxDown`'s `mse/` and
`reference/mtorrent/mtorrent-core/src/pe/` were both read, and the protocol is
what they agree on: the 768 bit MODP group from RFC 2409 with generator 2, a
160 bit private exponent, RC4 with the first 1,024 keystream bytes discarded,
and `keyA`/`keyB` derived from the shared secret and the info hash. The code
here is written against that description and checked against sources neither
project wrote: `pow(2, x, P)` from an arbitrary precision implementation for the
exchange, and RFC 6229 for the cipher. So `THIRD_PARTY.md` is owed nothing and
neither is `UPSTREAM.md` beyond the seam.

**What the seam has to do that a wrapper would not.** Two things, and both are
in `patches/UPSTREAM.md` under "a peer connection cannot be wrapped before the
handshake". The accepting end is handed **every info hash the session holds**,
because MSE keys its handshake on one and it is inside what has not been
decrypted yet. And the dialling end may answer `RetryPlaintext`, because a
transform that offered encryption to a peer which does not speak it has spent
that connection finding out.

**Measured.** `scripts/check-encryption.ps1`, seven phases, three seeders
differing only in `--encryption`, two of them controls that must fetch nothing.
`bench/encryption-20260823T030511908Z.json`. The first three phases are one
seeder process on one port, which is the "no second port and no mode flag" half
of the entry's acceptance. One 768 bit exponentiation costs **51.4
microseconds** and a handshake needs two.

## 4. The nzbd series: nine patches, and the licence permits using them

<https://github.com/pjunod/nzbd/tree/main/contrib/rqbit> is a maintained series
of nine patches against rqbit **v8.1.1**, by another project vendoring the same
dependency.

**Licence: MIT OR Apache-2.0**, stated in that repository's `README.md` under
`## License`. Note that the GitHub API reports `license: null` for it, because
there is no `LICENSE` file for the detector to classify, and reading that as
"no licence" is wrong. Using any of it means attribution in `UPSTREAM.md` and
in `THIRD_PARTY.md`.

They are against 8.1.1 and this tree is at 9.0.1, so every one needs forward
porting. Every file they touch still exists at 9.0.1 except one test module,
checked against the vendored tree.

| their patch | what it does | ours |
| --- | --- | --- |
| `0009-bound-pending-incoming-handshakes` | caps pre-routing handshake checks at 256 | **not our fix.** See below. |
| `0010-bound-known-peer-records` | 1,024 retained peer records per torrent, 4,096 per session | [T-040](../TODO/memory.md) |
| `0012-bound-peer-response-backlog` | 128 queued piece and metadata responses per peer | [T-040](../TODO/memory.md) |
| `0014-bound-discovery-pressure` | bounds DHT and magnet-metadata queues and retained candidates | [T-040](../TODO/memory.md), [`TODO/dht.md`](../TODO/dht.md) |
| `0016-limit-peer-metadata-before-allocation` | enforces a BEP 9 ceiling **before** allocating | [T-040](../TODO/memory.md), and it is a denial-of-service shape |
| `0001-allow-persistence-without-auto-restore` | keeps persistence available while disabling implicit admission | [T-016](../TODO/disk-io.md), **done another way**. See below |
| `0005-bound-tracker-requests` | 1 MiB decoded cap, 30 s completion, 60 s minimum announce | [`TODO/trackers.md`](../TODO/trackers.md) |
| `0007-bound-session-peers` | 80 live peers per torrent, 400 per session | `--peer-limit` already exists here; read before adopting |
| `0018-propagate-file-sizing-errors` | stops initialization on the first sizing failure | [`TODO/disk-io.md`](../TODO/disk-io.md) T-014, which is already done |

**`0009` is the one to be careful with, and it is instructive.** It caps the
pending handshake set, and T-020 measured that the cap is *not* the cause of
this repository's P0: `bit-cli` already sets
`max_pending_incoming_handshake_checks` to `usize::MAX` deliberately, because
that is what removed the panic that was defect one. The entry says it outright:
"a reader who fixed the cap would have fixed nothing." Two projects, the same
file, and different problems. Adopt the eight, read the ninth, and do not let
its title decide anything.

**`0001` was called the highest value of the nine here, and the entry it was
for closed without it.** The argument was that [T-016](../TODO/disk-io.md) was
the only entry in the record blocked on a decision rather than on a defect:
decision 7.4 puts session persistence in Phase C and `librqbit` 9.0.0 offered
no resume cache without it, so a seam separating persistence from auto-restore
would remove the conflict without touching 7.4.

That was the right diagnosis and the wrong remedy. Separating auto-restore from
persistence still leaves a session store on disk; what T-016 needed was no
session store at all. `SessionOptions` takes a `BitVFactory` now, so the cache
exists and nothing about the session is written down. **Read `0001` before
adopting it for anything else**: it solves a problem this repository no longer
has, and its value here was the reading rather than the patch.

### All nine have been read now, 2026-08-23, and one was taken

The four that were still unread are read, against the vendored tree rather than
against 8.1.1, and the verdicts differ.

**`0005-bound-tracker-requests`: taken, in part.** The defect is real and this
tree has all of it. Neither `reqwest` client the session builds carries a
timeout; `Response::bytes()` reads an announce body with no ceiling, so the
size of this process's allocation was a number the tracker picked; and
`interval: 0` gave an announce loop with no sleep in it. All three are bounded
now and `patches/UPSTREAM.md` carries it.

**The part not taken is the interval floor's value**, and it is the worked
example of `README.md`'s "upstream is not automatically right" applied to a
third party. `0005` floors at 60 seconds, and its own draft says outright that
this is a policy tradeoff rather than a safety check: a tracker legitimately
asking for 10 seconds is delayed to 60. The UDP path **in the same file**
already clamped to five, so five is the number this codebase had already chosen
for the same question. Matching it makes one protocol have one answer; raising
both to sixty is a decision about how often to talk to honest trackers, and it
was not this change's to make.

**`0016-limit-peer-metadata-before-allocation`: not taken, and it found
something.** The cap it adds already exists at 9.0.1: `HandlerLocked::new`
refuses a `metadata_size` over 32 MiB **before** allocating, at
`vendor/rqbit/crates/librqbit/src/peer_info_reader/mod.rs:87`. What `0016` adds
beyond that is `PeerConnectionOptions::max_metadata_size`, a knob with no
caller here, and this repository does not ship infrastructure written against a
guess.

Reading it against the tree found the thing that **is** unbounded, and it is
not the per-peer cap: `dht_utils.rs:42` runs 128 metadata reads at once, so the
product is 4 GiB and nothing bounds a product. That is
[T-212](../TODO/memory.md), filed rather than fixed, with the arithmetic and
both citations and an acceptance that says what would replace the arithmetic
with a measurement.

**`0012-bound-peer-response-backlog`: read, not taken, and the reason is that
its premise is half true here.** It gives each peer a 128 permit response
budget and advertises that as BEP 10 `reqq`. The web seed bridge in this
repository already advertises `reqq` 250 and refuses a request over
`MAX_REQUEST_LEN`, but that is the bridge and not the session: the session's
own writer channel is still unbounded and a peer can still queue piece
responses faster than a slow socket drains them. So the defect is real and the
patch is 14 KB across two files carrying a permit through the upload scheduler,
the writer queue, the disk read and the socket write. It is a change worth
making and it is not one to make at the end of a session on somebody else's
description of it. It belongs to [T-040](../TODO/memory.md)'s family and needs
its own entry and its own measurement first.

**`0014-bound-discovery-pressure`: read, not taken, and it is the largest of
the nine.** 33 KB against that repository's `dht.rs`, bounding the outgoing
datagram queue, the recursive node queues, the delivered peer queue and the
metadata resolver, and changing DHT traversal behaviour under saturation in
each. Two of its findings are worth carrying forward whatever happens to the
patch: the resolver retains every address it has seen, which
[T-212](../TODO/memory.md) records, and a recursive request was being sent
twice, once for the callback and once for traversal, doubling DHT traffic and
discarding half the returned data. That second one is a defect rather than a
bound and it should be checked against 9.0.1 before anything larger is
attempted. Nothing here has checked it.

**What the reading was worth, counted.** One patch taken in part, one entry
filed, one deliberate departure recorded, and two patches whose value was the
reading rather than the code. Section 2 said the same about `0010` and it was
right about these too.

## 5. The rest, in the order the entries already argue for

**[T-022](../TODO/peers.md) is done.** An HTTP tracker was told about one of
this host's two addresses while a UDP tracker in the same file was told about
both. `task_single_tracker_monitor_http` in
`vendor/rqbit/crates/tracker_comms/src/tracker_comms.rs` holds a `reqwest`
client per family now, pinned by overriding the resolution, and announces once
over each in sequence. Measured against `loopback-tracker` on both loopback
addresses at one port: **ipv6 alone before, both after**, and which family the
old code picked was the resolver's order rather than a choice.
`scripts/check-tracker-family.ps1` is the acceptance and its `literal_host`
case is the control that says it can fail.

**[T-132](../TODO/multi-source.md) is done.** The row above used to say it was
waiting on "peer identity on `TorrentStorage`", which was never what the entry
said: the entry named `prepare_for_download` taking the peer it is throttling.
`Limits` carries a second download limiter and a list of peer id prefixes it
skips, `bit-cli` grows `--max-peer-rate`, and its own bridge is registered as
exempt. An 8 MiB/s peer cap holds the swarm to **8.42 MiB/s** and lets an
attached HTTP source run at **151.84 MiB/s**.

That took a defect out with it. The exemption matched nothing at first because
`librqbit` filed every **incoming** peer under this session's own peer id,
handing `on_handshake` the handshake it had just built to send instead of the
one it read. That is [T-210](../TODO/peers.md), P1, done, and the bridge dials
in, so it was exactly the case that took the wrong path.

Then [T-100](../TODO/bep-coverage.md), [T-167](../TODO/bep-coverage.md) and
[T-102](../TODO/bep-coverage.md). Each entry names its seam with a line number
and none of them needs re-deriving.

## 6. DONE. T-244, and the five trees that are not librqbit's

`TODO/cli-surface.md` T-244 ships a client that presents itself as a current
Chrome when it fetches a source document, by the operator's ruling of
2026-08-29. Every part of that presentation is decided below `reqwest`, and
none of it is reachable from a published crate.

| tree | what it is for | patched |
| --- | --- | --- |
| `rustls`, apify's fork | the `ClientHello`: fifteen ciphers, fifteen extensions, GREASE, ECH, ALPS, certificate compression, ML-DSA signature algorithms | the workspace member list only |
| `h2`, upstream at `v0.4.19` | the pseudo-header order, as a request extension rather than an environment variable | four files |
| `impit` | the fingerprint database and the client that drives it | HTTP/3, a charset detector, a proxy error downcast, and the environment variable, all removed; extra roots added |
| `reqwest`, the 0.13 line | a request's extensions reaching `hyper`, and two HTTP/2 settings `hyper` takes and it does not offer | one file |
| `hyper-util` | one method upstream took after 0.1.20 shipped | **nothing** |

**`apify/h2` is not what is vendored, and that is a decision.** Its fork is
`0.4.7` against the `0.4.19` every requirement in this graph asks for, so
cargo declines the `[patch]` with a warning and the fork never runs. Vendoring
it and bumping the version would leave a recorded base that does not describe
the tree, which `README.md` says makes the next merge wrong in a way nothing
detects. The tree here is upstream `0.4.19` and the ordering is our own patch.

**`apify/tower-http` and `apify/hyper-util` are not vendored either**, and both
were measured rather than assumed. apify's `tower-http` comments out the two
lines that strip `Content-Encoding` and `Content-Length` after decompressing,
which would make every response this client reads claim an encoding it no
longer has. apify's `hyper-util` adds a status code to a proxy tunnel error
that nothing here reads. `impit` compiles against the published versions of
both once one downcast is removed.

**What it is worth, measured.** JA4 moved from
`t13i1010h2_61a7ad8aa9b6_3fcd1a44f3e3` to
`t13i1515h2_8daaf6152771_806a8c22fdea`, which is Chrome's published cipher and
extension hash, and the Akamai HTTP/2 fingerprint from nothing at all to
`1:65536;2:0;4:6291456;6:262144|15663105|0|m,a,s,p`, which is Chrome's.
`scripts/check-fingerprint.ps1` asserts both against a golden on every push.

## Returning to ordinary work

The fork is a means. When the P0 items are closed and the entries above have
moved, `PROGRESS.md`'s work order goes back to being derived from
`TODO/INDEX.md`'s four questions, and the vendored trees become maintenance:
run `scripts/upstream-scan.ps1` on a version bump, reconcile with
`scripts/vendor-sync.ps1`, and keep `UPSTREAM.md` true.

The signal that it is time: no entry in the table at the top of this file is
still waiting on a seam.
