# XIU2/TrackersListCollection

**Verdict: filed elsewhere**

## Provenance

| item | value |
| --- | --- |
| repository | `https://github.com/XIU2/TrackersListCollection` |
| commit read | `e9f9ba2dfea24f67d9a90ff7bd8f5fc998c3d763` |
| tree in this repo | [`references/XIU2__TrackersListCollection/tree`](../../references/XIU2__TrackersListCollection/tree) |
| tracker | [`references/XIU2__TrackersListCollection/issues.json`](../../references/XIU2__TrackersListCollection/issues.json) -- issues **and** pull requests, both states, capped at 100 |
| read on | 2026-08-29 |

```bash
cat references/XIU2__TrackersListCollection/COMMIT
```

**Not obtained:** Discussions (GraphQL only; the credential-free route is REST).
Review comments. Issue comments except where a section below quotes one.
[`references/PROVENANCE.md`](../../references/PROVENANCE.md) is the full gap list.

## Findings

Daily at 00:00 UTC, `contents: write`, `timeout-minutes: 45`, and -- better than
the pkgforge exhibit -- `concurrency.cancel-in-progress: false`, which **queues**
rather than cancelling a run in flight.

Sources: ngosang `trackers_all`, `newtrackon /api/live`, DeSireFire `AT_best`,
and `http://github.itzmx.com/...` over **plain HTTP**.

Its workflow header states, in the maintainer's own words, that after migrating
to Actions "the filtering process has been temporarily streamlined" -- an honest
admission that the published quality is currently below its own historical bar.

It sets a **browser-like User-Agent** (`Mozilla/5.0 ... Chrome/69`). That is a
data point for RULES 5.3 / `C-43` and is **filed there**, not adopted:
this sweep fetched every source in the census with one honest descriptive
User-Agent and received **zero 401/403 responses**, so no impersonation is
warranted.

---

The overview, the ordering of verdicts, and **what the sweep did not establish**
are in [`../reference-sweep.md`](../reference-sweep.md). Read that first: it
opens with the limitations, and this file assumes them.
