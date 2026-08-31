# Stage {{NN}}: {{TITLE}}, {{one-line theme}}

<!-- TEMPLATE for the stage model. Copy to plans/stage-{{NN}}-{{slug}}.md,
     delete these comments, fill every {{PLACEHOLDER}}.
     A stage file is NORMATIVE: it encodes decisions and proves, not narration.
     Authored per docs/methodology/authoring.md, and NOT written to disk until
     the operator approves it. -->

> **Status: DRAFT, awaiting approval.**
> <!-- Flip to "ACCEPTED ({{ISO 8601 UTC}})" only after sign-off. -->
>
> {{One paragraph: what this delivers and why now. If it grew from an intake,
> say so.}}
>
> Ships in {{N}} phase(s), each with its own handoff and its own kickoff.

---

## 0. Prerequisites read, and what each one changed

<!-- ⛔ Not boilerplate. It exists because a skimmed prerequisite is invisible
     until a wrong number ships. FILL THE THIRD COLUMN HONESTLY: "nothing" is a
     legal answer once or twice, and a table where every row says "nothing"
     means it was skimmed. -->

| file | read in full | ⭐ what it changed about this plan |
| --- | --- | --- |
| {{the router}} | {{yes}} | {{the rule that moved a decision, or why none did}} |
| {{the conventions}} | {{yes}} | {{the forbidden-pattern row this plan must obey}} |
| {{the code map}} | {{yes}} | {{the seam this touches, and the one-path consequence}} |
| {{the technical reference}} | {{yes}} | {{the constant or state machine this depends on. ⛔ Quote it; do not recall it.}} |
| {{the previous handoff}} | {{yes}} | {{deployed version, migrations, what it left owed}} |

⛔ **Anything I could not read, with the reason:** {{name it, or "none". A
missing prerequisite is a finding, never a licence to proceed on memory.}}

**Instruments this plan uses.** ⛔ Name the command, not the intention:
{{the commands that will produce the evidence}}

---

## 1. What this is, and what it is NOT

**It is:** {{the delta, in one or two sentences. The smallest true statement of
the change.}}

**It is NOT:** {{explicit non-goals. What a reader would plausibly expect and
this will not do, so scope creep is a visible violation rather than a
judgement call.}}

### 1.1 The constraint this lives under
<!-- Delete if none. A limit you must design WITHIN, not around. Write it into
     the behaviour, not around it. -->
{{e.g. a memory ceiling, a rate limit, a platform policy}}

### 1.2 What already exists, verified against the tree
<!-- ⛔ Audit the intake against the code FIRST. The task is usually a delta,
     and rebuilding what exists is the most expensive mistake available. -->

| the ask | the reality, verified, at file and line |
| --- | --- |
| {{what was asked for}} | {{what is already there}} |

### 1.3 Decisions
<!-- The genuine forks, RULED. Recommend the best option; record the operator's
     call where it changed the plan. This is where you challenge the intake. -->

- **D1, ruled {{date}}: {{the decision}}.** {{Why, and the reason the
  alternative lost.}}

---

## 2. Tasks

<!-- One per coherent deliverable. Each carries the anchors, the invariant it
     must hold, the one path it reuses rather than forking, and a PROVE.
     ⛔ A prove with no command is a paragraph. -->

### T1: {{name}}

{{What to build, where at file and line, the invariants it must not break, and
the one existing path it reuses rather than forking.}}

**Prove:** {{the command that produces the evidence, and what exit code or
output counts as passing.}}

**Checkpoint:** {{what to verify before the next task depends on this. Delete
if not warranted.}}

### T2: {{name}}

{{...}}

---

## 3. Pitfalls

<!-- The traps specific to THIS stage, plus the recurring classes it re-enters.
     Check against docs/conventions/forbidden-patterns.md. -->

1. {{pitfall, and what it would cost}}

---

## 4. Required tests

{{Beyond each task's prove: the cross-cutting properties. Name what any mock
stands in for, and what only the real environment can show. Every defect found
later becomes a named regression test.}}

---

## 5. Acceptance: the three-part gate

Specified in [`docs/methodology/gate.md`](../docs/methodology/gate.md). This
stage's specifics:

**(a) Headless suites.** {{the exact commands}} , ⛔ file count against disk ,
⛔ every guard exits 0, read unpiped.

**(b) The agent drives the real thing.** {{how to reach it, as which
identities, and the exact flow that makes the change provable}}
⛔ Deferring this to the operator is a failed gate.

**(c) At least three deep reviews**, three different lenses:
[`docs/methodology/reviews.md`](../docs/methodology/reviews.md). Findings
recorded per pass, and fixed.

Then: the record updated in the same change , the handoff written , the summary
table printed and saved , the next prompt printed in chat only.

---

## 6. Self-review, answered honestly in the handoff

<!-- The questions that force honesty. ALWAYS include the last two. -->

1. {{a stage-specific honesty question, e.g. "what did you reuse rather than
   rebuild? Show the change summary."}}
2. **Name every door onto {{the new affordance}} and every surface showing its
   state. Which did you find only by grepping?** ⚠ The answer is never "none".
3. **What did the driven pass and the reviews find that the green suite could
   not?** ⚠ "Nothing" means they were too shallow. Say what you drove and what
   you swept, and what would have had to be true for a pass to fire.

---

## 7. For the operator

<!-- ONLY the things a human must do: a real secret, billing, a DNS change, a
     key rotation, an account. ⛔ The driven pass is NOT here. -->

- [ ] {{the item, with the exact command or click}}
