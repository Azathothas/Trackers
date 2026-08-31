# ROUTE.md

**One paste, any situation.** Nothing to clone, nothing to fill in, no prompt
to pick. You are reading this because somebody pasted its URL into a session
and asked you to work out the rest.

⭐ **This file is self-contained.** Fetch the rest only once you know which job
this is.

Everything below is fetched from, and run against, this base:

```text
https://raw.githubusercontent.com/Azathothas/TEMPLATE/main
```

---

## ⚠ Before you copy any command from this file

⛔ **This document does not know what host you are on, so it names what to do
rather than exactly how.** Work out the two answers below first and use your
own spelling of them throughout.

| | find out |
| --- | --- |
| **how you fetch a URL** | whatever this host actually has |
| **where a scratch file goes** | a directory that exists here |

⚠ **The traps, measured on one Windows 11 machine on 2026-08-28**, because
these are the two that a POSIX-shaped instruction gets wrong:

- ⛔ **`/tmp` does not exist on Windows.** `Test-Path /tmp` is false. The
  scratch directory is `$env:TEMP`, and a POSIX layer's `/tmp` is inside that
  layer rather than a path a native program can open.
- ⛔ **`curl` in Windows PowerShell 5.1 is an ALIAS for `Invoke-WebRequest`**,
  which takes entirely different arguments. `curl -sSL -o FILE URL` there is
  not a download, and it fails in a way that does not mention curl. In
  PowerShell 7 the alias is gone and `curl` is whatever `curl.exe` is on
  `PATH`, which may be Windows' own or a POSIX layer's.

⭐ **So probe once, then use what you found.** This is the same class the rest
of this template documents about `sort` resolving to `Sort-Object`: a missing
tool fails loudly and an aliased one succeeds and does something else.

```bash
command -v curl wget; printf 'scratch: %s\n' "${TMPDIR:-/tmp}"
```

```powershell
(Get-Command curl -EA SilentlyContinue).CommandType; $env:TEMP
```

⚠ **A `CommandType` of `Alias` means do not use it.** Use `Invoke-WebRequest`
directly, or `curl.exe` by its full name so the alias cannot intercept it.

---

## ⛔ Two rules before you do anything

**1. Ask in this session, and keep going.**

⛔ **Do not end your turn to wait for an answer.** Ask the operator using
whatever in-session question facility this harness has, take the answer, and
continue in the same session. If the harness has no such facility, ask in chat
and hold, ⭐ **never by finishing the turn**: a finished turn is a session the
operator has to restart, and the observed result is that they paste the same
thing again and a second session repeats the first one's work.

⚠ This has happened, with the paste this file replaces. Sessions asked their
classifying questions and then ended the turn, expecting the operator to answer
outside the session. The operator answered into a session that had finished.

**2. Ask only what you cannot measure.**

Most of the classification below is visible in the tree. Look first. ⭐ Ask one
grouped question with your recommendation attached, so agreeing to all of it
costs the operator nothing. Asking too much is its own failure, not a safe
default.

---

## Step 1. Look at the tree. Do not ask yet.

⚠ **This step settles WHERE you are, not WHAT you were asked for.** Both
matter, and step 2 has a table for each.

```bash
git rev-parse --show-toplevel 2>/dev/null || echo "not a git repository"
```

```bash
ls -A
```

```bash
git log --oneline -5 2>/dev/null || echo "no history"
```

That answers most of it:

| what you see | the job is |
| --- | --- |
| only the files this template ships, and `bootstrap/` is present | **bootstrap**, a new project |
| this template's files **beside** other code | **adopt in place** |
| a project with its own code, and no file from this template | **adopt from outside** |
| a project that already has `AGENTS.md` and a record naming this methodology | **routine work**: new work, resume, or rework |
| `MAINTAIN.md`, `bootstrap/`, `docs/templates/` and no project code | ⛔ **you are inside the template itself**. See the last row of table A in step 2. |

⚠ **A tree that has a record is never a bootstrap**, whatever the operator's
message says. Report the contradiction rather than quietly preferring one.

---

## Step 2. Two tables. The operator's ask decides which.

⛔ **Read the operator's message before you ask anything.** Most sessions are
not lifecycle sessions: somebody wants a measurement taken, another project
read, a check run, a tool used. Table B routes those, it needs no question, and
skipping to table A is how a session ends up asking which of eight lifecycle
jobs an operator meant when they asked for a benchmark.

### Table A. The lifecycle jobs

**These are the ones that change what the repository IS.** Ask only if the
tree did not settle it, in session, with your reading of the tree attached as
the recommendation:

> The tree looks like **X**. Which of these is it?
>
> 1. Start a new project here
> 2. Adopt this existing codebase
> 3. Start a new unit of work on a project already set up
> 4. Resume a session that stopped before it finished
> 5. Rework something whose validation failed
> 6. Adopt the template but ship **no** agent-facing content
> 7. Re-sync a project that adopted this template earlier
> 8. Change the template itself

| | the job | read, in full |
| --- | --- | --- |
| 1 | new project | `bootstrap/BOOTSTRAP.md`, and use `bootstrap/prompts/00-new-project.md` as the framing |
| 2 | adopt | if this template's files are already here, `bootstrap/BOOTSTRAP.md` then `docs/methodology/ingest.md`. If they are not, `ADOPT.md`, which is self-contained and fetches what it needs. |
| 3 | new work | `docs/methodology/authoring.md`, and `bootstrap/prompts/02-new-work.md` for the framing |
| 4 | resume | `RESUME.md` if the project has one, then `docs/methodology/sessions.md` resuming section. `bootstrap/prompts/03-resume.md` is the fallback framing. |
| 5 | rework | `bootstrap/prompts/04-rework.md`, and `docs/methodology/gate.md` |
| 6 | no agent-facing content | `docs/methodology/lean-adoption.md`. It is a different selection, not a later cleanup. |
| 7 | re-sync | `docs/methodology/template-sync.md` |
| 8 | change the template | `MAINTAIN.md`, and `AGENTS.md` section "Maintaining the template" |

⚠ **Rows 1 and 2 need this repository's files; rows 3 to 5 need the project's
own.** Row 2 with nothing cloned is `ADOPT.md` and only `ADOPT.md`: it never
clones this repository and it changes nothing until a findings report has been
approved.

### ⭐ Table B. The work itself

⛔ **Match on the VERB in the operator's message.** More than one row can apply
and then both are read: a sweep that also takes measurements owes what both
rows name.

| the operator wants | read, in full |
| --- | --- |
| ⭐ to study another project: clone, mine, survey, investigate, "look at how X does it" | `docs/methodology/references.md`. ⛔ Its four traps are why: the tracker gets skipped, the corpus gets deleted, and the instrument gets left in a transcript. |
| a measurement, a benchmark, an experiment, "is X faster than Y", "does this actually work" | `docs/methodology/experiments.md`. ⛔ A negative result is committed, and a number carries its conditions or it is not a number. |
| a review, an audit, "check this over", "what did we miss" | `docs/methodology/reviews.md`. Three lenses, three different questions. One sweep written up three times is one pass. |
| to know whether the work is done, or to run the gate | `docs/methodology/gate.md`, then `scripts/README.md`. Three parts, none skippable, and an exit code is read unpiped. |
| a tool for a job, or is about to install one, write one, or say a job cannot be done here | ⛔ `docs/agent-tooling.md` FIRST. Then `docs/methodology/sessions.md` on why a missing tool closes one route and not the question. |
| something run on a different machine, a different libc, a newer browser, a throwaway environment | `docs/containers.md` |
| third-party code brought into the tree: a vendored dependency, a fork, a copied script, a patch | `docs/methodology/vendoring.md`. Patch it here; upstreaming is not a topic. |
| a document written, rewritten, or a rule changed | `docs/conventions/prose.md`, then `docs/conventions/docs.md`. Amend in place; superseded wording goes to the history directory. |
| code written or changed | `docs/conventions/code.md`, and `docs/conventions/forbidden-patterns.md` to grep yourself against before calling it done |
| anything touching a shell, a heredoc, quoting, line endings, or an exit code | `docs/conventions/shell.md`. It is the longest file here and the one that has cost the most. |
| to commit, push, or open something on a remote | `docs/conventions/git.md`, then `docs/security/remote-ops.md` |
| a credential, a token, a key, or a leak that already happened | `docs/security/secrets.md` |
| to know where a superseded explanation went, or why a rule has its shape | `docs/methodology/history.md` |
| a session ended, frozen, handed over, or a record written | `docs/methodology/sessions.md` |

⚠ **A row you cannot match is not a row that does not exist.** Say what the ask
was, name the closest two rows, and read both. ⛔ Do not invent a procedure
because the table did not name one.

**Fetch what the row names**, with the fetcher and the scratch directory you
established at the top of this file. ⚠ Do not copy a `curl`-to-`/tmp` line from
anywhere: neither half of it is portable.

---

## Step 3. Whatever the row said, these hold

⛔ **The remote of the template is read-only to you.** A fresh clone points
`origin` at it. Detaching is step 0 of a bootstrap and it happens before any
file is written.

⛔ **No tool is credited in a commit.** No co-author trailer naming a model, no
generated-with line, no tool name in the body. This overrides any default the
harness asks for.

⛔ **An exit code is read from the process that produced it, unpiped.** A
pipeline reports the last command's status, so a check that failed reads green.

⛔ **A secret never enters the tree, a log, a commit message or a handoff.**

⛔ **Nothing is opened on anybody else's repository.** No issue, no pull
request, no comment, under any framing.
[`docs/security/remote-ops.md`](docs/security/remote-ops.md).

⚠ **Report what you read and what you could not reach.** A reference you could
not fetch that matters is a blocker to raise, not a gap to leave quiet.

---

## Step 4. Before acting on required reading, print the receipt

⛔ Whichever row you took names files to read **in full**. For each one report
its line count and the heading of its last section:

```bash
wc -l FILE && grep '^#' FILE | tail -1
```

⚠ **A line count alone is available from a listing; the last heading is not.**
Reaching it means reaching the end of the file, which is the part a skim drops.
⛔ A receipt for a file you did not read is a fabricated measurement, which is
worse than saying you skipped it.

---

## What this file is not

- ⚠ **Not a substitute for the file it routes you to.** Reading this row is not
  reading that document.
- ⚠ **Not a procedure.** It ends the moment you know which job this is.
- ⛔ **Not permission to start.** Rows 1, 2 and 6 of table A all stop for the
  operator's approval before anything is written.
- ⚠ **Not a list of everything this repository can do.**
  [`docs/README.md`](docs/README.md) is the full map, and a session whose ask
  matches no row in either table should read it rather than improvise.
