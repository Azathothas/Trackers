# 2026-09-05, review 1: attack the code this session added

**The question:** the session added a DNS client, a concurrency runner, a
credential refusal and a live probe of other people's servers. **Which of them
fails in a way its own tests would not notice?**

⭐ **The standpoint is an attacker of my own work, not a reader of it.** A
review that re-reads code alongside the tests written for it re-derives the
author's assumptions. So each subject here was *executed* against an input its
tests do not use, and the finding is what the code did, not what it looked like
it would do.

Scope: `src/trackers/bep34.py`, `src/trackers/sweep.py`,
`src/trackers/exclusion.py`, `scripts/check-no-secrets.py`,
`scripts/probe-corpus.py`, `.github/workflows/health-sweep.yml`.

---

## Found: one raising probe destroyed every other measurement

**Severity: high. Fixed in the same change as this review.**

`sweep()` ran `list(pool.map(run_one, chosen))`. `run_one` called `probe_fn`
with no guard, on the strength of `probe`'s docstring: *"Never raises; every
failure becomes a recorded result."*

Attacked by handing the sweep a prober that raises on one host of six:

```
THE WHOLE SWEEP DIED: RuntimeError: a prober defect
-> one broken probe loses every other tracker's measurement
```

**That is RULES 3.8 with the word "source" swapped for "tracker":** one failing
tracker must not fail the others. On the 200-tracker run it would have
discarded 199 good measurements, and on a full sweep 1326.

⭐ **The interesting part is why no test caught it.** Twenty tests exercised
this function and every one of them passed a `probe_fn` that returns. The
tests were written from the same belief as the code -- that the probe does not
raise -- so they could only ever confirm it. **A promise is not a mechanism**,
and `probe()`'s own promise is thinner than it reads: `probe_http` has a
catch-all, `probe_udp` does not, and neither does the BEP 34 consultation that
now runs before both.

Fixed by recording `PROBE_ERROR` for the tracker that raised and continuing.
`health_state` maps that to `error`, which is the state that exists precisely
so a broken probe is never published as somebody else's outage.
`test_one_broken_probe_does_not_lose_the_other_measurements` is the regression,
and it fails against the old code.

---

## Held: the BEP 34 gate cannot be reached around

**What would have had to be true for this to fire:** a path to a socket that
does not consult `bep34`, or a way to make the consultation return `ALLOW`
without asking.

Checked three ways rather than one:

1. **Both public probers, not the dispatcher alone.** `probe_udp` and
   `probe_http` each consult before resolving. This was the review's first
   finding *during* the work rather than after it: the oracle tests call the
   probers directly, so gating only `probe()` would have left two ungated doors
   into the same action.
2. **The port the gate checks is the port the prober opens.** `effective_port`
   is the single definition and both read it.
   `test_the_gate_checks_the_port_the_probe_would_contact` fails if they drift.
3. **No switch.** `Bep34Config` selects *who is asked*; nothing selects
   *whether*. Mutating `_consult_operator` to always allow fails 5 tests.

⚠ **One real hole is left open and is recorded rather than closed:** a corpus
URL naming a host by IP literal has no name to look up, so a denial published
on the name does not protect it. It is on T-032 and belongs with T-031's
resolved-address work.

---

## Held, after being wrong once: the credential refusal

The first version keyed the refusal record on the **masked** URL, which
collapsed two people's passkeys on one endpoint into one row: seven refused,
six recorded. The count was wrong before anybody read it.

⭐ **A checker that measured the same thing twice is what caught it** -- the
count of removed entries disagreed with the count of report lines. Keyed on the
raw URL and rendered masked, `test_the_refusals_are_counted_per_url_not_per_masked_string`
is the regression and it fails if the collision stops being exercised.

Attacked again afterwards, on the narrowing that lets the tests carry
credential-shaped strings: a synthetic vector at the start of a line decided
the verdict for a real credential later on the same line, because the check
used `search`. That is the allowlist-hides-the-banned-thing row in
`forbidden-patterns.md`, reintroduced by a fix. It reads every match now.

---

## Held: nothing published claims more than it measured

The sweep's own output was read rather than trusted:

- `dead` is **0** and structurally cannot be otherwise from one sweep --
  `MIN_SAMPLES_FOR_DEATH` is 3 and `sample_count` is 1 per record.
- The 12 `unmeasurable` decompose into **8 operator refusals, 2 structurally
  unsupported and 2 `no_usable_address`**, and the refusals were checked one by
  one against the records: every one names a real published record and the
  reason it decided.

  ⚠ **The first draft of this line said "2 structural and 10 operator
  refusals", which was wrong**, and it is left visible rather than quietly
  corrected. I wrote it from memory of the failure counts instead of grouping
  the records by state, and `no_usable_address` -- an IPv6-only host this
  vantage cannot use, which is `C-04` -- also lands on `unmeasurable`. **A
  review pass asserting that nothing claims more than it measured is exactly
  where an unbacked number should not have survived**, and the only reason it
  did not is that the number was re-derived before this file was committed.
- ⚠ **`live: 25` of 200 is the number most likely to be misread**, so
  `PROGRESS.md`, the entry and this review all say the same thing: it is one
  datacenter, IPv4 only, on one day, and it is not a liveness rate.

---

## What this review did not cover

- **The DNS client against a hostile *real* resolver.** Everything hostile was
  the loopback oracle. A real resolver that lies differently is not modelled,
  and `T-007` is where resolver divergence belongs.
- **The sweep under a real deadline expiry.** The deadline tests inject a
  clock. A real 900 s expiry has never been observed; the one live run
  finished in 44 s and reported `not_reached: 0`.
- **Whether probing changed the answer.** RULES 2 asks and T-029's entry
  records that the oracle can be told to rate-limit and currently is not. One
  `rate_limited` appeared in the live run, which is a tracker saying so rather
  than us measuring it.
