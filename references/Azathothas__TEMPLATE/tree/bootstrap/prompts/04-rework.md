# Reject a unit of work

Paste this when your validation failed. The agent said it was done; you found
it was not.

⭐ **Be specific in three parts per issue: what you did, what happened, what you
expected.** The third is the one usually left out, and it is the one that says
whether this is a defect at all or a disagreement about scope.

---

```text
Read, IN FULL, before anything else.

- [ ] the project's AGENTS.md
- [ ] the plan or entry for the work being rejected
- [ ] its handoff, especially the driven-pass log and the review findings

<THE UNIT> is NOT accepted. What I found during validation:

1. What I did:        ...
   What happened:     ...
   What I expected:   ...

2. What I did:        ...
   What happened:     ...
   What I expected:   ...

HOW TO WORK THIS

- ⛔ REPRODUCE EACH ONE FIRST, before changing anything. A fix for a defect you
  have not reproduced is a guess, and a guess that happens to make the symptom
  go away is worse than the defect, because now nobody knows what it was.
- Fix the ROOT CAUSE. No workarounds, and no special-casing the input I
  happened to use.
- ⭐ Add a named regression test per issue. Every defect found after the fact
  becomes a test, so the class does not recur silently.
- ⭐ Then ask the question this list is really evidence for: WHY DID THE GATE
  PASS? Each of these got through three parts that exist to catch it. Say, per
  issue, which part should have caught it and why it did not. That answer is
  worth more than the fix, and it usually changes a check rather than a line of
  code.
- Re-run the FULL gate, all three parts, against the current state. Not the
  parts you think are affected: the whole thing.
- Update the handoff, including what the gate analysis above found.

⛔ Do not proceed to anything else. Do not start the next unit of work, and do
not fold in an improvement you noticed on the way. If you found something else
worth doing, tell me and I will file it.
```
