# Licensing

`bit-cli` ships under **MIT** alone (decision 7.1). This file records the
determination for every project it reads, copies from, or depends on, and the
per-subsystem provenance for anything learned from a copyleft reference.

Determinations were made on 2026-08-19 and extended on 2026-08-21, against the
trees under `reference/`, which is gitignored and untracked. This file is what
may be done with them, and it stands on its own: nothing tracked depends on
anything under `reference/`.

**On 2026-08-21 the four trees whose licence is incompatible with MIT were
deleted.** They were read for shape and nothing was taken from any of them.
What that decision means, and how the entries that used to cite them now
stand, is in [reference-map.md](reference-map.md), T-122.

---

## The references

| Project | License | May copy code? | Verified from |
| --- | --- | --- | --- |
| `kist` | MIT OR Apache-2.0 | Yes, under MIT | `LICENSE-MIT` in the fork base |
| intermodal | CC0-1.0 | Yes, adapt directly | its `LICENSE` |
| the other reference trees | mostly MIT, `joal` Apache-2.0, four unclear | read only. Nothing is copied from any of them | each tree's own licence file where there is one; `reference/RESEARCH.md` section F, and `TODO/reference-map.md` |
| `rqbit` issue and PR corpus | n/a, data | It is JSON, not code | the GitHub API |

**The corpus is thirty-nine trees**, and it stopped being uniformly
permissive on 2026-08-24. `intermodal` is CC0-1.0, `joal` is Apache-2.0, most
of the rest are MIT, and four are unclear or conflicting: `DOAL`, `NetDrop`,
`gaia`, and the `librtbit` family that `RESEARCH.md` entry 40 triages. None of
that reaches a shipped artefact, because nothing in the corpus is copied into
this repository.
[reference-map.md](reference-map.md) lists them and `reference/RESEARCH.md`
section F carries the per-tree evidence. Copying from any of them attaches the
MIT notice requirement and nothing more, which `cargo about` already handles for
every dependency; `intermodal` attaches nothing at all.

**fx-torrent was recorded here as Apache-2.0 and it is MIT.** Its `LICENSE`
file is the plain MIT text. The Apache determination was wrong, and the
paragraph that followed it, about Apache-2.0 section 4's licence-text,
`NOTICE` and statement-of-changes requirements, described obligations that do
not apply. Nothing was copied from it under either reading, so the error
attached no obligation that went unmet. It is corrected rather than deleted
because a licence determination that was once wrong is worth leaving visible:
the lesson is to read the file rather than the badge or the manifest, which is
the same lesson `vortex` and `tc` carry in section F.

**Two trees state their licence only in a manifest.** `nanotorrent` and
`mtorrent` carry no `LICENSE` file, and both declare `license = "MIT"` in
`Cargo.toml`. Those two manifests were kept back from the corpus's manifest
sweep for exactly that reason. A manifest is weaker evidence than a licence
file, and the `fx-torrent` error above is what that weakness looks like when it
goes wrong, so treat both as MIT and check upstream before copying from either.

### The four that were deleted

Two AGPL-3.0 trees, one GPL-3.0-or-later, and one with no `LICENSE` file at
all. None of them may be copied from, and an absent licence is the strictest
case of the four: it is not permissive, it is all rights reserved.

**Nothing was taken from any of them, and the tables that would have recorded
it are empty.** The boundary was procedural rather than a matter of care: every
finding in the corpus is written as a description of a technique with a
citation to check it against, never as a snippet, and every entry that cited
one of the four has been rewritten to stand on the BEP or the decision it
actually rests on.

They are deleted because a copy that is not there cannot be committed by
accident, and because the work they were read for is done. `cargo deny` refuses
copyleft dependencies outright and
[check-licence-gate.ps1](../scripts/check-licence-gate.ps1) proves it against a
probe crate, so the boundary stays mechanical rather than a matter of memory.

### kist

Dual licensed MIT OR Apache-2.0, so taking its code under MIT alone is
permitted: a disjunctive dual license lets the recipient pick either half.

The MIT half requires the original copyright notice to survive. The exact
holder string, read from the upstream `LICENSE-MIT`, is:

```
Copyright (c) 2026 Rabindra Dhakal
```

**This corrects the original prompt**, which said the holder was "QaidVoid".
That is the GitHub account; the copyright line names a person. Use the string
above verbatim in `LICENSE`.

`LICENSE-APACHE` is deleted, because the Apache half of the disjunction is not
being exercised.

### Why `panic = "abort"` is not set

This is the one conclusion that arrived by way of a copyleft tree, and it is
worth stating on its own footing because the tree is gone.

`bit-cli`'s release profile does **not** set `panic = "abort"`, and the reason
is that a download manager wants `catch_unwind` to survive a task panic:
aborting the process on one poisoned torrent takes every other torrent in the
invocation with it. That is recorded in a comment in the release profile and
in decision 7.6. A shared conclusion about a Cargo profile setting is not a
derivative work of anything, and the reasoning stands without the tree that
prompted it.

### intermodal, CC0-1.0

CC0 is a public domain dedication, not a copyleft license. Its code may be
copied, adapted, and shipped inside an MIT project with no license obligation.
This is the one reference where copying is allowed.

Two caveats worth stating plainly:

1. **CC0 explicitly does not grant patent rights.** Section 4(a) of the CC0
   1.0 text: "No trademark or patent rights held by Affirmer are waived,
   abandoned, surrendered, licensed or otherwise affected by this document."
   Copying CC0 code carries no patent licence, express or implied. For a
   BitTorrent metainfo tool the practical risk is negligible, but the fact is
   recorded because "public domain" is often read as covering more than it does.
2. **Attribution is not required and is given anyway.** `intermodal` is
   credited in `THIRD_PARTY.md` because it costs nothing and it is accurate.

What was actually adapted from `intermodal`:

| Subsystem | From | What was taken |
| --- | --- | --- |
| `crates/bit-cli/src/env.rs` | `src/env.rs` | The pattern of injecting args, working directory, and the three streams into the program rather than reading globals. This is what makes the headless parity requirement in rule 0.11 testable rather than aspirational. The `bit-cli` implementation is written against `bit-cli`'s own types; the idea is theirs. |
| `crates/bit-cli-core/src/torrent/lint.rs` | `src/subcommand/torrent/create.rs` | The `--allow <LINT>` model: refuse at creation on conditions that are legal but usually mistakes, and require the lint to be named to proceed. Ten lints, each with a stable name usable in a script. |
| `crates/bit-cli/src/cli.rs`, `create` | `src/subcommand/torrent/create.rs` | The flag surface: `--announce-tier` building BEP 12 tiers, `--sort-by KEY:ORDER`, `--no-created-by`, `--no-creation-date`, `--glob` with a leading `!` for exclusion, `-o -` for stdout. |
| `crates/bit-cli-core/src/units.rs` | `src/bytes.rs` | Size parsing and formatting with binary units. |
| `crates/bit-cli/src/output.rs` | `src/table.rs` | Aligned table output. |

---

## Dependencies

### librqbit, Apache-2.0

`librqbit` and its sibling crates (`librqbit-core`, `librqbit-bencode`,
`librqbit-peer-protocol`, and the rest) are **Apache-2.0 only**, not dual
licensed. Copyright 2021 Igor Katson.

As an ordinary crates.io dependency this is fine: an MIT source tree may depend
on an Apache-2.0 crate. Two obligations follow, and both are on the binary
distribution rather than on the source:

1. Ship the Apache-2.0 licence text with any binary distribution.
2. Ship any `NOTICE` content the upstream provides.

Both go in `THIRD_PARTY.md` and in the release archives.

**If the section 2.2 benchmark concludes a patched `librqbit` should be
vendored** (Candidate B), three more obligations attach, from Apache-2.0
section 4:

- The vendored subtree keeps its own `LICENSE` intact.
- Every file that is modified carries a prominent notice saying it was changed.
- A `CHANGES` file inside the vendored directory records the modifications.

No fork exists today. `TODO/webseed.md` T-001 is the decision gate.

### Everything else

`THIRD_PARTY.md` is generated mechanically (`cargo about` or
`cargo bundle-licenses`) so it cannot drift from `Cargo.lock`, and regenerated
in CI.

---

### T-120 THIRD_PARTY.md is not generated

Source:      the operator's brief
Category:    licensing
Priority:    P1
Effort:      S
Status:      **done**

Problem:     No `THIRD_PARTY.md` exists, so no binary distribution can carry
             the Apache-2.0 text `librqbit` requires.
Relevance:   It is a licence obligation on every release, not a nicety.
Approach:    `cargo install cargo-about`, a `about.toml` naming the accepted
             licences, and a CI job that regenerates and fails on drift. The
             accept list is the check that matters: a new dependency under a
             copyleft licence has to fail the build, not appear quietly in a
             generated file.
Acceptance:  `THIRD_PARTY.md` exists, carries the full Apache-2.0 text for
             `librqbit`, and CI fails when a dependency's licence is not on the
             accept list.

**Done.** `THIRD_PARTY.md` is generated from `Cargo.lock` by `cargo about`
against `about.toml` and `about.hbs`, covers **310 crates**, and carries the
Apache-2.0 text attributed to `librqbit 9.0.0` along with every other licence
that ships. It opens by naming the two dependencies that need saying out loud:
`librqbit`, Apache-2.0 only rather than dual licensed, and `intermodal`, CC0
and credited anyway.

```bash
cargo about generate --config about.toml --output-file THIRD_PARTY.md about.hbs
```

The `notices` job in `ci.yml` runs that on every push and does two things with
it. Generation itself is the gate: `cargo about` refuses a licence that is not
in `about.toml`'s `accepted` list, which is the same list `deny.toml` allows.
And the crates the regenerated file covers are compared against the committed
one, so a dependency added without regenerating fails there.

**The comparison is the set of crates, not the bytes, and that is deliberate.**
Regenerating here today produces a file 835 lines longer than the committed
one, with `librqbit`'s Apache-2.0 text repeated in four blocks instead of
grouped into one. The crate sets are identical, all 310 of them, so nothing is
missing and nothing is extra: what changed is how `cargo about` groups
identical texts. A byte-exact check would fail on a `cargo about` release and
make the notice file something that has to be regenerated to make CI green,
which is how a notice file stops being read.

**One thing found while establishing that**, worth knowing before anyone
regenerates and sees a diff. `librqbit` ships no licence file at its crate
root, so `cargo about` falls back to scanning the crate directory, and that
directory holds a `webui/` tree. `librqbit`'s build script runs `npm install`
in it under the `webui` feature, which `bit-cli` does not enable but something
else on a shared machine may have: this one has **147 licence files under
`webui/node_modules`** for the scan to find. Moving them aside changes nothing
about the drift above, so they are not its cause, but they are exactly the
kind of thing that makes a local regeneration disagree with a clean one. The
`notices` job builds nothing before it generates for that reason.

### T-121 No cargo-deny configuration

Source:      the operator's brief
Category:    licensing
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `cargo deny check` is required in CI for licences, advisories,
             bans, and sources. There is no `deny.toml`.
Relevance:   It is the mechanism that stops a transitive AGPL dependency
             arriving unnoticed, which for this project is a licence incident
             rather than a lint.
Approach:    Allow MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode,
             CC0-1.0, and Zlib. Deny everything else, including every GPL and
             AGPL variant. Every exception carries a comment saying why.
Acceptance:  `cargo deny check` passes, and adding a GPL dependency makes it
             fail.

**Done, and both halves of the acceptance are now a command rather than an
assertion.** `deny.toml` allows fourteen permissive licences and denies
everything else. Two of them carry their reason inline:
`CDLA-Permissive-2.0`, which covers the Mozilla CA bundle and is a data
licence rather than a code one, and `Apache-2.0`, whose obligation is on the
binary distribution and is met by `THIRD_PARTY.md`.

```
$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
```

The second half is the one that needed building. A configuration that passes
says the tree is clean today; it does not say the gate would catch a copyleft
dependency arriving tomorrow. `scripts/check-licence-gate.ps1` proves it does:

```
$ pwsh -NoProfile -File scripts/check-licence-gate.ps1
checking this repository against its own deny.toml
  passes
checking a tree with one GPL-3.0-or-later dependency
  rejected: license is not explicitly allowed

verdict: the tree is clean and a GPL dependency is refused
```

The probe is a throwaway crate depending on a local crate whose manifest
declares `GPL-3.0-or-later` and whose `lib.rs` is empty, checked against this
repository's own `deny.toml`. Nothing is downloaded and no network is touched:
a licence is a string in a manifest, and that string is what `cargo deny`
reads. The check requires the failure to name the licence, so a gate that
failed for some other reason does not pass for the right one by accident.

The `deny` job in `ci.yml` runs `cargo deny check licenses advisories bans
sources` through `EmbarkStudios/cargo-deny-action` on every push.

