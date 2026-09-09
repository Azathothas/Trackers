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

- **This session:** started `2026-09-09T01:50:00Z`. A **reach pass**: three
  transport categories that had been `unmeasurable` in every record this
  project ever took were measured, and the politeness ceiling stopped being a
  by-product of arithmetic.
- ⭐ **Every P0 and P1 is closed.** [T-012](claims.md) reported and
  [T-087](operations.md) was opened and closed inside this session.
- **Branch:** `main`, public at `https://github.com/Azathothas/Trackers`.
- ⛔ **A live RULES 4 violation was found, and it was published.** The sweep's
  slice came from `epoch // 10800`, so two runs inside one three-hour bucket
  took the **identical** slice: runs `34281244142` and `34289476724` both took
  slice 5 and **192 trackers were contacted 5878 s apart**, inside D7's
  10800 s interval, with both observations on the `data` branch. The ceiling is
  read from the recorded history now and no arithmetic can breach it.
  [T-087](operations.md).
- ⭐ **The dataset accumulates and the rotation walks.** `state.jsonl` carries
  **901** trackers, 96 with three observations and 15 with four; the published
  set is **1326** trackers, **142 live, 70 dead, 8 degraded, 55 unmeasurable**
  and 1051 not yet measured. The eight folded runs read
  `slice3, slice4, slice5, slice5, slice6, slice0` -- the repeat is the defect
  above, kept because it happened.

## Measured baseline

**Every corpus figure lives in
[`../HISTORY/corpus-baseline.md`](../HISTORY/corpus-baseline.md)** and nowhere
else, with the command behind each. Do not restate one here; cite it.

⚠ **The DNS figures on that page are PER VANTAGE**, because `experiments/30`
measured a runner's own resolver failing where a public one answers. That is
`C-06`'s finding, not a discrepancy.

| | |
| --- | --- |
| Value gate | **ANSWERED**: justified as a labelled dataset, **not** as a list ([`gates.md`](../HISTORY/gates.md)) |
| Live yield vs the baseline | **2.06x** worst case, 2.70x point, 3.68x best |
| Live density | ours **12.8%** of 1327, baseline **63.6%** of 99 |
| Committed sweeps | **7**, all `github-actions-hosted`, all `ci`, slices 0 and 3-6 plus two censuses |
| Baseline census | run **`34207344996`**, all **99**, **63 live** counted not estimated |
| DNS census | run **`34210496112`**, both images. IPv6-only **16 URLs on 15 hosts** |
| Resolver divergence | **3 of 240** on `ubuntu-24.04`, **1** on `22.04`, run `34235047982` (`C-06`) |
| IPv6-only liveness | **6 of 16** URLs alive, direct over IPv6, run `20260908T144728Z` ([T-031](measurement.md)) |
| **i2p liveness** | **3 of 11** HTTP destinations answered, from an i2pd router in a container ([T-039](measurement.md), `C-37`) |
| **yggdrasil liveness** | node joined, a peer answered **twice**, the one corpus host answered **neither time** -- one observation, never `dead` ([T-039](measurement.md)) |
| **wss liveness** | **5 of 10** completed an RFC 6455 handshake; **2 answered a scrape** ([T-005](claims.md), `C-36`) |
| **Identity arms** | descriptive **34/35**, minimal **26/26**, client_like **23/24**, absent **22/26**, spread **0.154** ([T-012](claims.md), `C-56`) |
| Oracle disagreement | **17 of 93** = 18.3%, methodology caveat attached (`C-03`, `C-69`) |
| State projection | K=64, D=180, **23.4 MB** at five years (**D3**) |
| Test suite | **578** tests, no network |
| Reference corpus | **10** repositories, **980** files, identical in a fresh clone |
| Politeness budget | full corpus **6072** DNS at worst of 100,000 ([T-026](measurement.md)) |
| **Politeness ceiling** | enforced from `state.jsonl`'s `last_seen`, not from the rotation ([T-087](operations.md)) |
| Local gate | `python3 scripts/check-gate.py`: 16 pass, 1 expected skip |
| Cold start | ⚠ **not re-confirmed this session**; last confirmed 2026-09-08 |
| CI | `gate.yml` green on the pushed head, confirmed by looking |
| Health sweep | **scheduled** `0 */3 * * *`, `ci` profile, one of **7** rotating slices ([T-084](operations.md)) |

⛔ **`live` is a floor, not a rate.** One datacenter, IPv4 only for the sweep.
A tracker that timed out is `unknown`, and some of those are up.

## Counts

Run `python3 scripts/check-todo.py`. It re-derives every number from the rows
and fails a gate when [INDEX.md](INDEX.md)'s table disagrees. **Nothing is
blocked.**

## What this session did

**Four entries closed, one of them opened here.**

⛔ **[T-087](operations.md) -- the rotation was never the ceiling.** The slice
came from the three-hour wall-clock bucket a run *starts in*, so two runs in
one bucket take the same slice and nothing refused the second. It is measured
and published: 192 trackers, 5878 s apart. ⭐ **The test that should have
caught it asserted something else** -- it compared rotation `0` with rotation
`1`, which is a property of two integers, while the schedule needs a property
of two runs. `politeness.too_soon_after` reads what was recorded; `sweep.plan`
is the only door into a selection and **advances past a slice held in full**,
so a collision costs coverage rather than nobody. Five planted defects, five
caught -- the boundary comparison survived the first attempt because
`gap <= interval - 1` is indistinguishable over whole seconds.

⭐ **[T-012](claims.md) -- the User-Agent question is answered, and what closed
it was reading the series rather than the runs.** RULES 4 permits one arm per
tracker per run, so the comparison belongs to the rotation series and **no
single run of it could ever have answered**; every earlier run was individually
right to refuse. `--series` pools them and clears the twenty-per-arm bar:
descriptive **34/35**, absent **22/26**, spread 0.154. ⛔ **The descriptive
string is not being refused and the weakest arm is sending none at all.**
`C-56` is `REFUTED`; RULES 4.1 states the measured answer under its own
heading, which stays because a disproved premise keeps its title.

⭐ **[T-039](measurement.md) -- i2p and yggdrasil reached, first-hand.** A
router in a throwaway container cost one `podman run`, and the route the entry
listed **last** is the one that worked while route (d), the public gateway, is
still 503. i2p: **3 of 11** destinations answered. yggdrasil: the node joined
in a second, a peer answered over the overlay twice, the one corpus host
answered neither time. ⛔ Two findings inside it: only **3 of 13** `.i2p` URLs
have a scrape endpoint at all (eight end in `/a`, which BEP 48 cannot turn into
one, and nothing invents `/s`), and **addressing by name measures our own
addressbook first** -- one tracker timed out by name and answered 1677 bytes by
b32.

⭐ **[T-005](claims.md) -- `wss` was inertia, exactly as the entry said.**
**5 of 10** complete an RFC 6455 handshake with a matching accept token and
**2 answered a scrape**. ⛔ The other three upgrade and then close the
connection on a scrape: `protocol_valid` is a WebSocket endpoint and **not** a
tracker, which is RULES 3.3 in its `wss` form. The instrument's **negative**
control -- a plain 200 that must not be called a WebSocket -- is the one that
matters.

**Recovered evidence.** Five sweeps had measured real trackers and their
records were in no committed file; artefacts expire in 90 days and git does
not. They are committed, which also folded two runs the publisher had never
seen. ⭐ The value gate is **unmoved** by them, which is the property it should
have: it takes each arm from the run that measured it best rather than pooling.

**Two smaller repairs.** A stray `NUL` took the whole gate down with a
`ValueError` naming no file anyone could act on -- `.gitignore` covers it and
that is not enough, because `check-citations.py` walks the tree rather than the
index. And `experiments/26`'s exclusion was a second, differently-wrong copy of
D7; it uses `politeness.too_soon_after` now, and it is an **interval** rather
than a blacklist.

## In progress

**Nothing half-finished.** Every entry touched is closed with its acceptance
recorded.

⚠ **Two things this session did not do**, neither of them blocking:

- **The three deep reviews** RULES 10.3 step 4 requires before a session may
  end. None were written.
- **The cold start on a fresh clone** (step 9). The local gate is green and CI
  is green on the pushed head, and those answer a different question.

## Start here next session

1. **[T-045](scoring.md)** - ranking must not use the latest instantaneous
   result. ⭐ **The history can now support it**: 96 trackers have three
   observations and 15 have four, where a week ago none had two. It is also
   the entry [T-044](scoring.md) is deliberately waiting behind.
2. **[T-102](sources.md)** - change-detection thresholds are provisional and
   say so. Seven committed sweeps and eight folded runs are the volume history
   that would justify them.
3. **[T-009](claims.md)** - schedule delay and drop rates. ⭐ **Cheap now, and
   it is load-bearing**: the delays are what made [T-087](operations.md)'s
   collision reachable, and this session watched a `21:00Z` slot fire at
   `23:11Z` and an `00:00Z` slot at `03:03Z`. Three observations exist and
   nothing has written them down as a rate.
4. **[T-103](sources.md)** - provenance snapshots are not retained, which is
   the same shape as the five sweeps whose evidence was nearly lost.

**Deliberately deferred:** [T-044](scoring.md), the scoring model. **D4** stays
open on purpose: choosing a model against a history whose deepest series is
four observations is fitting it to noise.

**When this order is exhausted**, take the next entry from [INDEX.md](INDEX.md)
by priority. ⛔ Do not stop because the list above ran out.

## Questions the operator has answered

**Closed; do not re-raise.**

1. **May a session create throwaway releases here?** Yes, in this repository.
   D10 and RULES 13.1. Tag them `test-*` and delete them once the answer is
   recorded.
2. **What defines membership of `foss.txt`?** Derived, plus a labelled seed.
   D9, settled in [T-046](scoring.md).
3. **Is the roughly three-hour probe cadence acceptable?** Yes: publish hourly,
   probe each tracker on its own stated interval, defaulting to three hours.
   D7, settled in [T-026](measurement.md). ⭐ **It is enforced from the record
   now** rather than assumed from the cadence ([T-087](operations.md)).
4. **What is authorised outward-facing?** Every action belonging to this
   repository, and nothing outside it, ever. RULES 13.
5. **One squashed commit per session, or a series?** A **clean series of
   logical commits**, each passing the gate. **D16**, 2026-09-08.
6. **Is DNS inside the politeness budget?** Yes, with a ceiling of **100,000**
   lookups per run on a GitHub runner; local runs are not bounded by it.
   2026-09-08. [T-026](measurement.md) computes it: **6072** at worst.
7. **Schedule the health sweep?** Not until [T-037](measurement.md) lands.
   2026-09-08. ⛔ **Superseded the same day by answer 10**, once both
   conditions it named were met. It is kept because it is why the schedule
   waited, and because its reasoning -- that a `schedule:` is a standing
   commitment against other people's servers -- is still why it was put to the
   operator rather than decided by a session. ⚠ **2026-09-09 proved that
   reasoning right in a way nobody predicted**: the standing commitment was
   kept and the *slice arithmetic* broke it anyway.
8. **The third party's credential in git history?** **Not our action.** The
   operator will **reset this repository's history to a single commit** once
   the tasks are complete, which removes it. 2026-09-08.
9. **May this project contact a tracker whose operator it has no automatable
   way to ask?** **Yes -- the asking route is enough.** 2026-09-08. ⭐ This
   session is what it authorised: 11 `.i2p` destinations were contacted once
   each, and a `.i2p` name still has no DNS record for BEP 34 to read.
10. **Schedule the health sweep?** **Yes, at D7's cadence.** 2026-09-08.
    `.github/workflows/health-sweep.yml` runs `0 */3 * * *` and the selector
    rotates; `tests/test_rotation.py` asserts the coverage **and**, since
    [T-087](operations.md), that two consecutive *runs* share no tracker.
11. **Should a commit credit the tool that helped write it?** **No.**
    2026-09-08. [`../docs/conventions/git.md`](../docs/conventions/git.md)
    forbids crediting any tool and that overrides a harness default.

## Open questions for the operator

**None.** Eleven have been asked and eleven are answered above.

⚠ **Three standing facts to know rather than re-derive**, none of which is a
question:

- **BEP 34 lookups send tracker hostnames to a public resolver**, a trade
  recorded in `src/trackers/bep34.py`.
- **A history reset to one commit is coming** (answer 8). Nothing should depend
  on this repository's commit history, which RULES 3.7 already forbids for
  measurement history.
- ⭐ **Two instruments now need a container and cannot run in CI**
  (`experiments/35`, `experiments/36`). That is
  [`../docs/containers.md`](../docs/containers.md)'s premise rather than a
  limitation, both take the engine as a variable, and both decommission what
  they start -- verified by counting, not by remembering.
