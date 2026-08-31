# lean-adoption.md

Taking this template's engineering without shipping any agent-facing content.

⭐ **A first-class option, not a downgrade.** Some projects cannot carry it for
compliance reasons. Some maintainers do not want it, on taste, and that is a
complete reason. Either way the checks, the conventions, the probe and the
dotfiles are the parts that catch defects, and none of them mentions an agent.

---

## What this is, in one line

**Take the machinery. Leave the instructions written for a machine.**

⚠ **It is a selection, not a cleanup.** A project that installs everything and
strips it afterwards has a history full of the thing it did not want, and
history is the part that is expensive to change later. ⭐ Decide at adoption.

---

## What goes, and what stays

⛔ **The test is not "was this useful to an agent".** Nearly everything here
was. The test is **does this file address a reader as an agent, or exist only
to route one.**

### Stays. None of it addresses an agent.

| | |
| --- | --- |
| `scripts/common/*` | the checks. The largest part of the value, and each one catches a defect a person makes too. |
| `scripts/doctor/*` | the probe. It reports a machine; it does not instruct anyone. |
| `docs/conventions/shell.md` | measured platform traps. Useful to anybody who writes a shell script. |
| `docs/conventions/prose.md`, `docs.md` | a house style, and a check that holds it |
| `docs/conventions/code.md`, `forbidden-patterns.md` | read these two first: they are the most likely to carry a phrasing aimed at an agent, and it is a small edit rather than a deletion |
| `docs/security/secrets.md` | what never enters a tree |
| `docs/conventions/git.md` | commit rules. Including the one that forbids crediting a tool, which is the rule this project most wants. |
| `dotfiles/*`, `LICENSES/*` | ignore, attribute, editor, CI, licence texts |
| `docs/containers.md` | a procedure for measuring in a throwaway machine. Nothing in it addresses a session. |
| `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md` | ordinary project documents |

### Goes.

| | |
| --- | --- |
| `AGENTS.md`, and any file of that family under any name | ⛔ the routers. This is the whole point. |
| `ROUTE.md`, `ADOPT.md`, `MAINTAIN.md`, `bootstrap/` | entry points for an agent session |
| `docs/agent-tooling.md` | ⚠ the CATALOGUE is worth keeping and the framing is not. It opens by telling a session what to do before it installs something, and its first table is about a session's reflexes. Lift the tool rows into the project's own documentation and drop the rest. |
| `docs/methodology/*` | read the note below before deleting all of it |
| `docs/templates/*` | skeletons an agent fills in |
| the record, the handoff, the work model | a session-shaped way of tracking work. An issue tracker does the same job for people. |

⚠ **`docs/security/remote-ops.md` is a judgement call.** Its tiers are about
what may be done to systems outside the machine, which is a real policy for a
person too. Keep it and delete the paragraphs addressed to a session, or drop
it and write the policy in the project's own words.

### ⭐ The methodology is worth reading before it is deleted

⛔ **Do not just delete the directory.** Three things in it are engineering
practice rather than agent instruction, and a project that throws them out
loses the part that was actually paid for:

| from | the practice, restated for people |
| --- | --- |
| `gate.md` | a change passes automated suites, a run of the real thing, and a human reading, and each is blind to what the other two catch |
| `reviews.md` | a review pass asks one question. Three passes asking the same question is one pass written up three times. |
| `experiments.md` | a measurement lives in a script in the tree, carries its conditions, and a negative result gets committed |
| `vendoring.md` | patch what you vendor, record what you changed, reconcile a release by reading rather than by preferring |

⭐ **Lift those four into the project's own `CONTRIBUTING.md`, in the project's
own voice, and delete the originals.** That is a page of prose and it is the
highest-value thing in this procedure.

---

## The procedure

1. ⛔ **Decide before adopting.** At bootstrap this is a field in the answer
   sheet. Retrofitting is the expensive path.
2. **Take the "stays" set.**
3. **Read `code.md` and `forbidden-patterns.md`** and edit any sentence that
   addresses a session rather than a contributor. They are the two most likely
   to need it.
4. **Write the four practices above into `CONTRIBUTING.md`**, in the project's
   voice.
5. **Wire the checks into CI**, which is what makes them real for a human team.
6. **Run the checks.** ⭐ A first run producing findings has done its job.

```bash
sh scripts/common/check-markers.sh
```

⚠ **The marker rule is the one to think about rather than take.** ⛔ ⭐ ⚠ in
prose is opinionated on purpose, and a repository with an established voice
should not inherit it by accident. Either adopt it deliberately or edit the
allowlist in both halves of the check and say what the project's set is.

---

## ⚠ Retrofitting, when the project already took everything

Possible, and it is a normal thing to want.

⭐ **There is a tool for the listing**, `deslop`, and
[`../agent-tooling.md`](../agent-tooling.md) says where it lives. It reports
every agent-facing file in the tree and every reference to one, and changes
nothing until it is told to.

⛔ **Read the list before running anything that acts on it.** It is a deletion,
and a deletion in somebody's repository is the operation with no undo.

⚠ **Whether a file addresses an agent is a reading, so the listing matches
NAMES.** Anchor any match you write on the whole path: an unanchored match on
"agent" takes `src/agents/` in a project that builds one, which is a deletion
of somebody's source code.

Then, in order:

1. **Lift the four practices out** before deleting the files that hold them.
2. **Delete the agent-facing set**, in one commit, with a message that says
   what was removed and why.
3. **Fix every link that pointed into it.** ⭐ `check-docs` is what proves this,
   and it is why the check is in the "stays" set.
4. **Re-run the whole gate.**

### ⛔ About making it look as though no agent was ever involved

⚠ **Say plainly what each step can and cannot do**, because these two get
confused and only one of them is real:

- **Removing the agent-facing files** is complete and it works. After it, the
  repository contains no instruction addressed to a machine, and the commit
  rules already forbid crediting a tool in any message.
- ⛔ **Rewriting history does not un-publish anything.** A force push over a
  branch that has been fetched, forked, mirrored, or archived anywhere leaves
  every one of those copies intact, and it breaks every clone and every open
  contribution. It is a red-line operation:
  [`../security/remote-ops.md`](../security/remote-ops.md).

⭐ **The honest options, and both are legitimate:**

| | |
| --- | --- |
| **squash before the first publish** | if the repository is not public yet, a single initial commit is clean, costs nothing and loses nothing anybody has |
| **remove going forward** | on a published repository. The files are gone from the working tree and from every future clone's checkout. |

⛔ **A force push over published history is the operator's decision, made
explicitly, never a step a session takes on its own** and never something to
offer as a tidy finish.
