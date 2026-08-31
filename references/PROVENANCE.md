# references/ -- the tracked corpus

Every reference this project's conclusions rest on, at the commit it was read
at. Tracked **in the tree** rather than in scratch, because
`Azathothas/TEMPLATE`'s `docs/methodology/references.md` records that failure
twice: *"an untracked corpus exists on one machine, and every claim built on it
becomes unsourced the moment that machine is not the one asking."*

**The test this directory has to pass, in one sentence:** could somebody who
distrusts the write-up re-run every load-bearing claim without asking anyone?

The write-up is [`HISTORY/reference-sweep.md`](../HISTORY/reference-sweep.md);
the per-reference findings are in
[`HISTORY/references/`](../HISTORY/references/).

## Layout

```
references/<owner>__<repo>/
    COMMIT          the commit the tree was captured at
    tree/           the repository, .git stripped AFTER the commit was recorded
    issues.json     issues AND pull requests, both states, up to 100 items
    comments/<n>.json   the comment thread for issue <n>, where it has any
```

Four loose `i_<n>.json` / `c_<n>.json` files sat directly in `references/`
until 2026-08-31, outside the documented layout. The issue bodies duplicated
`issues.json`; the comment threads are now under `comments/`. Both sets were
removed.

`.git` was removed only after `git rev-parse HEAD` was recorded: once the git
directory is gone the commit is unrecoverable and every line citation becomes
unverifiable. The trees were trimmed by **deleting**, never by moving, so every
path cited in the write-up still resolves.

## Contents

| directory | commit | captured | licence | determined from |
| --- | --- | --- | --- | --- |
| `Aseem0xff__pacman-static` | `38f7e3e` | 2026-08-31 | 0BSD | `tree/LICENSE` |
| `AvalynSouvlaki__T-244-RESEARCH` | `88a8410` | 2026-08-31 | Unlicense | `tree/LICENSE` |
| `Azathothas__TEMPLATE` | `6206166` | 2026-08-31 | 0BSD | `tree/LICENSE` |
| `Azathothas__bit-cli` | `cce8131` | 2026-08-31 | MIT | `tree/LICENSE` |
| `CorralPeltzer__newTrackon` | `7da7dde` | 2026-08-29 | MIT | `tree/LICENSE.txt` |
| `DeSireFire__animeTrackerList` | `e59508b` | 2026-08-29 | GPL-3.0 | `tree/LICENSE` |
| `GerryFerdinandus__bittorrent-tracker-editor` | `c5f5b82` | 2026-08-29 | MIT | **`tree/README.md` line 122 -- there is no licence file in that repository** |
| `XIU2__TrackersListCollection` | `d169e6e` | 2026-08-31 | GPL-3.0 | `tree/LICENSE` |
| `ngosang__trackerslist` | `562bdc0` | 2026-08-31 | GPL-2.0 | `tree/LICENSE` |
| `pkgforge-security__Trackers` | `7f2d00b` | 2026-08-29 | Unlicense | `tree/LICENSE` |

**Licences were read from the file on disk, every time.** Where a repository
carries no licence file the row says which weaker source the determination came
from, because a README statement is a real declaration and it is weaker than a
file.

### What the licences mean for this project

Three references are copyleft -- `ngosang/trackerslist` (GPL-2.0),
`XIU2/TrackersListCollection` and `DeSireFire/animeTrackerList` (GPL-3.0).
**Nothing is copied from any of them.** This project consumes their published
*tracker URLs*, which are facts about third-party servers rather than
copyrightable expression, and reads their code only to check what it does.
Where a mechanism is adopted, the implementation here is written independently
from the observed behaviour and cites theirs at `path:line` with its commit.
This repository is 0BSD (`LICENSE`), matching `Azathothas/TEMPLATE`, which was
checked rather than assumed (`C-42`).

## Re-mining, 2026-08-31

Three trees had moved since the 2026-08-29 capture and were re-cloned at their
new HEADs, per the methodology's *"re-mine a reference even if it has been
swept before"*:

| reference | was | now | what changed |
| --- | --- | --- | --- |
| `Azathothas/TEMPLATE` | `6eaf4b5` | `6206166` | `docs/methodology/references.md` gained one paragraph (a template's own corpus exemption); `experiments.md`, `work-todo.md` and `choosing-a-work-model.md` are **byte-identical**, so nothing this project's rules rest on moved |
| `ngosang/trackerslist` | `1e61597` | `562bdc0` | daily regeneration of the output lists and the README's count. No structural change: still no generator, still 16 tracked files |
| `XIU2/TrackersListCollection` | `e9f9ba2` | `d169e6e` | daily regeneration. Workflow unchanged |

The four unchanged trees (`newTrackon`, `animeTrackerList`,
`bittorrent-tracker-editor`, `pkgforge-security/Trackers`) were confirmed
identical by `git ls-remote` and not re-cloned.

Three references were **added**: `Azathothas/bit-cli` (a real BitTorrent client
whose tracker implementation is the closest thing to an oracle for this
project's protocol decisions), and `Aseem0xff/pacman-static` and
`AvalynSouvlaki/T-244-RESEARCH`, which had been named as the documentation
standard since the beginning and never fetched (T-011).

## Route used

* **Trees:** `git clone --depth 1` over HTTPS, direct.
* **Trackers:** the credential-free public proxy `api.gh.pkgforge.dev`
  (RULES 16). **Reads only.** No write verb was issued against any third-party
  repository, and no issue or comment was created anywhere.
* **Comments:** `python3 scripts/fetch-reference-comments.py`, one request at a
  time with a 2 s spacing and exponential backoff on a rate limit.

## What was trimmed, and why

Stated because a trim that is not recorded is indistinguishable from a gap.

* **Binary and image files** -- 25 across the corpus, mostly `XIU2`'s
  screenshots. The methodology's trim list names images explicitly.
* **`Aseem0xff/pacman-static`'s own `references/` tree** (15 MB). It is that
  project's evidence for *its* claims, not source this project cites. Its
  methodology documents, patches and experiments are kept in full.
* **`Azathothas/bit-cli`'s `vendor/`, `man/`, `patches/scan/` and lock files** --
  a vendored dependency tree, generated manpages, and upstream scan dumps.
  `crates/`, `docs/`, `scripts/`, `fingerprints/`, `bench/`, `TODO/` and every
  licence file are kept.
* **Every `.gitignore` inside a captured tree** -- ten of them, one per
  reference plus three nested. An upstream `.gitignore` is written for
  upstream's build and has no authority over what this project keeps as
  evidence, but git honours it anyway: two of the ten were dropping **111
  files** from every clone, including all 91 of the `bench/` results named in
  the line above. See *What was missing from every clone* below.
* **`GerryFerdinandus/bittorrent-tracker-editor`'s `.gitmodules`** -- it
  declares one submodule, `submodule/dcpcrypt` (a Pascal crypto library, from
  SourceForge). `git clone --depth 1` does not recurse, so the submodule's
  content was never captured and the directory does not exist here. The file
  is removed rather than left to promise content the capture does not have.
  Nothing this project cites is inside it: the `C-40`/`C-41` evidence is
  `tree/source/code/torrent_miscellaneous.pas`, which is captured in full.
* **Three agent instruction files** -- `Azathothas__TEMPLATE/tree/AGENTS.md`,
  `Azathothas__TEMPLATE/tree/docs/templates/AGENTS.md` and
  `Azathothas__bit-cli/tree/docs/AGENTS.md`. A file with that name anywhere
  under a repository is read as instructions by the tools working in it, so
  keeping one puts a third party's instructions inside this project. They are
  data about somebody else's process and nothing here cites them.
  `docs/methodology/vendoring.md`, *What is never vendored*. Every other
  document in those two trees is kept, including the methodology files
  `TODO/RULES.md` rests on.
* **Three directories that trimming emptied** -- `bit-cli/tree/.codegraph`,
  `XIU2/tree/img`, `newTrackon/tree/data`. Git cannot track an empty
  directory, so a husk left behind exists on the capturing machine and in no
  clone.

Nothing was trimmed by moving. Every trim was a delete.

## What was missing from every clone, until 2026-08-31

Recorded here rather than only in `HISTORY/corrections.md` because this is the
file an auditor reads before trusting the corpus, and for a week it described a
corpus **larger than the one anybody else could obtain**.

`find references -type f` on the capturing machine returned 994. `git ls-files
references` returned 883. The 111-file difference was invisible in `git status`,
because ignored files are ignored quietly. Two independent causes:

| files | dropped by | what was lost |
| --- | --- | --- |
| 91 | `references/Azathothas__bit-cli/tree/.gitignore` (removed) lines 44-45 -- the captured tree's own rules, `/bench/*.json` and `/bench/*.csv` | **all** of `bit-cli`'s bench results, which the trim list above claimed were kept, among them the announce and interop-magnet timings |
| 20 | this repository's `.gitignore`, which said `out/` rather than `/out/` -- an unanchored directory pattern matches at any depth | `references/Aseem0xff__pacman-static/tree/experiments/out`, that project's committed instrument output -- the evidence behind the documentation standard this project adopted |

No load-bearing citation resolved into a dropped file, so nothing published was
wrong. That is luck, not a control: the two directories are exactly where a
future agent would look for a prior measurement of a tracker announce.

Fixed in three places: the ten `.gitignore` files are trimmed (above), this
repository's own rules are anchored to the repository root, and
`scripts/check-corpus-integrity.py` is a gate -- disk against index, plus
`git check-ignore --no-index` over every tracked corpus path, which is the form
that still fails in a fresh CI checkout where nothing is missing yet.

## What could NOT be obtained

Stated because a silently skipped source is the failure the whole procedure
exists to prevent.

* **Discussions -- none, for any reference.** They are GraphQL-only and the
  credential-free route is REST. Where a maintainer kept a design argument in
  Discussions, this corpus does not contain it. **This is a real gap.**
* **Review comments -- none, for any reference.** The methodology calls these
  "the densest technical content a project produces". Not fetched: they need a
  separate endpoint per pull request, and the pull requests in this corpus are
  overwhelmingly dependency bumps. Recorded as a gap rather than argued away.
* **Tracker items are capped at 100 per repository.** `ngosang/trackerslist`
  and `CorralPeltzer/newTrackon` both returned exactly 100, so **both are
  truncated** and older items were not seen.
* **Four trackers were not fetched at all**, so four of the ten references have
  no `issues.json`: `Azathothas/bit-cli`, `Azathothas/TEMPLATE`,
  `Aseem0xff/pacman-static` and `AvalynSouvlaki/T-244-RESEARCH`. All four carry
  their engineering arguments in-repo, under `TODO/`, `docs/` or `RESEARCH.md`,
  which is why the tree was the priority -- but the methodology is explicit that
  a tracker holds what a repository does not, so **this is a gap and not a
  judgement that there was nothing there.** The three that were mined for
  method rather than for mechanism (`TEMPLATE`, `pacman-static`,
  `T-244-RESEARCH`) are the least costly; `bit-cli` is the one most likely to
  be hiding something, because it is the only reference that speaks the tracker
  protocols.
* **No generator exists to read for `ngosang/trackerslist`** -- the repository
  publishes outputs only. That absence is a finding, not a gap in fetching.

**Issue comments are almost no longer a gap.** **216 threads carrying 501
comments** are captured under `comments/`, against **four threads** before
2026-08-31. That is every issue with a non-zero comment count except six.

### The six that could not be fetched, and why it matters

`GerryFerdinandus/bittorrent-tracker-editor` issues **#1 through #6** report 1,
5, 3, 3, 4 and 1 comments respectively, and **every route tried returns a
syntactically valid empty array** -- the literal bytes `[\n\n]`. Routes tried:

1. `api.gh.pkgforge.dev`, with and without `per_page`, with and without `page`;
2. `api.rv.pkgforge.dev` wrapping `api.github.com` -- same empty array;
3. `api.github.com` directly -- refused by this session's egress policy, not by
   GitHub.

**Inferred cause, not established.** `Aseem0xff/pacman-static`'s
`docs/patches/mine-repo-page-join.md` documents this exact signature in
`Azathothas/TEMPLATE`'s `scripts/common/mine-repo.sh`: a paginated joiner that
recovers array bounds by counting `[` and `]` over concatenated raw text counts
the brackets inside string values too, and comment bodies are markdown, so a
thread whose comments contain unbalanced brackets joins to `[]` while the
fetch reports success. Six threads out of 222, all in one repository, all
old, is the shape that predicts. It is **not confirmed**, because confirming it
needs a route that returns the bodies and no such route is available here.

**What is confirmed is that it happened**, and that is the operational point:
`scripts/fetch-reference-comments.py` **refuses** an empty array when the
issue's own `comments` count is non-zero, so these six are recorded as a gap
rather than written into the corpus as "this thread is empty". Without that
guard the sweep would carry six silently empty threads and no way to tell them
from real ones.

One capture *is* legitimately empty and was checked rather than assumed:
`CorralPeltzer/newTrackon` #353 reports `comments: 0` and a live re-fetch
returns `[]`. A real zero, not a failed fetch.

## Refreshing

Re-mine rather than trusting these commits: projects move, and a previous
verdict was taken against a tree that has since changed.

```sh
OWNER=someone REPO=something
git clone --depth 1 "https://github.com/$OWNER/$REPO" ".tmp/$REPO"
git -C ".tmp/$REPO" rev-parse HEAD            # record BEFORE stripping .git
python3 scripts/fetch-reference-comments.py --reference "${OWNER}__${REPO}"
```

A refreshed capture gets a **new** commit recorded here and a dated note in
[`HISTORY/reference-sweep.md`](../HISTORY/reference-sweep.md); it does not
overwrite a previous finding's provenance, because a citation must keep meaning
what it meant.
