# Measurement

Health checking to the ladder, the probe and its oracle, vantage metadata, and
the value gate. This is P2, and the measurement gate that authorises it is
answered and passed -- see [`HISTORY/gates.md`](../HISTORY/gates.md).

**The ladder** (each layer separately recorded, because each fails for different
reasons and a consumer troubleshooting a tracker needs to know which broke):

```
DNS resolution
  +- TCP connect  /  UDP datagram sent
       +- TLS handshake (https only)
            +- transport response received      <- HTTP status, or UDP reply
                 +- protocol-valid response     <- bencode parses / BEP 15 fields
                      +- tracker-semantic response
```

The last two rungs are what separate a tracker from a web server, and they are
cheap to reach. `src/trackers/model.py` `Rung` is the enum; nothing assigns a
health state without one.

---

### T-020 The health checker does not exist

Source:      the measurement ladder below; RULES 3.3
Category:    measurement
Priority:    P1
Effort:      L
Status:      done

Problem:     `src/trackers/` aggregates and publishes; it measures nothing.
             `HealthState` and `Rung` are defined and unused.
Premise:     **Measured that it is possible.** `experiments/02` reached
             `protocol_valid` on 9 of 11 UDP targets and `experiments/05`
             reached `tracker_semantic` on 5 of 6 HTTP targets, on two runner
             images, with controls. The probe logic exists in the experiments
             and has not been lifted into the pipeline.
Approach:    `src/trackers/probe.py`. Per transport, walk the ladder and record
             the highest rung reached plus the failure classification. Reuse the
             BEP 15 codec from `experiments/02-udp-bep15-connect.py` and the
             bencode reader from `experiments/05-http-tracker-protocol.py`
             rather than writing either twice; an experiment that is also the
             production code path cannot drift from it.
Decision:    The probe stops at scrape. Announce is not implemented at all, so
             the prohibition in RULES 4 is a property of the code rather than a
             policy somebody has to remember.
Prove:       `python3 -m unittest tests.test_probe -v` passes, and every result
             carries a `Rung`.

**Done.** `python3 -m unittest tests.test_probe -v` -- 28 tests, no network.
`src/trackers/probe.py` walks the ladder for udp/http/https and
every result carries a `Rung`, asserted rather than asserted-of.
The codecs were lifted into `src/trackers/bep15.py` and
`src/trackers/bencode.py` so the experiments and the production
path are the same code and cannot drift.

⛔ **That last sentence is false and is corrected here rather than
edited away (RULES 7).** Found by the door sweep of 2026-09-05:
`experiments/02-udp-bep15-connect.py:89` defines its own
`build_connect_request` and `:94` its own
`parse_connect_response`, alongside `src/trackers/bep15.py:91` and
`:96`, and **no experiment imports from `src/` at all**. The
codecs were **copied**, not lifted, so the two can drift and a fix
to one never reaches the other -- which is the exact defect this
paragraph claimed to have prevented. [T-033](measurement.md) is
the entry that does the work; the rest of this acceptance stands.

⭐ **T-033 closed on 2026-09-08 and the sentence above is true
again.** Both experiments now import the codecs from `src/` and
the copies are deleted, so the claim this acceptance made in
advance is finally the claim the tree supports. It is left written
out in full rather than tidied, because the interesting part is
that it was asserted for eight days before it was true.
Landed with it: T-023 and T-025. **Not** T-022 or T-024 -- see
those entries for what is actually left.

---

### T-021 The probe has no oracle, so a silently broken probe would mark everything dead

Source:      the brief's section 22.3 (the probe needs its own oracle);
             RULES 2 "an absence is not a zero"
Category:    measurement
Priority:    P0
Effort:      M
Status:      done

Problem:     Without a fake tracker the test suite controls, there is no way to
             distinguish "the internet is quiet" from "the probe has been broken
             since Tuesday". A silently broken probe marks the entire dataset
             dead, and the publication volume guard only catches that **if the
             guard was itself tested against this case**.
Premise:     Two seeds already exist and are proven on runners:
             `LoopbackBEP15Tracker` in `experiments/02` (a correct BEP 15
             connect responder) and the positive/negative control servers in
             `experiments/05` (a bencoded `failure reason` responder and a plain
             HTML web server). Both passed on both runner images.
Approach:    `tests/fake_tracker.py`, promoted from those two. It must speak
             correct BEP 15 and correct bencoded HTTP, **and be tellable to**:
             time out, return HTML, return truncated data, return a bencoded
             failure, return a malformed bencode, and close mid-response.
             The probe is then tested against **every** failure mode.
Decision:    The negative control is the load-bearing half and must fail the
             build. A probe that calls an HTML 200 a tracker has reproduced the
             anti-pattern in RULES 11, so it exits non-zero rather than logging.
Prove:       `python3 -m unittest tests.test_probe_oracle -v` covers every
             failure mode listed above, including a bencoded failure response.

**Done.** `python3 -m unittest tests.test_probe_oracle -v` -- 26 tests.
`tests/fake_tracker.py` speaks correct BEP 15 and correct bencoded
HTTP and can be told to: time out, return HTML, return an empty
200, truncate, return either `failure reason` spelling, return
malformed bencode, close mid-response, answer 403, answer 429,
echo a **wrong transaction id**, and return a BEP 15 error.
Verified by mutation rather than by passing: a lenient
discriminator (`any 200 is a tracker`) fails 6 tests, removing the
BEP 15 transaction-id check fails 1, and restoring the seeds'
process-global behaviour flag fails the concurrency test. A test
that does not fail when the claim stops being true is not evidence
(RULES 1), so each load-bearing one was made to fail on purpose.
One bug was fixed on promotion: both seeds selected behaviour with
a **class** attribute, which two live servers silently share.

---

### T-022 UDP scrape needs a synthetic infohash and the ladder does not model that

Source:      `C-50`, found while reading BEP 15
Category:    measurement
Priority:    P2
Effort:      S
Status:      done

Problem:     On HTTP, scrape takes `info_hash` as an optional query parameter.
             On UDP it does not: BEP 15's scrape request carries `info_hash` at
             offset 16 + 20, n as a required field. So **the second rung of the
             ladder is strictly more intrusive on UDP than on HTTP**, and the
             ladder currently treats them as equivalent.
Premise:     **Verified** against BEP 15's own message tables. The connect
             request, by contrast, is three fields ending at offset 16 with
             nowhere to put an infohash -- which is why connect is ethically free
             and scrape is not.
             **Half of this now exists and the half that does not is the
             sending.** `src/trackers/bep15.py` builds a scrape request,
             refuses an empty hash list and refuses any hash that is not
             exactly 20 bytes; `synthetic_infohash()` is the only way to obtain
             one and nothing in the tree can read a real one; the health record
             carries `used_synthetic_infohash`. What is missing is the path in
             `probe_udp`, which today sends **connect only**. A `udp_scrape`
             config flag was written and then deliberately removed rather than
             shipped unwired -- a flag that can be set and ignored is worse than
             its absence, and `test_the_udp_path_sends_connect_and_nothing_else`
             now asserts against the actual datagram instead of a flag.
Approach:    Where a UDP scrape is performed it uses a **synthetic random**
             20-byte infohash, generated per run, corresponding to no content,
             and that fact is recorded in the health record. RULES 4 permits
             exactly this and requires it to be documented.
Decision:    Prefer connect and only scrape on UDP where connect is shown
             insufficient for a decision the project actually needs. Connect
             already yields liveness and RTT, so the bar for scraping is high.
Prove:       `python3 -m unittest tests.test_probe.ProbeConfiguration -v`
             passes with a new case asserting the datagram `probe_udp` actually
             transmits carries a `synthetic_infohash()` value and never one
             read from anywhere else, and that the emitted record names which
             was used. Assert against the **bytes sent**, not a config flag: a
             flag can be set and ignored, and the datagram is what the tracker
             sees.

**Done.** Not by wiring the scrape. `python3 -m unittest
tests.test_probe.ProbeConfiguration` -> `Ran 6 tests`, `OK`.

⭐ **The entry's own `Decision` set the bar and the bar is not met.** It says
scrape on UDP "only where connect is shown insufficient for a decision the
project actually needs". Re-derived against the code rather than assumed:

```python
_PROVING_RUNG[Transport.UDP] is Rung.PROTOCOL_VALID
```

A BEP 15 connect **already reaches the rung that proves a tracker** on UDP,
because nothing but a tracker answers the magic constant with our transaction
id echoed back. A scrape with a synthetic infohash returns zeros for content
that does not exist, so it adds no liveness, no latency and no swarm
information -- it costs an operator a second round trip and a required
`info_hash` to tell us what we already know. RULES 4's "prefer connect >
scrape > announce, always" then decides it, and RULES 9.1 forbids implementing
it anyway to satisfy a checkbox.

⛔ **So the `Prove` clause above could not be satisfied honestly**, because the
datagram it asks about is one this project should not send. RULES 9's three
parts are on **D15**, and what replaces it is a test that keeps the decision
from being reversed by accident: `test_no_code_path_in_src_can_send_a_udp_
scrape` parses every module under `src/trackers/` with `ast` and fails if
anything calls `build_scrape_request`. **Mutation-proved**: adding one such
call to `probe_udp` fails it.

**What the entry was actually about is already modelled.** The Problem is that
"the ladder treats UDP and HTTP scrape as equivalent" -- and it does not:
`model.SCRAPE_REQUIRES_INFOHASH` names UDP as the transport whose scrape needs
one, and `_PROVING_RUNG` gives UDP a lower proving rung precisely because its
cheap rung is already conclusive.

**The capability is kept and left unreachable.** `build_scrape_request` still
refuses an empty hash list and any hash that is not exactly 20 bytes, and
`synthetic_infohash()` is still the only way to obtain one --
`test_a_synthetic_infohash_is_the_only_one_obtainable` asserts it is random per
call. Deleting the builder would hide the asymmetry `C-50` records; wiring it
would spend somebody else's bandwidth for nothing.

---

### T-023 Yggdrasil trackers addressed by hostname are silently misclassified as clearnet

Source:      `C-37`; limitation printed by `experiments/19`
Category:    measurement
Priority:    P2
Effort:      M
Status:      done

Problem:     `classify_network` reads the URL only. ngosang's single yggdrasil
             entry is `http://yggtracker.i2p.rocks:80/announce` -- an ordinary
             hostname -- so it is classified `clearnet`, routed to the clearnet
             prober, and will be recorded **dead**. That is the exact
             correctness bug RULES 3.1 forbids, surviving inside the fix for it.
Premise:     **Measured.** Only the `_ip` variant of that list exposes the
             `0200::/7` literals that identify the network. `experiments/19`
             prints this limitation about itself rather than hiding it.
Approach:    Resolve the hostname during health checking and classify on the
             **resolved address**, recording the address and the timestamp as
             evidence. This cannot live in the census: it is a time-varying
             inference, not a property of the URL. **Resolution needs no
             yggdrasil connectivity** -- a DNS answer is a DNS answer -- so this
             is solvable from this vantage today, and reaching the tracker
             afterwards is T-031's problem, not this entry's.
Decision:    A resolution-derived network classification is recorded with its
             observation time and never cached as though permanent, for the same
             reason dedup refuses to collapse hosts on a shared address.
Prove:       A test that a host resolving into `0200::/7` is classified
             `yggdrasil` and reported `unmeasurable`, with the resolved address
             and timestamp present in the record.

**Done.** `python3 -m unittest tests.test_probe.YggdrasilByResolvedAddress -v`.
`classify_network_resolved` in `src/trackers/probe.py` refines the
URL-derived network from the addresses DNS actually returned; a
host resolving into `0200::/7` becomes `yggdrasil` and is reported
`unmeasurable`, never `dead`. The resolved address and the
observation time travel in the record (`resolved_ip`,
`observed_at`), and the **disagreement itself** is recorded in
`network_reclassified_from` rather than being silently resolved in
favour of one side. An explicit `.i2p`/`.onion` suffix beats a
resolved address, because those names do not resolve in the
ordinary DNS at all. Confirmed to need only a DNS answer: no
yggdrasil connectivity was required to close this.

---

### T-024 No health record carries vantage metadata, because no health record exists

Source:      RULES 3.4; decision D2 requirement 1
Category:    measurement
Priority:    P1
Effort:      S
Status:      done

Problem:     RULES 3.4 requires every health record to carry environment class,
             region where determinable, IP-family availability and probe
             version. `scripts/check-vantage-metadata.py` exists and currently
             exits 2, "could not run", because there is nothing to check.
Premise:     The gate is written and deliberately refuses to pass vacuously.
             **The record shape now exists and is tested; nothing writes one to
             disk yet, which is the whole of what is left.**
             `src/trackers/vantage.py` collects the required fields and
             `ProbeResult.as_record()` emits exactly the keys the gate reads --
             `tests.test_probe.EveryRecordCarriesItsEvidence` asserts that.
             Three distinctions were kept apart there that are commonly
             conflated: `ipv6_stack_present` (can we make a socket),
             `ipv6_route_present` (is there a route -- determined by a UDP
             `connect()` routing-table lookup that sends **zero packets**), and
             `ipv6_egress` (do packets actually make it), which only a real
             round trip answers and which therefore renders as a dash unless a
             caller passes the measured value in. Where egress is measured
             false, the measurement beats the routing table and `ipv6` is
             withheld from the usable families -- `C-04` is precisely the case
             where a runner has both a stack and a route and still cannot get a
             packet out.
             `probe_version` is a hand-bumped string **and** `probe_code_sha256`
             is a hash over the modules that define a measurement, because a
             human can forget to bump a version and cannot forget a hash.
Approach:    Emit the conditions block into every health record. The vantage for
             this project is `C-54`: AS8075 Microsoft datacenter space, the
             runner image, and `ipv6_egress: false`.
             **Remaining:** a runner that probes the corpus and writes records
             under `out/` or `data/`, at which point the gate flips 2 -> 0. It
             needs T-029's concurrency bound first; probing 1337 trackers
             serially is over an hour.
Prove:       `python3 scripts/check-vantage-metadata.py` exits **0** rather
             than 2, over real records.

**Done.** Workflow run **`33938543488`**, `ubuntu-24.04`, 2026-09-05.
`python3 scripts/check-vantage-metadata.py --path sweep-out` printed:

```
checked 200 health records

OK  every record carries vantage metadata and a measurement rung; nothing
    unmeasurable is reported live, dead or degraded.
```

**The first time this project measured a real tracker.** 200 of 1327 (`ci`
probes a sample, RULES 15.2), from `github-actions-hosted`, IPv4 only, 900 s
deadline not reached. Records committed at
`experiments/results/health-sweep.github-actions-hosted.run33938543488.json`,
because a workflow artefact expires after 90 days and git does not. States:
`live` 25, `degraded` 1, `unknown` 162, `unmeasurable` 12.

**`dead` is 0 and cannot be otherwise from one sweep.**
`MIN_SAMPLES_FOR_DEATH` is 3, so a single observation of a tracker that did not
answer is `unknown` -- too few samples, not gone. Accumulating across runs is
[T-040](scoring.md), now closed. **No number here is a liveness rate:** 25 of
200 is one datacenter, IPv4, one day.

**The refusals are the finding, recorded as `C-72`.** Eight endpoints across
seven hosts refused by a published BEP 34 record, three more skipped because
public resolvers would not answer. One host spells its denial as a bare
`BITTORRENT`, the normative form a naive implementation misses.

**The offline gate keeps its expected skip, correctly.**
`scripts/check-vantage-metadata.py` finds no records in a clean checkout
because none are committed where it looks; where health records live is
[T-063](publication.md)'s decision. The flag comes off when the data branch
does.

**One number in the record this entry produced is wrong, and the correction is
here rather than in a silent edit (RULES 7).** The committed `health.json`
reports `counts.corpus: 200` against a corpus of **1327**:
`scripts/probe-corpus.py` selected the sample and passed the *sample* to
`sweep()`, which records `corpus=len(trackers)`, so the pair of fields whose
purpose is to say "200 of 1327" were the same number. Found by
[T-027](measurement.md).

**The sample itself was never affected**, which is why nothing caught it:
`select()` returns a list whole when the sample size is not smaller than it, so
applying it twice changed nothing and every health state stands. Only the
denominator was wrong, which is the worse half -- a wrong state gets argued
with, a wrong denominator gets divided by.

**The record is not rewritten.** It says what the instrument said, which is
what makes it evidence. `scripts/probe-corpus.py` now hands `sweep()` the
corpus, `tests.test_concurrency.TheSweepScriptReportsTheCorpusItSampledFrom` is
the regression test and was mutation-proved, and
`experiments/27-value-gate.py` derives the corpus size itself.

---

### T-025 The health state machine and failure classification are undefined

Source:      RULES 3.3
Category:    measurement
Priority:    P1
Effort:      M
Status:      done

Problem:     The six states exist as an enum. Nothing decides which one a
             measurement produces, and the distinctions are the whole point:
             `unknown` (never checked / too few samples) and `error` (the probe
             itself failed) must never collapse into `dead`.
Premise:     The rungs are recorded; the mapping from rung to state is not
             written.
Approach:    An explicit table from (rung reached, transport, network, sample
             count) to state, plus a failure classification (`dns_failure`,
             `no_usable_address`, `timeout`, `refused`, `tls_failure`,
             `not_a_tracker`, `unsupported`). `no_usable_address` already exists
             in `experiments/02` and is the model: a name that resolves only to
             IPv6 is not a DNS failure and is not death.
Prove:       A test that DNS resolution alone never yields `live`, and that an
             `unmeasurable` transport or network never yields `dead`,
             `live` or `degraded`.

**Done.** `python3 -m unittest tests.test_probe.StateTable -v`. `health_state`
in `src/trackers/probe.py` is the only place a `HealthState` is
produced: an ordered table whose order is the specification.
Tested **exhaustively over the enums**, not by example -- DNS alone
is never `live` across every transport x network x sample count,
and `measurable=False` yields `unmeasurable` across every
combination including ones where the caller claims success.
The failure vocabulary splits facts about the tracker from facts
about us (`ABOUT_US`), and no member of the second set can produce
`dead`. Two conflations were found and fixed while writing it:
a 403 is `unknown` (it may be about our identity, T-012) while a
429 is `degraded` (it answered); and a truncated body became its
own `TRUNCATED_RESPONSE` rather than sharing `NOT_A_TRACKER` with
a web server, because a cut-off answer is evidence something *was*
answering and publishing it as death would be a network fault
reported as an outage.

---

### T-026 The politeness budget is neither computed nor published nor asserted

Source:      RULES 4; decision D7
Category:    measurement
Priority:    P1
Effort:      S
Status:      open

Problem:     RULES 4 states a ceiling. Nothing computes
             `trackers x probes-per-tracker x runs-per-day`, nothing publishes
             the resulting per-tracker rate, and no test asserts it against the
             configured schedule. A ceiling nobody measures is a preference.
Premise:     **The anchor is measured and it is not the brief's.** The brief's
             "clients re-announce every ~30 minutes" was unsourced. Two agreeing
             sources replace it: newTrackon's `tracker.py:163` floors its
             recheck interval at **10800 s**, and its maintainer states in issue
             #334 that "the current checking frequency (every ~3 hours) is
             reasonable for the server load". That monitor **announces**; this
             project only scrapes, so it does strictly less work per check.
Approach:    Compute the budget from the real corpus size
             (`HISTORY/corpus-baseline.md`), publish it in the run report, and
             assert it in a test that reads the configured schedule.

             **Read the floor, not only the interval (`C-65`).** A tracker may
             state `min interval` as well as `interval`, and `min interval` is
             the stricter number -- the one an operator would judge us by. BEP 3
             spells it with a space and the underscore form occurs in the wild;
             a production client reads both
             (`references/Azathothas__bit-cli/tree/crates/bit-cli-core/src/tracker.rs:739`).
             `src/trackers/bencode.py` now returns `min_interval` from
             `classify_body` alongside `interval`; **the scheduler must prefer
             `max(min_interval, interval)` and this entry is what asserts it.**

             **DNS is inside the budget and its ceiling is 100,000 lookups
             per run on a GitHub runner.** Operator ruling 2026-09-08, in
             response to the tracker-operator review measuring this session at
             roughly 4,000 -- `experiments/29` resolves 965 hostnames and
             `experiments/30` asks a second resolver about each one the first
             could not answer for. Local runs are not bounded by this.

             ⚠ 4,000 is 4% of the ceiling, so nothing built so far is near it.
             The number exists so a future census cannot grow into a load
             nobody counted: publish the DNS figure beside the probe figure.
Decision:    **D7 -- CLOSED by operator ruling 2026-08-29. Publish hourly; probe
             each tracker on its own stated `interval`, defaulting to 3 h.**
             Hourly *generation* touches no tracker and was never in question.
             Hourly *probing of every tracker* is 3x the load of the closest
             production analogue and nothing measured justifies it.
             Rejected: hourly probing of every tracker (unjustified by any
             measurement, and 3x a monitor that does strictly more work per
             check); 30-minute probing (6x); a fixed global ~3 h interval
             (simpler, but ignores what each tracker actually asks for, and the
             tracker is the only authority on the load it wants).
Prove:       A test that fails when the configured schedule would exceed one
             probe per tracker per its stated interval.

---

### T-027 The value gate is unanswered: uniqueness is measured, liveness is not

Source:      HISTORY/gates.md -- a gate, not an aspiration
Category:    measurement
Priority:    P0
Effort:      M
Status:      done

Problem:     The project is not justified unless the dataset adds measurable
             value over redistributing `ngosang/trackerslist`. **Half the
             measurement exists and it is the easy half.**
Premise:     **Measured:** the aggregate holds 1337 accepted trackers against
             `trackers_all.txt`'s 99, and `desirefire_all` alone contributes 995
             URLs unique among primary sources. **Not measured:** whether any of
             those unique entries is *alive*. Uniqueness is a string comparison;
             value is not.
Approach:    Once T-020 lands, answer all three questions the gate asks with
             numbers and sample counts: trackers present here and absent there
             **that are alive**; trackers present there and dead by measurement
             here; and health disagreements, with which side the evidence
             supports.
Decision:    If the delta is negligible, **say so in the README, prominently**,
             and let the project be a well-documented mirror with provenance --
             or recommend not shipping it. Reaching that conclusion honestly is
             a successful outcome. **Do not manufacture a difference to justify
             existence.**
Prove:       `python3 experiments/27-value-gate.py --expect-answered`
             exits 0, having computed the three deltas against
             `ngosang/trackerslist` from committed health records; then
             `HISTORY/gates.md` carries the answer with its conditions and
             sample counts, and the README states the verdict -- including if
             the verdict is that this project is not justified.

**Done.** `python3 experiments/27-value-gate.py --expect-answered` -> exit 0,
result at `experiments/results/27-value-gate.unclassified-host.20260908T090220Z.json`. It is in the gate as
`offline-value-gate`, so the answer is re-derived from the committed records on
every push rather than transcribed once.

**Verdict: justified as a labelled dataset, NOT justified as a list.** The
count bar clears and the density bar fails.
[`../HISTORY/gates.md`](../HISTORY/gates.md) owns every figure and both bars;
the README carries the two-sided verdict with the unflattering half first, as
this entry's `Decision` requires. The numbers are not restated here -- an
earlier revision of this paragraph did restate them, and they were superseded
by [T-034](measurement.md)'s census within the day (RULES 17.2).

**The first decision rule this entry used was wrong in our favour and is kept
visible rather than edited away** (RULES 7): it compared our interval's lower
bound against the baseline's *point* estimate, charging us our sampling error
and forgiving theirs. `DECISION_RULE` now compares our worst case against their
best, and the constant carries why.

**Two defects came out of building it, both fixed.**

1. **The committed sweep record's `counts.corpus` said 200 against a corpus of
   1327.** `scripts/probe-corpus.py` selected the sample and handed the
   *sample* to `sweep()`, which records `corpus=len(trackers)`. The sample was
   never wrong -- `select` is idempotent at that boundary -- so it was invisible
   in every health state; only the denominator was affected, which is the worse
   half. Regression-tested by
   `tests.test_concurrency.TheSweepScriptReportsTheCorpusItSampledFrom` and
   mutation-proved. The record is **not** rewritten; the correction is under
   [T-024](measurement.md)'s title, and experiment 27 derives the denominator
   itself.
2. **`PRIVATE_CREDENTIAL` could not see `authkey=`.** Widening it -- measured
   first at **zero change to every published count**, because those URLs are
   already refused by the character check -- found the same stranger's
   credential in **four places outside the capture directories**: a review, a
   test, a source comment and an experiment result. All redacted;
   [`../docs/security/secrets.md`](../docs/security/secrets.md) carries the
   class.

**What it did not settle at the time:** the baseline arm was 17 trackers.
[T-034](measurement.md) made it a census of 99 the same day, and the verdict
survived.

---

### T-028 newTrackon is available as an oracle and is not being used as one

Source:      `C-26` refuted; decision D2
Category:    measurement
Priority:    P2
Effort:      M
Status:      done

Problem:     The single most valuable thing this dataset could publish is
             **disagreement between independent observers**, and the oracle that
             makes it possible is measured, working, and unused.
Premise:     **Verified.** `/api/<int:percentage>` is an uptime filter, measured
             monotone 261 -> 82 -> 55 -> 15 by
             `experiments/20-newtrackon-api-surface.py`. `/api/stable` is
             `api_percentage(95, added_before=10 days)`.
Approach:    Fetch the oracle alongside our own measurements and publish a
             per-tracker comparison: agree-live, agree-dead, we-say-live, they
             say-dead, and the reverse.
Decision:    **The caveat travels with every comparison or the comparison is
             misleading.** newTrackon derives uptime by *announcing*
             (`scraper.py:232`, `:279`, `thash=urandom(20)`); this project stops
             at scrape. "Uptime" there and "live" here answer different
             questions, and a disagreement is a methodology difference first and
             a finding second. Also: it reports **one preferred protocol per
             tracker** (issue #324), so `/api/udp` is not "supports UDP" and must
             not be compared to a per-endpoint measurement.
Prove:       `python3 experiments/28-newtrackon-crosscheck.py` exits
             0 and emits a report whose header states the methodology
             difference -- **newTrackon announces and we scrape**, so its
             "uptime" and our "live" answer different questions (`C-69`) -- and
             whose every rate carries a sample count.

**Done.** `python3 experiments/28-newtrackon-crosscheck.py --expect-crosscheck`
-> exit 0, result at `experiments/results/28-newtrackon-crosscheck.unclassified-host.20260908T090220Z.json`, in the gate as
`offline-oracle-crosscheck`. The methodology sentence is a module constant
carried into every emitted block rather than a header line somebody can drop.

**The cross-check, run `33938543488` against the committed snapshots:** agree
live 11, agree not-live 26, we-live-they-not **0**, they-live-we-not 4 --
**90.2% of 41**.

**The zero did not survive a bigger sample (RULES 7).** The baseline census of
[T-034](measurement.md) added 52 more trackers assessed by both, where the cell
is **3**; across both runs it is **3 of 93**. The hedge above is what held --
41 trackers on one day was not proof of anything. The surviving claim carries a
confound: the committed newTrackon snapshot is 2026-08-31 and the census is
2026-09-08, so a tracker that came up in between reads as our false positive
rather than their staleness.

**The `vs stable` row is reported and labelled NOT an agreement rate.** Our
`live` is current responsiveness; their `stable` is >=95% uptime over a window
with a 10-day age floor.

**Two facts about the oracle, measured rather than assumed.** `stable` is
**not** a subset of `live`, so the three sets are not a ladder and code
treating them as one would drop a tracker. And `/api/all` covers **260 of our
1327**, so the denominator excludes what newTrackon has never heard of: an
absence is not a zero (RULES 2).

**What it gives [T-031](measurement.md):** route (c) returns **4** trackers
recorded `unknown` that an observer elsewhere lists live, one
`blocked_by_policy`, each with its source and a provenance string marking it
second-hand. **And a negative result that is kept: 0** of our `unmeasurable`
trackers got a signal -- newTrackon does not cover the i2p, yggdrasil or `wss`
entries, which are the four categories T-031 was written for. Coverage is the
constraint, not correctness.

---

### T-029 Probing has no concurrency control, timeout budget or cancellation behaviour

Source:      RULES 5.2; RULES 5.2
Category:    measurement
Priority:    P1
Effort:      M
Status:      done

Problem:     The whole corpus probed serially at a 5 s timeout is over an hour in
             the worst case, and probed in parallel without a bound is an
             unbounded burst at somebody else's server and at the runner.
             Neither is acceptable and neither is currently prevented.
Premise:     Measured RTTs are 109-127 ms median, so the workload is latency-
             bound and benefits from fan-out. The job timeout is a hard ceiling.
Approach:    `asyncio` with a bounded semaphore, a per-host serialisation so one
             host never sees concurrent probes, a global deadline, and defined
             cancellation that records `unknown` rather than `dead` for anything
             not reached before the deadline.

             **The UDP retry budget has a shape, and BEP 15's own is unusable.**
             BEP 15 specifies retrying at `15 * 2^n` seconds for `n` in 0..8 --
             nine attempts, up to 62 minutes for one tracker. A production
             client refuses it for exactly the reason that applies here: a
             diagnostic that takes an hour to say "this tracker is down" has
             not answered the question
             (`references/Azathothas__bit-cli/tree/docs/trackers.md`). It does
             three attempts inside one timeout, one attempt being
             `max(timeout / 3, 1s)`.

             **The arithmetic to copy, not the seconds.** A UDP exchange is
             *two* round trips -- connect, then scrape -- and either can be the
             one that dies, so the worst case is **five** attempts, not three:
             a connect answered on its third attempt leaves three more for the
             scrape. The per-tracker budget is therefore
             `5 x max(timeout / 3, floor)`, and the floor matters more than the
             nominal timeout at small values. Their seconds were measured on
             their hardware against their own loopback tracker and are **not**
             adopted.
Decision:    **A cancelled probe is `unknown`, never `dead`.** Running out of
             time is a fact about us.

             **Also unmeasured: whether probing changes the answer.** RULES 2
             requires checking whether observing perturbed the subject. A
             tracker that rate-limits after the first request answers the second
             differently, so a per-host serialisation is not only politeness --
             it is what keeps the second measurement meaningful. The oracle in
             `tests/fake_tracker.py` can be told to do this and currently is
             not.
Prove:       `python3 -m unittest tests.test_concurrency -v` (planned) passes
             three cases: a deadline expiry produces `unknown` and never `dead`
             for unprobed trackers; no two concurrent probes target one host;
             and the computed per-tracker UDP budget equals
             `5 x max(timeout / 3, floor)`.

**Done.** `python3 -m unittest tests.test_concurrency` -> `Ran 21 tests`, `OK`,
no network. ⚠ It was 20 at acceptance; the twenty-first is the regression
for the defect review 1 found in this same module, and the number was stale
in this paragraph until review 3's claim audit re-ran it. `src/trackers/sweep.py` is the runner and `scripts/probe-corpus.py`
is the instrument that drives it.

All three `Prove` cases hold, and the two that could fail silently were
**mutation-proved** rather than merely passed:

* replacing the per-host lock with a fresh lock per call fails
  `PerHostSerialisation`;
* removing **both** deadline checks fails `Deadline`.

⭐ **Removing only the outer deadline check failed nothing, and that is a
finding rather than a gap.** The check before the host lock is a *drain*, not a
correctness guard: it stops a queue of threads waiting on one slow host from
each taking its turn only to discover the deadline has passed. The check
*inside* the lock is the one that decides correctness, and the mutation proved
which is which.

⚠ **`asyncio` was rejected**, which the `Approach` above proposed. It would
require an async rewrite of `probe_udp` and `probe_http`, so the project would
carry **two implementations of the probe** and a fix to one would never reach
the other -- the copy-pasted-logic row in
[`../docs/conventions/forbidden-patterns.md`](../docs/conventions/forbidden-patterns.md).
The workload is latency-bound IO where a bounded thread pool and a bounded
event loop are the same shape, so a `ThreadPoolExecutor` runs the **production
probe path unmodified**. Rejected with it: an unbounded pool (the burst this
entry exists to prevent), and a per-host queue rather than a lock (same
guarantee, more machinery).

**Selection is a stride, not a head, and that was a defect avoided rather than
found.** `Tracker.sort_key` leads with the transport, so `ci`'s 200-tracker
sample taken from the front of a sorted corpus would have been entirely `http`
and a wholly broken UDP path would never have appeared in a CI run. The stride
keeps every transport, and `test_a_sample_keeps_every_transport` fails if it
stops doing so.

⭐ **The sweep fills in the vantage rather than trusting the probe to.** RULES
3.4 is the sweep's promise because the sweep is what emits the record, and a
record missing its vantage fails silently: the consumer reads `dead` and cannot
tell it means `dead from one datacenter, over IPv4`.

---

### T-030 Experiments 3-18 from the original programme were never run

Source:      the brief's section 23, items 3-18 (the experiment programme)
Category:    measurement
Priority:    P2
Effort:      L
Status:      open

Problem:     The experiment programme names 20 experiments. Numbers 1, 2, 19 and
             20-as-API-surface are done; **3 through 18 are not**, and several
             feed decisions that are currently being made without them.
Premise:     Recorded so they are not rediscovered as new ideas. The numbering
             here is the original programme's, not the `experiments/` filename
             numbering, which is independent and never reused.
Approach:    Each becomes its own numbered script when its decision comes due:
             alternative measurement architectures (3); safe check frequency (4
-- feeds T-026); safe concurrency (5 -- feeds T-029); tracker
             behaviour under repeated checks (6); source overlap (7 -- partly
             done by `experiments/19`); source freshness (8); source reliability
             (9); anime-source uniqueness (10 -- uniqueness done, liveness is
             T-027); cache behaviour (11); browser-like User-Agent behaviour
             (12); 401/403 fallbacks (13 -- currently unnecessary, `C-43`);
             ranking approaches against the invariants (14); data-branch history
             growth (15); release and tag behaviour (16 -- T-003); raw GitHub
             consumption (17 -- done, `experiments/21`); external-service single
             points of failure (18).
Prove:       Each closes with its own numbered committed script; this entry
             closes when none remain unaddressed or deliberately refused with a
             reason.

---

### T-033 The experiments and the probe carry two copies of one codec

Source:      review 2 of 2026-09-05, the door sweep; corrects [T-020](measurement.md)
Category:    measurement
Priority:    P2
Effort:      M
Status:      done

Problem:     `experiments/02-udp-bep15-connect.py` and
             `experiments/05-http-tracker-protocol.py` each carry their own
             BEP 15 and bencode implementations, and `src/trackers/bep15.py`
             and `src/trackers/bencode.py` carry another. **Two
             implementations of one wire format is the copy-pasted-logic row
             in [`../docs/conventions/forbidden-patterns.md`](../docs/conventions/forbidden-patterns.md)**:
             each acquires its own defects and a fix to one never reaches the
             other.

             It also makes an instrument and the production path answer
             differently about the same bytes, which is worse here than in
             ordinary code: the experiments are what this project offers as
             evidence that the probe is correct.
Premise:     **Measured, not suspected.** `grep -n "def build_connect_request"`
             finds it at `experiments/02-udp-bep15-connect.py:89` and at
             `src/trackers/bep15.py:91`; `grep -n "sys.path"` over
             `experiments/` shows every script adds only its own directory.
             T-020's acceptance claims the opposite and the claim is corrected
             under its title.
Approach:    Put `src/` on the experiments' path -- `experiments/_consent.py`
             already does exactly that and is the precedent -- and delete the
             copies. ⚠ **Do not rewrite the committed results.** They were
             taken by the copied code, and a result whose instrument changed
             underneath it means something different on a re-run; the
             conditions block records the commit, which is what makes that
             checkable.
Decision:    Not done in the same pass that found it. Swapping a codec
             underneath instruments whose output is this project's evidence is
             a change that deserves its own gate run and its own reading, not a
             tail-end edit to a review commit.
Prove:       `grep -rn "def build_connect_request\|def parse_connect_response\|def bdecode" experiments/`
             returns nothing, every experiment imports from `src/trackers/`,
             and `python3 experiments/02-udp-bep15-connect.py --expect-control`
             still exits 0 against the loopback control.

**Done.** `grep -rn "def build_connect_request|def parse_connect_response|def
bdecode" experiments/` -> exit 1, no matches. `scripts/check-gate.py` -> 16
passed, 1 expected skip. `experiments/02` imports the BEP 15 codec from
`src/trackers/bep15.py`; `experiments/05` imports `bdecode`, `classify_body`,
`BencodeError` and `FAILURE_KEYS` from `src/trackers/bencode.py`. The copies
are deleted.

**Equivalence was measured before the copies were deleted**, as this entry's
`Decision` required. 5025 inputs, 5000 of them random byte strings: the BEP 15
pair differed on **0**; `bdecode` on **0** accept/reject decisions and **0**
parsed values; `classify_body` on **0** `kind` values. The differences were in
error text only. `experiments/19 --offline` then reproduced the committed
2026-08-31 run's counts identically, so no published number moved. The
committed results are not re-run and not rewritten; their conditions block
records the commit they were taken at.

**A third copy existed and this entry did not name it.**
`experiments/19-scheme-census.py` duplicated `src/trackers/model.py`'s
`classify_network` line for line, under a comment claiming to be the single
home of `YGGDRASIL_NET` while `model.py` held the same constant. It now imports
the production one. That classifier decides `.i2p` against clearnet, which is
failure mode 1 in [`../docs/AGENTS.md`](../docs/AGENTS.md) section 5.
`parse_entries` beside it is **not** the same defect and stays: a census
accepts what the pipeline rejects, by design.

**Two corrections to the `Prove` clause (RULES 9).**

1. It asked that *every* experiment import from `src/trackers/`. The bar that
   matters is **no experiment carries a second implementation of something
   `src/` owns**; `01`, `03`, `04` and `20`-`24` have nothing to share.
2. `02 --expect-control` could not be run here: it probes eleven real trackers
   on the way to reporting the control, and this host is a residential
   `unclassified-host` -- the route [T-024](measurement.md) refused. Three
   routes were considered and two taken. The control is proved offline by
   `tests.test_probe_oracle.TheExperimentsControlStillAnswersOnTheProductionCodec`,
   which runs the experiment's own responder and asserts both functions resolve
   to `trackers.bep15`; mutation-proved by restoring a shadowing copy. The real
   run happened on a runner: push `29c11ef`, both images green, control passed
   with 2 datagrams received on each of its two runs.

**Experiment 05 proved 3 of 6 against the baseline's 4, and the codec is not
why.** `tracker.leechshield.link` failed at `rung=dns`, `kind=no_response`: no
body reached the decoder, so the decoder cannot have changed its verdict. The
recorded rung is what makes that a minute of work to rule out.

**T-020's acceptance is true as of this entry.**

---

### T-031 Liveness for networks this vantage cannot reach -- the leverage entry

Source:      operator ruling 2026-08-29; RULES 10.1a
Category:    measurement
Priority:    P1
Effort:      L
Status:      open

Problem:     Four categories of tracker are currently labelled `unmeasurable`
             and left there: IPv6-only (no IPv6 egress), `i2p` (14), yggdrasil
             (>=1), and `ws`/`wss` (13). **`unmeasurable` is the honest label on
             the data; it is not a reason to stop trying**, and treating it as
             one is how a whole class of tracker silently stops being
             researched.
Premise:     **The constraint is on one route, not on the question.** No IPv6
             egress from a GitHub runner is a physical fact. "Is this tracker
             alive" is not answerable *only* by opening an IPv6 socket from
             here, and every alternative below avoids needing one.
Approach:    Build one indirect-liveness mechanism that serves all four
             categories, rather than four special cases. Candidate routes, to be
             evaluated and each recorded with its trade:
             (a) **NAT64 / DNS64** -- a public gateway makes an IPv6-only host
             reachable over IPv4 from here. Cheapest by far if it works; verify
             the gateway is not itself the thing being measured.
             (b) **A relay or proxy with IPv6 egress**, including the
             operator-approved read proxies for HTTP-shaped probes.
             (c) **Oracle correlation** -- newTrackon already publishes uptime
             (`C-26`) and observes from a vantage with different reachability.
             A tracker it reports up is evidence, recorded **second-hand** with
             its source, its date and its methodology caveat (it announces; we
             scrape).
             (d) **Public i2p / yggdrasil gateways**, where they exist and are
             honest about what they proxy.
             (e) **The dual-stack shortcut** -- check whether an apparently
             IPv6-only host actually resolves dual-stack, or has an IPv4
             sibling. Measure this first: it may dissolve part of the problem
             for free.
             (f) **`wss`** needs a WebSocket handshake, which is ordinary TCP
             and TLS -- reachable from here today. It is in this entry only
             because it shares the "labelled unmeasurable and left alone"
             failure, not because it needs indirection. See T-005.
Decision:    **Second-hand evidence is recorded as second-hand and never
             promoted to a direct measurement.** A distinct rung and a distinct
             provenance field: who observed it, when, by what method. That keeps
             the honesty rule intact while removing the excuse -- the data says
             exactly what it is, and the category stops being a dead end.
             Where a route is rejected, the entry records **why**, so the next
             session does not re-derive it (RULES 10.1a: name three routes).
Prove:       At least one of the four categories moves from `unmeasurable` to a
             recorded liveness signal with its provenance, an instrument that
             re-runs it, and a test that the signal is never reported as a
             direct probe result.

**Route (c) is built and measured, and it did not do what this entry hoped.**
[T-028](measurement.md) closed 2026-09-08:
`experiments/28-newtrackon-crosscheck.py` emits second-hand liveness with its
source, its snapshot and a provenance string, for **4** trackers -- and all
four are `unknown`, **none** is `unmeasurable`. newTrackon's list does not
cover the i2p, yggdrasil or `wss` entries in the sample, so the observer that
was supposed to see further cannot see into the networks that need it.

⭐ **What it did unlock is the fifth case this entry names in passing:** one of
the four is `blocked_by_policy`, which is the blocked-vantage case, and one is
`refused`. So the mechanism works and its **coverage** is the constraint, not
its correctness.

⚠ **So the `Prove` clause above is NOT met**, and this entry stays open rather
than being closed on a partial. The remaining routes are (a) NAT64/DNS64, (b) a
relay with IPv6 egress, and (d) public i2p and yggdrasil gateways.

**Route (e) is measured, and it did dissolve part of the problem.**
`experiments/29-address-family-census.py`, 2026-09-08, resolved all 965
distinct corpus hostnames (206 were address literals and needed no lookup) and
contacted **no tracker** -- it resolves names and stops.

| | hosts | tracker URLs |
| --- | --- | --- |
| IPv4-capable | 706 | 960 |
| **IPv6-only** | **15** | **16** |
| did not resolve | 244 | 351 |

⭐ **The first thing it did was fill a dash.**
[`../HISTORY/gates.md`](../HISTORY/gates.md) carried `-` for the IPv6-only
count since the measurement gate was written, so every statement about the
IPv6 limitation was about an unmeasured population. It is **16 of 1327**, about
1.2%.

⭐ **And three of the fifteen are not lost at all.** Each has an IPv4 host
under the same registrable domain **already in this corpus and already
probed**:

* `anna.bt.bontal.net` -> `bt.bontal.net`, `yuki.bt.bontal.net`
* `ipv6.govt.hu` -> `tracker.govt.hu`
* `ipv6.tracker.harry.lu` -> `ipv4.tracker.harry.lu`

That is exactly what this entry's route (e) predicted -- "many IPv6-only
entries are only IPv6 *in one list*" -- and it means the operator behind those
three is reachable from this vantage today. ⚠ **It does not make the IPv6-only
URL measurable**; it makes it *redundant*, which is a different and weaker
statement and is what the records must say.

⛔ **A much larger number came out of the same run and it is not IPv6.** **351
URLs on 244 hosts do not resolve at all** -- fifteen times the IPv6 population
this entry was written about. That is `unknown`, never `dead`, and it is a
bigger unexplored question than the one being worked. It is
[T-036](measurement.md).

⚠ **What route (e) did NOT do**: produce a liveness signal. Twelve IPv6-only
hosts have no sibling and remain exactly as unmeasurable as before, so the
`Prove` clause is still open on routes (a), (b) and (d).

---

### T-032 The exclusion route the README promises operators is not implemented

Source:      `C-51`; RULES 4; found by review 5
Category:    measurement
Priority:    P0
Effort:      S
Status:      done

Problem:     `README.md` tells tracker operators, in the present tense:
             *"publish a `BITTORRENT` TXT record on your tracker's hostname
             denying connections, and this project stops."* **Nothing in `src/`
             reads DNS TXT records.** There is no BEP 34 code path at all.

             This is a promise made to third parties, in the document most
             likely to be read by one, that the code does not keep. It is also
             load-bearing internally: RULES 4.1 withdrew the descriptive
             User-Agent requirement partly on the argument that *"BEP 34
             achieves the same end far better"* -- an argument that only holds
             if BEP 34 is honoured.

             ⚠ **Half of that title was already false when this was worked,
             and the correction is here rather than in a silent edit (RULES 7).**
             The README no longer makes the promise: the 2026-09-01 session
             removed it and left the reasoning pointing at RULES 4.1, which is
             what this entry's own `Decision` asked for. So the defect that
             remained was **not** a false promise in the README; it was the
             larger one underneath it -- RULES 4 forbids a corpus-wide probe
             until the automatable route exists, and it did not exist, so the
             gap was blocking every entry that needs to touch a real tracker
             rather than merely embarrassing one document.
Premise:     **Measured, not suspected.** `grep -rn "BEP 34\|bep_34" src/`
             returned nothing (exit 1), re-run at the start of the session that
             closed this. `C-51` records the mechanism as `VERIFIED` and says
             "adopt it"; the adoption never happened, and the sweep write-up
             lists it under *mechanisms adopted*, which overstates it.

             What limited the damage: **no operator was ever probed against
             this promise.** The probe had never been pointed at the corpus, so
             the gap was a conduct defect waiting on the first corpus probe
             rather than one already committed.
Approach:    A `bep34` module beside `bep15`: resolve the tracker hostname's
             `TXT`, parse a `BITTORRENT` record, and return allow / deny per
             protocol. Wire it into the probe **before** the ladder, so a denied
             tracker is never contacted at all rather than contacted and then
             filtered.

             Two things the reference already paid for. **Use public
             resolvers, not the host's** -- newTrackon issue #316 records BEP 34
             opt-outs being silently not honoured on its production instance
             because Hetzner's internal resolvers did not follow CNAMEs, and it
             failed *silently*, which is the worst way for an opt-out to fail.
             And a **DNS failure is not consent**: an unresolvable TXT lookup
             must not be read as permission, so it records `unknown` and the
             tracker is skipped rather than probed.
Decision:    **P0, and it gates any live corpus probing.** Nothing about it is
             hard; it is P0 because RULES 4 is absolute and because the cost of
             getting it wrong is borne by somebody who explicitly asked not to
             be contacted. Until it lands, the README must not claim the route
             works -- corrected in the same change as this entry.
             Rejected: honouring BEP 34 only at publication time (too late -- the
             operator objects to being *probed*, not to being listed); treating
             a missing record as denial (would empty the corpus).

             Six further calls were made while building it, each with what was
             rejected, because each is a place a later session would otherwise
             re-argue from scratch:

             1. **The record is an allow-list, not a deny-list.** BEP 34 says a
                `BITTORRENT` record means the host runs trackers *only* on the
                ports it names, so a bare `BITTORRENT` denies everything and a
                record naming `UDP:1337` denies `UDP:6969` on the same host.
                Rejected: looking for the word `DENY`, which honours only the
                readable spelling in the spec's second example and misses the
                normative one.
             2. **An unadvertised endpoint is skipped, never redirected.** The
                spec tells a *client* to retry on an advertised port; this
                project measures the endpoint a list published, so retrying
                elsewhere would report the health of an endpoint nobody listed.
                Same reasoning as `Tracker.scrape_url` refusing to invent one.
             3. **Two failure values, not one.** `excluded_by_operator` and
                `exclusion_undetermined` are distinct because a run that
                skipped a thousand trackers on a broken resolver must not read
                as a thousand operators refusing us. Rejected: a single
                "skipped" value, which would hide our own outage inside their
                choice.
             4. **A denial is `unmeasurable`, not a seventh health state.**
                Nothing was learned about the tracker either way; the reason
                lives in the `failure` field, which is where reasons live
                (RULES 3.10). Rejected: an `excluded` state, which would add a
                value to a published vocabulary for a consumer that does not
                exist yet.
             5. **First definitive resolver answer wins**, rather than querying
                all three and honouring any denial among them. The stricter
                option triples this project's DNS load against the whole corpus
                (RULES 15.2) to catch a divergence `experiments/04` measured at
                0 of 17 names. **If T-007 ever measures meaningful divergence,
                this is the decision it reopens** -- recorded in the code at the
                function that makes it.
             6. **Conflicting records are `undetermined`, not first-wins.** DNS
                does not order an answer set, so believing the first would make
                the verdict depend on send order (RULES 3.6).

             ⚠ **One gap is left open rather than half-built**, and it is
             recorded in the code at the branch that creates it: a corpus URL
             naming a host by **IP literal** has no hostname to query, so it is
             allowed. An operator who denies `tracker.example` is therefore not
             protected on an entry that names the same machine by address.
             Closing it needs a denial to propagate to siblings sharing a
             *resolved* address, which the probe only learns after resolving,
             and it belongs with T-031's resolved-address work rather than
             bolted on here.
Prove:       `python3 -m unittest tests.test_bep34 -v` (planned) passes against
             the local oracle: a deny record skips the tracker without opening a
             socket, an allow record probes it, a malformed record is `unknown`
             and skips, and a resolver failure is `unknown` and skips. Then
             `grep -rn "bep34" src/trackers/probe.py` shows the check runs
             **before** the ladder, and the README's claim becomes true.

**Done.** `python3 -m unittest tests.test_bep34` -> `Ran 33 tests`, `OK`.
`python3 -m unittest discover -s tests` -> `Ran 160 tests` at acceptance, `OK`,
still with no network. `python3 scripts/check-gate.py --strict` -> 14 passed, 1 expected skip.

`src/trackers/bep34.py` implements the record parser and, because the standard
library has no TXT resolver and D1 forbids a dependency, the DNS client too:
UDP with a TCP fallback on truncation, bounded response sizes, compression
pointers that cannot loop, and the transaction id and echoed question checked
before any answer is believed.

⭐ **The gate is in `probe_udp` and `probe_http`, not in `probe`.** Both are
public entry points that open their own sockets, and the oracle tests call them
directly -- gating only the dispatcher would have left two ungated doors into
the same action, which is the most recurring hole in
`docs/conventions/forbidden-patterns.md`. `effective_port` was extracted in the
same change so the port the gate checks is provably the port the prober opens;
a gate checking a different port is decorative, and there is a test for exactly
that.

**The acceptance is about conduct, not about parsing.** `test_a_denial_sends_
nothing` points the probe at a real loopback tracker that records every datagram
it receives, and asserts it received none; `test_the_same_tracker_is_probed_when_
the_record_permits_it` is the positive control, without which a gate that
refused everything would pass. Mutation-proved as `code.md` requires: forcing
`_consult_operator` to always allow fails 5 tests including that one.

⚠ **The suite stays offline** because BEP 34 is keyed on a hostname, so an
address literal is short-circuited before any query. `tests/fake_dns.py` is the
oracle -- a resolver on loopback that can be told to answer NXDOMAIN, SERVFAIL,
silence, a truncated datagram, a wrong transaction id, garbage, or a compression
pointer aimed at itself -- and every one of those is asserted `undetermined`
rather than consent.

**What this unblocks is the point.** RULES 4's "until it is, no corpus-wide
probe runs" is now satisfied, which is what T-012, T-027, T-028 and the corpus
half of T-024/T-029 were all standing behind.

---

### T-034 The value gate rests on a 17-tracker arm, and a census would cost 82 probes

Source:      [T-027](measurement.md)'s acceptance; RULES 2 on sample counts
Category:    measurement
Priority:    P1
Effort:      S
Status:      done

Problem:     The value gate is answered, and the arm that **is** the comparison
             is 17 trackers. `experiments/27` measures the baseline's live
             floor at 9 of 17, a 95% interval of 31%-74% -- wide enough that the
             baseline's whole live yield is known only to within a factor of
             two, and every ratio in `HISTORY/gates.md` inherits that width.
Premise:     **Measured, not assumed.** The 200-tracker sweep is a stride over
             the sorted corpus, so it caught 17 of the baseline's 99 by
             sampling fraction alone (expected 14.9, `p = 0.573` -- the sample
             is unbiased, and thin on this arm).
Approach:    Probe the **whole** baseline list rather than a sample of it: 99
             trackers, of which 17 already have a record, so **82 probes**.
             That turns the arm from a sample into a census and removes its
             sampling error entirely -- the interval on the baseline's live
             count collapses to the measurement's own uncertainty.

             `scripts/probe-corpus.py` has no way to say "probe exactly these",
             so it needs a selection input. ⛔ **A selection input is not a
             gate bypass**: BEP 34 is consulted per host in `probe_udp` and
             `probe_http` regardless of how the list was chosen, and the
             concurrency, per-host and deadline bounds are `sweep()`'s. Anything
             that reached a tracker by another route would be the door-sweep
             defect again.
Decision:    A census of the baseline, **not** a bigger random sample. 82
             probes spent on the arm that decides the ratio buys more than 82
             spent uniformly, because the unique-to-us arm is already n=183 and
             its interval is a quarter as wide. RULES 15.2's budget is a
             reason to aim the requests, not only to cap them.
             Rejected: extrapolating harder from 17, which is what the wide
             interval already says cannot be done; and re-running the whole
             200-tracker sweep, which spends 200 requests to narrow one arm.
Prove:       A sweep whose selection is the baseline's 99 URLs, committed under
             `experiments/results/`, then
             `python3 experiments/27-value-gate.py --expect-answered` exits 0
             with the baseline arm reporting `measured` 99 and an interval
             visibly narrower than 31%-74%. ⛔ **If the narrower interval moves
             the verdict, `HISTORY/gates.md` and the README change with it** --
             including if it moves to "not justified".

**Done.** Workflow run **`34207344996`**, `ubuntu-24.04`, dispatched with
`only_source=ngosang_all`; its record is committed as
`experiments/results/health-sweep.github-actions-hosted.run34207344996.json`
and carries the selection block marking it a census rather than a sample.
`python3 experiments/27-value-gate.py --expect-answered` -> exit 0, result at
`experiments/results/27-value-gate.unclassified-host.20260908T090220Z.json`.

**The baseline arm is counted, not estimated: 63 live of 99.** The 17-tracker
sample said 52.9% with a 95% interval of 31.0-73.8; the census says **63.6%**,
inside it. So the sample was thin rather than biased, and the interval on that
arm is gone rather than narrowed.

| | sample | census |
| --- | --- | --- |
| baseline live | 52 [31-73] | **63**, counted |
| live yield ratio, worst case | 1.92x | **2.06x** |
| baseline live density | 52.9% | **63.6%** |
| our live density | 12.0% | **12.8%** |

**The verdict does not change and the argument gets harder**: a properly
measured baseline is better than the sample suggested, so the density gap is
wider. `HISTORY/gates.md` and the README carry both directions.

**The cost is a time confound**: the two arms now come from runs three days
apart, which the same-run design existed to avoid. `best_evidence()` declares
the seam in every field it produces, a consistency check licenses it, and
`--expect-answered` **fails** if a future census falls outside the sample's
interval. Rejected: re-probing the whole 200-tracker sample in the same run,
which spends 200 requests to remove a confound the check tests for nothing.

**Still thin, and now the only thin thing:** the arm carrying the "what we add"
figure is **183 of 1228**. A census of it is 1045 further probes, which belongs
with [T-084](operations.md)'s scheduled-sweep decision rather than an ad-hoc
dispatch.

---

### T-036 351 tracker URLs do not resolve, and nobody has asked why

Source:      `experiments/29-address-family-census.py`, 2026-09-08;
             [T-031](measurement.md)'s route (e) run
Category:    measurement
Priority:    P2
Effort:      M
Status:      done

Problem:     **351 URLs on 244 hosts did not resolve at all**, which is 26% of
             the corpus and **fifteen times** the IPv6-only population this
             project has spent far more words on. Every one of them is
             `unknown`, correctly, and `unknown` is not a place to leave a
             quarter of the dataset.
Premise:     **Measured, not suspected.** `experiments/29` resolved every one
             of the 965 distinct hostnames with `AF_UNSPEC` and recorded the
             `gaierror` class per host. ⚠ Two runs minutes apart disagreed by
             one host, so the figure moves with DNS and any analysis has to
             survive that.
Approach:    The question is not "are they dead" -- it is **which kind of
             not-resolving each one is**, because the kinds have different
             consequences and are currently one bucket:

             1. **NXDOMAIN**: the name is gone. Strong evidence, still not
                proof from one resolver on one day.
             2. **SERVFAIL or timeout**: somebody's nameserver is broken or
                slow. That is a fact about the DNS path, not the tracker.
             3. **A name that resolves elsewhere.** `experiments/04` measured
                resolver divergence at 0 of 17 and carries [T-007](claims.md)
                for how thin that is; 244 hosts is a far better sample for the
                same question, and it comes free with this one.
             4. **A tracker whose sibling resolves**, as route (e) found for
                three IPv6-only hosts. Same join, different bucket.
Decision:    Classify before concluding. ⛔ **The one thing that must not
             happen is these 351 quietly becoming `dead`** because a quarter
             of the corpus looks untidy: `MIN_SAMPLES_FOR_DEATH` is 3 and
             RULES 3.1 governs, and a resolver failure is a fact about the path
             to the tracker at least as much as about the tracker.
             Rejected: dropping non-resolving entries from the published
             dataset, which would destroy the historical record that makes the
             dataset valuable (RULES 11) and would delete the evidence needed
             to tell the four cases apart.
Prove:       An instrument reports the four classes above with counts, over at
             least two resolvers so that "does not resolve for us" and "does
             not resolve" are separated, and `HISTORY/corpus-baseline.md`
             carries the breakdown. ⛔ No entry moves to `dead` on its output.

**Done.** `python3 experiments/30-resolution-failure-classes.py
--expect-no-mass-divergence` -> exit 0, result at
`experiments/results/30-resolution-failure-classes.unclassified-host.20260908T092405Z.json`, breakdown in
[`../HISTORY/corpus-baseline.md`](../HISTORY/corpus-baseline.md). No health
state was written by any of it, and there is no `dead` among the six class
names: the vocabulary is about the lookup, not the tracker.

**Two resolvers, and not the same kind of thing** -- this host's `getaddrinfo`,
which produced the 351, and `src/trackers/bep34.py`'s own client against
1.1.1.1 / 8.8.8.8 / 9.9.9.9. Only hosts the first could not answer for were
asked twice; re-asking about the 515 that already resolve would triple this
project's DNS load to confirm something known (RULES 15.2).

**On the authoring host** -- the vantage is part of the number, which the claim
audit of the same day caught this table stating without:

| class | hosts | URLs |
| --- | --- | --- |
| resolves for both | 515 | 737 |
| **resolves only for the public resolver** | **11** | **14** |
| resolves only for this host | 0 | 0 |
| gone, NXDOMAIN confirmed | **179** | **256** |
| no address records | 43 | 61 |
| lookup failed, undetermined | 11 | 20 |

**The runner disagrees, and that is [T-007](../TODO/claims.md)'s finding rather
than a discrepancy**: 3 of 239 rescued on `ubuntu-24.04`, 2 on `22.04`.
[`../HISTORY/corpus-baseline.md`](../HISTORY/corpus-baseline.md) carries both
columns and names the runner canonical, because that is the vantage every
health record came from.

**The biggest class is evidence about the trackers**: 179 hosts NXDOMAIN by
public resolvers, so 256 URLs name something that does not exist. Still **not
`dead`** -- two resolvers on one day, and `MIN_SAMPLES_FOR_DEATH` is 3.

**And 14 URLs resolve perfectly well.** Eleven hosts, including
`tracker.parrotsec.org`, answer for a public resolver and not for this one. A
sweep from here would have recorded every one `dns_failure`.

⛔ **Corrected 2026-09-08 by [T-037](measurement.md): none of those 14 resolves
to anything reachable.** All eleven hosts answer `0.0.0.0`, `::` or both, which
is an answer and not an address. The paragraph above stands as what this
instrument reported before it distinguished them, and the count it gave is why
the classifier now has a seventh class,
`resolves_to_an_unusable_address`. Re-measured at **0 of 243 rescued**:
`experiments/results/30-resolution-failure-classes.unclassified-host.20260908T134349Z.json`.
The instrument recorded families and not addresses, which is what let a null
answer read as a rescue; it records both now.

**The classes are not literally the four this entry listed** (RULES 9):
`resolves_only_for_the_public_resolver` replaces "a name that resolves
elsewhere", `lookup_failed_undetermined` replaces "SERVFAIL or timeout", and
the sibling case is not here because `experiments/29` already reports it.

---

### T-037 A `dns_failure` records our resolver's opinion, and a better one is already in the tree

Source:      `C-06` re-measured; `experiments/30`, run `34210496112`
Category:    measurement
Priority:    P1
Effort:      M
Status:      done

Problem:     `src/trackers/probe.py`'s `_resolve` calls
             `socket.getaddrinfo`, so `Failure.DNS_FAILURE` means **"the
             runner's resolver did not answer"** and is published as though it
             meant "this name does not exist". Those are different facts and
             this project's whole premise is not conflating them.
Premise:     **Measured on the vantage that matters, on both images.**
             `experiments/30`, run `34210496112`: of 239 hosts the runner's own
             resolver could not answer for, public resolvers answer for **3**
             on `ubuntu-24.04` and **2** on `ubuntu-22.04` -- including
             `openbittorrent.com` and `tracker.openbittorrent.com`, **5 corpus
             URLs**, with `Temporary failure in name resolution`.

             ⭐ **The better resolver is already here.**
             `src/trackers/bep34.py` carries this project's own DNS client
             against three public resolvers, and it grew `addresses()` for
             exactly this question. Nothing needs building; something needs
             wiring.

             ⚠ **No committed record is wrong today.** Neither sweep sampled
             those hosts. A full-corpus sweep would carry five, which is why
             this is worth doing before one runs rather than after.
Approach:    ⛔ **Not "replace `getaddrinfo`".** It is the resolver a consumer
             on that machine would use, so what it says is a real fact about
             that vantage and must not be discarded.

             Record both, and distinguish the states:
             1. `getaddrinfo` answers -> unchanged, and no second query is
                made. The overwhelming majority (520 of 759) take this path and
                the DNS load does not move.
             2. `getaddrinfo` fails and the public resolver answers ->
                a **new failure value**, distinct from `DNS_FAILURE`, meaning
                "our resolver could not, another can". It belongs in `ABOUT_US`
                so it can never produce `dead` (RULES 3.1), and the record
                carries both answers.
             3. both fail, NXDOMAIN -> the strongest not-resolving signal this
                project can produce, and still not `dead`.
             4. both fail, no definitive answer -> `undetermined`.
Decision:    A second lookup only where the first failed, never for every
             tracker. 239 of 759 hosts in the current corpus, one extra query
             each at worst, and RULES 15.2 bounds what CI may spend.
             Rejected: querying the public resolver first (it would triple this
             project's visible DNS footprint against public resolvers for a
             gain that only applies to a quarter of hosts); and silently
             preferring whichever resolver answers, which would delete the
             fact that they disagreed -- the disagreement is the finding.
Prove:       `python3 -m unittest tests.test_probe -v` covers a host that
             `getaddrinfo` refuses and the injected public resolver answers,
             asserting the new failure value, that it is in `ABOUT_US`, and
             that `health_state` cannot return `dead` for it. Then a sweep
             record shows the value in use, and `HISTORY/claims.md` `C-06`
             cites it.

**Done.** `python3 -m unittest
tests.test_probe.OurResolversOpinionIsNotTheNamesProperty -v` -> **12 tests,
OK**. `second_opinion` in `src/trackers/probe.py` asks
`src/trackers/bep34.py`'s resolvers whenever `getaddrinfo` fails, and the four
states are `resolver_divergence` (new, in `ABOUT_US`), `dns_failure` for a
definitive not-resolving, `dns_undetermined` (new, in `ABOUT_US`) for no
answer, and unchanged behaviour where the first lookup succeeded. Both
answers are kept on the record under `dns`, because preferring one would
delete the disagreement.

⭐ **Both probers, not one.** `probe_udp` and `probe_http` are separate public
entry points and a control on one leaves the other reaching the same published
field. Four call sites take the second opinion, including the `gaierror`
`urlopen` raises after the probe's own lookup already succeeded.

⛔ **The premise was measured again and it does not hold on this vantage.** Of
243 hosts this host's resolver could not answer for, **0 are rescued by a
public resolver** -- not 11. Every one of the 11 that `experiments/30` had
counted answers `0.0.0.0`, `::`, or both, `tracker.parrotsec.org` among them.
An unspecified address is an answer and not an address (RFC 1122 section
3.2.1.3), so a seventh class exists, `resolves_to_an_unusable_address`, and it
is `dns_failure`: both resolvers agree there is nothing to connect to.
Two runs, `20260908T134349Z` committed. Without the distinction this entry
would have shipped 14 URLs labelled "our resolver's fault" that no resolver
can reach.

⛔ **`openbittorrent.com` never reaches this code from here.** All six public
queries for it time out, so the **BEP 34 consent lookup fails first** and the
tracker is skipped as `exclusion_undetermined`. Driven:
`python3 scripts/probe-corpus.py --offline-corpus --only-host
openbittorrent.com`. On the runner, where public resolvers answered on
2026-09-08, consent succeeds and the resolution failure is the one this entry
reclassifies -- so the premise holds there and the fix is still what stops 5
URLs being published as gone.

**What it buys here, counted:** the 10 hosts and **18 URLs** in
`lookup_failed_undetermined` can no longer accumulate toward `dead`, because
`dns_undetermined` is in `ABOUT_US`. Before this they were `dns_failure`,
which is not.

**A sweep record shows both values in use**, from
`python3 scripts/probe-corpus.py --offline-corpus --only-host bt1.xxxxbt.cc`:

```json
{"url": "http://bt1.xxxxbt.cc:6969/announce", "health_state": "unknown",
 "failure": "dns_failure", "measurement_rung": "none",
 "used_synthetic_infohash": false,
 "detail": "gaierror: [Errno 11004] getaddrinfo failed; public resolvers: resolves_to_an_unusable_address"}
```

⚠ **That record is deliberately not committed.** `experiments/27`, `28` and
`32` glob every `health-sweep.*.json` under `experiments/results/` and merge
their records, so a one-host demonstration dropped there would enter the value
gate as evidence. The instrument is the deliverable and `--only-host` re-runs
it in a second.

⛔ **Two findings came from reading a real record rather than from the suite.**
`used_synthetic_infohash` was `true` on every HTTP result including ones that
opened no socket -- a scrape claimed on a record where nothing was sent, which
is the hardcoded-status forbidden pattern and had already reached a committed
sweep. It is now set at the point the request goes out. And the null-address
class above was visible only because the record carries the addresses.

**The Decision's cost is corrected (RULES 9).** It said "one extra query each
at worst". It is **two** per failing host, one per address family, because
knowing a host is IPv6-only is the point of asking; and up to **six** where no
resolver answers. Measured: 243 hosts asked on this vantage, so 486 queries at
best and 1458 at worst, against the 100,000 per run the operator set. Wall
time is the real cost: six queries at a 3 s timeout is 18 s for one host that
nobody answers for.

**The classifier has one home.** `experiments/30` defined the vocabulary and
now imports it from `src/trackers/bep34.py`, so a committed result and a
health record cannot disagree about what a name's resolution was. The
experiment also records the addresses now, not only the families -- the
committed runner runs cannot be reclassified because they did not, and that is
what a summary that drops the evidence costs.
