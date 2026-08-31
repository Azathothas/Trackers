# MAINTAIN.md

**For the operator.** Paste the block below into a fresh session pointed at a
clone of this template, whenever it needs improving: a rule that turned out
wrong, a check that needs writing, a lesson from a project that started here.

⚠ **A change here lands in every project started afterwards and in none of the
ones started before.** There is no migration path. That asymmetry is the whole
difficulty of maintaining a template, and it is why this prompt spends most of
its length on restraint.

---

```text
Read, IN FULL, before anything else. Do not skim, do not grep, and do not work
from a previous session's memory.

- [ ] ./AGENTS.md, and its "Maintaining the template" section in particular
- [ ] ./docs/README.md, the map of which document answers what
- [ ] ./docs/conventions/prose.md, because you will be writing here
- [ ] ./scripts/README.md, the contract every check follows

⛔ ABORT AND SAY SO if you cannot locate one.

You are MAINTAINING this template. It is not a project and it holds no project
code. Everything you write here is inherited, unreviewed, by every repository
started from it afterwards.

FIRST, ESTABLISH THE BASELINE. Run the whole gate and report what it says,
before changing anything:

    sh scripts/common/check-gate.sh
    pwsh -NoProfile -File scripts/common/check-gate.ps1 -Fast
    sh scripts/doctor/doctor.sh --fast
    pwsh -NoProfile -File scripts/doctor/doctor.ps1 -Fast

check-gate delegates to every check and reads each exit code unpiped. It prints
a row per check and one verdict. ⚠ It is slow because check-twins is: pass
--fast to skip that one and nothing else, then run it once on its own.

⭐ check-twins.sh runs BOTH halves of every pair and compares them, so it is
the one that catches a fix applied to only one implementation. Every check in
scripts/common/ has a PowerShell twin.

Read each exit code from the process that produced it, unpiped. If any is
already red, that is the first finding and it comes before my request.

⛔ A SKIP IS NOT A PASS, and check-gate says so on its own line. A row reading
SKIP means that check did not run and nothing about its subject was verified.

⚠ check-changelog exits 2 here, and 2 is "could not run", not "failed": this
repository has no CHANGELOG.md of its own. A project that starts from it and
adds one gets the real check.

WHAT I WANT CHANGED:

<describe it. A rule that was wrong. A check that is missing. A trap a project
found. A document that contradicts another. Leave blank and ask me if you are
here to review rather than to change something specific.>

HOW TO WORK IT

- ⛔ Nothing goes in because it might be useful. Every file a project does not
  need is a file its bootstrap has to recognise and delete, and a wrong
  inclusion is paid for once per project forever. A rule earns its place by
  naming the defect it prevents.
- ⛔ Every rule says what it cost to learn. A rule with no incident behind it is
  a preference, and a preference stated as a rule is what makes an agent stop
  believing the rules that matter. If you cannot say what it cost, say that,
  and let me decide whether it still goes in.
- ⛔ Amend in place. When a rule changes, rewrite the rule. Do not stack a dated
  box under the old text: an agent reads the first paragraph of such a box,
  stops, and acts on the retired rule. That has happened. Move superseded
  wording to a history file and link it once.
- ⭐ A rule that can be checked should be a CHECK, not a paragraph. A rule
  enforced by a script is a rule nobody has to remember.
- ⭐ Mutation-prove anything you add or change in a check. Plant the defect it
  exists to catch, run it, read the exit code unpiped. A guard that has never
  been seen to refuse is a guard nobody knows works. This has already caught a
  real corruption in this repository's own licence filler.
- ⛔ If you add a check, add BOTH halves and add the pair to
  scripts/common/check-twins.sh in the same change. A twin nobody compares is
  how drift starts, and a sh-only check does not run on a native Windows
  session: no sed, and sort is an alias for Sort-Object.
- ⚠ Measure on this machine and SAY WHICH MACHINE. This repository makes claims
  about hosts it cannot see. A number carries its conditions or it is not a
  number.
- ⛔ This repository is PUBLIC. Nothing naming a real host, account, domain,
  path, credential file or private project goes in it. Use example.com and
  OWNER/REPO in examples.

CONSISTENCY IS A DELIVERABLE HERE, NOT A NICETY.

Before you finish, check that what you wrote does not contradict what was
already here. The documents cross-reference heavily and a rule changed in one
place and not the other is worse than the original problem, because now a
reader has two answers and no way to tell which is live.

WHAT I EXPECT BACK

  1. The baseline, before and after, from every check.
  2. What you changed, and the defect each change prevents.
  3. Three deep reviews, three DIFFERENT questions, not one sweep written up
     three times. At minimum: what else does this touch; can the guard I
     changed actually fail; which sentence of mine is not backed by an artefact.
     ⚠ A pass with no findings means that pass was too shallow: say what it
     swept and what would have made it fire.
  4. Anything you found that I did not ask about, as a finding for me to rule
     on rather than a change you made.
  5. ⛔ The working tree clean, and every check green.

Do not push. Commit locally and tell me what you would have pushed.
```

---

## The recurring jobs

Things worth pasting the prompt for, on a cadence rather than on a problem:

| when | what to ask for |
| --- | --- |
| after finishing a project that started here | "What did this project have to work around? Anything a future one should inherit?" |
| ⭐ a dependency bot has opened something | "Verify it before applying it: does the commit exist in that repo, does the tag comment match the pin, and what runtime does the pin declare?" |
| ⭐ when a check has never fired | "Mutation-prove every check. One that has never been seen to refuse is one nobody knows works." |
| when the CI has been green for a long time | "Is it green because everything passes, or because a step stopped running?" |
| after a language or platform moves | "Re-measure the probe's runtimes and re-resolve the pinned action commits." |
| when a document has grown | "What in here is now restated somewhere else? One fact, one home." |
| yearly, at least | "Re-fetch the licence texts from SPDX and diff them against what is committed." |

## What not to ask for

⚠ **Do not ask for a rewrite of something that is working.** The value of this
repository is that its rules were paid for. A rewrite that reads better and
remembers less is a net loss, and it is the most likely thing a capable agent
will offer to do.
