# 2026-09-08, pass 3: from the far end of the socket

**Lens:** *what would a tracker operator make of this?* --
[`../../docs/methodology/reviews.md`](../../docs/methodology/reviews.md), the
fourth lens, which exists because this project probes other people's servers
and RULES 4 is absolute.

**Subject:** everything this session sent to a third party. Not what it
intended to send -- what the run logs show it sent.

---

## The bill, itemised

`p0-ground-truth.yml` contacts **28 tracker endpoints per runner image** -- 11
UDP twice plus 6 HTTP -- so **56 per run** across the two images.

| run | trigger | tracker contacts | did it buy anything |
| --- | --- | --- | --- |
| `34205529114` | push `cf6b67c` | 56 | ⛔ **nothing** |
| `34206054314` | push `37529fa` | 56 | yes -- `_conditions.py` changed |
| `34206715133` | push `29c11ef` | 56 | yes -- this is T-033's proof on a real runner |
| `34210488915` | push `1305907` | **>=11, then cancelled** | ⛔ **nothing** |
| `34210496112` | dispatch | 56 | yes -- T-007's answer |
| `34207344996` | dispatch | **96** | yes -- the baseline census, T-034 |

**About 331 endpoint contacts. At least 67 of them -- one endpoint in five --
bought nobody anything.**

---

## Finding 1 -- an offline instrument fired a probe at seventeen operators

`34205529114` ran because `p0-ground-truth.yml` triggered on
`paths: experiments/**`, and the commit added
`experiments/27-value-gate.py` -- an instrument that **reads committed JSON and
opens no socket at all**.

⛔ **Fifty-six endpoint contacts, on two images, for a change that could not
possibly have affected what they answer.** From an operator's side that is
indistinguishable from a project probing them on a whim.

**Fixed in the same session.** The workflow now names the five instruments it
runs plus their shared modules and fixture, and the comment above the filter
says why a wildcard is the wrong shape here: *the cheapest request is the one a
path filter never fires*. RULES 15.2 already made request noise a correctness
rule rather than a tidiness one; this is the first time this project has
measured itself breaking it.

⚠ **The path filter now needs maintaining**, and that is the trade. A new
probing instrument whose path is missing runs only on dispatch -- a visible
failure. A wildcard that fires on everything is invisible and is paid for by
somebody else.

## Finding 2 -- eleven operators were contacted for a result that was thrown away

`34210488915` was triggered legitimately: it edited `p0-ground-truth.yml`, and
a workflow edit is exactly the change whose effect is only visible in a run.

Then, **twelve seconds later, this session dispatched `34210496112`** -- and
the workflow's `concurrency` group cancelled the in-flight run.

⛔ **It had already reached the trackers.** The cancelled run's log carries
`SUBJECTS  11 UDP trackers` on `ubuntu-24.04`. So eleven operators answered a
probe whose result was discarded, no artefact was kept, and nothing in this
tree is better for it.

⭐ **The concurrency group is not the defect -- it is doing its job**, which is
stopping two sweeps hitting one tracker at once. The defect is dispatching
into it without looking at what was already running. **This is the worst trade
available in this project**: cost borne entirely by somebody else, benefit
zero, and invisible unless you go and read a cancelled run's log.

**Not fixed in code, and deliberately.** Three routes were considered
(RULES 10.1a):

1. **`cancel-in-progress: false`** -- already set on the health sweep, and it
   is the wrong instrument here: it would queue the dispatch behind the push
   run, so the trackers get contacted twice instead of once.
2. **A guard that refuses to dispatch while a run is in flight.** There is no
   place to put it: the dispatch is a person or a session typing
   `gh workflow run`, not code this repository executes.
3. **Look first.** `gh run list --workflow "P0 ground truth" --limit 1` before
   dispatching, which costs one command.

Route 3 is the only one that reaches the actual failure, so it is written into
[`../../docs/security/remote-ops.md`](../../docs/security/remote-ops.md) as a
rule rather than left as a lesson somebody remembers.

## Finding 3 -- the DNS load was real and is not counted anywhere

The census instruments resolve **965 hostnames**, and `experiments/30` asks a
**second** resolver about every host the first could not answer for. Run twice
locally and once on two runner images, this session issued roughly **four
thousand DNS queries naming other people's trackers**.

⚠ **Nothing forbids that and nothing counts it.** RULES 15.2 bounds requests to
*upstreams and trackers*; a DNS lookup is neither, so the politeness budget is
silent on it. And `PROGRESS.md`'s open question 1 already records the related
trade -- BEP 34 lookups send tracker hostnames to a public resolver -- which
means the shape was known and its scale was not.

⭐ **From an operator's side a resolution is close to free**, which is why this
is a note rather than a finding with a fix. What is worth writing down is that
this session multiplied a load nobody had bounded, and that
[T-026](../../TODO/measurement.md)'s politeness budget should say whether DNS
is inside it or outside it, rather than leaving it unmentioned.

---

## What an operator would have no complaint about

* ⭐ **The census asked before it probed.** Of 99 baseline trackers, **3 were
  skipped** on a BEP 34 refusal or an undetermined lookup -- and an undetermined
  lookup skips, because a DNS failure is not consent. The operators who
  published a record got what they asked for, without contacting us.
* **Nothing announced.** There is still no announce code path to reach, and
  `--only-source` did not add one: it narrows *which* trackers are chosen and
  changes nothing about *how* they are contacted, which pass 1 checked by
  reading the callers rather than by trusting the description.
* **One connection per host, everywhere.** Not configurable, in either profile.
* ⭐ **Five URLs stopped being called dead.** `experiments/30` found this
  vantage cannot resolve `openbittorrent.com`, so a full sweep would have
  published one of the best-known public trackers as gone. The operator would
  never have known; [T-037](../../TODO/measurement.md) exists so it does not
  happen.

---

## What this pass did not look at

* **Whether the pinned 17 subjects still consent.** Their BEP 34 records are
  consulted at probe time by `_consent.py`, and this pass read the run logs
  rather than re-querying the records.
* **What upstreams were fetched.** The pipeline ran offline from committed
  fixtures all session; no source was re-fetched, so upstream load was zero.
* **The scheduled cadence.** There is still no `schedule:` trigger anywhere,
  so every contact this session made was one somebody chose to make. That is
  D7's and [T-026](../../TODO/measurement.md)'s question and remains open.
