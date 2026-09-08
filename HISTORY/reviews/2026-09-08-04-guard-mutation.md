# 2026-09-08, pass 4: the guard mutation

**Lens:** *can my new guard actually fail?* --
[`../../docs/methodology/reviews.md`](../../docs/methodology/reviews.md) lens 2.

**Subject:** every guard this session added, and specifically the five
`--expect-*` flags that had only ever been observed **passing**.

---

## The enumeration

Fourteen guards were added or widened. Nine had already been planted against
and seen to refuse, at the time they were written:

| guard | defect planted | result |
| --- | --- | --- |
| `TheSweepScriptReportsTheCorpusItSampledFrom` | hand `sweep()` the sample | exit 1 |
| `--only-source` on an unknown source | a source that contributed nothing | exit 2, wrote nothing |
| `check-no-third-party-imports` collision | a colliding module on `PYTHONPATH` | exit 1 |
| `check-docs` inventory drift | an unlisted module under `src/trackers/` | exit 1 |
| `check-todo` stale `(planned)` | the marker restored on T-027 | exit 1 |
| `state.py` corrupt-file recovery | `except CorruptState: return {}, []` | 2 tests |
| `state.py` new-tracker rate | start `ewma` at 0.0 | 1 test |
| `state.py` success accounting | count every observation as a success | 4 tests |
| `state.py` daily cap | cap without sorting first | 1 test |
| `bep34` answer-type check | stop comparing the echoed type | 1 test |
| experiment 23 `--expect-all` | `render_plaintext` emits a blank line | exit 1 |

⚠ **Five had not been.** They had passed, repeatedly, and passing is not
evidence about a guard.

---

## Finding 1 -- a guard that "fired" by crashing

⛔ **`experiments/27-value-gate.py --expect-answered` did not fail. It
raised.**

Driven against a health record with an empty `trackers` list -- the exact
condition the flag exists for -- it exited **1**, which is the expected code,
having printed a **`KeyError: 'bar_1_count'`** traceback.

**How it hid.** `judge()` correctly returns a verdict with no bars when an arm
has no records, because there is nothing to compute a bar from. The report then
printed `verdict["bar_1_count"]` unconditionally. A crash and a measured
expectation failure both exit 1, so every check of the exit code agreed with
every check of the exit code, and nobody looked at the output.

⭐ **This is the project's own central distinction turned on itself.**
`p0-ground-truth.yml` carries a step whose comment says *"a crash and a
measured failure are not the same fact"*; the experiments' exit vocabulary says
`1` means *measured and an expectation failed*. This one meant *the reporter
broke*.

**Fixed.** An unanswerable gate now prints `VERDICT: UNANSWERABLE` with the
reason, exits **0** without the flag and **1** with it, and reports no bar
because there is none to report. Both directions verified.

## Finding 2 -- two flags have no defect that can be planted from here

`30 --expect-no-mass-divergence` fires when more than half the hosts asked
resolve for a public resolver and not for ours. `32 --expect-reported` fires
when there are fewer than two vantage addresses or no tracker assessed by both
sides.

⚠ **Neither can be tripped without fabricating evidence**, and fabricating a
result file to make a guard fail would make the guard's next pass meaningless.
So both are recorded here as **observed in the passing direction only**, which
is what the lens asks for when a pass genuinely cannot fire: *what would have
had to be true.*

* `30` would fire if this vantage's resolver failed on more than half of the
  239 hosts it already cannot answer for. At the measured rate -- 11 of 244
  here, 3 of 239 on a runner -- that is a resolver outage, not a drift.
* `32` would fire if the committed results stopped carrying `public_ipv4`, or
  if every run came from one address. It reads five addresses today from runs
  nobody arranged for diversity.

⭐ **Both are cheap to make provable later** and neither is worth faking now:
`30` needs an injectable resolver, which `experiments/30` does not have because
it uses the production one deliberately.

## Finding 3 -- one guard fired on the right outcome for the wrong reason

`28 --expect-crosscheck` was planted against by replacing the oracle's `all`
set so that `live` was no longer a subset of it. It exited 1 -- correctly --
but the message it printed was *"no tracker was assessed by both sides"*, not
the subset violation.

⚠ **The planted defect removed the shared trackers as a side effect**, so the
first of the flag's two conditions fired before the one under test. The guard
works; **this pass did not prove the condition it set out to prove**, and that
is recorded rather than counted as a pass. Proving the subset condition alone
needs an oracle snapshot where `live` has a member `all` does not, which is a
fixture this experiment does not carry.

---

## The negative direction

The lens requires it: *a guard that refuses everything is as useless as one
that refuses nothing.* Every guard above was also run against the **real**
evidence and exited **0** -- including the three re-proved here, and including
experiment 27 after the fix, which exits 0 on the committed records and 0 even
without the flag on empty ones.

---

## What this pass did not look at

* **The nine guards proved when they were written.** Re-planting them would
  re-run a test that had already been observed failing.
* **Guards older than this session.** `check-citations`, `check-markers`,
  `check-no-secrets` and the P1 suite were not re-planted against; the
  2026-09-05 passes cover that tree.
* **Whether a guard checks the right thing.** A guard can be perfectly
  falsifiable and still assert the wrong property. That is lens 3's question
  and pass 2 asked it of the numbers, not of the guards.
