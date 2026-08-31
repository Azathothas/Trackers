# Exit codes

The exit code is the primary success signal. A caller branches on it without
parsing any text.

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Generic failure |
| 2 | Usage or argument error |
| 3 | Configuration error |
| 4 | Source resolution failed |
| 5 | Network failure |
| 6 | No usable sources |
| 7 | Hash verification failed |
| 8 | Disk error |
| 9 | Timeout or deadline exceeded |
| 10 | Interrupted, partial state saved |
| 11 | Coverage gap: some pieces have no source |
| 12 | Binding error: a scope selector or composition mode is invalid |
| 13 | A lint refused a torrent at creation |
| 14 | Threshold not met |
| 15 | Would change the info hash |
| 16 | A resource ceiling was crossed |
| 17 | This run's own listener stopped answering |

Codes 11 through 17 exist so a script can tell "your mirrors are
misconfigured" from "the network is down" from "your server is slow" from "the
process is out of handles" from "the port is open and answers nobody".

## The whole table, and whether a retry could succeed

[`man/bit-cli.json`](../man/bit-cli.json) carries every code with its `kind`,
its description, and a `retryable` boolean. That is the field to read before a
script retries anything.

The same table is in [`man/bit-cli.md`](../man/bit-cli.md) as prose. Both are
generated from the exit code enumeration and a test fails until they are
regenerated, so neither can drift from the binary.

```bash
pwsh -NoProfile -File scripts/check-man.ps1 -Fix
```

## What exits 2, and what exits 4

The two are easy to confuse and the difference is worth stating: **2 is "this
is not a source", 4 is "this source could not be resolved".** A retry fixes
neither, but only one of them is fixed by editing the command line.

Three shapes exit 2 rather than 4, and each one used to come back as a file
error naming something that was not the cause:

| what you typed | what you get |
| --- | --- |
| a directory | `<path> is a directory, not a .torrent` and the name of the command that takes one |
| a subcommand with a typo, as `bit-cli tre album.torrent` | `` `tre` is not a command `` and the nearest command it could be |
| a URL under another scheme, as `ftp://host/x.torrent` | `` `ftp:` is not a scheme this reads `` and the forms that are read |

The second only fires on a bare word with nothing of that name on disk. A
source written as a path is a path: `./tre` and `tre.torrent` are read as
files, present or missing, and a torrent actually named `tre` is downloaded.

```bash
bit-cli info ./album
```

Exit 4 is what is left: a `.torrent` that is not there, a URL that answered
with something that is not a torrent, a magnet whose metadata could not be
resolved.
