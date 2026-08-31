# Changelog

Notable changes, newest first. Versions follow [semantic
versioning](https://semver.org/spec/v2.0.0.html), and the released version is
driven from the git tag.

## 0.2.0, unreleased

Since `1b0117e3fe77`.

### The client that fetches a source document is a browser, off the wire

`bit-cli` fetches a `.torrent`, a Metalink or a web page presenting as Chrome
151, and that now goes all the way down. Measured against a loopback
fingerprint oracle, before and after:

| | JA4 | Akamai HTTP/2 |
| --- | --- | --- |
| before | `t13i1010h2_61a7ad8aa9b6_3fcd1a44f3e3` | not reachable |
| after | `t13i1515h2_8daaf6152771_806a8c22fdea` | `1:65536;2:0;4:6291456;6:262144\|15663105\|0\|m,a,s,p` |
| Chrome 151 | `t13i1515h2_8daaf6152771_806a8c22fdea` | `1:65536;2:0;4:6291456;6:262144\|15663105\|0\|m,a,s,p` |

Ten ciphers and ten extensions became fifteen and fifteen, with GREASE, ECH,
ALPS, certificate compression and the ML-DSA signature algorithms. The header
set is sent in Chrome's order, `user-agent` and `accept-encoding` in Chrome's
positions rather than appended last. `--page-client plain` still sends
`bit-cli/<version>` and nothing else.

**A web seed is unaffected and still identifies itself honestly.** So is a
tracker announce, a peer handshake and a tracker or web seed list fetched by
URL. The impersonation is confined to the one document a caller named.

It costs five more vendored upstreams, `rustls`, `h2`, `impit`, `reqwest` and
`hyper-util`, 26 more packages in the graph and 1.25 MiB of binary.
`docs/vendoring.md` says why each is there and `patches/UPSTREAM.md` says what
changed in it.

### `BIT_CLI_EXTRA_CA_FILE` adds a certificate authority

A PEM bundle named by that variable is trusted **in addition to** the platform
and bundled roots when a source document is fetched. Nothing is replaced and
verification is not weakened: a certificate still has to chain to some root. It
is used by `scripts/check-fingerprint.ps1`, which has to complete a real
handshake against a certificate it minted itself in order to read this client's
HTTP/2 fingerprint at all.

It logs a warning naming the file and the number of roots whenever it is read.

### `bit-cli` does not speak HTTP/3, and did not before either

The impersonating client's upstream carries an HTTP/3 path behind an unstable
`reqwest` feature. It is removed rather than carried: enabling it needs
`--cfg reqwest_unstable` in six places across `.cargo/config.toml` and the CI
workflow, where a workflow's `RUSTFLAGS` replaces the config rather than adding
to it, and nothing in this tree can read an HTTP/3 fingerprint to check that it
is right.

### A UDP announce carries the same three events an HTTP one does

`bit-cli` sends `started` once, `completed` once when a download finishes, and
`stopped` when the run ends. Over a `udp://` tracker the announces in between
carried an event too: a torrent still downloading repeated `started` at every
interval, and one that had finished repeated `completed`. A seeder, which has
the whole file before it starts, sent `completed` on every announce, and BEP 3
says a client in that position does not send it at all.

Measured over one 22 second run with the same payload on both protocols: five
`started` events over UDP against one over HTTP.

The cost was the tracker's rather than this client's, and it was invisible from
here. `completed` is how a tracker counts finished downloads, which is the
`downloaded` field of a BEP 48 scrape, so one seeder announcing every five
minutes added 288 a day to a number that should never have moved.

Nothing about an HTTP announce changed, and the events a run sends are
unchanged: what stopped is the ones it sent in between.

### `bit-cli trackers` reads a `.torrent` named by a URL

It was the one command that offered "an HTTP(S) URL" in its `SOURCE` help and
refused it, saying "an info hash is needed to announce, and this source does not
carry one" when the document behind the URL carries one. A URL and the same
file on disk now produce the same report, including `left`, which is the
torrent's total length and so can only be right if the fetch happened. A
metalink resolves the same way, and `--scrape` takes both.

A magnet and a bare info hash are unchanged: they carry the hash an announce
needs and no length, so `left` is a placeholder and `known` is false.

### `loopback-tracker` speaks BEP 15, and can redirect or refuse

The test fixture under `crates/bit-cli-core/examples/` answers UDP announces
and scrapes as well as HTTP ones, refusing a connection id it never issued.
`--redirect-announce <N>` answers the first `N` announces with a `302`, and
`--fail-announce <REASON>` answers every one with a `failure reason` key.
`scripts/check-announce.ps1` has nine judged cases where it had six.

### One peer id, and it is `bit-cli`'s own

`bit-cli` announces and hand shakes as **`-CL0200-`** now: `-CL` is this
client's BEP 20 Azureus-style code, `0200` is its own version, and twelve
printable characters follow.

It was six identities before, and five of them claimed `-BC`, which
libtorrent's client table maps to BitComet. `download` and `seed` announced as
`-rQ9010-`, the vendored engine's, so the version a tracker was told about
`bit-cli` moved whenever the vendored tree was bumped. `trackers` and
`bench probe` announced as `-BC0100-`. The web seed bridge, the swarm bench's
synthetic peer and the listener health check each used a `-BC` variant on
loopback. `bench probe`'s own client table also read `-BC` as `bit-cli`, so
probing a real BitComet peer reported this client's name.

`CL` was checked against six registries before it was used and appears in none
of them, in either case. The version is built from `CARGO_PKG_VERSION_*` at
compile time, so it follows this crate and not the vendored one, and two
compile-time assertions stop it from being silently wrong: a version component
past 61 has no single-character encoding, and a prerelease cannot ship while
the build slot is still `0`.

[`docs/peers.md`](docs/peers.md) is what the identity is. The suffix is
printable now, so an announce log reads without percent-escaping.

### A `.torrent` URL and a metalink resolve under every command, not just `download`

Nine commands offered "an HTTP(S) URL" and "a metalink" in their `SOURCE` help
and refused both. `bit-cli info https://host/album.torrent` exited 4 saying the
URL "has to be fetched before it can be read", while `bit-cli download` fetched
the same URL and completed. `info`, `files`, `magnet`, `verify`, all four
`webseed` subcommands and `bench webseed` are the nine.

They resolve it now, and report exactly what the same torrent read off disk
reports: every `--json` field matches but `generated_at` and `source_kind`.
A magnet and a bare info hash are still refused, because those need a swarm
lookup rather than one `GET`, and that refusal already said so.

Three bounds apply to the fetch:

- `--timeout` is the deadline, and 30 seconds when it is not set.
- A fetch that runs out of time exits **9** and names the deadline in
  milliseconds. It exited 5 saying "error decoding response body" before, which
  described the transport rather than the flag the caller set.
- A `.torrent` body is capped at 16 MiB and a metalink at 1 MiB, counted as the
  bytes arrive. The metalink cap was applied after the whole body was already in
  memory, so it bounded what was returned rather than what was held.

A URL that answers with something else fails naming the content type the server
declared, rather than only the byte the bencode parser stopped on.

[`docs/examples/inputs.md`](docs/examples/inputs.md) carries the matrix, which
was measured rather than reasoned about.

### `README.md` is a map, and the detail moved into `docs/`

`README.md` was 83 KB across 37 sections and is 12 KB across nine: what
`bit-cli` is, install, the command surface as a table, features and BEP
coverage as tables with a document per row, exit codes, building,
interoperability and licence. Every row links to the page that carries the
commands behind it.

Nothing was dropped. The 353 line addressing model is `docs/webseed.md`, the
path planning is `docs/windows.md`, the four stage cost attribution is
`docs/examples/mirror-benchmark.md`, and the rest is in the twelve topic pages
`README.md` now indexes. Seven worked examples are new, including
`docs/examples/cloudflare-webseed.md`, which is the origin story written out:
serving a payload from R2 or a Worker and proving the origin honours `Range`.

**Two rows of the BEP table were wrong and are corrected.** BEP 29 said "no.
No flag enables it", and `--transport tcp|utp|both` has enabled it since 0.2.0;
it is `partial` now, because what is left is a latency figure loopback cannot
produce. BEP 33, 44 and 51 were named in the prose under it and had no row at
all.

`scripts/check-docs.ps1` is new and runs in the gates and in CI. It resolves
every relative link and anchor in `README.md` and `docs/`, every `scripts/`
path, and every flag and command an example names against `man/bit-cli.json`,
so a renamed flag cannot leave a runnable-looking example behind.

### Hooks on `seed`, and three commands stop accepting flags they ignored

`bit-cli seed` runs `--on-complete` once, when the payload has passed its hash
check and the listener is up, and `--on-error` when the run fails. A seeder has
no completion of its own, so the trigger is the moment it starts being useful.
`docs/hooks.md` has the table for both commands.

**`peers`, `bench leech` and `bench seed` no longer accept `--on-complete`,
`--on-error` or `--on-piece-verified`.** They accepted all three and ran none:
the flags lived in the argument struct five commands share and one honoured.
Passing one of them to those three commands is exit 2 now. See
`TODO/cli-surface.md`, T-214.

### The integrity guarantee is written down, and `--verify-on-complete`

`docs/integrity.md` states what a finished download guarantees and what stands
behind it: four independent checks, what each catches, what each costs, and a
closing section on what none of them tells you. `README.md` carries the summary.

`--verify-on-complete` re-reads the finished payload and reports a sha256 per
file under `torrents[].verified_files`. It is redundant with the piece checks by
construction: it is the check a caller can run without trusting the thing that
wrote the bytes, and the only one whose output can be compared against a digest
published somewhere else. It never changes the exit code. See
`TODO/multi-source.md`, T-136.

### Hooks fire once per torrent, and `--on-piece-verified` fires at all

`--on-complete` and `--on-error` ran once for the whole run, with the first
torrent's identity and the run's totals, which describes neither. They fire once
per torrent now, and a run where one torrent finished and another did not fires
both. `--on-piece-verified` reached no code at all and now fires once per
verified piece, on its own thread with a bounded queue: what does not fit is
counted into `hooks.skipped` and warned about rather than dropped or waited for.

`BIT_CLI_TOTAL_BYTES` and `BIT_CLI_DOWNLOADED_BYTES` are **this torrent's** now
rather than the run's, and the four run-level figures have their own names.
`docs/hooks.md` lists every variable.

On Windows, a hook whose command contained a quoted path, a redirect or an `&&`
reached `cmd.exe` mangled and failed: Rust quotes an argument for the C
runtime's parser and `cmd` uses rules of its own. See `TODO/cli-surface.md`,
T-115.

### `-O`/`--index-out` renames a file

It parsed and reached no code. The requested path goes through the same plan a
torrent path does, so it is sanitised, truncated and disambiguated: `-O` renames
a file and cannot be used to leave the output directory. `--json` reports the
mapping with reason `requested`. `verify` takes the same flag, so a payload
renamed on the way down can still be checked. See `TODO/cli-surface.md`, T-116.

### `download --dry-run` writes its own document kind

`kind: "download_dry_run"` rather than `kind: "download"`. A dry run and a real
run shared a kind and almost no fields: a consumer selecting by `kind`, which is
the documented way to select, got two shapes under one name, and `docs/schema.md`
could not sample the dry run at all without making the `download` table a union
of both. `dry_run: true` is still on the document. Its field table is in
`docs/schema.md` now. See `TODO/cli-surface.md`, T-156.

### `bench` subcommand flags are filed under their own help headings

`--peers`, `--torrents`, `--dir` and `--connect-timeout` appeared under **Report
options** in `bench swarm --help`, and `bench leech`, `bench seed` and `bench
disk` did the same with their own flags. Each subcommand has its own heading now.
`bit-cli --help` also had no **Arguments** section at all: `[SOURCE]...` was
documented at the bottom of the global flags. See `TODO/cli-surface.md`, T-159.

### The command surface is generated and committed

`man/bit-cli.1`, `man/bit-cli.md` and `man/bit-cli.json` hold every command,
flag and exit code, generated from the command definition. The JSON is a
CLIspec 0.3 document, for a caller that is a program. `cargo test -p bit-cli`
fails when any of the three stops describing the binary. See `docs/man.md`.

### Message stream encryption

`--encryption off|prefer|require`, defaulting to `prefer`. A peer configured to
require encryption would not exchange a byte with `bit-cli` before this, so the
swarm it could reach was smaller than the swarm that exists. One listening port
serves both kinds: an accepting end tells them apart by reading the first
twenty bytes. `--json` reports what each peer settled on as
`peers[].encryption`.

### BEP 6, the fast extension, and BEP 54

Both directions of both. A peer that says `have all` in two bytes instead of
sending a bitfield is understood, one that says `reject request` no longer
stalls a request until it times out, and a source that loses a file retracts
the pieces it covered on the connection it is already on rather than by
reconnecting with a smaller bitfield.

### Fixed in the vendored trees

- A torrent past 131,960 pieces could not be served or fetched at all: its
  bitfield did not fit the fixed per-connection message buffer.
- One handshake check that failed stopped the accept loop draining, so a peer
  that closed without handshaking cost the next real peer its handshake, and a
  seeder went on reporting itself as seeding while serving nobody.
- Nothing reclaimed a peer row, so a long-lived session grew one per completed
  handshake forever. Bounded at 1,024 per torrent.
- No BEP 6 at all: five message ids and a reserved bit, so a peer that spoke
  the fast extension was answered with an unsupported-message error and
  dropped.
- BEP 54 `lt_donthave` was received and ignored, and honouring one now also
  gives the retracted piece back to the queue rather than leaving it assigned
  to the peer that just refused it.
- A seam for wrapping a peer connection before the BitTorrent handshake, which
  is where the encryption above plugs in.

`patches/UPSTREAM.md` carries each with its measurement.

### Dependencies

`sha1`, `sha2` and `md-5` to 0.11, `clap_mangen` to 0.3, `nix` to 0.31 in the
vendored workspace, and the minor and patch versions of everything else.
`THIRD_PARTY.md` is regenerated with them: it is generated from `Cargo.lock`,
so a bump that does not regenerate it fails the `Third party notices` job and
nothing else.

### Vendored upstreams

The binary is built from these trees, not from crates.io. See
`docs/vendoring.md`.

- `rqbit` at `v9.0.1`, commit `a499d2f243d1`, from https://github.com/ikatson/rqbit
- `librqbit-utp` at `c26f57b2debbe35ed0ace1ad419de529f7a5bf95`, commit `c26f57b2debb`, from https://github.com/ikatson/librqbit-utp
- `librqbit-dualstack-sockets` at `e2f221ca745c25c7790abb593ed260ce5a499fa1`, commit `e2f221ca745c`, from https://github.com/ikatson/librqbit-dualstack-sockets
- `rustls` at `23b2c17427c095b768e22ccf0dadb97266860cf1`, commit `23b2c17427c0`, from https://github.com/apify/rustls
- `h2` at `v0.4.19`, commit `d57d1b852fec`, from https://github.com/hyperium/h2
- `impit` at `4fd6c31`, commit `4fd6c3167c55`, from https://github.com/apify/impit
- `reqwest` at `v0.13.4`, commit `11489b34eda6`, from https://github.com/seanmonstar/reqwest
- `hyper-util` at `8ae9e8b1e338a5b7d35155a0dc3708cfd94bcae2`, commit `8ae9e8b1e338`, from https://github.com/hyperium/hyper-util

## 0.1.0, unreleased

First release. `bit-cli` started as a fork of
[`kist`](https://github.com/QaidVoid/kist) and shares no released version with
it, so the history starts here.

### The reason the project exists

- Web seeds attach to an existing torrent at runtime. The `.torrent` is never
  rewritten, never re-hashed, and the info hash does not change.
- A web seed binding is a `(source, scope, composition)` triple, and the three
  are orthogonal. A mirror holding part of a payload is a first-class case
  rather than an error.
- Four composition modes: `auto` (BEP 19), `exact`, `prefix`, and `template`
  with eleven placeholders.
- Scope selectors by file index, index range, path, glob, negated glob, piece
  range, byte range, and byte range within a file.
- Binding tables in TOML or JSON with the same schema, for the cases a command
  line cannot express cleanly.
- Coverage is computed before any request goes out. A gap names the uncovered
  piece indices and `--web-seed-require` turns it into exit 11.
- BEP 19 (GetRight) and BEP 17 (Hoffman) wire styles.
- HTTP sources are presented to the session as peers over loopback. The
  announced bitfield carries only the pieces the source's scope covers in
  full, and a partial source advertises BEP 21 `upload_only`.
- Fetched pieces are hash-checked at the source, so a mirror serving wrong data
  is named rather than showing up as "a peer sent something wrong".
- `--web-seed-connections <N>` presents one source over N connections, which is
  N peers to the session and so N receive paths. They share one HTTP client,
  one window cache, and one concurrency budget divided between them, so the
  mirror sees the same number of requests. On loopback two connections reach
  1.92 times one, measured in `TODO/webseed.md` under T-009.
- `--prefer-web-seed` doubles each source's connections rather than its request
  budget. On a loopback swarm of one mirror and one peer it moves the HTTP
  share of a 1 GiB payload from 46.72% to 62.60% across five paired runs. It
  moves the odds and not the decision: `librqbit`'s piece picker is not
  reachable from outside the crate. See `TODO/webseed.md` under T-003.
- `--web-seed-speed-limit` and a binding table's `rate_limit` are enforced. They
  parsed and were never applied, so a source told to stay under 24 MiB/s ran at
  116. A token bucket per source now paces requests before they go out.
- `--max-download-rate` and `--max-upload-rate` are measured and hold. A 4MiB/s
  cap sustains 4.10 MiB/s against 223.39 MiB/s uncapped, and the seeder side
  caps a downloader that asked for no cap at 4.01 MiB/s.
  `pwsh scripts/check-rate-limit.ps1` is the measurement. See
  `TODO/performance.md` under T-031.
- A download recovers from every peer going away and coming back, and how long
  it takes is now written down. A dropped peer is retried at about 10 seconds,
  then 70, then 430, so an outage ending between two attempts waits for the
  next one. `--stop-timeout` set shorter than that turns a recoverable outage
  into exit 9. `pwsh scripts/check-peer-recovery.ps1` drives both. See
  `TODO/peers.md` under T-021.
- A seeder no longer goes deaf under a burst of connections that close before
  they handshake. `librqbit`'s accept loop is a `tokio::select!` whose two
  branches can both be disabled at once, and when they are it panics, killing
  the listener while the process carries on reporting itself as seeding.
  Measured, 3000 such connections did it in 79 seconds and 2411 of them then
  failed to connect at all. `bit-cli` removes the branch that carries it, and
  the same flood finishes in 8.8 seconds with the listener alive. See
  `TODO/peers.md` under T-020.
- `--max-handles <N>` stops a run that holds more than that many handles, with
  `"stopped": "handle_ceiling"` and exit 16. Off by default. It is a backstop
  for the socket that a connection closing before its handshake strands about
  half the time, which is upstream and open: `pwsh scripts/check-close-wait.ps1`
  measures it, and `--max-handles` turns an unbounded stranding inside a
  `seed --seed-time 7d` into a loud exit a supervisor restarts.
- Exit code 16, `resource_ceiling`, for a resource ceiling the caller set.
- A source URL may be `file:`, so bytes already on the disk under another name
  are a source with a scope, a composition, a chunk size, a rate limit, and the
  same per-piece verification. `webseed list`, `webseed test`, `webseed probe`,
  and `bench webseed` all take one. It is never offered to a swarm: `file:` is
  in neither BEP 17 nor BEP 19 and exists so the same 64 MiB is not fetched
  three times. `pwsh scripts/check-local-source.ps1` drives six cases with no
  server and no bound port, including one payload landing under three info
  hashes and three piece lengths with one distinct hash between them. A `..` in
  a resolved path is refused, because `auto` and `prefix` composition append
  the torrent's own name and path and a hostile `.torrent` would otherwise
  choose the tail of it. See `TODO/multi-source.md` under T-133.
- `--web-seed-retry-status` and `--web-seed-fatal-status` decide which HTTP
  statuses retire a source, per source, as codes and inclusive ranges. A CDN
  that signs its URLs answers 403 when a signature expires and the next request
  to the stable URL succeeds, so `--web-seed-retry-status 403` is what makes
  that survivable: in the recorded run, 22 signatures expired over 64 MiB and
  the payload completed byte for byte, where the same run without the flag
  downloaded nothing. The binding table takes `retry_status` and `fatal_status`
  per source and in `[default]`. A code in both lists is a usage error. See
  `TODO/multi-source.md` under T-130 and `scripts/check-signed-source.ps1`.
- A source is no longer retired by one request that ran out of retries. A
  transient failure reconnects the bridge instead, bounded by
  `--web-seed-max-errors` consecutive failed requests. Before this, a mirror
  that answered 503 for four seconds and then recovered was lost for the rest
  of the run with no flag set, and `--web-seed-max-errors` could never be
  reached.
- **A permanent failure on one file narrows a source rather than retiring it.**
  A mirror that answers 404 for one file of twelve gives up the pieces that
  file touches and goes on serving the other eleven. It used to lose the whole
  source, including the files it was serving correctly a moment earlier, which
  contradicted the scope model this project exists for. A source with no pieces
  left is still retired, and the reason says it ran out rather than naming one
  file. `--json` carries `gone_files` and `pieces_dropped` per source, both
  omitted when nothing was lost. A permanent failure no longer spends
  `--web-seed-max-errors` either: that budget counts transient failures that
  exhausted their retries, and charging a permanent one as well put a narrowed
  mirror into cooldown through the back door. See `TODO/webseed.md` under
  T-005.
- `download` reports `retries` and `retries_by_status` per source, in the text
  output and in `--json`.
- `--peer <ADDR>` dials a peer whether or not a tracker or the DHT answers, and
  `download` takes `--no-dht` and `--no-lsd` as `seed` already did. Together
  they make a swarm of exactly the members named on the command line, which is
  what a measurement needs and what a private network wants.

### Commands

- `download`, `seed`, `peers`, `trackers`, `verify`, `info`, `files`, `magnet`,
  `create`, `edit`, `config show`, `completions`, `man`, `version`.
- `webseed list` resolves every binding and prints the exact URL each file maps
  to, without touching the network.
- `webseed test` probes each source for range support, entity length against
  the torrent, the redirect chain hop by hop, and the negotiated TLS version
  and cipher.
- `webseed probe` measures ranged-GET latency percentiles and throughput as
  concurrency rises.
- `webseed fetch` pulls one named range from one named source and verifies it.
- `files --against <TORRENT>` decides from the metadata alone whether two
  torrents hold the same file, and says what the answer rests on:
  `piece-hashes` when the pieces line up and their hashes agree, which proves
  the bytes equal, and `length` when only the size matches, which proves
  nothing. Two files can be compared by hash only where the pieces cover the
  same bytes of each, so it needs the same piece length and the same offset
  modulo it.
- `trackers` announces and scrapes over HTTP and UDP directly, reporting each
  tracker's tier, interval, seeder and leecher counts, and failure reason. It
  binds the port it announces, for as long as the announce lasts, and then
  withdraws the record with a second announce carrying `event=stopped`. It sent
  a hardcoded 6881 before, which registers a peer nobody can dial and leaves it
  for the tracker's whole interval. `--port` takes a port or a range and
  `--no-withdraw` leaves the record in place.
- A whole `download` run tells its trackers what happened: `completed` the
  moment the torrent finishes and `stopped` when the run ends, both from the
  session's own peer id and listening port so a tracker updates one record
  rather than seeing a second peer. The session sends `started` and then
  repeats on the interval; it says nothing about either of the other two, which
  leaves a seeder count wrong and a dead address handed out until the record
  expires. `--json` reports them under `announced`. See `TODO/trackers.md`
  under T-062.
- `peers` joins the swarm and reports every peer it saw with the bytes that
  came from each. It used to add its torrent paused, which in `librqbit` 9.0.0
  means the torrent never gets its peer stream, so the command never announced
  and every run reported an empty swarm. It now takes `--peer`, `--no-dht`,
  `--no-lsd`, and the tracker and limit flags `download` carries, so a sample
  can be exactly the members named on the command line. What it pulls goes to a
  temporary directory the process removes on exit. See `TODO/peers.md` under
  T-142.
- `bench webseed` measures HTTP sources: latency percentiles for connection
  establishment, first byte, and completion; a concurrency curve; per-source
  attribution; and error counts by class and by HTTP status.
- `bench leech` measures a download and splits its cost three ways: the block
  request pipeline, piece verification, and the disk. All three are measured
  rather than modelled, and all three appear per interval as well as in the
  summary.
- `bench probe` answers what comes before "how fast": one exchange with one
  target, no payload, and the same environment every other bench report
  carries. A `HOST:PORT` gets a BitTorrent handshake and a short listen, so the
  report names the peer id, the client, the reserved bytes and what they claim,
  the extended handshake, and the messages it volunteered. An `http(s)` URL
  gets one ranged GET for a single byte, with the redirect chain hop by hop and
  the TLS version and cipher. An unreachable target exits 6.
- `bench disk` measures the disk on its own: a payload written through the same
  storage a download writes through, from N threads, with no session and no
  network. `--layout shared|handles|split` decides whether the threads share
  one file behind one handle, share one file behind a handle each, or take a
  file each, and comparing the three is what says where a limit lives. Every
  step reads its payload back and checks each block is the one written to it,
  and exits 7 rather than reporting a rate when it is not.
- `bench swarm` puts synthetic peers on a target that is somebody else's
  process, so it takes a `HOST:PORT` rather than a torrent and that address is
  the only thing it contacts: no tracker, no DHT, no PEX, and no peer list read
  from a torrent or the configuration. Two loads, chosen by `--for`. With it,
  the peers leech a torrent the target already serves and check every completed
  piece against the torrent's own hashes. Without it, info hashes are
  generated that the target cannot have, and what is measured is the accept and
  handshake path. Measured against `bit-cli seed` on loopback: 333.33 MiB/s at
  one peer, 666.67 at four, 941.18 at sixteen. Two halves are not built and are
  open under `TODO/bench.md` T-092: `--disk-budget` bounds the piece bytes kept
  and not the file length on disk, because a held piece is written at its own
  offset, and a synthetic peer keeps its verified pieces without serving them.

### Measurement

- Every `bench` report carries the machine it was taken on. `bit-cli` version,
  target triple, build profile, and whether debug assertions were on. OS and
  kernel version, CPU model, logical core count, total memory, and NIC link
  speed. The exact command line and working directory. Start and end timestamps
  in ISO 8601 UTC with millisecond precision. Peak RSS, user and system CPU
  time, and open handle count, sampled on the metrics interval as well as at
  the end. All of it read through the platform's own interfaces, with no new
  dependency.
- `bench seed` measures a seeder: what leaves, per peer, and what the disk cost
  to send it. The same envelope `bench leech` fills with every counter facing
  the other way, bytes sent rather than received and positioned reads rather
  than writes. `--include-hash-check` reports what the check on add cost before
  the clock started, and `--exit-when-idle` stops a run nobody is pulling from.
  Measured on loopback: 738.25 MiB to three peers reading 772.83 MiB off the
  disk, a read amplification of 1.047.
- Latency percentiles come from a histogram rather than a sorted vector, so a
  six hour run costs the same memory as a six second one.
- Rates in a report carry their unit. A field named `rate` used to serialize
  `"human": "2.75 MiB"`; it now reads `2.75 MiB/s`, with the same integer
  beside it and the same wire shape, so an older report still reads back and
  `--baseline` still compares the same field.
- `scripts/check-stall.ps1` runs one command hundreds of times and reports the
  distribution rather than a mean, because a mean says nothing about a tail.
  A bridge counts every time it lost its connection to the session, what it
  waited to make another, and what ended the attempt before it, so a run that
  was waiting and one that was slow no longer look the same.
- `scripts/soak.ps1` samples a long-lived seeder every 30 seconds for as long
  as it is given, under one of six workloads, and writes resident memory,
  handles, threads, CPU, and TCP socket states to a CSV with the slope of each.
  The summary is rewritten after every sample, to a temporary file that is
  renamed over the target, so a run that is killed leaves the slopes it had
  reached. It used to write in place, and a killed run left a file of NUL
  bytes.
- A long run does not leak descriptors, and it does leak memory slowly. An
  idle seeder holds **189 handles at every one of 533 samples over 4.6
  hours**, one TCP socket, and 21 threads, with resident memory flat inside
  0.03 MiB over its last two and a half hours. Under a deployment-shaped load
  of downloads plus tracker announces, resident memory rises **0.804 MiB an
  hour**, linear rather than settling: the last three hours give the same slope
  as the whole run, and every saturating model fits worse. `CLOSE_WAIT` is zero
  at all 1,064 samples across both runs. See `TODO/memory.md` under T-040.
- The warmup window is reported rather than dropped. A sample taken during
  warmup is marked and excluded from the summary, because "it was slow for the
  first three seconds" is itself a result.
- Connection establishment is measured on its own cadence, one connection per
  source per metrics interval, because a pooled HTTP client cannot report what
  opening a connection costs.
- Four report formats: `json`, `ndjson`, `csv`, and `text`. The report goes to
  stdout unless `--report <PATH>` names a file. `csv` carries the time series
  only, which is said in the docs rather than left to be discovered.
- `--fail-under <RATE>` exits 14 when sustained throughput falls below the
  rate. `--baseline <PATH>` prints a delta per metric with a sign, a
  percentage, and which direction is an improvement, and refuses a comparison
  across different hardware, a different subcommand, or a newer report version,
  naming the reason.
- `--target-rate` paces the run against its own totals rather than per worker,
  so the target is the aggregate.
- Storage counts its positioned reads and writes, their bytes, and their time,
  and brackets every piece check: a check is a run of reads walking the piece
  from its start followed by the session declaring it complete, all on one
  thread, so the wall time between them is the whole cost of the check with the
  SHA-1 included. Two `Instant::now()` calls per operation, always on, because
  a counter that is only on when someone is measuring measures a different
  program.
- The loopback bridge counts the blocks the session has asked for and not yet
  been given, the deepest that ever got, and the time to answer each one. That
  is the session's own request window seen from the other end, and it is what
  says whether the window is what bounds a run.
- `bench leech` refuses to run against an output directory that already holds
  the complete payload. That run finishes without fetching anything and would
  report the hash checker's rate as a download rate.
- Every source reports the bytes it pulled over HTTP beside the bytes that
  reached the session. The two differing is the amplification: separate
  sources at one URL each keep their own window cache and fetch the same
  window once each, which measured 3.98x against 1.004x for the same number of
  connections on one source.
- A share of a stated ceiling is no longer clamped at a hundred percent. It is
  a comparison rather than a progress, and `--ceiling` names a reference the
  caller supplies, so a run that beat it now says so. The clamping renderer is
  still what progress uses.
- Each `bench disk` step drains its writeback after the clock stops and reports
  it as `flush`. Without that, a step that filled the page cache hands its cost
  to whichever step runs after it, and a sweep reports the order the steps ran
  in rather than the thread count.

### Paths

- Every torrent path is planned before anything is opened. A component the
  platform reads as a drive or a root cannot leave the output directory, a name
  the filesystem refuses is rewritten rather than failing the download, and two
  names that collide only on a case-insensitive filesystem both land under
  distinct names.
- The rules run on every platform, not only Windows, so a payload downloaded on
  Linux and copied to a Windows machine still works.
- A payload path past the 260 characters the classic Windows API allows lands
  as written and verifies from the same path, with nothing renamed. The one
  limit that applies is per component: a name over 255 bytes is truncated to
  fit, keeps its extension, and is reported like any other rename.
- Every change is reported on stderr and in `--json` as a `renamed` array
  carrying the file index, both paths, and the reason. The key is absent when
  nothing changed.
- `bit-cli` supplies its own storage to do this. Reads and writes are addressed
  by file index and offset rather than by a cursor, so many pieces can be in
  flight against one file.
- Whether a torrent unpacks into a directory of its own follows BEP 3: `name`
  is the file's name when the metainfo carries no `files` list and the
  directory's name when it does, however many entries that list holds. Deciding
  it by counting files instead dropped the directory for a torrent whose
  `files` list held one entry, so two such torrents in one invocation wrote the
  same path and both reported success. `aria2c` 1.37.0 creates the directory
  for the same torrent. See `TODO/performance.md` under T-036.

### Disk

- A payload file is created when it is first written, not when the torrent is
  added, and a read of a file that is not there does not bring one into
  existence. `--select-file 0` therefore writes one file and leaves the rest
  off the disk instead of creating them empty beside it.
- `--max-open-files` caps how many payload files stay open, closing the least
  recently opened when it is reached. The default is 128. A torrent with twenty
  thousand files needs the cap in descriptors and not twenty thousand.
  `scripts/check-handles.ps1` measures it: the steps in the cap and the steps
  in the process handle count match exactly.
- `--file-allocation` does four different things rather than four names for
  one. `none` sets the length, `sparse` marks the file sparse first, `prealloc`
  writes and flushes zeroes, and `falloc` asks the filesystem to reserve the
  blocks. `falloc` on Windows needs a privilege an ordinary process does not
  hold, so it falls back to `prealloc` and says so on stderr.
  `scripts/check-allocation.ps1` measures all four against a real download by
  reading volume free space before the payload arrives.
- Concurrent positioned writes to one file are safe against each other, which
  is why the handle lock is taken by the read half. A test drives eight threads
  at one file and checks every block for the byte its writer owned.
- They are safe but they do not scale, and `bench disk` says why: on NTFS
  writes to one file serialise whatever handle they arrive on, so more handles
  buy nothing and only spreading the work over more files helps. The
  serialisation is charged per operation rather than per byte, so the same
  gigabyte in 1 MiB writes reaches 2.30 times what it reaches in 16 KiB writes
  at eight writers. See `TODO/disk-io.md` under T-017.

### Concurrency

- `--piece-selector` decides what order pieces arrive in, and it has three
  values rather than four. `sequential` and its `aria2` spelling `in-order`
  hold the session's piece priority at the earliest piece still missing, so a
  download is readable front to back: over ten runs at one connection, zero
  pieces arrived before one already reported, against one in every run of the
  default. It costs nothing at one connection and about seven percent at four,
  and above one connection the order is not exact and cannot be, because a
  selector decides which piece is asked for next and not which of four
  transfers in flight finishes first. `scripts/check-piece-order.ps1` is the
  measurement.
- The default value is `default`, not `rarest-first`. `rarest-first` named
  behaviour nothing here has: `librqbit` 9.0.0's picker does not count how many
  peers hold a piece anywhere. What it does is the first piece of each file,
  then the last, then the middle in ascending order, and `default` says so.
  `random` is gone for the opposite reason: nothing implemented it and there is
  no way to ask for it. `TODO/performance.md` T-032 has both, with the
  measurement that establishes the first.
- `download` notices a torrent finishing when it finishes, rather than on the
  next `--report-interval` tick. The watch loop woke only on the tick and
  checked completion afterwards, so a run that finished 1.1 seconds in ended at
  2.0, and `-j 1` with four torrents paid that four times. `--timeout` and
  `--stop-after` had the same lag and now wake the loop themselves. Measured
  against the same runs with only the tick: 1.42x for one torrent alone, 1.31x
  at `-j 1`, 1.36x at `-j 2`, 1.18x at `-j 4`.
- `-j` scales. Four torrents of 256 MiB at `-j 4` finish 3.54 times faster than
  running them one invocation at a time, and 3.50 times faster than `-j 1` in
  the same process, at 71.73% of what the HTTP source serves with no torrent
  machinery at all. Putting the same total connection count on one torrent at a
  time reaches 0.59 times that, so the flag buys concurrency rather than
  connections. `scripts/check-multi-torrent.ps1` is the measurement and it
  writes `bench/multi-torrent-<timestamp>.json`.
- Concurrency costs about 22 MiB of peak RSS and twelve handles per torrent in
  flight. CPU is flat for the same bytes.
- Sources start in the order they were given, so `-j 1` is a sequence a caller
  can depend on: a torrent whose source is a file an earlier torrent writes can
  name it. The plans are a queue taken by a fixed pool of workers rather than a
  task per plan queuing on a semaphore, which is what made the order the
  scheduler's before.
- `--web-seed-for` may name one torrent by info hash, `<40 hex>:file:N=URL`, so
  a run over several torrents that share a file can say which one a binding
  means. A hash naming no torrent in the run is a usage error rather than a
  binding that quietly does nothing.
- Two torrents in one invocation that hold the same file fetch it once, with no
  binding written by the caller. Every pair is compared by the piece hashes
  covering each file before the session starts, and where they prove two files
  are the same bytes the later torrent reads the copy the earlier one wrote.
  Only a proof counts, never a matching length; only a torrent that has already
  finished donates, so `-j 1` is what makes the order true; and the copy is
  checked per piece on the way in like any other source. Measured over three
  info hashes with the file at a different path and index in each: 16 MiB
  fetched once over HTTP, read off the disk twice, one distinct hash across
  three output directories. `--no-share-files` turns it off, and
  `pwsh scripts/check-shared-files.ps1` is the measurement. See
  `TODO/multi-source.md` under T-140.
- `--redial-after <DUR>` drops every peer connection and dials again when
  nothing has arrived for that long, which throws away the reconnect backoff
  instead of waiting it out. Measured against a 120 second outage: without it
  the run exits 9 with 17.00 MiB of 128 after 300 seconds of patience, with it
  the run re-dials four times and completes byte for byte.
- `--web-seed-cooldown` is honoured. A source that spends its error budget
  sleeps for that long and then reconnects with the error run cleared. It is
  zero by default, meaning the source does not come back, which keeps a run
  against one dead mirror failing in half a minute rather than sitting on a
  timer. A sleeping source reports `"state": "cooling"` rather than `failed`.

### Contract

- 16 exit codes. Codes 11 through 15 exist so a script can tell "your mirrors
  are misconfigured" from "the network is down" from "your server is slow".
- stdout carries data only; stderr carries logs, progress, warnings, and
  errors.
- Every JSON document carries `schema_version`, `generated_at`, and
  `bit_cli_version`.
- `--jsonl` emits one event per line with a monotonic `seq` and an ISO 8601 UTC
  millisecond timestamp, and every run ends with a `session_end` event carrying
  the exit code, so a consumer can tell "finished" from "the pipe broke".
- `docs/schema.md` lists every document `kind` and every event `type` with the
  fields each carries, and it is generated from what the program writes rather
  than written by hand: a test drives every command, flattens the JSON, and
  fails when a report carries a field the document does not. All thirty-one
  names have a run behind them, 669 field rows over 992 lines, and a second
  test fails when a name stops being produced.
- A resumed download no longer charges its existing bytes to the swarm.
  `from_web_seeds`, `from_peers`, and `from_resume` partition the total.
- `--log-file` writes and rotates. `--log-max-size` is the size a live log may
  reach and `--log-max-files` is the count kept in total, the live one
  included. It adds a destination rather than replacing stderr, so
  `bit-cli ... --json | jq` behaves the same either way.
- Nothing is TTY-gated. Terminal detection decides colour and progress
  rendering and nothing else.
- Six-layer configuration precedence, and `config show` reports the origin of
  every value.
- A failed add carries the code that says why. `download --json` reports a
  `code` per torrent and the run exits with the worst of them, so an existing
  file exits 8 rather than a generic 1.
- `seed` and `verify` carry the same `renamed` array `download` does, because
  they serve and read the files it wrote. `verify` also reads the planned paths
  rather than the torrent's own, which it did not before.
- `--port` reaches `download` and `peers`, not only `seed`. `--no-continue`
  turns off `--continue`, which previously defaulted on with no way to clear
  it. `--init-timeout` bounds the hash check and names the phase when it fires.

### Build

- Every release binary is self-contained, and the check reads the binary rather
  than trusting the build. `scripts/check-static.ps1` picks its check from the
  file's own magic bytes: a PE must import no `VCRUNTIME`, `MSVCP`, `MSVCR`,
  `UCRT`, `CONCRT`, or `api-ms-win-crt-*`, and an ELF must carry no `PT_INTERP`
  and no `DT_NEEDED`. CI and the release workflow both run it on all three
  targets. The `x86_64-pc-windows-msvc` binary imports `kernel32`, `ntdll`,
  `combase`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0`, `ws2_32`,
  `shell32`, `crypt32`, `bcrypt`, `userenv`, `advapi32`, and `iphlpapi`. The
  one api-set is a core OS set, not a CRT redirect. The script prints the size
  of whichever binary it checked rather than the number being pinned here,
  because a size moves with every commit and a pinned one goes stale.
- The minimum supported Rust version is 1.88, which is the highest
  `rust-version` in the resolved dependency graph rather than a preference. It
  is stated in `Cargo.toml`, pinned in CI's `MSRV` job, and named in the
  README, and a test fails if the three stop agreeing.
- Link flags are set per target in `.cargo/config.toml` and repeated in
  `ci.yml`, because setting the `RUSTFLAGS` environment variable **replaces**
  per-target `rustflags` rather than adding to them. That file also records
  which flags were considered and rejected, so nobody re-adds `opt-level=z` to
  a profile that deliberately optimises for throughput.
- `create` output is byte-identical on repeat runs and independent of input
  order, with paths `/`-separated and sorted by raw bytes so no locale can
  affect it.
  A constant in the test suite pins the bytes, so every platform CI runs on
  compares against one number rather than against each other, and `ci.yml`
  builds the same fixture on Linux and Windows and compares the two hashes.
- `create`, `verify`, and `seed` round trip through two other implementations.
  `scripts/interop-roundtrip.ps1` seeds a four-file 490,012 byte payload on
  loopback and downloads it with `aria2c` 1.37.0 and with `rqbit` 9.0.0, byte
  for byte, in three cases: plain v1, `--private`, and `--web-seed` with no
  peer at all. `rqbit` skips the third, which it cannot do: it has no BEP 19.
  CI runs the `aria2c` matrix on Linux and Windows. The record is in
  `TODO/create-seed.md` under T-084.
- `THIRD_PARTY.md` is generated from `Cargo.lock` by `cargo about` and covers
  310 crates, including the Apache-2.0 text `librqbit` requires. `deny.toml`
  allows permissive licences only, and CI fails on anything else, both through
  `cargo deny` and because generation itself refuses an unaccepted licence.
  `scripts/check-licence-gate.ps1` proves the gate rejects a GPL dependency
  rather than assuming it would.
- `edit` splices the original `info` bytes back verbatim and re-hashes before
  returning, so it cannot change the info hash even for a torrent whose
  original encoding was not canonical.

### Metalink

- A `.meta4` (RFC 5854) or `.metalink` (Metalink 3) is a source.
  `bit-cli download release.meta4` reads the document, fetches the `.torrent`
  its `<metaurl>` names, registers every `<url>` mirror as a web seed source,
  downloads, and verifies the payload against the document's own checksum.
  Mirrors carry `origin: "metalink"` in `--json`.
- Several `<metaurl>` entries are a mirror list for the `.torrent` itself, and
  are tried in the document's preferred order until one parses. The failures
  are reported as `torrent_fallbacks`, so a run says the preferred one was not
  the one used.
- The exact bytes fetched are handed to the session rather than the URL. Two
  fetches of one URL can return two documents, and a report describing one
  torrent while the session downloads another would be worse than a failure.
- **A Metalink and a `.torrent` are two independent descriptions of one
  payload, and both are checked.** The declared lengths are compared before a
  byte is fetched. The document's digest is then checked against a payload the
  session has already verified piece by piece against the torrent's own SHA-1
  hashes, so a digest that disagrees says the Metalink is the document that is
  wrong, and the warning says so. Either disagreement exits 7.
- `sha-256`, `sha1`, and `md5` are computed, strongest first. An algorithm this
  cannot compute is reported as `not_checked` with the reason, and `matched` is
  absent rather than `true`. A checksum that was not computed is not one that
  passed.
- `--dry-run` reads the document with no network at all and reports the
  mirrors, the size, the checksum, and the torrent URLs. It is the cheapest way
  to check that a `.meta4` says what its author meant.
- `--no-torrent-web-seed` drops a Metalink's mirrors along with the torrent's
  own `url-list`. Both are the sources the source document declared rather than
  the ones the caller named.
- `ftp:` mirrors are counted and not registered, per-piece hashes under
  `<pieces>` are not collected as whole-file checksums, and a document that
  stops mid-element is refused rather than accepted as a shorter mirror list.
- `pwsh scripts/check-metalink.ps1` drives ten cases on loopback and
  `pwsh scripts/check-metalink-real.ps1` drives four against
  `download.documentfoundation.org`. The real one records a finding: no
  MirrorBrain instance reachable in August 2026 emits
  `<metaurl mediatype="torrent">`, so a real document alone has nothing to
  download from, and the error names how many mirrors it did have. See
  `TODO/cli-surface.md` under T-113.

### Not in this release

BEP 52 v2 and hybrid creation ([T-081](TODO/create-seed.md)), BEP 16
superseeding ([T-082](TODO/create-seed.md)), BEP 6 fast extension
([T-100](TODO/bep-coverage.md)), BEP 55 holepunch
([T-102](TODO/bep-coverage.md)), uTP ([T-101](TODO/bep-coverage.md)), MSE/PE
peer encryption ([T-163](TODO/peers.md)), non-UTF-8 filenames
([T-103](TODO/bep-coverage.md)), and `-i/--input-file`
([T-114](TODO/cli-surface.md)). Each has an entry in `TODO/` with what closes
it.

**No command is stubbed, and two flags are.** A command that is not
implemented says so and exits with a code a script can branch on, and a flag
that cannot yet do what it says warns rather than staying silent:
`--superseed` for BEP 16, and `--no-pex`, because `librqbit` 9.0.0 has no
switch for peer exchange.

The two that are still silent are `-O/--index-out`
([T-116](TODO/cli-surface.md)) and `--on-piece-verified`
([T-115](TODO/cli-surface.md)), and they are now the whole list rather than
part of it. A test walks the `clap` tree and fails on any flag outside it that
nothing reads, so a third cannot be added quietly
(`crates/bit-cli/src/cli.rs`, `every_flag_reaches_code_or_is_a_named_exception`).

Two revisions of this section have been wrong about this in opposite
directions. The first claimed nothing was stubbed and six flags were. The
second listed six and missed a seventh, `--web-seed-list-url`, which **was**
read and read only into a function that always errors, on every command that
accepts it ([T-183](TODO/cli-surface.md), fixed). Neither the audit nor the
test above could have found it, because both look for a field with no reader
and that one has a reader. The count is not the point; the test is, and what
the test is weak about is written in its own docstring.

`bench swarm` ships and is not finished. Both loads work and the two halves
that are missing are named above rather than left for a reader to discover.
