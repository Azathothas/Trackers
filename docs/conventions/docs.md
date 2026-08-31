# docs.md

Which documents exist and what each one owns.

[`prose.md`](prose.md) is how they are written. RULES 17 is what a document
owes and is normative. This page is the set.

---

## The set

| file | owns |
| --- | --- |
| [`../AGENTS.md`](../AGENTS.md) | ⭐ the router. What to read for which task. Restates nothing, links everything, and is written to be read in full by a session with no memory. |
| [`../../README.md`](../../README.md) | what this is, for a competent stranger. The state, the limitation, one command, and where to go next. |
| [`../../TODO/PROGRESS.md`](../../TODO/PROGRESS.md) | ⭐ the record. The measured baseline, what the last session did, and the work order. Nothing else carries a work order, and it is rewritten every session. |
| [`../../TODO/RULES.md`](../../TODO/RULES.md) | how this repository is worked on, rule by rule, with what each cost. Normative. |
| [`../../TODO/INDEX.md`](../../TODO/INDEX.md) | every entry, one line each, sorted by priority |
| `../../TODO/<category>.md` | the entries themselves, which close in place |
| [`../README.md`](../README.md) | the documentation map: which page answers which question |
| `conventions/` | how things are written and built here |
| `security/` | what never leaves, and what may be touched outside this machine |
| `methodology/` | how work is measured, gated, reviewed and vendored |
| [`../../experiments/README.md`](../../experiments/README.md) | the instruments: what each one answers and how to re-run it |
| [`../../scripts/README.md`](../../scripts/README.md) | the checks: what defect each exists to catch, and the contract all of them meet |
| [`../../references/PROVENANCE.md`](../../references/PROVENANCE.md) | the corpus: what was captured, at which commit, under which licence, and what could not be got |
| ⭐ [`../../HISTORY/`](../../HISTORY/) | **the story.** Superseded wording, reversed decisions, corrections, reference sweeps, review passes. Everything above says what is true now; this says what was believed and why that changed. |

⛔ **[`../../HISTORY/`](../../HISTORY/) exists so none of the rows above fill up
with narrative.** The instinct to record why a design has its shape is right,
which is why forbidding it does not work. It needed a destination, and
[`../../HISTORY/README.md`](../../HISTORY/README.md) is that directory's own
contract.

⚠ **Create what the project has a use for and nothing else.** A file nobody
selected is a file a future session reads, believes, and follows into a rule
that was never meant to apply.

### What this project has deliberately not created

| | why |
| --- | --- |
| `CHANGELOG.md` | nothing has shipped. A changelog over an unpublished skeleton is a file with one entry saying so, and `check-changelog` semantics treat an absent one as "could not run", which is the honest answer. It arrives with the first published dataset. |
| `SECURITY.md` | there is no deployed system and no credential in the tree. The threat model that does apply is about **hostile upstream input** and lives in RULES 5, where the code that enforces it can cite it. |
| `docs/architecture.md` `(planned)` | `src/trackers/` is nine modules with module docstrings that carry the design, and a second description of them would be the second home this page forbids. T-120 is the entry that revisits it once the shape stops moving. |

---

## The invariants

### One fact, one home

Every fact lives in exactly one document. A count, a commit, a limit, a schema:
one place. Where it must appear twice, derive it or have a check assert the two
agree. [`prose.md`](prose.md) carries the rule and the check.

⚠ The trap is that a value which never changes cannot expose a missing check.
It sits correct for a year and drifts the first time it moves.

### Documentation ships with the code it describes

⛔ The moment code changes a documented behaviour, the document changes with
it, in the same commit. RULES 7 makes the record part of the change for the
same reason.

### Prefer a shape a check can assert

Where a document names a file, a rule, a claim id or an entry id, prefer a form
`python3 scripts/check-citations.py` can verify against the tree, so a rename
fails a gate instead of rotting quietly. That check resolves paths, links,
`RULES n.n`, `C-nn`, `T-nnn`, `D-n`, and line numbers into the corpus.

⭐ The strongest version of it is
[`../../experiments/fixtures/load-bearing-citations.tsv`](../../experiments/fixtures/load-bearing-citations.tsv),
which pins the substring a cited line must still contain. A citation checker
proves a line exists; only that file proves it still says what it was cited
for.

### Every claim is verified before it is written

Writing the documentation is the audit. Being forced to say precisely what
something does, and then checking whether that is true, is where a surprising
share of real defects are found. ⚠ The most confident sentence in a file is
regularly the only false one.

### Say what is not true

Reserve a place for the truths that are tempting to hide. This has a known gap.
This estimate excludes something unmeasurable. This dataset is measured from
one datacenter and not from your connection.

⛔ A limit hidden is a defect filed against a user later.
