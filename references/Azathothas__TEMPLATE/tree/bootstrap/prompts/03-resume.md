# Resume an interrupted session

Paste this when a session stopped before it finished: it ran out of budget, it
hit a blocker, the harness died, or you interrupted it.

⭐ **The agent normally prints its own resume prompt** at the end of any session
that left something unfinished. Use that one when you have it: it names the
resume point. This is the fallback for when you do not, including when a
session died without printing anything.

⚠ It works for a **different** agent with none of the previous session's
context. That is the normal case, not the exception.

---

```text
Read, IN FULL, before anything else. Do not skim and do not grep.

- [ ] the project's AGENTS.md
- [ ] docs/methodology/sessions.md, the resuming section
- [ ] the record

⛔ ABORT AND SAY SO if you cannot locate one.

RESUME an interrupted session.

⛔ THE CONVERSATION IS NOT THE SOURCE OF TRUTH. The tree, the record and the
running system are. Reconstruct the state from those. Do not trust old context,
and do not trust the two lines below: verify them.

What I last saw you doing: <optional, one line>
What I think happened:     <optional, one line>

RECONSTRUCT FIRST, before touching anything:

- what is committed, and what is uncommitted work in progress
- what the record says is done, and how it says to verify that
- whether the deployment is ahead of or behind the tree
- whether the checks are green NOW

THEN RECONCILE, before continuing:

- Uncommitted work in the tree means an edit was mid-flight. Read the diff and
  understand it. Finish it or revert it. ⛔ Never build on work in progress you
  do not understand: a broken half-edit is worse than nothing, because the next
  reader cannot tell it from finished work.
- A deployment ahead of the tree means something was deployed and never
  committed. Recover it now. That state cannot be resumed from.
- Red checks mean the last edit broke something. Fix to green FIRST. A session
  that builds on red compounds the break.
- A file the record claims was written that is not on disk means the write
  never landed. Redo it. The claim was not real.

Re-run the capability check for THIS session. The session that stopped may have
had a capability this one lacks, or the reverse, and that changes what you can
prove.

Then report the reconstructed state to me, and continue at the next unstarted
task. If you are winding down instead, freeze cleanly: get the tree coherent,
leave the checks green, write the partial record with a resume pointer, and
print the next prompt.
```
