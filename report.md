# Run report

generated_at: 2026-09-08T21:24:55Z
code_version: 0.1.0+norm1

## Sources

- ok:       8 ['desirefire_all', 'newtrackon_all', 'ngosang_all', 'ngosang_blacklist', 'ngosang_i2p', 'ngosang_ws', 'ngosang_yggdrasil', 'xiu2_all']
- failed:   0 []
- rejected: 0 []
- empty:    0 []

`failed` and `empty` are different states and are counted separately.
A failed source contributed nothing and blocked nothing; the previous
accepted data for it stands (RULES 3.10).

## Dataset

- accepted trackers: 1334
- rejected lines:    3
- dedup decisions:   602 (240 removed)

### Transport

- http: 712
- https: 250
- udp: 362
- wss: 10

### Network

- clearnet: 1321
- i2p: 13

### Measurability

- measurable from this vantage: 1311
- unmeasurable:                 23

An unmeasurable tracker is one this vantage cannot reach at all
(no IPv6 egress; i2p/yggdrasil/onion need routers; ws/wss unverified).
It is never reported dead -- that would measure the probe, not the
tracker (RULES 3.1 requirement 1).

## Refused entries

- refused: 7

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

- `http://tracker.anirena.com/<redacted>/announce` -- carries a private-tracker credential (T-107) [desirefire_all]
- `http://tracker.anirena.com:80/<redacted>/announce` -- carries a private-tracker credential (T-107) [desirefire_all]
- `http://www.ansktracker.net/announce.php?passkey=<redacted>` -- carries a private-tracker credential (T-107) [desirefire_all]
- `http://www.arabp2p.net:2052/<redacted>/announce` -- carries a private-tracker credential (T-107) [desirefire_all]
- `https://k3tracker.cc/announce/<redacted>` -- carries a private-tracker credential (T-107) [desirefire_all]
- `https://tracker.monikadesign.uk/announce/<redacted>` -- carries a private-tracker credential (T-107) [desirefire_all]
- `https://tracker.monikadesign.uk/announce/<redacted>` -- carries a private-tracker credential (T-107) [desirefire_all]
