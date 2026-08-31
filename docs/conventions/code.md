# code.md

How code is constructed here.
[`forbidden-patterns.md`](forbidden-patterns.md) is the table of what must
never appear; this is how to build so it does not.

The stack is fixed by decision D1 and RULES 12: **Python 3.11 or newer,
standard library only.**
[`../../scripts/check-no-third-party-imports.py`](../../scripts/check-no-third-party-imports.py)
is the gate, and it parses imports with `ast` rather than grepping, because a
grep for `import` matches prose about importing things.

---

## One read path, one write path

⭐ **Every consumer of a thing goes through the same code, and so does every
producer.** Where stream, parsing or IO logic is about to be copied into a
second place, extract it instead.

Copy-pasted logic becomes divergent copies, each acquiring different defects,
and a fix in one never reaches the others. Applied to authorization it is the
same rule: one gate per action, because a control enforced on one path into an
operation and not on its siblings is the most recurring hole there is.

---

## Fail loud, never silently corrupt

⛔ **The worst outcome is not a crash. It is quietly producing wrong data, or
destroying good data.**

- **Validate the shape before trusting it.** Upstream data is hostile input
  (RULES 5.1), and everything this project reads came from somebody else.
- **A guard that detects a mismatch errors the operation.** Never pad, never
  guess, never overwrite. A BEP 15 connect response shorter than 16 bytes is a
  refusal, not a short read to pad out.
- ⛔ **A failed fetch is `None`, never an empty list.** RULES 3.2, and it is the
  single defect both pieces of prior art here share.

A loud failure is a defect report. A silent corruption is an incident somebody
finds weeks later, by which time the good data is gone.

---

## Assume the worst case per feature

Where a fact is unknown and being wrong in one direction costs correctness
while the other only costs efficiency, take the pessimistic reading. You are
then right under both.

⭐ **In this project the asymmetry is usually about somebody else's server.** A
validation skipped costs a consumer a bad row they can see; a probe fired costs
an operator a request. RULES 15.3 states the rule that follows from it, and
this is the reading of it that applies while writing code.

---

## Right-sized, and how it reconciles with the above

Two different axes, and the code owes both.

- **Right-sized** forbids machinery for scale or a consumer that does not
  exist. No plugin framework for six sources.
- **Fail loud** forbids removing the validation, the version field or the guard
  to save lines.

⛔ **The line is sharp.** Removing scope nobody needs is good engineering.
Removing the validation on a volatile input is an outage being pre-written. The
arithmetic settles it: a few defensive lines cost a bounded, known amount, and
a silent corruption when a format shifts costs an unbounded amount found later.

---

## Style

- ⭐ **Comments state constraints and invariants, never narration.** "Ports are
  preserved exactly as written, because udp has no default-port convention" is
  worth writing. "Now we loop over the sources" is not.
- **Every file write names its encoding and its newline.**
  `open(path, "w", encoding="utf-8", newline="\n")`. The platform default is
  not UTF-8 on Windows and the platform newline is not `\n` there. RULES 15.5.
- **Paths resolve from the file's own location**, never from the working
  directory. A tool run from a subdirectory that silently acts on a smaller
  tree is the shape [`shell.md`](shell.md) and the checks both guard against.
- **Timestamps are ISO 8601 UTC**, and **the clock is injected**. Nothing in
  the pipeline calls `datetime.now()`, because RULES 3.6 makes determinism a
  correctness property and `gate.yml` asserts it on every push.
- **Bounded everything.** Every socket, every read, every retry has a limit.
  RULES 5.2.
- ⚠ **Anything consuming structured output selects by name, never by
  position.**
- **No ad-hoc printing in library code.** `src/trackers/` returns values;
  `scripts/` and `experiments/` are where output is printed.

---

## Testing

Tiers of trust, in this order:

1. **A real tracker**, probed from a real vantage. Only CI has one, and what it
   can reach is itself a measurement (RULES 15.1).
2. **The oracle**, [`../../tests/fake_tracker.py`](../../tests/fake_tracker.py):
   trackers this project controls, which can be made to truncate, stall, refuse
   or answer wrongly on demand.
3. **Pinned fixtures.** Captured upstream bodies under
   `tests/fixtures/sources/`, which is what makes the whole gate runnable
   offline on any host.
4. **Unit tests** over pure logic.

Rules that fall out of it:

- ⭐ **The oracle exists for determinism, not convenience.** A real tracker
  cannot be asked to return a truncated bencode body on demand, and that is
  exactly the case the probe has to get right.
- ⛔ **A test that cannot fail is not evidence.** Mutation-prove the guards:
  plant the defect the guard exists to catch and read the exit code, unpiped.
  Every check added in this repository has been through that, and the results
  are in the review it was added under.
- ⛔ **Test the production default of every injectable seam.** A suite where
  every test injects the double leaves exactly one branch untested, and it is
  the one that ships.
- **Every defect found later becomes a named regression test.**
- ⚠ **Where a negative test passes suspiciously well, doubt the harness before
  the code.** The thing under test is probably not the thing answering.
- **The suite touches no network.** That is not a preference: it is what makes
  "it works on my machine" checkable, and `gate.yml` is offline by
  construction.
