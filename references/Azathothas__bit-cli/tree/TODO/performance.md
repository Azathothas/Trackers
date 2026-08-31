# Performance and throughput

Thirty-one issues touch piece picking, pipelining, block size, endgame, and
buffering.

---

### T-030 Throughput collapses with several torrents at once

Source:      https://github.com/ikatson/rqbit/issues/590 (open)
Category:    performance
Priority:    P0
Effort:      L
Status:      **done**

Problem:     Two reports in one: adding several torrents slows all of them well
             past what sharing a link explains, and a single large torrent
             (over 4 GB) shows start-stop-start-stop behaviour where the rate
             drops to zero and only a pause and resume clears it.
Relevance:   `-j` exists to run several sources in one invocation. If that is
             slower than running them one at a time, the flag is a trap.
Approach:    Measure before theorising. Three runs with the same total payload:
             one torrent alone, three torrents at `-j 1`, three at `-j 3`. If
             `-j 3` is slower than `-j 1` in wall time for the same bytes, the
             contention is real and the next question is where: the tokio
             blocking pool, the disk, or the peer connection budget.
Acceptance:  A `bench/multi-torrent-<timestamp>.json` report with the three
             wall times, peak RSS, and CPU time, and this entry naming which
             resource saturated.

**The first report was real and it was two defects, neither of them
contention.** Both are fixed. `-j 4` now moves four torrents 3.54 times faster
than running them one invocation at a time, at 72% of what the HTTP source
serves with no torrent machinery at all. The second report, the intermittent
stall, reproduced once and is [T-037](#t-037-a-run-stalls-for-minutes-roughly-once-in-fifty).

`scripts/check-multi-torrent.ps1` is the measurement. Six modes rather than the
three the acceptance asks for, because three cannot separate what the extra
processes cost from what the shared session costs, and cannot say whether `-j`
bought concurrency or bought connections:

| Mode | What it runs |
| --- | --- |
| `one` | One torrent, one invocation. The per-torrent rate with nothing to share. |
| `serial` | N torrents, N invocations, one after another. What a caller who avoided `-j` would pay, process startup included. |
| `j1` | N torrents, one invocation, `-j 1`. Same session, one download at a time. |
| `j2`, `j4` | N torrents, one invocation, at each step of the sweep. |
| `control` | One torrent at a time with as many connections as the deepest sweep step has in total. |

Every mode moves the same bytes off the same loopback server, and the run
starts by measuring what that server serves through `bit-cli`'s own HTTP path
with no bridge, no hashing, and no disk. Without that ceiling a rate says
nothing: a mode that reaches it is describing the server.

Acceptance, four torrents of 256 MiB, three iterations, medians,
2026-08-20T08:07:01.379Z. Report: `bench/multi-torrent-20260820T080701379Z.json`.

```
$ pwsh -NoProfile -File scripts/check-multi-torrent.ps1 -Torrents 4 -PayloadSize 256MiB -Runs 3

ceiling:  808.84 MiB/s through bit-cli's own HTTP path, no bridge, no hashing, no disk

mode    wall  bytes      rate         of ceiling peak RSS   CPU ms handles
one     1.46s 256.00 MiB 175.95 MiB/s 21.75%     43.61 MiB    2124     220
serial  6.24s 1.00 GiB   164.02 MiB/s 20.28%     44.48 MiB    8605     228
j1      6.18s 1.00 GiB   165.78 MiB/s 20.50%     48.49 MiB    8468     227
j2      3.01s 1.00 GiB   340.20 MiB/s 42.06%     74.09 MiB    9061     242
j4      1.76s 1.00 GiB   580.17 MiB/s 71.73%     114.24 MiB  10656     264
control 2.97s 1.00 GiB   344.32 MiB/s 42.57%     107.59 MiB  15108     289
```

**Which resource saturated: none of `bit-cli`'s.** `-j 4` runs at 71.73% of
what the file server itself serves. Attributing the remaining 28% needs a
faster source than this machine can run beside the client, so the honest answer
is that the measurement ran out of server before it ran out of `bit-cli`.

**`-j` buys concurrency, not connections.** That is what `control` is for.
`-j 4` gives four torrents four connections each, sixteen in flight. Putting
those same sixteen on one torrent at a time reaches 344 MiB/s where `-j 4`
reaches 580, so the flag is worth 1.69 times what the connections alone are
worth.

**Memory scales with the flag and nothing else does.** Peak RSS goes 48.49,
74.09, 114.24 MiB across `-j 1`, `-j 2`, `-j 4`, which is about 22 MiB per
concurrent torrent. Handles go 227, 242, 264, which is about twelve per
concurrent torrent. CPU is flat at 8.5 to 10.7 seconds for the same gigabyte.
Those are the numbers [T-040](memory.md) needs.

## The two defects

### One: completion was noticed on the next report tick

`download`'s watch loop woke only on `--report-interval`, which defaults to one
second, and completion was checked after the tick. So a torrent that finished
1.1 seconds in was noticed at 2.0 seconds, and `-j 1` with four torrents paid
that four times. `--timeout` and `--stop-after` had the same problem and would
fire up to a second late.

The loop now wakes on three things: the tick, the torrent completing, and the
earliest deadline the caller set. `should_stop` still decides what any of them
means, so a seeding run that keeps going after completion is unchanged.

Measured on its own: the same script, the same fixture, the same machine, with
the path fix below already in and the completion and deadline branches of the
`select!` the only difference. Tick-only report:
`bench/multi-torrent-20260820T081542263Z.json`.

| Mode | Woken by the tick only | Also woken by completion | Gain |
| --- | --- | --- | --- |
| `one` | 2.08s | 1.46s | 1.42x |
| `serial` | 8.28s | 6.24s | 1.33x |
| `j1` | 8.12s | 6.18s | 1.31x |
| `j2` | 4.08s | 3.01s | 1.36x |
| `j4` | 2.07s | 1.76s | 1.18x |
| `control` | 5.11s | 2.97s | 1.72x |

The shape is what the explanation predicts. `-j 1` runs four batches and saves
1.94 seconds, which is four times the half-second a uniformly distributed
finish loses to a one-second tick. `-j 4` runs one batch and saves about one
tick's worth. `one` is a single 1.46-second download that was taking 2.08
seconds, which is the whole of the difference.

### Two: a multi-file torrent with one file lost its directory

This is the one that made "several torrents" look like contention, and it is a
correctness bug rather than a slow one. It is written up as
[T-036](#t-036-a-multi-file-torrent-with-one-file-lands-without-its-directory).
In short: four torrents were writing to one file, so the run was paying the
per-file write serialisation [T-017](disk-io.md) measured, and three of the
four payloads were being destroyed while all four reported success.

## What was checked and did not explain it

- **Ephemeral ports.** Ten alternating `-j 4` and `-j 2` runs moved
  `TimeWait` from 276 to 500 against a 16,384-port dynamic range, and
  `CloseWait` stayed at zero throughout. Not the port table.
- **The `-j` semaphore.** The permit is bound to a named local and held for the
  whole download, so it is released when the worker ends and not before.
- **The file server.** It is measured every run and reported as `ceiling`, so a
  mode that approaches it is visible rather than being read as a `bit-cli`
  limit.


### T-031 The rate limit did not apply to the session

Source:      https://github.com/ikatson/rqbit/issues/391 (closed, 2025-06-10)
Category:    performance
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `SessionOptions::ratelimits` was reported not to take effect.
Relevance:   `--max-download-rate` and `--max-upload-rate` go straight into
             `LimitsConfig`. If that is ignored, the flags are decorative, and
             rule 0.10 says a knob that does not move a number does not ship.
Approach:    The issue is closed upstream, which suggests the pinned 9.0.0
             carries the fix, but "closed" is not "verified here". Measure it:
             download a known payload with and without a cap and compare the
             sustained rate.
Acceptance:  `bit-cli download <TORRENT> --max-download-rate 1MiB/s` sustains
             within 10 percent of 1 MiB/s over 60 seconds, and the same run
             uncapped is meaningfully faster. Both numbers recorded here.
Closed:      Both caps hold, and the pinned 9.0.0 does carry the fix.
             `pwsh -NoProfile -File scripts/check-rate-limit.ps1` is the
             measurement: one seeder, one 128 MiB payload, three paired runs
             alternating order, peers only.

             ```
             run mode     exit wall  bytes      rate
               1 capped      0 31.2s 128.00 MiB 4.10 MiB/s
               1 uncapped    0 0.6s  128.00 MiB 220.31 MiB/s
               2 uncapped    0 0.6s  128.00 MiB 229.39 MiB/s
               2 capped      0 31.2s 128.00 MiB 4.10 MiB/s
               3 capped      0 31.2s 128.00 MiB 4.10 MiB/s
               3 uncapped    0 0.6s  128.00 MiB 223.39 MiB/s

             with the seeder capped at 4MiB/s and the downloader uncapped: 4.01 MiB/s
             ```

             `--max-download-rate 4MiB/s` sustains 4.10 MiB/s, 2.5% over the
             cap and inside the 10% the acceptance asks for, against 223.39
             MiB/s uncapped, which is 54 times faster. The other direction is
             the same `LimitsConfig` field seen from the other end: with the
             seeder started under `--max-upload-rate 4MiB/s` and the
             downloader uncapped, the transfer comes out at 4.01 MiB/s.

             The rate is computed from the wall clock and the bytes the report
             says landed rather than from the report's own mean, so the
             limiter is not measured by the thing it limits. Each run gets a
             fresh output directory, because reusing one lets the hash check
             on add find the payload already there and report the disk.

             What this does **not** cover is `--max-overall-download-rate` and
             `--max-overall-upload-rate` across several torrents in one
             invocation, and the asymmetry that
             [T-132](multi-source.md) is about: a session cap applies to peers
             and to HTTP sources together, because a source reaches the
             session as a peer.

### T-032 The piece selector strategy is not implemented

Source:      the operator's brief
Category:    performance
Priority:    P1
Effort:      L
Status:      **done**

Problem:     `--piece-selector rarest-first|sequential|in-order|random` parses
             and is carried through the config, and none of the four reaches
             `librqbit`'s picker, which is rarest-first and not configurable.
Relevance:   Sequential is what makes streaming work, and it is the difference
             between a usable and an unusable `bit-cli download | vlc -`.
Approach:    `librqbit` has a `FileStream` type and streaming support, which
             suggests some ordering control exists; find it before assuming a
             fork is needed. If the picker is genuinely fixed, this needs
             either an upstream API or Candidate C.
Acceptance:  `bit-cli download <TORRENT> --piece-selector sequential --jsonl`
             emits `piece_verified` events whose indices are non-decreasing for
             at least the first 90 percent of the run.

**The entry's premise is half wrong, and the wrong half is the important one.
`librqbit` 9.0.0 is not rarest-first.** Nothing in its picker counts how many
peers hold a piece. `ChunkTracker::iter_queued_pieces` walks the files in
priority order and hands each one to `FileInfo::iter_piece_priorities`, which
is:

```rust
// crates/.../librqbit-9.0.0/src/file_info.rs:15
// First and last of each file first, then the rest of pieces in that file.
first.chain(last).chain(mid).take(r.len())
```

So the natural order is **first piece, last piece, then ascending**. Measured
on a 48 piece torrent over four connections, the order pieces were verified in:

```
47, 0, 1, 3, 4, 6, 2, 5, 7, 8, 9, 10, 12, 14, 11, 13, 15, 16, ... 46
```

Near-sequential already, with the tail pulled forward and some local
reordering from four transfers finishing out of turn. So the flag was never
selecting between rarest-first and sequential. It was selecting between
"almost sequential" and nothing.

**The lever that does exist.** `PieceTracker::acquire_piece` checks a
`priority_pieces` iterator **before** the natural order, and that iterator is
built by the streaming subsystem from every registered `FileStream`: each one
contributes the pieces covering the 32 MiB after its own read position.
`ManagedTorrent::stream(file_id)` is public. So a stream held at the earliest
piece the torrent still needs is a supported way to say "this part next", and
it needs no fork, which is what [T-002](webseed.md) priced for the other
approach.

`bit_cli_core::piece_order::InOrder` is that stream and nothing else. It never
reads a byte: it seeks, which is what moves the window, and lets the session do
the work. One stream at a time, on whichever file holds the earliest missing
piece, moved every 50 ms.

**`--piece-selector` now has three values, and it had four.** Both removals are
rule 0.10, from opposite directions:

- `rarest-first` was the default and named behaviour nothing here has. It is
  now `default`, documented as what the session actually does.
- `random` named behaviour nothing implemented and nothing can ask for. The
  `priority_pieces` iterator is the only input to the picker and a stream's
  contribution is a contiguous window, so there is no way to express a
  scattered order through it. It is gone rather than accepted and ignored.

`sequential` and `in-order` are one behaviour under two names, one common and
one `aria2`'s. `cmd::download::tests::sequential_and_in_order_are_the_same_selector`
is the single place that says so.

**Acceptance, `scripts/check-piece-order.ps1`, 10 runs per cell, 48 pieces of
1 MiB over a loopback file server:**

| connections | selector | descents, max | descents, mean | wall, mean |
| --- | --- | --- | --- | --- |
| 1 | `default` | 1 | 1.00 | 389 ms |
| 1 | **`sequential`** | **0** | **0.00** | 368 ms |
| 2 | `default` | 4 | 2.20 | 232 ms |
| 2 | `sequential` | 3 | 0.90 | 244 ms |
| 4 | `default` | 4 | 2.20 | 209 ms |
| 4 | `sequential` | 3 | 1.60 | 224 ms |

A descent is a `piece_verified` event carrying a lower index than the one
before it.

**At one connection the answer is exact: zero descents in all ten runs, against
one in all ten for the default.** That is the acceptance met, and met harder
than it asks: it wanted non-decreasing over the first 90 percent of the run and
this is non-decreasing over all of it.

**Above one connection it is not, and that is not the selector.** A selector
decides which piece is asked for next. It cannot decide the order in which
transfers already in flight finish. At four connections four pieces are moving
at once and they complete in whatever order the mirror answers, so the arrival
order is the request order with local swaps in it. Sequential still helps,
because the descent it removes is the structural one: the last piece is no
longer pulled to the front. The residual is concurrency, and the honest thing
is to say so rather than to report a number that looks like a failure of the
flag.

**What it costs: nothing at one connection and about 7 percent at four.** 368 ms
against 389 at one, which is inside the noise and slightly the other way; 224 ms
against 209 at four, a ratio of 1.07. Two costs make that up, and both are
real: the open stream holds one permit from `librqbit`'s blocking spawner
semaphore, which is sized at the session's worker thread count and defaults to
eight, and pointing every peer at the same part of the file gives them less to
choose between. The check fails the run if the ratio passes 1.6, so a
regression here is caught rather than absorbed.

**One race, found by the measurement and not by the design.** The window was
first registered in the watch loop, which starts after the sources are
attached. In one run of five, the session had already handed out the last piece
by then and the order came back
`0,1,2,3,4,47,5,6,...`: one descent, at position six. It is registered before
any source is attached now, so under `--web-seed-only` nothing can ask for a
piece before the window exists, because the bridges are the only peers and they
do not exist yet. Against a real swarm it is best effort, and a peer dialled
during the hash check may still be holding an assignment. That is written into
the code at the point it matters.

**Two things this leaves.** `piece_verified` is derived from polling the
bitfield, which is [T-111](cli-surface.md), so the acceptance runs at
`--report-interval 20ms`: a coarse interval folds several pieces into one tick
and reports them in index order whatever order they arrived in, which would
make every selector look sequential. And the 32 MiB lookahead is a `librqbit`
constant this code does not control, so a run that completes more than 32 MiB
between two 50 ms advances would fall back to the natural order for the
overshoot. Neither has been reached here; both are worth knowing.

### T-033 --split, -x, and -k do not reach the fetch path

Source:      the operator's brief; premise disproved 2026-08-21, see the correction below
Category:    performance
Priority:    P3
Effort:      S
Status:      **done**, 2026-08-24

Problem:     `-s/--split`, `-x/--max-connection-per-server`, and
             `-k/--min-split-size` parse and do nothing. They are the aria2
             flags a migrating script will already be passing.
Relevance:   Rule 0.10 again. Three flags that look like they work and do not
             are worse than three flags that error.
Approach:    All three are about how one HTTP source is fetched in parallel.
             `--web-seed-concurrency` and `--web-seed-chunk-size` already
             express the same two ideas, so the honest wiring is to make `-x`
             an alias of `--web-seed-concurrency`, `-k` a floor on
             `--web-seed-chunk-size`, and `-s` the segment count per source,
             then prove each moves a number.
Acceptance:  `bench/split-<timestamp>.json` shows throughput at `-x 1`,
             `-x 4`, and `-x 16` against one mirror, with the curve recorded
             here. If the curve is flat, the flags do not ship.

**The premise is wrong and the measurement is one command.** Checked on
2026-08-21 against the built binary: none of the three flags exists. There is
no `--split`, no `-x`, no `--max-connection-per-server`, no `-k`, and no
`--min-split-size` anywhere in `crates/bit-cli/src/cli.rs` or in
`bit-cli <SUBCOMMAND> --help` for any of the sixteen subcommands.

```
$ bit-cli download --split 4
error: unexpected argument '--split' found
$ echo $LASTEXITCODE
2
```

They do not "parse and do nothing". They are rejected with exit 2, `Usage`,
which is what this entry's own Relevance argues *for*: "three flags that look
like they work and do not are worse than three flags that error." They error.

So the defect this entry describes does not exist, and what is left is a real
but different question: **should `bit-cli` accept the aria2 spellings at all?**
That is aria2 parity, not a broken flag, and the case for it is that a script
written from aria2 muscle memory passes them. The case against is that all
three concepts already have flags here that do work and are measured:

| aria2 | `bit-cli` | State |
| --- | --- | --- |
| `-x`, `--max-connection-per-server` | `--web-seed-connections` | works, measured under [T-009](webseed.md): two connections are worth 1.92x on loopback |
| `-s`, `--split` | `--web-seed-concurrency` | works |
| `-k`, `--min-split-size` | `--web-seed-chunk-size` | works |

Adding three aliases is half an hour. What makes it a decision rather than a
chore is that the mappings are not exact. aria2's `-x` is a per-server
connection cap and `--web-seed-connections` is a per-source one, which differ
when two sources share a host, so an alias that is close but not identical is
the failure this project's own short-flag rules exist to prevent.
`docs/flags.md` states the rule: an `aria2` letter is never reassigned to a
different concept, and `cli.rs:2741` `short_flags_never_contradict_aria2`
enforces it. Under that rule `-x` may only be taken if it means what aria2
means.

**Re-scoped rather than closed**, because the entry's underlying question is
open and this project does not close things by deciding they were never
broken. Status stays open; the priority drops from P2 to P3, since a flag that
errors is not a defect and nothing here is unmeasured. The Acceptance is now:
decide whether the three aliases ship, and if they do, prove each moves a
number under its aria2 meaning rather than its `bit-cli` one.

This is the same shape as [T-032](#t-032-the-piece-selector-strategy-is-not-implemented)
and [T-141](webseed.md), both of which closed by disproving their own premise,
and the same shape as [T-118](cli-surface.md), which turned out to be built.
Three entries in this directory have now described a state the tree was not in.
The common cause is that all three were written from a specification of what
`bit-cli` should do rather than from the binary, and the fix each time was one
command.

## The curve is measured, 2026-08-23, and it is not flat

The entry stays open. Its Acceptance has two halves and the measurement is the
first: **"`bench/split-<timestamp>.json` shows throughput at `-x 1`, `-x 4`,
and `-x 16` against one mirror, with the curve recorded here. If the curve is
flat, the flags do not ship."**

**Taking it needed the instrument fixed first**, which is
[T-229](bench.md): `bench webseed --concurrency-sweep` charged the run's
warmup to its own first steps. How wrong the first point was depended on how
much of it fell inside the warmup: at the defaults, 30 seconds over five steps
is six seconds a step against a three second warmup, so the first point read
about half. The sweeps taken here used shorter steps and it read zero, and
`--concurrency-sweep 16,1` reported *best concurrency 1*. The first attempt at
this measurement produced `1: 0 B/s`, `2: 0 B/s`, and it was believing that
number for ten seconds that found the defect.

**The curve, once the instrument was honest.** 64 MiB payload, 1 MiB pieces,
one loopback source, 20 seconds a step, committed at
`bench/split-20260823T182709577Z.json`:

| `--web-seed-concurrency` | rate | requests | p50 | p99 |
| --- | --- | --- | --- | --- |
| 1 | 940.53 MiB/s | 944 | 3ms | 18ms |
| 2 | 1.61 GiB/s | 1,655 | 4ms | 17ms |
| 4 | 2.85 GiB/s | 2,924 | 5ms | 10ms |
| 8 | **3.44 GiB/s** | 3,534 | 8ms | 14ms |
| 16 | 3.38 GiB/s | 3,480 | 18ms | 29ms |

It is the shape a sweep is supposed to find: 3.7 times from one connection to
eight, a knee at eight, and past it throughput stops while p99 doubles. **So
the curve is not flat and the flags are not disqualified by their own
Acceptance.**

**What is left is the surface, and it is a decision rather than a measurement.**
`-x`/`--max-connection-per-server` is an alias of `--web-seed-concurrency` and
the number above is what justifies it. `-k`/`--min-split-size` is a floor on
`--web-seed-chunk-size`. `-s`/`--split` is the segment count per source, and it
is the one with no existing flag behind it: aria2 splits **one file** across N
ranges, which is what `--web-seed-concurrency` already does per source, so `-s`
and `-x` would mean nearly the same thing here and a migrating script passing
both would expect them not to.

Decide those three together, with the man page written before the code, which
is what [T-198](cli-surface.md) is for. The measurement half of the Acceptance
is done and committed; nothing else about this entry is blocked.

**One caveat on the numbers and it does not change the verdict.** This is a
loopback file server on the machine under test, so the payload is in the page
cache and the rates are the local HTTP stack rather than a mirror. What the
curve says is that concurrency moves *this* path by 3.7 times, which is the
question the Acceptance asks; a real mirror's knee will be at a different
place for a different reason.

**Ruled on 2026-08-24: take all three, and warn.** The recommendation put to
the operator was to refuse them, on the grounds that `docs/flags.md` forbids
reassigning an `aria2` letter to a different concept and `-x` is a per-server
cap here and a per-source cap there. That was not the answer.

So all three ship as aliases, and the difference ships with them: a one-line
warning on first use of `-x`, naming that it caps connections per source rather
than per server, and that the two differ when two sources share a host. The
same rule that would have refused the alias is what the warning satisfies: the
difference is stated rather than hidden, which is what
`cli.rs:2741` `short_flags_never_contradict_aria2` exists to prevent losing.

`-s` and `-x` meaning nearly the same thing here is the part to get right in
the man page before the code, per [T-198](cli-surface.md). A script passing
both must not get the product of the two.

## Taken, 2026-08-24, with the man page written first

All three ship. `-x/--max-connection-per-server` and `-s/--split` are two
spellings of `--web-seed-concurrency`, and `-k/--min-split-size` is a floor
under `--web-seed-chunk-size`.

**The man page went first**, per [T-198](cli-surface.md): the three flags were
declared with their help text, `scripts/check-man.ps1 -Fix` was run, and
`man/bit-cli.json` was read back before a line of wiring existed. That is what
caught the sentence worth catching, which is that `-s` needs to say it is the
same knob as `-x` rather than a second one.

**The largest given wins, and it is not the product.**
`webseed_args::aria2_concurrency` takes the maximum of the three, so
`-x 4 -s 16` is sixteen concurrent requests per source rather than sixty-four.
That is the failure the ruling names and it is held by
`passing_both_aria2_spellings_is_not_multiplied`.

**`-k` is a floor and the test proves it is a floor rather than a value.** The
first version of that test asserted `-k 2MiB` sets the chunk size to 2 MiB. It
does not: the default is 4 MiB and a floor below it changes nothing. The test
now raises the default with `-k 8MiB` and asserts `-k 1MiB` leaves it alone,
which is the property the flag actually has.

**The warning, which is what `docs/flags.md`'s rule asks for instead of a
refusal:**

```
$ bit-cli download album.torrent --dry-run -x 4 -s 16 -k 2MiB
warning: -x caps concurrent requests per source, not per server: -x 4 with two sources on one host is 8 requests to that host. --web-seed-max-total is the run-wide cap.
warning: -x and -s are one setting here, so -x 4 -s 16 is 16 concurrent requests per source rather than 64.
```

The second fires only when the two differ, so the common migrating case,
`-x 8 -s 8`, gets one line rather than two. `-s` alone gets none: there is no
per-server reading of it to correct.

Four tests in `crates/bit-cli/src/webseed_args.rs`, and the three short letters
moved from the reserved table in `docs/flags.md` to the assigned one, with a
section under it naming what each is close to and not exact about.

The measurement half of the Acceptance was already done and committed at
`bench/split-20260823T182709577Z.json`: 940 MiB/s at one concurrent request,
3.44 GiB/s at eight, a knee at eight, and past it throughput stops while p99
doubles. Nothing was re-measured here, because nothing about the numbers
changed: the aliases point at the flag that curve was taken over.

### T-034 Endgame mode is not observable

Source:      corpus, performance category
Category:    performance
Priority:    P3
Effort:      M
Status:      open

Problem:     The last few pieces of a download are the ones that decide the
             wall time, and nothing in the report says whether endgame
             duplication happened or how long the tail took.
Relevance:   `bench leech` is meant to answer "is my server serving well". A
             run whose last piece took 40 seconds looks the same as one that
             was uniformly slow.
Approach:    Record time to 90, 99, and 100 percent separately in the download
             report, and the number of pieces requested from more than one
             source.
Acceptance:  `bit-cli download --json` carries `p90_ms`, `p99_ms`, and
             `total_ms` for progress, and the tail is visible as the difference.

### T-035 The web seed rate limit was never applied

Source:      the [T-003](webseed.md) hybrid measurement
Category:    performance
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `--web-seed-speed-limit`, and `rate_limit` in a binding table,
             parsed, validated, and reached `SourceLimits.rate_limit`. Nothing
             read it. A source told to stay under 24 MiB/s ran at 116 MiB/s.
Relevance:   It is the flag an operator uses to leave headroom on a mirror they
             do not own. A cap that is accepted and ignored is worse than one
             that is refused, because the caller believes they set it.
Approach:    A token bucket per source in `webseed::fetch::Fetcher`, refilled
             continuously, holding one second of burst. Tokens are taken before
             a request goes out rather than after its bytes arrive: a limiter
             that lets the bytes land and then sleeps has not limited anything
             the mirror can see.

             Two details worth keeping. The bucket may go negative, because a
             request larger than a second of burst can never be satisfied from
             a full bucket and taking what it needs and waiting out the deficit
             is what keeps the average right rather than deadlocking. And the
             cap is on bytes off the mirror, so it is taken where a window is
             fetched and not where a block is served: a block answered from the
             window cache crossed no wire.
Acceptance:  A 256 MiB payload under `--web-seed-speed-limit 24MiB/s` takes
             about ten seconds rather than one, and the unit tests pace the
             bucket on a paused clock.

Found while building the [T-003](webseed.md) acceptance, which needed a slow
mirror and did not get one. Under `--web-seed-speed-limit 24MiB/s`, a 256 MiB
payload took 1,114 ms before the fix, about 116 MiB/s, and 10,192 ms after it,
about 25 MiB/s. Reproduce either with:

```
$ bit-cli download <TORRENT> --web-seed <URL> --web-seed-only \
    --web-seed-speed-limit 24MiB/s --json
```

The acceptance run itself is uncapped, because a cap decides the split by
itself, so the committed report under `bench/` does not carry this number.

The unit tests are `webseed::fetch::tests::a_rate_limit_paces_after_the_first_second_of_burst`,
which times the bucket on tokio's paused clock so the assertion is about the
delay the limiter asked for rather than how busy the machine was, and
`a_source_limit_becomes_a_fetcher_rate`, which proves the cap reaches the
fetcher from the spec.

Making the bucket testable needed one decision: it reads `tokio::time::Instant`
rather than `std::time::Instant`, so it refills on the same clock its own
sleeps advance. Outside a test the two are the same clock. Under a paused one
they are not, and a limiter that refills on a clock its sleeps do not advance
cannot be tested at all.

This is not [T-031](#t-031-the-rate-limit-did-not-apply-to-the-session), which
is the session-wide `--max-download-rate` and `--max-upload-rate`. **That one
is done**, measured: 4.10 MiB/s against a 4 MiB/s cap and 223.39 MiB/s
uncapped, over three paired runs. This sentence said "still open" and named
`--max-overall-download-rate` as part of T-031, and both halves were wrong.
T-031's own Closed note excludes the two `--max-overall-*` flags explicitly,
and nothing owned them until [T-181](cli-surface.md), which is where they are
now: accepted, and reaching no code at all.

---

### T-036 A multi-file torrent with one file lands without its directory

Source:      the [T-030](#t-030-throughput-collapses-with-several-torrents-at-once) measurement
Category:    paths
Priority:    P0
Effort:      S
Status:      **done**

Problem:     `SafeStorage` decided whether a torrent unpacks into a directory
             of its own by counting files rather than by asking whether the
             metainfo carries a `files` list. BEP 3 makes `name` the file's
             name in the single-file case and the directory's name in the
             multiple-file case, and a `files` list holding one entry is still
             the multiple-file case. So a torrent named `album` whose one file
             is `movie.bin` wrote `movie.bin` into the output directory instead
             of `album/movie.bin`.
Relevance:   P0 because it loses data silently. Two such torrents in one
             `download` invocation whose one file has the same name write the
             same path, and both report success: each hash-checks its own
             pieces as it writes them, so each check passes at the moment it
             runs and the bytes are gone afterwards. It is the same failure
             [T-072](windows.md) fixed for names that collide only on NTFS,
             reached by a different route.
Acceptance:  A torrent with a one-entry `files` list unpacks into its own
             directory, a torrent with no `files` list does not, and two of the
             first kind carrying the same file name both land intact.

The fix is one line in `storage::subfolder_for`: the multiple-file case is
`metadata.info.info().files.is_some()`, not `file_infos.len() >= 2`. Everything
else about the function is unchanged, and for a torrent with two or more files
the behaviour is identical, because such a torrent always has the list.

`bit-cli info` already reported this correctly, which is what made the
discrepancy findable: the same torrent reads `"multi_file": true,
"file_count": 1`.

`aria2c` 1.37.0 is the external check. Given the same torrent it creates the
directory:

```
$ aria2c --dir=out payload0.torrent
$ find out -type f
out/payload0/movie.bin
```

Before the fix, `bit-cli download` on four such torrents in one invocation:

```
$ find out -type f
out/movie.bin
```

One file, 128 MiB, for four torrents of 128 MiB each, and the run reported
`"completed": 4, "failed": 0`. After:

```
$ find out -type f
out/payload0/movie.bin
out/payload1/movie.bin
out/payload2/movie.bin
out/payload3/movie.bin
```

and every one hashes equal to its source.

Three tests in `crates/bit-cli-core/tests/hostile_paths.rs` hold it, and the
first two are a pair because either half alone would pass with the rule
inverted:

```
$ cargo test -p bit-cli-core --test hostile_paths
test a_one_file_multi_file_torrent_still_gets_its_directory ... ok
test a_single_file_torrent_gets_no_directory_of_its_own ... ok
test two_one_file_torrents_with_the_same_file_name_do_not_collide ... ok
test result: ok. 11 passed; 0 failed
```

The third drives the failure end to end: one session, one output directory, two
torrents whose single file is `movie.bin` in both, and both files present
afterwards.

`scripts/interop-roundtrip.ps1` passes against `aria2c` 1.37.0 and `rqbit`
9.0.0 after the change, which is what says the layout still matches what other
clients produce.

---

### T-037 A run stalls for minutes, roughly once in fifty

Source:      the [T-030](#t-030-throughput-collapses-with-several-torrents-at-once) measurement
Category:    performance
Priority:    P1
Effort:      M
Status:      **done**, by the acceptance's second branch

Problem:     One `-j 2` run of four 128 MiB torrents took 274,546 ms where the
             same command usually takes about 3,200 ms. It completed, and every
             byte arrived. CPU time over that run was 5,155 ms, so the process
             was waiting rather than working for four and a half minutes. The
             run is in `bench/multi-torrent-20260820T071833862Z.json` under
             `runs`, taken before either [T-030](#t-030-throughput-collapses-with-several-torrents-at-once)
             fix and therefore with a shorter `commands` list than the script
             writes now.
Relevance:   This is the second half of what [T-030](#t-030-throughput-collapses-with-several-torrents-at-once)
             reports: "start-stop-start-stop behaviour where the rate drops to
             zero and only a pause and resume clears it". The first half is
             fixed and measured; this is not.
Approach:    It has been seen once in about seventy runs and has not been
             reproduced deliberately. What has been ruled out:

             - **Ephemeral ports.** Ten alternating `-j 4` and `-j 2` runs
               moved `TimeWait` from 276 to 500 against a 16,384-port dynamic
               range, with `CloseWait` at zero throughout.
             - **A repeat of the same shape.** Sixty runs stepping `-j` from 1
               to 4 with `--log-level info` produced no run over 8.1 s, and
               that one is explained by the reconnect backoff below.
             - **The `-j` semaphore.** The permit is held for the whole
               download and released when the worker ends.

             What is worth trying next, in order:

             1. The bridge's reconnect backoff is `RECONNECT_BASE` 1 s doubling
                to `RECONNECT_MAX` 30 s, and it never gives up on a link
                failure. Nine consecutive failures is 274 s, which is the
                observed number. Recording every reconnect in the report, with
                the reason, would say whether that is what happened. The
                8,144 ms run in the sixty-run sweep is the same signature at
                three failures.
             2. If it is the reconnect loop, the question becomes why the link
                fails: the bridge dials the session's own listener, and the
                session, the listener, and the torrent are all live by then.
             3. A bound on the loop. A bridge that has reconnected N times
                without serving a byte is not going to, and it should say so
                and fail rather than retry until the run's deadline.
Acceptance:  Either a deliberate reproduction with the log showing where the
             time went, or a bridge that reports its reconnect count and reason
             in `--json` plus a run of at least two hundred invocations with
             none over five times the median. The report and the command go
             here either way.

**The second branch, and the tail is 1.25 times the median over four hundred
invocations.**

**The instrument first.** A bridge now counts every time its connection to the
session ended and it made another, what it waited to do so, and what ended the
attempt before it. `BridgeStatus::record_reconnect` charges one reconnect to a
reason, and `SourceReport` carries `reconnects`, `reconnect_wait_ms`, and
`reconnect_reasons` per source, summed across the connections that source is
presented over. The reasons are the `BridgeError` variants rather than their
text, so a report groups by what happened: `disconnected`, `link`, `stalled`,
`cooldown`.

That is the number the entry was missing. A stalled run and a slow one look the
same in the byte counts, and different here: the backoff is `RECONNECT_BASE`
1 second doubling to `RECONNECT_MAX` 30, so the delays are 1, 2, 4, 8, 16, 30,
30, and thirteen consecutive failures is **271 seconds**. The entry said nine,
which is 61 seconds, and the arithmetic is in
`the_reconnect_backoff_doubles_to_a_thirty_second_ceiling`. Thirteen is the
number that matches the 274,546 ms run.

**The measurement.** `scripts/check-stall.ps1` runs one fixed command N times
and reports the distribution rather than a mean, because a mean says nothing
about a tail: T-037's run was 85 times the median and no average over the same
sixty runs would have shown it. The shape is the one the stall was seen in,
four 128 MiB torrents at `-j 2` off a loopback file server.

Two sweeps of 200, differing only in `--web-seed-connections`:

| connections | median | p95 | p99 | slowest | max/median | reconnects |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 1494 ms | 2050 ms | 2581 ms | 2972 ms | 1.99 | 0 |
| 4 | 957 ms | 1077 ms | 1145 ms | 1201 ms | **1.25** | 0 |

```
$ pwsh -NoProfile -File scripts/check-stall.ps1 -Runs 200 -PayloadSize 128MiB
```

The second row is the exact shape `scripts/check-multi-torrent.ps1` runs, which
is where the stall was seen, four connections per source being that script's
default. `bench/stall-20260820T134721637Z.json` is that run.

**Zero reconnects in 400 invocations.** Not one bridge lost its connection to
the session in 1600 torrent-runs. That is what makes the counter worth having:
a healthy run reports nothing, so a future run reporting anything is anomalous
by construction rather than by comparison with a baseline nobody wrote down.

Three things this settles and one it does not.

- **The median moved, and that is expected.** The entry's "about 3,200 ms" was
  measured before [T-030](#t-030-throughput-collapses-with-several-torrents-at-once)
  and [T-036](#t-036-a-multi-file-torrent-with-one-file-lands-without-its-directory)
  were fixed. The same command now runs in 957 ms at four connections. The
  ratio is what the acceptance asks for and it does not depend on the baseline.
- **The reconnect loop is not what a healthy run does.** The hypothesis in the
  Approach was that a run of link failures at the doubling backoff explains the
  274 seconds. It remains the only mechanism in the code that produces exactly
  that shape, and it is now reported, so the next occurrence names itself.
- **The 8,144 ms run in the earlier sixty-run sweep** was attributed to the
  same backoff at three failures. Three failures is 1 + 2 + 4 = 7 seconds, which
  fits.
- **What this does not do is reproduce it.** 400 invocations produced no run
  over twice the median. The acceptance's first branch stays unreached, and
  what closes this entry is the second, which asks for the instrument and the
  distribution rather than the reproduction.

Point three of the Approach, a bound on the loop, is not implemented and is not
needed by this acceptance. A bridge that has reconnected many times without
serving a byte is bounded today by `--web-seed-max-errors` on the fetch side
and by `--timeout` or `--stop-timeout` on the run. If the counters ever show a
run of link failures in the wild, that is when the bound gets written, against
a number rather than a guess.

---

### T-242 The request depth is a constant, and the run sits at 40 percent of it

Source:      `RESEARCH.md` entry 13 re-mined at
             `4a1acdf8f196328c7ca284368e0f6652540d1a99`, 2026-08-24, against
             [T-001](webseed.md)'s own measurement
Category:    performance
Priority:    P2
Effort:      M
Status:      open

Problem:     `librqbit`'s request window is a fixed 128 blocks and nothing
             moves it. [T-001](webseed.md) measured that a bridge run reaches
             that peak and then sits at **40% of what it would allow**, so the
             window is not the bound and holding it constant is not free
             either: it is too deep for a slow supplier and too shallow for a
             fast one, and neither case can be told apart in the report.

Premise:     Somebody else derives it. `seedchamp/docs/design.md:197` sizes the
             per-peer depth from an exponential moving average of that peer's
             own wire rate, `desired = 5 s * rate / 16 KiB`, with a configured
             initial depth and a configured cap. Five seconds of that peer's
             own throughput, in blocks.

             That is bandwidth-delay product sizing with the delay fixed, and
             the choice of five seconds is theirs and unexplained. It is also
             **unmeasured**: `seedchamp/docs/` carries defaults and invariants
             and no numbers from a run, and its `bench/` harness has no results
             committed. So this is a design to test, not a result to copy.

             `bit-cli` is the other way round. `bench leech` already reports
             `summary.pipeline` with a `window_ceiling`
             ([T-090](bench.md)), so the instrument for judging this exists
             before the change does.

Approach:    Measure first, which is the only defensible order given the
             premise above is somebody else's default.

             Three depths on one fixture at one supplier rate, then three
             supplier rates at one depth, from `scripts/bench-leech.ps1`. If
             the best depth is the same across supplier rates, the constant is
             right and this entry closes as a correction with the curve under
             it. If it moves, the derived depth is worth building and the
             measurement says what the coefficient should be rather than
             taking five seconds on trust.

             `scripts/bench-leech.ps1` cannot hold the depth fixed today. That
             is the first change and it is small: the depth is
             `librqbit`'s and `vendor/` is this repository's, so it is a
             parameter rather than a patch to negotiate.

Prove:       ```
             pwsh -NoProfile -File scripts/bench-leech.ps1 -Json bench/pipeline-sweep.json
             ```

             The committed report must carry both sweeps and the depth that won
             each row. A comparative claim without a committed benchmark does
             not ship here, which is why the sweep is the acceptance rather
             than the feature.

Notes:       The second half of entry 13's staging model is **not** this entry.

**Ruled on 2026-08-24: run the sweep first.** Which is this entry's own
Approach, so nothing about it changes; what changed is that it is authorised
rather than waiting. The alternative put to the operator was to adopt
seedchamp's five-second EMA now and measure afterwards, and it was refused.

             `seedchamp/docs/design.md:199` caps a per-torrent buffer pool per
             peer, at `ceil(N/16)` of the freelist and at most two pieces when
             the piece length is 4 MiB or more. [T-041](memory.md) closed on
             the equivalent for HTTP sources: a total budget across sources
             rather than per source. What `bit-cli` does not have is the
             **per supplier fraction**, which is what stops one slow supplier
             holding the whole pool. It is worth a separate entry once this one
             has a curve, because the two share a fixture.
