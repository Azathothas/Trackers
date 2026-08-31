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
| **P2 -- measure** | health checking to the ladder, JSON and CSV, vantage metadata, the fake-tracker oracle. D6 decided | probe validated against the local fake tracker for **every** failure mode; **the value gate answered** | not started |
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
| IPv6-only trackers (no IPv6 egress, `C-04`) | - | `unmeasurable` |
| `i2p` network | 13 | `unmeasurable` |
| `yggdrasil` network | >=1, under-counted ([T-023](../TODO/measurement.md)) | `unmeasurable` |
| `wss` (`C-36` unverified, [T-005](../TODO/claims.md)) | 10 | `unmeasurable` |

Every one **MUST** be published as `unmeasurable` and **MUST NOT** be scored or
reported `dead`.

**The residual honesty problem the gate cannot fix.** Every measurement comes
from AS8075 datacenter address space (`C-54`). "Live from GitHub Actions" is not
"live", which is why the vantage labelling is load-bearing rather than
decorative, and why [T-004](../TODO/claims.md) stays open.

---

## The value gate -- **UNANSWERED**

**The question.** Measure the delta between this project's dataset and
redistributing `ngosang/trackerslist`:

* trackers present here and absent there, **that are alive**;
* trackers present there and dead by measurement here;
* disagreements in health, and which side the evidence supports.

If the delta is negligible, say so in the README, prominently, and let the
project be a well-documented mirror with provenance -- or recommend not shipping
it. **Do not manufacture a difference to justify existence.**

**What is measured.** The aggregation half, by `experiments/19`:

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

**This gate is deliberately not answered rather than answered optimistically.**
It closes with [T-027](../TODO/measurement.md), which cannot start until
[T-020](../TODO/measurement.md) exists.

**A negative answer is a real possible outcome and is not a failure of the
work.** If the alive-delta turns out to be small, the correct response is the
README statement and a recommendation, not a search for a different metric that
makes the number look better.

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
- [ ] Test: bootstrap from **no** prior state succeeds; corrupt state fails safely without reinitialising -- [T-040](../TODO/scoring.md)
- [x] Test: `hardcoded.txt` keeps manual order and self-deduplicates. The renderer is tested; there is no input file for it yet, which is separate work and does not affect this property
- [ ] Test: JSON, CSV and plaintext describe the same accepted dataset -- [T-061](../TODO/publication.md)
- [x] Test: the whole pipeline runs end-to-end with **no network access**

### Honesty

- [ ] Every health record carries vantage metadata and the rung reached -- [T-024](../TODO/measurement.md)
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
- [ ] `latest` / `daily` / `weekly` semantics defined, implemented, and verified against real platform behaviour -- [T-064](../TODO/publication.md). **No longer blocked**: RULES 13.1 authorises throwaway releases here, so it waits on [T-003](../TODO/claims.md) being run, not on a human
- [x] Raw GitHub paths work and their caching behaviour is documented
- [x] Least-privilege permissions, timeouts, and pinned action versions on every workflow
- [ ] Security review complete; no upstream content is ever executed or interpolated into a shell command -- structurally held (no shell layer); review is [T-086](../TODO/operations.md)
- [ ] The long-term review answered with mechanisms cited at file and line -- [T-083](../TODO/operations.md)

### Justification

- [x] The measurement gate answered with evidence
- [ ] The value gate answered with numbers -- [T-027](../TODO/measurement.md)
- [x] The design brief corrected in place, with corrections visible, before it was retired -- see [`corrections.md`](corrections.md)
- [x] The known-weaknesses record describes the *current* weaknesses and still ends with "assume more remain" -- [`corrections.md`](corrections.md)
