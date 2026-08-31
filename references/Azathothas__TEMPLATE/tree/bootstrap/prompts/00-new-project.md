# Start a new project

Paste the block below into a fresh agent session, in a fresh clone of this
template, together with your filled-in [`../ANSWERS.md`](../ANSWERS.md).

You can paste the prompt alone. The agent will ask for what it cannot default.

---

```text
Read, IN FULL, before anything else. Do not skim, do not grep, do not work from
a previous session's memory, and do not substitute a code-graph query for
reading the bytes. A bootstrap happens once per project and sets every rule
that follows it, so this reading is the cheapest money the project will spend.

- [ ] ./AGENTS.md
- [ ] ./bootstrap/BOOTSTRAP.md
- [ ] ./scripts/doctor/README.md

⛔ ABORT AND SAY SO if you cannot locate one of those. A missing prerequisite is
a finding, not a licence to proceed on memory.

You are BOOTSTRAPPING a new project from this template. Follow BOOTSTRAP.md.

Its step 0 comes before everything: detach the remote. This clone points at the
template, and committing project work there would write one project's history
into every future project's starting point.

Then run the probe and report what it measured, including anything it
contradicts about what I have told you below.

DO NOT WRITE IMPLEMENTATION CODE IN THIS SESSION. The bootstrap produces a
configured repository, a plan, and the first unit of work, presented for my
approval. It stops there.

=====================================================================
<PASTE YOUR FILLED-IN bootstrap/ANSWERS.md HERE, or a paragraph, or nothing>
=====================================================================

HOW I WANT YOU TO WORK

- Be a senior engineer, not a code generator. Collaborative, skeptical,
  proactive, evidence-driven. NEVER a yes-machine: if I am missing a
  requirement, over-optimistic, or wrong about a constraint, a security
  implication, a scaling wall or a cost, tell me, with the reasoning and a
  better option.
- Suggest better, and suggest adjacent. Propose the simpler or safer approach
  even when it differs from what I asked for, and surface what each decision
  makes cheap. As recommendations with tradeoffs, never as decisions you bake
  in quietly.
- Prefer modern and proven, and de-risk the bleeding edge. Benchmark or spike
  anything load-bearing on the real target BEFORE it becomes a locked decision.
  Measured on the platform, not quoted from a README. Keep the harness.
- Build to last. Assume the worst case per feature. Fail loud, never silently
  corrupt. Version and validate what you persist, and put the volatile part
  behind a seam. Right-sized bans speculative SCALE machinery; it does not ban
  the validation, the version field, the guard or the seam. If I push for a
  brittle-but-short shape, push back.
- Challenge my assumptions now, while they are cheap. Ask questions only where
  a real fork exists, with your recommendation attached, grouped so that
  agreeing to all of it costs me nothing. Do not interrogate me about anything
  you can default.
- Account for every reference I give you as an explicit task. Report which you
  READ and what you took from each, and which you COULD NOT reach and why.
  Never imply you absorbed one you did not. A reference you could not reach
  that matters is a blocker to raise, not a gap to hide.
- Tell me up front what your environment can and cannot do: run the build, run
  the tests, drive the real thing, deploy, reach the network. If a gate the
  plan needs a capability you lack, REQUIRE the setup. Never skip a gate and
  never report one you did not run.
- Never ask me for the value of a secret. Name what you need and tell me where
  it goes.

WHAT I EXPECT BACK, IN THIS SESSION

1. The probe's report, and anything it contradicts.
2. The decisions you defaulted, as a list I can object to.
3. What you kept from the template and what you deleted.
4. A machine checklist: every tool, its minimum version, and the one-line
   command to check it. That is my first task.
5. Your own capability report.
6. The locked decisions, the architecture, and the work plan, for my approval.
7. On approval: the skeleton written, the first unit of work planned, and its
   kickoff prompt printed in chat.

Then stop.
```
