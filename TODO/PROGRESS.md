# Progress

**The record.** Everything that changes from session to session is here: the
measured baseline, what the last session did, and the work order.

⛔ **It carries no history and every session rewrites it in full.** For history,
read [`../HISTORY/`](../HISTORY/) and the git log.

**Read [`../docs/AGENTS.md`](../docs/AGENTS.md) first.** It carries the
absolutes, the routing table, and the box on why a session may not stop or
defer. [RULES.md](RULES.md) is normative for all of it. Every entry, one line
each: [INDEX.md](INDEX.md).

> **The shape this file must keep:** the state line with the session's start
> instant in ISO 8601 UTC; the measured baseline, citing the file that owns each
> number; the entry counts; what the session did; what is in progress;
> **Start here next session** as an ordered list with entry ids; and open
> questions for the operator. `python3 scripts/check-todo.py` prints the counts,
> and none of them is typed here.

---

## State

- **Current session:** started `2026-09-05T01:00:00Z`, in progress. A
  **measurement-conduct pass**: the exclusion route RULES 4 requires was built,
  which lifted the standing block on every entry that needs a live tracker, and
  the credential defect the previous session found was fixed.
- **Branch:** `main`. The repository is public at
  `https://github.com/Azathothas/Trackers`.
- **Nothing is published as data.** No dataset exists at any public URL, and
  nothing in the repository claims any tracker is alive. The probe exists and
  **has still never been pointed at the corpus.**
- ⚠ **This session ran on a Windows 11 host, not a runner and not the proxied
  authoring sandbox.** `environment_class` calls it `unclassified-host`. No
  network measurement was taken from it, so nothing here is a new vantage
  claim.

## Measured baseline

**Every corpus figure lives in
[`../HISTORY/corpus-baseline.md`](../HISTORY/corpus-baseline.md)** and nowhere
else, with the command behind each. Do not restate one here; cite it.

Network measurements are from workflow run **`33383406869`**, 2026-09-01, two
runner images, `ubuntu-24.04` and `ubuntu-22.04`. Results are committed under
`experiments/results/` because workflow artefacts expire after 90 days and git
does not. ⚠ **None of them was re-taken this session**, so every network figure
below is the previous session's and is quoted as such.

| | |
| --- | --- |
| UDP arbitrary-port egress | **true**, both images, tier-0 loopback plus four tier-1 controls |
| BEP 15 connect | **10/11, 9/11, 10/11, 10/11** across four runs, median RTT 97.5 to 103.9 ms. ⚠ 10 is the ceiling: one target has no IPv4 address |
| IPv6 egress | **false**, both images, stack present |
| TCP ports | open 80, 443, 2095, 6969, 8080. **None blocked.** One target no longer resolves (`C-71`) |
| HTTP tracker discrimination | 4 of 6 subjects proved tracker, 4 scrape-capable, 2 no response, `announce_sent: false` |
| DNS resolver divergence | **0 divergent of 17** on both images, and **1 divergent on the run 40 minutes earlier**. Thin, and carried as T-007 |
| Corpus, accepted dataset, transport mix | [`corpus-baseline.md`](../HISTORY/corpus-baseline.md) |
| Test suite | **195** tests, no network |
| Reference corpus | **10** repositories, **216** comment threads, **501** comments |
| Local gate | `python3 scripts/check-gate.py --strict`: 14 checks pass, 1 expected skip |
| Private-tracker credentials in the generated plaintext | **0**, refused by the pipeline (`C-70`, T-107 closed) |
| CI | `gate.yml` **green on `ubuntu-24.04` and `windows-2025`** at `ff79b9a`, run `33937458370`, confirmed by looking |

## Counts

Run `python3 scripts/check-todo.py`. It re-derives every number from the rows
and fails a gate when [INDEX.md](INDEX.md)'s table disagrees. **Nothing is
blocked.**

## What this session did

**It found that the work order it was handed could not be executed in order,
and fixed the reason.**

⭐ **RULES 4 forbade a corpus-wide probe until BEP 34 was honoured, and
[T-032](measurement.md) -- the entry that would honour it -- was not in the work
order at all.** The previous order opened with [T-012](claims.md), whose design
requires probing the full HTTP/HTTPS corpus. RULES is normative over this file,
so T-032 was done first. It is also the leverage entry in RULES 10.1c's sense:
one small piece of work unblocked T-012, [T-027](measurement.md),
[T-028](measurement.md) and the corpus half of [T-024](measurement.md) and
[T-029](measurement.md) at once.

**[T-032](measurement.md) is closed.** `src/trackers/bep34.py` reads the
operator's TXT record and, because the standard library has no TXT resolver and
D1 forbids a dependency, implements the DNS client as well: UDP with a TCP
fallback on truncation, bounded sizes, compression pointers that cannot loop,
and the transaction id and echoed question checked before an answer is
believed. The record is treated as the **exhaustive allow-list** the
specification says it is, so a bare `BITTORRENT` denies everything.

⛔ **The gate is in `probe_udp` and `probe_http`, not in `probe`.** Both are
public entry points that open their own sockets and the oracle tests call them
directly; gating only the dispatcher would have left two ungated doors into the
same action. `effective_port` was extracted so the port the gate checks is
provably the port the prober opens.

**[T-107](sources.md) is closed.** Seven URLs carrying six people's passkeys no
longer reach the output; the accepted count moved **1334 to 1327**. They are
refused rather than redacted, and the run report's new *Refused entries*
section names all fifteen refusals with reasons and with credentials removed.

**[T-029](measurement.md) is closed.** `src/trackers/sweep.py` bounds a run
three ways -- concurrency across hosts, exactly one connection per host in both
profiles, and a whole-run deadline -- and `scripts/probe-corpus.py` drives it.
⚠ **`asyncio` was rejected**: it would mean an async rewrite of both probers and
therefore two implementations of the probe. A thread pool runs the production
probe path unmodified.

**[T-022](measurement.md) is closed, and not by building what it asked for.**
Its own `Decision` said to scrape on UDP only where connect is shown
insufficient, and `_PROVING_RUNG[UDP] is PROTOCOL_VALID`: **a connect already
proves a tracker**, so a scrape would spend an operator's second round trip and
a required `info_hash` to learn nothing. The `Prove` clause could not be
satisfied honestly, which is D15. What replaces it is a test that parses `src/`
with `ast` and fails if anything ever calls `build_scrape_request`.

**[T-003](claims.md) is closed and it refuted something.** `experiments/24`
created throwaway releases here, measured, and deleted them; the repository had
0 releases and 0 tags before and after, asserted by the script. `C-14` is
verified on both halves -- a tag named `latest` earns nothing and a newer
*prerelease* does not take the channel. **`C-17` is refuted**: a moved tag
leaves the release's `target_commitish` on the old commit while `tarball_url`
follows the tag, so two consumers reading one release disagree silently.
**Delete-and-recreate is the route for [T-064](publication.md).**

⭐ **`C-15` is the one that nearly went in wrong.** The first run fetched once,
three seconds after replacing an asset, saw the old bytes with an unchanged
`ETag`, and would have recorded "assets cannot be replaced". The control RULES
2 requires -- the asset's API metadata, which separates a failed replacement
from a cached one -- showed the replacement had landed and a cache was serving
stale bytes. The window is **variable**: three runs minutes apart gave 0 s once
and between 10 s and 40 s once. `Cache-Control` was absent every time, so
nothing warns a consumer. Publication must not assume read-after-write.

**[T-024](measurement.md) is advanced, not closed, and the reason is a vantage
rather than a missing piece.** The emitter exists and
`tests.test_concurrency.RecordsSatisfyTheVantageGate` runs the real gate over
records this project produced against trackers it controls. ⛔ **What is left is
a probe of the real corpus, and this session refused to run one**: RULES 13.1
authorises probing live trackers from CI, and this host is an
`unclassified-host` on a residential connection. The three routes considered,
and why two were refused, are on the entry.

**Four defects were found by building those two, none of them in either plan:**

1. **The refusal count was wrong before anyone read it.** Keying the record on
   the *masked* URL collapsed two passkeys on one endpoint: seven refused, six
   recorded.
2. **A narrowing reintroduced the whole-line allowlist.**
   `check-no-secrets.py` matched with `search`, so the first token on a line
   decided the verdict for the whole line, and a synthetic test vector would
   have hidden a real credential beside it.
3. ⭐ **The test oracle was flaky, one run in three, and the cause is a host
   fact worth keeping.** Windows keeps **separate** port exclusion ranges per
   protocol, so an ephemeral port free for UDP is not necessarily bindable for
   TCP, and retrying `bind(0)` walks *through* an excluded block rather than
   away from it. Measured here: 25 excluded TCP ranges, 23 UDP.
   [`../docs/conventions/shell.md`](../docs/conventions/shell.md) section 6.
4. **T-032's own premise was stale.** It said the README promises the BEP 34
   route in the present tense; the previous session had already removed that.
   The correction is written under the entry's title rather than edited away.

**Two requirements were changed, neither silently** (RULES 9): **D13** replaces
RULES 4's blanket ban on corpus probing with the narrower permanent rule that a
probe runs only through the code path consulting BEP 34 first, and **D14**
replaces the private-credential ceiling with a path rule that has no exemption.
Both carry their rejected alternatives.

**Both guards were mutation-proved.** Forcing the BEP 34 consultation to always
allow fails 5 tests including the one that asserts a real loopback tracker
received no datagram; a credential written outside a verbatim capture fails
`check-no-secrets.py` with exit 1.

## In progress

**Nothing half-finished.** The two entries that were advanced-but-open remain
so, unchanged:

- **[T-024](measurement.md)**: the emitter, the instrument and the gate's
  `--path` all exist and are tested end to end against the loopback oracle. No
  record of a **real** tracker exists, because no sanctioned vantage has run
  the sweep, so `scripts/check-vantage-metadata.py` still exits 2 and that
  remains correct.

## Start here next session

1. **[T-024](measurement.md)** - run `scripts/probe-corpus.py` **on a
   runner** and commit the records. Everything else for it is built; what is
   missing is a sanctioned vantage, which is why it is first and why it is
   cheap. **`check-vantage-metadata.py` flips 2 to 0 the moment records exist**,
   and [`../docs/methodology/gate.md`](../docs/methodology/gate.md) says its
   `expect_skip` flag comes off in the same change.

   ⚠ **Read the entry's three routes before reaching for a shortcut.** Emitting
   `unknown` records offline would flip the gate today and prove nothing, which
   is why `probe-corpus.py` has no offline mode.

   ⚠ **The BEP 34 consultation costs a DNS round trip per host.** The sweep
   already shares one `Resolver` across the run, which caches per host; a
   caller that builds one per probe would multiply the DNS load by the number
   of URLs per host.

2. **[T-027](measurement.md)** - the value gate. Answerable as soon as (1)
   lands, and **a negative answer is a successful outcome.** T-107 has already
   moved one half of it: refusing seven credential-bearing URLs is measurable
   value over concatenation that no upstream in the corpus provides.

3. **[T-012](claims.md)** - measure whether our identity gets us blocked. **Now
   permitted, and it was not before.** Two axes, not one (`C-63`): the
   `User-Agent` header *and* the BEP 20 `peer_id` prefix. `ProbeConfig` already
   takes both a `user_agent` and `extra_headers` so the arms run through one
   code path.

   ⚠ **Three constraints, and the third is new.** It cannot run from a proxied
   sandbox (`C-62`). The arms are spaced per host and interleaved across hosts.
   And **twelve cells over the HTTP corpus is roughly twelve thousand requests
   at somebody else's expense**, so the politeness ceiling of RULES 4 decides
   the schedule before the statistics do; this is a workflow that runs over
   days, not a command a session fires once.

4. **[T-028](measurement.md)** - the newTrackon cross-check. Cheap once records
   exist, and the first thing that produces something no upstream publishes:
   **disagreement between independent observers.**

5. **[T-001](claims.md)** - run a real torrent client against the plaintext.
   P0, independent of all of the above, and good work for a session that does
   not want to hold the measurement context.

6. **[T-064](publication.md)** - release channel semantics. **Unblocked by
   T-003 and its shape is now decided by measurement rather than by guess:**
   delete-and-recreate rather than move-the-tag (`C-17` is refuted), and no
   read-after-write assumption (`C-15`).

**Deliberately deferred, and not a blocker:** [T-044](scoring.md), the scoring
model. It waits on history existing (T-040), because choosing a model now would
be fitting one to zero samples.

**[T-031](measurement.md) is the highest-value entry and is deliberately not
numbered above**, because it is not sequential: it is what to reach for when
the ordered work stalls or feels mechanical. One indirect-liveness mechanism
serves IPv6-only, i2p, yggdrasil, `wss` and blocked-vantage cases at once.
**Two of its six routes are cheap**: the dual-stack shortcut is a two-line call
on `Resolution.families`, and `wss` needs only a transport added to a ladder
that already exists. ⭐ **It has also acquired a third**: T-032 left a gap where
a corpus URL naming a host by IP literal cannot be protected by a denial
published on the name, and closing it needs a denial to propagate across a
shared *resolved* address, which is the same machinery.

**When this order is exhausted**, take the next entry from
[INDEX.md](INDEX.md) by priority. ⛔ Do not stop because the list above ran out.

## Questions the operator has answered

All four were put and settled before 2026-08-29. **They are closed; do not
re-raise them.**

1. **May a session create throwaway releases here?** Yes, in this repository.
   D10 and RULES 13.1. Tag them `test-*` and delete them once the answer is
   recorded.
2. **What defines membership of `foss.txt`?** Derived, plus a labelled seed.
   D9, settled in [T-046](scoring.md).
3. **Is the roughly three-hour probe cadence acceptable?** Yes: publish hourly,
   probe each tracker on its own stated interval, defaulting to three hours.
   D7, settled in [T-026](measurement.md).
4. **What is authorised outward-facing?** Every action belonging to this
   repository, and nothing outside it, ever. RULES 13.

## Open questions for the operator

**None blocking.** Two things a future session should know rather than
re-derive:

1. The first publication of this repository **force-pushed over a placeholder
   commit** that existed on the remote before this tree did. That was the
   operator's instruction and it is the only history rewrite this project has
   performed. [`../docs/conventions/git.md`](../docs/conventions/git.md)
   section 2 records it as an exception rather than a precedent.
2. ⚠ **BEP 34 lookups send tracker hostnames to a public resolver.** That is a
   deliberate trade recorded in `src/trackers/bep34.py`: the alternative is the
   host's own resolver, which is the recorded way this mechanism fails
   silently in production (newTrackon issue #316). If the operator would rather
   the project ran its own recursive resolver, that is a decision to take
   before the first corpus sweep, not after.
