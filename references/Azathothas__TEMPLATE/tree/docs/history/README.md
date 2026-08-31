# history

Superseded wording from this repository's own documents and scripts, kept
verbatim so a future session can find out why a rule is what it is instead of
re-deriving it wrongly.

⭐ **This is the template's own history, not a skeleton.**
[`../templates/HISTORY.md`](../templates/HISTORY.md) is the skeleton a project
receives; this directory is where THIS repository's retired text goes.
[`../methodology/history.md`](../methodology/history.md) is the rule both
follow.

⛔ **A bootstrap deletes this directory** and creates the project's own empty
one from the skeleton. A project inheriting the template's history would start
life with a record of decisions that were never its own.

---

## What is here

| file | what it holds |
| --- | --- |
| [`twins-and-scripts.md`](twins-and-scripts.md) | the retired rule that only the environment probe needed a PowerShell twin, and the removed licence and commit helpers |

---

## Claims this repository has published and later withdrawn

⭐ [`../methodology/history.md`](../methodology/history.md) asks for this list on
the front page, because a reader who trusts a document without checking it
trusts sentences that are wrong.

| claim | where it was | what withdrew it |
| --- | --- | --- |
| "Every other check here is POSIX sh alone, deliberately" | `scripts/common/check-twins.sh` header | a native PowerShell session on one Windows 11 machine, 2026-08-25, had no `sed` and resolved `sort` to `Sort-Object`. Every check gained a twin, and the header kept the retired sentence for as long as it took a maintenance session to read it. [`twins-and-scripts.md`](twins-and-scripts.md) |
| "The pair was proved instead by running both halves and both routes against one target, which is stronger evidence" | `scripts/README.md`, about `mine-repo` | it was not stronger. That comparison ran against a target whose comment bodies had balanced brackets, and the page joiner's defect was invisible to it. A consumer found it. `mine-repo --selftest` replaced the claim with a comparison that runs on every gate. |
