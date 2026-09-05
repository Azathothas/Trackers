# Progress

**The record.** Everything that changes from session to session is here: the
measured baseline, what the last session did, and the work order.

⛔ **It carries no history and every session rewrites it in full.** For history,
read [`../HISTORY/`](../HISTORY/) and the git log.

**Read [`../docs/AGENTS.md`](../docs/AGENTS.md) first.** It carries the
absolutes, the routing table, and the box on why a session may not stop or
defer. [RULES.md](RULES.md) is normative for all of it. Every entry, one line
each: [INDEX.md](INDEX.md).

> **The shape this file must keep:** the state line with the session's start
> instant in ISO 8601 UTC; the measured baseline, citing the file that owns each
> number; the entry counts; what the session did; what is in progress;
> **Start here next session** as an ordered list with entry ids; and open
> questions for the operator. `python3 scripts/check-todo.py` prints the counts,
> and none of them is typed here.

---

## State

- **Last session:** started `2026-09-05T01:00:00Z`, ended on operator
  instruction (RULES 10.2, first way). A **measurement pass**: the exclusion
  route RULES 4 requires was built, the corpus was probed for the first time,
  and six entries closed.
- **Branch:** `main`, public at `https://github.com/Azathothas/Trackers`.
- ⛔ **Still nothing published as data.** No dataset exists at any public URL.
  The corpus has now been measured once, from a runner, and those records live
  under `experiments/results/` as evidence rather than as a published dataset.

## Measured baseline

**Every corpus figure lives in
[`../HISTORY/corpus-baseline.md`](../HISTORY/corpus-baseline.md)** and nowhere
else, with the command behind each. Do not restate one here; cite it.

Network figures are from workflow run **`33940109175`**, 2026-09-05, on
`ubuntu-24.04` and `ubuntu-22.04`. Committed under `experiments/results/`,
because a workflow artefact expires after 90 days and git does not.

| | |
| --- | --- |
| UDP arbitrary-port egress | **true**, both images, tier-0 loopback plus four tier-1 controls |
| BEP 15 connect | **10 of 11**, both images. ⚠ 10 is the ceiling: one target has no IPv4 address |
| IPv6 egress | **false**, both images, stack present |
| TCP ports | open 80, 443, 2095, 6969, 8080. **None blocked** (`C-71`) |
| HTTP tracker discrimination | **4 of 6** proved tracker, both images, `announce_sent: false` |
| DNS resolver divergence | **0 of 17** divergent, and **1 divergent** on an earlier run. Thin, and carried as [T-007](claims.md) |
| Corpus, accepted dataset, transport mix | [`corpus-baseline.md`](../HISTORY/corpus-baseline.md) |
| First corpus sweep | run **`33938543488`**, 200 of 1327 sampled: `live` 25, `degraded` 1, `unknown` 162, `unmeasurable` 12, **`dead` 0** |
| Operators refusing us by BEP 34 | **8 endpoints across 7 hosts**, in that 200 (`C-72`) |
| Test suite | **196** tests, no network |
| Reference corpus | **10** repositories, **216** comment threads, **501** comments |
| Local gate | `python3 scripts/check-gate.py --strict`: 14 pass, 1 expected skip |
| Private-tracker credentials in the published plaintext | **0**, refused by the pipeline (`C-70`, [T-107](sources.md)) |
| CI | `gate.yml` green on `ubuntu-24.04` and `windows-2025`; `p0-ground-truth.yml` green on both Linux images. Confirmed by looking |

⛔ **`live` 25 of 200 is not a liveness rate.** It is one datacenter, IPv4
only, on one day, from a single observation of each tracker.
`MIN_SAMPLES_FOR_DEATH` is 3, so nothing can be `dead` until history exists
([T-040](scoring.md)).

## Counts

Run `python3 scripts/check-todo.py`. It re-derives every number from the rows
and fails a gate when [INDEX.md](INDEX.md)'s table disagrees. **Nothing is
blocked.**

## What the last session did

**Six entries closed.** [T-032](measurement.md) BEP 34 exclusion,
[T-107](sources.md) credential refusal, [T-029](measurement.md) the bounded
sweep, [T-022](measurement.md) closed by deciding *not* to scrape,
[T-003](claims.md) release behaviour, [T-024](measurement.md) the first corpus
measurement. Three decisions recorded: **D13** what BEP 34 binds, **D14** what
replaced the credential ceiling, **D15** whether the UDP probe scrapes.

⭐ **The work order it was handed could not be executed in order.** RULES 4
forbade a corpus-wide probe until BEP 34 was honoured, and the entry that would
honour it was not in the order at all. RULES is normative over this file, so
T-032 went first and unblocked four entries at once.

**Two claims were refuted by measurement.** `C-17`: moving a git tag does
**not** move the release's `target_commitish`, while `tarball_url` follows the
tag, so two consumers reading one release disagree silently -- delete-and-recreate
is the route for [T-064](publication.md). `C-15`: an asset *is* replaceable at
a stable URL, but the URL can serve the previous bytes afterwards **carrying the
old `ETag`**, for a variable window measured at 0 s and at 10-to-40 s minutes
apart. Publication must not assume read-after-write.

**Five reviews ran, and every one found something.** They are under
[`../HISTORY/reviews/`](../HISTORY/reviews/) and the findings that changed the
tree are these:

1. `sweep()` lost every other tracker's measurement when one probe raised.
   Twenty tests missed it because each passed a prober that returns.
2. ⛔ **The exclusion gate was built and then bypassed the same day.**
   `experiments/02` and `05` contact trackers and consulted nothing, and
   `p0-ground-truth.yml` fired twice after the gate landed. No refusal is known
   to have been violated -- all 17 pinned subjects permit us -- and that is luck.
3. **The fix for that broke the other runner image and the build stayed green**:
   importing `src/` pulled in the Python 3.11 floor, `ubuntu-22.04` ships 3.10,
   and `continue-on-error` hid it. A step now fails the job when an experiment
   wrote no result.
4. The README told operators to ask and there was no contact route in the
   repository at all -- RULES 4 half-satisfied inside the change meant to
   satisfy it.
5. The routing table read as a start-of-session step, so the page describing
   finding 2 in advance was never opened.

⚠ **Three claims made *by the reviews* did not survive checking** and are left
visible in them rather than edited away.

## In progress

**Nothing half-finished.** Every entry touched is either closed with its
acceptance recorded, or open with what remains written into it.

## Start here next session

1. **[T-027](measurement.md)** - the value gate, and it is now answerable.
   Liveness exists for a 200-tracker sample and the credential refusal is a
   second axis of value no upstream in the corpus provides. ⛔ **A negative
   answer is a successful outcome.**
2. **[T-012](claims.md)** - whether our identity gets us blocked. Two axes
   (`C-63`): the `User-Agent` **and** the BEP 20 `peer_id` prefix. ⚠ Twelve
   cells over the HTTP corpus is roughly twelve thousand requests at somebody
   else's expense, so RULES 4's ceiling decides the schedule before the
   statistics do. It is a workflow over days, not a command.
3. **[T-028](measurement.md)** - the newTrackon cross-check. Cheap now that
   records exist, and the first output no upstream publishes: **disagreement
   between independent observers.**
4. **[T-033](measurement.md)** - unify the codecs the experiments and the probe
   each carry a copy of. New, from review 2, and it corrects
   [T-020](measurement.md)'s acceptance.
5. **[T-001](claims.md)** - run a real torrent client against the plaintext.
   P0, independent of the above.
6. **[T-064](publication.md)** - release channels. Its platform half is
   measured: no tag move, no read-after-write.

**Deliberately deferred:** [T-044](scoring.md), the scoring model. It waits on
history existing ([T-040](scoring.md)); choosing now would fit a model to zero
samples.

**[T-031](measurement.md) is the highest-value entry and is deliberately not
numbered**, because it is not sequential: one indirect-liveness mechanism
serves IPv6-only, i2p, yggdrasil, `wss` and blocked-vantage cases at once. ⭐ It
has a third cheap route now: a BEP 34 denial cannot protect a corpus URL written
as an IP literal, and propagating one across a shared *resolved* address is the
same machinery.

**When this order is exhausted**, take the next entry from [INDEX.md](INDEX.md)
by priority. ⛔ Do not stop because the list above ran out.

## Questions the operator has answered

All settled before 2026-08-29 unless dated. **Closed; do not re-raise.**

1. **May a session create throwaway releases here?** Yes, in this repository.
   D10 and RULES 13.1. Tag them `test-*` and delete them once the answer is
   recorded. Exercised by [T-003](claims.md); the repository was left with 0
   releases and 0 tags.
2. **What defines membership of `foss.txt`?** Derived, plus a labelled seed.
   D9, settled in [T-046](scoring.md).
3. **Is the roughly three-hour probe cadence acceptable?** Yes: publish hourly,
   probe each tracker on its own stated interval, defaulting to three hours.
   D7, settled in [T-026](measurement.md).
4. **What is authorised outward-facing?** Every action belonging to this
   repository, and nothing outside it, ever. RULES 13.

## Open questions for the operator

**None blocking.** Three things to know rather than re-derive:

1. ⚠ **BEP 34 lookups send tracker hostnames to a public resolver.** A
   deliberate trade recorded in `src/trackers/bep34.py`: the alternative is the
   host's own resolver, which is the recorded way this mechanism fails silently
   in production. Overturning it is a decision to take **before** the first
   scheduled sweep, not after.
2. ⚠ **The health sweep has no `schedule:` trigger.** Adding one is not a
   detail: the cadence is D7's, the budget [T-026](measurement.md)'s and the
   architecture [T-084](operations.md)'s.
3. The first publication of this repository **force-pushed over a placeholder
   commit**. It is the only history rewrite this project has performed and
   [`../docs/conventions/git.md`](../docs/conventions/git.md) section 2 records
   it as an exception rather than a precedent.
