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
Status:      open

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

---

### T-041 History must distinguish seven shapes over time, not seven values

Source:      the brief's section 14.2 (the shapes history must distinguish)
Category:    scoring
Priority:    P2
Effort:      M
Status:      open

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
Prove:       Seven tests, one per shape, each over a synthetic history.

---

### T-042 The state size over five years has never been computed

Source:      the brief's section 14.3 -- "compute the size", not estimate it
Category:    scoring
Priority:    P2
Effort:      S
Status:      open

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

---

### T-043 The six scoring invariants are not enforced by anything

Source:      the brief's section 15.2 (the six scoring invariants)
Category:    scoring
Priority:    P1
Effort:      M
Status:      open

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
