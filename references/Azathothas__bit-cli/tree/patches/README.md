# Working on the vendored upstreams

`vendor/` holds three upstream repositories and thirteen crates that `bit-cli`
is built from. Why they are vendored at all is [`docs/vendoring.md`](../docs/vendoring.md).
This is how to work on them.

Three files and four scripts, and nothing else binds:

| | |
| --- | --- |
| [`vendor/upstream.json`](../vendor/upstream.json) | what is vendored, from where, at which commit |
| [`UPSTREAM.md`](UPSTREAM.md) | every change this repository has made, and why |
| [`TASKS.md`](TASKS.md) | the work the fork exists to do, in order |
| `scripts/vendor-sync.ps1` | put a tree in, or reconcile a new release onto it |
| `scripts/vendor-diff.ps1` | regenerate the patch series from the tree |
| `scripts/upstream-scan.ps1` | everything upstream has, ranked against our open entries |
| `scripts/vendor-status.ps1` | one screen: is the fork healthy, is a merge due |

## The model: the tree is the truth

The vendored tree is edited in place, like any other source in this
repository. `patches/<upstream>/*.patch` is **derived** from it and is never
applied to anything.

The alternative, a pristine tree plus patches applied by a setup step, was
considered and rejected: every edit then needs a refresh, a dirty tree is easy
to lose, and `rust-analyzer` reads the applied tree while the truth lives
somewhere else. Here there is nothing to forget, and `cargo build` on a fresh
clone builds what this machine builds.

What the derived series buys is the two things a working tree cannot say:

- **Review.** A change to somebody else's code, on its own, without the 389
  files around it.
- **Attribution.** Apache-2.0 asks a distributor to mark changed files as
  changed. The series and `UPSTREAM.md` are that mark.

So: after changing a vendored file, regenerate.

```bash
pwsh -NoProfile -File scripts/vendor-diff.ps1
```

```bash
pwsh -NoProfile -File scripts/vendor-diff.ps1 -Check
```

`-Check` fails when the series and the tree disagree, which is the state a
commit must never be in.

## Making a change

1. **Read the entry in `TODO/` first.** Every vendored change exists to unblock
   one, and the entry names the seam with a line number. A change with no entry
   behind it is a change nobody can review against anything.
2. **Edit the tree** under `vendor/`.
3. **Write it down in [`UPSTREAM.md`](UPSTREAM.md)**, before running anything.
   One section per change: what it is, which entry it unblocks, why it cannot
   be done outside the vendored tree, and whether it is meant to go upstream.
   The last one matters: a change shaped for upstream and a change shaped for
   this repository are different changes, and mixing them makes both harder.
4. **Regenerate the series** and **run the gates**. The `record` gate reads
   this directory: [`TASKS.md`](TASKS.md)'s table has to agree with the entry
   each row names, its totals have to agree with its rows, and every path
   cited from here, [`UPSTREAM.md`](UPSTREAM.md) or this file has to resolve,
   including into `vendor/`. See `TODO/RULES.md` section 5 under "The record is
   part of the change" for the session that paid for it.

```bash
pwsh -NoProfile -File scripts/gates.ps1
```

5. **Run upstream's own tests** when the change is in `librqbit` itself.
   `gates.ps1` runs `cargo test --workspace`, and the vendored crates are not
   workspace members, so their tests are not in it. CI does compile them under
   `-D warnings`, so a warning in them is still ours to patch: cargo caps lints
   for a registry dependency and does not cap them for a path dependency. The
   first entry in `UPSTREAM.md` is exactly that case.

```bash
cargo test --manifest-path vendor/rqbit/Cargo.toml --target-dir target/vendor-rqbit
```

   **`--target-dir` is not optional.** Without it cargo writes its build output
   to `vendor/rqbit/target`, which is 7.2 GB and 9,894 files inside a tree this
   repository treats as somebody else's source. Git ignores it, because
   upstream's own `.gitignore` has `/target`, so nothing catches it: the next
   `vendor-diff.ps1` walks and hashes all of it and looks hung. The scripts
   skip a top-level `target/` now, and the directory still should not be there.
   See [`TODO/cli-surface.md`](../TODO/cli-surface.md), T-197.

## Taking a new upstream release

```bash
pwsh -NoProfile -File scripts/vendor-status.ps1
```

That says whether anything is due at all, in a few seconds. Then read what is
in the release rather than merging blind:

```bash
pwsh -NoProfile -File scripts/upstream-scan.ps1
```

The scan fetches **everything** each upstream has: every release, every issue
and every pull request, open and closed, plus the commits since our base. Then
it ranks them, because six hundred items nobody reads is the same as no scan.
The vocabulary is the nouns in the titles of entries that are still open,
partial or blocked, taken from `TODO/INDEX.md` so it cannot go stale, plus a
short curated list of protocol and type names a title never says. A high tier
means a person should look, not that anything is wrong; the JSON record under
`patches/scan/` keeps every item either way.

```bash
pwsh -NoProfile -File scripts/vendor-sync.ps1 -Upstream rqbit -Ref v9.1.0 -Check
```

`-Check` says what the merge would do and changes nothing: how many files
merge cleanly, how many upstream added or removed, and how many conflict.

```bash
pwsh -NoProfile -File scripts/vendor-sync.ps1 -Upstream rqbit -Ref v9.1.0
```

Without `-Check` it performs the three-way merge, using the base commit
recorded in `vendor/upstream.json` as the common ancestor. A file changed on
one side only is taken from that side. A file changed on both is merged by
`git merge-file`, the same three-way merge git itself runs, and a conflict is
left in place with markers.

### Upstream is not automatically right

**Reconcile by reading, never by preferring.** A new release is a proposal, not
an authority. Before taking any hunk that touches something this repository has
already changed, answer three questions and write the answer into
`patches/UPSTREAM.md`:

1. **Does upstream's version actually fix the thing?** A change that moves a
   defect somewhere else, or that fixes one shape of it, is not a fix. Check it
   against the entry's own acceptance, which is a command that already exists.
2. **Is it complete, and does it regress anything?** A feature that lands half
   done is worse than a seam we already patched around, because the next
   reconciliation has to carry both.
3. **Have we already done it better?** If our version is more correct, faster,
   or bounded where theirs is not, **keep ours** and record why in the
   patch's section. The patch does not go away because upstream touched the
   same lines.

Take upstream's when it properly fixes something or completes a feature with no
regression. Otherwise keep ours, and say which in the section, so the next
reconciliation does not re-litigate it from scratch. A merge that took
upstream's hunk because it was upstream's is the failure this paragraph exists
to prevent.

The one case that is not a judgement: a patch upstream **accepted** should be
deleted here at the release that carries it, because then it is the same change
arriving from the other direction rather than a competing one.

**The base is not advanced while anything conflicts.** Resolve the markers, run
the gates, then run the same command again to record the new base. That is
deliberate: a recorded base that does not describe the tree makes the next
merge wrong in a way nothing detects.

Then regenerate the series, because every patch's header names the base commit
it is against, and update the changelog:

```bash
pwsh -NoProfile -File scripts/release.ps1 -Bump patch
```

## What the sync script refuses to do

- **Write over an existing tree under `-Init`.** That is the operation that
  silently loses a fork.
- **Advance the base while a file is in conflict.**
- **Finish while a vendored file is one this repository's own `.gitignore`
  would swallow.** `.vscode/` did exactly that on the first vendoring: the
  files land on disk, never reach a commit, and a fresh clone then builds a
  different tree from the one that was tested. Either exclude the path in
  `vendor/upstream.json` or un-ignore it.

## Nothing here is sent upstream

**Settled, and not to be relitigated.** [`TODO/RULES.md`](../TODO/RULES.md)
section 6 is where the decision lives and section 6a is the wider rule it sits
under: this repository is the only one an agent may write to, and no issue, pull
request, discussion or comment is opened anywhere else, under any framing.

These patches are for `bit-cli`. Upstream has no interest in the work and closes
what arrives from an agent unread, so an offer costs a maintainer's time and
gains this repository nothing. That the series is in the shape a pull request
wants is a property of `git format-patch`, not a reason.

`UPSTREAM.md` is therefore a record and not a queue. Its `Upstream:` field
answers the one question a reconciliation asks: **could a release retire this
patch on its own?** A defect upstream may fix independently is worth naming with
its issue number, so the next merge checks for it rather than carrying a patch
that has become a duplicate. A patch a release does carry is **deleted** here at
that release, not merged with itself.
