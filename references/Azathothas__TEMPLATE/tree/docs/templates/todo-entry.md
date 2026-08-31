# {{T-NNN}}: {{title}}

<!-- TEMPLATE for the todo model. Append to TODO/{{category}}.md, and add its
     row to TODO/INDEX.md in the same change.
     Authored per docs/methodology/authoring.md, and NOT filed until the
     operator approves it.
     ⛔ The title is how this entry will always be referred to. If a
     measurement later disproves the premise, the title STAYS and the
     correction is written underneath. -->

**Source:** {{where the idea came from. Provenance, not a path a reader must be
able to open. "The operator", "found while measuring T-184", a citation.}}
**Category:** {{category}}
**Priority:** {{P0 | P1 | P2 | P3}}
**Effort:** {{S | M | L | XL. XL is almost always two entries.}}
**Status:** {{open | partial | blocked | done}}

---

## Problem

{{What is wrong, in terms of what a user or a script actually sees. Not the
implementation. The symptom.}}

## Premise

{{What is believed, and ⭐ how it was checked. A premise that was READ rather
than MEASURED says so, in those words.}}

⚠ {{If this entry describes what the code does, the code is on disk. Measure
before building: two entries in one session recommended work the code had
already made unnecessary, and each took one command to check.}}

## Approach

{{What to do, with the seam named at file and line. The one existing path it
extends rather than forking.}}

⛔ {{What it must not do: the ceiling it must not design, the abstraction it
must not build.}}

## Decision

<!-- Delete if there is no fork. Where one exists, write it as a decision with
     a recommendation rather than leaving it open, so the operator rules on one
     question instead of reading an essay. -->

{{The fork. The recommendation. The reason the alternative lost. Blank until
ruled; ruled entries carry the date and the ruling.}}

## Prove

⛔ **The acceptance, and it is a command.**

```bash
{{the command}}
```

{{What counts as passing: the exit code, and the specific output.}}

Three rules this has to satisfy:
- it waits on the condition, never on a guessed duration;
- it does not assert a scheduling outcome it does not control;
- a comparative claim names the benchmark that produces the number, and if no
  such benchmark exists, writing it is part of this entry.

---

## Closing

<!-- Filled when the entry closes, in place, in the same change as the work. -->

**Closed {{ISO 8601 UTC}}.** {{What was done.}}

```text
{{the acceptance command's ACTUAL output, pasted}}
```

{{⛔ If a measurement disproved the premise above: the correction goes HERE,
underneath, never as an edit to the premise. Say what was believed, what was
measured, and what that changes.}}

{{⚠ If this is blocked rather than done: it stays open, with the blocker named
and what would unblock it. Nothing closes as "won't fix", "upstream's problem"
or "out of scope".}}
