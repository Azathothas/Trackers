# TUI command mapping

`kist` exposed its capabilities as a `CommandId` enum in `src/commands.rs`, which drove the
command palette, the help overlay, and the footer hints. `bit-cli` has no interactive interface,
so every entry in that enum is accounted for here before the file was deleted.

Each command maps to exactly one destination:

- **Phase A** - reachable from the `bit-cli` CLI in this phase.
- **Phase C** - needs a running session to mean anything. Recorded in `TODO/phase-c.md`.
- **Removed** - dropped with a reason.

| `CommandId` | TUI label | Key | Destination | Where it lives now |
| --- | --- | --- | --- | --- |
| `Add` | Add a torrent | `a` | Phase A | `bit-cli download <SOURCE>`. The TUI queued a torrent into a live session; the CLI runs one to completion in the foreground. The queue sense of "add" is Phase C. |
| `AddWithOptions` | Add with options | `A` | Phase A | `bit-cli download` flags: `--dir`, `--select-file`, `--exclude-file`, `--index-out`. Starting paused has no meaning without a session, so it is Phase C. |
| `Search` | Search indexers | `f` | Removed | `bit-cli` is transport, not discovery. Decision 7.8. `src/search.rs` and the apibay client are deleted, with no stub and no mention in the docs. |
| `OpenDetails` | Open details | `i` | Phase A | Split across `bit-cli info`, `bit-cli files`, `bit-cli peers`, and `bit-cli trackers`. Each prints what one detail tab showed, then exits. |
| `Pause` | Pause | `p` | Phase C | Requires a session to pause. `TODO/phase-c.md`. |
| `Resume` | Resume | `r` | Phase C | Requires a session to resume. `TODO/phase-c.md`. |
| `Remove` | Remove | `d` | Phase C | Removing from a queue requires a queue. `TODO/phase-c.md`. |
| `AttachWebSeed` | Attach a web seed | `w` | Phase A | The whole `--web-seed*` flag family plus `bit-cli webseed list\|test\|probe\|fetch`. Attaching is now something you do to an invocation, not to a stored record. |
| `Filter` | Filter by name | `/` | Phase A | `bit-cli` takes the sources it is given, so there is no list to filter. The equivalent selection lives at the file level: `--select-file`, `--exclude-file`, and the web seed scope selectors. |
| `ClearFilter` | Clear the filter | - | Phase A | Same as `Filter`: no persistent list, so nothing to clear. Omitting the selection flags is the cleared state. |
| `Limits` | Set rate limits | `L` | Phase A | `--max-download-rate`, `--max-upload-rate`, `--max-overall-download-rate`, `--max-overall-upload-rate`. |
| `ToggleMark` | Mark or unmark | `space` | Phase C | Marking selects rows in a live list. `TODO/phase-c.md`. |
| `ClearMarks` | Clear marks | `esc` | Phase C | As above. |
| `MarkAll` | Mark everything shown | - | Phase C | As above. |
| `SortByName` | Sort by name | - | Phase A | Output ordering. `bit-cli files --sort name`, and torrent ordering follows the order sources were given on the command line. |
| `SortByState` | Sort by state | - | Phase A | `--sort state` where a listing has a state column. |
| `SortByProgress` | Sort by progress | - | Phase A | `--sort progress`. |
| `SortBySpeed` | Sort by speed | - | Phase A | `--sort speed`, used by `bit-cli peers`. |
| `ReverseSort` | Reverse sort direction | `S` | Phase A | `--sort <KEY>:desc`. One flag carries key and direction, matching `bit-cli create --sort-by KEY:ORDER`. |
| `Help` | Show keys | `?` | Phase A | `bit-cli --help`, `bit-cli <CMD> --help`, `bit-cli man`, and `bit-cli completions <SHELL>`. |
| `Quit` | Quit kist | `q` | Removed | Nothing to quit. Every `bit-cli` command runs in the foreground and exits on its own. Interruption is `SIGINT` handling, which exits with code 10. |

## Counts

21 commands. 13 land in Phase A, 6 in Phase C, 2 removed.
