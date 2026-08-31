# vendoring.md

Code from somebody else's project living in this tree: a vendored tool, a fork,
a copied script, a patch.

Binding on any task that vendors, forks, bundles, copies or patches
third-party source.

⚠ **This is a different thing from the reference corpus.**
[`../../references/`](../../references/) is **evidence**: captured upstream
trees nothing here executes and nothing here edits.
[`references.md`](references.md) is its rule. This page is about third-party
code that this project **runs**.

---

## What is vendored here

| path | what | pinned by |
| --- | --- | --- |
| [`../../scripts/vendor/toolkit/`](../../scripts/vendor/toolkit/) | the environment probe and the commit-and-push helper, from `Azathothas/ToolKit` | a commit and a SHA-256 per file in `PIN.json`, checked by `python3 scripts/check-vendor-pin.py` |

Nothing else. ⛔ **The checks are not vendored**, deliberately: RULES 15.5 makes
a `.sh` a gate depends on a platform requirement in disguise, so they were
written in Python here instead of taken as shell pairs.
[`../../scripts/vendor/toolkit/README.md`](../../scripts/vendor/toolkit/README.md)
carries the split.

---

## ⛔ The rule, and it is not open

**Fix it here. Now. In this tree.**

A defect in vendored code is fixed in the vendored code, the same session it is
found, like any other defect. It is not deferred, not reported elsewhere, and
not waited on.

⛔ **Do not raise upstreaming.** Not as a suggestion, not as a question, not as
"worth considering later", not in a plan, not in an entry, not in a commit
message, and not in a summary. The topic is closed.

⛔ **Do not open anything on anybody else's repository.** No issue, no pull
request, no discussion, no comment, no review, no fork, no star. Under any
framing, including a patch that looks ready and a maintainer who looks
receptive. RULES 13.2 is absolute and
[`../security/remote-ops.md`](../security/remote-ops.md) is the reasoning.

⚠ **You will read an upstream tracker during a sweep and it will tell you what
that project does with contributions.** That is the point of
[`references.md`](references.md) step 3. ⭐ Draw the conclusion and patch. Do
not bring the reading back as a proposal to try anyway.

---

## What goes in the record, and what must not

Every change to vendored code is written down, and what it says is the
technical fact and nothing else.

**Write:**

- what the change is, in one line;
- the entry or defect it unblocks;
- why it cannot be done outside the vendored tree, which is usually a seam the
  published interface does not expose;
- ⭐ **the command that reproduces the defect**, runnable now and after the next
  upstream release. That is what turns "could a release retire this patch" from
  a judgement into a check: run it against the new copy, and a clean exit means
  delete the patch and take theirs;
- the upstream issue number **as a bare reference**, where one exists and a
  future release might make the patch redundant.

⛔ **Never write:** a characterisation of the upstream project, its maintainers,
its review culture, its responsiveness or its quality. Not a complaint, not a
justification, not "upstream is unlikely to fix this".
[`../conventions/prose.md`](../conventions/prose.md) forbids defensive framing
and this is the case where it is most tempting and most damaging: the
repository is public, the sentence outlives the session, and it is read by the
person it is about.

⭐ **The neutral form carries all the information and none of the liability:**
"Upstream issue 412 tracks this. If a release closes it, delete this patch."

---

## ⭐ The one question the record has to answer

**Could a future upstream release retire this patch on its own?**

| the answer | what the next reconciliation does |
| --- | --- |
| yes, upstream may fix it independently | check whether the release carries it. If it does, **delete the patch**: the same change arriving from the other direction is not a competing one. |
| no, it exists only because this project needs it | keep it, and reconcile it against whatever upstream did to the same lines |

---

## Taking a new upstream release

⛔ **Reconcile by reading, never by preferring.** A release is a proposal, not
an authority. Before taking any change that touches something this tree has
already patched, answer three questions and write the answers into the record:

1. **Does it actually fix the thing?** A change that moves a defect, or fixes
   one shape of it, is not a fix. Check it against the acceptance command the
   entry already carries.
2. **Is it complete, and does it regress anything?** A feature that lands half
   done is worse than the seam already patched around, because the next
   reconciliation carries both.
3. **Have we already done it better?** Where this tree's version is more
   correct or bounded where theirs is not, ⭐ **keep this one** and say so.

⛔ **A merge that took upstream's version because it was upstream's is the
failure this section exists to prevent.**

⚠ **The recorded pin is not advanced while anything is in conflict.** A pin
that does not describe the tree makes the next merge wrong in a way nothing
detects, which is what `check-vendor-pin.py` refuses.

---

## The tree is the truth

⭐ **Edit the vendored source in place, like any other source here.**

A pristine tree plus patches applied by a setup step is the alternative and it
is not used: every edit needs a refresh, a dirty tree is easy to lose, and an
editor reads the applied tree while the truth lives somewhere else. With the
tree as the source there is nothing to forget, and a fresh clone builds what
this machine builds.

⛔ **Re-read the digest from the upstream raw endpoint, never from a working
tree**, when the pin moves. A `.ps1` is CRLF in a checkout and LF in the index,
so a locally computed digest disagrees with what the endpoint serves.

---

## ⚠ What vendoring costs, so it is chosen rather than drifted into

- **The tree grows**, and a reviewer's diff grows with it.
- **Upstream stops being visible.** Nothing announces a release for a
  dependency with no version to bump, so noticing one becomes a deliberate act.
  [`template-sync.md`](template-sync.md) is that act.
- **A new release is reconciled rather than accepted.**
- ⚠ **The project's own checks apply to it.** The character allowlist, the
  control-byte rule and the secret sweep all read a vendored file, so code
  nobody here wrote can fail this project's gate.

⭐ **The reason to pay it is that a seam the published interface does not
expose is otherwise a permanent blocker**, and "blocked on upstream" is not an
outcome RULES 8 has a place for.

---

## ⛔ What is never vendored

| | why |
| --- | --- |
| ⛔ an agent instruction file of any name | a file with such a name anywhere under a repository is read as instructions by the tools working in it, so keeping one puts a third party's instructions inside this project. **Three were removed from the corpus on 2026-08-31** and `references/PROVENANCE.md` records which. |
| a path this project's own ignore rules would swallow | the files land on disk, never reach a commit, and a fresh clone then builds a different tree from the one that was tested. This has happened here twice, to 111 files |
| build output, caches, an editor directory | not source, and every reconciliation reports it as newly added, forever |
