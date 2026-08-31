# template-sync.md

Taking a later version of `Azathothas/TEMPLATE` into this project.

---

## What this project took, and when

| | |
| --- | --- |
| adopted | 2026-08-31 |
| template commit | `6206166` |
| corpus copy | [`../../references/Azathothas__TEMPLATE/`](../../references/Azathothas__TEMPLATE/), the same commit. `diff -r` against a fresh clone reports three differences and no others: the upstream's own ignore file, which was never captured, and two agent instruction files removed under the rule in [`vendoring.md`](vendoring.md) |
| work model | the **todo** model, `docs/methodology/work-todo.md`, first read at `6eaf4b5` and re-read at `6206166`, byte-identical between the two |
| helpers | `Azathothas/ToolKit` at `bf11930`, pinned in [`../../scripts/vendor/toolkit/PIN.json`](../../scripts/vendor/toolkit/PIN.json) |

RULES.md's opening states the work-model choice and why. This page states what
to do when a newer version exists.

### What was deliberately not taken

| | why |
| --- | --- |
| the checks as `.sh` and `.ps1` pairs | RULES 15.5 makes a `.sh` a gate depends on a platform requirement in disguise. They were rewritten in Python, one implementation each, and the rules they hold are unchanged |
| `docs/history/` under `docs/` | this project's history directory was already [`../../HISTORY/`](../../HISTORY/) at the root, with several hundred citations into it. Moving it would be churn with no measured benefit |
| `CHANGELOG.md`, `SECURITY.md`, `docs/architecture.md` `(planned)` | [`../conventions/docs.md`](../conventions/docs.md) says why each is absent and what would bring it back |
| the bootstrap and template directories | this project is not a template and does not start others |

---

## ⭐ The asymmetry that makes a sync necessary

**A change to the template lands in every project started afterwards and in
none of the ones started before.** There is no migration path and there is not
going to be one.

⚠ **This is a pinned dependency, in the direction people forget.** Pinning
protects a consumer from a change it did not review, which is exactly why it
also withholds a fix it would have wanted.

---

## ⛔ What the template asks of this project

**Nothing.** Not attribution, not a notice, not a link back. The licence is
`0BSD`, which is the whole point of choosing it.

⛔ **So a sync never adds a line saying where a file came from**, and a session
doing one never proposes adding one.

---

## Finding out that a newer version exists

```bash
curl -sSL "https://api.github.com/repos/Azathothas/TEMPLATE/commits?per_page=5"
```

Where the direct route is blocked, RULES 16's read-only proxies reach it.

⚠ **A newer commit says a version exists. It does not say the version is good
for this project.** Read the change before taking it, the same as any
dependency.

---

## ⛔ The procedure, and the one rule that matters

**Take what is missing. Overwrite nothing.**

1. **Fetch the newer template to `.tmp/`.** Never over this tree.
2. **Diff file by file** against what this project actually has.
3. **Sort every difference into three piles:**

| pile | what to do |
| --- | --- |
| this project does not have the file | take it, if there is a use for it. Most of the value is here |
| this project has it, unmodified since adoption | take the new version |
| ⛔ **this project has it and has changed it** | ⛔ **stop.** Show the diff and let the operator decide |

⛔ **The third pile is the whole risk.** A project's edit to a convention is
usually the most considered thing in the file: somebody hit a case the template
did not anticipate and wrote the answer down. An automatic overwrite deletes
exactly that, silently, because the file still looks right.

⚠ **How to tell the second pile from the third without guessing:** compare
against the template **at the commit this project adopted**, which is the copy
in `references/Azathothas__TEMPLATE/tree/`, not against the newest one.
Identical means unmodified.

4. **Run the gate afterwards and compare against the numbers from before.**
   ⭐ A sync that did not move a number did not do anything, and a sync that
   moved one downward is the finding.
5. **Update the table at the top of this page**, and re-capture the corpus copy
   if the commit moved, which also moves `references/PROVENANCE.md`.

---

## ⛔ What a sync never does

| | why |
| --- | --- |
| overwrite a file this project has edited | above. It is the one irreversible mistake available here |
| replace [`../AGENTS.md`](../AGENTS.md), the record, the work order, or any entry | those are this project's state, not the template's content |
| append to `.gitignore` or `.gitattributes` without showing the diff | both carry rules that cost real time to work out. ⚠ Every path rule here is anchored, and an unanchored one copied in has already dropped 20 corpus files |
| take a document this project deliberately declined | absence is a decision. The table above records each one |
| ⛔ delete something because the template no longer ships it | a removal upstream is not an instruction. What is worth doing is comparing: if the upstream copy fixed something this one has not, that is an entry, not a sync |
| rewrite history, or force-push | RULES 13 and [`../conventions/git.md`](../conventions/git.md) |

---

## ⭐ What is actually worth syncing, in order

1. **The checks.** They are where defects are found and fixed, and they carry
   their reasoning in their headers. ⚠ Here they have to be read and ported
   rather than copied, so the unit is the **rule**, not the file.
2. **`docs/conventions/shell.md`.** It accumulates measured platform traps and
   almost nothing in it is project-specific.
3. **The pinned helpers**, which are a re-fetch and a digest rather than a
   merge.
4. **A convention this project has not edited.**
5. ⚠ **Nothing else, by default.** A newer wording is not automatically a
   better fit for a project that has been running its own for weeks.

---

## ⚠ How often

**When something breaks in a way a newer check would have caught**, and
otherwise on no schedule at all.

⛔ A calendar-driven sync of a document set nobody is having trouble with is
churn: a large diff, no measured improvement, and conventions that now differ
from what this project's own sessions have learned.

⭐ The honest trigger is a defect. The second best is a new check, because a
check is the one thing here that can be proved to work before it is taken:
plant the defect it catches and read the exit code.
