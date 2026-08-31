# Asking whether two torrents hold the same files

Two torrents with different info hashes can still hold the same bytes. A
re-release with one extra file, the same album at two piece lengths, a cross-
seed with a `source` key added: all of them produce a different info hash over
some of the same data. Downloading it twice is the cost of not knowing.

`bit-cli files --against` answers it from the metadata, without reading the
payload and without touching the network.

## What one run looks like

Three files in a torrent, compared against another torrent built over the same
directory at the same piece length with a different `source` key:

```bash
bit-cli files one.torrent --against three.torrent
```

```text
INDEX  SIZE        SHARE   PIECES  PATH
0      1.00 MiB    59.73%  0-3     a.bin
1      683.59 KiB  39.87%  4-6     sub/b.bin
2      6.84 KiB    0.40%   6-6     sub/c.txt

INDEX  EVIDENCE      PROVEN      OTHER       OTHER PATH
0      piece-hashes  1.00 MiB    e54a6a73:0  a.bin
1      piece-hashes  512.00 KiB  e54a6a73:1  sub/b.bin
2      length        -           e54a6a73:2  sub/c.txt
```

Three verdicts, three different amounts of confidence, and the difference
between them is the whole point.

## What `piece-hashes` proves, and what `length` admits

A `.torrent` carries SHA-1 hashes of fixed-size pieces of the whole payload,
not of each file. So a file's bytes are covered by a piece hash only where a
piece lies **entirely inside** that file. The piece straddling the boundary
also covers the end of the file before it, and its hash says nothing about
either one alone.

For two files in two torrents to be compared by hash, two things have to hold:

- **the same piece length**, because a 2 MiB hash and a 1 MiB hash are hashes
  of different amounts of data and can never be equal for the same bytes;
- **the same alignment**, meaning each file's offset within its own torrent is
  congruent modulo the piece length, so the first whole piece starts at the
  same place in the file both times.

When both hold, the whole pieces line up one to one and comparing their hashes
proves those bytes equal to the strength of SHA-1.

Read the three rows above against that rule:

| index | size | verdict | why |
| --- | --- | --- | --- |
| 0 | 1.00 MiB | `piece-hashes`, 1.00 MiB proven | starts at offset 0 and is exactly four 256 KiB pieces, so all of it is covered |
| 1 | 683.59 KiB | `piece-hashes`, 512.00 KiB proven | starts on a piece boundary, two whole pieces fit, and the tail shares its piece with the next file |
| 2 | 6.84 KiB | `length` | smaller than one piece, so no piece lies entirely inside it and nothing can be compared |

`length` means the sizes match and that is all that could be checked. The files
may or may not be the same, and only reading them says which. It is a candidate
rather than a proof, and the report never calls it more than that.

Change the piece length and every row drops to `length`:

```bash
bit-cli files one.torrent --against two.torrent
```

```text
INDEX  EVIDENCE  PROVEN  OTHER       OTHER PATH
0      length    -       3e335faf:0  a.bin
1      length    -       3e335faf:1  sub/b.bin
2      length    -       3e335faf:2  sub/c.txt
```

Same bytes on disk, same three files, and nothing provable from the metadata,
because 256 KiB hashes and 512 KiB hashes are hashes of different things.

The rule is implemented and documented at
[`../../crates/bit-cli-core/src/equivalence.rs`](../../crates/bit-cli-core/src/equivalence.rs).

## The machine form carries the evidence too

```bash
bit-cli files one.torrent --against three.torrent --json
```

```json
{
  "bytes_proven": { "bytes": 1048576, "human": "1.00 MiB" },
  "evidence": "piece-hashes",
  "index": 0,
  "info_hash": "e54a6a732a318c9d1567b78fc180d51e8cdf94d0",
  "path": "a.bin",
  "pieces_compared": 4,
  "proven": true,
  "torrent": "three.torrent"
}
```

`proven` is the field a script should branch on. `evidence` is why, and
`pieces_compared` is how much work backed it.

`--against` is repeatable, and each comparison gets its own rows:

```bash
bit-cli files one.torrent --against three.torrent --against two.torrent
```

## It happens without being asked during a download

Give one invocation several torrents and the same comparison runs before the
session starts:

```bash
bit-cli download c.torrent a.torrent b.torrent --dir out -j 1
```

Where the piece hashes prove two files are the same bytes, the later torrent
gets a `file:` source pointing at the copy the earlier one wrote, as soon as
the earlier one has finished. No path, no info hash, no flag. The bytes are
still hash-checked against the torrent that asked for them, so a wrong match
costs a failed source rather than a corrupt payload.

`--no-share-files` turns it off. [`../webseed.md`](../webseed.md) has the
measured run: three info hashes, three piece lengths, one 64 MiB file fetched
once and landing in three output directories with one distinct hash between
them.

```bash
pwsh -NoProfile -File scripts/check-shared-files.ps1
```

## What this cannot answer

`--against` compares files. It says nothing about the rest of a torrent, and
there is no command that does. Trackers, web seeds, the `private` flag, the
`source` key, the piece geometry, the comment, and the directory structure are
all things you would compare by running `bit-cli info` twice and reading both.

A torrent against a directory on disk is the other missing case. `bit-cli
verify` hash-checks one torrent against one payload, which answers "is this
data complete" rather than "how do these two differ".

Both are [T-248 in the TODO](../../TODO/metainfo.md), which is one `diff`
command over any two inputs with a mode for each axis. The shape of a torrent
printed as a tree rather than a flat list is [T-249](../../TODO/metainfo.md).
