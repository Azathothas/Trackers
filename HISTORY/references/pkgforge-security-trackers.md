# pkgforge-security/Trackers

**Verdict: anti-pattern exhibit**

## Provenance

| item | value |
| --- | --- |
| repository | `https://github.com/pkgforge-security/Trackers` |
| commit read | `7f2d00b329172cc41c90922d0796f52739af0d75` |
| tree in this repo | [`references/pkgforge-security__Trackers/tree`](../../references/pkgforge-security__Trackers/tree) |
| tracker | [`references/pkgforge-security__Trackers/issues.json`](../../references/pkgforge-security__Trackers/issues.json) -- issues **and** pull requests, both states, capped at 100 |
| read on | 2026-08-29 |

```bash
cat references/pkgforge-security__Trackers/COMMIT
```

**Not obtained:** Discussions (GraphQL only; the credential-free route is REST).
Review comments. Issue comments except where a section below quotes one.
[`references/PROVENANCE.md`](../../references/PROVENANCE.md) is the full gap list.

## Findings

The closest prior art, and archived. Read from
`.github/workflows/fetch_update_trackers.yaml` @ `7f2d00b`, not from its README.

It publishes five files: `trackers_general.txt`, `trackers_stable.txt`,
`trackers_anime.txt`, and the two concatenations `trackers_all_general.txt`
(general + stable) and `trackers_all.txt` (all three).

**Its README is wrong about its own sources.** The README lists
`https://newtrackon.com/list` (an HTML page). The workflow fetches
`https://newtrackon.com/api/stable` (`text/plain`). The workflow's *comments*
are misaligned with its *commands* by one line, which is the likely origin of
the error the register inherited as `C-20`.

Corroboration that the code, not the README, is right: the published
`trackers_stable.txt` (57 entries) shares **52** entries with today's
`/api/stable` (53), and consists of bare URLs an HTML page could not yield
without a parser the repository does not contain.

**The silent failure mode, which is why this is kept as an exhibit.** Every
step is both `set +e` and `continue-on-error: true`, and each source is fetched
with `curl -qfSL ... -o FILE`. `curl -o` **truncates the output file before the
transfer**, and `-f` makes it produce nothing on an HTTP error. So a failed
fetch leaves an **empty file**, which is then concatenated with `sort -u` into
the published lists. An entire source disappears from the output and **nothing
reports it**. That is RULES 3.10's "source failed" vs. "source returned zero
trackers" invariant, violated in production, in the project this one exists to
improve on.

Two further observations:

* `sort -u` **destroys ngosang's popularity ordering**, so the derivative is
  strictly worse-ordered than its input while advertising the same content.
* `reset_commits.yaml` implements the >5000-commit orphan-branch reset that
  T-081 describes. It is safe *here* only because the repository
  stores no history worth losing -- which is precisely RULES 3.7's point.

**Why it was archived: not recoverable from the tracker.** All 24 tracker items
are **pull requests from `dependabot` and `renovate`**; there are **zero human
issues**. The tracker records no reason for archival. What it does record is the
maintenance cost the design actually generated: 100 % dependency churn against
three tag-pinned actions, over roughly two and a half years.

---

The overview, the ordering of verdicts, and **what the sweep did not establish**
are in [`../reference-sweep.md`](../reference-sweep.md). Read that first: it
opens with the limitations, and this file assumes them.
