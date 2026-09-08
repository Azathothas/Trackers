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
Status:      done

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

**Done.** `python3 -m unittest tests.test_labelled -v` -> **16 tests, OK**.
`src/trackers/labelled.py` renders JSON and CSV,
[`../docs/schema.md`](../docs/schema.md) defines all 18 fields, and the test
diffs the two **in both directions**: a field emitted without a definition is
one consumers will misread, and a definition with no field behind it is a
promise the data does not keep. Checking one direction catches half.

⭐ **Four candidate fields from this entry are deliberately absent**, and the
schema says which and why rather than leaving a reader to wonder. `latency` and
its statistics: the history ring keeps each observation's outcome and rung and
not its round-trip time, so the column would be a number this project does not
retain. `reliability_score` and `score_version`: no model is chosen
([T-044](scoring.md)), deliberately. `confidence`: over an unchosen model that
is an adjective with a decimal point. `category`: [T-046](scoring.md) has not
built them.

⛔ **An unprobed tracker is `unknown` with a `null` rate, never `0`.** A new
tracker and one that failed every check are different facts and a zero says the
second about the first. Asserted in both directions: a tracker with one failed
observation renders `success_rate` 0.0, and one with none renders `null`.

**The two axes stay two columns.** `transport` and `network` are separate, so
`udp://tracker.i2p/announce` is `udp` and `i2p` rather than one collapsed
`protocol` that would re-import the defect the classifier exists to prevent.

---

### T-061 Cross-format consistency is unverified

Source:      the brief's section 17.3 (cross-format consistency)
Category:    publication
Priority:    P1
Effort:      S
Status:      done

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

**Done.** `python3 -m unittest tests.test_publication -v` -> **11 tests, OK**.
`scripts/generate.py`'s `verify()` compares the three formats' **tracker sets**
rather than their counts, and refuses to publish on any difference.

⛔ **Sets, not counts, and the difference is the whole entry.** Two formats
holding the same number of different URLs is a silent corruption a consumer
cannot detect, and a count check passes it. Mutation-proved in both directions:
dropping a row from the JSON fails, and swapping a URL for another while
keeping the count identical fails too -- the second is the one a count check
would have shipped.

---

### T-062 Nothing is versioned, so a consumer cannot tell what they received

Source:      the brief's section 17.4 (versioning)
Category:    publication
Priority:    P2
Effort:      S
Status:      done

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

**Done.** `python3 -m unittest tests.test_versions -v` -> **9 tests, OK**.
Three versions, independent because they change for different reasons:
`schema_version` for the field set, `normalization_version` for the rules that
produced the URLs, and `scoring_version` -- which is ⛔ **`null`, and that is
the honest value**: no model is chosen ([T-044](scoring.md)), so there is no
methodology to version and a `1` would tell a consumer one exists and is
stable.

⭐ **A `digest` over the rows answers the question the timestamp cannot.**
Every run writes a new `generated_at`, so a consumer comparing timestamps sees
movement on every publish whether or not anything moved. Two documents with one
digest carry the same data. Hashed over the rows alone, so adding a document
field does not change the digest of data that did not move.

⛔ **CSV cannot carry document metadata, so `metadata.json` exists** and the
requirement is met rather than dropped (RULES 9). A version column repeated on
1334 identical rows is not a header and a comment line breaks the format for
the readers people choose CSV for, so the release describes itself in a file
beside the data, carrying the versions and a **size and `sha256:` per published
file**. A consumer of the plaintext or the CSV hashes what they hold and
compares.

**The second half of the `Prove` clause is the one with teeth**, and it is a
golden table: `NORMALIZATION_VERSION` is pinned to what version 1 promises, so
changing normalization semantics without moving the number fails. ⭐ An
unpinned version is a number somebody remembers to update, which is to say one
that is eventually wrong while looking authoritative.

⚠ **Mutation testing took four attempts to break it, and the first three
failures were findings rather than gaps.** Two rules are enforced twice, so a
single-line change to either does not alter behaviour: `normalize.py`'s
`host.lower()` runs on a host `urlsplit().hostname` has **already** lowercased,
and the whitespace strip runs on both sides of comment removal. The fourth
mutation -- dropping an explicit `:80` when rendering the URL, which is a single
decision point -- fails the table with the before and after printed.

---

### T-063 There is no data branch and nothing is published anywhere

Source:      the brief's section 18.1 (repository data publication);
             decision **D5**
Category:    publication
Priority:    P1
Effort:      M
Status:      done

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

**Done.** The dataset is public. Workflow run `34280454871` created the
`data` branch and published to it. The `Prove` command, run verbatim:

```
http://00.mercax.com:443/announce
http://00.xxtor.com:443/announce
http://004430.xyz:80/announce
```

1334 lines, and every one of the six files answers HTTP 200 at
`raw.githubusercontent.com/Azathothas/Trackers/data/`: the plaintext, the JSON,
the CSV, the report, `schema.md` beside the data it defines, and `state.jsonl`.

⭐ **It is published as a labelled dataset**, which is the only form the value
gate justifies. 1334 trackers: **79 live**, 1 degraded, 34 unmeasurable, 1220
honestly `unknown`, and ⛔ **nothing `dead`**, because that needs three
observations of one tracker and the history is younger.

⚠ **1334 rather than the 1327 this repository's fixtures hold**, and the
difference is the point: the workflow fetches the upstreams live, while the
committed figure comes from a pinned snapshot. Neither is wrong and neither is
the other.

`python3 -m unittest tests.test_publication -v` -> **11 tests, OK**: a failed
generation writes nothing and leaves the previous output byte-identical, driven
through the real script against an empty fixture directory rather than by
patching a function.

**State lives on the branch**, not in an artefact. History is what makes a label
worth reading and a workflow artefact expires after 90 days, so `state.jsonl`
is fetched from `data`, folded, and pushed back with the dataset it produced.
That is also the shared-across-runs state [T-084](operations.md) named as
missing when it recorded the re-run hazard.

---

### T-064 Release channel semantics rest on three unverified platform claims

Source:      the brief's section 18.2 (release channels); decision **D5**;
             depends on T-003
Category:    publication
Priority:    P2
Effort:      M
Status:      done

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

**Done.** `python3 -m unittest tests.test_channels -v` -> **17 tests, OK**.
`src/trackers/channels.py` is the semantics and every one of its shapes is
decided by a measurement rather than by a preference.

⭐ **The tests read `experiments/24`'s committed result and fail if the design
rests on something that run refuted.** That is the `Prove` clause's "against
real platform behaviour" in the strongest form available without creating a
release per test run: the platform was measured once, the measurement is in the
tree, and the design is re-checked against it on every push.

**Three measured answers, three design consequences.**

| measured | consequence |
| --- | --- |
| `C-14`: a release tagged `latest` **lost** `/releases/latest` to a newer one | the rolling channel is **not** called `latest`. D5 says rename rather than ship a naming coincidence, and the name promised a contract the platform does not deliver |
| `C-17`: **refuted** -- a moved tag left `target_commitish` at the old commit | a channel is updated by **delete-and-recreate**, never a tag move. Otherwise the release object and the tag disagree and neither is wrong |
| `C-15`: the stable asset URL served the **previous bytes** 10 s later, with the old `ETag` and no `Cache-Control` | publication **may not** verify by reading the download URL back. Made structural: the module imports no network module at all, and a test parses it to check |

⭐ **A period is published once.** `daily-2026-09-08` is that day rather than
the last run inside it, so a second run does not replace it; the rolling
channel carries no such promise and always moves. Deciding this from the tags
that exist on the remote rather than from a stored cursor is deliberate: the
remote is what a consumer reads.

⚠ **The ISO week is the explicit part D5 asked for.** `weekly-2027-W53` is what
2027-01-01 gets, because ISO puts that Friday in 2026's week 53 and a tag built
from the calendar year would sort before every week of the year it belongs to.
A test asserts exactly that date.

**Five mutations planted, five caught**: the rolling tag renamed to `latest`,
the week taken from the calendar year, a timestamp without a zone accepted, an
empty output allowed to take a channel, and a published period republished.

⚠ **What this does not do is publish.** Where the artefacts go is
[T-063](publication.md) and nothing here creates a release; the semantics exist
so that when it does, they are not invented at the point of writing a workflow.

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
