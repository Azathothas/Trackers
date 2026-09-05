# Decision record

The work model of RULES 7, in its leanest defensible form. One index, one
entry per decision, every entry carrying its own history.

**Counts in this file are checked by a command, not by hand:**

```sh
python3 scripts/check-decision-record.py
```

RULES 7 requires that "counts must agree with rows, enforced by a checker
that runs as a gate -- not by hand". Hand-maintained counts drift silently, and a
drifting count is a small lie that trains readers to ignore the document.

## Index

| id | decision | gate | status |
| --- | --- | --- | --- |
| D1 | Implementation language and runtime | P0 | **closed** |
| D2 | Measurement vantage: runners only, or a second vantage | P0 | **closed** |
| D3 | State/history storage format and location | P1 | open |
| D4 | Scoring model | P3 | open |
| D5 | Publication topology: data branch vs. releases vs. both | P4 | open |
| D6 | Whether unmeasurable-protocol trackers are published, and where | P2 | open |
| D7 | Probe cadence, against the measured politeness anchor | P2 | **closed** |
| D8 | Which upstream exclusions this project adopts | P1 | **closed** |
| D9 | `foss.txt` membership rule | P3 | **closed** |
| D10 | Standing authorisations for an unattended session | - | **closed** |
| D11 | Two execution profiles, and which is the default | P2 | **closed** |
| D12 | What shape the template adoption takes | - | **closed** |
| D13 | What BEP 34 binds, and what it unblocks | P0 | **closed** |

**Counts:** 13 entries, 9 closed, 4 open, 0 blocked

**Nothing closes as "won't fix" or "out of scope"** (RULES 7). A blocked
entry stays open with its blocker named and what would unblock it.

---

## D1 -- Implementation language and runtime, **closed**

**Source** the brief's section 26, **Category** architecture, **Priority** high,
**Effort** low, **Gate** P0

**Problem.** The project must run unattended for years. Every runtime choice is
a bet on what will still install, build and behave in five years.

**Premise, and how it was checked.** RULES 12's starting hypothesis is
"Python 3.12+, stdlib-first, async I/O". *Measured, not assumed:* the runner
conditions block in `experiments/results/01.ubuntu-24.04.run33383406869.json`
records `python: 3.12.3` present on the image with no install step, and the
`ubuntu-22.04` sibling records 3.10.12. ⚠ **The older image is below the 3.11
floor**, which is a fact about that image rather than about the project: the
experiments are written to run on it, the pipeline is not shipped to it, and
`src/trackers/__init__.py` refuses to import below 3.11 rather than failing
obscurely. The harder
question -- whether stdlib alone can do the actual work -- is answered by working
code rather than by argument: `experiments/02` implements BEP 15 connect with
`socket` and `struct`, `experiments/05` implements a bencode parser and both a
positive and a negative control server, and `experiments/19` fetches and
censuses 16 sources with `urllib`. All run on the runner with **zero installed
dependencies**.

**Decision.** **Python 3.12+, standard library only.** No third-party runtime
dependency without a recorded decision naming what it earns.

Concurrency is `asyncio` where I/O-bound fan-out needs it, not threads: the
workload is network-bound, and the measured probe RTT (median **109-127 ms**)
is dominated by the network rather than by anything the language does.

**Rejected alternatives.**

| rejected | why it lost |
| --- | --- |
| Go or Rust, single static binary | The stated attraction is deployment robustness, but the deployment target is a GitHub runner that already ships Python. It buys a build step, a toolchain to pin for five years, and a cross-compilation matrix, against a workload that is I/O-bound. RULES 12 permits it "if a measured need appears" -- none has. |
| Node.js | The domain's prior art is Python (newTrackon is Flask), and the ecosystem's norm is many small dependencies, which is the opposite of the five-year requirement. |
| Python plus a shell layer | This is what the anti-pattern exhibit does, and its `set +e` / `continue-on-error` / `curl -o` interaction is precisely how it loses a whole source silently. A shell layer also makes RULES 5.1's "never interpolate upstream content into a shell command" a thing to remember rather than a thing that is structurally impossible. |
| `requests` / `httpx` for fetching | `urllib` did the whole census with zero 401/403 (`C-43`). Adding an HTTP library buys convenience and a supply-chain dependency for something already working. |

**Prove.**

```sh
python3 scripts/check-no-third-party-imports.py
```

Fails when any module outside the standard library is imported by shipped code.

---

## D2 -- Measurement vantage, **closed**

**Source** the brief's section 10.3, **Category** architecture, **Priority**
critical, **Effort** medium, **Gate** P0

**Problem.** Every measurement from one cloud provider's address space carries
that provider's bias. RULES 3.4 calls a mislabelled single-vantage number
the "confident wrongness" failure that destroys the project's value.

**Premise, and how it was checked.** The original brief assumed runners might
be unable to measure at all, which would have forced a second environment.
*Measured:* `C-01` **refuted** that -- `udp_arbitrary_port_egress: true` on both
runner images, with tier-0 and four tier-1 controls. Runners are sufficient to
reach the top rung of the measurement ladder on `udp`, `http` and `https`.

Separately, `C-26` was **refuted**: newTrackon exposes machine-readable uptime
at `/api/<int:percentage>`, measured monotone (261 -> 82 -> 55 -> 15). That
provides an independent *oracle* at zero operational cost.

**Decision.** **One vantage -- GitHub-hosted runners -- plus newTrackon as an
independent oracle. No second measurement environment.**

Three requirements make this honest rather than merely convenient:

1. Every health record carries vantage metadata (`C-54`: AS8075, image,
   IP-family availability, probe version). Non-negotiable.
2. `unmeasurable` is a first-class state, and IPv6-only, `i2p`, `yggdrasil` and
   `ws`/`wss` entries **must** occupy it rather than `dead` (`C-04`, `C-37`).
3. The oracle is a **cross-check, not a source of truth**, and every comparison
   states the methodology difference: newTrackon *announces* to derive uptime,
   this project stops at scrape, so its "uptime" and our "live" answer
   different questions.

**Rejected alternatives.**

| rejected | why it lost |
| --- | --- |
| A self-hosted runner as second vantage | Directly contradicts RULES 6 "not a hosted service": it is an operational burden the project explicitly cannot carry, and it becomes a single point of failure whose outage looks identical to trackers going down. |
| A third-party measurement API as a primary signal | HISTORY/decisions.md D2: trades a GitHub limitation for a third-party outage. Worse, it answers a different question than ours and the difference would be invisible in the output. |
| No oracle at all | Cheapest, and it discards the single most informative thing this dataset could publish (RULES 3.4): disagreement between independent observers. |
| Claiming two vantages by also probing from the authoring sandbox | It is also datacenter address space, behind an HTTP proxy, and **UDP is blocked there** -- measured. Two biased vantages that share the bias are one vantage with extra steps. |

**What this decision knowingly costs, stated because it is a real limitation.**
`C-03` -- whether trackers block datacenter ranges -- **stays unresolved**. The
dataset is single-vantage and cannot distinguish "dead" from "dead from AS8075".
The mitigation is labelling, not measurement, and labelling does not make the
number better; it only stops it lying. Resolving `C-03` properly needs
experiment 20's second vantage, which is deferred, not abandoned.

**Prove.**

```sh
python3 scripts/check-vantage-metadata.py
```

Fails when any emitted health record lacks vantage metadata or a measurement
rung, or when an unmeasurable transport/network appears with a `live`, `dead`
or `degraded` state.

**It currently exits 2, "could not run", and that is the correct answer.**
Health records are a P2 deliverable and do not exist yet. The script refuses to
return 0 over an empty set on purpose: a green tick reading "every record
carries vantage metadata" while checking nothing is a false assurance that
would survive into the phase where it matters.

---

## D3 -- State/history storage, open, gate P1

Blocked on nothing; not yet due. RULES 3.7's resolution (history
lives in **files**, never inferred from git history) is a MUST and is treated as
settled input to this decision, not as part of it.

## D4 -- Scoring model, open, gate P3

T-043's six invariants are the defensible part and get written first,
per T-043's own advice, which is that the invariants outlive the model. The model choice is deliberately deferred until there is
history to fit it against; choosing now would be fitting a model to zero samples.

## D5 -- Publication topology, open, gate P4

**No longer blocked.** `C-14`, `C-15` and `C-17` remain unverified -- how
`/releases/latest` resolves against a tag literally named `latest`, whether
release assets can be replaced at a stable URL, and whether moving a tag moves
the release -- but the reason they were unverifiable is gone. **Operator ruling
2026-08-29: creating, mutating and deleting throwaway releases in this
repository is sanctioned** (RULES 13.1). T-003 answers all three; this decision
follows from its answers.

**What is already settled by evidence:** `C-16` -- `raw.githubusercontent.com`
serves `max-age=300` with a strong ETag, so the data-branch path is viable and
the feared "caching defeats hourly generation" failure does not occur. And
`C-55` -- scheduled workflows run **only on the default branch**, so a data
branch can never carry its own cron.

## D6 -- Publishing unmeasurable trackers, open, gate P2

Evidence already gathered that this decision must respect: the census found
**14 i2p** and **13 ws/wss** entries, and `trackers_all.txt` upstream **silently
excludes** exactly this class -- 17 trackers with no notice to the consumer.
Whatever is decided, the choice must be **visible in file naming** rather than
silent (RULES 3.1 requirement 2), because the alternative is the upstream
behaviour this project exists to improve on.
## D8 -- Which upstream exclusions this project adopts, **closed**

**Source** raised by evidence during P1, **Category** data policy,
**Priority** high, **Effort** medium, **Gate** P1

**Problem.** The pre-publication verifier caught 182 URLs in the output that
`ngosang/trackerslist` had blacklisted. Something had to be decided, and both
obvious answers are wrong.

**Premise, measured.** `blacklist.txt` @ `1e61597`, 346 entries, reasons
tabulated from the committed fixture:

| count | reason | what kind of statement it is |
| --- | --- | --- |
| 178 | registered torrents | editorial policy |
| 135 | duplicate of `<url>` | the resolved-address inference, `src/trackers/dedup.py` question 3 |
| 11 + 2 | malfunction | somebody else's measurement |
| 7 | deprecated by owner | **the operator** |
| 5 | detected by antivirus software | safety |
| 2 | fake seeds | somebody else's measurement |
| 2 | **requested by sysadmin** | **the operator** |
| 1 each | error, blocked by IDNA ban, redirects to, detected as suspicious | measurement / safety |

A blacklist mixes two incompatible kinds of claim. "The operator asked you to
stop" is not a measurement and is not ours to re-litigate. "We measured this
and disliked it" is an opinion from a vantage we cannot inspect, produced by a
generator that is **not published** (`C-22`).

**Decision.** Classify each reason and enforce only two classes.

* **HONOUR** -- operator requests. Excluded always, whether or not the tracker
  works. RULES 4 requires it.
* **SAFETY** -- antivirus/malware/suspicious. Excluded; publishing a
  credibly-malicious endpoint harms consumers.
* **OPINION** -- everything else. **Kept, and flagged with the reason.**

Measured effect on the real corpus: of 346 upstream exclusions, **9 honour +
6 safety = 15 enforced**, and **331 opinions kept and flagged**. Eight entries
were actually removed from the dataset by an enforced exclusion.

Unrecognised reasons default to OPINION. That is the safe direction: treating
an operator request as an opinion would mean continuing to probe somebody who
asked us to stop, so HONOUR is matched explicitly and generously while an
unknown reason merely leaves the entry in place with a flag.

**Rejected alternatives.**

| rejected | why it lost |
| --- | --- |
| Adopt the blacklist wholesale | Inherits 331 unauditable filtering decisions from a generator nobody can read, which is precisely what HISTORY/reference-sweep.md warns against. It would also delete the `bt.okmp3.ru` case -- blacklisted upstream as "fake seeds", listed **live** by newTrackon, and proved a working tracker by this project's own runner probe. Three observers, three answers; RULES 3.4 calls that disagreement the most informative thing this dataset could publish. |
| Ignore the blacklist entirely | Cheapest, and it breaks RULES 4 outright: two entries are explicit operator requests, and continuing to probe those is the behaviour that gets an address range blocked and corrupts our own future measurements. |
| Exclude on reason keywords without classifying | "malfunction" and "requested by sysadmin" are both strings in the same column and have opposite consequences. Keyword matching without a class is how the two get conflated. |

**What this knowingly costs.** 331 entries stay in the dataset that a
respected upstream removed. Some of them are certainly bad trackers. The
project's answer is that it will measure them itself and publish what it finds,
including where it disagrees -- not that it knows better.

**Prove.**

```sh
python3 -m unittest tests.test_p1.TestExclusionClassification -v
```

Eight tests, including one asserting that an operator request removes a tracker
that appears in other sources, and one asserting that an upstream *opinion*
does not.

---

## D7 -- Probe cadence, **closed**

**Source** raised by evidence, not by the brief, **Category** politeness,
**Priority** high, **Effort** S, **Gate** P2

**Problem.** The brief specified "approximately hourly" probing and asked
whether 30 minutes adds value. Its anchor for that -- "well-behaved clients
re-announce every ~30 minutes" -- had no measurement behind it.

**Premise, measured.** Two independent and agreeing sources replace it.
`references/CorralPeltzer__newTrackon/tree/newtrackon/tracker.py:163` floors its
recheck interval at **10800 s (3 h)** and otherwise takes `interval` from the
tracker's own response (`:136-138`). Its maintainer states in issue #334: *"The
current checking frequency (every ~3 hours) is reasonable for the server load.
Trackers that have been down for more than 1.5 years are already automatically
removed."* That monitor **announces**; this project stops at scrape, so it does
strictly less work per check.

**Decision -- operator ruling 2026-08-29. Publish hourly; probe each tracker on
its own stated `interval`, defaulting to 3 h where none has been observed.**

Hourly *generation* touches no tracker and was never in question: aggregation,
validation and publication are local work. Hourly *probing of every tracker* is
3x the load of the closest production analogue, and nothing measured justifies
it.

**Rejected alternatives.**

| rejected | why it lost |
| --- | --- |
| Hourly probing of every tracker, as the brief specified | 3x a production monitor that does strictly *more* work per check. No measurement supports it, and the brief's own anchor for it was unsourced. |
| 30-minute probing | 6x. The brief raised it as a question; the answer is no. |
| A fixed global ~3 h interval | Simpler to reason about, and it ignores what each tracker actually asks for. The tracker is the only authority on the load it wants, which is the whole reason the anchor moved. |

**Prove.** A test that fails when the configured schedule would exceed one probe
per tracker per its stated interval. T-026.

---

## D9 -- `foss.txt` membership, **closed**

**Source** T-046, **Category** data policy, **Priority** low,
**Effort** M, **Gate** P3

**Problem.** `foss.txt` is "trackers primarily associated with Linux
distributions and FOSS ecosystems". That is a category boundary, not a
measurement, and **a hand-curated list presented as derived is a lie about
methodology.** The rule had to be stated before the file could be generated.

**Decision -- operator ruling 2026-08-29: derived, plus a labelled seed.**

Two halves, and they stay distinguishable in the output because that is the
entire point:

* **derived** -- FOSS-ecosystem sources go in the registry (Fosstorrents and
  related, T-105) and membership follows from FOSS provenance, auditable like
  any other source;
* **seed** -- a small hardcoded list in its own file, **labelled as curated
  rather than measured**.

**The operator will supply seed entries to a future session as "Additional
References".** Until they arrive the seed file is present and **empty** rather
than guessed -- an empty labelled seed is honest, and a guessed one is exactly
the methodology lie this decision exists to prevent.

**Rejected alternatives.**

| rejected | why it lost |
| --- | --- |
| Pure source-derived | Discards curation the operator wants to provide, and would make `foss.txt` empty or near-empty for no gain. |
| Pure curated | Makes no derivable claim and wastes provenance the project already has for every entry. |
| Drop the category | The requirement is satisfiable, so dropping it would be a silent downgrade -- forbidden by RULES 9. |

**Prove.** A test that every entry in the derived half traces to a FOSS-provenance
source, and that the seed half is emitted with its curated label intact. T-046.

---

## D10 -- Standing authorisations for an unattended session, **closed**

**Source** the operator, 2026-08-29, **Category** process, **Effort** S

**Problem.** Sessions are unattended. An earlier rule told a session to stop and
ask before outward-facing actions -- publishing a first dataset, creating
releases, opening issues. An unattended session that stops is a session that
does nothing, and the questions it would ask have no one to answer them.

**Decision.** **Every outward-facing action belonging to this repository is
authorised** without asking: releases and tags (including throwaway ones for
measurement), the `data` branch, publishing the first dataset once the value
gate is honestly answered, issue automation here, probing live trackers within
the politeness budget, and pushing to `main`.

**Nothing outside this repository may ever be written to.** No pull request,
issue, discussion or comment on any other repository, on the operator's behalf
or the session's own. No contact with tracker operators or maintainers. Reads
of other repositories are how the reference corpus exists and remain fine; the
proxies are read-only routes and must never carry a write.

**Rejected alternatives.**

| rejected | why it lost |
| --- | --- |
| Keep asking before outward-facing actions | Guarantees an unattended session halts at P4 with the work undone and no one present to unblock it. |
| Authorise writes to other repositories too | Filing issues upstream on the operator's behalf is speaking for them to third parties. Not delegable, and the work never requires it -- a would-be upstream action becomes an open entry naming the blocker. |

**Prove.** RULES 13. A session that stops for authorisation on a sanctioned
action has misread it; a session that writes to another repository has broken it.

---

## D11 -- Two execution profiles, and which is the default, **closed**

**Source** the operator, 2026-08-31, **Category** architecture, **Effort** S

**Problem.** This project runs in two places with very different budgets. On a
GitHub runner, request noise against other people's servers is the binding
constraint and there is no IPv6 egress (`C-04`), no I2P or Yggdrasil router
(`C-37`), and a job timeout. On a contributor's machine -- possibly Windows with
Podman -- there is usually real IPv6, a residential resolver, working UDP, and no
reason to ration requests beyond politeness. **The wrong resolution is to build
for the runner and call the difference a limitation**, because that permanently
caps what this project can measure at what the cheapest environment allows.

**Decision.** **One code path, an explicit profile, never a reduced feature
set.** `src/trackers/profile.py` defines two budgets -- `ci` and `local` -- as
data, and every caller reads permissions from the budget rather than sniffing
its environment. RULES 15 is the normative statement.

Three properties make it a mechanism rather than a preference:

1. **`ci` is the default and `local` is opted into**, via one variable
   (`TRACKERS_PROFILE`). A run that says nothing gets the restrictive budget on
   any host, including a laptop. The expensive mistake available here is a
   full-corpus sweep fired by accident; the cheap one is a contributor running
   the restricted profile and wondering why.
2. **A capability withheld by a profile is still in the code.** IPv6 probing
   exists and is skipped in `ci` for a measured reason, and the vantage record
   says *"ipv6 withheld by the ci profile"* rather than the family being
   silently absent. RULES 15.4.
3. **A profile never overrides a measurement.** `C-04` measured a runner with
   an IPv6 stack *and* a route that still cannot get a packet out; passing
   `ipv6_egress=False` withholds the family even under `local`. Asserted by
   `tests.test_profile.TestVantageCarriesTheProfile`.

**A profile is not a correctness switch.** Nothing in it changes what a
measurement means, what counts as `dead`, or what the pipeline outputs from the
same inputs. Determinism (RULES 3.6) is unaffected and the offline gates pass
identically under both. It bounds how much work touches third parties and which
optional transports are attempted.

**Rejected alternatives.**

| rejected | why it lost |
| --- | --- |
| Auto-detect `local` from the absence of CI variables | Auto-escalates the budget on any machine that happens not to set them, and breaks when a CI vendor renames one. Detection reads exactly one variable and defaults to the restrictive side. |
| Default to `local`, restrict on a runner | Inverts the risk. The accident this guards against is a full sweep from an environment that should not run one, and defaulting to the permissive budget makes that accident the default. |
| Fall back to `ci` when `TRACKERS_PROFILE` is misspelled | Silently running the restrictive profile when somebody asked for `locl` is quiet wrongness. An unknown value raises and names the rule. |
| Separate scripts, or a `--ci` flag on each entry point | Two code paths drift, and the drift is invisible until a measurement disagrees. One path with a data-driven budget cannot. |
| Make it a correctness switch too (e.g. treat unreachable as `dead` in `ci`) | That is the confident-wrongness failure the whole project exists to prevent. `unmeasurable` is a statement about our vantage in **both** profiles (RULES 3.1). |

**Prove.** `python3 -m unittest tests.test_profile` -- 15 tests, including that
`ci` is tighter on every axis that costs a third party, that `local` is never
permitted less reach, and that a measured egress failure outranks either
profile.

---

## D12 -- What shape the template adoption takes, **closed**

**Source** the operator, 2026-09-01, **Category** architecture, **Effort** M

**Problem.** This project's rules had cited `Azathothas/TEMPLATE` as their work
model since the first draft, and the tree had never been held to the rest of
that methodology. Adopting it raised three choices that could not each be made
independently, because the wrong combination produces a gate that only runs on
one platform or a document set that says two different things.

**Decision, in three parts.**

**1. The checks are rewritten in Python, not copied as shell pairs.** The
template ships every check as a `.sh` and a `.ps1` twin, with a further check
to stop the two answering differently. RULES 15.5 already forbade that shape
here: a `.sh` a gate depends on is a platform requirement in disguise. Six
checks were ported, one runner was written, and the rules they hold are the
template's unchanged. `scripts/README.md` is the contract.

**2. The two helpers that are not gates are vendored verbatim and pinned.**
The environment probe and the commit-and-push helper do a job that has nothing
to do with tracker measurement, they are maintained upstream, and copying them
unchanged means a later version is a re-fetch rather than a merge.
`scripts/vendor/toolkit/PIN.json` records a commit and a digest per file and
`scripts/check-vendor-pin.py` holds them to it, offline.

**3. The character rule is applied mechanically, not by rewriting sentences.**
1655 characters outside the allowed five were in 55 files, 840 of them em
dashes. Every em dash became a spaced double hyphen, which is meaning-preserving
by construction and therefore safe to apply to a normative rule and to a claims
register. `HISTORY/reviews/2026-09-01-01-unrequested-change.md` states the limit
of that: it satisfies the letter of the rule everywhere, and rewriting the
remaining parentheticals into commas and colons is real work a later session can
still do.

**Rejected alternatives.**

| rejected | why it lost |
| --- | --- |
| Copy the `.sh` and `.ps1` pairs unchanged | Two implementations of one rule need a third check to police them, the corpus exemption this project needs would fork both, and the gate would then require a POSIX shell or a PowerShell host. RULES 15.5 is the rule it breaks. |
| Port the probe and the push helper to Python as well | A tool kept in two repositories acquires two sets of defects and one of the two never gets fixed. Neither is a gate, so RULES 15.5 does not bind them, and a pinned copy costs a digest check rather than a maintenance burden. |
| Move `HISTORY/` under `docs/`, which is where the template puts it | Several hundred citations point into it, `check-citations.py` verifies every one, and the move would be churn with no measured benefit. The template's own reason is about a capitalised prose directory sitting beside source, which is a preference. |
| Rewrite all 840 em dashes as sentences | Better prose, and it would have edited the wording of normative rules and verified claim rows by hand at a scale nobody can review. The mechanical substitution is checkable; the rewrite is a reading. |
| Leave the character rule unapplied, as `ADOPT.md` suggests for a repository with an established voice | The operator asked for the opposite, with the measured count in front of them, and the em dash is the specific thing being asked about. |
| Take the template's `CHANGELOG.md` and `SECURITY.md` skeletons | Nothing has shipped and there is no deployed system. `docs/conventions/docs.md` records both absences and what would bring each back, so the absence is a decision rather than an oversight. |

**Prove.**

```sh
python3 scripts/check-gate.py --strict
```

Fourteen checks and one expected skip. `scripts/check-vendor-pin.py` covers
part 2 on its own, and `scripts/check-markers.py` covers part 3.

---

## D13 -- What BEP 34 binds, and what it unblocks, **closed**

**Source** RULES 4, [T-032](../TODO/measurement.md), 2026-09-05,
**Category** conduct, **Effort** S

**The requirement as written.** RULES 4 read: *"Two routes: asking --
implemented and tested -- and a BEP 34 `BITTORRENT` DNS TXT record, which is
automatable, needs no contact with us at all, and is not implemented yet.
**Until it is, no corpus-wide probe runs**, because the automatable route is
the one an operator can use without knowing we exist."*

**The evidence it needed changing.** The clause was not wrong; it was
satisfied. `src/trackers/bep34.py` implements the record parser and the DNS
client it needs, and both probers consult it before opening a socket. What
made the change necessary rather than cosmetic is that the sentence was the
standing block on [T-012](../TODO/claims.md), [T-027](../TODO/measurement.md),
[T-028](../TODO/measurement.md) and the corpus half of
[T-024](../TODO/measurement.md) and [T-029](../TODO/measurement.md) -- and
`PROGRESS.md`'s work order listed T-012 first without noticing that RULES
forbade it. A rule that blocks the work order silently is worse than one that
blocks it loudly.

**The replacement.** The blanket ban on corpus-wide probing is lifted and
replaced by a narrower, permanent one: **a corpus-wide probe runs only through
the code path that consults BEP 34 first.** What is forbidden is reaching a
tracker by a route that skips the check, not the sweep itself.

**Why the gate sits in both probers rather than in `probe`.** `probe_udp` and
`probe_http` are public entry points that open their own sockets, and the
oracle tests call them directly. A control on the dispatcher alone would have
left two ungated doors into the same action.

**Rejected alternatives.**

| rejected | why it lost |
| --- | --- |
| Keep the ban until a second exclusion route also exists | Nothing names a second route, so the ban would have no closing condition. RULES 8 forbids an open-ended block; a blocker must say what lifts it. |
| Honour BEP 34 at publication time instead of at probe time | The operator objects to being *probed*, not to being listed. Filtering afterwards means the packet was already sent. |
| Treat an unanswerable lookup as permission | It is the recorded production failure of this mechanism (newTrackon issue #316), and it fails silently, which is the worst way for a refusal to fail. |
| Treat a missing record as a denial | Would empty the corpus, since almost no host publishes one. |
| A seventh `HealthState` for an excluded tracker | Nothing was learned about the tracker either way, so `unmeasurable` is already true of it. The reason belongs in the `failure` field, which is where a rejection's reason lives (RULES 3.10). |
| Query all three resolvers and honour any denial among them | Strictly safer for the operator, and it triples the DNS load this project generates across the whole corpus (RULES 15.2) to catch a divergence `experiments/04` measured at 0 of 17. Recorded at the function that decides it, so [T-007](../TODO/claims.md) can reopen it with a number. |
| A `ProbeConfig` flag to skip the consultation | A documented route to contacting a host that refused us. RULES 4.1's immovable line. |

**Prove.**

```sh
python3 -m unittest tests.test_bep34
```

`tests.test_bep34` reports `Ran 33 tests`, `OK`, with no network. The
load-bearing one points the probe at
a loopback tracker that records every datagram and asserts it received none.

