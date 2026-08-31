# `Azathothas/bit-cli` -- adopt

**Commit:** `cce8131`, **Licence:** MIT (`tree/LICENSE`),
**Captured:** 2026-08-31, **Corpus:**
[`references/Azathothas__bit-cli/`](../../references/Azathothas__bit-cli/)

A command-line BitTorrent client in Rust. It is the **only reference in this
corpus that actually speaks the tracker protocols**, and it is the source the
operator identified as having written this project's original design brief --
so where it and that brief disagree, the code is the evidence.

## What this reading did NOT establish

* **Its tracker was not read.** Issues and pull requests were not fetched
  (`references/PROVENANCE.md`). Its engineering arguments live in-repo under
  `TODO/` and `docs/`, which is why the tree was the priority, but the
  maintainer's rulings on anything not written down are unseen.
* **Nothing was executed.** No `bit-cli` binary was built or run. Every claim
  below is source read at `cce8131`, not behaviour observed.
* **Its measured numbers are its own.** The UDP retry timings in
  `docs/trackers.md` were measured on its author's hardware against its own
  loopback tracker. They are cited as *its* measurements, never adopted as
  ours.
* **Passes taken: three.** WHAT, MECHANISM, and AGAINST-us. The fourth pass
  the methodology describes -- how it handles the thing *we* find hard -- is
  partly vacuous here: it is a client and we are a monitor, so several of our
  hard problems (vantage bias, source aggregation, scoring) have no counterpart
  in it at all.

## Verdict: **adopt**, five mechanisms

### 1. `min interval` is spelled two ways in the wild -- `tracker.rs:739`

```rust
min_interval_s: count_of(&value, "min interval")
    .or_else(|| count_of(&value, "min_interval")),
```

A real client checks **both** spellings. So does its failure key handling:
`failure reason` at `tracker.rs:745` for an announce, and `failure reason`
falling back to `failure_reason` at `tracker.rs:776` for a scrape -- which is
correct, because **BEP 48 itself specifies the underscore form** for scrape
while BEP 3 uses the space form for announce.

**What this changes here.** `src/trackers/bencode.py` already checks both
failure spellings (`FAILURE_KEYS`). It reads `interval` and **does not read
`min interval` in either spelling** (`C-65`). D7 makes the tracker's own stated interval
the politeness anchor, and `min interval` is the tracker stating a *floor* it
wants respected -- the more binding of the two. Filed as work on T-026.

### 2. A negative count is unknown, not zero -- `tracker.rs:717`

```rust
fn count_of(value: &Value, key: &str) -> Option<u64> {
    match value.get(key).and_then(Value::as_int) {
        Some(n) if n >= 0 => Some(n as u64),
        _ => None,
    }
}
```

A tracker answering `complete: -1` has not said the swarm has no seeders.
Clamping to zero would publish "no seeders" as a fact nobody stated.

**This is RULES 2's "an absence is not a zero", running in the inbound
direction**, and it is independent confirmation from a codebase that had to
learn it. Our BEP 15 scrape parser already refuses to zero-fill a short
response; the HTTP side has no numeric fields yet and will when T-060 lands.

### 3. The BEP 48 derivation must not guess -- `tracker.rs:695`

```rust
let (head, last) = base.rsplit_once('/')?;
let rest = last.strip_prefix("announce")?;
```

It returns `None` rather than inventing an endpoint, and says so to the user:

> `http://example/t/9f3c does not follow the BEP 48 convention, so its scrape
> URL cannot be derived. Name it with --scrape-url`

The reason is stated in the source and it is exactly our failure mode:
*"guessing one produces a 404 that reads like a tracker failure."*

**What this changes here.** `Tracker.scrape_url` in `src/trackers/model.py`
replaces the **first** `announce` anywhere in the path, per BEP 48's literal
wording ("locating the string `announce` in the path section"), which was
verified against the specification text on 2026-08-31 (`C-66`). That is right for
`/announce.php` and for `/a/announce`, and **wrong for `/announcements/feed`,
which it turns into `/scrapements/feed`** -- a fabricated endpoint whose 404
would be recorded against the tracker. bit-cli's anchoring on the last path
component does not have that defect. Fixed here by requiring the match to
start a path component; `tests.test_p1` carries the case.

### 4. BEP 15's own retry ladder is unusable for a diagnostic -- `tracker.rs:615`

BEP 15 specifies retrying at `15 * 2^n` seconds for `n` in 0..8: nine attempts,
up to **62 minutes** before giving up on one tracker. bit-cli refuses it and
says why: *"a foreground diagnostic that can take an hour to say 'this tracker
is down' has not answered the question the caller asked."* It does three
attempts inside `--tracker-timeout`, one attempt being
`max(timeout / 3, 1s)` (`tracker.rs:626`), and documents the worst case as
five attempts -- because a UDP announce is two exchanges and either can be the
one that dies.

**What this changes here.** T-029 owes a timeout budget and has no number.
This supplies the shape of the argument and the arithmetic to copy: the budget
for one UDP tracker is `attempts x per-attempt`, the worst case is not
`attempts x 1` because connect and announce are separate exchanges, and the
per-attempt floor matters more than the nominal timeout at small values. Filed
on T-029.

### 5. Two identities for two jobs, and neither impersonates -- `peer_id.rs:36`

`bit-cli` presents an Azureus-style peer id `-CL0200-` plus twelve random
printable characters. The two-character client code was **checked against six
registries before use** -- libtorrent `v2.0.11`'s own `identify_client.cpp`
table of 92 codes, plus five independent reimplementations -- and `CL` appears
in none of them, in either case, *because the lookup is a byte comparison*:
`lt` is rTorrent and `LT` is libtorrent, and the two have coexisted for two
decades. The test that holds it is
`the_client_code_is_not_one_a_registry_already_names`.

Separately, `fingerprints/bit-cli-plain.json` and `bit-cli-browser.json` record
**two TLS/HTTP fingerprint profiles**, captured off the wire rather than
asserted.

**What this changes here, and it is the most important finding of this
reading.** T-012 asks whether *our User-Agent* gets us blocked. For HTTP
trackers that question is under-specified: an HTTP tracker announce carries
**both** a `User-Agent` header and a `peer_id` query parameter, and the
Azureus prefix in `peer_id` is the more discriminating of the two -- it is what
a tracker's client-filtering rules are actually written against, and it is what
a tracker's statistics page reports. An experiment that varies only the
User-Agent measures half the question. Recorded as `C-63`; T-012's design is
amended to vary both axes.

Two further consequences:

* **This project never sends a `peer_id` at all today**, because it never
  announces and a BEP 15 connect has no such field. If T-022's UDP scrape or
  any HTTP scrape ever needs one, the honest choice is bit-cli's: a code no
  registry claims, so we are not filed under somebody else's client. RULES 4's
  line about never evading an exclusion is unaffected either way.
* **Impersonating a mainstream client to obtain an accurate measurement is a
  live option** and bit-cli demonstrates it is a considered one, not a hack.
  RULES 4.1 already separates "blending in to measure accurately" from
  "evading a refusal we were given"; this is evidence that a serious client
  maintains both identities deliberately.

## Confirms, not new work

* **`C-30`, the BEP 15 magic constant.** `tracker.rs:26` --
  `const UDP_PROTOCOL_ID: u64 = 0x0417_2710_1980;` -- the same value our
  `src/trackers/bep15.py` uses, written with a leading nibble so the digit
  grouping is even. Actions 0/1/2/3 match ours exactly.
* **`C-32`, the tracker/web-server discriminator.** bit-cli treats a
  `failure reason` key at HTTP 200 as a **failed tracker with a reason**, and
  its own check script exercises *"one a tracker rejects at HTTP 200 with a
  `failure reason` key"* as a distinct case, noting it is *"the one a caller
  reading the status alone would record as a success"*. That is our
  `classify_body` argument, reached independently.
* **`C-36`, WebTorrent.** `docs/bep-coverage.md` lists WebTorrent under
  *completeness* gaps -- a real, current BitTorrent client does not implement
  `ws`/`wss` tracker support. Our `unmeasurable` classification for `wss` is
  not timidity; the transport is genuinely a different protocol that clients
  choose not to carry.
* **Hostile parsing of a tracker's answer.** One bad peer entry costs that
  entry and nothing else: an entry that is not a dictionary (*"the `peers: [42]`
  that crashed `anacrolix/torrent` before its PR 1055"*), an entry missing `ip`
  or `port`, a port outside 0-65535, a compact list whose length is not a whole
  number of addresses. Each is named in the output rather than dropped. Same
  discipline as RULES 3.10, applied to a tracker's response rather than a
  source's list.
* **An absent key is not an error.** *"A response with no `peers` key at all is
  a well-formed empty swarm rather than an error."*

## Filed elsewhere

* **BEP 7's separate peer lists and per-family announcing.** A tracker records
  the source address of the connection it was announced over, so one announce
  registers one address family, and a tracker keyed by peer id alone keeps only
  the last. Relevant to T-004 (vantage bias) and to any future dual-stack
  probe; not actionable while the runner has no IPv6 egress (`C-04`).
* **`left` semantics.** `0` means seed; `-1` is refused by real trackers
  (bit-cli cites the AWS S3 tracker's `400 left(-1) was not in the valid range`);
  absent draws a `500`; `i64::MAX` is accepted (`tracker.rs:96`
  `UNKNOWN_LEFT`). Only reachable through an announce, which RULES 4 forbids
  here. Recorded so that a future session proposing an announce path knows the
  field is a trap before it writes one.

## Refused

* **Its retry timings as our numbers.** Measured on its hardware against its
  own loopback tracker. We take the *shape* of the budget argument, not the
  seconds.
* **Its architecture.** It is a client that joins swarms; we are a monitor that
  must not. The BEP coverage table is a map of what a client owes, and most of
  it is out of scope by RULES 6.

## Where it disagrees with the brief this project came from

The design brief called `ws`/`wss` *"a different protocol... few clients support
it"* and left it there. bit-cli's coverage table is the concrete version of
that: WebTorrent is not a gap in reach, it is a gap in completeness, and a
client can be complete without it. That supports treating `wss` as
`unmeasurable` **and** treating a future WebTorrent probe as genuinely
optional rather than owed -- which is a softer requirement than T-005 currently
states, and T-005 is amended to say so.
