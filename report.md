# Run report

generated_at: 2026-09-11T22:58:23Z
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

- accepted trackers: 1321
- rejected lines:    3
- duplicates removed: 614 (255 removed)

### Transport

- http: 713
- https: 240
- udp: 358
- wss: 10

### Network

- clearnet: 1308
- i2p: 13

### Measurability

- measurable from this vantage: 1298
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

- health observations: 4054 across 1318 tracker(s)
- never observed:      3
- health states:       {'degraded': 8, 'live': 191, 'unknown': 1048, 'unmeasurable': 71}
- measurement rungs:   {'connected': 195, 'dns': 391, 'no_usable_address': 13, 'none': 409, 'protocol_valid': 75, 'tracker_semantic': 119, 'transport_response': 116}
- observation depth:   median 3, deepest 7
- sustained failures:  710 (3+ observations, none successful)

⛔ A tracker that did not answer is `unknown`, never `dead`: saying
dead needs 3 observations of one tracker and the state machine is
the only place that decision is made.

- sustained: `http://00.xxtor.com:443/announce`
- sustained: `http://0123456789nonexistent.com:80/announce`
- sustained: `http://106.14.254.164:6969/announce`
- sustained: `http://107.189.31.134:6969/announce`
- sustained: `http://116.252.176.125:6969/announce`
- sustained: `http://116.9.207.121:6969/announce`
- sustained: `http://13.115.115.32:6969/announce`
- sustained: `http://140.82.21.192:8080/announce`
- sustained: `http://144.202.33.210:6961/announce`
- sustained: `http://144.76.118.107:6969/announce`
- sustained: `http://147job.com:6969/announce`
- sustained: `http://151.115.49.115:1337/announce`
- sustained: `http://155.248.200.105/announce`
- sustained: `http://156.234.201.18/announce`
- sustained: `http://156.234.201.18:80/announce`
- sustained: `http://157.90.169.123/announce`
- sustained: `http://158.101.137.177:6969/announce`
- sustained: `http://159.69.65.157:6969/announce`
- sustained: `http://160.251.78.190:6969/announce`
- sustained: `http://163.172.209.40/announce`

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

### common.txt -- 191

- rule: the other categories merged and deduplicated, plus every tracker whose most recent observation was `live`
- why:  40 from the other categories and 151 more that measured live

### foss.txt -- 0 (evidence absent)

- rule: provenance from a source the registry classifies `foss`, plus a seed labelled as curated rather than measured (D9)
- why:  no source in the registry is classified `foss` and the curated seed is empty. The seed stays empty until the operator supplies entries: a guessed one is the methodology lie D9 exists to prevent

### hardcoded.txt -- 0 (evidence absent)

- rule: the maintainer's manual list, in their order, deduplicated against itself, never sorted and never ranked
- why:  no input file exists yet (T-106); the renderer that preserves manual order is implemented and tested

### stable.txt -- 40

- rule: at least 5 observations and a success rate of 0.95 or better, measured
- why:  40 qualified; deepest history is 7
