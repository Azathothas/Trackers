# Rules

How this repository is worked on. `TODO/` is the authoritative record and this
file is the part of it that does not change from session to session.

Read [PROGRESS.md](PROGRESS.md) first: what the last session did, the measured
baseline, and the work order under "Start here next session".
Read [INDEX.md](INDEX.md) for every entry, one line each, and for how an
ordering is derived.

---

## 1. Starting a session

1. Read [PROGRESS.md](PROGRESS.md). It says what the last session did, what is
   in progress, and where to resume. It carries no history.
2. The work order is `PROGRESS.md`'s "Start here next session", not
   [INDEX.md](INDEX.md)'s. INDEX carries the entry list and the argument that
   produced the last ordering; read that when re-deriving one, and say in
   `PROGRESS.md` that you did.
3. Record the start instant on `PROGRESS.md`'s state line, in ISO 8601 UTC.
   Everything at the end that measures the session reads it from there.

```bash
date -u +"%Y-%m-%dT%H:%M:%SZ"
```

4. Rewrite `PROGRESS.md` to say what **this** session is going to do, before
   doing it. Name the entry ids and the files.
5. Re-measure the baseline rather than trusting a recorded one. One command,
   not four: it kills stray release processes first, filters test failures by
   test name rather than by the summary line, and prints one verdict.

```bash
pwsh -NoProfile -File scripts/gates.ps1
```

```bash
pwsh -NoProfile -File scripts/check-todo.ps1
```

```bash
gh run list --limit 1
```

6. If the session touches `vendor/`, one more, and it goes first because it is
   the one that says whether a reconciliation is due:

```bash
pwsh -NoProfile -File scripts/vendor-status.ps1
```

## 2. Ending a session

When the operator says the session is ending, in this order:

1. Finish or checkpoint the current task. A half-finished change is recorded in
   `PROGRESS.md` as partial, with the entry id and the file, never left silent.
2. Update `PROGRESS.md`. It is the only thing the next session is told to read,
   so it carries everything the kickoff prompt does not, and section 3 says the
   kickoff carries almost nothing. It must hold:

   - **The state line**: when this session ran, in ISO 8601 UTC, and whether it
     was attended.
   - **The measured baseline**: tests passing, the gates, and the CI run id and
     the commit it describes. Named by id, never as "the latest".
   - **The entry counts**, which `scripts/session-report.ps1` prints.
   - **What this session did**, in a few lines per entry, and every premise a
     measurement disproved.
   - **What is in progress**, with the `TODO/<file>` and the entry id.
   - **Start here next session**: the ordered list, with the entry ids, the
     files, and the corpus sources for each by repository and path. This is the
     work order. It used to live in the kickoff prompt and it belongs here.
   - **Open questions for the operator**, or that there are none.
3. Update the affected `TODO/` entries and [INDEX.md](INDEX.md), including the
   counts table, which must be exact against the rows. **`patches/TASKS.md` is
   in this step too**, and so is `patches/UPSTREAM.md` when the session touched
   `vendor/`. Section 5 says why under "The record is part of the change".

3a. **Update `docs/` in the same push**, when the session changed what the tool
   does. A behaviour that only a `TODO/` entry describes is a behaviour nobody
   using the tool can find.

   - [`docs/AGENTS.md`](../docs/AGENTS.md) whenever the tree layout, a tool, a
     gate or the reading order moved. It is the first thing an agent reads
     after this file, so a stale one costs every later session.
   - [`docs/examples/`](../docs/examples/) whenever a flag or a command an
     example uses changed, or a new capability wants a worked case. **Every
     command in an example is run before it is written down.** An example that
     has not been executed is a guess.
   - The topic page that owns the behaviour. `docs/` says what the tool does
     and never what the project did: a fixed defect belongs in `docs/` only
     when a reader needs it to use the tool correctly.

```bash
pwsh -NoProfile -File scripts/check-docs.ps1
```

   That resolves every relative link and anchor, every `scripts/` path, and
   every flag and command an example names against `man/bit-cli.json`, and it
   enforces the mechanical half of the prose rule. It is a gate and a CI job,
   so a doc naming a flag that was renamed cannot reach a commit.
4. **Two deep reviews, and the machine does the half it can.**

```bash
pwsh -NoProfile -File scripts/check-todo.ps1
```

   That answers four of the questions review 2 asks: a status in `INDEX.md`
   that disagrees with the entry's own `Status:` line, an entry id that names
   nothing, a cited path that does not resolve, counts that no longer add up.
   It is one second and it has already caught two things that had been wrong
   for a session.

   What it does not answer is whether a claim is **true**, and that is review
   1: every claim written this session against the code or the path it cites.
   Doing the mechanical half in one second is what leaves the time for the half
   that needs reading. Review 2 is then a cold read of the prose, as if someone
   else wrote it.
5. Commit and push with `scripts/git-sync.ps1`. Nothing else. Pass `-Summary
   -Since <the start instant>` on the last push of the session, so the numbers
   in `PROGRESS.md` are measured rather than counted by hand.

   A session that touched `vendor/` also has to leave these two clean, and
   neither is covered by the gates:

```bash
pwsh -NoProfile -File scripts/vendor-status.ps1
```

```bash
pwsh -NoProfile -File scripts/check-prompts.ps1
```
6. Read the CI run the push started. A push that leaves CI red without an entry
   naming why is not finished. A push carrying only documentation should carry
   `-NoCi` and then there is no run to read.
7. **Print the session summary in chat, as a Markdown table**, brief enough to
   read at a glance and no longer than the numbers need. It is the operator's
   view of the session and it is chat only: `PROGRESS.md` is the record and
   this is the receipt. `scripts/session-report.ps1` produces every figure in
   it, so nothing here is counted by hand.

   The rows that always appear, because they are the ones asked for:

   | row | from |
   | --- | --- |
   | Elapsed | the start instant on `PROGRESS.md`'s state line, to now |
   | Commits | `git log` over the session's range |
   | Entries | done out of workable, and what closed or advanced |
   | LOC this session | files changed, lines added and removed |
   | LOC in tree | `scc --no-cocomo crates/` |
   | Tests | the gates' count, and what it was at the start |
   | CI | the run id and the commit it covers |

   Anything the session found that a table cannot hold goes under it in a
   sentence or two, not in the table.
8. Print the kickoff prompt in chat only, in a code block. Section 3 says what
   it must contain.

## 3. The kickoff prompt

Printed in chat, in a code block, at the end of the session.

**Three of them are kept as samples**, at `reference/PROMPT-SAMPLEs/`, on the
`references` branch: `PROMPT_TASK.md` for ordinary work, `PROMPT_VENDOR.md`
for the vendored trees, and `PROMPT_MISC.md` for a one-off, which is the only
one with blanks to fill. They are samples rather than the source of truth: what
goes to chat at the end of a session is written for that session, and a sample
is what it is written from. `scripts/check-prompts.ps1` checks that every path
and script they name still resolves, because a prompt is the first thing a
session reads and nothing else would catch a renamed script there.

**It is generic and it stays generic.** Everything that changes from session to
session lives in `PROGRESS.md`, which is tracked, versioned, and read first
anyway. A prompt that restates the work order is a second copy of it that goes
stale the moment an entry closes, and it costs the next session's context to
read something it is about to read again.

So the prompt carries only what a reader cannot get from the repository:

- The one-line statement of what `bit-cli` is and why it exists, because the
  next agent has to know that before it opens anything.
- What to read, in order, and nothing about what is in it.
- Whether the session is attended or unattended, and what to do when blocked.
- The one-line restore command for the corpus, because a fresh machine cannot
  read `RULES.md` section 7 out of a directory it does not have.

It carries **no** entry ids, no counts, no test numbers, no CI run id, and no
work order. Those are `PROGRESS.md`'s, which is where they are already correct.

The kickoff is therefore the same text every session, and the only thing that
makes one session different from the next is what `PROGRESS.md` says. That is
the point: the prompt is a pointer, and the record is the record.

`PROGRESS.md` has to hold up its end. Section 2 step 2 says what it must carry,
and the template at the top of the file says it again where a session will
see it.

The kickoff is the last thing printed, after the summary table in section 2
step 7. Two things go to chat at the end of a session and only two: the table,
which is the receipt, and this, which is the pointer.

## 4. Git

**`scripts/git-sync.ps1` is the only sanctioned way to commit and push.** It
enforces the rules below mechanically, so they stop being things to remember.

**Write the commit body to a file and pass `-BodyFile`.** Never type a body
into the shell. A body typed into `bash` as a PowerShell here-string ends at the
first apostrophe, and everything after it becomes shell commands: "the run's own
deadline" is enough to do it. That has cost two failed pushes and it will cost
more, because the failure is silent until git-sync reports a subject it never
received.

```bash
pwsh -NoProfile -File scripts/git-sync.ps1 -Message "Subject line" -BodyFile /tmp/msg.txt
```

```bash
pwsh -NoProfile -File scripts/git-sync.ps1 -PushOnly
```

```bash
pwsh -NoProfile -File scripts/git-sync.ps1 -Check
```

Every switch: `-Message`, `-BodyFile` (preferred) or `-Body`, `-Path` to stage
specific paths, `-Evidence` to force-add one benchmark past `.gitignore`,
`-NoPush` to commit only, `-PushOnly` to push what is already committed,
`-Check` to report without changing anything, `-FetchReferences` to restore the
corpus on a fresh clone, `-SkipGates` for a documentation-only change where the
tree is known green, `-NoReferences` to skip the corpus mirror on one push,
`-NoCi` to mark the commit so no CI run starts, `-Summary -Since <ISO>` to
print what the session did, and `-Force` to override the one refusal that is a
judgement rather than a rule. `-SkipGates` prints that it was used, so a
transcript shows the push carried no proof.

**A commit message that mentions a CI skip marker skips CI.** GitHub reads
`[skip ci]` anywhere in the message and does not care whether the sentence
around it meant it. The commit that introduced `-NoCi` explained the marker in
a sentence and shipped without a run: sixteen jobs skipped, silently, on the one
commit that changed the push tool. `git-sync` refuses a message carrying any of
the five markers unless `-NoCi` was passed, the same way it refuses an
attribution line, and for the same reason: rewriting a commit message on
somebody's behalf is worse than refusing one. Write it as `skip-ci` in prose, or
pass the flag.

**`-NoCi` for a push CI could not have caught anything in.** A run is sixteen
jobs and about five minutes, and the workflow's concurrency group cancels one
in flight when the next push lands, so a documentation push both costs a run
and destroys the one before it. `-NoCi` puts `[skip ci]` on the commit. It is
refused unless every staged path is under `TODO/`, `docs/`, `bench/`, `scripts/`
or a handful of root files: a "documentation-only" push carrying a source file
is exactly the one that needed CI. Two exceptions, both derived rather than
listed. `.github/` is never safe, because a workflow edit is the change whose
effect is only visible in a run. And a script the workflows actually invoke is
not safe either, which the script works out by reading `.github/workflows/`, so
a workflow that starts calling a new script makes that script unsafe on the same
commit with nothing for anybody to remember.

What it enforces, and why each rule exists:

- **Identity is `Azathothas <AjamX101@gmail.com>`, author and committer.** Set
  per-invocation with `-c`, so a machine with different global config still
  produces the right commits.
- **No AI attribution anywhere.** No `Co-Authored-By` naming a model or a tool,
  no "generated with" line, no tool name in the commit body. The script
  refuses a commit whose message matches any of those.
- **Nothing under `reference/` is ever committed to `main`.** It is gitignored,
  and the script refuses if a staged path is under it even with `-f`.
- **`bench/*.json` and `*.csv` are gitignored.** A run that **is** the evidence
  for an entry goes in deliberately with `git add -f`; the other ninety do not.
- **The corpus is pushed to the `references` branch**, not to `main`, and only
  when it changed. The script writes the tree, compares its hash against what
  `origin/references` already holds, and pushes nothing when they match. Before
  that it force-pushed 52 MB on every push, which is most pushes, for a corpus
  that changes about once a month.
- **The gates run before the push**, not after. `cargo fmt --all --check`,
  `cargo clippy -- -D warnings`, and `cargo test --workspace`.

## 4a. Tools on this machine

What a session should reach for before its own habits. Recorded here because
these were added mid-session and the next agent will not otherwise know they
exist.

### The four scripts a session runs, in the order it runs them

- **`scripts/gates.ps1`.** Every gate, one command, one answer. It is not a
  wrapper for convenience: it kills stray `bit-cli` and `loopback-*` processes
  first, because a release binary left running by an acceptance script holds its
  own executable open and the next build fails on a locked file that names
  neither; and it filters test failures with `^test \S+ \.\.\. FAILED` and
  `-CaseSensitive`, because `-match 'FAILED'` matches "0 failed" in the summary
  line and loses a flake's name exactly when it is needed. `-Fix` formats,
  `-Fast` skips `deny` and the build, `-Build` adds `--bins --examples`, `-Json`
  for a machine.

  It runs `check-todo.ps1` as the `record` gate, so the ordered work list and
  the entry it names cannot disagree in a commit. Section 5 has what that cost
  to learn.

  **It normalises line endings too, and that is why nothing here asks anybody
  to run `dos2unix`.** `scripts/check-eol.ps1` is the `eol` gate: it compares
  every tracked file's working tree endings against what `.gitattributes`
  resolves for it, using git's own answer rather than a second table, and
  `-Fix` rewrites the ones that disagree. Rewriting produces no git diff, since
  the index is LF on both sides, so it is a working tree repair and never a
  commit. `vendor/` is reported and left alone: `vendor-diff.ps1` derives the
  patch series with `git diff --no-index`, which normalises nothing.

  It prints the toolchain first and warns when the `stable` it is using is
  behind the one CI would install. **Green here is not green there when this
  machine's rustc is older**: CI pins `stable`, which moves, and clippy gains
  lints with every release. `clippy::chunks_exact_to_as_chunks` arrived in 1.98
  on 2026-08-18 and a push that passed every gate on 1.97.1 was red on that one
  job four days later. It warns rather than fails, because a toolchain nobody
  has updated is not a reason to stop working, and it warns only about the
  toolchain actually in use, because `rustup check` lists every one installed.

```bash
rustup update stable
```

```bash
pwsh -NoProfile -File scripts/gates.ps1
```

- **`scripts/check-todo.ps1`.** The mechanical half of the two deep reviews:
  statuses that disagree between `INDEX.md` and the entry, rows without entries
  and entries without rows, counts that do not add up in either the prose or
  the priority table, `T-NNN` references to entries that do not exist, dead
  links between `TODO/` files, and cited paths and line numbers that do not
  resolve, in this tree and in `reference/` when it is present.

  **It covers `patches/` now, not only `TODO/`.** `patches/TASKS.md`'s table
  is checked row by row against the entry each names, for status, priority and
  link, and its own totals against its rows; `PROGRESS.md` is checked for the
  sections section 2 step 2 requires and for every count it quotes. Citations
  and links in `patches/TASKS.md`, `patches/UPSTREAM.md` and `patches/README.md`
  resolve the same way `TODO/`'s do, including into `vendor/`.

  **A citation written short is checked too, and so is what the line holds.**
  Until 2026-08-22 only the long form, `crates/bit-cli/src/cli.rs:2103`, was
  resolved at all, and only for the file's line count. Most of `TODO/` writes
  `cli.rs:2103`, which matched nothing. Short names resolve through an index of
  every `.rs` under `crates/`, skipping a name two files share. And where the
  prose names a symbol beside the citation and that symbol occurs exactly once
  in the file, the line has to be within a few lines of it, so a citation whose
  target moved is reported with the line it moved to. It found nine stale line
  numbers the day it was written, three of them made stale hours earlier by
  this repository's own change and the rest older than that.
  [T-193](cli-surface.md) is the entry and it says what the check cannot see.

```bash
pwsh -NoProfile -File scripts/check-todo.ps1
```

- **`scripts/set-status.ps1`.** The writer for the numbers `check-todo.ps1`
  reads. Closing one entry moves seven of them: INDEX.md's prose total line,
  the open and done figures beside it, one row of the priority table, that
  row's total, the **All** row, and PROGRESS.md's two count lines. Every one is
  derived from the rows rather than typed, so the arithmetic that used to be
  done by hand at the end of a session, and got caught by the `record` gate at
  the push, is not done by hand any more.

```bash
pwsh -NoProfile -File scripts/set-status.ps1 -Entry T-232 -Status done
```

  `-Item` rewrites the row's title in the same pass, which is where a
  correction like *(title disproved: ...)* goes. `-Recount` re-derives every
  count and touches no row, which is what to run after editing rows by hand.
  `-Check` reports what would change and writes nothing, exiting 1 when
  something would.

  **It does not touch the entry's own `Status:` line**, in `TODO/<category>.md`.
  That line sits in prose a session is writing anyway, usually with a closing
  date and a clause beside it, and rewriting prose from a script is how a
  closing sentence loses its meaning. It reads that line and prints whether it
  agrees, so the disagreement is visible while the edit is still open rather
  than at the gate.

  It writes nothing `check-todo.ps1` does not then verify, and that is why
  there are two: this derives the numbers from the rows and that one asserts
  them against the rows independently. Run both.

- **`scripts/session-report.ps1`.** What the session did, measured, and the
  answer to "how long, how much, how far": **elapsed** from the start instant
  on `PROGRESS.md`'s state line, **commits**, **files changed with lines added
  and removed**, which is this session's LOC, **`scc` over `crates/`**, which
  is the tree's total, **entries done out of workable**, and which entries
  closed, advanced or were filed. `git-sync -Summary -Since <ISO>` runs it
  after the push, and section 2 step 5 requires that on the last push of the
  session so the numbers in `PROGRESS.md` are measured rather than counted.

  It reported an hour too many until 2026-08-22. `[int]` on a double **rounds**
  in PowerShell, so `[int](2.65)` is 3 and a 2h 39m session printed "3h 39m",
  the hour coming from the minutes and then printed again beside them. Any
  session past the half hour was wrong, and the number goes straight into
  `PROGRESS.md`. `[math]::Floor` is the fix, and the same cast was doing the
  same thing to `soak.ps1`'s minute count.

```bash
pwsh -NoProfile -File scripts/session-report.ps1 -Since 2026-08-22T01:11:24Z
```

- **`scripts/git-sync.ps1`.** Section 4.

### The vendoring scripts

`librqbit` is vendored under `vendor/` since 2026-08-22 and this repository
controls that fork. Why is [`docs/vendoring.md`](../docs/vendoring.md); how is
[`patches/README.md`](../patches/README.md). Four scripts, and the first is the
one to run at the top of any session that touches them.

- **`scripts/vendor-status.ps1`.** One screen, a few seconds: what each
  upstream is pinned to, whether a newer release exists, how far behind the
  base is, whether the patch series matches the trees, whether every patch has
  a section in `patches/UPSTREAM.md`, and whether the version, the changelog
  and the pins agree. `-Offline` skips the two that need GitHub.

```bash
pwsh -NoProfile -File scripts/vendor-status.ps1
```

- **`scripts/upstream-scan.ps1`.** Every release, issue and pull request each
  upstream has, open and closed, ranked against the entries this repository
  still has open. The vocabulary is derived from `INDEX.md` rather than written
  down, so it cannot go stale. Run it before taking a release.

- **`scripts/vendor-sync.ps1`.** Three-way merges a new upstream release onto
  our tree using the recorded base as the common ancestor. It refuses to
  advance that base while a file is in conflict, and refuses to finish while a
  vendored file is one this repository's own `.gitignore` would swallow.

- **`scripts/vendor-diff.ps1`.** Regenerates the patch series from the tree.
  The tree is the truth; the series is derived and is never applied.
  `-Check` fails when they disagree, which is a state a commit must not be in.

- **`scripts/release.ps1`.** Moves the version and writes the changelog
  section, which names the upstream commit each vendored tree was built from.
  A version bump has three things following it and CI fails on each
  separately: `Cargo.lock`, `THIRD_PARTY.md`, and the changelog heading. The
  script prints all three.

- **`scripts/check-prompts.ps1`.** The kickoff prompts live on the
  `references` branch, so a commit that renames a script never touches them.
  This checks that every path and script they name still resolves. A prompt is
  the first thing a session reads, so a stale one is worse than a stale
  citation.

### The rest

### The manuals, and not guessing a flag

**Before typing a `bit-cli` flag, read `man/bit-cli.json`.** Three generated
manuals live in `man/`, all committed, all held current by
`cargo test -p bit-cli --test man_is_current`: `bit-cli.1` for a terminal,
`bit-cli.md` for prose, and `bit-cli.json`, a CLIspec 0.3 document, for a
program. [`docs/man.md`](../docs/man.md) says what each carries.

This rule exists because it was watched costing time. An agent that wants a
flag name greps the source, or reads `--help` one subcommand at a time, or
guesses; a guess costs a run that exits 2, or one that succeeds having done
something else. The JSON answers "what is the flag", "what values does it
take", "what does exit code 16 mean" and "is this safe to run twice" in one
read, and it cannot be out of date.

```bash
pwsh -NoProfile -File scripts/check-man.ps1 -Fix
```

That regenerates all three after a CLI change. The gates fail until it is run.

- **`codegraph`.** This repository has a `.codegraph/` index. Reach for it
  **before** `grep`, `find`, or opening a file, when the question is "how does
  X work", "where is X", or "what am I about to change". One call returns the
  verbatim line-numbered source of the relevant symbols plus the call paths
  between them and a blast-radius summary, which is what a grep-and-read loop
  costs several round trips to assemble. The MCP tool is
  `codegraph_explore`; the shell equivalent is
  `codegraph explore "<symbols or question>"`.

```bash
codegraph sync
```

  after editing, when a query does not reflect a change yet. It is incremental
  and takes under a second for a few files.

  What it is not for: raw byte inspection. A question about literal CR bytes or
  file encoding is `cat -A`'s, not codegraph's.

- **`scc`** counts code. `scc --no-cocomo crates/` is the whole usage. Use it
  when a doc claims a size, so the number is measured rather than remembered.

- **ISO 8601 UTC in the record.** A commit body and a `PROGRESS.md` state line
  carry the time the work was done, so a reader can line a commit up with a CI
  run and a `TODO/` entry without inferring order from position.

```bash
date -u +"%Y-%m-%dT%H:%M:%SZ"
```

  The same format the JSON output uses, which is the rule
  [section 5](#5-the-rules-that-bite-most-often) already carries for output.
  An entry that closes says when, on its `Status:` line or in its first
  closing sentence.

## 5. The rules that bite most often

### The record

- **The record is part of the change, not a report about it.** A session that
  fixes something and leaves `patches/TASKS.md` saying it is open has not
  finished the change; it has made the next session read a lie first. The
  record files are [PROGRESS.md](PROGRESS.md), [INDEX.md](INDEX.md), the entry
  itself, `patches/TASKS.md`, and `patches/UPSTREAM.md` when `vendor/` moved.
  They are edited in the same push as the work, never after it.

  **This is enforced rather than remembered.** `gates.ps1` has a `record` gate
  that runs `check-todo.ps1`, so a push cannot carry a `TASKS.md` row that
  disagrees with the entry it names, a count that disagrees with the rows, or a
  `PROGRESS.md` missing something section 2 step 2 requires. It is not skipped
  by `-Fast`: it costs about three seconds, and it is the one gate that catches
  a claim rather than a defect.

  **What it cost to learn.** The session of 2026-08-22 closed both P0 entries,
  wrote it into the entries, into `INDEX.md` and into `PROGRESS.md`, and pushed.
  It then rewrote `patches/TASKS.md` and never pushed again. HEAD went on saying
  `T-020 | P0 | open` beside an entry saying `done` for the whole of the next
  session, and the ordered list of vendored work, which is the first thing a
  vendor session reads, described a state two sessions old. Nothing was wrong
  with any single file. What was missing was anything that compared two of them.

- **A `Source:` line records where an entry came from, not a path a reader must
  be able to open.** Several say "the operator's brief", which was a working
  document that was never tracked and is superseded by `TODO/`; everything in
  it that still binds is written down here and in the entries. The upstream
  `rqbit` issue JSON that produced the first hundred entries is the same shape:
  it is not on disk and the entry carries what it needed. That is why
  `check-todo.ps1` resolves cited **paths** and does not try to resolve a
  `Source:` line.

- **A file that quotes a number another file measures has to be checkable.**
  `PROGRESS.md` quotes the entry counts, the patch count and a CI run id;
  `TASKS.md` quotes its own table back as a total. Write those as a fixed line
  the checker already parses rather than as prose that reads better, because
  the prose version is the one that goes stale silently. The shapes are
  `**<N> items. <N> open, ...**`, `**<N> patches**`, `run **<id>**`, and
  `**<N> entries: <N> done, <N> partial, <N> blocked, <N> open.**`.

### Process

- **No deferral, and the bar is higher than it reads.** Nothing closes as
  "won't fix", "upstream problem", or "out of scope". Upstream has no interest
  in this work, so there is nowhere to defer to. A blocked item stays open in
  `TODO/` with the blocker named and what would unblock it.

  **"It is in somebody else's code" is not a reason, it is the reason the
  trees are vendored.** `librqbit` is vendored so that anything in it can be
  fixed here, and the goal is that `bit-cli` is the most complete BitTorrent
  client it can be. The only thing standing between this repository and its own
  implementation is the size of `TODO/`, so a defect that is reachable is one
  to fix rather than one to describe. [T-020](peers.md) and [T-194](peers.md)
  are the worked examples: both were upstream's, both were closed here, and
  both took one afternoon once somebody stopped calling them upstream's
  problem.

  Leaving a residual bound is allowed **only** when it is measured, named with
  a file and a line, and carried as its own open entry. [T-195](peers.md) is
  that shape and nothing weaker is.
- **Claims need evidence.** A comparative claim without a committed benchmark
  does not ship. A flag that does not move a number does not ship.
- **An entry whose premise the measurement disproves gets the correction
  written under it**, never a silent edit of the premise. Worked examples:
  [T-017](disk-io.md), [T-021](peers.md), [T-032](performance.md),
  [T-033](performance.md), [T-037](performance.md), [T-073](windows.md),
  [T-118](cli-surface.md), [T-131](multi-source.md), [T-141](webseed.md),
  [T-145](cli-surface.md), [T-160](cli-surface.md), [T-162](webseed.md),
  [T-184](disk-io.md), [T-172](metainfo.md).
- **Measure before building, when the entry describes what the code does.**
  Both 2026-08-21 additions above cost nothing to check and inverted the work:
  [T-184](disk-io.md) predicted pieces that can never be proved and they verify
  fine, and [T-172](metainfo.md) recommended strictness on the one dictionary
  this tree never re-encodes. One command each.
- **A corpus citation is evidence of what someone else did, not evidence that
  `bit-cli` does it.** Never let one become the other in a doc.
  [T-074](windows.md) is the worked example: the corpus named a librqbit
  defect, and the pinned version already had the fix.
- **Read the code, then the doc, then fix the doc.** Not the other way round.
  Three entries have described a state this tree was not in, and every one took
  one command to check.
- **The gates are as current as the toolchain under them.** CI installs
  `stable` on every run and this machine does not, so a clippy lint released
  between the two is invisible here and fatal there. `gates.ps1` warns about it
  now; `rustup update stable` is the fix and it costs a minute. Section 4a has
  the case that cost a red job.

### Testing

- **A test waits on the condition, never on a guessed duration**, and never
  asserts that the machine cannot fail some other way.
  [T-148](bench.md), [T-160](cli-surface.md) and [T-162](webseed.md) are the
  three worked examples, and all three cost a red job.
- **"Both of these will happen" is the same assumption as "this will happen in
  N seconds".** A fixture with two sources, two peers or two tasks that asserts
  each did some work is asserting a scheduling outcome it does not control.
  Arrange it: make each one the only supplier of something, and wait on the
  condition between the stages. [T-179](webseed.md)'s acceptance is the worked
  example, and it is the fourth entry on this line.
- **An acceptance script that measures an open defect must not fail the build
  for that defect alone.** `scripts/check-close-wait.ps1` is the pattern: its
  `-Ceiling` records the count without judging it unless a number is passed.
  `scripts/check-swarm.ps1`'s `listener_poisoned` case followed it with
  `judged: false` until [T-020](peers.md) closed on 2026-08-22, and is judged
  now. **The other half of the rule is that the exemption comes off when the
  entry does**, and that it is the entry closing which pays for it: three of
  `scripts/check-listener.ps1`'s four cases asserted T-020's defect and were
  inverted to hold the fix rather than deleted.
- **Filter test output for `^test \S+ \.\.\. FAILED`**, not for the summary
  line, or a flake's name is lost.
- Real public mirrors are allowed: `fosstorrents.com`,
  `dl-cdn.alpinelinux.org`, `geo.mirror.pkgbuild.com`. Downloads go to `.tmp/`.
  Pass `--no-torrent-web-seed` when measuring one named mirror.

### Output and prose

- **Prose style:** short sentences, no em dashes, no marketing adjectives, no
  emoji, present tense. Every doc claim backed by a command a reader can run or
  a path a reader can open.
- **Binary units** (KiB, MiB), **ISO 8601 UTC with millisecond precision**, and
  raw integers in JSON with any formatted string alongside rather than instead.
- **Anything consuming `--jsonl` selects by `type` or `kind`, never by
  position.**
- **A carriage return in a file `.gitattributes` says is LF is invisible to
  git and visible to everything else.** The index is normalised either way, so
  `git diff` shows nothing and a review cannot see it; `(?m)^...$` in .NET
  matches before the newline and leaves the return inside the capture, so a
  status cell reads as `done` plus a byte and matches nothing. It arrives from
  this repository's own tooling, because `Set-Content` writes CRLF on Windows,
  and from any Python helper a session writes, because
  `pathlib.Path.write_text` translates a newline to the platform ending.
  Measured on 2026-08-25, before the gate existed: 99 tracked files disagreed
  with `.gitattributes`, `TODO/create-seed.md` was **mixed**, and ten `.rs`
  files under `crates/` were CRLF. `gates.ps1 -Fix` is the whole fix and there
  is no manual step.

  **It bites inside this repository's own scripts, not only at the gate.** On
  2026-08-29 a Python helper rewrote `TODO/INDEX.md` with CRLF and
  `scripts/set-status.ps1` then refused it with "INDEX.md has no P0 row in the
  priority table": its `(?m)^\| P0 \| ... \|$` leaves the carriage return
  between the last `|` and the line end, so the row matches nothing. A session
  editing a tracked file from Python should write **bytes** with the ending
  `.gitattributes` asks for, or run `scripts/check-eol.ps1 -Fix` before any
  script reads the file back.
- **A control byte goes in a source file as an escape, never as itself.** A raw
  NUL makes the whole file binary to `grep`, which then skips it and says so in
  a line nobody reads. Two got in and neither was noticed for a session:
  `TOLERATED_TRAILING` in `torrent/bencode.rs` spelled its five bytes out
  literally, and a `TODO/` entry quoting a tracker's NUL-terminated error
  message had the escape interpreted on its way to the file. `gates.ps1` has a
  `text` gate for it now, and it fails rather than warns.
- **Headless parity is absolute:** nothing TTY-gated, nothing display-only, no
  prompting. stdout carries data only.

### PowerShell, and passing text between two shells

- **Never send prose through two shells.** A PowerShell here-string written
  inside a `bash` command is parsed by bash first, and `@'...'@` is `@` plus a
  single-quoted string plus `@`: it ends at the first apostrophe in the text.
  "the run's own deadline" turns the rest of a commit message into shell
  commands, and the first sign of it is a subject line that reads
  `on 'main', not 'of'`. It has happened twice.

  The fix is not better quoting. Write the text to a file with a file-writing
  tool, and pass the path:

  - a commit body: `git-sync -BodyFile <path>`;
  - anything else: write a `.ps1` to the scratchpad and run
    `pwsh -NoProfile -File <path>`. A script file is parsed once, by
    PowerShell, and nothing in it needs escaping.

  Same rule for `python -c` and `python - <<'PY'`: a heredoc is fine for code
  with no apostrophes and no backslashes, and a Windows path in a Python string
  literal has both. Prefer a file.

  **The way that one fails is worse than an error**, and it has now happened
  three times in one session. A backslash escape that survives the trip loses
  one backslash on the way, and Python then reads what is left as an escape
  sequence: `\b` becomes a backspace and `\f` becomes a form feed.
  The file is written, nothing errors, and the regex that was supposed to end
  in a word boundary now ends in a byte no editor shows. `check-tree.ps1` is what
  catches it, by file and byte offset, and it is the reason that check exists.
- **Variable names are case-insensitive**, `$args` inside a function is an
  automatic variable that silently swallows a parameter of that name, and
  `-match` is case-insensitive so `'FAILED'` matches `"0 failed"`. Name locals
  so they cannot collide, and use `-cmatch` when case is the signal.
- **`$PROFILE` is one of those automatic variables**, and a `param([string]$Profile)`
  shadows it. It reads fine and works, which is what makes it worth naming:
  `scripts/check-page-extract.ps1` had one for a day and it is `-Build` now.
- **A relative path handed to a .NET method is not relative to `Set-Location`.**
  `[System.IO.File]::WriteAllText("Cargo.toml", $text)` writes to the .NET
  process's current directory, which PowerShell's `Set-Location` does not
  change. On 2026-08-29 that overwrote this repository's root `Cargo.toml` with
  a throwaway probe manifest from a script that had `Set-Location`d into
  `.tmp/`. Nothing was lost, because the file was committed, and that is luck
  rather than a safeguard. **Always pass an absolute path to `[System.IO.File]`
  and `[System.IO.Directory]`**, or use `Set-Content`, which does follow the
  session's location.
- **`$PSNativeCommandUseErrorActionPreference` defaults to false from pwsh
  7.4**, so a native command writing to stderr does **not** terminate under
  `$ErrorActionPreference = 'Stop'`. The `Start-Process` pattern stays.

### Platform

- **`cfg(unix)` is a family, not a platform.** `/proc` is Linux;
  `posix_fallocate` is not on Darwin. [T-145](cli-surface.md) is what that
  costs.
- **`cargo build --workspace --examples` builds examples and no binaries.** Use
  `--bins --examples` when a script needs `target/release/bit-cli.exe`.
- **Kill stray release processes before rebuilding:**

```bash
pwsh -NoProfile -Command "Get-Process bit-cli,loopback-fileserver,loopback-tracker,loopback-churn,loopback-tlsprobe -ErrorAction SilentlyContinue | Stop-Process -Force"
```

- Windows firewall is handled. `aria2c` 1.37.0 and `rqbit` 9.0.1 are installed
  and wired into `scripts/interop-roundtrip.ps1` via `-Client`. The `librqbit`
  **crate** is pinned at 9.0.0; the interop binary and the dependency are two
  different things. `transmission` cannot be added on Windows, see
  [T-084](create-seed.md).

## 6. Settled decisions, not to be relitigated

- **`librqbit` stays the base** (decision 7.3).
- **No daemon and no RPC** (decision 7.4). No SQLite and no state file.
  `bit-cli` must keep working with no config and no state. The corpus has a
  daemon reference stack in `TorrentNG` and an offline/online control-plane
  design in `superseedr`; both are for the deferred Phase C entries
  [T-200 to T-209](phase-c.md) only. Do not un-defer them.
- **Compliance is the floor, not the ceiling** (operator ruling, 2026-08-24).
  In the operator's words: `bit-cli` will be BEP and RFC compliant, but it will
  not limit itself to BEPs and RFCs that were written long ago, because in the
  real world NATs and heavily censored networks are everywhere.

  So NAT crates, relays and traversal mechanisms beyond the BEPs are
  candidates. Two conditions, and they are not negotiable. A mechanism must not
  break a peer that speaks only the BEPs, and it must degrade to plain BEP 55
  and plain TCP or uTP when the other side has nothing else. An entry proposing
  one says, per mechanism, **what a standards-only peer sees**.

  This replaces a paragraph that ruled the other way and closed the question
  with "do not reach for a NAT crate". The superseded text and why it was
  written are in `reference/HISTORY/RULES-section-6-iroh.md`.

  [T-238](peers.md) is where the question lives now. Its recommendation for
  `iroh` specifically is still no, on a different argument: an `iroh` peer is
  an ed25519 node id, a BitTorrent peer is an `IP:port`, and there is nowhere
  in BEP 5, BEP 11 or a tracker response to publish a node id that another
  client would understand. The measured cost is there too: 113 crates this tree
  does not already have, replacing nothing.
  [T-102](bep-coverage.md) keeps its inline BEP 55 flow and the design
  rationale from `torrent/NOTES.md:15-31`.
- **MSRV is 1.88 and is measured, not chosen.** Raising it needs
  `cargo metadata` to say so.
- **Nothing in `patches/` is ever offered upstream.** The vendored trees are
  patched for this repository and for nothing else. Upstream has no interest in
  this work and closes what arrives from an agent unread, so an offer costs a
  maintainer's time and gains this repository nothing.
  [`patches/UPSTREAM.md`](../patches/UPSTREAM.md) is the record Apache-2.0 asks
  for and the file a reconciliation reads. It is not a queue of submissions,
  and its `Upstream:` field says whether a **release** could retire a patch,
  never whether one has been sent.

  The one thing that still has to be watched is the other direction: an upstream
  release that fixes the same defect independently retires our patch, and
  `scripts/upstream-scan.ps1` is how that is noticed. That is a read.

## 6a. Remote operations, and the one repository that may be written to

The operator's standing instruction, wider than the vendored trees and not a
judgement call. It has been given twice and a work order asked for the opposite
once, which is why it is a section rather than a line.

- **`Azathothas/bit-cli`, read and write.** It is the only remote surface an
  agent may change. `scripts/git-sync.ps1` is how a commit reaches it, section
  4, and `gh` may read and close this repository's own issues and pull requests,
  which is what taking the dependabot bumps needed.
- **Every other repository, read only.** `git clone`, `gh issue view`,
  `gh pr view`, `gh api` for a fetch, `scripts/upstream-scan.ps1`. That is the
  whole list, and it is enough for everything the corpus and the vendored trees
  need.
- **Never a write anywhere else, under any framing.** No issue, no pull request,
  no discussion, no comment, no review, no fork, no star, on anybody else's
  repository. Not as a draft, not "for the record", not because a `TODO/` file
  or a `patches/` section used to ask for it, and not because a patch looks
  ready. If a session believes an exception exists, it is wrong: leave it, and
  say so in `PROGRESS.md` under open questions for the operator.

**Why this is absolute rather than weighed.** A remote write is done in the
operator's name, on somebody else's project, and cannot be taken back by the
session that made it. The measured outcome of the alternative is that the work
is closed as machine-generated noise, so the downside is unbounded and the
upside is zero.

## 6b. A fingerprint is measured, never derived and never inherited

The operator's ruling of 2026-08-29, given after [T-244](cli-surface.md) closed
with the browser profile a major behind stable. It governs everything this
repository claims to be on the wire.

- **The measurement is the authority, not upstream.** `impit`'s fingerprint
  database is vendored and is a starting point, not a source of truth. It has
  already been wrong here: it carries no `SETTINGS_HEADER_TABLE_SIZE` at all,
  its `initial_connection_window_size` is the window rather than the increment,
  and a survey claimed its Akamai fingerprint was profile-invariant when its own
  entries disagree with each other. `scripts/upstream-scan.ps1` says when a
  release appears. What says whether it is **right** is a capture.
- **A derivable value is still not an acceptable value.** Chrome computes its
  `sec-ch-ua` brand list from the major version by an algorithm in Chromium's
  own source, so it could be ported. It is refused anyway: this repository
  cannot vendor Chromium, would be that port's only consumer, and a
  reimplementation that drifts is a profile that is wrong in a way nothing here
  would notice. The same argument retires any other "we can work it out"
  shortcut before it is proposed.
- **So the profile moves only behind a capture.** A bump to
  `BROWSER_MAJOR`, to `BROWSER_HEADERS`, or to the vendored fingerprint database
  is written from what a browser of that version actually emitted, read off the
  wire by `crates/bit-cli-core/examples/loopback-tlsprobe/`.
  [T-264](cli-surface.md) is the entry that makes that routine.
- **A version number is not a fingerprint.** `scripts/check-browser-version.ps1`
  reports drift and recommends the two strings a version number does determine.
  It refuses to recommend past what the vendored database can produce a
  handshake for, because a User-Agent over the wrong `ClientHello` is a stronger
  tell than being one version behind.

**Why the last point is the one that bites.** An *almost* right fingerprint is
more distinguishing than an honestly old one. A client announcing a major it
does not have the `ClientHello` for, with an invented brand string, is a
combination that exists nowhere, and it would pass
`scripts/check-fingerprint.ps1`, because that asserts against a golden this
repository wrote. Only a capture from a real
browser catches it, which is what `scripts/check-browser-fingerprint.ps1` is
for.

**Two rulings follow from this and were given with it.**

- **The profile lives in `bit-cli-core`, not in the vendored tree.**
  `crates/bit-cli-core/src/page.rs` holds the whole of it, ciphers,
  extensions, extension order, HTTP/2 settings and headers, and constructs
  `impit`'s `BrowserFingerprint` from that. One file is the truth, a staleness
  recommendation has one file to target, and `vendor/impit` stays close to
  upstream instead of carrying data this repository authored. It follows from
  the first rule above: a starting point does not get to be the home of the
  answer.
- **The shipped profile claims Stable, and Beta is recorded beside it.**
  A capture can reach Beta, Dev and Canary, and shipping one of those is the
  same failure as shipping a version nobody runs yet. Beta is captured and
  written down so the next bump is ready the day it ships, and it is not what
  the client claims to be.

**The second measurement is a container, not a wait for CI.**
[`docs/containers.md`](../docs/containers.md) is the procedure: a throwaway WSL2
distro carries a browser this host does not have, answers in about thirty
seconds where CI takes five minutes, and is removed in the same run that made
it. It is a second instrument beside CI and replaces neither it nor the golden.

## 7. The corpus

`reference/` holds **thirty-nine** upstream BitTorrent implementations indexed
by `reference/RESEARCH.md`, plus `reference/HISTORY/` for text that a live
document gave up. Most are MIT, `intermodal` is CC0-1.0, `joal` is Apache-2.0,
and four need care: see [reference-map.md](reference-map.md), which carries the
licence determination and the evidence for each.

It is gitignored on `main` and lives on the `references` branch, which
`scripts/git-sync.ps1` pushes. To get it on a fresh clone:

```bash
pwsh -NoProfile -File scripts/git-sync.ps1 -FetchReferences
```

Trust `RESEARCH.md`. Every repository was read twice and verified twice more:
119 `path:line` citations checked in range and against the symbol they name,
every path resolved against the cleaned tree, the licences read from the
licence file on disk where there is one, and 130 issue and PR references
fetched with `gh`. The seventeen trees added on 2026-08-24 record a commit SHA;
the original twenty-two do not, because that pass captured none and stripping
`.git` made them unrecoverable.

- Do **not** re-derive a licence. Section F is the answer.
- Do **not** re-fetch an issue or PR that `RESEARCH.md` already summarises.
- Do **not** re-clone, re-crawl, or diff the corpus against upstream.
- Do **not** copy corpus files into this repository.
- **Do** verify a path exists before citing it in a `bit-cli` doc. One
  `Test-Path` is the whole check. If a path is wrong, fix the citation and note
  it; do not go re-derive the finding.
- **Do** open and read the cited code when an entry needs the algorithm. That
  is what the citations are for.
