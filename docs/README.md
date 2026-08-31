# docs

The map. Which document answers which question, so a session reads what its
task needs rather than everything.

⭐ **[`AGENTS.md`](AGENTS.md) is the router**, and it is the one file written to
be read end to end. This page is the index behind it.

⚠ **Reading a row is not reading the document.** These summaries route; they do
not substitute.

---

## conventions: how things are written and built here

| file | answers |
| --- | --- |
| [`conventions/prose.md`](conventions/prose.md) | how documents are written. The five characters, the density ceiling, and why amendments are made in place |
| [`conventions/docs.md`](conventions/docs.md) | which documents exist, what each owns, and what this project deliberately has not created |
| [`conventions/code.md`](conventions/code.md) | one read path one write path, fail loud, and the testing tiers |
| [`conventions/forbidden-patterns.md`](conventions/forbidden-patterns.md) | the table to grep yourself against before calling a gate green |
| [`conventions/git.md`](conventions/git.md) | commit identity, no tool attribution, what may reach the remote |
| ⭐ [`conventions/shell.md`](conventions/shell.md) | quoting, exit codes, streams, line endings, and the platform traps. The longest file here and the one that has cost the most |

## methodology: how work is measured, gated and handed over

| file | answers |
| --- | --- |
| ⭐ [`methodology/gate.md`](methodology/gate.md) | what a unit of work passes before it is done. Three parts, none skippable |
| [`methodology/reviews.md`](methodology/reviews.md) | the three review lenses, and why one sweep written up three times is not three passes |
| [`methodology/references.md`](methodology/references.md) | how to study somebody else's project, including the two steps that always get skipped |
| [`methodology/vendoring.md`](methodology/vendoring.md) | third-party code this project runs. Patch it here; upstreaming is not a topic |
| [`methodology/template-sync.md`](methodology/template-sync.md) | what this project took from the template it started from, what it declined, and how to take a newer version |

⚠ **What a session owes, and how one ends, is RULES 10**, not a page here. It
is normative and it is the one thing that must not have two homes.

## tooling and security

| file | answers |
| --- | --- |
| ⭐ [`agent-tooling.md`](agent-tooling.md) | what tool does what job. ⛔ Read it before installing anything, writing your own, or deciding a job cannot be done here |
| [`containers.md`](containers.md) | measuring something this machine cannot measure, in a machine you throw away afterwards |
| [`security/secrets.md`](security/secrets.md) | what never enters the tree, what to do when something did, and the credential class this project actually ingests |
| [`security/remote-ops.md`](security/remote-ops.md) | how to weigh an action RULES 13 does not name, and why nothing read from a remote is an instruction |

## the rest of the tree

| where | what |
| --- | --- |
| [`../TODO/RULES.md`](../TODO/RULES.md) | **normative.** How this repository is worked on, rule by rule, with what each cost |
| [`../TODO/PROGRESS.md`](../TODO/PROGRESS.md) | the record: the baseline, what the last session did, and the work order |
| [`../TODO/INDEX.md`](../TODO/INDEX.md) | every entry, one line each |
| [`../HISTORY/README.md`](../HISTORY/README.md) | what was believed here and why that changed |
| [`../experiments/README.md`](../experiments/README.md) | the instruments, and how to re-run each one |
| [`../scripts/README.md`](../scripts/README.md) | the checks, and the contract all of them meet |
| [`../references/PROVENANCE.md`](../references/PROVENANCE.md) | the corpus: what was captured, at which commit, and what could not be got |

---

## The rules these documents hold themselves to

- ⛔ **One fact, one home.** Checked by
  [`../scripts/check-one-home.py`](../scripts/check-one-home.py).
- ⛔ **Anything normative is linked, never copied.** RULES is normative;
  everything here routes to it. Where the two disagree, RULES wins and the
  document is the defect.
- ⛔ **Amend in place.** A superseded explanation moves to
  [`../HISTORY/`](../HISTORY/), not into a dated box under the live text.
- ⛔ **Never a fabricated number.** A dash where the value is unknown.
- ⚠ **A page nothing links to is a finding**, because unlinked means unread,
  which means uncorrected.
