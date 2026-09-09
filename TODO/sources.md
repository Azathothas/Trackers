# Sources

The source registry, acquisition behaviour, provenance, and source quality.

---

### T-100 The source registry is missing fields the design requires

Source:      the brief's section 12 (source registry fields)
Category:    sources
Priority:    P2
Effort:      S
Status:      open

Problem:     `src/trackers/registry.py` carries id, URL, role, trust, category,
             upstream, notes, expected range, observed count, required, enabled.
             The required set also includes: expected format, parser, fetch
             strategy, cache strategy, validation rules, source-specific
             normalization, **last successful fetch, last failure, failure
             count, health state**.
Premise:     The last four are per-run state rather than static configuration,
             so they belong with T-040's state file and not in the table. The
             rest are static and absent.
Approach:    Add the static fields. Keep the state fields in state.
Decision:    **Avoid a plugin framework for six sources** -- that abstraction
             costs more than it saves and is a common way this kind of project
             becomes unmaintainable. Adapters only where source differences
             genuinely require them; today every source is newline-ish plaintext
             and one parser handles all of them.
Prove:       A test that every registry entry has every required field
             populated, so adding a source without one fails.

---

### T-101 Source quality is asserted per source and measured for none

Source:      the brief's section 19 (source quality)
Category:    sources
Priority:    P2
Effort:      M
Status:      open

Problem:     `Trust` is assigned by hand in the registry from a reading of each
             project. **MUST NOT treat all sources equally** is satisfied; the
             measurement behind the trust levels is not.
Premise:     **One dimension is measured**: unique contribution, by
             `experiments/19` -- `desirefire_all` 995 of 1091, `newtrackon_all`
             146 of 261, `ngosang_i2p` 13 of 13, `xiu2_all` 8 of 150,
             `ngosang_ws` 3 of 3, `ngosang_all` **2 of 99**. The rest --
             uptime, freshness, historical reliability, format stability,
             maintainer activity, correctness, duplicate percentage -- are not.
Approach:    Measure each per source and re-run periodically; **source quality
             is not a one-time judgement**.
Decision:    **Two asymmetric findings drive action and neither is "drop it".**
             A source contributing no unique data may not justify its
             maintenance cost -- **but check whether it serves as corroboration
             before dropping it**, because two sources agreeing is evidence and
             one source is a single point of failure. `ngosang_all` at 2 unique
             of 99 is exactly this case and is **kept** for corroboration.
             A source with many unique but poor-quality entries needs **lower
             trust or stricter filtering, not removal** -- uniqueness is what an
             aggregator exists to capture. `desirefire_all` is that case.
Prove:       A documented methodology plus a per-source quality report with
             sample counts, regenerated on a schedule.

---

### T-102 Change-detection thresholds are provisional and say so

Source:      the brief's section 13.2 (change detection)
Category:    sources
Priority:    P2
Effort:      S
Status:      done

Problem:     `expected_min` and `expected_max` are ~40% and ~3x of a **single**
             observation taken on 2026-08-29. They are wide on purpose and they
             are not derived from behaviour.
Premise:     **Thresholds MUST be derived from observed source behaviour, not
             invented.** Until enough history exists, conservative provisional
             values marked as provisional are the correct interim -- and they are
             marked, in the registry's own comment.
Approach:    Once history accumulates, derive per-source bands from the observed
             distribution and replace the provisional values, recording the
             sample the derivation used.
Decision:    **A magic number nobody can justify is a future outage.** Widen
             rather than narrow while the sample is one.
Prove:       Each threshold cites the observation window it was derived from,
             and a test fails when a threshold has no derivation recorded.


**Done.** 2026-09-09.
`python3 -m unittest tests.test_thresholds` -> **10 tests, OK**, and the `Prove` clause is met in both halves: every band
names the observations it came from, and a test fails when one does not.

⛔ **Two of the eight bands did not satisfy the rule their own comment
claimed.** The comment said "~40% of the single observation" and "~3x";
`ngosang_all` was `(40, 300)` where 0.4x/3x of 99 is `(39, 298)`, and
`xiu2_all` was `(60, 450)` where 3x of 150 is 451. Each was **one entry**
narrower than its stated derivation -- which is harmless in itself and is
RULES 2.1 in miniature: numbers typed *beside* a method drift from it
silently. Both are widened to contain their derivation.

⭐ **The derivation is the authority now, not the comment.** `Derivation`
carries the observations, the instant each was taken at, and the committed
instrument that measured them; `derived_band()` computes what the method
justifies, and `tests/test_thresholds.py` fails when a registry band is
**narrower** than that. Wider is always allowed and is usually right -- a small
source's natural variation is bigger than a multiple of a small number -- but
narrower is now unrepresentable without also narrowing the evidence.

⚠ **The window is still one observation per source, and that is the
honest state rather than a finished job.** `TheWindowIsHonestlySmall` asserts
it and is **written to start failing**: the day a second observation is
recorded for any source, it fails and sends the next session back here to
derive that band from a real distribution. A wide band nobody revisits is the
exemption-nobody-removes row of
[`../docs/conventions/forbidden-patterns.md`](../docs/conventions/forbidden-patterns.md),
and this is the alarm on it.

⛔ **What would actually accumulate the window is not built, and it is not
this entry.** Nothing records per-source volume over time: `experiments/19` has
two committed runs and **both read the same pinned fixtures**, so they are one
observation rather than two, and the publisher fetches every upstream eight
times a day while keeping none of the counts. That is
[T-103](sources.md)'s shape -- provenance snapshots are not retained -- and it
is what turns this entry's alarm into work somebody can do.


---

### T-103 Provenance snapshots are not retained

Source:      the brief's section 13.3 (provenance)
Category:    sources
Priority:    P2
Effort:      M
Status:      done

Problem:     `FetchResult` carries a `content_sha256` and it is discarded after
             the run. Nothing can answer, later: what did the source return,
             when, what parser version processed it, what changed, and why was
             it accepted or rejected.
Premise:     Per-tracker provenance *is* retained (`Aggregate.provenance` maps
             URL to contributing source ids). Per-*source* snapshot history is
             not.
Approach:    Retain source id, first seen, last seen, category and
             validation/check history per tracker; and compact per-source
             snapshots or hashes sufficient to answer the questions above.
Decision:    **MUST NOT retain unlimited raw upstream data in git.** Use hashes,
             compact artefacts, rolling history, or workflow/release artefacts.
             **Compute the growth rate before choosing** -- same arithmetic
             discipline as T-042. Note `C-44`: artefacts expire, so an artefact
             is not durable evidence.
Prove:       A test that the retained provenance answers "why did this tracker
             disappear" for a synthetic disappearance.


**Done.** 2026-09-09.
`python3 -m unittest tests.test_provenance` -> **19 tests, OK**, including the
`Prove` clause's synthetic disappearance.

⭐ **The question has an answer and the answer refuses the plausible wrong
one.** `provenance.explain_absence` takes a URL, the sources recorded carrying
it, and each source's fetch history, and returns whether it was **genuinely
removed** or whether this is **our blind spot**:

| what the history says | verdict |
| --- | --- |
| every carrying source fetched cleanly, none lists it | `genuinely_removed` |
| a carrying source's last fetch `FAILED` | **not** removed -- we could not read it |
| a carrying source's last fetch was `REJECTED` | **not** removed -- we refused the body, so we hold no listing to act on |
| a carrying source returned `EMPTY` | removed: it *told* us it has nothing, which is evidence |
| no source ever carried it | not a disappearance at all |

⛔ **That table is RULES 3.2 extended through time**, and time is the only
place a consumer can ask it: by the time anybody notices a tracker is missing,
the run that lost it is over. Both pieces of prior art here get the one-run
version wrong, in two languages; getting the across-time version wrong would
tell a consumer a tracker is gone every time an upstream had a bad afternoon.

⛔ **The module committed that exact conflation in its first draft**, and a
test caught it: `SourceObservation.ok` excluded `EMPTY`, so a source that
successfully reported having nothing was filed as a blind spot. `acquire.Outcome`
says in as many words that `EMPTY` is information and `FAILED` is not.

⭐ **The growth was computed before the shape was chosen**, which the
`Decision` demands by name. `experiments/38-source-history-size.py` builds a
full record **through the production writer** and measures it:

| | |
| --- | --- |
| widest record | **34109 bytes**, at the configured caps |
| steady-state file | **0.26 MB** across 8 sources |
| days to fill the ring | **30**, at D7's eight publishes a day |
| the same file **without** caps, over five years | **3799 MB** |

A ring of 240 and 365 daily aggregates is what makes the difference between
those last two numbers, and neither is a guess.

⛔ **Hashes and counts, never bodies**, as the `Decision` requires: a
digest answers "did it change", a count answers "by how much", and the raw
bodies stay in the snapshot cache, which is not history.

⛔ **The clock is injected, and it was not in the first draft.** Each
observation was stamped with its own `FetchResult.fetched_at`, read from an
ambient clock -- so two runs over identical inputs produced different bytes
(RULES 3.6) and the idempotence guard could never recognise a repeat, because
every timestamp was new. Every observation in a run carries the run's injected
instant now, exactly as `state.py` stamps a sweep's records, and a test asserts
two folds at one instant add one observation.

⭐ **It accumulates where the state does**, for the reason `C-44` names: an
artefact expires after 90 days and a branch does not. `generate.py` folds this
run's own fetches and writes `sources.jsonl`; the publisher carries it off the
`data` branch and back.

⭐ **And it is what [T-102](sources.md) is waiting for.**
`SourceHistory.entries_seen()` returns exactly the quantity the change detector
compares against -- `validate_counts` is called with `len(accepted)`, and that
is what is recorded -- so the bands can be derived from a real distribution
rather than from one observation. A test asserts a failed fetch contributes
**no** count to it.


---

### T-104 Conditional requests are not implemented

Source:      RULES 5.4
Category:    sources
Priority:    P2
Effort:      S
Status:      done

Problem:     Every fetch is unconditional. ETag and Last-Modified are ignored,
             so every run re-downloads every source in full.
Premise:     **Measured that it would work**: `experiments/21` observed a strong
             ETag on `raw.githubusercontent.com` with `max-age=300`, so
             conditional requests are supported and polling can be cheap.
Approach:    Send `If-None-Match` / `If-Modified-Since`, handle 304 as a
             distinct outcome meaning "unchanged", not as a failure and not as
             empty.
Decision:    RULES 5.4 owns the caching rule and this entry implements it: no
             unnecessary cache defeat, no random query parameter, and
             source-scoped busting only against a documented demonstration.
             What this entry adds is the outcome shape. A 304 is a **third**
             outcome alongside OK/EMPTY/FAILED and must not be squeezed into
             one of them -- the same class of conflation RULES 3.2 is about.
Prove:       A test that a 304 preserves the previously accepted data and is
             recorded distinctly from both success and failure.

**Done.** `python3 -m unittest tests.test_conditional_requests -v` -> **15
tests, OK**. `fetch` sends `If-None-Match` and `If-Modified-Since` when a
previous run's validators are held, and the publisher keeps them in a workflow
cache. ⛔ No cache defeat and no random query parameter, which RULES 5.4 calls
rude, ineffective and a fast route to 403.

⛔ **`UNCHANGED` is a third outcome and that is the entry.** Not `OK`, because
no body arrived; not `FAILED`, because nothing went wrong; and emphatically not
`EMPTY`, which would delete the source. A 304 carries the held snapshot forward
and its trackers reach the dataset, which the `Prove` clause asserts directly.

⚠ **A 304 with no snapshot is a `FAILED`, and it says whose fault it is.** The
server is right that nothing changed and we have nothing to show for it, which
is a fact about our cache rather than about the source; reporting it as
unchanged would publish an empty source as current.

⚠ **The snapshot cache is treated as a cache.** Absent, expired, partial or
corrupt all degrade to a full fetch rather than to an error, because the worst
case of a miss is one download and the worst case of an error is no dataset.

⛔ **THIS FOUND A LIVE RULES 4 VIOLATION, WHICH IS WORTH MORE THAN THE ENTRY.**
`load_corpus` populated the raw source bodies on its `--offline` branch only,
and a blacklist's reasons live in the raw text the ordinary parser strips -- so
an online run collected **zero** exclusions and enforced none. The publisher
runs online. **Eight URLs an operator had asked to be excluded were in the
published dataset.** Both paths carry the body on the result now, a test
asserts the two agree, and the published dataset went from 1334 trackers to
1326 with **0** excluded URLs remaining. [`../HISTORY/corrections.md`](../HISTORY/corrections.md)
round 4 row 1 records it.

---

### T-105 Sources named by the brief and by other aggregators are not in the registry

Source:      HISTORY/reference-sweep.md
Category:    sources
Priority:    P3
Effort:      M
Status:      open

Problem:     Eight sources are registered. Several named upstreams are not, and
             one reference is a registry of sources in its own right.
Premise:     **Read**: `references/GerryFerdinandus__bittorrent-tracker-editor/tree/source/code/`
             carries `ngosang_trackerslist.pas` and `newtrackon.pas`, which
             enumerate the source URLs a real client fetches -- that is the
             "registry of tracker-list sources" the brief pointed at, expressed
             as code. XIU2's workflow additionally names
             `github.itzmx.com/1265578519/OpenTracker` (over **plain HTTP**),
             `tinytorrent.net`, and `torrenttrackerlist.com`.
Approach:    Evaluate each for usefulness, maintenance, uniqueness and rot
             before adding. **Expect substantial overlap and substantial rot;
             measure both.**
             **Also now in scope: FOSS-ecosystem sources.** T-046's settled rule
             makes `foss.txt` half source-derived and it has no sources yet.
             Fosstorrents and distribution-run trackers are the candidates.
Decision:    A plain-HTTP source is an integrity question, not merely a style
             one: an unauthenticated fetch can be modified in transit and its
             contents go straight into a published list. Either fetch over HTTPS
             or record the risk explicitly with its mitigation.
Prove:       Each candidate has a recorded verdict -- adopted with measured
             unique contribution, or refused with the reason.

---

### T-106 `hardcoded.txt` has no input file

Source:      T-046
Category:    sources
Priority:    P3
Effort:      S
Status:      open

Problem:     The rendering path for a manual list is implemented and tested
             (`render_plaintext(preserve_order=True)` preserves order and
             self-deduplicates). **There is no file for a maintainer to edit.**
Premise:     The behaviour is proven; only the input is missing.
Approach:    A tracked `hardcoded.txt` read as a source with its own role, never
             sorted, never ranked, never silently rewritten, health-checked like
             everything else.
Prove:       A test that editing the file changes the output in the maintainer's
             order, and that the pipeline never rewrites the file itself.

⚠ **This file is the second door onto the credential rule**, found by the door
sweep of 2026-09-05. [T-107](sources.md) refuses a credential-bearing URL in
`pipeline.aggregate`, and `render_plaintext` -- the single write path for the
published format -- does not check: it is public and takes any list of
trackers. The manual path calls it directly, so **the moment `hardcoded.txt`
has content, a passkey URL in it reaches the output ungated.** Nothing reaches
that door today, which is why it was recorded rather than fixed; whatever
creates the file closes it, either by routing the manual list through the same
refusal or by making `render_plaintext` refuse loudly.

### T-107 The pipeline republishes private-tracker credentials

Source:      `C-70`, found by the secret sweep adopted on 2026-08-31
Category:    sources
Priority:    P1
Effort:      S
Status:      done

Problem:     Seven announce URLs in the accepted dataset carry a passkey: six
             distinct credentials, entering from `DeSireFire/animeTrackerList`
             and `pkgforge-security/Trackers`, reaching `trackers_all.txt`
             unchanged. `src/trackers/normalize.py` already knows announce
             paths carry passkeys and preserves them deliberately; nothing
             downstream acts on that.
Premise:     The shape is recognisable without a network call: a `passkey`
             query parameter, or a path component of 20 or more opaque
             characters beside `announce` or `scrape`.
             `scripts/check-no-secrets.py` already matches it.
Approach:    Refuse at the exclusion stage, not the normalization stage, so the
             decision is auditable per RULES 3.10 and the reason is recorded
             rather than the row silently vanishing. A private tracker is not a
             public tracker and a passkey-bearing URL is not usable by anybody
             else in any case, so this removes nothing a consumer could have
             used.
             **Do not redact and republish.** A URL with the token stripped is
             an endpoint that answers differently, and publishing it as though
             it were the tracker is the invented-endpoint mistake `C-66`
             already cost this project once.
             **Do not edit the fixtures.** They are verbatim captures and a
             rewritten fixture is not a capture.
Prove:       `python3 scripts/generate.py --offline --out DIR` writes zero
             matches for
             `(passkey=|/announce/[0-9a-f]{20,}|/[0-9a-f]{20,}/announce)`,
             the exclusion report names each refused URL with its reason, and
             the ceiling in `scripts/check-no-secrets.py` comes off in the same
             change.
Decision:    Refuse rather than redact, and refuse in `exclusion.py` rather
             than in `normalize.py`. Rejected: redact-and-publish (invents an
             endpoint); drop silently in the parser (unauditable, RULES 3.10);
             leave it to the consumer (this project exists to do better than
             concatenation, and this is the clearest instance of it).

             ⚠ **The `Prove` clause above said the ceiling "comes off", and
             taking that literally would have broken the check.** The ceiling
             counted credentials in *tracked files*, and six of them live in
             `tests/fixtures/` and `experiments/fixtures/` -- verbatim captures
             this entry itself forbids editing. Removing the ceiling with the
             scope unchanged would have failed the gate over files that are
             supposed to contain them. RULES 9 permits changing a requirement
             and forbids changing one silently, so:

             **as written**: allow six credentials in the tree, and remove that
             allowance when the pipeline stops publishing them.
             **why it is wrong**: the allowance and the defect were never the
             same thing. The defect was publication; the tree's copies are
             third-party evidence and are permanent.
             **the replacement**: no ceiling anywhere, and a path rule instead
             -- **zero** credentials outside a verbatim capture, and inside one
             a count that is reported and never fails. The publication defect is
             now caught structurally instead: the pipeline refuses the URL and
             `scripts/generate.py` refuses to publish one.

             That is strictly stronger than the ceiling it replaces. The ceiling
             would have passed a seventh credential written into `src/`; the
             path rule fails on the first.

**Done.** `python3 -m unittest tests.test_credentials` -> `Ran 13 tests`, `OK`.
`python3 scripts/generate.py --offline --out DIR` publishes **1327** trackers,
seven fewer than before, and

```bash
grep -cE '(passkey=|/announce/[0-9a-f]{20,}|/[0-9a-f]{20,}/announce)' DIR/trackers_all.txt
```

prints `0` and exits 1. The run report's new *Refused entries* section names all
fifteen refusals with reasons: seven for a credential, eight for an upstream
operator request or safety.

**The pattern has one home.** `src/trackers/exclusion.py` owns
`PRIVATE_CREDENTIAL` and `scripts/check-no-secrets.py` imports it, because two
patterns for one rule are two places for it to be wrong. A test asserts the
import is still there and that the retired ceiling has not come back.

⭐ **Two defects were found by building it, and neither was in the plan.**

1. **The refusal count was wrong before it was ever read.** Keying the record
   on the *masked* URL collapsed two people's passkeys on one endpoint into one
   row: seven URLs refused, six recorded. It is keyed on the raw URL and
   rendered masked, and `test_the_refusals_are_counted_per_url_not_per_masked_
   string` is the regression.
2. **The narrowing that lets the tests exist reintroduced the whole-line
   allowlist.** `check-no-secrets.py` matched with `search`, so the *first*
   token on a line decided the verdict for the whole line -- a synthetic test
   vector would have hidden a real credential written beside it, which is the
   row in `forbidden-patterns.md` this project already had a name for. It reads
   every match now, and the test plants exactly that pair.

**Mutation-proved.** A credential-bearing line written into `docs/` fails the
check (exit 1); removing it passes (exit 0). The narrowing is proved on the
matched token rather than the line, both directions.

**Nothing was redacted and republished**, and the report is held to the same
rule as the dataset: a refused URL is listed with the token removed, because
refusing to publish a credential in one file and printing it in the next one is
not a fix. `mask_credential` is the only thing that writes one down, and its
output is asserted to no longer match the detector.
