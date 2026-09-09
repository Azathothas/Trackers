# 2026-09-09-04 -- the tracker operator

**"What would a tracker operator make of this?"**
The fourth lens in
[`../../docs/methodology/reviews.md`](../../docs/methodology/reviews.md), and
this session earned it: it opened three new ways to contact somebody's server
and fixed a rule it had been breaking.

⛔ RULES 4 is absolute and is about other people's machines. This pass counts
every packet the session sent on purpose.

---

## What an operator saw from us today

| contact | count | why |
| --- | --- | --- |
| health sweep, slice 6 | 194 selected, **181 probed** | scheduled, one request each |
| health sweep, slice 0 | **193** | scheduled |
| `experiments/26` rotation 2 | **56** | one arm per tracker, D7 checked first |
| `experiments/25` wss | **10** | one handshake each, no announce |
| `experiments/35` i2p | **11** + 2 registries + 1 control | one request each |
| `experiments/36` yggdrasil | **1** subject, 2 runs | 4 pings and 2 TCP attempts each |
| jump services / eepsite control | 4 | **not trackers**, deliberately |

⭐ **Every capability question was answered against something that is not a
tracker.** The i2p router's readiness was tested against `i2p-projekt.i2p`, the
name-resolution failure shape against a name nobody owns, and yggdrasil's
routing against a peer's own overlay address. Spending an operator's request to
learn something about ourselves is the thing this lens exists to catch, and it
did not happen.

## ⛔ The breach this session found, and what an operator would have seen

**192 operators were asked twice in 98 minutes** on 2026-09-08, inside an
interval this project tells them is three hours. From the far end that is
indistinguishable from a badly-written scraper. It is fixed at the level that
matters -- the ceiling is read from what was recorded, so no clock arithmetic
can breach it again -- and ⛔ **the two observations stay in the published
history**, because deleting them would be tidying away evidence of our own
misconduct. RULES 3.9 forbids the recovery and the honesty forbids it twice.

## ⭐ What the identity measurement means for them, not for us

T-012 closed with the descriptive User-Agent answering **34 of 35**. Read from
the operator's side that is the better outcome: the string that names the
project and links to it is **not** costing us data, so there is no measurement
argument for hiding behind `qBittorrent/4.3.9` the way the closest analogue
does. An operator who wants to find us in a log still can.

## ⚠ Three things an operator might reasonably object to, recorded rather than
## dismissed

1. **The i2p `/a` trackers got a request to `/` they did not advertise.** Eight
   `.i2p` URLs have no derivable scrape endpoint, so reachability was measured
   against the destination's root path. It is one request, it is not an
   announce, and it is not a scrape -- but it is a request to a path the
   operator published nothing at. The alternative was leaving the category
   unmeasurable, and the trade is recorded on [T-039](../../TODO/measurement.md)
   rather than hidden.
2. **BEP 34 cannot be consulted for a `.i2p` name at all** -- there is no
   ordinary DNS record to hold the TXT. The operator ruling of 2026-09-08 is
   that the *asking* route suffices, and it does; but those thirteen operators
   have no automatable way to refuse us, and that asymmetry should stay visible
   rather than being filed as settled.
3. **`experiments/36` connects to eight public yggdrasil peers** who did not
   volunteer to carry a measurement of a tracker. It is the published
   public-peer list and that is what it is for, and it is still somebody's
   bandwidth.

## ⭐ What got politer

- The sweep now **skips** a tracker contacted inside its interval instead of
  relying on the rotation not to select it, and **advances** to a slice with
  work rather than spending the slot.
- `experiments/26`'s exclusion became an **interval** rather than a blacklist:
  a tracker contacted last week is a subject again, so the same evidence costs
  fewer distinct operators a first contact.
- The i2p run addresses **destinations** rather than names, which removed six
  requests that would have gone nowhere and been recorded as the tracker's
  silence.

## What this pass did **not** look at

- The reference corpus fetches. They are GitHub reads, not tracker contacts.
- The publisher's eight upstream fetches per run. They are lists, not trackers,
  and T-104's conditional requests already bound them.
- Whether any tracker rate-limited us. No `429` appeared in any result this
  session, but nothing actively looked for a pattern across runs.
