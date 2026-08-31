# HISTORY

**What was believed here, what is known, and why either changed.**

⛔ **Nothing here is read to do work.** [`../TODO/PROGRESS.md`](../TODO/PROGRESS.md)
carries the work order and is the only file that does.
[`../TODO/RULES.md`](../TODO/RULES.md) is normative. A session that opens this
directory is reading what was true once, with two exceptions noted below.

This directory exists so that none of the pages that answer questions fill up
with narrative. The instinct to record why a design has its shape is right,
which is why forbidding it does not work: it needed a destination.
[`../docs/conventions/prose.md`](../docs/conventions/prose.md) is the rule that
sends things here.

---

## What is here

| file | what it holds | ⭐ read it when |
| --- | --- | --- |
| [`claims.md`](claims.md) | every `C-nn` factual claim, its status, the experiment that settled it, and the consequence if it is false | before relying on any fact about the platform, the protocols or the upstreams. **Grep it; a full read is not needed** |
| ⭐ [`corpus-baseline.md`](corpus-baseline.md) | every corpus figure, with the command behind each | ⛔ **before quoting any corpus number.** It is the only file that states them, and it exists because three contradictory sets were once in circulation and none came from an instrument |
| [`decisions.md`](decisions.md) | every `D-n` decision, its rationale, and the alternatives it rejected | before re-opening a settled question |
| [`corrections.md`](corrections.md) | every claim this project published and later withdrew | ⭐ before trusting a sentence in any document here |
| [`gates.md`](gates.md) | the definition of done for each phase, and which gates are still open | to find out whether the project has earned its next step |
| [`idea-coverage.md`](idea-coverage.md) | where every section of the retired design brief went | only if you are asked about a requirement no entry seems to own |
| [`reference-sweep.md`](reference-sweep.md) | what reading ten upstream projects established | before designing something one of them has already tried |
| [`references/`](references/) | one verdict per reference: adopt, confirm, decline or archaeology | when a specific upstream is in question |
| [`reviews/`](reviews/) | what each deep review swept, what it found, and what it did not look at | before repeating a pass somebody has already run |

⚠ **Two of these are consulted rather than historical**, and that is
deliberate: `claims.md` and `corpus-baseline.md` hold live facts, and the
alternative was a second home for them somewhere else.

---

## ⭐ The first thing to read: what has been withdrawn

[`corrections.md`](corrections.md) is the list of claims this project made and
later took back. A reader who trusts a document without checking that list
trusts sentences that are wrong.

⚠ **Notice what it costs to catch one.** Most were found by a review or by a
later measurement. At least one needed the same instrument run a second time,
which is the argument for repeating a diagnosis before publishing it.

---

## The rules on this directory

⛔ **Append, never edit.** A premise a later measurement disproves **keeps its
wording**, and the correction is written underneath it. This is not politeness
about the past: a silently corrected document teaches nobody, and the reader
who needs this directory is the one about to make the same mistake.

⛔ **Moved, not summarised.** A superseded passage arrives here in its original
words. A summary of a retired explanation is a new document about an old one,
and it loses the detail that made it worth keeping.

⛔ **It is not the work order**, and a page here that starts being consulted for
"what next" is a second work order going stale.

⚠ **It is exempt from one rule and one only.** One fact, one home does not
apply here, because a superseded page states things the live pages now state
differently, which is the point.
[`../scripts/check-one-home.py`](../scripts/check-one-home.py) carries the
exemption in the check rather than in a comment nobody reads. The character
allowlist, the density ceiling and the banned vocabulary all apply here as
everywhere.

---

## ⭐ The test, for a passage you are not sure about

**Does a reader need this to use the thing correctly today?**

- **Yes**: it is a constraint and it stays on the page that answers that
  question. "Every measurement comes from one cloud provider's address space"
  is a constraint.
- **No**: it is history and it comes here. "The scrape-URL derivation used to
  corrupt a path" is history.
