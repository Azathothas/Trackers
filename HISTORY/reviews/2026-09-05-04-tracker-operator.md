# 2026-09-05, review 4: read from the far end of the socket

The fourth lens in [`../../docs/methodology/reviews.md`](../../docs/methodology/reviews.md),
which says it is not covered by the other three and pays often here.

**The question:** on 2026-09-05 this project contacted **200 strangers' servers
for the first time**, plus 17 more through the ground-truth workflow. **What did
each of them receive, what could they tell about it, and what could they do
about it?**

---

## What they actually received

| | |
| --- | --- |
| a UDP tracker | one 16-byte BEP 15 connect, up to 3 attempts at 1.67 s. No infohash, no announce, nothing that could join a swarm |
| an HTTP tracker | one scrape, with a synthetic infohash corresponding to no content |
| every host | DNS TXT lookups **at a public resolver**, which the operator never sees |
| all of them | one probe per host at a time; never two at once |

⛔ **Nothing announced.** That is a property of the code rather than a policy:
`bep15.py` has no function that builds an announce, and review 2 confirmed the
only tracker sockets in the tree are the two gated probers plus the two
experiments now gated as well.

---

## Found: the README told operators to ask, and there was nobody to ask

**Severity: medium. It is a RULES 4 defect. Fixed.**

RULES 4: *"MUST honour any operator's request to be excluded, and
**documentation must say how to make one**."*

Earlier in this same session I added an exclusion section to the README to
satisfy that rule, and wrote:

> Asking also works and is honoured; `src/trackers/exclusion.py` is where a
> request lands.

⛔ **A request does not land there.** `exclusion.py` parses *other projects'*
blacklist files. Nothing in it receives anything from a tracker operator, and
`grep` for a contact route across `README.md`, `docs/` and `.github/` returned
**nothing at all**: no address, no issue template, no instruction.

So the DNS half of RULES 4 was implemented and documented properly, and the
asking half was **asserted to work while having no mechanism** -- inside the
change whose purpose was to satisfy that rule. ⚠ That is the shape RULES 9.1
names: a requirement quietly satisfied in a weaker version.

Fixed by saying what is true. Issues are enabled on this repository, so there
is a real direct route and the README now names it; the indirect route through
an upstream blacklist is described as indirect; and both are marked as needing
a human here to act, **which is the actual argument for preferring BEP 34** and
was previously stated as a preference rather than a reason.

---

## Held: an operator who refuses us is refused for the right reasons

Eight endpoints published a record that excluded us and all eight were skipped
without a socket. Read from their side, each refusal decoded to what the
operator wrote:

- three said "no trackers here" -- two as `BITTORRENT DENY ALL`, one as a bare
  `BITTORRENT`, and ⭐ **the bare one is the case that matters**: an operator
  who spells the denial the way the specification defines it, rather than the
  readable way, is refused by fewer implementations and would have been probed
  by ours a day earlier;
- five named ports other than the one a public list advertises for them, which
  usually means the list is stale rather than that the operator is hostile. We
  skip rather than "helpfully" retry on the advertised port -- their record
  says where their tracker is, not that they want our traffic redirected onto
  it.

⚠ **Three hosts were skipped because our own resolvers failed.** From their
side that is indistinguishable from being refused, and it costs them nothing;
from ours it is a missing measurement, correctly labelled `unknown`.

---

## Held: they can tell who we are, unless they run UDP

An HTTP operator reading their logs sees
`trackers/0.1 (+https://github.com/Azathothas/Trackers; tracker health probe)`
and can find this repository. **A UDP operator sees 16 bytes and cannot**, because
BEP 15 has no field for it -- which is why [T-012](../../TODO/claims.md) matters
and why RULES 4.1 withdrew the claim that a descriptive UA makes us
accountable. ⭐ **It never applied to UDP at all**, and UDP was the larger half
of what we contacted.

**What would have made this fire:** a probe carrying something identifying that
we did not intend, or a UA claiming to be a real client. Neither: the default
UA is descriptive, and no arm of T-012 has been run.

---

## Held: no operator will be probed again without somebody choosing to

`health-sweep.yml` has `workflow_dispatch` and no `schedule:`. **What would
have made this fire:** a cron trigger, or a probe reachable from `gate.yml`.
`grep -n "schedule:" .github/workflows/` returns only nothing for the sweep,
and `gate.yml` is offline by construction.

---

## What this pass did not look at

- **Whether 200 probes in 44 seconds is polite at the aggregate.** Per host it
  is one connection; across hosts it is 8 at once. The budget that would answer
  it is [T-026](../../TODO/measurement.md), unbuilt.
- **What an operator would think of being *listed*** rather than probed. This
  project publishes no dataset yet, and the question belongs with the first
  publication.
- **The one `rate_limited` response.** The tracker said slow down; we sent one
  request and stopped, so nothing was owed within the run. Across runs there is
  no history to honour it with, which is [T-040](../../TODO/scoring.md).
