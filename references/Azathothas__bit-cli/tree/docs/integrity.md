# What `bit-cli` guarantees about the bytes

**A finished download is bit-for-bit the payload the `.torrent` describes.**
That is the contract, and this file is what it rests on: four independent
checks, what each one catches, what each one costs, and what none of them can
tell you.

It matters more here than in a conventional client. `bit-cli` exists to point
**several sources at one payload**, and a mirror on a CDN is not a peer that
earned its place in a swarm. The whole reason to trust a source you found on
the internet is that nothing it says is taken on its word.

See `TODO/multi-source.md`, T-136.

## The four checks

| Check | Catches | Costs | Default |
| --- | --- | --- | --- |
| Per-source piece check | a mirror serving wrong bytes, named | one sha1 per piece, at the source | on, `--web-seed-verify piece` |
| The session's own piece check | anything wrong from any source | one sha1 per piece | always on, not a flag |
| The hash check on add | a resumed payload that changed on disk | one full read | on for a resume, `-V` to force |
| `--verify-on-complete` | the disk, the filesystem, and this program | one full read | off |

### 1. Every piece from an HTTP source is checked at the source

`--web-seed-verify piece`, which is the default. A source's bytes are hashed
against the torrent's own piece hash **before** they are handed to the session,
so a mirror serving wrong data is named rather than showing up as "a peer sent
something wrong".

A piece can be filled from several sources at once, which is the normal case
here and the ambiguous one. A block-to-source ledger records which supplier
gave which block, and when a piece fails its hash the ledger convicts the
supplier whose block disagrees with the verified payload, not everyone who
touched the piece. A convicted source is retired for the run; a healthy one
beside it is not. `sources[].convictions` in `--json` names the source, the
piece, and both digests. See `TODO/webseed.md`, T-179.

`--web-seed-verify none` turns this off. It does not turn off check 2.

### 2. The session checks every piece, from every source

Not a flag, and there is no way to disable it. A piece that does not hash to
what the torrent says is not counted, whatever supplied it. This is the check
that makes the guarantee: 1 exists to say **who** was wrong, and 3 and 4 exist
to say what happened after the fact.

### 3. Existing data is hash-checked before it is trusted

A resumed download hash-checks what is already on disk, so a partial payload
that was modified between runs is re-fetched rather than kept. `-V` /
`--check-integrity` forces it on a run that would not otherwise do one, and
`--hash-check-only` does it and exits, which is how you ask "is what I have
still right?" without a network.

### 4. `--verify-on-complete` re-reads the payload and hashes every file

```bash
bit-cli --json download release.torrent --verify-on-complete
```

**Redundant with 1 and 2 by construction, and that is the point.** Checks 1 to
3 all hash the bytes as they pass through this program. This one reads the
files back off the disk after the run and reports a sha256 per file:

```json
"verified_files": [
  { "index": 0, "torrent_path": "disc 1/a.flac", "algorithm": "sha256",
    "hex": "…", "bytes": 1500, "length": 1500 }
]
```

It is the check a caller can run **without trusting the thing that wrote the
bytes**, and the only one whose output can be compared against a digest
published somewhere else, because nobody publishes a per-file sha1 of a
torrent's contents.

- **`sha256`**, not the torrent's `sha1`, for exactly that reason.
- **Only a finished torrent** is hashed. Digests of files that are not yet the
  files are a wrong answer rather than a missing one, so an unfinished run
  carries no `verified_files` at all.
- **Only selected files.** A file `--select-file` skipped was not written by
  this run, so hashing it would report a digest of whatever was there before.
- **It never changes the exit code.** The digests are facts about the payload
  and this run has nothing to compare them against. A caller that does have
  something to compare them against is the one that can decide.
- **A file that cannot be read carries its `error`** rather than being left
  out, so a caller counting rows against the file list is never short one.

The cost is one full read of the payload.

### And `bit-cli verify`, afterwards, from nothing but the torrent

```bash
bit-cli verify release.torrent --data ./out/release --per-piece
```

A separate invocation, later, from a `.torrent` and a directory. It is check 2
run on demand: every piece against the torrent's own hashes, with `--per-piece`
reporting each one. It exits 7 on a mismatch and names the pieces. A payload
renamed on the way down with `-O` needs the same `-O` here, because `verify`
looks where the bytes went rather than where the torrent said they would go.

## What none of this tells you

**Whether the torrent describes the file you wanted.** Every check above proves
the payload matches the `.torrent`. If the `.torrent` is wrong, or is not the
one its publisher made, they all pass.

That is what a Metalink is for, and it is why `bit-cli` checks both documents
against each other rather than either alone: a Metalink carries a `.torrent`
**and** an independently published checksum over the same bytes. When the two
disagree, the payload has already passed the torrent's piece hashes, so the
report says which document is wrong rather than which byte is:

```
the metalink's sha256 checksum does not match the payload: it says 0000…,
the bytes hash to ad33…
```

Either disagreement exits 7. See the Metalink section of `README.md`.

Outside a Metalink, `--verify-on-complete` is how you close that gap yourself:
compare its `hex` against a digest the publisher signed.

## Where this is checked

| Claim | Held by |
| --- | --- |
| A mirror serving wrong bytes is convicted and the healthy one is not | `a_mirror_that_serves_wrong_bytes_is_named_and_the_healthy_one_is_not` |
| Two honest mirrors filling one payload convict nobody | `two_honest_mirrors_filling_one_payload_convict_nobody` |
| Corrupt data never completes the torrent | `corrupt_data_never_completes_the_torrent` |
| A source-side check names the mirror that served a wrong piece | `source_side_verification_names_the_mirror_that_served_a_wrong_piece` |
| `--verify-on-complete` reports the payload's real digest per file | `verify_on_complete_reports_a_digest_per_file` |
| An unfinished run is not hashed | `verify_on_complete_hashes_nothing_when_the_run_did_not_finish` |
| A Metalink checksum that disagrees exits 7 and names both digests | `scripts/check-metalink.ps1`, case `bad_checksum` |

## What is guaranteed about the bytes

**A finished download is bit-for-bit the payload the `.torrent` describes.**
Four independent checks stand behind that, and
[`docs/integrity.md`](integrity.md) says what each one catches, what it
costs, and what none of them can tell you.

| Check | Catches | Default |
| --- | --- | --- |
| Per-source piece check | a mirror serving wrong bytes, **named** | on, `--web-seed-verify piece` |
| The session's own piece check | anything wrong from any source | always on, not a flag |
| The hash check on add | a resumed payload that changed on disk | on for a resume, `-V` to force |
| `--verify-on-complete` | the disk, the filesystem, and this program | off |

It matters more here than in a conventional client. `bit-cli` exists to point
several sources at one payload, and a mirror on a CDN is not a peer that earned
its place in a swarm. A piece filled from two sources where one of them lied is
the normal case here rather than the exotic one, and it is why a block-to-source
ledger convicts the supplier whose bytes disagree rather than everyone who
touched the piece: a lying mirror is retired for the run and the honest one
beside it keeps working.

```bash
bit-cli --json download release.torrent --verify-on-complete
```

That re-reads the finished payload off the disk and reports a sha256 per file
under `verified_files`. It is redundant with the piece checks by construction,
which is the point: it is the check that does not trust the thing that wrote the
bytes, and the only one whose output can be compared against a digest published
somewhere else.

**What none of it tells you is whether the `.torrent` describes the file you
wanted.** That is what the Metalink below is for.
