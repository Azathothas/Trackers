# Rules

<!-- TEMPLATE. Copy to the record directory, fill every {{PLACEHOLDER}}, delete
     this comment.

     This is the part of the record that does NOT change from session to
     session. What changes lives in PROGRESS.md.

     ⛔ THIS FILE RESTATES NOTHING. Earlier versions of it re-explained the
     session procedure, the kickoff prompt, the check contract and the
     exit-code rule, all of which live in the conventions. Measured in a
     project built from that shape: 8 sentences appeared verbatim in two
     documents and 3 whole sections were near-copies of a convention. Its
     maintainer cut the file from 149 lines to 66, and the only content with no
     other home was the part about THIS project.

     ⭐ So: what is specific here, and what each rule COST. A rule with no
     incident behind it is a preference, and a preference stated as a rule is
     what makes an agent stop believing the rules that matter. If you cannot
     say what a rule cost, it belongs in the conventions, not here. -->

How **this** repository is worked on. [`PROGRESS.md`](PROGRESS.md) is the
record and carries the work order; this file is the part that does not change
between sessions.

⭐ **Everything general is a link.** The rows below name where each rule lives,
and following the link is how you read it.

| topic | where it lives |
| --- | --- |
| what a session owes at its start and its end | [`docs/methodology/sessions.md`](../docs/methodology/sessions.md) |
| the kickoff and resume prompts, and the summary table | the same file |
| what a unit of work passes before it is done | [`docs/methodology/gate.md`](../docs/methodology/gate.md) |
| commit identity, and what may reach a remote | [`docs/conventions/git.md`](../docs/conventions/git.md) |
| anything outside this machine | [`docs/security/remote-ops.md`](../docs/security/remote-ops.md) |
| what a check must satisfy to be one | [`scripts/README.md`](../scripts/README.md) |
| how documents are written | [`docs/conventions/prose.md`](../docs/conventions/prose.md) |
| where superseded wording goes | [`docs/methodology/history.md`](../docs/methodology/history.md) |

---

## 1. This project's specifics

The procedure is in [`sessions.md`](../docs/methodology/sessions.md). What
only this project can say:

| | |
| --- | --- |
| the one command that runs every gate | `{{command}}` |
| the record | [`PROGRESS.md`](PROGRESS.md){{, and the index}} |
| the resume file | `RESUME.md`, written at the START of a session |
| {{anything else a session here must run}} | `{{command}}` |

⛔ **Re-measure the baseline rather than trusting the recorded one**, with the
command above, before touching anything.

---

## 2. Git, for this project

{{The push policy, stated in one sentence and meant. The default is: commit
freely and locally, never push. Publishing is the operator's.}}

Everything else is [`git.md`](../docs/conventions/git.md) and
[`remote-ops.md`](../docs/security/remote-ops.md).

{{What it cost to learn: {{the incident, if there was one}}.}}

---

## 3. The tools this project has

⚠ **Reach for the purpose-built tool before the general one.** A general tool
used where a specific one exists produces answers that are plausible and wrong,
which is the hardest kind to catch.

| question | tool |
| --- | --- |
| what host is this | `sh scripts/doctor/doctor.sh` |
| {{is the tree green}} | {{command}} |
| {{does the record agree with itself}} | {{command}} |
| {{an item closed, so the counts must move}} | {{command}}. ⛔ Never retype a count. |
| {{what has this session done, measured}} | {{command}} |
| {{commit}} | {{command}}, and nothing else |

⛔ **Every exit code above is read from the process that produced it.**
[`shell.md`](../docs/conventions/shell.md) section 2 says what that costs when
it is not.

---

## 4. The rules that bite most often, here

<!-- ⭐ THIS IS THE SECTION THAT EARNS THE FILE, and it is the only one with no
     other home. Each entry: the rule, then what it cost. Grow it every time
     something bites. A rule that could sit in the conventions belongs there
     instead, with a link from this table. -->

### The record is part of the change

⛔ [`PROGRESS.md`](PROGRESS.md), {{the index}} and the item are edited in the
**same change** as the work, never after it. A session that fixes something and
leaves it saying the work is open has not finished; it has published something
false into the one file the next session reads first.

⭐ **Enforced rather than remembered:** {{the gate that runs the record check}}.

{{What it cost: {{the incident}}.}}

### Claims need evidence

⛔ A comparative claim without a committed benchmark does not ship. A flag that
does not move a number does not ship.

### A blocked item stays open

⛔ Nothing here closes because somebody else would have to fix it. A blocked
item keeps its status, names the blocker, and says what would unblock it.

⚠ Where the blocker is code this project vendors, the answer is to patch it:
[`vendoring.md`](../docs/methodology/vendoring.md), which also settles that
upstreaming is not a topic.

### {{Add a rule here every time something bites}}

{{The rule. Then: what it cost to learn.}}

---

## 5. Settled decisions, not to be relitigated

<!-- ⭐ Rewrite in place when one changes, and move the superseded wording to
     the history directory. docs/methodology/history.md says where and why. -->

- **{{The decision}}.** {{The ruling, and the date. Why the alternative lost.}}
