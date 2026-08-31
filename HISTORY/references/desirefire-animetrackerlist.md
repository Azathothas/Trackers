# DeSireFire/animeTrackerList

**Verdict: confirms**

## Provenance

| item | value |
| --- | --- |
| repository | `https://github.com/DeSireFire/animeTrackerList` |
| commit read | `e59508bebb45ee4d9850ba3807324b60f3dd5325` |
| tree in this repo | [`references/DeSireFire__animeTrackerList/tree`](../../references/DeSireFire__animeTrackerList/tree) |
| tracker | [`references/DeSireFire__animeTrackerList/issues.json`](../../references/DeSireFire__animeTrackerList/issues.json) -- issues **and** pull requests, both states, capped at 100 |
| read on | 2026-08-29 |

```bash
cat references/DeSireFire__animeTrackerList/COMMIT
```

**Not obtained:** Discussions (GraphQL only; the credential-free route is REST).
Review comments. Issue comments except where a section below quotes one.
[`references/PROVENANCE.md`](../../references/PROVENANCE.md) is the full gap list.

## Findings

Last push **2024-01-12** (~2.6 years). Not archived. 43 human issues, nearly
all "new tracker" submissions, which is a live *audience* around a dead
generator.

HISTORY/reference-sweep.md asks whether its unique entries are real. Measured
(`experiments/19`): **995 unique URLs of 1091**, against every other *primary*
source combined. It is by a wide margin the largest unique contributor in the
corpus.

**A defect in my own first measurement, recorded because it is instructive.**
The first run of `experiments/19` reported this source as contributing **0**
unique trackers. That was an artefact: the comparison set included
`pkgforge_all`, which is a **strict superset** of this source (1091 of 1091).
Comparing a source against its own downstream copy always shows redundancy.
Sources now carry a `role` and the arithmetic runs over primaries only
(`C-52`).

**What is still unknown, and it is the half that decides the question:**
whether those 995 unique entries are *alive*. Uniqueness measured; liveness
not. That is P2 work, and until it is done "abandoned but unique" is not yet
"abandoned but valuable".

---

The overview, the ordering of verdicts, and **what the sweep did not establish**
are in [`../reference-sweep.md`](../reference-sweep.md). Read that first: it
opens with the limitations, and this file assumes them.
