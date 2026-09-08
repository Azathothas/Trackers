# Gates

The phase gates and the two justification gates. A gate is a command or a
committed artefact, never a feeling that a phase is finished.

**Two of these can legitimately end the project**, and reaching either
conclusion honestly is a *successful* outcome of the work. Building the system
anyway to avoid delivering bad news is not.

---

## The phases

| phase | delivers | gate | state |
| --- | --- | --- | --- |
| **P0 -- ground truth** | reference sweep, runner experiments, claims register, D1 and D2 decided | experiments committed and runnable; no `UNVERIFIED` row a later phase depends on; **the measurement gate answered** | **passed** |
| **P1 -- acquire** | source registry, fetch, validate, normalize, deduplicate, provenance, plaintext. No health checking. D3 decided | a published plaintext list, generated end-to-end from fixtures with no network, byte-identical across two runs | **passed**, except D3 which is [T-040](../TODO/scoring.md) |
| **P2 -- measure** | health checking to the ladder, JSON and CSV, vantage metadata, the fake-tracker oracle. D6 decided | probe validated against the local fake tracker for **every** failure mode; **the value gate answered** | **both gate conditions met**; the phase's own deliverables are not all built |
| **P3 -- score** | history and state, scoring, ranking, categories. D4 decided | the six invariants enforced by property tests; bootstrap-from-nothing passes | not started |
| **P4 -- publish** | data branch and/or releases, channels, reports, atomic publication. D5 decided | a failed generation demonstrably leaves prior public data intact -- tested, not asserted | **the gate itself is already met**; the phase is not |
| **P5 -- operate** | issue automation, housekeeping, self-healing, the long-term review | the fourteen questions answered with mechanisms, not intentions | not started |

Phases may overlap where genuinely independent. **A gate may not be skipped, and
a gate that cannot be met is reported rather than quietly downgraded.**

P4's gate is met ahead of its phase because atomic publication was cheaper to
build correctly than to retrofit: `.github/workflows/gate.yml` regenerates with
an empty fixture directory so every source fails, and asserts the previous
output's sha256 is unchanged.

---

## The measurement gate -- **PASSED**, 2026-08-29

**The question.** If the environment cannot support any tracker-protocol
measurement more meaningful than "the hostname resolves", then the ranking,
scoring and reliability half of this project is unbuildable as specified, and
the honest response is to report that and propose the reduced project --
aggregation, validation and provenance, honestly labelled -- rather than shipping
a scoring system whose scores mean nothing.

**The evidence.** Workflow run `33383406869`, 2026-09-01, two runner images,
instruments `experiments/01`, `02` and `05`, with the results committed under
`experiments/results/`.

| transport | rung reached | evidence |
| --- | --- | --- |
| `udp` | **protocol-valid** (BEP 15 connect, connection id returned) | 10/11, 9/11, 10/11, 10/11 across four runs; loopback positive control passed every run. 10 is the ceiling: one target has no IPv4 address |
| `http` / `https` | **tracker-semantic** (well-formed bencoded scrape response) | 4/6 subjects; positive **and negative** controls passed on both images |

**The negative control is what makes this a pass rather than a hope.** A local
server returning HTTP 200 with HTML was correctly **not** classified as a
tracker, so the discriminator is not the naive status-code check that the
anti-pattern table exists to prevent.

**Verdict: the scoring and reliability half is buildable**, for clearnet
trackers on `udp`, `http` and `https` -- **1333 of 1346** distinct URLs in the
census.

**What the gate does not clear.** These are retained as explicit requirements
with a stated limitation, not dropped:

| not measurable here | count | required state |
| --- | --- | --- |
| IPv6-only trackers (no IPv6 egress, `C-04`) | **16** on 15 hosts, of 1327 -- `experiments/29`, 2026-09-08 | `unmeasurable` |
| `i2p` network | 13 | `unmeasurable` |
| `yggdrasil` network | >=1, under-counted ([T-023](../TODO/measurement.md)) | `unmeasurable` |
| `wss` (`C-36` unverified, [T-005](../TODO/claims.md)) | 10 | `unmeasurable` |

Every one **MUST** be published as `unmeasurable` and **MUST NOT** be scored or
reported `dead`.

⭐ **The IPv6 row carried a dash until 2026-09-08 and now carries a number.**
`experiments/29-address-family-census.py` resolved all 965 distinct corpus
hostnames -- 206 of them address literals needing no lookup -- and found **16
tracker URLs on 15 hosts** that offer IPv6 and no IPv4, against **960 URLs on
706 hosts** that offer IPv4. So the IPv6 limitation is **real and it is about
1.2% of the corpus**, and every statement this project makes about it can now
say how small instead of gesturing at an unmeasured population. Result at
`experiments/results/29-address-family-census.unclassified-host.20260908T091721Z.json`.

⚠ **A larger number came out of the same census and it is not this row's.**
**351 URLs on 244 hosts did not resolve at all** -- fifteen times the IPv6
problem. That is `unknown`, never `dead` (RULES 3.1), and it is a bigger
unexplored question than the one this table was worrying about.

⚠ **One resolver on one day.** Two runs minutes apart disagreed by one host,
so the figure moves with DNS and is dated for that reason.

**The residual honesty problem the gate cannot fix.** Every measurement comes
from AS8075 datacenter address space (`C-54`). "Live from GitHub Actions" is not
"live", which is why the vantage labelling is load-bearing rather than
decorative, and why [T-004](../TODO/claims.md) stays open.

---

## The value gate -- **ANSWERED**, 2026-09-08

**Verdict: justified as a labelled dataset, and NOT justified as a list.**

**The question.** Measure the delta between this project's dataset and
redistributing `ngosang/trackerslist`:

* trackers present here and absent there, **that are alive**;
* trackers present there and dead by measurement here;
* disagreements in health, and which side the evidence supports.

If the delta is negligible, say so in the README, prominently, and let the
project be a well-documented mirror with provenance -- or recommend not shipping
it. **Do not manufacture a difference to justify existence.**

### The answer

**Instrument:** `experiments/27-value-gate.py`, offline, in the gate as
`offline-value-gate`. **Evidence:** two GitHub-hosted runs, IPv4 only, **one
observation per tracker** -- the 200-tracker stride sweep `33938543488` of
2026-09-05, and the **census** of all 99 baseline trackers, `34207344996` of
2026-09-08 ([T-034](../TODO/measurement.md)).

| the gate's question | the answer |
| --- | --- |
| present here, absent there, **alive** | **16 of 183** sampled, a floor rate of 8.7% [5.5-13.7], scaling to **107 live [67-169]** of the 1228 we add |
| present there, **dead** here | **0**, and it is zero *by construction*: `MIN_SAMPLES_FOR_DEATH` is 3 and this is one observation each. The weaker claim the evidence supports is `not_live`, **36 of 99** |
| health disagreements | vs `ngosang_all` **63 of 99** agree-live; vs `newtrackon_live` **36 of 46**. Neither side is presumed right: newTrackon **announces** and we scrape (`C-69`), so a disagreement is a methodology difference before it is a finding |

⭐ **The baseline arm is a census, so it carries no sampling error at all**:
**63 live of 99**, counted rather than estimated. The 17-tracker sample it
replaced had put that rate at 52.9% with a 95% interval of 31.0-73.8, and the
census landed at **63.6%** -- inside it. ⚠ **The two arms therefore come from
different days**, which is a real confound and is not waved away: that
consistency check is what licenses the seam, the instrument fails
`--expect-answered` if the census ever falls outside the sample's interval, and
each run is also reported on its own so a reader can decline the combination.

**The control that makes the extrapolation legitimate.** The sweep takes a
*stride* over a sorted corpus, not a random draw, so the arithmetic above
assumes baseline membership is not correlated with `Tracker.sort_key` position.
Measured: 17 baseline members observed against 14.9 expected, two-sided
`p = 0.573`. Consistent with an unbiased sample. ⛔ **Had it not been, every
figure in the table would have been invalid**, and the instrument fails
`--expect-answered` on that condition rather than reporting anyway.

### Two bars, and this project clears one of them

⛔ **The first decision rule drafted for this was wrong in our favour**, and it
is recorded rather than deleted because it is the exact shape the gate exists
to catch: it compared our interval's *lower* bound against the baseline's
*point* estimate -- charging us our sampling error and forgiving theirs. Under
it the verdict read comfortably positive. The rule now compares our worst case
against the baseline's best.

| bar | measured | |
| --- | --- | --- |
| **count** -- our worst case against their best | live yield **2.06x** worst, 2.70x point, 3.68x best | **clears** |
| **density** -- what share of each list answered us | ours **12.8%** of 1327, theirs **63.6%** of 99 | **fails** |

**So the honest reading is two-sided and both halves belong in the README.** A
consumer who takes our plaintext *unfiltered* gets a list 13.4x longer whose
entries are **five times less likely** to answer -- a worse list. A consumer who
takes it *filtered to what we measured live* gets between **2.1x and 3.7x** as
many live trackers as the whole baseline contains.

⚠ **The census moved both bars, and it moved the unflattering one further.**
Measuring the baseline properly raised its live share from 52.9% to **63.6%**,
so the density gap this project has to answer for is **wider** than the sample
suggested, while the count ratio's worst case improved from 1.92x to 2.06x
because the baseline's own upper bound collapsed onto a counted number. Both
directions are the same correction, and neither was chosen.

⭐ **Which makes the verdict conditional, and the condition is the deliverable:
the value is in the labels, not the URLs.** Publishing the plaintext without
the health data would be the prior art -- and the prior art is measured beside
it here. `pkgforge-security/Trackers` publishes 1162 parseable entries, of
which [`corpus-baseline.md`](corpus-baseline.md) measures **one** as content no
primary source already has; **three** of its lines are not URIs at all, and two
of those carry a stranger's private-tracker credential into a file the README
tells consumers to pipe into a client.

⚠ **A second axis of value, which is not a liveness one.** The baseline has
**0** lines this project's parser refuses; the concatenation has **3**. That is
the validation half doing work no liveness figure captures, and it needs no
probe to demonstrate.

### What the answer does not establish

* **Not a liveness rate.** One datacenter, IPv4 only, one day, one observation.
  `live` is a **floor** on both arms: a tracker that timed out is `unknown` and
  some of those are up. The comparison survives this because both arms were
  probed **in the same run by the same code**; the absolute rates do not.
* **Not that the baseline is worse maintained.** The opposite is measured. Its
  entries answered us at 52.9% against our unique additions' 8.7%.
* **Not a settled verdict.** The baseline arm is now a census, but the arm
  that carries the whole "what we add" figure is still **183 trackers of
  1228**, and its interval is what the ratios above inherit.
  `experiments/27-value-gate.py` runs in the gate, so the day a further sweep
  moves the answer, the gate says so instead of this page going quietly stale.

**What is measured of the aggregation half**, by `experiments/19`:

| | |
| --- | --- |
| distinct URLs across 16 source files | **1346** ([`corpus-baseline.md`](corpus-baseline.md)) |
| accepted into the dataset after normalization, dedup and exclusions | **1334** |
| `ngosang/trackerslist` `trackers_all.txt` | **99** |
| unique to `desirefire_all` among primary sources | **995 of 1091** |
| unique to `ngosang_all` among primary sources | **2 of 99** |

**What is not measured, and it is the half that decides the gate.** Whether any
of those unique entries is **alive**. Uniqueness is a string comparison; value
is not. A dataset that is thirteen times larger and mostly dead is worse than a
short accurate one.

**This gate was deliberately not answered rather than answered optimistically**,
and the paragraph above is what it said while [T-020](../TODO/measurement.md)
was being built. [T-027](../TODO/measurement.md) closed it on 2026-09-08 and
the answer is at the top of this section.

**A negative answer was a real possible outcome and would not have been a
failure of the work.** The outcome is partly negative and is recorded as such:
**bar 2 fails**, this project's list is four times less live-dense than the
baseline, and the correct response was the README statement rather than a
search for a different metric that makes the number look better. What the
project may not now do is publish the plaintext alone and call the gate
cleared -- that is the configuration the gate measured and rejected.

---

## The definition of done

The full checklist the project is measured against. **Where it says *test*, it
means an automated test that fails when the property stops holding -- not a
manual observation.**

### Research

- [x] Every reference swept: corpus tracked at captured commits, one verdict each, write-up committed. **Trackers read in both states for six of ten**; the four unfetched are named in [`../references/PROVENANCE.md`](../references/PROVENANCE.md)
- [x] The write-up opens with **what it did not establish**
- [ ] Every claim row that anything depends on is `VERIFIED` or `REFUTED` with an experiment id -- [`TODO/claims.md`](../TODO/claims.md) carries the remainder
- [x] Every experiment is a numbered committed script that re-runs and prints its conditions
- [x] At least one **negative result** committed
- [ ] Every decision closed with its rejected alternatives -- D1, D2, D7, D8, D9, D10, D11 closed; D3, D4, D5, D6 open ([`decisions.md`](decisions.md) derives the counts)

### Correctness

- [x] Test: a malformed, empty, HTML, truncated, or vanished source cannot corrupt canonical data
- [x] Test: "source failed" and "source returned zero trackers" produce different states
- [x] Test: normalization and deduplication are deterministic; every rule has a test proving it preserves identity
- [x] Test: protocol classification covers every scheme found by the census
- [x] Test: an unmeasurable protocol is never reported `dead`
- [x] Test: health states are assigned by measurement rung, and DNS resolution alone never yields `live` -- [T-025](../TODO/measurement.md), done
- [x] Test: the probe is validated against the local fake tracker for **every** failure mode, including a bencoded failure response -- [T-021](../TODO/measurement.md), done
- [ ] Test: each of the six scoring invariants -- [T-043](../TODO/scoring.md)
- [x] Test: running the pipeline twice over identical inputs produces byte-identical output apart from declared metadata
- [x] Test: bootstrap from **no** prior state succeeds; corrupt state fails safely without reinitialising -- [T-040](../TODO/scoring.md), closed. `tests.test_state.BootstrapAndCorruption`: an absent file is the first run, a wrong header or a zero-byte file **raises and leaves the file untouched**, and one damaged line is quarantined while the rest survive. ⛔ Mutation-proved: `except CorruptState: return {}, []` fails 2 tests
- [x] Test: `hardcoded.txt` keeps manual order and self-deduplicates. The renderer is tested; there is no input file for it yet, which is separate work and does not affect this property
- [ ] Test: JSON, CSV and plaintext describe the same accepted dataset -- [T-061](../TODO/publication.md)
- [x] Test: the whole pipeline runs end-to-end with **no network access**

### Honesty

- [x] Every health record carries vantage metadata and the rung reached -- [T-024](../TODO/measurement.md). Asserted over **real** records rather than a shape: workflow run `33938543488` swept 200 trackers from a runner and `scripts/check-vantage-metadata.py --path` passed over all 200. ⚠ The offline gate still reports an expected skip, correctly: a clean checkout has no records where the checker looks, and where they live is the data-branch entry's decision rather than this one's
- [x] The README -- not only a methodology page -- states what the measurements do and do not generalise to
- [x] UDP limitations, if any, are represented as they were measured, not as they were assumed
- [x] No published number lacks its conditions
- [x] The announce policy is implemented and documented, including how an operator requests exclusion. Documented in the README and RULES 4; **enforced by the absence of any announce code path** -- `src/trackers/bep15.py` has no function that builds one, so it is a property of the code rather than a policy somebody has to remember
- [ ] The politeness budget is computed, published, and asserted by a test against the configured schedule -- [T-026](../TODO/measurement.md)
- [x] Every capability in the documentation is classified

### Operations

- [x] Test: a failed generation leaves prior public data intact -- demonstrated by an actual failed run, not asserted
- [ ] Test: overlapping runs cannot race; concurrency controls are in place -- partly; [T-085](../TODO/operations.md)
- [ ] Test: automated issues deduplicate, carry evidence, and close when resolved -- [T-080](../TODO/operations.md)
- [ ] Test: history housekeeping preserves the dataset and never touches `main` -- [T-081](../TODO/operations.md)
- [x] Data-branch history reset is safe **because history lives in files** -- RULES 3.7, and no code infers history from git
- [x] Consumer pin-target guidance is documented
- [ ] `latest` / `daily` / `weekly` semantics defined, implemented, and verified against real platform behaviour -- [T-064](../TODO/publication.md). **The platform half is now measured** ([T-003](../TODO/claims.md), closed): `/releases/latest` ignores both a tag named `latest` and a newer prerelease, an asset can be replaced at a stable URL but **not** read back immediately, and moving a tag does **not** move the release, so delete-and-recreate is the route. What is left is defining and implementing the channels
- [x] Raw GitHub paths work and their caching behaviour is documented
- [x] Least-privilege permissions, timeouts, and pinned action versions on every workflow
- [ ] Security review complete; no upstream content is ever executed or interpolated into a shell command -- structurally held (no shell layer); review is [T-086](../TODO/operations.md)
- [ ] The long-term review answered with mechanisms cited at file and line -- [T-083](../TODO/operations.md)

### Justification

- [x] The measurement gate answered with evidence
- [x] The value gate answered with numbers -- [T-027](../TODO/measurement.md), closed. **Justified as a labelled dataset, not as a list**; the verdict and both bars are above, and `experiments/27-value-gate.py` re-derives it in the gate
- [x] The design brief corrected in place, with corrections visible, before it was retired -- see [`corrections.md`](corrections.md)
- [x] The known-weaknesses record describes the *current* weaknesses and still ends with "assume more remain" -- [`corrections.md`](corrections.md)
