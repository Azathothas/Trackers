# Many sources for one payload

Five scenarios the operator put to this project, each about pointing several
different kinds of source at one payload and getting the bytes as fast as
possible without fetching any of them twice.

This file is written for the session that implements them. It is in three
parts:

1. **What already works**, each claim backed by a command that was run against
   a local fixture, with the output.
2. **The gaps**, as ordinary `TODO/` entries with an acceptance criterion.
3. **The harness** the acceptance needs, because most of it does not exist.

Everything below was checked against the code at commit `74986c3`, not
inferred from the documentation. Where a claim came out of running something,
the command is here and the fixture is reproducible.

---

## Vocabulary

The scenarios say "web seed" and "ddl" as if they were different things. In
`bit-cli` they are the same thing under one model, and that is why several of
these scenarios need less work than they look like they need.

A **binding** is a triple: a **source** (an HTTP URL with its own headers,
auth, agent, timeouts, concurrency, connections, and rate cap), a **scope**
(which part of the torrent that source may serve), and a **composition** (how
the request URL is built from the source URL and the torrent's `name` and
`path`).

A "direct download link for one file" is therefore already expressible: it is
a source with scope `file:N` and composition `exact`. There is no separate
"ddl" concept to add and nothing about the term needs to reach the CLI.

---

## Part 1: what already works

The fixture for everything in this part:

```powershell
# A deeply nested three-file torrent, and a CDN copy of one file under a
# different name in a different directory.
payload/deep/nested/dirs/file.blob   64 MiB
payload/deep/other.bin                8 MiB
payload/readme.txt                    1 MiB
cdn/a3f1b2c4-signed-blob.dat         the same 64 MiB, renamed

bit-cli create payload --name payload --piece-length 1MiB `
  --no-creation-date --output torrent_a.torrent
```

```
$ bit-cli files torrent_a.torrent
INDEX  SIZE       SHARE   PIECES  PATH
0      64.00 MiB  87.67%  0-63    deep/nested/dirs/file.blob
1      8.00 MiB   10.96%  64-71   deep/other.bin
2      1.00 MiB   1.37%   72-72   readme.txt
```

### Scenario 1 works today, in full

One selected file out of a deep tree, 70% already on disk, accelerated by an
arbitrary CDN URL whose name and path have nothing to do with the torrent's.
Everything below was run. What happens when the CDN starts answering 403 is
[T-130](#t-130-a-source-cannot-be-told-which-statuses-are-worth-retrying),
which is now **done**: with `--web-seed-retry-status 403` a source whose
signature expires 22 times over a 64 MiB payload completes it byte for byte.

```bash
bit-cli download torrent_a.torrent --dir out \
  --select-file 0 \
  --web-seed-for 'file:0=http://cdn.example/cdn/a3f1b2c4-signed-blob.dat' \
  --web-seed-mode exact \
  --continue
```

Run against the fixture with 45 MiB of the 64 MiB file pre-seeded:

```
exit=0 completed=1 failed=0
total=64.00 MiB downloaded=64.00 MiB from_web_seeds=19.00 MiB
file.blob: MATCHES source
other files present? payload\deep\nested\dirs\file.blob
```

Four things that answers:

- **Only the missing bytes were fetched.** 19 MiB over HTTP for the 19 MiB
  that was not on disk. The hash check on add is what establishes that, and
  `--continue` is on by default.
- **The URL needed no relationship to the torrent.** `--web-seed-mode exact`
  means the URL is the complete resource and nothing is appended.
- **Only the selected file was written.** The other two paths were never
  created, which is [T-013](disk-io.md).
- **The result is byte-identical to the source.** Every HTTP-sourced piece is
  hash-checked at the source before the session sees it, which is
  `--web-seed-verify piece` and the default.

`bit-cli webseed list` resolves the binding without touching the network, so
the addressing can be checked before any bytes move:

```
[0] http://127.0.0.1:55654/cdn/a3f1b2c4-signed-blob.dat
  scope              file:0 (87.67%, 1 files, 64 whole pieces, 0 partial)
  composition        exact / auto / priority 0
  FILE  IN SCOPE   PATH                        URL
  0     64.00 MiB  deep/nested/dirs/file.blob  http://127.0.0.1:55654/cdn/a3f1b2c4-signed-blob.dat
```

`uncovered pieces 64-72` is printed for the pieces no source covers, which is
what tells the caller the other two files still need the swarm.

### Scenario 3 works today, in full

Scenario 3 is Scenario 1 without `--select-file`. Drop that flag and the other
files keep coming from peers while file 0 comes from the CDN. Nothing else
changes, because a scope of `file:0` already restricts the source to that file
and the coverage report already names what is left for the swarm.

### Redirects, including a URL that re-signs on every request

The fetcher does not pin a resolved URL. Every ranged request goes to the URL
the caller gave, and `reqwest`'s default redirect policy follows up to ten
hops per request. So a CDN whose stable URL 302s to a freshly signed URL each
time is handled by doing nothing: each request gets its own signature.

`webseed test` reports the chain hop by hop, so the behaviour is checkable
before a download:

```bash
bit-cli webseed test torrent_a.torrent --web-seed-for 'file:0=https://cdn.example/blob'
```

### Per-source everything, through the binding table

Anything the command line sets globally, the table sets per source. This is
Scenario 4's mapping problem and most of Scenario 5's control problem:

```toml
[[source]]
url         = "https://mirror-a.example.com/pub/"
scope       = "*"
mode        = "auto"
priority    = 10
connections = 2
concurrency = 8
rate_limit  = "40MiB/s"
headers     = { X-Region = "apac" }
user_agent  = "bit-cli/0.1"
auth        = "bearer:TOKEN"

[[source]]
url   = "https://cdn.example.com/blobs/a3f1b2/payload.bin"
scope = "file:0"
mode  = "exact"

[[source]]
url      = "https://odd.example.com/store/{raw:path}?v=2"
scope    = "file:3-9"
mode     = "template"
headers  = { Authorization = "Basic ..." }
```

Every field in that table is read and applied: `url`, `scope`, `mode`,
`template`, `style`, `priority`, `concurrency`, `connections`, `chunk_size`,
`rate_limit`, `timeout_ms`, `connect_timeout_ms`, `retries`, `max_errors`,
`cooldown_ms`, `user_agent`, `headers`, `auth`, plus a `[default]` block that
supplies any of them to every source that does not override it.

`template` mode is what handles a server that lays the payload out differently
from the torrent. Eleven placeholders: `{name}` `{path}` `{filename}`
`{index}` `{piece}` `{offset}` `{length}` `{end}` `{piece_offset}`
`{piece_length}` `{infohash}`, percent-encoded unless written `{raw:path}`.

### The three transports already run at once

Peers, the torrent's own `url-list`, and command-line sources are all live in
the same run, and the report keeps them apart:

```json
"from_web_seeds": { "bytes": 19922944, "human": "19.00 MiB" },
"from_peers":     { "bytes": 47185920, "human": "45.00 MiB" }
```

Per source it also reports `http_bytes` beside `bytes`, so fetching the same
range twice is visible as an amplification ratio rather than hidden.

---

## Part 2: the gaps

### T-130 A source cannot be told which statuses are worth retrying

Category:    webseed
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `classify_status` in `webseed/fetch.rs:1192-1211` makes 401, 403,
             404, 410, and 416 permanent, and a permanent failure retires the
             source for the run. A CDN that signs URLs answers 403 when a
             signature expires, and the next request to the stable URL would
             redirect to a fresh signature and succeed.
Relevance:   This is the one thing standing between Scenario 1 and working
             unattended for longer than a signature lasts. It is also
             Scenario 4's "override the status code as some servers return
             different codes even though the content exists".
Approach:    A per-source status policy, on the command line and in the table:

                 --web-seed-retry-status 403,429,503
                 --web-seed-fatal-status 404

             `retry_status` moves a code from permanent to transient, and
             `fatal_status` moves one the other way. Both take a list of codes
             and ranges. The table gets `retry_status` and `fatal_status`
             arrays per source and in `[default]`.

             The existing per-source `retries`, `max_errors`, and `cooldown_ms`
             then bound it, so a source whose signature cannot be refreshed
             still retires rather than looping. Nothing new is needed for the
             backoff.
Acceptance:  The fixture below, driven end to end. A source that answers 403
             after N requests completes the payload with
             `--web-seed-retry-status 403` and fails without it, and the run
             reports how many retries each status cost.

             Measured today, without the flag, against
             `loopback-fileserver --status 403 --fail-after 6`:

             ```
             exit=1 completed=0 failed=1 stopped=failed
             downloaded=5.00 MiB of 64.00 MiB
             warning: web seed .../cdn/a3f1b2c4-signed-blob.dat is unusable:
               403 Forbidden, check --web-seed-auth and --web-seed-header
             ```
Closed:      `--web-seed-retry-status` and `--web-seed-fatal-status` take codes
             and inclusive ranges (`403`, `403,429`, `500-599`). The table
             takes `retry_status` and `fatal_status` per source and in
             `[default]`, as integers, as range strings, or as one string. A
             code in both lists is a usage error rather than a precedence rule,
             because there is no defensible answer and picking one silently
             hides the mistake. `webseed list` prints both when they are set,
             so the policy is checkable before any bytes move.

             `bit-cli download --json` now carries `retries` and
             `retries_by_status` per source, and the text output prints
             `retries 22 (22 on 403)` when there were any.

             Acceptance, `pwsh -NoProfile -File scripts/check-signed-source.ps1`
             at 64 MiB. The pair that carries it is `expiring_default` and
             `expiring_retry`: the same server, the same window, differing only
             in the flag.

             ```
             expiring_default    1 0 B        -         1   1       0 yes
             expiring_retry      0 64.00 MiB  matches  86  22      22 yes
             ```

             `fatal_override` and `recovering_503` are the other direction and
             its control: `--status 503 --fail-after 4 --recover-after 8`
             completes with no policy, because 503 is already transient, and
             fails with `--web-seed-fatal-status 503`.

**Two defects turned up while building the acceptance, and the second one is
larger than this entry.**

**One.** The bridge retired a source on the first request that ran out of its
own retries, whatever the classification. `--web-seed-max-errors` could
therefore never be reached: one exhausted request ended the source before a
second error could be counted. `crates/bit-cli-core/src/webseed/bridge.rs`
carried the reason for it as a comment, "the fetcher already retried what was
worth retrying, so anything surfacing here means the source is done", and that
is true of a permanent failure and false of a transient one.

It showed up as the `recovering_503` control failing: a mirror that answers 503
for four requests and then serves normally killed the source, with **no flag
set at all**. That is not a status policy problem, it is the default path, and
it means every mirror that restarted mid-download was lost.

Fixed. A block failure now carries whether the source could still answer, and a
transient one reconnects like a link failure instead of retiring. What bounds
the loop is `--web-seed-max-errors` consecutive failed requests tripping the
source's cooldown, which the bridge reads and retires on. Measured: the same
control now completes 64 MiB with 6 retries.

**Two.** `--web-seed-cooldown` sets a timer nothing waits out. A source whose
budget runs out is retired for the rest of the run rather than sitting out the
cooldown and coming back, so the flag moves no number. That is
[T-137](#t-137-a-cooled-down-source-never-comes-back), open, with the
trade-off named. The two doc comments that implied a source returns now say
what happens instead.

### T-131 The loopback file server cannot simulate a signed URL

Category:    bench
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `crates/bit-cli-core/examples/loopback-fileserver.rs` has
             `--ignore-range`, `--status`, `--stall-after`, `--fail-after`, and
             `--no-keep-alive`. It cannot redirect, and it cannot expire a
             signature, so neither half of Scenario 1's hard case can be
             tested end to end.
Relevance:   [T-130](#t-130-a-source-cannot-be-told-which-statuses-are-worth-retrying)
             cannot be accepted without it, and rule 3 says nothing counts as
             proven until it has run against real infrastructure. A local
             server that behaves like the real one is the closest thing
             available for a CDN nobody here controls.
Approach:    Three flags on the example server:

             - `--sign-redirect <SECONDS>`: a request to a stable path answers
               302 to the same path with `?sig=<random>&exp=<unix>`, valid for
               that many seconds.
             - `--require-sig`: a request carrying no `sig`, or an expired
               one, answers 403.
             - `--redirect-chain <N>`: N hops before the payload, for the
               redirect-following test.

             Keep it a single-file example with no new dependency, the way it
             is now.
Acceptance:  `--sign-redirect 2 --require-sig` serves a 64 MiB payload to
             `bit-cli download` over more than two seconds, so at least one
             signature expires mid-run, and the download completes with the
             right hash once [T-130](#t-130-a-source-cannot-be-told-which-statuses-are-worth-retrying)
             is in.
Closed:      All three flags are in, plus a fourth,
             `--recover-after <M>`, which ends a `--fail-after` window after M
             requests so a status that recovers can be produced without a
             clock. Eleven unit tests cover the routing and the signature
             check, and `[[example]] test = true` in
             `crates/bit-cli-core/Cargo.toml` puts them in
             `cargo test --workspace`.

             The signature is SplitMix64 over a per-process secret and the
             window index, so it is unguessable from the URL and stable for
             the length of its window. `exp` is unix milliseconds rather than
             seconds, because a window can be shorter than a second and the
             measurement below needs it to be.

             The server on its own, checked with `curl` against a 1 MiB
             payload under `--sign-redirect 2 --require-sig`:

             ```
             GET /blob.bin        -> 302 .../blob.bin?sig=cc11a1..&exp=1787216384041
             GET /blob.bin?sig=.. -> 206  (immediately)
             GET /blob.bin?sig=.. -> 403  (two seconds later, signature expired)
             GET /blob.bin        -> 206  (the stable path, redirected to a fresh signature)
             ```

             That last pair is exactly what
             [T-130](#t-130-a-source-cannot-be-told-which-statuses-are-worth-retrying)
             describes: a real 403, and a stable URL that recovers.

             `pwsh -NoProfile -File scripts/check-signed-source.ps1` drives all
             six cases end to end and is the acceptance for this entry and for
             [T-130](#t-130-a-source-cannot-be-told-which-statuses-are-worth-retrying).
             It lands with T-130, because two of its six cases need that flag.
             The script has since grown to nine: the last three are
             [T-137](#t-137-a-cooled-down-source-never-comes-back)'s and use
             `--down-for`, which was added there. At 64 MiB, all six as
             described:

             ```
             case             exit downloaded hash    302 403 retries ok
             redirects           0 64.00 MiB  matches 256   0       0 yes
             too_many_hops       1 0 B        -        11   0       0 yes
             expiring_default    1 0 B        -         1   1       0 yes
             expiring_retry      0 64.00 MiB  matches  86  22      22 yes
             fatal_override      1 4.00 MiB   -         0   0       0 yes
             recovering_503      0 64.00 MiB  matches   0   0       6 yes
             ```

**The acceptance as written above cannot fire, and that is the finding.**
`--sign-redirect 2 --require-sig` serves a 64 MiB payload with **zero** 403s,
because `bit-cli` re-resolves the stable URL on every ranged request and the
signature it is handed is a millisecond old when it uses it. A signature
expires mid-run only when the window is shorter than the round trip from the
`302` to the request that carries it. Measured, 64 MiB in 1 MiB chunks against
`--sign-redirect W --require-sig`:

| window | 302 | 206 | 403 | exit |
| --- | --- | --- | --- | --- |
| 2s | 64 | 64 | 0 | 0 |
| 0.1s | 64 | 64 | 0 | 0 |
| 0.01s | 26 | 19 | 7 | 1 |
| 0.002s | 1 | 0 | 1 | 1 |

So the entry's premise held for the wrong reason. The half it was written to
test, "a CDN whose stable URL 302s to a freshly signed URL each time is handled
by doing nothing", is now proven rather than asserted: the `redirects` case
above answers 256 redirects for 64 requests, four hops each, and completes with
the payload byte for byte. A client that pinned a resolved URL would show one
redirect for the whole run.

`scripts/check-signed-source.ps1` runs at `-Window 0.01` for that reason, and
says so in the report's `notes`.

### T-132 The swarm cannot be rate limited separately from HTTP sources

Category:    performance
Priority:    P1
Effort:      M
Status:      **done**, 2026-08-22T17:55Z

Problem:     `--max-download-rate` goes into the session's `LimitsConfig`, and
             HTTP sources reach the session as peers over loopback, so a
             session cap applies to both. `--web-seed-speed-limit` caps HTTP
             sources only. There is no way to cap peers only.
Relevance:   Scenario 5 asks to "cap/limit speed/connection per method". Two
             of the three directions exist; the third does not, and the
             asymmetry is not documented.
Approach:    The bridge already has a token bucket per source
             ([T-035](performance.md)). A peer-side cap needs either the
             session cap set to the peer budget and the bridge exempted, which
             it cannot be because the bridge is a peer, or an accounting split
             the session does not expose.

             What is likely to work: leave the session cap off and derive it.
             When both `--max-peer-rate` and a web seed cap are set, set the
             session cap to their sum and hold each side to its own bucket.
             The web seed bucket already exists; the peer side would be the
             session cap minus what the buckets are allowed.

             Measure before committing to that. The first question is whether
             the session cap is even enforced, which is
             [T-031](performance.md), still open.
Acceptance:  A hybrid run with `--max-peer-rate 10MiB/s --web-seed-speed-limit
             50MiB/s` reports peer bytes within 10% of 10 MiB/s and HTTP bytes
             within 10% of 50 MiB/s over sixty seconds, both from the same
             report, plus the same run with each cap alone.

**Partial, 2026-08-22. The premise holds, the workaround does not, and the
documentation half is done.**

The Approach says to measure before committing, and names
[T-031](performance.md) as the first question. That is stale: T-031 closed, the
session cap is enforced. The question worth asking instead is whether the
session cap reaches the bridge, which is the sentence the Problem asserts.

`bench/rate-scope-20260822T061715516Z.json`, one 128 MiB payload, one mirror,
one seeder, an 8 MiB/s cap:

| phase | total | HTTP | peers |
| --- | --- | --- | --- |
| `http_ceiling` | 195.42 MiB/s | 195.42 | 0 |
| `http_session_cap` | **8.41 MiB/s** | 8.41 | 0 |
| `http_webseed_cap` | 8.23 MiB/s | 8.23 | 0 |
| `hybrid_ceiling` | 354.57 MiB/s | 138.50 | 216.07 |
| `hybrid_webseed_cap` | **35.96 MiB/s** | 1.40 | 34.55 |
| `hybrid_session_cap` | 8.27 MiB/s | 4.14 | 4.14 |

```powershell
pwsh -NoProfile -File scripts/check-rate-scope.ps1
```

**The premise is right.** `--max-overall-download-rate 8MiB/s` takes HTTP from
195 MiB/s to 8.41, so the session limiter does bound the bridge, and there is
no cap that reaches peers without also reaching HTTP.

**Why, with a line number.** `librqbit`'s download limiter is acquired in the
peer's own request loop, once per outgoing `Request`, against the torrent's
limiter and then the session's: `torrent_state/live/mod.rs:1698-1706`. The
bridge is a peer the session requests blocks from, so its requests pass through
the same two calls. `LimitsConfig` has exactly two fields, `upload_bps` and
`download_bps` (`limits.rs:11`), and nothing anywhere is scoped to a peer or a
connection. **That is the blocker**: a cap that excludes one peer cannot be
expressed. What would unblock it is `prepare_for_download` taking the peer it
is throttling, or a `LimitsConfig` on the connection.

**The workaround in the Approach does not survive the measurement.** It
proposes setting the session cap to the sum of a peer cap and a web seed cap
and holding each side to its own bucket, so peers get the session cap minus
what HTTP took. That bounds the peer share only while HTTP is taking its whole
bucket, and `hybrid_webseed_cap` is what happens when it is not: HTTP ran at
**1.40 MiB/s against an 8 MiB/s cap** because the peer was faster and the
picker gave HTTP little to do, and the run reached 35.96 MiB/s. Under that
arrangement peers would have been handed the whole unused remainder. A
`--max-peer-rate` that holds only when the mirror is saturated is a flag that
lies in the common case, so it is not built.

**What is done: the asymmetry is documented**, which is what the Relevance line
says is missing. `README.md` has the table above under
[Capping one source and not the other](../README.md#capping-one-source-and-not-the-other),
and `scripts/check-rate-scope.ps1` is the acceptance that keeps it true. The
caps in it are judged and the splits between two sources are recorded, because
a split is a scheduling outcome and [RULES.md](RULES.md) section 5 says a
fixture must not assert one. `hybrid_webseed_cap` is judged in both directions:
HTTP stays under its cap, and the run as a whole does not. If that second
assertion ever fails, a peer cap has appeared and this entry is closeable.

---

**Closed 2026-08-22 in the vendored tree.** The section above ends by naming
what would unblock it: "`prepare_for_download` taking the peer it is
throttling". That is what was built.

**`--max-peer-rate RATE`.** A download cap that bounds swarm peers and not an
HTTP source this process attached. `librqbit`'s `Limits` grows a second
download limiter beside the total, plus a list of peer id prefixes it does not
apply to; `bit-cli` registers its own bridge's prefix, `-BCws01-`, whether or
not a cap is set. `prepare_for_download_from` charges the total for everyone
and the peer limiter for everyone else.

**A prefix rather than an address**, because the bridge dials in from an
ephemeral port and reconnects on a new one, so there is no stable address to
name. A prefix rather than a whole id, because it generates a fresh id per
connection and only the first eight bytes say who it is.

**There is no upload counterpart and that is not an oversight.** The bridge is
a seed: it sends `Bitfield` and `Unchoke` and answers `Request`, and never
sends `Interested` and never requests. Nothing is ever uploaded to it, so
`--max-upload-rate` and `--max-overall-upload-rate` already reach peers alone.

**The first attempt did not work, and what it found is now
[T-210](peers.md).** The exemption matched nothing, because `librqbit` filed
every **incoming** peer under this session's own peer id:
`manage_peer_incoming` handed the handshake it had just built to send to
`on_handshake` instead of the one it read. The bridge dials in, so it was
exactly the case that took the wrong path. That is a P1 of its own, fixed, with
its own entry.

**Measured**, `scripts/check-rate-scope.ps1`, ten phases,
`bench/rate-scope-20260822T175543220Z.json`:

| phase | total | HTTP | peers |
| --- | --- | --- | --- |
| `http_ceiling` | 167.32 MiB/s | 167.32 | 0 |
| `http_session_cap` | 8.39 MiB/s | 8.39 | 0 |
| `http_webseed_cap` | 8.21 MiB/s | 8.21 | 0 |
| **`http_peer_cap`** | **151.84 MiB/s** | **151.84** | 0 |
| `peer_ceiling` | 259.11 MiB/s | 0 | 259.11 |
| **`peer_peer_cap`** | **8.42 MiB/s** | 0 | **8.42** |
| `hybrid_ceiling` | 228.16 MiB/s | 185.38 | 42.78 |
| `hybrid_webseed_cap` | 301.89 MiB/s | 11.79 | 290.09 |
| `hybrid_session_cap` | 8.35 MiB/s | 3.91 | 4.43 |
| **`hybrid_both_caps`** | 27.57 MiB/s | **18.31** | **9.26** |

```powershell
pwsh -NoProfile -File scripts/check-rate-scope.ps1
```

`http_peer_cap` is the row the whole entry turns on, and it is judged in the
direction that would be a defect: an 8 MiB/s peer cap must **not** hold an
attached HTTP source, and it ran at 151.84 MiB/s. Before [T-210](peers.md) it
ran at 8.40 MiB/s, which is the cap.

**The acceptance is not taken literally, and the reason is a rule this
repository adopted after the acceptance was written.** It asks for "peer bytes
within 10% of 10 MiB/s and HTTP bytes within 10% of 50 MiB/s". The upper half
is a cap and is judged. The lower half asks each source to be **at** its cap,
which is a scheduling outcome: the picker decides how much each source is asked
for, and [RULES.md](RULES.md) section 5 forbids a fixture asserting one. It is
arranged instead, which is what that rule says to do: `peer_peer_cap` and
`http_peer_cap` each make one source the only supplier, so "the cap binds
peers" and "the cap does not bind HTTP" are invariants rather than races.
`hybrid_both_caps` then shows both caps in one run and one report, each side
under its own, which is the rest of what the acceptance asked for.

**A cap is now judged as rate plus burst over the run's own length**, and that
fixed a latent flake rather than loosening anything. `governor`'s
`Quota::per_second(n)` refills n a second and holds n, so a run of t seconds
may pass `n * t + n` bytes: 16% over at four seconds and 2% over at sixty. The
old plain-rate ceiling passed `hybrid_webseed_cap` at 1.12 MiB/s on one run and
would have failed the same limiter at 11.79 MiB/s on the next, because how long
that phase lasts is decided by the uncapped peer. The entry's "over sixty
seconds" was asking for the same thing by making the burst small; this says it
without needing the run to last that long. `-PayloadMiB` lengthens the window
for anyone who wants both.

**What is still true from the section above.** A session cap still bounds
everything including HTTP, which is what `--max-overall-download-rate` means,
and `README.md`'s table still describes it.

### T-133 Two torrents holding the same file cannot share its bytes

Category:    webseed
Priority:    P1
Effort:      L
Status:      **done**. Layers 1 and 2 closed here, and layer 3 closed under
             [T-140](#t-140-a-proven-shared-file-is-not-turned-into-a-source-on-its-own)

Problem:     Scenario 2. Three torrents with different info hashes each contain
             a bit-identical `file.blob`. One is 60% done and stalled, one is
             slow, and one is slow but carries a fast web seed. Nothing in
             `bit-cli` connects them: there is no cross-torrent identity, no
             shared content store, and a source URL must be `http` or `https`,
             so a completed copy on the local disk could not be named as a
             source either:

             ```
             $ bit-cli webseed list t.torrent --web-seed-for 'file:0=file:///C:/path/file.blob'
             error: only http and https sources are supported
             ```

             That last part is fixed: see **Layer 1** below.
Relevance:   It is the difference between downloading the same 64 MiB once and
             downloading it three times, which is Scenario 5's "minimize
             bandwidth usage" in its sharpest form.
Approach:    Three layers, and the first is worth doing on its own:

             1. **A local source.** Accept `file:` URLs as a source with a
                scope, reading ranges out of a local path. Then Scenario 2 is
                a two-step the operator can drive today: finish `file.blob`
                under torrent C, then point torrents A and B at it with
                `--web-seed-for 'file:0=file:///...'`. Effort S, and it also
                serves "I already have this file somewhere".
             2. **Declared equivalence.** `--same-file
                'HASH_X:file:0=HASH_Y:file:3'` or a table, asserting two
                torrents' files are identical. `bit-cli` verifies the claim
                per piece before trusting it, because a wrong assertion would
                otherwise corrupt a payload silently. Verification is possible
                only where the two torrents' piece boundaries align on that
                file; where they do not, the claim can still be checked once
                the bytes are complete.
             3. **Derived equivalence.** Same length and same piece hashes over
                the aligned range implies the same bytes with no assertion
                from the caller. This is the one that makes Scenario 2 need no
                flags at all, and it only works when the piece length and the
                file's offset within the torrent line up.
Acceptance:  Layer 1: a `file:` source completes a torrent with no network at
             all, and the payload hashes equal. Layer 2 and 3: three torrents
             built from one payload with different piece lengths and different
             surrounding files, added in one invocation, and the report shows
             the shared file's bytes fetched once and written into all three
             output directories, with all three hashing equal.
Layer 1:     **done.** A source URL may be `file:`, and everything else about a
             source still applies to it: scope, composition, chunk size, rate
             limit, retries, per-piece verification, per-source accounting, and
             the same loopback bridge. `crates/bit-cli-core/src/webseed/local.rs`
             is the whole of the URL handling and the positioned read;
             `Fetcher::fetch_once` branches on the scheme and nothing above it
             changes.

             `webseed list`, `webseed test`, `webseed probe`, and
             `bench webseed` all take a `file:` source. `test` reports the
             length off the filesystem and `range_support: yes` without asking,
             because a positioned read always works. `probe` and `bench` read
             the same windows at the same concurrencies, so a local source gets
             the same curve an HTTP one does.

             `pwsh -NoProfile -File scripts/check-local-source.ps1` is the
             acceptance. Six cases at 64 MiB, no server and no bound port:

             ```
             case        exit downloaded hash    ok
             exact          0 64.00 MiB  matches yes
             auto           0 68.00 MiB  matches yes
             shared_a       0 64.00 MiB  matches yes
             shared_b       0 64.00 MiB  matches yes
             wrong_bytes    1 0 B        -       yes
             missing        1 0 B        -       yes

             the shared file landed with 1 distinct hash across three info hashes
             ```

             `shared_a` and `shared_b` are Scenario 2's two-step: torrent C
             finishes `file.blob` from the CDN copy, then torrents A and B read
             the copy C wrote. Three info hashes, three piece lengths (2 MiB,
             1 MiB, 512 KiB), one 64 MiB payload fetched once, four copies
             hashing equal.

             `wrong_bytes` is the case that says the source is not trusted: a
             file of exactly the right length holding something else is refused
             by the per-piece check with the path and the piece named. Only
             that check can catch it, and it is the default.

             A `..` in a resolved path is refused. `auto` and `prefix`
             composition append the torrent's own `name` and `path`, so the
             tail of a source URL is written by the `.torrent` rather than by
             the caller, and a hostile one naming `../../../Windows/win.ini`
             would otherwise read out of a directory the caller did not name.
             The bytes would fail their piece hash, but reading them at all is
             not this tool's business.

             Two things layer 1 does not do, both of which are layers 2 and 3.
             A `--web-seed-for` binding applies to every torrent in the
             invocation, so `-j 2` over torrents A and B needs the shared file
             to be at the same index in both; it is index 0 in A and index 1 in
             B, so the two need separate invocations. And nothing derives the
             equivalence: the caller names the path.

Layer 2:     **done.** Scenario 2 is one invocation.

             Two things were missing and both are small. A binding applies to
             every torrent in the invocation, and `file.blob` is index 0 in A
             and C and index 1 in B, so `--web-seed-for` now takes an optional
             info hash prefix: `<40 hex>:file:N=URL`. The rule is mechanical,
             exactly forty hexadecimal characters followed by a colon, and a
             hash naming no torrent in the run is a usage error rather than a
             binding that quietly does nothing. The binding table takes the
             same thing as a `torrent` field on a `[[source]]`.

             The second was ordering. `-j 1` ran the sources one at a time but
             not in the order they were given: every plan was its own task
             queuing on a semaphore, and which task reached the semaphore first
             was up to the runtime. The plans are now a queue taken by a fixed
             pool of workers, so `-j 1` is a sequence a caller can depend on,
             which is what lets torrent A read the file torrent C writes.
             `sources_start_in_the_order_they_were_given` is the test.

             The `one_invocation` case of `scripts/check-local-source.ps1` is
             the acceptance, and it is the acceptance sentence run as one
             command:

             ```bash
             bit-cli download C.torrent A.torrent B.torrent --dir out -j 1 \
               --web-seed-only --no-torrent-web-seed --web-seed-mode exact \
               --web-seed-for '<HASH_C>:file:0=file:///cdn/a3f1b2c4-signed-blob.dat' \
               --web-seed-for '<HASH_A>:file:0=file:///out/payload_c/a/b/c/file.blob' \
               --web-seed-for '<HASH_B>:file:1=file:///out/payload_c/a/b/c/file.blob'
             ```

             ```
             sources_from_cdn 1
             distinct_hashes  1
             from_web_seeds   192.00 MiB
             from_peers       0 B
             from_resume      12.00 MiB
             elapsed_ms       930
             ```

             Exactly one source read the CDN copy, which is what "fetched
             once" means when the CDN is a real one: the other two read what
             torrent C wrote. Three info hashes, three piece lengths, one 64
             MiB payload, one distinct hash across all three output
             directories.

             The declaration is verified rather than trusted. Every piece a
             `file:` source serves is hash-checked against the torrent that
             asked for it before the session sees it, which is what the
             `wrong_bytes` case measures with a file of exactly the right
             length. So a wrong `--web-seed-for` costs a failed source, not a
             corrupt payload, and no separate verification step is needed for
             the assertion this entry proposed.

             Closing it found [T-139](#t-139-a-resumed-download-charges-its-existing-bytes-to-the-swarm):
             the three pre-placed files were reported as coming from peers on a
             run with `--web-seed-only`, which disables peers.

Layer 3:     **the detection is done, and the answer for this fixture is that
             nothing is provable.**

             `bit_cli_core::equivalence` decides whether two files in two
             torrents are the same from the metadata alone, and
             `bit-cli files <TORRENT> --against <OTHER>` reports it.

             The rule the entry states in the abstract has a consequence it
             does not draw. A `.torrent` hashes fixed-size pieces of the whole
             payload rather than of each file, so two files can be compared by
             hash only where the pieces cover the same bytes of each. That
             needs the same piece length, because a 2 MiB hash and a 1 MiB hash
             are hashes of different amounts of data, and the same offset
             modulo it, or piece k of one covers different bytes of the file
             than piece k of the other.

             **The three-torrent fixture is built from three different piece
             lengths, which is exactly the case where that cannot hold.** Run
             against it, nothing is proven:

             ```
             $ bit-cli files torrent_a.torrent --against torrent_b.torrent \
                 --against torrent_c.torrent

             INDEX  EVIDENCE  PROVEN  OTHER       OTHER PATH
             0      length    -       c2806b5a:1  media/file.blob
             0      length    -       31084dc6:0  a/b/c/file.blob
             1      length    -       31084dc6:1  a/extra.bin
             2      length    -       c2806b5a:2  notes/changelog.txt
             ```

             Four candidates and zero proofs, and **two of the four are not the
             same bytes at all**: `deep/other.bin` against `a/extra.bin` and
             `readme.txt` against `notes/changelog.txt` are equal in length and
             nothing else. That is why the evidence is reported rather than a
             yes or a no, and it is what a length-only heuristic would have got
             wrong.

             Against a pair built to line up, the same code proves the whole
             file:

             ```
             INDEX  EVIDENCE      PROVEN     OTHER       OTHER PATH
             0      piece-hashes  64.00 MiB  c3dabcae:0  file.blob
             ```

             Both halves are the `equivalence` case of
             `scripts/check-local-source.ps1`, which requires zero proofs
             across the fixture and exactly one proof of exactly the shared
             file's length across the aligned pair. Seven unit tests in
             `equivalence::tests` cover the rest: a differing hash is an answer
             rather than a missing proof, a differing length is not a match at
             all, and a file shorter than one piece can only ever be a
             candidate.

             Layer 3 is turning a proof into a source automatically, which is
             a scheduling question rather than an identity one: the donor
             torrent has to finish the file before the others can read it.
             `-j 1` and an explicit binding did that first, which is what layer
             2 closed. Doing it with the caller naming nothing is
             [T-140](#t-140-a-proven-shared-file-is-not-turned-into-a-source-on-its-own),
             which is done: the run computes the proofs before it starts and
             gives each torrent a `file:` source per file an earlier one has
             already written. Above `-j 1` nothing has finished yet and nothing
             is donated, which is [T-143](#t-143-a-source-cannot-be-attached-to-a-torrent-that-has-already-started).

### T-140 A proven shared file is not turned into a source on its own

Source:      came out of closing T-133 layer 2
Category:    webseed
Priority:    P2
Effort:      M
Status:      **done**

Problem:     `bit-cli files --against` proves two torrents hold the same file,
             and `--web-seed-for '<HASH>:file:N=file:///...'` uses one torrent's
             copy in another. Nothing joins the two: the caller reads the proof
             and writes the binding.
Relevance:   [T-133](#t-133-two-torrents-holding-the-same-file-cannot-share-its-bytes)
             layer 3 asks for Scenario 2 with no flags at all. What ships needs
             three flags and the output directory known in advance.
Approach:    Two pieces, and only the second is hard.

             1. **Compute the bindings.** For every pair of torrents in one
                invocation, run `equivalence::matches`, keep the proofs, and
                pick a donor per equivalence class: the torrent that already
                has the most of that file on disk, or the first one given.
                That is a few lines against the code that exists.
             2. **Attach a source to a torrent that has already started.**
                Sources attach in `one_inner` before `watch` runs, so a
                receiver torrent would need its source when the donor
                completes, not when the run starts. Either the receiver waits
                for the donor, which `-j 1` already does and which serialises
                a run that did not have to be, or `watch` learns to attach a
                source mid-run.

             The second one is the design decision. A source attached mid-run
             is a new bridge and a new peer in a live session, which the engine
             supports; what does not exist is the plumbing to build one after
             `attach_sources` has returned, and the accounting to fold it into
             a report that has already reported its sources.
Acceptance:  Three torrents built from one payload **with the same piece
             length** and different surrounding files, added in one invocation
             with no `--web-seed-for` at all, and the report shows the shared
             file's bytes fetched once, written into all three output
             directories, with all three hashing equal and the report naming
             which torrent donated it.

**Done by the first piece, and the second is not needed for the acceptance.**

`bit-cli download` with several torrents computes the equivalence between every
pair before the session starts, keeps the proofs, and gives each torrent a
`file:` source per proven file that an earlier torrent in the run has already
written. No flag, no path, no info hash typed by anyone.
`--no-share-files` turns it off.

**The acceptance, 2026-08-20T17:03:27.094Z**, in
`bench/shared-files-20260820T170327094Z.json`:

```
$ pwsh -NoProfile -File scripts/check-shared-files.ps1
```

```
torrent   finished over http from disk resumed  shared proven    hash
payload_c     True 20.00 MiB 20.00 MiB 0.00 B        0 0.00 B    42ee6db050db50ce
payload_a     True 0.00 B    16.00 MiB 3.00 MiB      1 16.00 MiB 42ee6db050db50ce
payload_b     True 0.00 B    16.00 MiB 3.00 MiB      1 16.00 MiB 42ee6db050db50ce

shared file:  16.00 MiB, sha256 42ee6db050db50ce...
over http:    20.00 MiB for the whole run
distinct hashes across three output directories: 1
```

Three info hashes, one piece length, the shared file at a different path and a
different index in each, one invocation, exit 0 in 511 ms. Torrent C fetched
its 16 MiB shared file and its own 4 MiB extra from the mirror. A and B read
the shared file off C's finished copy: **zero bytes over HTTP**, 16 MiB from a
source, 3 MiB already on disk, and a `shared` row naming
`a0f16220418c110ee3b5dba0a689c2c1b4791ca5` as the torrent it came from. One
distinct hash across the three output directories.

**Why the whole file is safe when only part of it is proven.** A proof covers
the whole pieces lying entirely inside the file, and the donated source is
scoped to the whole file. Those are the same set. A bridge advertises only the
pieces its scope covers *in full*, and a piece lying entirely inside the file
is exactly a piece the proof compared, so nothing the source can serve is
unproven. The bytes at the ends of the file, in pieces shared with the file
before or after it, are never offered. On top of that the source is checked per
piece on the way in like every other source, so a proof that was somehow wrong
costs a retry rather than a corrupt payload.

**Four decisions worth naming.**

- **Proof only.** `Evidence::Length` says two files are the same size, which is
  what the `equivalence` module exists to stop anyone acting on. Only
  `Evidence::PieceHashes` donates.
- **The earliest torrent donates.** The entry suggested "the one that already
  has the most of that file on disk". The first one given is what shipped: it
  is the one most likely to have finished by the time a later one starts, and
  it makes the choice a function of the command line rather than of the order
  things happened to complete. Which files a torrent has on disk is not known
  until its hash check has run, which is after the plan is needed.
- **A donation is only a donation once the donor has finished.** A `file:`
  source over a half-written file serves bytes that are not there. The donor
  publishes its output paths when it completes, so under `-j 1` a later torrent
  finds them and above `-j 1` it does not. That is the honest behaviour rather
  than a race, and it is the residue below.
- **It is on by default.** The acceptance says "no `--web-seed-for` at all",
  and a flag the caller has to know about is the thing this entry exists to
  remove. `--no-share-files` is there for a caller who wants every torrent
  fetched independently.

Two tests, no network and no ports:
`a_proven_shared_file_is_read_from_the_torrent_that_holds_it` drives a donor
complete on disk and a receiver missing only the shared file, and asserts the
bytes, the pieces compared, the origin, and that both copies are identical.
`no_share_files_leaves_the_receiver_with_nothing_to_fetch_from` is the same run
with the flag, where the receiver cannot finish at all, which is what says the
flag moves a number.

**What is left, and it is the second piece.** Above `-j 1` the torrents are in
flight together, so nothing has finished and nothing is donated: the run
behaves exactly as it did before. Making that work needs `watch` to attach a
source to a live torrent, which is the scheduling change this entry priced. It
is recorded as [T-143](#t-143-a-source-cannot-be-attached-to-a-torrent-that-has-already-started)
rather than left inside a closed item.

`scripts/make-scenario-fixture.ps1` grew two parameters for this: `-PieceLength`
gives all three torrents one piece length instead of three, which is what makes
the shared file provable from the metadata, and `-WebSeed` puts a real URL in
torrent C's url-list so the mirror can be on a port the OS chose.

### T-143 A source cannot be attached to a torrent that has already started

Source:      the residue of [T-140](#t-140-a-proven-shared-file-is-not-turned-into-a-source-on-its-own)
Category:    webseed
Priority:    P2
Effort:      M
Status:      **done**, 2026-08-22T02:00Z

Problem:     Sources attach in `cmd::download::one_inner` before `watch` runs,
             and nothing can add one after that. So a source that becomes
             usable during a run, which is what a donated file is above
             `-j 1`, is not used at all.
Relevance:   [T-140](#t-140-a-proven-shared-file-is-not-turned-into-a-source-on-its-own)
             works under `-j 1` and does nothing above it, which serialises a
             run that did not have to be serialised. The same plumbing is what
             [T-005](webseed.md) needs to re-scope a source mid-run, and what a
             Metalink resolved after the start would need.
Approach:    `swarm::attach_sources` builds the bridges, the bitfields, and the
             accounting rows in one pass and returns them. Splitting it into
             "attach one" and "attach these" is most of the work; the rest is
             the report, which currently reads `sources` once at the end and
             would have to tolerate a row that appeared partway through, and
             the `source_added` event, which already carries everything a late
             source would need to say.
Acceptance:  Three torrents holding one file, added in one invocation with
             `-j 3`, and the second and third read the file from the first as
             soon as it finishes rather than fetching it. The same
             `scripts/check-shared-files.ps1` with `-j 3` reports one fetch.

**Somebody else has written the acceptance test for this, and its premise is
this entry's premise.** `torrent/tests/add-webseed-after-priorities/` is an
integration test whose whole point is attaching a source to a torrent that has
already started: `herp_test.go:80-84` calls `DownloadAll()`, sleeps a second,
and **then** calls `AddWebSeeds(["http://localhost:3003/test.img"])`. Its
`README` states the acceptance condition in two clauses, and the second is the
one worth adopting here: "The seeder should start fetching from HTTP, despite
the webseed being added after `Torrent.DownloadAll` is called. **It should
still fetch even if the leecher does not connect**", which is `bit-cli`'s own
`--web-seed-only` case, applied to a late attachment. A late source that only
works when a peer happens to be present is a source that works by accident.

The fixture is a 500 MiB sparse `test.img` served by Python's
`rangehttpserver` on port 3003 with the `.torrent` committed, which is a
smaller version of this repository's own `loopback-fileserver` example, so the
rig costs nothing to reproduce.

`torrent/tests/webseed-partial-seed/` is the same rig used for a different
property and it is a warning rather than a template. From anacrolix
discussion 916: the seeder and leecher must progress completed pieces in lock
step, because the bug was that **the leecher reached the end of its maximum
unverified-bytes window before hitting a piece the seeder had available**, and
deadlocked. `torrent/internal/request-strategy/NOTES.md:14` gives that window
as 64 MiB by default. `bit-cli` has a bounded per-source window cache that
[T-041](memory.md) says is bounded but not measured, and this is the deadlock
that bound can cause once sources appear and disappear mid-run. Attaching a
source late is exactly the case that makes the window and the availability
disagree, so measure it here rather than discovering it under `-j 3`.

**Closed 2026-08-22T02:00Z, and the failure above `-j 1` was worse than the
entry says.** The entry has the takers fetching the shared file rather than
reading it. They do not fetch it: they have no source at all, so they do not
finish.

Measured first, before anything was built. `scripts/check-shared-files.ps1`
grew a `-Jobs` parameter, which is what the acceptance needed anyway, and at
`-Jobs 3` against `76e33e8`'s behaviour:

```
torrent   finished over http from disk resumed  shared proven hash
payload_c     True 20.00 MiB 20.00 MiB 0.00 B        0 0.00 B 42ee6db050db50ce
payload_a    False 0.00 B    0.00 B    3.00 MiB      0 0.00 B 080acf35a507ac98
payload_b    False 0.00 B    0.00 B    3.00 MiB      0 0.00 B 080acf35a507ac98

check-shared-files: the run exited 9, not 0
distinct hashes across three output directories: 2
```

Report: `bench/shared-files-20260822T014247442Z.json`, `ok: false`, fourteen
failures. The donor fetched its 20 MiB and the two takers sat on 3 MiB of
resumed data for the whole run. Exit 9 is no usable sources, which is the
honest code for what happened.

After, same script, same fixture, `-Jobs 3`:

```
torrent   finished over http from disk resumed  shared proven    hash
payload_c     True 20.00 MiB 20.00 MiB 0.00 B        0 0.00 B    42ee6db050db50ce
payload_a     True 0.00 B    16.00 MiB 3.00 MiB      1 16.00 MiB 42ee6db050db50ce
payload_b     True 0.00 B    16.00 MiB 3.00 MiB      1 16.00 MiB 42ee6db050db50ce

over http:    20.00 MiB for the whole run
distinct hashes across three output directories: 1
verdict: one fetch, three copies, one hash
```

Report: `bench/shared-files-20260822T015216397Z.json`, `ok: true`, 649 ms.
`-Jobs 1` still passes, which is [T-140](#t-140-a-proven-shared-file-is-not-turned-into-a-source-on-its-own)'s
own acceptance and had to keep passing.

**What was built, and it is the split the approach named.**
`swarm::attach_bindings` is the per-binding half of `attach_sources_with`,
called once with every binding at the start and once with one binding for a
late attach. `swarm::attach_late` resolves one spec on its own and renumbers
its binding, because a set resolved alone always numbers from zero and the
ledger is keyed on the source index: two sources sharing one index would
convict each other. Coverage is not re-checked, because
`--web-seed-require` asks about the sources a run declared and a late one can
only add.

`donated_sources` now returns a third list, the donations whose donor has not
published where it wrote. `Attachments` carries the sources, the ledger, the
report rows and that pending list together, and `watch` takes it by mutable
reference; `attach_pending` runs at the top of each report tick, before the
accounting reads the list, so the tick that attaches a source already counts
it. Once the pending list is empty it costs nothing, which under `-j 1` is
from the first tick.

**The report needed less than the approach expected.** `sources` is read once
at the end and already tolerates a row that appeared partway through, because
it is built from the list rather than from the specs. The `source_added` event
is the same event for a late source as for an early one, and `origin:
shared_file` plus its position in the stream is what distinguishes it. What
did need deciding is the request budget: `--web-seed-max-total` divides across
the declared sources, and a pending donation is not in the list because its URL
is the path the donor has not written yet. `apply_max_total` takes the divisor
explicitly now, so a pending donation's share is reserved at the start and
attaching it never takes requests back off a bridge that is already serving.

**The deadlock the entry warned about did not happen, and the reason is worth
recording rather than treating as luck.** anacrolix discussion 916's case is a
leecher filling its unverified-bytes window with pieces no seeder has yet.
`bit-cli`'s window cache is per source and holds what that source fetched, and
a source here is attached against a live handle with the piece list it can
serve, so a source that does not exist yet contributes no window. The two
cannot disagree, because the window is created by the same call that makes the
pieces available. What would reproduce 916 is a source that attaches and then
loses pieces, which is [T-005](webseed.md)'s re-scope and not this.

`a_donated_file_attaches_to_a_torrent_that_has_already_started` is the
in-process test, and it was run against the old behaviour first: the receiver
never finishes and the run exits `Timeout`. The mirror is bound to the donor's
info hash alone, so the receiver cannot reach it and the 4,096 bytes it takes
can only have come off the donor's disk.

One script defect fixed on the way. `check-shared-files.ps1` waited exactly
`-TimeoutSeconds` for a run whose own `--stop-after` is `-TimeoutSeconds`, so a
run that stopped on its own deadline and wrote its report was killed at the
same instant and read as a run that wrote nothing. That is what the first
`-Jobs 3` measurement did. The wait now carries a thirty second margin, which
is what separates "it stopped and said so" from "we killed it".

### T-134 v1 and v2 info hashes are not reconciled

Category:    bep
Priority:    P2
Effort:      L
Status:      open

Problem:     A hybrid torrent carries both a v1 and a v2 info hash for the same
             payload, and the two name the same bytes. `bit-cli` has no v2
             support at all: [T-081](create-seed.md) is open and
             no BEP coverage document has been written yet.
Relevance:   Scenario 5 asks to reconcile them. Without it, the same payload
             offered as a v1 torrent and a v2 torrent is two unrelated
             downloads, which is the same waste as
             [T-133](#t-133-two-torrents-holding-the-same-file-cannot-share-its-bytes)
             and a case where the equivalence is not a guess.
Approach:    It depends on [T-081](create-seed.md) landing first. A hybrid
             torrent's `info` dict carries both `pieces` and `file tree`, so
             once v2 parses, two torrents that share either hash are the same
             payload by definition and no verification is needed.
Acceptance:  A hybrid torrent and the v1-only torrent cut from the same payload
             are recognised as one payload, and adding both in one invocation
             fetches the bytes once.

### T-135 Source selection cannot be steered by method or by priority at run time

Category:    performance
Priority:    P2
Effort:      L
Status:      open

Problem:     `--web-seed-priority` and the table's `priority` order sources
             against each other, and `--prefer-web-seed` biases HTTP against
             peers by giving sources more connections. Neither is a decision:
             [T-003](webseed.md) established that `librqbit`'s piece picker is
             not reachable from outside the crate, so a piece a peer answers
             first still comes from the peer.
Relevance:   Scenario 5's "smartly use web seeds + ddls + p2p swarm based on a
             priority". What ships today moves the odds. Measured, that is
             worth moving the HTTP share of a hybrid run from 46.72% to
             62.60%, and no further.
Approach:    [T-002](webseed.md) priced the real fix: an in-process peer needs
             four `pub(crate)` markers changed in `librqbit`, and the
             machinery underneath already takes an arbitrary byte stream.
             Owning the picker means owning that fork. Decide that explicitly
             rather than drifting into it.
Acceptance:  A hybrid run with a stated priority order fetches every piece from
             the highest-priority source that holds it, proven by per-source
             byte attribution against a fixture where each source holds a known
             disjoint set.

### T-136 Nothing states the end-to-end integrity guarantee

Category:    cli
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T08:24Z

Problem:     Scenario 5 asks for a guarantee that a finished file is bit-for-bit
             correct. The mechanisms are all there and none of them is stated
             as a contract: the per-source piece check
             (`--web-seed-verify piece`, the default), the session's own check,
             the hash check on add that makes resume safe, and
             `bit-cli verify`.
Relevance:   A guarantee nobody wrote down is not a guarantee, and this one is
             the reason a caller would trust a source it found on a CDN.
Approach:    A section in `README.md` naming each check, what it catches, and
             what it costs, and a `--verify-on-complete` flag that re-reads the
             finished payload and reports the hash of every file. It is
             redundant with the piece checks by construction, which is the
             point: it is the check a caller can run without trusting the
             thing that wrote the bytes.
Acceptance:  A run against a mirror serving one corrupt byte completes from
             another source, and the report names the piece, the source, and
             the mismatch. `--verify-on-complete` on the finished payload exits
             0 and prints a hash per file.

**Done 2026-08-23T08:24Z, and half the Acceptance was already met.** Measuring
before building, which is [RULES.md](RULES.md)'s own rule, found that the first
clause is what [T-179](webseed.md) built and holds:
`a_mirror_that_serves_wrong_bytes_is_named_and_the_healthy_one_is_not` runs a
mirror serving one corrupt piece beside an honest one, and asserts that every
conviction names source 0, that each carries a piece index and two hashes that
differ, that the honest mirror survives, and that the payload arrives complete.
The piece, the source and the mismatch, all three. Nothing was owed there and
nothing was written for it.

What was owed is the other clause and the contract itself.

**`--verify-on-complete`** re-reads the finished payload and reports a sha256
per file under `torrents[].verified_files`. Four decisions in it, each of which
could have gone the other way:

- **`sha256`, not the torrent's `sha1`.** This digest exists to be compared
  against one published somewhere else, and nobody publishes a per-file sha1 of
  a torrent's contents. The piece hashes have been checked twice by the time
  this runs, so a third sha1 would prove nothing new.
- **Only a finished torrent.** Digests of files that are not yet the files are
  a wrong answer rather than a missing one, and `verify_on_complete_hashes_nothing_when_the_run_did_not_finish`
  holds it.
- **Only selected files.** A file `--select-file` skipped was not written by
  this run, so hashing it would report a digest of whatever was there before.
- **It never changes the exit code.** The digests are facts about the payload
  and this run has nothing to compare them against. A caller that does is the
  one that can decide. A file that cannot be read carries its `error` rather
  than being left out, so a caller counting rows is never short one.

**`docs/integrity.md` is the contract**, which is what the entry's Relevance
asks for: a guarantee nobody wrote down is not a guarantee. Four checks, what
each catches, what each costs, which is on by default, and a closing section on
**what none of them tells you** — that every check proves the payload matches
the `.torrent`, and if the `.torrent` is wrong they all pass. That is what a
Metalink is for and the file says so. `README.md` carries the summary table and
points at it. The last section of `docs/integrity.md` names the test behind each
claim, so a reader can check the contract against something that runs.

**One duplicate removed on the way.** `metalink.rs` had a private streaming
`Digest` enum for checking a Metalink's checksum, and this needed the same
thing. It is `bit_cli_core::digest` now and `metalink.rs` uses it: two answers
to "what does this file hash to" is the one place two answers is the whole
problem. Its tests check all three algorithms against the **published** vectors
for the empty input and for `abc` rather than against this code's own previous
output, and one case is longer than a single read so the streaming loop is what
is tested rather than a single `update`.

**A test of this session's own turned out to assert something else**, found by
the full-suite run rather than by the module run. T-155's
`hash_check_only_over_a_metalink_still_reports_the_document` started a DHT it
did not need, and once the module had enough parallel tests it failed with
"error initializing persistent DHT". A hash check reads the disk; asserting that
a DHT can be started is the same class of mistake as
[T-215](webseed.md). `--port 0 --no-dht`, and the module runs clean three times
over.

```
$ cargo test -p bit-cli --lib verify_on_complete
test result: ok. 2 passed; 0 failed; 0 ignored; 409 filtered out

$ cargo test -p bit-cli-core --lib digest::
test result: ok. 5 passed; 0 failed; 0 ignored; 695 filtered out

$ cargo test -p bit-cli --lib cmd::download
test result: ok. 43 passed; 0 failed; 0 ignored; 368 filtered out
```

### T-137 A cooled-down source never comes back

Category:    webseed
Priority:    P2
Effort:      S
Status:      **done**

Problem:     `--web-seed-cooldown` and the table's `cooldown_ms` set a timer,
             and nothing waits it out. `SourceStats::record_error` stores an
             epoch millisecond deadline after `max_errors` consecutive failed
             requests, `SourceStats::is_cooling_down` reads it, and the bridge
             retires the source the moment it is true
             (`crates/bit-cli-core/src/webseed/bridge.rs`, the
             `BridgeError::Stalled` arm of `run`). So the flag changes nothing
             a caller can measure: any positive value behaves the same as any
             other.

             It was found closing
             [T-130](#t-130-a-source-cannot-be-told-which-statuses-are-worth-retrying),
             which made `max_errors` reachable for the first time and so made
             the cooldown reachable too.
Relevance:   Rule 0.10: a flag that does not move a number does not ship.
             Either it moves one or it goes. It also decides how long an
             unattended run tolerates a mirror that is down: today the answer
             is `retries` attempts times `max_errors` requests, about 17
             seconds at the defaults, and then the source is gone for good.
Approach:    Two options, and the choice is a trade-off rather than a bug fix.

             1. **Honour it.** The bridge sleeps until
                `stats.cooldown_until()` and reconnects, and the source's
                consecutive-error count resets. `cooldown_ms` then means what
                it says. The cost is that a run against one dead mirror with
                `--web-seed-only` stops failing fast: it sits for the default
                ten minutes instead of exiting in seconds, and only
                `--timeout` or `--stop-timeout` ends it. That is the wrong
                default for an unattended caller.
             2. **Cut it.** Remove `--web-seed-cooldown` and `cooldown_ms`,
                and let `--web-seed-max-errors` alone decide when a source is
                out. Smaller surface, nothing lost that a caller can observe
                today.

             The likely answer is 1 with a default of zero, meaning "do not
             come back", so fail-fast stays the default and a caller who wants
             a mirror to be given another chance says how long to wait. That
             needs the reported state to distinguish a cooling source from a
             failed one, or `--web-seed-require` and the "every source failed"
             stop condition in `crates/bit-cli/src/cmd/download.rs` will read
             a sleeping source as a live one and wait out the deadline.
Acceptance:  Two runs against `loopback-fileserver --status 503 --fail-after 4
             --recover-after 200`, one with a cooldown shorter than the outage
             and one with a cooldown longer than it. The first completes and
             the second does not, and the report says which source cooled down
             and for how long. Plus a run against a dead mirror with
             `--web-seed-only` proving the fail-fast path still exits in
             seconds.

**Option 1, with a default of zero, which is what the entry expected.**

The bridge sleeps out the deadline and reconnects with the error run cleared.
`--web-seed-cooldown 0`, the default, retires the source instead, so the
fail-fast path is unchanged and the flag is entirely opt-in.

Three things had to be separated that were one thing before:

- **The budget being spent and the wait being over.** `SourceStats` now has
  `budget_spent`, true from the moment `max_errors` consecutive requests fail
  until `end_cooldown` clears it, and `is_cooling_down`, true only while the
  deadline is ahead. They differ exactly when the cooldown is zero: the budget
  is spent and there is nothing to wait for. The guard on the fetch path is
  `budget_spent`, so a source that is out stays out whatever the timer says.
  `record_error` stores `until.max(1)` rather than `until`, because zero is the
  sentinel for "never tripped" and a zero-millisecond cooldown has to be
  distinguishable from one.
- **A sleeping source and a dead one.** `BridgeState::Cooling` sits between
  `Idle` and `Failed` in `AttachedSource::state`'s ranking. The report carries
  `cooldowns`, `cooldown_until`, and `cooldown_remaining_ms`, and a
  `source_cooling` event fires once per cooldown rather than once per source,
  so a mirror that goes out, comes back, and goes out again is reported each
  time. The "every source is dead" stop condition in `cmd::download::watch` is
  unchanged and now means what it says: a cooling source is not failed, so the
  run waits for it, bounded by `--timeout` or `--stop-timeout`.
- **Which deadline a waking bridge is allowed to clear.** Several connections
  share one `SourceStats`, so `end_cooldown` takes the deadline the caller
  slept on and compare-exchanges it. Without that, a bridge waking from an old
  cooldown could clear a newer one another connection had only just tripped.

**The outage had to become a clock.** `loopback-fileserver`'s failure window
was counted in requests, and a source that is cooling down makes no requests,
so the window never advanced while it waited and the mirror never came back.
`--down-for <SECONDS>` ends the window on a clock instead, starting at the
first request that falls into it, so `--fail-after` still decides when the
outage begins. Three unit tests in the example cover it.

**The measurement, 2026-08-20T13:26:02.637Z**, in
`bench/signed-source-20260820T132602637Z.json`. `scripts/check-signed-source.ps1`
now drives nine cases, the last three of which are this entry's:

```
case             exit downloaded hash    state   cooldowns
cooldown_short      0 64.00 MiB  matches active          4
cooldown_long       9 3.00 MiB   -       cooling         1
dead_mirror         1 0 B        -       failed          1
```

```
$ pwsh -NoProfile -File scripts/check-signed-source.ps1
```

`cooldown_short` and `cooldown_long` are the same server, the same 20 second
outage, the same `--timeout 60s`, and the same `--web-seed-max-errors 2
--web-seed-retries 0`. The only difference is `--web-seed-cooldown`: 5 seconds
against 300. The first cooled down four times, waking twice into a mirror that
was still down and once into one that was back, and completed in 24.3 seconds
with the payload hashing equal. The second cooled down once and was still
asleep with 241.1 seconds left when the deadline fired, at 3.00 MiB of 64.

`dead_mirror` is the fail-fast case at every default, including
`--web-seed-cooldown 0`: a mirror answering 503 forever retires the source and
the run exits 1 after 33.4 seconds. That is longer than the "about 17 seconds"
this entry predicted, and the difference is the bridge's own reconnect backoff
between attempts, which the estimate did not count. Both numbers are seconds
rather than the ten minutes the old default would have produced, which is the
point of the default.

Five unit tests cover the state machine without a network:
`a_zero_cooldown_spends_the_budget_with_nothing_to_wait_for`,
`ending_a_cooldown_clears_the_error_run_but_not_the_totals`,
`cooldown_trips_only_after_the_configured_run_of_errors`,
`a_timed_outage_closes_on_the_clock_rather_than_on_a_request_count`, and
`a_timed_outage_starts_when_the_failure_window_does`.

### T-139 A resumed download charges its existing bytes to the swarm

Source:      came out of closing T-133 layer 2
Category:    cli
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `from_peers` was `progress_bytes - from_web_seeds`, and
             `progress_bytes` is everything the torrent has rather than
             everything this run fetched. So a download that resumed 45 MiB of
             a 64 MiB file reported 45 MiB from peers with no peer in the
             swarm, and a run with `--web-seed-only` reported peer bytes at all.
Relevance:   The testing policy asks that "bytes attributed to peers and to web
             seeds are reported separately and sum to the total". They did sum
             to the total. One of the two was wrong, which is worse than a
             number that does not add up, because it adds up.
Approach:    Read `progress_bytes` once, after the hash check on add and
             before anything is fetched, and subtract it as well.
Acceptance:  A run whose payload is already complete reports every byte as
             resumed and none from peers or from web seeds.

**Done.** `TorrentReport` and `DownloadReport` carry `from_resume`, the text
output prints `already on disk` when it is non-zero, and `torrent_completed`
carries `from_resume` beside the other two. The three now partition the total
rather than two of them splitting it.

It turned up in the `one_invocation` case of
`scripts/check-local-source.ps1`, where three torrents were given every file
except the shared one and the report charged the pre-placed 5, 3, and 4 MiB to
peers on a run with `--web-seed-only`, which disables peers entirely.

`bytes_already_on_disk_are_reported_as_resumed_rather_than_from_peers` is the
test: a payload already complete on disk, no tracker, no DHT, no LSD, no
source. 3000 bytes resumed, zero from peers, zero from web seeds.

---

## Part 3: the harness

What the acceptances need, in the order it unblocks the most. The first two
exist.

1. **[T-131](#t-131-the-loopback-file-server-cannot-simulate-a-signed-url)**,
   the signing and redirecting file server, is **done**. `--sign-redirect`,
   `--require-sig`, `--redirect-chain`, `--recover-after`, and `--down-for`
   are on `crates/bit-cli-core/examples/loopback-fileserver.rs`, and
   `scripts/check-signed-source.ps1` drives all six cases Scenario 1 and 4
   need, plus the three [T-137](#t-137-a-cooled-down-source-never-comes-back)
   added.
2. **The fixture**, which exists: `scripts/make-scenario-fixture.ps1`. It
   builds one payload, three torrents with different piece lengths, different
   surrounding files, and three different info hashes, a CDN copy under an
   unrelated name, a second mirror layout with a space in a directory name,
   and the partial on-disk state each scenario starts from.

   ```
   $ pwsh scripts/make-scenario-fixture.ps1 -BlobSizeMiB 16 -Partial 70

   payload_a    5164aaf5bbb40cd396ba52945c5221074aa14f12   25.00 MiB  pieces   25 of 1.00 MiB
   payload_b    c2806b5adee5e75398f6741b9af66cb9951059c0   19.00 MiB  pieces   38 of 512.00 KiB
   payload_c    31084dc6ab74b846654ffecbc721fc1865989cf7   20.00 MiB  pieces   10 of 2.00 MiB

   the shared file, byte for byte the same in all three:
     42EE6DB050DB50CE  payload_a/deep/nested/dirs/file.blob
     42EE6DB050DB50CE  payload_b/media/file.blob
     42EE6DB050DB50CE  payload_c/a/b/c/file.blob
     42EE6DB050DB50CE  cdn/a3f1b2c4-signed-blob.dat
   ```

   The three piece lengths are the point. Equivalence that only holds when the
   piece boundaries line up is not equivalence, and 1 MiB against 512 KiB
   against 2 MiB is what makes
   [T-133](#t-133-two-torrents-holding-the-same-file-cannot-share-its-bytes)
   testable rather than assumed.
3. **A second file server on a different port** answering the same payload
   under the `mirror/pub files/payload/` layout the fixture already builds, for
   Scenario 4. `--ignore-range` and `--status` already cover the failure half,
   and one server rooted at the fixture serves both layouts today.

Nothing here needs the network. All five scenarios are testable end to end on
loopback, which is what makes them worth doing properly.

---

## State: there is none, and that is the design

The operator asked whether `bit-cli` uses SQLite and whether it should.

**It stores nothing.** No database, no session file, no resume cache, no
registry. The only file it reads outside the output directory is an optional
`config.toml`, and `--no-config` turns that off. Decision 7.4 puts every form
of stored session state in Phase C, and `SessionOptions::persistence` is
`None` for that reason.

Resume works without state because the payload is the state: adding a torrent
hash-checks what is on disk, and what checks out is not fetched again. That is
what made Scenario 1 fetch 19 MiB rather than 64. It costs a full read of the
payload on every add, which is [T-016](disk-io.md), blocked upstream because
`fastresume` in `librqbit` 9.0.0 does nothing without turning on the
persistence store that 7.4 forbids.

**Would SQLite help these scenarios?** For four of the six entries above, no.
[T-130](#t-130-a-source-cannot-be-told-which-statuses-are-worth-retrying),
[T-131](#t-131-the-loopback-file-server-cannot-simulate-a-signed-url),
[T-132](#t-132-the-swarm-cannot-be-rate-limited-separately-from-http-sources),
and [T-136](#t-136-nothing-states-the-end-to-end-integrity-guarantee) are all
within one invocation and need nothing remembered.

For [T-133](#t-133-two-torrents-holding-the-same-file-cannot-share-its-bytes)
it depends on which layer:

- Layer 1, a `file:` source, needs no state: the caller names the path.
- Layers 2 and 3 need no state either **when the torrents are added in one
  invocation**, which is how the scenario is written. That is now what
  happens: the bindings are per torrent, `-j 1` orders them, and
  `bit-cli files --against` computes the equivalence from the metainfo the
  run already has.
- A store is only needed to carry equivalence *between* invocations, so that
  torrent B added tomorrow knows about torrent A's file from today. That is
  the same shape as [T-016](disk-io.md)'s resume cache and the same shape as
  every Phase C item.

So the recommendation is: **do not add SQLite for these scenarios.** Build
them one-invocation-first, which is what decision 7.4 already requires, and
which is also the faster thing to build and the easier thing to test.

If a cross-invocation store is later wanted, the thing to weigh is not SQLite
against files but what the store is for. A content-addressed index of "which
local path holds the bytes for piece hash H" is a key-value lookup that a
single append-only file with an in-memory index serves at a fraction of the
dependency cost, and it degrades to "not found" safely. SQLite earns its place
when several processes write concurrently, and decision 7.4 says there is only
ever one.

Whatever is built, the rule stated in the operator's brief holds: `bit-cli`
must keep working with no config file and no state file. Every store is an
optimisation that a cold run reproduces by reading the payload.

---

## What the five scenarios need, in one table

| Scenario | Works today | Needs | Size |
| --- | --- | --- | --- |
| 1. DDL for one selected file, resumed | **Yes, in full.** Binding, resume, rename, selection, redirects, and a signature that expires mid-run, all run and recorded above | nothing | none |
| 2. Three torrents, one shared file | **Yes, in one invocation, with nothing written by the caller.** `-j 1` and no flag at all: C fetches from the mirror, A and B read what C wrote off the disk, one distinct hash across three info hashes. Run and recorded under T-140 | [T-143](#t-143-a-source-cannot-be-attached-to-a-torrent-that-has-already-started), for the same thing above `-j 1` | M |
| 3. DDL for one file, rest via swarm | **Yes, in full** | nothing | none |
| 4. Remapping and encoding | **Yes, in full**, through `exact`, `prefix`, `template`, per-source headers, and the status overrides | nothing | none |
| 5. All of it, with per-method control | Per-source caps, headers, auth, priority, and status policy: yes. Per-method caps and picker control: no | [T-132](#t-132-the-swarm-cannot-be-rate-limited-separately-from-http-sources), [T-134](#t-134-v1-and-v2-info-hashes-are-not-reconciled), [T-135](#t-135-source-selection-cannot-be-steered-by-method-or-by-priority-at-run-time), [T-136](#t-136-nothing-states-the-end-to-end-integrity-guarantee) | M to L |

The honest summary: the addressing model was built for exactly this and it
holds. Four of the five scenarios work in full. Cross-torrent identity is now
computed rather than asserted, under `-j 1`. What is genuinely missing is real
control of which source answers a piece, which was already known and already
priced by [T-002](webseed.md) and [T-003](webseed.md), and attaching a source
to a torrent that has already started, which is [T-143](#t-143-a-source-cannot-be-attached-to-a-torrent-that-has-already-started).

What none of this needs is a daemon, a database, or a state file. Every
scenario as the operator wrote it is one invocation with several sources, which
is what `bit-cli` is.
