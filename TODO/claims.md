# Claims

Verification work. Each entry is a claim in [`HISTORY/claims.md`](../HISTORY/claims.md)
that is not `VERIFIED` and that something depends on, or a verified claim whose
evidence is too thin to keep relying on.

A claim with no entry here is either verified, or verified-and-stable, or
recorded as not load-bearing. **A row that says `UNVERIFIED` is not usable**
(RULES 1.2), so an entry here is what stands between a claim and the code that
wants it.

---

### T-001 No torrent client has ever been run against our plaintext

Source:      `C-40`, `C-41`; HISTORY/gates.md "the primary audience"
Category:    claims
Priority:    P0
Effort:      M
Status:      open

Problem:     `render_plaintext` emits the format the primary consumers use, and
             nothing has ever fed that output to qBittorrent, Transmission,
             aria2, Deluge or BiglyBT. If the format is wrong, the project's
             main deliverable is unusable by the people it is for, and no test
             here would notice.
Premise:     **Read, not measured.** One client's parser was read at
             `references/GerryFerdinandus__bittorrent-tracker-editor/tree/source/code/torrent_miscellaneous.pas:174`
             (`SanitizeTrackerList`) and `:393` (`ValidTrackerURL`). It trims,
             truncates at the first space so a trailing ` # reason` is stripped,
             and accepts only the five known transport prefixes -- so a
             whole-line `#` comment is rejected as an invalid URL rather than
             accepted as a tracker. That is one client, and its tolerance is
             partly incidental: the body is loaded via
             `TStringList.DelimitedText`, which splits on whitespace.
Approach:    Experiment 23. Feed a fixture list to each client's list-import
             path and observe what is accepted. Where a client cannot be run,
             read its list parser at a captured commit and cite file and line.
             Four variants: plain `\n`; blank-line separated (newTrackon's own
             format, measured 78 blank of 156 lines); with `#` comments; with
             CRLF.
Decision:    Until this closes, `render_plaintext` stays at the most
             conservative intersection -- one URL per line, single `\n`, no
             comments, no blank lines. That is not a guess about clients; it is
             a refusal to guess.
Prove:       `python3 experiments/23-client-list-compatibility.py --expect-all`
             exits 0, and `HISTORY/claims.md` rows `C-40` and `C-41` cite it.

---

### T-002 A public repository's schedule stops after 60 days and nothing here notices

Source:      `C-12`, verified TRUE by `experiments/22`
Category:    claims
Priority:    P0
Effort:      S
Status:      open

Problem:     GitHub's own documentation: "In a public repository, scheduled
             workflows are automatically disabled when no repository activity
             has occurred in 60 days." The project's stated purpose is
             indefinite unattended operation. **A dataset that stops updating
             without telling anyone is the worst failure mode available**, and
             right now nothing would detect it.
Premise:     **Verified**, `experiments/22-actions-platform-contract.py`, the
             sentence pinned as a regex so the check fails if the docs change.
             The 60-day timer itself has *not* been observed -- that would take
             60 days of deliberate silence -- so this is documented behaviour,
             not measured behaviour.
Approach:    Two halves, and the second is the one that matters.
             (a) Guarantee qualifying activity: the hourly publication commits
             to the data branch, which is repository activity.
             (b) **Detect our own silence anyway**, because (a) is exactly the
             thing that breaks. The published metadata carries `generated_at`;
             a consumer-visible staleness marker and a watchdog that raises an
             issue when the newest dataset is older than N intervals.
Decision:    (a) alone is insufficient. If publication is what keeps the
             schedule alive, then publication failing takes the schedule with
             it, and the failure is silent by construction. Build both.
Prove:       A test that feeds the watchdog a dataset timestamped older than the
             threshold and asserts it reports stale; plus
             `python3 experiments/22-actions-platform-contract.py --expect-all`.

---

### T-003 Release and tag behaviour is unverified, and it blocks the publication topology

Source:      `C-14`, `C-15`, `C-17`
Category:    claims
Priority:    P1
Effort:      S
Status:      done

Problem:     Three unknowns: whether `/releases/latest` resolves to the newest
             non-prerelease rather than to a tag literally named `latest`;
             whether a release asset can be replaced at a stable
             `.../releases/download/<tag>/<name>` URL; and whether moving a git
             tag moves the release. The channel semantics in T-064 rest on all
             three.
Premise:     Unverified. Nothing was assumed in their place: the consumer
             guidance is written conservatively enough to be correct either way.
Authorised:  **Was blocked; unblocked by operator ruling 2026-08-29.** Creating,
             mutating and deleting throwaway releases **in this repository** is
             sanctioned (RULES 13.1). Tag them `test-*`, and delete them once
             the answer is recorded -- a throwaway that outlives its question is
             litter in the release list.
Approach:    Experiment 24. Create a release tagged `latest` and a second,
             newer, non-prerelease release; fetch `/releases/latest` and record
             which resolves. Upload an asset, replace it, re-fetch, and inspect
             cache headers. Move a tag and observe the release's target.
Prove:       `python3 experiments/24-release-channel-behaviour.py` exits 0 and
             `HISTORY/claims.md` rows `C-14`, `C-15`, `C-17` cite it.

**Done.** `python3 experiments/24-release-channel-behaviour.py --expect-design`
exits 0. Three runs are committed under `experiments/results/`, all from
`unclassified-host` on 2026-09-05.

| claim | answer |
| --- | --- |
| `C-14` | **VERIFIED, both halves.** A tag literally named `latest` earns nothing, and a newer *prerelease* does not take the channel from a stable release |
| `C-15` | **VERIFIED, with a condition** |
| `C-17` | **REFUTED** |

⭐ **The C-15 condition is the finding, and the first run got it wrong.** That
run fetched once, three seconds after replacing the asset, saw the old bytes
and an unchanged `ETag`, and would have recorded "assets cannot be replaced".
RULES 2 requires a control that isolates a cause before one is named, so the
instrument was given one: the asset's **API metadata** before and after, which
separates "the replacement never happened" from "it happened and something is
serving a cached copy". The id and the size both changed, so the replacement
landed and the URL was serving stale bytes.

⚠ **And the window is variable, which is why the second run was not enough
either.** Across three runs minutes apart from one host: one served the new
content immediately, one still served the old content at 10 s and had switched
by 40 s. `Cache-Control` was absent on every fetch, so **nothing warns a
consumer that what they have read is stale.**

⛔ **So `--expect-design` deliberately does not assert on it.** It asserts the
contract -- the channel resolves correctly, the URL is stable, the replacement
lands server-side -- and records the propagation window as a measurement.
Asserting a third party's CDN timing would make this check fail for a reason
nobody cares about, which is how a check stops being read.

**`C-17` is refuted and that decides something.** Moving the tag left the
release's `target_commitish` on the old commit while `tarball_url`, which is by
tag name, followed the tag. Two consumers reading the same release get
different commits and neither is wrong, silently. **Delete-and-recreate is the
route for [T-064](publication.md)**, not move-the-tag.

**The throwaways are gone.** The repository had 0 releases and 0 tags before
and after each run, asserted by the script rather than by eye, and a run that
dies mid-way deletes what it made on the way out. ⚠ One tag was deliberately
named `latest` rather than `test-*`: C-14 is the question "does a tag named
`latest` win?" and cannot be asked otherwise.

---

### T-004 Vantage bias is unresolved and the dataset cannot distinguish dead from dead-from-AS8075

Source:      `C-03`; decision D2 states this as its known cost
Category:    claims
Priority:    P1
Effort:      L
Status:      open

Problem:     Every measurement comes from one cloud provider's address space.
             Trackers commonly rate-limit or block datacenter ranges, so a
             tracker that is healthy for a residential consumer can measure as
             unreachable here. The dataset cannot tell the two apart.
Premise:     **Measured but far too small to conclude.** Of 11 UDP targets that
             newTrackon listed live at fixture capture, 9 answered us; the 2
             that did not are explained (one IPv6-only, one timeout), leaving at
             most **one** candidate disagreement. That is not enough to support
             or refute a claim about datacenter blocking.
Approach:    D2 rejected *operating* a second measurement environment. That
             closes one route and not the question (RULES 10.1a). Routes that
             remain, none of which need infrastructure this project runs:
             (a) **oracle correlation at scale** -- newTrackon observes from a
             different vantage; systematic disagreement across the whole corpus
             is the signal, and it is free;
             (b) **the read proxies** already approved for source fetches are a
             second network position for HTTP-shaped probes;
             (c) a **contributed vantage** the project consumes but does not
             operate;
             (d) **within-AS8075 variation** -- the two runner images already
             came from different IPs (64.236.141.183, 52.165.101.48); whether
             results differ by runner IP is measurable now, for nothing, and
             bounds how much of the bias is address-specific.
             Start with (d) and (a): both are available today.
Decision:    D2 stands -- no self-hosted runner. The mitigation is labelling, and
             **labelling does not make the number better; it only stops it
             lying.** This entry stays open because the limitation is real, not
             because the decision is in doubt.
Prove:       A cross-check report over the full corpus that states the
             disagreement rate with newTrackon *and* the methodology difference
             (they announce, we scrape), with a sample count.

---

### T-005 WebTorrent trackers are unmeasurable by default and nobody has tried

Source:      `C-36`
Category:    claims
Priority:    P2
Effort:      M
Status:      open

Problem:     **10 `wss` entries** in the census union, and one `ws` entry that
             exists only inside ngosang's blacklist
             (`HISTORY/corpus-baseline.md`; an earlier revision of this entry
             said 12 and 1, neither of which the instrument reports). They are
             classified `unmeasurable` because no handshake has been attempted,
             not because one was shown impossible. That is the honest default
             and it is also an untested assumption.
Premise:     Measured. `ws://` occurs **zero** times across the 1346-URL union;
             the corpus's single `ws://` is inside ngosang's blacklist, so `wss`
             is the live form (`HISTORY/corpus-baseline.md`).
Approach:    Experiment 25. Read the WebTorrent tracker specification, then
             attempt a WebSocket handshake against the `wss` entries from a
             runner. **Nothing stops this today** -- a WebSocket handshake is
             ordinary TCP plus TLS plus an HTTP Upgrade, all of which this
             vantage has. The `unmeasurable` label here is inertia, not a
             constraint, which is exactly the failure RULES 10.1a describes.
             If the handshake completes they join the ladder with their own rung
             set; if not, they stay `unmeasurable` **with a measured reason**,
             and T-031's indirect routes apply.
Decision:    **Priority stays P2 and the reason is now evidence rather than
             taste.** `Azathothas/bit-cli` -- a current, actively developed
             BitTorrent client -- lists WebTorrent under *completeness* gaps
             rather than *reach* gaps in
             `references/Azathothas__bit-cli/tree/docs/bep-coverage.md`: it does
             not implement `ws`/`wss` tracker support and is not thereby
             prevented from talking to any peer it can otherwise reach. So
             `wss` is genuinely a different protocol that serious clients
             choose not to carry, which supports both halves: `unmeasurable` is
             the correct label today, **and** a WebTorrent probe is optional
             work rather than owed work. Ten URLs of 1346 is the size of the
             prize.
Prove:       `python3 experiments/25-webtorrent-handshake.py` (planned) exits 0
             and `C-36` cites it.

---

### T-006 Actions billing for public repositories is unverified

Source:      `C-19`
Category:    claims
Priority:    P2
Effort:      S
Status:      open

Problem:     The cost model assumes Actions minutes are free for public
             repositories. If that is wrong, the hourly cadence is unaffordable
             and the whole schedule changes.
Premise:     Unverified. Not yet load-bearing because nothing is scheduled yet.
Approach:    Pin the billing documentation's sentence the way `experiments/22`
             pins the scheduling ones, so it fails when the wording changes.
Prove:       `experiments/22` extended with the billing assertion, `--expect-all`
             exits 0.

---

### T-007 Resolver agreement was measured at n=17 on one day

Source:      `C-06`
Category:    claims
Priority:    P2
Effort:      S
Status:      open

Problem:     `C-06` is marked `VERIFIED` on the strength of 17 hostnames x 4
             resolvers, one day, zero divergence. That is a real measurement and
             a thin one, and the consequence of being wrong is that
             `dns_failure` means "our resolver has an opinion" rather than "the
             tracker is gone".
Premise:     **Verified but weak, and there is production evidence the risk is
             real.** newTrackon issue #316: BEP 34 opt-outs were silently not
             honoured on the official instance because **Hetzner's internal
             resolvers did not follow CNAMEs**. A datacenter resolver differing
             from a public one broke a correctness property, silently. n=17 did
             not detect that class of problem; it would not have.
Approach:    Run `experiments/04` over the full corpus rather than the pinned
             fixture, at each phase gate, and record the divergence rate with
             its sample count. Add a CNAME-following case specifically.
Prove:       `python3 experiments/04-dns-resolver-divergence.py --targets <full corpus>`
             with the divergence rate recorded in `HISTORY/claims.md` `C-06`.

---

### T-008 Inbound connectivity is inconclusive and the instrument says so

Source:      `C-02`
Category:    claims
Priority:    P3
Effort:      M
Status:      open

Problem:     `experiments/03` establishes that a listener works locally and does
             not answer on the runner's public address. That is **consistent
             with** no usable inbound and is not proof: a failed hairpin is
             equally consistent with a NAT that does not hairpin.
Premise:     Measured, inconclusive, and the script reports it as inconclusive
             rather than rounding it to a conclusion.
Approach:    Needs a prober outside the runner. The project has none and
             inventing one means depending on a third-party port-scan service
             whose failure is indistinguishable from a closed port.
Decision:    **Do not build a design that depends on inbound in either
             direction.** That makes this entry P3: it is a gap in knowledge,
             not a blocker, precisely because nothing rests on it.
Prove:       An external prober confirms or refutes, with the prober named and
             its failure mode distinguishable from a closed port.

---

### T-009 Schedule delay and drop rates are documented but never observed

Source:      `C-11`
Category:    claims
Priority:    P3
Effort:      S
Status:      open

Problem:     GitHub documents that scheduled runs can be delayed and that
             "some queued jobs may be dropped". The *rate* is unknown, so the
             state-recovery design cannot be sized against reality.
Premise:     Documented behaviour verified by `experiments/22`; the rate needs
             >=100 scheduled runs to observe and none have happened.
Approach:    Once a schedule exists, record scheduled-vs-actual start times and
             missed intervals, and publish the distribution.
Decision:    Treat delayed, dropped and duplicated runs as **load-bearing
             regardless of the rate**, because designing for them costs little
             and the alternative is unrecoverable state corruption.
Prove:       A committed record of >=100 scheduled runs with the delay
             distribution and the drop count.

---

### T-010 The reason for pinning actions to SHAs is asserted, not verified

Source:      `C-45`
Category:    claims
Priority:    P3
Effort:      S
Status:      open

Problem:     Documentation says third-party actions are SHA-pinned "because tags
             are mutable". Tag mutability itself has not been demonstrated here.
             Stating a reason that turns out to be wrong is a real problem even
             when the practice is right.
Premise:     The practice is already followed and is defensible regardless. What
             is unverified is the stated *reason*.
Approach:    Either demonstrate tag mutability directly, or cite current
             supply-chain guidance and attribute the claim to it rather than
             asserting it in this project's own voice.
Prove:       `HISTORY/claims.md` `C-45` carries either a demonstration or an
             attributed citation.

---

### T-011 Two reference documents were never read in full

Source:      `C-29`
Category:    claims
Priority:    P3
Effort:      S
Status:      done

Problem:     `Aseem0xff/pacman-static` -> its `docs/patches/mine-repo-page-join.md`
             and `AvalynSouvlaki/T-244-RESEARCH` are named as the documentation
             quality bar. Their metadata was read and their licences confirmed.
             **The documents themselves were not opened.**
Premise:     Was: reachable, not read. **The provenance record also named the
             first repository under a fabricated owner** (`Azathothas/`, which
             returns 404); the real one is `Aseem0xff/`, confirmed 200. A
             citation that 404s costs the next session the time to discover it
             is wrong and teaches it to distrust the rest.
Approach:    Clone both into `references/` at captured commits, read in full,
             and record what transfers as a verdict in `HISTORY/references/`.
Decision:    `pacman-static`'s own `references/` tree (15 MB) was stripped from
             the capture, for the reason `references/PROVENANCE.md` records
             under *What was trimmed*. Recorded there rather than trimmed
             silently.
Prove:       `references/Aseem0xff__pacman-static/COMMIT` exists and
             `HISTORY/references/` carries a verdict for each.

**Done.** 2026-08-31. `references/Aseem0xff__pacman-static/COMMIT` =
             `38f7e3e45730f9a6dd4d62675dc1e9594b90f4e4`,
             `references/AvalynSouvlaki__T-244-RESEARCH/COMMIT` =
             `88a84107c52b5d22297c023b0f0bd447ed2d9e15`; verdicts in
`HISTORY/references/aseem0xff-pacman-static.md` and
`HISTORY/references/avalynsouvlaki-t-244-research.md`, both **adopt
(methodology)**. `python3 scripts/check-citations.py` exits 0, which is what
proves the corpus paths resolve.

**Three things came out of it that changed this project.** (1) The
`git rev-parse`-in-a-stripped-corpus defect, which this session then hit itself
and caught only because the real SHAs had been captured a step earlier. (2) A
corrections table needs a severity column. (3) `C-43`'s shortlist now has an
argument attached rather than four crate names.

---

### T-012 Nobody has measured whether our User-Agent gets us blocked

Source:      operator correction, 2026-08-29; RULES 4.1
Category:    claims
Priority:    P0
Effort:      M
Status:      open

Problem:     The probe sends a self-identifying User-Agent naming the project.
             **Trackers are reported to block clients whose UA does not resemble
             a well-known torrent client.** If that is true here, every HTTP and
             HTTPS health measurement this project takes is contaminated: a
             refused probe is recorded as an unreachable tracker, which is the
             "confident wrongness" failure the project exists to prevent. The
             dataset would be systematically wrong in a way nothing in it
             reveals.
Premise:     **Asserted, not measured, and the assertion has now been withdrawn
             (RULES 4.1).** The only evidence is `experiments/05`: 6 HTTP
             targets, one day, 5 answered. Six is not a sample, and those six
             were newTrackon-live at capture, so they are the friendliest
             possible subjects. Note also that this question **cannot** apply to
             UDP: BEP 15 is binary with no UA field, so 362 of 1346 corpus URLs
             are unaffected either way.
Approach:    Experiment 26, a **paired** design -- the same targets, the same
             run, the same code path, differing only in the identity, because
             comparing across runs would confound identity with tracker
             availability.

             **Two axes, not one (`C-63`).** An HTTP tracker request carries a
             `User-Agent` header *and* a `peer_id` whose BEP 20 Azureus prefix
             is what a tracker's client-filtering rules are actually written
             against -- it is what its statistics page reports.
             `references/Azathothas__bit-cli/tree/crates/bit-cli-core/src/peer_id.rs:36`
             shows a current client picking its two-character code against a
             92-entry registry precisely so it is not filed under somebody
             else's client. **An experiment that varies only the UA measures
             half the question**, and a "no block" result from it would be
             indistinguishable from "we happened to send an acceptable
             `peer_id`".

             Arms: UA absent / descriptive / mainstream-client / minimal
             generic, crossed with peer_id absent / an unclaimed code / a
             mainstream client prefix. Report the response-class distribution
             per cell (`tracker_semantic`, HTTP 403, 429, timeout, RST) with
             sample counts and the vantage. Run against the full HTTP/HTTPS
             corpus, not six targets, and repeat on a second day to separate an
             identity effect from an outage.

             **A prior worth stating:** `C-64` records an intermediary refusing
             this project's descriptive UA with HTTP 420 and accepting
             `curl/8.5.0` for the identical request in the same second. That is
             not a tracker and does not settle this, but "nobody refuses us" is
             no longer the measured position.
Decision:    Do not pre-commit to an outcome. **If arms differ materially, the
             measurement wins over the aspiration** -- a probe that is
             systematically refused produces bad data and calling that politeness
             does not make it true. Whatever wins, the exclusion route (BEP 34,
             which needs no UA) stays working and documented, and the one line
             that does not move is RULES 4.1's: never use any identity to evade
             an exclusion already given.
Prove:       `python3 experiments/26-user-agent-block-rate.py --expect-arms` exits
             0, `HISTORY/claims.md` carries `C-56` with the per-arm rates and
             sample counts, and RULES 4.1 is rewritten to state the measured
             answer instead of an open question.
