# Azathothas/TEMPLATE

**Verdict: adopt**

## Provenance

| item | value |
| --- | --- |
| repository | `https://github.com/Azathothas/TEMPLATE` |
| commit read | `6eaf4b5fbe8e3207de231f86641e95179e3bc79f` |
| tree in this repo | [`references/Azathothas__TEMPLATE/tree`](../../references/Azathothas__TEMPLATE/tree) |
| tracker | **none fetched -- the repository has 0 issues and 0 pull requests.** There is no `issues.json` for it, and that absence is the finding rather than a gap |
| read on | 2026-08-29 |

```bash
cat references/Azathothas__TEMPLATE/COMMIT
```

**Not obtained:** Discussions (GraphQL only; the credential-free route is REST).
Review comments. Issue comments except where a section below quotes one.
[`references/PROVENANCE.md`](../../references/PROVENANCE.md) is the full gap list.

## Findings

0BSD, matching this project. `docs/methodology/references.md` read in full and
followed here; `experiments.md` governs `experiments/`. Fifteen methodology
documents exist, against the three named in HISTORY/reference-sweep.md -- `gate.md`,
`work-stages.md`, `reviews.md` and `history.md` are relevant and unread.

Its rules that changed this sweep's output: keep the corpus **tracked** (it was
initially in session-local scratch -- the exact failure the document names twice);
read the **tracker**, not only the code; and open the write-up with what was not
established.

---

---

The overview, the ordering of verdicts, and **what the sweep did not establish**
are in [`../reference-sweep.md`](../reference-sweep.md). Read that first: it
opens with the limitations, and this file assumes them.

## What was deliberately NOT adopted

**The paired PowerShell and Bash script duplication.** TEMPLATE carries every
script twice, once per shell, and the brief instructed that this be dropped
**with the reason stated** rather than silently.

The reason: this project runs in GitHub CI on `ubuntu-*` runners, and decision
D1 makes it standard-library Python with **no shell layer at all**. A second
shell dialect would be maintenance weight for a platform nothing here targets,
and the gates are Python precisely so that "never interpolate upstream content
into a shell command" is structurally impossible rather than remembered
(RULES 5.1). TEMPLATE's scripts are `.ps1`; the equivalents here are
`scripts/*.py`.

**Adopt mechanisms, not architectures.** A reference's shape is a response to
its own constraints, which are not ours.
