# Run report

generated_at: 2026-09-08T23:49:32Z
code_version: 0.1.0+norm1

## Sources

- sources fetched: 8

- ok:       8 ['desirefire_all', 'newtrackon_all', 'ngosang_all', 'ngosang_blacklist', 'ngosang_i2p', 'ngosang_ws', 'ngosang_yggdrasil', 'xiu2_all']
- unchanged: 0 [] (304; the held snapshot is current)
- failed:   0 []
- rejected: 0 []
- empty:    0 []

`failed` and `empty` are different states and are counted separately.
A failed source contributed nothing and blocked nothing; the previous
accepted data for it stands (RULES 3.10).

## Dataset

- accepted trackers: 1326
- rejected lines:    3
- duplicates removed: 597 (239 removed)

### Transport

- http: 709
- https: 246
- udp: 361
- wss: 10

### Network

- clearnet: 1313
- i2p: 13

### Measurability

- measurable from this vantage: 1303
- unmeasurable:                 23

An unmeasurable tracker is one this vantage cannot reach at all
(no IPv6 egress; i2p/yggdrasil/onion need routers; ws/wss unverified).
It is never reported dead -- that would measure the probe, not the
tracker (RULES 3.1 requirement 1).

## Refused entries

- refused: 15

Every entry offered by a source and not published, with the reason.
A tracker that vanishes owes the consumer who noticed an explanation
(RULES 3.10), so this is a returned value and not a log line.

⚠ A URL refused for carrying a private-tracker credential is listed
with the credential removed. The token is what got it refused;
printing it here would republish in the report what the dataset
declined to republish (T-107).

So two of these lines can read identically: two people's credentials
on one endpoint differ only in the part that is not shown. They are
counted separately above, which is the number that matters.

- `http://btracker.top:11451/announce` -- upstream exclusion: operator request or safety [desirefire_all]
- `http://p2p.0g.cx:6969/announce` -- upstream exclusion: operator request or safety [desirefire_all]
- `http://tracker.anirena.com/<redacted>/announce` -- carries a private-tracker credential (T-107) [desirefire_all]
- `http://tracker.anirena.com:80/<redacted>/announce` -- carries a private-tracker credential (T-107) [desirefire_all]
- `http://tracker.breizh.pm:6969/announce` -- upstream exclusion: operator request or safety [desirefire_all]
- `http://www.ansktracker.net/announce.php?passkey=<redacted>` -- carries a private-tracker credential (T-107) [desirefire_all]
- `http://www.arabp2p.net:2052/<redacted>/announce` -- carries a private-tracker credential (T-107) [desirefire_all]
- `https://k3tracker.cc/announce/<redacted>` -- carries a private-tracker credential (T-107) [desirefire_all]
- `https://t.btcland.xyz:443/announce` -- upstream exclusion: operator request or safety [desirefire_all]
- `https://tr.fuckbitcoin.xyz:443/announce` -- upstream exclusion: operator request or safety [desirefire_all]
- `https://tr.highstar.shop:443/announce` -- upstream exclusion: operator request or safety [desirefire_all]
- `https://tracker.jiesen.life:8443/announce` -- upstream exclusion: operator request or safety [desirefire_all]
- `https://tracker.monikadesign.uk/announce/<redacted>` -- carries a private-tracker credential (T-107) [desirefire_all]
- `https://tracker.monikadesign.uk/announce/<redacted>` -- carries a private-tracker credential (T-107) [desirefire_all]
- `udp://tracker.breizh.pm:6969/announce` -- upstream exclusion: operator request or safety [newtrackon_all, xiu2_all]

## Health

- health observations: 672 across 424 tracker(s)
- never observed:      902
- health states:       {'degraded': 1, 'live': 101, 'unknown': 303, 'unmeasurable': 19}
- measurement rungs:   {'connected': 51, 'dns': 132, 'no_usable_address': 1, 'none': 104, 'protocol_valid': 57, 'tracker_semantic': 46, 'transport_response': 33}
- observation depth:   median 1, deepest 4
- sustained failures:  28 (3+ observations, none successful)

⛔ A tracker that did not answer is `unknown`, never `dead`: saying
dead needs 3 observations of one tracker and the state machine is
the only place that decision is made.

- sustained: `http://171.104.110.88:6969/announce`
- sustained: `http://49.12.76.8:6961/announce`
- sustained: `http://51.38.230.101/announce`
- sustained: `http://54.39.98.124:80/announce`
- sustained: `http://bithq.org:80/announce.php`
- sustained: `http://mediaclub.tv/announce.php`
- sustained: `http://opentracker.xyz:80/announce`
- sustained: `http://torrent-team.net/announce.php`
- sustained: `http://torrents.hikarinokiseki.com:6969/announce`
- sustained: `http://tracker.bz:80/announce`
- sustained: `http://tracker.cbase.cc:6969/announce`
- sustained: `http://tracker.privateseedbox.xyz:2710/announce`
- sustained: `http://tracker.sbsub.com:2710/announce`
- sustained: `http://wegkxfcivgx.ydns.eu:80/announce`
- sustained: `http://www.yqzuji.com/announce`
- sustained: `http://yggtracker.i2p.rocks:80/announce`
- sustained: `https://021912.xyz:443/announce`
- sustained: `https://bt.080609.xyz:443/announce`
- sustained: `https://tracker.alaskantf.com:443/announce`
- sustained: `https://tracker.nyaa.tk/announce`

## What this report cannot answer yet

- **stale sources**: whether an upstream has stopped being updated.
  Answering it needs each fetch's `Last-Modified` or `ETag` retained
  across runs, and provenance snapshots are not kept yet (T-103). ⚠ A
  source that contributed nothing **this** run is reported above as
  failed or empty, which is a different question and is answered.
- **latency distribution**: the history keeps each observation's
  outcome and rung, not its round-trip time. A distribution here
  would be a number this project does not retain.
- **ranking changes**: nothing is ranked. No scoring model is chosen
  (T-044), deliberately, because the history is too short to fit one
  against without fitting it to noise.
- **reliability distribution**: the same reason. The invariants a
  model must satisfy exist and are tested (T-043); the model does not.
- **whether publication succeeded**: this report is written *before*
  publication, by the step whose output is being published. It cannot
  report on an event that has not happened. The workflow's own summary
  answers it.

## Categories

Each file's membership rule, and why it holds what it holds.
⛔ An empty file is not a defect: it says below whether the rule
matched nothing or the evidence it needs does not exist yet.

### anime.txt -- 1074

- rule: provenance from a source the registry classifies `anime`
- why:  1074 contributed by desirefire_all

### common.txt -- 101

- rule: the other categories merged and deduplicated, plus every tracker whose most recent observation was `live`
- why:  0 from the other categories and 101 more that measured live

### foss.txt -- 0 (evidence absent)

- rule: provenance from a source the registry classifies `foss`, plus a seed labelled as curated rather than measured (D9)
- why:  no source in the registry is classified `foss` and the curated seed is empty. The seed stays empty until the operator supplies entries: a guessed one is the methodology lie D9 exists to prevent

### hardcoded.txt -- 0 (evidence absent)

- rule: the maintainer's manual list, in their order, deduplicated against itself, never sorted and never ranked
- why:  no input file exists yet (T-106); the renderer that preserves manual order is implemented and tested

### stable.txt -- 0 (evidence absent)

- rule: at least 5 observations and a success rate of 0.95 or better, measured
- why:  no tracker has 5 observations yet; the deepest history is 4. An empty file is the honest answer on day one and a reputation-seeded one would be a lie about methodology
