# Performance, and how it is measured

Every comparative claim in this repository rests on a committed benchmark. A
number without a run behind it does not ship, which is
[`TODO/RULES.md`](../TODO/RULES.md) section 5.

The entries behind this are in
[`TODO/performance.md`](../TODO/performance.md) and
[`TODO/bench.md`](../TODO/bench.md).

`bench webseed` reads real payload out of each source's scope and drops it. It
measures the transport: latency percentiles, how throughput moves with
concurrency, and what fails and why. No piece is written and no hash is
checked.

```bash
bit-cli bench webseed album.torrent \
  --web-seed https://mirror.example.com/pub/ \
  --duration 30s --warmup 3s --concurrency-sweep 1,2,4,8,16 --format text
```

```
bench webseed

started                2026-08-19T23:13:33.253Z
finished               2026-08-19T23:13:43.264Z
elapsed                10s

Environment
  bit-cli              0.1.0 (x86_64-pc-windows-msvc, release)
  os                   Windows 10.0.26200
  cpu                  12th Gen Intel(R) Core(TM) i7-12700H (20 logical, x86_64)
  memory               63.63 GiB
  link                 Intel(R) Ethernet Connection (16) I219-LM at 1.00 Gbit/s
  cost                 peak RSS 40.13 MiB, CPU 29s, 219 handles

Summary
  measured over        8s
  sustained            2.98 GiB/s
  requests             24418 (0 failed)
  connect              p50 1ms  p90 1ms  p99 1ms  p99.9 1ms  max 1ms
  first byte           p50 1ms  p90 3ms  p99 18ms  p99.9 23ms  max 24ms
```

The report goes to stdout in `--format`, which defaults to `json`. Pass
`--report <PATH>` to write it to a file instead, and stdout carries the text
summary. `--format csv` writes the time series as one row per sample, which is
the part a plotting tool wants; it carries the series and nothing else, because
a report is nested and a table is not.

Two flags turn a measurement into a check a script can branch on:

```bash
bit-cli bench webseed album.torrent --web-seed $URL --fail-under 50MiB/s
bit-cli bench webseed album.torrent --web-seed $URL --baseline last-week.json
```

`--fail-under` exits 14 when sustained throughput falls below the rate.
`--baseline` prints a delta per metric with a sign and a percentage, and
refuses the comparison, with the reason named, when the two reports were taken
on different hardware. Every report carries the machine, the exact command
line, and what the process cost, because two numbers from two machines are not
comparable and nothing in the number itself says so.

## Measuring a download

`bench leech` downloads the target and reports what it cost. It is `download`
with the clock running, so it takes the same source, tracker, and web seed
flags, and it answers what a rate on its own cannot: whether the run was
waiting on the network, on the hash, or on the disk.

```bash
bit-cli bench leech album.torrent \
  --web-seed http://127.0.0.1:52466/ --web-seed-only \
  --dir ./out --port 0 --warmup 0s --metrics-interval 250ms --format text
```

```
Summary
  measured over        5s
  bytes                1.00 GiB
  sustained            185.64 MiB/s
  peak                 242.47 MiB/s
  requests             65536 (0 failed)
  peak peers           1
  verification         1024 pieces, 1.56 GiB/s in 641ms
  choke                0 choke, 0 unchoke, queue depth 128
  disk read            1.00 GiB in 136ms over 16384 reads
  disk write           1.00 GiB in 1s over 65536 writes
  pipeline             24 blocks in flight on average, 128 at peak, 16.00 KiB block, 2092us to answer
  window allows        956.02 MiB/s at that depth and that service time; 185.64 MiB/s was measured, 19.42% of it

Sources
  source               http://127.0.0.1:52466/
    served             1.00 GiB at 185.64 MiB/s over 65536 requests (0 failed)
```

Three of those lines are measurements nothing else in the process can take.
`verification` is the wall time of every piece read back and hashed, bracketed
in `bit-cli`'s own storage. `disk read` and `disk write` are the positioned
reads and writes underneath it. `pipeline` is the session's block request
window seen from the other end of the loopback bridge, with `window allows`
saying what that depth would sustain at the measured service time: close to
the sustained rate means the window is the limit, far above it means something
else is.

The same three appear per interval under `series[].costs` in the JSON and as
columns in `--format csv`, so the shape over time is visible and not just the
total.

`--fail-under` and `--baseline` work here exactly as they do on `bench
webseed`.

## Measuring a seeder

`bench seed` serves a payload and reports what leaves, per peer. It is the same
report `bench leech` writes with every counter facing the other way: bytes sent
rather than received, and positioned reads rather than writes, because a
seeder's storage cost is reading the payload back.

```bash
bit-cli bench seed album.torrent --data ./payload \
  --port 51413 --duration 120s --exit-when-idle 5s \
  --include-hash-check --format text
```

```
Summary
  measured over        35s
  bytes                737.94 MiB
  sustained            20.89 MiB/s
  peak                 24.15 MiB/s
  peak peers           3
  verification         256 pieces, 1.48 GiB/s in 169ms
  disk read            772.83 MiB in 878ms over 49152 reads
  disk write           0 B in 0ms over 0 writes

Peers
  peer                 127.0.0.1:60374
    sent               245.84 MiB at 6.96 MiB/s
```

The rows are peers, not sources: a seeder serving one peer well and another
badly looks the same in the total and different here.

`disk read` against `bytes` is the read amplification. 772.83 MiB read to send
737.94 MiB, with three peers pulling the same payload at once, is 1.047: every
byte was read about once and nothing is re-reading a piece for the second peer.

`--include-hash-check` puts the check on add into the report. A seeder reads and
hashes the whole payload before it serves a byte, and that read is normally not
part of what is being measured, so it is reported separately rather than folded
into the rate.

`--exit-when-idle` stops the run once no peer has been connected for that long.
Without it the seeder waits out `--duration` with nobody connected and the
sustained rate is diluted by the idle tail.

```bash
pwsh scripts/bench-seed.ps1 -PayloadSize 256MiB -Leechers 3 -Rate 8MiB/s
```

That drives one seeder and N leechers on loopback and writes both reports to
`bench/`. The leechers are rate capped, because an uncapped loopback transfer
finishes inside one metrics interval. So the default run measures whether the
seeder keeps up with N capped leechers rather than how fast it can go: the
sustained rate is bounded by `-Leechers` times `-Rate`. Pass `-Rate 0` with a
larger payload for a capacity number.

## What the whole path costs

`bench webseed` measures the HTTP fetch on its own. Two scripts measure what
the torrent machinery adds on top of it, and both write a committed report to
`bench/`.

`scripts/bench-webseed.ps1` takes the same payload from the same server four
ways in one session: `curl` on one connection, `curl` on N, `bit-cli bench
webseed`, and `bit-cli download --web-seed-only`. Four stages rather than two
because one ratio says "slower" without saying where.

```bash
pwsh scripts/bench-webseed.ps1 -PayloadSize 256MiB -Runs 5
```

```bash
pwsh scripts/bench-webseed.ps1 `
  -Mirror https://geo.mirror.pkgbuild.com/iso/2026.08.01/ `
  -TorrentUrl https://geo.mirror.pkgbuild.com/iso/2026.08.01/archlinux-2026.08.01-x86_64.iso.torrent
```

`scripts/bench-leech.ps1` then divides that gap. It runs `bench webseed` and
`bench leech` against the same payload, steps `--web-seed-connections`, runs a
control that puts the same total HTTP concurrency on a single connection so the
two cannot be confused, and compares against the same URL named N times so the
cost of not sharing a window cache is visible.

```bash
pwsh scripts/bench-leech.ps1 -PayloadSize 1GiB -Runs 5 -ConnectionSweep "1,2,4,8"
```

The results are in `TODO/webseed.md` under T-001 and `TODO/bench.md` under
T-090, with the committed reports under `bench/`. In one line: one source is
one peer, one peer is one serial receive path, and that path is what bounds
the download.

## Several torrents at once

`download` takes any number of sources and `-j` says how many run at a time.

```bash
bit-cli download a.torrent b.torrent c.torrent d.torrent -j 4 --dir ./out
```

```bash
pwsh scripts/check-multi-torrent.ps1 -Torrents 4 -PayloadSize 256MiB -Runs 3
```

```
ceiling:  808.84 MiB/s through bit-cli's own HTTP path, no bridge, no hashing, no disk

mode    wall  bytes      rate         of ceiling peak RSS   CPU ms handles
one     1.46s 256.00 MiB 175.95 MiB/s 21.75%     43.61 MiB    2124     220
serial  6.24s 1.00 GiB   164.02 MiB/s 20.28%     44.48 MiB    8605     228
j1      6.18s 1.00 GiB   165.78 MiB/s 20.50%     48.49 MiB    8468     227
j2      3.01s 1.00 GiB   340.20 MiB/s 42.06%     74.09 MiB    9061     242
j4      1.76s 1.00 GiB   580.17 MiB/s 71.73%     114.24 MiB  10656     264
control 2.97s 1.00 GiB   344.32 MiB/s 42.57%     107.59 MiB  15108     289
```

`serial` is the same four torrents as four separate invocations, one after
another. `control` puts as many connections on one torrent at a time as `-j 4`
has in flight across four, which is what says the flag buys concurrency rather
than connections: `-j 4` reaches 580 MiB/s where the same sixteen connections
on one torrent reach 344.

`ceiling` is what the same source serves through `bit-cli`'s own HTTP path with
no bridge, no hashing, and no disk. Every mode reads off that one server, so a
mode approaching it is describing the server rather than the client.

Concurrency costs about 22 MiB of peak RSS and twelve handles per torrent in
flight, and no extra CPU for the same bytes. The full write-up is in
`TODO/performance.md` under T-030.

## Capping one source and not the other

Five rate flags, and they do not divide the way the names suggest.

| flag | what it bounds |
| --- | --- |
| `--max-overall-download-rate` | the whole session, **peers and HTTP together** |
| `--max-download-rate` | one torrent, peers and HTTP together |
| `--web-seed-speed-limit` | HTTP sources only, per source |
| `--max-peer-rate` | swarm peers only, not sources this run attached |

An HTTP source reaches the session as a peer over loopback, so every cap that
can reach a peer reaches the mirror as well, and `--max-peer-rate` is the one
that does not: it skips the bridge this process runs, by peer id prefix.
Measured on a 128 MiB payload with an 8 MiB/s peer cap and a 24 MiB/s web seed
cap:

| what was capped | total | HTTP | peers |
| --- | --- | --- | --- |
| nothing, HTTP only | 167.32 MiB/s | 167.32 | 0 |
| `--max-overall-download-rate` | 8.39 MiB/s | 8.39 | 0 |
| `--web-seed-speed-limit` | 8.21 MiB/s | 8.21 | 0 |
| **`--max-peer-rate`, HTTP only** | **151.84 MiB/s** | **151.84** | 0 |
| nothing, peer only | 259.11 MiB/s | 0 | 259.11 |
| **`--max-peer-rate`, peer only** | **8.42 MiB/s** | 0 | **8.42** |
| nothing, peer and HTTP | 228.16 MiB/s | 185.38 | 42.78 |
| `--web-seed-speed-limit`, peer and HTTP | 301.89 MiB/s | 11.79 | 290.09 |
| `--max-overall-download-rate`, peer and HTTP | 8.35 MiB/s | 3.91 | 4.43 |
| **both caps, peer and HTTP** | 27.57 MiB/s | **18.31** | **9.26** |

Two rows are worth reading. `--max-peer-rate` with HTTP only runs at 151.84
MiB/s against an 8 MiB/s cap, which is the point: a swarm cap must not reach a
source this run attached. And `--web-seed-speed-limit` with a peer in the swarm
reaches 301.89 MiB/s against an 8 MiB/s cap, because nothing there was asked to
bound the peer. If a run has to stay under one number, cap the session; if it
has to stay under two, set both.

There is no `--max-peer-upload-rate` and there does not need to be. A bridged
source is a seed: it never asks for a piece, so nothing is ever uploaded to it,
and the upload caps already reach peers alone.

```bash
pwsh scripts/check-rate-scope.ps1
```

`TODO/multi-source.md` under T-132 has why a peer-only cap is not there and
what upstream would have to expose for it to be.

## Comparing against a baseline

`--baseline` prints a delta per metric against a previous report, and
`--fail-under` exits 14 when a named metric falls below a floor. Together they
are what makes a benchmark a check rather than a number to read.
