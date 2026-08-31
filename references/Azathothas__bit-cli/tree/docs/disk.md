# Disk

Where the payload lands, how it is allocated, and what bounds the descriptors.

The entries behind this are in [`TODO/disk-io.md`](../TODO/disk-io.md) and
[`TODO/windows.md`](../TODO/windows.md).

A payload file is created when it is first written, not when the torrent is
added. Two things follow.

`--select-file 0` writes one file and leaves the rest off the disk, rather than
creating eleven empty ones beside the one you asked for.

With one exception, and it is the torrent's shape rather than a choice. A piece
is verified against its whole hash, so a piece straddling the boundary between
a file you selected and one you did not cannot be proved without both halves.
Those bytes are fetched and written into the file they belong to, which leaves
a file you did not ask for holding a few hundred kilobytes of payload and
nothing else. It can even land at its full length, which is what makes it worth
saying rather than leaving to be discovered:

```bash
bit-cli download album.torrent --select-file 1 --json
```

reports every one of them under `torrents[].partial`, with how much of each is
real, how long it ends up on disk, and how long the torrent says it is, and
says the same on stderr. A torrent whose file boundaries fall on piece edges
has none.

`verify` takes the same selection, and needs it to give the right answer:

```bash
bit-cli verify album.torrent --data out/album --select-file 1 --json
```

Without it, every piece outside the selection is reported as a failure and the
command exits non-zero, which is true of the bytes and wrong about the run:
nothing ever asked to fetch them. With it they are listed under `not_selected`,
the counts are against what was asked for, and a selection that arrived intact
is complete. The boundary pieces themselves verify and a `bit-cli seed` over
that directory offers them, because their bytes really are all there.

`--max-open-files` is a real cap. Files close on a least-recently-opened basis
when it is reached, so a torrent with twenty thousand files needs the cap in
descriptors and not twenty thousand:

```bash
bit-cli seed many.torrent --data . --max-open-files 64
```

```bash
pwsh scripts/check-handles.ps1
```

measures it: three seeds of a 300-file torrent at caps of 8, 64, and 128, with
the process handle count sampled while each runs. The steps in the cap and the
steps in the handle count match exactly.

`--file-allocation` picks how space is reserved, and the four methods do four
different things:

| Method | What happens |
| --- | --- |
| `none` | The length is set and nothing else. On NTFS that allocates; on ext4 it does not. |
| `sparse` | The file is marked sparse first, so the hole is explicit. The default. |
| `prealloc` | Zeroes are written across the file and flushed. Slow, and the space is certainly there. |
| `falloc` | The filesystem reserves the blocks without writing them. `posix_fallocate` on Linux. |

`falloc` on Windows needs `SeManageVolumePrivilege`, which an ordinary process
does not hold, so it falls back to `prealloc` and says so on stderr rather than
doing something other than what it was told.

```bash
pwsh scripts/check-allocation.ps1
```

measures all four against a real download, reading volume free space before the
payload arrives. On NTFS with a 512 MiB payload, `sparse` costs the volume
nothing and the other three cost it 512 MiB. Every method's output hashes equal
to the source.

## Where a payload lands

A `.torrent` is untrusted input, and its file names decide where bytes land.
Three of them cannot be used as written:

- A component the platform reads as a drive or a root. On Windows
  `Path::new("D:/out").join("C:")` is `C:`, so a two-character component
  relocates the download out of the output directory.
- A name the filesystem refuses: `CON`, `NUL`, `COM1`, a trailing dot or space,
  or any of `< > : " | ? *`.
- Two names that differ only in case. NTFS and APFS treat `README` and `readme`
  as one file, so the second write wins and the first payload is gone.

`bit-cli` plans every path before it opens anything. Each file lands inside the
output directory, under a name the filesystem accepts, and no two files collide.
The rules run on every platform, so a payload downloaded on Linux and copied to
Windows still works.

Nothing is silent. A changed path is reported on stderr and in `--json`:

```bash
bit-cli download hostile.torrent --json | jq '.torrents[0].renamed'
```

```json
[
  {
    "index": 0,
    "torrent_path": "C:/pwned.txt",
    "disk_path": "C_/pwned.txt",
    "reasons": ["escape", "illegal-character"]
  },
  {
    "index": 1,
    "torrent_path": "CON.txt",
    "disk_path": "CON_.txt",
    "reasons": ["reserved-name"]
  }
]
```

The key is absent when nothing changed, which is the common case. `index` is
the file's index in the torrent, so a caller can reconcile what it asked for
with what is on disk.

`seed` and `verify` carry the same array, because they serve and read the files
`download` wrote:

```bash
bit-cli seed hostile.torrent --data out --json   | jq '.renamed'
bit-cli verify hostile.torrent --data out --json | jq '.renamed'
```

A long path is not one of the three. A payload whose deepest path plus the
output directory runs past the 260 characters the classic Windows API allows
lands as written and verifies from the same path, with nothing renamed. The
one limit that does apply is per component: a name over 255 bytes is truncated
to fit, keeping its extension, and reported like any other rename.

## Paths, on the writing side

The rules above are the reading side. On the writing side `bit-cli create`
refuses to build such a torrent at all, through the `windows-path` and
`case-collision` lints, with `--allow <LINT>` to override either one. Those
lints only have anything to catch on a filesystem that can hold the input,
which Windows is not, so they are exercised on Linux and here:

```bash
cargo test -p bit-cli-core lint::
```

Two paths that are byte-for-byte the same are `duplicate-path` rather than
`case-collision`, because a message telling somebody to look for a casing
difference that is not there costs more than no message.

## What one download cost the disk

A finished `download` reports it, and `--stats` is how to read it without a
JSON parser:

```bash
bit-cli download album.torrent --dir out --stats
```

```
disk.bytes_written.bytes 444700
disk.write_calls     32
disk.write_ops       20
disk.write_time.ms   0
```

`bytes_written` is what reached the device, which is more than `downloaded`
when a piece was written twice and less when the run resumed. `write_ops` over
`write_calls` is the coalescing factor: in the run above, twenty writes for the
thirty-two the session asked for. It moves from run to run, because what can be
combined depends on the order blocks arrive in, and the same command a minute
later reported seventeen. `write_time` is wall time inside those writes, summed
across every worker, so it can exceed the run's own elapsed time.

The counters are always on. They cost two clock reads per write, about 50 ns
against the 95 us a 16 KiB block takes end to end, and a counter that is only
on when somebody is measuring measures a different program.

## Measuring the disk on its own

`bench disk` writes a payload through the same storage a download writes
through, from N threads, with no session and no network. A download has the
network, the session, the hash, and the disk running at once and cannot say
which of them a slow run was waiting for; this takes the other three away.

```bash
bit-cli bench disk --payload-size 1GiB --concurrency-sweep 1,2,4,8 --format text
```

```
Writers
  THREADS  LAYOUT   FILES  RATE           WALL      FLUSH     WRITE TOTAL  MEAN WRITE  OVERLAP
  1        shared   1      2.27 GiB/s     440ms     821ms     423ms        6us         0.96
  2        shared   1      1.57 GiB/s     635ms     412ms     1s           18us        1.93
  4        shared   1      1.65 GiB/s     606ms     915ms     2s           34us        3.73
  8        shared   1      1.46 GiB/s     685ms     1s        4s           75us        7.22
```

`--layout` decides how the same bytes are spread, and comparing the three is
the measurement:

| Layout | Files | Handles | What it is |
| --- | --- | --- | --- |
| `shared` | 1 | 1 | Every thread interleaves blocks into one file. What a torrent with one payload file and several peers does. |
| `handles` | 1 | N | The same file at the same offsets, one handle per thread. |
| `split` | N | N | One file per thread. |

`OVERLAP` is the summed write time over the wall clock: the thread count when
nothing serialises, and 1.00 when everything does. `FLUSH` is what the write
phase left in the page cache, drained after the clock stops so one step does
not hand its cost to the next.

Every step reads the payload back and checks that each block is the block that
was written to it. A step that reads back something else exits 7, because that
is a correctness failure and not a slow one. Pass `--no-verify` to skip it.

```bash
pwsh scripts/check-disk-contention.ps1
```

That runs the sweep across all three layouts and a range of block sizes,
alternating the order so no layout always gets the disk in the same state, and
writes the medians and a verdict to `bench/disk-contention-<timestamp>.json`.
What it found on NTFS is in `TODO/disk-io.md` under T-017: writes to one file
serialise whatever handle they arrive on, and the serialisation is charged per
write operation rather than per byte.

## Why sparse is the default

On NTFS, setting a file's length on a non-sparse file reserves every cluster at
once, and then every subsequent high-offset write zero-fills the range below
the valid data length. BitTorrent piece arrival is near random, so the first
high-offset piece triggers a zero write across the whole prefix. That is an
order of magnitude of write amplification on a large torrent.

Sparse gives up two things in exchange: early out-of-space detection becomes a
per-piece write error rather than one failure at allocation time, and cluster
contiguity is not guaranteed. Use `--file-allocation falloc` when the volume is
not NTFS and the payload is large, and `prealloc` when a full-length file has
to exist before the first byte arrives.

`ext4` and APFS make a length change sparse already, so the flag changes less
there. `scripts/check-allocation.ps1` measures all four methods by volume free
space before the payload arrives.
