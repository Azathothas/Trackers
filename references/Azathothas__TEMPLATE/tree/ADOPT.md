# ADOPT.md

**Bringing an existing repository under this template, without making a mess of
it.**

You are reading this because someone pasted its URL into a session pointed at a
project that already exists. That project may be barebones, small, enormous, a
monorepo, years old, undocumented, or all of those at once. It is probably
dirty. That is the normal case and it is what this procedure is for.

⭐ **This file is self-contained.** Everything you need to decide is here.
Fetch the rest only once you know which parts apply.

Base URL for anything named below:

```text
https://raw.githubusercontent.com/Azathothas/TEMPLATE/main
```

⚠ **The fetch commands in this file are written POSIX-first, and neither half
of `curl ... -o /tmp/x` is portable.** Establish two things once and use your
own spelling of them throughout:

| | on a POSIX host | ⚠ on Windows |
| --- | --- | --- |
| the fetcher | `curl` or `wget` | ⛔ `curl` in Windows PowerShell 5.1 is an **alias for `Invoke-WebRequest`** and takes different arguments. Use `curl.exe` by name, or `Invoke-WebRequest`. |
| the scratch directory | `${TMPDIR:-/tmp}` | ⛔ **`/tmp` does not exist.** Use `$env:TEMP`. |

⭐ **Probe rather than assume.** An absent tool announces itself; an aliased one
does not, and that is the whole hazard. Measured on one Windows 11 machine,
2026-08-28.

```powershell
(Get-Command curl -EA SilentlyContinue).CommandType; $env:TEMP
```

⚠ **On a Windows host, prefer the `.ps1` half of every script named below** and
run it with `pwsh -NoProfile -File`. The manifest at the end says which files
ship as a pair.

---

## ⛔ The safety contract

Read this before running anything. It is what separates adoption from damage,
and every rule exists because the opposite is tempting.

| ⛔ | rule |
| --- | --- |
| 1 | **Work on a new branch.** Never on the default branch. Create it first, before touching a file. |
| 2 | **Never overwrite an existing file.** If the target exists, write the template's version beside it with a `.template-new` suffix and show the diff. The human decides. |
| 3 | **Never delete anything.** Not build output, not dead code, not a stale document. Deleting is a separate decision made later, by a human, with the reason written down. |
| 4 | **Never rewrite history.** Not a rebase, not an amend, not a force push, not a filter. Whatever is in the history stays there for now. |
| 5 | **Never commit until the human has seen the diff.** Stage, show, wait. |
| 6 | **The project's existing conventions win.** If it already has a style, a linter, a commit format, a directory layout: that is the convention. You are adding what is missing, not replacing what works. |
| 7 | **Nothing runs that writes outside the repository.** No global config, no installs, no changes to the machine. |
| 8 | **A found secret is reported, never fixed silently.** Rotation is the owner's, and it comes first. |

⚠ **The single most likely way to make this worse is to be helpful.** A dirty
repository looks like it is asking to be tidied. It is not. It is asking to be
measured, so its owner can decide what to tidy.

---

## Phase 0. Measure. Change nothing.

**Fetch the probe and run it.** It is read-only and it exits 0 whether or not
anything is missing.

⚠ `BASE` and `SCRATCH` below are the two values you established at the top of
this file. They are named rather than hardcoded because neither has a portable
spelling.

```bash
BASE=https://raw.githubusercontent.com/Azathothas/TEMPLATE/main
SCRATCH=${TMPDIR:-/tmp}
```

```bash
curl -sSL -o "$SCRATCH/doctor.sh" "$BASE/scripts/doctor/doctor.sh"
```

```bash
sh "$SCRATCH/doctor.sh" --json
```

On a Windows host with no POSIX shell, fetch `scripts/doctor/doctor.ps1`
instead and run it with `-Json`. The two emit the same schema.

⭐ **Read `repo.ecosystems` from the output.** It is derived from the manifest
files actually present, which tells you which dotfiles apply without guessing
from directory names.

Then **fetch the checks and run them against the project as it is.** This is
the diagnostic, and it is the most valuable thing this template gives an
existing repository. Each one is read-only.

```bash
for c in check-no-secrets check-docs check-markers check-control-bytes; do curl -sSL -o "$SCRATCH/$c.sh" "$BASE/scripts/common/$c.sh"; done
```

```bash
sh "$SCRATCH/check-no-secrets.sh" --public
```

```bash
sh "$SCRATCH/check-docs.sh"
```

```bash
sh "$SCRATCH/check-control-bytes.sh"
```

⚠ On a Windows host with no POSIX shell, fetch the `.ps1` of each name instead
and run it with `pwsh -NoProfile -File`. The two halves answer identically.

⚠ Expect these to fail loudly on a dirty repository. **That is the output, not
an error.** A first run producing forty findings has done its job.

Two more that need no fetching and answer questions nobody thinks to ask:

```bash
git ls-files --eol | grep -v 'i/lf' | grep -v 'i/-text' | head -40
```

That names every tracked file whose committed line endings disagree with the
repository's own attributes. ⚠ It is invisible to `git diff`, because the index
is normalised either way, and very visible to every tool that reads the working
tree.

```bash
git count-objects -vH
```

⚠ A repository whose history is far larger than its checkout is usually
carrying build output, a dependency directory, or a large file somebody
committed once. That is a finding, not a task: fixing it is a history rewrite,
which rule 4 forbids here.

---

## Phase 1. Report, and stop.

⛔ **Present the findings and wait.** Do not start adopting.

Rank by consequence. For each finding give the owner what they need to decide
without redoing your investigation:

| field | what it must contain |
| --- | --- |
| **What** | the symptom in one sentence, with a path |
| **Why it matters** | the concrete consequence. ⚠ "Untidy" is not a consequence. |
| **Severity** | and the honest reason for it |
| **Fix** | what you would do, and what it costs |
| **The alternative** | including "leave it and write it down as accepted" |

⭐ **Put anything the secret sweep found at the top, on its own.** It is the one
class where the order of operations matters: rotate first, then decide about
the tree, and a history rewrite does not un-publish anything.

Then propose an **adoption set**: which parts of the template this project
should take, sized to what it is. The next section is how to size it.

⛔ Wait for an explicit yes. "Leave it" is a complete and final answer for any
item.

---

## Phase 2. Adopt, additively.

Only after approval.

```bash
git switch -c template-adoption
```

⚠ If that fails because the branch exists, stop and ask. A half-finished
adoption from a previous session is a state to reconcile, not to build on.

Then, per file in the approved set:

- **Target does not exist**: write it. This is most of the work and it is safe.
- **Target exists**: ⛔ write the template's version to `PATH.template-new` and
  show `git diff --no-index PATH PATH.template-new`. Do not merge them
  yourself unless the human asks. Leave the `.template-new` file in place; it
  is reviewable and it is obviously temporary.
- **Target exists and is better**: say so, and take nothing. ⭐ This is a real
  and common outcome, and reporting it is worth more than a change.

⛔ **`.gitignore` and `.gitattributes` are appended to, never replaced.** They
carry project-specific rules that took someone real time to work out. Append a
clearly marked block:

```bash
printf '\n# ---- from the template, added YYYY-MM-DD ----\n' >> .gitignore
```

⚠ **Adding an ignore rule does not untrack what is already tracked.** If the
sweep found committed junk, the rule alone changes nothing, and untracking it
is a separate approved decision.

---

## Sizing: what to take, by what the project is

⭐ The probe's `repo.ecosystems` and the repository's size decide this. Take
the smallest set that answers a finding you actually reported.

| the project | take | leave |
| --- | --- | --- |
| **Barebones**, a few files, no docs, no CI | nearly everything: the router, the record, the conventions, the dotfiles for its ecosystem, the checks, CI | the work-model machinery until there is work to track |
| **Small**, real code, some docs, no discipline | the checks first, then the conventions they enforce, then the router. ⭐ Checks before rules: a rule nobody checks is a preference. | the templates for documents the project does not yet need |
| **Large**, many contributors, existing process | ⭐ the checks and the security documents ONLY, at first. The project has conventions; yours would be a second set. | the router, the record, the work model. Those replace a process that already exists. |
| **Monorepo** | ⛔ adopt at ONE level and prove it there before touching another. Usually one package, or the root tooling and nothing under it. | anything that implies a single work model across independently owned packages |

⛔ **Never adopt the work model into a project that already has one.** A
sequence of numbered units or a backlog index landing on top of an existing
issue tracker produces two records that disagree within a week, and the one
people trust will be whichever they saw first.

⚠ **Never adopt the prose rule into a repository with an established voice.**
It is opinionated on purpose. Offer it; do not apply it.

---

## The manifest

Fetch only what the approved set names, under the base URL at the top, with the
fetcher you established there.

⛔ **Every check under `scripts/common/` ships as a PAIR**: `NAME.sh` and
`NAME.ps1`, same rules, same exit codes, same `--json` answer. Fetch the half
the project will actually run, or both.

⚠ **On Windows, fetch the `.ps1`.** A native PowerShell session is not Git
Bash: measured on one Windows 11 machine, it had no `sed` at all, and `sort`
resolved to PowerShell own `Sort-Object` alias rather than the coreutils
binary. The missing tool fails loudly; the aliased one succeeds and returns a
different answer, which is the worse of the two.

**Always worth having, in any project:**

| path | what it gives you |
| --- | --- |
| `scripts/doctor/doctor.sh` | the probe. Every later session runs it, and a session on a different machine needs it most. |
| `scripts/doctor/doctor.ps1` | its twin, for a host with no POSIX shell |
| `scripts/doctor/README.md` | its contract and schema |
| `scripts/common/check-no-secrets.sh` | what must never be published |
| `scripts/common/check-remote-items.sh` | ⭐ verifies what dependency bots and contributors open, rather than trusting the description. Needs `gh` and `jq`. |
| `scripts/common/check-gate.sh` | ⭐ runs every check above in one command and reads each exit code unpiped. A skip is reported as a skip. |
| `scripts/README.md` | the contract every check follows |
| `docs/security/secrets.md` | what never enters the tree, and what to do when something did |
| `docs/security/remote-ops.md` | the tiers governing anything outside this machine |

**Once there are documents to keep honest:**

| path | what it gives you |
| --- | --- |
| `scripts/common/check-docs.sh` | links resolve, shell blocks parse, prose rules hold |
| `scripts/common/check-control-bytes.sh` | ⭐ a literal control byte in ANY text file. `grep` skips such a file and `git diff` shows no diff for it, so it is invisible to both review tools at once. |
| `scripts/common/check-changelog.sh` | order, dates and record-pointers in `CHANGELOG.md`. ⚠ Exit 2 where there is no changelog. |
| `scripts/common/check-placeholders.sh` | no template placeholder left in a real file |
| `docs/conventions/docs.md` | one fact one home, and the changelog rules |
| `docs/conventions/shell.md` | ⭐ quoting, exit codes, streams, line endings, platform traps. Useful in any repository, opinionated about none of them. |
| `docs/conventions/forbidden-patterns.md` | the table to grep yourself against |
| `scripts/common/check-markers.sh` | ⚠ the character allowlist and the marker density. **Offer it; do not apply it.** It is the most opinionated check here and a repository with an established voice should not inherit it by accident. |

**Once the project carries third-party source:**

| path | what it gives you |
| --- | --- |
| ⭐ `docs/methodology/vendoring.md` | patch what you vendor, record the change neutrally, reconcile a release by reading rather than by preferring. ⛔ It also closes the topic of upstreaming, which is the rule an agent in somebody else's repository most needs. |
| `scripts/common/mine-repo.sh` | fetch a repository's tracker and tree and keep them, rather than writing a fetcher per session |

**Once the project wants the working method:**

| path | what it gives you |
| --- | --- |
| `docs/methodology/gate.md` | what a unit of work passes before it is done |
| `docs/methodology/reviews.md` | the three review lenses |
| `docs/methodology/sessions.md` | what a session owes, and how one is resumed |
| `docs/methodology/ingest.md` | the long form of this file, for the deeper pass |
| ⭐ `docs/agent-tooling.md` | what tool does what job, and where each lives. Fetch it early: it is what stops a session installing something on somebody else's machine. |
| `docs/containers.md` | only if the work needs a machine this host is not |
| `docs/templates/AGENTS.md` | the router, if the project has none. ⛔ It is a SKELETON: every double-brace marker and every guidance comment comes out when it is filled in, and `check-placeholders` is what proves it. A project that kept them has a router that says nothing, and one adopter shipped exactly that. |

**Ecosystem files**, one per ecosystem the probe reported:

| path | note |
| --- | --- |
| `dotfiles/common/gitattributes` | ⛔ append, never replace |
| `dotfiles/common/gitignore` | ⛔ append, never replace |
| `dotfiles/common/editorconfig` | safe to add if absent |
| `dotfiles/NAME/gitignore` | where NAME is node, python, rust, go, web, dotnet, java, c-cpp, shell |
| `dotfiles/os-editor/gitignore` | always applicable |

⚠ **Files under `dotfiles/` ship without a leading dot on purpose.** A real
`.gitignore` inside the template would apply to the template. Rename on the way
in, and check the result: a dot-file that did not get its dot is invisible in a
listing and does nothing, which looks exactly like success.

```bash
git check-ignore -v path/that/should/be/ignored
```

**CI**, only if the project has none or has asked for it:

| path | note |
| --- | --- |
| `dotfiles/github/workflows/gates.yml` | a scaffold. ⛔ Pin the actions to commits before using it. |
| `dotfiles/github/workflows/secret-sweep.yml` | pairs with the secret check |

⛔ **Never overwrite an existing workflow.** A broken CI configuration is worse
than none, and the one already there is load-bearing for someone.

---

## Phase 3. Verify, then hand back.

Run the same checks you ran in phase 0, and **compare against those numbers**.
An adoption that did not move a number did not do anything.

```bash
sh "$SCRATCH/check-docs.sh"
```

```bash
sh "$SCRATCH/check-no-secrets.sh" --public
```

Then review your own work, with three different questions rather than one
sweep written up three times:

1. **What did I change that I was not asked to change?** ⭐ On an adoption this
   is the pass that matters most. Every unrequested change is a surprise in
   somebody else's repository.
2. **Can each check I added actually fail here?** Plant the defect it exists to
   catch and read the exit code, unpiped. A check that has never been seen to
   refuse is a check nobody knows works.
3. **Which sentence in my report is not backed by something I can point at?**

⚠ A pass with no findings means that pass was too shallow. Say what it swept,
and what would have had to be true for it to fire.

Then hand back:

- the **diff**, as a summary: files added, files proposed, nothing deleted;
- the **before and after** numbers from the checks;
- every **`.template-new` file still awaiting a decision**;
- what you **did not** adopt, and why;
- ⛔ anything the secret sweep found, restated at the top;
- the branch name, and the fact that nothing has been merged.

⛔ **Do not merge the branch. Do not push it unless asked.** Adoption ends with
a reviewable branch and a report.

---

## If something goes wrong

The whole procedure is designed so this is cheap:

```bash
git switch -
```

```bash
git branch -D template-adoption
```

Nothing was deleted, nothing was committed to the default branch, and no
history was rewritten. ⭐ That is the point of rules 1 through 5, and it is why
they are not negotiable even when they feel slow.
