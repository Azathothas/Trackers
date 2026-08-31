# TEMPLATE

A starting point for a new software project worked on with an AI agent.

It carries the methodology, the conventions, the dotfiles, the licences and the
environment probe that every project here begins with, so an agent does not
have to rediscover them, and so nobody has to retrofit them later.

It holds no project code and is not itself a project.

---

## ⭐ Use it, whatever the situation

**Nothing to clone. Nothing to fill in. No prompt to pick.** Paste this into
the agent of whatever you are working on:

```text
Fetch and read this in full before doing anything else:
    https://raw.githubusercontent.com/Azathothas/TEMPLATE/main/ROUTE.md
Work out which job this is, ask me in this session if the tree does not settle
it, and follow the row you land on. Do not end your turn to ask me: ask and
keep going. Stop for my approval before writing anything.
```

[`ROUTE.md`](ROUTE.md) classifies the situation from the tree, asks you one
grouped question only if it has to, and routes. It has two tables:

- **the lifecycle jobs**, which change what the repository is: a new project,
  adopting an existing one, a new unit of work, resuming, reworking, adopting
  without any agent-facing content, re-syncing a project that adopted this
  earlier, or changing the template itself;
- ⭐ **the work itself**, which is most sessions: studying another project,
  taking a measurement, running the gate, a review, vendoring, writing a
  document, touching a shell, reaching for a tool, or needing a machine this one
  is not.

⚠ **The second table was missing and the heading above was a promise this file
did not keep.** An operator wanting research done had to name the files by hand
and the session got lost anyway, because nothing routed it.

⚠ **The rest of this section is the long form**, for when you already know
which of those you want.

---

## Use it: start a new project

**1. Clone it into a new directory.**

```bash
git clone https://github.com/OWNER/REPO.git my-new-project
```

```bash
cd my-new-project
```

**2. Fill in `bootstrap/ANSWERS.md`.** Two minutes. Every field has a default,
and blank is a valid answer to all but three.

**3. Paste `bootstrap/prompts/00-new-project.md`, with your answers, into a
fresh agent session.**

The agent detaches the remote, measures the machine, selects what applies,
deletes what does not, writes the project's own router and record, plans the
first unit of work, and stops for your approval. ⛔ It writes no implementation
code in that session.

---

## Use it on a repository that already exists

⚠ [`ROUTE.md`](ROUTE.md) above reaches this on its own. Paste the following
only when you already know adoption is what you want:

```text
Fetch and read this in full before doing anything else:
    https://raw.githubusercontent.com/Azathothas/TEMPLATE/main/ADOPT.md
Then bring this repository under it, following its safety contract.
Measure first, report, and STOP for my approval before changing anything.
```

[`ADOPT.md`](ADOPT.md) is self-contained: it carries the safety contract, the
procedure and a manifest of what else to fetch and when. The agent works on a
branch, overwrites nothing, deletes nothing, rewrites no history, and hands
back a reviewable diff.

⚠ It works on a barebones repository, a large one, and a monorepo, but not
identically: the sizing table in that file says what to take in each case, and
the answer for a large project with an existing process is deliberately small.

The longer form, when the template's files are already sitting beside the
project, is [`bootstrap/prompts/01-existing-project.md`](bootstrap/prompts/01-existing-project.md).

---

## What is in it

| directory | what |
| --- | --- |
| [`ROUTE.md`](ROUTE.md) | ⭐ the one paste that works out which job this is |
| [`AGENTS.md`](AGENTS.md) | the router an agent reads first |
| [`ADOPT.md`](ADOPT.md) | the self-contained procedure for an existing repository |
| [`MAINTAIN.md`](MAINTAIN.md) | the prompt to paste when improving this template |
| [`bootstrap/`](bootstrap/) | the once-only path: the procedure, your answer sheet, the prompts |
| [`docs/`](docs/) | the methodology, the conventions, the security rules, the templates |
| [`dotfiles/`](dotfiles/) | ignore, attribute, editor and CI files, by ecosystem |
| [`LICENSES/`](LICENSES/) | canonical SPDX texts, and which four must never have their notice altered |
| [`scripts/`](scripts/) | the environment probe, and the checks a project inherits. ⭐ [`docs/agent-tooling.md`](docs/agent-tooling.md) is what to reach for before installing anything. |
| [`tools/`](tools/) | no tools, deliberately. Read its README before adding one. |

Start at [`docs/README.md`](docs/README.md) for the map of which document
answers which question.

---

## Try the probe without cloning anything

```bash
sh scripts/doctor/doctor.sh
```

```bash
pwsh -NoProfile -File scripts/doctor/doctor.ps1
```

It reports the host, the shell, the installed tools with their versions, the
git state and the ecosystems the tree declares. Read-only, exits 0 whether or
not anything is missing, and no network call unless asked.
[`scripts/doctor/README.md`](scripts/doctor/README.md) is its contract.

---

## It checks itself

Every rule here that can be enforced by a script is, and the scripts run on
every push, on Linux and on Windows.

```bash
sh scripts/common/check-docs.sh
```

```bash
sh scripts/common/check-twins.sh
```

| check | refuses |
| --- | --- |
| `check-gate.sh` | ⭐ runs all of the below in one command. A skipped check reads as a skip, never as a pass, and zero passes is red. |
| `check-docs.sh` | a broken link, a shell block that does not parse, banned vocabulary, a page nothing links to |
| `check-markers.sh` | ⭐ a character outside the five, in **any** tracked text file, and a page carrying so many markers that none of them are visible |
| `check-placeholders.sh` | a template placeholder left in a real file |
| `check-control-bytes.sh` | a literal control byte, which makes a file invisible to `grep` and to `git diff` at once |
| `check-no-secrets.sh` | a credential or a fingerprint of a private system, in tracked **and** untracked files |
| `check-twins.sh` | ⭐ the two implementations of a check drifting apart |
| `check-remote-items.sh` | ⭐ a pull request whose claims do not survive checking |
| `mine-repo.sh` | ⚠ a helper, not a check: it fetches a repository's tracker and tree and **keeps** them, and refuses to write where an ignore rule would swallow the result |

⭐ **Each one has been mutation-proved**: the defect it exists to catch was
planted, and the check was watched refusing it. A guard that has never been
seen to refuse is a guard nobody knows works.

⚠ **And a green gate is a statement about the checks that RAN.** The page
joiner in `mine-repo` discarded an entire comment corpus for months while every
check above passed, because nothing compared it. A consumer of this template
found it and reported it. [`scripts/README.md`](scripts/README.md) carries what
that cost and what now runs.

---

## What it is opinionated about

Four things, because each of them cost somebody real time to learn.

**A router, not a rulebook.** The agent-facing entry point stays small and
links everything rather than restating it. A prerequisite file that grows past
a few hundred lines costs every session its reading budget and forks from the
documents it was supposed to point at. The routing table maps a task to the
files that task actually needs.

**A gate with three parts.** The automated suites prove the code. Driving the
real thing proves the product and the platform. The deep reviews prove the
composition. ⭐ Each is blind to the class the other two catch, which is why
there are three.

**The record is part of the change.** The file that says where the work stands
is edited in the same change as the work, never after it. A session that fixes
something and leaves the record saying it is open has made the next session
read a lie first.

**Every rule says what it cost to learn.** Nothing here is a preference wearing
a rule's clothes, because that is what teaches a reader to discount the rules
that matter. [`AGENTS.md`](AGENTS.md) states the standard this is held to.

---

## Honest limits

- **It is opinionated.** If you disagree with the three-part gate or the record
  discipline, most of the value is gone and you should fork it rather than
  fight it.
- **It assumes an agent that can read and run things.** A harness with no shell
  gets less from this.
- **The probe is measured on Windows, Linux and macOS shells, but the numbers
  in its README are from one Windows machine on one day.** Re-measure rather
  than quoting them if the answer matters.
- **The CI workflow files under `dotfiles/` are scaffolds.** They pin every
  action to a commit and carry a working `dependabot.yml`, but the build and
  test steps are commented out until a project fills in its own.
- **This repository's own CI is real and runs on every push**, on Linux and
  Windows. ⚠ A green run proves the checks pass, not that the rules are right.
- **Nothing here is legal advice**, least of all the licence guidance.

---

## Licence

**0BSD.** See [`LICENSE`](LICENSE). `SPDX-License-Identifier: 0BSD`.

Use it, copy it, change it, ship it. No attribution required, no notice to
carry, no conditions at all.

⭐ It is deliberately the most permissive option available, and the reason is
practical rather than ideological: a coding agent asked to read or reuse a file
will sometimes decline over licence terms, and every condition is one more
thing for it to decline over. 0BSD removes them all while staying OSI-approved
and SPDX-listed, so it is recognised rather than argued about.

⚠ CC0 and the Unlicense are not used here despite being similar in spirit.
CC0 explicitly withholds patent rights, which several organisations refuse on
that basis, and the Unlicense has a long-running reputation for legal
ambiguity. 0BSD is a plain BSD-family text with the attribution clause removed,
which is the least surprising thing to encounter.

The canonical licence texts under [`LICENSES/`](LICENSES/) are from the SPDX
license-list-data project and carry their own terms.
