# public

Rules that apply because this repository is public, or will be. Kept only when
the operator chose public visibility; deleted otherwise.

⚠ **"Will be public" counts as public from today.** Making a private repository
public does not un-publish what its history contains, and a history rewrite at
that point is a bad day. Decide once, at bootstrap, and hold to the stricter
reading.

---

## 1. Nothing that fingerprints a private system

[`../security/secrets.md`](../security/secrets.md) covers credentials. This is
the wider rule, and it catches things that are not credentials at all:

⛔ Real hostnames and domains. Account, project, tenant, namespace and zone
identifiers. Internal paths, especially absolute ones with a username in them.
Credential filenames specific to one machine. The names of private projects.
Email addresses. Internal service names and topology.

None of it is a credential. All of it is a map, and a map is what makes the
next attempt cheap.

⭐ **In an example, use an obvious placeholder.** `example.com`, `OWNER/REPO`,
`ACCOUNT_ID`. A reader can tell a placeholder from a real value; a scanner
cannot.

Before publishing, sweep and then **read the hits**:

```bash
git ls-files -z | xargs -0 grep -nIE '[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}'
```

```bash
git ls-files -z | xargs -0 grep -nIE '\b[0-9a-f]{24,}\b'
```

```bash
git ls-files -z | xargs -0 grep -nIE '([A-Za-z]:[\\/]|/home/|/Users/)[A-Za-z0-9]'
```

⚠ **A sweep finds the shapes it knows.** It will not find a password that looks
like a word, a hostname that reads as prose, or a page of correct-looking
examples that happens to describe a real system. The sweep narrows the reading.
It does not replace it.

⛔ **Sweep the whole history, not just the tree**, if the repository had a
private life before this decision.

---

## 2. The licence is load-bearing

A public repository with no licence is not open source. Default copyright
applies, which means nobody may legally use it, and the ambiguity is worse than
either clear answer.

- Pick one at bootstrap. [`../../LICENSES/`](../../LICENSES/) carries the texts
  and the guidance.
- The copyright holder comes from the machine's git configuration, never from a
  hardcoded value.
- ⛔ **Check the licences of anything vendored or derived from.** Compatible is
  not the same as permissive, and attribution obligations are real. Record the
  determination and the evidence for each, so nobody re-derives it, and so a
  wrong one can be traced.

---

## 3. Attribution and contribution

- The rule from [`../conventions/git.md`](../conventions/git.md) holds and
  matters more here, because the history is readable by anyone: ⛔ **no tool is
  credited in a commit.**
- Decide whether contributions are accepted at all, and say so in the README. A
  repository with no answer collects pull requests nobody triages, which is
  worse for the person who wrote one than a clear no.
- ⛔ **Nothing an agent does reaches another repository.**
  [`../security/remote-ops.md`](../security/remote-ops.md).

---

## 4. A security contact

⭐ **Say where to report a vulnerability, and how long a reporter should expect
to wait.** Without it, a finder's options are a public issue or silence, and
both are bad.

`SECURITY.md` carries it. Keep it short: where to send it, what to include,
what happens next. Do not promise a timeline you will not meet.

---

## 5. CI

⭐ **Actions minutes are free on public repositories and billed on private
ones.** That is the practical reason CI defaults on here and off there.

Two things to get right because they are public:

- ⛔ **A workflow triggered by a fork's pull request must not have access to
  secrets.** The trigger that grants them to untrusted code is a known
  escalation path. Use the untrusted trigger for untrusted input.
- ⛔ **Pin third-party actions to a commit, not a tag.** A tag moves, and a
  moved tag runs code you did not review with the permissions you granted.

---

## 6. The README is the front door

It is read by people deciding in fifteen seconds whether this is worth their
time, by search engines, and by agents.

- **What it is, in one sentence, at the top.** Not the motivation. The thing.
- **Why it exists**, if that is not obvious from what it is.
- **How to run it**, with commands that actually work on a fresh clone.
  ⚠ Do the literal fresh-clone test. A quick start that works only in the
  author's dirty tree is the most common broken thing in a public repository.
- **Honest limits.** What this does not do, and what it is not for. A limit
  stated is a defect not filed against you later.
- **Where the documentation is.**

⛔ **No fabricated badge, benchmark or status.**
[`../conventions/prose.md`](../conventions/prose.md).

---

## 7. Issue and pull request templates

Worth having once anyone other than the operator opens one, and not before.
[`../../dotfiles/github/`](../../dotfiles/github/) carries them.

⚠ **Do not ask for information you will not use.** A long form on a project
with three open issues costs the reporter and buys nothing.
