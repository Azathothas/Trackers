# Configuration

`bit-cli` runs with no config file and no state file, and that is a decision
rather than an omission: see [`TODO/RULES.md`](../TODO/RULES.md) section 6,
decision 7.4. A config file sets defaults; it never adds behaviour a flag
cannot reach.

Highest wins:

1. Command-line flags
2. Environment variables, prefixed `BIT_CLI_`
3. `--config <PATH>`
4. `./bit-cli.toml`
5. The user config directory
6. Built-in defaults

```bash
bit-cli config show --json
```

prints every value with where it came from, which is what makes the tool
debuggable in CI. A `BIT_CLI_*` variable matching no setting is an error, not a
silent no-op, because that is how a production setting goes missing. The
variables `bit-cli` sets for a hook are not settings and are not refused;
`docs/hooks.md` lists them.

Every layer above reaches every command, not only `config show`: a setting
becomes the **default** of the flag it names, so a flag on the command line
still wins and nothing else has to decide precedence. `--trace config` prints
the whole resolution on any run.

```bash
bit-cli download x.torrent --trace config
```

Two things follow from a setting being a default. `--config` naming a file that
is not there is an error on every command, not only on `config show`. And the
three `enable_*` settings are the defaults of `--no-dht`, `--no-pex` and
`--no-lsd`, so `enable_dht = false` in a file cannot be turned back on for one
run by a flag, because there is no `--dht`; `--no-config` is how that run
ignores the file.

## Binding tables

Anything expressible on the command line is expressible in a file, in TOML or
JSON, with the same schema.

```toml
[[source]]
url         = "https://mirror-a.example.com/pub/"
scope       = "*"
mode        = "auto"
priority    = 10
concurrency = 8
connections = 2

[[source]]
url   = "https://cdn.example.com/blobs/a3f1b2/payload.bin"
scope = "file:0"
mode  = "exact"

[[source]]
url     = "https://partial.example.com/chunks/{piece}.bin"
scope   = "piece:0-2047"
mode    = "template"
headers = { Authorization = "Bearer ...", X-Region = "apac" }

[[source]]
url        = "https://slow-but-complete.example.com/iso/"
scope      = "*"
mode       = "prefix"
priority   = 1
rate_limit = "5MiB/s"

[[source]]
url          = "https://signed.example.com/blob"
scope        = "file:1"
mode         = "exact"
retry_status = [403, 429]
fatal_status = ["500-599"]
```

```bash
bit-cli download release.torrent --web-seed-config web-seeds.toml
```

## Loading a target

`bench swarm` is the one subcommand that puts load on a machine other than
this one, so it takes a peer address rather than a torrent, and that address is
the only thing it ever contacts. It announces to no tracker, uses no DHT, and
reads no peer list.

```bash
bit-cli bench swarm 10.0.0.5:51413 --for album.torrent --peers 16 --disk-budget 2GiB
```

`--for` names a torrent the target already serves. The synthetic peers
handshake for it, declare interest, request blocks, and check every completed
piece against the torrent's own hashes, so the report measures the target's
serving path and would notice it serving wrong bytes.

```bash
bit-cli bench swarm 10.0.0.5:51413 --peers 100 --torrents 4 --disk-budget 2GiB
```

Without `--for`, four info hashes are generated and the target has none of
them. Nothing can be served, which is the point: what is measured is the accept
and handshake path. How many connections the target takes, how fast it answers
a handshake, and whether its listener survives.

The `.torrent` files for the generated info hashes are written to the scratch
directory, so a run is reproducible and the operator can add one to a target
and come back with `--for`.

Two limits are worth knowing before reading a report. `--disk-budget` bounds
the piece bytes a peer keeps, and a held piece is written at its own offset, so
the file on disk can be larger than the budget. And a synthetic peer keeps what
it verified without serving it, so this is a hundred leeches rather than a
swarm: a target that ranks peers by what they have uploaded sees no difference.
Both are open under [T-092](../TODO/bench.md).

## Where each value came from

`bit-cli config show` prints the fully resolved configuration with the origin
of every value, so a surprising default can be traced to the file, the
environment or the built-in that set it.

```bash
bit-cli config show --json
```

## Choosing which files land, and where

Four flags decide it and they compose, so it is worth knowing which one wins.

| flag | what it decides |
| --- | --- |
| `--dir` | the output directory. Everything else is relative to it |
| `--out` | a different name or path for the whole payload. It is the caller's own path and it may leave the output directory |
| `--select-file` | which files to write, by index. Accepts ranges: `1-5,8,10-` |
| `--exclude-file` | which files not to write, by index |
| `--index-out` | rename one file by index, as `INDEX=PATH` |

`--select-file` and `--exclude-file` are resolved together: with both, the
selection is taken first and the exclusion removes from it. With only
`--exclude-file`, everything not excluded is selected, which is the reading a
caller expects and was a defect until it was fixed.

`--out` may leave the output directory and that is deliberate: it is a path the
caller typed on their own command line, and `--dir` is unconstrained already. A
path inside a **torrent** is a different thing entirely and is always planned,
sanitised and confined. See [`disk.md`](disk.md).

`--index-out` is a request rather than a command: the path is sanitised and
disambiguated the same way a torrent path is, so it cannot escape the output
directory. The same flag is on `download`, `verify` and `seed`, because those
are one payload read three ways: `download` writes it there, `verify` reads it
back from there, and `seed` serves it from there. A renamed payload that could
be downloaded and then not seeded was a real defect.

## Logging

stderr always carries the log and `--log-file` adds a second destination rather
than taking stderr away.

| flag | what it decides |
| --- | --- |
| `--log-level` | how much |
| `--log-format` | the shape |
| `--log-file` | a second destination, appended to |
| `--log-max-size` | rotate at this size. `0` never rotates |
| `--log-max-files` | keep this many in total, the live one included |

Rotation is by size and count, so a run that logs for days does not fill a
disk. `--log-max-files` counts the live file, so `2` means one rotated file
beside it.

`--trace <SUBSYSTEM>` is the other lever and it is not the same thing: it turns
on detailed tracing for one subsystem without raising the level for everything
else. [`trace.md`](trace.md) lists the subsystems and what each one costs.

`--no-redact` shows credentials in trace output instead of redacting them. It
exists because a signed URL that fails cannot be debugged without it, and it is
off by default because a trace usually ends up in a bug report.
