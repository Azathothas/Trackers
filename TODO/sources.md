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
Status:      open

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

---

### T-103 Provenance snapshots are not retained

Source:      the brief's section 13.3 (provenance)
Category:    sources
Priority:    P2
Effort:      M
Status:      open

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

---

### T-104 Conditional requests are not implemented

Source:      RULES 5.4
Category:    sources
Priority:    P2
Effort:      S
Status:      open

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

### T-107 The pipeline republishes private-tracker credentials

Source:      `C-70`, found by the secret sweep adopted on 2026-08-31
Category:    sources
Priority:    P1
Effort:      S
Status:      open

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
