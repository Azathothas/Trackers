# What `bit-cli` tells a tracker, and what it does with the answer

Four decisions, each one measured and each one with a test named at the bottom.
They are here because every one of them is a place where the specification
allows more than one behaviour and a caller cannot tell from the outside which
was chosen.

See `TODO/trackers.md`, T-063, T-065 and T-180.

## 1. Every tracker is asked at once, and that is deliberate

BEP 12 gives a `.torrent` an `announce-list` of **tiers**: try tier one, and
fall through to tier two only if every tracker in tier one fails. `bit-cli`
does not do that. It announces to every tracker in every tier at the same time,
reports each answer separately, and keeps the tier index on each row so a
reader can see what the torrent asked for.

**Why, for `bit-cli trackers`.** The command's job is to report on the
trackers, not to join a swarm through one of them. Tier order is a fallback
rule for a client trying to stay connected; here it would only mean waiting out
a dead tier before asking a live one, so a single dead tracker would cost the
whole run its wall clock and the reader would learn less.

**Why, for `bit-cli download`.** The same, and one more thing: the tier
structure is gone before the download path sees it.
`vendor/rqbit/crates/tracker_comms/src/tracker_comms.rs:252` takes its trackers
as a `HashSet<Url>` and pushes every one into a `FuturesUnordered`, so neither
the tiering nor the order survives. That is a property of the vendored tree
rather than a rule, and this repository owns that tree, so it is a choice being
kept rather than a limit being accepted.

**What is not lost.** Every tracker is still contacted, so nothing an
`announce-list` names is skipped. What differs from BEP 12 is that a backup
tier is contacted even when the first tier answers, which costs a backup
tracker one request per run.

**There is no `--respect-tiers`.** A flag that makes a reporting command report
less needs a question that wants it, and nothing has asked. If one arrives, it
belongs here.

## 2. The order trackers are asked in, when several were named

Duplicates are removed keeping the first occurrence, and the sources are
concatenated in this order:

1. the torrent's own `announce` and `announce-list`, unless `--replace-trackers`,
2. every `--tracker`, as one tier,
3. every `--tracker-file`, tier by tier, blank line separated.

**The torrent's own first.** A run that adds fifty trackers still asks the ones
the torrent named before the ones the caller did. `mtorrent`'s issue 29 is what
this is written against: with many trackers configured, outgoing connects timed
out and the torrent's own trackers were never reached.

## 3. `left`, and what a magnet says before its metadata arrives

`left` is the number of bytes this client still wants. A magnet has no length
until its metadata arrives, so there is no true answer, and every available
untrue one has a cost:

| Sent | What a tracker does with it |
| --- | --- |
| `0` | treats this client as a **seed** and hands it to every peer asking for one |
| `-1` | refused by real trackers. The AWS S3 tracker answers `400 Bad Request: left(-1) was not in the valid range 0 - 9223372036854775807` |
| absent | refused by the same tracker with a `500` |
| `9223372036854775807` | accepted, and not a seed |

`bit-cli` sends the last: `i64::MAX`, the largest value that tracker names as
valid. It is present, it is not negative, it is not zero, and a tracker reading
the field as signed or unsigned reads the same number.
`anacrolix/torrent`'s `tracker/http/http.go:36` clamps to exactly this and
carries the comment those two failures come from.

**Zero is the one to avoid**, and it is what this program used to send. It is a
well-formed answer that means something specific and false: other clients are
handed an address that cannot serve them, and nothing anywhere reports an
error.

**A caller is told which it was.** `bit-cli trackers --json` carries `left`,
with `bytes`, `known`, and the reason:

```json
"left": {
  "bytes": 9223372036854775807,
  "known": false,
  "reason": "the source carries no length yet, and zero would say this client is a seed"
}
```

`known` is the field that matters. Without it a reader has to recognise
`9223372036854775807` to tell a placeholder from a run that really does have
8 EiB left.

**Coming the other way, a negative count is not zero either.** A tracker that
answers `complete: -1` has not said the swarm has no seeders, so `seeders` is
absent from the report rather than `0`. That is the same distinction in the
opposite direction, and it is what `aquatic`'s own WebTorrent types record:
`left` there is an `Option<i64>` that is `None` "when opening a magnet link".

## 4. Scraping a tracker that does not follow the convention

BEP 48 derives a scrape endpoint from an announce URL by replacing a trailing
`announce` path component with `scrape`. A tracker whose path does not end that
way has no endpoint to derive, and guessing one produces a 404 that reads like
the tracker being down.

`bit-cli` says so instead:

```
http://example/t/9f3c does not follow the BEP 48 convention, so its scrape URL
cannot be derived. Name it with --scrape-url
```

`--scrape-url` replaces the derivation, including the protocol: an `http://`
announce may be pointed at a `udp://` scrape if that is what the tracker runs.
It names **one** endpoint, so a run carrying more than one tracker is refused
rather than scraping the same URL several times and reporting one answer as
many. Narrow the run first:

```bash
bit-cli trackers album.torrent --scrape --replace-trackers --tracker http://example/t/9f3c --scrape-url http://example/t/9f3c/scrape
```

There is no UDP equivalent of the problem: BEP 15 scrapes over the same socket
as the announce, so there is nothing to derive.

## What a malformed answer does

A tracker's URL comes out of a `.torrent`, which is untrusted input, so its
answer is untrusted too. One bad entry in a peer list costs that entry and
nothing else:

- an entry that is not a dictionary, such as the `peers: [42]` that crashed
  `anacrolix/torrent` before its PR 1055,
- an entry with no `ip` or no `port`,
- a `port` outside 0 to 65535, which would otherwise format into an address
  string nothing can dial,
- a compact list whose length is not a whole number of addresses.

Each one is named in `trackers[].invalid_peers` and warned about on stderr, and
every valid peer beside it is kept. A response with **no** `peers` key at all is
a well-formed empty swarm rather than an error.

## Where this is checked

| Claim | Held by |
| --- | --- |
| A source with no length does not announce itself as a seed | `an_announce_with_no_metadata_does_not_claim_to_be_a_seed` |
| A torrent with metadata announces the length it knows | `an_announce_with_metadata_sends_the_length_it_knows` |
| A negative count from a tracker is unknown, not zero | `a_negative_count_is_unknown_rather_than_zero` |
| One entry that is not a peer does not cost the others | `a_peer_list_with_an_entry_that_is_not_a_peer_keeps_the_others` |
| A truncated compact list keeps what it can and names the rest | `a_truncated_compact_peer_list_keeps_what_it_can_and_names_the_rest` |
| A named endpoint scrapes a tracker the convention cannot | `a_named_scrape_endpoint_reaches_a_tracker_the_convention_cannot` |
| One endpoint cannot stand for several trackers | `a_named_scrape_endpoint_is_refused_when_the_run_has_several_trackers` |
| The torrent's own trackers come before the caller's | `a_tracker_added_at_runtime_is_a_tier_after_the_torrents_own` |
| A repeated tracker is announced to once | `a_repeated_tracker_is_announced_to_once` |
| A torrent named by URL announces what the same file on disk does | `a_torrent_named_by_url_announces_the_same_as_one_on_disk` |
| A magnet announces from its hash, with no length to send | `a_magnet_announces_from_its_hash_with_no_metainfo` |

## Using the command

```bash
bit-cli trackers album.torrent --json
```

Announces to every tracker in the torrent and reports what each one said: its
tier, its protocol, its interval, its seeder and leecher counts, the peers it
returned, and its failure reason when it has one. `--scrape` asks for the
counts without announcing.

The announce is a real one, so the command binds the port it announces for as
long as the announce lasts and then withdraws the record with a second
announce carrying `event=stopped`. A diagnostic that registers a peer nobody
can dial, and leaves it registered for the tracker's interval, is worse than
no answer. `--port` chooses the port or the range, and `--no-withdraw` leaves
the record in place.

A tracker records the source address of the connection it was announced over,
so **one announce registers one of this host's addresses**. `--family auto`,
the default, announces once per address family the tracker resolves to and
reports each separately under `families`, with the endpoint each one reached.
`--family v4` and `--family v6` send one. The port is bound on both families,
because an IPv6 announce naming a port listening only on IPv4 registers the
same black hole the listener above exists to prevent.

```bash
bit-cli trackers album.torrent --family v6 --json
```

Whether a tracker **keeps** both addresses is the tracker's choice: one keyed
by peer id alone holds the last announce and drops the other, which is what
BEP 7's separate peer lists exist to fix. Announcing over both is what tells
it; `families` in the report is what says what came back.

A `download` run announces the same three events a client should:
`started` when the torrent goes live, `completed` the moment it finishes, and
`stopped` when the run ends. The last two come from `bit-cli` rather than from
the session, carrying the session's own peer id and port so the tracker
updates one record, and `--json` reports them under `announced`.

**Three events and no more, over HTTP and over UDP alike.** The periodic
announces in between carry no event at all, which is what BEP 3 asks for: an
event is a transition, and a client that repeats `started` is telling the
tracker it restarted while a client that repeats `completed` is adding to the
count of finished downloads a scrape reports. A run that already has the whole
file, which is what `bit-cli seed` is, sends `started` and then nothing: BEP 3
says `completed` is not sent when the file was complete to begin with.

```bash
pwsh scripts/check-announce.ps1
```

That runs the same six assertions over an HTTP announce and over a BEP 15 UDP
one, plus a redirected announce and one a tracker rejects at HTTP 200 with a
`failure reason` key. A rejection in the body rather than in the status is the
one a caller reading the status alone would record as a success; `bit-cli`
reports it as a failed tracker with the reason, and exits 6 when no tracker
answered.

**A magnet does not announce itself as a seed.** `left=0` means "I have all of
it", and a source with no metadata yet has no length to report, so what goes
out is `9223372036854775807` and the report says which it was under `left`,
with `known: false`. A tracker whose scrape endpoint is not BEP 48's is named
with `--scrape-url`. [`docs/trackers.md`](trackers.md) has both decisions,
why every tracker is asked at once rather than tier by tier, and what a
malformed answer costs.

## What a UDP tracker that does not answer costs

BEP 15 says to retry at `15 * 2^n` seconds for `n` from 0 to 8, which is nine
attempts and up to 62 minutes before giving up. **`bit-cli` does not do that,
on purpose.** A foreground diagnostic that can take an hour to say "this
tracker is down" has not answered the question the caller asked. What it does
instead is **three attempts inside `--tracker-timeout`**, one attempt being
`max(--tracker-timeout / 3, 1s)`.

The one second floor is why `--tracker-timeout 1s` and `--tracker-timeout 3s`
cost the same three seconds. Below three seconds the flag buys nothing.

The total is not one number, because a UDP announce is two exchanges, connect
then announce, and either can be the one that dies. Measured:

| what happens | attempts | at `--tracker-timeout 6s` |
| --- | --- | --- |
| nothing answers, so the announce is never sent | 3 | 6.06 s |
| connect answered at once, announce dead | 3 | 6.06 s |
| connect answered on its third attempt, announce dead | 5 | 10.10 s |

**Five attempts is the worst case there is**, so the budget for one UDP tracker
is `5 * max(--tracker-timeout / 3, 1s)`: **fifty seconds** at the default
`--tracker-timeout` of 30 seconds, and never under five. Six attempts cannot
happen, because a connect that is not answered by its third gives up and the
announce that would spend three more is never sent.

Every tracker is asked at once rather than tier by tier, so that budget is per
tracker and not per torrent: a torrent with twelve dead UDP trackers still
answers in fifty seconds.

```bash
pwsh scripts/check-udp-retry.ps1
```

[`examples/tracker-diagnostics.md`](examples/tracker-diagnostics.md) walks a
tracker that answers and one that does not, with the real output of each.
