# Reference map

What is kept under `reference/`, and under what licence.

This file keeps the licence determinations, because those are the
safety-critical half and have to survive `reference/` being deleted. See
`TODO/licensing.md` for how each was made.

**No tracked file cites a path under `reference/`, and none depends on
anything there.** Every finding a `TODO/` entry rests on is written into that
entry, which is what T-122 below closed. A corpus citation in a `TODO/` entry
names where somebody else solved a problem; it is never evidence that `bit-cli`
solves it. `reference/` is untracked working material and a reader without it
can still work through this list.

## The corpus as it stands

The operator replaced the four-tree corpus with **twenty-two upstream
implementations** on 2026-08-21 and added **seventeen more** on 2026-08-24, so
the corpus is **thirty-nine trees**, indexed by `reference/RESEARCH.md`. That
file is the entry point: three tiers by usefulness, then cross-cutting sections
A to H. Section D maps `bit-cli` TODO ids to the best source for each, section C
lists eleven metainfo shapes a parser has to survive, section F is the licence
determination, section G records what was removed during cleaning and why.

Two of the 2026-08-24 sources are not trees and are not counted in the
thirty-nine: `TheDancingDeveloper-org`, an organisation of 33 repositories
triaged read-only and cloned not at all, and a Rust GUI survey, which is a
document. `RESEARCH.md` entries 40 and 41 carry both.

| Tree | Licence | Where the determination came from |
| --- | --- | --- |
| `intermodal` | CC0-1.0 | `LICENSE` |
| `torrent`, `TorrentNG`, `superseedr`, `fx-torrent`, `mkbrr`, `gosh-dl`, `vortex`, `rustorrent`, `seedchamp`, `aria2_rust`, `FluxDown`, `aquatic`, `torrust-actix`, `create-torrent`, `parse-torrent`, `bqti`, `dht-spider`, `tc` | MIT | each tree's own licence file |
| `nanotorrent`, `mtorrent` | MIT | `Cargo.toml` only, no licence file |
| `n0-mainline` | **MIT OR Apache-2.0** | `LICENSE-MIT` and `LICENSE-APACHE` upstream, and `Cargo.toml`. **The corpus copy kept only `LICENSE-MIT`**, so this file recorded MIT alone until 2026-08-24 |
| `RatioTracker`, `Seedr`, `RatioForge`, `rustatio`, `demagnetize-rs`, `dht-crawler`, `tcp-transfer-ice`, `iroh-fm`, `ed2k-server`, `fake-torrent-client` | MIT | each tree's own licence file. `fake-torrent-client`'s leaves the holder and year as the template placeholders |
| `joal` | **Apache-2.0** | `joal/LICENSE`. The only Apache-2.0 tree in the corpus, and the one `scripts/make-client-profile.ps1` is an independent implementation from |
| `iroh-experiments` | Apache-2.0 OR MIT | `LICENSE-APACHE` and `LICENSE-MIT` |
| `dig-nat` | Apache-2.0 OR MIT | `Cargo.toml` and README, no licence file |
| `Hollow` | MIT | `Cargo.toml` and README, no licence file |
| `DOAL` | MIT, claimed | **one README line only**, and it forks Apache-2.0 `joal` |
| `NetDrop` | **conflicting** | `LICENSE` is GPL-3.0 and `Cargo.toml` says MIT |
| `gaia` | **none found** | no licence file, no manifest key, no statement in any document |

Thirty-five permissive and unambiguous, one CC0-1.0 among them, and **four that
need care**: `DOAL`, `NetDrop`, `gaia` and the `librtbit` family that
`RESEARCH.md` entry 40 triages. `RESEARCH.md` section F carries the per-tree
evidence.

**Nothing in the corpus is copied into this repository**, which is what makes
an unclear licence a reading question rather than a shipping one. The one
exception the rules allow is `intermodal`, CC0-1.0. Three of the four above are
read-only for a second reason as well: `NetDrop`'s GPL-3.0 file, `gaia`'s
absence of any statement, and `DOAL`'s relicensing of an Apache-2.0 work.

Two to handle with care, both recorded in section F: `tc`, whose README and
`LICENSE` disagree, and `vortex`, whose badge and `LICENCE.txt` disagree. In
both cases the file on disk is MIT. Confirm before reusing anything from
either.

**Two records this replaces.** `fx-torrent` was recorded here as Apache-2.0;
its own `LICENSE` file is MIT, and the Apache determination was wrong. And this
file described a four-tree corpus of `intermodal`, `fx-torrent`, the `aria2`
documentation and the `rqbit` issue JSON. That corpus is gone, superseded
rather than deleted, and the `aria2-next` and `rqbit` trees it named are not
present. The `rqbit` issue corpus is still the source of most entry `Source:`
lines below and in every other file here; those lines record where an entry
came from and stay true whether or not the JSON is on disk.

Forgetting which pile a file came from is how a copyleft function ends up in an
MIT tree. That risk is why this table exists, and with a wholly permissive
corpus the table is now a record rather than a fence.

## One tree the index names as kept and that is not on disk

`intermodal/book/` is listed in `RESEARCH.md` section G as deliberately
retained, and `intermodal/README.md` points at
`book/src/bittorrent/bep-support.md` by name. Neither is on disk: `intermodal/`
holds `LICENSE`, `README.md`, `benches`, `build.rs` and `src`. Checked with
`Test-Path` during the doc pass of 2026-08-21, which is the whole reason to
check a path before citing it. **No `bit-cli` document cites anything under it**
and none ever did.

**Nothing in it is lost, and it was generated rather than authored.** The book
is an mdBook whose CLI pages are produced at build time from `intermodal`'s own
`clap` definitions, so the half that documented `imdl` is a rendering of
`intermodal/src/`, which **is** on disk and is what
[create-seed.md](create-seed.md) already cites for the `create` option surface.
The four pages worth anything to `bit-cli` are each covered better here than
they were there:

| Page | Where the same ground is covered |
| --- | --- |
| `bep-support.md`, a BEP 0 to 55 matrix | [bep-coverage.md](bep-coverage.md), which now carries 30 rows and gives every one a symbol in this tree or the entry id that closes it. A status column for somebody else's tool is strictly less. |
| `piece-length-selection.md`, `piece-length.md` | `crates/bit-cli-core/src/torrent/piece_length.rs:1-14` states the trade in full: metadata size against transfer granularity, the BEP 9 exchange cost, lossy links, and web seed scope granularity. `RESEARCH.md` section B tabulates five algorithms with their bounds, and [T-176](create-seed.md) carries the ceilings and why they are what they are. |
| `udp-tracker-protocol.md`, the BEP 15 wire format | `crates/bit-cli-core/src/tracker.rs` implements it, with a test asserting the announce request is exactly 98 bytes. [T-064](trackers.md) and [T-180](trackers.md) carry the retry and parse questions the prose would not have answered. |
| `metainfo-utilities.md`, `distributing-large-data-sets.md`, `prior-art.md` | Surveys of other tools. No bearing on anything here. |

So the correction is to `RESEARCH.md` section G's retained list and to
`intermodal`'s own trimmed README, not to any entry. Both are left as written,
because they are the corpus's record of its own cleaning and editing them here
would make this file disagree with the tree it describes. This paragraph is the
correction.

`cargo deny` refuses copyleft dependencies outright, and
`scripts/check-licence-gate.ps1` proves it against a probe crate. That gate is
what makes the boundary mechanical.

## What is not under reference/

The `librqbit` source. It is a crates.io dependency, and every claim in `TODO/`
about "the pinned 9.0.0" was verified against the registry cache at
`~/.cargo/registry/src/index.crates.io-*/librqbit-9.0.0/`.

---

### T-122 The copyleft and unlicensed reference trees are deleted

Source:      the operator's decision, closed on 2026-08-21
Category:    licensing
Priority:    P2
Effort:      S
Status:      **done**

Problem:     `reference/` held **three** copyleft source trees and one
             unlicensed one inside the working directory of an MIT project. It
             is gitignored, so it could not be committed by accident, but it
             was one `git add -f` away.
Relevance:   The safest copy of an AGPL tree is the one that is not there.
Approach:    Delete them, and rewrite every entry that cited one so it rests on
             the specification or the decision rather than on the tree.
Acceptance:  None of the four is on disk, no tracked file names one, and every
             entry that used to cite one still says what it needs to say.

**Done.** Four trees were removed on 2026-08-21: two AGPL-3.0, one
GPL-3.0-or-later, and one with no `LICENSE` file at all. The reason is the one
this entry always gave, plus the operator's: their licences are incompatible
with MIT and the work they were read for is finished.

**Nothing was taken from any of them.** The provenance tables in
`TODO/licensing.md` that existed to record it were empty when they were
deleted, and they were empty because every finding in the corpus is written as
a description of a technique with a citation, never as a snippet.

Four entries cited one of the four by path, and all four now stand on their
own:

| Entry | Cited | Now rests on |
| --- | --- | --- |
| [T-081](create-seed.md) | a v2 merkle implementation | BEP 52 itself, with the padding-truncation case written into the entry |
| [T-092](bench.md) | a synthetic load generator | the four properties the entry needs, listed in its Approach |
| [T-102](bep-coverage.md) | a NAT traversal crate | BEP 55, and the `librqbit` type that blocks it |
| [T-207](phase-c.md), [T-209](phase-c.md) | a status command's mode enum | decision 7.4 and the aria2 parity list |

What was **not** deleted: `intermodal` (CC0-1.0), `fx-torrent`, the `aria2`
documentation, and the `rqbit` issue corpus, which is JSON rather than code.
Those are permissive or are data, they are still cited, and the reason this
entry existed does not apply to them.

Two corrections to this entry as it was written, neither of which changes what
it did. `fx-torrent` is **MIT**, not Apache-2.0: its `LICENSE` file says so and
the earlier determination was wrong. And the corpus this entry describes was
replaced on 2026-08-21 by the twenty-two trees above, so the `aria2-next` and
`rqbit` directories it names are no longer on disk. The entry stays **done**:
what it closed was the removal of four incompatible trees and the rewriting of
the four entries that cited them, and both of those happened.
