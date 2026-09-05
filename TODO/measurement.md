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

**Done.** Workflow run **`33938543488`**, `ubuntu-24.04`, 2026-09-05. The
`Every record carries its vantage` step is
`python3 scripts/check-vantage-metadata.py --path sweep-out` and it printed:

```
checked 200 health records

OK  every record carries vantage metadata and a measurement rung; nothing
    unmeasurable is reported live, dead or degraded.
```

⭐ **This is the first time this project has measured a real tracker.** The
sample is 200 of 1327 (`ci` probes a sample, RULES 15.2), from
`github-actions-hosted`, IPv4 only, with a 900 s deadline that was not reached.
The records are committed at
`experiments/results/health-sweep.github-actions-hosted.run33938543488.json`,
because a workflow artefact expires after 90 days and git does not.

| | |
| --- | --- |
| `live` | 25 |
| `degraded` | 1 |
| `unknown` | 162 |
| `unmeasurable` | 12 |

⛔ **`dead` is 0 and cannot be otherwise from one sweep.**
`MIN_SAMPLES_FOR_DEATH` is 3, so a single observation of a tracker that did not
answer is `unknown` -- "too few samples", not "gone". Accumulating observations
across runs is [T-040](scoring.md), and until it exists **no number here is a
liveness rate.** 25 of 200 is one datacenter, on IPv4, on one day.

**The refusals are the finding, and they are recorded as `C-72`.** Eight
endpoints across seven hosts were refused by a published BEP 34 record and
three more were skipped because public resolvers would not answer. One host
spells its denial as a bare `BITTORRENT`, which is the normative form and the
one a naive implementation misses.

⚠ **The offline gate keeps its expected skip, and that stays correct.**
`scripts/check-vantage-metadata.py` finds no records in a clean checkout
because none are committed where it looks, and where health records eventually
live is [T-063](publication.md)'s decision rather than this entry's to
pre-empt. The flag comes off when the data branch does.

**What ran it:** `.github/workflows/health-sweep.yml`, `workflow_dispatch`
only. ⛔ It has no `schedule:` trigger deliberately -- a workflow that runs
hourly against other people's servers is a load generator, and the cadence is
D7's, the budget [T-026](measurement.md)'s and the architecture
[T-084](operations.md)'s.

             **Two of the three parts now exist, and the third is a run rather
             than a piece of work.** `src/trackers/sweep.py` emits a record per
             tracker with its vantage, `scripts/probe-corpus.py` writes
             `health.json`, and `scripts/check-vantage-metadata.py` takes a
             `--path` so records can live in scratch instead of dirtying the
             tree. `tests.test_concurrency.RecordsSatisfyTheVantageGate` runs
             **the real gate over records this project produced**, against
             trackers it controls on loopback, and it exits 0.

             ⛔ **What remains is a probe of the real corpus from a sanctioned
             vantage, and it was deliberately not run here.** RULES 13.1
             authorises probing live trackers **from CI**; the session that
             built this ran on a contributor's Windows host, whose
             `environment_class` is `unclassified-host` and whose address is a
             residential connection rather than the vantage every other figure
             in this project was taken from. Probing a thousand strangers'
             servers from it would have produced records that are not
             comparable with anything and would have spent somebody's home
             address doing it.

             ⚠ **Three routes to closing it were considered** (RULES 10.1a):

             1. **A workflow step on a runner.** The right one. It needs a
                schedule and a politeness budget, which is T-084's decision and
                T-026's number, and it is where this entry actually closes.
             2. **Emit `unknown` records for the whole corpus offline**, which
                would flip the gate today without probing anything. **Refused**:
                the file would satisfy the checker while nothing had been
                measured, which is the forbidden pattern about a step that
                exits 0 having done nothing. `scripts/probe-corpus.py` has no
                offline mode for that reason, and its docstring says so.
             3. **Probe a handful of trackers from this host** to produce a few
                real records. **Refused**: a measurement from an unclassified
                vantage that is then compared with runner figures is the
                vantage-conflation this project exists to avoid, and RULES 15.4
                requires the profile to travel with the result rather than the
                result to be taken anywhere convenient.

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
Status:      open

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
Prove:       `python3 experiments/27-value-gate.py --expect-answered` (planned)
             exits 0, having computed the three deltas against
             `ngosang/trackerslist` from committed health records; then
             `HISTORY/gates.md` carries the answer with its conditions and
             sample counts, and the README states the verdict -- including if
             the verdict is that this project is not justified.

---

### T-028 newTrackon is available as an oracle and is not being used as one

Source:      `C-26` refuted; decision D2
Category:    measurement
Priority:    P2
Effort:      M
Status:      open

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
Prove:       `python3 experiments/28-newtrackon-crosscheck.py` (planned) exits
             0 and emits a report whose header states the methodology
             difference -- **newTrackon announces and we scrape**, so its
             "uptime" and our "live" answer different questions (`C-69`) -- and
             whose every rate carries a sample count.

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

**Done.** `python3 -m unittest tests.test_concurrency` -> `Ran 20 tests`, `OK`,
no network. `src/trackers/sweep.py` is the runner and `scripts/probe-corpus.py`
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
