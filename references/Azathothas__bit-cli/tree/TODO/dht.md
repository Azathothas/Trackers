# DHT

Twenty-two issues touch bootstrap, routing table health, announce, and IPv6.

**The 2026-08-21 corpus adds three things to this file.** Two new entries
below, [T-169](#t-169-bep-33-dht-scrape-and-bep-51-infohash-indexing-are-not-implemented)
and [T-170](#t-170-bep-44-mutable-items-are-not-implemented), and one design
answer for [T-050](#t-050-the-dht-cache-costs-disk-io-even-when-nothing-is-running)
and [T-052](#t-052-dht-is-not-reported) that is worth stating before either.

**A short-lived CLI should almost certainly never become a DHT server.**
`n0-mainline`'s documented default is to start as a client and switch to
server mode only after **fifteen minutes** with a publicly reachable address,
so that only stable reachable nodes carry routing load. `bit-cli` is a
foreground one-shot: a `download` that runs for ninety seconds has no business
in anyone's routing table, and taking queries it will not be around to answer
is a cost imposed on the network for no gain here. That is the argument
T-050 needs, and it is stronger than "check what the default persists": the
answer is client mode, no persistence, and a documented path if persistence is
ever wanted. `n0-mainline/src/common/closest_nodes.rs:127` `dht_size_estimate`
and its `n0-mainline/docs/dht_size_estimate.md` are what a `dht` report object could carry
beyond a routing-table count, for T-052.

`fx-torrent/src/dht/` is the widest BEP 5 surface in the corpus to read
against: `krpc.rs` handles `ping`, `find_node`, `get_peers`, `announce_peer`,
`sample_infohashes` (`:18`), `put` (`:19`) and `get` (`:20`) in one message
enum. Two of its closed issues are one-line interop traps worth knowing before
touching any of this. [Issue 16](https://github.com/yoep/fx-torrent/issues/16):
the KRPC transaction id was fixed at two bytes, because BEP 5 says two is
*typically* enough; real nodes send four, and the result is
`Invalid Length: 4 (expected: a byte array of length 2)` on every reply from
those nodes. [Issue 21](https://github.com/yoep/fx-torrent/issues/21): a KRPC
error response is a **list** `[code, message]` and not a dict, and reading it
as a dict turns every error into a parse failure, which hides the error that
was being reported.

---

### T-050 The DHT cache costs disk I/O even when nothing is running

Source:      https://github.com/ikatson/rqbit/issues/310 (open)
Category:    dht
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T10:55Z

Problem:     A reporter running the daemon with no active torrents saw it as
             the busiest writer on the machine, from periodically saving the
             DHT routing table.
Relevance:   `bit-cli` is a foreground one-shot, so it does not sit idle
             writing a cache. It uses `DhtSessionConfig::default()`, which
             enables persistence, so a short run may still write one.
Approach:    Check what `DhtSessionConfig::default()` persists and where. If it
             writes outside the download directory, that is state a one-shot
             tool leaves behind, which decision 7.4 rules out. Either turn
             persistence off or document the path.
Acceptance:  `bit-cli download <MAGNET>` writes nothing outside `--dir` and the
             system temp directory, verified by watching the process with
             Process Monitor for one run and recording the write list here.

**Done, and it was worse than the entry supposed.** The Relevance says a short
run "may still write one". It did, and the file it wrote is not this program's.

`DhtSessionConfig::default()` sets `persistence: Some(..)`, and
`dht/src/persistence.rs:98` builds the path from
`get_configuration_directory("dht")`, which is `com.rqbit.dht`. So the file is
`%LOCALAPPDATA%/rqbit/dht/cache/dht.json`: the routing table of whatever
`rqbit` install is on the machine. There is one on this machine, and
[RULES.md](RULES.md) section 5 records it as installed for interop. **This
program was overwriting another program's state.**

**Measured rather than reasoned about.** One 90 second run against an info hash
that resolves to nothing:

```
before   2026-08-23 00:38:17  95,248 bytes
after    2026-08-23 15:48:11  81,752 bytes
```

The timestamp moved and the file shrank, which is a rewrite rather than a
touch. With `persistence: None` the same run leaves it at
`2026-08-23 15:48:11  81,752`, byte for byte and second for second.

**The line above it said this could not happen.** `SessionOptions` carries
`persistence: None` with the comment "No persistence, ever. A stored session is
Phase C, and writing one from a foreground command would leave state behind".
That was true of the session and false of the DHT, one field away, in the same
struct literal. `engine.rs:444` takes `dht_config()` now, and
`the_dht_keeps_no_cache_on_disk` asserts both halves: that this program does
not persist, and that the default still does, so the test says something about
a choice rather than about a tautology.

**What the acceptance asked for and what was done instead.** It asks for a
Process Monitor write list. Process Monitor is a GUI, it cannot run unattended,
and its output is not a thing a later session can re-derive. Two file
timestamps and two sizes answer the same question for the one path that was
found, are reproducible with `stat`, and are in this entry. **A full write list
for one run is still not measured**, so a second path outside `--dir` would not
have been found by this. That residual is real and is smaller than what it
replaced.

**Upstream's issue 310 is about the daemon writing every 60 seconds while
idle.** `bit-cli` is a foreground one-shot, so the frequency was never the
problem here. The path was.

### T-051 A magnet with no DHT and no trackers fails without saying so

Source:      design gap
Category:    dht
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T11:05Z

Problem:     `--web-seed-only` turns off DHT, LSD, and trackers. A magnet
             source then has no way to resolve its metadata, so the run waits
             on `wait_until_initialized` until the deadline.
Relevance:   The combination is a reasonable thing to ask for and it cannot
             work. It should fail immediately with a clear reason.
Approach:    Refuse at argument-validation time: a magnet or bare info hash
             with `--web-seed-only` and no `.torrent` is a usage error, because
             web seeds carry payload and not metadata.
Acceptance:  `bit-cli download <MAGNET> --web-seed-only --web-seed <URL>` exits
             2 immediately, naming the conflict, rather than timing out.

**Done, and the Problem's account of the failure is wrong.** It says the run
"waits on `wait_until_initialized` until the deadline". It does not. Measured
by taking the new check out and running the acceptance case, it failed in
**0.01 seconds**, with exit **6**, `no_usable_sources`, and this:

```
error   magnet:?xt=urn:btih:0123...: no known way to resolve peers
        (no DHT, no trackers, no initial_peers)
```

`librqbit` refuses the add itself. So the entry's premise, that this is a
timeout nobody explains, was never true, and the work it implies, a deadline to
short-circuit, was not the work.

**What was actually wrong is the two things a caller reads.** The exit code
said `no_usable_sources`, which is the code for a source that might be there
next time, so a script retries an arrangement that cannot work on any attempt.
And the message names `initial_peers`, a `librqbit` field that appears in no
`bit-cli` flag, no manual and no document, so the one sentence a reader gets
points at a thing they cannot set.

It is exit **2** now, before the session is built, and it says what to do:

```
magnet:?xt=urn:btih:0123...: carries no metadata and every way of fetching it
is off. A web seed serves payload, not the torrent file: name a .torrent, or
leave one of the DHT, the trackers or local discovery on
```

**The Approach was too narrow by one flag and too wide by another.**

It names `--web-seed-only`. `--no-dht --no-lsd --no-tracker` is the same
arrangement spelled out and has the same answer, so the check is on the
condition rather than on the flag.

And it would have refused a run that works. `--peer` is dialled whether or not
discovery ever answers, and BEP 9 carries metadata from a peer, so a magnet
with every discovery mechanism off and one named peer is exactly how a private
swarm is reached. `a_magnet_with_no_discovery_but_a_named_peer_is_not_refused`
is the test that says the check is not that wide, and it was written because
the first version of the check was.

```
$ cargo test -p bit-cli --lib a_magnet_with
test result: ok. 3 passed; 0 failed; 0 ignored; 420 filtered out
```

### T-052 DHT is not reported

Source:      the operator's brief
Category:    dht
Priority:    P3
Effort:      M
Status:      open

Problem:     `--trace dht` is accepted and enables the tracing target, but
             nothing in the JSON reports says whether the DHT found anything:
             no bootstrap status, no routing table size, no announce result.
Relevance:   On a torrent with dead trackers the DHT is the only discovery
             path, and "did it work" currently has to be inferred from the peer
             count.
Approach:    `librqbit` exposes DHT stats through its API. Surface bootstrap
             state, routing table size, and peers found through the DHT as a
             `dht` object in the download and peers reports.
Acceptance:  `bit-cli peers <MAGNET> --json` carries `"dht": {"bootstrapped":
             true, "routing_table_size": N, "peers_found": M}`.

### T-169 BEP 33 DHT scrape and BEP 51 infohash indexing are not implemented

Source:      `reference/RESEARCH.md` section D, 2026-08-21
Category:    dht
Priority:    P3
Effort:      M
Status:      open

Problem:     `bit-cli trackers --scrape` scrapes a tracker. There is no way to
             ask the DHT the same question, and no way to participate in
             BEP 51 infohash sampling.
Relevance:   BEP 33 is the one that earns its place. It answers "how many
             seeders and leechers" from the DHT rather than from a tracker,
             which is exactly the case a torrent with dead trackers leaves
             `bit-cli` unable to answer at all, which is the same case
             [T-052](#t-052-dht-is-not-reported) exists for. BEP 51 is
             discovery infrastructure rather than a capability a download
             needs, and its main relevance here is that participation should
             be **opt-out**, which fx-torrent
             [Issue 30](https://github.com/yoep/fx-torrent/issues/30) concluded
             independently. A one-shot CLI that indexes info hashes for
             strangers by default is the same overreach as becoming a server
             by default.
Approach:    BEP 33 answers a `get_peers` with two bloom filters, one for
             seeders and one for leechers, and the arithmetic is the whole
             trick. `fx-torrent/src/bloom_filter.rs` is 229 lines with the
             implementation in the first 130 and eight tests after: `:5`
             `has_bits` and `:20` `set_bits` take the
             **first 4 bytes of the key as two little-endian `u16`
             indices**; `:46` `len()` estimates
             the population as `-(m/k) * ln(zero/m)` with `k = 2`; `:93`
             `count_zero_bits` uses a 16-entry nibble table. The DHT side is
             `fx-torrent/src/dht/tracker.rs:449` `scrape_peers` and `:2469`
             `scrape_info_hashes`.

             For BEP 51, `fx-torrent/src/dht/krpc.rs:18` carries
             `sample_infohashes` in the message enum, and
             `fx-torrent/src/dht/tracker.rs:1736` logs "detected spoofed
             sample_infohashes", which is the reminder that a sampling response
             is untrusted input like any other.
Blocker:     `bit-cli` does not own its DHT. `librqbit` supplies it and
             `librqbit-dht` exposes no hook for a custom KRPC method or a
             custom `get_peers` response. What would unblock BEP 33 is either
             an upstream change or a second DHT client used only for scrape,
             which is a real option because the query is stateless and needs
             no routing table of its own beyond bootstrap.
Acceptance:  `bit-cli trackers <TORRENT> --scrape --dht` reports seeder and
             leecher estimates from the DHT beside the tracker's own numbers,
             and the two are printed separately rather than merged, because
             they are measuring different populations.

### T-170 BEP 44 mutable items are not implemented

Source:      `reference/RESEARCH.md` section D, 2026-08-21
Category:    dht
Priority:    P3
Effort:      L
Status:      open

Problem:     No DHT put or get of arbitrary items, mutable or immutable.
Relevance:   This is the half that pairs with something `bit-cli` already has.
             `create` and `edit` write BEP 39 `update-url`, an HTTP URL a
             client can poll for a newer version of a torrent. BEP 44 mutable
             items are the same idea without the HTTP server: a public key
             addresses a slot in the DHT, the holder of the private key signs
             updates into it, and a reader who knows the key gets the current
             version. That is what BEP 46 mutable torrents are built on. For a
             tool whose whole subject is attaching sources to a torrent that
             already exists, a torrent that can announce its own successor
             without a web server is a natural fit, and `bit-cli` is already
             half of it.
Approach:    `n0-mainline/src/common/mutable.rs` is the reference and it is
             small. `:32` `MutableItem::new(signer, value, seq, salt)`, `:46`
             `target_from_key` = **SHA-1 of `public_key || salt`**, and `:145`
             `encode_signable(seq, value, salt)`, which is the exact byte
             sequence that gets ed25519-signed and therefore the only part
             where a mistake is silent rather than loud.
             `src/common/immutable.rs` is the immutable half and
             `src/core/put_query.rs` is the put path.
             `n0-mainline/beps/` carries the normative reStructuredText of
             **BEP 5, 42, 43 and 44**, which is worth more than any
             implementation when the question is what the specification
             actually requires.
             [PR 9](https://github.com/n0-computer/n0-mainline/pull/9)
             (MERGED) ports "mainline 6.4.1 mutable put security fixes" and is
             required reading before implementing `put`.
Blocker:     Same as [T-169](#t-169-bep-33-dht-scrape-and-bep-51-infohash-indexing-are-not-implemented):
             `librqbit`'s DHT exposes no put or get. Unlike T-169 this one is
             genuinely separable, because a BEP 44 client needs no
             relationship to any torrent, so a small standalone DHT client is
             a legitimate route rather than a workaround.
Acceptance:  `bit-cli` reads a mutable item by public key and salt, verifies
             its signature, and resolves it to a torrent; and writes one,
             re-read from a second process. Both directions, with the sequence
             number visible in `--json`, because a reader has to be able to
             tell a stale read from a current one.

---

### T-240 A DHT node that answers slowly or emptily is queried again at the same rank

Source:      `RESEARCH.md` entry 37, `gaia/docs/future_plan_peer_quality.md`,
             2026-08-24
Category:    dht
Priority:    P3
Effort:      M
Status:      open

Problem:     `bit-cli` selects DHT nodes for an iterative lookup by XOR
             distance. Distance says which nodes are worth asking about a
             target; it says nothing about which nodes answer. A routing table
             entry that has timed out four times in a row is asked again at the
             same rank as one that answered 40 ms ago.

             `bit-cli`'s DHT is `librqbit_dht` in `vendor/`, so this is a
             vendored change and [T-052](dht.md) is what would report it.

Premise:     The model is somebody else's and it is a **plan rather than an
             implementation**, which the corpus entry says plainly: grepping
             `gaia` for its own vocabulary finds the words in two documents and
             in no source file. So this is a design to evaluate rather than
             code to port, and its own numbers are absent.

             What it proposes, per node: `last_response_time`, `rtt_ema`,
             `query_count`, `fail_count`, and `last_useful_response`. That last
             field is the one worth having and the one a naive version misses:
             it separates a node that **answered** from a node that answered
             with something, and an empty `get_peers` response is a successful
             query that was worth nothing.

             Selection combines distance with reputation rather than replacing
             one with the other. The fields ride in the routing table snapshot
             that is already persisted, so there is no new store.

             Its two other mechanisms are already this repository's:
             a short-term negative peer cache is [T-164](peers.md), partial and
             blocked on `Session::blocklist` being immutable, and the
             per-source cooldown for HTTP sources is
             [T-137](multi-source.md), done. That the document arrives at both
             independently is corroboration for them rather than new work.

             Its warning against a **global cross-torrent positive peer cache**
             is worth recording so nobody builds one: a peer that had torrent A
             need not have torrent B, addresses and NAT mappings churn, and the
             peers that are stable across torrents are seedboxes, so relying on
             them concentrates load on a few addresses and invites being
             blocked by them.

Approach:    Measure before building, which is [RULES.md](RULES.md) section 5's
             line and the reason this entry is P3 rather than P2. The claim is
             that reputation-ranked selection reduces wasted queries. Nothing
             here has measured how many queries are currently wasted.

             So the first half is instrumentation, not selection: count
             queries, timeouts, and responses that carried no peer and no node,
             per lookup, and report them under `bit-cli` DHT output. If the
             wasted fraction is small the rest of the entry is not worth its
             vendored change.

Prove:       A new check beside the existing ones, `check-dht-quality.ps1`,
             named without its directory because it does not exist yet.

             Against a loopback DHT fixture holding a mix of nodes that answer
             fast, answer slow, answer empty, and never answer, the check must
             show the wasted query fraction before and after, from the same
             fixture and the same target, with the ranking as the only
             difference. A comparative claim needs a committed benchmark, which
             is why the fixture is part of the entry rather than a detail of
             it.
