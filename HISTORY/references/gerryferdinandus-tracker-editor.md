# GerryFerdinandus/bittorrent-tracker-editor

**Verdict: adopt**

## Provenance

| item | value |
| --- | --- |
| repository | `https://github.com/GerryFerdinandus/bittorrent-tracker-editor` |
| commit read | `c5f5b82e3ba1ef3a1fb8e26efc0a13dad9793acb` |
| tree in this repo | [`references/GerryFerdinandus__bittorrent-tracker-editor/tree`](../../references/GerryFerdinandus__bittorrent-tracker-editor/tree) |
| tracker | [`references/GerryFerdinandus__bittorrent-tracker-editor/issues.json`](../../references/GerryFerdinandus__bittorrent-tracker-editor/issues.json) -- issues **and** pull requests, both states, capped at 100 |
| read on | 2026-08-29 |

```bash
cat references/GerryFerdinandus__bittorrent-tracker-editor/COMMIT
```

**Not obtained:** Discussions (GraphQL only; the credential-free route is REST).
Review comments. Issue comments except where a section below quotes one.
[`references/PROVENANCE.md`](../../references/PROVENANCE.md) is the full gap list.

## Findings

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

---

The overview, the ordering of verdicts, and **what the sweep did not establish**
are in [`../reference-sweep.md`](../reference-sweep.md). Read that first: it
opens with the limitations, and this file assumes them.
