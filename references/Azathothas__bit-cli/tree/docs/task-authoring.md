# How a rough idea becomes a filed entry

The front door for new work. When a defect, a feature or a refactor arrives,
the job is to **author an entry, not to implement it**.

**Authoring and implementing are different sessions.** An entry that is written
and implemented in one sitting is an entry whose premise was never checked
against the code, and this repository has three whose titles are known false
for exactly that reason. The entry is the artefact; the implementation is the
next session's.

The shape of an entry is in [`TODO/RULES.md`](../TODO/RULES.md) and the
existing entries are the examples. This page is the procedure that produces
one.

## 1. Ground first, and never author in a vacuum

Read the code the idea touches before writing a line about it.
`codegraph_explore` answers "how does this work, who calls it, what is the
blast radius" in one call, including dynamic-dispatch hops grep structurally
cannot follow.

**Audit against what already exists.** Most ideas are a delta. Rebuilding
something the tree already does is the most expensive mistake available, and
[`TODO/INDEX.md`](../TODO/INDEX.md) is 192 rows: the thing being proposed may
already be filed, or already closed.

**Before typing a flag, read [`man/bit-cli.json`](../man/bit-cli.json).** A
guessed flag is an entry that proposes something that already exists under
another name, or a collision nobody notices until implementation.
[`flags.md`](flags.md) is the naming convention this CLI already follows.

Cite `file:line`. `scripts/check-todo.ps1` resolves every cited path and line,
and checks the line against the symbol the prose names beside it.

## 2. Measure before building, when the entry describes what the code does

This is [`TODO/RULES.md`](../TODO/RULES.md) section 5's line and it is the one
that pays most often here.

An entry that says "X does not work" or "Y is unbounded" is a claim about the
binary, and the binary is on disk. Two entries in one session recommended work
the code had already made unnecessary, and each took one command to check.

An entry whose premise a measurement disproves keeps its title, because the
title is how it has always been referred to, and **the correction is written
underneath rather than over the premise**.

## 3. Challenge the intake

This is where the value is, not in typing. Propose the better approach even
when it differs from what was asked, with the reason and the evidence.

Two failure modes, and they are opposite halves of one mistake:

**Designing a ceiling.** A hard-coded limit or a single-scale assumption is a
defect. The question is not "is this too big for our use" but "does this design
a ceiling".

**Building machinery nothing asked for.** A speculative abstraction is the
other half. An entry that adds a knob with no caller is not smaller for having
been made configurable.

Where the intake and the better answer differ, say so, then author the better
one under a stated assumption. Where a genuine fork exists, write it as a
**Decision** with a recommendation rather than leaving it open, so the operator
rules on one question rather than reading an essay.

## 4. Right-size it

| size | what it looks like |
| --- | --- |
| S | under a day. One flag, one check, one fixture |
| M | a few days. A new seam, or a measurement that needs a fixture built first |
| L | a week. A subsystem, or something that needs an operator ruling before it is workable |
| XL | longer. Almost always two entries pretending to be one |

An entry that cannot be started without a ruling is honest about it in its
`Status:` line, carries its recommendation, and states the question. It is not
smaller for being written as if the ruling had happened.

## 5. What every entry carries

The fields are the existing entries' and they are not decoration:

- **`Source:`** where the idea came from. It records provenance, not a path a
  reader must be able to open: "the operator", "found while measuring T-184",
  or a corpus citation.
- **`Category:`, `Priority:`, `Effort:`, `Status:`** as
  [`TODO/INDEX.md`](../TODO/INDEX.md) defines them.
- **Problem** what is wrong, in terms of what a user or a script sees.
- **Premise** what is believed, and how it was checked. A premise that was read
  rather than measured says so.
- **Approach** what to do, with the seam named at `file:line`.
- **Decision** where a fork exists, with a recommendation and the reason the
  alternative lost.
- **Prove** the acceptance, and it is a command.

## 6. The acceptance command, and why it is the hard part

**A "prove" with no command is a paragraph.** "Verify the source is retried" is
not an acceptance; `pwsh -NoProfile -File scripts/check-signed-source.ps1`,
exit 0, is.

Three rules the acceptance has to satisfy:

**It waits on the condition, never on a guessed duration**, and never asserts
that the machine cannot fail some other way. Seven entries have cost a red CI
job for that.

**"Both of these will happen" is the same assumption as "this will happen in N
seconds."** A fixture with two sources or two peers that asserts each did some
work is asserting a scheduling outcome it does not control. Arrange it instead:
make each one the only supplier of something, and wait on the condition between
stages.

**A comparative claim needs a committed benchmark.** If the entry claims
something is faster, smaller or fewer, the acceptance names the bench script
that produces the number, and if no such script exists, filing that script is
part of the entry.

Where the acceptance needs a check script that does not exist yet, name it
**without its directory**: `scripts/check-todo.ps1` resolves every
`scripts/...` path a `TODO/` file writes, so the resolvable form arrives with
the file.

## 7. Filing it

1. Append the entry to the `TODO/<category>.md` that owns it.
2. Add its row to [`TODO/INDEX.md`](../TODO/INDEX.md), sorted by id.
3. Update the counts, in the prose and in the priority table.
4. Run the check, unpiped:

```bash
pwsh -NoProfile -File scripts/check-todo.ps1
```

That resolves statuses between the index and the entry, rows without entries
and entries without rows, the counts in both places, `T-NNN` references, dead
links, and cited paths and line numbers. It is one second and it is a gate, so
a disagreement cannot reach a commit.

5. Say in [`TODO/PROGRESS.md`](../TODO/PROGRESS.md) what was filed and why.

## 8. What an authoring session does not do

**It does not implement.** Barrelling into the implementation is the mistake
this page exists to prevent.

**It does not close anything as "won't fix", "upstream problem" or "out of
scope".** The trees are vendored so that anything in `librqbit` can be fixed
here. A blocked entry stays open with the blocker named and what would unblock
it.

**It does not touch anybody else's repository.**
[`TODO/RULES.md`](../TODO/RULES.md) section 6a is absolute, and an entry whose
approach is "send this upstream" is an entry that has not been authored yet.
