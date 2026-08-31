# ingest.md

How to take over, or join work on, a project that already exists.

[`initialize.md`](initialize.md) is the companion and everything in it applies
here unchanged: the mindset, the collaboration model, the gate, the testing
philosophy, living documentation, handoffs. **This document is the front half**,
how to build an accurate picture before touching anything, plus the rules
specific to inheriting somebody else's decisions.

The project may be complete, half-finished, abandoned, undocumented,
inaccurately documented, subtly broken, or an experiment nobody finished.
⛔ **Assume nothing about which.**

---

## 0. The cardinal rule

⛔ **Documentation is helpful context, not truth. Verify everything
independently.**

Existing documents, READMEs, comments, commit messages, and the operator's own
description are all **claims**. Some are true, some were true once, some were
never true. Treat them as a map drawn by someone who may have taken a wrong
turn: useful for orienting, dangerous to trust.

The failure this prevents is **building on a state you assumed instead of one
you verified.** A document says the build works; it does not. A comment says a
function is the entry point; it is dead code. A README states a security
parameter that the platform silently caps far below, and the feature has been
broken in production for months. Every one of these is invisible until you run
the thing and look.

⭐ **Your first deliverable is an understanding, not a change.**

---

## 1. Lay of the land

Survey, in roughly this order, and write the map as you go:

- **Repository structure.** The layout, where code lives, where tests live,
  what is generated and what is hand-written.
- **Documentation.** Every README, doc directory, design note. Read them as
  claims, and ⭐ **note where they disagree with each other**, which is a common
  tell.
- **Architecture.** Entry points, the layers and their dependencies, the data
  model, the state machines, the core algorithms, the seams.
- **Dependencies**, with the **actual versions in use** pinned, and current
  APIs verified from the installed packages rather than from memory.
- **Build.** How it builds, and whether a **fresh checkout** builds clean. Do
  the literal fresh-clone test: a build that works only in the author's dirty
  working tree is a broken build.
- **Deployment.** Where, how, with what configuration and secrets.
- **Testing.** What exists, how it runs, whether it is green *now*.
- **CI.** What runs, what gates a merge, what has been failing.
- **Security.** How authentication works, where secrets live, what the trust
  boundaries are, whether a threat model is written down at all.
- **Debt.** The TODOs, the type-checker suppressions, the stubs, the
  commented-out code, the places that smell rushed.

⭐ **Writing the map forces you to notice what you do not actually understand
yet.** That is the point of writing it rather than holding it.

### Ask for the artefacts only a tool or the operator can produce

After a first pass, ask for whatever would sharpen the map. This is expected,
not an admission of weakness: a call graph or dependency graph, coverage or
profiling reports, a schema dump, CI logs, a dependency manifest, or read-only
access to a staging or production environment.

Say what each would let you verify, so the operator can judge whether it is
worth producing. ⚠ **Treat whatever comes back as another claim to check
against the code.** A generated graph can be stale too.

---

## 2. Verify reality. Run it, do not just read it

Reading tells you what the project claims. Running tells you what it does.

- **Build it.**
- **Run the tests.** Green *now*? ⛔ How many test files does the runner report
  against how many are on disk? What do the failures actually say?
- **Run the application.** Start it, drive it, exercise the primary workflows
  as a user would.
- ⭐ **Validate documented behaviour.** Take each important claim and check it
  against the running system. **This is where most of your early findings come
  from.**
- **Inspect runtime behaviour.** Logs, network, database, real state. Not the
  code's intentions; its effects.

⛔ **When something cannot be verified, say so explicitly.** "I could not verify
X because Y" is a first-class output. What is not acceptable is quietly
treating an unverified claim as a fact and building on it.

⚠ **The runtime you test in may not enforce every policy the deployed platform
does.** If there is an environment you are authorised to inspect read-only, a
pass against it catches the class of "green locally, broken in production" that
no amount of local testing can.

⛔ **If your harness cannot build, run or drive the project at all, you cannot
verify reality, and verifying reality is the entire job of this phase.**
Surface the gap and require setup. An unrunnable project honestly mapped from
source, plus a note saying you could not execute it and what you need, is worth
far more than a confident map that quietly never ran.

---

## 3. Documentation first, before behaviour

```
understand -> verify -> present findings -> discuss -> correct the docs
  -> fill what is missing -> improve the tests -> THEN change behaviour
```

⛔ **Avoid changing production behaviour before the documentation reflects
reality.** Two reasons, both load-bearing:

1. ⭐ **Writing the documentation is the audit.** Expect the pass to *generate*
   the findings list. That is the feature.
2. **You cannot safely change what you have not accurately described.** A
   change built on a wrong mental model breaks something you did not know was
   connected.

So correct the documents as you verify, in the same pass. Where documentation
is missing, write it. Where a claim was wrong, fix it and note what it said.
Where tests are missing for behaviour you are about to change, add them first.

---

## 4. Present the findings, then stop

After the understand, verify and document passes you will have a list: defects,
wrong documents, dead code, security concerns, missing tests, opportunities.

⛔ **Do not start fixing them.** Present them first, ranked by consequence, and
let the operator decide.

For each finding, give them what they need to decide **without redoing your
investigation**:

| field | what it must contain |
| --- | --- |
| **What** | the symptom, in one sentence, with file and line |
| **Why it matters** | the concrete consequence. Who gets hurt and how. ⚠ "Untidy" is not a consequence. |
| **Severity** | and the honest reason for it |
| **Proposed fix** | what you would do, and what it costs |
| **The alternative** | including "do nothing, and document it as an accepted limitation" |

Then ⛔ **stop and let them pick.** "No, leave it" is a legitimate and final
answer for any item.

⭐ **Rank by consequence.** Ten findings the operator can decide beat fifty they
must triage, and a finding with no consequence is noise hiding the ones with
one.

⚠ **This gate matters most precisely when you are eager to fix things.** On an
inherited codebase, a confident unrequested fix is how you break a behaviour
somebody depended on for a reason you did not know.

---

## 5. Read the architecture for what it unlocks

An existing architecture has latent capability, things that are cheap now
because of what is already built. As you plan any change, look for:

- **Reusable abstractions** a new feature should extend rather than duplicate.
- **Shared infrastructure** a new capability can ride instead of reinventing.
- ⭐ **Seams built for extension.** An interface with one implementation is an
  invitation to add the second cheaply. The worked example: a storage driver
  interface built for one backend made a whole class of new backends "a driver
  plus a configuration row" instead of an engine rewrite. The value was
  recognising the seam was already there.

Surface these during planning as optional recommendations, so the operator can
decide whether to spend the usually small extra effort.

---

## 6. Inherited decisions

**Respect existing architectural decisions the way you would respect your own
locked ones.** Do not re-litigate on a whim.

⛔ But if one is genuinely wrong or unbuildable, that is a finding to **raise**,
with reasoning and an alternative, not a pivot to make silently.

⭐ **Write the existing decisions down as locked decisions if nobody has**, so
future work stops re-opening them.

⚠ **On an inherited codebase, build-to-last bites hardest.** A brittle
positional parser or an unversioned format **you did not write** is exactly the
thing to make fail-loud and pluggable rather than extend in place. And a modern
replacement for a legacy dependency earns a benchmark before you commit the
migration. ⛔ Never a rewrite that trades the old code's ugly working safety for
fewer, prettier, more fragile lines.
[`../conventions/code.md`](../conventions/code.md).

---

## 7. Testing an existing project

**If tests exist:** run them and record the real result, with the file count
against disk. Analyse the failures: real regressions, environment problems, or
flakes? ⚠ A test that passes alone and fails in company is a harness problem;
diagnose it rather than re-running it.

⭐ **Audit the tests themselves, not just their pass or fail.** Is every
injectable seam's *production default* tested, or does every test inject the
double? Is any guard theatre? Mutation-prove the critical ones.

**If tests are thin or absent:** recommend a strategy for the project's shape
and risk. Not a coverage number; the tiers of trust in
[`../conventions/code.md`](../conventions/code.md), weighted to where the risk
actually is. Implement tests where warranted, especially for behaviour you are
about to change and for every defect you find.

---

## 8. Then work it like any other project

Once the understanding is shared and the documents reflect reality, everything
runs on the full method. Nothing is different because the project was
inherited: planning first, approval gates, the three-part gate on every unit,
the deep reviews, living documentation, handoffs as memory, and every new piece
of work entering through the same authoring flow.

⭐ **The deep reviews pay double on an unfamiliar codebase**, because the
connections you do not yet know are exactly where the one-gated-door defects
hide.

---

## 9. The through-lines

- **Understand before you touch.** The first deliverable is a verified
  understanding.
- **Documents are claims. Verify them.** Build it, run it, drive it. Local is
  not production.
- **Say what you cannot verify.** Unverified is not the same as true.
- **Know your environment and account for your inputs.** If your harness cannot
  run the project, require setup. Do not map from source and call it verified.
- **Writing the documents is the audit.** Expect that pass to surface most of
  your findings.
- **Present the findings and let the operator pick.** Do not fix before the
  nod, especially when you are sure.
- **Read the architecture for what it unlocks.** Extend seams, do not duplicate.
- **Respect inherited decisions like your own**, but a wrong one is a finding.
- **Do not trade working safety for fewer lines.**
