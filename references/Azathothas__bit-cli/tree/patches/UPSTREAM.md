# What this repository changed in the vendored upstreams

One section per change. This file is the record Apache-2.0 asks for, which is
that changed files are marked as changed, and it is what a reviewer reads
instead of a 389 file diff. The patch series beside it is generated from the
tree by `scripts/vendor-diff.ps1`; this is the part a script cannot write.

How to add one: [`README.md`](README.md).

**Nothing here is ever sent upstream**, and no session opens an issue, a pull
request or a discussion anywhere. That is settled by
[`TODO/RULES.md`](../TODO/RULES.md) section 6, and section 6a is the wider rule:
this repository is the only one an agent may write to. So the `Upstream:` field
in each section below does **not** track a submission. It answers the one
question a reconciliation asks: **could a release retire this patch on its own?**
A defect upstream may fix independently is named with its issue number so the
next merge checks for it rather than carrying a duplicate; a patch shaped for
this repository says that no release retires it.

Verify that this file describes the tree:

```bash
pwsh -NoProfile -File scripts/vendor-diff.ps1 -Check
```

---

## librqbit-dualstack-sockets: the Windows `new_from_name` ignores its argument

```
Unblocks:    nothing, and that is why it is first
Files:       vendor/librqbit-dualstack-sockets/src/bind_device.rs
             patches/librqbit-dualstack-sockets/0001-src-bind_device.rs.patch
Upstream:    ours. A lint in their code, so a release may silence it their own
             way; check at the next one.
Added:       2026-08-22T12:24Z
```

`BindDevice::new_from_name` has two bodies. The `#[cfg(not(windows))]` one
resolves the interface name to an index. The `#[cfg(windows)]` one returns
`Error::BindDeviceNotSupported` without looking at its argument, and takes that
argument as `name`, so rustc's `unused_variables` fires on every Windows build.
The parameter is now `_name`, which is the whole change: the signature, the
behaviour and the public API are identical.

**Why it has to be here.** Because this repository ships the crate. Cargo passes
`--cap-lints allow` to a dependency it resolved from a registry and does **not**
pass it to a path dependency, so `[patch.crates-io]` made every warning in the
vendored trees ours. Under CI's `RUSTFLAGS: -D warnings` this one failed four
Windows jobs on the vendoring commit.

Dropping `-D warnings` was tried first and reverted on the operator's
instruction, and the reason is worth keeping: development happens on Windows,
so CI is the only place a warning on another platform is ever seen. A build that
does not fail on one cannot catch sloppy work. The cost of that decision is
exactly this file.

**How it was proved.** The build is clean with the flag on, where before it was
not:

```bash
RUSTFLAGS="-D warnings" cargo build --workspace --all-features
```

**What would retire it.** A release that names the parameter `_name`, or that
stops taking it at all. It is a one-word fix to a real lint in their code with
no behaviour attached, so it is cheap for them to make and cheap for us to
notice: the patch stops applying and this section goes.

---

## The template

Copy this for the next change.

```
## <upstream>: <what it is>

Unblocks:    T-NNN, and the line in TODO/<file>.md that names the seam
Files:       vendor/<upstream>/<path>, and the patch that carries it
Upstream:    ours | ours, and <issue> is the same defect | fixed upstream in
             <ref>, delete this
Added:       <ISO 8601 UTC>

What it does, in a paragraph a reviewer can check against the diff.

Why it cannot be done outside the vendored tree. This is the part that dates
fastest: a seam that was private may become public, and then the patch should
go rather than stay.

How it was measured, or which test holds it.
```

Three things that section must always answer, because each has already cost
somebody a session in this repository:

- **Which entry.** A vendored change with no `TODO/` entry behind it cannot be
  reviewed against anything, and it is the first thing a reconciliation has to
  weigh when upstream touches the same lines. The one above has none, and says
  so: it exists because of a build flag, not because of a defect in `bit-cli`.
- **Whether a release could retire it.** Nothing here is ever sent upstream,
  `TODO/RULES.md` section 6, so the only way a patch goes away is upstream
  fixing the same thing on its own. A section that names the defect and its
  issue number is what makes the next reconciliation check for that instead of
  carrying a duplicate.
- **Why it has to be here.** `TODO/RULES.md` section 5 has a rule about a doc
  describing a state the tree is not in, and a patch justified by a seam that
  has since opened is exactly that.

---

## librqbit: a bitfield larger than MAX_MSG_LEN cannot be sent

```
Unblocks:    T-194, TODO/peers.md, and it is a P0
Files:       vendor/rqbit/crates/peer_binary_protocol/src/lib.rs
             vendor/rqbit/crates/librqbit/src/peer_connection.rs
             vendor/rqbit/crates/librqbit/src/peer_info_reader/mod.rs
             vendor/rqbit/crates/librqbit/src/torrent_state/live/mod.rs
             patches/rqbit/0009-crates-librqbit-src-peer_connection.rs.patch
             patches/rqbit/0010-crates-librqbit-src-peer_info_reader-mod.rs.patch
             patches/rqbit/0018-crates-librqbit-src-torrent_state-live-mod.rs.patch
             patches/rqbit/0026-crates-peer_binary_protocol-src-lib.rs.patch
Upstream:    ours. A defect in their code, so a release may fix it
             independently; check at the next one.
Added:       2026-08-22T13:52Z
```

Every peer message is serialized into one fixed buffer per connection, sized
`MAX_MSG_LEN` = 16,500 bytes. That number is built for a `ut_metadata` data
message: a 16,384 byte chunk, its bencode header, and 64 bytes of slack. The
comment above it says the `ut_metadata` request is "the largest known message".

A bitfield is not bounded that way. It carries one bit per piece, so its length
is a property of the torrent: `5 + ceil(pieces / 8)` bytes. Past **131,960
pieces** it does not fit, `Message::serialize` returns `NoSpaceInBuffer`, and
`manage_peer` drops the connection with "not enough space in buffer". A seeder
then serves nobody and a magnet for such a torrent never resolves, in both
cases with nothing above DEBUG to say why.

The change is that the bitfield no longer goes through the shared buffer:

- `Message::bitfield_message_len` is the exact length `serialize` needs, so a
  caller can size a buffer without knowing the wire layout.
- `PeerConnectionHandler::serialize_bitfield_message_to_buf` takes a
  `&mut Vec<u8>` rather than a `&mut [u8]`. Only the implementor knows the
  piece count, so that is where the buffer is sized.
- The send site in `manage_peer` uses a buffer of its own rather than
  `write_buf`. It is allocated once per connection, written once, and dropped.
- The comment on `MAX_MSG_LEN` now says what it actually bounds.

`peer_info_reader`'s handler is the other implementor and returns `Ok(0)`
without touching the buffer, so it takes the signature and nothing else.

**Why it has to be here.** The buffer, the trait and both implementors are
private to `librqbit`, and the constant is in `peer_binary_protocol`. Nothing
about it is reachable from a dependent crate: there is no option, no builder
and no trait a caller can implement differently. `bit-cli` cannot seed or fetch
a torrent past the threshold by any configuration.

**How it was measured.** Torrents of 1 KiB pieces, seeded on loopback with
trackers and DHT off, fetched by magnet by a second process given nothing but
`--peer 127.0.0.1:<port>`. The pass is metadata resolving and the file
appearing, both of which need the bitfield to have crossed.

| pieces | `.torrent` | bitfield | before | after |
| --- | --- | --- | --- | --- |
| 131,960 | 2,639,339 B | 16,500 B | resolves | resolves |
| 131,961 | 2,639,359 B | 16,501 B | no space in buffer | resolves |
| 163,840 | 3,276,939 B | 20,485 B | no space in buffer | resolves |

One piece apart, and 16,500 is `MAX_MSG_LEN` exactly.

```bash
pwsh -NoProfile -File scripts/check-bitfield.ps1
```

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

139 upstream tests passed when this landed, one of them new:
`test_bitfield_larger_than_max_msg_len` in `peer_binary_protocol`, which asserts
that the fixed buffer refuses a 131,961 piece bitfield and a sized one round
trips it.

**What it did not fix, until later the same day.** The read side. `ReadBuf`
was a 32,768 byte ring buffer, so the same message failed on receipt past
**262,104 pieces** with "read buffer is full". That was
[`TODO/peers.md`](../TODO/peers.md) T-195, and it is closed: the section below,
"a message larger than the read buffer cannot be received", is the change.

**What would retire it.** A release that sizes the write buffer from the
bitfield rather than from `MAX_MSG_LEN`. It is a defect in their code with a one
line reproduction, so it is findable without us; the reproduction is in
[`TODO/peers.md`](../TODO/peers.md), T-194.

---

## rqbit: the workspace lists a member this repository does not vendor

```
Unblocks:    nothing. It makes a documented command run at all
Files:       vendor/rqbit/Cargo.toml, and the two lockfiles that follow it
             vendor/rqbit/Cargo.lock
             vendor/rqbit/package-lock.json
             patches/rqbit/0001-Cargo.lock.patch
             patches/rqbit/0002-Cargo.toml.patch
             patches/rqbit/0030-package-lock.json.patch
Upstream:    ours by construction. It is a consequence of our exclusion list
             rather than a defect, so no release retires it
Added:       2026-08-22T13:47Z
```

`vendor/rqbit/Cargo.toml` lists `desktop/src-tauri` in `[workspace] members`.
`desktop/` is deliberately not vendored: it is a Tauri application, 1.6 MB of
the 4.4 MB upstream ships, depending on nothing this tree builds.
[`vendor/upstream.json`](../vendor/upstream.json) carries the exclusion and
[`docs/vendoring.md`](../docs/vendoring.md) carries the reason.

Cargo loads every member's manifest before it does anything, `default-members`
included, so the missing directory made the whole workspace unloadable:

```
error: failed to load manifest for workspace member
`vendor\rqbit\desktop/src-tauri`
```

That is the manifest [`README.md`](README.md) and
[`docs/vendoring.md`](../docs/vendoring.md) both tell a session to run
upstream's tests against, and the kickoff prompt names it too. The command had
never worked. The member is removed rather than the directory vendored, because
vendoring a Tauri desktop application to satisfy a manifest is the wrong half of
the trade.

`default-members` already excluded it, so nothing that used to build stopped.

**The two lockfiles follow from it and are most of the diff.** Removing the
member removed its dependencies: `Cargo.lock` loses 220 packages, 739 down to
519, and `package-lock.json` loses the `desktop` workspace entry. Neither gains
anything. Every `name`/`version` pair left in `Cargo.lock` is one the base
already had, so nothing was added and nothing was bumped; the 55 added lines are
the diff re-flowing around removed blocks. They are committed rather than
reverted because they are stable: cargo and npm rewrite them to exactly this on
the next run, and a reverted lockfile would make `vendor-status` report the
series stale every time somebody ran upstream's tests.

**Why it has to be here.** It is the vendored manifest. There is nowhere else
to say it.

**How it was proved.** The command in `README.md` runs:

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

139 passed, 0 failed, 6 ignored, across thirteen crates.

---

## librqbit: one failed handshake check stops the accept loop draining

```
Unblocks:    T-020, TODO/peers.md, the record's only open P0
Files:       vendor/rqbit/crates/librqbit/src/session.rs
             patches/rqbit/0013-crates-librqbit-src-session.rs.patch
Upstream:    ours. It is rqbit#311 (https://github.com/ikatson/rqbit/issues/311),
             open, so a release may carry a fix of their own.
Added:       2026-08-22T14:37Z
```

`task_listener` is a `tokio::select!` over two arms: accept a connection, or
take a finished handshake check off `futs`. The second arm was

```rust
Some(Ok((live, checked))) = futs.next(), if !futs.is_empty() => { ... }
```

A `select!` arm whose **pattern fails** is disabled for the rest of that
`select!` call. A handshake check that resolves to `Err`, which is what a peer
that connects and closes without handshaking produces, fails `Some(Ok(..))`.
The arm goes away and the loop waits on `l.accept()` alone, which on an idle
seeder is forever. Nothing in `futs` is polled until the next connection
arrives, so the queue drained **one entry per accepted connection**.

A check that resolves to `Ok` matched, ended the iteration, and cost no accept,
which is why this was invisible under ordinary traffic and why a busy seeder
cleared itself.

The arm now matches `Some(result)` and handles the error inside, so the pattern
cannot fail. The error was already logged by the `map_err` on the future where
it is pushed, so the `Err` case does nothing but let the loop come round.

**What it cost, which is more than a socket count.** While the queue was
backed up the seeder accepted TCP and completed **no handshake for any info
hash, including one it was serving**, and went on reporting itself as seeding.
A supervisor watching the process, the port or the log saw nothing. T-020
measured it one for one: twenty connections that closed without handshaking,
then single peers one at a time, and the twentieth peer got a handshake while
the nineteen before it got nothing.

**Why it has to be here.** `task_listener` is a private method on `Session`
and the `select!` is inside it. There is no option, no callback and no trait
between a caller and that loop. `bit-cli` already sets
`max_pending_incoming_handshake_checks` to `usize::MAX`, which removed a panic
that was the same entry's first defect, and T-020 records that the cap has
nothing to do with this one: "a reader who fixed the cap would have fixed
nothing."

**How it was measured.**

```bash
pwsh -NoProfile -File scripts/check-listener.ps1
```

| case | before | after |
| --- | --- | --- |
| `recovery`, connections to clear a 20 connection backlog | 20 | **1** |
| the same load, probes / failed | 6 / 3 | **13 / 0** |
| the seeder | stopped, exit 17, `listener_unhealthy` | still serving |

Three of that script's four cases asserted the defect, so they are inverted
rather than deleted and now hold the fix. `check-swarm.ps1`'s
`listener_poisoned` case carried `judged: false` for as long as T-020 was open,
and is judged now.

**What would retire it.** It is
[rqbit#311](https://github.com/ikatson/rqbit/issues/311), open since before this
repository existed, and the change is one match arm. A release that closes that
issue is the one to check this patch against.

---

## librqbit: nothing ever reclaimed a peer row

```
Unblocks:    T-040, TODO/memory.md, the record's other P0
Files:       vendor/rqbit/crates/librqbit/src/torrent_state/live/peers/mod.rs
             vendor/rqbit/crates/librqbit/src/torrent_state/live/mod.rs
             patches/rqbit/0018-crates-librqbit-src-torrent_state-live-mod.rs.patch
             patches/rqbit/0022-crates-librqbit-src-torrent_state-live-peers-mod.rs.patch
Upstream:    ours. It is rqbit#525 (https://github.com/ikatson/rqbit/issues/525),
             open, so a release may carry a fix of their own.
Added:       2026-08-22T15:30Z
```

`PeerStates::states` is a `DashMap<SocketAddr, Peer>` that only ever grew. A
peer that hands over cleanly ends in `PeerState::NotNeeded` and stays there;
`drop_peer` was called on exactly two paths, a bug branch and backoff
exhaustion. T-040 measured it: **one row per completed handshake, exactly**,
2,000 connections leaving 2,000 rows, `live` and `dead` both zero, and a minute
of silence returning none of the memory. A row is 2,891 to 4,281 bytes
depending on the range fitted, which is most of a long seeder's memory slope.

The change is a bound:

- `MAX_PEER_RECORDS`, 1,024 per torrent, well above any real swarm's live peer
  count, so a working torrent never reaches it and this only reclaims history.
  Zero disables it, which is the previous behaviour.
- `PeerStates::reclaim_records` takes rows in `NotNeeded` or `Dead` and never
  `Live`, `Connecting` or `Queued`, because those have a task or a dial behind
  them. It counts what it may take rather than evicting by age.
- Called **before** an insert on both paths that add a row, and never while an
  `Entry` on the same map is held: `DashMap` locks per shard and a second guard
  on the same shard deadlocks.
- Reclaimed rows go through `Peer::destroy`, which decrements both the
  per-torrent and the session counters and clears `live_outgoing_peers`, rather
  than through `drop_peer`, which does not do the last of those.

**The second half, and it is the part that would have looked like a bug.** A
`Dead` row can be sitting in the dial queue when it is reclaimed, and
`task_manage_outgoing_peer` answered a missing row with `Error::BugPeerNotFound`.
A bound that logs "bug" for its own correct behaviour is worse than no bound,
so that path now returns quietly: a queued handle outliving its row is ordinary
once the map is bounded.

**Why it has to be here.** `PeerStates` is `pub(crate)` inside `librqbit`, the
map is private, and no option, builder or trait reaches it. `bit-cli` carries
`--max-rss` as a backstop precisely because nothing in this tree could free a
row, and that flag stops the run rather than fixing anything.

**How it was measured.** `scripts/check-peer-rows.ps1`, 2,000 connections in
steps of 200 against one seeder, reading the row count out of the seeder's own
`progress` events:

| connections | rows before | rows after |
| --- | --- | --- |
| 1,000 | 1,000 | 1,000 |
| 1,200 | 1,200 | **1,024** |
| 2,000 | 2,000 | **1,024** |

Exactly 1,024 and flat, and one row per handshake below the bound, which is the
attribution T-040 rests on and is asserted separately: a bound that reclaimed a
live peer would also make the count flat.

**RSS at 2,000 connections is unchanged, and that is expected rather than a
disappointment.** Freeing a row returns it to the allocator, not to the
operating system, so the saving does not show as resident memory at this scale;
976 reclaimed rows are inside the run-to-run variation. What the bound changes
is that demand stops growing, which is what a process that fails at 3am needs.
The six hour soak that would show it as a flat line is `TODO/memory.md` T-040's
own acceptance and has not been run since the change.

**What would retire it.** It is
[rqbit#525](https://github.com/ikatson/rqbit/issues/525), open, and reported as
exactly this: RSS climbing in a long-lived server. A release that closes it is
the one to check this patch against.
---

## librqbit: an HTTP tracker is told about one of our two addresses

```
Unblocks:    T-022, TODO/peers.md, and it is the half that was left open
Files:       vendor/rqbit/crates/tracker_comms/src/tracker_comms.rs
             vendor/rqbit/crates/librqbit/src/session.rs
             patches/rqbit/0013-crates-librqbit-src-session.rs.patch
             patches/rqbit/0029-crates-tracker_comms-src-tracker_comms.rs.patch
Upstream:    ours. It is rqbit#537 (https://github.com/ikatson/rqbit/issues/537),
             open, so a release may carry a fix of their own.
Added:       2026-08-22T17:26Z
```

A UDP tracker is announced to once per address family. `tracker_comms.rs`
resolves the host, keeps the first IPv4 and the first IPv6 address, and fires
both. An HTTP tracker got one announce, over whichever family the connector
picked, and a tracker records the source address of the connection it was
announced over. So a dual-stack seeder registered one of its two addresses and
peers on the other family learned nothing reachable, connected, failed, and
retried.

The change gives the HTTP path the same two announces:

- `UdpTrackerResolveResult` and `udp_tracker_to_socket_addrs` are
  `TrackerResolveResult` and `tracker_to_socket_addrs`. Same code, same
  behaviour: which of our addresses a tracker can be told about has nothing to
  do with the scheme, and the HTTP path calls it now.
- `TrackerComms` takes a `ReqwestClientFactory` beside the client it already
  took. A built `reqwest::Client` cannot be reconfigured, so a second one has
  to be built, and building it from the session's own builder is what keeps
  the proxy, the bound interface and the user agent from drifting apart.
- `task_single_tracker_monitor_http` resolves the tracker each round and holds
  one client per family, each with `resolve_to_addrs` pinning that family. It
  rebuilds them only when the resolution changes, so a tracker that gains an
  AAAA record is announced to over both families from the next announce rather
  than from the next restart.
- The two announces go **in sequence**. A tracker that keys its peer records
  by peer id alone, which is all BEP 3 asks for, keeps whichever announce it
  saw last, so concurrent announces make which of our addresses it holds a
  race. T-022 measured that against the fixture before it keyed by
  `(peer id, family)`.
- Every path that cannot pin a family falls back to the session's client and
  one announce, which is exactly the previous behaviour: a URL naming an
  address, a host that resolves in one family, a resolution that fails, a
  client that will not build, and a session behind a proxy. `librqbit` passes
  `None` for the factory when a proxy is configured, because then the proxy
  resolves and the local family is not ours to choose.

**`ClientBuilder::local_address` is the obvious thing to reach for and it does
not work.** `hyper-util` binds the local address only when it already matches
the destination's family, and otherwise falls through to the unspecified
address of the destination's own family, so an announce from a client with
`0.0.0.0` set still goes out over IPv6. T-022 recorded that at
`hyper-util-0.1.20/src/client/legacy/connect/http.rs:794-820` when
`bit-cli trackers` hit the same wall. Overriding the resolution is what pins it.

**Why it has to be here.** `TrackerComms` owns its `reqwest::Client`, takes it
as an argument to `TrackerComms::start`, and exposes no seam for a second one.
`task_single_tracker_monitor_http` is a private method and the send site is
inside it. Nothing about any of it is reachable from a dependent crate: there
is no option, no builder and no trait a caller can implement differently, and
`bit-cli` cannot make a session register both of its addresses with an HTTP
tracker by any configuration.

**How it was measured.** `loopback-tracker` bound on `127.0.0.1` and `[::1]` at
one port, keying peer records by `(peer id, family)` and logging the source
address of every announce. One `bit-cli seed`, DHT, LSD and PEX off, announcing
to that tracker under two names.

| case | tracker URL | before | after |
| --- | --- | --- | --- |
| `dual_host` | `http://localhost:<port>/announce` | **ipv6 only**, from `::1` | **ipv4 from 127.0.0.1 and ipv6 from ::1** |
| `literal_host` | `http://127.0.0.1:<port>/announce` | ipv4, from `127.0.0.1` | ipv4, from `127.0.0.1` |

```bash
pwsh -NoProfile -File scripts/check-tracker-family.ps1
```

`bench/tracker-family-20260822T172231576Z.json` is the before, taken with the
two files stashed and the tree rebuilt, and
`bench/tracker-family-20260822T172549738Z.json` is the after.

**`literal_host` is the control and it is the point of having two cases.** It
takes the fallback path, which is the old code, so it announces once and must
keep announcing once. A check that reported two families there would be
reporting that something announces twice regardless, which measures nothing.
The before run is what says the check can fail: it names `ipv6` alone, which is
the resolver's order rather than a choice, and an IPv4-only peer reading that
tracker got nothing it could dial.

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

**What would retire it.** It is
[rqbit#537](https://github.com/ikatson/rqbit/issues/537), open, and the UDP path
in the same file is already the shape ours makes the HTTP one match. A release
that closes that issue is the one to check this patch against.
---

## librqbit: an incoming peer is recorded under our own peer id

```
Unblocks:    T-210, TODO/peers.md, and T-132 could not work without it
Files:       vendor/rqbit/crates/librqbit/src/peer_connection.rs
             patches/rqbit/0009-crates-librqbit-src-peer_connection.rs.patch
Upstream:    ours. A defect in their code, so a release may fix it
             independently; check at the next one.
Added:       2026-08-22T17:55Z
```

`manage_peer_incoming` builds the handshake it is about to send, writes it, and
then hands **that** handshake to `on_handshake` and asks **it** whether the
extension protocol is supported:

```rust
let handshake = Handshake::new(self.info_hash, self.peer_id);
let hlen = handshake.serialize_unchecked_len(&mut *write_buf);
// ... written to the peer ...
let handshake_supports_extended = handshake.supports_extended();
self.handler.on_handshake(handshake, incoming.kind)
```

Both answers are about this session rather than about the peer. Two things
follow:

- **Every incoming peer is filed under our own peer id.** `on_handshake` calls
  `set_peer_live`, which records `handshake.peer_id`, so anything asking who a
  peer is gets ourselves.
- **Every incoming peer is assumed to speak BEP 10.** `Handshake::new` always
  sets the extension bit, so `handshake_supports_extended` is unconditionally
  true for an incoming connection whatever the peer said.

`manage_peer_outgoing`, forty lines below, reads the peer's handshake off the
wire and uses that for both. The two paths disagreeing is what says this is a
slip rather than a design.

The change uses `incoming.handshake`, which is the peer's, already read by the
session's accept path and already validated eight lines above for a wrong info
hash and for a self-connection. The handshake being sent is renamed `ours`, so
neither can be reached for by accident again.

**Why it has to be here.** `manage_peer_incoming` is a method on
`PeerConnection`, the handshake never leaves it, and no option, callback or
trait a dependent crate can implement reaches either line.

**How it was measured.** By a rate limit that did not limit.
`scripts/check-rate-scope.ps1`'s `http_peer_cap` phase caps swarm peers and
attaches an HTTP source, which reaches the session as an **incoming** peer over
loopback and is exempt from that cap by its peer id prefix:

| | before | after |
| --- | --- | --- |
| HTTP under an 8 MiB/s peer cap | **8.40 MiB/s**, the cap | **151.84 MiB/s** |

The exemption matched nothing before, because the id it was matching against
was ours. `bench/rate-scope-20260822T175543220Z.json`.

**What would retire it.** Three lines, a defect in their code, and the outgoing
path beside it is what makes it obvious once anybody looks. A release that
records an incoming peer under the peer id it sent takes this patch with it.

---

## librqbit: a download limit that some peers do not pass through

```
Unblocks:    T-132, TODO/multi-source.md
Files:       vendor/rqbit/crates/librqbit/src/limits.rs
             vendor/rqbit/crates/librqbit/src/torrent_state/live/mod.rs
             patches/rqbit/0008-crates-librqbit-src-limits.rs.patch
             patches/rqbit/0018-crates-librqbit-src-torrent_state-live-mod.rs.patch
Upstream:    ours, and shaped for this repository rather than for anyone else.
             No release retires it.
Added:       2026-08-22T17:55Z
```

`Limits` had two limiters, a total up and a total down, and `LimitsConfig` has
exactly two fields. Nothing was scoped to a peer, so a cap that excludes one
peer could not be expressed. That is a problem for any client that also feeds
the session from a source of its own: `bit-cli` bridges each HTTP web seed in
as an ordinary peer over loopback, so every download cap reached it too and
there was no way to cap the swarm alone.

The change adds a third limiter and a way to skip it:

- `Limits::peer_down`, a second download limit, off unless set. `down` still
  bounds everything, which is what a total does.
- `Limits::exempt`, a list of peer id prefixes `peer_down` does not apply to.
  A prefix rather than a whole id because a client's own bridge generates a
  fresh id per connection and only the first eight bytes identify it; a prefix
  rather than an address because that bridge dials in from an ephemeral port
  and reconnects on a new one.
- `prepare_for_download_from(peer_id, len)` charges `down` for everyone and
  `peer_down` for everyone not exempt. The old `prepare_for_download` stays.
- `PeerHandler` carries the peer's id in a `OnceLock`, set in `on_handshake`,
  which is the first point an outgoing peer's id is known. The chunk requester
  cannot run before the handshake, so it is always set by the time it is read.

**Set through a setter rather than through `LimitsConfig`.** `LimitsConfig` is
`Serialize`, `Deserialize` and constructed as a struct literal in four places
across two repositories, so a third field would break each of them for a value
that is set at runtime anyway, next to `set_download_bps`.

**There is no upload counterpart and that is deliberate.** A source bridged
into a session is a seed: it sends `Bitfield` and `Unchoke`, answers `Request`,
and never sends `Interested` and never requests. Nothing is uploaded to it, so
the upload limits already reach peers alone. The doc comment on `peer_down`
says so, because the asymmetry is the first thing a reader will ask about.

**Why it has to be here.** `Limits` is `librqbit`'s, both limiter calls are
inside a private method of a private type, and `LimitsConfig` has no field a
caller could use to say "not this peer". `bit-cli` cannot cap its swarm without
capping its own HTTP sources by any configuration.

**How it was measured.** `scripts/check-rate-scope.ps1`, ten phases against one
payload, one mirror and one seeder, `bench/rate-scope-20260822T175543220Z.json`:

| phase | total | HTTP | peers |
| --- | --- | --- | --- |
| `http_peer_cap` | 151.84 MiB/s | 151.84 | 0 |
| `peer_ceiling` | 259.11 MiB/s | 0 | 259.11 |
| `peer_peer_cap` | 8.42 MiB/s | 0 | 8.42 |
| `hybrid_both_caps` | 27.57 MiB/s | 18.31 | 9.26 |

Each assertion is arranged so it is an invariant rather than a race: one source
is the only supplier in the two rows that matter. `peer_ceiling` exists so
`peer_peer_cap` means something, because a cap that holds on a slow peer has
measured nothing.

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

**Nothing retires this one.** The others in this file are defects in upstream's
code and a release can fix them. This is a feature, and the shape is ours: a
list of peer id prefixes exempt on the session. If upstream ever grows a
per-connection `LimitsConfig`, that is the seam to rebuild this on rather than a
fix that lands underneath it.
---

## librqbit: a message larger than the read buffer cannot be received

```
Unblocks:    T-195, TODO/peers.md, the residual T-194 left behind
Files:       vendor/rqbit/crates/librqbit/src/read_buf.rs
             vendor/rqbit/crates/librqbit/src/peer_connection.rs
             vendor/rqbit/crates/librqbit/src/peer_info_reader/mod.rs
             vendor/rqbit/crates/librqbit/src/torrent_state/live/mod.rs
Upstream:    ours. A defect in their code, so a release may fix it
             independently; check at the next one.
Added:       2026-08-22T18:57Z
```

`ReadBuf` is the ring buffer every peer connection reads into, and it was a
`Box<[u8; BUFLEN]>` with `BUFLEN` = 32,768. A message that does not fit in it
fails with "read buffer is full" and the connection dies. One message is not
bounded by anything that constant knows about: a bitfield is one bit per piece,
so past **262,104 pieces** it does not fit, and no configuration of either end
changes that. T-194 moved the send side off a fixed buffer; this is the same
defect read from the other side, and it was the binding limit afterwards.

The change is that the buffer grows:

- `buf` is a `Box<[u8]>`, and every place the ring arithmetic said `BUFLEN`
  reads the current capacity instead. `BUFLEN` is what a connection starts
  with.
- `grow` doubles into a new allocation and copies the two halves contiguously
  to the front, which is what `make_contiguous` already did for a different
  reason. It is called from one place, the `NotEnoughData` arm, when the buffer
  is full and the message is not finished. When it refuses, the caller fails
  exactly as it did before.
- `ReadBuf::max_len` bounds it, and `set_max_len` never lowers it below
  `BUFLEN`, so this can only ever permit more than the old behaviour.

**Where the bound comes from is the whole of the design.** It is never the
length prefix the peer sent, which is the number a hostile peer picks. It comes
from a new trait method, `PeerConnectionHandler::max_incoming_message_len`,
whose default is `BUFLEN`, so an implementor that does not answer keeps the
behaviour it had:

- The live torrent's handler answers from **its own piece count**: one bitfield
  plus `MAX_MSG_LEN` of slack. A peer can make the buffer as large as one
  bitfield for the torrent it is talking about and no larger.
- `peer_info_reader` cannot, and that is the interesting case. A seeder sends
  its bitfield immediately after the handshake, before this side has the
  metadata, so the message that arrives is as large as the torrent makes it
  while the piece count is the exact thing not known yet. It answers with a
  constant, `MAX_BITFIELD_BEFORE_METADATA` = 1 MiB, which is 8,388,568 pieces,
  128 GiB at a 16 KiB piece length and 32 TiB at 4 MiB.

The connection sets it: `manage_peer_outgoing` on the buffer it creates, and
`manage_peer_incoming` on the one the session handed it, which was filled with
the handshake before anyone knew which torrent it was for.

**Why it has to be here.** `ReadBuf` is private to `librqbit`, the buffer is a
private field, `read_message` is where the failure is raised, and
`PeerConnectionHandler` is `pub(crate)`. There is no option, no builder and no
trait a dependent crate can reach any of it through. `bit-cli` cannot fetch a
torrent past 262,104 pieces by any configuration.

**How it was measured.** `scripts/check-bitfield.ps1`, a seeder and a magnet
fetch on loopback with trackers and DHT off. Metadata resolving and the file
appearing is the pass, and both need the bitfield to have crossed:

| pieces | `.torrent` | bitfield | before | after |
| --- | --- | --- | --- | --- |
| 262,104 | 5,242,219 B | 32,768 B | resolves | resolves |
| **262,105** | 5,242,239 B | 32,769 B | `read buffer is full` | **resolves** |
| **524,288** | 10,485,900 B | 65,541 B | `read buffer is full` | **resolves** |
| **1,048,576** | 20,971,661 B | 131,077 B | `read buffer is full` | **resolves** |

`bench/bitfield-20260822T185725425Z.json` is the million-piece run.

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

140 upstream tests passed when this landed, one of them new:
`test_read_buf_grows_for_a_message_larger_than_itself`, which asserts both
directions, that the message is refused with the default bound and read with a
raised one.

**The unsafe reborrow is still sound and the growth path is inside what proves
it.** `read_message` holds a stacked reborrow of `self` across the deserialize,
and growth reallocates the buffer that reborrow points into.
`test_read_buf_miri` reads an oversized bitfield as well as a piece now, so
that happens under miri:

```bash
cargo +nightly miri test --manifest-path vendor/rqbit/Cargo.toml -p librqbit --features miri test_read_buf_miri -- --ignored
```

Two things about running that on Windows, because both cost time here.
`cargo-miri` fails with "cargo uses an argfile to invoke rustc" once the
command line gets long, and a short `CARGO_TARGET_DIR` is the way past it. And
`with_timeout` is a no-op only under `--features miri`, so a test that reaches
it cannot run outside miri without a tokio runtime.

**What it does not fix.** The pre-metadata bound is a constant rather than a
fact about the torrent, so it is a limit rather than the absence of one.
Removing it properly means skipping a message this side has no use for instead
of buffering it, which changes `read_message` from "return a message" to "may
drop one". T-195 records that and nothing in this repository needs it.

**What would retire it.** A release that sizes the read the same way. It is a
defect in their code with a one line reproduction, and the trait method's
default means no implementor of theirs has to change, so it is a cheap fix for
them to make independently.
---

## librqbit: a resume cache cannot exist without a session store

```
Unblocks:    T-016, TODO/disk-io.md, which was blocked on exactly this
Files:       vendor/rqbit/crates/librqbit/src/lib.rs
             vendor/rqbit/crates/librqbit/src/session.rs
             vendor/rqbit/crates/librqbit/src/torrent_state/initializing.rs
             vendor/rqbit/crates/rqbit/src/main.rs, which builds the struct
             patches/rqbit/0007-crates-librqbit-src-lib.rs.patch
             patches/rqbit/0013-crates-librqbit-src-session.rs.patch
             patches/rqbit/0017-crates-librqbit-src-torrent_state-initializing.rs.patch
             patches/rqbit/0027-crates-rqbit-src-main.rs.patch
Upstream:    ours. Two thirds is a seam and a feature; the third part, the
             validate_fastresume key, is a defect a release may fix on its own
Added:       2026-08-22T19:28Z
```

`SessionOptions::fastresume` exists and does nothing unless `persistence` is
also set. `persistence_factory` reads it inside a macro that only the two
persistence arms reach; the `None` arm returns `NonPersistentBitVFactory`
whatever `fastresume` says. So the only way to have a resume cache is to turn
on a store that writes a record of every torrent in the session.

Those are different things. A resume cache is derived data: delete it and the
next run recomputes it, slowly, and is otherwise identical. A session store is
state: delete it and the session forgets what it was doing. `bit-cli` is a
one-shot foreground tool that keeps no state by decision, and re-hashing the
payload on every invocation costs eight minutes on a 40 GiB seed.

The change is a seam, not a policy:

- `SessionOptions::bitv_factory: Option<Arc<dyn BitVFactory>>`. Used wherever
  the session would otherwise use `NonPersistentBitVFactory`, so it changes
  nothing for a caller that leaves it unset or that already has persistence
  and `fastresume` on.
- `bitv` and `bitv_factory` are public modules, and `BitV`, `BoxBitV`,
  `DiskBackedBitV`, `BitVFactory`, `NonPersistentBitVFactory` and `BF` are
  re-exported. A caller supplying a factory has to be able to name the trait,
  return a `BitV` from it, and reach the disk-backed implementation rather than
  write a third one.
- `validate_fastresume` clears by the key `check` loaded with. It cleared by
  `shared.id`, a torrent id, while everything around it used the info hash, so
  a factory keyed by hash could not resolve it and a cache found to be corrupt
  was never removed. That third one is a defect rather than a seam.

**Nothing about the validation changed and none of it needed to.** `librqbit`
already checks the bitfield's length against the torrent, re-hashes at least
one claimed piece per file plus a random sample of the rest, and throws the
whole thing away and clears the cache on a single failure. That is the part
that makes a resume cache safe and it was already written.

**Why it has to be here.** `persistence_factory` is a nested function inside
`Session::new_with_opts`, the modules were private, and `SessionOptions` had no
field to put a factory in. There is no option, builder or trait a dependent
crate could reach any of it through, which is why T-016 sat blocked on a
decision it was not really about.

**How it was measured.** `scripts/check-fastresume.ps1`, one 512 MiB payload of
1 MiB pieces, five `bit-cli seed --announce-only` runs:

| run | `--fastresume` | elapsed | reports complete |
| --- | --- | --- | --- |
| `cold` | yes, empty cache | 2.38 s | yes |
| `warm` | yes | **2.06 s** | yes |
| `stale`, one byte rewritten | yes | 2.38 s | **no** |
| `refresh` | yes | 2.05 s | no |
| `no_flag` | no | 2.37 s | no |

The clock says the check was skipped and the `complete` column says the cache
was right: `warm` claims the whole payload without hashing it, and `stale`
refuses the cache, hashes again, and finds the one piece that changed. A run
that trusted a stale cache would have announced a piece it does not have.

**What would retire which third.** The `bitv_factory` seam and the module
exports are a feature in our shape, and no release lands them. The
`validate_fastresume` key is a defect, so a release may fix that third on its
own and leave the rest of this patch standing. Check the three separately at a
reconciliation.
---

## librqbit: the enum a public type's only field holds is private

```
Unblocks:    T-025, TODO/peers.md
Files:       vendor/rqbit/crates/librqbit/src/http_api_types.rs
             patches/rqbit/0006-crates-librqbit-src-http_api_types.rs.patch
Upstream:    ours. A one line omission in their code, so a release may add the
             export on its own
Added:       2026-08-22T19:38Z
```

`http_api_types` re-exports `PeerStatsFilter`. That type has exactly one field,
of type `PeerStatsFilterState`, and the enum was not re-exported with it. So a
dependent crate could name the filter and not build one: the variant that asks
for every peer rather than only the connected ones has no name outside the
crate.

`bit-cli` needs every peer, including one that took two gigabytes and left, so
it built the value through the type's own `Deserialize` from the literal
`{"state":"all"}` with an `unwrap_or_default()` behind it. That works and reads
badly, and the fallback would have quietly narrowed the report to live peers if
the literal had ever stopped parsing.

The change adds `PeerStatsFilterState` to the same `pub use`.

**Why it has to be here.** It is an export. There is nowhere else.

**How it is held.** `crates/bit-cli-core/src/engine.rs`, `all_peers_filter`,
constructs `PeerStatsFilter { state: PeerStatsFilterState::All }` with no
`serde_json` in it. `cargo test --workspace` covers the reports that read it.

**What would retire it.** One line, no behaviour, and the type it completes is
already public, so a release that exports the enum takes this patch with it.
---

## librqbit: BEP 54 lt_donthave is received and ignored

```
Unblocks:    T-167, TODO/bep-coverage.md, which was blocked on exactly this
Files:       vendor/rqbit/crates/peer_binary_protocol/src/lib.rs
             vendor/rqbit/crates/peer_binary_protocol/src/extended/mod.rs
             vendor/rqbit/crates/librqbit/src/torrent_state/live/mod.rs
             vendor/rqbit/crates/librqbit/src/piece_tracker.rs
Upstream:    ours. A BEP two other clients implement, so a release may add the
             receive side on its own; check at the next one.
Added:       2026-08-22T19:49Z, extended 2026-08-23T03:26Z
```

A peer's bitfield only ever grew. BEP 3 has `Have` and no inverse, so a peer
that loses a file cannot withdraw the claim and the far end goes on asking it
for pieces it cannot serve. BEP 54 `lt_donthave` is that message and `librqbit`
had no receive side: `PeerExtendedMessageIds` carried `ut_metadata` and
`ut_pex` and nothing else, so one arrived as `ExtendedMessage::Dyn` and fell to

```rust
message => {
    warn!("received unsupported message {:?}, ignoring", message)
}
```

which is a log line per retracted piece and no change to what is requested.

The change is the whole receive side:

- `MY_EXTENDED_LT_DONTHAVE = 4`, and `PeerExtendedMessageIds` carries
  `lt_donthave`. That struct **is** the `m` dictionary of the extended
  handshake, so adding the field advertises the extension.
- `ExtendedMessage::LtDontHave(u32)`, with its own serialize and deserialize.
  It cannot go through the generic `Dyn` arm: every other extension message
  here has a bencoded body and this one's payload is four big-endian bytes and
  nothing else.
- `PeerHandler::on_donthave` clears the bit, and is the inverse of `on_have`
  down to the shape. One difference, deliberate: a bitfield that was never
  allocated is left alone rather than allocated and cleared, because a peer
  that has claimed nothing cannot retract anything.

**Why it has to be here.** `PeerExtendedMessageIds` is the wire's `m`
dictionary, the message dispatch is a private method on a private type, and
`peers::update_bitfield` is `pub` inside a private module tree, so it reaches
nothing. T-167 recorded the near miss: `pub` in a private module looks like a
seam and is not one.

**How it was measured.** Two tests in `peer_binary_protocol`:
`test_lt_donthave_round_trips` asserts the ten byte wire form, the extension
id, the big-endian payload, and that it deserializes back to the same piece;
`test_lt_donthave_needs_the_peer_to_have_asked_for_it` asserts a peer that
never advertised the extension cannot be sent one.

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

143 upstream tests passed when this landed, three of them new. The
current total is on `PROGRESS.md`'s state line: a number stated here is what
it was on the day, because a section that quotes a moving total is a number
two files disagree about.

**What is proved and what is not.** The message round trips on the wire and is
dispatched to a handler that clears the bit. It is not yet proved end to end,
because nothing in this repository sends one: that is the send half, which is
`bit-cli`'s own web seed bridge, and T-167 carries it. This patch is what makes
sending one worth doing.

**Extended 2026-08-23: clearing the bit was not the whole of honouring it.**
The first version cleared the bitfield bit and stopped there, which stops the
peer being **picked** for that piece again and does nothing about the piece
already assigned to it. A retracted piece stayed in flight against a peer that
had just said it cannot serve it, until something else timed it out, and the
entry's acceptance says the session has to stop asking.

`PieceTracker::release_piece_owned_by(piece, peer)` is the missing half:
`on_peer_died` already releases **every** piece a peer owns, through
`release_pieces_owned_by`, and this is one piece of that. `on_donthave` calls
it after the bitfield closure returns, outside the peer lock that closure
holds, and notifies the piece waiters so another peer can take it. A peer that
does not own the piece cannot release it, which is the case that would let one
peer's retraction cancel another peer's download, and
`test_release_piece_owned_by_one_peer` asserts exactly that.

**What would retire it.** A BEP with an implementation in two other clients and
a receive side of twenty lines is one a release may well add. A client that
honours a retraction without sending one is a posture another project has taken
deliberately, so upstream landing the receive half alone would retire this patch
and leave our send half where it is.

---

## librqbit: a peer connection cannot be wrapped before the handshake

```
Unblocks:    T-163, TODO/peers.md, MSE/PE peer encryption
Files:       vendor/rqbit/crates/librqbit/src/stream_transform.rs (new)
             vendor/rqbit/crates/librqbit/src/lib.rs
             vendor/rqbit/crates/librqbit/src/type_aliases.rs
             vendor/rqbit/crates/librqbit/src/stream_connect.rs
             vendor/rqbit/crates/librqbit/src/peer_connection.rs
             vendor/rqbit/crates/librqbit/src/session.rs
             vendor/rqbit/crates/rqbit/src/main.rs
             patches/rqbit/0007-crates-librqbit-src-lib.rs.patch
             patches/rqbit/0009-crates-librqbit-src-peer_connection.rs.patch
             patches/rqbit/0013-crates-librqbit-src-session.rs.patch
             patches/rqbit/0015-crates-librqbit-src-stream_connect.rs.patch
             patches/rqbit/0016-crates-librqbit-src-stream_transform.rs.patch
             patches/rqbit/0023-crates-librqbit-src-type_aliases.rs.patch
             patches/rqbit/0027-crates-rqbit-src-main.rs.patch
Upstream:    ours. A seam rather than a fix, and no release will carry this
             shape of it
Added:       2026-08-23T02:52Z
```

A new trait, `StreamTransform`, and one `SessionOptions` field holding it. It
is called once per peer connection in each direction, on the two halves, before
any protocol byte crosses them: after `StreamConnector::connect` in
`manage_peer_outgoing`, and before `ReadBuf::read_handshake` in
`Session::check_incoming_connection`. It hands back the halves to use from
there on.

Two parts of the shape are not obvious and both are forced by MSE.

**The accepting side is handed every info hash the session holds.** MSE keys
its handshake on the info hash, and the info hash is inside what the transform
is about to decrypt, so it cannot be told which torrent the connection is for.
It resolves it against the candidates instead and reports which one it found.
`check_incoming_connection` already reads the session database a few lines
later for exactly the same purpose.

**The dialling side may ask for the connection to be made again.**
`OutgoingTransform::RetryPlaintext` is that answer, and `manage_peer_outgoing`
redials once with the transform skipped. A transform that offered encryption to
a peer which does not speak it has spent that connection finding out: the peer
saw 96 bytes of Diffie-Hellman key where it wanted the plaintext protocol
header and dropped the socket. Without the redial, "prefer encryption" would
mean "lose every plaintext peer until librqbit's own backoff dials it again",
and that backoff is [T-138](../TODO/peers.md)'s, which grows by six.

The transform call is bounded by the same read timeout the handshake gets, on
both paths. Without that on the accepting side, a peer that opens a connection
and then says nothing holds a slot in the accept loop's queue for as long as it
likes, which is the shape [T-020](../TODO/peers.md) already cost a session.

Two smaller changes come with it. `type_aliases::BoxAsyncReadVectored` and
`BoxAsyncWrite` are `pub` rather than `pub(crate)`, because the trait hands both
halves to another crate and takes them back. `AsyncReadVectored` is re-exported
from the crate root for the same reason; the module around it stays private
because `AsyncReadVectoredExt` beside it has an `async fn` in a public trait and
would warn under this repository's `-D warnings` the moment it became public
API. `rqbit`'s own `main.rs` builds `SessionOptions` field by field rather than
with `..Default::default()`, so it names the new field as `None`.

**Why it has to be here.** Because there is nowhere else to stand. The connect
and accept paths are private, `PeerConnection` is `pub(crate)`, and the two
halves are `pub(crate)` type aliases over private traits. `TODO/peers.md`
T-163 recorded this as the blocker before the trees were vendored, and
`TODO/webseed.md` T-002 and `TODO/bep-coverage.md` T-102 record the same wall
from two other directions.

**Why the encryption itself is not here.** The implementation lives in
`crates/bit-cli-core/src/mse/`, this repository's own code, for three reasons.
`cargo test --workspace` runs it and does not run the vendored crates' tests;
the vendored diff stays a seam, which is what the next reconciliation has to
read; and the policy, the flag and the reporting are `bit-cli`'s rather than
`librqbit`'s.

**What was decided against, and why.** `patches/TASKS.md` section 3 asked
whether to take upstream's shape from
[rqbit#633](https://github.com/ikatson/rqbit/pull/633), which adds
`crates/librqbit/src/mse/` inside the library, or our own. Ours, and the reason
is the paragraph above: their shape puts the whole implementation in the tree
this repository has to reconcile, and reconciling somebody else's crypto on
every release is a cost paid forever for a feature that already works. If that
pull request lands, this seam is a change across seven files to weigh against
it rather than a competing implementation, and the decision can be taken then
with the code in hand.

**How it was measured.** `scripts/check-encryption.ps1`, seven phases, three
seeders differing only in `--encryption`, and two of the seven are controls
that must fetch nothing.

```bash
pwsh -NoProfile -File scripts/check-encryption.ps1
```

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

**Nothing retires this one, and #633 would not.** The argument for the seam is
the same one that produced #633: encryption cannot be written outside the crate.
But #633 puts MSE **inside** the library, so a release carrying it gives us
somebody else's crypto in the merge and still no hook for ours. If that lands,
the question this section poses is whether to keep the seam and our transform or
to take theirs, and `patches/TASKS.md` section 3 has the argument for keeping
ours: the tests run here.

---

## librqbit: BEP 6, the fast extension, is not implemented at all

```
Unblocks:    T-100, TODO/bep-coverage.md, part three, which was the whole of
             what kept it open
Files:       vendor/rqbit/crates/peer_binary_protocol/src/lib.rs
             vendor/rqbit/crates/librqbit/src/peer_connection.rs
             vendor/rqbit/crates/librqbit/src/torrent_state/live/mod.rs
             vendor/rqbit/crates/librqbit/src/torrent_state/live/peer/mod.rs
             patches/rqbit/0009-crates-librqbit-src-peer_connection.rs.patch
             patches/rqbit/0018-crates-librqbit-src-torrent_state-live-mod.rs.patch
             patches/rqbit/0022-crates-librqbit-src-torrent_state-live-peers-mod.rs.patch
             patches/rqbit/0026-crates-peer_binary_protocol-src-lib.rs.patch
Upstream:    ours. It is rqbit#584 (https://github.com/ikatson/rqbit/issues/584),
             open, so a release may carry an implementation of their own.
Added:       2026-08-23T03:55Z
```

Five message ids and one reserved bit, none of which this crate had. A peer
that spoke BEP 6 got `UnsupportedMessageId` and was dropped, and the two that
cost the most were `have all`, which a seeder sends in place of a bitfield, and
`reject request`, which is how a peer says no without hanging up.

**The wire.** `MSGID_SUGGEST_PIECE` 13, `MSGID_HAVE_ALL` 14,
`MSGID_HAVE_NONE` 15, `MSGID_REJECT_REQUEST` 16, `MSGID_ALLOWED_FAST` 17, each
with a `Message` variant, serialize and deserialize.
`Handshake::supports_fast` reads the reserved bit, which is the **last**
reserved byte and `0x04`, a different byte from BEP 10's; `Handshake::new` sets
it. `reject request` shares its three `u32` body with `request` and `cancel`
and is a third variant rather than a flag on either, because confusing them
turns a refusal into a demand.

**The receive side.** `have all` fills the peer's bitfield up to the piece
count and no further, so the spare bits past the last piece stay zero exactly
as a wire bitfield's must; `have none` sets an empty one, which is not the same
as sending no bitfield at all and is recorded as the peer having said so.
`reject request` releases the whole piece rather than the one chunk: a peer
that will not serve one chunk of a piece is not about to serve the rest, and
leaving the piece assigned to it stalls it just as long.
`PeerState::drop_inflight_requests_for_piece` is the half that forgets the
chunks without sending a `Cancel` back for each, which would be noise addressed
to the peer that just refused.

`suggest piece` and `allowed fast` are understood, traced and not acted on. A
suggestion is advice about which piece to ask for and this picker has its own
order, and an allowed-fast piece is one the peer would serve while choking, and
nothing here chokes. Both would be worth acting on and neither is claimed to
be: what this buys is that a peer sending either is no longer a protocol
error.

**The send side, and one thing it forced.** BEP 6 makes the first message
mandatory: a peer that negotiated it expects a bitfield, a have-all or a
have-none before anything else, and sending nothing is a protocol violation
rather than an omission. `should_send_bitfield` returns false when this end has
nothing, and that used to mean silence; with the extension it means `have
none`. `PeerConnectionHandler::have_shortcut` answers from the have-bitfield
rather than from a byte count, because "we have every piece" and "we have every
selected piece" are different statements and only the first may be sent as
`have all`.

A request this end cannot serve is answered with `reject request` when the
extension was negotiated, where it used to end the connection. Asking a partial
seed for a piece it does not hold is a normal thing to do.

**Why it has to be here.** The message ids are a private `const` block in
`peer_binary_protocol` and `Message` had no variant for any of the five, which
a dependent crate cannot add; the reserved bit is set inside `Handshake::new`;
and the receive side is a private method on a private type. T-100 recorded this as
"part three, blocked, upstream" and named these exact two files.

**A dead test found on the way, and it had been dead for a session.**
`test_bitfield_larger_than_max_msg_len`, which is [T-194](../TODO/peers.md)'s
own regression test, carried no `#[test]` attribute: the one it needed had
landed on the test above it, which then had two. It was compiled and never run.
It is attributed now and it passes. Nothing catches this from the workspace,
because `cargo clippy --workspace` does not compile the vendored crates' test
targets, so the duplicate-attribute warning only appears when the vendored
tests are run.

**How it was measured.** Three new tests in `peer_binary_protocol`, all five
messages round-tripping on the wire with their BEP ids and lengths, `reject
request` proved distinct from `request` and `cancel`, and both reserved bits
asserted at the byte they live in. Then two in this repository's own
`bridge_protocol.rs`, against a session written by hand that shares no constant
with the bridge: a complete source announces `have all` when the extension is
negotiated and a bitfield when it is not, and an out-of-scope request comes
back as `reject request` with the connection still serving afterwards.

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

```bash
cargo test -p bit-cli-core --test bridge_protocol
```

---

## rqbit: `nix` is pinned a minor version behind

```
Unblocks:    nothing. It is dependency maintenance, and it is here because the
             file it changes is somebody else's.
Files:       vendor/rqbit/Cargo.toml
             vendor/rqbit/Cargo.lock
             patches/rqbit/0002-Cargo.toml.patch
             patches/rqbit/0001-Cargo.lock.patch
Upstream:    ours, and the shortest lived patch here: upstream bumps the crate
             on its own schedule and this patch disappears when it does.
Added:       2026-08-23T04:20Z
```

`nix = "0.30"` to `"0.31"`. `librqbit` uses it for one call,
`nix::sys::uio::pwritev` in `storage/filesystem/opened_file.rs`, and nothing in
0.31 touches that signature.

**Why it has to be here.** Because `[patch.crates-io]` redirects the crate to
this tree, so the version this repository ships is the one
`vendor/rqbit/Cargo.toml` names, and dependabot opened a pull request against
that file rather than against ours. Leaving it would keep this repository a
minor version behind on a crate it does not itself depend on and cannot bump
from its own manifest.

**How it was checked.** The vendored tests, which are the only thing that
exercises the `pwritev` path, and they are unix-only so this is the platform
where it matters.

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

---

## librqbit-tracker-comms: an HTTP announce has no deadline and no ceiling

```
Unblocks:    nothing on its own. It is nzbd's 0005 read and taken in part, and
             TODO/trackers.md is where the tracker entries live.
Files:       vendor/rqbit/crates/tracker_comms/src/tracker_comms.rs
             patches/rqbit/0029-crates-tracker_comms-src-tracker_comms.rs.patch
Upstream:    ours. nzbd has a draft of the same bound, unsent, at
             contrib/rqbit/TRACKER_REQUEST_BUDGET_PR.md in that repository, so
             a release may carry a bound from that direction rather than ours.
Added:       2026-08-23T04:35Z
```

Three things a tracker could decide for this process, and none of them was
bounded.

- **How long an announce takes.** Neither `reqwest` client `session.rs` builds
  carries a timeout, so a tracker that accepts the connection and answers one
  byte a minute held an announce task for as long as it liked. Thirty seconds
  over the whole exchange now, headers and body together.
- **How much it allocates.** `Response::bytes()` reads the whole body with no
  ceiling, so the size of this process's allocation was a number the tracker
  picked. One megabyte now, checked against the running total rather than
  against `Content-Length`: the header is checked when it is there and never
  trusted, so a missing or lying one changes nothing. A compact peer list is
  six bytes per peer, so the limit is about 175,000 peers.
- **How often we come back.** `interval: 0` gave an announce loop with no
  sleep in it. Floored now.

**The floor is five seconds and not sixty, and that is a deliberate departure
from the patch this was read from.** nzbd's `0005` takes 60, and its own draft
says outright that this is a policy tradeoff rather than a safety check: a
tracker legitimately asking for 10 seconds would be delayed to 60. The UDP path
**in this same file** already clamps to five, so five is the number this
codebase had already chosen for the same question, and matching it makes one
protocol have one answer. Raising both to sixty is a decision about how often
to talk to honest trackers, and it is not this change's to make.

**Why it has to be here.** `tracker_one_request_http` is a private method and
the client is built inside `Session::new_with_opts`. `bit-cli` sets
`--tracker-timeout` on **its own** tracker client, in
`crates/bit-cli-core/src/tracker.rs`, which is the one `bit-cli trackers` uses
and not the one the session announces with. Two clients, one of which was
reachable from the command line and one of which was not.

**How it was measured.** Three tests in `bounds_tests`: a zero interval is
floored, an honest interval from five seconds to thirty minutes is not touched,
and the UDP path's clamp produces the same number as the HTTP path's floor,
which is what says the two agree rather than happening to look similar.

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit -p librqbit-tracker-comms
```

**What is not proved.** That a hostile tracker is refused, because there is no
fixture that is one. `loopback-tracker` answers correctly by construction, and
the three bounds are asserted on the functions rather than through a socket.

---

## librqbit: a peer row says a peer is dead and never why

```
Unblocks:    T-024, TODO/peers.md
Files:       vendor/rqbit/crates/librqbit/src/torrent_state/live/peer/mod.rs
             vendor/rqbit/crates/librqbit/src/torrent_state/live/peer/stats/atomic.rs
             vendor/rqbit/crates/librqbit/src/torrent_state/live/peer/stats/snapshot.rs
             vendor/rqbit/crates/librqbit/src/torrent_state/live/mod.rs
             patches/rqbit/0018-crates-librqbit-src-torrent_state-live-mod.rs.patch
             patches/rqbit/0022-crates-librqbit-src-torrent_state-live-peers-mod.rs.patch
Upstream:    ours. Two counters and one bounded list on a private method, so a
             release may expose the reason its own way; check at the next one.
Added:       2026-08-23T05:19Z
```

Two questions a peer row could not answer, and the second is the one that
stings: **`on_peer_died` takes the reason as an `Option<Error>` and threw it
away.** The state went to `Dead` and the snapshot said `dead`, which is a fact
about the row rather than about what happened.

- **`Peer::disconnects`**, a bounded `VecDeque` of `(when, why)`, newest last.
  Four per peer, because a flapping peer produces one per flap and the peer
  table holds 1,024 rows, so this is the second factor in a product that has to
  stay small. The reason is truncated at 200 bytes: an `anyhow` chain can be a
  paragraph and the first line is the part that says what happened. `None` for
  a connection that ended with no error, which is a peer that hung up cleanly
  and is a different fact from a peer whose reason is unknown.
- **`times_choked` and `times_unchoked`** on the counters, bumped in
  `on_i_am_choked` and `on_i_am_unchoked`. A peer that chokes goes quiet and
  looks exactly like one that is slow, and these are the two numbers that tell
  them apart.

**Why it has to be here.** `on_peer_died` is a private method, `Peer` is
`pub(crate)`, and `PeerStats` is what a dependent crate is given. There is no
seam that would let the reason out.

**How it was measured.** `a_peer_that_leaves_is_reported_with_a_reason_and_a_time`
in `crates/bit-cli/src/cmd/seed.rs`. A raw socket completes a BEP 3 handshake
against a running `bit-cli seed --json` and closes, and the report carries the
row with an ISO 8601 time and the reason the read actually failed with. What is
asserted is that the reason is a real one rather than a stand-in, which is
T-024's own wording.

```bash
cargo test -p bit-cli --lib a_peer_that_leaves_is_reported
```

---

## librqbit: a Windows write that makes no progress loops forever

```
Unblocks:    T-178, TODO/windows.md
Files:       vendor/rqbit/crates/librqbit/src/storage/filesystem/opened_file.rs
             patches/rqbit/0014-crates-librqbit-src-storage-filesystem-opened_file.rs.patch
Upstream:    ours. A four line guard on their own loop, and the read side of it
             is already theirs, so a release may add the write side its own
             way; check at the next one.
Added:       2026-08-23T09:32Z
```

`pwrite_all` on Windows loops because `seek_write` may write fewer bytes than
it was given. It subtracted what was written from what is left and had no
branch for a write that reported success having written nothing: `remaining`
then never decreases and the loop asks again forever, on the thread that owns
that write. `WriteFile` succeeding with zero bytes written is rare and is not
impossible, and a full volume, a disconnected network share or a filter driver
can each produce one. The guard returns `std::io::ErrorKind::WriteZero`, which
is what `write_all` in the standard library returns for the same thing, and the
message names the offset and how many bytes were left.

`pread_exact`, twenty lines above it in the same file, already refuses the
read side of exactly this shape with `UnexpectedEof`. This is the write side of
a guard upstream wrote, not a new idea imposed on their code.

**Why it has to be here.** Because it is their loop. Nothing outside the crate
can reach it: `OurFileExt` is `pub` but the loop is the body of the trait
implementation for `std::fs::File`, so a caller gets the loop or writes its
own.

**What holds it, and what does not.** This half is proved by reading and is
deliberately the smaller half. `bit-cli` never reaches this function: the one
`add_torrent` call in the workspace, `crates/bit-cli-core/src/engine.rs:847`,
installs `SafeStorageFactory` on every add, so every payload byte the tool
writes goes through `crates/bit-cli-core/src/storage.rs` instead. That copy of
the same loop carries the same guard and five tests, including one that drives
it with a write that returns `Ok(0)` and asserts it is asked exactly once. See
T-178, which records what happens with the guard removed.

```bash
cargo test -p bit-cli-core --lib a_write_that_makes_no_progress
```

---

## librqbit-utp: a deprecated constant that the next release turns into a failed build

```
Unblocks:    T-218, TODO/cli-surface.md
Files:       vendor/librqbit-utp/src/congestion/cubic.rs
             patches/librqbit-utp/0001-src-congestion-cubic.rs.patch
Upstream:    ours, and it is a lint in their code that a release may fix their
             own way. It is also the kind that arrives with a toolchain rather
             than with a commit, so check it at the next reconciliation.
Added:       2026-08-23T11:20Z
```

`cubic.rs` opened with `use std::{f64, time::{Duration, Instant}}`. Importing
the **module** `std::f64` puts it in scope as a path, so `f64::INFINITY` twelve
lines down resolves to `std::f64::INFINITY`, the legacy module constant, rather
than to the associated constant on the primitive. `rustc` 1.99 deprecates the
module constants, and CI sets `RUSTFLAGS: -D warnings` for the whole workflow,
so on the day 1.99 is stable that is a failed build rather than a warning.

The change is to stop importing the module. The expression is unchanged, and
with the module gone it resolves to `<f64>::INFINITY`, which is the same value
and is not deprecated. The associated constant has been stable since 1.43, well
under the 1.88 MSRV.

**Why it has to be here.** Because it is their file, and because
`[patch.crates-io]` makes their warnings ours: cargo caps lints for a registry
dependency and does not cap them for a path dependency. The first section of
this file is the same case.

**How it was proved.** Both toolchains, with the flag CI sets, before and
after. Before, `beta` fails on this line and `stable` is silent. After, both
are clean.

```bash
RUSTFLAGS="-D warnings" cargo +beta clippy --workspace --all-targets --all-features
```

---

## librqbit: handshake, piece and picker tracing have no target of their own

```
Unblocks:    T-219, TODO/cli-surface.md
Files:       vendor/rqbit/crates/librqbit/src/peer_connection.rs
             vendor/rqbit/crates/librqbit/src/file_ops.rs
             vendor/rqbit/crates/librqbit/src/torrent_state/live/mod.rs
             patches/rqbit/0009-crates-librqbit-src-peer_connection.rs.patch
             patches/rqbit/0005-crates-librqbit-src-file_ops.rs.patch
             patches/rqbit/0018-crates-librqbit-src-torrent_state-live-mod.rs.patch
Upstream:    ours. Nothing upstream is wrong: a target defaults to the module
             path, which is the right default for a library. A release could
             split these targets for its own reasons and would then retire the
             patch; check the three call sites at the next reconciliation.
Added:       2026-08-23T14:10Z
```

Thirteen `trace!` and `debug!` calls take an explicit `target:` instead of the
module path they defaulted to: three on `librqbit::handshake`, six on
`librqbit::piece`, four on `librqbit::picker`. Nothing else changes: not a level, not a field,
not a message, not a line of control flow.

- `peer_connection.rs`, three calls, to `librqbit::handshake`: the incoming
  connection, the `connected` record after the handshake is read, and the
  extended handshake going out. The `connected` record also gains
  `supports_extended` and `supports_fast`, which the function already had in
  locals and which are the result of the negotiation the record is about.
- `file_ops.rs`, two calls, and `torrent_state/live/mod.rs`, four, to
  `librqbit::piece`: the hash matching, a piece marked as needed again, the
  chunk marking result, a piece completed, a piece someone else completed, and
  a piece downloaded and verified.
- `torrent_state/live/mod.rs`, four calls, to `librqbit::picker`: choked with
  nothing to acquire, a piece reserved, a piece stolen, and nothing to request.

**Why it has to be here.** Because a `tracing` target defaults to the module
path, and the modules do not divide the way the subsystems do. `--trace
handshake` and `--trace peer` both resolve to `librqbit::peer_connection`,
which holds the handshake and every wire message in one module, so the narrower
of the two names would print the wider one's traffic: on the 2 MiB fixture
below that is 2 records against 266. `--trace picker` and `--trace piece` both resolve to
`librqbit::torrent_state::live`, which holds the picker, the piece lifecycle
and peer management together. A subsystem that raises a target carrying three
other subsystems is not a subsystem, and `--trace` exists to raise one thing
and leave the rest alone.

Doing it outside the tree is not possible: a target is decided at the callsite,
and there is no wrapper, layer or filter that can split one module's records
into three by anything other than matching their message text.

**How it was proved.** `crates/bit-cli/tests/trace_subsystems.rs` drives the
real binary once per subsystem and asserts a record on a target that subsystem
raises. Two of its cases assert the vendored targets specifically:
`handshake_traces_the_negotiation` requires `librqbit::handshake`, and
`tracker_covers_the_session_announce_as_well` requires
`librqbit_tracker_comms::tracker_comms`, which needed no patch. Measured on
one 2 MiB `download` with each name traced in turn:

```
--trace handshake   librqbit::handshake=2          bit_cli::handshake=3
--trace peer        librqbit::peer_connection=266  bit_cli::peer=133
--trace piece       librqbit::piece=153            bit_cli::piece=128
--trace picker      librqbit::picker=9             bit_cli::picker=1
```

```bash
cargo test -p bit-cli --test trace_subsystems
```

Upstream's own tests were run, because the change is in their tree:

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

---

## librqbit: `name.utf-8` and `path.utf-8` are not read, and the detector cannot be shared

```
Unblocks:    T-103, TODO/bep-coverage.md, and the seam is
             vendor/rqbit/crates/librqbit_core/src/torrent_metainfo.rs:372,
             `detect_encoding`, which is the whole of what decides a name
Files:       vendor/rqbit/crates/librqbit_core/src/torrent_metainfo.rs
             vendor/rqbit/crates/librqbit/src/create_torrent_file.rs
             vendor/rqbit/crates/librqbit/src/upnp_server_adapter.rs
             patches/rqbit/0003-crates-librqbit_core-src-torrent_metainfo.rs.patch
             patches/rqbit/0004-crates-librqbit-src-create_torrent_file.rs.patch
             patches/rqbit/0024-crates-librqbit-src-upnp_server_adapter.rs.patch
Upstream:    ours. No issue upstream names it, and `TorrentMetaV1Info` has
             carried the same field list since 9.0.0, so nothing suggests a
             release retires this on its own. Check the struct at the next
             reconciliation: a release that adds either key makes ours a
             duplicate.
Added:       2026-08-23T17:05Z
```

**Two keys and one function.** `TorrentMetaV1Info` gains `name_utf8`, from
`name.utf-8`, and `TorrentMetaV1File` gains `path_utf8`, from `path.utf-8`.
Both are `Option`, both are skipped when absent, so a torrent that carries
neither serializes exactly as before. `iter_file_details_raw` and
`ValidatedTorrentMetaV1Info::name` prefer the twin where there is one and it
holds valid UTF-8, decoding it as UTF-8 rather than through the detected
encoding; `utf8_twin` is the one predicate both use. `detect_encoding` keeps
feeding the **raw** keys only, because a correctly written twin would
otherwise talk the detector out of the encoding the raw keys are actually in.

`detect_encoding` also loses its body to `detect_encoding_of`, a free function
over an iterator of byte slices. That is the part `bit-cli` needs: this
repository parses metainfo itself and has to reach the same answer, and a
second `chardetng` call site configured by hand is a second answer waiting to
happen.

`create_torrent_file.rs` is two `None`s in struct literals, because the fields
are new and that file builds both structs by hand. Nothing created there needs
a twin: its raw keys are already UTF-8. `upnp_server_adapter.rs` is the same
two `None`s in a test helper, and it is in the series because upstream's own
tests do not build without it.

**Why it cannot be done outside the tree.** The decoded name is what
`FileInfo::relative_filename` holds, and that is what the session hands to
storage and to the web seed URL composer. There is no seam between the
deserialize and that: `iter_file_details` is the only way to read a file's
name and it applies the encoding itself. A reader outside the tree can parse
the two keys, and `bit-cli` does, but it cannot change the name the download
writes. Doing only the outside half is what produced the defect T-103 records:
a report that named `音楽/曲.bin` beside a run that wrote `‰¹Šy/‹È.bin`.

**How it was measured.** `chardetng` is right often enough that a single
example proves nothing, so fourteen names were tried across six encodings and
the guess was wrong for six of them. The one carried as a fixture is `音楽`
with `曲.bin`, cp932, which reads as windows-1252 and decodes to `‰¹Šy` and
`‹È.bin`. The common real shape is in that list too and is worse: an ASCII
release name with one non-ASCII filename under it, where the ASCII dominates
the detector's input and every non-ASCII name in the torrent comes out wrong.

**What holds it.** `the_two_decoders_in_this_tree_agree`, in
`crates/bit-cli-core/src/torrent/metainfo.rs`, parses the same bytes with both
implementations and compares every file path and the torrent name, over four
shapes. Reverting the multi-file half of this patch fails it and names both
sides:

```
the utf-8 keys: file paths disagree
  left: [["曲.bin"]]
 right: [["‹È.bin"]]
```

```bash
cargo test -p bit-cli-core --lib torrent::metainfo
```

Upstream's own tests were run, because the change is in their tree:

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

---

## librqbit-tracker-comms: a UDP announce sends its event on every request

```
Unblocks:    T-256, TODO/trackers.md, and it is a P1
Files:       vendor/rqbit/crates/tracker_comms/src/tracker_comms.rs
             vendor/rqbit/crates/tracker_comms/src/tracker_comms_udp.rs
             patches/rqbit/0029-crates-tracker_comms-src-tracker_comms.rs.patch
             patches/rqbit/0028-crates-tracker_comms-src-tracker_comms_udp.rs.patch
Upstream:    ours. No issue names it, and the HTTP path in the same file has
             the discipline already, so a release could fix this by copying
             its own neighbour.
Added:       2026-08-24T21:50Z
```

`tracker_one_request_udp` read the BEP 3 event off the torrent's **current
state** on every announce. An event is a transition and not a state, so a live
incomplete torrent sent `started` at every interval and a live complete one
sent `completed` at every interval, for as long as the process ran. The HTTP
monitor a few hundred lines above already does it correctly:
`task_single_tracker_monitor_http` sets `event = Some(Started)` once and
`event = None` after the first answered announce.

The event moves to the monitor loop as `UdpAnnounceEvents`, because only the
loop knows what the announces before this one carried. Three properties of
that placement are deliberate:

- **Peek and commit are separate.** An announce nothing answered has not
  delivered its event, so `started` is not spent on a datagram that went
  nowhere.
- **One event per round rather than per datagram.** A dual-stack tracker is
  announced to over both families at once and both carry the same event, which
  is what `tracker_one_request_http_each` does on the other side.
- **`completed` is not sent from the loop at all.** The HTTP monitor has no
  `Completed` arm, and `bit-cli` announces its own completion at the instant it
  happens, over whichever protocol the tracker speaks. A `completed` here as
  well made one run tell the tracker twice. `EVENT_COMPLETED` stays in
  `tracker_comms_udp.rs` under an `allow(dead_code)`, because BEP 15's four
  numbers are a table and a table with a hole in it reads as a mistake.

**Why it cannot be done outside the vendored tree.** The announce loop is a
private method on `TrackerComms`, the struct is built inside
`Session::new_with_opts`, and the event is not a parameter of anything public.
`bit-cli`'s own tracker client, `crates/bit-cli-core/src/tracker.rs`, is a
different client: it is what `bit-cli trackers` uses and what sends the
`completed` and `stopped` events for a download, and it never sees the
session's periodic announces.

**How it was measured.** One client, one 22 second run, the same payload over
both protocols against `loopback-tracker --announce-log`, with no seeder so
every announce is an ordinary one:

| protocol | before | after |
| --- | --- | --- |
| udp | `started` x5, `stopped` | `started`, none x4, `stopped` |
| http | `started`, none x4, `stopped` | unchanged |

**What holds it.** Four tests in `bounds_tests`, and the `udp` case in
`scripts/check-announce.ps1`, which fails on `4 completed events, and BEP 3
asks for one` with this patch reverted.

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit -p librqbit-tracker-comms
```

```bash
pwsh -NoProfile -File scripts/check-announce.ps1
```

---

## h2: a client cannot choose the order its pseudo-headers are written in

```
Unblocks:    T-244, TODO/cli-surface.md, the `Left:` list item that says the
             HTTP/2 half of a fingerprint is the one every survey gets wrong
Files:       vendor/h2/src/ext.rs
             vendor/h2/src/frame/headers.rs
             vendor/h2/src/client.rs
             vendor/h2/src/proto/streams/streams.rs
             patches/h2/0002-src-client.rs.patch
             patches/h2/0003-src-ext.rs.patch
             patches/h2/0004-src-frame-headers.rs.patch
             patches/h2/0007-src-proto-streams-streams.rs.patch
Upstream:    ours. RFC 9113 section 8.3 leaves the order among the
             pseudo-headers to the sender and this crate has always written one
             fixed sequence, so a release could expose the choice on
             `client::Builder` or on a request extension the way it exposes
             `Protocol`. Nothing says it will.
Added:       2026-08-29T09:20Z
```

`h2::ext::PseudoOrder` is a permutation of the six pseudo-header names, read
off the request's extensions the same way `ext::Protocol` is, and carried down
to the `Iter` that feeds the HPACK encoder. Absent, the order is the one this
crate has always written, so a request that does not carry one produces
byte-identical output.

`PseudoOrder::parse` takes the `":method,:authority,:scheme,:path,:protocol,:status"`
form and refuses a list that names something else, repeats a name or omits one.
Omitting one would silently drop a pseudo-header from the frame, which is a
malformed request rather than a different fingerprint.

**Why it has to be here.** The order is decided where the HEADERS frame is
encoded, which is this crate, and nothing above it can reach that. `hyper`
passes a request's extensions through untouched and `reqwest` did not, which
is the one-line patch beside this one.

**Why it is a field and not an environment variable.** The prior art this was
taken from, `apify/h2`, reads `IMPIT_H2_PSEUDOHEADERS_ORDER` on every header
block it encodes. `std::env::set_var` is unsafe in Rust 2024, is undefined
behaviour beside another thread reading the environment, and is process-global:
two clients with different profiles in one process overwrite each other. This
tree runs a multi-thread `tokio` runtime and has two profiles, `browser` and
`plain`, whose whole point is that they look different on the wire.

**One subtlety that cost a build.** `proto::streams::Streams::send_request`
clears the request's extensions before the frame is built, which is why it
already lifts `Protocol` out first. The order is lifted in the same place, and
reading it later in `convert_send_message` finds an empty map.

**How it was measured.** Off the wire, `loopback-tlsprobe` decoding the HPACK
block of a real HTTP/2 request:

| | pseudo-header order |
| --- | --- |
| before | `m,s,a,p` |
| after | `m,a,s,p` |
| Chrome | `m,a,s,p` |

```bash
pwsh -NoProfile -File scripts/check-fingerprint.ps1
```

---

## h2: a client cannot open a stream with the PRIORITY block a browser sends

```
Unblocks:    T-262, TODO/cli-surface.md
Files:       vendor/h2/src/ext.rs
             vendor/h2/src/frame/priority.rs
             vendor/h2/src/frame/headers.rs
             vendor/h2/src/client.rs
             vendor/h2/src/proto/streams/streams.rs
             patches/h2/0002-src-client.rs.patch
             patches/h2/0003-src-ext.rs.patch
             patches/h2/0004-src-frame-headers.rs.patch
             patches/h2/0005-src-frame-priority.rs.patch
             patches/h2/0007-src-proto-streams-streams.rs.patch
Upstream:    unlikely, and for a reason rather than by neglect. RFC 9113
             section 5.3.1 deprecates stream priority and says a sender SHOULD
             NOT send the PRIORITY frame, so a release adding a way to send one
             would be adding a way to do the thing the specification tells
             clients not to do. A release could still expose it as an
             impersonation feature the way it exposes `Protocol`, and nothing
             says it will.
Added:       2026-08-30T02:30Z
```

`h2::ext::StreamPriority` carries a dependency stream id, a wire weight and an
exclusive flag, read off the request's extensions the same way `PseudoOrder`
is. Absent, nothing is written and the frame is byte-identical to before.

`Headers::set_stream_priority` sets the payload and the PRIORITY flag in one
call, because the two are one decision: a head carrying the flag with no block
is a frame a peer cannot parse, and a block with no flag is five bytes of
header block. `StreamDependency::encode` is the other new half; `load` already
existed, because this crate parses a priority block on receive and wrote none
on send.

**Why the encoder needed nothing new.** `EncodingHeaderBlock::encode` already
takes a closure that runs after the head and before the header block, which is
how `PushPromise` writes its promised stream id, and the payload length is
measured after it runs. So the five bytes are counted in the frame length and
in any CONTINUATION split without either being computed here.

**Why it is the HEADERS block and not a PRIORITY frame.** A standalone PRIORITY
frame on the connection is what a browser also sends, and it is a different
seam: a connection level write rather than part of converting one request. The
block is what the four field Akamai fingerprint reads.

**How it was measured.** Off the wire, `loopback-tlsprobe` reading a real
HTTP/2 request, and against a real Chrome on the same probe:

| | PRIORITY field |
| --- | --- |
| before | `0` |
| after | `1:1:0:255` |
| Chrome 151 and Chrome 152 | `1:1:0:255` |

```bash
pwsh -NoProfile -File scripts/check-fingerprint.ps1
```

---

## impit: the PRIORITY block is not part of the fingerprint database

```
Unblocks:    T-262, TODO/cli-surface.md
Files:       vendor/impit/impit/src/impit.rs
             vendor/impit/impit/src/lib.rs
             patches/impit/0006-impit-src-impit.rs.patch
             patches/impit/0007-impit-src-lib.rs.patch
Upstream:    ours, and the same shape as the two HTTP/2 settings beside it.
Added:       2026-08-30T02:30Z
```

`ImpitBuilder::with_http2_stream_priority` takes the block and the client puts
it in each request's extension map beside the pseudo-header order.
`impit::h2_ext` reexports `h2::ext`, so a caller configures this without
depending on `h2` directly and cannot end up naming a different `h2` than this
graph resolves.

**Why it is on the builder and not on `Http2Fingerprint`.** It belongs to the
HTTP/2 fingerprint by rights, and putting it there would edit twenty-seven
profile literals in the vendored database that do not use it, every one of them
a conflict at the next reconciliation. `http2_header_table_size` and
`http2_omit_max_frame_size` are on the builder for the same reason and this
follows them.

**Where the value lives.** `crates/bit-cli-core/src/page.rs`,
`BROWSER_H2_STREAM_PRIORITY`, with the rest of the profile.
[RULES.md](../TODO/RULES.md) section 6b: the vendored database is a starting
point and not the home of the answer.

---

## h2: an HPACK story whose fixture is not vendored fails rather than skips

```
Unblocks:    nothing. It is the cost of not vendoring 59 MB of test vectors
Files:       vendor/h2/src/hpack/test/fixture.rs
             patches/h2/0006-src-hpack-test-fixture.rs.patch
Upstream:    ours, and it would make no sense upstream: their fixtures are
             always there.
Added:       2026-08-29T09:20Z
```

`fixtures/` is 59 MB of h2spec and HPACK test vectors that upstream's own
`Cargo.toml` already excludes from the published crate, and `vendor/upstream.json`
excludes it here. Without this change every one of the 382 generated story
tests fails on `File::open(...).unwrap()` and says nothing about why.

`test_fixture` returns early when the file is not there, after printing which
one it skipped.

**How it was measured.**

```bash
cargo test --manifest-path vendor/h2/Cargo.toml --target-dir target/vendor-h2
```

382 failures before, 438 passing after.

---

## h2: the fuzz test crate cannot be built on Windows

```
Unblocks:    nothing. It is what makes `cargo test` on the vendored workspace
             runnable at all on the machine this repository is developed on
Files:       vendor/h2/Cargo.toml
             patches/h2/0001-Cargo.toml.patch
Upstream:    ours. `honggfuzz` says plainly that it does not support Windows,
             so a release will not change this.
Added:       2026-08-29T09:20Z
```

`tests/h2-fuzz` is a workspace member and depends on `honggfuzz`, whose build
script exits with "honggfuzz-rs does not currently support Windows". The crate
moves from `members` to `exclude`, which is what upstream `rustls` already does
with its own fuzz crate. The directory stays in the tree.

**How it was measured.**

```bash
cargo test --manifest-path vendor/h2/Cargo.toml --workspace --target-dir target/vendor-h2
```

A build failure before, 479 tests after.

**One upstream flake found while measuring, and not ours.**
`proto::streams::recv::tests::clear_recv_buffer_caps_capacity_before_overflow`
fails about one run in one under `--workspace` and passes alone. It reproduces
on a pristine 0.4.19 checkout carrying only this change and the fixture skip
above, so it is upstream's and predates everything here. Nothing was done for
it.

---

## impit: the pseudo-header order is exported to the whole process

```
Unblocks:    T-244, TODO/cli-surface.md
Files:       vendor/impit/impit/src/impit.rs
             patches/impit/0006-impit-src-impit.rs.patch
Upstream:    ours. apify's own `h2` fork is what reads the variable, and this
             repository does not carry that fork, so there is nothing for a
             release to retire.
Added:       2026-08-29T09:20Z
```

`Impit::new` set `IMPIT_H2_PSEUDOHEADERS_ORDER` from the fingerprint. It now
parses the same list into an `h2::ext::PseudoOrder` once, keeps it on the
client, and puts it on each request's extensions. A list that cannot be parsed
is an error from `build()` rather than a panic inside the HTTP/2 encoder.

The order reaches the request through `reqwest`'s two public conversions,
`Request -> http::Request` and back, because `reqwest::Request::extensions_mut`
is crate-private. Both conversions copy the extension map whole, so the
per-request timeout set just above survives the round trip.

**Why it has to be here.** See the `h2` section above for why a process-global
is wrong for this tree specifically: two profiles, one process, a multi-thread
runtime.

**Two of this crate's defaults are load bearing and are not changed here**,
recorded so a reconciliation that moves them is noticed rather than discovered.
`vendor/impit/impit/src/impit.rs:133` defaults `vanilla_fallback` to false, so
a connect failure produces **no second request**; nothing in `bit-cli` calls
`with_fallback_to_vanilla`. And `cookie_store` defaults to `None`, so
`reqwest`'s cookie provider is never set and no jar exists to write to. Both
are what "one `GET`, no retry, nothing stored" rests on, and both are in the
abuse read for `TODO/cli-surface.md` T-244.

---

## impit: HTTP/3 costs an unstable `rustc` flag in six places

```
Unblocks:    T-244, and the T-146 shape its Blocked block warns about
Files:       vendor/impit/impit/Cargo.toml
             vendor/impit/impit/src/impit.rs
             vendor/impit/impit/src/lib.rs
             vendor/impit/impit/src/tls/mod.rs
             impit/src/http3.rs, deleted with the feature, tree-relative
             patches/impit/0002-impit-Cargo.toml.patch
             patches/impit/0005-impit-src-http3.rs.patch
             patches/impit/0006-impit-src-impit.rs.patch
             patches/impit/0007-impit-src-lib.rs.patch
             patches/impit/0009-impit-src-tls-mod.rs.patch
Upstream:    ours. `reqwest` gates `http3` behind `--cfg reqwest_unstable` on
             purpose, and a release of either could make it stable, at which
             point this patch is worth reconsidering rather than deleting.
Added:       2026-08-29T09:20Z
```

`reqwest`'s `http3` feature refuses to compile without `--cfg reqwest_unstable`.
`.cargo/config.toml` sets rustflags **per target** and a `[build]` entry does
not merge with a `[target.*]` one, so the flag would have to be added to all
three target blocks **and** to the three `RUSTFLAGS` blocks in
`.github/workflows/ci.yml`, where a workflow's `RUSTFLAGS` replaces the config
rather than adding to it. That is exactly the shape of
[T-146](../TODO/cli-surface.md), which cost a Windows binary built against the
dynamic C runtime.

So the feature goes, and with it `http3.rs`, the `H3Engine` that discovers
HTTP/3 support through HTTPS DNS records, `with_http3`, and the
`hickory-proto` and `hickory-resolver` dependencies it needed.
`RequestOptions::http3_prior_knowledge` still exists and now always answers
`Http3Disabled`, because a caller who set it meant it and a silent fall back to
HTTP/2 would be worse.

**What it is worth, measured.** `--cfg reqwest_unstable` in six places, gone,
and four packages with it: `h3`, `h3-quinn`, `futures` and `futures-executor`.

**What it costs.** `bit-cli` cannot fetch a source document over HTTP/3.
Nothing measured says an origin serving a torrent page requires it, every
HTTP/3 claim in the survey this work came from is unverified, and a client
whose HTTP/3 fingerprint nothing in this tree can read is a claim rather than a
capability.

---

## impit: a proxy tunnel error message needs a fork of hyper-util

```
Unblocks:    T-244, by removing an upstream from the list it needed
Files:       vendor/impit/impit/src/errors.rs
             patches/impit/0003-impit-src-errors.rs.patch
Upstream:    ours. `hyper-util` took the wider `TunnelError` in its own time;
             when a release carries `TunnelError::TunnelUnsuccessful(Option<u16>)`
             this can go back.
Added:       2026-08-29T09:20Z
```

`ImpitError::from` downcast a connect failure to
`hyper_util::client::legacy::connect::proxy::TunnelError` to name a proxy
CONNECT refusal and its status code. That type is only public in apify's fork
of `hyper-util`, and the whole of that fork is one status code on one error
message. The downcast goes; a connect failure is `ConnectError` with the
transport's own text.

**What it is worth.** One upstream tree not vendored. `bit-cli` configures no
proxy on the source-document path, so nothing here could ever have read that
message.

---

## impit: a charset detector brings an unmaintained crate and a whole HTML rewriter

```
Unblocks:    T-244, and the `deny` gate, which fails on the advisory
Files:       vendor/impit/impit/Cargo.toml
             vendor/impit/impit/src/lib.rs
             impit/src/response_parsing/mod.rs, deleted, tree-relative
             patches/impit/0002-impit-Cargo.toml.patch
             patches/impit/0007-impit-src-lib.rs.patch
             patches/impit/0008-impit-src-response_parsing-mod.rs.patch
Upstream:    ours, and RUSTSEC-2021-0153 is the defect: `encoding` has had no
             release since 2016 and the advisory names `encoding_rs` as the
             replacement. A release that ports it retires this.
Added:       2026-08-29T09:20Z
```

`impit::utils` exported a WHATWG charset detector: BOM sniffing, then a
prescan of the first 1024 bytes for `<meta charset>` using `lol_html`. Nothing
in `impit`'s own request path calls it; it exists for the Node and Python
bindings, which this repository does not vendor. It is removed with its three
dependencies.

**What it is worth, measured.** `cargo deny check` fails on RUSTSEC-2021-0153
with it and passes without. The whole graph goes from 470 packages to 446: the
44 `lol_html` brings, plus `encoding` and `mime`.

**Why not port it to `encoding_rs`.** Because the tier that parses a page is
`crates/bit-cli-core/src/page.rs`, which has its own tag scanner and 33 tests,
and T-244 already measured `lol_html` at 44 packages against a hand-written
scanner at 0. A charset detector belongs beside the parser that needs it, not
in the HTTP client.

---

## impit: a certificate a caller minted itself cannot be trusted

```
Unblocks:    T-244, the half of its acceptance that needs a completed handshake
Files:       vendor/impit/impit/src/tls/mod.rs
             vendor/impit/impit/src/impit.rs
             patches/impit/0006-impit-src-impit.rs.patch
             patches/impit/0009-impit-src-tls-mod.rs.patch
Upstream:    ours. `rustls-platform-verifier` already takes extra roots and
             `impit` passes it the `webpki` set; surfacing the caller's is a
             small thing a release could do.
Added:       2026-08-29T09:20Z
```

`TlsConfigBuilder` and `ImpitBuilder` take `with_extra_roots`, a list of DER
certificate authorities handed to `Verifier::new_with_extra_roots` **in
addition to** the bundled `webpki` roots. Nothing is replaced and nothing is
disabled: a certificate still has to verify against some root.

The provider-and-verifier cache is keyed on the fingerprint alone, so a client
carrying extra roots neither reads from it nor writes to it.

**Why it has to be here.** The HTTP/2 half of a fingerprint only exists after a
handshake completes, and `scripts/check-fingerprint.ps1` drives `bit-cli` at a
loopback probe whose certificate it mints per run. The alternative is a flag
that stops verifying certificates, which is not something to put in a shipping
binary for a test, and it would also be measuring the wrong client: one told to
skip verification can offer a different `signature_algorithms` list.

**How it is reached.** `BIT_CLI_EXTRA_CA_FILE`, read by
`bit_cli_core::fetch`. An environment variable rather than a flag for the same
reason `SSL_CERT_FILE` is one.

---

## reqwest: a request's extensions never reach hyper

```
Unblocks:    T-244. Without it the `h2` patch above is set on a request that
             never carries it
Files:       vendor/reqwest/src/async_impl/client.rs
             patches/reqwest/0001-src-async_impl-client.rs.patch
Upstream:    ours. `hyper` documents request extensions as a way to reach the
             connection, and dropping them is arguably a defect rather than a
             design, so a release could take the same line.
Added:       2026-08-29T09:20Z
```

`Client::execute_request` takes the request apart, uses the extension map for
its own per-request settings, builds a `hyper::Request` from the method, URI,
version and headers, and drops the extensions. Anything a caller put in one is
therefore invisible to `hyper` and to `h2`. One line copies them across.

**How it was measured.** With the line, the pseudo-header order set by `impit`
reaches the wire; without it, `h2` sees `None` and writes its own order. Both
were captured off the wire before the line was written.

---

## reqwest: two HTTP/2 settings hyper takes and reqwest does not offer

```
Unblocks:    T-244, the SETTINGS field of the Akamai HTTP/2 fingerprint
Files:       vendor/reqwest/src/async_impl/client.rs
             patches/reqwest/0001-src-async_impl-client.rs.patch
Upstream:    ours for the second; the first is a plain gap that a release could
             close, since `hyper-util` has taken the method already.
Added:       2026-08-29T09:20Z
```

`ClientBuilder::http2_header_table_size` forwards to the `hyper-util` builder
method of the same name, which upstream took in hyperium/hyper-util#274.
`ClientBuilder::http2_omit_max_frame_size` asks for `SETTINGS_MAX_FRAME_SIZE`
to be left out of the SETTINGS frame: `hyper` announces the protocol's own
default, 16384, on every connection, and a peer that receives no such setting
uses that same default, so the connection carries exactly what it carried
before.

**Why a fingerprint cares about a setting that changes nothing.** The first
field of an Akamai HTTP/2 fingerprint is the SETTINGS list, in order, and a
client that announces one setting more than the browser it claims to be is
distinguishable on that alone.

**How it was measured**, off the wire, at each step:

| | Akamai fingerprint |
| --- | --- |
| before | `2:0;4:6291456;5:16384;6:262144\|15663105\|0\|m,s,a,p` |
| pseudo-header order patched | `2:0;4:6291456;5:16384;6:262144\|15663105\|0\|m,a,s,p` |
| max frame size omitted | `2:0;4:6291456;6:262144\|15663105\|0\|m,a,s,p` |
| header table size announced | `1:65536;2:0;4:6291456;6:262144\|15663105\|0\|m,a,s,p` |
| Chrome, for comparison | `1:65536;2:0;4:6291456;6:262144\|15663105\|0\|m,a,s,p` |

---

## rustls: a hello carries no GREASE at the ends and its extensions never move

```
Unblocks:    T-263, TODO/cli-surface.md
Files:       vendor/rustls/rustls/src/msgs/enums.rs
             vendor/rustls/rustls/src/msgs/handshake.rs
             vendor/rustls/rustls/src/client/hs.rs
             vendor/rustls/rustls/src/crypto/emulation/mod.rs
             patches/rustls/0003-rustls-src-client-hs.rs.patch
             patches/rustls/0004-rustls-src-crypto-emulation-mod.rs.patch
             patches/rustls/0005-rustls-src-msgs-enums.rs.patch
             patches/rustls/0006-rustls-src-msgs-handshake.rs.patch
Upstream:    ours, and only meaningful behind the `impit` feature. Upstream
             rustls already shuffles its own extension order per connection,
             which is half of this; what it has no reason to add is a second
             and third GREASE slot at codepoints chosen per connection, because
             that is an impersonation feature rather than a TLS one. The fork
             carries the emulation module it belongs to.
Added:       2026-08-30T03:00Z
```

Two changes, and the first is smaller than it looks.

**The shuffle was already built.** `ClientExtensions::order_insensitive_extensions_in_random_order`
sorts by a hash of a per-connection `order_seed`, and
`used_extensions_in_encoding_order` emits those first and the
`contiguous_extensions` list after them. A fingerprint naming **every**
extension in `extension_order` leaves the random set empty, so nothing moves.
Naming none of them, which is what this repository's profile now does, gives
the shuffle for free. Nothing in rustls changed for that half.

**GREASE at the two ends needed a new shape.** `ClientExtensions` has one typed
field per extension type and one GREASE slot, `reserved_grease` at the fixed
codepoint `0xbaba`. A browser sends **two**, at two different codepoints it
picks per connection, one first and one last, which that shape cannot express.
So there are two more slots, `ReservedGreaseFirst` and `ReservedGreaseLast`,
whose enum codepoints are placeholders, and a `grease_codepoints` field
carrying the pair actually written. `ClientExtensions::encode` writes those two
directly rather than through `encode_one`, because `encode_one` takes the
codepoint from the type.

`used_extensions_in_encoding_order` places them, and the last one goes **before**
any PSK offer, because RFC 8446 requires `pre_shared_key` to be the final
extension and that is not negotiable. Both are excluded from the shuffle, since
an extension placed at an end must not also be dealt into the middle.

**The bodies are measured, not chosen.** Read off a real Chrome's hello: the
first GREASE extension has an empty body and the last has a single zero byte.
That is why they are two fields rather than one repeated.

**The codepoints are drawn from the provider's secure random**, mapped onto the
sixteen RFC 8701 reserves by `grease_codepoint`, and drawn **distinct**: a
client sending the same value at both ends is advertising a constant, which is
the opposite of the point.

**All three GREASE fields carry `Option<Payload>` and that is a fix rather than
a choice.** Because these variants sit on real GREASE codepoints, they are also
what a **received** hello's GREASE extension decodes into, and RFC 8701 lets a
client put any body in one. Typed `Option<()>`, they rejected a GREASE
extension carrying a byte, which is what a browser sends at the back of its
list and what this client sends there: `TrailingData("Empty")` on about one
handshake in five. The pre-existing `reserved_grease` at `0xbaba` had the same
defect, so a server built from this fork rejected some real browser hellos.

**How it was measured.** Two consecutive captures of the same binary, raw
hello, `loopback-tlsprobe --raw --hello-out`:

| | first extension | last extension | order |
| --- | --- | --- | --- |
| capture 1 | `0x6a6a` | `0x7a7a` | one permutation |
| capture 2 | `0x7a7a` | `0x0a0a` | a different one |
| before | none | none | fixed, every time |

The JA4 does not move, which is the point of asserting it: JA4 sorts and strips
GREASE, so a golden written before this passes after it unchanged.

```bash
pwsh -NoProfile -File scripts/check-fingerprint.ps1
```

---

## impit: a fingerprint cannot ask for GREASE at both ends

```
Unblocks:    T-263, TODO/cli-surface.md
Files:       vendor/impit/impit/src/fingerprint/mod.rs
             patches/impit/0004-impit-src-fingerprint-mod.rs.patch
Upstream:    ours, and the same shape as `with_new_alps_codepoint` beside it.
Added:       2026-08-30T03:00Z
```

`TlsExtensions::with_grease_both_ends` sets a flag that reaches rustls's
`TlsExtensionsConfig`. It is a builder method rather than a fourteenth
positional argument to `TlsExtensions::new`, which is how the two switches
beside it are done and which is what keeps the vendored profile database from
changing at all.

`ExtensionType::Grease` in an `extension_order` list is the older, single,
fixed-codepoint form and stays what it was. This is a different thing and the
two are independent.

---

## rustls: the workspace lists eleven members this repository does not vendor

```
Unblocks:    nothing. It is what makes the vendored tree loadable at all
Files:       vendor/rustls/Cargo.toml
             patches/rustls/0002-Cargo.toml.patch
Upstream:    ours, and it would make no sense upstream: their members are all
             there.
Added:       2026-08-29T09:20Z
```

`vendor/upstream.json` vendors the `rustls` crate and `rustls-test` and leaves
out the other eleven workspace members, which are a documentation site, a
provider test suite, benchmark harnesses that need valgrind and OpenSSL, and
example binaries. Cargo loads the workspace root when it loads the crate, so
`members` and `default-members` have to name what is there.

**Why `rustls-test` stays** although nothing builds it: `rustls`'s own
`[dev-dependencies]` names it by path, and dropping it would mean patching a
second manifest to run no tests.

**Nothing else in `apify/rustls` is patched.** The fingerprint emulation is
theirs and is used as it is.

---

## hyper-util: nothing

```
Unblocks:    T-244, by carrying one method a release has not shipped yet
Files:       none
Upstream:    n/a. The tree is upstream's, unchanged, at a commit past 0.1.20.
Added:       2026-08-29T09:20Z
```

`vendor/hyper-util` is vendored and **not patched**. It is here for one method
that upstream took after 0.1.20 shipped, `Builder::http2_header_table_size`,
without which a client cannot announce `SETTINGS_HEADER_TABLE_SIZE` and its
Akamai fingerprint differs from any browser's. `vendor/upstream.json` records
the commit and the reason; the next release that carries the method makes the
pin an ordinary one rather than a reason.

It is **not** apify's fork of the same repository. That fork's own change is a
status code on a proxy tunnel error, which nothing here reads, and taking it
would have meant carrying somebody else's patch to get upstream's.
