# `--trace`

Detailed tracing for one subsystem, without raising the level on everything
else. Turning `trace` on globally in a torrent client buries the thing you are
looking for under peer chatter, so `--trace disk` raises the disk and leaves
the rest alone.

```bash
bit-cli download x.torrent --trace disk
```

Repeatable and comma separated, so `--trace disk,retry` and
`--trace disk --trace retry` are the same run. Records go to stderr, at every
level and whatever else is set: stdout carries data only. `--log-format json`
puts the target in a field rather than in a line to be parsed, and
`--log-file` adds a second destination without taking stderr away.

## What each name shows, and what puts it in the path

A subsystem shows nothing when nothing it covers happens. `--trace tracker` on
a run with `--no-tracker` is silent because there were no announces, and that
is the flag working. The third column is a command that does put it in the
path, so "silent" can be told apart from "broken".

| Name | What it shows | A run that exercises it |
| --- | --- | --- |
| `peer` | Wire messages: type, index, begin, length, direction, peer id | any `download` or `seed` with a peer or a web seed |
| `handshake` | Peer handshakes and extension negotiation | the same |
| `tracker` | Announce and scrape requests and responses in full | `bit-cli trackers`, or a `download` without `--no-tracker` |
| `dht` | DHT queries, responses, and routing table changes | a `download` without `--no-dht` or `--web-seed-only` |
| `http` | Web seed requests and responses, status, headers, ranges, redirects, TLS | a `download` with `--web-seed` |
| `piece` | Piece request, receipt, verification result, and timing | any `download` |
| `picker` | Why a piece was requested from a given source | any `download`; `--piece-selector in-order` adds this tool's own picker |
| `disk` | Reads, writes, flushes, and allocation, with offsets and sizes | any `download`, `verify`, or `seed` |
| `ratelimit` | Token bucket decisions and stalls | a `download` with `--web-seed-speed-limit` |
| `retry` | Retry attempts, backoff, and cooldown | a `download` against a source that fails transiently |
| `config` | Resolution of every configuration value and its origin | any command: the configuration is resolved once per run |

The list is `SUBSYSTEMS` in `crates/bit-cli/src/logging.rs` and it is also in
`bit-cli version --json`, under `trace_subsystems`, so a program can read it
rather than parse `--help`.

## What a name raises

Each name raises one or more `tracing` targets. `bit_cli::<name>` is where
this repository's own code writes. The `librqbit*` targets are where the
vendored session writes the same kind of fact, and they are this repository's
code too: the trees under `vendor/` are ours, and three of these targets exist
because this repository put them there.

| Name | Targets |
| --- | --- |
| `peer` | `bit_cli::peer`, `librqbit::peer_connection` |
| `handshake` | `bit_cli::handshake`, `librqbit::handshake` |
| `tracker` | `bit_cli::tracker`, `librqbit_tracker_comms` |
| `dht` | `bit_cli::dht`, `librqbit_dht` |
| `http` | `bit_cli::http` |
| `piece` | `bit_cli::piece`, `librqbit::piece` |
| `picker` | `bit_cli::picker`, `librqbit::picker` |
| `disk` | `bit_cli::disk` |
| `ratelimit` | `bit_cli::ratelimit` |
| `retry` | `bit_cli::retry` |
| `config` | `bit_cli::config` |

`librqbit::handshake`, `librqbit::piece` and `librqbit::picker` are not module
paths. They are explicit targets on thirteen trace calls in
`vendor/rqbit/crates/librqbit`, added so that a name means one thing:
`peer_connection` carries the handshake and the wire messages in the same
module, and `torrent_state::live` carries the picker, the piece lifecycle and
peer management together, so raising the module would have made `--trace
handshake` print every message on every connection. The record is
`patches/UPSTREAM.md`, under "handshake, piece and picker tracing have no
target of their own".

Both halves matter and neither is enough on its own. `bit_cli::dht` says
whether there is a DHT at all, which the vendored crate cannot say because
with the DHT off it writes nothing. `librqbit::peer_connection` carries the
messages of a swarm peer, which this repository's code never sees.

`config` is the one subsystem whose records are written from a single place
rather than from wherever the fact is decided, and the ordering is why: the
configuration decides `--log-level`, so it has to be resolved before there is
a subscriber to write to. `Resolved::trace` runs immediately after the
subscriber is installed, once per run, and prints every file considered and
every setting with the layer it came from.

## Where a target is not a module path

`EnvFilter` matches a directive against the **prefix** of a record's target,
so `librqbit_dht=trace` admits `librqbit_dht::dht`, and a target is any
string rather than a module that has to exist. That is convenient and it is
also the failure this whole surface had: a directive naming a target nothing
writes to is accepted, matches nothing, and produces no error.

That is why every name in the table above is a target something writes to,
and why the mapping is a table rather than a rule. Deriving `bit_cli::<name>`
from the flag value is the version that silently matches nothing: a record from
`cmd/peers.rs` has the target `bit_cli::cmd::peers`, which `bit_cli::peer` does
not match, and there is no `bit_cli::disk` module at all. Measured on one
`download` tracing
all ten: **0** lines of stderr, against 32 for `http` on the same run.
`TODO/cli-surface.md` T-219 is the entry.

## Adding one

Three things, and a test fails until all three are done.

1. A row in `SUBSYSTEMS`, with the targets it raises.
2. A `tracing::trace!(target: "bit_cli::<name>", ...)` at the place that knows
   the facts the description promises, or an explicit target in `vendor/` with
   a section in `patches/UPSTREAM.md`.
3. A case in `crates/bit-cli/tests/trace_subsystems.rs`, which drives the real
   binary once per subsystem and asserts a record on a target that subsystem
   raises. `every_documented_subsystem_has_a_case` fails when a name is added
   without one.

Then regenerate the manuals:

```bash
pwsh -NoProfile -File scripts/check-man.ps1 -Fix
```

## What it costs

Measured, on `download --web-seed-only`, in three configurations up to 16,384
trace lines: in every one the difference between a traced run and a plain one
is smaller than the plain run's own run-to-run spread, and in one the traced
run used less memory. `TODO/bench.md` T-094 has the numbers and
`scripts/check-trace-cost.ps1` takes them again.

```bash
pwsh -NoProfile -File scripts/check-trace-cost.ps1
```

A subsystem that is off costs a level check at the callsite and nothing else:
`tracing` does not evaluate a record's fields unless something is listening,
which is why the record on the payload write path carries the offset and the
size without paying for them on a run that is not watching.
