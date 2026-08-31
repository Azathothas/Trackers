# Publication

Outputs, formats, versioning, the data branch and release channels. P4.

The P4 gate is: **a failed generation demonstrably leaves prior public data
intact -- tested, not asserted.** That half is already done and running in CI
(`.github/workflows/gate.yml`, "Atomic publication"); it regenerates with an
empty fixture directory so every source fails, and asserts the previous output's
sha256 is unchanged.

---

### T-060 JSON and CSV outputs do not exist

Source:      the brief's section 17.2 (JSON and CSV outputs)
Category:    publication
Priority:    P1
Effort:      M
Status:      open

Problem:     Only plaintext is emitted. The richer formats are where health,
             provenance and vantage actually fit, and none of them exist.
Premise:     The data exists in `Aggregate` -- provenance, dedup decisions,
             rejections, exclusions -- and is currently discarded at render time.
Approach:    Candidate fields: tracker, normalized tracker, transport,
             network, hostname, port, category, source(s), liveness,
             health state, **measurement rung reached**, latency, latency
             statistics, **vantage metadata**, reliability score, score
             version, checks, successes, failures, first seen, last seen,
             last checked, last success, last failure, failure classification
, confidence and sample information.
Decision:    **Every field MUST have a defined meaning in the schema
             documentation, and MUST NOT be included because it sounds useful.**
             A field nobody can define is a field consumers will misread. The
             two-axis model means `transport` and `network` are separate
             columns; collapsing them into one "protocol" column would re-import
             the bug the model exists to prevent.
Prove:       Every emitted field appears in `docs/schema.md` (planned) with a definition,
             enforced by a test that diffs the field set against the schema.

---

### T-061 Cross-format consistency is unverified

Source:      the brief's section 17.3 (cross-format consistency)
Category:    publication
Priority:    P1
Effort:      S
Status:      open

Problem:     JSON, CSV and plaintext must represent the same accepted dataset.
             Divergence between formats is a silent-corruption failure that
             **consumers cannot detect**.
Premise:     Partly held today: `scripts/generate.py` `verify()` already asserts
             the plaintext line count equals the tracker count. With one format
             that is nearly vacuous.
Approach:    Extend the pre-publication verifier to compare the three format's
             tracker sets exactly, not only their counts.
Prove:       A test that mutating one format and not the others fails
             publication.

---

### T-062 Nothing is versioned, so a consumer cannot tell what they received

Source:      the brief's section 17.4 (versioning)
Category:    publication
Priority:    P2
Effort:      S
Status:      open

Problem:     `NORMALIZATION_VERSION` exists in `src/trackers/__init__.py` and
             reaches only the run report. The schema and the scoring methodology
             have no versions at all.
Premise:     Trivially available; not wired through.
Approach:    Generated metadata must let a consumer determine **what dataset
             they received, when it was generated, what methodology produced it,
             and what schema it follows.** Version the JSON/CSV schema, the
             scoring methodology and the normalization rules independently,
             because they change for different reasons.
Prove:       A test that every published artefact carries all four, and that
             changing normalization semantics without bumping the version fails
             a gate.

---

### T-063 There is no data branch and nothing is published anywhere

Source:      the brief's section 18.1 (repository data publication);
             decision **D5**
Category:    publication
Priority:    P1
Effort:      M
Status:      open

Problem:     `scripts/generate.py` writes to a local `out/` that is gitignored.
             Nothing reaches a consumer.
Premise:     **The raw-hosting half is measured and favourable.**
             `experiments/21` found `raw.githubusercontent.com` serves
             `cache-control: max-age=300` with a strong ETag, content current
             within seconds of a push. The feared "caching defeats hourly
             generation" failure **does not occur**: 300 s << 3600 s.
             Also measured (`C-55`): **scheduled workflows run only on the
             default branch**, so a data branch can never carry its own cron.
Approach:    `main` carries code, tests, workflows, methodology and
             configuration; a `data` branch carries generated data at its root,
             giving `raw.githubusercontent.com/<owner>/<repo>/data/<file>`.
Decision:    **D5, open**, and see T-064 for the blocked half. The data-branch
             half is not blocked and can proceed.
Prove:       `curl -sS https://raw.githubusercontent.com/Azathothas/trackers/data/trackers_all.txt`
             returns the generated content, and
             `python3 -m unittest tests.test_publication -v` (planned) asserts a
             failed generation never pushes.

---

### T-064 Release channel semantics rest on three unverified platform claims

Source:      the brief's section 18.2 (release channels); decision **D5**;
             depends on T-003
Category:    publication
Priority:    P2
Effort:      M
Status:      open

Problem:     Three channels are specified -- `latest` (rolling, updated every
             successful generation, **never** replaced by a failed or suspicious
             run), `daily` (once per UTC day), `weekly` (once per week at 00:00
             UTC on the first day of the defined week). Their implementation
             depends on how GitHub actually behaves.
Premise:     Unverified: `C-14`, `C-15`, `C-17`.
Depends on:  T-003, which is no longer blocked -- creating throwaway releases
             here is sanctioned (RULES 13.1). Do T-003 first; this entry is
             cheap once its three answers exist and guesswork otherwise.
Approach:    Once T-003 reports how the platform actually behaves, implement
             each channel against the observed behaviour rather than the
             assumed one, and assert each channel's semantics in a test that
             exercises a real release. Until then, publish over the data branch
             only (T-063), which is not blocked.
Decision:    **If a tag named `latest` collides with GitHub's own
             `/releases/latest` resolution, rename the channel.** Do not ship a
             naming coincidence as a contract. If moving tags is safer than
             rewriting releases, use the safer one. The week convention
             **SHOULD** be ISO-8601 (Monday) unless evidence favours otherwise,
             and must be defined explicitly rather than implied.
             **MUST NOT assume mutable release semantics behave like immutable
             versioned releases.**
Prove:       Each channel's semantics are asserted by a test against real
             platform behaviour, not against an assumption.

---

### T-065 Filenames, checksums and asset duplication are undecided

Source:      the brief's section 18.3 (formats and filenames)
Category:    publication
Priority:    P3
Effort:      S
Status:      open

Problem:     Publish plaintext, JSON, CSV, reports, metadata and checksums under
             **stable predictable filenames**, avoiding unnecessary duplication
             of large assets. Only one filename exists today
             (`trackers_all.txt`) and no checksums.
Premise:     Stable paths are part of the consumer contract (RULES, and the
             README's contract section), so renaming later is a breaking change.
Approach:    Fix the filename set before first publication, because that is the
             cheapest moment; publish checksums beside the data.
Prove:       A test that the published filename set matches the documented one
             exactly, so an accidental rename fails rather than silently
             breaking consumers.

---

### T-066 Run reports do not answer the questions observability requires

Source:      the brief's section 24 (observability and reports)
Category:    publication
Priority:    P2
Effort:      M
Status:      open

Problem:     `render_report` covers sources, counts, transport, network and
             measurability. The required set is much larger and currently
             unanswerable because health does not exist.
Premise:     The source half is done; the health half waits on T-020.
Approach:    Every successful run must make answerable: which sources were
             fetched, succeeded, failed, were rejected; how many trackers
             accepted and discarded; duplicates; health checks run;
             live/dead/unknown/unmeasurable counts; latency distribution;
             ranking changes; suspicious changes; whether publication succeeded;
             whether old data was retained.
             Per category: total, unique, duplicate count and percentage,
             valid/invalid, live/dead/unknown/unmeasurable, protocol
             distribution, latency distribution with median and percentiles,
             reliability distribution, check counts, success/failure counts,
             last checked, last successful check, last failure, source
             contribution, source failures, stale sources, sustained tracker
             failures, **measurement-rung distribution**.
Decision:    **Reports must make ranking changes understandable** -- a consumer
             seeing a tracker drop should be able to find out why. Logs useful
             without becoming enormous.
Prove:       A test that the report contains every required field for a fixture
             dataset, so a field silently disappearing fails.
