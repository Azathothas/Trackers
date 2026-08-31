# git.md

How this repository is committed to, and what may reach a remote.

What an agent is *authorised* to do outward-facing is
[`../../TODO/RULES.md`](../../TODO/RULES.md) section 13, which is normative.
This page is the mechanics.

---

## 1. No tool is credited

⛔ **Every commit is attributed to the operator alone.**

- No trailer naming a model, a vendor or a tool.
- No "generated with" line, in a commit message, a pull request body, a tag or
  a release note.
- No tool name in the commit body.

**This overrides any default the harness asks for.** Several agent harnesses
instruct the model to append a co-author trailer. That instruction does not
apply here, and a commit carrying one is corrected before it is pushed rather
than explained afterwards.

The work is published under the operator's name. The history is theirs and
tooling is not a contributor to it.

⚠ **Refuse the commit rather than rewriting the message.** Silently editing
somebody's commit message is worse than declining to make the commit: the
author never learns the rule and the next message carries the same line.

### The identity is set per invocation

A machine whose global config says something else must still produce the right
commit, and `--author` sets only the author, so both are set:

```bash
git -c user.name="$NAME" -c user.email="$EMAIL" -c committer.name="$NAME" -c committer.email="$EMAIL" commit --file message.txt
```

⭐ **Reach for the script rather than typing that.**
[`../../scripts/vendor/toolkit/git-sync.sh`](../../scripts/vendor/toolkit/)
enforces every rule on this page mechanically, refuses an attribution line
rather than stripping it, and verifies the identity that actually landed rather
than assuming the flags took:

```bash
sh scripts/vendor/toolkit/git-sync.sh --check
```

```bash
pwsh -NoProfile -File scripts/vendor/toolkit/git-sync.ps1 -Check
```

⛔ **Nothing about that script knows who you are.** The identity comes from the
flags or from `git config`, and it refuses rather than guessing.

---

## 2. What may reach the remote

This project's push policy is **commit-and-push to its own remote**,
`Azathothas/Trackers`, on `main`. RULES 13.1 is the standing authorisation and
RULES 13.2 is the absolute limit: every other repository is read-only, and
nothing may be opened on one under any framing.

Commit freely and locally. Commit per task, small and logical.

⚠ **An unattended run reaches a point where pushing looks like finishing the
job.** RULES 10.3 is what actually ends a session, and it has nine steps of
which the push is the seventh.

### History rewriting

⛔ **No force push, no rebase of anything published, no amend of a commit that
has left the machine**, with two exceptions and both are the operator's to
authorise:

- a rewrite to remove a credential, which comes after the rotation and is not
  itself the fix ([`../security/secrets.md`](../security/secrets.md));
- the first publication of this repository, which replaced a placeholder
  commit that existed on the remote before the tree did.

---

## 3. Commit messages

- A subject line saying what changed, in the imperative.
- A body saying **why**. The diff says what.
- ⛔ **The body goes through a file.** Never typed into a shell, never a
  here-string, never an unquoted heredoc. A prose payload passed inline to a
  shell has had its backticks executed even inside a quoted heredoc, and this
  session measured a related failure: this harness's own Bash tool consumed a
  backslash escape inside a quoted heredoc twice.
  [`shell.md`](shell.md) section 1.

```bash
git commit --file message.txt
```

⛔ Read a timestamp from the machine, never type one:

```bash
date -u +%Y-%m-%dT%H:%M:%SZ
```

⚠ Dates written before a project adopts a stamping rule are **not**
retro-stamped. Nobody recorded the hour, and inventing one is fabricating
evidence.

---

## 4. What is never committed

[`../security/secrets.md`](../security/secrets.md) is the rule.
`python3 scripts/check-no-secrets.py --public` is the check, and it runs in the
gate. The short form: no credential of any kind, including an expired one and
one that looks redacted; no local environment file; no build output.

⛔ **[`../../references/`](../../references/) is NOT on that list.** The corpus
is the evidence, and a conclusion nobody can re-check is an opinion.
`python3 scripts/check-corpus-integrity.py` counts the disk against the index
because two ignore rules once dropped 111 corpus files from every clone without
a word in `git status`.
[`../methodology/references.md`](../methodology/references.md) is the rule.

⚠ **An ignore rule only applies to files git is not already tracking.** Adding
a line untracks nothing. Check rather than assume:

```bash
git check-ignore -v PATH
```

⚠ **Anchor every path rule with a leading slash.** An unanchored `out/` matches
at any depth, including inside a captured upstream tree, which is exactly how
20 files of somebody else's committed instrument output were dropped from every
clone.

---

## 5. CI

- ⚠ **A commit message that mentions a CI skip marker skips CI.** GitHub reads
  the marker anywhere in the message and does not care what the sentence around
  it meant. `git-sync` refuses one unless its flag was passed, because a commit
  explaining the marker in prose once shipped with every job silently skipped.
- ⚠ **A documentation-only push that skips CI must actually be
  documentation-only.** A "docs" push carrying a source file is exactly the one
  that needed the run, and `.github/` is never safe to skip: a workflow edit is
  the change whose effect is visible only in a run.
- ⛔ **Confirm CI is green on the pushed head by looking.** RULES 10.3 step 8
  carries what this project has already paid for not looking.
