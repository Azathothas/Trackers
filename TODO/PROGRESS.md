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

- **Last session:** started `2026-09-08T13:30:00Z`, ended on operator
  instruction (RULES 10.2, first way). A **reachability pass**: the probe
  stopped publishing this machine's resolver as a property of a name, stopped
  being willing to dial an address that points at itself, and reached a
  category of tracker that every previous record called unmeasurable.
- ⭐ **Every P0 and P1 is closed except [T-012](claims.md)**, which waits on the sweep's rotation covering more of the corpus rather than on work.
- **Branch:** `main`, public at `https://github.com/Azathothas/Trackers`.
- ⭐ **The dataset is published and it accumulates.** `data` branch, six files
  at `raw.githubusercontent.com/Azathothas/Trackers/data/`. The chain sweep ->
  artefact -> fold -> generate -> publish is driven end to end: run
  `34281330192` folded **192 fresh observations**, taking the dataset from 276
  measured to **424 of 1334** and from 16 trackers with two observations to
  **56**. Three is what `dead` needs. Every measurement also lives under
  `experiments/results/` as evidence.
- ⛔ **Nothing is `dead`, and nothing can be.** `MIN_SAMPLES_FOR_DEATH` is 3
  and no tracker has three observations. The most any record says is
  `unknown`.

## Measured baseline

**Every corpus figure lives in
[`../HISTORY/corpus-baseline.md`](../HISTORY/corpus-baseline.md)** and nowhere
else, with the command behind each. Do not restate one here; cite it.

⚠ **The DNS figures on that page are now PER VANTAGE**, because
`experiments/30` measured a runner's own resolver failing where a public one
answers. That is `C-06`'s finding, not a discrepancy, and a single table would
be the failure that page exists to prevent.

| | |
| --- | --- |
| Value gate | **ANSWERED**: justified as a labelled dataset, **not** as a list ([`gates.md`](../HISTORY/gates.md)) |
| Live yield vs the baseline | **2.06x** worst case, 2.70x point, 3.68x best |
| Live density | ours **12.8%** of 1327, baseline **63.6%** of 99 |
| First corpus sweep | run **`33938543488`**, 200 of 1327 by stride |
| Baseline census | run **`34207344996`**, all **99**, **63 live** counted not estimated |
| DNS census | run **`34210496112`**, both images. IPv6-only **16 URLs on 15 hosts** |
| Resolver divergence | **3 of 240** on `ubuntu-24.04`, **1** on `22.04`, run `34235047982`, addresses routable (`C-06`) |
| Null-addressed hosts | **11 hosts, 14 URLs** answer `0.0.0.0` or `::`. Zero on a runner, where `getaddrinfo` returns them |
| IPv6-only liveness | **6 of 16** URLs alive, direct over IPv6, run `20260908T144728Z` ([T-031](measurement.md)) |
| Oracle disagreement | **17 of 93** = 18.3%, methodology caveat attached (`C-03`, `C-69`) |
| Within-AS8075 variation | **0** of 34 subject-days, across **5** distinct addresses (`C-03`) |
| Client compatibility | plaintext survives **aria2 1.37.0** unchanged (`C-40`, `C-41`) |
| State projection | K=64, D=180, **23.4 MB** at five years (**D3**) |
| Test suite | **486** tests, no network |
| Reference corpus | **10** repositories, **980** files, identical in a fresh clone |
| Politeness budget | full corpus **6072** DNS at worst of 100,000; **10,616** probes/day at D7 ([T-026](measurement.md)) |
| Identity arms | descriptive UA **15 of 15** against live trackers, **no verdict** under 20 per arm ([T-012](claims.md)) |
| Local gate | `python3 scripts/check-gate.py --strict`: 16 pass, 1 expected skip |
| Pre-commit hook | available, **opt-in**: `python3 scripts/install-hooks.py` |
| Cold start | confirmed on a fresh clone: gate green, `references/` and `experiments/results/` identical |
| CI | `gate.yml` green on the pushed head, confirmed by looking |
| Health sweep | **scheduled** `0 */3 * * *`, `ci` profile, one of **7** rotating slices per run ([T-084](operations.md)) |

⛔ **`live` is a floor, not a rate.** One datacenter, IPv4 only, one
observation per tracker. A tracker that timed out is `unknown`, and some of
those are up.

## Counts

Run `python3 scripts/check-todo.py`. It re-derives every number from the rows
and fails a gate when [INDEX.md](INDEX.md)'s table disagrees. **Nothing is
blocked.**

## What the last session did

**Six entries closed**, including one **`L`**: [T-031](measurement.md)
indirect liveness (**L**), [T-037](measurement.md) the resolver's opinion,
[T-041](scoring.md) the seven shapes, [T-026](measurement.md) the politeness
budget, [T-038](measurement.md) the prober's address, and
[T-064](publication.md) the release channels. [T-084](operations.md)'s `Prove`
clause is met and that entry stays open for the workflow architecture.
**Three opened**: [T-038](measurement.md) and [T-039](measurement.md) from
findings, and T-038 closed in the same session.

⭐ **Six IPv6-only trackers are alive.** That category was `unmeasurable` in
every record this project had ever taken.
`experiments/33-ipv6-only-liveness.py` reaches them two ways: directly from a
vantage that has IPv6, and through the operator-approved read proxy, which was
measured to have IPv6 egress of its own (`C-73`). ⛔ The entry's own route (a),
NAT64, was **backwards** -- it makes an IPv4 server reachable from an IPv6
client, which is the opposite problem.

⛔ **Probing IPv6 for the first time found two defects that would each have
published a live tracker as gone.** A host resolving into `0200::/7` was
reclassified as yggdrasil and **probed anyway** -- [T-023](measurement.md)'s
bug one layer below [T-023](measurement.md)'s fix, and three observations is
`dead`. And `ipv6.tracker.harry.lu` answers **`::1`**, so the probe connected
to this machine and recorded the reset as the tracker's (`C-74`).

⛔ **One keystroke was a corrupted dataset.** Folding a sweep into the history
twice recorded two observations from one measurement; three folds reached
`MIN_SAMPLES_FOR_DEATH`, which is every non-live tracker in that sweep
published `dead` on a single probe. Re-running `scripts/update-state.py` over a
directory it had already read did it. Two guards now, per observation and per
sweep, and the second one's first test could not tell them apart.

⛔ **The number D7's whole rule rests on was measured and thrown away.** A
tracker's stated `interval` and `min interval` have been read by
`classify_body` since `C-65` and dropped by `ProbeResult.as_record`, so nothing
could have honoured a request the probe had already been told.
[T-026](measurement.md) carries them onto the record and computes what a run
costs: a full-corpus sweep is **6072 DNS lookups at worst against a ceiling of
100,000**, over the 759 name-addressed hosts of 965.

⛔ **`used_synthetic_infohash` was `true` on records where nothing was sent**,
including in a committed sweep. Found by reading one record, not by the suite.

⭐ **The identity question has an instrument, and its design had to change to
be runnable at all.** Four arms against one tracker in one run is four times
RULES 4's ceiling, so each tracker gets one arm per run and the pairing is
recovered across four rotations. Two runs: 200 corpus trackers, where 174
answered nobody; then **32 trackers a sweep recorded live**, at 16% of the
load, where 30 answered and the descriptive User-Agent answered **15 of 15**.
⛔ Still no verdict, and the instrument refuses to give one below 20 subjects
per arm. ⛔ The crossed `peer_id` axis **cannot be run**: a scrape has no such
field and this project never sends one.

⭐ **The seven shapes have definitions, and the release channels have
semantics.** `src/trackers/shapes.py` says which of the seven a history is,
with the numbers that decided it; `src/trackers/channels.py` is built on what
`experiments/24` measured, and its tests read that result and fail if a design
choice rests on something the run refuted.

**Seven reviews ran and every one found something**, under
[`../HISTORY/reviews/`](../HISTORY/reviews/):

1. **Door sweep** -- the module written this session to be the one home for
   second-hand evidence had a second implementation in an experiment that
   predates it.
2. **Guard mutation** -- 26 mutations, two survivors: a test whose name claimed
   more than it checked, and a guard in the guard module that could not fail
   because a dict copy had already done its work.
3. **Claim audit** -- the DNS budget published in this session had the **wrong
   denominator**: 206 of 965 hosts are address literals and cost no lookup.
   7720 became 6072. Two record edits reported as made had never been written.
4. **Tracker operator** -- 348 tracker-facing contacts, itemised. **68 of them
   bought nobody anything**, because a one-line helper in
   `experiments/_conditions.py` fired the probing workflow twice.
5. **Adversarial sweep** -- attack the newly scheduled sweep by running it.
   ⛔ Adding **one** tracker to the corpus made the next run re-probe the
   **identical** slice, so the rotation stopped rotating the day an upstream
   regenerated. Two more landed: two slices at one instant collided in the
   idempotence guard and 190 observations would have been dropped, and a clock
   the workflow could not parse pinned every run to slice 0 silently.
7. **The acquisition path** -- every hop from an upstream byte to a
   filesystem path or a parser. ⭐ Two of the four threats are **unreachable
   rather than mitigated**: no shell call exists in `src/`, and nothing
   decompresses, so a bomb has no expansion step. Closed [T-086](operations.md).
6. **Measured but never verified** -- ⛔ `C-73` was marked `VERIFIED` on a
   transcript, and the verification it named could not have separated a proxy
   with no IPv6 from a tracker that did not answer. It has a committed control
   now. The i2p gateway finding is labelled an observation rather than a
   measurement, with its command written down.

⭐ **The sweep is scheduled**, at D7's three hours, after the operator settled
the three questions this session raised, and **the first scheduled run has
fired**: `34276432980`, `trigger: schedule`, 173 probed of 189 selected, slice
4 one on from the dispatch's slice 3. That run is the only thing that could
confirm the two workflow fixes below, because a dispatch always supplies its
own inputs. ⭐ It also delivered [T-009](claims.md)'s **first observation**: the
`18:00Z` slot fired **163 minutes late**. ⛔ Scheduling it exposed two defects
only a schedule could have: a fixed sample would have probed the same 190
trackers eight times a day and the other 1137 never, and a `schedule:` event
carries **no inputs**, so `--deadline ""` would have made every scheduled run
exit 2 having probed nothing. The selector rotates through seven slices now,
and every input has a fallback that a test enforces.

⚠ **`skip_tracker_probes` is the structural half of that last one.**
Re-measuring the runner's resolver used to drag 17 endpoints per image along
with it; the census that produced this session's canonical figures contacted
**no tracker at all**.

## In progress

**Nothing half-finished.** Every entry touched is either closed with its
acceptance recorded, or open with what remains written into it.

## Start here next session

1. **[T-012](claims.md)** - whether our identity gets us blocked. The
   instrument exists and two runs have gone out. ⭐ **Run rotations 1 to 3
   against the live subject set**, at least three hours apart, and the pairing
   is complete. ⚠ Each arm needs 20 contacted subjects before the instrument
   will compare them and the live set is 32, so what the verdict actually waits
   on is a wider one: a fresh sweep's live trackers.
2. **[T-080](operations.md)** - issue automation. ⭐ Every unattended piece
   now exists and nothing tells anybody when one fails: the report names
   sustained failures and nobody reads a report. [T-047](scoring.md) and
   [T-002](claims.md)'s watchdog both want this.
3. **[T-081](operations.md)** - history housekeeping. ⭐ `state.jsonl` is on
   the `data` branch and grows with every sweep; the entry says its threshold
   is unjustified, and there is a projection to justify it against now
   ([T-042](scoring.md)).
4. **[T-039](measurement.md)** - i2p and yggdrasil, the two categories
   [T-031](measurement.md) did not move. ⭐ **The consent question is
   answered**: contact is permitted, the asking route covers those operators.
   ⚠ Route (d) is measured and the public gateway is out of service, so what
   is left is a router in a container.

**Deliberately deferred:** [T-044](scoring.md), the scoring model. **D4** stays
open on purpose: history exists now, but no tracker has more than four
observations, and choosing a model against that is fitting it to noise.

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
   D7, settled in [T-026](measurement.md).
4. **What is authorised outward-facing?** Every action belonging to this
   repository, and nothing outside it, ever. RULES 13.
5. **One squashed commit per session, or a series?** A **clean series of
   logical commits**, each passing the gate. **D16**, 2026-09-08. The squash
   could not coexist with confirming CI at every push and with the
   no-force-push rule.
6. **Is DNS inside the politeness budget?** Yes, with a ceiling of **100,000
   lookups per run on a GitHub runner**; local runs are not bounded by it.
   2026-09-08. [T-026](measurement.md) carries it and now computes it: a
   full-corpus sweep is **6072 at worst**, 6.1% of the ceiling.
7. **Schedule the health sweep?** Not until [T-037](measurement.md) lands.
   2026-09-08. A scheduled sweep would have recorded `openbittorrent.com` as
   `dns_failure` on a vantage that cannot resolve it while public resolvers
   can. ⛔ **Superseded the same day by answer 10 below**, once both
   conditions it named were met: the sweep records `resolver_divergence`
   there, which can never become `dead`, and [T-026](measurement.md) computes
   what a run spends. The answer is kept because it is why the schedule waited,
   and because the reasoning it gives -- that a `schedule:` is a standing
   commitment against other people's servers -- is still the reason it was put
   to the operator rather than decided by a session.
8. **The third party's credential in git history?** **Not our action.** The
   operator will **reset this repository's history to a single commit** once
   the tasks are complete and the prose has been rewritten, which removes it.
   2026-09-08. Until then the working tree stays clean of it outside the
   capture fixtures, where a verbatim capture is expected and where rewriting
   one would destroy the evidence that the refusal works.
9. **May this project contact a tracker whose operator it has no automatable
   way to ask?** **Yes -- the asking route is enough.** 2026-09-08. A `.i2p`
   name has no ordinary DNS record, so BEP 34 cannot be consulted for the 13
   `.i2p` URLs; the documented request route in `src/trackers/exclusion.py`
   covers those operators as it covers everyone else, and RULES 4 requires *a*
   route rather than that one. ⛔ It does not weaken the consent gate for a
   host whose name does resolve. [T-039](measurement.md) carries it.
10. **Schedule the health sweep?** **Yes, at D7's cadence.** 2026-09-08, after
   both conditions the earlier answer named were met.
   `.github/workflows/health-sweep.yml` runs `0 */3 * * *`. ⭐ The selector
   **rotates**, because a fixed sample scheduled every three hours would probe
   the same 190 trackers eight times a day and the other 1137 never; a pass
   over the corpus takes seven runs, so each tracker is probed once per 21
   hours. `tests/test_rotation.py` asserts the coverage.
11. **Should a commit credit the tool that helped write it?** **No.** 2026-09-08.
   The harness asks for a co-author trailer;
   [`../docs/conventions/git.md`](../docs/conventions/git.md) forbids crediting
   any tool and says that overrides a harness default. The operator confirmed
   the project rule wins, so the page stands unchanged and no commit carries
   attribution.

## Open questions for the operator

**None.** Eleven have been asked and eleven are answered above.

⚠ **Two standing facts to know rather than re-derive**, neither of which is a
question:

- **BEP 34 lookups send tracker hostnames to a public resolver**, a trade
  recorded in `src/trackers/bep34.py`. The evidence for it strengthened on
  2026-09-08: the host's own resolver was measured failing on names public
  resolvers answer, which is the silent failure the trade avoids.
- **A history reset to one commit is coming** (answer 8). Nothing should be
  built that depends on this repository's commit history, which RULES 3.7
  already forbids for measurement history.
