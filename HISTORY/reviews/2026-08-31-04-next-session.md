# Review 4 -- the session that has to do the work

**Date:** 2026-08-31, **Standpoint:** the next session, oriented, holding
`PROGRESS.md`'s work order, trying to actually close item 0 and then item 1.

**What I looked for:** the point at which "start here" stops being executable
and becomes a research problem of its own. Not whether the plan is right;
whether it can be *carried out* by someone with no more context than the tree.

---

## Method

1. **Walk the work order.** For each of the eight numbered entries, open it and
   ask: could I start this now, and would I know when I had finished?
2. **Parse every acceptance.** Mechanically, across all entries: does the
   `Prove:` field contain something executable?
3. **Follow one entry end to end.** T-012, the P0 item that is first and gates
   everything after it.

---

## What it found

### 1. 46 of 63 acceptances could not be run as written -- **partly fixed, rest filed**

The work model this project adopted requires an entry's `Prove:` to be **the
acceptance, which is a command**. `bit-cli`'s mining guide puts the cost in one
line: *"a 'prove' with no command is a paragraph."*

Measured by parsing every entry for a backticked command in its `Prove:` field:

| | |
| --- | --- |
| entries | 63 |
| `Prove:` containing an executable command | 17 |
| `Prove:` that is prose | **46** |

Four of the eight entries in the *current work order* were among them -- T-022,
T-027, T-028 and T-063 -- so the next session would have hit this immediately,
on the items it was told to start with.

**What "not runnable" costs concretely.** T-028's acceptance read *"a
cross-check report whose header states the methodology difference and whose
every rate carries a sample count."* A session can satisfy that by writing a
markdown file. Nothing distinguishes having done the cross-check from having
described one.

**Fixed for the six in the work order**, by hand, each naming a command and
marking `(planned)` where it names something unbuilt. **The remaining 40 are
filed as T-123**, with the decision *not* to bulk-rewrite them by pattern
recorded explicitly: inventing a specific command for unbuilt work manufactures
precision the entry does not have, which is the defect behind corrections 15
and 17 in this same session. Each gets its command when a session next touches
it, and T-123's own acceptance is the gate that makes "next touches it" mean
something.

### 2. T-012 is startable, and it is the one that was checked hardest

It is first, it is P0, and it contaminates everything after it, so it got the
end-to-end read:

* **The instrument's hard part exists.** The four arms are a `ProbeConfig`
  difference and nothing else, asserted by
  `tests.test_probe.ProbeConfiguration` -- including that the arms reach the
  wire as genuinely different requests, so two cannot silently collapse.
* **The positive control exists.** `tests/fake_tracker.py`'s
  `BLOCK_UNKNOWN_UA` lets the experiment prove it can detect a block in a case
  where one is known to exist, before any conclusion is drawn from real
  trackers. Without it, "no arm was blocked" and "the detector is broken" are
  the same output.
* **Two conditions are stated and both are checkable**: it cannot run from a
  proxied vantage (`C-62`), and the arms must not fire back to back at one
  host.
* **It now has two axes** (`C-63`), and a prior from the corpus (`C-68`).

**One gap, and it is in the entry's favour**: the entry says the instrument
must "refuse to emit subject results from a proxied vantage unless explicitly
overridden". Nothing enforces that yet, because the instrument does not exist --
but `Vantage` already records `environment_class`, so the check is a
two-line assertion and the entry should say so. It now does not; that is left
for the session that writes experiment 26, which is the right place.

---

## What passed

* **The reading order terminates and each file says what it is for.**
* **Every entry in the work order is `open`** -- none is silently done or
  blocked.
* **The ordering argument is in `INDEX.md`**, separate from the order itself in
  `PROGRESS.md`, so a session can disagree with the order without losing the
  reasoning.
* **The leverage entry is called out and deliberately not numbered.** T-031 is
  described as what to reach for when the ordered work stalls, which is the
  right shape for it: numbering it would make it look sequential.
* **Nothing waits on a human.** The open-questions section is empty and the
  four historical questions are recorded as settled with their decisions.

## What I looked for and did not find

* **An entry whose premise is contradicted by the code.** I spot-checked the
  six in the work order against `src/`; each premise matches what is there.
* **An entry that cannot be started without another that is not named.** The
  dependencies that exist are stated (T-027 needs T-024; T-064 needs T-003).
* **A work-order item that is already done.** None.
* **An instruction to defer.** RULES 10 forbids it and no entry contains one.

## What this review did NOT establish

* **That the work order is the best order.** I checked executability, not
  priority. The argument for the ordering is in `INDEX.md` and I did not
  challenge it.
* **That the 17 runnable acceptances would actually pass if run.** Most name
  tests for code that does not exist yet.
* **That T-012 will produce a usable answer.** It is startable. Whether four
  arms across the HTTP corpus yields a signal is exactly what it is for, and a
  null result would be a real result.
