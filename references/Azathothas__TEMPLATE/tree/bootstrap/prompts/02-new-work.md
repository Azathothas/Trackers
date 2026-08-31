# Add new work

Paste this into a **fresh** session to add a unit of work to a project that is
already running. It works the same for a three-item project and a three-hundred
item one.

⛔ **The agent authors a plan from this. It does not implement.** Implementation
begins in another session, from the kickoff it prints at the end.

---

```text
Read, IN FULL, before anything else. Do not skim and do not grep.

- [ ] the project's AGENTS.md
- [ ] docs/methodology/authoring.md
- [ ] the record, for where things stand
- [ ] the technical reference, for the area this touches

⛔ ABORT AND SAY SO if you cannot locate one.

NEW WORK INTAKE. Author a plan from this. ⛔ DO NOT IMPLEMENT.

Title:
Type:            bug | feature | refactor | hardening | polish | chore
What and why:    <the problem or the ask, in your words>
Evidence:        <file and line, an error, a log, a report, a URL>
In scope:        <what IS included>
Out of scope:    <what is explicitly NOT included>
Constraints:     <locked decisions, red lines, must-nots>
Size guess:      <you right-size it>
Already decided: <so you do not re-litigate it>

Anything blank is yours to propose, with a recommendation rather than silence.

HOW TO AUTHOR IT

- Ground first. Read the code this touches before writing a line about it, and
  cite file and line. Audit against what ALREADY exists: most asks are a delta,
  and rebuilding what the tree already does is the most expensive mistake
  available. Check whether this is already filed or already closed.
- Measure before building, if this describes what the code does. A claim that
  something does not work is a claim about the program, and the program is on
  disk. It usually takes one command.
- Challenge the intake. This is where the value is, not in the typing. Propose
  the better approach even when it differs from what I asked for, with the
  reason and the evidence. Two failure modes to avoid in opposite directions:
  designing a ceiling, and building machinery nothing asked for.
- Where a genuine fork exists, write it as a DECISION with a recommendation, so
  I rule on one question rather than reading an essay.
- Every task carries a PROVE, and ⛔ a prove with no command is a paragraph.
  It waits on the condition, never on a guessed duration. It does not assert a
  scheduling outcome it does not control. A comparative claim names the
  benchmark that produces the number, and if none exists, writing it is part of
  this work.
- The acceptance is the full three-part gate. Not a subset, and not trimmed
  because this is "just a fix": a fix's blast radius is exactly what the driven
  pass and the reviews find.

THEN

Present the scope in and out, the decisions each with your recommendation, the
task list with its proves, and the size. ⛔ WAIT for my explicit yes. Do not
write the file to disk until I approve.

On approval: write it, update the record, run the record check unpiped, and
print the kickoff prompt in chat only.
```
