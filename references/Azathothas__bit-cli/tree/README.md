# bit-cli

A non-interactive BitTorrent and HTTP download tool that lets you attach
arbitrary web seeds to an existing torrent, from the command line, without
rewriting the `.torrent`.

```bash
bit-cli download ubuntu.torrent \
  --web-seed https://mirror-a.example.com/pub/ \
  --web-seed-for 'file:0=https://cdn.example.com/blobs/a3f1/payload.iso'
```

The torrent is not modified. Its info hash does not change. The sources exist
for the length of that one invocation.

No other command-line client has a named, documented way to say "here is a
torrent, here are N extra HTTP sources, go". `aria2`'s 207 documented options
contain no web seed option of any kind: what it has is an RPC method whose
`uris` array is used for web seeding, and undocumented positional URIs
alongside a `.torrent`. Neither gives per-file control, header control, or a
way to feed a list from a file. [`docs/webseed.md`](docs/webseed.md) is the
addressing model that replaces them.

## Install

From source. This is the only way today: no version has been tagged, so there
are no published binaries yet.

```bash
cargo install --path crates/bit-cli --locked
```

The minimum supported Rust version is **1.88**. That is not a preference: it is
the highest `rust-version` in the resolved dependency graph, and CI pins exactly
that in its `MSRV` job. A test fails when this paragraph, `Cargo.toml` and the
workflow stop agreeing.

```bash
cargo metadata --format-version 1 --all-features
```

`.github/workflows/release.yml` builds `x86_64-linux`, `aarch64-linux` and
`x86_64-windows` on a `v*` tag, each with a BLAKE3 checksum and a build
provenance attestation.

## Commands

Every command runs in the foreground, does its work, and exits. There is no
daemon and no stored session.

| command | what it does | more |
| --- | --- | --- |
| [`download`](man/bit-cli.md) | fetch to completion, then exit | [webseed.md](docs/webseed.md) |
| [`info`](man/bit-cli.md) | parse a torrent, magnet or metalink and print its metadata | [metainfo.md](docs/metainfo.md) |
| [`files`](man/bit-cli.md) | list files with index, path, size and piece span | [metainfo.md](docs/metainfo.md) |
| [`peers`](man/bit-cli.md) | connect, sample the swarm, report peers, exit | [peers.md](docs/peers.md) |
| [`trackers`](man/bit-cli.md) | announce or scrape, report tier, interval, seeders, leechers | [trackers.md](docs/trackers.md) |
| [`webseed`](man/bit-cli.md) | `list`, `test`, `probe`, `fetch` | [webseed.md](docs/webseed.md) |
| [`verify`](man/bit-cli.md) | hash-check existing data, per piece | [integrity.md](docs/integrity.md) |
| [`create`](man/bit-cli.md) | create a `.torrent` | [create-seed.md](docs/create-seed.md) |
| [`edit`](man/bit-cli.md) | rewrite metainfo fields, writing a new file | [metainfo.md](docs/metainfo.md) |
| [`magnet`](man/bit-cli.md) | convert a torrent to a magnet URI | [metainfo.md](docs/metainfo.md) |
| [`seed`](man/bit-cli.md) | seed existing data in the foreground | [create-seed.md](docs/create-seed.md) |
| [`bench`](man/bit-cli.md) | `leech`, `seed`, `webseed`, `disk`, `swarm`, `probe` | [performance.md](docs/performance.md) |
| [`config show`](man/bit-cli.md) | print the resolved configuration with the origin of every value | [configuration.md](docs/configuration.md) |
| [`completions`](man/bit-cli.md) | bash, zsh, fish, powershell, elvish, nushell | [man.md](docs/man.md) |
| [`man`](man/bit-cli.md) | generate a man page, Markdown, or a CLIspec document | [man.md](docs/man.md) |
| [`version`](man/bit-cli.md) | version, build metadata, features, protocol support | [bep-coverage.md](docs/bep-coverage.md) |

`bit-cli <SOURCE>` with no subcommand is `bit-cli download <SOURCE>`.

Sources accepted: a path to a `.torrent`, an HTTP or HTTPS URL to one, a magnet
URI, a bare info hash, a local Metalink (`.meta4` or `.metalink`), and `-` for
stdin.

**The whole surface is committed under [`man/`](man/)**, generated from the
command definition and held current by the test suite:
[`bit-cli.1`](man/bit-cli.1) for a terminal, [`bit-cli.md`](man/bit-cli.md) for
reading, and [`bit-cli.json`](man/bit-cli.json) for a program, as a
[CLIspec](https://github.com/rvben/clispec) document carrying every flag, the
values it accepts, its default, and every exit code with whether a retry could
succeed. If you are scripting this tool, or you are an agent driving it, read
that file rather than guessing a flag. [`docs/man.md`](docs/man.md) says how it
is kept honest.

## Features

State is measured, not aspirational: each row links to the document that
carries the commands behind it.

| capability | state | doc |
| --- | --- | --- |
| per-scope web seeds attached at runtime | done | [webseed.md](docs/webseed.md) |
| one source over several connections | done | [webseed.md](docs/webseed.md) |
| a `file:` URL as a source | done | [webseed.md](docs/webseed.md) |
| per-source headers, auth, agent, timeouts and rate cap | done | [webseed.md](docs/webseed.md) |
| a bad piece attributed to the source that filled it | done | [integrity.md](docs/integrity.md) |
| Metalink `.meta4` and `.metalink` | done | [metainfo.md](docs/metainfo.md) |
| torrent creation, editing without moving the info hash, verification | done | [create-seed.md](docs/create-seed.md), [metainfo.md](docs/metainfo.md) |
| seeding, with ratio and time bounds | done | [create-seed.md](docs/create-seed.md) |
| tracker announce and scrape, with tiers and both address families | done | [trackers.md](docs/trackers.md) |
| swarm sampling and per-peer reporting | done | [peers.md](docs/peers.md) |
| message stream encryption | done | [peers.md](docs/peers.md) |
| uTP transport | reachable, and one combination stalls | [peers.md](docs/peers.md) |
| six benchmark subcommands with baselines and floors | done | [performance.md](docs/performance.md) |
| JSON and newline-delimited JSON output with a versioned schema | done | [schema.md](docs/schema.md), [machine-output.md](docs/examples/machine-output.md) |
| hooks on every documented trigger | done | [hooks.md](docs/hooks.md) |
| Windows path planning, long paths, case collisions | done | [windows.md](docs/windows.md) |
| four file allocation methods, measured | done | [disk.md](docs/disk.md) |
| BEP 52 v2 and hybrid creation | not implemented | [bep-coverage.md](docs/bep-coverage.md) |
| BEP 55 holepunch, WebTorrent, BEP 16 superseeding | not implemented | [bep-coverage.md](docs/bep-coverage.md) |
| a daemon, an RPC surface, or a stored session | **not planned** | [configuration.md](docs/configuration.md) |

## BEP coverage

**Yes** means `bit-cli`'s own code implements it and a test covers it.
**Inherited** means the vendored engine provides it and `bit-cli` has no test of
its own. **Partial** and **read only** each name what is missing in the row
itself. **No** means it is not there, and the entry that would close it is
named.

[`docs/bep-coverage.md`](docs/bep-coverage.md) is the whole table with the
symbol behind every row and the entry behind every gap. The short version:

| | BEPs |
| --- | --- |
| yes | 7, 10, 12, 15, 17, 19, 20, 21, 23, 27, 39, 48, 53 |
| inherited | 3, 5, 9, 11, 14 |
| partial | 6 fast extension, 29 uTP, 54 `lt_donthave` |
| read only | 47 padding files, parsed and skipped but never written |
| no | 16, 33, 44, 51, 52, 55, and WebTorrent |

## Exit codes

Seventeen failures and success, and every one is in
[`man/bit-cli.json`](man/bit-cli.json) with its `kind`, its meaning, and
whether a retry could succeed.
[`docs/exit-codes.md`](docs/exit-codes.md) is the table with the argument for
each, and `bit-cli version` prints it too.

```bash
bit-cli version
```

## Documentation

| page | what it covers |
| --- | --- |
| [webseed.md](docs/webseed.md) | the addressing model: source, scope, composition |
| [integrity.md](docs/integrity.md) | what is guaranteed about the bytes |
| [metainfo.md](docs/metainfo.md) | reading, editing and converting a torrent, and Metalink |
| [create-seed.md](docs/create-seed.md) | making a torrent and seeding it |
| [trackers.md](docs/trackers.md) | announce, scrape, tiers and families |
| [peers.md](docs/peers.md) | the swarm, transports, encryption, outages |
| [dht.md](docs/dht.md) | DHT, PEX and local discovery |
| [disk.md](docs/disk.md) | where the payload lands, and how it is allocated |
| [memory.md](docs/memory.md) | what is bounded, by which flag |
| [performance.md](docs/performance.md) | how every number here was measured |
| [windows.md](docs/windows.md) | paths, handles, and redirecting output |
| [configuration.md](docs/configuration.md) | config files, binding tables, source resolution |
| [exit-codes.md](docs/exit-codes.md) | every code and whether to retry |
| [schema.md](docs/schema.md) | every field of every output document |
| [flags.md](docs/flags.md) | the naming conventions this CLI follows |
| [command-mapping.md](docs/command-mapping.md) | what an `aria2` or `rqbit` invocation maps to |
| [hooks.md](docs/hooks.md) | running something when an event happens |
| [trace.md](docs/trace.md) | tracing one subsystem without raising the log level |
| [man.md](docs/man.md) | the three generated manuals, and how they stay honest |
| [bep-coverage.md](docs/bep-coverage.md) | the protocol table, with the argument |
| [vendoring.md](docs/vendoring.md) | why `librqbit` is vendored and how a change is recorded |

### Worked examples

| page | the task it walks |
| --- | --- |
| [inputs.md](docs/examples/inputs.md) | what a `SOURCE` argument may be, and which commands take which |
| [cloudflare-webseed.md](docs/examples/cloudflare-webseed.md) | serving a payload from R2 or a Worker, and proving the origin honours `Range` |
| [s3-webseed.md](docs/examples/s3-webseed.md) | serving a payload from S3 or an S3-compatible bucket, with the latency and request cost measured |
| [comparing-torrents.md](docs/examples/comparing-torrents.md) | proving two torrents hold the same file when the info hashes differ |
| [multi-source.md](docs/examples/multi-source.md) | a swarm, a CDN, a signed URL and a local copy, at once |
| [mirror-benchmark.md](docs/examples/mirror-benchmark.md) | measuring a mirror before trusting it |
| [create-and-seed.md](docs/examples/create-and-seed.md) | making a torrent and seeding it, end to end |
| [tracker-diagnostics.md](docs/examples/tracker-diagnostics.md) | a tracker that will not answer |
| [machine-output.md](docs/examples/machine-output.md) | consuming the output from a script |
| [interop.md](docs/examples/interop.md) | checking against `aria2c` and `rqbit` |

## Building

```bash
cargo build --release --locked
```

```bash
cargo test --workspace
```

The fixtures the acceptance scripts drive are built by `--examples`, and the
binary by `--bins`. Leaving one out is the commonest way a check exits 2:

```bash
cargo build --release --bins --examples
```

Windows release builds link the C runtime statically, set in
`.cargo/config.toml`, and the musl targets carry no interpreter at all. Verify
either:

```bash
pwsh -NoProfile -File scripts/check-static.ps1
```

```bash
pwsh -NoProfile -File scripts/check-static.ps1 -Path target/release/bit-cli
```

Every gate in one command:

```bash
pwsh -NoProfile -File scripts/gates.ps1
```

## Interoperability

`bit-cli create`, `verify` and `seed` round trip byte for byte through
`aria2c` 1.37.0 and `rqbit` 9.0.1, in both directions, for v1, `--private` and
`--web-seed`.

```bash
pwsh -NoProfile -File scripts/interop-roundtrip.ps1
```

[`docs/examples/interop.md`](docs/examples/interop.md) says what that covers
and what it does not.

## Working on this

[`docs/AGENTS.md`](docs/AGENTS.md) is the orientation: the tree layout, the
tools, the gate contract, and the reading order.
[`TODO/RULES.md`](TODO/RULES.md) is how the repository is worked on and it is
the normative one. [`TODO/INDEX.md`](TODO/INDEX.md) is every entry, one line
each.

Two procedures have their own pages:
[`docs/reference-mining.md`](docs/reference-mining.md) for studying somebody
else's project, and [`docs/task-authoring.md`](docs/task-authoring.md) for
turning an idea into a filed entry.

## Licence and attribution

`bit-cli` is MIT. See [`LICENSE`](LICENSE).

It started as a fork of [`kist`](https://github.com/QaidVoid/kist), which is
dual licensed MIT OR Apache-2.0, and its copyright notice is kept in `LICENSE`.
It builds on [`librqbit`](https://github.com/ikatson/rqbit), which is
Apache-2.0 and is **vendored** under `vendor/` rather than depended on: see
[`docs/vendoring.md`](docs/vendoring.md) and
[`patches/UPSTREAM.md`](patches/UPSTREAM.md), which is the record Apache-2.0
asks for. Torrent creation, linting, and the environment-injection pattern that
makes the whole binary drivable from a test are adapted from
[`intermodal`](https://github.com/casey/intermodal), which is CC0-1.0.

[`THIRD_PARTY.md`](THIRD_PARTY.md) carries the full licence text for every
dependency and is generated from `Cargo.lock`:

```bash
cargo about generate --config about.toml --output-file THIRD_PARTY.md about.hbs
```

`deny.toml` allows permissive licences only. Everything else fails the build
rather than appearing in a generated file, and it is checked both ways:

```bash
cargo deny check
```

```bash
pwsh -NoProfile -File scripts/check-licence-gate.ps1
```

The first says this tree is clean. The second builds a throwaway crate with one
`GPL-3.0-or-later` dependency and requires the same configuration to refuse it,
because a gate that has never rejected anything has not been tested.
