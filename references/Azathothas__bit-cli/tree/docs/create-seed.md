# Creating and seeding

The entries behind this are in
[`TODO/create-seed.md`](../TODO/create-seed.md).

A `seed` run with `--seed-time 7d` is a long-lived process, and two things in
it grow without bound in the published `librqbit`. Both are fixed in the
vendored one, which is one of the reasons the trees are vendored at all:
[`vendoring.md`](vendoring.md).

**Sockets stranded in `CLOSE_WAIT`.** A peer that connected and closed before
sending a handshake stranded one about half the time, and the cause was worse
than the count: one failed handshake check disabled the arm of a `select!` that
drained the queue, so the seeder went on accepting TCP and completing **no
handshake for any info hash, including one it was serving**, while reporting
itself as seeding. 4000 such connections stranded 2053 sockets. Now:

```bash
pwsh scripts/check-close-wait.ps1
```

**986 stranded sockets to 0**, and handles that went 188 to 1210 now go 188 to
194. `TODO/peers.md` T-020 has the reproduction.

**Peer records that were never reclaimed.** One row per completed handshake,
kept for the life of the process. 2,000 connections left 2,000 rows at `live 0`
and `dead 0`. They are bounded at 1,024 per torrent now, taking only rows that
have no task or dial behind them; `TODO/memory.md` T-040 has the measurement.

What `bit-cli` still carries, because a backstop for a process that runs for
days is worth having whether or not a known leak is closed:

```bash
bit-cli seed release.torrent --seed-time 7d --max-handles 4096
```

`--max-handles` is sampled once per `--report-interval` against the whole
process. Over it, the run stops with `"stopped": "handle_ceiling"` and exit 16,
which a supervisor restarts. It is off by default, because the right number
depends on the deployment; read `cost` in a healthy run's report for a
baseline.

Memory has the same backstop:

```bash
bit-cli seed release.torrent --seed-time 7d --max-rss 512MiB
```

`--max-rss` stops the run with `"stopped": "rss_ceiling"` and exit 16. A seeder
with nothing connected sits near 12 MiB, so pick a number from `cost` in a
healthy report rather than from this paragraph. A seeder under load still grows,
and `TODO/memory.md` T-040 carries what is measured and what is not: bounding
the peer records did not flatten the slope, so something else in a loaded
session accounts for most of it.

```bash
pwsh scripts/check-peer-rows.ps1
```

**A seeder that restarts does not have to re-hash its payload.** Every add
hash-checks the whole thing, at about 1.6 GiB/s here, so a 40 GiB payload
spends about 25 seconds of disk read before it announces anything:

```bash
bit-cli seed release.torrent --seed-time 7d --fastresume
```

The verified bitfield is kept at `<data>/.bit-cli-resume/<info hash>.bitv`,
beside a `.meta` sidecar naming every file's length and modification time.
Change a byte of the payload and the cache is refused and deleted, and the run
hash-checks as it always did. `--fastresume-dir` moves the cache; there is no
equivalent on `download`, because a download writes its payload continuously
and would find its own cache stale on every run.

```bash
pwsh scripts/check-fastresume.ps1
```

The write-up is in `TODO/memory.md` under T-040.

One thing that was in reach is fixed. `librqbit`'s accept loop panics when its
pending handshake-check set fills and one of those checks fails, and the panic
kills the listener while the process keeps running and keeps reporting itself
as seeding. Measured, 3000 connections that closed before handshaking did it in
79 seconds. `bit-cli` removes the branch that carries it, and the same flood
now finishes in 8.8 seconds with the listener alive.

## Two lints are about what another client will refuse

`piece-count-unopenable` and `piece-length-too-large` are not opinions about
what is tidy. µTorrent will not open a torrent with more than 65,535 pieces,
and piece lengths above 16 MiB have been reported to break clients, so a
torrent that clears every other check can still be one a recipient cannot use.

They are separate from `piece-count` and `piece-length-not-power-of-two`, which
are opinions, and the split is so they clear independently: deciding to live
with 200,000 pieces of hash data is not deciding to ship a file µTorrent cannot
read.

```bash
bit-cli create ./payload --piece-length 64MiB --allow piece-length-too-large
```

## Proving the round trip

A torrent this tree wrote that another client will not open is the failure
worth catching, and it is checked rather than assumed:

```bash
pwsh -NoProfile -File scripts/interop-roundtrip.ps1
```

`bit-cli create`, `verify` and `seed` round trip byte for byte through
`aria2c` 1.37.0 and `rqbit` 9.0.1 for v1, `--private`, and `--web-seed`.
[`examples/create-and-seed.md`](examples/create-and-seed.md) walks it.
