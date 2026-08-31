# secrets.md

What never enters the tree, and what to do when something did.

---

## The rule

⛔ **A secret never enters the repository, a log line, a commit message, a
document, an issue, or a handoff.**

Not an expired one. Not one that looks redacted. Not one in an example. Not
"just for a moment, I will remove it before committing."

A secret is any value that grants access: an API token, a password, a private
key, a session string, a connection string with credentials in it, a signed
URL, a webhook secret, a recovery code. ⚠ An account identifier, a project id
and an internal hostname are not credentials, but they are still fingerprints
of a private system and they do not belong in a public repository.

---

## An agent never asks for a value

⛔ **The operator names what they hold. You say where it goes.**

```text
operator:  I have a Cloudflare API token.
agent:     Put it in .dev.vars as CF_API_TOKEN. That file is ignored by
           .gitignore line 12. Tell me when it is there; do not paste it here.
```

This holds even when the operator offers. A value pasted into a session is in a
transcript, and a transcript is not a secret store.

Two consequences that come up constantly:

- ⛔ **Do not set a secret you cannot read back.** Writing a platform secret
  clobbers a working value with no way to restore it. That is the operator's
  action, always.
- ⚠ **"Blocked on credentials" is often the wrong words.** Check what is
  actually on disk and what the project's own tooling already reads before
  reporting a blocker. A session that says it is blocked while the value sits
  in an ignored file the project documents has not looked.

---

## Where a secret does live

| kind | where |
| --- | --- |
| local development | an ignored environment file, with a committed `.example` twin carrying fake values and every required key |
| deployed | the platform's own secret store, set by the operator |
| CI | the CI provider's secret store, set by the operator |
| a token an agent may use | an ignored file the project documents **by name**, never by value |

⭐ **The `.example` twin is the useful half.** It documents which keys exist and
what shape each takes, so a reader knows what to obtain without anyone
publishing what they are.

⛔ **List the ignore rule before the file exists.** A credential file added to
`.gitignore` after the fact was trackable in between, and once staged, a later
ignore rule does nothing: `.gitignore` only applies to files git is not already
tracking. This has happened: a rule was written for one token, a second token
arrived with a different filename, and it sat tracked for three days.

⚠ **Re-exclude credentials last** in a `.gitignore`, by name rather than by
pattern precedence, so no re-inclusion rule above can reach them.

---

## Logging

⛔ Never log a secret, or a prefix of one long enough to be useful.

Route every secret-shaped value through one redaction helper and make that the
only way such a value reaches output. A redactor used in most places is a
redactor that fails, because the one call site that forgot is the one that
leaks. One function, and a check that nothing else formats those fields.

⚠ Log the **fact** rather than the value. "authenticated as the deploy
identity" is useful; the token that authenticated is not.

---

## When something got in

Order matters, and the first step is not the git one.

1. ⛔ **Rotate the credential.** Immediately, before anything else. It is
   compromised from the moment it was written, and the rest of this list does
   not change that. A rewritten history does not un-publish a value: it was
   readable, it may be cached, mirrored, or already indexed.
2. **Tell the operator.** They own the account and they may need to check for
   use. Do not quietly clean it up.
3. **Remove it from the working tree**, and add the ignore rule.
4. **A history rewrite is the operator's call and the operator's action.** It
   is destructive, it breaks every clone, and it is not the fix. It is tidying
   after the fix. Never run one unprompted, and back up before running one at
   all.

⚠ **Do not report this as handled until the rotation is confirmed.** The
tempting failure is to remove the file, see a clean tree, and call it done.

---

## A public repository

Everything above, and one addition: ⛔ **nothing that fingerprints a private
system.** Real hostnames, account identifiers, project ids, internal paths,
credential filenames specific to one machine, the names of private projects.

None of it is a credential and all of it is a map. Read
[`../public/README.md`](../public/README.md) before adding a worked example to
anything that will be published.

A sweep before publishing, over the whole tree:

```bash
git ls-files -z | xargs -0 grep -nIE '[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}'
```

```bash
git ls-files -z | xargs -0 grep -nIE '\b[0-9a-f]{24,}\b'
```

Then read every hit. ⚠ A grep sweep finds the shapes it knows. It does not find
a password that looks like a word, or a hostname that looks like prose, and it
will not tell you that a file full of correct-looking examples describes a real
system. The sweep narrows the reading; it does not replace it.
