# bootstrap

Everything used once, when a project starts. ⛔ **Deleted at the end of the
bootstrap**, this directory with it.

---

## For you, the operator

Three steps.

**1. Clone the template into a new directory.**

```bash
git clone https://github.com/OWNER/REPO.git my-new-project
```

```bash
cd my-new-project
```

**2. Fill in [`ANSWERS.md`](ANSWERS.md).** Two minutes. Every field has a
default, and blank is a valid answer to all but three of them. The three worth
a sentence each are what it is, who it is for, and whether it will be public.

⚠ **Do not put a secret in it.** Name what you hold; the agent tells you where
it goes.

**3. Paste a prompt from [`prompts/`](prompts/), with your answers, into a
fresh agent session.**

⭐ **Or paste nothing but [`../ROUTE.md`](../ROUTE.md)'s URL** and let the agent
work out which of these it is from the tree. That is the shorter path and it
covers cases this table does not, including re-syncing a later version of the
template and adopting it with no agent-facing content at all.

| you are | paste |
| --- | --- |
| starting something new | [`prompts/00-new-project.md`](prompts/00-new-project.md) |
| adopting a codebase sitting right here, beside these files | [`prompts/01-existing-project.md`](prompts/01-existing-project.md) |
| ⭐ adopting a repository **somewhere else**, with nothing cloned | [`prompts/05-adopt-existing-repo.md`](prompts/05-adopt-existing-repo.md) |

⭐ **The last row is the common case and it needs none of this directory.** The
agent fetches [`../ADOPT.md`](../ADOPT.md) over HTTPS and works from that: it
carries its own safety contract, procedure and manifest. Paste it into the
agent of the project you want adopted, not into one here.

That is the whole procedure. The agent does the rest and stops for your
approval before writing any implementation code.

---

## The other prompts, for later

These are for a project that is already running. ⭐ Keep them somewhere outside
the project, because this directory is deleted at the end of the bootstrap.

| when | paste |
| --- | --- |
| adding a unit of work | [`prompts/02-new-work.md`](prompts/02-new-work.md) |
| a session stopped before it finished | [`prompts/03-resume.md`](prompts/03-resume.md) |
| your validation failed | [`prompts/04-rework.md`](prompts/04-rework.md) |

⚠ **For a normal boundary, use the prompt the agent printed**, not one of
these. It names the resume point and carries the warnings from the session that
just ended. These are the fallback for when you do not have one.

---

## For the agent

[`BOOTSTRAP.md`](BOOTSTRAP.md) is the procedure, and it is read end to end
before step 1 runs. It happens once per project and sets every rule that
follows, so partial reading here is a defect rather than a judgement call.

Its step 0 has no prerequisite and comes before everything else: **detach the
remote.** A fresh clone points at the template, and a writing session with the
wrong remote configured is one push away from putting one project's history
into every future project's starting point.

---

## What you get back

Not code. A bootstrap produces:

1. the probe's report of the machine, and anything it contradicts;
2. the decisions the agent defaulted, as a list you can object to;
3. what it kept from the template and what it deleted;
4. a machine checklist: every tool, its minimum version, the command to check
   it. That is your first task, and it exists so work does not stall three
   tasks in on a missing tool;
5. the agent's own capability report, so a gate it cannot run is a setup
   requirement you hear about now rather than a step skipped later;
6. the locked decisions, the architecture and the work plan, for approval;
7. the first unit of work, and its kickoff prompt.

⛔ Then it stops. Nothing is implemented until you approve.

---

## Why this directory is deleted

It is scaffolding for starting a project, and a project that has started does
not need the instructions for starting. Leaving it costs every future session
the moment it takes to work out that the directory does not apply, and that
cost is paid on every session forever.

⭐ **If your filled-in answers carried context worth keeping**, the agent copies
it into the project's record first, ⛔ with the secrets section stripped, and
says in its report that it did.
