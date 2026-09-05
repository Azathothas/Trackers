# HISTORY/reviews

What each deep review swept, from whose standpoint, and what it did not look
at.

[`../../docs/methodology/reviews.md`](../../docs/methodology/reviews.md) is the
specification: at least three per session, each asking a **different** question,
because one sweep written up three times finds only what the author was already
looking for. RULES 10.3 step 4 is the requirement.

⭐ **A pass that reported nothing was too shallow.** Where one genuinely found
nothing, its write-up says what would have had to be true for it to fire. That
sentence is the evidence the pass happened.

## 2026-08-31

Six passes, run as the rescue session's ending. The findings they produced are
in [`../corrections.md`](../corrections.md), and the entries they opened are in
[`../../TODO/INDEX.md`](../../TODO/INDEX.md).

| review | standpoint |
| --- | --- |
| [`2026-08-31-01-cold-start.md`](2026-08-31-01-cold-start.md) | a session handed one instruction, with no memory and nobody to ask. Where would it be stopped, misled, or forced to guess |
| [`2026-08-31-02-auditor.md`](2026-08-31-02-auditor.md) | ⭐ somebody who believes none of the documents and wants to re-run every load-bearing claim. Which claim's evidence has evaporated |
| [`2026-08-31-03-requirements.md`](2026-08-31-03-requirements.md) | somebody holding the retired design brief, checking clause by clause whether the tree contains what a coverage row says it does |
| [`2026-08-31-04-next-session.md`](2026-08-31-04-next-session.md) | the next session, trying to actually carry out the work order. Where does "start here" become a research problem of its own |
| [`2026-08-31-05-tracker-operator.md`](2026-08-31-05-tracker-operator.md) | ⭐ somebody who runs one of these trackers and has noticed the traffic. Which promise about their servers does the code not keep |
| [`2026-08-31-06-hostile-input.md`](2026-08-31-06-hostile-input.md) | somebody who controls an upstream list, and then the direction that gets forgotten: somebody who consumes what this project publishes |

## 2026-09-01

The adoption session's ending. Six passes: RULES 10.3 step 4 requires three,
and the operator asked for six.

| review | standpoint |
| --- | --- |
| [`2026-09-01-01-unrequested-change.md`](2026-09-01-01-unrequested-change.md) | ⭐ what changed that nobody asked to change. On an adoption this is the pass that matters most |
| [`2026-09-01-02-guard-mutation.md`](2026-09-01-02-guard-mutation.md) | can each check added here actually fail. Plant the defect and read the exit code |
| [`2026-09-01-03-claim-audit.md`](2026-09-01-03-claim-audit.md) | which sentence written this session is not backed by something on disk |
| [`2026-09-01-04-cold-start.md`](2026-09-01-04-cold-start.md) | a session handed only the rewritten router. Where does following it mechanically stop working |
| [`2026-09-01-05-tracker-operator.md`](2026-09-01-05-tracker-operator.md) | ⭐ somebody whose tracker this project probes, and whose users' passkeys it publishes |
| [`2026-09-01-06-measured-never-verified.md`](2026-09-01-06-measured-never-verified.md) | ⭐ which numbers have exactly one observation. The DNS finding disagrees with itself between two runs |
| [`2026-09-05-01-adversarial-sweep.md`](2026-09-05-01-adversarial-sweep.md) | ⭐ attack the code this session added, by running it rather than re-reading it. One raising probe destroyed every other measurement, and twenty tests could not see it because they shared the code's belief |
| [`2026-09-05-02-door-sweep.md`](2026-09-05-02-door-sweep.md) | ⛔ what other door reaches this code. Two instruments contact trackers with no consent check, and the workflow that runs them fired twice on the day the gate was built |
| [`2026-09-05-03-claim-audit.md`](2026-09-05-03-claim-audit.md) | which sentence is not backed by an artefact. Every stated test count was re-run rather than re-read, and one had drifted four commits after it was written |
| [`2026-09-05-04-tracker-operator.md`](2026-09-05-04-tracker-operator.md) | ⭐ read from the far end of the socket, after the first 200 strangers' servers were contacted. The README told them to ask and there was nobody to ask |
| [`2026-09-05-05-cold-start.md`](2026-09-05-05-cold-start.md) | a session handed only the router. The routing table reads as a start-of-session step, so the page describing this session's worst defect in advance was never opened |
