# Review 6, what was measured but never verified

**Date:** 2026-09-01. **Standpoint:** somebody looking only for numbers taken
once. Not numbers that are wrong, and not numbers with no instrument behind
them, which is review 3. **Numbers with exactly one observation.**

**What I looked for:** a figure this project would act on that no second run,
no second image and no second source has ever confirmed. RULES 2's own
instruction is to run the control twice, and the reason this pass is separate
from the claim audit is that a single-sample number passes every check a claim
audit runs: it has an instrument, it has conditions, and it is exactly what the
instrument said.

**What I did not look at:** the corpus figures. They come from a deterministic
offline census against pinned fixtures, so repetition adds nothing;
[`../corpus-baseline.md`](../corpus-baseline.md) owns them.

---

## Findings

### 1. ⭐ The DNS divergence figure disagrees with itself, and the disagreement was one run away

`experiments/04` was run twice on 2026-08-31, forty minutes apart, on two
images each.

| run | ubuntu-24.04 | ubuntu-22.04 |
| --- | --- | --- |
| `33383156641` | agree 13, both_failed 3, **divergent 1** | agree 13, both_failed 3, **divergent 1** |
| `33383406869` | agree 14, both_failed 3, divergent 0 | agree 14, both_failed 3, divergent 0 |

The divergent host was `tracker.torrent.eu.org`: `89.234.156.205` from the
local resolver, `91.216.110.52` and `.53` from both public resolvers, then
agreement on the next run.

⛔ **The old baseline reported "0 divergent of 17" and carried T-007 for being
thin.** It was thinner than that: with two runs available, one of them
disagrees. Both are now recorded together in `C-06` and in `TODO/foundation.md`
with the instruction that neither may be quoted without the other.

⭐ **This is the pass paying for itself.** The second run only exists because a
defect in a different instrument forced one, and without it the project would
have replaced one single-sample figure with another.

### 2. The BEP 15 connect rate is the one figure with four observations, and it still hides a constant

10/11, 9/11, 10/11, 10/11 across two images and two rounds each.

⚠ **The denominator is misleading and no amount of repetition would have shown
it.** One of the eleven targets has no IPv4 address, and this vantage has no
IPv6 egress, so it can never reach the connect rung from here. **10 is the
ceiling.** The instrument records it as `no_ipv4_address` rather than as a
failure, which is correct, and the prose that quoted "9/11" was reading the
denominator as if all eleven were reachable.

The one run that scored 9 is also informative: its histogram puts the eleventh
target at `datagram_sent`, meaning the packet left and nothing came back. That
is a timeout, not a refusal, and the two are different facts about a tracker.

### 3. Numbers that are still single-observation, listed rather than fixed

| figure | observations | why it is not repeated |
| --- | --- | --- |
| `C-02` inbound connectivity | 2 images, 1 run, and **inconclusive by construction** | a failed hairpin cannot distinguish blocked inbound from a NAT that does not hairpin. Repetition does not help; a prober outside the runner does. `T-008` |
| `C-64`, a public intermediary refusing this project's User-Agent with HTTP 420 | **1**, observed in passing from one sandbox | it is the strongest signal on the project's biggest open question and it has never been reproduced. `T-012` is the entry that would |
| the marker density ceiling of 30 per 100 lines | 3 trees, 1 measurement each | it is a threshold rather than a measurement, and the trees it was set from are not this one |
| the 6 private-tracker credentials | 1 corpus snapshot | the corpus is re-fetched from upstreams that change, which is why the check holds a ceiling rather than pinning the six strings |

⚠ **`C-64` is the one that would change a decision.** Everything else on this
list is either structurally unrepeatable or not load-bearing.

### 4. A number nobody has ever taken, and the project acts as if they had

⛔ **Nothing has been measured about what happens when this project's plaintext
is handed to a real torrent client.** One client's parser was read. The README
says so under known weaknesses and `T-001` is the entry, and it is worth
repeating here because it is the largest gap between what the project publishes
and what it has observed.

---

## What would have made this pass fire harder

⚠ **A tree with more measurements in it.** Most of this project's numbers come
from instruments that have run two or four times because CI runs them on two
images, which is a genuine second observation and is why finding 1 was visible
at all. ⭐ The pass that will fire hardest is the one run after P2 emits health
records, when there will be figures with thousands of samples and a real
question about which of them mean anything.
