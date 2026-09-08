# Scoring

State, history, the scoring model, ranking and categories. P3.

**The invariants are written first and the model second**, deliberately. The
model is a guess; the invariants are the defensible part, and they survive a
change of model.

---

### T-040 There is no state or history, so nothing can be scored

Source:      the brief's section 14 (state, history and the housekeeping
             contradiction); decision **D3**
Category:    scoring
Priority:    P1
Effort:      L
Status:      done

Problem:     Scoring needs history and none is stored. Nothing distinguishes a
             new tracker from one that has failed twice from one that has been
             degrading for a month.
Premise:     The storage contradiction is already resolved and is a rule, not a
             decision: **history lives in files** (RULES 3.7), never inferred
             from git history, because the data branch is reset by design.
Approach:    Per tracker, retain an EWMA success rate; a fixed-size ring of the
             last *K* check outcomes with timestamps; daily aggregates for *D*
             days; lifetime counters; first and last seen; last success; last
             failure. The shape must support the distinctions in T-041.
Decision:    **D3, open.** `K` and `D` are chosen from the arithmetic in T-042,
             not from taste. Format and location still to decide; the constraint
             is that it must be recoverable from published artefacts, so a lost
             working copy is not a lost dataset.
Prove:       A test that a tracker's history survives a full pipeline run and
             that the state file is readable by a fresh process.

**Done.** `python3 -m unittest tests.test_state -v` -> **25 tests**, no
network, no clock. `scripts/check-gate.py` -> 16 passed, 1 expected skip.
**D3 is closed** with its rejected alternatives in
[`../HISTORY/decisions.md`](../HISTORY/decisions.md).

`src/trackers/state.py` stores, per tracker: an EWMA rate, a ring of the last
**K = 64** outcomes with timestamps, **D = 180** daily aggregates, lifetime
counters that never roll, and `first_seen` / `last_seen` / `last_success` /
`last_failure`. `scripts/update-state.py` folds a sweep in, so this is a
pipeline stage rather than a library nobody calls.

**K and D came from arithmetic, as this entry required.**
`experiments/31-state-size-projection.py` builds a full record and takes its
length rather than estimating JSON overhead, then projects five years: the
chosen pair is **23.4 MB**, K=128/D=365 is 46.2 MB. Result at
`experiments/results/31-state-size-projection.unclassified-host.20260908T094845Z.json`.

**Both halves of the `Prove` clause.** *Survives a pipeline run*: driven
against the real committed sweeps -- 299 observations, **282 distinct
trackers**, and 200 + 99 - 17 = 282 where 17 is the overlap the value gate
measured independently. Re-run, it accumulates rather than resets. *Readable by
a fresh process*: a second interpreter reads the bytes off disk, because an
in-memory round trip proves the objects and only a second process proves the
file is the contract.

**The refusals are the load-bearing half and each was seen to fire.**

| planted | tests that failed |
| --- | --- |
| recover from a corrupt file by starting fresh | 2 |
| a new tracker starts at rate 0.0 | 1 |
| count every observation as a success | 4 |
| cap daily aggregates without sorting first | 1 |

⛔ The first is RULES 3.9's exact shape, and the tempting implementation is
`except CorruptState: return {}, []`. A bad header raises and
`update-state.py` exits 1 with the file untouched; one bad line is quarantined
and the other records survive.

⭐ **A new tracker's rate is `None`, not 0.0**, or "never checked" and "failed
every check" become the same number -- shapes 1 and 4 of [T-041](scoring.md).

**Not settled.** The scoring model is **D4** and stays open: no tracker has
more than four observations. [T-041](scoring.md)'s shapes have a store that can
express them and nothing computes them. Nothing writes this file in CI yet --
where it lives is [T-063](publication.md)'s decision.

---

### T-041 History must distinguish seven shapes over time, not seven values

Source:      the brief's section 14.2 (the shapes history must distinguish)
Category:    scoring
Priority:    P2
Effort:      M
Status:      done

Problem:     The states that matter are: new tracker, temporarily unavailable,
             intermittently failing, consistently unreliable, degrading,
             improving, apparently gone. **These are different shapes over
             time, not different instantaneous values**, which is exactly why a
             single last-result field cannot express any of them.
Premise:     Follows from T-040's shape. Recorded separately because it is the
             requirement the storage design has to satisfy, and it is the one
             most easily lost when the storage gets simplified.
Approach:    Each shape gets a definition in terms of the stored series, and a
             test with a synthetic series that exhibits it.
Decision:    **`src/trackers/shapes.py`, an ordered table whose order is the
             specification**, in the shape of `probe.health_state` because the
             failure mode is the same: scattered conditionals over a series
             produce a classification nobody can predict or audit. Every
             threshold is a count of observations with a duration attached at
             D7's cadence, and the verdict carries the numbers that decided it
             (RULES 3.10).

             ⛔ **No clock.** Rejected: taking `now` and calling a tracker gone
             after a period of silence. It would make one state file classify
             differently on two days, and a tracker nobody has checked since
             March is a fact about our sweeping rather than about the tracker.
             `last_seen` is on the record for a consumer who needs it.

             Rejected: storing the shape in the state file, which would be a
             second copy of something derived and free to disagree with the
             series it came from. It is computed on read and printed by
             `scripts/update-state.py`.

             Rejected: making the shapes an eighth, ninth and tenth
             `HealthState`. They answer a different question from different
             inputs, and folding them in would let a pattern over a month
             decide a value that means "what happened when we last looked".

             Rejected: tuning the thresholds against this corpus. No tracker
             has more than four observations, so there is nothing to tune
             against and the numbers are stated as arithmetic instead.
Prove:       Seven tests, one per shape, each over a synthetic history.

**Done.** `python3 -m unittest tests.test_shapes -v` -> **21 tests, OK**.
Seven of them are the seven shapes, each over a synthetic series written check
by check through `state.observe` rather than assembled as a fixture.

⭐ **The pairs are what the definitions had to survive.** Four failures
scattered and four in a row are the same rate and are not the same fault; a
new tracker and one that has failed everything both have no successes; a dip
and a decline both end in failure. Each pair is a test asserting the two do
not collapse, and `THE_SEVEN` is asserted to still have seven members so the
vocabulary cannot quietly lose one.

⭐ **The trend comes from the daily aggregates once there are enough days**,
because the ring is 8 days and "degrading over a month" cannot be seen inside
it. That claim is falsifiable rather than stated:
`test_the_ring_alone_would_have_missed_it` asserts the ring's own two halves
differ by less than the threshold for the same history the aggregates classify
as degrading.

⛔ **Five mutations were planted and one survived.** Widening
`MAX_TEMPORARY_RUN` to 99 failed nothing, because every series in the file was
caught by another rule first. The constant was not redundant -- a six-check
outage on a spotless record would have been published as "temporarily
unavailable" for as long as it lasted -- so the missing test was written and
the mutation now fails it.

⚠ **An eighth value exists and is not one of the seven.** `STEADY` is the
residual for a tracker that works and keeps working. Naming it is what stops
seven from being stretched to cover everything.

⚠ **This is not `D4`.** A shape is a label with a definition a reader can
check by hand; the scoring model stays open, and nothing here produces a
number to rank on.

---

### T-042 The state size over five years has never been computed

Source:      the brief's section 14.3 -- "compute the size", not estimate it
Category:    scoring
Priority:    P2
Effort:      S
Status:      done

Problem:     `trackers x bytes-per-record` at the intended `K` and `D`,
             projected over five years, is unknown and unpublished. **A state
             file that grows unboundedly is the same outage as a git history
             that does.**
Premise:     One input is measured: the accepted-tracker count today, from the
             pipeline union (`HISTORY/corpus-baseline.md`, which owns both
             figures). The rest depends on T-040's shape.
Approach:    Compute it, publish the number, and **choose `K` and `D` from that
             arithmetic rather than from taste**.
Prove:       The computed projection is in `docs/` with its assumptions, and a
             test asserts the on-disk state stays under the stated bound for a
             synthetic corpus at the projected size.

**Done.** `python3 experiments/31-state-size-projection.py --expect-under-mb 25`
-> exit 0. Result at `experiments/results/31-state-size-projection.unclassified-host.20260908T094845Z.json`; the projection and the
reasoning are under **D3** in
[`../HISTORY/decisions.md`](../HISTORY/decisions.md), which is where a reader
asking why K is 64 will go.

**Computed, not estimated**, as the source demanded: a full record is built
with `src/trackers/state.py` and `len()` taken of the serialised line.

| K | D | record bytes | 5-year MB | ring covers |
| --- | --- | --- | --- | --- |
| 32 | 90 | 3883 | 12.2 | 4.0 days |
| **64** | **180** | **7449** | **23.4** | **8.0 days** |
| 64 | 365 | 10964 | 34.5 | 8.0 days |
| 128 | 365 | 14676 | 46.2 | 16.0 days |

**The population is every tracker ever seen**, because RULES 11 forbids
deleting one that fails, compounded by an assumed 20%/year. ⚠ That growth rate
is assumed and labelled: this corpus has one census and there is no rate in the
tree to read. `--growth` re-runs it against a different belief.

`tests.test_state.TheBoundsHold` asserts the other half: the ring never exceeds
K, keeps the **newest** outcomes, the daily aggregates never exceed D, and the
lifetime counters do not roll.

⚠ **The clause said `docs/` and the projection is in `HISTORY/`** (RULES 9). It
belongs beside the decision it decides; `docs/` is how the project is worked
on, and a reader asking "why 64" is asking about D3.

---

### T-043 The six scoring invariants are not enforced by anything

Source:      the brief's section 15.2 (the six scoring invariants)
Category:    scoring
Priority:    P1
Effort:      M
Status:      done

Problem:     The invariants are the part that actually matters and they exist
             only as prose.
Premise:     They are cheap to get right and they survive a change of model,
             which is why they are written before the model.
Approach:    Property tests, one per invariant:
             **I1** more successes at the same success rate never lowers the score.
             **I2** a tracker with one success must not outrank a tracker with
             hundreds at an equal-or-better rate.
             **I3** identical inputs produce an identical ordering, including ties.
             **I4** adding a failure never raises the score.
             **I5** an `unmeasurable` tracker is never scored as if measured.
             **I6** score is invariant to input ordering and to source ordering.
Decision:    I5 and I6 are already partly held elsewhere -- `Tracker.sort_key` is
             total and `aggregate()` sorts by source id -- so the property tests
             must cover the scoring path specifically, not re-test those.
Prove:       `python3 -m unittest tests.test_scoring_invariants -v`, six tests
             minimum, each generating adversarial inputs rather than one example.

**Done.** `python3 -m unittest tests.test_scoring_invariants -v` -> **12 tests,
OK**. `src/trackers/scoring.py` holds the six as executable properties over any
candidate of the shape `scorer(checks, successes, measurable) -> float | None`,
and the tests run three candidates through them.

⛔ **There is still no scoring model, and this entry does not choose one.** It
holds the part that survives a change of model, which is the entry's own
premise for writing the invariants first.

⭐ **The obvious candidate is refuted, which is the finding.** The plain
success rate -- what `state.py`'s EWMA converges to and what anybody reaches
for first -- **fails I2**:

| observations | plain rate | Wilson lower bound |
| --- | --- | --- |
| 1 of 1 | **1.0000** | 0.2065 |
| 10 of 10 | **1.0000** | 0.7225 |
| 100 of 100 | **1.0000** | 0.9630 |
| 500 of 500 | **1.0000** | 0.9924 |

A tracker seen once and answering once ranks level with one seen five hundred
times. Publishing that would be ranking a single lucky observation as
equivalent to months of evidence, which is what I2 exists to forbid.

**A Wilson lower bound passes all six**, and that is recorded as an observation
rather than as a choice -- [T-044](scoring.md) owns the decision and now
inherits a shortlist with a reason attached. ⚠ The candidates live in the test
file rather than in `src/`, because a scorer in the library is a model this
project ships.

⛔ **The harness is mutation-proofed by a deliberately broken candidate** that
scores unmeasurable trackers, so a check that never fires cannot be mistaken
for a check that passes.

---

### T-044 No scoring model has been chosen

Source:      the brief's section 15.3 (the recommended model); decision **D4**
Category:    scoring
Priority:    P2
Effort:      M
Status:      open

Problem:     Nothing computes a score.
Premise:     **The recommended model is explicitly a guess** and is expected to
             be challenged: a Wilson lower bound on a time-decayed success
             ratio, combined with a latency factor, with deterministic
             tie-breaking.
Approach:    Start with the simplest thing satisfying T-043's invariants.
             Evaluate against Bayesian smoothing, confidence-weighted scores,
             exponential decay, rolling windows, survival models, and something
             simpler. Record why each alternative lost.
Decision:    **D4, open, and deliberately deferred until there is history to fit
             against.** Choosing now would be fitting a model to zero samples.
             **MUST NOT over-engineer the statistic.**
             The binding constraint is RULES 11: **a sophisticated statistic
             over a single-vantage measurement is precision applied to the wrong
             quantity**, and more misleading than a crude score that is honest
             about its inputs. Latency must not be a primary ranking term -- it
             measures the path from one datacenter.
Prove:       The model is documented, versioned, and passes every T-043 test;
             `HISTORY/decisions.md` D4 records the rejected alternatives.

⭐ **The shortlist is narrower than it was, measured on 2026-09-09 by
[T-043](scoring.md).** The plain success rate **fails invariant I2**: 1 of 1
and 500 of 500 both score 1.0, so one lucky observation ranks level with months
of evidence. A **Wilson lower bound** passes all six and has no free parameter
beyond the confidence level. That is not a decision -- this entry still owns it,
and D4 stays open until there is enough history to fit anything against -- but
whatever is chosen now has to clear `tests/test_scoring_invariants.py` first.


---

### T-045 Ranking must not use the latest instantaneous result

Source:      the brief's section 15.1 (ranking requirements)
Category:    scoring
Priority:    P2
Effort:      S
Status:      open

Problem:     **MUST NOT rank on the latest instantaneous result.** Ranking on
             the most recent check is the failure mode that makes a reliability
             dataset worthless, and nothing currently prevents it because
             nothing ranks.
Premise:     Follows from T-040 existing.
Approach:    Ranking is deterministic, reproducible, documented, testable and
             **versioned**, and the scoring version appears in generated
             metadata so a consumer can tell which methodology produced a
             dataset.
Prove:       A test that two runs over identical history produce an identical
             ordering including ties, and that the scoring version is present in
             the output metadata.

---

### T-046 The five required categories do not exist

Source:      the brief's section 16 (the five required categories)
Category:    scoring
Priority:    P1
Effort:      M
Status:      open

Problem:     Only one output file exists. Five are required and each has its own
             rule.
Premise:     Two of the five have constraints already satisfiable:
             `hardcoded.txt`'s ordering and self-deduplication are implemented
             and tested (`render_plaintext(preserve_order=True)`).
Approach:    **`stable.txt`** -- qualifying on **measured** evidence: long
             observed history, high availability, low failure rate, consistent
             latency, protocol reliability, sufficient sample size. **MUST NOT
             be defined by reputation.**
             **`foss.txt`** -- Linux-distribution and FOSS ecosystems.
             **Investigate Fosstorrents and related ecosystems** as candidate
             sources; none of them is in the registry today. Membership defined
             **by evidence** (provenance-based, source-based, or another
             defensible rule) and the rule stated. **A hand-curated list
             presented as derived is a lie about methodology.**
             **`hardcoded.txt`** -- the maintainer's manual list. Deduplicates
             against itself, preserves manual order, is not sorted, is not
             ranked, is not silently rewritten, and is health-checked like
             everything else.
             **`common.txt`** -- merged, deduplicated, checked, ranked from
             stable, foss, hardcoded and other justified sources.
             **`anime.txt`** -- may be larger than common and may contain unique
             entries; informed by the 995 unique URLs from `desirefire_all`.
Decision:    **The bootstrap problem is real and must be visible.** On day one
             there is no history, so `stable.txt` is either empty or seeded.
             **An empty `stable.txt` on day one is honest; a reputation-seeded
             one pretending to be measured is not.** Decide and make the choice
             visible.
             **`foss.txt` is SETTLED -- operator ruling 2026-08-29: derived plus
             a labelled seed.** Two halves, and they must stay distinguishable
             in the output because that is the whole point:
             (a) **derived** -- add FOSS-ecosystem sources to the registry
             (Fosstorrents and related; see T-105) and take membership from
             FOSS provenance, auditable like any other source;
             (b) **seed** -- a small hardcoded list, in its own file, **labelled
             as curated rather than measured**, so no consumer can read it as
             derived.
             **The operator will supply seed entries to a future session as
             "Additional References".** Until they arrive, build (a) and leave
             the seed file present but empty rather than inventing entries -- an
             empty labelled seed is honest; a guessed one is the methodology lie
             this decision exists to avoid.
             Rejected: pure source-derived (discards curation the operator wants
             to provide); pure curated (makes no derivable claim and wastes the
             provenance already available); dropping the category (satisfiable,
             so dropping would be a silent downgrade).
Prove:       Per-category tests: `stable.txt` is empty or every member has a
             sample count above the stated threshold; `hardcoded.txt` preserves
             order and self-deduplicates; category invariants hold.

---

### T-047 A hardcoded tracker unreachable for 48 hours must raise an issue, not vanish

Source:      T-046 `hardcoded.txt`
Category:    scoring
Priority:    P2
Effort:      S
Status:      open

Problem:     No mechanism exists.
Premise:     Depends on T-040's history and on the issue automation in T-080.
Approach:    Create or update a GitHub Issue with evidence and timestamps when
             an entry is unreachable for more than 48 hours.
Decision:    **MUST NOT auto-delete an entry for being unreachable.** That is
             the maintainer's decision, and RULES 3.4 is exactly why: an entry
             may be unreachable from AS8075 and fine everywhere else. The issue
             owes the vantage metadata so a maintainer can tell "dead" from
             "dead from CI".
Prove:       A test that 48 hours of failure produces one issue and not a
             deletion, and that a second run updates rather than duplicates it.
