# TODO

Every entry, one line each. The entry itself lives in the `TODO/<category>.md`
the row links to, and it closes there with its own acceptance command, actually
run, with the output recorded.

**What to work on next is not here.** [PROGRESS.md](PROGRESS.md)'s "Start here
next session" is the work order and is the only place that carries one. This
file carries the list, the definitions, the counts, and the argument behind the
current ordering.

[RULES.md](RULES.md) is how this repository is worked on, rule by rule.
[`docs/AGENTS.md`](../docs/AGENTS.md) is the orientation for a session that has
never seen this repository.

`scripts/check-todo.py` checks this file against the entries: a status that
disagrees, a row with no entry, an entry with no row, a count that does not add
up, a missing field, a `done` entry with no recorded acceptance, a `blocked`
entry that never says what would unblock it, and a dead link.

```bash
python3 scripts/check-todo.py
```

**Ids are stable and are never renumbered or reused.** The table below is sorted
by priority rather than by id, because the question a session asks of this file
is "what matters most", and gaps in the id sequence are intentional.

## Priority

- **P0** breaks correctness, loses data, or takes the process down.
- **P1** a documented capability does not work, or a flag does nothing.
- **P2** worth doing, nothing is wrong without it.
- **P3** worth recording so it is not rediscovered.

## Effort

S is under a day, M is a few days, L is a week, XL is almost always two entries
pretending to be one.

## Status

`open`, `blocked` (the blocker is named, with what would unblock it), `done`
(the acceptance was run and its output recorded in the entry).

**Nothing closes as "won't fix", "upstream's problem" or "out of scope"**
(RULES 8). `scripts/check-todo.py` fails on those strings.

## Entries

| ID | Priority | Category | Status | Item |
| --- | --- | --- | --- | --- |
| [T-001](claims.md) | P0 | claims | open | No torrent client has ever been run against our plaintext |
| [T-002](claims.md) | P0 | claims | open | A public repository's schedule stops after 60 days and nothing here notices |
| [T-012](claims.md) | P0 | claims | open | Nobody has measured whether our User-Agent gets us blocked |
| [T-021](measurement.md) | P0 | measurement | done | The probe has no oracle, so a silently broken probe would mark everything dead |
| [T-027](measurement.md) | P0 | measurement | open | The value gate is unanswered: uniqueness is measured, liveness is not |
| [T-032](measurement.md) | P0 | measurement | **done** | The exclusion route the README promises operators is not implemented |
| [T-140](foundation.md) | P0 | foundation | **done** | Runner network and protocol behaviour was never measured on a runner |
| [T-003](claims.md) | P1 | claims | open | Release and tag behaviour is unverified, and it blocks the publication topology |
| [T-004](claims.md) | P1 | claims | open | Vantage bias is unresolved and the dataset cannot distinguish dead from dead-from-AS8075 |
| [T-020](measurement.md) | P1 | measurement | done | The health checker does not exist |
| [T-024](measurement.md) | P1 | measurement | open | No health record carries vantage metadata, because no health record exists |
| [T-025](measurement.md) | P1 | measurement | done | The health state machine and failure classification are undefined |
| [T-026](measurement.md) | P1 | measurement | open | The politeness budget is neither computed nor published nor asserted |
| [T-029](measurement.md) | P1 | measurement | **done** | Probing has no concurrency control, timeout budget or cancellation behaviour |
| [T-031](measurement.md) | P1 | measurement | open | Liveness for networks this vantage cannot reach -- the leverage entry |
| [T-040](scoring.md) | P1 | scoring | open | There is no state or history, so nothing can be scored |
| [T-043](scoring.md) | P1 | scoring | open | The six scoring invariants are not enforced by anything |
| [T-046](scoring.md) | P1 | scoring | open | The five required categories do not exist |
| [T-060](publication.md) | P1 | publication | open | JSON and CSV outputs do not exist |
| [T-061](publication.md) | P1 | publication | open | Cross-format consistency is unverified |
| [T-063](publication.md) | P1 | publication | open | There is no data branch and nothing is published anywhere |
| [T-085](operations.md) | P1 | operations | open | Overlapping runs are prevented in the gates but not in publication |
| [T-086](operations.md) | P1 | operations | open | Security review has not been run against the acquisition path |
| [T-141](foundation.md) | P1 | foundation | **done** | No reference had been read below README depth |
| [T-142](foundation.md) | P1 | foundation | **done** | The protocol model was mis-factored and would have marked I2P trackers dead |
| [T-143](foundation.md) | P1 | foundation | **done** | There was no pipeline, and determinism had never been demonstrated |
| [T-146](foundation.md) | P1 | foundation | **done** | The README did not exist, so the honesty statements had nowhere to live |
| [T-107](sources.md) | P1 | sources | **done** | The pipeline republishes private-tracker credentials |
| [T-005](claims.md) | P2 | claims | open | WebTorrent trackers are unmeasurable by default and nobody has tried |
| [T-006](claims.md) | P2 | claims | open | Actions billing for public repositories is unverified |
| [T-007](claims.md) | P2 | claims | open | Resolver agreement was measured at n=17 on one day |
| [T-022](measurement.md) | P2 | measurement | open | UDP scrape needs a synthetic infohash and the ladder does not model that |
| [T-023](measurement.md) | P2 | measurement | done | Yggdrasil trackers addressed by hostname are silently misclassified as clearnet |
| [T-028](measurement.md) | P2 | measurement | open | newTrackon is available as an oracle and is not being used as one |
| [T-030](measurement.md) | P2 | measurement | open | Experiments 3-18 from the original programme were never run |
| [T-041](scoring.md) | P2 | scoring | open | History must distinguish seven shapes over time, not seven values |
| [T-042](scoring.md) | P2 | scoring | open | The state size over five years has never been computed |
| [T-044](scoring.md) | P2 | scoring | open | No scoring model has been chosen |
| [T-045](scoring.md) | P2 | scoring | open | Ranking must not use the latest instantaneous result |
| [T-047](scoring.md) | P2 | scoring | open | A hardcoded tracker unreachable for 48 hours must raise an issue, not vanish |
| [T-062](publication.md) | P2 | publication | open | Nothing is versioned, so a consumer cannot tell what they received |
| [T-064](publication.md) | P2 | publication | open | Release channel semantics rest on three unverified platform claims |
| [T-066](publication.md) | P2 | publication | open | Run reports do not answer the questions observability requires |
| [T-080](operations.md) | P2 | operations | open | Issue automation does not exist |
| [T-081](operations.md) | P2 | operations | open | History housekeeping is unimplemented and its threshold is unjustified |
| [T-082](operations.md) | P2 | operations | open | Self-healing is unimplemented, and its limit matters more than its coverage |
| [T-083](operations.md) | P2 | operations | open | The five-year operational review is unanswered |
| [T-084](operations.md) | P2 | operations | open | No schedule exists and the workflow architecture is undecided |
| [T-100](sources.md) | P2 | sources | open | The source registry is missing fields the design requires |
| [T-101](sources.md) | P2 | sources | open | Source quality is asserted per source and measured for none |
| [T-102](sources.md) | P2 | sources | open | Change-detection thresholds are provisional and say so |
| [T-103](sources.md) | P2 | sources | open | Provenance snapshots are not retained |
| [T-104](sources.md) | P2 | sources | open | Conditional requests are not implemented |
| [T-120](docs.md) | P2 | docs | open | The documentation set is a fraction of what is required |
| [T-121](docs.md) | P2 | docs | **done** | Nothing checks that documentation citations still resolve |
| [T-122](docs.md) | P2 | docs | open | The consumer contract is documented but nothing enforces it |
| [T-123](docs.md) | P2 | docs | open | Most acceptances cannot be run as written |
| [T-144](foundation.md) | P2 | foundation | **done** | 182 blacklisted URLs reached the output |
| [T-145](foundation.md) | P2 | foundation | **done** | Nothing enforced the rules the project had written down |
| [T-008](claims.md) | P3 | claims | open | Inbound connectivity is inconclusive and the instrument says so |
| [T-009](claims.md) | P3 | claims | open | Schedule delay and drop rates are documented but never observed |
| [T-010](claims.md) | P3 | claims | open | The reason for pinning actions to SHAs is asserted, not verified |
| [T-011](claims.md) | P3 | claims | **done** | Two reference documents were never read in full |
| [T-065](publication.md) | P3 | publication | open | Filenames, checksums and asset duplication are undecided |
| [T-105](sources.md) | P3 | sources | open | Sources named by the brief and by other aggregators are not in the registry |
| [T-106](sources.md) | P3 | sources | open | `hardcoded.txt` has no input file |

## Counts

**Counts:** 66 entries, 50 open, 0 blocked, 16 done

Derived from the rows above by `scripts/check-todo.py`, which fails a gate when
a number here disagrees with them. Do not edit them by hand.

| Priority | Open | Blocked | Done | Total |
| --- | --- | --- | --- | --- |
| P0 | 4 | 0 | 3 | 7 |
| P1 | 13 | 0 | 8 | 21 |
| P2 | 27 | 0 | 4 | 31 |
| P3 | 6 | 0 | 1 | 7 |
| **All** | **50** | **0** | **16** | **66** |

**Nothing is blocked.** The two entries that were -- [T-003](claims.md) and
[T-064](publication.md) -- were waiting on authorisation to create throwaway
releases here, and RULES 13.1 now grants it. A session that finds itself
blocked pivots (RULES 10.1); it does not stop.

## How the current ordering is derived

Four questions, asked in this order, because a later answer never outranks an
earlier one. This is the argument; [PROGRESS.md](PROGRESS.md) carries the
ordered work.

### 1. Would it publish something wrong while reporting success?

A wrong answer that exits 0 outranks a visible failure, because nothing in the
output says the wrong answer happened.

[T-021](measurement.md) is the worked example and it is why the question is
first: without an oracle, a silently broken probe marks the **entire dataset**
dead, and every number in the report would be internally consistent.
[T-002](claims.md) is the other shape of it -- a schedule that stops after 60
days of inactivity, publishing nothing, telling nobody.
[T-023](measurement.md) is the same failure at smaller scale: a yggdrasil
tracker addressed by hostname is currently classified clearnet and would be
recorded dead, which is the exact bug the two-axis model was built to prevent,
surviving inside the fix for it.

### 2. Is the project's justification unanswered?

[T-027](measurement.md) is the value gate, and reaching a negative conclusion
honestly is a **successful** outcome. Half of it is measured -- the aggregate
holds 1337 trackers against ngosang's 99, and one source alone contributes 995
unique URLs -- and the half that decides it, whether any of those are alive, is
not. Building further on an unjustified dataset costs more the longer it runs.

[T-001](claims.md) is P0 for the same reason from the other end: the plaintext
format is the primary deliverable for the primary audience, and **no torrent
client has ever been run against it**.

### 3. What unblocks a measurement?

An entry that cannot be measured cannot be closed, so the thing that unblocks a
measurement outranks the measurement. [T-020](measurement.md) gates
[T-027](measurement.md), [T-040](scoring.md) and most of `scoring.md`.
[T-003](claims.md) gates [T-064](publication.md) and needs a human, not work,
which is why it is raised now rather than when P4 starts.

### 4. What is cheapest to close in one pass?

Entries cluster by file because they share a fixture and a mental model.
`measurement.md` is the current cluster: T-020, T-021, T-024, T-025 and T-029
are one body of work, and splitting them across sessions means building the
same fixture three times.
