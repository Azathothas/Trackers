# `Aseem0xff/pacman-static` -- adopt (methodology)

**Commit:** `38f7e3e`, **Licence:** 0BSD (`tree/LICENSE`),
**Captured:** 2026-08-31, **Corpus:**
[`references/Aseem0xff__pacman-static/`](../../references/Aseem0xff__pacman-static/)

Named as this project's documentation quality bar since the first draft, and
never opened until now (T-011, `C-29`). Its subject -- a statically linked
`pacman` for five architectures -- has nothing to do with trackers. **Its
research document does.**

## What this reading did NOT establish

* **Nothing was built or run.** The claims below are its documents and scripts
  read at `38f7e3e`; no `pacman-static` binary exists here.
* **Its tracker was not read.** Its own corpus was stripped from our capture
  (15 MB of somebody else's evidence), so its citations into that corpus cannot
  be followed from here without re-fetching what it fetched.
* **Its measured numbers are about `pacman`.** None is imported.
* **Passes taken: two.** WHAT and MECHANISM. Pass three -- how it handles the
  thing *we* find hard -- is genuinely narrow, because the only overlap is
  method, and that is what this entry is about.

## Verdict: **adopt**, three mechanisms -- and one is a defect we already hit

### 1. `git rev-parse` in a stripped corpus lies confidently

Its correction #8:

> `30-reference-defects.sh` printed each reference's commit. ⛔ **It printed
> *this* repository's.** Once the corpus trees lost their `.git` directories,
> `git -C <corpus> rev-parse HEAD` did not fail -- it walked **up** and answered
> with the enclosing repository's HEAD. A provenance line that is confidently
> wrong is worse than a missing one.

**This project hit the identical defect on 2026-08-31, in this session.** While
re-mining the three references whose upstream HEAD had moved, a
`git -C .tmp/remine/<name> rev-parse HEAD` run *after* stripping `.git`
returned `3db05c2` -- this repository's own HEAD -- three times, once per
reference. It was caught only because the real SHAs had been captured from the
clone output a step earlier and did not match.

**Consequence.** The corpus's ordering rule is not a style preference: capture
the commit **before** stripping `.git`, because the command that reads it
afterwards does not fail, it answers wrongly. `references/PROVENANCE.md` states
the ordering; this is the evidence for why it is stated in that order.

### 2. The write-up opens with what it got wrong about *itself*

Not "what it did not establish" as a caveat -- a numbered table of **nine
corrected claims**, each with what was claimed and what measurement said, above
the recommendation. Its own summary of them is the transferable part:

> **Claims 2, 4 and 8 are the same defect in three costumes: a tool answering
> confidently where it should have failed.** All three were mine, in
> instruments written to prevent exactly that.

**Consequence.** `HISTORY/reference-sweep.md` already opens with what it did not
establish. What it did not carry is the sharper form: a table of claims *this
project* got wrong, with the correction beside each. `HISTORY/corrections.md`
is that table and the two are now cross-linked.

### 3. Clone your own output before believing it reproduces

Its correction #9: everything passed on the machine that wrote it, and a fresh
clone failed at `meson setup`, because `git add -A` honours a `.gitignore` at
any depth and a vendored tree's own ignore file dropped a needed file from the
commit.

**Consequence.** RULES 10.3 step 9 says to confirm the cold start works. That
is currently interpreted as *re-read the documents*. It should mean **clone the
pushed head into a fresh directory and run the gates there** -- the only check
that catches a tested tree and a committed tree diverging. Filed on T-086.

## Confirms

* **An absence is not a zero, and a tool can manufacture one.** Its correction
  #2 -- an emulator reported missing because a script derived its name wrongly
-- is RULES 2's positive-control rule with a concrete price.
* **A negative result is a result.** Its own section 9, "the crash that is not closed", is
  committed with the control matrix rather than dropped.
* **The instrument is the deliverable.** Thirteen numbered `experiments/`
  scripts, including `40-mine-repo-joiner-defect.sh`, which exists purely to
  fail while an upstream defect is present and to pass once it is fixed.

## Filed elsewhere

`docs/patches/mine-repo-page-join.md`, read in full: `Azathothas/TEMPLATE`'s
`scripts/common/mine-repo.sh` recovers paginated API responses by counting `[`
and `]` over concatenated raw text, which counts brackets inside string values.
Comment bodies are markdown, so they carry brackets; measured on their fixture,
**38 bracket characters inside comment bodies, net imbalance +2, oracle 100
items, the joiner 0** -- while the enclosing function printed `comments: ok`.
Their fix took comments from 0 to 202.

**Does this project have the same defect?** No -- `scripts/fetch-reference-comments.py`
parses each response with `json.loads` and **refuses an empty array when the
issue's own `comments` count is non-zero**, which is their second change. The
one empty capture in our corpus, `CorralPeltzer/newTrackon` #353, was checked
against a live re-fetch and is a real zero. The guard exists because of this
document.

## Refused

Its subject matter, its architecture, and every one of its numbers. It is a
build-toolchain project and this is a network-measurement project; the overlap
is method and nothing else.
