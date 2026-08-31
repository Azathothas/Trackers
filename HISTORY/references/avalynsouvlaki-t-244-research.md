# `AvalynSouvlaki/T-244-RESEARCH` -- adopt (methodology), and it bears on `C-43`

**Commit:** `88a8410`, **Licence:** Unlicense (`tree/LICENSE`),
**Captured:** 2026-08-31, **Corpus:**
[`references/AvalynSouvlaki__T-244-RESEARCH/`](../../references/AvalynSouvlaki__T-244-RESEARCH/)

Named as this project's documentation quality bar since the first draft, and
never opened until now (T-011, `C-29`). It is a survey of nine
browser-fingerprinted HTTP clients -- **which is the `C-43` question**, not a
tangent: `C-43` lists `apify/impit`, `h4ckf0r0day/obscura` and
`0x676e67/wreq-util` as candidate 401/403 mitigations, and this document
measures all three.

## What this reading did NOT establish

* **Nothing was built or run here.** Its Rust probes were not compiled.
* **Its measurements are one Linux x86_64 box against localhost only**, and it
  says so in a box above its own recommendation. macOS, Windows, HTTP/3 and
  any live origin are untested by it.
* **Its tracker was not read.**
* **Passes taken: two.** WHAT and MECHANISM.
* **It does not settle `C-43` for us**, because this project has measured
  **zero** 401/403 across its source fetches. It changes what we would reach
  for *if* that stops being true.

## Verdict: **adopt** for method; **filed** against `C-43`

### 1. The revision table is the honest-error-estimate, done properly

Revision 2 opens with a table of **four claims revision 1 got wrong**, each
with a severity column, and one of them *reverses* a stated weakness of the
crate it recommends:

> r1 said impit emitted a fixed extension order, a potential tell. **Wrong.**
> Across 6 captures: 3 distinct extension orders, 3 distinct JA3 hashes, 1
> stable JA4. impit *does* shuffle. **High** -- r1 wrongly penalised impit.

And the closing move: *"The r1 recommendation stands, but it is now a trade
with open eyes rather than a clean win."*

**Consequence.** A corrections table needs a **severity** column, because "I
got a hash format wrong" and "I recommended against the right tool" are not the
same finding. `HISTORY/corrections.md` gains one.

### 2. Choose a metric that is stable under changes you do not care about

It asserts on JA4 and **records JA3 without asserting on it**, because JA3
preserves wire order and flakes on a reordering that means nothing. The same
choice is visible in `Azathothas/bit-cli`'s captured fingerprints:

> `ja3` is recorded and never asserted, because it preserves wire order and
> flakes.

**Consequence.** This is the rule TEMPLATE's `experiments.md` states abstractly
and neither of our two documents carried. It is now RULES 2. Concretely here:
an `--expect` assertion on a tracker's exact `interval` value would flake for
reasons we do not care about; an assertion that *some* interval was stated does
not.

### 3. The oracle is a capture server, not the subject's self-report

`research/tlsprobe/` is a TLS-terminating capture server with its own CA; every
candidate is pointed at it and the fingerprint is read **off the wire**. Nine
libraries' documentation claims were not reported; nine libraries' behaviour
was.

It also records that **observing changed the answer**: disabling certificate
verification so the probe could terminate the connection *also changed the
client's advertised algorithms*, so the field had to be captured passively
instead.

**Consequence.** Direct confirmation of the design of `tests/fake_tracker.py`
-- our oracle is a tracker we control, and the probe is measured against it
rather than against a live third party. The perturbation point is the one we
have not asked: **does our own probe change what a tracker answers?** A tracker
that rate-limits after the first request answers the second differently.
Recorded on T-029.

## Filed against `C-43` -- what to reach for, and only if measurement says so

If a source ever does refuse this project, this document is the shortlist and
the argument, so no future session re-derives it:

* **`impit` (Apache-2.0) is the only one of the nine that is not BoringSSL**,
  and therefore the only one that does not mean two TLS stacks in one binary.
* **Its HTTP/2 fingerprint was measured wrong and profile-invariant**, root
  cause found: its `h2` patch silently does not apply. A two-line fix in a
  fork.
* Five of the nine depend on `wreq`/BoringSSL and inherit that problem.

**None of this is actionable here today**, and adopting any of it would break
D1 (standard library only) for a problem this project does not have. It is
filed so the decision has evidence attached rather than a crate name.

**But `C-64` moved the prior.** On 2026-08-31 a public intermediary refused
this project's descriptive User-Agent with HTTP 420 and accepted `curl/8.5.0`
for the identical request in the same second. That is not a tracker and does
not settle `C-56`. It does mean "nobody refuses us" is no longer the measured
position it was, and T-012 is the entry that finds out.

## Refused

* **Its recommendation.** A Rust crate cannot enter a standard-library-only
  Python project (D1, RULES 12), and nothing here has yet been refused by a
  source.
* **Its numbers.** One Linux box, localhost only, by its own statement.
