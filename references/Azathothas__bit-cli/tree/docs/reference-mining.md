# How bit-cli mines a reference

The procedure for studying somebody else's project: cloning it, reading it,
**reading its issues and pull requests**, and writing down what a later session
can act on.

**Who this is for.** Any session whose work is to clone, mine, survey or
investigate. It is named `reference-mining.md` rather than `references.md` on
purpose, so it is not confused with `reference/README.md`, which holds the
lessons, or [`TODO/reference-map.md`](../TODO/reference-map.md), which holds
the licence determinations.

It exists because the same failures keep recurring: a tree skimmed once and
dismissed, a citation that stops resolving after a trim, and a sweep that never
opened an issue tracker.

## 1. The order, and it is not negotiable

1. clone shallow
2. **capture the SHA, before anything else**
3. read the code
4. **read the issues and the pull requests, both states**
5. trim by **deleting**, never by moving
6. write the corpus entry, the lesson, and the filed entries

**Step 4 is the one that gets skipped and it is where the engineering arguments
are.** A repository shows you what somebody built. Its tracker shows you what
broke, what was measured, what was refused and why, and what the maintainer
says the project is actually for.

## 2. Cloning, and the two rules that keep a citation alive

```bash
git clone --depth 1 -q "https://github.com/<owner>/<repo>.git" <name>
git -C <name> rev-parse HEAD
```

**Capture the SHA before stripping `.git`.** Once it is gone the commit is
unrecoverable and every line citation becomes unverifiable. `reference/` is
gitignored on `main` and lives on the `references` branch, so the SHA in the
corpus entry is the only provenance that survives to another machine.

This rule is stated first because it was learned the hard way here. The
2026-08-21 pass captured no SHA for any of its twenty-two trees, and the
2026-08-24 re-mine could establish only that nothing had changed, not what had.

**Trim by deleting.** A trim that moves paths invalidates every citation
already written, including the ones in the entry being written. Strip vendored
dependency trees, build output, `node_modules`, CI caches, images, binaries,
archives and lock files. Keep source, tests, docs, and every licence file.

**Keep every licence file, not the first one.** `n0-mainline` is dual licensed
and the corpus copy kept only `LICENSE-MIT`, so the record said MIT alone for
three days.

## 3. Reading the tracker

Both states. `open` alone is a bug list; `closed` is where the decisions are.

```bash
gh api repos/<o>/<r> --jq '"open \(.open_issues_count) stars \(.stargazers_count) pushed \(.pushed_at)"'
```

```bash
gh api "repos/<o>/<r>/issues?state=all&per_page=100" --paginate --jq '.[] | "\(.number)\t[\(.state)]\t\(if .pull_request then "PR" else "IS" end)\t\(.title)"'
```

```bash
gh api repos/<o>/<r>/issues/<n> --jq '"[\(.state)] \(.title)\n\(.body)"'
```

```bash
gh api repos/<o>/<r>/issues/<n>/comments --jq '.[] | "--- \(.created_at)\n\(.body)"'
```

**`/issues` returns pull requests too**, and `open_issues_count` counts both.
Discriminate on `.pull_request` or a dependency bump gets reported as an issue.

**Read the comments, not only the body.** An issue still open with a maintainer
comment saying "check the latest version" means fixed in code and unconfirmed
by the reporter, which is neither fixed nor open. Report the state actually
found.

Cache the tracker JSON under `.tmp/` so a rate limit does not cost a re-read.
If `gh` cannot reach a host, say so in the corpus entry rather than skipping
quietly. A Codeberg repository answers on its own Forgejo API without a
credential, which is how `fake-torrent-client`'s single issue was read.

**What to search a tracker for:**

| ask | why it pays |
| --- | --- |
| the thing the entry is about | somebody has usually tried it, and a closed "nice idea, never built" is cost evidence code cannot give |
| memory, OOM, large file, concurrency | those numbers are measured on real hardware |
| the failure mode being designed against | its absence is information too |
| "is this superseded by" | whether the reference is live or archaeology, in the maintainer's own words |
| the maintainer's answers | "this cannot be done because" is a costing that would otherwise have to be derived |
| the test-harness confessions in pull request bodies | the richest single lines a tracker produces |

**`gh` is read only here.** Never a write verb, never an issue or comment
created on anybody else's repository. [`TODO/RULES.md`](../TODO/RULES.md)
section 6a is the rule and it is absolute.

## 4. Passes, and what makes one real

**At least three per reference, and a pass is only real if it asks a different
question.** Three readings with one question is one pass written up three
times.

| pass | the question |
| --- | --- |
| 1, WHAT | what is this, what problem does it solve, what shape is it |
| 2, MECHANISM | the actual construction, in its source, at `file:line` |
| 3, THE HARD PART | how it handles the thing `bit-cli` finds hard |
| 4, AGAINST bit-cli | what transfers, what must not, and what it **changes** |

**Where a reference cannot support that many, say which and why.** Claiming a
fourth pass over a tree with nothing to say about the fourth question is worse
than admitting three.

## 5. Verdicts

Every reference gets exactly one, and a verdict without a reason is an opinion.

| verdict | meaning |
| --- | --- |
| **ADOPT** | a specific mechanism, cited at `file:line`, is going into a named entry |
| **CONFIRMS** | `bit-cli` already does this. Independent evidence, not new work. Name the entry that closed it |
| **ANTI-PATTERN EXHIBIT** | kept on purpose. A shipped defect is worth more than an absence. Record the defect and whether the project's own tests missed it |
| **FILED ELSEWHERE** | not this topic's. Write it into the `TODO/` file that owns it. Never dropped, never chased here |
| **REFUSED** | with the reason, so no future session re-derives it |

## 6. Licences

**Read the licence file on disk, every time, and record it in
[`TODO/reference-map.md`](../TODO/reference-map.md) and in `RESEARCH.md`
section F.** Do not accept a claim that a repository is permissively licensed,
including from the operator. That claim was checked on 2026-08-24 against an
organisation of 33 repositories and did not hold.

Where there is no licence file, check the manifest, the README, source headers
and other repository metadata, and **record which of those the determination
came from**. A manifest key is a real declaration and it is weaker than a file.
Distinguish an explicit declaration from an inference and from an absence.

**Corpus files are never copied into this repository.** `intermodal`, CC0-1.0,
is the one tree that may be. Reading a non-permissive tree is allowed and
copying from it is not, and a non-permissive licence does not by itself forbid
writing an independent implementation from the observed behaviour.

**Where a script or a mechanism is ported, write our own implementation from
the observed behaviour, cite theirs at `path:line` with its SHA, and record the
licence in the port's own header.**
`scripts/make-client-profile.ps1` is the worked example: `joal` is Apache-2.0,
nothing was copied, and the header names the SHA and says what the port does
differently.

**A port has to be re-checked, because upstream moves and the port does not.**
`scripts/check-client-profile.ps1` is that half: it runs the derivation against
both clients at their newest stable release and their newest prerelease, and
fails when the construction the port reproduces is no longer in the source it
was read from.

```bash
pwsh -NoProfile -File scripts/check-client-profile.ps1
```

It is not a gate. It needs the network and reads two public repositories, so it
runs on the same cadence as `scripts/upstream-scan.ps1` rather than on every
push.

**What it found on its first run is the reason to have it.** The port hardcoded
one character of the peer id that both clients derive. For a prerelease neither
derives what was hardcoded, so the port described a client that does not exist
for exactly the builds a mask most wants to imitate. The port had reproduced a
format faithfully and the client not at all, which is the defect it was written
to catch.

If a licence expressly blocks an independent implementation from observed
behaviour, stop and file it as a question for the operator rather than deciding
it. If anything is genuinely copied rather than reimplemented, it has to appear
in `THIRD_PARTY.md` and satisfy `about.toml` and `deny.toml`.

## 7. The traps, each one paid for

1. **Skipping the tracker.** Four of the most valuable findings of 2026-08-24
   came from a tracker and none was visible in the code.
2. **Believing a doc over its code.** READMEs go stale and some are wrong on
   the day they are written. Three trees on 2026-08-24 described a transport,
   a dependency or a licence their own code contradicted. Read the doc, then
   check the code, then cite the code.
3. **Trusting a reference's own citations.** Resolve a cited issue number
   before repeating it.
4. **Grep false positives.** Grep locates; it does not confirm. Open the file.
5. **`Get-Content | Measure-Object -Line` does not count blank lines**, an
   undercount of eight to ten percent that reads like a precise figure. Use
   `wc -l`.
6. **Do not delegate a reference's reading to a subagent.** Operator ruling: a
   summary of a tree nobody opened is not evidence.
7. **Re-mine a tree even if it has been swept before.** Projects move, and the
   earlier verdict was taken against a different HEAD.

## 8. Never send prose through two shells

The hazard is not "writing a file", it is **any multi-line payload crossing a
shell**: a commit message, an issue body being quoted, a regex over a
reference's source. A PowerShell here-string written inside a `bash` command is
parsed by bash first and ends at the first apostrophe.

Write the text to a file with a file-writing tool and pass the path. A commit
body goes through `git-sync -BodyFile <path>`; anything else goes in a `.ps1`
under the scratchpad, run with `pwsh -NoProfile -File <path>`.

[`TODO/RULES.md`](../TODO/RULES.md) section 5 has what that has cost twice.

## 9. What a sweep owes

**Per reference:**

- an entry in `reference/RESEARCH.md`, tiered into Tier 1, 2 or 3 with the
  ranking argued, carrying the commit SHA, the licence, the passes taken, the
  verdict, and **what the pass did not do**
- a lesson in `reference/README.md` carrying the **actual code lines**. The
  test is specific: if a session doing the implementation cannot act from it
  without re-cloning, it failed
- a row in [`TODO/reference-map.md`](../TODO/reference-map.md) with the licence
  and where the determination came from
- a line in `RESEARCH.md` section H, so a repository that was skipped is named
  with its reason rather than simply absent

**Per finding:** an entry in the `TODO/` file that owns the category, with a
priority, an effort, a `Source:` line and an acceptance command. A "prove" with
no command is a paragraph.

**The corpus tree is left clean**: no vendored dependency trees, no build
output, no binaries, no images.

## 10. What this procedure does not give you

- **It does not tell you the reference is correct.** Ranking it against
  `bit-cli` is a judgement and it is yours to defend with a citation.
- **It does not survive a trim that moves paths**, which is why section 2
  exists.
- **It does not make a subagent's summary trustworthy.**
- **It cannot see a private repository, an archived discussion or a chat
  thread**, and where the real argument lives there, the honest entry says the
  argument was not reachable.
