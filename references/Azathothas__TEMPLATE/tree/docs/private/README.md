# private

Rules that apply because this repository is private. Kept only when the
operator chose private visibility; deleted otherwise.

⚠ **Private is a setting, not a property.** A repository can be made public
later, a collaborator can be added, an account can be compromised, and a backup
can end up somewhere unintended. Everything here assumes the tree may be read
by someone it was not written for.

---

## 1. What actually relaxes

Almost nothing, and the list is short on purpose:

- **Internal hostnames, account identifiers and service topology may appear**,
  because the whole point is to describe the system being built. The public
  rules against fingerprinting do not apply.
- **Worked examples may use real names.** A runbook that says "the staging
  cluster" is more useful than one that says "a cluster".
- **A licence is optional.** Add one if the code may ever be shared; the
  ambiguity costs nothing while it is only yours.
- **A public security contact is unnecessary.** There is no external reporter.

---

## 2. What does not relax

⛔ **Credentials.** Everything in
[`../security/secrets.md`](../security/secrets.md) applies unchanged. A private
repository is not a secret store: it is readable by every collaborator, every
integration, every backup, and every machine with a clone. A token in a private
tree is a token that will be rotated in a hurry later.

⛔ **The remote rules.** Everything in
[`../security/remote-ops.md`](../security/remote-ops.md) applies unchanged.
Private does not mean the agent may push, and it does not widen what may be
touched elsewhere.

⛔ **`gh` still does not read your other private repositories.** An
authenticated CLI can, and that is exactly why the restraint is written down:
it is policy, not a capability limit.

⛔ **The gate, the reviews and the conventions.** None of them are about
visibility. A private project that skips the driven pass ships the same class
of defect as a public one, with fewer people to notice.

---

## 3. CI costs money here

⚠ **Actions minutes are free on public repositories and billed on private
ones.** That is the practical reason CI defaults off in a private project, and
it is a real constraint rather than a preference.

Three workable answers, in the order they are usually right:

1. ⭐ **Local gates, run before every commit.** A single command that runs every
   check and prints one verdict. This is the default here and it is often
   enough: the checks are the same, they run faster, and they cost nothing.
2. **CI on a narrow trigger.** Only on the default branch, or only on a tag,
   rather than on every push. Most of the value at a fraction of the minutes.
3. **CI on a self-hosted runner**, if one already exists. ⚠ Not worth standing
   one up for this alone.

⛔ **Whichever you pick, say so in the project's rules**, because a session that
assumes CI will catch something will skip checking it locally. An assumed gate
that does not exist is worse than a stated absence.

⚠ **A local-only gate is as current as the machine under it.** CI installs the
current toolchain on every run; a local machine does not. Report the local
version and warn when it is behind, rather than failing on it: a stale toolchain
is not a reason to stop working.

---

## 4. If it may go public later

⭐ **Decide now, not then.** Making a repository public does not un-publish what
its history contains, and cleaning a history at that point means rewriting
every commit and rotating everything the history touched.

If public is plausible:

- Adopt [`../public/README.md`](../public/README.md) section 1 from today. It
  is the only expensive part to retrofit.
- Keep internal names in configuration and in ignored files rather than in
  tracked documents, so the split already exists when it matters.
- Pick a licence early.

If public is genuinely not plausible, say so in the project's rules, so a
future session does not spend effort on a constraint that does not apply.

---

## 5. The record still gets written

⚠ A private project has fewer readers, and the temptation is to skip the parts
that feel like they are for an audience.

They are not. The record, the handoff and the summary are for **the next
session**, which has none of this one's memory and is the most common reader
this repository will ever have. A private project running long unattended agent
chains needs them more than a public one, not less.
