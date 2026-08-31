# Measurement, load generation, and telemetry

Twenty-six issues touch metrics and statistics. This file is mostly forward
work: `bench` is a first-class deliverable by decision 7.12 and it is the
largest unbuilt piece.

---

### T-090 bit-cli bench is not implemented

Source:      the operator's brief
Category:    bench
Priority:    P0
Effort:      XL
Status:      **done**

Problem:     `bench leech|seed|webseed|swarm|probe` parse, appear in `--help`,
             and fail with a message pointing here. `webseed probe` covers part
             of what `bench webseed` should do, and nothing else exists.
Relevance:   Decision 7.12: `bit-cli` is a measurement instrument as well as a
             client, held to the same standard as the download path.
Approach:    Build in this order, because each reuses the last:
             1. The report envelope and environment capture (T-091), which
                every subcommand needs. **Done.**
             2. `bench webseed`, which is `webseed probe` plus the envelope,
                `--baseline`, and `--fail-under`. **Done.**
             3. `bench leech`, which is `download` plus the time series.
                **Done.**
             4. `bench seed`, which is `seed` plus the time series. **Done.**
             5. `bench probe`, a one-shot reachability check. **Done.**
             6. `bench swarm`, the synthetic load generator, which is the
                largest and should come last. **Done**, and it is the largest
                thing in this file:
                [T-092](#t-092-bench-swarm-has-no-synthetic-load-generator).

             `bench disk` was added to this list after the fact, by
             [T-017](disk-io.md), which needed the disk measured on its own and
             found the envelope already there to put it in. **Done.**
Acceptance:  Each subcommand writes a report with the metrics A3.11 lists, and
             `--fail-under` set above the observed rate exits 14.

Done so far:

`bench webseed` is built. It reads real payload out of each source's scope and
drops it, so it measures the transport and nothing else: no piece is written,
no hash is checked, and no retry or cooldown runs, because a retry that hides a
failure also hides it from the measurement.

`--fail-under` exits 14 above the observed rate and 0 below it, against a
32 MiB payload on the loopback file server:

```
$ bit-cli bench webseed .tmp/bench/payload.torrent --web-seed $URL \
    --duration 2s --warmup 500ms --fail-under 100GiB/s --format text
threshold              100.00 GiB/s required, 4.23 GiB/s observed: not met
$LASTEXITCODE = 14

$ bit-cli bench webseed .tmp/bench/payload.torrent --web-seed $URL \
    --duration 2s --warmup 500ms --fail-under 1MiB/s --format text
$LASTEXITCODE = 0
```

Every metric A3.11 lists is in the report except the ones that only a peer
carries: choke and unchoke events, request queue depth, and piece verification
have fields and a recorder path, and `bench leech` and `bench seed` are what
will populate them.

`bench leech` is built. It is `download` with the clock and the counters on,
and it answers the question a rate on its own cannot: whether a slow download
was waiting on the network, on the hash, or on the disk. Three measurements
make that possible, and all three are taken from `bit-cli`'s own code rather
than modelled:

- **Verification.** `bit_cli_core::storage::SafeStorage` brackets each piece
  check. A check is a run of positioned reads walking the piece from its
  start, followed by the session declaring the piece complete, all on one
  thread with nothing awaited in between, so the wall time between the first
  of those reads and that declaration is the whole cost of the check, the
  SHA-1 included. It lands in `summary.hashing`.
- **The disk.** The same storage counts positioned reads and writes, their
  bytes, and their time. It lands in `summary.disk`. Two `Instant::now()`
  calls per operation, always on: a counter that is only on when someone is
  measuring measures a different program.
- **The request pipeline.** `BridgeStatus` counts the blocks the session has
  asked for and not yet been given, the deepest that ever got, and the total
  time from a request arriving to its block going back out. It lands in
  `summary.pipeline`, with `window_ceiling`: what a pipeline held at the peak
  depth would sustain at the measured service time.

Every one of those also appears per interval in `series[].costs`, and in the
CSV columns, so the shape over time is visible and not just the total.

Acceptance, 2026-08-20T04:06:06.879Z, release build, 1 GiB payload on the
loopback file server, five runs per step, medians:

```
$ pwsh -NoProfile -File scripts/bench-leech.ps1 -PayloadSize 1GiB -Runs 5 -ConnectionSweep "1,2,4,8"
```

Report: `bench/leech-20260820T040606879Z.json`.

| Stage | Median | Slowest | Fastest | Share of fetch |
| --- | --- | --- | --- | --- |
| `bench webseed`, no bridge | 855.90 MiB/s | | | 100.00% |
| `bench leech`, 1 bridge | 184.40 MiB/s | 169.73 MiB/s | 204.27 MiB/s | 21.55% |
| `bench leech`, 2 bridges | 314.69 MiB/s | 313.53 MiB/s | 340.20 MiB/s | 36.77% |
| `bench leech`, 4 bridges | 338.40 MiB/s | 313.53 MiB/s | 372.23 MiB/s | 39.54% |
| `bench leech`, 8 bridges | 292.07 MiB/s | 213.20 MiB/s | 340.09 MiB/s | 34.12% |
| control: 1 bridge, 64 requests in flight | 150.37 MiB/s | 126.33 MiB/s | 169.54 MiB/s | 17.57% |

These bridges are the same URL named N times, which is N separate sources.
That was the only way to get N connections when this ran.
[T-009](webseed.md) built `--web-seed-connections` and re-measured: the numbers
there are the shipped flag and they are the ones to quote, because N separate
sources at one URL keep N window caches and pull the payload nearly N times
over.

What that says is written up under
[T-001](webseed.md#the-measurement-bench-leech-took), because it is the
answer to that entry's question. In one line: the cost is the per-peer serial
receive path, not the request window, not hashing, and not the disk until
several paths contend for it.

`--fail-under` above the observed rate exits 14 on `leech` as it does on
`webseed`, covered by
`cmd::bench::tests::a_leech_below_the_threshold_exits_fourteen`.

One refusal was added while building it. A payload already sitting in the
output directory hash-checks clean on add, and the torrent is finished before
a byte is fetched. A rate taken from that run describes the hash checker, so
`bench leech` refuses it and names the directory. The benchmark script hit
exactly this when its own cleanup silently failed, which is how it was found.

`bench disk` is built. It writes a payload through the same
`bit_cli_core::storage::SafeStorage` a download writes through, from N threads,
with no session and no network, so the disk can be measured on its own instead
of inferred from a download doing four things at once. It was built for
[T-017](disk-io.md) and it answered that entry: writes to one file serialise
whatever handle they arrive on, and the serialisation is charged per operation
rather than per byte.

Three layouts make that readable, and the difference between two of them is the
whole measurement: `shared` is one file behind one handle, `handles` is the same
file and the same offsets behind one handle per thread, and `split` is one file
per thread. It fills the same envelope as every other subcommand, adds
`disk_steps` for the per-thread cost a concurrency curve cannot carry, and
exits 7 rather than 0 when a step reads back a block it did not write, because
that is a correctness failure and not a slow one.

```
$ bit-cli bench disk --payload-size 1GiB --concurrency-sweep 1,2,4,8 --format text

Writers
  THREADS  LAYOUT   FILES  RATE           WALL      FLUSH     WRITE TOTAL  MEAN WRITE  OVERLAP
  1        shared   1      2.27 GiB/s     440ms     821ms     423ms        6us         0.96
  2        shared   1      1.57 GiB/s     635ms     412ms     1s           18us        1.93
  4        shared   1      1.65 GiB/s     606ms     915ms     2s           34us        3.73
  8        shared   1      1.46 GiB/s     685ms     1s        4s           75us        7.22
```

`scripts/check-disk-contention.ps1` drives the sweep across all three layouts
and a block-size range, alternating the order so no layout always gets the disk
in the same state, and writes the medians and a verdict to
`bench/disk-contention-<timestamp>.json`.

`bench seed` is built. It is `seed` with the clock on, and every counter faces
the other way from `bench leech`: `uploaded_bytes` per peer rather than
`downloaded_bytes`, and positioned reads rather than writes, because a seeder's
storage cost is reading the payload back.

Three things a leech run has that this one does not, and saying so is the
point of the entry. There is no source list, because a seeder has no HTTP
sources: the rows are the peers. There is no pipeline depth, because the
request window belongs to the side asking. And there is no piece verification
inside the measured window: a seeder hash-checks the whole payload once on add
and then serves it, so `--include-hash-check` is what puts that read into the
report rather than leaving it before the clock starts.

Two refusals. Serving a payload that is not there at all is a missing payload
rather than a slow seeder, so it exits 2 and names the directory. A run where
nobody connected exits 6, the same code a leech run with no usable source
takes, because zero bytes with no peer is not a measurement.

`scripts/bench-seed.ps1` drives one seeder and N leechers on loopback, and the
record is `bench/seed-20260820T144744522Z.json` beside
`bench/bench-seed-20260820T144823484Z.json`:

```
$ pwsh -NoProfile -File scripts/bench-seed.ps1 -PayloadSize 256MiB \
    -Leechers 3 -Rate 8MiB/s -IncludeHashCheck
```

```
peer            kind sent       rate
127.0.0.1:50677 peer 245.94 MiB 6.96 MiB/s
127.0.0.1:50678 peer 246.11 MiB 6.97 MiB/s
127.0.0.1:50679 peer 246.20 MiB 6.97 MiB/s

sent 738.25 MiB at 20.90 MiB/s sustained, 24.09 MiB/s peak;
read 772.83 MiB off the disk over 49152 reads
read amplification: 1.047
hash check on add: 256 pieces, 256.00 MiB in 169ms at 1.48 GiB/s
```

**What that run measures is whether the seeder keeps up with three capped
leechers, not how fast it can go.** The cap is what makes a loopback transfer
last long enough for a one second metrics interval to sample it, and the
sustained rate is bounded by three times 8 MiB/s. Reading 20.90 MiB/s as a
capacity number would be reading the cap. The script says so in its header and
takes `-Rate 0` with a larger payload for the capacity run.

The number worth reading here is **read amplification, 1.047**: 772.83 MiB off
the disk to put 738.25 MiB on the wire, with three peers pulling the same
payload at once. Every byte was read about once, so nothing is re-reading a
piece for a second peer.

`--fail-under` above the observed rate exits 14, checked by hand at
`--fail-under 100GiB/s` against this fixture.

One change to the report envelope came with it. Rates were `Size`, so a field
named `rate` serialized `"human": "2.75 MiB"` where ground rule 0.2 says rates
carry `MiB/s`. `bit_cli_core::units::Rate` is the same wire shape with the
right string, so an older report still reads back and `--baseline` still
compares the same field. `a_rate_and_a_size_share_a_wire_shape_and_differ_in_the_string`
is the test.

That paragraph said "still open: `probe` and `swarm` refuse with exit 1 naming
this entry" and was true of neither by the time anyone read it. `probe` is
built and is described immediately below; `swarm` is
[T-092](#t-092-bench-swarm-has-no-synthetic-load-generator) and closed on
2026-08-22.


`bench probe` is built, which is step 5. It answers the question that comes
before "how fast": is the thing there, and what does it speak. It moves no
payload, so its report carries the environment and the facts and no time
series.

A target is a peer address or an HTTP endpoint, decided from the address
itself. Against a live `bit-cli seed` on loopback:

```
$ bit-cli bench probe 127.0.0.1:51999 --for pb.torrent --format text

Probe
  target               127.0.0.1:51999
  kind                 peer
  reachable            yes
  connect              1ms
  first response       0ms
  peer id              -rQ9000-1%ba%01%06%ad0%b4xM%f5%d0%7f
  client               rqbit 9000
  reserved             0000000000100000
  extensions           extension-protocol
  info hash            echoed
  says it is           bit-cli 0.1.0
  extension messages   ut_metadata, ut_pex
  messages             extended, bitfield, unchoke
  pieces advertised    10
```

Two things that output says which are worth reading twice. The wire peer id is
`librqbit`'s `-rQ9000-` while the extended handshake says `bit-cli 0.1.0`,
because the session is handed a client name and picks its own peer id. And the
reserved bytes claim BEP 10 and nothing else: no DHT bit, no fast extension.
Both are facts about what `bit-cli` puts on the wire, and neither was visible
from inside the tool before this.

Against an HTTP endpoint it is a one-byte ranged `GET`, redirects followed by
hand and reported hop by hop, with the TLS version and cipher when the scheme
is `https`:

```
$ bit-cli bench probe http://127.0.0.1:64341/pb/payload/blob.bin --format text
  status               206
  ranges               supported
  length               292.97 KiB
  http                 HTTP/1.1
```

`--for <SOURCE>` names the torrent a peer is asked about, as a `.torrent`, a
magnet, or an info hash. Without it the handshake carries a zero info hash, a
peer is entitled to hang up on it, and the report says so in a note rather
than leaving the reader to wonder.

A probe ends when the peer goes quiet rather than when the deadline expires.
A peer volunteers its greeting in one burst, and waiting out `--timeout` after
that made every probe cost ten seconds: 8.736s before, 0.546s after, for the
same three messages.

An unreachable target exits 6, `no_usable_sources`, which is what a script
branches on. Four tests cover it, all on loopback: a real seeder read off the
wire, an HTTP endpoint that answers a range, a port nothing listens on, and a
target that is neither.

**Building it found one thing in the fixtures.** `test_support::FileServer`
matched `Range: bytes=` exactly, and every HTTP client writes header names in
lower case, so it had never matched a range in its life: every ranged request
was answered with the whole file and a `200`. Small fixtures still verified,
which is what hid it. It now matches the name case insensitively, and the
probe's `range_support` is the assertion that would have caught it.

Every subcommand is now built, which
`cmd::bench::tests::every_bench_subcommand_is_built` asserts against `clap`.

**Done 2026-08-22**, when
[T-092](#t-092-bench-swarm-has-no-synthetic-load-generator) closed and took the
last of the seven with it. The acceptance has two halves and both hold across
all seven of `webseed`, `leech`, `seed`, `disk`, `probe`, `swarm` and the
report envelope they share. Every subcommand writes a report carrying the
A3.11 metrics, with the exception recorded above: the peer-only metrics are
populated by `leech` and `seed`, which are the two subcommands that have peers.
And `--fail-under` above the observed rate exits 14, measured on the last one
to get it:

```
$ bit-cli bench swarm 127.0.0.1:52891 --for payload.torrent --peers 2 \
    --duration 10s --fail-under 100GiB/s --format text
threshold              100.00 GiB/s required, 195.12 MiB/s observed: not met
$LASTEXITCODE = 14
```

The same run at `--fail-under 1MiB/s` prints `333.33 MiB/s observed: met` and
exits 0.

### T-189 The bench reports are not in the schema contract

Source:      found doing T-018's review, 2026-08-22
Category:    bench
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-22T10:52Z

Problem:     `docs/schema.md` documents every `--json` document and every
             `--jsonl` event, and `schema_gen` fails the build when a field
             appears that the file does not carry. The `bench` reports are not
             among them. `bench leech`, `bench seed`, `bench webseed`,
             `bench disk` and `bench swarm` each write a large JSON report and
             none of their fields is in the contract; the only `bench` entry
             is the `bench_sample` event from `bench disk --jsonl`.
Relevance:   The file's own rule is that "a versioned contract nobody has
             written down is not a contract". These reports are exactly what a
             regression harness reads, `--baseline` parses one back, and
             `scripts/check-swarm.ps1` and `scripts/bench-leech.ps1` select
             fields out of them by name. A field renamed in one of them breaks
             a consumer and nothing fails.
Approach:    **The exclusion is deliberate and it is written down**, so the
             first job is to argue with the reason rather than to assume it
             was an oversight. `schema_gen::collect` already runs
             `bit-cli bench disk --jsonl` and folds in **only its events**,
             with a comment saying why: a `bench` report is "a versioned
             document of its own, with `report_version` and its own `kind`",
             and under `--jsonl` it renders as an NDJSON record carrying
             `record` rather than `type`, so `observe_events` does not pick it
             up.
             The half of that reason which does not hold is
             `report_version`. It is a constant at 1, nothing bumps it when a
             field moves, and the only thing that reads it is `--baseline`
             refusing a report from a **newer** build. So it protects a reader
             from a future format and protects nothing from a rename today.
             Adding a field is harmless either way; what is unprotected is a
             field **renamed or removed**, which is what `check-swarm.ps1`,
             `bench-leech.ps1` and `--baseline` all break on.
             Two ways out, and the entry does not pick one: document the
             reports in `docs/schema.md` beside the rest, starting with
             `bench disk` because the generator already reaches it; or leave
             them out and make `report_version` mean something by failing the
             build when the report's field set changes without it moving.
Acceptance:  `docs/schema.md` carries a section for at least the `bench disk`
             report, `every_produced_kind_and_event_is_documented` covers it,
             and adding a field to that report fails the build until the file
             is regenerated.

**How it was found, which is the argument for the priority.**
[T-018](disk-io.md) added `write_calls` to `bench::report::Disk`, a field every
`bench leech` and `bench seed` report now carries. `BIT_CLI_UPDATE_SCHEMA=1`
produced **no diff at all**, and the schema test passed. A new field in a
document consumers parse went in with the contract check green.

An added field is the harmless case, and that is the point: the same silence
covers a renamed one. `scripts/check-swarm.ps1` selects `swarm.serving.
pieces_announced`, `scripts/bench-leech.ps1` selects
`summary.disk.write_time.ms`, and `--baseline` compares fifteen metrics by
name. Renaming any of them passes every gate in this repository.

The same session added seven fields to the `trackers` document and the schema
test caught every one, so the mechanism works where it reaches. It just does
not reach here.

**Which of the two ways out was taken, and why.** The reports are documented,
which is the half the `Acceptance` names. `docs/schema.md` now carries a
`disk` section generated from a run of its own, and every field a consumer
selects is in it: `summary.disk.write_time.ms` for `scripts/bench-leech.ps1`,
the whole `summary` object `--baseline` compares, and `summary.disk.write_calls`,
the field [T-018](disk-io.md) added with the contract check green.

`report_version` was not made to mean something, and the entry's argument
against it stands: it is a constant at 1, nothing bumps it, and the only reader
is `--baseline` refusing a **newer** report. Documenting the fields protects
against the rename; a version nobody bumps protects against nothing. The two
were never exclusive, and the second is still available if a format break ever
needs announcing.

**A second run, because `--jsonl` pins the format.** `Output::resolve` at
`crates/bit-cli/src/cmd/bench.rs:221-225` sets NDJSON whenever `--jsonl` is
given, whatever `--format` says, so the existing run in `schema_gen::collect`
cannot also produce the JSON document. Folding in the NDJSON head instead would
have documented the wrong thing: `render::ndjson` at
`crates/bit-cli-core/src/bench/render.rs:60-64` empties `series`, `sources`,
`concurrency_curve` and `disk_steps` out of the head and splits them into
records of their own, so the head is missing four of the report's arrays and
carries a `record` field the JSON form does not have. The generator runs
`bench disk --json` a second time, at 16 MiB rather than 64. It costs nothing
measurable: the schema test was **31.21 s** before the change and **31.28 s**
after, because 16 MiB is written well inside the ten second cap.

**What is left out, and the reason it had to be.** `environment`, and nothing
else. Folding it in would have made the contract a record of whichever machine
last regenerated it and turned CI red on the next platform, which is three
jobs: `Test` runs on `ubuntu-latest`, `windows-latest` and `macos-latest`.

- `Os::distribution` is read from `/etc/os-release` and is
  `skip_serializing_if = "Option::is_none"`, so the row exists on Linux and
  nowhere else. `crates/bit-cli-core/src/sysinfo.rs:114-124`.
- The macOS module does not import `Nic` at all, so `host.network` is empty
  there and that one row renders as `array`, where Windows and Linux produce
  the object rows under it. `crates/bit-cli-core/src/sysinfo.rs:869-873`, and
  the three-implementation split is [T-145](cli-surface.md)'s.
- Both `unavailable` lists are `skip_serializing_if = "Vec::is_empty"` and
  appear only when a read failed.

Nothing any consumer selects is under `environment`, so the gap is bounded and
it is written down in the generated file itself rather than only here.

**The header claim that would have become false.** `docs/schema.md` said "every
document carries four fields before its own: `schema_version`,
`bit_cli_version`, `generated_at`, and `kind`". A `bench` report carries
`kind` and `report_version` and none of the other three, so adding the section
without touching the header would have left the file contradicting its own
table one screen further down. The header names the exception now.

Acceptance, met, and the third clause measured rather than argued. Renaming
`Disk::write_calls` to `writes_asked_for` with one `#[serde(rename)]`:

```bash
cargo test -p bit-cli --lib the_committed_schema_matches
```

```
test schema_gen::tests::the_committed_schema_matches_what_the_program_writes ... FAILED
docs/schema.md does not describe 1 field(s) this run produced:
  | `summary.disk.writes_asked_for` | integer |
```

The rename was reverted and the tests are green. Before this change the same
rename passed every gate, and it could not have failed: the file held no row
under `summary.disk` at all, so there was nothing for a rename to go missing
from.

**What it found on the way out: [T-191](#t-191-two-different-documents-answer-to-kind-seed).**
`bit-cli seed --json` writes `kind: "seed"` and so does a `bench seed` report.

### T-191 Two different documents answer to kind seed

Source:      found closing [T-189](#t-189-the-bench-reports-are-not-in-the-schema-contract), 2026-08-22
Category:    bench
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T12:25Z

Problem:     `bit-cli seed <TORRENT> --json` writes a document with
             `kind: "seed"`, holding `data_directory`, `complete` and who
             connected. `bit-cli bench seed --json` writes a report with
             `kind: "seed"` too, holding `report_version`, `parameters` and
             `summary`. They share nothing but the discriminator.
Relevance:   `RULES.md` says anything consuming this output selects by `type`
             or `kind` and never by position, and for these two `kind` does not
             decide which document is in hand. It is also a live hazard in the
             generator: `schema_gen::fold_document` keys the sample map by
             `kind`, so the day somebody folds `bench seed` in beside
             `bench disk`, the two field lists union silently into the one
             section headed `seed`, and the file claims a document that exists
             nowhere. Nothing would fail.
Approach:    Decide whether the report's discriminator should be the bench
             target or the document. `Kind::as_str` at
             `crates/bit-cli-core/src/bench/report.rs:47-55` is the whole
             surface, and changing what it emits is a break in the report
             format, which is what `report_version` is for and would be the
             first thing to bump it. The alternative is to leave the wire
             format alone and make the collision impossible to reach by
             accident: key the generator's document map by something that
             already distinguishes them, and fail rather than merge when two
             runs claim one name. `leech`, `webseed`, `swarm` and `probe` do
             not collide with anything today, so `seed` is the only pair.
Acceptance:  Two runs whose documents share a `kind` cannot merge into one
             schema section without something failing, and a test names the
             pair.

**Done, and the wire format is unchanged**, which is the second of the two
options the Approach names and the one it prefers. Changing `Kind::as_str`
would break the report format for every consumer to fix a hazard no consumer
has met: `report_version` exists for a break worth making and this is not one.

**What fails now.** `fold_document` refuses to fold a document under a `kind`
another **command** already claimed. The discriminator is the leading words of
the sample's label, so `bit-cli trackers <TORRENT> --json` and
`bit-cli trackers <TORRENT> --scrape --json` are one command run two ways and
still merge, which they have to: that is how an optional field gets into the
contract at all. `bit-cli seed` and `bit-cli bench seed` are two commands and
are refused, by name, with what to do about it.

**Both directions are tested**, because a guard that refuses everything is the
same defect wearing a different hat:
`two_commands_cannot_claim_one_document_kind` is the pair this entry is about,
and `the_same_command_with_different_flags_merges` is the case that must keep
working. The first is a `#[should_panic]`, so without the guard it fails rather
than passing quietly.

**The hazard was one call away, and this session walked past it twice.**
[T-065](../TODO/trackers.md#t-065-scrape-is-only-implemented-for-the-bep-48-url-convention)
added a second `trackers` sample to the generator on 2026-08-23, which is
exactly the shape that would have merged had it been a different command. It is
the same command, so it merges correctly, and nothing would have said which of
the two it was.

```
$ cargo test -p bit-cli --lib schema
test result: ok. 11 passed; 0 failed; 0 ignored; 417 filtered out
```

### T-091 Bench reports do not capture their environment

Source:      the operator's brief
Category:    bench
Priority:    P0
Effort:      M
Status:      **done**

Problem:     "A benchmark without its environment recorded is not a result, and
             the `--baseline` comparison is meaningless without it."
Relevance:   Comparing two numbers taken on different machines, or before and
             after a kernel update, without knowing that is how a benchmark
             lies.
Approach:    Capture `bit-cli` version and build metadata (the target triple is
             already recorded by `build.rs`), OS and kernel version, CPU model
             and logical count, total memory, NIC link speed where obtainable,
             the exact command line, and start and end timestamps in ISO 8601
             UTC with millisecond precision. Peak RSS, CPU time, and handle
             count come from [T-042](memory.md).
Acceptance:  `bit-cli bench webseed <TORRENT> --format json` carries an
             `environment` object with every field above populated on Windows
             and on Linux.

`bit_cli_core::sysinfo` reads the machine and the process through the
platform's own interfaces rather than a crate. On Windows:
`K32GetProcessMemoryInfo`, `GetProcessTimes`, `GetProcessHandleCount`,
`GlobalMemoryStatusEx`, `RtlGetVersion`, and `GetIfTable`. On Linux:
`/proc/self/status`, `/proc/self/stat`, `/proc/self/fd`, `/proc/meminfo`,
`/proc/sys/kernel`, `/etc/os-release`, and `/sys/class/net`. The CPU model
comes from the `CPUID` brand string on x86, which is the same string on both
platforms and needs no filesystem and no registry.

Acceptance run, 2026-08-19T23:13:33.253Z, release build:

```
$ bit-cli bench webseed .tmp/bench/payload.torrent --web-seed $URL \
    --format json --duration 10s --warmup 2s --concurrency 8 --request-size 1MiB
```

```
started       2026-08-19T23:13:33.253Z
finished      2026-08-19T23:13:43.264Z
os            Windows 10.0.26200
cpu           12th Gen Intel(R) Core(TM) i7-12700H, 20 logical, x86_64
memory        63.63 GiB
link          Hyper-V Virtual Ethernet Adapter #2 at 1.00 Gbit/s;
              ZeroTier Virtual Port at 100.00 Mbit/s
build         0.1.0 x86_64-pc-windows-msvc release debug_assertions=false
peak_rss      42074112 (40.13 MiB)
cpu_ms        29859
open_handles  219
sustained     2.98 GiB/s
requests      24418 errors 0
series        9 samples, 1 in warmup
```

Two decisions worth recording, both made because the first draft reported a
number that was not true:

- A `dwSpeed` of `0xFFFFFFFF` from `GetIfTable` is the saturation value of the
  field, not a 4.29 Gbit/s link. Every NDIS filter layer and virtual adapter on
  a Windows box reports it. Those rows are dropped rather than repeated as a
  speed.
- `GetIfTable` returns every NDIS binding, so one ethernet port comes back once
  as itself and again for each filter driver over it, all sharing a physical
  address. Rows are deduplicated by MAC, keeping the shortest name, because a
  filter layer is named for its parent with a suffix appended.

`debug_assertions` is in the report because it is the difference between a
number and a number that means nothing, and nothing else in the report would
say so.

The Linux half of the acceptance is not run yet: this machine is Windows and
there is no Linux runner wired up. The code is there and the CI matrix in
`.github/workflows/ci.yml` builds Linux, so adding the assertion to CI is what
closes the gap. Recorded in [T-085](create-seed.md), which has the same shape.

### T-092 bench swarm has no synthetic load generator

Source:      the operator's brief
Category:    bench
Priority:    P1
Effort:      XL
Status:      **done**

Problem:     `bench swarm` is meant to generate synthetic peers and torrents to
             load a target. Nothing exists.
Relevance:   It is how the operator answers "where does my seeding
             infrastructure fall over".
Approach:    The shape worth having is the warmup window, the bounded disk
             budget, the adaptive step search toward a target rate, and
             periodic metrics. Three hard requirements: the disk budget is
             enforced and never exceeded, generated payload lives in the
             scratch directory and is cleaned up, and the tool refuses to
             load-test a host it was not explicitly pointed at.
Acceptance:  `bit-cli bench swarm <TARGET> --peers 100 --torrents 4
             --disk-budget 2GiB --duration 60s` completes, never exceeds 2 GiB
             on disk, cleans up, and refuses to run without an explicit target.

The report envelope, the recorder, the warmup window, and the periodic metrics
are built and shared with `bench webseed`, so what is left is the load
generator itself.

**The target model, decided before any code was written.**

The acceptance names a command with no `--for` in it:

```
bit-cli bench swarm <TARGET> --peers 100 --torrents 4 --disk-budget 2GiB --duration 60s
```

`--torrents 4` says this command generates four torrents. A target that is
someone else's process cannot be serving a torrent this run just invented, and
decision 7.4 rules out a daemon and an RPC, so there is no way to hand it one.
Those two facts cannot both be satisfied by a single load, and the entry could
not be built until that was resolved. It is resolved as **two loads under one
verb, chosen by `--for`**, because both are real measurements and each answers
half of what the entry asks for.

**Leech load, `--for <TORRENT>` repeatable.** The target already serves these
torrents. `--peers N` synthetic peers connect to it, handshake for the info
hash, declare interest, request blocks, and check each piece against the
torrent's own hashes. This is the one that answers the entry's Relevance line,
"where does my seeding infrastructure fall over": bytes out, per-peer rate, how
many peers the target accepts before it stops accepting, when it chokes, and
where the aggregate rate stops rising with peer count.

A swarm is not a hundred leeches, and a load generator that only ever takes is
not the load a seeder meets. A target that superseeds, or that ranks peers by
what they have uploaded, behaves completely differently against peers holding
nothing. So a synthetic peer **keeps** the pieces it has verified, announces
them, and serves them to the other synthetic peers and to the target if it
asks. That is what `--disk-budget` bounds, and it is the only thing in this
command that writes: past the budget a verified block is counted and dropped,
and the report says how many were dropped, because a swarm that stopped growing
is a different measurement from one that did not.

**Connection load, no `--for`.** This is the acceptance's literal command.
`--torrents N` synthetic torrents are generated and the target does not have
any of them, which is the point: what is measured is the accept and handshake
path, not the serving path. How fast the target answers a handshake, how many
connections it accepts before it stops, whether the listener survives, and
whether it strands a socket per rejected connection.

That is not a fallback reading, it is the load that has already broken this
software once. [T-020](peers.md) is exactly this shape: 3000 connections that
closed before handshaking killed `librqbit`'s accept loop in 79 seconds while
the process kept reporting itself as seeding, and the half of T-020 that is
still open is that those connections strand a socket about half the time.
`bench swarm` with no `--for` is the tool that measures that against a host
rather than against a fixture.

**What generation does and does not produce.** A generated torrent is an info
dictionary and nothing else: a name, a length, a piece length, and piece
hashes. No payload bytes are written for it, because nothing will ever verify
them. `--payload-size` and `--piece-size` decide the shape of that dictionary,
which decides the info hash and the size of the `.torrent`, and the `.torrent`
files are written to the scratch directory so a run is reproducible and so the
operator can add them to a target and come back with `--for`.

**The deviation, recorded.** In connection mode `--disk-budget 2GiB` bounds
kilobytes: four torrents describing 256 MiB at 1 MiB pieces are about 20 KiB of
piece hashes between them. The budget is enforced and the bytes written are
counted and reported either way, so "never exceeds 2 GiB on disk" is a measured
number rather than a claim, but in the acceptance's own command it is not a
tight bound. It is tight in leech mode, which is where a synthetic peer holds
real pieces. Both are run as the acceptance rather than only the literal one.

**"Refuses to load-test a host it was not explicitly pointed at."** Read as a
property of the whole run and not only of argument parsing, because a required
positional is something `clap` gives for free and is not worth an acceptance
clause. `bench swarm` dials the target and nothing else, ever: no tracker
announce, no DHT, no PEX, and no peer list read out of a `--for` torrent or out
of the configuration file. The report says which address was dialled and how
many peers reached it, and the acceptance checks that against the target it was
given.

**Built, and where it stands. This is a checkpoint, not a close.**

Both loads are implemented and both work.
`crates/bit-cli-core/src/bench/swarm.rs` is the peer, 2,168 lines with 28 unit
tests, two of which drive the whole peer against a scripted target on loopback
because a real seeder never asks a peer for anything.
`crates/bit-cli/src/cmd/bench.rs` wires it, generates the info dictionaries,
and turns the outcome into notes. `scripts/check-swarm.ps1` drives ten cases
against a live `bit-cli seed`.

The last full run is `bench/swarm-20260821T063418798Z.json`, and its verdict is
**fail on one clause of the acceptance**. Everything else in the entry is met.

What is proven:

| case | result |
| --- | --- |
| `acceptance` | 100 peers dialled, 100 connected, 20,964 bytes on disk against a 2 GiB budget, exit 0 |
| `acceptance_cleanup` | no `--dir`, and zero scratch directories survive |
| `leech_1` | 1 peer, 8 MiB, 32 pieces verified, 0 failed, **333.33 MiB/s** |
| `leech_4` | 4 peers, 33.5 MiB received, held once at 8 MiB, **666.67 MiB/s** |
| `leech_16` | 16 peers, 134.2 MiB received, held once at 8 MiB, **941.18 MiB/s** |
| `budget` | 2,097,152 bytes held **and 2,097,152 on disk** against a 2,097,152 byte budget, 48 refused (2026-08-22) |
| `no_target` | exit 2 |
| `dead_target` | exit 6, four `connect_refused`, no rate reported |

The serving curve is the entry's Relevance line answered: the target's
aggregate rises 1x, 2.00x, 2.82x across 1, 4, and 16 peers, so it stops scaling
between 4 and 16 rather than falling over.

**The one failure, and it is a real one.** `--disk-budget` bounds the bytes
written and not the bytes on disk. A held piece is written at its own offset in
the torrent, so a budget of 2,097,152 bytes accounts for exactly 2,097,152
bytes of piece data and leaves a **4,980,736 byte file**, because the
highest-numbered piece kept was index 18 and `19 * 262144` is where the file
ends. The zeroes in between are allocated on NTFS. The entry's first hard
requirement is "the disk budget is enforced and never exceeded", and measured
as bytes on disk it is exceeded by 2.4 times.

The fix is to hold pieces packed rather than at their torrent offset, with a
map from piece index to slot. `Held::keep` in `swarm.rs` is where it goes.
Nothing reads the held bytes back today, so the offset buys nothing; it was
written that way because it is what a real client does.

**Fixed, 2026-08-22, and it took the shape above.** `Held::keep` writes each
piece at the next free byte of its torrent's file and keeps a per-torrent used
count, so the file is exactly as long as the bytes kept. The `budget` case is
`on_disk_bytes` 2,097,152 against a 2,097,152 byte budget where it was
4,980,736, and **`check-swarm.ps1` passes for the first time**:
`bench/swarm-20260822T055731078Z.json`, `verdict: pass`, nine cases and no
failures. The two committed records before it, from 2026-08-21 and from earlier
today, both say `verdict: fail` with this and only this.

That is worth naming on its own. An acceptance script that always fails cannot
tell a new failure from the known one, and this repository's own rule is that a
script measuring an open defect carries `judged: false` rather than failing the
build. `listener_poisoned` follows that rule and `budget` did not, so for two
sessions the script's exit code said nothing. Fixing the defect was the better
of the two ways out.

Two things came with it.

**The unit test that should have caught this passed.** `the_budget_is_never_crossed`
kept pieces 0 through 9 in order, so the last piece kept was also the highest
and the file ended exactly where the budget did. Real peers do not arrive in
order, and `peers_do_not_all_start_at_the_same_piece` in the same file is this
tool making sure of it, so the test's own fixture was the one shape that could
not fail. `pieces_kept_out_of_order_do_not_make_the_file_longer_than_the_budget`
is the replacement, and the old test now asserts the length exactly rather than
`<=`.

**Three questions are answered under one lock now**, where they were under two
and an atomic: has this piece been kept, does it fit, and where does it go. The
budget claim used to happen before the write, and a write that then failed gave
the bytes back but left the piece marked as kept, so it could never be retried.
Answering all three together is also what makes packing correct, because the
offset a piece gets and the bytes the budget counts have to be decided in the
same breath.

The hold file is now truncated on open. Packed from zero, a leftover from an
earlier run into the same `--dir` would be counted as this run's bytes on disk.

**`pieces_dropped_over_budget` reads 48 in the `budget` case where it read 24,
and the new number is the right one.** A piece used to be marked as held before
the budget was checked, so the second peer to verify a piece the budget had
already refused hit the dedup and returned without being counted. It is counted
now, because it was verified and it was not kept and the budget is why. The
case is two peers over a 32 piece payload at a quarter of it: 64 verified, 8
kept, 8 duplicates of the kept ones, and 48 refused.

**The serving side, built 2026-08-22.** A synthetic peer is a peer on the
connection it already has rather than a downloader. After the handshake it
sends a bitfield, which is all zeros because it starts with nothing, then
unchokes the target, then declares interest. Every piece it verifies and keeps
is announced with a `have`, and a request for one of those pieces is answered
out of the packed hold file. A request for anything else is refused: a BEP 6
`reject request` where the target negotiated the fast extension, and silence
where it did not, because BEP 3 has no way to decline.

Three rules fell out of building it, and each is a test.

- **A piece the budget refused is not announced.** `Held::keep` reports back
  whether the bytes are on disk when it returns, and only then does a `have` go
  out. A peer that announces what it cannot serve spends the target's requests
  on refusals. The `budget` case is the measurement: 8 pieces on disk, 2 peers,
  **16 announced**, and nothing for the 48 that were dropped.
- **Packing had to become reversible.** The store kept a set of the pieces it
  held; it keeps the offset each one landed at now, because a piece is at the
  byte it was written to rather than at the byte it occupies in the torrent.
  Nothing read the held bytes back before this, which is what let the offset go
  unrecorded.
- **A peer that has everything stays only if the target wants something.** It
  stopped at `complete` before; it stops at `complete` unless the target
  declared interest, and then it stays to the deadline serving. Against a
  seeder, which is never interested, the three leech cases finish the moment
  they complete exactly as they did.

**What a synthetic peer cannot do, and the entry's target model got this
wrong.** The model says a peer serves its pieces "to the other synthetic peers
and to the target if it asks". The second half is built. The first half cannot
be, and neither half can put a byte into the target:

- **A synthetic peer has exactly one source, which is the target.** Everything
  it can announce is something the target served it, so the pieces it can offer
  are pieces the target already has. There is no arrangement of this load in
  which the target is missing something a synthetic peer holds. Measured, over
  the three leech cases: **32, 128 and 512 pieces announced, and
  `peers_asked` 0 in every one**, with `target_interested` 0. The target is a
  seeder and a seeder has nothing to ask for.
- **Serving other synthetic peers contradicts the clause the acceptance
  checks.** Peers would have to dial each other, and "it dials the target and
  nothing else, ever" is the property `sources_ignored` now proves from the
  operating system's socket table. It also measures nothing about the target:
  it is the load generator loading itself.

So what the serving side changes is what the target **sees**, which is the half
the entry's own Relevance line rests on: a target that superseeds, or that
ranks peers by what they hold, is reading the announcement. `pieces_announced`
is the number that says it happened, and `peers_asked` is the number that says
what the target did about it.

**What the acceptance found that is not this entry's defect.** The first full
run reported zero peers handshaked in every leech case and read as a broken
handshake. It is not. The script used one seeder for all cases and ran the
connect load first, and **the connect load leaves the target unable to complete
a handshake for any info hash, including one it is serving**. Measured, against
one `bit-cli seed`:

| step | result |
| --- | --- |
| leech 1 peer | handshaked, unchoked, 8,388,608 bytes |
| connect load, 100 peers, 4 generated torrents | 100 connected, 0 handshaked, 99 `handshake_timeout`, 1 `closed_before_handshake` |
| leech 1 peer, same seeder | **connected, 0 handshaked, 0 bytes** |

The seeder is still alive and still reporting itself as seeding throughout.
That is [T-020](peers.md), which is open, and it now has a case of its own in
`check-swarm.ps1` called `listener_poisoned`, carrying `judged: false` so it
records rather than failing the build. Every other case starts its own seeder.

**One clause of the target model is checked by reading rather than by running.**
`bench swarm` opens exactly one kind of socket, a `TcpStream::connect` to
`options.target`, and `swarm.dialled` in every report is the address it was
given. There is no announce, no DHT, and no peer list read from a `--for`
torrent. What is not yet built is the case that proves it from outside: a run
with a configuration file naming a different peer, showing that peer is never
contacted. `swarm.dialled` makes it a one-case addition to
`check-swarm.ps1` and it is not there yet.

**Two things a review of `swarm.rs` found, neither of which fires against
`librqbit`. Both fixed 2026-08-22.**

`leech` removed an outstanding request as `(piece, begin, length)` using the
length of the block that arrived. A target that answers a 16 KiB request with a
shorter block left the original tuple in `in_flight` forever. The piece still
completed, because `PieceBuffer::place` marks the block received either way,
but the window slot never came back and `Leecher::finished` never saw an empty
`in_flight`, so the peer ran to `--duration` instead of stopping at `complete`.
`in_flight` is keyed by `(piece, begin)` now, which is also all `next_gap`
needs: there is only ever one outstanding request per offset.

And `read_handshake` bounded itself on the run deadline rather than on
`--connect-timeout`. That is right in connect mode, where holding the
connection is the measurement and where `handshake_timeout` is the class 99 of
100 peers report against a poisoned listener. It is wrong in leech mode, where
a target that accepts and never answers cost the whole `--duration` before the
peer said so. The bound is the mode's now, and the cost of it is measured:
`listener_poisoned` took **30.3 s** where it took about 50, because the leech
probe against the poisoned listener gives up at `--connect-timeout` instead of
sitting out its thirty seconds.

**The configuration-file case, and the entry asked for something that does not
exist.** The residue named "a run with a configuration file naming a different
peer, showing that peer is never contacted". There is no such setting.
`ConfigFile` in `crates/bit-cli-core/src/config.rs:82-105` has twenty-two keys
and not one of them carries a peer address, so a config file cannot name a
peer. The same question in the form the configuration surface does have is a
file that turns on every mechanism which **discovers** peers, and the case is
`sources_ignored`:

- `enable_dht`, `enable_pex` and `enable_lsd` are all true in a config file
  passed with `--config`, and the case fails if the file was not read, because
  a mistyped path would leave the run with no config and pass.
- A second seeder serves the same torrent on its own port with local service
  discovery left on, so it announces itself on this machine. Every other seeder
  in the script has it off, which is what keeps those two from finding each
  other and handing this case a connection it cannot attribute.
- The judgement reads the **operating system's socket table** rather than the
  report. `swarm.dialled` is the tool's own claim about itself and this entry
  had been resting on it; `Get-NetTCPConnection` over the running process is
  somebody else's account. Measured: 6 samples, 42 sightings of the target,
  `remote_endpoints` exactly `["127.0.0.1:49294"]`, **0 UDP endpoints**, **0
  listening sockets**, and the second seeder never saw a peer.

**What running it found in the script itself, and it had been passing.** The
first version of `sources_ignored` sampled the decoy's connections into a local
called `$peers`. PowerShell variable names are case-insensitive, so that is the
script's own `$Peers` parameter, and every case after it built its argument list
from whatever the loop last measured, which was 0. `listener_poisoned`'s connect
load exited on `--peers cannot be zero`, opened no socket, poisoned nothing, and
**the case recorded three nulls and passed**. RULES.md section 5 names this
exact trap and it still cost a run.

The case is what let it through, so the case is fixed too. `listener_poisoned`
read only the reports and judged only what they said, so a run that never
happened was indistinguishable from one that found nothing. It records the exit
code of all three runs now and fails when any of them wrote no report, and when
the connect load reached no peers, which is a run that did not happen rather
than a listener that survived. What the reports **say** is still `judged:
false`, because that is T-020 and T-020 is open.

Acceptance, run 2026-08-22:

```powershell
pwsh -NoProfile -File scripts/check-swarm.ps1
```

Exit 0, ten cases, no failures: `bench/swarm-20260822T074823843Z.json`.
`acceptance` completes with 20,964 bytes on disk against a 2 GiB budget,
`acceptance_cleanup` leaves no directory, `no_target` exits 2, and
`sources_ignored` is the whole-run reading of "refuses to load-test a host it
was not explicitly pointed at".

**Two of these cases were resting on a defect, 2026-08-22.** [T-020](peers.md)
closed, and closing it broke both.

`listener_poisoned` is judged now. It carried `judged: false` for as long as
T-020 was open, which the paragraph above ends on, and the exemption comes off
with the entry. The same three runs against one seeder now read:

| step | connected | handshaked | bytes |
| --- | --- | --- | --- |
| leech before the load | 1 | 1 | 8,388,608 |
| the 100 connection load | 100 | 0 | 0 |
| leech after the load | 1 | **1** | **8,388,608** |

against 1, 0 and 0 bytes for that last row in
`bench/swarm-20260821T063418798Z.json`. The case fails the build if the target
stops serving after the load. `bench/swarm-20260822T145312435Z.json`.

`sources_ignored` had to be rebuilt, and the reason is worth keeping. It reads
the operating system's socket table while the run is connected, and it used the
**connect** load because, in the comment it carried, "its peers hold their
connections for the whole duration". They did, but only because the target
could not answer them: that was T-020. With the accept loop draining, the
target closes an unknown info hash immediately, the run has nothing left to do,
and it exits. Measured: **53 ms** for the connect load and 111 ms for a leech,
where one `Get-NetTCPConnection` call is longer than either. The case went from
6 samples and 42 sightings to **1 and 0**, and failed on its own premise, which
is the one thing it was written to do rather than pass quietly.

The window is now made rather than borrowed: the seeder for that case runs with
`--max-overall-upload-rate 512KiB`, so an 8 MiB payload outlasts the run's own
`--duration 6s`, and the load is the leech load. 6 samples, 48 sightings, and
the only remote endpoint is the target. `--duration` is a ceiling and not a
floor, which is why pacing the client with `--target-rate` did not work: the
load still finished in 87 ms, inside its own warmup, and said so.

**The general point, and it is not only about this script.** An acceptance
that needs the system under test to be slow is measuring the defect, not the
behaviour. This one did for two sessions without anybody noticing, because it
passed.

### T-093 --baseline comparison is not implemented

Source:      the operator's brief
Category:    bench
Priority:    P2
Effort:      S
Status:      **done**

Problem:     `--baseline <PATH>` parses and does nothing.
Relevance:   It is what turns a benchmark into a regression test.
Approach:    Read the prior report, compare every summary metric, and print the
             delta with a sign. Refuse to compare reports whose environment
             objects disagree on CPU model or OS, because that comparison is
             not meaningful.
Acceptance:  Two runs, the second with `--baseline` pointing at the first,
             print a delta per metric, and a comparison across different
             hardware refuses with a clear reason.

`bit_cli_core::bench::compare` produces a delta per metric with a sign, a
percentage, and a `higher_is_better` flag so a reader knows which way the sign
points. Fifteen metrics are compared: sustained and peak rate, bytes, requests,
errors, six latency percentiles, peak RSS, CPU time, open handles, and hash
rate where there is one.

A comparison is refused, with the reason named, when the baseline is a
different `bench` subcommand, when its report version is newer than this build
understands, or when the two hosts disagree on CPU model, logical core count,
or OS name. Kernel patch level and total memory do not refuse a comparison: an
OS update is worth measuring across, and a different machine is not.

The refusal is a note in the report and a warning on stderr rather than a
failure, because a run that measured something should not be thrown away
because the baseline beside it was wrong.

Acceptance, run in-process by the test suite:

```
$ cargo test -p bit-cli --lib cmd::bench::tests
```

covering `a_report_written_to_a_file_reads_back_as_a_baseline`,
`a_baseline_from_other_hardware_is_refused_and_the_run_still_reports`, and
`a_baseline_that_is_not_a_report_names_the_file`, plus the unit tests in
`bench::report::tests` for each refusal.

Making this work needed one fix elsewhere: `units::Size` and `units::Millis`
serialized as `{bytes, human}` but deserialized only from a bare integer, so no
document `bit-cli` wrote could be read back. Both now accept either form.

### T-094 Trace output has no measured cost

Source:      the operator's brief
Category:    bench
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T12:45Z

Problem:     "Tracing never changes behaviour or timing in a way that
             invalidates a measurement. If enabling a trace costs measurable
             throughput, say so in the docs and in the bench report."
             Nobody has measured it.
Relevance:   `--trace http` records every request in memory. On a long run that
             is both memory and time.
Approach:    Run `bench webseed` with and without `--trace http` and compare
             sustained throughput and peak RSS.
Acceptance:  Both numbers recorded here, and if the cost is measurable, the
             bench report carries a `tracing_enabled` field and the docs say
             what it costs.

The report already carries `environment.tracing_enabled` and
`environment.trace_subsystems`, so a report taken with a trace on is
distinguishable from one taken without. The measurement itself is what is left.

**Measured, and both of the entry's premises are wrong.**

**"`--trace http` records every request in memory" is not what it does.** It is
a log filter. `filter_directive` raises the `bit_cli::http` target to `trace`
and `record()` in `webseed/fetch.rs` emits one line per ranged GET to stderr,
and to `--log-file` when one is set. Nothing is retained, so there is no
in-memory growth to measure and none was measured.

**And the command the Approach names cannot answer the question.**
`bench webseed` builds its own `reqwest::Client` at
`crates/bit-cli-core/src/bench/webseed.rs:383` and never goes through
`webseed::fetch`, so `--trace http` produces **no records at all** there.
Measured: a traced `bench webseed` run writes one line of stderr, the one
naming the report it wrote. Five traced runs against five plain ones were five
comparisons of a run with itself, which is what the first version of this
measurement did.

`scripts/check-trace-cost.ps1` measures `download --web-seed-only` instead,
where the trace fires, alternating the arms so drift falls on both. Three
configurations, five runs per arm except the middle one, which is seven:

| payload | chunk | trace lines | throughput cost | peak RSS cost | plain spread |
| --- | --- | --- | --- | --- | --- |
| 512 MiB | 1 MiB | 512 | 0.3% | **-3.5 MiB** | 5.0% |
| 1 GiB | 1 MiB | 1,024 | 1.0% | **-7.0 MiB** | 4.2% |
| 1 GiB | 64 KiB | 16,384 | 2.1% | +5.3 MiB | 60% |

The cost is the best traced run against the best plain one, and the spread is
the plain arm's own range as a share of its best. **In every configuration the
difference between the arms is smaller than that spread**, and in two of the
three the traced arm's peak RSS was **lower**, which is what noise looks like
rather than a saving. So the answer to "does enabling a trace cost measurable
throughput" is no, on this machine, up to 16,384 lines in four seconds. The
acceptance's condition is not met, and its consequent is already true anyway:
the report carries `tracing_enabled` and `trace_subsystems` whatever the
answer.

**What the numbers do not cover.** stderr went to a file in every run, which is
the cheap destination and the normal one. A console is slower and a slow one
would show. The cost also scales with lines rather than with bytes, so a run
with small chunks pays more per byte: that is what the 64 KiB row is, and its
spread is why it says nothing more precise.

`bench/trace-cost-512m-20260823.json`, `bench/trace-cost-20260823.json` and
`bench/trace-cost-64k-20260823.json` are the three runs, one per row.

**One thing found on the way is a separate defect and has its own entry.** Ten
of the eleven documented `--trace` subsystems raise a target nothing emits on.
[T-219](cli-surface.md#t-219-ten-of-the-eleven-trace-subsystems-raise-a-target-nothing-writes-to).

### T-148 The peer probe test asserted an exit code inside its own retry loop

Source:      CI run 32407214253, `Test (ubuntu-latest)`, 2026-08-20
Category:    bench
Priority:    P2
Effort:      S
Status:      **done**

Problem:     `cmd::bench::tests::a_peer_probe_reads_the_handshake_and_what_follows_it`
             starts a real seeder on a thread and dials it. It cannot know when
             the listener is up, so it retries, which is right. The retry went
             through the `report` helper, which asserts an exit code:

             ```
             left: NoUsableSources
              right: Success
             ```

             A dial that arrives before the listener binds exits 6, and that is
             `bench probe` working: `an_unreachable_peer_exits_no_usable_sources`
             asserts the same code on purpose. So the first attempt panicked
             and the loop never ran a second one. It passed on Windows and on
             macOS because the seeder happened to bind first.
Relevance:   A test that fails on whichever machine is slower is a test that
             teaches everyone to re-run CI. It also hid the two real failures
             beside it in the same job, [T-147](windows.md).
Approach:    Run the command without asserting inside the loop, treat any
             non-`Success` exit as "not up yet", and assert once at the end
             that a probe connected. Bound the loop by a deadline rather than
             by a count of attempts: 40 attempts is four seconds on this
             machine and an unknown number on a loaded runner.
Acceptance:  The test passes on `ubuntu-latest`, `windows-latest`, and
             `macos-latest` in the same run, and fails with a message naming
             the port when the seeder never binds.

The deadline is eight seconds against the seeder's own `--stop-after 10s`, so
the loop cannot outlive its subject.

### T-149 The last window of a leech bench was never counted

Source:      CI run 32437262089, `Test (windows-latest)`, 2026-08-21
Category:    bench
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `cmd::bench::tests::a_leech_measures_the_transfer_the_hashing_and_the_disk`
             failed on `windows-latest` and passed everywhere else:

             ```
             panicked at crates\bit-cli\src\cmd\bench.rs:2142:47:
             called `Option::unwrap()` on a `None` value
             ```

             The value is `summary.hashing.pieces`, and `hashing` is `None`
             when nothing was hashed. Something had been: the payload landed
             and its hash was checked.

             The sampling loop reads `engine.storage_counts()` at the top of
             its body and decides whether to stop at the bottom. Work between
             the last read and the break is in no interval at all. The
             iteration that ends the loop is exactly the one in which the last
             pieces were verified, so on a run that finishes inside one
             `--metrics-interval` most of the hashing is the part that is
             dropped.
Relevance:   This is a benchmark under-reporting its subject, which is worse
             than a flaky test. Every `bench leech` report has been missing its
             final window of disk operations and piece verification, and the
             shorter the run the larger the share. It is the same lesson
             [T-117](cli-surface.md) recorded for `bench_sample` at a different
             scale: a measurement whose resolution is its own sample interval
             says nothing about a run shorter than one.
Approach:    Read the counters once more after the loop and before
             `recorder.stop()`, and fold the delta in. `observe_disk` and
             `observe_hashing` are plain accumulators with no window gate, so
             the last delta lands in the measured window where it belongs.
Acceptance:  The test passes on all three runners in one run, and a `bench
             leech` short enough to finish in one interval still reports the
             pieces it verified.

```
$ cargo test -p bit-cli --lib a_leech_measures_the_transfer_the_hashing_and_the_disk
test result: ok. 1 passed; 0 failed
```

The test was not changed. It was asserting something true that the report had
stopped carrying.

### T-152 A disk bench shorter than one sample interval reported no series at all

Source:      CI run 32440386139, `Test (macos-latest)`, 2026-08-21
Category:    bench
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `schema_gen::tests::coverage_of_the_documented_names_matches_what_is_recorded`
             failed on `macos-latest` and passed on the other two:

             ```
             the set of names no run produces changed
               left: ["bench_sample"]
              right: []
             ```

             The generator drives `bench disk --payload-size 64MiB
             --metrics-interval 10ms` to produce one `bench_sample`. The
             sampler emits only when an interval boundary passes, and 64 MiB on
             a fast NVMe is about twenty milliseconds against a ten millisecond
             interval. That is a margin of two, and the macOS runner was on the
             wrong side of it: the phase finished before the first boundary and
             the series was empty.
Relevance:   A report whose time series has no points is a measurement that was
             not taken, and nothing said so. The same sampler also dropped the
             window between the last boundary and the end of every longer run,
             so every `bench disk` report has been short by up to one interval
             of writes. It is [T-149](#t-149-the-last-window-of-a-leech-bench-was-never-counted)
             at a different scale, in the other bench target, found the same
             way: by fixing what was above it in the same job.
Approach:    Emit one last point after the writers stop and before the phase
             ends, exactly as `bench leech` now does. The condition is "any
             writes since the last boundary, or no points at all", so a run
             that already ended on a boundary does not gain an empty sample.
Acceptance:  A phase with a metrics interval longer than the phase still
             reports one sample, that sample accounts for the whole payload,
             and the callback sees the same point the series does.

`bench::disk::tests::a_phase_shorter_than_one_interval_still_reports_a_sample`
sets the interval to an hour, which is the same thing every fast disk was
already doing to a ten millisecond one, made deterministic:

```
$ cargo test -p bit-cli-core --lib bench::disk
test result: ok. 8 passed; 0 failed
```

The generator's parameters were left alone. Raising the payload size would have
moved the margin without removing the dependence on it, and the dependence is
the defect.

**The run is in.** `Test (macos-latest)` passed in 2m10s on CI run
32444424026, 2026-08-21, alongside every other job in that run:

https://github.com/Azathothas/bit-cli/actions/runs/32444424026

---

### T-211 Two bench tests fail on the CI runner and pass on every local run

Source:      two red CI runs on main, 2026-08-22
Category:    bench
Priority:    P1
Effort:      M
Status:      **done**, 2026-08-23T03:10Z

Problem:     `cmd::bench::tests::a_leech_measures_the_transfer_the_hashing_and_the_disk`
             and
             `cmd::bench::tests::a_source_over_several_connections_stays_one_row_and_serves_between_them`
             each failed once on `Test (ubuntu-latest)`, on different commits,
             with a green run between them, and neither reproduces here.
Relevance:   A test that fails one run in three is worse than no test: it
             costs a session's attention every time, and the next real
             regression it catches will be read as the flake.
Approach:    Both assert an exact byte or piece count over a loopback swarm
             running to a wall-clock budget, which is the shape
             [RULES.md](RULES.md) section 5 warns about. Find which side of
             each assertion is the scheduling outcome and arrange it instead,
             the way [T-179](webseed.md) did. Do not widen the assertion to
             make it pass.
Acceptance:  Both tests run 50 times under `--test-threads` pressure without
             failing, and the run that proves it is recorded here.

**What was seen, exactly.**

| run | commit subject | test | assertion |
| --- | --- | --- | --- |
| 32592590875 | Receive a message larger than the buffer it lands in | `a_leech_measures_...` | `hashing["pieces"]` was **2**, expected 3 |
| 32594170837 | Keep the hash check between runs without keeping the session | `a_source_over_several_connections_...` | `summary.bytes` was **4024**, expected 3000 |

Both were the only failure in a run of 384 tests, both on `ubuntu-latest`, and
both are `bench.rs`. The push between them, `Name the filter value instead of
parsing it out of a literal`, was green on all sixteen jobs, and so were the
three before them.

**4024 is 3000 plus 1024**, which is one block counted twice. That test
reconnects a source part way through, so a block requested before the
disconnect and served again after it is the obvious candidate. The other
direction, two pieces hashed where three were expected, is the same run ending
before the third piece arrived.

**Locally both pass.** Three consecutive runs of each, release toolchain
1.98.0, and `cargo test --workspace` passes 1,131 tests on every run this
session, which is more than ten.

**What is not known and matters.** Whether a vendored change this session made
it more likely. Both failures land after the vendored work started, and the
changes that could plausibly touch peer accounting are
[T-210](peers.md), which changed the peer id recorded for an **incoming** peer,
and [T-132](multi-source.md), which added a second limiter acquire on the
download path. The green run at 32589619210 already carried both, which is
evidence against but not proof: one green run does not clear a test that fails
one time in three.

**The commit is not the variable, and that is measured.** `Test
(ubuntu-latest)` from run 32594170837 was re-run on its own commit, unchanged,
and **passed**. So the same code fails and passes on the same runner image, and
what differs between them is the run rather than the tree. That clears the
vendored work of causing it and leaves the test itself.

The other one, 32592590875, could not be re-run: it had already been cancelled
by the concurrency group, and GitHub refuses to retry a cancelled run. Worth
knowing before trying: the CI workflow groups by `workflow-ref` with
`cancel-in-progress`, so re-running an older commit's job while a newer push is
in flight cancels one of them, and a cancelled run is then permanently
un-retryable.

**Where to start.** `assert_eq!` on `summary.bytes` is the wrong assertion for a
test that reconnects a source mid-transfer: a block served twice is a
legitimate outcome of a reconnect, and the payload on disk being correct is the
invariant. `hashing.pieces` is the same shape read the other way. Arrange each
so the count is not a race, the way [T-179](webseed.md) did, rather than
widening the comparison until it stops failing.

**Closed 2026-08-23, and the two were different defects.** One was the
benchmark's, not the test's.

**Two pieces where three were hashed was a lost interval, not a lost piece.**
`drive_leech` took its storage baseline at the top of its own body, and by then
the caller had already attached the sources: `attach_sources` returns with the
bridges dialling, and there is an `await` between that and the first counter
read. Every counter the report carries is a sum of interval deltas, so a piece
verified in that gap is in no interval at all and is counted nowhere. The
payload was complete, the bytes were right, and `hashing.pieces` was one short,
which is exactly what the runner reported.

The baseline moves ahead of `attach_sources` and is passed in as
`LeechOptions::storage_baseline`. It cannot move further: the hash check on add
happens before it and a resumed run would otherwise report its pieces as
transfer work.

**4,024 bytes against 3,000 was the wrong assertion.** `summary.bytes` counts
what arrived from the source. With `--web-seed-connections 3` the session can
ask twice for a block that is already outstanding and be answered twice, which
is a legitimate outcome of several connections and is [T-008](webseed.md). An
equality against the payload length was asserting that this never happens,
which is a scheduling outcome the test does not control.

What replaced it is not a widening. The payload on disk is asserted exactly, as
before; the counted total has to be at least the payload; **the one source row
has to equal the counted total**, which is the claim the test is named for and
which an equality against the payload length would have passed with the row and
the summary both wrong by the same amount; and the row has to report **three**
connections, so a run that quietly fell back to one no longer passes as "one
row".

**Proved by running them.** The whole `cmd::bench::tests` module, which is what
the two run beside and contend with for blocking threads, **50 times at
`--test-threads 8`, 0 failures**.

```bash
cargo test -p bit-cli --lib --no-run
```

```bash
for ($i = 1; $i -le 50; $i++) { & (Get-ChildItem target/debug/deps/bit_cli-*.exe | Select-Object -First 1) cmd::bench::tests --test-threads 8 }
```

**What is not proved.** That the runner cannot find a third way. Fifty local
runs is evidence and not a proof, and the first of the two failures was one run
in three on a machine this is not. What makes this closable rather than
hopeful is that both causes were found and named: neither was a tolerance, and
both are defects a reader can check against the code.

### T-223 The leech bench reads its transfer counters before deciding to stop

Source:      CI run 32645146193, `Test (windows-latest)`, 2026-08-23
Category:    bench
Priority:    P1
Effort:      S
Status:      **done** 2026-08-23T15:05Z

Problem:     `cmd::bench::tests::a_leech_measures_the_transfer_the_hashing_and_the_disk`
             failed for the **third** time, and for a third distinct reason:

             ```
             assertion `left == right` failed
               left: 1976
              right: 3000
             ```

             1,976 is 3,000 minus 1,024, which is one block. `summary.bytes`
             is a sum of interval deltas over the peer counters, and
             `drive_leech` read those counters at the top of its loop body and
             read the completion flag near the bottom. A block that landed
             between the two was written, hashed, counted as disk work, and
             counted as transfer **nowhere**: the loop broke on a flag that
             already knew about work no read had taken.
Relevance:   This is the same defect as
             [T-149](#t-149-the-last-window-of-a-leech-bench-was-never-counted),
             in the counter the report is named for, and T-149 is what left it
             behind. That entry added a final read of the **storage** counters
             after the loop and did not add one for the peer counters, so the
             gap it closed for hashing and disk stayed open for the transfer.

             It is a benchmark under-reporting its subject, which is worse
             than a flaky test: every `bench leech` report that ended on
             completion rather than on its deadline could be short by up to
             one interval of transfer, while its disk and hashing totals were
             right. A reader comparing the two would have concluded the disk
             wrote more than arrived.

             Third time for this test and third distinct cause.
             [T-211](#t-211-two-bench-tests-fail-on-the-ci-runner-and-pass-on-every-local-run)
             found two and named both; this is the one neither of them was.
Approach:    Two changes, and the first is the one that makes it impossible
             rather than unlikely.

             **The completion flag is read before the counters.** `finished`
             true then means every read below it happened after the last byte;
             `finished` false costs nothing, because the next tick reads again.
             There is no longer a gap for a block to fall into, and that is a
             property of the ordering rather than of the timing.

             **The transfer counters are read once more after the loop**, the
             way T-149 made the storage counters read once more. The ordering
             above covers the completion break; this covers the other two,
             the deadline and the interrupt, where work can still be in flight
             when the loop ends. The peer-accounting block becomes
             `observe_transfer`, a free function, because it is called twice.
Acceptance:  The test asserts an invariant the failure violated and which is
             not a scheduling outcome, and the module runs many times under
             thread pressure without failing.

**Done.** `crates/bit-cli/src/cmd/bench.rs` carries both changes, each with the
comment that says why the ordering is the fix rather than an ordering.

**The new assertion is the useful part.** `summary.bytes >= summary.disk.write_bytes`:
every byte on the disk came off a source, so the transfer total cannot be under
the write total on a run that started from nothing. Unlike the equality beside
it, nothing about scheduling can lower the left side. A block served twice
raises it, which is [T-008](webseed.md) and legitimate. At the moment of the
failure it was 1,976 against 3,000.

**Proved by running them**, the way [T-211](#t-211-two-bench-tests-fail-on-the-ci-runner-and-pass-on-every-local-run)
was: the whole `cmd::bench::tests` module, which is what this test contends
with for blocking threads, **50 times at `--test-threads 8`**.

```bash
cargo test -p bit-cli --lib --no-run
```

**What is not proved, and it is the same limit T-211 recorded.** That the
runner cannot find a fourth way. What makes this closable is that the cause is
named and the fix is an ordering a reader can check without running anything:
there is no window between the last counter read and the break for the
completion path, because the flag that ends it is read first.

### T-229 A concurrency sweep charged its warmup to its own first steps

Source:      measured while taking [T-033](performance.md), 2026-08-23
Category:    bench
Priority:    P1
Effort:      S
Status:      **done** 2026-08-23T18:27Z

Problem:     `bit-cli bench webseed --concurrency-sweep` divides `--duration`
             across the steps and starts the first one immediately. The
             recorder's warmup, three seconds by default, runs on the same
             clock: it excludes warmup samples from a step's byte count, and
             `end_step` divides by the step's **own wall time**. So a step that
             fell inside the warmup reported its real seconds against no bytes
             and came out at 0 B/s.
Relevance:   The curve is what the command exists to produce, and its first
             point was understated by however much of the step fell inside the
             warmup. At the defaults, `--duration 30s` over five steps is six
             seconds a step against a three second warmup, so the first point
             read about half; the sweeps below used shorter steps and it read
             zero. Ten steps at the defaults is three seconds each and swallows
             the first whole.

             `best_concurrency` is derived from that curve, so the verdict
             could be inverted outright: `--concurrency-sweep 16,1` reported
             **best concurrency 1**, because whichever step went first was the
             one that was crippled.

             It is [T-152](#t-152-a-disk-bench-shorter-than-one-sample-interval-reported-no-series-at-all)'s
             family, a bench reporting a number for a window it did not
             measure, and it is worse than that one because the number is not
             obviously absent. A zero at the left of a concurrency curve reads
             as "one connection gets nothing", which is a plausible result.
Approach:    The code already says what it means to do. The comment above the
             loop reads "the warmup is paid once, before the first step,
             rather than once per step", and nothing paid it: the recorder's
             window is a wall clock and the loop simply ran over it.

             Drive the source at the first step's concurrency until
             `Recorder::in_warmup` is false, **before** the first
             `begin_step`. A sweep then costs the warmup on top of
             `--duration` rather than out of it. A single fixed concurrency is
             left alone: it has no curve and its summary already reads the
             measured window, so warming it separately would add three seconds
             to every run for nothing.
Acceptance:  No step of a sweep issues zero requests when the run served
             any, asserted with a warmup long enough to swallow the first step
             whole. That is the half a test can hold exactly.

             Two steps of the same concurrency reporting the **same rate** is
             the sharper statement and it is measured rather than asserted:
             897.15 against 896.85 MiB/s below, where it was 2.66 against
             908.73. A test of it would need a tolerance on a throughput
             number, which is the assertion
             [RULES.md](RULES.md) section 5 refuses, so it is a row in this
             entry and the run that produced it is repeatable in one command.

**Measured, before and after, on a 64 MiB loopback payload with 20 second
sweeps.** The control is the last row and it is the whole argument: the same
concurrency twice, with nothing about the run to tell the two apart.

| sweep | before | after |
| --- | --- | --- |
| `1,2,4,8,16` at 6s | 0, 0, 1.34, 3.28, 3.38 | 903 MiB/s, 1.51, 2.58, 3.26, 3.26 |
| `4,8` | 5.32 MiB/s, 3.16 GiB/s | 2.75 GiB/s, 3.19 GiB/s |
| `16,1` | 22.49 MiB/s, 930 MiB/s, **best 1** | 3.42 GiB/s, 935 MiB/s, **best 16** |
| `1,1` | **2.66 MiB/s, 908.73 MiB/s** | **897.15 MiB/s, 896.85 MiB/s** |

The rows come from `bench webseed` driven directly, against
`loopback-fileserver` on a payload `bit-cli create` made. There is no script
for it, because a sweep against one loopback source is three commands:

```bash
cargo run --release --example loopback-fileserver -- --root .tmp/split/srv --port 0
```

```bash
cargo run --release -- bench webseed .tmp/split/payload.torrent --web-seed <BASE> --no-torrent-web-seed --concurrency-sweep 1,1 --duration 20s
```

`--concurrency-sweep 1,1` is the one to run: the two rows have to agree, and
before this they did not.

**Why no test caught it, and this is the part worth keeping.**
`bench_webseed_reports_a_concurrency_curve_with_its_own_latency` in
`crates/bit-cli-core/tests/webseed_e2e.rs` already asserts
`step.requests > 0` for every step, and it has always passed. Its options come
from `bench_options`, which sets **`warmup: Duration::ZERO`**. Every test of
the sweep turned off the one thing that breaks it.

`a_sweep_pays_its_warmup_before_the_curve_rather_than_out_of_it` turns it on:
two steps of 300 ms against a 500 ms warmup, so the first step falls entirely
inside the warmup window. Both boundaries are measured from the same
`Instant`, so that is arithmetic rather than a race. Run against the defect it
fails and names the step.

```bash
cargo test -p bit-cli-core --test webseed_e2e -- a_sweep_pays_its_warmup
```

**What the fix costs, written down rather than rounded away.** The requests
already in flight when the warmup closes complete with no step open, so they
are in the measured window and in no step. The bound is exact rather than a
tolerance: at most `concurrency` requests of at most one chunk each, which is
64 KiB in that test and is what it asserts. Against a step reported at 0 B/s
it is a good trade.

#### Correction, 2026-08-24: that bound is not exact and it went red

**"The bound is exact rather than a tolerance: at most `concurrency` requests
of at most one chunk each" is wrong about the code it describes**, and CI run
**32687202487** is where it cost a job. `Test (macos-latest)` failed on

```
114688 bytes fell outside every step, which is more than the
four in-flight requests the warmup handover can leave
```

which is seven chunks against a bound of four. Nothing was broken. The runner
was slow.

**Why the reasoning is wrong.** The warmup is not one drive. It is
`while recorder.in_warmup() { drive(..) }` at
`crates/bit-cli-core/src/bench/webseed.rs:222`, **every iteration spawns
`concurrency` fresh workers**, and a worker that passes its
`Instant::now() < deadline` check starts one more request and finishes it after
the deadline. So the tail is `concurrency` per iteration, and how many
iterations there are depends on how the clock lands, which depends on the
machine. On this one it is usually one iteration; on a loaded shared runner it
is more.

**So the assertion asserted that the machine cannot be slow**, which is the
line [RULES.md](RULES.md) section 5 already carries three worked examples of,
and this is the fourth. The rule beside it is to fix the file rather than the
line.

**What replaced it.** The `total <= summary.bytes` half stays, because a step's
bytes really are a subset of the window's. The invented bound is gone, with the
reasoning above written where it was. In its place is the control the test's own
doc comment already named and never asserted: **the two steps are the same
concurrency against the same server, and with the warmup paid out of the first
they differed by a factor of 340**, so they must now be within an order of
magnitude of each other. That is a wide bound on purpose. The claim is that
neither step was crippled, not that a shared runner is repeatable.

**Run against the defect**, with the warmup pre-payment disabled: the test
fails at `step.requests > 0`, which is the assertion that catches the original
defect. The new control is what covers the partial case, where a step falls
inside **part** of the warmup and is charged some of it rather than all.

```bash
cargo test -p bit-cli-core --test webseed_e2e -- a_sweep_pays_its_warmup
```
