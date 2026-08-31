# Checking interoperability with another client

A torrent this tree wrote that another client will not open, or a payload this
tree seeded that another client cannot take, is the failure worth catching. It
is checked rather than assumed.

The binaries and the fixtures have to exist first, and `--bins --examples` is
the part that is easy to leave out: `--examples` on its own builds the fixtures
and no binary.

```bash
cargo build --workspace --bins --examples
```

```bash
pwsh -NoProfile -File scripts/interop-roundtrip.ps1
```

```bash
pwsh -NoProfile -File scripts/interop-roundtrip.ps1 -Client rqbit
```

## What it drives

Two other clients, both installed on this machine and both real rather than
mocked:

| client | version | what it covers |
| --- | --- | --- |
| `aria2c` | 1.37.0 | v1 torrents, `--private`, and **web seeds**, which is the one that matters here |
| `rqbit` | 9.0.1 | v1 torrents. It has no BEP 19, so the web seed case is skipped for it |

`-Client aria2c` or `-Client rqbit` runs one of them.

The `librqbit` **crate** this tree vendors and the `rqbit` **binary** the
script drives are two different things. Testing against the binary is worth
doing precisely because the crate is vendored: a change made under `vendor/`
that breaks compatibility with the shipped client shows up here.

## What it proves, in both directions

**Ours out.** `bit-cli create` writes a torrent, a loopback tracker is started,
`bit-cli seed` serves the payload, and the other client downloads it. Then the
bytes are hashed and compared, not the report.

**Theirs in.** The other client creates a torrent and seeds it, and `bit-cli
download` takes it. Same comparison.

Three variants each: plain v1, `--private` under BEP 27, and `--web-seed` with
a `url-list` entry pointing at a loopback file server.

Hybrid and v2 are **not** covered, because `bit-cli create --version hybrid`
is not implemented. That is T-081, open, and it is named here rather than left
as a silent gap in the matrix.

## The one that cost a red job

The script hashes the payload the client just wrote. An earlier version hashed
it **immediately after killing the client**, and on Windows the file is still
open at that moment: the hash read a partially flushed file and the comparison
failed for a reason that had nothing to do with either client.

It waits for the process to exit and for the handle to be released now. A
comparison that races the thing it is comparing is a test that asserts a
scheduling outcome, which is the rule in
[`../../TODO/RULES.md`](../../TODO/RULES.md) section 5 that has cost this
repository seven red CI jobs.

## Adding a third client

`transmission` cannot be added on Windows, which is where this runs. That is
recorded under T-084 in
[`../../TODO/create-seed.md`](../../TODO/create-seed.md) rather than left as an
unexplained absence.

The client worth adding next is **libtorrent**, and the reason is specific: it
is the only widely deployed BEP 52 implementation, so it is the only thing that
could validate T-081's v2 and hybrid creation once that is built. Every other
client in the corpus would take a v2 torrent and refuse it for the same reason
this one cannot make it.

## Nothing reaches the network

The tracker and the web seed are two fixtures in this repository, both bound to
`127.0.0.1`. Either can be run on its own:

```bash
cargo run -p bit-cli-core --example loopback-tracker
```

```bash
cargo run -p bit-cli-core --example loopback-fileserver -- --root .
```

Each prints its URL on the first line of stdout and logs every request to
stderr, so a script reads the port rather than guessing it. `loopback-tracker`
also takes `--announce-log <PATH>`, which appends one JSON object per announce
carrying the raw query as received; `scripts/check-announce.ps1` is what reads
it.

`loopback-tracker` speaks BEP 15 as well as BEP 3 and prints a `udp://` URL
after its two HTTP ones. Read that line rather than assuming the ports match:
it asks for the HTTP port and falls back to any free one, because on Windows a
UDP bind inside a reserved range fails even when the same TCP port was free.
Two more flags make it behave like the trackers a client actually meets:
`--redirect-announce <N>` answers the first `N` announces with a `302`, and
`--fail-announce <REASON>` answers every one with a `failure reason` key at
HTTP 200, or BEP 15 action 3 over UDP.

```bash
cargo run -p bit-cli-core --example loopback-tracker -- --fail-announce "not tracked here"
```

A third fixture, `loopback-churn`, generates connection churn against a target
and is what `scripts/soak.ps1` drives.

## Interoperability that is not a client

Two more surfaces are checked and neither involves another BitTorrent client:

**A real tracker's announce shape**, by comparing what `bit-cli` sent against
what a tracker recorded:

```bash
pwsh -NoProfile -File scripts/check-announce.ps1
```

**A real Metalink**, against a live `MirrorBrain` instance:

```bash
pwsh -NoProfile -File scripts/check-metalink-real.ps1
```

That one found the thing worth knowing about Metalink in practice: no
`MirrorBrain` instance reachable in August 2026 emits
`<metaurl mediatype="torrent">`, so the document a user actually gets has
mirrors and checksums and nothing to start a torrent download from. `bit-cli`
names the mirror count and says so rather than failing obscurely.
