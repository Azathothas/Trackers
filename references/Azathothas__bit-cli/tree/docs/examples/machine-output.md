# Consuming the output from a script

Two shapes, and they are for different jobs.

`--json` writes one document when the run ends. `--jsonl` writes one JSON
object per line as things happen. Both go to stdout, both are UTF-8 with no
BOM, and neither is affected by whether stdout is a terminal.

**Select by `type` or by `kind`, never by position.** A new event type can
appear in any release and a field can be added to any document. Nothing
promises that the third line is the one you want.

## What a run emits

From a real download of a three file, 1.47 MiB torrent from a loopback seeder:

```bash
bit-cli download album.torrent --dir out --report-interval 2s --jsonl
```

```text
      1 "type":"session_start"
      1 "type":"torrent_added"
      1 "type":"metadata_resolved"
     31 "type":"progress"
      1 "type":"torrent_completed"
      1 "type":"session_end"
```

The first line, in full:

```json
{"at":"2026-08-24T09:26:36.647Z","directory":"...\\leech","listen_addr":"[::]:6881","max_concurrent_downloads":1,"seq":0,"sources":1,"type":"session_start"}
```

Three fields on every event and they are the ones a consumer relies on:

| field | what it is for |
| --- | --- |
| `type` | what happened. Switch on this |
| `seq` | a monotonic counter from 0. A gap means a line was lost |
| `at` | ISO 8601 UTC with millisecond precision |

`listen_addr` on `session_start` is how a script learns the port when
`--port 0` was passed. Every acceptance script in `scripts/` reads it from
there rather than from a socket table, because a uTP listener is a UDP socket
and `Get-NetTCPConnection` cannot see it.

## What a tick carries, and what only the final document carries

A `progress` event is a tick of `--report-interval`, and it describes the run
**right now**. A `seed` tick's `peer_detail` is therefore the peers the session
is holding at that instant: the three states it reports a count for, `live`,
`connecting` and `queued`. The length of the array is
`peers.live + peers.connecting + peers.queued` from the same event, which is a
cross-check worth making.

Peers that have disconnected are history and they are in the final document,
under `peers`, which is where a caller counting who ever connected already
looks. `peers.seen` in every tick is the running total.

So do not read a tick's array to count a swarm. `peers.seen` is that number and
it is in the same event.

What the split buys is a tick whose size follows what is connected rather than
how long the run has been going. On a seeder that peers arrive at and leave,
the difference is the whole history: at the last sample of a six hour run,
none of the rows a tick would otherwise carry described a connected peer.

## Waiting for the port, in PowerShell

```powershell
$seed = Start-Process -FilePath bit-cli -NoNewWindow -PassThru -ArgumentList @(
    "seed", "album.torrent", "--data", "payload", "--port", "0", "--jsonl"
) -RedirectStandardOutput seed.out

$listen = $null
$deadline = (Get-Date).AddSeconds(60)
while (-not $listen -and (Get-Date) -lt $deadline) {
    foreach ($line in @(Get-Content seed.out -ErrorAction SilentlyContinue)) {
        if (-not $line -or -not $line.Trim().StartsWith("{")) { continue }
        try { $event = $line | ConvertFrom-Json } catch { continue }
        if ($event.listen_addr) { $listen = $event.listen_addr; break }
    }
    if (-not $listen) { Start-Sleep -Milliseconds 200 }
}
```

Two things that look like noise and are not. The `try`/`catch` around
`ConvertFrom-Json` handles a partially written final line, which happens when
the file is read while the process is still appending. And the loop waits on
**the condition** rather than sleeping a guessed number of seconds, which is
the rule in [`../../TODO/RULES.md`](../../TODO/RULES.md) section 5 that has
cost this repository seven red CI jobs.

## Reading the final document

```bash
bit-cli download album.torrent --dir out --json
```

Every byte figure is an object rather than a bare number:

```json
{
  "downloaded": { "bytes": 1543000, "human": "1.47 MiB" },
  "from_peers": { "bytes": 1543000, "human": "1.47 MiB" },
  "from_web_seeds": { "bytes": 0, "human": "0 B" },
  "from_resume": { "bytes": 0, "human": "0 B" }
}
```

**Read `.bytes` and never the pair.** The formatted string is beside the
integer rather than instead of it, which is
[`../../TODO/RULES.md`](../../TODO/RULES.md) section 5's output rule. A
consumer that parses `"1.47 MiB"` is parsing a presentation decision.

The three `from_*` figures add up to `downloaded` and are what says where the
bytes came from. A resumed download that charged its existing bytes to the
swarm was a real defect here, and these three fields are what made it visible.

## The same numbers without a JSON parser

Every figure above is in the text rendering too, behind `--stats`:

```bash
bit-cli download album.torrent --dir out --stats
```

```
completed            1
disk.bytes_written.bytes 444700
disk.bytes_written.human 434.28 KiB
disk.write_calls     32
disk.write_ops       20
disk.write_time.human 0ms
disk.write_time.ms   0
downloaded.bytes     444700
downloaded.human     434.28 KiB
elapsed_human        3s
elapsed_ms           3655
from_peers.bytes     0
from_web_seeds.bytes 444700
process.cpu_ms       77
process.open_handles 245
process.peak_rss_bytes 30240768
```

One line per field, named the way [`../schema.md`](../schema.md) names it, so a
line here and a row there are the same field. A field the run did not produce
is absent rather than printed as `null`, and an empty array prints as `[]`
because "this run had none" is an answer.

**It is a rendering flag and nothing else.** It takes no measurement, changes
no behaviour, and leaves `--json` byte for byte identical. Every number it
prints was already computed and already in the document; the usual summary is a
selection from it.

`disk.write_ops` over `disk.write_calls` is the coalescing factor: in the run
above, 20 writes reaching the device for the 32 the session asked for. The
ratio moves from run to run, because what can be combined depends on the order
blocks arrive in. `disk.write_time` is wall time inside those writes, summed
across workers, so it can exceed the run's own elapsed time on a machine with
several of them.

## Exit codes are the other half

```bash
bit-cli download --not-a-flag
```

exits **2**, usage. Every code, its meaning, and whether a retry could succeed
are in [`../../man/bit-cli.json`](../../man/bit-cli.json) under `errors`, and
in [`../exit-codes.md`](../exit-codes.md).

**Read the exit code from the process that produced it, unpiped.** A check
piped into anything reports the pipeline's status.

## The schema

[`../schema.md`](../schema.md) documents every field of every document kind,
and `--schema-version` prints the version the binary emits. A field is added
without a version bump; a field is never removed or retyped without one.
