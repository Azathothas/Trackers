# reviews.md

The deep review pass, which is part (c) of [`gate.md`](gate.md) and step 4 of
RULES 10.3.

At least three passes, and ⛔ **they are three different questions, not one
sweep written up three times.** A single pass finds what you were already
looking for.

Each one is committed as a file under
[`../../HISTORY/reviews/`](../../HISTORY/reviews/), named
`YYYY-MM-DD-NN-<lens>.md`.

---

## The three lenses

### 1. The door sweep

**"What other door reaches this code?"**

Enumerate every affordance the change adds, then list every caller and every
surface that can reach it. Then ⭐ **grep for the ones you did not
enumerate.** The list written from memory has never been complete.

- Check the **guard**, not only the guarded code.
- Grep an abstraction's **callers** before believing it is load-bearing.
- A rule applied at one stage of the pipeline and not the next is this
  project's shape of the class. The 2026-08-31 credential finding is one: the
  normalizer knows announce paths carry passkeys, and nothing downstream acts
  on that.

⚠ The task list is never the enumeration. It has never once contained them all.

### 2. The guard mutation

**"Can my new guard actually fail?"**

⛔ **Plant the defect the guard exists to catch, and read the exit code
unpiped.** A guard that has never been seen to refuse is a guard nobody knows
works.

Two shapes to test for specifically:

- a test whose **name** claims more than it **checks**;
- a check that passes because a different code path happens to satisfy it.

⛔ **And prove the negative case too.** A guard that refuses everything is as
useless as one that refuses nothing, and it looks identical from a passing
mutation test.

⚠ **A scope rule needs a fixture, not a comparison.** A check whose scope
silently narrowed produces an identical number on a tree with nothing in the
dropped scope to exercise it.

### 3. The claim audit

**"Which sentence in what I am about to publish is not backed by an artefact I
can point at?"**

Re-read the record, the documents and the summary against the tree and the
data. This is the pass that catches:

- a number with the wrong denominator;
- a conclusion drawn from a single sample;
- a measurement quoted without its conditions;
- a file a summary claims was written that is not on disk;
- ⭐ **a figure whose instrument output is no longer in the tree.** That is how
  this project found its own baseline resting on workflow artefacts that no
  longer existed.

⛔ **A summary is a claim like any other.** "I wrote X, the tests pass" is real
only once git or the suite confirms it.

---

## More than three, when the change earns it

Two that pay often here:

- **"What did I measure but never verify?"** A number taken once and never
  checked against a second source.
- ⭐ **"What would a tracker operator make of this?"** The project probes other
  people's servers, and RULES 4 is absolute. A pass that reads the change from
  the far end of the socket is not covered by any of the three above.

---

## A pass with no findings

⭐ **A pass that reports nothing means that pass was too shallow.**

Three passes reporting nothing is a weaker result than one pass reporting a
real defect. Where a pass genuinely found nothing, the write-up says **what
would have had to be true for it to fire.** That sentence is the evidence the
pass happened at all.

---

## The mechanical half is a script's job

Anything a check can assert should be asserted by a check, not by a reading:
statuses that disagree between two files, counts that no longer add up, a
citation naming nothing, a dead link, a line number that does not resolve.

⭐ Doing the mechanical half in one second is what leaves time for the half
that needs reading. `check-citations.py` found 374 broken citations on its first
run over a tree several reviews had already read.

⛔ **What no check can answer is whether a claim is true.** That is lens 3.

---

## What a review owes

- The findings, per pass, with which lens found each.
- What was **fixed** as a result, and where anything not fixed is now tracked.
- For any pass with no findings, what would have made it fire.
- ⛔ What the pass did **not** look at. A review that does not bound itself
  invites the next reader to assume it covered everything.
