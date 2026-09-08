# Progress

**The record.** Everything that changes from session to session is here: the
measured baseline, what the last session did, and the work order.

⛔ **It carries no history and every session rewrites it in full.** For history,
read [`../HISTORY/`](../HISTORY/) and the git log.

**Read [`../docs/AGENTS.md`](../docs/AGENTS.md) first.** It carries the
absolutes, the routing table, and the box on why a session may not stop or
defer. [RULES.md](RULES.md) is normative for all of it. Every entry, one line
each: [INDEX.md](INDEX.md).

> **The shape this file must keep:** the state line with the session's start
> instant in ISO 8601 UTC; the measured baseline, citing the file that owns each
> number; the entry counts; what the session did; what is in progress;
> **Start here next session** as an ordered list with entry ids; and open
> questions for the operator. `python3 scripts/check-todo.py` prints the counts,
> and none of them is typed here.

---

## State

- **Last session:** started `2026-09-08T08:15:00Z`, ended on operator
  instruction (RULES 10.2, first way). A **measurement and justification
  pass**: the value gate was answered, the corpus was measured three more
  times, and per-tracker history exists.
- **Branch:** `main`, public at `https://github.com/Azathothas/Trackers`.
- ⛔ **Still nothing published as data.** No dataset exists at any public URL.
  Every measurement lives under `experiments/results/` as evidence.
- ⛔ **Nothing is `dead`, and nothing can be.** `MIN_SAMPLES_FOR_DEATH` is 3
  and no tracker has three observations. The most any record says is
  `unknown`.

## Measured baseline

**Every corpus figure lives in
[`../HISTORY/corpus-baseline.md`](../HISTORY/corpus-baseline.md)** and nowhere
else, with the command behind each. Do not restate one here; cite it.

⚠ **The DNS figures on that page are now PER VANTAGE**, because
`experiments/30` measured a runner's own resolver failing where a public one
answers. That is `C-06`'s finding, not a discrepancy, and a single table would
be the failure that page exists to prevent.

| | |
| --- | --- |
| Value gate | **ANSWERED**: justified as a labelled dataset, **not** as a list ([`gates.md`](../HISTORY/gates.md)) |
| Live yield vs the baseline | **2.06x** worst case, 2.70x point, 3.68x best |
| Live density | ours **12.8%** of 1327, baseline **63.6%** of 99 |
| First corpus sweep | run **`33938543488`**, 200 of 1327 by stride |
| Baseline census | run **`34207344996`**, all **99**, **63 live** counted not estimated |
| DNS census | run **`34210496112`**, both images. IPv6-only **16 URLs on 15 hosts** |
| Resolver divergence | **3 of 239** on `ubuntu-24.04`, **2** on `22.04`. `openbittorrent.com` fails on both (`C-06`) |
| Oracle disagreement | **17 of 93** = 18.3%, methodology caveat attached (`C-03`, `C-69`) |
| Within-AS8075 variation | **0** of 34 subject-days, across **5** distinct addresses (`C-03`) |
| Client compatibility | plaintext survives **aria2 1.37.0** unchanged (`C-40`, `C-41`) |
| State projection | K=64, D=180, **23.4 MB** at five years (**D3**) |
| Test suite | **246** tests, no network |
| Reference corpus | **10** repositories, **980** files, identical in a fresh clone |
| Local gate | `python3 scripts/check-gate.py --strict`: 16 pass, 1 expected skip |
| Pre-commit hook | available, **opt-in**: `python3 scripts/install-hooks.py` |
| Cold start | confirmed on a fresh clone: gate green, `references/` and `experiments/results/` identical |
| CI | `gate.yml` green on the pushed head, confirmed by looking |

⛔ **`live` is a floor, not a rate.** One datacenter, IPv4 only, one
observation per tracker. A tracker that timed out is `unknown`, and some of
those are up.

## Counts

Run `python3 scripts/check-todo.py`. It re-derives every number from the rows
and fails a gate when [INDEX.md](INDEX.md)'s table disagrees. **Nothing is
blocked.**

## What the last session did

**Ten entries closed**, including **two `L`** and both `P0`s in the work
order: [T-027](measurement.md) the value gate, [T-001](claims.md) the client
check, [T-040](scoring.md) state and history (**L**), [T-004](claims.md)
vantage bias (**L**), [T-028](measurement.md) the oracle cross-check,
[T-033](measurement.md) the duplicated codecs, [T-036](measurement.md)
resolution classes, [T-034](measurement.md) the baseline census,
[T-042](scoring.md) the state projection, [T-007](claims.md) resolver
agreement. **D3 closed.**

⭐ **The value gate is answered and the answer is two-sided.** Bar 1 clears:
filtering to what measured live yields 2.06x-3.68x as many working trackers as
the whole baseline. Bar 2 **fails**: our list is 13.4x longer and five times
less live-dense. ⛔ **So publishing the plaintext without the health data would
make this project the thing it exists to improve on**, and the README says so
with the unflattering half first.

⛔ **The first decision rule drafted for that gate was wrong in our favour**
and is kept visible in `DECISION_RULE` rather than edited away: it charged us
our sampling error and forgave the baseline's.

**Five findings that changed the tree.**

1. ⛔ **A file named after a package switches off the dependency gate.**
   Measured at exit 0 printing *"D1 holds"*. Fixed by refusing a local name
   that also resolves outside the repository.
2. ⛔ **This project wrote a stranger's private-tracker credential into four
   of its own files** while refusing to publish it in the dataset. Found by
   widening `PRIVATE_CREDENTIAL` to see `authkey=`; all four redacted.
3. ⛔ **This vantage cannot resolve `openbittorrent.com`.** Both runner
   images; public resolvers can. A full sweep would publish one of the
   best-known public trackers as gone -- [T-037](measurement.md).
4. ⛔ **The committed sweep record's own `counts.corpus` said 200 against a
   corpus of 1327.** The sample was right and its denominator was not.
5. ⛔ **One endpoint in five that this session contacted bought nobody
   anything** -- 56 because a wildcard path filter fired a tracker probe for an
   offline instrument, 11 because a dispatch cancelled an in-flight run after
   it had already reached the trackers.

**Five reviews ran and every one found something**, under
[`../HISTORY/reviews/`](../HISTORY/reviews/): the door sweep (findings 1 and
2), the claim audit (`corpus-baseline.md` had acquired a contradictory number,
from this session), the tracker-operator pass (finding 5), the guard mutation,
and the cold start.

⛔ **`experiments/27 --expect-answered` did not fail, it raised.** Driven
against the condition it exists for, it exited 1 with a `KeyError` traceback.
A crash and a measured expectation failure share an exit code, so every check
of the exit code agreed and nobody read the output. Fixed; both directions
verified.

⛔ **The work order sent a cold session at a command that does not exist.**
T-012's `Prove` clause names an experiment that was never built and was not
marked `(planned)`. `check-citations.py` could not see it: it skipped any
backticked token containing a space, and **every `Prove:` clause is a
command**. The check now splits a command and checks each path in it.

⚠ **Two commits went out with the gate red**, both the same way: the gate was
run, one more edit landed, and the reading was not repeated. Neither was
carelessness about the rule -- both messages quote a real gate run.
`scripts/install-hooks.py` is the structural answer, **opt-in and never
automatic**. ⭐ Its first version broke on the hazard `shell.md` section 6
documents: `command -v python3` finds the Windows stub, which then does not
run.

## In progress

**Nothing half-finished.** Every entry touched is either closed with its
acceptance recorded, or open with what remains written into it.

## Start here next session

1. **[T-037](measurement.md)** - a `dns_failure` records our resolver's
   opinion, and a better resolver is already in the tree. ⭐ **Do this before
   any full-corpus sweep**, because five URLs would be published wrong.
2. **[T-041](scoring.md)** - the seven shapes over time. The store that can
   express them now exists ([T-040](scoring.md)) and nothing computes them.
3. **[T-031](measurement.md)** - still the leverage entry. Route (c) returned
   nothing for any `unmeasurable` tracker and route (e) dissolved 3 of 15;
   routes (a) NAT64, (b) a relay and (d) public gateways are untried.
4. **[T-012](claims.md)** - whether our identity gets us blocked. ⚠ Twelve
   cells over the HTTP corpus is roughly twelve thousand requests at somebody
   else's expense: a workflow over days, not a command.
5. **[T-026](measurement.md)** - the politeness budget. Both inputs are now
   settled: D7's cadence, and DNS inside the budget at **100,000 lookups per
   run**. What is left is computing it, publishing it in the run report, and
   asserting it in a test.
6. **[T-064](publication.md)** - release channels. Its platform half is
   measured.

**Deliberately deferred:** [T-044](scoring.md), the scoring model. **D4** stays
open on purpose: history exists now, but no tracker has more than four
observations, and choosing a model against that is fitting it to noise.

**When this order is exhausted**, take the next entry from [INDEX.md](INDEX.md)
by priority. ⛔ Do not stop because the list above ran out.

## Questions the operator has answered

**Closed; do not re-raise.**

1. **May a session create throwaway releases here?** Yes, in this repository.
   D10 and RULES 13.1. Tag them `test-*` and delete them once the answer is
   recorded.
2. **What defines membership of `foss.txt`?** Derived, plus a labelled seed.
   D9, settled in [T-046](scoring.md).
3. **Is the roughly three-hour probe cadence acceptable?** Yes: publish hourly,
   probe each tracker on its own stated interval, defaulting to three hours.
   D7, settled in [T-026](measurement.md).
4. **What is authorised outward-facing?** Every action belonging to this
   repository, and nothing outside it, ever. RULES 13.
5. **One squashed commit per session, or a series?** A **clean series of
   logical commits**, each passing the gate. **D16**, 2026-09-08. The squash
   could not coexist with confirming CI at every push and with the
   no-force-push rule.
6. **Is DNS inside the politeness budget?** Yes, with a ceiling of **100,000
   lookups per run on a GitHub runner**; local runs are not bounded by it.
   2026-09-08. This session used roughly 4,000. [T-026](measurement.md) carries
   it.
7. **Schedule the health sweep?** Not until [T-037](measurement.md) lands.
   2026-09-08. A scheduled sweep today would repeatedly record
   `openbittorrent.com` as `dns_failure`, which this vantage cannot resolve and
   public resolvers can.
8. **The third party's credential in git history?** **Not our action.** The
   operator will **reset this repository's history to a single commit** once
   the tasks are complete and the prose has been rewritten, which removes it.
   2026-09-08. Until then the working tree stays clean of it outside the
   capture fixtures, where a verbatim capture is expected and where rewriting
   one would destroy the evidence that the refusal works.

## Open questions for the operator

**None.** All eight are answered above.

⚠ **Two standing facts to know rather than re-derive**, neither of which is a
question:

- **BEP 34 lookups send tracker hostnames to a public resolver**, a trade
  recorded in `src/trackers/bep34.py`. The evidence for it strengthened on
  2026-09-08: the host's own resolver was measured failing on names public
  resolvers answer, which is the silent failure the trade avoids.
- **A history reset to one commit is coming** (answer 8). Nothing should be
  built that depends on this repository's commit history, which RULES 3.7
  already forbids for measurement history.
