# docs

The map. Which document answers which question, so a session reads what its
task needs rather than everything.

⭐ **Read the row, then read the document.** Reading the row is not reading the
rule: these summaries exist to route, not to substitute.

---

## methodology: how work is planned, gated and handed over

| file | answers |
| --- | --- |
| [`initialize.md`](methodology/initialize.md) | how to start a project that does not exist yet. The mindset, the phases, the approval gates. |
| [`ingest.md`](methodology/ingest.md) | how to take over one that already exists. Verify before you touch. |
| ⭐ [`gate.md`](methodology/gate.md) | what a unit of work passes before it is done. Three parts, none skippable. |
| ⭐ [`reviews.md`](methodology/reviews.md) | the three review lenses, and why one sweep written up three times is not three passes. |
| ⭐ [`sessions.md`](methodology/sessions.md) | what a session owes at its start and its end, how to resume one, how to freeze cleanly. |
| [`authoring.md`](methodology/authoring.md) | how a rough idea becomes an approved unit of work. Authoring and implementing are different sessions. |
| [`choosing-a-work-model.md`](methodology/choosing-a-work-model.md) | stage or todo, and the migration between them. Deleted once chosen. |
| [`work-stages.md`](methodology/work-stages.md) | the stage model: numbered units, a plan each, a handoff each. |
| [`work-todo.md`](methodology/work-todo.md) | the todo model: an index, a record, entries that close in place. |
| [`references.md`](methodology/references.md) | how to study somebody else's project, including the two steps that always get skipped. |
| [`experiments.md`](methodology/experiments.md) | taking your own measurements: the script, the conditions, and why a negative result is committed. |
| ⭐ [`vendoring.md`](methodology/vendoring.md) | third-party code in this tree. ⛔ Patch it here; upstreaming is not a topic. |
| ⭐ [`history.md`](methodology/history.md) | where the story goes, so it stops being written into the pages that answer questions. |
| [`template-sync.md`](methodology/template-sync.md) | taking a later version of the template into a project that adopted it already. |
| [`lean-adoption.md`](methodology/lean-adoption.md) | ⭐ taking the engineering without shipping any agent-facing content. |

## conventions: how things are written here

| file | answers |
| --- | --- |
| [`prose.md`](conventions/prose.md) | how documents are written. The three markers, and why amendments are made in place. |
| [`docs.md`](conventions/docs.md) | the document set, one fact one home, and the changelog rules. |
| [`git.md`](conventions/git.md) | commit identity, what may reach a remote, what is never committed. |
| [`code.md`](conventions/code.md) | one read path one write path, build to last, and the testing tiers. |
| ⭐ [`forbidden-patterns.md`](conventions/forbidden-patterns.md) | the table to grep yourself against before declaring a gate green. |
| ⭐ [`shell.md`](conventions/shell.md) | quoting, heredocs, exit codes, streams, line endings, and the platform traps. |

⚠ The entry points that live at the repository root rather than under `docs/`:
⭐ [`ROUTE.md`](../ROUTE.md), the one paste that works out which job a session
is, [`ADOPT.md`](../ADOPT.md) for an existing repository elsewhere, and
[`MAINTAIN.md`](../MAINTAIN.md) for improving the template itself.

## tooling: what to reach for, before you install or invent one

| file | answers |
| --- | --- |
| ⭐ [`agent-tooling.md`](agent-tooling.md) | what tool does what job, and where each one lives. ⛔ Read it before installing anything, writing your own, or deciding a job cannot be done here. |
| [`containers.md`](containers.md) | measuring something this machine cannot measure, in a machine you throw away afterwards. |

## security

| file | answers |
| --- | --- |
| [`secrets.md`](security/secrets.md) | what never enters the tree, and what to do when something did. |
| [`remote-ops.md`](security/remote-ops.md) | the three tiers governing action on anything outside this machine. |

## history: this repository's own superseded wording

| file | answers |
| --- | --- |
| [`history/README.md`](history/README.md) | what was believed here and why that changed, plus the claims this repository has withdrawn. ⛔ A bootstrap deletes it and creates the project's own. |

## visibility: one of these is kept, the other deleted

| file | answers |
| --- | --- |
| [`public/README.md`](public/README.md) | what changes because the repository is, or will be, public. |
| [`private/README.md`](private/README.md) | what changes because it stays private, and what does not relax. |

## templates: the skeletons a new project receives

Everything in [`templates/`](templates/) carries double-brace placeholder
markers and guidance comments. ⛔ **Both are removed when a file is filled in**,
and `scripts/common/check-placeholders.sh` is what proves it.

⛔ **The directory itself is deleted at the end of a bootstrap**, in the same
command as `bootstrap/`. A project that kept it inherited a set of half-written
documents that its next sessions read as the project's own, and the check above
could not see them because its exemption came across with the directory. It is
scanned now the moment `bootstrap/` is gone.

⚠ This paragraph describes the marker rather than showing one, on purpose. A
document that demonstrates the thing a checker looks for makes the checker fire
on correct writing, which is the same class as a page about escape sequences
containing the byte it warns about.

| file | becomes |
| --- | --- |
| ⭐ [`AGENTS.md`](templates/AGENTS.md) | the project's router, carrying the task routing table. Under 300 lines. |
| [`PROGRESS.md`](templates/PROGRESS.md) | the record. The one file every session reads first. |
| ⭐ [`HISTORY.md`](templates/HISTORY.md) | `docs/history/README.md`. The destination for superseded wording, created at bootstrap even though it starts empty. |
| [`INDEX.md`](templates/INDEX.md) | the entry list, in todo mode. |
| [`RULES.md`](templates/RULES.md) | how this repository is worked on, with what each rule cost. |
| [`HUMAN.md`](templates/HUMAN.md) | the operator's side: setup, validation, runbooks, prompts. |
| [`README.md`](templates/README.md) | the front door, for a competent stranger. |
| [`SECURITY.md`](templates/SECURITY.md) | the threat model. Writing it is the audit. |
| [`CHANGELOG.md`](templates/CHANGELOG.md) | what shipped, when, and where the evidence is. |
| [`stage.md`](templates/stage.md) | one unit of work, in stage mode. |
| [`todo-entry.md`](templates/todo-entry.md) | one entry, in todo mode. |
| [`handoff.md`](templates/handoff.md) | the durable memory between sessions, in stage mode. |

---

## The rules these documents hold themselves to

- ⛔ **One fact, one home.** A value in two documents with no check between them
  drifts, and the copy a reader trusts is the wrong one.
- ⛔ **Amend in place.** When a rule changes, the rule is rewritten and the
  superseded wording moves to a history file. Stacking a dated box under retired
  text has a documented failure mode, and
  [`conventions/prose.md`](conventions/prose.md) records what it broke.
- ⛔ **Every claim verified before it is written.** Writing the documentation is
  the audit, and the most confident sentence in a file is regularly the only
  false one.
- ⛔ **Never a fabricated number.** A dash where the value is unknown.
- ⚠ **A page nothing links to is a finding.** Unlinked means unread, which means
  uncorrected, which is the state every stale document passes through.
