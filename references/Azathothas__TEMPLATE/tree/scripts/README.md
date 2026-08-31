# scripts

The probe, the checks, and the helpers a project inherits.

⭐ **One command runs all of it**, and it is the one to reach for rather than
typing a list from memory:

```bash
sh scripts/common/check-gate.sh
```

```bash
pwsh -NoProfile -File scripts/common/check-gate.ps1 -Fast
```

| directory | what is in it |
| --- | --- |
| [`doctor/`](doctor/) | ⭐ the environment probe. Two implementations, one schema. Every project keeps this. |
| [`common/`](common/) | the checks, and the one helper. ⛔ Every one has a POSIX sh implementation AND a PowerShell twin. |

⛔ **What is here is what a project must be able to run with NO NETWORK.** That
is the whole selection rule, and it is why every check is here and the writing
helpers are not: a gate that has to fetch a check is red whenever somebody
else's host is, and a check fetched at gate time is code nobody reviewed
judging the tree it is judging.

⚠ **Everything that failed that test now lives in
[`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit)**, catalogued in
[`../docs/agent-tooling.md`](../docs/agent-tooling.md) with a link each.
⭐ **The reason is one sentence: a tool kept in two repositories acquires two
sets of defects, and one of the two never gets fixed.** Two of the four that
left were carrying a defect upstream had already fixed, and one of those two
could have put a fabricated author on a commit.
[`../docs/history/twins-and-scripts.md`](../docs/history/twins-and-scripts.md)
carries the comparison and names the two that had not drifted.

⚠ **`powershell-windows/` used to be here** for a job with no POSIX form. Its
one file was a wrapper around a tool that lives upstream, and the wrapper went
with the tool. ⛔ The directory is gone rather than kept empty: git does not
track an empty directory, so a fresh clone would not have had what this table
described. [`../docs/containers.md`](../docs/containers.md) is the procedure
that replaced it.

---

## ⭐ Everything in `common/` has two implementations, and here is what that cost

⛔ **A POSIX sh check cannot be assumed to run on Windows.** This was the
template's original position and it was wrong. The reasoning was that `sh`
would be present because Git Bash ships with git, so one implementation was
enough. Measured on one Windows 11 machine, 2026-08-25, from a native
PowerShell session with Git Bash NOT on `PATH`:

| tool the checks need | native PowerShell resolves it to |
| --- | --- |
| `sed` | ⛔ nothing. Not installed. |
| `sort` | ⚠ PowerShell's own `Sort-Object` alias, not the coreutils binary |
| `awk`, `grep`, `tr`, `comm`, `xargs` | present here only because scoop and a coreutils package happen to be installed |

⚠ **The second row is the dangerous one.** A missing tool fails loudly and
somebody fixes it. An ALIASED one succeeds and returns a DIFFERENT ANSWER.
`Sort-Object` even accepts `-u`, which is what makes it convincing. Measured on
the same machine, same day, over the five values `b A a B a`:

| | result |
| --- | --- |
| `LC_ALL=C sort -u` | `A B a b` |
| `Sort-Object -u` | ⛔ `A b` |

⛔ **It dropped two of the four distinct values**, because it compares
case-insensitively and keeps whichever it saw first. A check that deduplicates
a file list that way does not crash and does not warn. It reports on a smaller
set than it was asked about, and reports success.

⭐ **What did NOT reproduce, and is worth writing down so nobody re-derives
it:** git and `gh` behaved identically from both shells on this machine. Same
`git.exe` 2.55.0.windows.3, same `credential.helper manager` from the same
system config, same authenticated `gh`. So the argument for twins here is the
TOOLCHAIN, not credential scoping. A machine that installs git differently per
shell would add a second reason; this one did not have it.

### ⛔ Wherever a twin exists, `check-twins.sh` covers it

That is not advice, it is the rule that keeps two implementations from becoming
two behaviours. [`common/check-twins.sh`](common/) runs BOTH halves of every
pair on one tree and compares the `--json` answer and the exit code, both read
from the process that produced them.

⚠ **It compares ANSWERS on the tree it is run against, not the rules.** A scope
difference with nothing in the tree to exercise it is invisible: dropping `.py`
from one twin's extension list changed no number here, because this repository
has no `.py` file. Dropping `.md` was caught instantly. ⭐ Prove a scope rule
with a fixture, not by trusting the comparison to notice.

### The things that do NOT have twins, or are not compared, and why

| | |
| --- | --- |
| [`common/check-twins.sh`](common/) | ⛔ **It cannot have one.** It works by running both halves of every pair, so it needs a POSIX shell to run the sh half no matter what language it is written in. A PowerShell twin would still require `sh`, which is the exact dependency a twin exists to remove. It is a maintainer's tool and it runs where both implementations do: this machine, and the CI job that has `pwsh` on an Ubuntu runner. |
| [`common/check-gate`](common/) | ⭐ **Has both halves, and is deliberately NOT compared.** It invokes `check-twins`, so putting the pair in `check-twins`'s own list would recurse. ⚠ The two exclusions are a shared contract: dropping either reintroduces a hang that once left twenty stray shells open. |
| [`common/mine-repo`](common/) | ⚠ **Its FETCH is not compared; its JOINER is.** A fetch comparison would pull a live third-party repository twice per run and make a local check depend on somebody else's uptime. ⛔ **That reasoning was read as covering the whole script and it never covered the join**, which is the part that was wrong. `--selftest` compares the join and its guard against a built-in fixture with no network, and that pair IS in `check-twins.sh`. |

⭐ **The question to ask is whether the JOB exists on the other platform, not
whether the language does.** Every check in `common/` passes that test, which
is why every one of them has two halves.

⛔ **And ask it again about every EXCLUSION, which is what this repository got
wrong.** An exclusion is written for a reason that covers part of a script, and
then it is read as covering the script. `mine-repo`'s exclusion was correct
about the fetch and silently protected a joiner that discarded an entire
comment corpus while printing "ok". A consumer found it, not this file.

---

## The check contract

⛔ **Every check in this repository, and every check a project inherits from it,
satisfies all five.** A script that does not is not a check; it is a script
somebody has to remember to interpret.

1. **A header comment saying what defect it exists to catch.** Not what it
   does: what goes wrong without it. ⭐ This is the field that decides whether a
   future session keeps it, deletes it, or writes a second one that overlaps.
2. **Exit 0 pass, 1 fail, 2 could not run.** ⚠ Those are three different facts.
   "The check failed" and "the check could not run" mean opposite things about
   whether you can ship, and a script that returns 1 for both hides the
   difference.
3. **A json switch**, so a gate runner can consume it.
4. **No dependence on the directory it is run from.** Resolve paths from the
   script's own location.
5. **Read only, unless a fix flag is passed.** A check that repairs things by
   default is a check nobody can use to find out whether something is wrong.

⚠ **A check that measures an open defect must not fail the build for that
defect alone.** Record the count and judge it only past a stated ceiling.
⭐ The other half of that rule is that the exemption comes off when the item
closes. An exemption nobody removes is a check that stopped checking.

---

## ⛔ An exit code is read from the process that produced it, unpiped

```bash
sh scripts/common/check-no-secrets.sh
```

Not `check | grep`, not `check | Select-String`, not `check | tee`. A pipeline
reports the **last** command's status, so a check that failed reads as green.

⚠ This has caught the author of this sentence, in the session that wrote it.

---

## What is here

### `doctor/`

The environment probe. Read
[`doctor/README.md`](doctor/README.md) for the schema and the measured
runtimes.

⭐ It is a **probe, not a gate**: a missing tool is data, so it exits 0 whether
or not anything is missing. Nothing here belongs in a gate chain.

### `common/check-no-secrets.sh`

Does any file in this tree carry something that must not be published.

⚠ **Tracked plus untracked-but-not-ignored, not tracked alone.** A file that
has never been staged is exactly when a new file is likeliest to carry a
credential, and exactly what the next `git add -A` would take.

⛔ **It finds the shapes it knows, and a green run is not a clearance.** It
cannot find a password that looks like a word or a page of correct-looking
examples that happens to describe a real system.

`--public` adds the rules that only matter for a repository that will be
public: emails, absolute home paths, long hex identifiers. In a private project
those are legitimate content, which is why they are not the default.

### `common/check-one-home.sh`

Does any sentence of 12 words or more appear in two documents.

⛔ **The rule was in `prose.md` from the start and nothing checked it**, so it
drifted the way an unchecked rule always drifts. Its first run over this tree
found **42** duplicated sentences of 8 words or more, ⭐ five of them in
`docs/templates/RULES.md`, which is the skeleton this repository ships for
recording a project's rules and which opened by saying it restated nothing.
That file went from 198 lines to 134 and now links what it used to copy.

⚠ **THE FIRST VERSION OF THE INSTRUMENT REPORTED ZERO AND WAS WRONG.** Its file
collector handed git a quoted pathspec through a shell that treats a quote as
an ordinary character, so it matched nothing and reported a clean tree it had
never opened. ⭐ Both halves now refuse to report success over an empty scope,
and that is the only reason the number above exists.

⛔ **The three entry-point routers are exempt from each other and from nothing
else.** `AGENTS.md`, `ROUTE.md` and `docs/templates/AGENTS.md` state the
absolutes in full on purpose, because a session may be handed exactly one of
them. ⚠ A sentence shared between a router and any other file is still refused,
which was verified by planting one.

⚠ It compares SENTENCES, so a fact restated in different words passes here and
fails a review. That is the same split every prose rule has.

### `common/check-placeholders.sh`

Did a template placeholder survive into a real file. Run at the end of a
bootstrap, and as a gate afterwards.

### `common/check-gate.sh`

⭐ **Run the whole local gate with one command.** Part (a) of
[`../docs/methodology/gate.md`](../docs/methodology/gate.md) is a list, and a
list run by hand is run in the order somebody recalls it. The session that
wrote this ran its gate five times and typed a different subset each time.

It delegates and holds no rules of its own. ⛔ **A skipped check is reported as
a skip, never as a pass**, because a runner that dropped one quietly and
printed green would be the forbidden-patterns row about a step that exits 0
having done nothing. `--strict` makes a skip a failure, which is what CI should
pass; `--fast` drops `check-twins` and nothing else.

⛔ **Zero passes is red whatever the skips say.** It produced exactly the
opposite on its own first run: a broken presence test made every row report
"not present", and it printed a green verdict over nothing at all. That is the
defect its header describes, produced by the script itself, and it is the
argument for the rule.

⚠ **Neither half is in `check-twins.sh`'s pair list**, deliberately: this
runner invokes `check-twins`, so comparing the two runners from inside it would
recurse. An earlier version of this idea elsewhere did exactly that and left
twenty stray shells holding their own files open.

### `common/check-docs.sh`

Do the documents still resolve, and are they written the way this
repository writes documents. Relative links, fenced shell blocks that
parse, shell-unsafe placeholders, banned vocabulary, and orphan pages.

⚠ The template directories are exempt from the **link** check only: their
links are written relative to where the file will live in a project. The
prose rules still apply to them.

⛔ **The character rules are NOT here any more.** No em dash and no character
outside the five moved to `check-markers.sh`, which reads every tracked text
file rather than markdown alone. Two checks enforcing one rule is two places
for it to be wrong, which is the same move the control-byte rule already made
out of this file.

### `common/check-markers.sh`

Only the five defined characters, and not too many of them. Two rules, one
subject, one home.

⛔ **It reads every tracked text file.** The rule it inherited scanned markdown
alone, and on the day it was widened this repository's own scripts held **2290**
characters outside the five across 22 files. Every one was in a script, so the
markdown-only version had never seen any of them.

⭐ **The density ceiling is 30 markers per 100 non-blank lines, and the number
is measured rather than chosen.** Over three trees on 2026-08-28: the one that
reads worst 38.6 overall with a worst file of 53.3, this one 9.0 and 26.3, the
one that reads best 8.6 and 21.8. ⭐ The two ADOPTER trees had been ranked by
eye first and the ranking came out in that order. ⚠ This tree was not ranked
against them; its number simply falls between.

⚠ **Two exemptions, each load-bearing.** `LICENSES/*.txt` is canonical SPDX
text compared byte-for-byte elsewhere, so a check asking anybody to edit it
would be asking for a corruption. A **leading** byte-order mark is exempt
because every `.ps1` here needs one; a mark anywhere else is still reported.

⚠ **A specimen inside a code span is permitted**, because a page that bans a
character cannot otherwise show which one, and this file could not describe the
check without it.

### `common/check-twins.sh`

Do the two probe implementations still answer the same way. It runs both on
one machine and compares the schema, the section keys, and the host and repo
facts that describe that machine.

⚠ It compares the SHAPE and the FACTS, not the tool-by-tool verdicts. Each
twin reports what its own host can reach, and on a Windows machine with msys
installed `bash`, `tar` and `zsh` genuinely differ between them.

⭐ **It also compares the CLI surface, which the schema cannot show.** Every
comparison above reads what the probes OUTPUT; none of them reads what the
probes ACCEPT. `doctor.sh --text` exited 0 while `doctor.ps1 -Text` exited 1
with a parameter-binding error, and every other comparison in the file passed
the whole time that was true.

### `common/check-remote-items.sh`

What is open against the repository, and does it say anything that survives
being checked. For every pinned action a pull request proposes: the commit
exists in the repository the ref names, the tag comment resolves to that same
commit, and ⭐ the runtime it DECLARES is not one the platform has deprecated.

⛔ **It never merges, closes, comments or approves.** It reports, and deciding
is the operator's.

⚠ It cannot tell you whether a change is a good idea. It checks the facts an
item asserts about the world; whether you want the change is a reading.

⭐ It exists because this repository was pinned to an action targeting a Node
runtime GitHub had deprecated, and the warning sat in a log nobody read. A
dependency bot is right almost every time, and that is precisely what makes
the wrong one expensive.

### `common/check-control-bytes.sh`

Is there a literal control byte in any text file in the tree.

⭐ **It covers every text file, not only markdown.** The rule used to live in
`check-docs.sh` and scanned `.md` alone, which left every `.ts`, `.py`, `.rs`,
`.sh` and `.yml` unchecked for the one defect that makes a file invisible to
both review tools at once: `grep` calls it binary and skips it, and `git diff`
prints "Binary files differ" so a code review shows no diff at all.

⚠ The runtime value is identical either way, so only reviewability is ever at
stake. That is exactly why it survives unnoticed.

### `common/check-changelog.sh`

Does `CHANGELOG.md` still obey the four rules a machine can hold: newest first,
every heading dated, every entry naming its record, every entry saying whether
it deployed.

⭐ It exists because [`../docs/conventions/docs.md`](../docs/conventions/docs.md)
stated those four rules, said in as many words that each was mechanical enough
to check, and nothing checked them.

⚠ **No `CHANGELOG.md` is exit 2, not exit 0.** A project without one has
neither broken these rules nor satisfied them, and reporting green over an
absent file is how a check quietly stops applying.

## The one helper, which is not a check

⚠ **A helper writes; a check reports.** The five-point contract above is for
checks. This one is held to the header rule and the exit-code rule, and
deliberately not to "read only": writing is what it is for.

⚠ **There used to be four more**, and
[`../docs/agent-tooling.md`](../docs/agent-tooling.md) says where each went.
⭐ `mine-repo` stayed on the operator's ruling: it encodes this methodology's
reference-sweep procedure rather than a general job, so it has no home upstream
and no second copy to drift from.

### `common/mine-repo.sh`

Fetch everything a reference sweep needs, and ⭐ **keep it**.

⛔ **It exists because the evidence kept being thrown away.** One sweep kept its
conclusions and deleted eleven clones, so every citation became a claim nobody
could check. One session spent about fifteen minutes writing its own fetchers,
produced real data, and deleted the data and the fetchers on the way out
because both lived in session-local scratch. Same defect twice: the DERIVED
file treated as the product and the EVIDENCE as scratch.

It fetches metadata, issues and pull requests in both states, comments, review
comments, releases, tags, discussions where it can reach them, and the tree with
its commit captured **before** the strip. It writes a `PROVENANCE.md` naming the
commit, the route, and ⛔ what it could not get.

⚠ **It probes `gh` rather than assuming it**, because a token `command -v` says
is there has been dead on a live run, and falls back to a public proxy carrying
none of the caller's credentials. ⛔ Reads only, on both routes.

⛔ **ITS PAGE JOINER WAS WRONG FOR MONTHS AND THIS FILE SAID WHY NOBODY WOULD
FIND OUT.** The credential-free route pages by hand, and the sh half joined the
pages by counting `[` and `]` over the raw text, which counts the brackets
inside string values. Comment bodies are markdown. Measured on 2026-08-30 on
one Windows 11 machine, against `firasuke/mussel` on the proxy route: **0
comments before, 202 after**, with the run printing `comments: ok` both times.
A consumer reported it.

⚠ **The exclusion that hid it was CORRECT and too broad.** Comparing two
miners does mean fetching a live third-party repository twice per run, which
would make a local check depend on somebody else's uptime. That covers the
FETCH. It never covered the JOIN, and the sentence that used to sit here said
the pair "was proved instead by running both halves against one target, which
is stronger evidence". ⛔ It was not stronger: that target's comment bodies had
balanced brackets, so the comparison agreed on a defect both halves did not
share.

⭐ **`--selftest` is the replacement, and it is in `check-twins.sh`.** It runs
the joiner and its guard against a built-in fixture carrying unbalanced
brackets inside string values, with no network and no credential, and both
halves must answer identically. It is in the local gate and in both CI jobs.
Mutation-proved by restoring the old joiner: it exits 1.

```bash
sh scripts/common/mine-repo.sh --selftest
```

⚠ **It found three more defects on its own first runs**, which is the argument
for having it rather than a footnote. None was the defect it was written for:

| what it found | why nothing else would have |
| --- | --- |
| `command -v python3` resolving to a Store stub that is on `PATH` and exits 49 without running | presence is not capability, and the probe for it was `command -v` |
| `node -e` argv indices read one off the end, so the join wrote no file and returned success | the guard read a file that was not there and found nothing wrong with it |
| the PowerShell dispatch captured its own report into a return value, printing nothing while exiting 0 | a check that reports success having shown nothing looks exactly like a check that passed |

---

## Adding one

1. **Name the defect first.** If you cannot say what goes wrong without this
   script, it is not a check.
2. **Follow the contract**, all five points.
3. ⭐ **Mutation-prove it.** Plant the defect it exists to catch, run it, and
   read the exit code unpiped. **A guard that has never been seen to refuse is
   a guard nobody knows works.**

   This is not optional advice. A licence filler here reported success over a
   licence whose warranty clause it had corrupted, because its check only ever
   asked whether a placeholder *survived*, never whether the substitution had
   reached too far. The mutation test is what found it. ⚠ That filler has since
   moved upstream; the instruction it produced has not.

   ⛔ **And prove the NEGATIVE case too.** A guard that refuses everything is
   as useless as one that refuses nothing, and it looks identical from a
   passing mutation test. `mine-repo --selftest` asserts that an empty join
   over a page with records is REFUSED and that an empty join over a genuinely
   empty page is ACCEPTED, because a guard with only the first would turn every
   repository that has no comments into a failed fetch.

4. **Wire it into the gate**, if it can fail.
5. **Document it**: here, and in the project's own tool table.

⚠ **A script that lives only in a transcript is re-derived every session.**
When a scratch helper does something a future session will also need, promote
it: write it into `scripts/` with the contract above, document it where agents
are told to look, and wire it into the gate if it is a check rather than a
one-off.
