# secrets.md

What never enters the tree, and what to do when something did.

`python3 scripts/check-no-secrets.py --public` is the mechanical half and runs
in the gate. This page is the rest.

---

## The rule

⛔ **A secret never enters the repository, a log line, a commit message, a
document, an issue, or a record.**

Not an expired one. Not one that looks redacted. Not one in an example. Not
a value kept "for a moment" and removed before committing.

A secret is any value that grants access: an API token, a password, a private
key, a session string, a connection string with credentials in it, a signed
URL, a webhook secret, a recovery code. ⚠ An account identifier, a project id
and an internal hostname are not credentials, but they are fingerprints of a
private system and they do not belong in a public repository either.

---

## ⭐ The credential class this project actually handles

⛔ **A private tracker's passkey is a credential, and this project ingests
them.**

A private tracker authenticates by a token carried in the announce URL:
`?passkey=<token>`, or a long opaque path component beside `announce`. They
reach the upstream lists because a contributor pasted their own URL, and they
belong to a real person whose tracker can see every use of them.

**Measured 2026-08-31 (C-70):** six distinct such credentials are in the
captured source fixtures, on seven URLs, and
`python3 scripts/generate.py --offline` publishes all seven into
`trackers_all.txt`. Nothing between the fixture and the dataset refuses them.

⚠ **The fixtures themselves are not edited.** They are verbatim captures, and a
fixture somebody rewrote is not a capture. The defect is in the pipeline, and
[T-107](../../TODO/sources.md) is the entry that closes it.

⛔ **The check holds a ceiling at the measured count and the ceiling comes off
when the entry closes.** Until then a seventh distinct credential fails the
gate, which is what matters: the corpus is re-fetched from upstreams that keep
publishing these.

---

## An agent never asks for a value

⛔ **The operator names what they hold. You say where it goes.**

This holds even where the operator offers. A value pasted into a session is in
a transcript, and a transcript is not a secret store.

Two consequences that come up constantly:

- ⛔ **Do not set a secret you cannot read back.** Writing a platform secret
  clobbers a working value with no way to restore it. That is the operator's
  action, always.
- ⚠ **"Blocked on credentials" is usually the wrong words.** Check what is on
  disk and what the project's own tooling already reads first. Nothing in this
  repository's gate needs a credential at all, and the reference sweep has a
  credential-free route (RULES 16).

---

## Where a secret would live, if this project had one

| kind | where |
| --- | --- |
| local development | an ignored environment file, with a committed `.example` twin carrying fake values and every required key |
| CI | the provider's own secret store, set by the operator |
| a token an agent may use | an ignored file the project documents **by name**, never by value |

⭐ **The `.example` twin is the useful half.** It documents which keys exist and
what shape each takes, so a reader knows what to obtain without anyone
publishing what they are.

⛔ **The ignore rule is listed before the file exists.** A credential file added
to [`../../.gitignore`](../../.gitignore) after the fact was trackable in
between, and once staged a later ignore rule does nothing.

---

## When something got in

Order matters, and the first step is not the git one.

1. ⛔ **Rotate the credential.** Immediately, before anything else. It is
   compromised from the moment it was written, and nothing below changes that:
   it was readable, and it may be cached, mirrored or already indexed.
2. **Tell the operator.** They own the account and may need to check for use.
   Do not quietly clean it up.
3. **Remove it from the working tree**, and add the ignore rule.
4. **A history rewrite is the operator's call and the operator's action.** It
   is destructive, it breaks every clone, and it is tidying after the fix
   rather than the fix.

⚠ **Do not report this as handled until the rotation is confirmed.** The
tempting failure is to remove the file, see a clean tree, and call it done.

---

## A public repository

This one is public. Everything above, and one addition: ⛔ **nothing that
fingerprints a private system.** Real hostnames, account identifiers, internal
paths, credential filenames specific to one machine, the names of private
projects.

⚠ **A grep sweep finds the shapes it knows and a green run is not a
clearance.** It cannot find a password that looks like a word, and it will not
tell you that a page of correct-looking examples describes a real system. The
sweep narrows the reading; it does not replace it.
