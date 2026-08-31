# Trackers

Forty-one issues touch UDP tracker handling, announce backoff, BEP 12 tier
logic, and scrape.

---

### T-060 The announced port is wrong when no port is configured

Source:      https://github.com/ikatson/rqbit/issues/507 (open)
Category:    trackers
Priority:    P1
Effort:      S
Status:      **done**

Problem:     On 8.1.1 the announced port was always 0, which some trackers
             (aquatic among them) reject. On main it is always 4240 even when
             the session is listening elsewhere.
Relevance:   A wrong announced port means no peer can dial in. The torrent
             still downloads, so it looks fine and seeds nothing.
Approach:    `ListenerOptions::announce_port` exists in 9.0.0
             (`listen.rs:57`) and `bit-cli` leaves it `None`, which makes the
             session announce the port it actually bound. Verify that is what
             reaches the tracker rather than assuming: `bit-cli trackers` uses
             its own client and announces 6881 unconditionally, which is a
             separate bug of the same shape.
Acceptance:  `bit-cli trackers <TORRENT> --json` announces the port the session
             is listening on, and a packet capture or a tracker that echoes the
             peer list confirms the announced address is dialable.

**Done, and it was a verification rather than a fix.** `bit-cli` leaves
`ListenerOptions::announce_port` unset, so the session announces the port it
bound, and the test proves it end to end rather than by reading the source.

`cmd::seed::tests::the_session_announces_the_port_it_listens_on` runs
`bit-cli seed --port <N>` against a loopback tracker that records every
announce, waits for the first one, and asserts two things: the `port`
parameter is `N`, and a TCP connection to that port is accepted while the run
lasts. The second is the half a recorded number does not prove, and it is what
the acceptance asks for in place of a packet capture.

The tracker is `crate::test_support::Tracker`, a fixture that answers every
announce with the same bencoded reply and keeps the request lines. It is not
`crates/bit-cli-core/examples/loopback-tracker.rs`, which tracks a real swarm
and is what the interop scripts drive; a test cannot run an example binary.

The `bit-cli trackers` half of this entry was its own defect and is
[T-061](#t-061-bit-cli-trackers-announces-a-fixed-port).

### T-061 bit-cli trackers announces a fixed port

Source:      `bit-cli` defect, found while writing T-060
Category:    trackers
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `cmd::trackers::run` builds its `Announce` with a hardcoded 6881.
             The command does not start a session, so it has no listening port
             to announce, and announcing one it is not listening on registers
             an unreachable peer with the tracker.
Relevance:   `bit-cli trackers` is a diagnostic. Registering a fake peer as a
             side effect of asking a question is wrong.
Approach:    Two options, and the second is better: either bind a real port for
             the length of the announce, or send `numwant` with the announce
             and no port at all so the tracker treats it as a query. BEP 3
             requires `port`, so the honest version is to bind.
Acceptance:  `bit-cli trackers <TORRENT>` either binds the port it announces,
             or announces `event=stopped` immediately after so the tracker
             record does not linger. Whichever it does is tested.

**Done, and it does both.** The command binds a port for as long as the
announce lasts, announces that port, and then withdraws the record with a
second announce carrying `event=stopped`. Either alone leaves something
wrong: a bound port that stays registered after the process exits is a dead
address for the tracker's whole interval, and a withdrawal of a port nothing
ever listened on is a wrong answer politely retracted.

`--port` takes a port or a `START-END` range, the same spelling `download` and
`seed` use, and defaults to the same `6881-6889`. `--no-withdraw` leaves the
record in place for a caller who wants exactly the announce a client would
send. A scrape binds nothing and withdraws nothing: it carries no port and no
event.

The report carries `announced_port` and `withdrawn`, so what the command did
is in the JSON rather than only in what the tracker saw.

Three tests, all against the recording tracker in `test_support`:

- `the_announced_port_is_bound_and_the_record_is_withdrawn` asserts the
  announced port is neither zero nor 6881, that both announces carry the same
  port, that the events are `started` then `stopped`, and that the port is
  free again once the command exits, which is what says it was held.
- `no_withdraw_sends_one_announce_and_reports_no_withdrawal`.
- `a_scrape_carries_no_port_and_no_withdrawal`.

### T-062 Announce timing has no started, completed, or stopped events

Source:      https://github.com/ikatson/rqbit/issues/539 (open)
Category:    trackers
Priority:    P1
Effort:      M
Status:      **done**

Problem:     The session announces on unpause and then loops on the interval.
             It never sends `completed` when a download finishes, and never
             sends `stopped` when it shuts down.
Relevance:   Trackers use `completed` for the seeder count and `stopped` to
             drop the peer promptly. Without them a private tracker's ratio
             accounting is wrong and a public one keeps handing out a dead
             address for an hour.
Approach:    `bit-cli` runs in the foreground and knows exactly when both
             happen. Send `completed` from the watch loop on the transition to
             finished, and `stopped` in the shutdown path, through
             `bit-cli`'s own tracker client rather than waiting for upstream.
Acceptance:  A capture of a full `bit-cli download` run against a local tracker
             shows `event=started`, then `event=completed`, then
             `event=stopped`, in that order.

**Done, exactly as the approach describes it.** `cmd::download::announce_event`
sends one event to every tracker the torrent uses: `completed` from the watch
loop the moment the torrent finishes, and `stopped` after the loop ends however
it ended.

**The peer id is the part that had to be right.** An announce from a second
identity does not update the session's record, it creates another one, so a
`stopped` sent that way would leave the original peer registered and add a
phantom beside it. Both announces carry `handle.shared().peer_id` and the
session's own listening port, which is what makes them updates rather than a
second peer. The test asserts one peer id and one port across all three
announces.

One thing the shape of this costs. The `completed` announce is awaited inside
the watch loop, so a tracker that is slow to answer delays the next progress
tick by up to `--tracker-timeout`. It is bounded, it happens once, and it
happens at the moment the payload is already on disk, which is why it is
awaited rather than spawned: a run that exits before its own announce has left
has not announced.

The acceptance, run as a test rather than as a capture. The tracker records
every request line and the run is a real transfer from a loopback file server:

```
GET /announce?...&peer_id=-rQ9000-...&event=started&port=59193&...
GET /announce?...&peer_id=-rQ9000-...&port=59193&...&numwant=0&event=completed
GET /announce?...&peer_id=-rQ9000-...&port=59193&...&numwant=0&event=stopped
```

`a_run_announces_started_then_completed_then_stopped` asserts that sequence,
and that the report's `announced` array carries `completed` and `stopped` with
how many trackers accepted each.

**A payload already on disk announces in a different order, and that is not a
defect.** A torrent complete on its hash check finishes before the session's
own `started` announce has left, so a tracker sees `completed` first. The test
fetches its payload for that reason, and it is worth knowing before someone
reads the log of a resumed run and files it as a bug.

Three things this deliberately does not do. It does not fail a run when a
tracker is unreachable at the end: the announce is a courtesy and the payload
is already on disk. It does not send `started` itself, because the session
already does. And it counts trackers rather than reporting each one, because a
withdrawal that failed leaves a record that expires on its own, which is the
state the run was in anyway.

### T-063 Tracker tiers are announced in parallel rather than in order

Source:      `bit-cli` design decision, BEP 12
Category:    trackers
Priority:    P3
Effort:      S
Status:      **done** 2026-08-23T10:20Z

Problem:     `bit-cli trackers` asks every tracker at once. BEP 12 says a
             client should try tier one, and only fall through to tier two if
             every tracker in tier one fails.
Relevance:   For a client trying to stay connected, the tier order is the
             point. For a command whose job is to report on all of them,
             waiting out a dead tier one to reach tier two only makes one dead
             tracker cost the whole run.
Approach:    This is deliberate and documented in `cmd/trackers.rs`. The entry
             exists so the divergence is recorded rather than discovered. If a
             `--respect-tiers` flag is wanted later, it goes here.
Acceptance:  Decide, and either add the flag or close this with the reasoning
             in `docs/`.

**Decided: parallel, everywhere, and the reasoning is
[`docs/trackers.md`](../docs/trackers.md) section 1.** No `--respect-tiers`. A
flag that makes a reporting command report less needs a question that wants it,
and none of the four sessions that have touched this file has had one. The
section says what to do if one arrives.

**One of the two situations this entry described as different is not.** The
corpus note above says the divergence is "real for the command and *forced* for
the download path", because `librqbit` flattens `announce_list` into a
`HashSet`. That is still true of the code and no longer true of the
conclusion: the tree is vendored, so a `HashSet` in it is a choice this
repository is keeping rather than a limit it is under. Measured at
`vendor/rqbit/crates/tracker_comms/src/tracker_comms.rs:252`, which takes
`trackers: HashSet<Url>` and pushes every one into a `FuturesUnordered`. Kept,
for the same reason the command keeps it: every tracker an `announce-list`
names is contacted, and what BEP 12 would add is a delay.

**One clause of the corpus note was already true and now has a test.** mtorrent
issue 29 asks that a torrent's own trackers be announced to before any the
caller added. `tracker_tiers` has always concatenated them in that order and
nothing held it, so it was one edit from being lost:
`a_tracker_added_at_runtime_is_a_tier_after_the_torrents_own`.

**What is not adopted, and why.** Promoting a working tracker to the front of
its tier, which `TorrentNG/crates/rt-tracker/src/tier.rs:55` does, is for a
client that announces to the same tier repeatedly. `bit-cli trackers` announces
once and exits, and a download announces to every tracker on its own interval,
so there is no second choice for a promotion to inform.

### T-064 UDP tracker retry does not follow the BEP 15 backoff

Source:      BEP 15
Category:    trackers
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-22

Problem:     BEP 15 specifies retrying at `15 * 2^n` seconds for n from 0 to 8.
             `bit-cli`'s UDP client makes three attempts inside the configured
             timeout instead.
Relevance:   The spec backoff takes up to 62 minutes to give up, which is
             wrong for a foreground diagnostic. Three attempts inside
             `--tracker-timeout` is the right shape for this tool, but it is a
             deliberate divergence and should be written down.
Approach:    Keep the behaviour, document it in `docs/`, and make the attempt
             count configurable if a caller ever needs the spec timing.
Acceptance:  `docs/` states the retry policy and why it differs from BEP 15.

**Done, 2026-08-22, and the total the corpus note asked for is not the number
anyone would have written down.**

The Acceptance says `docs/`. That directory holds a generated schema, the
short-flag table, and a retired TUI mapping, and nothing a reader goes to for
behaviour; the user-facing documentation is `README.md`. The policy is under
[**What a UDP tracker that does not answer costs**](../README.md#what-a-udp-tracker-that-does-not-answer-costs),
and the BEP 15 row of the protocol table links to it, so a reader arriving from
either direction lands on the same paragraph.

**The behaviour is unchanged and the divergence stands.** Three attempts inside
`--tracker-timeout`, one attempt being `max(timeout / 3, 1s)`
(`tracker.rs:364`), against BEP 15's nine attempts at `15 * 2^n` and up to 62
minutes. The reasoning in Relevance is the reasoning: an hour to say "this
tracker is down" has not answered the question a foreground diagnostic was
asked.

**The total is five attempts, not three and not six.** A UDP announce is two
exchanges, connect then announce, and which one dies decides the cost.
`bench/udp-retry-20260822T052822784Z.json`:

| what happens | attempts | at `--tracker-timeout 6s` |
| --- | --- | --- |
| nothing answers | 3 | 6.06 s |
| connect answered at once, announce dead | 3 | 6.06 s |
| connect answered on its third attempt, announce dead | 5 | 10.10 s |

Six cannot happen: a connect that is not answered by its third attempt gives
up, so the announce that would spend three more is never sent. So the budget
for one UDP tracker is `5 * max(--tracker-timeout / 3, 1s)`, which is **fifty
seconds** at the default 30 second timeout and never under five. Trackers are
asked concurrently (`cmd/trackers.rs:166`, a `JoinSet`), so that is per tracker
and not per torrent.

The one second floor is worth stating on its own: `--tracker-timeout 1s` and
`--tracker-timeout 3s` both cost three seconds, so below three the flag buys
nothing. Measured at both.

```powershell
pwsh -NoProfile -File scripts/check-udp-retry.ps1
```

Three cases at three timeouts, judged against the attempt count rather than
recorded. It fails on either side of the budget: over it the budget is not the
budget, and under it an attempt was skipped, which is the same defect read the
other way round.

**What is not done here**, because it is not this entry's: the Approach also
offered to make the attempt count configurable "if a caller ever needs the spec
timing". Nobody has, `--tracker-timeout` already moves the whole ladder, and a
flag with no caller is a flag to maintain. If one appears it is a new entry.

**Connection id expiry**, which the corpus note below says this entry should
mention, cannot bite and here is why, so nobody re-derives it. `Client::udp`
(`tracker.rs:302`) opens a socket, connects, announces, and returns, once per
announce. **Nothing caches a connection id**, so there is no id to go stale:
the `download` path's three announces, `started`, `completed` and `stopped`,
are three separate connects however far apart they fall. What that costs is one
extra round trip per announce, which is the trade this shape makes and the
right one for a tool with no session to hang a cache off. A future change that
caches an id inherits the whole problem the corpus note describes, including
anacrolix's one-minute reissue rule and the tracker that answers
`"Connection ID missmatch."` with a trailing NUL byte, and must not be
made without it.

### T-065 Scrape is only implemented for the BEP 48 URL convention

Source:      BEP 48
Category:    trackers
Priority:    P3
Effort:      S
Status:      **done** 2026-08-23T10:20Z

Problem:     `tracker::scrape_url` derives the scrape endpoint by replacing a
             trailing `announce` path component with `scrape`. A tracker whose
             announce path does not end that way has no derivable scrape URL,
             and `bit-cli` reports that rather than guessing.
Relevance:   Guessing produces a 404 that reads like the tracker being down,
             which is a worse answer than "cannot be derived".
Approach:    Add `--scrape-url` so a caller who knows the endpoint can supply
             it.
Acceptance:  `bit-cli trackers <TORRENT> --scrape --scrape-url <URL>` scrapes
             a tracker whose convention differs.

**Done.** `--scrape-url` replaces the derivation, including the protocol, so an
`http://` announce may be pointed at a `udp://` scrape if that is what the
tracker runs. `a_named_scrape_endpoint_reaches_a_tracker_the_convention_cannot`
is the acceptance and it holds both halves: the same tracker, at a path BEP 48
cannot transform, fails with `cannot be derived` without the flag and answers
`5` seeders, `3` leechers and `9` completed with it. A test that only ran the
fixed case would not have said the flag was what fixed it.

**Two things the Approach did not decide.**

**It names one endpoint, so the run has to be about one tracker.** Applying it
to every tracker would scrape the same URL five times and report one answer as
five, which is a wrong number rather than a missing one. A run carrying more
than one tracker is refused with exit 2 and told how to narrow it, which is the
loud failure rather than the silent one.

**The message that fails now says what to do.** `does not follow the BEP 48
convention, so its scrape URL cannot be derived` was already right and left the
reader nowhere. It ends `. Name it with --scrape-url` now, because the whole
defect this entry describes is a caller who knows something the program does
not and has no way to say it.

**The document kind was undescribed and now is.** `docs/schema.md` had no
scrape sample at all, so `scrape_url` and every field a scrape produces went
undocumented. `schema_gen` drives one now, against a fixture serving a BEP 48
document at a non-conventional path.

---

## What the 2026-08-21 corpus adds to the three entries above

**T-063, tier order.** `TorrentNG/crates/rt-tracker/src/tier.rs` is the BEP 12
rule implemented: `:8` `Tier { trackers, active }`, `:55`
`TierSet { tiers, active_tier }`, `promote_active()` which **swaps a successful
tracker to the front of its tier**, and `advance()` which moves to the next
tracker on failure and then to the next tier. That is the whole algorithm and
it is small.

`bit-cli`'s divergence stands, and this entry's reasoning survives contact with
it: a command whose job is to report on every tracker should not wait out a
dead tier. What the corpus adds is that a `--respect-tiers` flag would be
cheap, and one fact that changes where the work is. `nanotorrent`'s patch 0008
records that **librqbit flattens `announce_list` tiers into a `HashSet`**, so
tier order is not available from the session at all without patching it.
`bit-cli`'s own `tracker.rs:115` keeps the tier index for the `trackers`
command, so the divergence is real for the command and *forced* for the
download path. Those are two different situations and this entry currently
reads as though they were one. Note also that promoting a working tracker to
the front of its tier is useful even without tier fallthrough, and costs
nothing.

mtorrent [Issue 29](https://github.com/DanglingPointer/mtorrent/issues/29)
adds the ordering rule worth having whatever is decided: **announce to the
torrent's own trackers before any configured extras.** With many trackers
configured, outgoing connects timed out and peers were never reached.

**T-064, UDP backoff.** Two ladders exist and both are defensible, which
supports this entry's decision to diverge deliberately rather than copy.
`torrent/tracker/udp/timeout.go:9` is BEP 15 as written, `15 * 2^n` clamped at
`n = 8`, which is 3840 seconds, in nine lines of code and up to 62 minutes.
`mtorrent/mtorrent-core/src/trackers/udp.rs:150` takes `MAX_RETRANSMISSIONS = 3`
with `:160` `timeout_sec = 15 * (1 << retransmit_n)`, so 15, 30, 60 and 120
seconds, giving up at 225 seconds and documenting that total. `bit-cli` makes
three attempts inside `--tracker-timeout`, dividing it by three
(`tracker.rs:364`), which is a third shape. **Documenting the total budget is
what the other two do and this entry should adopt**: the Acceptance says "state
the retry policy", and stating the worst-case wall clock is what a caller
setting a deadline actually needs.

One thing this entry does not mention and should. Connection ids expire, and a
client that caches one too long **will** be rejected.
`aquatic/crates/udp/src/workers/socket/validator.rs` shows why from the server
side: a `ConnectionId` is four bytes of seconds-since-start plus four bytes of
truncated keyed BLAKE3 over those bytes and the client IP, validated in
constant time and expiring after `max_connection_age`. anacrolix caches ids
with a one-minute reissue rule and carries an explicit workaround for one
tracker, forcing a reconnect when the error body is literally
`"Connection ID missmatch.\x00"`. A one-shot `bit-cli trackers` run is short
enough that this rarely bites, and a `download` that announces
`started`, then `completed`, then `stopped` over a long transfer is not.

**T-065, scrape convention.** Corroborated and closed as a question.
`torrent/tracker/http/scrape.go` derives the scrape URL with
`url.JoinPath("..", "scrape")`, the same BEP 48 convention, and **no repository
in the corpus implements another one**. So "cannot be derived" is the right
answer and `--scrape-url` is the right escape hatch, which is what this entry
already proposes. Related, from aquatic
[Issue 232](https://github.com/greatest-ape/aquatic/issues/232): there is no
canonical announce path for a UDP tracker either. The path in a `udp://` URL
is advisory, carried as a BEP 41 option if wanted, so a client must not
assume `/announce` there.

---

### T-180 A negative left in a tracker exchange has no decided handling

Source:      `reference/RESEARCH.md` section D, 2026-08-21
Category:    trackers
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T10:20Z

Problem:     Two halves of one question, and neither has been decided.

             **On the way out.** `bit-cli` announces `left` as a byte count.
             Before a magnet's metadata arrives there is no total length, so
             the true answer is unknown, and nothing in the tree records what
             is sent in that window.

             **On the way in.** A tracker or a peer-facing announce relay can
             carry a negative `left`, and `bit-cli`'s response parsing has no
             fixture for one.
Relevance:   aquatic [PR 254](https://github.com/greatest-ape/aquatic/pull/254)
             (MERGED) is the evidence that this is real rather than
             theoretical: **some clients send `left = -1`** when the length is
             unknown, rather than omitting the parameter, and a `usize` parse
             rejected the whole announce. That PR cross-references
             anacrolix/torrent#981, so at least two implementations met it
             independently. `aquatic/crates/ws_protocol/src/incoming/announce.rs:13`
             separately records that `left` **may be absent entirely**, for
             instance when a magnet is opened.

             `bit-cli` is on both sides of this. It announces for real from
             `trackers` and from `download`'s `started`, `completed` and
             `stopped` events, and it parses responses. A magnet is a first
             class source here, so the unknown-length window is a normal path
             and not an edge case.
Approach:    Decide both halves and test both.

             Outbound, there are three candidate answers and the third is
             probably right: send `left=0`, which claims to be a seed and is a
             lie that costs other peers; omit the key, which some trackers
             reject; or send a large sentinel. What settles it is that a
             tracker rejecting the announce is a loud failure and claiming to
             seed a payload you do not have is a silent one, so prefer
             correctness over acceptance and record which trackers refuse.
             `bit-cli trackers <MAGNET>` against a real tracker is the
             measurement, and it is cheap.

             Inbound, accept a negative or absent value and normalise it to
             "unknown" rather than to zero, because zero means seed and
             unknown does not. A signed parse plus an `Option` is the whole
             change.

             While the response parser is open, anacrolix
             [PR 1055](https://github.com/anacrolix/torrent/pull/1055) is the
             other fixture to add: a tracker returning `peers: [42]`, or a peer
             dictionary missing `ip` or `port`, crashed the client. The fix
             keeps the good entries and errors on the bad ones, which is the
             right shape for `bit-cli`, whose trackers come from untrusted
             torrents. aquatic
             [Issue 82](https://github.com/greatest-ape/aquatic/issues/82) adds
             the empty case: a response with **no `peers` key at all** is a
             well-formed empty swarm, not a parse error.
Acceptance:  `bit-cli trackers <MAGNET> --json` states what it sent for `left`
             and why, a fixture response carrying `left = -1` parses to
             "unknown" rather than to a seed, and a fixture response carrying
             `peers: [42]` keeps every valid peer and names the invalid entry
             without failing the run.

**Done, and the outbound half was a live defect rather than an undecided
question.** The Problem says "nothing in the tree records what is sent in that
window". What was sent is `0`: `cmd/trackers.rs` passed
`meta.map(total_length).unwrap_or(0)` and `cmd/download.rs` subtracted from a
`total_bytes` that is zero until metadata arrives. Zero is not an absence of an
answer. It is the answer "I am a seed", so every magnet this tool announced was
offered to other clients as a source and could serve none of them.

**The value sent is `i64::MAX`, and the corpus decided it rather than
taste.** `torrent/tracker/http/http.go:36` carries the two failures that rule
the other candidates out, both from a real tracker: `left=-1` gets
`400 Bad Request: left(-1) was not in the valid range 0 - 9223372036854775807`,
and omitting the key gets a `500`. So the answer is the largest value that
tracker names as valid. `anacrolix/torrent` clamps to exactly it.

**`Option<u64>` rather than a sentinel in the struct**, which is what found the
second site. The type change turned four call sites into compile errors, and
one of them was `download.rs`'s announce: the entry only describes `trackers`.

**The report says which it is.** `left` carries `bytes`, `known` and a reason,
because a reader who cannot tell a placeholder from a measurement would have to
recognise `9223372036854775807` by eye.

**The inbound half is the same distinction pointed the other way**, and the
entry's wording does not survive contact with the wire: an announce **response**
carries no `left`. What it carries is `complete`, `incomplete` and `downloaded`,
and this tree clamped a negative to zero with `n.max(0)`. Zero seeders is a
statement about the swarm that a tracker sending `-1` did not make, so those
are `None` now, which is what an absent key already produced. `count_of` is the
one function all six sites go through.

**`peers: [42]` was already survived and never mentioned**, which is the half
worth having: `filter_map` dropped it silently, so the run reported a smaller
swarm than the tracker described with nothing to say why. Four shapes are named
now, in `trackers[].invalid_peers` and on stderr: an entry that is not a
dictionary, one with no `ip`, one with no `port`, and a `port` outside 0 to
65535 that would otherwise format into an address nothing can dial. A compact
list whose length is not a whole number of addresses is the fifth, and
`chunks_exact` was dropping that remainder without a word too.

**The measurement, run against the defect.** With the call site put back to
`unwrap_or(0)` the acceptance test fails and prints what the old tree sent:
`"left":{"bytes":0,"known":true,...}` for a magnet. That is the whole entry in
one line of JSON.

**The contract is [`docs/trackers.md`](../docs/trackers.md)**, with the table
of the four candidate values, what each costs, and a test named for every
claim.

```
$ cargo test -p bit-cli --lib trackers::
test result: ok. 20 passed; 0 failed; 0 ignored; 396 filtered out
```

---

### T-235 Nothing compares the numbers a tracker sees against the run that made them

Source:      the operator, 2026-08-24. Corpus: `RESEARCH.md` entry 29,
             `RatioTracker/ratiotracker.py` at
             `45dc7d40a365921dc9d050bff06c57a16cd82ab7`
Category:    trackers
Priority:    P1
Effort:      S
Status:      **done 2026-08-24**

Problem:     `uploaded`, `downloaded` and `left` are the only thing a tracker
             knows about a client, and nothing in this repository compared them
             against what the run itself reported. Every other announce
             property had a check: [T-062](trackers.md) covers the events,
             [T-063](trackers.md) the tier order, [T-064](trackers.md) the UDP
             backoff, [T-060](trackers.md) and [T-061](trackers.md) the port.
             The three numbers had none.

             A wrong number there is invisible locally, permanent on the
             tracker, and indistinguishable from cheating.

Approach:    `scripts/check-announce.ps1`, driving the fixtures that already
             exist rather than new ones: `loopback-tracker` for the tracker,
             `bit-cli seed` for the swarm, `bit-cli download` as the subject.
             Loopback only, and it changes no number it reports.

             The evidence had to come from the tracker rather than from the
             client, because the client is what is under test.
             `crates/bit-cli-core/examples/loopback-tracker.rs` gains
             `--announce-log <PATH>`, which appends one JSON object per
             announce. It carries the **raw query string** as received, the
             request headers, and the parsed fields.

             The raw query is the part that matters and it is why the log is
             not built from the parsed map: `parse_query` returns a
             `BTreeMap`, which has already sorted the parameter order away, and
             order is what a real tracker fingerprints
             (`RESEARCH.md` entry 25). Nothing else can recover it afterwards.

             The headers are kept for the same reason. Before this the fixture
             drained them with a comment saying nothing depends on any of them.

Prove:       ```
             pwsh -NoProfile -File scripts/check-announce.ps1
             ```

             Six cases, all judged, all holding on 2026-08-24 at 8 MiB over
             loopback with a 5 second announce interval, 10 announces recorded
             and 6 from the subject:

             | case | what it asserts | result |
             | --- | --- | --- |
             | started-left | the first event is `started` and `left` is the whole payload | `started`, left 8,388,608 of 8,388,608 |
             | completed | `completed` is sent exactly once and `left` is 0 by then | one event, left 0 |
             | stopped | `stopped` is sent | `started,completed,-,-,-,stopped` |
             | left-monotonic | `left` never rises | 8,388,608 then 0 five times |
             | totals-match | the last announce covers the payload and does not exceed the report | announced 8,388,608, report 8,388,608 |
             | interval | the gap between ordinary announces is at least `min interval` | smallest 5.01s against 5s over 3 |

             `totals-match` asserts a bound rather than equality on purpose.
             The tracker's figure is taken at the last announce and the
             report's at exit, so requiring them equal to the byte would be
             asserting a scheduling outcome, which is the line
             [RULES.md](RULES.md) section 5 draws and the reason
             [T-148](bench.md), [T-160](cli-surface.md) and
             [T-162](webseed.md) each cost a red job.

             `interval` allows one second of slack for the same reason: the
             tracker stamps on arrival and the client times from its own clock.

Notes:       **The six cases passed and the run still found two things**, both
             about identity rather than about the numbers, and both printed
             beside the verdict because the check reports what it saw:

             - The peer id prefix is `-rQ9010-`, and `bit-cli trackers` uses
               `-BC0100-`. That is [T-236](peers.md).
             - The query order is `info_hash`, `peer_id`, `event`, `port`,
               `uploaded`, `downloaded`, `left`, `compact`, `no_peer_id`,
               `key`. No client in `RESEARCH.md` entry 23's ninety-four
               profiles puts `event` third, and none omits `numwant`. That is
               design input for [T-234](peers.md) rather than a defect.
             - The `User-Agent` is `bit-cli 0.2.0`, with a space. Every profile
               in entry 23 uses `Name/version`.

             What this does not cover, said plainly rather than left to be
             discovered: a redirected announce, a tracker that answers with a
             `failure reason`, and the UDP announce path. The first two are one
             case each against a fixture that does not exist yet; the third
             needs a UDP tracker fixture, and `loopback-tracker` is HTTP.
             Filed as [T-237](trackers.md).

             Nothing from `RatioTracker`'s eight tests was ported. All eight
             send a number the run did not make, which is the one thing this
             harness must never do.

---

### T-237 Three announce paths have no fidelity case

Source:      [T-235](trackers.md)'s own closing, 2026-08-24
Category:    trackers
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-24

Problem:     `scripts/check-announce.ps1` covers the ordinary HTTP announce and
             says so. Three paths it does not reach, each of which is one case
             against a fixture that does not exist:

             - **A redirected announce.** `scripts/check-redirect.ps1` exists
               and is about `--json` capture on Windows, not about trackers, so
               the name is taken and the coverage is not there. A tracker that
               answers `301` or `302` to `/announce` is ordinary, and what
               matters is whether the redirected request still carries the same
               `uploaded`, `downloaded` and `left`.
             - **A `failure reason`.** `RESEARCH.md` entry 29 records the rule:
               a rejection is a non-200 **or** a 200 whose bencode carries
               `failure reason`, and a check reading only the status calls the
               second one a success. `loopback-tracker` has a `failure()`
               helper already and no way to ask it for one.
             - **The UDP announce.** `loopback-tracker` is HTTP.
               [T-064](trackers.md) covers the BEP 15 backoff with its own
               fixture; the three numbers over UDP are uncovered.

Approach:    Two flags on the fixture and one on the check, rather than a
             second fixture: `--redirect-announce <N>` to answer the first N
             announces with a 302 to itself, and `--fail-announce <REASON>` to
             answer with a bencoded failure. The UDP path is the larger half
             and is the reason this is S rather than XS.

Prove:       ```
             pwsh -NoProfile -File scripts/check-announce.ps1
             ```

             Three more judged cases in its table: `redirect` shows the
             redirected request carrying the same three numbers as the one that
             was redirected; `failure-reason` shows a non-zero exit and the
             reason in `--json` rather than a reported success; and `udp` shows
             the same six assertions over a UDP announce.

#### Closed 2026-08-24, and the third path was hiding a defect

Nine judged cases where there were six, all holding.
`bench/announce-20260824T222123899Z.json` is the run.

```
case           judged   ok detail
started-left     True True first event 'started', left 8388608, payload 8388608
completed        True True one completed event, left 0
stopped          True True events: started,completed,-,-,-,stopped
left-monotonic   True True left: 8388608 -> 0 -> 0 -> 0 -> 0 -> 0
totals-match     True True announced downloaded 8388608, uploaded 0; report downloaded 8388608, uploaded 0
interval         True True smallest gap 5.01s against a min interval of 5s over 3 ordinary announces
udp              True True 6 of 6 judged and all hold over 6 announce(s)
redirect         True True followed, exit 0, same up=0 down=0 left=8388608 across the hop
failure-reason   True True 2 of 2 row(s) carry the reason, responded 0, exit 6, 2 announce(s) reached the tracker
```

**Two flags on the fixture and a UDP socket, as the approach said.**
`--redirect-announce <N>` answers the first `N` requests to `/announce` with
`302 Found` and a `Location` of `/announce-r` carrying the same query, and
`/announce-r` is served identically, so the log holds the request that was
redirected and the one that followed it. `--fail-announce <REASON>` answers
every announce with `REASON` in a `failure reason` key: HTTP 200 over TCP,
BEP 15 action 3 over UDP. Both record the announce **before** refusing or
redirecting it, which is what lets the check tell a rejection apart from a
request that never arrived.

The announce log gained `protocol` and `path`, and the UDP path fills every
other field with the same spelling the HTTP path uses, including turning
BEP 15's event number back into `started`, `completed` or `stopped`. That is
why one function in the check reads both and the `udp` row is the same six
assertions rather than a second set.

**`bit-cli trackers` is the subject for `redirect` and `failure-reason`**
rather than a download, because one announce is the whole question in both and
a transfer would add nothing. The UDP round is a full seeder and leecher,
because four of the six assertions are about a transfer.

**The UDP round needed its own payload.** The announce URL is not in the info
dictionary, so the same payload under the same name produces the same info
hash for the HTTP and UDP torrents, and one tracker would have handed each
round the other's peer records.

**A Windows detail that cost a wrong explanation before it cost anything
else.** The UDP socket asks for the TCP listener's port and does not require
it: `netsh int ipv4 show excludedportrange udp` lists twelve reserved bands on
this machine, and a bind inside one fails with `os error 10013` rather than
"address in use". Both failures seen were inside a listed band, 53502 and
53521 in 53495-53594 and 65389 and 65390 in 65356-65455. The first guess
written down was that the running soak's leechers held the port, which twelve
consecutive clean starts disproved. The fixture falls back to an OS-chosen
port and prints whichever it got.

**And CI's beta clippy job caught the fixture on the day it was written.**
`take_redirect` used `AtomicU32::fetch_update`, which is deprecated on beta in
favour of `try_update`. `try_update` is not on stable yet, so neither name is
portable and the compare and exchange loop both of them wrap is what shipped.
That job exists for exactly this and it is the second time it has paid, after
[T-218](cli-surface.md): it fails without failing the run, so the warning
arrives while the code is still in hand.

Notes:       **The `udp` case failed the first time it was run, and it was
             right to.** That is [T-256](trackers.md), and it is the reason
             this entry is worth more than the three rows it added.

### T-256 A UDP announce sends its event on every request, where an HTTP one sends it once

Source:      [T-237](trackers.md)'s `udp` case, the first time it was run,
             2026-08-24
Category:    trackers
Priority:    P1
Effort:      S
Status:      **done**, 2026-08-24

Problem:     `vendor/rqbit/crates/tracker_comms/src/tracker_comms.rs` derived
             the BEP 3 event from the torrent's **current state** on every UDP
             announce:

             ```rust
             event: match stats.torrent_state {
                 TrackerCommsStatsState::None => EVENT_NONE,
                 TrackerCommsStatsState::Initializing => EVENT_STARTED,
                 TrackerCommsStatsState::Paused => EVENT_STOPPED,
                 TrackerCommsStatsState::Live => {
                     if stats.is_completed() { EVENT_COMPLETED } else { EVENT_STARTED }
                 }
             },
             ```

             An event is a transition and not a state. The HTTP monitor in the
             same file already knows that: `task_single_tracker_monitor_http`
             sets `event = Some(Started)` once and `event = None` after the
             first answered announce, and has no `Completed` arm at all.

             One client, one 22 second run, the same payload over both
             protocols against `loopback-tracker`, with nobody to talk to so
             every announce is an ordinary one:

             | protocol | events the tracker recorded |
             | --- | --- |
             | udp | `started`, `started`, `started`, `started`, `started`, `stopped` |
             | http | `started`, none, none, none, none, `stopped` |

             And with a seeder present, so the leecher finishes: over UDP the
             leecher sent `completed` four times and the seeder, which had the
             whole payload before it started, sent it on every announce.
Relevance:   The cost is on the tracker and is invisible here, which is the
             shape [INDEX.md](INDEX.md)'s first ordering question ranks above
             everything else.

             `completed` is how a tracker counts finished downloads: it is the
             `downloaded` field of a BEP 48 scrape. A seeder announcing every
             five minutes adds 288 a day to a number that should never have
             moved, and BEP 3 says a client that already had the whole file
             does not send `completed` at all.

             `started` repeated is the same defect in the other direction. A
             tracker is entitled to treat `started` as a new session, and a
             client that sends one every interval is telling it so.

             It is also a divergence between two paths of one client, which is
             what [T-235](trackers.md) built the announce log to find and what
             [T-237](trackers.md) reached the UDP path to look at.
Approach:    Give the UDP monitor loop the same discipline the HTTP one has,
             in the loop rather than in the request builder, because only the
             loop knows what the announces before this one carried.
             `UdpAnnounceEvents` is `started` on the first announce, `stopped`
             while paused, and nothing otherwise.

             Two details that are not obvious:

             - **The event is peeked and committed separately.** An announce
               nothing answered has not delivered anything, so `started` is
               not spent on a datagram that went nowhere.
             - **One event per round, not per datagram.** A dual-stack tracker
               is announced to over both families at once, and both carry the
               same event, which is what `tracker_one_request_http_each`
               already does on the HTTP side.

             **`completed` is not sent from the loop at all.** That is the
             half worth arguing about, and it is settled by what the HTTP path
             does: `bit-cli` announces its own completion at the instant it
             happens ([T-062](trackers.md)), over whichever protocol the
             tracker speaks, and the HTTP monitor sends no `completed` beside
             it. A loop that sent one too made the same run tell the tracker
             twice, which double counts a finished download as surely as
             sending it every interval did. That was measured: with the loop's
             own `completed` in place the run produced two, one at the
             transition and one at the next interval.
Prove:       ```
             pwsh -NoProfile -File scripts/check-announce.ps1
             ```

             The `udp` case is the acceptance and it was run both ways on
             2026-08-24, against the same tree, rebuilding in between:

             | | `udp` row |
             | --- | --- |
             | with the fix reverted | `False`, "1 of 5 failed: completed (4 completed events, and BEP 3 asks for one)", exit 1 |
             | with the fix | `True`, "6 of six judged and all hold over 6 announce(s)", exit 0 |

             **Five judged rather than six is part of the evidence.** Against
             the defect the `interval` case has nothing to measure, because
             every announce carried an event and the case only looks at
             ordinary ones. A defect that hides a case is worse than one that
             fails it.
Notes:       The patch is `patches/UPSTREAM.md`'s tracker-comms section. It is
             for this repository and is never offered upstream,
             [RULES.md](RULES.md) section 6.

### T-251 A web seed has twelve knobs of its own and a tracker has none

Source:      the operator's brief of 2026-08-24, measured the same day
Category:    trackers
Priority:    P2
Effort:      M
Status:      partial

Problem:     The asymmetry is the whole entry.

             A web seed source is a `SourceSpec` at
             `crates/bit-cli-core/src/webseed/binding.rs:469`, and every field
             on it is per source: `scope`, `mode`, `template`, `style`,
             `priority`, `headers`, `user_agent`, `auth`, and a `SourceLimits`
             carrying `concurrency`, `connections`, `chunk_size`, `timeout_ms`,
             `connect_timeout_ms`, `retries`, `max_errors`, `cooldown_ms`,
             `rate_limit`, `retry_status` and `fatal_status`. A binding table
             sets any of them for any one source.

             A tracker is a URL in a list. `tracker::Client` at
             `crates/bit-cli-core/src/tracker.rs:314` holds one `timeout` and
             one `connect_timeout` for the whole run, fixed at construction.
             `--tracker-timeout`, `--tracker-connect-timeout` and
             `--tracker-interval` are all run-wide. There is no per-tracker
             anything.

             A peer is thinner still: `--peer` adds one, `--block-peer` refuses
             one, `--max-peers` caps the count. Nothing else is addressable.
Relevance:   One dead tracker in a tier of five costs every announce the full
             run-wide timeout, and the only way to give that one a shorter
             deadline is to give it to all five.

             The same shape appears in the other direction: a private tracker
             that wants a longer interval and a public one that wants a shorter
             one cannot both be honoured, because the override is one number.

             It is worth stating plainly that the web seed side is the
             differentiator and it is done. This entry is about the two axes
             that were left flat next to it.
Approach:    The binding table is the model and it already works, so the
             cheapest honest version is the same shape: a `[[tracker]]` table
             in the same file `--web-seed-config` reads, with `url`, `tier`,
             `timeout`, `connect_timeout`, `interval`, `enabled`, `key`, and
             the headers.

             The flags stay as the defaults every entry inherits, which is what
             `--web-seed-*` already does for sources, so nothing existing
             changes meaning.

             Peers are the smaller half and the one to do second: a
             `[[peer]]` table with `addr`, `priority` and `rate_limit`, plus
             what [T-234](peers.md) is already adding for identity.

             [T-114](cli-surface.md) is the neighbour, not a duplicate: it is
             per-source options in an aria2 input file, one entry per download.
             This is per-tracker options within one download.
Acceptance:  A config naming five trackers where one has a 1s timeout and the
             rest have 30s produces, in one announce round against
             `loopback-tracker`, one request that gave up at 1s and four that
             did not, read from `--announce-log` rather than from the report.
             `scripts/check-announce.ps1` grows the case.

#### The source half is done, 2026-08-24, and the twelve knobs are not

[T-245](cli-surface.md) left one command behind and named this entry as the
one that owns it: `bit-cli trackers` refused a `.torrent` named by URL, with
"an info hash is needed to announce, and this source does not carry one",
while the URL's document carries one and five other commands fetch it happily.
Measured over loopback before the change, one torrent served by
`loopback-fileserver`:

| command | before |
| --- | --- |
| `info`, `files`, `tree`, `magnet`, `peers` | exit 0 |
| `verify` | exit 7, which is the answer: it read the torrent and the payload was not beside it |
| `trackers` | exit 4, the refusal above |

`run` at `crates/bit-cli/src/cmd/trackers.rs:95` read the metainfo for
`Kind::File` and `Kind::Stdin` and nothing else, so every other kind fell to
`None` and then to a refusal that only a magnet or a bare info hash should
ever have reached. It calls `crate::source::resolve_source` now, which is the
same one line `info`, `files` and `tree` use, and the two kinds that genuinely
carry a hash and no metainfo keep the old path.

Against the same fixture afterwards, a URL and the file on disk produce the
same report:

```
TIER  TRACKER                          FAMILY  STATUS  RTT   SEED  LEECH  INTERVAL  PEERS
0     http://127.0.0.1:57416/announce  v4      ok      14ms  0     1      5s        0
```

`left` is the field that says the fetch really happened: 131,072 bytes over
the URL, the same as on disk, and a number nothing but the metainfo could
supply. `--scrape` over a URL works for the same reason.

**Two tests, and the second is the half that must not change.**
`a_torrent_named_by_url_announces_the_same_as_one_on_disk` runs both sources
through one fixture tracker and compares the info hash and `left`;
`a_magnet_announces_from_its_hash_with_no_metainfo` holds the case the refusal
was written for, where `left` is a placeholder and says so.

```bash
cargo test -p bit-cli --lib cmd::trackers
```

**What is still open is the entry's own subject**: the `[[tracker]]` table,
the per-tracker `timeout`, `connect_timeout`, `interval`, `enabled` and `key`,
and the `[[peer]]` table after it. Nothing above touches any of that, and the
Acceptance is unchanged.

### T-261 There is no way to get a current tracker list, so every torrent carries whatever it was born with

Source:      the operator's ruling of 2026-08-29, while ruling on T-244
Category:    trackers
Priority:    P2
Effort:      M
Status:      open

Problem:     A torrent announces to the trackers in its own `announce` and
             `announce-list`, and nothing else. Those were chosen by whoever
             made the file, often years earlier, and a tracker that has moved,
             died or started refusing is announced to on every run at the BEP
             15 backoff until it gives up.

             `bit-cli` can already add a tracker: `create --announce` writes
             one and [T-251](#t-251-a-web-seed-has-twelve-knobs-of-its-own-and-a-tracker-has-none)
             is giving a tracker per-instance knobs. What is missing is
             **where the list comes from**. Every caller has to find one,
             judge it, and paste it.
Relevance:   It is the same shape as the web seed argument this whole tool is
             built on: the file names sources, the file is not editable, and
             the useful thing is attaching better sources at run time. A
             tracker list is that for the swarm half.

             It is also the second consumer the published-data path needs.
             [T-260](cli-surface.md) publishes `fingerprints/` because
             something has to be first; a tracker list is the file people
             actually want by URL, and designing the publishing format against
             one consumer is how it ends up fitting only that one.
Approach:    Four stages, and each is separately useful, so they are separately
             shippable.

             **Fetch and merge.** Several published lists, named in the source
             the way `scripts/check-page-fetch.ps1` names its pages rather than
             discovered at run time, fetched over the source-document path that
             [T-244](cli-surface.md) built, so they get the same client and
             the same deadline as every other document fetch.

             **Dedupe, and it is not string equality.** `udp://t.example:6969`
             and `udp://t.example:6969/announce` are one tracker;
             `http://` and `https://` on one host are one tracker offering two
             transports. Normalise scheme, host, port and path before
             comparing, and keep every form seen so the report can say what it
             collapsed.

             **Liveness and rank.** Announce or scrape each one and record
             whether it answered, then rank. The operator's stated axes are
             time to first response and time to a usable peer set, so a
             tracker that answers quickly with nothing ranks below one that is
             slower and useful. `scripts/check-announce.ps1` and the loopback
             tracker are where the shape of that measurement already lives.

             **Two outputs, and neither is a daemon.** Write the ranked list to
             a file for another tool to read, and attach it to a torrent at run
             time for this one. Attaching is the same argument as a web seed
             scope: it must not rewrite the `.torrent`, which is decision 7.3's
             territory and this repository's whole reason for existing.
Acceptance:  Against `loopback-tracker` standing in for several trackers, with
             one of them refusing and one of them slow: the merge collapses the
             duplicate forms and says so, the ranking puts the healthy fast one
             first and the refusing one last, and the written file round trips
             into a run that announces to exactly those trackers in that order.
             No case reaches the network.

             One case does, separately and named: the real lists fetch and
             parse. It is the same shape as
             `scripts/check-metalink-real.ps1`, which is the worked example of
             a check that is allowed the internet.
Notes:       **Nothing here scrapes an index or a private tracker.** The lists
             are the published, openly distributed ones, and a tracker that
             needs an account is out of scope by construction: there is nowhere
             to put a credential and decision 7.4 says there is no state file
             to keep one in.

             The liveness measurement announces under this client's real peer
             id, which [T-236](peers.md) fixed, so a tracker's statistics
             see `bit-cli` and not something it is pretending to be. That is
             deliberate and it is the opposite of what
             [T-244](cli-surface.md) does for a web page: a tracker is a
             peer of this tool, and a page is a document served to whoever asks.
