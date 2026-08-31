# Review 5 -- the tracker operator

**Date:** 2026-08-31, **Standpoint:** somebody who runs one of the trackers in
this corpus, has noticed traffic from this project, and is deciding whether to
be annoyed. They will read the README, not the rules, and they will check
whether what it says is true.

**What I looked for:** a claim this project makes about how it treats other
people's servers that the code does not keep. The brief's own note that this
document *"has not been reviewed by anyone who operates a tracker"* is still
true -- this is the closest available substitute and it is not the same thing.

---

## Method

Not by reading the policy. By asking the code what it can actually emit, and
checking each README promise against it.

1. **What can this codebase send a tracker?** Build every request it can
   construct and look at the bytes.
2. **What can it not send?** Structurally -- is the prohibition a property of
   the code or a rule somebody has to remember?
3. **Every promise in the README's operator section**, checked against `src/`.

---

## The finding: an exclusion route promised in the present tense and not implemented

The README told operators:

> **BEP 34** -- publish a `BITTORRENT` TXT record on your tracker's hostname
> denying connections, and this project stops. Automatable, and it needs no
> contact with us at all.

**Nothing in `src/` reads a DNS TXT record.** `grep -rn "BEP 34\|bep_34" src/`
returns nothing. There is no BEP 34 code path at all.

This is the most serious thing in the session, because of *who* it is aimed at:
it is a commitment to a third party, in the document a third party reads, and
the third party is the one whose server we probe. Two aggravating details:

* It is the route offered **first**, and the only one that needs no contact --
  so it is the one an operator who does not want to talk to us would use.
* RULES 4.1 withdrew this project's descriptive-User-Agent requirement partly
  on the argument that *"BEP 34 achieves the same end far better"*. That
  argument only holds if BEP 34 is honoured. It was resting on a mechanism the
  project had verified in **somebody else's** code (`C-51`, `newtrackon/scraper.py:217`)
  and never written in its own -- and the sweep write-up listed it under
  *mechanisms adopted*, which overstated a decision as an implementation.

**What limits it:** no operator has been probed against the unkept promise. The
probe has never been pointed at the corpus, so today it is a documentation
defect. It becomes a conduct defect the first time a corpus probe runs.

**Fixed, in four places and one entry.** The README now says which route works
and which does not, in those words. RULES 4 says the same and adds that **no
corpus-wide probe runs until it is built**. The sweep and `C-51` are corrected
to distinguish adopting a decision from adopting code.
[T-032](../../TODO/measurement.md) is the implementation, **P0**, carrying the
two things newTrackon already paid for: use public resolvers rather than the
host's (its issue #316 -- Hetzner's internal resolvers did not follow CNAMEs and
opt-outs failed *silently*), and treat a DNS failure as `unknown` rather than
as consent.

---

## What held, and held structurally

**It cannot announce, and that is a property of the code.** The only datagram
this codebase can build for a UDP tracker is 16 bytes:

```
00000417271019800000000012345678
protocol_id=0x41727101980  action=0 (connect)  txid=random
```

There is no `info_hash` field, because there is nowhere to put one in 16 bytes.
`src/trackers/bep15.py` has no function that builds an announce -- adding one
would be a reviewable change to a named file, not a lapse. That is a materially
stronger guarantee than a policy.

**The HTTP side stops at scrape.** A default probe requests
`https://tracker.example/scrape`, derived per BEP 48, which the specification
itself says *"has no effect on a peer's participation in a swarm"*. And it
refuses to derive one where the convention does not apply, rather than guessing
an endpoint whose 404 would then be blamed on the tracker.

**It sends nothing else.** The full header set is two lines. No cookies, no
cache-busting parameter, no retry storm.

**A malformed info_hash cannot be sent by accident.** A 19-byte hash is refused
at construction: `info_hash must be exactly 20 bytes, got 19`.

**It does not adopt other people's opinions about you.** Of 346 upstream
exclusions, only *"requested by sysadmin"* and *"deprecated by owner"* (and
safety reasons) are enforced -- 15 entries. The 331 that are somebody's
measurement opinion are kept and flagged. If another aggregator decided your
tracker serves fake seeds, this project keeps you and checks for itself.

**There is no subprocess or shell anywhere in `src/`**, so nothing an upstream
sends can become a command.

## What I looked for and did not find

* **A retry loop that could hammer a slow tracker.** The UDP path sends one
  connect per attempt with a bounded count; there is no unbounded retry.
* **A code path that reads a real info_hash from anywhere.** `synthetic_infohash()`
  is the only source, and it is `os.urandom(20)`.
* **Concurrency against a single host.** Not implemented yet either way, and
  T-029 specifies per-host serialisation as non-negotiable in both execution
  profiles.
* **A published dataset naming any tracker as dead.** None exists.

## What this review did NOT establish

* **That an actual operator would accept any of this.** I am not one. The
  brief's own weakness -- *"this document has not been reviewed by anyone who
  operates a tracker"* -- is unchanged, and no substitute closes it.
* **That the politeness budget is respected**, because nothing computes it yet
  (T-026). The ceiling is stated and unenforced.
* **That probing does not perturb the subject.** RULES 2 requires checking
  whether observing changed the answer, and nobody has: a tracker that
  rate-limits after the first request answers the second differently. Recorded
  on T-029.
* **What we will actually identify ourselves as.** T-012 is unanswered, and
  `C-68` shows the closest production analogue impersonating qBittorrent on
  both identity axes. An operator reading this project's rules today cannot be
  told what the probe will send tomorrow, and the honest position is that it is
  being measured first.
