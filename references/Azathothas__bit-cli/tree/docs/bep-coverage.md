# BEP coverage

What `bit-cli` implements, what it does not, and the argument for each. The
entries behind this are in
[`TODO/bep-coverage.md`](../TODO/bep-coverage.md).

**Yes** means `bit-cli`'s own code implements it and a test covers it; the
symbol column names where. **Inherited** means `librqbit` provides it,
`bit-cli` reaches it through the session, and `bit-cli` has no test of its own.
**Partial** and **read only** each name what is missing in the row itself.
**No** means it is not there, and the entry that closes it is named.

| BEP | What | Status | Where |
| --- | --- | --- | --- |
| 3 | The BitTorrent protocol | inherited | the session; `tracker.rs:9` for the announce half |
| 5 | DHT | inherited | `--no-dht` reaches `enable_dht`, `swarm.rs:160` |
| 7 | IPv6 tracker extension | yes | `peers6` read at `tracker.rs`; `trackers --family` announces once per family |
| 9 | Metadata from peers | inherited | magnets resolve through the session |
| 10 | Extension protocol | yes | `webseed/bridge.rs:84` `MSGID_EXTENDED`, `:888` `extended_handshake`, `:102` `OUR_EXTENSIONS` |
| 11 | PEX | inherited | no `bit-cli` code; `--no-pex` warns that it cannot turn it off, [T-181](../TODO/cli-surface.md) |
| 12 | Multitracker metadata | yes | `tracker.rs:115` tiers; `create`, `edit`, `trackers` |
| 14 | Local service discovery | inherited | `--no-lsd` reaches `enable_lsd`, `swarm.rs:161` |
| 15 | UDP tracker protocol | yes | `tracker.rs:25`, `:301`, `:643`. The retry ladder diverges on purpose: three attempts inside `--tracker-timeout` rather than `15 * 2^n`. [`trackers.md`](trackers.md) has the argument |
| 17 | HTTP seeding, Hoffman style | yes | `webseed/fetch.rs`; the style is keyed by the metainfo list a URL came from, and probed for a `--web-seed` given on the command line |
| 19 | HTTP seeding, GetRight style | yes | `webseed/composition.rs`, the headline feature |
| 20 | Peer id conventions | yes | `webseed/bridge.rs` handshake |
| 21 | Extension for partial seeds | yes | `webseed/bridge.rs:897` `upload_only` |
| 23 | Compact peer lists | yes | `tracker.rs:552` |
| 27 | Private torrents | yes | `torrent/metainfo.rs`, `create`, `edit` |
| 39 | Updating torrents via feed URL | yes | `create`, `edit` |
| 47 | Padding files | read only | parsed and skipped: `torrent/metainfo.rs:116`, `storage.rs:728`; `create` does not emit them ([T-081](../TODO/create-seed.md)) |
| 48 | Tracker scrape | yes | `tracker.rs:427`, `:499`; BEP 48 URL convention only ([T-065](../TODO/trackers.md)) |
| 53 | Magnet file selection, `so=` | yes | `torrent/magnet.rs:211` |
| 6 | Fast extension | partial | the allowed-fast derivation is `fast_set.rs`, with BEP 6's mask and aria2's; `bench swarm` reads all five messages and reports which mask a target used. Nothing sends one: [T-100](../TODO/bep-coverage.md), and the vendored `librqbit` has no BEP 6 either |
| 16 | Superseeding | no | [T-082](../TODO/create-seed.md). `--superseed` is accepted and warns |
| 29 | uTP | partial | `--transport tcp|utp|both`, default `tcp`, on every command that starts a session. A transfer completes over uTP with `--encryption off` and stalls with encryption on, which is T-233 in [`../TODO/peers.md`](../TODO/peers.md). What is unmeasured is the induced latency uTP exists for, and loopback cannot show it: [T-101](../TODO/bep-coverage.md) |
| 52 | BitTorrent v2 | no | [T-081](../TODO/create-seed.md), [T-134](../TODO/multi-source.md) |
| 54 | `lt_donthave` | partial | received and honoured: the vendored `librqbit` clears the bit, `extended/mod.rs` `LtDontHave`. Nothing sends one: [T-167](../TODO/bep-coverage.md) |
| 55 | Holepunch | no | [T-102](../TODO/bep-coverage.md) |
| 33 | DHT scrape | no | [T-169](../TODO/dht.md) |
| 44 | DHT mutable items | no | [T-170](../TODO/dht.md) |
| 51 | DHT infohash indexing | no | [T-169](../TODO/dht.md) |
| MSE/PE | Peer encryption | no | [T-163](../TODO/peers.md) |

`TODO/bep-coverage.md` tracks the gaps.

**uTP is not reachable.** `librqbit-utp` appears in `cargo tree` because
`librqbit` depends on it, not because `bit-cli` uses it: `ListenerOptions::mode`
is never set, so the session stays `TcpOnly`, and no flag changes that. Earlier
revisions of this table said "available, off by default", which read as a
capability a user could turn on. There is nothing to turn on. [T-101](../TODO/bep-coverage.md)
carries the work.

## What a gap costs

The distinction worth making is between a gap that costs reach and a gap that
costs completeness.

**Reach**: a peer configured to require encryption will not exchange traffic
with a plaintext-only client at all, which is why message stream encryption
went first and is done.

**Completeness**: uTP, BEP 55 holepunch, WebTorrent, BEP 33, BEP 44 and BEP 16
each add something and none of them stops `bit-cli` talking to a peer it can
otherwise reach.

That distinction is what orders the remaining work rather than the BEP number.
