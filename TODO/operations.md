# Operations

Issue automation, housekeeping, self-healing, the CI schedule, and the
five-year review. P5.

---

### T-080 Issue automation does not exist

Source:      the brief's section 20 (issue automation)
Category:    operations
Priority:    P2
Effort:      L
Status:      done

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

**Done.** `python3 -m unittest tests.test_issues -v` -> **23 tests, OK**. All
three `Prove` properties are unit-tested rather than integration-hoped, because
the decision is a **pure function**: `src/trackers/issues.py` takes the
conditions a run observed plus the issues already open and returns what to
open, update and close. `scripts/raise-issues.py` is the thin layer that acts,
and it is ⛔ **dry run by default** -- `--apply` is required to write anything.

**Driven against the real repository and the published state**: 430 histories,
newest observation `2026-09-08T21:33:23Z`, **0 conditions, nothing written**.
⭐ That is the property that matters most: an automation which cries wolf gets
muted, so being silent when nothing is wrong is the behaviour to verify first.

⛔ **Deduplication is on a marker in the body, never the title.** A person may
edit a title, and an automation matching on it files a second issue because
somebody clarified the first one. A body with **no** marker is somebody's own
issue and is never touched -- closing a person's issue because the title looked
familiar would do more damage than the condition being reported.

⚠ **Lowest issue number wins among duplicates**, which is the oldest and
therefore the one people are subscribed to. The first version took whichever
the API returned first while its comment claimed otherwise, and the test is
what found it.

**Three conditions are wired**, and the rest wait on the thing they would
report on rather than on effort:

| condition | state |
| --- | --- |
| source failed / rejected / empty | **wired**, with the evidence T-080 lists and RULES 3.2 in the issue text so a maintainer does not think data was lost |
| watched tracker failing every check | **wired** ([T-047](../TODO/scoring.md)), carrying the **vantage**, so `dead` and `dead from one datacenter` are distinguishable. ⛔ Only the maintainer's hardcoded entries: filing for every tracker that stops answering would be a thousand issues |
| dataset stale | **wired** ([T-002](../TODO/claims.md)), naming the 60-day rule so the first thing checked is whether the schedule is still enabled |
| release failure | waits on releases being used at all ([T-064](../TODO/publication.md) built the semantics; nothing uses them) |
| housekeeping failure | waits on [T-081](operations.md) |
| persistent CI failure | waits on a run-history read this project does not do |
| dependency or security issue | there is no dependency to fail (D1: standard library only) |

⛔ **A condition for a feature that does not exist would be dead config**, which
is the forbidden pattern about a setting no code reads. Each row above names
what it waits on rather than being quietly dropped (RULES 9.1).

**The body is capped at 8000 bytes and says when it truncated**, because a
maintainer reading half the evidence with no sign there was more is worse than
a shorter summary. ⚠ `C-44`: an artefact expires after 90 days, so evidence is
summarised **into** the body and a run reference is a supplement.

---

### T-081 History housekeeping is unimplemented and its threshold is unjustified

Source:      the brief's section 14.5 (history housekeeping)
Category:    operations
Priority:    P2
Effort:      M
Status:      done

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

**Done.** `python3 -m unittest tests.test_housekeeping -v` -> **8 tests, OK**,
driven against a real synthetic repository because the subject is git's
behaviour under `checkout --orphan` and a stub would prove nothing about it.

⭐ **The threshold is derived rather than inherited**, which is what the entry
asked for. `experiments/34-data-branch-growth.py` clones the branch shallow and
full and takes the difference: **14.3 KiB per commit** on 2026-09-09. For a
full clone to stay under **250 MiB** that is **17,853 commits**, about **6.1
years** at eight publishes a day. The prior art's 5000 would be **1.7 years**
here.

⛔ **And the count is a proxy whose validity expires**, which is the finding
worth more than the number. Every commit rewrites `state.jsonl`, and
[T-042](../TODO/scoring.md) projects that file at **23.4 MB** in five years
against ~145 KB today -- so a late commit costs far more than an early one and
any projection from today's rate **understates the future**. The script checks
the **measured size** as well, and the size decides.

**The `Prove` clause, and the refusals around it:** the reset keeps every
published file byte for byte including `state.jsonl`, `main` is refused
outright by checking the branch the checkout is actually on rather than the one
it was told about, a branch already missing a file is refused because a reset
would make that permanent, and it verifies **after** the rewrite before
anything could be pushed.

**The workflow the Approach asked for is a script instead (RULES 9).**
*Requirement*: a workflow that resets the branch, holding a lock and refusing
during publication. *Evidence it should change*: `docs/conventions/git.md`
forbids force-pushing published history, the threshold is **six years** away,
and an automated force-pusher that sits unused and untested for six years is
the worst kind of automation to own. *Replacement*: the script does every
`MUST` the entry lists -- never touches `main`, preserves the data, refuses
while a publication is in flight, verifies before and after, fails safely,
leaves its assessment as evidence -- and **prints the push rather than running
it**, so the last step is the operator's, which is what git.md requires of a
history rewrite. That page now names this as its **third** exception, with the
RULES 3.7 argument for why it is safe.

**The consumer consequence is documented where a consumer reads it**
([`../docs/schema.md`](../docs/schema.md)): a commit SHA on the `data` branch is
not a durable reference and breaks by design, so the pin target is the branch.

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
Status:      done

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

**The `Prove` clause was met before the rest was**, because the entry asks for
two things and only one of them is a test.
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

**Done.** The schedule exists and the architecture is decided, 2026-09-09.

**Five workflows, and each separation buys something nameable** -- the entry's
rule is separation for safety or maintainability, never aesthetics:

| workflow | trigger | why it is its own workflow |
| --- | --- | --- |
| `gate.yml` | every push | offline by construction. A validation workflow that reached the network would be red whenever somebody else's host is down |
| `p0-ground-truth.yml` | experiments change, dispatch | contacts trackers for measurement rather than for health. Running it on every push would spend other people's requests on a documentation edit |
| `health-sweep.yml` | **`0 */3 * * *`** | the only scheduled tracker contact. D7's interval exactly, one rotating slice per run |
| `publish.yml` | after a sweep | the only holder of `contents: write`. ⛔ Separate from the sweep because a measurement failure must not stop publication of what is already known |
| `issues.yml` | after a publish | holds `issues: write` and **not** `contents`. One workflow with both is one job whose bug reaches the data and the tracker together |

⭐ **Two candidates from the entry's list are deliberately absent** and each
names what it waits on rather than being dropped: daily and weekly promotion
waits on [T-064](publication.md)'s channels being used at all, and data-branch
housekeeping is [T-081](operations.md).

**The platform facts are honoured rather than assumed**, and one is now
observed rather than documented: the first scheduled run fired **163 minutes
late** (`C-11`, [T-009](../TODO/claims.md)). The rotation takes its slice from
the clock at run time, so a displaced run walks to the bucket it actually ran
in -- slices are skipped, never repeated, which is the right direction for that
to fail in.

⛔ **A duplicated run cannot corrupt state**, which was one keystroke from a
corrupted dataset before `tests/test_run_safety.py` existed, and the
adversarial pass of 2026-09-08 found a second way in: two rotations at one
injected instant collided in the idempotence guard and 190 observations would
have been dropped. The slice is part of the sweep identity now.

---

### T-085 Overlapping runs are prevented in the gates but not in publication

Source:      T-084; the RULES 3.8 invariant
Category:    operations
Priority:    P1
Effort:      S
Status:      done

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

**Done.** `python3 -m unittest tests.test_concurrency.NoTwoPublishersAtOnce -v`
-> **4 tests, OK**. `.github/workflows/publish.yml` exists now, which is what
this entry was waiting for, and it carries `concurrency: publish-data` with
`cancel-in-progress: false`.

⛔ **The `false` is the entry.** Cancelling a publication mid-write leaves half
a dataset on a branch people fetch, so a second publish queues rather than
killing the first. The prior art cancels in progress on its update workflow;
XIU2 queues, and queuing is the safer of the two.

**Four assertions, read out of the workflow files** rather than stated in
prose: every workflow has a group, anything granting `contents: write` sets
`cancel-in-progress: false`, the publisher is the **only** workflow that
writes, and it grants nothing beyond `contents` and the `actions: read` it uses
to download the sweep's records. Mutation-proved in both directions: flipping
the publisher to cancel fails, and granting a second workflow write fails.

⚠ **A push race is still possible and fails in the right direction.** If the
`data` branch moves between this run's fetch and its push, the push is rejected
and the job fails having published nothing -- the previous dataset stands, and
the next sweep three hours later republishes. A retry loop would trade a loud
failure for a quiet one, and RULES 3.5 prefers the loud one.

---

### T-086 Security review has not been run against the acquisition path

Source:      RULES 5.1; HISTORY/gates.md
Category:    operations
Priority:    P1
Effort:      S
Status:      done

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

**Done.** `python3 -m unittest tests.test_acquisition_security -v` -> **10
tests, OK**, and the review is
[`../HISTORY/reviews/2026-09-09-12-acquisition-path.md`](../HISTORY/reviews/2026-09-09-12-acquisition-path.md).

⭐ **The path is five hops and two of the four threats are unreachable rather
than mitigated.** There is no shell call anywhere in `src/`, so command
injection has nothing to inject into; and `fetch` sends no `Accept-Encoding`
while `urllib` does not decompress on its own, so a decompression bomb has no
expansion step to exploit. The entry asks for decompression handling "if
compression is ever accepted" -- it is not, and the test is what would fail if
somebody accepted it.

**One test per threat**: every registry id is a bare path component, an
oversized body is **rejected rather than truncated**, a control character is
refused rather than stripped and says which class it was, and a gzip body is
not expanded.

⚠ **Two of the tests read the source of the function they test**, which is
deliberate: `read(MAX + 1)` against `read()` then a length check is invisible
in behaviour on any input small enough to run in a suite, and the ordering of
the path computation against the body read is structural rather than
observable.

**Three things the review found that the tests do not cover**, recorded rather
than left implicit: `errors="replace"` silently transforms a non-UTF-8 body,
which is correct because the affected lines then become **recorded rejections**
rather than disappearing; the 8 MiB ceiling is a 200x margin over the largest
observed source, which is the right direction for an exhaustion bound and is
the volume-swing check's job rather than this one's ([T-102](sources.md)); and
`scripts/fetch-reference-comments.py` writes paths derived from remote data but
never runs in the pipeline, so it needs its own pass rather than a sentence.

---

### T-087 The rotation is a wall-clock bucket, so two runs took one slice

Source:      the state of the published `data` branch, 2026-09-09
Category:    operations
Priority:    P0
Effort:      M
Status:      done

Problem:     ⛔ **D7's ceiling was never enforced. It was inferred from
             arithmetic, and the arithmetic does not hold.**
             `probe-corpus.py:rotation_for` maps the injected instant to
             `epoch // 10800`, so the slice a run takes is a property of which
             three-hour wall-clock bucket the run *starts in* -- not of what
             has already been probed. Two runs inside one bucket take the
             **identical** slice, and nothing anywhere refused the second one.
Premise:     **Measured, not reasoned.** Runs `34281244142`
             (`2026-09-08T21:33:23Z`, dispatch) and `34289476724`
             (`23:11:21Z`, schedule) each reported
             `rotation: slice 5 of 7 (rotation 165639)` in their own logs, and
             `state.jsonl` on the `data` branch carries **192 trackers whose
             consecutive observations are 5878 s apart** -- inside D7's
             10800 s interval, published, and paid for by the operators who
             answered twice.

             ⚠ **Reachable schedule-to-schedule, not only by dispatch.**
             [T-009](../TODO/claims.md) has already measured this repository's
             own scheduled sweeps firing **163** and **131** minutes late; a
             slot delayed past the next slot's nominal instant puts two
             scheduled runs in one bucket. The same skew skips a bucket in the
             other direction, and a skipped bucket is a slice never probed in
             that pass -- which is what `MIN_SAMPLES_FOR_DEATH` and
             [T-012](claims.md)'s subject set are both waiting on.

             ⛔ **The test that should have caught it asserted something
             else.** `test_two_runs_in_a_row_share_no_tracker` compared
             rotation `0` with rotation `1` and found them disjoint. That is a
             property of two rotation *integers*; the schedule needs a property
             of two *runs*, and nothing asserted that two runs get different
             integers. The name carried the stronger claim and the assertion
             did not, which is the "a test whose name claims more than it
             checks" row of
             [`../docs/conventions/forbidden-patterns.md`](../docs/conventions/forbidden-patterns.md).
Approach:    Stop deriving the ceiling and start enforcing it. `politeness.py`
             gets the one home for *may this tracker be contacted at this
             instant*, answered from the recorded history's `last_seen` rather
             than from an assumed cadence; `sweep.plan` is the only door into a
             selection and applies it; the workflow fetches `state.jsonl` from
             the `data` branch so the sweep has a history to be polite against.

             ⭐ **And it advances rather than idling.** Holding a slice back
             converts a politeness breach into a coverage gap -- no requests,
             the slot spent, the corpus walked slower than the seven-run pass
             the schedule is sized for. A slice with nothing due yields to the
             next, bounded by one turn of the rotation.
Decision:    **The rotation stays as it is.** It is a *sampler* and it is a
             good one: `slice_of` already made membership a property of the
             tracker rather than of its position. Rewriting it to advance by a
             counter would need state the sweep does not have, and would leave
             the ceiling resting on arithmetic -- one class of clock defect
             traded for another. Two checks enforcing one rule is its own
             forbidden row, so there is exactly one: the per-tracker one, which
             is true whatever the sampler does.

             ⛔ **Rejected: recording a held tracker as an observation.** A
             `skipped_too_soon` health record would satisfy every schema in the
             tree and would be a **non-observation folded into the history as
             an observation** -- three of which say `dead`. Held trackers are a
             count, never a record.

             ⛔ **Rejected: `continue-on-error` on the history fetch.** A run
             that cannot read the history cannot be polite against it, and
             proceeding would spend a ceiling whose record it had lost.
             The only case that probes without a history is the one where there
             is none: no `data` branch, which is a first run.

             ⚠ **The interval is D7's default, and that is precise rather than
             lazy.** D7 is the tracker's own stated interval, defaulting to
             three hours where none has been observed. **None has been
             observed**: `stated_interval` reads two keys that `classify_body`
             has populated since `C-65`, and 0 of 299 committed sweep records
             carry either. `too_soon_after` takes the interval as a parameter,
             so a caller holding a record that states one passes it; inventing
             a history field for a value no tracker has yet sent would be a
             ceiling derived from nothing.
Prove:       `python3 -m unittest tests.test_rotation tests.test_politeness`
             passes, including a test that plants the two measured instants and
             asserts the second run contacts nobody the first one did; and
             `python3 scripts/probe-corpus.py --offline-corpus --dry-run
             --generated-at 2026-09-08T23:11:21Z --state <history as of 21:33>`
             advances off slice 5 instead of re-taking it.

**Done.** 2026-09-09. `python3 -m unittest tests.test_rotation
tests.test_politeness` -> **51 tests, OK**, and the replay below advances off
the slice the second run re-took. The ceiling is enforced from the recorded history in
`src/trackers/politeness.py`, applied in `src/trackers/sweep.py`'s `plan`, and
supplied to the scheduled sweep by `.github/workflows/health-sweep.yml`.

**Both `Prove` clauses were run.** Replaying the collision against the
history reconstructed as it stood between the two runs -- the published
`state.jsonl` with the `23:11:21Z` observations removed, which rolled back
exactly the **192** trackers -- gives:

```
rotation:     slice 6 of 7 (rotation 165640)  ⭐ advanced from 165639: slice 5
              was entirely inside D7's interval
ceiling:      430 trackers with a recorded last contact
selected:     194 (a sample; RULES 15.2)
```

⛔ **The guard was mutation-proved, and one mutation survived the first
attempt.** Five defects were planted and the suite run unpiped for each: the
ceiling removed, the advance removed, an unreadable `last_seen` buying a probe,
an unreadable clock guessed instead of raised, and the boundary comparison
shifted by one second. The last **survived**, because `gap <= interval - 1` is
indistinguishable from `gap < interval` over whole seconds and every instant in
the tree is whole seconds -- so the test that pins the boundary now asserts a
fractional gap as well. 5 of 5 caught.

⛔ **Writing the fix reintroduced the defect it was written for, one layer
down.** `plan` first reported only the slice it landed on, so an advanced run
said `held_by_politeness_ceiling: 0` while 192 trackers had been held -- the
"nobody was held" / "nobody was checked" confusion that `enforced_from_history`
exists to prevent, inside the fix for it. Caught by
`test_it_advances_rather_than_idling_through_the_slot`, which is the one test
that read the count rather than the outcome.

⚠ **What this does not fix.** The two observations already published are real
contacts and they stay: RULES 3.9 forbids recovering by deleting valid data,
and 192 trackers genuinely were asked twice. The record of it is
[`../HISTORY/corrections.md`](../HISTORY/corrections.md) round 4, and the
history's own spacing is now the evidence that it stopped.

---

### T-088 D7 is a mechanism on the sweep path and a convention on the experiment path

Source:      the door sweep of 2026-09-09
             ([`../HISTORY/reviews/`](../HISTORY/reviews/))
Category:    operations
Priority:    P2
Effort:      S
Status:      open

Problem:     [T-087](operations.md) made the politeness ceiling structural for
             the health sweep: `sweep.plan` is the only door into a selection
             and it consults `politeness.too_soon_after`. ⛔ **Nothing enforces
             it on the other door.** An experiment calls `probe()` directly,
             and the ceiling is whatever flags its author remembered to add --
             which is the "a control gated on one of several paths into the
             same action" row of
             [`../docs/conventions/forbidden-patterns.md`](../docs/conventions/forbidden-patterns.md),
             named there as the most recurring hole there is.
Premise:     **Measured, and the hole is narrower than it looks today.** Of the
             instruments that open a socket to a tracker, only
             `experiments/26-user-agent-block-rate.py` draws from the same
             population the sweep probes -- and it carries the ceiling, through
             the same `too_soon_after` since 2026-09-09. The other three are
             disjoint by construction: `probe()` returns `unsupported` for
             `wss` and for `.i2p` **before opening a socket**, verified
             2026-09-09, so `25`, `35` and `36` cannot collide with a sweep
             however often they run.

             ⚠ So no double-contact is reachable today. What is missing is
             the **mechanism**: a new experiment probing HTTP trackers would
             have nothing stopping it, and the reason it is safe now is a fact
             about the current set of experiments rather than about the code.
Approach:    One shared entry point for "may I contact this tracker now",
             taking the recorded history, so an instrument gets the ceiling by
             using the ordinary path rather than by remembering a flag. The
             pieces exist -- `politeness.too_soon_after` is the rule and
             `sources.jsonl`/`state.jsonl` are the record -- so this is wiring
             and a test, not a design.
Decision:    ⛔ **Not solved by documentation.** A note saying "remember to
             pass --state" is exactly the guard-by-convention this entry is
             about. Either the probe path consults the ceiling itself, or a
             check refuses an experiment that reaches `probe()` without one.

             ⚠ Rejected for now: making `probe()` itself refuse. It is
             called by the fake-tracker oracle and by loopback controls, where
             no ceiling applies and where a history does not exist, so a
             refusal there would fail the tests that prove the probe works.
Prove:       A test that an instrument reaching a tracker without consulting
             the recorded history fails a check, and that the existing
             loopback controls still pass.
