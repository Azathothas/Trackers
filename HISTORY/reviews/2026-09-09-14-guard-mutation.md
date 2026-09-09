# 2026-09-09-02 -- guard mutation

**"Can my new guard actually fail?"**
Lens 2 of [`../../docs/methodology/reviews.md`](../../docs/methodology/reviews.md).

⛔ **Every defect below was planted in the tree, the suite run, and the exit
code read unpiped.** The file was restored in a `finally` block whatever the
outcome, and `git diff --stat src/` confirmed the tree came back.

---

## Round 1 -- T-087's ceiling: 5 planted, 4 caught, 1 survivor

| planted defect | verdict |
| --- | --- |
| the ceiling removed: `plan` never holds anything | CAUGHT |
| the ceiling holds but never advances past a held slice | CAUGHT |
| an unreadable `last_seen` buys a probe instead of refusing | CAUGHT |
| an unreadable clock is guessed rather than raised | CAUGHT |
| the boundary comparison shifted: `gap <= interval - 1` | ⛔ **SURVIVED** |

⛔ **The survivor is a test gap and it is instructive.** `gap <= interval - 1`
and `gap < interval` are **identical over whole seconds**, and every instant
this project writes is whole seconds -- so the mutant is unreachable from a
sweep and invisible to a boundary test built from real-looking timestamps. It
is still a real behaviour change: a tracker at 10799.5 s would be contacted
half a second early. The boundary test now asserts a fractional gap, and the
re-run caught it. **5 of 5.**

## Round 2 -- the session's other new guards: 12 planted, 12 caught

| planted defect | suite | verdict |
| --- | --- | --- |
| a failed fetch records `0` entries instead of `None` | provenance | CAUGHT |
| `EMPTY` filed as a blind spot again | provenance | CAUGHT |
| `REJECTED` counted as knowing the listing | provenance | CAUGHT |
| a failing source no longer blocks a removal verdict | provenance | CAUGHT |
| the ring is unbounded | provenance | ⛔ **SURVIVED, then caught** |
| a duplicate instant recorded twice | provenance | CAUGHT |
| a corrupt header silently reinitialised | provenance | CAUGHT |
| a band narrower than its derivation | thresholds | CAUGHT |
| a derivation naming an instrument that does not exist | thresholds | CAUGHT |
| an unknown failure rate reported as `0.0` | quality | CAUGHT |
| a question fires on a single observation | quality | CAUGHT |
| `unique` and `corroborating` collapsed into one | quality | CAUGHT |

## ⛔ The survivor, and it is exactly the shape `reviews.md` warns about

> *"A scope rule needs a fixture, not a comparison. A check whose scope
> silently narrowed produces an identical number on a tree with nothing in the
> dropped scope to exercise it."*

`test_the_ring_never_exceeds_its_cap` asserted `len(ring) <= RING_SIZE` over a
fixture that built **62 observations against a cap of 240**. Deleting the slice
that bounds the ring changed nothing, because the fixture never reached the
thing being tested. The test builds `RING_SIZE * 2` now, asserts the fixture
**still** exceeds the cap (so it cannot silently stop testing it again), and
additionally asserts the newest observation survives and the lifetime counter
does **not** roll with the ring -- a long-dead source must not forget it ever
worked.

## ⭐ The negative case, which a passing mutation test hides

A guard that refuses everything looks identical to a good one under mutation.
Three checks assert the **permissive** direction and are the reason the mutation
results mean anything:

- `experiments/25`'s tier 0b: a loopback server that answers `200` and never
  upgrades **must not** be recorded as a WebSocket. Wired to a non-zero exit.
- `test_a_tracker_with_no_history_is_the_one_permissive_case`: a tracker nobody
  has contacted must **not** be held, or the sweep could never take a first
  measurement of anything.
- `test_a_question_is_never_raised_on_a_thin_sample`: one failed fetch is a
  100% failure rate and must raise nothing.

## ⚠ A test whose name claimed more than it checked

Found and repaired, in the class this lens names explicitly.
`test_two_runs_in_a_row_share_no_tracker` compared rotation `0` with rotation
`1` -- a property of two **integers** -- under a name that reads as a property of
two **runs**. It is `test_two_runs_in_a_row_share_no_slice` now, and the
run-level property it was mistaken for has its own class with the measured
instants in it.

## What this pass did **not** look at

- The pre-existing guards. Only what this session added or touched was mutated;
  the 2026-09-08 pass covered the sweep's earlier ones.
- `check-docs`, `check-markers`, `check-one-home`. They fired repeatedly during
  the session on real defects -- a missing README row, an unindexed module, a
  shell-unsafe placeholder -- which is evidence they work, but no defect was
  **planted** in them here.
- Concurrency. Nothing added this session runs in the thread pool.
