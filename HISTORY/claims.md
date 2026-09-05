# Claims

**The evidence register.** Every factual statement this project relies on has a
row here, with its status, the instrument that checked it, and what changes if
it is false.

[`TODO/RULES.md`](../TODO/RULES.md) section 1.2 is the rule this file serves:
**a claim may not become load-bearing until it is verified**, and only
`VERIFIED` permits load-bearing use. A row that says `UNVERIFIED` is not usable.

Rows cite the rule, entry or record that now owns the requirement.
That document is gone from the tree; the citation is provenance for where a
claim came from, not a path to open. `git log --diff-filter=D -- the design brief`
finds it if a future session needs the original wording.

**Verification status vocabulary**

| status | meaning |
| --- | --- |
| `UNVERIFIED` | nobody has checked it. **The default. Not usable.** |
| `SANDBOX-1` | checked once from the authoring sandbox, single sample, no control run, through an HTTP proxy. Does not generalise to a runner. |
| `README-CLAIMED` | asserted by a third party's own documentation; code not inspected |
| `INFERRED` | deduced from another observation, not directly observed |
| `VERIFIED` | checked with a committed instrument and recorded conditions |
| `REFUTED` | checked and found false. The row stays; what replaced it is recorded. |

**Open verification work is in [`TODO/claims.md`](../TODO/claims.md)**, not here.
This file is the evidence; that file is the backlog.

---

Every factual statement in the design brief appears here. Numbering has intentional gaps
so that ids stay stable; **never reuse an id**, and never renumber.

**Verification round 1 -- 2026-08-29.** The rows below were checked by the
session recorded in `HISTORY/reference-sweep.md`. Every `VERIFIED` and
`REFUTED` row names a committed instrument that re-runs the check. Rows still
`UNVERIFIED` say what specifically is missing, because an unchecked row that
looks checked is the failure this register exists to prevent.

**The single most important result:** `C-01` is **REFUTED in the direction that
removes work**. GitHub-hosted runners *do* permit outbound UDP to arbitrary
ports. The original brief's central premise -- that they do not, and that a
workaround is needed -- is false, and the workaround was never built.

**The single most important hazard:** `C-04` is **VERIFIED**. Runners have an
IPv6 stack and **no IPv6 egress**. Every IPv6-only tracker therefore *must* be
recorded `unmeasurable`; recording it `dead` would be a correctness bug, not a
limitation.

### 7.1 Execution environment

Measured on GitHub-hosted runners in workflow run
[`33383406869`](https://github.com/Azathothas/Trackers/actions/runs/33383406869),
head `3ec6dcd`, on **two** runner images so that no claim rests on one image:

| image | version | public IPv4 | operator |
| --- | --- | --- | --- |
| `ubuntu-24.04` | `20260823.283.1` | `172.208.127.32` | AS8075 Microsoft |
| `ubuntu-22.04` | `20260824.273.3` | `20.109.38.118` | AS8075 Microsoft |

⚠ **Every record below was re-taken on 2026-09-01 and every figure moved.** The
run these claims were originally verified against, `33246108348`, belonged to
this repository's prior history: its artefacts are gone and its URL 404s. The
instruments are unchanged apart from the `C-71` fix, and the results are
committed under `experiments/results/`.

| id | claim | status | verify by | if false |
| --- | --- | --- | --- | --- |
| C-01 | GitHub-hosted runners permit outbound UDP to arbitrary ports, so BEP 15 probing is possible | **`VERIFIED`** -- true, and the brief's opposite premise is refuted | `experiments/01-host-network-baseline.py`, `experiments/02-udp-bep15-connect.py` | Consequence applied: the UDP workaround in the original brief was **not built**, because it is unnecessary. |
| C-02 | Runners have no usable inbound connectivity | `UNVERIFIED` -- measured, **inconclusive by construction** | `experiments/03-inbound-connectivity.py` | Unchanged. `D2` must not assume inbound in either direction. |
| C-03 | Trackers commonly rate-limit or block datacenter address ranges, so runner measurements under-report liveness | `UNVERIFIED` -- one weak sample, not enough to conclude | `experiments/02`, cross-read against newTrackon as oracle | Vantage labelling in RULES 3.4 stays regardless; it costs nothing and is correct either way. |
| C-04 | Runners may have no IPv6 egress | **`VERIFIED`** -- no IPv6 egress, on both images | `experiments/01-host-network-baseline.py` | Consequence applied: IPv6-only trackers are `unmeasurable`, never `dead`. |
| C-05 | From the authoring sandbox, `github.com` and `api.github.com` returned HTTP 403 while `raw.githubusercontent.com` returned 200 | `SANDBOX-1`, **and environment-specific** | `curl -o /dev/null -w '%{http_code}'` | Confirmed environment-specific: in *this* session `api.github.com` returned **200**. The row is retained because it explains why the authoring round's GitHub claims were README-depth. |
| C-06 | Runner DNS resolvers may filter or behave differently from a consumer resolver | **`VERIFIED`** -- no divergence observed at n=17 | `experiments/04-dns-resolver-divergence.py` | No filtering found, so `dns_failure` may be read as a property of the name -- **at this sample size only**. |

**Verification records**

* **C-01, `experiments/01`, `experiments/02`, 2026-09-01, run `33383406869`.**
  Result: `udp_arbitrary_port_egress: true` on **both** images. Tier-0 loopback
  control passed; **all four** tier-1 third-party controls passed on non-53 UDP
  ports -- STUN/RFC 5389 to `stun.l.google.com:19302` and
  `stun.cloudflare.com:3478`, NTP/RFC 5905 to `pool.ntp.org:123` and
  `time.cloudflare.com:123`. Subject probe: BEP 15 connect completed **10/11,
  9/11, 10/11, 10/11** across four runs (two images x two runs), with the
  loopback BEP 15 positive control passing on every run. Median RTT
  **97.5-103.9 ms**.
  ⚠ **10 is the ceiling, not 11.** One of the eleven targets has no IPv4
  address at all, so it can never reach the connect rung from a vantage with no
  IPv6 egress. It is classified `no_ipv4_address` rather than counted as a
  failure, which is RULES 3.1 inside the instrument.
  ⚠ **The one run that scored 9 is informative, not noise.** Its histogram
  records the eleventh target at `datagram_sent`, meaning the packet left and
  nothing came back, which is a timeout rather than a refusal.
  *An absence would have been ambiguous; the controls are what make the
  presence meaningful.*
* **C-02, `experiments/03`, 2026-09-01, run `33383406869`.** Result: bind
  and listen succeeded; loopback control matched; connection to the runner's
  own public address timed out. The instrument reports this itself as
  **inconclusive**: a failed hairpin is equally consistent with blocked inbound
  and with a NAT that does not hairpin. Establishing C-02 needs a prober
  outside the runner, which this project does not have. **Recorded as
  unavailable rather than guessed.**
* **C-03, `experiments/02` + newTrackon oracle, 2026-08-29.** Result: of 11
  UDP targets that newTrackon listed live at fixture capture, 9 answered us.
  The 2 that did not are *explained*, not silent: one resolves only to IPv6
  (unreachable from here per C-04), one timed out. That leaves **at most one**
  candidate vantage disagreement -- far too small a sample to support or refute
  a claim about datacenter blocking. Needs experiment 20 (a second vantage).
* **C-04, `experiments/01`, 2026-09-01, run `33383406869`.** Result:
  `ipv6_stack_present: true`, `ipv6_egress: false` on **both** images. Both
  IPv6 targets (`ipv6.google.com:443`, `ipv6.icanhazip.com:443`) returned
  `OSError: [Errno 101] Network is unreachable`. Independently corroborated:
  `experiments/05` hit the same error against `tracker.ipv6tracker.ru`, and
  `experiments/02` classified `retracker.hotplug.ru` as `no_ipv4_address`.
* **C-06, `experiments/04`, 2026-09-01, run `33383406869`.** Conditions: 17
  hostnames x (local resolver + 3 pinned public resolvers). Result on **both**
  images: **agree 14, both_failed 3, divergent 0**, and no case of "local
  fails, public resolves", which is the filtering signature.
  ⛔ **The run 40 minutes earlier, `33383156641`, found 1 divergent on both
  images**: `tracker.torrent.eu.org` resolved to `89.234.156.205` locally and
  to `91.216.110.52`/`.53` at both public resolvers, then agreed on the next
  run. **Two runs of one instrument disagree about the headline finding.** That
  is not a divergence measurement, it is the reason `T-007` exists, and
  neither number may be quoted without the other. **n=17 is small**; re-check
  at each phase gate.

### 7.2 GitHub platform

| id | claim | status | verify by | if false |
| --- | --- | --- | --- | --- |
| C-10 | `schedule:` has a minimum interval (the brief assumes ~5 minutes) | **`VERIFIED`** -- 5 minutes, exactly as assumed | `experiments/22-actions-platform-contract.py` | Not load-bearing: the chosen cadence is far above the floor. |
| C-11 | Scheduled workflows can be delayed or dropped entirely under platform load | **`VERIFIED` as documented behaviour**; rate still unmeasured | `experiments/22` | Confirmed in GitHub's own words, **including dropped**, not merely delayed. T-084's requirement is load-bearing and now evidenced. |
| C-12 | Scheduled workflows in a public repository are disabled automatically after a period of repository inactivity (~60 days) | **`VERIFIED` -- TRUE, and it is the project's worst failure mode** | `experiments/22` | Consequence: the project **must** either guarantee qualifying activity or detect and report its own silence. Not optional. |
| C-13 | The default workflow token is rate-limited per repository per hour (~1000 API requests) | `UNVERIFIED` | Observe `x-ratelimit-*` headers during a heavy run | Caps issue automation; not yet reached because no issue automation runs. |
| C-14 | GitHub's `/releases/latest` resolves to the newest non-prerelease by date, **not** to a tag literally named `latest` | `UNVERIFIED` | Create both and observe | **Unresolved and blocking `D5`.** Creating real releases in the user's repository is an outward-facing act; not done unilaterally. |
| C-15 | Release assets can be replaced, and `.../releases/download/<tag>/<name>` remains a stable URL | `UNVERIFIED` | Upload, replace, re-fetch | Same as C-14. |
| C-16 | `raw.githubusercontent.com/<owner>/<repo>/<branch>/<file>` serves branch content, caches for a period, and is a supported consumption path | **`VERIFIED`** -- and the caching fear is resolved | `experiments/21-raw-github-consumption.py` | Consequence: **hourly generation is not undermined by raw caching.** |
| C-17 | Moving a git tag updates the associated release's target | `UNVERIFIED` | Move a tag on a test repository | Blocks the move-tag vs. delete-and-recreate decision in `D5`. |
| C-18 | Force-pushing a branch invalidates commit SHAs for consumers, and old objects are not immediately purged | `UNVERIFIED` | Force-push a test branch; try to fetch the old SHA | Consumer pin-target guidance is written conservatively (pin a branch or tag, never a data-branch SHA) so it is correct either way. |
| C-19 | Actions minutes are free for public repositories | `UNVERIFIED` | Billing documentation | If false the hourly cadence is unaffordable and the whole schedule changes. |
| C-19b | Commits pushed by a workflow using the default token do not themselves trigger workflows | **`VERIFIED`** | `experiments/22` | Confirmed. No design here depends on a chained trigger, so only the protective direction is relied on. |

**Verification records**

* **C-10, C-11, C-12, C-19b, `experiments/22-actions-platform-contract.py`,
  2026-08-29.** Five sentences pinned as regexes against GitHub's
  *events-that-trigger-workflows* reference; the instrument re-fetches the page
  and **exits non-zero when a sentence is gone**, so this stays a check rather
  than a thing somebody once read. All five present, quoted verbatim in the
  result file:
  *"The shortest interval you can run scheduled workflows is once every 5
  minutes"* (C-10); *"The `schedule` event can be delayed during periods of
  high loads"* (C-11); *"some queued jobs may be dropped"* (C-11b -- runs are
  **dropped**, not merely late); *"In a public repository, scheduled workflows
  are automatically disabled when no repository activity has occurred in 60
  days"* (C-12); *"other `GITHUB_TOKEN`-triggered events do not create workflow
  runs at all"* (C-19b).
  **What a pass means and does not mean:** GitHub still *says* these things. It
  does not establish that GitHub *does* them -- documentation is correct about
  intent and is sometimes behind the platform (RULES 1.1). C-11's actual delay/drop
  **rate** and C-12's 60-day timer remain unobserved and need months; the rows
  say so rather than borrowing this instrument's confidence.
* **C-16, `experiments/21-raw-github-consumption.py`, 2026-08-29.** Conditions:
  fetched a file from this repository's own branch immediately after pushing it,
  from the authoring sandbox. Result: **HTTP 200**, `content-type: text/plain;
  charset=utf-8`, **`cache-control: max-age=300`** (five minutes), strong
  `etag` present, `via: 1.1 varnish`. Content was **current within seconds** of
  the push. The same file addressed by commit SHA also returned 200.
  **Consequence:** the register's own worry -- "if caching is longer than the
  update interval, hourly generation is partly pointless" -- **does not
  materialise**: 300 s << 3600 s. The ETag also means conditional requests work,
  so a polite consumer can poll cheaply.

### 7.3 Upstream projects and references

Corpus cloned and pinned; commits recorded in
`HISTORY/reference-sweep.md`. These rows are no longer README-depth.

| id | claim | status | verify by | if false |
| --- | --- | --- | --- | --- |
| C-20 | `pkgforge-security/Trackers` aggregates three sources -- ngosang (general), DeSireFire (anime), `newtrackon.com/list` (stable) -- hourly, into concatenated lists, with no health checking or validation described | **`REFUTED` in part** | Read `.github/workflows/fetch_update_trackers.yaml` at commit `7f2d00b` | The **"no health checking, no validation" half is confirmed**. The **source half is wrong**: it reads `newtrackon.com/api/stable`, a `text/plain` feed -- *not* the HTML `/list`. |
| C-20a | That repository is archived | **`VERIFIED`** -- archived; **reason not recoverable** | Repository metadata + all 24 tracker items, both states | The tracker records **no** reason. It contains **zero human issues**: all 24 items are `dependabot`/`renovate` **pull requests**. |
| C-21 | `ngosang/trackerslist` advertised 99 trackers, daily updates, and per-protocol lists including `udp`, `http`, `https`, `ws`, `i2p`, `yggdrasil`, plus `_ip` variants | **`VERIFIED` as to files; `REFUTED` as to the scheme model** | `experiments/19-scheme-census.py` at commit `1e61597` | The *files* exist as described. The *model* behind them is wrong -- see the record below. This is the row that changed the domain model. |
| C-22 | It removes trackers sharing a domain or resolved IP, maintains a blacklist, and sorts "by popularity and latency" | **`UNVERIFIABLE from the repository`** | Attempted: read the generator source | **The generator is not in the repository.** It publishes outputs only. "Popularity" is therefore **unauditable**, which is itself the finding. |
| C-23 | newTrackon serves `/api/stable`, `/api/live`, `/api/all`, `/api/udp`, `/api/http` as `text/plain`; `/api/dead`, `/api/added`, `/api/percentage` return 404 | **`VERIFIED` but materially incomplete** | Re-probed, then read `newtrackon/views.py` at commit `7da7dde` | The 404s are real but **misleading**; two endpoints were missed entirely. See C-26. |
| C-24 | `/api/live` returned 74 entries; `/api/stable?include_ipv4_only_trackers=false` returned 31; entries appeared blank-line separated | **`VERIFIED`, and the missing control has now been run** | `experiments/20-newtrackon-api-surface.py` | Counts are volatile as predicted. **Parameter semantics now observed rather than assumed**, and confirmed against source. |
| C-25 | `newtrackon.com/list` and `/raw` are HTML pages, not machine-readable feeds | **`VERIFIED`** | `Content-Type` inspection | Confirmed: both `text/html`. But see C-20 -- the prior art never consumed them. |
| C-26 | newTrackon exposes no machine-readable uptime-percentage endpoint | **`REFUTED`** | Read the route table in `newtrackon/views.py` at `7da7dde`, then probe | **It does.** `@app.route("/api/<int:percentage>")`. This is the difference between a cross-check and a mirror, and it lands on *cross-check*. |
| C-27 | `DeSireFire/animeTrackerList` is abandoned | **`VERIFIED` as to staleness; `REFUTED` as to worthlessness** | Repository metadata + `experiments/19` | Last push **2024-01-12** (~2.6 y). But it contributes **995 unique URLs of 1091** against all other primary sources. Abandoned **and** by far the largest unique contributor. |
| C-28 | `Azathothas/TEMPLATE` carries methodology docs at `docs/methodology/{experiments,references,work-todo}.md` on `main` | **`VERIFIED`** | Tracked at `references/Azathothas__TEMPLATE/tree/docs/methodology/`; first read at `6eaf4b5`, re-read at `6206166` | Present and read. TEMPLATE is **0BSD**, matching this project (`LICENSE`, read on disk). Between the two commits `experiments.md`, `work-todo.md` and `choosing-a-work-model.md` are **byte-identical** and `references.md` gained one paragraph, so nothing this project's rules rest on moved. |
| C-29 | `Azathothas/pacman-static` and `AvalynSouvlaki/T-244-RESEARCH` demonstrate the documentation standard, and `docs/patches/mine-repo-page-join.md` in that tree exists | `UNVERIFIED` -- metadata only | Fetch and read in full | Both repositories exist and are reachable (`pacman-static` 0BSD, `T-244-RESEARCH` Unlicense). The named document has **not** yet been read in full. |

**Verification records**

* **C-20, `pkgforge-security/Trackers` @ `7f2d00b`, 2026-08-29.** The workflow's
  own comments are **misaligned with its commands by one line**, which is the
  likely origin of the README error the register inherited. The commands fetch
  `ngosang/.../trackers_all.txt`, `https://newtrackon.com/api/stable`, and
  `DeSireFire/.../AT_all.txt`. Corroboration that `/api/stable` is the real
  source, not `/list`: the published `trackers_stable.txt` (57 entries) shares
  **52** entries with today's `/api/stable` (53 entries), and consists of bare
  URLs that an HTML page could not yield without a parser the repository does
  not contain. **Confirmed absent: any health check, any validation, any
  provenance, any change detection.** Additional finding not in the register:
  every step is `set +e` **and** `continue-on-error: true`, and `curl -o`
  truncates its output file before it fails -- so a failed fetch yields an
  *empty* file which is then concatenated, silently removing an entire source
  from the published list with nothing reported. That is RULES 3.10's
  "source failed vs. source returned zero" invariant, violated in production.
  A second finding: `sort -u` **destroys ngosang's popularity ordering**, so
  the derivative is strictly worse-ordered than its input.
  *Correction applied during this round:* an earlier draft of this row called
  those 24 tracker items "issues". They are **pull requests**. `references.md`
  warns about exactly this -- "the issues endpoint returns pull requests too ...
  or you will report a dependency bump as an issue" -- and the first draft did
  it anyway. Discriminated on the `pull_request` field: **0 issues, 24 PRs**.
* **C-21, `experiments/19-scheme-census.py`, re-read 2026-08-31, union of
  16 source files, 1346 distinct URLs.** Transports: `http` 723, `udp` 362,
  `https` 251, `wss` 10; **no bare `ws` in the union**. Networks:
  `clearnet` 1333, `i2p` 13. Figures live in
  [`corpus-baseline.md`](corpus-baseline.md); a previous revision of this row
  carried a different set entirely, which no committed result file records
  (see RULES 2.1). Three corrections:
  (a) `trackers_all_i2p.txt` contains schemes **`http` and `udp`** -- `.i2p` is
  a *hostname suffix*, not a scheme, so RULES 3.1's single-axis table is
  mis-factored and a scheme-keyed classifier would route I2P entries to the
  clearnet prober and record them dead; (b) `trackers_all_ws.txt` contains
  **`wss`**, and `ws` occurs exactly once in the whole union -- inside
  ngosang's **blacklist**; (c) `trackers_all.txt` (99) = udp 48 + http 37 +
  https 14 **exactly**, so it silently excludes every `ws`, `i2p` and
  `yggdrasil` entry -- 17 trackers a consumer of that one file loses without
  being told.
* **C-22, `ngosang/trackerslist` @ `1e61597`, 2026-08-29.** Complete tracked
  file list is 16 entries: `LICENSE`, `README.md`, `_config.yml`,
  `.github/FUNDING.yml`, `blacklist.txt` and 11 output `.txt` lists. **No
  generator, no workflow, no script.** The verification method this row
  prescribes cannot be executed. `blacklist.txt` (346 entries) is however
  strong evidence *about* the filtering: reasons include **178 "registered
  torrents"**, ~90 "duplicate of <url>" (the domain/IP collapse this row
  describes, now visible with its evidence), 11 "malfunction", 7 "deprecated by
  owner", 5 "detected by antivirus software", and -- directly relevant to
  RULES 4 -- **2 "requested by sysadmin"**. Tracker operators do ask to
  be removed, and a real upstream honours it.
* **C-23 / C-26, `newtrackon/views.py` @ `7da7dde`, 2026-08-29.** The real
  route table: `/api/<int:percentage>`, `/api/stable`, `/api/best`, `/api/all`,
  `/api/live`, `/api/udp`, `/api/http`, `/api/add`. The earlier 404 on
  `/api/percentage` was a **false negative caused by probing the parameter's
  name as a literal path**. Measured: `/api/0` -> 261, `/api/50` -> 82, `/api/95`
  -> 55, `/api/100` -> 15 entries. From source, `/api/stable` **is**
  `api_percentage(95, added_before=<min age 10 days>)` -- so "stable" means
  **>=95 % uptime and >=10 days old**, a definition not previously known.
  `/api/best` is a **301 redirect** to `/api/stable`.
* **C-24, `experiments/20-newtrackon-api-surface.py`, 2026-08-29.** The
  control the register said was never run, has now been run: `/api/stable`
  -> **53**; `?include_ipv4_only_trackers=false` -> **31**;
  `?include_ipv4_only_trackers=true` -> **53**. So the default is `true` and the
  parameter *excludes* IPv4-only trackers -- confirmed independently in source
  (`default="true"`). Separator structure confirmed: `/api/live` returned 156
  lines, **78 non-blank and 78 blank** -- entries are `\n\n`-separated.
* **C-27, repository metadata + `experiments/19`, 2026-08-29.** `pushed_at`
  **2024-01-12**, not archived, GPL-3.0, 4 862 stars, 18 open issues. Unique
  contribution **995 of 1091** measured against the other *primary* sources.
  A first draft of experiment 19 reported 0 unique, which was an **artefact**
  of including `pkgforge_all` -- a strict superset of this source (1091/1091) --
  in the comparison. Recorded because the artefact is instructive: comparing a
  source against its own downstream copy always shows redundancy.

### 7.4 Protocol and measurement

| id | claim | status | verify by | if false |
| --- | --- | --- | --- | --- |
| C-30 | BEP 15 UDP tracker connect uses magic protocol id `0x41727101980`, `action=0`, and returns a connection id | **`VERIFIED`** against BEP 15 and against live trackers | BEP 15 text + `experiments/02` | - |
| C-31 | A BEP 15 connect handshake requires no info_hash and creates no swarm participation | **`VERIFIED`** -- structurally impossible to carry one | BEP 15 message tables | The probe hierarchy stands. |
| C-32 | HTTP trackers return bencoded dictionaries, and a `failure reason` key is a well-formed tracker response proving a working tracker | **`VERIFIED`**, with a **negative** control | `experiments/05-http-tracker-protocol.py` | The discriminator holds; the naive status-code approach is not needed. |
| C-33 | `numwant=0` with `event=stopped` minimizes swarm participation on an announce | `UNVERIFIED` -- **and deliberately not exercised** | BEP 3; a tracker under our control | Not needed: the ladder stops at scrape, so no announce is performed at all. |
| C-34 | Announcing with a real info_hash inserts the announcing IP into that swarm's peer list | `UNVERIFIED` -- **treated as true regardless, precautionarily** | A tracker under our control | Prohibition stands and is enforced by the absence of any announce code path. |
| C-35 | Scrape is conventionally the announce path with the final `/announce` replaced by `/scrape`, and not all trackers implement it | **`VERIFIED`, with a precision correction** | BEP 48 + `experiments/05` | See record. |
| C-36 | `ws://`/`wss://` trackers speak WebTorrent over WebSocket, not the HTTP tracker protocol | `UNVERIFIED` | WebTorrent specification; attempt a handshake | 12 `wss` entries exist in the corpus. Until checked they are `unmeasurable`, never `dead`. |
| C-37 | I2P and Yggdrasil trackers are unreachable without their respective routers | **`VERIFIED` for I2P by construction**; Yggdrasil partially | `experiments/19` + `experiments/04` | Consequence applied: both are `unmeasurable`. |
| C-38 | Well-behaved BitTorrent clients re-announce on the order of every 30 minutes, anchoring the politeness ceiling | **`REFUTED` as an anchor; replaced with a better one** | `newtrackon/tracker.py` @ `7da7dde` | The anchor is now the **tracker's own stated `interval`**, exactly as the row's "if false" clause instructs. |

**Verification records**

* **C-30, BEP 15 (`bittorrent.org/beps/bep_0015.html`, fetched 2026-08-29),
  `experiments/02`.** The specification's connect-request table reads: offset 0,
  64-bit `protocol_id` = `0x41727101980` (magic constant); offset 8, 32-bit
  `action` = 0; offset 12, 32-bit `transaction_id`; **total 16**. The
  connect-response table: `action` 0, `transaction_id`, 64-bit `connection_id`.
  `experiments/02` implements exactly this (`struct.pack(">QII", ...)`) and it
  completed against **9 live trackers**, so the encoding is confirmed against
  both the specification and independent implementations.
* **C-31, BEP 15 message tables, 2026-08-29.** The connect request has
  **three fields and ends at offset 16**. There is no info_hash field and no
  room for one. `info_hash` first appears at offset 16 of the **announce**
  request, and at offset 16 + 20, n of the **scrape** request. A connect
  therefore cannot express interest in any content. **Corollary discovered
  here and not previously recorded: UDP scrape *does* require an info_hash**,
  unlike HTTP scrape -- so on UDP the ladder's second rung is strictly more
  intrusive than on HTTP, and needs a synthetic random infohash under
  RULES 4.
* **C-32, `experiments/05-http-tracker-protocol.py`, 2026-09-01, run
  `33383406869`, both images.** Positive control (a local server returning a
  bencoded `failure reason`) -> recognised as a tracker: **PASS**. Negative
  control (a local server returning **HTTP 200 with HTML**) -> correctly **not**
  recognised as a tracker: **PASS**. Both controls run twice. Subjects: **4 of
  6** proved themselves trackers by answering scrape with a well-formed
  `tracker_scrape_response`, and `announce_sent` is `false` in every result.
  ⚠ **It was 5 of 6 when this was first measured.** The two that did not answer
  failed for reasons about us or about the host rather than about the tracker
  (IPv6,
  per C-04). The the design brief Appendix A anti-pattern is wired to a non-zero exit,
  so it cannot silently return.
* **C-35, BEP 48 (fetched 2026-08-29), `experiments/05`.** Correction to the
  row's wording: BEP 48 specifies locating the string `announce` **in the path
  section** of the announce URL and replacing it with `scrape` -- not
  specifically "the final `/announce`". The distinction matters for paths such
  as `/announce.php`. Measured support: **5 of 6** corpus trackers answered
  scrape. BEP 48 additionally states, in terms, that **"scrape exchanges have
  no effect on a peer's participation in a swarm"** -- primary-source support
  for the ethics position in RULES 4.
* **C-37, `experiments/19`, 2026-08-29.** I2P entries are identified by the
  `.i2p` hostname suffix and are unreachable without a router **by
  construction**, so they are `unmeasurable` without needing a failed probe to
  prove it. **Yggdrasil is only partially detectable**: ngosang's single
  yggdrasil entry is `http://yggtracker.i2p.rocks:80/announce`, an ordinary
  hostname that URL inspection alone reports as clearnet; only the `_ip`
  variant exposes the `0200::/7` literals. Correct classification therefore
  requires DNS resolution -- a **time-varying inference**, not a property of the
  URL. Recorded as a limitation the census prints about itself.
* **C-38, `newtrackon/tracker.py` and `scraper.py` @ `7da7dde`, 2026-08-29.**
  The "30 minutes" figure has no measurement behind it and is not used. A
  long-running production monitor of this exact kind instead re-checks each
  tracker at **the interval the tracker itself returns** in its announce
  response (`self.interval = response["interval"]`), falling back to
  **10 800 s (3 h)** once a tracker's uptime reaches 0. That is the anchor
  RULES 4 asks for -- the trackers are the authority on the load they
  want. **Methodology caveat, load-bearing for any comparison:** newTrackon
  reaches that field by **announcing** (`announce_http`/`announce_udp`, with
  `thash = urandom(20)`), whereas this project stops at scrape. So newTrackon's
  "uptime" and this project's "live" **answer different questions**, and any
  cross-check must say so rather than treating disagreement as error.

### 7.5 Consumers, publication, tooling

| id | claim | status | verify by | if false |
| --- | --- | --- | --- | --- |
| C-40 | Common clients accept newline-separated tracker lists; some upstreams separate entries with blank lines | **`VERIFIED` for the upstream half only** | `experiments/20` | The blank-line half is now measured. The **client** half is not, and it is the half the plaintext guarantee rests on. |
| C-41 | Comments (`#`) in tracker list files break some clients | `UNVERIFIED` | Read client parsers; test | **Conservative default retained**: no comments in plaintext output. Cheap to check and still worth checking. |
| C-42 | 0BSD requires no attribution or credit | **`VERIFIED`** | Read the committed `LICENSE` | Documentation may state "no attribution required" truthfully. |
| C-43 | `pkgforge-dev/reverse-proxies`, `apify/impit`, `h4ckf0r0day/obscura`, `0x676e67/wreq-util` are candidate 401/403 mitigations | **`VERIFIED` as unnecessary so far**, and **weakened on 2026-08-31** | Observed status codes across all source fetches, `experiments/19` | **No mitigation adopted**, and none is warranted: nothing has refused a *source* fetch. But `C-64` records an intermediary refusing this project's descriptive User-Agent with HTTP 420 while accepting `curl/8.5.0` for the identical request, so "a plain descriptive User-Agent suffices" is now true of our sources and not true in general. `HISTORY/references/avalynsouvlaki-t-244-research.md` carries the shortlist and the argument if that changes. |
| C-44 | Workflow artefacts have a retention limit, so an issue citing one will eventually cite nothing | **`VERIFIED`, and re-confirmed 2026-09-01** | Artefact metadata from run `33383406869` | Consequence applied: runner results are **committed to git**, not left in artefacts. ⛔ **The stronger form is now measured: a run URL does not outlive its repository either.** Every artefact and log from run `33246108348` went with this repository's prior history, and that link 404s. |
| C-45 | Third-party GitHub Actions should be pinned to a commit SHA rather than a tag, because tags are mutable | `UNVERIFIED` as to the *reason*; **adopted regardless** | Confirm tag mutability | Practice already followed. See record for supporting evidence. |

**Verification records**

* **C-40, `experiments/20-newtrackon-api-surface.py`, 2026-08-29.** newTrackon's
  `/api/live` returned **156 lines: 78 non-blank, 78 blank** -- entries are
  separated by `\n\n`. So the upstream half of the claim is confirmed for at
  least one major source. **What this does not establish, and it is the
  important half: no torrent client was tested.** T-001's plaintext
  guarantee still rests on an unchecked assumption, and the honest position is
  that the project emits the most conservative format (one URL per line, single
  `\n`, no comments, no blank lines) precisely *because* the client behaviour
  is unknown.
* **C-42, committed `LICENSE`, 2026-08-29.** The text grants "Permission to
  use, copy, modify, and/or distribute this software for any purpose with or
  without fee is hereby granted" and -- decisively -- **omits** the
  notice-retention proviso that ISC and BSD-2 carry. That omission is precisely
  what makes it 0BSD. The documentation's "no attribution or credit required"
  is therefore accurate rather than aspirational.
* **C-43, all source fetches in `experiments/19`, 2026-08-29.** Every source
  in the census was retrieved with a single honest descriptive User-Agent
  naming the project and its URL. **Zero 401 or 403 responses.** Per the row's
  own instruction -- "assume none is necessary until measurement says
  otherwise" -- no proxy, no impersonation library and no browser-like
  User-Agent has been added. Each would have been a dependency and a
  supply-chain risk bought for nothing.
* **C-44, run `33383406869` artefact metadata, 2026-09-01.** Both artefacts
  report `expires_at` **2026-11-29**, a 90-day retention, against a
  `created_at` of 2026-08-31. Confirmed: an issue citing an artefact would
  eventually cite nothing.
  ⛔ **And the claim is weaker than the world.** The previous run's artefacts
  did not survive 90 days: they survived two days, because the repository they
  belonged to did not survive. A citation to a run URL is not durable evidence
  at any retention setting, which is the argument for committing the result
  JSONs rather than linking them. **Consequence applied in this round**: the runner
  result JSONs were downloaded and committed under `experiments/results/`, so
  the evidence for C-01 and C-04 outlives the artefact.
* **C-45, `pkgforge-security/Trackers` tracker, 2026-08-29.** Not a
  proof of tag mutability, but a measured cost of tag-pinning: **all 24 tracker
  items** in that repository are automated dependency-bump **pull requests** for
  its three tag-pinned actions. Tag-pinning generated the entire maintenance load the project ever
  carried. This project's workflow pins `actions/checkout` and
  `actions/upload-artifact` to full commit SHAs.

### Round-1 correction: `unmeasurable` is not `unknowable`

Recorded 2026-08-29 after an operator correction. Several rows below conclude
that this vantage cannot reach something -- no IPv6 egress (`C-04`), no I2P or
yggdrasil router (`C-37`), no WebTorrent handshake attempted (`C-36`).

**Every one of those is a statement about one route, not about the question.**
The label `unmeasurable` is the honest description of *our direct probe data*
and it is correct. It is **not** a finding that the tracker's liveness is
unknowable, and it must never be read as a reason to stop investigating.
[`TODO/measurement.md`](../TODO/measurement.md) `T-031` carries the indirect
routes -- NAT64/DNS64, relays, oracle correlation, public gateways, and the
dual-stack check that may dissolve part of the problem for free.

A row that says "cannot be measured here" owes the reader the pointer to what
*can* be done instead. Rows corrected accordingly.

### Round-1 correction: the User-Agent rule was asserted, not measured

An earlier revision of RULES 4 asserted as non-negotiable that the probe
identifies itself with a descriptive User-Agent. **Withdrawn** -- see RULES 4.1.
It rested on `experiments/05`'s six HTTP targets, and it never applied to UDP
at all, since BEP 15 is binary and carries no UA field for the **362 of 1346**
`udp://` URLs in the corpus. `C-56` is the row that replaces the assertion.

| id | claim | status | verify by | if false |
| --- | --- | --- | --- | --- |
| C-56 | A self-identifying User-Agent materially raises the refusal rate against HTTP/HTTPS trackers, relative to a mainstream-client or absent UA | `UNVERIFIED` -- **and load-bearing for every HTTP health number** | `experiments/26-user-agent-block-rate.py` (planned), a paired design over the full HTTP/HTTPS corpus, four UA arms in one run, repeated on a second day | If true: every HTTP/HTTPS liveness measurement taken with the descriptive UA is contaminated and must be re-taken; a refused probe was being recorded as an unreachable tracker. If false: the descriptive UA costs nothing and stays. Either way the exclusion route (BEP 34) is unaffected -- it needs no UA. |

### 7.6 Adding rows

When you meet a factual statement with no row -- in the design brief, in this file, in a
reference, or in your own reasoning -- **add a row before relying on it.** Use the
next free id in the appropriate block. The register is complete when no
load-bearing statement in the project lacks one.

**Rows added in verification round 1**, for statements that were being relied on
without one:

| id | claim | status | verify by | if false |
| --- | --- | --- | --- | --- |
| C-50 | UDP **scrape** (BEP 15) requires an info_hash, unlike a connect; HTTP scrape (BEP 48) takes info_hash as an optional query parameter | **`VERIFIED`** -- BEP 15 offset 16 + 20, n; BEP 48 "there is only one key, the `info_hash` key" | The two BEP message tables | If wrong, the UDP ladder's intrusiveness ranking is wrong. As it stands, UDP scrape needs a synthetic infohash and connect does not. |
| C-51 | A tracker operator can refuse this project by DNS, via a BEP 34 `BITTORRENT` TXT record, and at least one production monitor honours it | **`VERIFIED`** -- `newtrackon/scraper.py:217 get_bep_34` @ `7da7dde` | Read the implementation | Gives RULES 4's "operator requests exclusion" a **standard, automatable** mechanism instead of an email address. **Adopted in code on 2026-09-05** ([T-032](../TODO/measurement.md), closed): `src/trackers/bep34.py` reads the record and both probers consult it before opening a socket. The line this row used to carry -- *"this project has not adopted it in code"* -- is kept here as what was true until then. Two things newTrackon already paid for come with it: use public resolvers rather than the host's (its issue #316 -- Hetzner's internal resolvers did not follow CNAMEs and opt-outs failed *silently*), and a DNS failure is not consent. |
| C-52 | `pkgforge_all` is a strict superset of `desirefire_all`, so it is a derivative source and must be excluded from unique-contribution arithmetic | **`VERIFIED`** -- 1091 of 1091 entries contained | `experiments/19` | If wrong, the source-quality ranking in T-101 is wrong. It was wrong in the first draft, for exactly this reason. |
| C-53 | `newtrackon.com/api/stable` means ">=95 % uptime **and** >=10 days since first seen", not a curated list | **`VERIFIED`** -- `views.py:170` calls `api_percentage(95, added_before=...10 days)` | Read the route | Determines whether `/api/stable` can seed this project's own `stable.txt`. It cannot be copied blindly: its uptime is **announce**-derived (C-38). |
| C-54 | GitHub-hosted runner egress for this repository originates from AS8075 (Microsoft) datacenter address space | **`VERIFIED`** -- `64.236.141.183` and `52.165.101.48`, both AS8075 | `experiments/01`, `experiments/02` conditions block | This is the vantage every published number is conditioned on, and RULES 3.4 requires it in every health record. |
| C-55 | Scheduled workflows run **only on the default branch**, and always on its latest commit | **`VERIFIED`** -- "Scheduled workflows will only run on the default branch" | `experiments/22` | Found while checking C-12. Means the production schedule cannot live on a feature branch, and that a data branch can never carry its own cron. Shapes `D5` and T-084. |
### 7.7 Rows added in verification round 2

Added while building the measurement core (T-020, T-021, T-023, T-025). Each is
here because something in `src/trackers/probe.py` now depends on it, and RULES
1.2 says a statement with no row is not usable.

| id | claim | status | verify by | if false |
| --- | --- | --- | --- | --- |
| C-57 | A correct BEP 15 connect response is **tracker-specific**: nothing but a BEP 15 tracker answers the magic constant `0x41727101980` with our own transaction id echoed back, so `protocol_valid` proves a tracker on UDP where on HTTP it does not | **`VERIFIED` by construction**, not by sampling -- it follows from BEP 15's message tables plus a 32-bit transaction id an off-path party would have to guess | `_PROVING_RUNG` in `src/trackers/probe.py`; `tests.test_probe.StateTable.test_live_requires_the_proving_rung_for_the_transport` | If wrong, UDP liveness is over-claimed across the whole `udp://` half of the corpus (362 URLs). The conservative fallback is to raise UDP's bar to a scrape, which costs an info_hash and is why it is not the default. |
| C-58 | Checking the BEP 15 transaction id **before** trusting the action field is what stops an unsolicited or spoofed datagram being recorded as a live tracker | **`VERIFIED` by mutation** -- removing the check makes `test_spoofed_transaction_id_is_rejected` fail against an oracle that answers correctly except for the id | `tests.test_probe_oracle.UdpOracle.test_spoofed_transaction_id_is_rejected` | UDP is unauthenticated. Without the check, any host that answers first can tell this project any tracker is alive. |
| C-59 | An HTTP body that begins as bencode and stops mid-value is evidence that **something answered**, and is a materially different fact from an HTML page served at the tracker's URL | **`VERIFIED`** -- the decoder distinguishes them: a declared string length running past the end of input, versus a parse that never began | `tests.test_probe_oracle.HttpOracle.test_close_midway_is_truncation_and_never_death` | Conflating them publishes a network fault as a dead tracker. Found by reading what the oracle actually produced rather than accepting a passing test. |
| C-60 | A UDP `connect()` performs a routing-table lookup **without transmitting any datagram**, so IPv6 route availability is determinable at zero cost to anyone else | **`VERIFIED`** -- POSIX connect(2) semantics for `SOCK_DGRAM`; no packet is generated until a send | `_route_present` in `src/trackers/vantage.py` | If wrong, every probe run would be emitting unsolicited traffic to a public resolver address purely to collect vantage metadata. The fallback is to drop the check and record the family as unknown. |
| C-61 | A route existing is **not** the same as egress working: `C-04` measured a runner with an IPv6 stack **and** a route that still cannot get a packet out | **`VERIFIED`** -- `experiments/01`, both images | `experiments/results/`; `Vantage.detect(ipv6_egress=False)` withholds the family | This is why the routing table must not override a measurement. Believing it would make the probe attempt IPv6 and record healthy trackers as dead -- the exact failure `C-04` exists to prevent. |
| C-62 | The authoring sandbox routes outbound HTTPS through an egress proxy (`HTTPS_PROXY`, `CCR_EGRESS_GATEWAY_ENABLED`), so any header-sensitive measurement taken there measures the proxy as well as the tracker | **`VERIFIED`** -- the variables are set in the environment | `env` in the authoring sandbox; absent on a GitHub runner | Decides **where T-012 may run at all**. A User-Agent block-rate measured through a proxy that can add or rewrite headers is not a measurement of tracker behaviour. |

### 7.8 Rows added in verification round 3

Added on 2026-08-31 while mining `Azathothas/bit-cli` -- the only reference in
this corpus that actually speaks the tracker protocols -- and while rebuilding
the reference corpus. Each is here because something in `TODO/` or `src/` now
depends on it.

| id | claim | status | verify by | if false |
| --- | --- | --- | --- | --- |
| C-63 | An HTTP tracker request carries **two** identity fields -- the `User-Agent` header and the BEP 20 Azureus prefix inside the `peer_id` query parameter -- and a tracker's client-filtering rules key on the prefix at least as much as on the header | **`VERIFIED` by construction** -- BEP 3 makes `peer_id` a required announce parameter, BEP 20 defines the `-XXvvvv-` prefix, and `references/Azathothas__bit-cli/tree/crates/bit-cli-core/src/peer_id.rs:36` shows a current client choosing its two-character code against a 92-entry registry precisely so it is not filed under another client | Read BEP 3 and BEP 20; read that file | **T-012 as originally designed measures half the question.** A UA-only experiment that reports "no block" may have been filed under whatever `peer_id` it happened to send. The design is amended to vary both axes. |
| C-64 | A public intermediary will refuse a self-identifying descriptive User-Agent and accept a mainstream-tool one, for identical requests | **`VERIFIED` -- observed first-hand, 2026-08-31.** Every request to `api.gh.pkgforge.dev` through this session's egress proxy carrying `trackers/0.1 (+https://github.com/...)` returned HTTP 420 with an empty body; the identical request carrying `curl/8.5.0` returned 200 in the same second. Header set, spacing and route held constant | `scripts/fetch-reference-comments.py`, whose `USER_AGENT` constant carries the measurement | This is **not** a tracker and does not settle `C-56`. It does establish that the effect RULES 4.1 describes as "reported" is real against at least one class of server, which raises the prior that HTTP tracker liveness measured under a descriptive UA is contaminated. |
| C-65 | HTTP trackers state a re-announce **floor** in a `min interval` key, spelled either with a space or with an underscore, and it binds more tightly than `interval` | **`VERIFIED`** -- BEP 3 defines `min interval`; `references/Azathothas__bit-cli/tree/crates/bit-cli-core/src/tracker.rs:739` reads both spellings in production | Read BEP 3; read that line | D7 anchors the politeness budget on the tracker's own stated interval. Reading only `interval` ignores the stricter number a tracker actually asked for, which is the one an operator would judge us by. `src/trackers/bencode.py` reads `interval` only; T-026 carries the fix. |
| C-66 | BEP 48's scrape-URL derivation is defined as replacing the string `announce` **in the path section**, so a path with no `announce` has no derivable scrape endpoint and guessing one produces a 404 that reads as tracker failure | **`VERIFIED`** -- the specification text, fetched 2026-08-31: *"This is done by locating the string `announce` in the path section of the announce URL and replacing it with the string `scrape`. Performing a scrape request to URLs that are not determined by this method are outside of the scope of this specification."* | `https://www.bittorrent.org/beps/bep_0048.html`; `Tracker.scrape_url` in `src/trackers/model.py` returns `None` rather than guessing | A derived endpoint that the tracker does not serve returns 404, and a 404 recorded against the tracker is a measurement of our guess. The same page also states that **"scrape exchanges have no effect on a peer's participation in a swarm"**, which is the specification-level basis for RULES 4 preferring scrape over announce. |
| C-67 | This project's read proxies return a syntactically valid **empty array** for some GitHub issue-comment threads that demonstrably have comments | **`INFERRED`, not established** -- six threads (`GerryFerdinandus/bittorrent-tracker-editor` #1-#6, reporting 1/5/3/3/4/1 comments) return the literal bytes `[\n\n]` from `api.gh.pkgforge.dev`, from `api.rv.pkgforge.dev`, with and without `per_page`/`page`. The direct route is refused by session policy, so no route returns the bodies and the cause cannot be confirmed | `scripts/fetch-reference-comments.py`, which refuses an empty array when the issue's own count is non-zero and records the failure | **A corpus tool without that guard silently records six empty threads as real.** `Aseem0xff/pacman-static`'s `docs/patches/mine-repo-page-join.md` documents this exact signature upstream -- a joiner recovering array bounds by counting `[` and `]` over raw text, which counts brackets inside markdown comment bodies. Six of 222, all in one repository, is the shape that predicts, and it is a prediction, not a measurement. |
| C-68 | The closest production analogue to this project -- newTrackon, a long-running public tracker monitor -- **impersonates qBittorrent on both identity axes**, consistently | **`VERIFIED`** -- `references/CorralPeltzer__newTrackon/tree/newtrackon/scraper.py:53` sets `User-Agent: qBittorrent/4.3.9` in `SCRAPING_HEADERS`, used for every HTTP fetch at `:429`; `:234` builds `peer_id` as `-qB4390-` plus twelve random characters, which is qBittorrent 4.3.9's BEP 20 prefix. The two agree on the version | Read those three lines at `7da7dde` | **This is the strongest evidence available on `C-56` and it was in the corpus, unread, from round 1.** A production monitor that probes the same trackers this project intends to, over years, made the identity choice deliberately and made it on both axes at once -- which is `C-63`. It does not prove a descriptive UA gets blocked; it proves the operator of the closest analogue judged it not worth the risk. T-012 measures; this sets the prior. |
| C-69 | newTrackon's HTTP announce sends `left=0`, which tells every tracker it probes that it is a **seed** for a random infohash | **`VERIFIED`** -- `scraper.py:238-244` builds `args_dict` with `"left": 0` alongside `peer_id` and a `urandom(20)` info_hash | Read `scraper.py:234-250` at `7da7dde` | Independent of whether it is intentional, it is a concrete cost of the announce-based method this project does not use: `Azathothas/bit-cli`'s `docs/trackers.md` records that `left=0` "treats this client as a **seed** and hands it to every peer asking for one". This project stops at BEP 15 connect and HTTP scrape and therefore never sends `left` at all (RULES 4), which is a real difference in swarm impact and belongs in any newTrackon cross-check (T-028). |
| C-70 | The published dataset carries **private-tracker credentials**: announce URLs whose path or query holds a passkey belonging to a real person | **`VERIFIED` by measurement, 2026-08-31.** `python3 scripts/generate.py --offline` writes seven such URLs into `trackers_all.txt` of 1334 accepted entries, carrying six distinct credentials. They enter from two upstreams that publish them (`DeSireFire/animeTrackerList` and `pkgforge-security/Trackers`) and nothing between the fetch and the output refuses them | `python3 scripts/check-no-secrets.py --public`; and `grep -nE '(passkey=|/announce/[0-9a-f]{20,})' OUT/trackers_all.txt` after a generation | **A tracker aggregator that republishes a passkey hands a stranger's credential to every consumer**, and the tracker it belongs to sees every use of it. It is also the sharpest available answer to the value gate (T-027): refusing them is measurable value over concatenation that no upstream in the corpus provides. **Fixed on 2026-09-05 by T-107**, which is closed: the pipeline refuses all seven, `generate.py` refuses to publish one, the accepted count moved 1334 -> 1327, and the ceiling this row cites was replaced by a path rule with no exemption. The credentials remain in the verbatim upstream captures, which are evidence and are not edited. |
| C-71 | A failed TCP probe does not establish that the **port** is blocked, and this project's own baseline instrument conflated the two | **`VERIFIED`, measured 2026-08-31.** `experiments/01` derived `tcp_ports_blocked` from any row where `ok` was false. On run `33383156641` that reported `[2710]` on both images while the row's own detail read `resolve: gaierror: No address associated with hostname`, and `experiments/04` recorded the same host NXDOMAIN in the same run. After the fix, run `33383406869` reports `tcp_ports_blocked: []` and `tcp_targets_unresolvable: ["bt.okmp3.ru"]` | `experiments/results/01.ubuntu-24.04.run33383406869.json`, and the `_is_resolution_failure` split in `experiments/01-host-network-baseline.py` | **It is RULES 3.1 inside an instrument.** A failure of ours, or of a host that has since disappeared, reported as a fact about the platform. The false verdict reads as "GitHub blocks the classic BitTorrent tracker port", which is a constraint this project would have sized work against and designed around. Every derived verdict in every instrument owes the same split. |
