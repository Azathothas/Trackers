# gate.md

What a unit of work passes before it is done. Three parts, none skippable,
with the commands actually run and the output actually read.

⭐ **Each part is blind to the class the other two catch.** The checks prove the
tree. The driven pass proves the product against reality. The reviews prove the
composition. Skip one and you ship the class it owns.

RULES 10.3 is what ending a *session* requires, which is a larger list that
contains this one.

---

## (a) The checks, in one command

```bash
python3 scripts/check-gate.py
```

⭐ **Run it with one command, not from memory.** This part is a list, and a
list run by hand is run in the order somebody recalls it. The check that gets
forgotten is the one added last, which is also the one nobody has seen fail.

`--fast` drops the test suite and the offline generation for an
edit-and-recheck loop. ⛔ It is never a verdict on whether the tree is green.

`--strict` turns a skip into a failure, which is what CI passes: there the
environment is built on purpose and a skip means the build broke.

What it runs, and what each owns, is
[`../../scripts/README.md`](../../scripts/README.md).

Three disciplines that catch what a pass count hides:

- ⛔ **A skipped check is reported as a skip, never as a pass.** A check that
  could not run means nothing about its subject was verified, which is the
  opposite of a pass.
- ⛔ **Zero passes is red whatever the skips say.** A runner that found nothing
  to run and printed green over nothing is the forbidden pattern about a step
  that exits 0 having done nothing.
- ⛔ **Read every exit code from the process that produced it, unpiped.**
  [`../conventions/shell.md`](../conventions/shell.md) section 2.

⛔ **Grep yourself against
[`../conventions/forbidden-patterns.md`](../conventions/forbidden-patterns.md)
before declaring green.**

⚠ **One check exits 2 on purpose.** `check-vantage-metadata.py` cannot run
until health records exist, and returning 0 would report "every record carries
its vantage" while checking nothing. That is a correct answer, the gate reports
it as an expected skip, and `--strict` does not fail on it. ⛔ **The moment P2
lands and it starts exiting 0, the `expect_skip` flag comes off.**

---

## (b) Drive the real thing

⛔ **For every change to behaviour, run the actual thing and look at what it
produced.** A green suite proves the code and nothing else.

For this project that means, depending on what changed:

| what changed | what to drive |
| --- | --- |
| the pipeline | `python3 scripts/generate.py --offline --out DIR`, then read `DIR/trackers_all.txt` and `DIR/report.md`. ⭐ The 2026-08-31 finding that seven private-tracker credentials reach the published output came from reading that file, not from the suite |
| the probe | the oracle in [`../../tests/fake_tracker.py`](../../tests/fake_tracker.py), which can be made to truncate, stall and refuse on demand |
| an instrument | run it, and read its conditions block. A result whose conditions say `unclassified-host` was not taken where you thought |
| a workflow | ⛔ push it and look at the run. A workflow edit is the change whose effect is visible only in a run |

⚠ **The one-gated-door class is invisible to a green suite.** A control
enforced on one path and not its siblings, or a rule the pipeline applies at
one stage and not the next, is caught by driving the real thing and by nothing
else.

⛔ **Deferring the pass to the operator is a failed gate, not a deferral.**
There is one narrow exception: where this environment genuinely cannot reach
the thing, say so, name what is needed, and record it. Reporting a pass that
did not happen is a failed gate wearing a green badge.

---

## (c) The deep reviews

At least three, each asking a **different** question.
[`reviews.md`](reviews.md) is the specification.

⭐ **A pass reporting nothing was too shallow.** Where one genuinely found
nothing, the write-up says what would have had to be true for it to fire.

Reviews are committed under
[`../../HISTORY/reviews/`](../../HISTORY/reviews/), one file each, and RULES
10.3 step 4 requires three before a session may end.

---

## Local is not CI, and CI is not the world

⚠ **A gate on your disk and the same gate on a clone answer different
questions.** This project has paid for that gap twice on the same day: a linked
empty directory existed on the author's disk and in no checkout anywhere, and
`references/` held 994 files locally and 883 in every clone.

⭐ **The instruction is to clone your own output before believing it
reproduces.** RULES 10.3 step 9 has the command.

⚠ **And CI is not the world.** Every measurement here comes from one cloud
provider's address space. "Dead from a GitHub runner" is not "dead", which is
RULES 3.1 and is the reason `unmeasurable` exists as a state.

---

## What "done" means

All three parts pass, with the commands actually run and the output actually
read, never checked off from memory. Then:

- the entry is updated **in the same change as the work**, never after it
  (RULES 7);
- the documentation describing the changed behaviour changes with it;
- what the reviews surfaced is **fixed**, not only listed. A finding that was
  not fixed says where it is now tracked.

⛔ A unit of work whose scope grew during implementation re-passes the gate
against its **new** scope.
