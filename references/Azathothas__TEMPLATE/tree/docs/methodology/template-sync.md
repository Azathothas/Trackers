# template-sync.md

Taking a later version of this template into a project that already adopted it.

⚠ **For the project's session, not for the template's.** Changing the template
itself is [`../../MAINTAIN.md`](../../MAINTAIN.md).

---

## ⭐ The asymmetry that makes this necessary

**A change to the template lands in every project started afterwards and in
none of the ones started before.** There is no migration path and there is not
going to be one: a template cannot reach into repositories it does not know
about, and it should not want to.

So a project that adopted this a month ago is running a month-old methodology,
and every fix made since is one it will never see unless somebody comes and
gets it.

⚠ **This is the same shape as a pinned dependency, in the direction people
forget.** Pinning protects a consumer from a change it did not review, which is
exactly why it also withholds a fix it would have wanted. Merging a fix here
deploys it nowhere.

---

## ⛔ What the template will never ask of a project

**Nothing.** Not attribution, not a notice, not a link back, not a mention in a
readme, not a marker in a file.

The licence is `0BSD` and that is the whole point of choosing it: use it, copy
it, change it, ship it, no conditions.

⛔ **So a sync never adds a line saying where a file came from**, and a session
doing one never proposes adding one. A project that wants no trace of its
origin is a project behaving exactly as intended.

---

## How a project knows a newer version exists

⭐ **Pin to a tag, not to a branch.** Record which tag the project took, in the
project's own rules, in one line.

```bash
curl -sSL -A curl/8 "https://api.github.com/repos/Azathothas/TEMPLATE/tags?per_page=5"
```

```bash
sh scripts/common/mine-repo.sh Azathothas/TEMPLATE --out /tmp/tsync --no-clone
```

The second one is the useful form when a project already carries the mining
helper: it fetches the tags, the releases and the tracker in one call, and
⭐ **the tracker is where a change's reasoning is**, which a tag name cannot
carry.

⚠ **A tag says a version exists. It does not say the version is good for you.**
Read the change before taking it, the same as any dependency.

---

## ⛔ The procedure, and the one rule that matters

**Take what you are missing. Overwrite nothing.**

1. **Fetch the newer template to a scratch path.** Never over the project.
2. **Diff, file by file**, against what the project actually has.
3. **Sort every difference into three piles:**

| pile | what to do |
| --- | --- |
| the project does not have this file at all | take it, if the project has a use for it. Most of the value is here. |
| the project has it, unmodified since adoption | take the new version |
| ⛔ **the project has it and has changed it** | ⛔ **stop.** Show the diff and let a person decide. |

⛔ **The third pile is the whole risk.** A project's edit to a convention is
usually the most considered thing in the file: somebody hit a case the template
did not anticipate and wrote the answer down. An automatic overwrite deletes
exactly that, and it deletes it silently because the file still looks right.

⚠ **How to tell the second pile from the third without guessing:** compare the
project's copy against the template **at the tag the project adopted**, not
against the newest one. Identical means unmodified. Any other method is a
judgement about somebody else's prose.

4. **Run the project's own gate afterwards**, and compare against the numbers
   from before the sync. ⭐ A sync that did not move a number did not do
   anything, and a sync that moved one downward is the finding.

---

## ⛔ What a sync never does

| | why |
| --- | --- |
| overwrite a file the project has edited | above. It is the one irreversible mistake available here. |
| replace the project's `AGENTS.md` | it is the project's router, written for the project. The template's is about bootstrapping. |
| replace the record, the work order, or any entry | those are the project's state, not the template's content |
| append to `.gitignore` or `.gitattributes` without showing the diff | they carry project-specific rules somebody worked out |
| take a document the project deliberately deleted at bootstrap | absence is a decision here. Check the project's rules before restoring anything. |
| ⛔ delete a script because the template no longer ships it | a removal upstream is not an instruction. The template dropped four helpers because they are maintained in one place now; a project that HAS them keeps working, and the only thing to take is the knowledge of where the maintained copy lives. ⚠ What is worth doing is comparing: if the upstream copy has fixed something yours has not, that is an entry, not a sync. |
| ⛔ rewrite history, or force-push | never, for this or anything else |
| add attribution | above |

---

## ⭐ What is actually worth syncing, in order

Most of the value is in the parts that are mechanical, because those are the
ones that get fixed:

1. **The checks under `scripts/common/`.** They are where defects are found and
   fixed, they carry their own reasoning in their headers, and a project rarely
   edits them. ⭐ Start here.
2. **`docs/conventions/shell.md`.** It accumulates measured platform traps and
   almost nothing in it is project-specific.
3. **The probe.** A newer one reports more, and reporting more is free.
4. **A convention the project has not edited.**
5. ⚠ **Nothing else, by default.** The methodology documents encode a working
   method the project has been running for weeks. A newer wording is not
   automatically a better fit.

---

## ⚠ How often

**When something breaks in a way a newer check would have caught**, and
otherwise on no schedule at all.

⛔ A calendar-driven sync of a document set nobody is having trouble with is
churn: it produces a large diff, no measured improvement, and a project whose
conventions now differ from what its own sessions have learned.

⭐ The honest trigger is a defect. The second-best is a new check, because a
check is the one thing here that can be proved to work before it is taken:
plant the defect it catches and read the exit code.
