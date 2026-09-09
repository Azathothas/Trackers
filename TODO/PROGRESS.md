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

- **Last session:** started `2026-09-09T01:50:00Z`, ended on operator
  instruction (RULES 10.2, first way). A **reach pass**: three transport
  categories that had been `unmeasurable` in every record this project ever
  took were measured, and the politeness ceiling stopped being a by-product of
  arithmetic.
- ⭐ **Every P0 and P1 is closed.** [T-012](claims.md) reported after eleven
  days open, and [T-087](operations.md) was opened and closed inside the
  session.
- **Branch:** `main`, public at `https://github.com/Azathothas/Trackers`.
- ⛔ **A live RULES 4 violation was found, and it had been published.** The
  sweep's slice came from `epoch // 10800`, so two runs inside one three-hour
  bucket took the **identical** slice: runs `34281244142` and `34289476724`
  both took slice 5 and **192 trackers were contacted 5878 s apart**, inside
  D7's 10800 s interval. The ceiling is read from the recorded history now and
  no arithmetic can breach it ([T-087](operations.md)). ⛔ The two observations
  **stay** in the published history: deleting them would be tidying away the
  evidence of our own misconduct.
- ⭐ **The dataset accumulates and the rotation walks.** `state.jsonl` carries
  **901** trackers, 96 with three observations and 15 with four; the published
  set is **1326** trackers, **142 live, 70 dead, 8 degraded, 55 unmeasurable**
  and 1051 not yet measured. The `data` branch now also carries
  `sources.jsonl` and `source-quality.json`.

## Measured baseline

**Every corpus figure lives in
[`../HISTORY/corpus-baseline.md`](../HISTORY/corpus-baseline.md)** and nowhere
else, with the command behind each. Do not restate one here; cite it.

⚠ **The DNS figures on that page are PER VANTAGE**, because `experiments/30`
measured a runner's own resolver failing where a public one answers. That is
`C-06`'s finding, not a discrepancy.

| | |
| --- | --- |
| Value gate | **ANSWERED**: justified as a labelled dataset, **not** as a list ([`gates.md`](../HISTORY/gates.md)) |
| Live yield vs the baseline | **2.06x** worst case, 2.70x point, 3.68x best |
| Live density | ours **12.8%** of 1327, baseline **63.6%** of 99 |
| Committed sweeps | **7**, all `github-actions-hosted`, all `ci` |
| Baseline census | run **`34207344996`**, all **99**, **63 live** counted not estimated |
| DNS census | run **`34210496112`**, both images. IPv6-only **16 URLs on 15 hosts** |
| Resolver divergence | **3 of 240** on `ubuntu-24.04`, **1** on `22.04`, run `34235047982` (`C-06`) |
| IPv6-only liveness | **6 of 16** URLs alive, direct over IPv6 ([T-031](measurement.md)) |
| **i2p liveness** | **3 of 11** HTTP destinations answered, from an i2pd router in a container ([T-039](measurement.md), `C-37`) |
| **yggdrasil liveness** | node joined, a peer answered **twice**, the one corpus host answered **neither time** -- one observation, never `dead` ([T-039](measurement.md)) |
| **wss liveness** | **5 of 10** completed an RFC 6455 handshake; **2 answered a scrape** ([T-005](claims.md), `C-36`) |
| **Identity arms** | descriptive **34/35**, minimal **26/26**, client_like **23/24**, absent **22/26**, spread **0.154** ([T-012](claims.md), `C-56`) |
| Oracle disagreement | **17 of 93** = 18.3%, methodology caveat attached (`C-03`, `C-69`) |
| State projection | K=64, D=180, **23.4 MB** at five years (**D3**) |
| **Source-history projection** | ring 240, daily 365, **0.26 MB** steady against **3799 MB** uncapped ([T-103](sources.md)) |
| Test suite | **578** tests, no network |
| Reference corpus | **10** repositories, **980** files, identical in a fresh clone |
| Politeness budget | full corpus **6072** DNS at worst of 100,000 ([T-026](measurement.md)) |
| **Politeness ceiling** | enforced from `state.jsonl`'s `last_seen`, not from the rotation ([T-087](operations.md)) |
| **Schedule delay** | **3 observations**: 163 m, 131 m, 3 m; **one slot produced no run**. 100 needed ([T-009](claims.md)) |
| Local gate | `python3 scripts/check-gate.py`: 16 pass, 1 expected skip |
| Cold start | confirmed on a fresh clone, 2026-09-09 |
| CI | `gate.yml` green on the pushed head, confirmed by looking |
| Health sweep | **scheduled** `0 */3 * * *`, `ci` profile, one of **7** rotating slices ([T-084](operations.md)) |

⛔ **`live` is a floor, not a rate.** One datacenter, IPv4 only for the sweep.
A tracker that timed out is `unknown`, and some of those are up.

## Counts

Run `python3 scripts/check-todo.py`. It re-derives every number from the rows
and fails a gate when [INDEX.md](INDEX.md)'s table disagrees. **Nothing is
blocked.**

## What the last session did

**Nine entries closed, one of them opened in the same session, and one new one
opened from a review.**

⛔ **[T-087](operations.md) -- the rotation was never the ceiling.** The slice
came from the three-hour wall-clock bucket a run *starts in*, so two runs in one
bucket take the same slice and nothing refused the second. ⭐ **The test that
should have caught it asserted something else**: it compared rotation `0` with
rotation `1`, a property of two integers, while the schedule needs a property of
two runs. `politeness.too_soon_after` reads what was recorded; `sweep.plan` is
the only door into a selection and **advances past a slice held in full**, so a
collision costs coverage rather than nobody.

⭐ **[T-012](claims.md) -- answered by reading the series, not by more
probing.** RULES 4 permits one arm per tracker per run, so the comparison
belongs to the rotation series and **no single run of it could ever have
answered**; every earlier run was individually right to refuse. `--series`
clears the twenty-per-arm bar. ⛔ **The descriptive User-Agent is not being
refused and the weakest arm is sending none at all.** `C-56` is `REFUTED`.

⭐ **[T-039](measurement.md) and [T-005](claims.md) -- three unmeasurable
categories measured.** A router in a throwaway container cost one `podman run`,
and the route T-039 listed **last** is the one that worked while route (d), the
public gateway, is still 503. ⛔ Findings inside them: only **3 of 13** `.i2p`
URLs have a scrape endpoint at all; **addressing by name measures our own
addressbook first** (one tracker timed out by name and answered 1677 bytes by
b32); and three `wss` endpoints upgrade and then close on a scrape, which is
`protocol_valid` and **not** a tracker.

⭐ **[T-103](sources.md) and [T-101](sources.md) -- provenance, and what it
makes answerable.** `explain_absence` separates "the sources stopped listing
it" from "a source failed to fetch", which is RULES 3.2 extended through time
and the only place a consumer can ask it. ⛔ The module committed that exact
conflation in its first draft -- `EMPTY` was filed as a blind spot -- and a test
caught it. The per-source quality report re-derives T-101's premise from the
corpus and, on **live** upstreams, raised the corroboration question the entry
was written for: `ngosang_all` contributes **0 unique of 79**.

⭐ **[T-045](scoring.md), [T-102](sources.md), [T-100](sources.md) -- three
prohibitions made checkable instead of accidentally true.** I7 rejects a model
somebody would plausibly write; two threshold bands did not satisfy the rule
their own comment claimed; and the four registry fields that are per-run state
are now asserted **absent** from the registry because they exist in
`provenance.SourceHistory`.

⚠ **[T-009](claims.md) has an instrument and stays open.** Three scheduled runs
observed and one slot with no run; the distribution needs 100 and the blocker is
time rather than work. ⛔ Its own claim that slices are "skipped, never
repeated" is **refuted** -- and it had already named T-087's mechanism,
deferring it to a decision ([T-063](publication.md)) that had by then been made.

**Four deep reviews**, under [`../HISTORY/reviews/`](../HISTORY/reviews/), and
every one found something: a control that is a mechanism on one path and a
convention on the other ([T-088](operations.md), opened), a test whose fixture
never reached the cap it tested, a `PROGRESS.md` describing a tree two sessions
old, and every tracker-facing contact itemised from the far end of the socket.

**Recovered evidence.** Five sweeps had measured real trackers with their
records in no committed file; artefacts expire in 90 days and git does not.
Committing them also folded two runs the publisher had never seen. ⭐ The value
gate is **unmoved** by them, which is the property it should have.

## In progress

**Nothing half-finished.** Every entry touched is closed with its acceptance
recorded, or open with what remains written into it.

## Start here next session

1. **[T-088](operations.md)** - D7 is a mechanism on the sweep path and a
   convention on the experiment path. ⭐ Opened by the door sweep, and the
   pieces already exist: `too_soon_after` is the rule and the recorded history
   is the record, so it is wiring and a test rather than a design.
2. **[T-123](docs.md)** - most acceptances cannot be run as written. ⭐ The
   highest-leverage entry left: it improves every other entry's `Prove` clause
   at once, and this session hit the problem repeatedly.
3. **[T-122](docs.md)** - the consumer contract is documented and nothing
   enforces it. The published dataset now has three more files than the
   contract describes.
4. **[T-082](operations.md)** - self-healing, whose limit matters more than its
   coverage.

**Deliberately deferred:** [T-044](scoring.md), the scoring model. **D4** stays
open on purpose: the deepest history is four observations, and choosing a model
against that is fitting it to noise. ⭐ It is better-defended than before --
seven invariants now, and a Wilson lower bound passes all of them.

**When this order is exhausted**, take the next entry from [INDEX.md](INDEX.md)
by priority. ⛔ Do not stop because the list above ran out.

## Questions the operator has answered

**Closed; do not re-raise.**

1. **May a session create throwaway releases here?** Yes, in this repository.
   D10 and RULES 13.1. Tag them `test-*` and delete them once answered.
2. **What defines membership of `foss.txt`?** Derived, plus a labelled seed.
   D9, settled in [T-046](scoring.md).
3. **Is the roughly three-hour probe cadence acceptable?** Yes: publish hourly,
   probe each tracker on its own stated interval, defaulting to three hours.
   D7, settled in [T-026](measurement.md). ⭐ **It is enforced from the record
   now** rather than assumed from the cadence ([T-087](operations.md)).
4. **What is authorised outward-facing?** Every action belonging to this
   repository, and nothing outside it, ever. RULES 13.
5. **One squashed commit per session, or a series?** A **clean series of
   logical commits**, each passing the gate. **D16**, 2026-09-08.
6. **Is DNS inside the politeness budget?** Yes, with a ceiling of **100,000**
   lookups per run on a GitHub runner. 2026-09-08. [T-026](measurement.md)
   computes it: **6072** at worst.
7. **Schedule the health sweep?** Not until [T-037](measurement.md) lands.
   2026-09-08. ⛔ **Superseded the same day by answer 10.** Kept because it is
   why the schedule waited, and because its reasoning -- that a `schedule:` is
   a standing commitment against other people's servers -- is still why it was
   put to the operator. ⚠ **2026-09-09 proved that reasoning right in a way
   nobody predicted**: the commitment was kept and the *slice arithmetic* broke
   it anyway.
8. **The third party's credential in git history?** **Not our action.** The
   operator will **reset this repository's history to a single commit** once the
   tasks are complete. 2026-09-08.
9. **May this project contact a tracker whose operator it has no automatable
   way to ask?** **Yes -- the asking route is enough.** 2026-09-08. ⭐ This is
   what the i2p measurement rests on: 11 `.i2p` destinations were contacted once
   each, and a `.i2p` name still has no DNS record for BEP 34 to read.
10. **Schedule the health sweep?** **Yes, at D7's cadence.** 2026-09-08.
    `tests/test_rotation.py` asserts the coverage **and**, since
    [T-087](operations.md), that two consecutive *runs* share no tracker.
11. **Should a commit credit the tool that helped write it?** **No.**
    2026-09-08. [`../docs/conventions/git.md`](../docs/conventions/git.md)
    forbids crediting any tool and that overrides a harness default.

## Open questions for the operator

**None.** Eleven have been asked and eleven are answered above.

⚠ **Three standing facts to know rather than re-derive**, none of which is a
question:

- **BEP 34 lookups send tracker hostnames to a public resolver**, a trade
  recorded in `src/trackers/bep34.py`. ⚠ And it **cannot be consulted at all**
  for a `.i2p` name, so those thirteen operators have no automatable way to
  refuse us. Answer 9 settles that the asking route suffices; the asymmetry
  should stay visible rather than be filed as solved.
- **A history reset to one commit is coming** (answer 8). Nothing should depend
  on this repository's commit history, which RULES 3.7 already forbids for
  measurement history.
- ⭐ **Two instruments need a container and cannot run in CI**
  (`experiments/35`, `experiments/36`). That is
  [`../docs/containers.md`](../docs/containers.md)'s premise rather than a
  limitation; both take the engine as a variable, and both decommission what
  they start -- verified by counting, not by remembering.
