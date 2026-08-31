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

- **Last session:** 2026-09-01, started `2026-09-01T09:00:00Z`, ended on
  operator instruction. It was an **adoption and publication pass**, not a
  feature session: the repository was brought under
  `Azathothas/TEMPLATE`'s methodology, cleared of the prose defects that
  methodology exists to catch, and published for the first time.
- **Branch:** `main`. The repository is public at
  `https://github.com/Azathothas/Trackers`, one commit.
- **Nothing is published as data.** No dataset exists at any public URL, and
  nothing in the repository claims any tracker is alive. The probe exists and
  **has never been pointed at the corpus.**

## Measured baseline

**Every corpus figure lives in
[`../HISTORY/corpus-baseline.md`](../HISTORY/corpus-baseline.md)** and nowhere
else, with the command behind each. Do not restate one here; cite it.

Network measurements are from workflow run **`33383406869`**, 2026-09-01, two
runner images, `ubuntu-24.04` and `ubuntu-22.04`. Results are committed under
`experiments/results/` because workflow artefacts expire after 90 days and git
does not.

| | |
| --- | --- |
| UDP arbitrary-port egress | **true**, both images, tier-0 loopback plus four tier-1 controls |
| BEP 15 connect | **10/11, 9/11, 10/11, 10/11** across four runs, median RTT 97.5 to 103.9 ms. ⚠ 10 is the ceiling: one target has no IPv4 address |
| IPv6 egress | **false**, both images, stack present |
| TCP ports | open 80, 443, 2095, 6969, 8080. **None blocked.** One target no longer resolves (`C-71`) |
| HTTP tracker discrimination | 4 of 6 subjects proved tracker, 4 scrape-capable, 2 no response, `announce_sent: false` |
| DNS resolver divergence | **0 divergent of 17** on both images, and **1 divergent on the run 40 minutes earlier**. Thin, and carried as T-007 |
| Corpus, accepted dataset, transport mix | [`corpus-baseline.md`](../HISTORY/corpus-baseline.md) |
| Test suite | **127** tests, no network |
| Reference corpus | **10** repositories, **216** comment threads, **501** comments |
| Local gate | `python3 scripts/check-gate.py --strict`: 14 checks pass, 1 expected skip |
| Private-tracker credentials in the generated plaintext | **6 distinct**, on 7 URLs (`C-70`, `T-107`) |
| CI | `gate.yml` green on `ubuntu-24.04` **and `windows-2025`** |

## Counts

Run `python3 scripts/check-todo.py`. It re-derives every number from the rows
and fails a gate when [INDEX.md](INDEX.md)'s table disagrees. **Nothing is
blocked.**

## What the last session did

**It adopted the methodology this project had been citing and never held itself
to, and the adoption pass found four defects in the process.**

**The template was adopted in place**, at commit `6206166`, with what was taken
and what was declined recorded in
[`../docs/methodology/template-sync.md`](../docs/methodology/template-sync.md).
The checks were **rewritten in Python rather than copied as shell pairs**,
because RULES 15.5 makes a `.sh` a gate depends on a platform requirement in
disguise. Six new checks, one runner, and
[`../scripts/README.md`](../scripts/README.md) is the contract all of them meet.

**The prose rule was applied and armed.** The tree carried **1655 characters
outside the allowed five across 55 files**, 840 of them em dashes, and 14 of
those files were under `src/` or `tests/` where a markdown-only rule would
never have looked. `check-markers.py` now refuses them and holds a density
ceiling. The banned-vocabulary and one-fact-one-home rules were stated in the
methodology and enforced by nothing; both are checks now.

**Four defects came out of the adoption itself, none of which the previous
gate could see:**

1. ⭐ **The pipeline republishes private-tracker credentials.** Six distinct
   passkeys belonging to real people reach `trackers_all.txt`, on seven URLs,
   from two upstreams. `C-70` records it, `T-107` fixes it, and
   `check-no-secrets.py` holds the count so a seventh fails the gate.
2. **Two instruments wrote text with the platform newline**, so the same
   experiment produced different bytes on Windows and on a runner. A committed
   result is evidence, and evidence whose bytes depend on who ran it cannot be
   diffed against the next run. Both now pass an explicit newline.
3. **`check-citations.py` reported a code span as a broken link.** A page about
   PowerShell rounding cites `[int](2.65)` inside backticks; markdown does not
   linkify inside a code span, and the checker did. A checker that cries wolf
   is one somebody switches off.
4. **Running the gate dirtied the working tree.** The offline census wrote a
   timestamped result into `experiments/results/` on every run, so RULES 10.3
   step 6 could never be satisfied. It writes to scratch now.

**Three vendored agent instruction files were removed from the corpus.** A file
with that name anywhere under a repository is read as instructions by the tools
working in it, so keeping one puts a third party's instructions inside this
project. `references/PROVENANCE.md` records which three and why.

**Two helpers were vendored, pinned and checked.** The environment probe and
the commit-and-push helper come from `Azathothas/ToolKit` at `bf11930`, with a
digest per file and `check-vendor-pin.py` holding them to it.

⭐ **The network baseline was re-taken, and re-taking it found a defect in the
instrument that takes it.** The figures this file used to carry came from a
workflow run whose artefacts went with this repository's prior history, so
`p0-ground-truth.yml` was run again. Its first run reported
`tcp_ports_blocked: [2710]` on both images, which reads as "GitHub blocks the
classic BitTorrent tracker port" and is false: the verdict counted any failed
TCP row as a blocked port, and the host had stopped resolving. `C-71` records
it, the instrument now separates the two, and the committed results are from
the run after the fix.

**The adoption's own shape is recorded as D12**, with what was rejected and
why, so the next session does not re-argue it. **The three deep reviews are
under [`../HISTORY/reviews/`](../HISTORY/reviews/)**: what changed that nobody
asked for, whether every new guard can actually fail, and which sentence
written this session is not backed by something on disk. Six passes ran, not three: the
three RULES 10.3 requires, plus the cold start, the tracker operator, and what
was measured but never verified.

⭐ **Three of them found something nothing mechanical could have.** The claim
audit found the headline number of this session's largest change wrong in three
documents. The cold start found RULES 10.3 step 9 instructing a reader to use
`/tmp`, which RULES 15.5 forbids. And the single-observation pass found that
the DNS divergence figure **disagrees with itself between the two runs**, which
only became visible because a defect forced a second run.

## In progress

**Nothing half-finished.** Two entries remain advanced-but-open, unchanged from
the previous session, and each says in its own text what exists and what does
not:

- **[T-022](measurement.md)**: the codec, the 20-byte refusal and the record
  field exist; the send path in `probe_udp` does not.
- **[T-024](measurement.md)**: the record shape exists and is tested; nothing
  writes one to disk, so `scripts/check-vantage-metadata.py` still exits 2,
  which remains correct.

## Start here next session

1. **[T-012](claims.md)** - measure whether our identity gets us blocked.
   **P0, and it contaminates everything after it.**

   **It has two axes, not one (`C-63`).** An HTTP tracker request carries a
   `User-Agent` header *and* a `peer_id` whose BEP 20 Azureus prefix is what a
   tracker's client-filtering rules are written against. A UA-only experiment
   reporting "no block" cannot be distinguished from "we happened to send an
   acceptable `peer_id`". The entry carries the crossed design.

   Two conditions the tree already establishes: it **cannot be run from an
   authoring sandbox** behind an egress proxy (`C-62`), so the instrument must
   refuse to emit subject results from a proxied vantage; and **the arms are
   spaced per host and interleaved across hosts**, because pairing requires the
   same run rather than the same instant.

2. **[T-107](sources.md)** - stop republishing private-tracker credentials. S
   effort, the shape is already matched by a check, and it is the clearest
   instance of the value this project claims over concatenation. Closing it
   also takes the ceiling out of `check-no-secrets.py`.

3. **[T-024](measurement.md)** and **[T-029](measurement.md)** - emit health
   records, with the concurrency bound. Take them together: the corpus probed
   serially at a 5 s timeout is over an hour, so the bound is what makes the
   run possible at all. **`check-vantage-metadata.py` flips 2 to 0 the moment
   this lands**, which is the cheapest visible proof that P2 has started.

4. **[T-027](measurement.md)** - the value gate. Answerable as soon as (3)
   lands, and **a negative answer is a successful outcome.** Do not defer it:
   the longer the project runs unjustified, the more expensive the honest
   answer becomes.

5. **[T-028](measurement.md)** - the newTrackon cross-check. Cheap once records
   exist, and it is the first thing that produces something no upstream
   publishes: **disagreement between independent observers.**

6. **[T-022](measurement.md)** - wire the UDP scrape path. Small, and the
   codec, the refusals and the record field are already there.

7. **[T-001](claims.md)** - run a real torrent client against the plaintext.
   P0, independent of all of the above, and good work for a session that does
   not want to hold the measurement context.

8. **[T-003](claims.md)** - release and tag behaviour. S effort, answers three
   claims, unblocks [T-064](publication.md). Throwaway releases here are
   sanctioned (RULES 13.1): tag them `test-*` and delete them afterwards.

**Deliberately deferred, and not a blocker:** [T-044](scoring.md), the scoring
model. It waits on history existing (T-040), because choosing a model now would
be fitting one to zero samples.

**[T-031](measurement.md) is the highest-value entry and is deliberately not
numbered above**, because it is not sequential: it is what to reach for when
the ordered work stalls or feels mechanical. One indirect-liveness mechanism
serves IPv6-only, i2p, yggdrasil, `wss` and blocked-vantage cases at once.
**Two of its six routes are cheap**: the dual-stack shortcut is a two-line call
on `Resolution.families`, and `wss` needs only a transport added to a ladder
that already exists.

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

**None blocking.** One thing a future session should know rather than
re-derive: the first publication of this repository **force-pushed over a
placeholder commit** that existed on the remote before this tree did. That was
the operator's instruction and it is the only history rewrite this project has
performed. [`../docs/conventions/git.md`](../docs/conventions/git.md) section 2
records it as an exception rather than a precedent.
