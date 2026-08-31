# Handoff: {{unit id}} {{phase, if any}}

<!-- TEMPLATE for the stage model. Copy to plans/executions/HANDOFF-{{id}}.md.
     Written for the NEXT session, which may be a different context with none
     of this one's memory.

     ⭐ The governing principle: the conversation is not the source of truth.
     The tree, this file and the running system are. A summary can claim
     anything; none of it is real until an artefact confirms it. -->

**Status:** {{complete | partial}}
**Written:** {{ISO 8601 UTC}}
**Commit at close:** {{short sha}}
**Deployed:** {{version, or "not deployed"}}

{{If partial: ⭐ RESUME HERE, at the exact next unstarted task or checkpoint, and
why it stopped. "Ran out of budget" is a reason.}}

---

## 1. For the operator

<!-- ⭐ At the top, because it is the only part they must act on. ONLY things a
     human can do: a real secret, billing, a DNS change, a key rotation.
     ⛔ The driven pass is never here. -->

- [ ] {{the item, with the exact command or click}}
- [ ] {{or "none"}}

---

## 2. What was built

{{Per task: what changed, at file and line. What it reuses rather than forks.}}

| task | status | where |
| --- | --- | --- |
| {{T1}} | {{done / partial / not started}} | {{file:line}} |

---

## 3. The gate

### (a) Headless suites

{{The actual commands run, and their trimmed output. Not a description of them.}}

```text
{{paste the real output}}
```

⛔ **File count against disk:** {{N reported by the runner, M on disk}}. A green
count beside an error line is a file that never ran.

⛔ **Every guard, read unpiped:** {{each one, and its exit code}}

### (b) The driven pass

<!-- ⛔ Not optional and not a description of intent. What you actually did with
     the running system, and what it showed. -->

{{How you reached it, as which identities, the exact flow, and what each step
returned. Assert exact state, not an impression.}}

{{⚠ If this could not be run: say so plainly, name the capability that was
missing, and say what is needed. That is a finding, not a deferral. Shipping as
though it happened is a failed gate wearing a green badge.}}

### (c) The deep reviews

| # | lens | what it swept | findings |
| --- | --- | --- | --- |
| 1 | the door sweep | {{every affordance's callers, then the grep for the ones not enumerated}} | {{what it found}} |
| 2 | the guard mutation | {{which guard, what defect was planted, the exit code read unpiped}} | {{what it found}} |
| 3 | the claim audit | {{which sentences were checked against which artefacts}} | {{what it found}} |

⚠ **A pass with no findings says what would have had to be true for it to
fire.** Three passes reporting nothing is a weaker result than one pass
reporting a real defect.

**Fixed as a result:** {{what, and where}}
**Not fixed:** {{what, and where it is now tracked}}

### Change summary

{{files touched, lines added, lines removed}}

---

## 4. Deviations from the plan

{{What differed, and why. ⛔ A deviation from a locked decision is recorded
here with the operator's ruling, never silently swallowed. The next reader
needs to know where reality and the plan diverged.}}

{{If the scope changed mid-flight: which case it was, a small deviation or a
re-scope, and if a re-scope, that the gate was re-run against the NEW scope.}}

---

## 5. Known gaps

{{Each one, and where it is tracked now. ⛔ A gap with no home is a gap that
gets silently abandoned.}}

---

## 6. Self-review

{{The plan's questions, answered honestly. ⚠ "Mostly", "should" and an
unanswered question are the three things a reviewer looks for.}}

---

## 7. ⭐ How to verify the current state from scratch

<!-- ⭐ This is the section that pays for the whole file. It is the runbook a
     cold session runs VERBATIM to confirm everything still works. Not a
     description: the exact commands, in order, with what each should print. -->

```bash
{{command 1}}
```

{{what it should print}}

```bash
{{command 2}}
```

{{what it should print}}

---

## 8. What the next session should know

{{Anything true that is not in the plan, the record or the code. Environment
facts, a trap you hit, a thing that looked broken and was not. ⚠ Keep it to
what a session would otherwise re-derive.}}
