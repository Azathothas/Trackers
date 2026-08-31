# CorralPeltzer/newTrackon

**Verdict: adopt**

## Provenance

| item | value |
| --- | --- |
| repository | `https://github.com/CorralPeltzer/newTrackon` |
| commit read | `7da7dde4a16d153790f4f3d2a6e0a245dceae641` |
| tree in this repo | [`references/CorralPeltzer__newTrackon/tree`](../../references/CorralPeltzer__newTrackon/tree) |
| tracker | [`references/CorralPeltzer__newTrackon/issues.json`](../../references/CorralPeltzer__newTrackon/issues.json) -- issues **and** pull requests, both states, capped at 100 |
| read on | 2026-08-29 |

```bash
cat references/CorralPeltzer__newTrackon/COMMIT
```

**Not obtained:** Discussions (GraphQL only; the credential-free route is REST).
Review comments. Issue comments except where a section below quotes one.
[`references/PROVENANCE.md`](../../references/PROVENANCE.md) is the full gap list.

## Findings

Covered above. Three mechanisms adopted:

1. `views.py:131` -- `/api/<int:percentage>` as an independent oracle.
2. `scraper.py:217` `get_bep_34` -- **BEP 34 DNS opt-out**. A tracker operator
   publishes a `BITTORRENT` TXT record and a monitor removes them
   automatically. This turns RULES 4's "operator requests exclusion"
   from an email address into a **standard, automatable** mechanism, and it is
   registered as `C-51`.
3. `tracker.py:136` -- the tracker's own `interval` as the recheck cadence.

**A production failure worth stealing the lesson from.** Issue #316: BEP 34
opt-outs were silently *not honoured* on the official instance, because
**Hetzner's internal DNS resolvers did not follow CNAMEs**. The maintainer
diagnosed it and switched to public resolvers. This is direct production
evidence for `C-06`: a datacenter resolver differing from a public one broke a
correctness property, and it broke it *silently*. `experiments/04` found no
divergence at n=17 -- this is why that result is recorded as "no divergence
observed at this sample size" rather than "resolvers agree".

**A methodology difference that would corrupt a naive comparison.** Issue #324,
and the maintainer's reply: newTrackon reports **one preferred protocol per
tracker** (UDP first, then HTTPS, then HTTP). So `/api/udp` is *not* "trackers
that support UDP"; it is "trackers whose preferred protocol is UDP". Comparing
it to a per-endpoint measurement compares different quantities.

## Round 2, 2026-08-31: what round 1 read past

Round 1 cited `scraper.py:217` and `scraper.py:232`. **The two most decisive
lines in that file are `:53` and `:234`, and neither was read.**

### It impersonates qBittorrent, on both identity axes (`C-68`)

```python
scraper.py:53   SCRAPING_HEADERS = {"User-Agent": "qBittorrent/4.3.9", ...}
scraper.py:234  pid = "-qB4390-" + "".join(random.choice(...) for _ in range(12))
```

`-qB4390-` is qBittorrent 4.3.9's BEP 20 Azureus prefix, and the header names
the same version. `SCRAPING_HEADERS` reaches every HTTP fetch through
`memory_limited_get` at `:429`, which is what `announce_http` calls.

**This is the strongest evidence in the corpus on the question T-012 exists to
answer, and it was already here.** The operator of a years-old public monitor,
probing the same trackers this project intends to, chose not to identify
themselves -- and chose it on **both** axes at once, which is exactly what
`C-63` says the axes are.

It does not prove a descriptive User-Agent gets refused. It proves somebody who
would know judged it not worth finding out. RULES 4.1 carries it as a prior;
T-012 is what turns a prior into a number.

### It announces as a seed (`C-69`)

```python
scraper.py:238  args_dict = {"info_hash": thash, "peer_id": pid, "left": 0, ...}
```

`thash` is `urandom(20)` -- a synthetic infohash, which is what RULES 4 would
require if this project announced at all. But `left: 0` tells the tracker the
announcing host **has the complete file**, so every tracker newTrackon probes
files it as a seed for a swarm that does not exist.
`Azathothas/bit-cli`'s `docs/trackers.md` names that value explicitly:
*"treats this client as a **seed** and hands it to every peer asking for one"*.

**This is the concrete cost of the announce-based method**, and it is why the
two projects' numbers answer different questions. This project stops at BEP 15
connect and HTTP scrape, never sends `left`, and never registers a peer record
anywhere. Any cross-check (T-028) has to say so: newTrackon's "uptime" is
measured by joining, ours by asking.

---

The overview, the ordering of verdicts, and **what the sweep did not establish**
are in [`../reference-sweep.md`](../reference-sweep.md). Read that first: it
opens with the limitations, and this file assumes them.
