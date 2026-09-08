# Operations

Issue automation, housekeeping, self-healing, the CI schedule, and the
five-year review. P5.

---

### T-080 Issue automation does not exist

Source:      the brief's section 20 (issue automation)
Category:    operations
Priority:    P2
Effort:      L
Status:      open

Problem:     Nothing surfaces an exception to a human, so every failure mode
             this project designs against would currently fail silently -- which
             is the failure mode it exists to avoid.
Premise:     The API budget is finite (`C-13`, unverified but documented as
             ~1000 requests per repository per hour) and issue automation
             running hourly **must stay inside it, or it will fail exactly when
             many things are wrong at once**.
Approach:    Create or update issues for: vanished source, repeated source
             failure, source format change, garbage source response,
             suspicious source change, parser failure, hardcoded tracker
             unreachable >48 h (T-047), release failure, data branch problem,
             housekeeping failure, persistent CI failure, severe dependency or
             security issue.
             **Evidence a source issue owes:** source id, URL, timestamp,
             HTTP status, error, expected format, observed format, content
             hash, previous accepted state, new candidate state, rejection
             reason, workflow run reference, change statistics.
             **Evidence a hardcoded-tracker issue owes:** tracker, first and
             last failure, recent check summary, sustained-failure duration,
             protocol, error class, **and the vantage metadata**, so a
             maintainer can tell "dead" from "dead from CI".
Decision:    Issues must deduplicate, carry evidence, be labelled, concise,
             actionable, automatically updated, and **automatically closed when
             the condition genuinely resolves**. **Avoid issue spam** -- an
             automation that cries wolf hourly gets muted, and then it is worse
             than no automation.
             **MUST NOT paste enormous upstream responses into issues**; use
             workflow artefacts for large evidence -- but note `C-44`: artefacts
             expire after 90 days, so **an issue citing one will eventually cite
             nothing**. Summarise into the issue body; link as a supplement.
Prove:       Tests that a repeated condition produces one issue and not many,
             that the issue closes when the condition clears, and that no issue
             body exceeds a stated size.

---

### T-081 History housekeeping is unimplemented and its threshold is unjustified

Source:      the brief's section 14.5 (history housekeeping)
Category:    operations
Priority:    P2
Effort:      M
Status:      open

Problem:     Continuous publication accumulates commits without bound.
Premise:     **Safe by construction because of RULES 3.7** -- history lives in
             files, so a branch reset discards *commits*, not *data*. The prior
             art's `reset_commits.yaml` does exactly this at >5000 commits, and
             it is safe there only because that repository stores no history
             worth losing.
Approach:    Reset the data branch to a single commit when the count crosses a
             **configurable, empirically justified** threshold. The workflow
             MUST: never touch `main`; preserve current generated data and
             release assets; hold a concurrency lock; refuse to run while a
             publication is in progress; verify the branch before and after;
             fail safely; leave evidence; and be recoverable.
Decision:    The original suggested ~5000. **That number needs deriving from
             the observed commit rate rather than inheriting** -- at hourly
             publication it is roughly seven months, which may or may not be the
             right cadence. Derive it from measured growth (experiment 15).
             Consequence that **must** be documented for consumers: commit SHAs
             on the data branch are not durable references, and any consumer or
             third-party CDN pinned to one **breaks by design**. This is why the
             pin target is a branch or a tag.
Prove:       A test over a synthetic branch that the reset preserves the current
             dataset and never touches `main`.

---

### T-082 Self-healing is unimplemented, and its limit matters more than its coverage

Source:      RULES 3.9
Category:    operations
Priority:    P2
Effort:      M
Status:      open

Problem:     Nothing recovers automatically from transient failure.
Premise:     Several failure classes are already distinguished by `Outcome`,
             which is the prerequisite: you cannot retry intelligently without
             knowing what failed.
Approach:    Recover from transient network failure, temporary source outage,
             temporary rate limiting, intermittent tracker failure, retryable
             workflow failures, stale intermediate state, and partial
             publication failure.
Decision:    **MUST NOT "recover" by deleting valid data.** Preserving state and
             retrying is always preferred to reconstructing from nothing. **A
             clean rebuild that discards history is data loss wearing the
             costume of a fix.** This is the limit, and it is the part worth
             enforcing with a test rather than a comment.
Prove:       A test that a recovery path never reduces the stored history, and
             that corrupt state fails safely rather than reinitialising.

---

### T-083 The five-year operational review is unanswered

Source:      the brief's section 30 (the five-year operational review)
Category:    operations
Priority:    P2
Effort:      M
Status:      open

Problem:     Fourteen questions must each have a **mechanism** as its answer,
             pointed at by file and line. Most currently have none.
Premise:     Two are already answered by built mechanisms: total source failure
             refuses to publish (`scripts/generate.py` `verify()`, demonstrated
             in CI), and a source failing does not fail the others
             (`aggregate()`).
Approach:    Answer each with a mechanism and a citation: a major source
             disappears, every source changes format at once, a source serves
             malicious data, tracker counts drop 99% or increase 1000%, GitHub
             delays or drops scheduled workflows (`C-11`) or disables them after
             inactivity (`C-12`, and see T-002), two workflow runs overlap, a
             release operation fails halfway, the data branch becomes enormous
, the ranking algorithm changes and historical scores must stay
             interpretable, a dependency or third-party fallback disappears,
             GitHub changes an API or a runner image, **all external tracker
             checks fail at once**, the maintainer is absent for a year, what
             data can safely be retained and reused.
Decision:    **Design for graceful degradation: the correct behaviour under
             total measurement failure is to publish the *previous* data with a
             stale marker, not an empty or all-dead dataset.**
Prove:       `docs/operations.md` (planned) answers all fourteen with a file-and-line
             citation each, and a checker asserts every citation resolves.

---

### T-084 No schedule exists and the workflow architecture is undecided

Source:      the brief's section 31 (CI schedule and workflow architecture)
Category:    operations
Priority:    P2
Effort:      M
Status:      open

Problem:     Nothing is scheduled. The two workflows that exist are push- and
             dispatch-triggered.
Premise:     **Measured platform facts to build against**: the schedule floor is
             5 minutes (`C-10`); runs can be delayed **and dropped** (`C-11`);
             public-repo schedules disable after 60 days of inactivity (`C-12`);
             workflow-token pushes do **not** trigger further workflows
             (`C-19b`), which protects against loops and breaks any design
             expecting a chained trigger; and scheduled workflows run **only on
             the default branch** (`C-55`).
Approach:    Separate workflows only where separation improves safety or
             maintainability, **not for aesthetics**. Candidates: validation and
             test CI (exists: `gate.yml`); source and data update; publication;
             stale checking; issue maintenance; daily/weekly promotion; data
             branch housekeeping.
Decision:    **MUST NOT assume a scheduled run occurs on the minute, occurs at
             all, or occurs exactly once.** Delayed, dropped and duplicated
             executions **MUST NOT corrupt state**. Cadence is D7 (T-026):
             publish hourly, probe on each tracker's own interval. Every
             workflow: least-privilege permissions, explicit timeouts,
             concurrency controls, SHA-pinned actions, diagnostic artefacts,
             safe retries, deterministic generation.
Prove:       A test that a duplicated run over the same inputs produces the same
             state, and that a skipped interval does not corrupt it.

**The `Prove` clause is met and the entry stays open**, because it asks for two
things and only one of them is a test.
`python3 -m unittest tests.test_run_safety -v` -> **11 tests, OK**. The
workflow architecture and the schedule itself are what remain.

⛔ **The property did not hold, and it was one keystroke from a corrupted
dataset.** Measured 2026-09-08: folding one sweep into the history twice
recorded **two observations from one measurement**, and three folds reached
`MIN_SAMPLES_FOR_DEATH` -- every non-live tracker in that sweep published
`dead` on the strength of a single probe. Re-running
`scripts/update-state.py` over a directory it had already read did it, which
is what a retry is, and `C-11` says a schedule may fire more than once anyway.

**Two layers, because they fail in different places.**

1. **Per observation**, in `state.observe`, keyed on the instant. Every record
   in one sweep carries that run's injected clock, so `(url, observed_at)`
   identifies an observation exactly. Its window is the ring.
2. **Per sweep**, in the state file's header: `applied_runs` remembers what
   produced the file, so `update-state.py` refuses a sweep it has already
   folded even after the evidence rolls off. Bounded at `APPLIED_RUNS_KEPT`,
   because a header that grows forever is the unbounded file `experiments/31`
   exists to prevent.

⚠ **The second layer's test had to be written twice.** The first version drove
the updater three times and passed with layer 2 disabled, because layer 1
caught it -- a test whose name claimed more than it checked. The version that
survives fills the ring past capacity first, which is the only case layer 2
owns.

**A skipped interval is not corruption**, and that half needed no fix: the
daily aggregates are keyed by day, so a dropped run is an absent day, and an
out-of-order run does not move `last_seen` backwards.

**The sweep is scheduled**, operator ruling 2026-09-08: `0 */3 * * *`, which is
D7's interval exactly. What remains open here is the rest of the architecture
-- what else runs, and where the records go ([T-063](publication.md)).

⛔ **Scheduling it exposed a defect that only a schedule could have.** `ci`
probes a sample and `select` took a fixed stride from index 0, so every
scheduled run would have probed **the same 190 trackers** eight times a day
while the other 1137 were never contacted again -- and
`MIN_SAMPLES_FOR_DEATH` needs three observations, so nothing about them could
ever have left `unknown`. The selector rotates now: seven disjoint slices whose
union is the corpus, chosen by a rotation derived from the injected clock.
⭐ **A pass takes 21 hours, so each tracker is probed once per 21 hours** --
below the ceiling rather than at it. `tests/test_rotation.py` asserts the
coverage, that consecutive runs share no tracker, and that no slice exceeds the
sample size.

⛔ **And the first fix for that was worse than it looked**, which the
adversarial pass caught by running it. Slicing by **position** degenerates the
moment the corpus changes size, and the upstreams regenerate daily: at 1327
trackers over 7 slices, adding **one** shifts every index by one, so the next
run's slice is *exactly* the previous run's set -- a 100% overlap, the rotation
silently ceasing to rotate, and the original defect back with a rotation bolted
on top. `slice_of` decides membership from the tracker's own hash now, so a
corpus that gains or loses entries moves nobody else. ⚠ Growing past a multiple
of the sample size does change the slice count and reshuffles; the overlap is
then a minority of a slice and a tracker in it is probed twice three hours
apart, at the ceiling rather than over it.

⛔ **And a second defect that a dispatch could never have shown.** A
`schedule:` event carries **no inputs**, so `--deadline "${{ inputs.deadline }}"`
would have been `--deadline ""` -- an argparse error, and every scheduled run
would have exited 2 having probed nothing while the workflow looked configured.
Every input now has a fallback and a test refuses one that does not.

**Both fixes are confirmed by a real scheduled event**, which is the only thing
that could confirm either: run `34276432980`, `trigger: schedule`, probed 173
of 189 selected, and the preview and the sweep both reported slice 4 -- one on
from the dispatch's slice 3.

⛔ **One hazard is known and not prevented: a re-run inside the same three-hour
bucket takes the same slice**, so ~190 trackers are contacted twice inside
D7's interval. It is reachable from GitHub's own re-run button. Three routes
were considered. **Refusing a duplicate needs state shared across runs**, which
does not exist until [T-063](publication.md) decides where records live.
**Deriving the rotation from the run id** instead of the clock would make a
re-run take a *different* slice, which trades a double contact for an
unpredictable walk and loses the property that a delayed run self-corrects.
**Disabling re-runs** is not something a workflow can express. So it is
recorded rather than fixed, and it is the smallest of the three costs: a
re-run is a human action, not an unattended one.

---

### T-085 Overlapping runs are prevented in the gates but not in publication

Source:      T-084; the RULES 3.8 invariant
Category:    operations
Priority:    P1
Effort:      S
Status:      open

Problem:     `gate.yml` and `p0-ground-truth.yml` both hold `concurrency`
             groups. **No publication workflow exists yet**, and publication is
             the one where a race actually corrupts something.
Premise:     Concurrency groups are proven to work here; the pattern is
             established and not applied where it will matter.
Approach:    Explicit `concurrency` groups so publication operations cannot
             race, with `cancel-in-progress: false` for publication --
             **cancelling a publication mid-write is the failure, not the fix**.
             Note the prior art cancels in progress on its update workflow;
             XIU2 queues instead, which is the safer of the two.
Prove:       A test or a workflow assertion that two publication runs cannot
             overlap.

---

### T-086 Security review has not been run against the acquisition path

Source:      RULES 5.1; HISTORY/gates.md
Category:    operations
Priority:    P1
Effort:      S
Status:      open

Problem:     RULES 5.1 states the threat model. Some of it is enforced by
             construction -- `parse()` rejects control characters and hostnames
             that could be paths, cache filenames derive from **registry source
             ids** and never from upstream content, and there is no shell layer
             at all -- but no review has confirmed the whole path.
Premise:     Partly held and partly unaudited. The strongest existing guarantee
             is structural: D1's no-shell decision makes "never interpolate
             upstream content into a shell command" impossible rather than
             remembered.
Approach:    Review every path from an upstream byte to a filesystem path, a
             subprocess, a parser, or an output file. Confirm bounded response
             size, decompression handling if compression is ever accepted, and
             that no source-supplied string reaches a path.
Prove:       A committed review and a test for each threat: path traversal,
             oversized response, control characters, and a decompression bomb if
             compression is accepted.
