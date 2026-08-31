# Reference sweep

**Round 1:** 2026-08-29, **Round 2:** 2026-08-31,
**Corpus:** [`references/`](../references/),
**Method:** `Azathothas/TEMPLATE` `docs/methodology/references.md`, re-read at
`6206166` (its only change since round 1 is one added paragraph, which does not
bear on anything here)

> **Round 2, 2026-08-31.** Three trees had moved and were re-mined at their new
> HEADs; three references were **added** -- `Azathothas/bit-cli`,
> `Aseem0xff/pacman-static` and `AvalynSouvlaki/T-244-RESEARCH`. Issue comments
> were fetched for every issue that has any, closing round 1's largest
> self-declared gap. What changed as a result is in **Round 2 findings** below.
> [`../references/PROVENANCE.md`](../references/PROVENANCE.md) carries the
> commits, the licences and what was trimmed.

---

## What this sweep did NOT establish

Stated first, before any recommendation, because a reader who reaches the
recommendation first has already stopped reading.

* **No torrent client was tested.** One client's *parser source* was read
  (`bittorrent-tracker-editor`). Nothing was executed against qBittorrent,
  Transmission, aria2, Deluge or BiglyBT. The plaintext compatibility
  guarantee (T-001, `C-40`, `C-41`) therefore still rests on one
  source reading and zero runtime tests.
* **No tracker operator was consulted.** RULES 4 is still unreviewed by
  anyone who runs a tracker. Two pieces of *indirect* evidence were found
  (ngosang's blacklist carries 2 "requested by sysadmin" entries; BEP 34 gives
  a DNS opt-out), but no operator has read this project's announce policy.
* **`GitHub Discussions` were not fetched** for any reference. The credential-free
  route used here is REST-only and Discussions are GraphQL. Where a maintainer
  kept the design argument in Discussions, this sweep did not see it.
* **Only one vantage point exists.** Every network measurement comes from
  either a GitHub-hosted runner (AS8075, Microsoft) or the authoring sandbox
  (also datacenter, behind an HTTP proxy). **Neither is a residential
  connection**, which is where this dataset's consumers actually sit. `C-03`
  is consequently still unresolved.
* **Nothing about long-run scheduling was measured.** `C-10`, `C-11`, `C-12`,
  `C-19`, `C-19b` all need either documentation this sweep did not read or
  observations over weeks. `C-12` in particular -- workflows disabling after
  repository inactivity -- remains **load-bearing and unchecked**.
* **Release and tag behaviour was not tested** (`C-14`, `C-15`, `C-17`).
  Creating releases in someone's repository is outward-facing and was not done
  unilaterally. `D5` is blocked on it.
* **`Azathothas/bit-cli`'s tracker was not read.** Its engineering arguments
  live in-repo under `TODO/` and `docs/`, which is why the tree was the
  priority, but its issues and pull requests are unfetched.
* **Nothing was executed, for any reference.** Every claim in this document is
  source read at a recorded commit, or a maintainer's words in a captured
  tracker item. No reference's code was built or run.

**How many claims the previous revision got wrong** -- the only honest estimate
of how many are still wrong. This round **refuted or materially corrected 8 of
the 24 upstream/protocol claims it examined** (`C-20`, `C-21`, `C-22`, `C-23`,
`C-26`, `C-27`, `C-38`, plus HISTORY/reference-sweep.md's description of
`bittorrent-tracker-editor`). One of them -- `C-26` -- had **reversed** the
actual capability of the most important reference in the project.

**Assume more remain.**

---

## Route the reader

| you have | read |
| --- | --- |
| two minutes | the box above, then **Verdicts** |
| ten minutes | **What changed** and **The three findings that move the design** |
| an implementation to write | the per-reference sections, in order |
| a reason to distrust me | **Provenance**, then the instruments in `experiments/` |

---

## Provenance

Corpus is **tracked in the tree** under `references/<owner>__<repo>/`, each with
a `COMMIT` file and a stripped `tree/`. Reachable with no re-fetch:

```sh
cat references/ngosang__trackerslist/COMMIT
ls  references/ngosang__trackerslist/tree
```

| reference | commit | depth reached | tracker |
| --- | --- | --- | --- |
| `pkgforge-security/Trackers` | `7f2d00b` | workflows read line by line; all outputs diffed against live sources | 24 items -- **0 issues, 24 bot PRs** |
| `ngosang/trackerslist` | `562bdc0` | complete file list; all 11 outputs censused; blacklist reasons tabulated | 100 items -- 85 issues, 15 PRs |
| `CorralPeltzer/newTrackon` | `7da7dde` | route table, scraper, tracker model read at line level | 100 items -- 24 issues, 76 PRs; **4 issues read with comments** |
| `XIU2/TrackersListCollection` | `d169e6e` | workflow read; sources and UA extracted | 18 items -- **0 issues, 18 PRs** |
| `DeSireFire/animeTrackerList` | `e59508b` | metadata + census only; no generator exists to read | 47 items -- 43 issues, 4 PRs |
| `GerryFerdinandus/bittorrent-tracker-editor` | `c5f5b82` | **parser and validator read at line level** | 60 items -- 48 issues, 12 PRs |
| `Azathothas/TEMPLATE` | `6206166` | `references.md`, `experiments.md`, `work-todo.md` read in full; 15 methodology docs present | 0 |
| `Azathothas/bit-cli` | `cce8131` | tracker implementation, peer id, BEP coverage and the trackers/mining docs read at line level | **not fetched** |
| `Aseem0xff/pacman-static` | `38f7e3e` | `RESEARCH.md` and `docs/patches/mine-repo-page-join.md` read in full | not fetched |
| `AvalynSouvlaki/T-244-RESEARCH` | `88a8410` | its `RESEARCH.md` sections 0-1 and the instrument layout read | not fetched |

**Gaps in this table:** Discussions unfetched everywhere (GraphQL-only, and the
credential-free route is REST); review comments not fetched for any reference;
`bit-cli`'s tracker not fetched. **Issue comments are no longer a gap** --
round 2 captured every thread with a non-zero comment count, where round 1 held
four.

---

## Verdicts

Exactly one per reference, per the methodology.

| reference | verdict | why |
| --- | --- | --- |
| `CorralPeltzer/newTrackon` | **adopt** | `newtrackon/views.py:131` `/api/<int:percentage>` as an independent reliability oracle; `scraper.py:217` BEP 34 as an operator opt-out; the tracker's own `interval` as the politeness anchor |
| `Azathothas/TEMPLATE` | **adopt** | `docs/methodology/references.md` and `experiments.md`, already governing this document and `experiments/` |
| `Azathothas/bit-cli` | **adopt** | five mechanisms from the only reference here that speaks the tracker protocols: both `min interval` spellings, a negative count as unknown, a BEP 48 derivation that refuses to guess, a usable UDP retry budget, and the peer-id identity axis T-012 was missing. [`HISTORY/references/azathothas-bit-cli.md`](references/azathothas-bit-cli.md) |
| `Aseem0xff/pacman-static` | **adopt** (methodology) | the `git rev-parse`-in-a-stripped-corpus defect -- **which this session then hit itself** -- plus the self-corrections table and "clone your own output before believing it reproduces". [`HISTORY/references/aseem0xff-pacman-static.md`](references/aseem0xff-pacman-static.md) |
| `AvalynSouvlaki/T-244-RESEARCH` | **adopt** (methodology) | a corrections table needs a severity column; assert on a metric that is stable under changes you do not care about; and it is the argument behind `C-43`'s four crate names. [`HISTORY/references/avalynsouvlaki-t-244-research.md`](references/avalynsouvlaki-t-244-research.md) |
| `GerryFerdinandus/bittorrent-tracker-editor` | **adopt** | `torrent_miscellaneous.pas:393` `ValidTrackerURL` -- a real consumer's accept rule, and the only client-side parsing evidence obtained |
| `ngosang/trackerslist` | **confirms** -- with a caveat | the transport set and the reality of operator exclusion requests. **Not** adoptable as a methodology reference: it publishes no generator |
| `pkgforge-security/Trackers` | **anti-pattern exhibit** | kept on purpose. Its failure mode is silent and it is exactly what RULES 3.10 exists to forbid -- see below |
| `DeSireFire/animeTrackerList` | **confirms** | abandoned *and* the largest unique contributor. Uniqueness and maintenance are independent axes |
| `XIU2/TrackersListCollection` | **filed elsewhere** | its browser-UA choice belongs to the `C-43` / RULES 5.3 decision, not to aggregation design |

---

## The three findings that move the design

### 1. `C-26` was backwards, and it changes what this project can be

TODO/RULES.md `C-26` recorded -- as an **inference from three 404s** -- that
newTrackon exposes no machine-readable uptime endpoint. That inference decided
whether newTrackon could be *an oracle* or merely *a list of URLs*, which
HISTORY/reference-sweep.md correctly calls "the difference between a cross-check and a
mirror."

It is wrong. `references/CorralPeltzer__newTrackon/tree/newtrackon/views.py:131`:

```python
@app.route("/api/<int:percentage>")
def api_percentage(percentage: int, added_before: int | None = None) -> Response:
```

`percentage` is an **integer parameter**, not a path segment. Probing the
literal string `/api/percentage` asks for the tracker set of a percentage named
"percentage", and 404 is the correct answer to that question. Measured
(`experiments/20`):

| endpoint | entries |
| --- | --- |
| `/api/0` | 261 |
| `/api/50` | 82 |
| `/api/95` | 55 |
| `/api/100` | 15 |

Monotone non-increasing, as a real uptime filter must be.

Two further facts fell out of the same file, neither previously known:

* `/api/stable` **is** `api_percentage(95, added_before=...10 days)` -- "stable"
  means **>=95 % uptime and >=10 days since first seen**. Measured corroboration:
  `/api/95` returns 55 and `/api/stable` returns 53; the two-entry gap is the
  age filter.
* `/api/best` is a **301 to `/api/stable`**, and `C-23` missed it entirely.

**Consequence:** newTrackon can be a genuine independent oracle. This is the
single most valuable capability found in the sweep.

**The caveat that must travel with it.** newTrackon reaches its numbers by
**announcing** -- `scraper.py:232` `announce_http(url, thash=urandom(20))` and
`scraper.py:279` `announce_udp(...)` -- while this project stops at scrape and
never announces. Its "uptime" and this project's "live" **answer different
questions.** Any cross-check must say so; treating a disagreement as an error
would be comparing two methods and calling the difference a finding.

### 2. The politeness ceiling had no number behind it. Now it has two, and they agree

`C-38` anchored the politeness budget on "well-behaved clients re-announce
about every 30 minutes" -- a figure with no measurement behind it. Two
independent sources in this corpus replace it:

* **Code:** `newtrackon/tracker.py:163` sets `self.interval = 10800` (3 h) once
  a tracker's uptime reaches 0, and otherwise takes `interval` from the
  tracker's own response (`tracker.py:136-138`).
* **The maintainer, in `newTrackon` issue #334:** *"The current checking
  frequency (every ~3 hours) is reasonable for the server load. Trackers that
  have been down for more than 1.5 years are already automatically removed."*

A production monitor that does **more** work per check than this project plans
to (it announces; we scrape) runs at **~3 hours**. T-084's "approximately
hourly", and its suggestion to investigate 30 minutes, would be **3x and 6x**
that load respectively.

**Consequence:** the anchor is the tracker's own stated `interval`, and the
default cadence should be justified against ~3 h rather than assumed at 1 h.
This is recorded as a challenge to the brief's hourly cadence, not a silent
change; T-084 carries it.

### 3. The protocol table was mis-factored, and the error direction is dangerous

RULES 3.1 tabulates `udp`, `http`, `https`, `ws`/`wss`, `*.i2p`,
"yggdrasil hosts" and `*.onion` as seven values of one variable. Measured
(`experiments/19`, ngosang @ `1e61597`):

```
trackers_all_i2p.txt        schemes present: http (11), udp (2)
trackers_all_yggdrasil.txt  schemes present: http (1)
trackers_all_ws.txt         schemes present: wss (3)   -- not ws
```

An I2P tracker is an **ordinary `http://` or `udp://` URL whose hostname ends
in `.i2p`**. A classifier keyed on scheme sees `http://`, routes it to the
clearnet prober, the probe fails, and the tracker is recorded **dead** -- the
exact correctness bug RULES 3.1 forbids.

Independent confirmation of the transport set from a real consumer,
`torrent_miscellaneous.pas:393`:

```pascal
Result := (Pos('udp://', TrackerURL) = 1) or (Pos('http://', TrackerURL) = 1) or
  (Pos('https://', TrackerURL) = 1) or WebTorrentTrackerURL(TrackerURL);
```

-- five transports, matching the census exactly.

**Consequence:** the domain model carries **two axes**, transport x network.
Recorded limitation: yggdrasil addressed by *hostname*
(`http://yggtracker.i2p.rocks:80/announce`) is indistinguishable from clearnet
by URL alone; only the `_ip` variant exposes the `0200::/7` literals. Correct
classification needs DNS, which is a time-varying inference (the three dedup questions in src/trackers/dedup.py).

---

## Per-reference

Each reference has its own file under [`references/`](references/), carrying its
provenance table, the commit it was read at, and what could not be obtained. The
sections below are the same findings, kept here so the sweep reads as one
document.

### `pkgforge-security/Trackers` -- anti-pattern exhibit

The closest prior art, and archived. Read from
`.github/workflows/fetch_update_trackers.yaml` @ `7f2d00b`, not from its README.

**Its README is wrong about its own sources.** The README lists
`https://newtrackon.com/list` (an HTML page). The workflow fetches
`https://newtrackon.com/api/stable` (`text/plain`). The workflow's *comments*
are misaligned with its *commands* by one line, which is the likely origin of
the error the register inherited as `C-20`.

Corroboration that the code, not the README, is right: the published
`trackers_stable.txt` (57 entries) shares **52** entries with today's
`/api/stable` (53), and consists of bare URLs an HTML page could not yield
without a parser the repository does not contain.

**The silent failure mode, which is why this is kept as an exhibit.** Every
step is both `set +e` and `continue-on-error: true`, and each source is fetched
with `curl -qfSL ... -o FILE`. `curl -o` **truncates the output file before the
transfer**, and `-f` makes it produce nothing on an HTTP error. So a failed
fetch leaves an **empty file**, which is then concatenated with `sort -u` into
the published lists. An entire source disappears from the output and **nothing
reports it**. That is RULES 3.10's "source failed" vs. "source returned zero
trackers" invariant, violated in production, in the project this one exists to
improve on.

Two further observations:

* `sort -u` **destroys ngosang's popularity ordering**, so the derivative is
  strictly worse-ordered than its input while advertising the same content.
* `reset_commits.yaml` implements the >5000-commit orphan-branch reset that
  T-081 describes. It is safe *here* only because the repository
  stores no history worth losing -- which is precisely RULES 3.7's point.

**Why it was archived: not recoverable from the tracker.** All 24 tracker items
are **pull requests from `dependabot` and `renovate`**; there are **zero human
issues**. The tracker records no reason for archival. What it does record is the
maintenance cost the design actually generated: 100 % dependency churn against
three tag-pinned actions, over roughly two and a half years.

### `ngosang/trackerslist` -- confirms, with a caveat

**It publishes no generator.** The complete tracked file list is 16 entries:
`LICENSE`, `README.md`, `_config.yml`, `.github/FUNDING.yml`, `blacklist.txt`,
and 11 output `.txt` files. No workflow, no script, no code.

`C-22`'s prescribed verification -- "read the generator source, determine how
popularity is actually computed" -- **cannot be performed by anyone.** The
"sorted by popularity and latency" claim is unauditable. That is the finding,
and it is the decisive input to HISTORY/reference-sweep.md's architecture question:
consuming this list means inheriting filtering decisions nobody can inspect.

**`trackers_all.txt` silently excludes three networks.** Measured: 99 =
udp 48 + http 37 + https 14, exactly. Every `ws`, `i2p` and `yggdrasil` entry --
17 trackers -- is absent, and the file does not say so.

**`blacklist.txt` is the most useful thing in the repository** (346 entries):

| reason | count |
| --- | --- |
| registered torrents | 178 |
| duplicate of `<url>` | ~90 |
| malfunction | 11 |
| deprecated by owner | 7 |
| detected by antivirus software | 5 |
| **requested by sysadmin** | **2** |

The last row matters for RULES 4: **tracker operators do ask to be
removed, and a real upstream honours it.**

**A three-way disagreement worth publishing.** `http://bt.okmp3.ru:2710/announce`
is blacklisted here as *"fake seeds"*, listed live by newTrackon, and proved a
working tracker by this project's own runner probe (`experiments/05`). Three
sources, three different answers, none of them wrong about the question it was
asking. This is the shape of the output RULES 3.4 calls the most
informative thing this dataset could publish.

Independent corroboration of the same pattern: newTrackon issue #353 reports
`torrent.tracker.durukanbal.com` returning implausible peer counts -- and
ngosang's blacklist carries that exact tracker as *"fake seeds"*. Two projects,
independently, same conclusion.

### `CorralPeltzer/newTrackon` -- adopt

Covered above. Three mechanisms adopted:

1. `views.py:131` -- `/api/<int:percentage>` as an independent oracle.
2. `scraper.py:217` `get_bep_34` -- **BEP 34 DNS opt-out**. A tracker operator
   publishes a `BITTORRENT` TXT record and a monitor removes them
   automatically. This turns RULES 4's "operator requests exclusion" from an
   email address into a **standard, automatable** mechanism, and it is
   registered as `C-51`. **Adopted as a decision, not yet as code** -- review 5
   found that nothing in `src/` reads a TXT record, while the README described
   the route in the present tense. [T-032](../TODO/measurement.md) is the
   implementation and it is P0.
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

### `GerryFerdinandus/bittorrent-tracker-editor` -- adopt

**HISTORY/reference-sweep.md mischaracterises this reference.** It describes `/source` as
"a registry of tracker-list sources". `/source` is the **Free Pascal
application source tree** (`code/`, `project/`, `test/`) of a desktop GUI
torrent editor. The registry does exist, but as *code*:
`ngosang_trackerslist.pas` and `newtrackon.pas` enumerate the source URLs.

This is the **only client-side parsing evidence** the sweep obtained, and it is
the closest thing to an answer for `C-40` / `C-41`.
`torrent_miscellaneous.pas:174` `SanitizeTrackerList`:

1. `UTF8Trim` each line -- **surrounding whitespace is tolerated**;
2. find the first space and **truncate everything after it** -- so a trailing
   `" # reason"` comment is stripped, which is exactly what lets this client
   consume ngosang's `blacklist.txt` directly;
3. `ValidTrackerURL` then accepts only the five known transport prefixes, so a
   whole-line `# comment` is rejected as an invalid URL rather than accepted as
   a tracker.

**What this supports:** in *this* client, `#` comments do not break parsing.
**What it does not support:** the general claim in `C-41`. One client is not
"clients", and the tolerance here is partly incidental -- the list is loaded via
`TStringList.DelimitedText`, which splits on whitespace, so a comment becomes
several tokens that each fail validation. T-001's conservative
no-comments rule stands.

Two more observations, both useful:

* `RandomizeTrackerList` (`torrent_miscellaneous.pas:207`) **shuffles the
  list**. At least one real consumer destroys upstream ordering outright, which
  bounds how much the "sorted by popularity" property in `C-22` is actually
  worth to consumers.
* `ngosang_trackerslist.pas:98` -- on any download exception the handler runs
  `FTRackerList[...].Clear`. **Source failure becomes zero trackers**, the same
  conflation as the pkgforge exhibit, in a different codebase.

### `DeSireFire/animeTrackerList` -- confirms

Last push **2024-01-12** (~2.6 years). Not archived. 43 human issues, nearly
all "new tracker" submissions, which is a live *audience* around a dead
generator.

HISTORY/reference-sweep.md asks whether its unique entries are real. Measured
(`experiments/19`): **995 unique URLs of 1091**, against every other *primary*
source combined. It is by a wide margin the largest unique contributor in the
corpus.

**A defect in my own first measurement, recorded because it is instructive.**
The first run of `experiments/19` reported this source as contributing **0**
unique trackers. That was an artefact: the comparison set included
`pkgforge_all`, which is a **strict superset** of this source (1091 of 1091).
Comparing a source against its own downstream copy always shows redundancy.
Sources now carry a `role` and the arithmetic runs over primaries only
(`C-52`).

**What is still unknown, and it is the half that decides the question:**
whether those 995 unique entries are *alive*. Uniqueness measured; liveness
not. That is P2 work, and until it is done "abandoned but unique" is not yet
"abandoned but valuable".

### `XIU2/TrackersListCollection` -- filed elsewhere

Daily at 00:00 UTC, `contents: write`, `timeout-minutes: 45`, and -- better than
the pkgforge exhibit -- `concurrency.cancel-in-progress: false`, which **queues**
rather than cancelling a run in flight.

Sources: ngosang `trackers_all`, `newtrackon /api/live`, DeSireFire `AT_best`,
and `http://github.itzmx.com/...` over **plain HTTP**.

Its workflow header states, in the maintainer's own words, that after migrating
to Actions "the filtering process has been temporarily streamlined" -- an honest
admission that the published quality is currently below its own historical bar.

It sets a **browser-like User-Agent** (`Mozilla/5.0 ... Chrome/69`). That is a
data point for RULES 5.3 / `C-43` and is **filed there**, not adopted:
this sweep fetched every source in the census with one honest descriptive
User-Agent and received **zero 401/403 responses**, so no impersonation is
warranted.

### `Azathothas/TEMPLATE` -- adopt

0BSD, matching this project. `docs/methodology/references.md` read in full and
followed here; `experiments.md` governs `experiments/`. Fifteen methodology
documents exist, against the three named in HISTORY/reference-sweep.md -- `gate.md`,
`work-stages.md`, `reviews.md` and `history.md` are relevant and unread.

Its rules that changed this sweep's output: keep the corpus **tracked** (it was
initially in session-local scratch -- the exact failure the document names twice);
read the **tracker**, not only the code; and open the write-up with what was not
established.

---

## Round 2 findings, 2026-08-31

### 4. The project's own corpus figures did not come from its own instrument

Not a finding about a reference -- a finding the re-mine forced, because
re-running `experiments/19-scheme-census.py` to check the refreshed trees
produced numbers the documents did not carry.

**Three mutually contradictory corpus figures were in circulation** across
`TODO/`, `HISTORY/` and `src/`, and none matched the eight committed result
files, which all say the same thing. One of the prose sets did not even sum to
its own total. [`corpus-baseline.md`](corpus-baseline.md) is now the single
sourced home for every corpus figure, RULES 2.1 is the rule that came out of
it, and `scripts/check-citations.py` fails on a retired figure.

**Why it belongs in a reference sweep.** The methodology's own trap list says
*"believing a document over its code"*, and names other people's documents. The
same trap applies to this project's documents, and it had it worse: a stale
README is somebody else's problem, while an unsourced number in your own
`PROGRESS.md` is the number the next session sizes its experiment with.

### 5. T-012 was measuring half its question

`Azathothas/bit-cli`'s `crates/bit-cli-core/src/peer_id.rs` picks its
two-character BEP 20 client code against a 92-entry registry so that it is not
filed under another client's statistics. That is the mechanism, and the
consequence for this project is that **an HTTP tracker request carries two
identity fields, not one** (`C-63`): the `User-Agent` header and the Azureus
prefix inside `peer_id`. A UA-only experiment reporting "no block" cannot be
distinguished from "we happened to send an acceptable `peer_id`". T-012's
design now crosses both axes.

**And the effect was then observed first-hand** (`C-64`): every request to the
read proxy carrying this project's descriptive User-Agent returned HTTP 420
with an empty body, while `curl/8.5.0` returned 200 for the identical request
in the same second. Not a tracker, so `C-56` is still open -- but "nobody
refuses us" is no longer the measured position it was in round 1.

### 6. A derivation that guesses is a measurement of the guess

`bit-cli` refuses to derive a scrape URL from an announce URL that does not
follow BEP 48, and says why: *"guessing one produces a 404 that reads like a
tracker failure."*

Checking our own implementation against that found the same class of defect in
it: `Tracker.scrape_url` replaced the first `announce` **anywhere** in the
path, turning `/announcements/feed` into `/scrapements/feed` -- an endpoint no
tracker serves, whose 404 would then have been recorded against the tracker.
Fixed, with the test. No such path is in the corpus today, which is why it
would have surfaced as a mystery rather than as a bug.

**This is the third time in this project that reading a reference found a
defect in our own code** rather than in theirs, which is the argument for the
sweep being work rather than reading.

### 7. `git rev-parse` in a stripped corpus answers wrongly rather than failing

`Aseem0xff/pacman-static` records it as its own correction #8: once a corpus
tree loses its `.git`, `git -C <corpus> rev-parse HEAD` **walks up** and
returns the enclosing repository's HEAD. *"A provenance line that is
confidently wrong is worse than a missing one."*

**This session then hit it.** Re-mining the three moved trees, a `rev-parse`
run after stripping `.git` returned this repository's own HEAD three times,
once per reference. It was caught only because the real SHAs had been captured
from the clone output a step earlier and did not match. That is the whole
reason the corpus's ordering rule is *capture the commit before stripping*, and
it is now written down with the evidence.

---

## What changed as a result

| # | change | driver |
| --- | --- | --- |
| 1 | Domain model becomes **transport x network**, not one enum | finding 3 |
| 2 | newTrackon promoted from *source* to **oracle**, with a documented methodology caveat | finding 1 |
| 3 | Politeness anchor becomes the tracker's own `interval`; the brief's hourly cadence challenged against ~3 h | finding 2 |
| 4 | **BEP 34** adopted as the operator opt-out mechanism (`C-51`) | newTrackon `scraper.py:217` |
| 5 | "Source failed" != "zero trackers" promoted from invariant to **regression test**, with two real codebases as the exhibit | pkgforge + tracker-editor |
| 6 | Blacklist reasons treated as **evidence to publish**, not noise to drop | ngosang `blacklist.txt` |
| 7 | Unique-contribution arithmetic excludes derivative sources | `C-52` |
| 8 | Every corpus figure moves to one sourced file; prose figures deleted | round 2, finding 4 |
| 9 | T-012 crosses **two** identity axes, UA and `peer_id` | `C-63`, bit-cli |
| 10 | `min interval` read in both spellings; the scheduler prefers the floor | `C-65`, bit-cli |
| 11 | BEP 48 derivation anchored on a path component, with the test | `C-66`, bit-cli |
| 12 | The UDP retry budget has an arithmetic (`5 x max(t/3, floor)`) instead of a gap | bit-cli `docs/trackers.md` |
| 13 | Every citation is machine-checked, including line numbers into the corpus | T-121 |

---

## Known-weak claims in this document

Read these before the recommendations above.

* **`C-40`/`C-41` rest on one client's source, not on running any client.** The
  strongest statement supported is "comments do not break *this* parser".
* **`experiments/19`'s network counts under-report yggdrasil**, by construction.
* **The three-way `bt.okmp3.ru` disagreement is n=1.** It is a good
  illustration and not yet a measured rate of cross-source disagreement.
* **`995 unique of 1091` is a string comparison**, not a liveness measurement.
* **`~3 hours` is one maintainer's statement about one service.** It is better
  evidence than the unsourced 30 minutes it replaces; it is not a law.
* **Every network number here is from datacenter address space.**
* **`C-64` is one observation against one intermediary**, not a tracker, and
  not a rate. It moves a prior; it settles nothing.
* **`bit-cli`'s tracker was not read**, so its maintainer's rulings on anything
  not written into the tree are unseen.
* **Round 2 re-mined three trees and read three new ones. It did not re-read
  the four unchanged trees**, on the grounds that `git ls-remote` showed them
  identical. That is a correct argument about the *code* and a weaker one about
  the *tracker*, which moves without the tree moving.

**Assume more remain.**
