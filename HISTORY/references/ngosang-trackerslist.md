# ngosang/trackerslist

**Verdict: confirms, with a caveat**

## Provenance

| item | value |
| --- | --- |
| repository | `https://github.com/ngosang/trackerslist` |
| commit read | `1e61597e0160027add8bbc36e7161796454d6f3a` |
| tree in this repo | [`references/ngosang__trackerslist/tree`](../../references/ngosang__trackerslist/tree) |
| tracker | [`references/ngosang__trackerslist/issues.json`](../../references/ngosang__trackerslist/issues.json) -- issues **and** pull requests, both states, capped at 100 |
| read on | 2026-08-29 |

```bash
cat references/ngosang__trackerslist/COMMIT
```

**Not obtained:** Discussions (GraphQL only; the credential-free route is REST).
Review comments. Issue comments except where a section below quotes one.
[`references/PROVENANCE.md`](../../references/PROVENANCE.md) is the full gap list.

## Findings

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

---

The overview, the ordering of verdicts, and **what the sweep did not establish**
are in [`../reference-sweep.md`](../reference-sweep.md). Read that first: it
opens with the limitations, and this file assumes them.
