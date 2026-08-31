# Creating, editing, and seeding torrents

Twenty-five issues touch creation and metainfo; forty-eight touch seeding,
upload, and ratio.

---

### T-080 librqbit's create_torrent writes an extra piece hash

Source:      found here, 2026-08-19, against the pinned `librqbit` 9.0.0
Category:    create
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `create_torrent` appends one spurious piece hash when the payload
             is an exact multiple of the piece length. Its final flush tests
             `remaining_piece_length > 0 && length > 0`, but
             `remaining_piece_length` has already been reset to a full piece by
             the loop that just closed the last complete piece, and `length` is
             the last file's length rather than the total. So it hashes an
             empty SHA-1 and appends it.

             Reproduced with a 327,680-byte payload at a 32,768-byte piece
             length: 11 hashes where 10 pieces exist. `bit-cli`'s own parser
             rejects the result:
             `torrent declares 11 pieces but 327680 bytes at 32768 bytes per
             piece needs 10`.
Relevance:   `bit-cli create` uses its own creator, so nothing shipped is
             affected. It matters because it was producing the test fixtures,
             and because any `.torrent` from `rqbit` with an exactly-aligned
             payload is malformed and will be rejected by strict clients.
Approach:    Upstream: the final flush should test whether any bytes are
             pending in the current piece, not whether the counter is non-zero.
             Here: fixtures are built with `bit_cli_core::torrent::create`,
             which is the code that ships. Report it upstream.
Acceptance:  A `.torrent` for a payload that is an exact multiple of the piece
             length, built by `bit-cli create`, has exactly
             `total_length / piece_length` hashes. Covered by
             `webseed_e2e::a_bep_17_source_downloads_a_torrent` and friends,
             which use exactly-aligned payloads.

**Done, as a differential test rather than a fix.** `bit-cli create` uses
`bit_cli_core::torrent::create`, which writes one hash per piece, and
`crates/bit-cli-core/tests/create_alignment.rs` proves both halves over the
same bytes:

- `bit_cli_writes_one_hash_per_piece_for_an_exactly_aligned_payload` builds a
  327,680 byte payload at a 32,768 byte piece length and asserts the metainfo
  carries exactly ten hashes and parses.
- `librqbit_writes_one_hash_too_many_and_bit_cli_refuses_it` runs
  `librqbit::create_torrent` over the same file and asserts `Metainfo::parse`
  refuses the result naming the counts: eleven pieces declared where 327,680
  bytes at 32,768 needs ten.

The second test is the fixture rule 0.10 asks for: the failing input is
generated rather than committed, because generating it is three lines and a
committed `.torrent` says nothing about which version produced it. If
`librqbit` fixes this, that test fails and this entry gets its answer.

**The upstream report is what is left, and it needs the operator.** Filing it
means posting to `github.com/ikatson/rqbit` from an account, which is not
something this session does on its own. The report is written and ready: the
function is `create_torrent_raw` in `librqbit-9.0.0/src/create_torrent_file.rs`,
the condition is the final flush testing `remaining_piece_length > 0 && length
> 0`, and the reproduction is the test above.

### T-081 BEP 52 v2 and hybrid torrents are not implemented

Source:      https://github.com/ikatson/rqbit/issues/546 (open); the operator's brief
Category:    create
Priority:    P1
Effort:      XL
Status:      open

Problem:     `bit-cli create --version v2|hybrid` returns a usage error naming
             this item. Neither the merkle tree construction nor the BEP 47
             padding files a hybrid torrent needs exist.
Relevance:   v1 is what everything reads today, so this is not urgent, but a
             creation tool that cannot make a hybrid torrent will age badly.
Approach:    BEP 52 is the reference and is enough on its own; the shape is
             written out below. Upstream issue #546 carries a full design for
             `rqbit`. Creation is the tractable half: `bit-cli create --version
             hybrid` needs the v2 `file tree`, the `piece layers`, and BEP 47
             padding between files. Downloading a v2 torrent needs `librqbit`
             support and is a separate, larger item.
Acceptance:  `bit-cli create <PATH> --version hybrid` produces a torrent that
             `intermodal` and one mainline client both accept, and whose v1
             info hash matches a `--version v1` build of the same payload.

**The v2 shape, from BEP 52, written here so this entry does not depend on a
reading of somebody else's implementation.**

The hash tree:

- Leaves are SHA-256 over **16 KiB blocks**, not over pieces.
- The leaf layer is padded to the next power of two with **32 zero bytes per
  missing node**. The internal nodes above the padding are then computed
  normally rather than special-cased.
- A parent is SHA-256 over the **64 byte** concatenation of its two children.
- The root is reached by folding until one 32 byte node is left.

The metainfo:

- `piece length` is a power of two and at least `0x4000`.
- `piece layers` is a **top-level key, a sibling of `info`**, not inside it.
  Putting it inside changes the info hash.
- The v2 info hash is `SHA-256(bencode(info))` at full width.
- A file leaf in `file tree` is the `""` key holding `{length, pieces root}`.
- `piece layers` is **omitted for a file smaller than one piece**, because its
  root is the only node there is.

**The case nearly nobody implements.** `piece layers` stores the layer whose
nodes are `piece length` sized. At a 16 KiB piece length that **is** the leaf
layer. At 32 KiB or above it is a layer further up, and the stored layer must
have **the padding truncated off** before it is written. Emitting the padded
layer produces a torrent that hashes consistently against itself and is
rejected or silently mis-sized by anything that checks. Every v2 implementation
this work looked at either gets this wrong or documents that it does not handle
it. `bit-cli` has to.

Two more things worth carrying in from the same reading:

- **Piece size selection.** Aim for under 2,500 pieces, starting at 32 KiB and
  capping at 16 MiB, with an early exit above 20 GiB. Worth comparing against
  `bit-cli create`'s own heuristic, which came from `intermodal`.
- **BEP 47 padding.** When padding is on, a partial piece is filled with zeroes
  and a `.pad/<len>` file with `attr = "p"` is emitted so the next real file
  starts on a piece boundary. Padding must **not** be emitted after the last
  file.

One implementation detail that is a defect rather than a design choice: a
merkle helper that computes `ceil(log2(n))` in floating point rounds wrong for
a large leaf count, and a tree one layer short produces a wrong root with no
error at all. Use integer bit arithmetic.

**The 2026-08-21 corpus supplies a working v2-and-hybrid creator built on the
same engine `bit-cli` uses, and the specification above is now checkable
against three independent implementations rather than read alone.**

`nanotorrent/src/bittorrent/torrent_create.rs` is 618 lines and exists for
exactly this reason: librqbit creates v1 torrents only, so somebody else has
already written the piece `bit-cli` is missing, against the same base. It
confirms every clause above and settles two the specification leaves to
judgement. `:207` `hash_file_v2`: the **final short block is hashed as-is and
not zero-padded**, leaves are then padded to a power of two with the zero hash,
and the piece layer is the tree level where one node spans one piece,
**truncated to `ceil(num_blocks / blocks_per_piece)` real pieces** because
trailing all-padding pieces lie beyond the end of the file. That is the
"case nearly nobody implements" above, implemented. A file of one piece or less
gets an **empty** piece layer, and an empty file gets no `pieces root` at all.
`:280` `V1Hasher` with `:309` `pad_to_piece` is the hybrid path, emitting the
BEP 47 padding file at `:457-466` with `attr = "p"` and path
`[".pad", "<len>"]`. `:381` `auto_piece_length` starts at 256 KiB and doubles
while `total/pl > 2000`, capping at 16 MiB; `:390` `validate_piece_length`
enforces power-of-two and at least 16 KiB, which v1 does not require and v2
does. It hand-rolls a bencode encoder at `:46-99` and says why: the structure
is a recursive tree plus a dictionary keyed by **raw 32-byte hashes**, and
serde was considered and rejected. `bit-cli` has its own bencode writer, so
that decision is worth checking against it before assuming the existing one
will do.

`torrent/merkle/` is the primitive layer: `merkle.go:10` `BlockSize = 1<<14`,
`:12` `Root`, `:28` `RootWithPadHash`, `:47` `CompactLayerToSliceHashes`; and
`torrent/merkle/hash.go:9` `NewHash` is a **streaming** `hash.Hash` over 16 KiB
blocks with `:70` `SumMinLength` padding a short file tail with zero hashes.
Streaming matters for `bit-cli`, whose hasher already reads a payload once.

`rustorrent/src/torrent.rs:542` `validate_v2_piece_layers` and `:581`
`validate_hybrid_layout` are the most complete validation in the corpus and
belong on the **read** side of this entry rather than the write side.
`:542` checks that every file above one piece has exactly
`ceil(length / piece_length)` hashes that reconstruct its `pieces root`, **and
the reverse**, that every entry in `piece layers` corresponds to a file that
needs one, because piece layers are not an extension bucket. `:581` requires
every BEP 47 padding file to be exactly the bytes needed to reach the next
piece boundary, every non-padding file to start on a boundary, and the v1 and
v2 file lists to agree pairwise in path and length.
`rustorrent/src/sha256.rs:186` `merkle_root_from_piece_layer` states the rule
in one sentence: omitted balancing nodes are supplied using the zero hash **for
the selected piece layer**, derived by repeatedly hashing the zero hash up from
the block level.

**One construction in the corpus is wrong, and it is wrong in a way that
passes its own tests.** `bqti/src/bit_torrent/torrent/merkle.rs:35`
`from_piece_hashes` reduces by `chunks(2)`, hashing `H(chunk[0] || 0^32)`
whenever a level has an odd count. BEP 52 pads the **layer to a power of two**
with the layer's pad hash before reducing. The two agree when the layer length
is already a power of two and diverge otherwise: for a five-hash layer the
correct construction pairs the padding as `H(H(h4,P), H(P,P))` while that one
produces `H(H(h4,0), 0)`. Follow the rustorrent, anacrolix and nanotorrent
construction.

**Three peer-facing crashes to read before implementing v2 hash exchange**, all
reachable from input `bit-cli` accepts, since it takes torrents from magnets,
URLs and stdin. anacrolix
[PR 1056](https://github.com/anacrolix/torrent/pull/1056): a `pieces root` that
is not exactly 32 bytes panicked inside the file-tree iterator, reachable from
`AddTorrent`, `SetInfoBytes` **and peer metadata exchange**; the fix is to
validate the tree before anything sets the info.
[PR 1054](https://github.com/anacrolix/torrent/pull/1054): a `Hashes` message
was processed without checking it answered an outstanding request, so a bogus
`pieces root` dereferenced a nil file.
[PR 1066](https://github.com/anacrolix/torrent/pull/1066): a v2 file with
`fileNumPieces % 512 == 1` leaves one hash in the last request block, and the
BEP 52 minimum request length is two, a precise off-by-one any implementation
will meet.

**Fixtures, so this does not need building from scratch.**
`torrent/testdata/bittorrent-v2-test.torrent` is pure v2 and
`torrent/testdata/bittorrent-v2-hybrid-test.torrent` is hybrid.
`superseedr/integration_tests/torrents/` holds sixteen more in `v1`, `v2` and
`hybrid` subdirectories, holding `single_4k`, `single_8k`, `single_16k`, `multi_file`
and `nested` in each, with payload descriptors in
`superseedr/integration_tests/test_data/`. Those are the cheapest route to v2
coverage in `cargo test` and they cost nothing to add before any of the code.
`superseedr/src/torrent_manager/merkle.rs` is 541 lines of which most are
regression tests whose names are themselves a list of v2 edge cases somebody
got wrong: `verify_tail_padding_fix`,
`test_v2_small_file_less_than_piece_len`, `test_v2_merkle_parity_regression`,
`test_compute_root_3_blocks_padding`.

**And one argument for the priority.** mkbrr
[Issue 112](https://github.com/autobrr/mkbrr/issues/112) (OPEN) is a v2 request
where the requester's own summary is that v2 is not really used by many people,
while noting what it does give: a stable per-file merkle hash and 16 KiB
re-download granularity. That is the honest case for keeping this P1 rather
than raising it. Against that, `bit-cli`'s
[T-084](#t-084-the-create-round-trip-has-not-been-proven-against-another-client)
round trip cannot cover `--version hybrid` until this lands, and validating v2
output needs a libtorrent leg in the interop harness, because libtorrent is the
only widely deployed BEP 52 implementation. That dependency is worth knowing
before starting: without it, a v2 torrent `bit-cli` writes is checked only
against `bit-cli`.

### T-082 BEP 16 superseeding is not implemented

Source:      the operator's brief
Category:    seeding
Priority:    P2
Effort:      M
Status:      open

Problem:     `bit-cli seed --superseed` is accepted and warns that it does
             nothing.
Relevance:   Superseeding is what makes initial distribution of a large payload
             from one seed efficient, which is exactly the netdisk case.
Approach:    Superseeding means advertising one piece at a time per peer and
             only advertising the next once the first has been seen elsewhere
             in the swarm. That is picker and bitfield control, which
             `librqbit` does not expose. Same blocker as
             [T-032](performance.md).
Acceptance:  `bit-cli seed --superseed --json` reports, per peer, which single
             piece it was offered and when that changed.

**The corpus holds one implementation, and it is worth reading mostly for what
it leaves out.** `rustorrent/src/main.rs:10577`: in super-seed mode an
outbound connection sends **a single `Have` for one pseudo-randomly chosen
piece instead of the bitfield**, and remembers it. `:11050`: when that peer
later advertises the same piece, proving it redistributed it, advance to
`(index + 1) % piece_count` and send that. `:12588` is the inbound path doing
the same, seeded from the peer tag. The `else` branches at `:10593` and
`:12601` carry the BEP 3 rule that a bitfield is always the first message after
the handshake even when it is empty, which is the same rule
[T-166](peers.md) exists to keep true here.

What that implementation does **not** do is the harder and more important half,
and a `bit-cli` acceptance has to cover it: tracking which piece each peer was
given, refusing to advance until redistribution is confirmed **to a different
peer**, and disconnecting a peer that never redistributes. Without those three
it is piece-at-a-time advertising rather than superseeding, and it gives away
more copies than a plain seed would.

This does not change the blocker. `librqbit` exposes neither the bitfield sent
at handshake nor per-peer `Have` control, so this stays open on the same wall
as [T-032](performance.md) and [T-002](webseed.md). What the corpus adds is
that the algorithm is now written down, so when that wall moves the work is
small.

### T-083 Seeding does not report choke state or disconnect reasons

Source:      the operator's brief
Category:    seeding
Priority:    P2
Effort:      M
Status:      open

Problem:     See [T-024](peers.md). The seed report carries bytes, pieces,
             chunks, errors, direction, client, and connect time, and not choke
             history or why a peer left.
Relevance:   A3.4b names both.
Approach:    Blocked on the same upstream stats gap.
Acceptance:  As T-024.

**What the state to be reported actually is**, from
`vortex/bittorrent/src/torrent.rs:488` `recalculate_unchokes`, which is the
fullest choking implementation in the corpus and is the seeding half in
particular. A peer that is not interested, or is pending disconnect, is choked
immediately and its round counters reset; if it held the optimistic slot the
optimistic timer resets too, so somebody else gets it. **Leeching** sorts by
`downloaded_in_last_round` descending. **Seeding** is libtorrent-style round
robin: a peer unchoked for over a minute that has received more than
`piece_length * seeding_piece_quota` bytes is demoted, ties breaking on
`uploaded_in_last_round` and then on time since last unchoke. One fifth of
`max_unchoked`, minimum one, is reserved for optimistic unchokes (`:594`
`recalculate_optimistic_unchokes`), and a previously optimistic peer that earns
a normal slot is promoted with the timer reset. Config at `:55-97`:
`max_unchoked = 8`, recalculated every 15 ticks, optimistic every 30.

Every quantity in that paragraph is a field this entry wants reported, which is
the useful part: the report shape follows from the algorithm, and `bit-cli`
does not have to invent one. fx-torrent
[PR 79](https://github.com/yoep/fx-torrent/pull/79) is the hazard that comes
with it, upload-slot bookkeeping that deadlocked the whole torrent tick, and
is the reason to report the state rather than only compute it.

### T-084 The create round trip has not been proven against another client

Source:      the operator's brief, testing matrix item 14
Category:    create
Priority:    P0
Effort:      M
Status:      **done** for v1, `--private`, and `--web-seed`. The
             `--version hybrid` case waits on T-081.

Problem:     `bit-cli create` then `verify` then `seed` then a download by a
             different client had never been run. Determinism was proven
             against `bit-cli` itself; interoperability was not proven at all.
Relevance:   A torrent nobody else can read is not a torrent. This was the
             single most important untested claim in the tool.
Approach:    `scripts/interop-roundtrip.ps1` runs the whole round trip on
             loopback. Two fixtures make it possible without a network:
             `cargo run -p bit-cli-core --example loopback-tracker`, a BEP 3
             HTTP tracker that lets two clients on one machine find each other
             without the DHT or LSD, and
             `cargo run -p bit-cli-core --example loopback-fileserver`, a
             static server with byte ranges for the web seed case.
Acceptance:  Byte-identical payload from the second client, with the exact
             commands and the resulting hashes recorded here.

Evidence:    Run at 2026-08-19T18:55:16.569Z on Microsoft Windows 10.0.26200,
             against aria2 1.37.0 (`aria2c --version`), a different
             implementation in a different language.

    pwsh -NoProfile -File scripts/interop-roundtrip.ps1 -Keep

             And again at 2026-08-19T22:21:04.696Z against rqbit 9.0.0, a
             third implementation:

    pwsh -NoProfile -File scripts/interop-roundtrip.ps1 -Client rqbit

    CASE      RESULT  INFO HASH                                 BYTES
    v1        pass    a6291a9a2794b3ff158e6db9d9424e6b166ddca7  490012
    private   pass    7240f139d5bbabedba0e2c7522bcafd6b087e8c5  490012
    webseed   skip    rqbit does not implement BEP 19

             Re-run on 2026-08-21 against **aria2 1.37.0 (3 of 3 pass) and
             rqbit 9.0.1 (2 of 2 pass, web seed skipped)**. The `rqbit` binary
             moved from 9.0.0 to 9.0.1 between the two runs and the info hashes
             are unchanged, which is what a byte-for-byte round trip is
             supposed to survive. The `librqbit` **crate** this tree depends on
             is still pinned at 9.0.0 in `Cargo.lock`; the interop client and
             the dependency are two different things and only the first moved.

             The web seed case is skipped for `rqbit` and named in the report's
             `cases_skipped`, never silently dropped. Skipping is correct here:
             the case asks the second client to resolve a `url-list` with no
             peer at all, and a client without BEP 19 cannot, which says
             nothing about `bit-cli`. That absence is the gap this project
             exists to fill.

             The two clients are checked differently at the parse step because
             they print different things. `aria2c -S` prints the info hash, so
             that is asserted. `rqbit download --list` prints the file list and
             not the hash, so the file names are asserted. Agreement on the
             info hash is proven either way by the transfer: the tracker keys
             its swarm on the hash, so a client that computed a different one
             never finds the seeder and the case fails.

             Exit code 0. Payload: 4 files, 490012 bytes, one directory name
             carrying a space (`disc 1/`), 32 KiB pieces, 15 pieces. The
             payload bytes are generated by a fixed LCG in the script, so the
             info hashes below reproduce.

    CASE      RESULT  INFO HASH                                 BYTES
    v1        pass    a6291a9a2794b3ff158e6db9d9424e6b166ddca7  490012
    private   pass    7240f139d5bbabedba0e2c7522bcafd6b087e8c5  490012
    webseed   pass    a6291a9a2794b3ff158e6db9d9424e6b166ddca7  490012

             Per case, in order: `bit-cli create` wrote the `.torrent`,
             `bit-cli verify` reported `complete: true`, `aria2c -S` reported
             the same info hash, and `aria2c` downloaded to a fresh directory.
             Every file matched its source SHA-256 and no extra file appeared.

             `v1` and `private` transferred over BitTorrent from `bit-cli
             seed`. The seeder's own final report accounts for the bytes, so
             the payload is not attributed by inference:

    "uploaded": 490012, "peers_served": 1, "ratio": "1.000"

             The tracker log shows both ends of that swarm, `-rQ9000-`
             announcing `left=0` and `A2-1-37-0-` announcing `left=490012` and
             then `event=stopped` with `left=0`.

             `webseed` had no peer and no tracker at all. `aria2c` resolved the
             `url-list` and fetched the four files over HTTP, and the server
             log shows the BEP 19 composition including the percent-encoded
             space:

    GET /payload/disc%201/a.flac range=bytes=0-299999 -> 206 300000 byte(s)
    GET /payload/disc%201/b.flac range=bytes=0-149999 -> 206 150000 byte(s)
    GET /payload/extras/notes.nfo range=bytes=0-39999 -> 206 40000 byte(s)
    GET /payload/tiny.bin range=bytes=0-11 -> 206 12 byte(s)

             The script asserts the served total covers the payload, so the
             case cannot pass on bytes that came from somewhere else.

             The `v1` and `webseed` info hashes are identical by design:
             `announce` and `url-list` sit outside the info dict, so attaching
             either does not change it. `--private` does change it, because
             `private` is inside.

             The failure path is exercised too. `-TimeoutSeconds 1` fails all
             three cases and exits 1, naming the unmet deadline, the seeder
             that served nobody, and every hash mismatch. A missing client
             exits 2.

Remaining:   1. `--version hybrid` is not covered because it does not exist.
                Tracked by T-081;
                add a fourth case to the script when it lands.
             2. `transmission` cannot join the matrix on Windows.
                `winget install Transmission.Transmission` was run here on
                2026-08-19 and installs version 4.1.3, which ships
                `transmission-qt.exe` and nothing else: no `transmission-cli`,
                no `transmission-remote`, no `transmission-daemon`, no
                `transmission-show`. Verified with
                `find "/c/Program Files/Transmission" -iname "*.exe"`. A GUI
                cannot be driven headlessly, and rule 0.11 makes a
                TTY-dependent test worthless anyway. What would unblock it: the
                Linux side of the `interop` CI job, where
                `apt-get install transmission-cli` gives a real command-line
                client. Tracked as item 4 below.
             3. `ci.yml` carries an `interop` job on Linux and Windows that
                installs `aria2` and runs the script. It has not run: nothing
                is pushed. Same blocker as T-085.
             4. The Linux leg of that job should also install
                `transmission-cli` and run the script a third time with
                `-Client transmission-cli`, which needs a new branch in the
                script's invocation block. Not written yet, because it cannot
                be run here to check it.

### T-085 Creation determinism is not proven across platforms

Source:      the operator's brief, testing matrix item 15
Category:    create
Priority:    P1
Effort:      S
Status:      **done**

Problem:     Byte-identical output on repeat runs is tested. Byte-identical
             output between a Windows and a Linux build is not, and path
             separator handling is exactly the bug that catches.
Relevance:   Reproducible builds of a torrent are what let two mirrors publish
             the same info hash independently.
Approach:    A CI job that builds the same fixture on both platforms and
             compares the BLAKE3 of the `.torrent`.
Acceptance:  `ci.yml` carries the job and it passes.

**The job exists and a second, stronger check now runs beside it.**

`ci.yml` carries `determinism`, which builds the same fixture on
`ubuntu-latest` and `windows-latest` and uploads the SHA-256 of the
`.torrent`, and `determinism-compare`, which fails when the two differ. That
is the acceptance, and it holds only for the commit CI ran on.

The stronger check is a constant.
`cmd::create::tests::a_fixture_torrent_hashes_the_same_on_every_platform`
builds the same fixture the job builds and asserts its SHA-1 is
`069804535e172027dfd40388bc0b7a64d8e8770b`. The test suite runs on both
platforms in CI, so both compare against one number rather than against each
other, and a platform added later is checked by the same line. It also fails
locally, where the job cannot run at all.

The fixture is deliberately the job's: two files, one nested, sorted by path,
`--no-creation-date --no-created-by --piece-length 16KiB`, and `--name
fixture` so the temporary directory's own name cannot reach the metainfo. The
last one is what makes the constant stable across runs on one machine as well
as across platforms.

**The run is in.** CI run 32407214253, 2026-08-20:

| job | result |
| --- | --- |
| `Create determinism (ubuntu-latest)` | pass, 38s |
| `Create determinism (windows-latest)` | pass, 1m35s |
| `Compare determinism hashes` | pass, 4s |

https://github.com/Azathothas/bit-cli/actions/runs/32407214253

The compare job is the one that matters: it is what fails when the two
platforms disagree, and it had never been green before because the run it
first appeared in was red for other reasons. The two hashes it compared are
equal, and the same commit's `Test (windows-latest)` asserted the constant, so
the number both platforms produce is also the number written down.

### T-175 create does not normalise NFD filenames

Source:      `reference/RESEARCH.md` section D, 2026-08-21
Category:    create
Priority:    P2
Effort:      M
Status:      open

Problem:     `bit-cli create` writes whatever bytes the filesystem hands it.
             On macOS, and on any SMB or NFS mount reached from it, that can be
             **NFD**: a decomposed spelling where `é` is `e` plus a combining
             acute, while the same name stored on the origin was **NFC**, one
             code point. The two are different byte strings and therefore
             different paths in a torrent, and `bit-cli` runs on macOS.
Relevance:   mkbrr [Issue 182](https://github.com/autobrr/mkbrr/issues/182)
             (CLOSED, fixed by
             [PR 183](https://github.com/autobrr/mkbrr/pull/183)) is the
             best-documented cross-platform torrent-creation bug in the corpus,
             and what makes it worth a P2 here is **when** it is discovered.
             Torrents were created on macOS against an SMB mount from a
             Synology NAS. The NAS stored NFC; the mount presented NFD; the
             torrent recorded NFD. macOS path lookup is
             normalisation-insensitive, so the torrent verified clean locally
             **including with the tool's own check command**. The breakage
             appeared only on Linux and Windows, which is to say only after the
             torrent was published. The reported case was a 41-file season pack
             with 19 accented names, all of which showed as missing to
             everybody who was not the person who made it.

             A creation bug that a local verify cannot see is the worst kind
             this project can ship, because `bit-cli`'s whole answer to "did
             this work" is `bit-cli verify` and an interop round trip, and both
             would have passed.
Approach:    `mkbrr/torrent/normalize.go` is the fix and its restraint is the
             interesting part.

             `:18` `decomposed(s)` returns true only when `s` differs from its
             NFC form **purely by combining marks**. Canonical singletons
             (U+212B ANGSTROM SIGN, the CJK compatibility ideographs) and
             composition exclusions are deliberately excluded, because for
             those "the bytes are what the filesystem genuinely holds, not a
             decomposition artifact". Rewriting them would corrupt a legitimate
             name.

             `:58` `nfcPath(dir, rel)` rewrites to NFC **only when `Lstat`
             proves both spellings are `os.SameFile`**. That is the whole
             safety argument: on a filesystem that genuinely stores NFD, the
             NFC spelling does not resolve, so nothing is rewritten. The
             rewrite happens only where the filesystem itself says the two
             names are one file.

             `:80` `pathKey` and `:86` `resolveNormalized` are the
             comparison-only half, for matching a torrent written in one form
             against files stored in the other. That half belongs in `verify`
             and in the path planner rather than in `create`.
Acceptance:  A fixture directory whose names are NFD on disk produces a torrent
             whose paths are NFC, on a filesystem where both spellings resolve
             to the same file; and the same fixture on a filesystem that
             genuinely stores NFD produces NFD, unchanged. The second half is
             the one that proves the rule was implemented rather than a blanket
             `ToNFC`. If the second cannot be tested on CI's runners, say so
             here and test the predicate directly.

             A lint is the cheaper half and worth landing first: a path that is
             decomposed by the `:18` definition fires `nfd-path`, clearable
             with `--allow`, so a user who means it can proceed and a user who
             does not finds out before publishing.

### T-176 Three lints the corpus names are missing, and one message is wrong

Source:      `reference/RESEARCH.md` section D, 2026-08-21
Category:    create
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T15:30Z

Problem:     `bit-cli create` refuses on ten lints
             (`crates/bit-cli-core/src/torrent/lint.rs:26`):
             `private-no-tracker`, `piece-count`,
             `piece-length-not-power-of-two`, `empty-payload`, `empty-file`,
             `windows-path`, `case-collision`, `bad-web-seed`, `bad-tracker`
             and `long-path`. Three checks the corpus names are missing, and
             two of the three are cases where a torrent `bit-cli` calls clean
             cannot be opened by a widely deployed client.

             **1. More than 65535 pieces.** intermodal
             [Issue 499](https://github.com/casey/intermodal/issues/499)
             (OPEN): **µTorrent refuses to open such a torrent.** `bit-cli`'s
             `piece-count` lint fires above **100,000**
             (`lint.rs:172`), which is above that limit, so the band from
             65,536 to 100,000 pieces passes every check `bit-cli` has and
             produces a torrent µTorrent cannot open. That is not a style
             opinion, it is a client that will not read the output.

             **2. A piece length over 16 MiB.** intermodal
             [Issue 358](https://github.com/casey/intermodal/issues/358)
             (OPEN) gives 16 MiB as the practical ceiling, and larger has been
             reported to break clients. `bit-cli` agrees with that ceiling in
             one place and not the other: `piece_length.rs:23` caps the
             **automatic** choice at `MAX = 16 MiB`, while
             `piece_length.rs:59` `validate` refuses only zero, so
             `--piece-length 64MiB` is accepted in silence. The doc comment on
             `validate` explains why small lengths are permitted, which is
             sound, and says nothing about large ones.

             **3. Duplicate paths.** create-torrent
             [Issue 126](https://github.com/webtorrent/create-torrent/issues/126)
             (OPEN): two inputs with the same relative path produce a torrent
             with duplicate entries, which is invalid. `bit-cli` **does** catch
             this, but by accident and under the wrong name: `lint.rs:213`
             keys a `BTreeSet` on `path.to_lowercase()`, so an exact duplicate
             collides too and fires `case-collision` with the message
             "collides with another path that differs only in case", which is
             false when the two paths are identical. A user reads that message
             and goes looking for a casing difference that is not there.
Relevance:   All three are one-line checks against a table `bit-cli` already
             has, and two of them are the difference between a torrent that
             works everywhere and one that works here. The third is a wrong
             sentence in an error, which is the class of defect
             [T-147](windows.md) already cost a red job over: the disk paths
             agreed and the reason in `--json` did not.
Approach:    Two new lints and one message split.

             `piece-count` gains a second threshold at 65,535 with its own
             message naming µTorrent, or a separate `piece-count-uopenable`
             lint if the two want to be cleared independently, and they do, since
             one is a performance opinion and the other is a compatibility
             fact.

             `piece-length-too-large` fires above 16 MiB from `validate`'s
             caller, not from `validate`, so the "only zero is impossible" rule
             in that function stays true and the judgement stays in the linter
             where the rest of the judgement lives.

             `duplicate-path` fires when the exact path repeats;
             `case-collision` keeps its current message and fires only when the
             paths differ. Insert into two sets rather than one.

             `mkbrr/internal/trackers/trackers.go:319` `DefaultPieceSizeRanges`
             is worth reading alongside: fourteen bands from 32 KiB at 64 MB to
             128 MiB above 128 GB, with `:310` `GetTrackerMaxPieceLength` and
             `:336` `GetTrackerPieceSizeExp` applying **per-tracker** caps as a
             hard ceiling a user may lower and not exceed. That is a larger
             feature than this entry and is not proposed here, but it is where
             the 16 MiB number comes from in practice and where anyone raising
             it should look first.
Acceptance:  A payload producing 70,000 pieces fires the new lint and names
             µTorrent; `--piece-length 64MiB` fires
             `piece-length-too-large`; two identical input paths fire
             `duplicate-path` and not `case-collision`; and two paths differing
             only in case still fire `case-collision`. Four cases, one test
             each, all clearable with `--allow`.

**Done, and all three claims held against the tree.** It is the one entry this
session whose **Approach** survived contact as well as its premise: T-219's and
T-222's premises held and their Approaches did not, and T-173's premise did
not. Checked before anything was written:
`lint.rs` fired `piece-count` above 100,000 and nothing below it;
`piece_length::validate` refused only zero while `MAX = 16 MiB` capped the
automatic choice alone; and the collision check keyed one `BTreeSet` on the
lower-cased path, so an exact duplicate fired `case-collision` with a message
about case.

**Two lints and one message split, ten lints to thirteen.**

`piece-count-unopenable` fires above 65,535 and its message names µTorrent. It
is a separate lint rather than a second threshold on `piece-count` because the
two are different kinds of thing and clear independently: one is an opinion
about how much hash data is reasonable, the other is a client that refuses the
file. A caller who has decided to live with 200,000 pieces of hash data has not
thereby decided to ship a torrent µTorrent cannot read.

`piece-length-too-large` fires above `piece_length::MAX`, from the linter
rather than from `validate`, so the "only zero is impossible" rule in that
function stays true and the judgement stays where the rest of the judgement
lives. The constant is read from `piece_length::MAX` rather than written again,
so the automatic ceiling and the lint cannot drift.

`duplicate-path` fires when the exact path repeats. Two sets rather than one,
and `case-collision` keeps its message and now only fires when the paths
actually differ.

**Four cases, one test each, and two of them assert what does not fire**, which
is the half that would have passed with the old code:

| test | what it holds |
| --- | --- |
| `a_piece_count_above_65535_is_unopenable_and_says_so` | 70,000 pieces fires the new lint, **not** `piece-count`, and names µTorrent |
| `the_two_piece_count_lints_clear_independently` | 120,000 fires both, and `--allow piece-count` leaves the other |
| `a_piece_length_above_16_mib_is_reported` | 64 MiB fires it and exactly 16 MiB does not |
| `two_identical_paths_are_a_duplicate_and_not_a_case_collision` | fires `duplicate-path`, not `case-collision`, and the message does not say "case" |
| `two_paths_differing_only_in_case_still_collide` | the case that is a case collision still is one |

```bash
cargo test -p bit-cli-core --lib torrent::lint
```

**The manuals did not change and that is correct**: `--allow` takes a lint name
as a free-form value rather than an enumerated one, so the surface is the same.
The list a program reads is `bit-cli version --json` under `lints`, which
reports thirteen now, and `README.md` gained a section saying why two of them
are about what a recipient's client will refuse rather than about tidiness.

**What was not taken.** `mkbrr`'s per-tracker piece-size ceilings, which the
Approach names as where the 16 MiB number comes from in practice. That is a
feature rather than a lint and the entry already said it was not proposed here.


### T-225 The interop script hashes files the client it just killed still holds

Source:      CI run 32649574641, `Create round trip (windows-latest)`, 2026-08-23
Category:    create
Priority:    P1
Effort:      S
Status:      **done** 2026-08-23T15:55Z

Problem:     A push carrying `TODO/`, `bench/`, two documentation files and one
             comment turned `Create round trip (windows-latest)` red:

             ```
             Get-FileHash: scripts/interop-roundtrip.ps1:196
             The process cannot access the file
             '...\.tmp\interop\out-v1\payload\disc 1\a.flac'
             because it is being used by another process.
             ```

             The timestamps say what happened. `seeder announced` at
             15:49:04 and the failure at 15:52:04, which is exactly the
             `-TimeoutSeconds 180` CI passes. The leech did not finish inside
             its budget, `Invoke-Recorded` force-killed it, and the script went
             straight on to hash the output directory.

             `Stop-Process -Force` returns before Windows has finished tearing
             a process down, so `aria2c` still held its output files open. The
             run then failed on a sharing violation whose message names neither
             the client nor the timeout that caused it.
Relevance:   **A slow runner became a red job with a message about the wrong
             thing**, which is the worst shape a CI failure can have: the next
             session debugs `Get-FileHash` rather than reading "the download
             did not finish in 180 seconds".

             It is also the seventh entry of the family
             [RULES.md](../TODO/RULES.md) section 5 names, and the first one in
             a `scripts/` acceptance rather than in a `cargo` test: a step that
             assumes a process is gone because it was asked to go is the same
             assumption as waiting a guessed duration.

             Nothing in the push could have caused it. The commit changed no
             source the interop path touches, which is the cleanest available
             proof that the script was wrong rather than the tree, and this
             repository has had that proof four times before.
Approach:    Two changes, and both wait on the condition.

             `Invoke-Recorded` waits for the process to actually exit after it
             kills it, bounded at 30 seconds, so nothing downstream runs while
             a killed client is still holding handles.

             `Get-TreeHashes` hashes through `Get-FileHashWhenReadable`, which
             retries a sharing violation until the file opens or 30 seconds
             pass and then throws with the path and the wait in the message. A
             violation there is transient by construction: the only thing that
             had the file is the client this script started and has already
             stopped.
Acceptance:  The round trip passes locally, and a timeout reports itself as a
             timeout rather than as a failure to read a file.

**Done.** Both are in `scripts/interop-roundtrip.ps1`, each with the comment
that says why the wait is on the condition.

**The round trip passes**, three of three cases byte for byte, against
`aria2 version 1.37.0`:

```
v1         pass     a6291a9a2794b3ff158e6db9d9424e6b166ddca7   490012 bytes matched
private    pass     7240f139d5bbabedba0e2c7522bcafd6b087e8c5   490012 bytes matched
webseed    pass     a6291a9a2794b3ff158e6db9d9424e6b166ddca7   490012 bytes matched
```

```bash
pwsh -NoProfile -File scripts/interop-roundtrip.ps1 -TimeoutSeconds 180
```

**What is fixed and what is not.** The reporting is fixed: a leech that runs
out of budget is now recorded as `timed_out` with its own message rather than
crashing the script three lines later. **Why that leech needed more than 180
seconds on that runner is not answered**, and this entry does not claim to. The
local run above takes 2,143 milliseconds for the same case, so the budget is
not tight by any ordinary measure. If the job goes red on a genuine timeout,
that is a different entry and it will now say so in its own words.
