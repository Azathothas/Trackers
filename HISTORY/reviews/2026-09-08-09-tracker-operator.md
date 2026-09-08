# 2026-09-08-09 the tracker operator's view

The fourth lens `docs/methodology/reviews.md` recommends when a change earns
it: *what would a tracker operator make of this?* This session opened more
sockets to other people's servers than any before it, so it earns it.

---

## What this session cost other people, counted

Derived from the committed results rather than from memory:

| what ran | tracker-facing contacts |
| --- | --- |
| `experiments/33`, two runs, direct and proxied | 46 |
| `experiments/26` rotation 0, corpus order | 197 |
| `experiments/26` rotation 0, live subjects | 32 |
| `scripts/probe-corpus.py --only-host`, four runs | 5 |
| `p0-ground-truth.yml`, two push-triggered runs | 68 |
| **total** | **348** |

Every one was a connect or a scrape. There is no announce code path, so no
operator saw this project join a swarm.

## Findings

### 1. Sixty-eight of the 348 bought nobody anything ⛔ recorded and mitigated

Two push-triggered P0 runs fired because `experiments/_conditions.py` is in
that workflow's path filter -- correctly, since every probing instrument
imports it -- and neither change could have altered a measurement. One was a
one-line timestamp helper.

⭐ **That is one contact in five, spent on nothing.** The mitigation is
sequencing rather than a filter change, and it is written where a session will
read it before editing that file, in `experiments/README.md`. The note is
deliberately **not** in `_conditions.py`, because committing it there would
have fired the job again.

`skip_tracker_probes` now exists so that re-measuring the runner's DNS costs
zero tracker contacts, and the census that produced this session's canonical
resolver figures used it: **two full-corpus DNS censuses on two runner images,
no tracker contacted**.

### 2. The identity experiment as specified would have broken RULES 4 ⛔ redesigned

T-012's design crosses four User-Agent arms with three `peer_id` values over
the whole HTTP corpus. Read from the far end of the socket, that is **four to
twelve requests arriving at one tracker within one run**, against a ceiling of
one per three hours that this project set for itself and published.

An experiment that breaks the politeness rule in order to measure whether the
project is polite is not a measurement an operator would recognise as good
faith. Redesigned to one arm per tracker per run, recovered across rotations.

### 3. The first identity run wasted 87% of its requests ⚠ fixed

174 of 200 subjects answered nothing. Those contacts were spent on trackers
that could not have expressed a preference either way, and the arms were
compared on eight informative rows. Drawing subjects from a sweep's live
trackers gave a better comparison at **16% of the load**, which is the rare
case where the polite choice and the one with more evidence in it are the
same choice.

### 4. Consent is asked for every contact, including the new routes ⭐ holds

Checked rather than assumed, because two new contact paths appeared this
session:

* `experiments/33`'s **proxied** arm consults BEP 34 before asking the proxy to
  fetch anything, and a denial or an undetermined lookup skips the tracker in
  that arm exactly as in the direct one;
* the **i2p gateway** route was measured against two non-tracker i2p sites
  rather than against a tracker, so nothing was contacted through an untested
  intermediary. And it stopped there: a `.i2p` name has no DNS record, so BEP
  34 cannot be consulted for those hosts at all, which is now the one open
  question for the operator rather than a decision taken quietly.

### 5. Two defects would have published a live tracker as gone ⭐ fixed

Both matter from the far end: an operator whose tracker is running does not
want it listed as dead by a monitor that never reached it.

* A host resolving into `0200::/7` was probed over clearnet and recorded
  `timeout`. Three of those is `dead`.
* `ipv6.tracker.harry.lu` answers `::1`, and the probe connected to **this
  machine** and recorded the reset as the tracker's answer.

## What this pass did not look at

* Whether any operator has in fact noticed this project. Nothing here reads
  another party's logs and nothing should.
* The BEP 34 records themselves, which the previous session's operator pass
  audited at `2026-09-05-04-tracker-operator.md`.
* Load on the operator-approved proxy, which is not a tracker but is somebody's
  server: 7 requests this session.
