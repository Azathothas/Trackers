# Corpus baseline -- the numbers, and the command behind each one

**Every corpus figure this project quotes is defined here and nowhere else.**
Other documents cite this file rather than restating a count, for the same
reason `scripts/check-todo.py` derives the entry counts instead of trusting
them (RULES 3.11).

**Taken:** 2026-08-31, **Inputs:** the committed fixture snapshots, not the
network, **Reproduce:** the commands below, on any host, offline.

---

## Why this file exists

Three mutually contradictory sets of corpus numbers were in circulation across
`TODO/`, `HISTORY/` and `src/` before 2026-08-31, and **none of them matched
the committed instrument output**:

| where it appeared | distinct | http | https | udp | ws/wss |
| --- | --- | --- | --- | --- | --- |
| every committed run of `experiments/19` | **1346** | **723** | **251** | **362** | **10** |
| `HISTORY/claims.md`, `HISTORY/corrections.md` | 1510 | 780 | 260 | 457 | - |
| `TODO/PROGRESS.md` | 1510 | 946 | 254 | 448 | 17 |

The second set is internally plausible and unsourced. The third does not sum to
its own total (946 + 254 + 448 + 17 = 1665, not 1510). Eight committed result
files, spanning two days, all said 1346.

The repair was to delete the prose figures and re-read the instrument. RULES
2.1 is the rule that came out of it.

---

## The two corpora, which are not the same corpus

Conflating these produced part of the damage above, so they are named
separately and never averaged.

### A. The census corpus -- 16 source files

What `experiments/19-scheme-census.py` reads: every list this project *might*
consume, including derivative lists (`ngosang_best`, `ngosang_udp`,
`newtrackon_live`, `xiu2_best`, `pkgforge_all`, ...) that are copies or subsets of
a primary. It answers "which schemes and networks occur in the wild", so
breadth is the point.

```bash
python3 experiments/19-scheme-census.py --offline
```

| | |
| --- | --- |
| source files read | **16** |
| distinct URLs in the union (the blacklist is **not** in the union) | **1346** |
| transports | `http` **723**, `https` **251**, `udp` **362**, `wss` **10** |
| networks | `clearnet` **1333**, `i2p` **13** |
| transport x network | `http\|clearnet` 712, `http\|i2p` 11, `https\|clearnet` 251, `udp\|clearnet` 360, `udp\|i2p` 2, `wss\|clearnet` 10 |

**There is no bare `ws://` in the union.** The corpus's only `ws://` URL is a
*blacklisted* one, in `ngosang/blacklist.txt`. Any statement that "`ws` occurs
once in the union" is wrong in both directions: it occurs zero times there, and
`wss` occurs ten.

### B. The pipeline corpus -- 7 tracker sources plus 1 exclusion list

What `scripts/generate.py` actually consumes, through `src/trackers/registry.py`
and the production parser. It answers "what do we publish", so correctness of
identity is the point.

```bash
python3 scripts/generate.py --offline
```

| | |
| --- | --- |
| registry sources | **8** -- 7 tracker sources + `ngosang_blacklist` (role: exclusion) |
| distinct URLs across the 7, after `normalize.parse` | **1345** |
| duplicates removed | **272** |
| rejected lines | **3** -- a stray `"` leaked by an HTML scraper and two `|` in query strings; none is a URI (RFC 3986) and all three used to reach the published plaintext |
| removed by an *enforced* exclusion | **8** |
| refused for carrying a private-tracker credential | **7** -- six distinct credentials, one tracker listed twice (`C-70`, T-107) |
| **accepted trackers published** | **1327** |

**The three rejections are the point, not a rounding.** Until 2026-08-31 the
character check did not exist and all three reached the published plaintext --
which is the format the README tells consumers to `curl | client`, and `"` and
`|` are both shell-significant in exactly that idiom. Review 6 found them by
running the emitted file through RFC 3986's character set rather than by
reading it. The count moved 1337 -> 1334 and the rejections are returned with
their reasons, so the disappearance is explainable (RULES 3.10).

**1334 -> 1327 on 2026-09-05**, when T-107 began refusing the seven URLs that
carry somebody's passkey. That is the largest deliberate subtraction this
project has made from its own output, and it is the one most worth making:
those seven rows were never usable by a consumer, and publishing them handed a
stranger's credential to everybody who read the list. Each is named, with its
reason and with the credential removed, in the run report's *Refused entries*
section.

Upstream exclusion classes in `ngosang/blacklist.txt` (346 entries):
`honour` **9**, `safety` **6**, `opinion` **331**. Enforced: **15** (operator
request + safety). Kept and flagged: **331** (somebody else's measurement --
RULES 15.3).

### Why A and B differ by exactly one

`1346 - 1345 = 1`, and the one is nameable. Of the eight derivative sources in
the census, seven contribute nothing outside the primaries. `pkgforge_all`
contributes **1 URL of its 1165** that is in no primary source:

```
derivative_orphans:  pkgforge_all  {'not_in_any_primary': 1, 'total': 1165}
```

That single URL is the entire independent content of the closest prior art.
It is also the sharpest available statement of what "concatenation without
measurement" produces, and it belongs in the value-gate argument
([`gates.md`](gates.md), T-027).

Restricting the census to the same 7 sources the pipeline reads gives **1345**,
identical to the production parser -- so the two parsers agree on this corpus,
and the difference above is source selection, not parsing.

---

## Per-source entry counts

From the census, `--offline`. `unique` is against the other **primary** sources
only; comparing a source against its own downstream copy answers no question
(`C-52`).

| source | role | entries | unique among primaries |
| --- | --- | --- | --- |
| `desirefire_all` | primary | 1091 | **995** |
| `pkgforge_all` | derivative | 1165 | - (1 not in any primary) |
| `newtrackon_all` | primary | 261 | 146 |
| `xiu2_all` | primary | 150 | 8 |
| `ngosang_all` | primary | 99 | 2 |
| `newtrackon_live` | derivative | 79 | - (0) |
| `xiu2_best` | derivative | 77 | - (0) |
| `newtrackon_stable` | derivative | 53 | - (0) |
| `ngosang_udp` | derivative | 48 | - (0) |
| `ngosang_http` | derivative | 37 | - (0) |
| `ngosang_best` | derivative | 20 | - (0) |
| `ngosang_https` | derivative | 14 | - (0) |
| `ngosang_i2p` | primary | 13 | 13 |
| `ngosang_ws` | primary | 3 | 3 |
| `ngosang_yggdrasil` | primary | 1 | 1 |
| `ngosang_blacklist` | blacklist | 346 | *excluded from the union* |

`ngosang/trackers_all.txt` is exactly `udp` 48 + `http` 37 + `https` 14 = 99,
so **every `ws`, `i2p` and `yggdrasil` entry -- 17 trackers -- is absent from the
list most consumers take, and the file does not say so.**

Blacklist transports: `http` 208, `https` 28, `udp` 107, `ws` 1, `wss` 2.

---

## Address families and resolution, measured 2026-09-08

⛔ **These are DNS figures, not health.** Nothing that produced them contacted
a tracker, and none of them is a health state. They are here because the
corpus's *shape* includes whether its names resolve, and because
[`gates.md`](gates.md) carried a dash where the IPv6-only count belongs.

⛔ **AND THEY ARE PER VANTAGE, WHICH IS THE FINDING RATHER THAN A CAVEAT.**
Every other figure on this page is a property of the corpus and is the same
everywhere. These are not: `experiments/30` measured a **runner's own resolver
failing on `openbittorrent.com`** while public resolvers answer for it
(`C-06`). So a single table here would be the three-contradictory-sets failure
this whole page exists to prevent, wearing the shape of a reasonable
simplification. ⭐ **The canonical column is the runner**, because that is the
vantage every health record in this project was taken from.

```bash
python3 experiments/29-address-family-census.py
python3 experiments/30-resolution-failure-classes.py
```

**965 distinct hostnames**, of which **206 are address literals** needing no
lookup, and **759 name-addressed**. Those two are vantage-independent.

### Address family

| | GitHub runner (canonical) | authoring host |
| --- | --- | --- |
| IPv4-capable | **710** hosts, **966** URLs | 706 hosts, 960 URLs |
| **IPv6-only** | **15** hosts, **16** URLs | 15 hosts, 16 URLs |
| did not resolve | **240** hosts, **345** URLs | 244 hosts, 351 URLs |

⭐ **The IPv6-only figure is identical on both**, which is what makes it safe
for [`gates.md`](gates.md) to quote as a corpus property: it is a fact about
the names. Everything else in the table moves with the resolver.

**Three of the fifteen IPv6-only hosts have an IPv4 host in this corpus under
the same registrable domain**, already probed: `anna.bt.bontal.net`,
`ipv6.govt.hu` and `ipv6.tracker.harry.lu`. Identical on both vantages.

### Which kind of not-resolving

`experiments/30` asks a **second, independent resolver** about every host the
first could not answer for, so that "does not resolve" and "does not resolve
*for us*" are separate facts.

| class | runner `ubuntu-24.04` | runner `ubuntu-22.04` | authoring host |
| --- | --- | --- | --- |
| resolves for both | 519 / 743 | 519 / 743 | 516 / 739 |
| **resolves only for the public resolver** | **3 / 7** | **1 / 3** | **0 / 0** |
| resolves only for this host | 0 / 0 | 0 / 0 | 0 / 0 |
| gone, NXDOMAIN confirmed | **184 / 261** | 185 / 263 | 179 / 256 |
| no address records | 43 / 61 | 43 / 61 | 43 / 61 |
| resolves to an unusable address | **0 / 0** | **0 / 0** | **11 / 14** |
| lookup failed, undetermined | 10 / 16 | 11 / 18 | 10 / 18 |

*hosts / URLs. Runner figures from workflow run `34235047982`, the first
census with the null-address class; authoring-host figures from
`experiments/results/29-address-family-census.unclassified-host.20260908T091721Z.json` and `experiments/results/30-resolution-failure-classes.unclassified-host.20260908T134349Z.json`.*

⛔ **None of these is `dead`.** The vocabulary is deliberately about the lookup
rather than about the tracker, and `MIN_SAMPLES_FOR_DEATH` is 3.

⭐ **The rescues on the runner are real and their addresses are routable.**
`openbittorrent.com` and `tracker.openbittorrent.com` both answer
`52.223.13.41`, and `w.wwwww.wtf` answers four Cloudflare addresses. That is
the case [T-037](../TODO/measurement.md) was built for, and it is why
`resolver_divergence` exists as a value that can never become `dead`.

⛔ **The unusable-address row is a vantage difference in `getaddrinfo`, not in
DNS.** Eleven hosts answer `0.0.0.0`, `::` or both. This host's resolver
refuses that answer with `WSANO_DATA`, so the second lookup sees it; a runner's
`getaddrinfo` returns the null address, so those hosts land in the first row
and **reach the prober**. RFC 1122 section 3.2.1.3 makes the address invalid as
a destination and Linux routes a connect to it at the local host, so the probe
now refuses to open one.

⚠ **The two runner images disagree, and they disagreed differently last time.**
On 2026-09-08 run `34210496112` reported 3 and 2 rescued, with
`tracker.parrotlinux.org` on `24.04` only; the census two hours later reports 3
and 1, with `w.wwwww.wtf` in place of `tracker.parrotlinux.org`. **A resolver
census is a measurement of a moment.** `C-06` carries what that means.

⚠ **From the authoring host, all six public queries for the OpenBitTorrent
names time out**, so the BEP 34 consent lookup fails before resolution is
reached and those trackers are skipped rather than misclassified.

## What these numbers are not

* **Not liveness.** Nothing here probed anything. `995 unique of 1091` is a
  string comparison, and the resolution figures above are DNS. Whether the
  corpus is alive is [`gates.md`](gates.md)'s value gate, answered 2026-09-08
  from committed health records rather than from anything on this page.
* **Not stable.** `ngosang` and `XIU2` regenerate daily. These are the counts
  in the committed fixture snapshots, which is what makes them reproducible;
  a fresh fetch will differ, and that is the point of pinning the fixtures.
* **Not the whole scheme universe.** `.onion` does not occur in this corpus.
  Absence here is not absence in the world (RULES 2, "an absence is not a
  zero").
* **Not a yggdrasil count.** A yggdrasil tracker addressed by *hostname* is
  indistinguishable from clearnet by URL alone, so `networks` **under-counts**
  yggdrasil by construction. Only the `_ip` variants expose the `200::/7`
  literals. Correct classification needs DNS, which is a time-varying inference
  and belongs to the health checker, not to a census.

## Refreshing this file

Re-run both commands and replace the tables wholesale. **Do not edit a single
number in place** -- the failure this file exists to correct is exactly a number
that drifted from its instrument while looking authoritative.

```bash
python3 experiments/19-scheme-census.py --offline
python3 scripts/generate.py --offline
python3 scripts/check-citations.py      # catches a figure quoted outside this file
```
