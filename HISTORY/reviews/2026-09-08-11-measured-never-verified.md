# 2026-09-08-11 measured but never verified

*Which numbers have exactly one observation, and which have none that anybody
can re-take?* The lens `2026-09-01-06` used, aimed at a session that produced
more measurements than any before it.

Six claims audited. Two findings, and one of them is a `VERIFIED` row resting
on a transcript.

---

## The audit

| claim | observations | re-runnable? |
| --- | --- | --- |
| 6 of 16 IPv6-only alive | **2** runs, 12 minutes apart, **identical sets** | yes, `experiments/33` |
| 0 of 243 rescued by a public resolver | **2** runs agreeing | yes, `experiments/30` |
| runner rescues 3 and 1 | 1 workflow run, 2 images | yes, and see below |
| corpus hosts answer `0.0.0.0` / `::1` | 2 runs, plus a live probe for the `::1` host | yes, and a test drives both |
| descriptive UA answered 15 of 15 | **1** rotation, 1 day, 15 subjects | yes, and the entry says **no verdict** |
| `C-73`: the relay has IPv6 egress | **1** ad-hoc session | ⛔ **no** |

## Findings

### 1. A `VERIFIED` row rested on a transcript ⛔ fixed

`C-73` -- *the operator-approved read proxy has IPv6 egress* -- is what
`experiments/33`'s whole proxied arm depends on, and it is the reason the
IPv6-only category is reachable from CI at all. It was marked `VERIFIED` on the
strength of three fetches run inside this session's conversation.

⛔ **RULES 1.3: never mark a row verified without a committed command that
re-runs it.** RULES 1.1 is blunter: a number in a transcript is unrepeatable
the moment the session ends.

The row's `verify by` column pointed at the proxied arm, which cannot serve:
that arm fetches **trackers** through the proxy, so a failure there cannot
separate *"the proxy has no IPv6"* from *"the tracker did not answer"*. That is
the absence-is-not-a-zero problem inside a control.

**Fixed**: `experiments/33` grew a tier-1 control that fetches an IPv6-only
subject **and** a dual-stack subject through the proxy and reports both, so a
proxy that lost IPv6 and a proxy that broke entirely are distinguishable. Run
now: `IPv6-only subject through the proxy: 200; dual-stack control: 200`.

### 2. The i2p gateway finding is an observation, not a measurement ⛔ labelled

T-039 records that the `.i2p.to` inproxy resolves and returns 503. Nothing in
the tree re-runs it, so by RULES 2 it is a transcript claim sitting in an
entry, and the next session would have to take it on trust or re-derive it.

**Labelled as an observation**, with the exact command written into the entry.
⭐ **Not turned into a numbered instrument**, deliberately: an experiment whose
subject is permanently 503 measures nothing, and a number that is never reused
is a citation somebody has to keep meaning something. It becomes an instrument
when a gateway answers.

### 3. The runner's resolver figures disagree between two runs ⭐ already caught

Run `34210496112` reported 3 rescued on `ubuntu-24.04` and 2 on `22.04`, with
`tracker.parrotlinux.org` among them. Run `34235047982`, two hours later,
reported 3 and **1**, with `w.wwwww.wtf` in place of `parrotlinux`.

Not a new finding -- `corpus-baseline.md` already says *"a resolver census is a
measurement of a moment"* -- but it is the strongest evidence in the tree that
the sentence is not boilerplate, and this pass is where it gets checked rather
than assumed. Both runs are committed; neither is called canonical over the
other.

### 4. The IPv6 liveness set is stable across two runs ⭐ holds

The two committed runs of `experiments/33` name the **same six** live URLs, 12
minutes apart, with both controls passing in each. That is the one number this
session produced with genuinely independent agreement.

⚠ Two runs twelve minutes apart is not two days apart. What would make this
fire is a third run tomorrow disagreeing, and nothing schedules one.

## What this pass did not look at

* Numbers from earlier sessions, audited by `2026-09-01-06` and
  `2026-09-08-02`.
* Whether the instruments measure the right thing, which is the controls' job.
* The identity experiment's rates, which are single-observation by construction
  and which the entry and `C-56` both say do not support a verdict -- there is
  nothing here to catch that they have not already conceded.
