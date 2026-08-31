# Corrections

**What this project got wrong, and what the evidence replaced it with.**

Two kinds of correction live here. The first is what the retired design brief
got wrong, corrected while it still existed: each block keeps the original
wording with the correction underneath, so a disproved premise keeps its title
(RULES 7). Those blocks are reproduced **verbatim**, because deleting the file
they lived in would otherwise delete the record of what was wrong.

The second is what **this repository's own documents** got wrong. That table is
below and it is the more useful half, because a stale claim in somebody else's
brief costs a reader a paragraph while an unsourced number in `PROGRESS.md`
becomes the number the next session sizes an experiment with.

Section numbers in the verbatim blocks cite the retired brief. **They are
provenance, not paths** -- those documents were never committed to this
repository and cannot be recovered from it
([`idea-coverage.md`](idea-coverage.md)).

---

## What this repository's own documents got wrong

⚠ **Run ids recorded before 2026-09-01 do not resolve.** They belonged to this
repository's prior history, which is gone, so their Actions URLs 404 and their
artefacts went with them. The ids are kept because they are what was measured
at the time; the evidence that survives is the result JSON committed under
`experiments/results/`, and `C-44` records what that cost.

Read this before trusting anything else here. The severity column is borrowed
from `AvalynSouvlaki/T-244-RESEARCH`, whose revision table makes the point that
"I mis-stated a hash format" and "I recommended against the right tool" are not
the same finding and should not look the same in a list.

### Round 3, 2026-09-01

The adoption pass. Every row was found by a check that did not exist before it,
or by re-running an instrument rather than reading the number beside it.

| # | what the documents or the code said | what measurement said | severity |
| --- | --- | --- | --- |
| 1 | The pipeline publishes a validated tracker list | ⛔ **It publishes six private-tracker credentials.** Seven announce URLs in `trackers_all.txt` carry a passkey belonging to a real person, from two upstreams, and nothing between the fetch and the output refuses them. `C-70`, `T-107` | **Critical** -- a tracker aggregator that republishes a passkey hands a stranger's credential to every consumer, and the tracker it belongs to sees every use of it |
| 2 | `experiments/01` reported `tcp_ports_blocked: [2710]` on both runner images | ⛔ **Nothing was blocked.** The verdict counted any failed TCP row as a blocked port, and `bt.okmp3.ru` had stopped resolving, so 2710 was never attempted. Experiment 04 recorded the same host as NXDOMAIN in the same run. Corrected to classify a resolution failure separately, and re-run: `tcp_ports_blocked: []`, `tcp_targets_unresolvable: ["bt.okmp3.ru"]`. `C-71` | **High** -- it reads as "GitHub blocks the classic BitTorrent tracker port", which is a platform claim this project would have sized work against |
| 3 | Two instruments wrote their results with `open(path, "w", encoding="utf-8")` | ⚠ **Python translates to the platform newline**, so the same instrument produces different bytes on Windows and on a runner. A committed result is evidence and evidence whose bytes depend on who ran it cannot be diffed against the next run. RULES 15.5 already required the fix | Medium -- invisible until a contributor ran an instrument, which nobody had |
| 4 | `scripts/generate.py` printed its output path with `os.path.relpath` | ⚠ **That raises on Windows across drives.** On a GitHub Windows runner the checkout and the scratch directory are on different drives, so generating into scratch killed the run after every check had passed. Found by the Windows leg of the gate on its first run | Medium -- the last line of a successful run is not worth failing a build for |
| 5 | `check-citations.py` and `check-todo.py` resolved every markdown link | ⚠ **Including inside a code span.** `[int](2.65)` in a note about PowerShell rounding was reported as a broken link to a file called 2.65. Markdown does not linkify inside backticks | Medium -- a checker that cries wolf is a checker somebody switches off |
| 6 | The character allowlist covers every tracked text file this project owns | ⚠ **It exempted `references/PROVENANCE.md`**, which is this project's own writing rather than captured upstream source, because the exemption was written as the whole directory. The exemption is per captured repository now | Medium -- an exemption is invisible to the guard it exempts from |
| 7 | The gate can be run and the repository left clean | ⚠ **Running it wrote a timestamped result into `experiments/results/`**, so RULES 10.3 step 6 could never be satisfied. The census writes to scratch now, and CI asserts the tree is clean after the gate | Medium |
| 8 | "1213 characters outside the five, 741 of them em dashes", in three documents | ⚠ **Those are line counts, not character counts.** `check-markers.py` reports the first offender per line. Re-derived: **1655 characters across 55 files, 840 of them em dashes.** The instrument was right and the prose around it described a different quantity | Medium -- found by [`reviews/2026-09-01-03-claim-audit.md`](reviews/2026-09-01-03-claim-audit.md), and nothing mechanical would have caught it |

### Round 2, 2026-08-31

| # | what the documents said | what measurement said | severity |
| --- | --- | --- | --- |
| 1 | The corpus is **1510** distinct URLs, with `http` 780/946, `udp` 457/448, `https` 260/254 -- quoted across `TODO/`, `HISTORY/` and `src/` | ⛔ **No committed instrument ever produced any of it.** All eight committed runs of `experiments/19-scheme-census.py`, over two days, report **1346 / 723 / 251 / 362 / 10**. One prose set did not sum to its own total (946+254+448+17 = 1665). Repaired from the instrument into [`corpus-baseline.md`](corpus-baseline.md) | **Critical** -- every sizing decision, the value-gate arithmetic and the politeness budget rested on it |
| 2 | "Recoverable if you need the original wording: `git log --diff-filter=D -- IDEA.md`", in both `docs/AGENTS.md` and `idea-coverage.md` | ⛔ **Returns nothing, and always has.** Those files were never committed to this repository. A future session would have chased a git history that does not exist | **High** -- it is the first thing an orienting session would try |
| 3 | The coverage table mapped "**101 of 101**" brief sections, "generated by walking the headings of the actual files" | ⛔ **97 sections exist.** Four rows named sections present in neither retired document, and the count was inflated by exactly those four | **High** -- it is the file a session trusts *instead of* re-checking |
| 4 | `references/PROVENANCE.md` named `Azathothas/pacman-static` and recorded its licence | ⛔ **That repository does not exist** (404). The real one is `Aseem0xff/pacman-static` (200), and it is now in the corpus, read, with a verdict. The operator's own username had been substituted into a third-party reference | **High** -- an unfollowable citation teaches a reader to stop following the rest |
| 5 | `Tracker.scrape_url` derives a BEP 48 endpoint by replacing the first `announce` in the path | ⛔ **`/announcements/feed` became `/scrapements/feed`** -- an endpoint no tracker serves, whose 404 would have been recorded against the tracker. Anchored on a path-component boundary, with the test | **High** -- a confidently wrong measurement, and no such path is in the corpus today so it would have surfaced as a mystery |
| 6 | `PROVENANCE.md` pointed at a *docs/research/reference-sweep.md*; `normalize.py` pointed at a *tests/test_normalize.py* | ⛔ **Neither path exists.** The sweep is at `HISTORY/reference-sweep.md`; the tests are `tests.test_p1.TestNormalizationRules` | Medium |
| 7 | `experiments/05` identifies this project as `github.com/AvalynSouvlaki/trackers` in the User-Agent it sends to real trackers | ⛔ **Wrong owner.** Every other instrument says `Azathothas/`. The committed run results show the wrong string went out on a runner; they are evidence and are **not** rewritten | Medium -- the contact route in a probe's identity must reach *this* repository |
| 8 | RULES 12: "Python 3.12+" | ⚠ **Nothing required or checked 3.12** and the suite passes on 3.11. A floor nobody enforces is documentation; one set above what the code needs excludes contributors for nothing. Floor is now 3.11 and `src/trackers/__init__.py` enforces it | Medium |
| 9 | `HISTORY/corrections.md`'s own header: "The brief (the design brief) and the operating contract (TODO/RULES.md) were retired" | ⚠ **A botched find-replace.** `RULES.md` is the *current* rules file, not the retired operating contract, and the `git log` command it printed was not a command. Rewritten | Medium -- the file that records errors was itself garbled |
| 10 | T-005: "12 `wss` entries and 1 `ws` entry exist in the corpus"; `registry.py`: "`ws` occurs exactly once across the union" | ⚠ **10 `wss`, and `ws` occurs zero times in the union.** The corpus's single `ws://` is a *blacklisted* entry, which is a different claim | Low -- the conclusion (`wss` is the live form) survives |
| 11 | RULES 10.1b: "There are 61 entries" | ⚠ 63. A count typed by hand in the file that forbids counts typed by hand. Replaced with a pointer to `check-todo.py` | Low |
| 12 | RULES 13.2 cites "the proxies in section 5.3" | ⚠ 5.3 is 401/403 handling. The proxies now have their own section (RULES 16) | Low |
| 13 | Three line citations into the corpus: `ngosang_trackerslist.pas:93`, `views.py:168`, `torrent_miscellaneous.pas:206` | ⛔ **All three point at the wrong line** -- the `.Clear` is on 98, the `api_percentage(95, ...)` on 170, the `RandomizeTrackerList` on 207. Each cited line exists, so the existence check passed all three; two are blank lines and one is a comment | **High** -- a citation a reader follows and does not find is worse than none, and this class is invisible to every check that only asks whether the line exists |
| 15 | **Eighteen entries cited themselves** as their own `Source:` -- `T-080`'s source was "T-080" | ⛔ Produced by a substitution pass that rewrote the brief's section number into the id of the entry replacing it, **destroying the provenance the edit existed to preserve**. Every one now names the brief section it came from, and `check-todo.py` fails on a self-citing source | **High** -- provenance is the whole argument for retiring the brief; eighteen entries had none |
| 16 | Thirty-three bare `sectionn` references survived in `src/`, `scripts/`, `TODO/` and `HISTORY/` | ⚠ The first repair pass matched `IDEA sectionn` and missed the bare form, which resolves to nothing at all. `check-citations.py` now rejects a bare `sectionn` unless the line frames it as provenance ("the brief's section 8.2") | Medium |
| 17 | This session's own first attempt to repair 16 | ⛔ **A blind substitution produced worse text than it replaced** -- `normalize.py`'s docstring opened by citing itself, a reference's *own* section 9 became "RULES 4", and `model.py` said RULES 3.1 had tabulated something written years before RULES existed. Reverted whole and redone by hand, one line at a time | **High**, and it is the same defect as 15 -- *"a tool answering confidently where it should have failed"*, committed by the session repairing that exact class of error |
| 14 | The round-1 sweep read `newtrackon/scraper.py` at lines 217 and 232 | ⛔ **It read past `:53` and `:234`**, which are the two most decisive lines in the file: newTrackon sets `User-Agent: qBittorrent/4.3.9` and builds `peer_id` as `-qB4390-`. **The strongest available evidence on T-012's question was already in the corpus, unread** (`C-68`) | **High** -- T-012 is P0 and has been sized without it |

**Claims 1, 3 and 7 are the same defect in three costumes: a number or a name
written with the confidence of a measurement and no instrument behind it.**
That is why RULES 2.1 and 3.11 now exist and why
`scripts/check-citations.py` fails on a retired figure -- a rule with no gate is
a rule the next session optimises away.

| 18 | This session's own commit messages: *"every gate green"*, six times | ⛔ **CI was red for all six.** `RULES.md` linked `HISTORY/reviews/`, which was an **empty directory**: git does not track one, so it existed on the machine that made it and in no clone. `check-todo.py` resolved the link against the local filesystem and passed; CI resolved it against a fresh checkout and failed. It went green again only when the first review file made the directory non-empty by accident. `check-citations.py` now rejects a cited empty directory | **Critical** -- the claim was false in the record, and the class is precisely `Aseem0xff/pacman-static`'s correction #9, *"clone your own output before believing it reproduces"*, which this session had **written up as a lesson** in the same hour it committed the defect six times |

| 19 | `HISTORY/gates.md`'s definition-of-done checklist | ⚠ **Three items disagreed with the entries they name.** Two were unticked while their entry (`T-021`, `T-025`) was `done`; one said `T-064` was *blocked* when RULES 13.1 had unblocked it. Its i2p and `ws`/`wss` counts were also the retired figures (14 and 13; the census says 13 and 10). `check-todo.py` now fails when a checklist item and its entry disagree | **High** -- the checklist is what a reader consults to decide whether the project is finished, so an item that under-reports progress is as misleading as one that over-reports it |

| 20 | 46 of 63 entries had a `Prove:` that is prose rather than a command, including four in the current work order | ⚠ The work model requires the acceptance to be **a command**; `bit-cli`'s mining guide states the cost as *"a 'prove' with no command is a paragraph"*. T-028's read *"a cross-check report whose header states the methodology difference"* -- satisfiable by writing a markdown file, with nothing distinguishing having done the cross-check from having described one. The six in the work order are fixed by hand; the other 40 are T-123 | **High** -- it lands on the next session, on the items it was told to start with |

| 21 | The README told tracker operators, in the present tense, that publishing a BEP 34 `BITTORRENT` TXT record would stop this project | ⛔ **Nothing in `src/` reads a DNS TXT record.** There is no BEP 34 code path at all. It was the route offered *first* and the only one needing no contact, so it is the one an operator who does not want to talk to us would use -- and RULES 4.1 withdrew the descriptive-User-Agent requirement partly on the argument that BEP 34 serves that end better, which only holds if it is honoured. `C-51` verified the mechanism in **somebody else's** code and the sweep listed it under *mechanisms adopted*, overstating a decision as an implementation | **Critical** -- a commitment to a third party, in the document a third party reads, about the treatment of their server. Mitigated only by the probe never having been pointed at the corpus: a documentation defect today, a conduct defect the first time a corpus probe runs. Now [T-032](../TODO/measurement.md), P0, and RULES 4 forbids a corpus-wide probe until it lands |

| 22 | Three of 1337 published plaintext lines contained a character no URI may hold | ⚠ A stray `"` -- an HTML attribute terminator leaked by somebody's scraper -- and two `authkey=...\|...\|...` query strings. All three reached the primary output, and **both characters are shell-significant in the `curl \| client` idiom this project's own README recommends**. Found by running the emitted file through RFC 3986's character set rather than by reading it. `normalize.parse` now refuses them with the offending character named; 1337 -> 1334 | **Medium** -- the plaintext is the compatibility-critical format for the primary audience, and this project publishes hostile input to consumers who trust it |

| 23 | `references/` held 994 files on the capturing machine and **883 in every clone** | ⛔ **111 corpus files existed nowhere but one disk**, silently: ignored files do not appear in `git status`. Two independent causes, neither ours alone -- the captured `bit-cli` tree's *own* `.gitignore` dropped all 91 `bench/` results (which `references/PROVENANCE.md` explicitly listed as **kept**), and this repository's unanchored `out/` reached down into `references/Aseem0xff__pacman-static/tree/experiments/out` and dropped 20. Ten upstream `.gitignore` files and one `.gitmodules` promising an uncaptured submodule are now trimmed, our own rules are anchored to the repository root, and `scripts/check-corpus-integrity.py` gates it | **Critical** -- `references/PROVENANCE.md` states the corpus's whole purpose as *"could somebody who distrusts the write-up re-run every load-bearing claim without asking anyone?"*, and for a week the answer was no for 111 files. That no citation happened to land in one is luck; `bench/` holds this corpus's only prior announce timings |

**Claims 2, 4, 6 and 13 are the same defect in four costumes**: a citation that
looks like evidence and does not lead where it says. All four are now caught
mechanically -- 13 needed a second instrument, because a line number that exists
and says something else passes every check that only asks whether it exists.
`experiments/fixtures/load-bearing-citations.tsv` pins the substring each
load-bearing line must contain, and it found two of the three the moment it was
written.

**Claim 14 is a different failure and the more expensive one.** Nothing was
wrong; something was *not read*. No checker finds that, and the only defence is
the methodology's own rule that a reference gets at least three passes each
asking a different question -- which round 1 did not do for `scraper.py`.

**Claim 21 is the worst one.** Every other entry here costs a reader time or a
session an hour. That one made a promise to somebody outside this project about
how their server would be treated, and the code did not keep it. It was found
by asking the code what it can emit rather than by reading the policy that says
what it may.

**Claims 18 and 23 are the two worth reading twice, and they are one claim.**
Both are the gap between *this machine* and *a clone*. 18 was an empty
directory, which git cannot carry; 23 was 111 files git was told not to carry.
Neither was found by a check this session added -- 18 came from *looking at CI*,
which RULES 10.3 step 8 requires in exactly those words (**"Not 'should be' --
look."**), and 23 from counting `find` against `git ls-files` on a hunch after
18. A gate that runs on the author's filesystem answers a different question
from the same gate running on a clone, and the difference is invisible until
you look at the clone.

Both are also `Aseem0xff/pacman-static`'s correction #9, *"clone your own output
before believing it reproduces"* -- which this session had already read, written
up in `HISTORY/references/aseem0xff-pacman-static.md`, and then committed twice
anyway. **Knowing a defect class is not a control for it.** The controls are
`check-citations.py`'s empty-directory check, `check-corpus-integrity.py`, and
RULES 10.3 step 9's fresh-clone run; the lesson is that the write-up was never
going to be enough.

**What found them.** Claim 1 came from re-running an instrument rather than
reading the number beside it. Claims 2 and 4 came from *following* a citation
instead of accepting it. Claim 5 came from reading a reference's implementation
of the same rule. Claim 18 came from opening CI. **None was findable by reading
the documents alone**, which is the argument for RULES 1.1 being about this
project's documents and not only other people's.

**Assume more remain.**

---

## The correction blocks, verbatim

### ⚠ CORRECTED 2026-08-29 -- half right, and the wrong half was the accusation

*Original wording kept above.* Read from
`references/pkgforge-security__Trackers/tree/.github/workflows/fetch_update_trackers.yaml`
@ `7f2d00b`, not from the README:

* **"No health checking, no scoring, no provenance, no validation" -- CONFIRMED.**
  The pipeline is `curl` -> `cat` -> `sort -u` -> `dos2unix` -> auto-commit.
* **"One of its three sources is an HTML page" -- FALSE.** The workflow fetches
  `https://newtrackon.com/api/stable`, which is `text/plain`. It never touches
  `/list`. The README says `/list`; the *code* says `/api/stable`, and the
  code is what runs. The workflow's own comments are misaligned with its
  commands by one line, which is the likely origin of the error.
  Corroboration: its published `trackers_stable.txt` (57 entries) shares 52
  entries with today's `/api/stable` (53), and is bare URLs that an HTML page
  could not yield without a parser the repository does not contain.

**The more serious finding, which the original description missed entirely.**
Every step is both `set +e` **and** `continue-on-error: true`, and each source
is fetched with `curl -qfSL ... -o FILE`. `curl -o` truncates the output file
*before* the transfer and `-f` emits nothing on an HTTP error, so **a failed
fetch leaves an empty file** which is then concatenated into the published
lists. An entire source vanishes from the output and nothing reports it --
section 11's "source failed" vs. "source returned zero trackers" invariant, violated
in production, by the closest prior art. This is why it is kept as an
anti-pattern exhibit rather than dismissed.

Also: `sort -u` **destroys ngosang's popularity ordering**, so the derivative
is strictly worse-ordered than its own input while advertising the same
content.

---

### ✅ ANSWERED 2026-08-29 -- it IS obtainable. `C-26` was backwards.

*Question kept above.* `references/CorralPeltzer__newTrackon/tree/newtrackon/views.py:131`:

```python
@app.route("/api/<int:percentage>")
```

`percentage` is an **integer parameter**, not a path segment. The earlier
`/api/percentage` -> 404 that `C-26` was inferred from was a **false
negative**: it asked for the tracker set of a percentage literally named
"percentage", and 404 is the right answer to that question.

Measured (`experiments/20-newtrackon-api-surface.py`): `/api/0` -> 261,
`/api/50` -> 82, `/api/95` -> 55, `/api/100` -> 15. Monotone non-increasing, as
a real uptime filter must be.

Two definitions recovered from the same file, neither previously known:
`/api/stable` **is** `api_percentage(95, added_before=...10 days)`, i.e. **>=95 %
uptime AND >=10 days since first seen**; and `/api/best` is a **301 redirect**
to `/api/stable`. `C-23`'s endpoint list missed both.

**So newTrackon can be an oracle -- a cross-check, not a mirror.** That is the
most valuable capability found in the whole sweep.

**The caveat that must travel with every comparison.** newTrackon reaches
those numbers by **announcing** (`scraper.py:232` `announce_http(url,
thash=urandom(20))`, `scraper.py:279` `announce_udp(...)`), while this
project stops at scrape and never announces (section 9.3). Its "uptime" and this
project's "live" **answer different questions.** A disagreement between them
is a methodology difference first and a finding second, and any published
cross-check must say so.

One more, from issue #324 and the maintainer's reply: newTrackon reports **one
preferred protocol per tracker** (UDP first, then HTTPS, then HTTP). `/api/udp`
is therefore *not* "trackers that support UDP" but "trackers whose preferred
protocol is UDP" -- so it must not be compared to a per-endpoint measurement.

---

### ⚠ CORRECTED 2026-08-29 -- `/source` is not a registry; it is an application

*Original wording kept above.* `references/GerryFerdinandus__bittorrent-tracker-editor/tree/source/`
@ `c5f5b82` contains `code/`, `project/` and `test/`: it is the **Free Pascal
source tree of a desktop GUI torrent editor**, not a list of tracker-list
sources.

The registry does exist -- but as *code*, in `source/code/ngosang_trackerslist.pas`
and `source/code/newtrackon.pas`, which enumerate the source URLs the
application fetches (ngosang `trackers_all{,_http,_https,_ip,_udp,_ws}.txt`
plus `blacklist.txt`; newTrackon `/api/{add,all,http,live,stable,udp}`).

**Its real value to this project is something else entirely: it is the only
client-side parsing evidence available**, and therefore the closest thing to
an answer for `[C-40]` and `[C-41]`.
`source/code/torrent_miscellaneous.pas:174` `SanitizeTrackerList`:

1. `UTF8Trim` each line -- **surrounding whitespace is tolerated**;
2. find the **first space** and truncate everything after it -- so a trailing
   `" # reason"` comment is stripped, which is exactly what lets this client
   consume ngosang's `blacklist.txt` directly;
3. `ValidTrackerURL` (`:393`) then accepts only `udp://`, `http://`,
   `https://`, `ws://`, `wss://` -- so a whole-line `# comment` is rejected as
   an invalid URL rather than accepted as a tracker.

**What that supports:** in *this* client, `#` comments do not break parsing.
**What it does not support:** the general claim. One client is not "clients",
and the tolerance is partly incidental -- the body is loaded via
`TStringList.DelimitedText`, which splits on whitespace, so a comment becomes
several tokens that each fail validation. section 17.1's conservative no-comments
rule therefore stands.

Two further observations worth keeping:

* `RandomizeTrackerList` (`:206`) **shuffles the list**. At least one real
  consumer destroys upstream ordering outright, which bounds how much
  ngosang's "sorted by popularity and latency" `[C-22]` is worth downstream.
* `ngosang_trackerslist.pas:98` -- on any download exception the handler calls
  `FTRackerList[...].Clear`. **Source failure becomes zero trackers**, the
  same conflation as the pkgforge exhibit, in an unrelated codebase. Two
  independent occurrences is why section 11's invariant gets a regression test here
  rather than a paragraph.

---

### ✅ GATE ANSWERED 2026-08-29 -- **PASSED.** Measurement well above the DNS floor is possible.

Evidence: workflow run
`33246108348`,
two runner images, instruments `experiments/01`, `02`, `05`.

The gate asks whether anything more meaningful than "the hostname resolves"
is reachable. Measured, the ladder of section 9.1 reaches its **top rung** on both
measurable transports:

| transport | rung reached | evidence |
| --- | --- | --- |
| `udp` | **protocol-valid** (BEP 15 connect, connection id returned) | 9/11, 8/11, 9/11, 9/11 across four runs; loopback control passed every run |
| `http`/`https` | **tracker-semantic** (well-formed bencoded scrape response) | 5/6 subjects; positive **and negative** controls passed on both images |

The negative control is what makes this a pass rather than a hope: a local
server returning **HTTP 200 with HTML** was correctly **not** classified as a
tracker, so the discriminator is not the naive status-code check Appendix A
exists to prevent.

**Therefore the scoring and reliability half of the project is buildable**,
for `clearnet` trackers on `udp`, `http` and `https` -- which is
**1333 of 1346** distinct URLs in the census.

**What the gate does NOT clear, and these are requirements, not omissions:**

| not measurable here | count | required state |
| --- | --- | --- |
| IPv6-only trackers (no IPv6 egress, `C-04`) | - | `unmeasurable` |
| `i2p` network | 14 | `unmeasurable` |
| `yggdrasil` network | >=1, under-counted | `unmeasurable` |
| `ws`/`wss` (WebTorrent, `C-36` unverified) | 13 | `unmeasurable` |

Every one of these **MUST** be published as `unmeasurable` and **MUST NOT**
be scored or reported `dead` (section 8.1 requirement 1). They are retained as
explicit requirements with a stated limitation, per RULES 9.1 -- not
quietly dropped.

**The residual honesty problem the gate cannot fix:** every measurement comes
from AS8075 datacenter address space `[C-54]`. "Live from GitHub Actions" is
not "live", and section 9.2's labelling is therefore load-bearing rather than
decorative.

---

### ⚠ CORRECTED 2026-08-29 -- the table above is MIS-FACTORED, not merely incomplete

*The table keeps its original wording. It was right to distrust itself, and
the census it asked for found a deeper problem than incompleteness.*

Measured by `experiments/19-scheme-census.py` over 16 source files, **1346
distinct URLs**, ngosang pinned at `1e61597`:

```
trackers_all_i2p.txt        schemes present: http (11), udp (2)
trackers_all_yggdrasil.txt  schemes present: http (1)
trackers_all_ws.txt         schemes present: wss (3)      -- not ws
```

**The table lists seven values of one variable. They are two variables.** An
I2P tracker is not a URL with an `i2p://` scheme; it is an ordinary `http://`
or `udp://` URL whose **hostname ends in `.i2p`**. Yggdrasil is the same
shape with an IPv6 literal in `0200::/7`.

This matters in the most damaging direction: a classifier keyed on scheme
sees `http://`, sends the entry to the clearnet prober, the probe fails, and
the tracker is recorded **`dead`** -- the exact correctness bug requirement 1
below forbids.

**The model is therefore two axes:**

| axis | values | decides |
| --- | --- | --- |
| **transport** | `udp`, `http`, `https`, `ws`, `wss` | how we speak to it |
| **network** | `clearnet`, `i2p`, `yggdrasil`, `onion` | whether we can reach it at all |

Measured distribution across the union: transports `http` 723, `udp` 362,
`https` 251, `wss` 10, **no bare `ws`**; networks `clearnet` 1333, `i2p` 13
([`corpus-baseline.md`](corpus-baseline.md)).

Three further corrections:

* **`ws://` is effectively extinct.** It occurs **once** in the entire union,
  and that occurrence is inside ngosang's *blacklist*. `wss://` (12) is the
  live form. The table's `ws://`, `wss://` cell should not imply parity.
* **`*.onion` does not occur at all** in any source censused. The table asked
  "check whether it occurs in sources at all" -- checked, and it does not.
  The classifier still handles it, so that an appearance is never a surprise.
* **`trackers_all.txt` silently excludes three networks.** 99 = udp 48 +
  http 37 + https 14, exactly. Every `ws`, `i2p` and `yggdrasil` entry -- 17
  trackers -- is missing, and the file does not say so. Anyone consuming that
  single file inherits the omission invisibly.

Independent confirmation of the transport set from a real consumer, not from
a README: `references/GerryFerdinandus__bittorrent-tracker-editor/tree/source/code/torrent_miscellaneous.pas:393`
accepts exactly `udp://`, `http://`, `https://`, `ws://`, `wss://`.

**Recorded limitation.** Network classification from a URL alone
**under-counts yggdrasil**: ngosang's single yggdrasil entry is
`http://yggtracker.i2p.rocks:80/announce`, an ordinary hostname that URL
inspection necessarily reports as clearnet. Only the `_ip` variant exposes
the `0200::/7` literals. Correct classification requires DNS resolution -- a
**time-varying inference** (section 8.3 point 3), which belongs to the health
checker and not to a census.

---

### ⚠ CORRECTED 2026-08-29 -- the anchor had no measurement behind it. Two now replace it.

*Original rule kept above; the ceiling principle is unchanged, only the
number it is anchored to.*

"Every 30 minutes" was an unsourced figure, exactly the kind RULES 1.5
forbids. `C-38`'s own "if false" clause said to **use the trackers' own stated
intervals** instead. Two independent sources, which agree:

* **Code.** `references/CorralPeltzer__newTrackon/tree/newtrackon/tracker.py:136-138`
  takes `interval` from the tracker's own response; `:163` floors it to
  **10800 s (3 h)** once a tracker's uptime reaches 0.
* **The operator, in newTrackon issue #334.** *"The current checking frequency
  (every ~3 hours) is reasonable for the server load. Trackers that have been
  down for more than 1.5 years are already automatically removed."*

**The revised anchor: the interval the tracker itself asks for**, defaulting
to 3 h where none has been observed, and backing off -- not merely holding --
for trackers that have been down a long time.

**This challenges section 31's cadence.** newTrackon does strictly *more* work per
check than this project plans (it announces; we stop at scrape) and runs at
~3 h. section 31's "approximately hourly" is **3x** that load per tracker, and its
suggested 30-minute variant is **6x**. Hourly *generation* remains fine -- the
aggregation, validation and publication half touches no tracker -- but hourly
*probing of every tracker* is not justified by any evidence gathered here.
The defensible split is: publish hourly, probe each tracker on its own
interval. Recorded as a challenge under RULES 9, not applied
silently.

---

### ✅ RESOLVED 2026-08-29 -- measured, and the original assertion is REFUTED

*The paragraph above keeps its original wording. This is the answer to it.*

**GitHub-hosted runners permit outbound UDP to arbitrary ports.** Measured in
workflow run
`33246108348`
on **two** runner images (`ubuntu-24.04` image `20260823.283.1` from
`64.236.141.183`; `ubuntu-22.04` image `20260824.273.3` from `52.165.101.48`;
both AS8075 Microsoft):

* `experiments/01-host-network-baseline.py` -> `udp_arbitrary_port_egress:
  true` on both images, with the tier-0 loopback control **and all four**
  tier-1 third-party controls passing on non-53 UDP ports (STUN 19302, STUN
  3478, NTP 123 x2). The control tiers are what make this a statement about
  the network rather than about the probe.
* `experiments/02-udp-bep15-connect.py` -> BEP 15 connect completed **9/11,
  8/11, 9/11, 9/11** across four runs, loopback positive control passing every
  time, median RTT **109-127 ms**, across ports 80, 443, 451, 1337, 6969, 8081.

**Consequence, applied:** the workaround this section asked for was **not
built**, because it is unnecessary. `C-01`'s own "if false" clause instructed
exactly this: *"If UDP works: the original brief's premise was wrong and no
workaround is needed -- delete it."*

**What did not survive the same measurement:** `ipv6_egress: false` on both
images, while `ipv6_stack_present: true`. See the correction in section 9.2.

---

## Known weaknesses

Carried forward from the brief's Appendix C, which was rewritten after
verification round 1 to describe the *current* weaknesses rather than the
original author's. **It is kept current**: a session that measures something
which changes this list updates it here.


Stated first, per section 5, because a reader who reaches the recommendations first has
already stopped reading.

**Revised 2026-08-29, after verification round 1.** The previous version of this
appendix is superseded because it described a document nobody had checked. It
was right about the most important thing -- that more errors remained -- and the
round that followed found **eight**. What is below is the current list, and the
same warning applies to it with equal force.

### What round 1 actually corrected

Kept visible so the error rate is checkable rather than asserted. Each of these
had its original wording preserved in place with the correction underneath:

| section | the document said | measurement said |
| --- | --- | --- |
| section 10.1 | runners may not support UDP; build a workaround | **UDP works.** `udp_arbitrary_port_egress: true`, two images, four controls. No workaround built |
| section 8.1 | seven protocols in one table | **two axes.** `.i2p` is a hostname suffix, not a scheme; scheme-keyed classification records I2P trackers `dead` |
| the brief | `ws://`, `wss://` as a pair | **`ws` occurs zero times** in the 1346-URL union; the corpus's single `ws://` is a blacklisted entry |
| section 4.3 | pkgforge reads `newtrackon.com/list` (HTML) | it reads **`/api/stable`** (`text/plain`). The README was wrong; the code was right |
| section 4.3 | newTrackon may expose no machine-readable uptime | **it does** -- `/api/<int:percentage>`. `C-26` was backwards |
| section 4.3 | `bittorrent-tracker-editor/source` is a registry of sources | it is a **Free Pascal application tree** |
| section 9.4 | clients re-announce every ~30 minutes | unsourced. A production monitor runs at **~3 h**, and takes the interval from the tracker |
| section 7.1 | (gate unanswered) | **passed**, with 14 i2p + 13 ws/wss + all IPv6-only entries carved out as `unmeasurable` |

**Eight corrections from one round of checking is the honest prior for the
next round.**

### What is still unverified and load-bearing

These are the rows most likely to be wrong next, ordered by what they would cost:

* **`C-12` -- scheduled workflows disabling after repository inactivity.**
  Unchecked, and "runs indefinitely" depends on it entirely. A dataset that
  stops updating without telling anyone is the worst failure mode available,
  and nothing here yet detects its own silence.
* **`C-14`, `C-15`, `C-17` -- release and tag behaviour.** Unchecked because
  creating releases in someone's repository is outward-facing. **`D5` is
  blocked**, and section 18.2's channel semantics rest on assumptions.
* **`C-40`, `C-41` -- client compatibility.** **No torrent client was ever
  run.** The plaintext format -- the primary deliverable for the primary
  audience -- is validated by reading *one* client's parser. section 17.1 is the least
  evidenced section of this document relative to its importance.
* **`C-03` -- vantage bias.** One vantage point exists. The measured
  disagreement with newTrackon is at most one tracker out of eleven, which is
  far too small to conclude anything. section 9.2's labelling is correct regardless,
  which is the only reason this is survivable.
* **`C-36` -- WebTorrent.** 13 `wss` trackers are `unmeasurable` by default
  because nobody attempted a handshake, not because one was shown impossible.
* **`C-11`, `C-19`, `C-19b`** -- schedule reliability, cost, and whether
  workflow pushes chain. None observed over any meaningful period.

### Weaknesses in the evidence that now exists

The measurements are better than the assumptions they replaced. They are not
strong:

* **Every network number comes from datacenter address space** (AS8075, and an
  authoring sandbox behind an HTTP proxy). Neither is a residential connection,
  which is where consumers sit. "Live from GitHub Actions" is not "live".
* **Sample sizes are small.** 11 UDP targets, 6 HTTP targets, 17 hostnames for
  DNS. `C-06`'s "no resolver divergence" is a statement about 17 names on one
  day -- and newTrackon issue #316 records a **real production case** where a
  datacenter resolver silently broke correctness, which is exactly what n=17
  failed to detect.
* **The corpus trackers are truncated.** `ngosang/trackerslist` and
  `CorralPeltzer/newTrackon` each returned exactly 100 items, so older history
  was not read. No Discussions and no review comments were fetched for any
  reference.
* **`995 unique of 1091` for the anime source is a string comparison.**
  Whether those entries are *alive* is unmeasured, and that is the half that
  decides whether the source is worth keeping.
* **One experiment shipped a methodology bug** (`experiments/19` compared a
  source against its own downstream copy and reported the opposite of the
  truth). It was caught here. The next one may not be.

### Weaknesses that no amount of measurement will fix

* **This document has still not been reviewed by anyone who operates a
  tracker.** section 9.3 governs how this project behaves toward other people's
  servers, and not one of them has been asked. Two indirect signals exist --
  ngosang's blacklist carries 2 "requested by sysadmin" entries, and BEP 34
  gives a DNS opt-out `[C-51]` -- but indirect is what they are.
* **The scoring model (section 15.3) is still a guess.** The invariants (section 15.2) remain
  the defensible part, and they are cheaper to get right than the model.
* **A sophisticated statistic over a single-vantage measurement is precision
  applied to the wrong quantity**, and this document is more likely to be
  wrong about what it is measuring than about how it is measuring it.

**Assume more remain.**
