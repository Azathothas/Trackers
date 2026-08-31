# Foundation

P0 (ground truth) and P1 (acquisition), closed. These entries were written
after the work, when the todo model was adopted, so their acceptance evidence
is the committed instrument and the CI job rather than a session transcript.

They are here rather than only in `HISTORY/` because the index has to reflect
what exists. An index that lists only the backlog makes a cold session think
nothing was built.

---

### T-140 Runner network and protocol behaviour was never measured on a runner

Source:      the design brief's `C-01`-`C-06`
Category:    foundation
Priority:    P0
Effort:      M
Status:      **done**

Problem:     Six claims about what a GitHub-hosted runner can do decided the
             entire measurement architecture, and **none had been tested on a
             runner**. The brief asserted UDP was unavailable and asked for a
             workaround.
Premise:     Every row was `UNVERIFIED`. The authoring environment was not a
             runner and UDP is blocked there, so the sandbox could not answer it
             either way -- which is why the sandbox result is recorded as a
             control rather than as evidence about runners.
Approach:    Five instruments with a three-tier control hierarchy: a loopback
             responder this process starts (tier 0, proves the probe code), a
             third-party service on a non-53 UDP port (tier 1, separates network
             from code), then the subjects.

**Done.** Re-taken 2026-09-01 on workflow run `33383406869`, two images,
`ubuntu-24.04` and `ubuntu-22.04`. ⚠ The run this entry originally closed
against belonged to this repository's prior history and its artefacts went with
it, so every figure below is the new measurement rather than the old one.

```
udp_arbitrary_port_egress : true   (both images)
udp_port_53_egress        : true   (both images)
ipv6_stack_present        : true   (both images)
ipv6_egress               : false  (both images)
tcp_ports_open            : 80, 443, 2095, 6969, 8080
tcp_ports_blocked         : []
tcp_targets_unresolvable  : bt.okmp3.ru
```

**`C-01` is REFUTED and the workaround was never built**, per its own "if
false" clause. Tier-0 passed and all four tier-1 controls passed (STUN 19302,
STUN 3478, NTP 123 x2); BEP 15 connect completed **10/11, 9/11, 10/11, 10/11**
across four runs with the loopback control passing every time, median RTT
**97.5 to 103.9 ms**. ⚠ **11 is not the ceiling: 10 is.** One target has no
IPv4 address at all, so it can never reach the connect rung from here, and
counting it as a failure would be counting our own vantage as its liveness.

**`C-04` is VERIFIED as a hazard**: no IPv6 egress with the stack present, so
IPv6-only trackers are `unmeasurable` and never `dead`.

**`C-06`** found **14 agree / 3 both-failed / 0 divergent** at n=17 on both
images. ⚠ **The run 40 minutes earlier found 1 divergent**, at
`tracker.torrent.eu.org`, which resolved to a different address locally than at
either public resolver and then agreed on the next run. **One run of two is not
a measurement of divergence, it is the reason T-007 exists.** Carried forward.

**`C-71` was found by re-running this instrument**: `tcp_ports_blocked` used to
count a hostname that no longer resolves as a blocked port.

**`C-02`** is recorded inconclusive because a failed hairpin does not
distinguish blocked inbound from a NAT that does not hairpin; carried forward
as T-008.

Prove:       `experiments/01-host-network-baseline.py` on both runner images,
             with the results committed under
             `experiments/results/01.ubuntu-24.04.run33383406869.json` and its
             `ubuntu-22.04` sibling, because workflow artefacts expire after 90
             days and git does not.

---

### T-141 No reference had been read below README depth

Source:      the brief's section 4
Category:    foundation
Priority:    P1
Effort:      L
Status:      **done**

Problem:     Seven references decided the architecture and every claim about
             them came from a README. No repository had been cloned, no issue
             tracker read, no archival status confirmed.
Premise:     Recorded as README-depth and flagged as P0 work.
Approach:    Clone at captured commits, read the code, **read the tracker in
             both states**, keep the corpus tracked, one verdict each.

**Done.** Corpus at `references/` with `COMMIT` files and `PROVENANCE.md`
recording what could **not** be obtained. **Seven repositories at acceptance;
ten now** -- round 2 added `Azathothas/bit-cli`, `Aseem0xff/pacman-static` and
`AvalynSouvlaki/T-244-RESEARCH`, and took issue comments from four threads to
216. Discussions and review comments remain unfetched for every reference, and
four trackers were never fetched at all.

Five claims refuted by reading code rather than READMEs: `C-20` (the prior art
reads `/api/stable`, not the HTML `/list` its README names), `C-22` (ngosang
publishes **no generator**, so its sort order is unauditable by anyone), `C-26`
(newTrackon **does** expose uptime at `/api/<int:percentage>`; the earlier 404
came from probing the parameter's name as a path), `C-27` (DeSireFire is stale
**and** contributes 995 unique URLs of 1091), `C-38` (the politeness anchor).

Prove:       `HISTORY/reference-sweep.md`, which opens with what it did
             **not** establish, and `references/PROVENANCE.md`.

---

### T-142 The protocol model was mis-factored and would have marked I2P trackers dead

Source:      the brief's section 8.1, which told the reader not to trust its own table
Category:    foundation
Priority:    P1
Effort:      M
Status:      **done**

Problem:     The brief tabulated `udp`, `http`, `https`, `ws`/`wss`, `*.i2p`,
             yggdrasil and `*.onion` as seven values of one variable.
Premise:     One sample of one README, and it said so.
Approach:    A census over every candidate source, reporting transport and
             network as separate axes.

**Done.** `experiments/19-scheme-census.py`, union of 16 source files, **1346
distinct URLs** (`HISTORY/corpus-baseline.md`). `trackers_all_i2p.txt` contains schemes `http` (11) and `udp`
(2): **`.i2p` is a hostname suffix, not a scheme**, so a scheme-keyed classifier
routes I2P entries to the clearnet prober and records them dead. The model is
**transport x network**, independently confirmed by a real consumer's own
validator at `torrent_miscellaneous.pas:393` listing exactly those five
transports.

Two defects in the experiment's own first draft were fixed rather than shipped:
it compared sources against `pkgforge_all`, a strict superset of
`desirefire_all` (1091/1091), and so reported the corpus's largest unique
contributor as contributing **zero**; and it counted blacklist entries as
available trackers, inverting their meaning.

Prove:       `python3 experiments/19-scheme-census.py --offline --expect-known-schemes`

---

### T-143 There was no pipeline, and determinism had never been demonstrated

Source:      the brief's section 8.2, section 8.3, section 11, section 13, section 17.1, section 25 (now RULES 3.10,
             RULES 3.6, RULES 5, T-001, and `src/trackers/dedup.py`)
Category:    foundation
Priority:    P1
Effort:      L
Status:      **done**

Problem:     Nothing fetched, normalized, deduplicated or published anything.
Premise:     The invariants were prose.
Approach:    `src/trackers/`, standard library only, with the invariants made
             structural rather than remembered.

**Done.** 1337 accepted trackers at acceptance from 8 sources (**1334** now,
after review 6 added the RFC 3986 character check), generated end-to-end from
committed fixtures with **no network**, byte-identical across two runs, and the
same sha256 on the runner as locally -- so it is reproducible across machines,
not merely across runs.

`acquire.py` makes RULES 3.2 unrepresentable-as-wrong: `FetchResult.trackers` is
`None`, never `[]`, when a fetch failed. `dedup.py` implements question 1,
reports question 2 without acting, and **refuses** question 3. `normalize.py`
documents every rule with the reason it is safe, including the two refusals --
explicit ports are never defaulted away, and path case and trailing slashes are
preserved.

Prove:       `python3 -m unittest discover -s tests` (48 tests at acceptance,
             no network; the suite has grown since), and
             the `gate.yml` job that generates twice offline and diffs byte for
             byte.

---

### T-144 182 blacklisted URLs reached the output

Source:      found by the pre-publication verifier during T-143
Category:    foundation
Priority:    P2
Effort:      M
Status:      **done**

Problem:     A blacklist source contributes no trackers, but the *other* sources
             still carried entries ngosang had removed. Both obvious answers are
             wrong: adopting the blacklist wholesale inherits 331 unauditable
             filtering decisions, and ignoring it breaks the requirement to
             honour operator exclusion requests.
Premise:     Measured from the committed fixture, 346 entries: 178 "registered
             torrents", 135 "duplicate of <url>", 13 "malfunction", 7
             "deprecated by owner", 5 "detected by antivirus software", 2 "fake
             seeds", **2 "requested by sysadmin"**, and singletons.
Approach:    Classify the reason rather than counting the entry.

**Done.** `src/trackers/exclusion.py`. **HONOUR** (operator requests) and
**SAFETY** are enforced; **OPINION** is kept and flagged. Measured effect: 9 + 6
= **15 enforced**, **331 kept and flagged**, 8 entries actually removed.
Unrecognised reasons default to OPINION, which is the safe direction -- treating
an operator request as an opinion would mean continuing to probe somebody who
asked us to stop.

Adopting wholesale would have deleted `bt.okmp3.ru`: blacklisted upstream as
"fake seeds", listed **live** by newTrackon, and proved a working tracker by
this project's own runner probe. Three observers, three answers.

Prove:       `python3 -m unittest tests.test_p1.TestExclusionClassification -v`

---

### T-145 Nothing enforced the rules the project had written down

Source:      the brief's section 6 (counts enforced by a checker, not by hand)
Category:    foundation
Priority:    P2
Effort:      S
Status:      **done**

Problem:     Rules that only exist in prose get optimised away.
Premise:     The brief named the hazard explicitly for the record's counts.
Approach:    Gates that run in CI, not by hand.

**Done.** `check-no-third-party-imports.py` (parses with `ast`, not grep,
because this repository's prose is full of the word *import*),
`check-decision-record.py`, and `check-vantage-metadata.py` -- which exits **2**,
"could not run", and **refuses to pass vacuously** over an empty set.

The value of running them in CI was demonstrated immediately: the D1 gate failed
on the runner over `scripts/generate.py: imports 'trackers'`, a **false positive
in the checker itself** that passed locally only because it had been run before
`src/trackers/` existed. Fixed and verified in both directions -- the real tree
passes, and a deliberately planted `import requests` still fails it.

Prove:       `.github/workflows/gate.yml` runs all three on every push.

---

### T-146 The README did not exist, so the honesty statements had nowhere to live

Source:      the brief's section 28 and section 9.2 (now T-120 and RULES 3.4)
Category:    foundation
Priority:    P1
Effort:      S
Status:      **done**

Problem:     The requirement is that the **README** -- not only a methodology
             page -- states what the measurements do and do not generalise to,
             because the people most likely to misread the data will never open
             a methodology page. There was no README at all.
Premise:     The limitations were measured and written down in places a consumer
             would not look.
Approach:    Open with the limitation rather than burying it.

**Done.** `README.md` opens with the vantage constraint, classifies every
capability as guaranteed / best-effort / externally dependent / **unavailable**,
states plainly that no dataset is published and nothing claims any tracker is
alive, documents both operator exclusion routes, and checks the "no attribution"
claim against the actual `LICENSE` text rather than repeating it.

Prove:       `README.md`, and T-121 will make its citations checkable.
