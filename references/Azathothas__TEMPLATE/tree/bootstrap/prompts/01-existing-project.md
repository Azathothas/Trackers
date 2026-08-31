# Adopt an existing project

Paste the block below into a fresh agent session pointed at a codebase that
already exists, with this template's files copied in beside it.

Fill in what you know and leave the rest blank. The agent discovers most of it
by reading and running the project, and it verifies whatever you tell it rather
than trusting it.

Works for a codebase that is complete, half-finished, abandoned, undocumented
or subtly broken. You do not need to know which.

---

```text
Read, IN FULL, before anything else. Do not skim and do not grep.

- [ ] ./AGENTS.md
- [ ] ./bootstrap/BOOTSTRAP.md
- [ ] ./docs/methodology/ingest.md
- [ ] ./scripts/doctor/README.md

⛔ ABORT AND SAY SO if you cannot locate one.

You are ADOPTING an EXISTING project into this methodology. Follow ingest.md.

⭐ YOUR FIRST DELIVERABLE IS A VERIFIED UNDERSTANDING, NOT A CHANGE. Do not
change production behaviour until you have built the map, verified it against
the running system, corrected the documents to match reality, and shown me a
ranked findings list I have signed off on.

⛔ Treat all existing documentation, every comment, and my description below as
CLAIMS TO VERIFY, not as facts.

=====================================================================
PROJECT
What it is:          <one line is fine>
Current state:       <complete | partial | abandoned | experimental | broken |
                      unknown. Blank = you determine it by running it.>
Why I am bringing you in: <ship it? fix it? extend it? understand it?>
Known problems:      <"it fails when X" is the most useful thing here>
Technologies:        <as far as you know. You will pin the ACTUAL versions.>
Constraints:         <what must NOT break, what must NOT change, compatibility
                      I have to keep>
Priorities:          <what matters most, in order. Blank = you propose an
                      order with reasoning.>
Existing docs:       <where, and how much you trust them. "The README is
                      stale" is useful: expect to find drift and correct it.>
Existing tests:      <where, whether they pass, what you think they cover.
                      Blank = you run them and report the real state.>
Deployment:          <where and how it ships. Note any environment I authorise
                      you to inspect READ-ONLY: a local runtime does not
                      enforce every platform policy.>
References:          <related repos, design notes, tickets, anything that
                      explains WHY the code is the way it is>
What I cannot give you yet: <credentials, environments, services you cannot
                      reach. You will mark anything you cannot verify as
                      unverified rather than assuming it.>
=====================================================================

HOW I WANT YOU TO WORK

- Senior engineer, not a code generator. NEVER a yes-machine. If an existing
  decision is wrong, unbuildable, or a security hole, TELL ME with reasoning
  and an alternative. That is a finding to RAISE, not a pivot to make quietly.
- Lay of the land first, then VERIFY REALITY. Build it, run the tests with the
  file count against disk, run and DRIVE the application, and check each
  documented behaviour against what the running system actually does.
- Anything you CANNOT verify, say so explicitly. Never treat an unverified
  claim as a fact.
- Capability check up front: can you build, run, drive, deploy, reach the
  network. ⛔ If you cannot run this project, REQUIRE the setup. Do not
  substitute reading for running and call it verification.
- Ask me for the artefacts that sharpen the map: a call or dependency graph, a
  coverage report, a schema dump, CI logs, read-only access to a real
  environment. Treat whatever I hand back as another claim to check, not as
  ground truth.
- Account for every reference I give you as an explicit task. Report which you
  read and what you took from each, and which you could not reach and why.
- Documentation first: correct the documents to match reality BEFORE changing
  behaviour. Writing them is the audit, and it is where most of the findings
  will come from. Expect that.
- Respect the previous team's decisions the way you would respect your own
  locked ones. But write them down as locked decisions if nobody has, so future
  work stops re-opening them.
- ⛔ PRESENT A RANKED FINDINGS LIST AND STOP. Per finding: what, with file and
  line; why it matters, as a concrete consequence, not "untidy"; severity, with
  the honest reason; the fix and what it costs; and the alternative, including
  "do nothing and document it as an accepted limitation". Rank by consequence.
  I pick what gets acted on, and "leave it" is a complete answer.
- Do not fix anything before I confirm, especially when you are sure.

WHAT I EXPECT BACK, IN THIS SESSION

A verified map of the project, with the documents corrected to match reality,
and a ranked findings list for my sign-off. Change no production behaviour
until I approve what to act on. Then plan the agreed work and print the first
kickoff in chat.
```
