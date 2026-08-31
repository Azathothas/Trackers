# Web seeds

Attaching arbitrary HTTP sources to an existing `.torrent` at runtime, without
rewriting it. This is what `bit-cli` exists for, and the addressing model below
is the part no other client has.

Every flag named here is in [`man/bit-cli.json`](../man/bit-cli.json). The
entries behind the behaviour are in [`TODO/webseed.md`](../TODO/webseed.md) and
[`TODO/multi-source.md`](../TODO/multi-source.md).

A web seed is normally one flat thing: a URL that serves the whole torrent.
Here a binding is a triple.

**Source** is where bytes come from: an HTTP(S) URL with its own headers, auth,
user agent, timeouts, concurrency, and rate limit, or a `file:` URL naming
bytes already on the disk.

**Scope** is what part of the torrent that source may serve. A mirror holding
part of the payload is a first-class case, not an error.

**Composition** is how the request URL is built from the source URL and the
torrent's `name` and `path`.

The three are orthogonal. Any source can serve any scope under any
composition.

## One source, several connections

A source reaches the torrent session as a peer, and a peer's blocks are
written and hash-checked one at a time on that connection's own task. That
path is what bounds the transfer, so a source presented over one connection
runs at one path's speed however fast the mirror is.

`--web-seed-connections <N>` presents the source over N connections, which is
N of those paths. They share one HTTP client, one window cache, and one
concurrency budget divided between them, so the mirror sees the same number of
requests either way.

```bash
bit-cli download release.torrent \
  --web-seed https://mirror.example.com/pub/ --web-seed-connections 2
```

On loopback, two connections reach 1.92 times what one reaches, and the curve
is flat after that. Eight times the requests in flight on a single connection
reaches 0.81 times, so it is the connections and not the requests. The numbers,
the commands, and the control are in `TODO/webseed.md` under T-009, with the
report under `bench/`.

The default is one connection. Two is the measured knee on loopback and
loopback flatters the receive path, so raising the default waits on the same
measurement against a real mirror.

`--prefer-web-seed` is the same lever applied for a different reason. On a
hybrid run where peers and HTTP sources both hold a piece, it doubles each
source's connections, so HTTP is more often the side that answers first. On a
loopback swarm of one mirror and one peer, neither rate limited, it moves the
HTTP share of a 1 GiB payload from a mean of 46.72% to 62.60% across five
paired runs:

```bash
pwsh scripts/check-prefer.ps1 -PayloadSize 1GiB -Runs 5
```

It moves the odds, not the decision. `librqbit`'s piece picker is not reachable
from outside the crate, so a piece a peer happens to answer first still comes
from the peer. `TODO/webseed.md` under T-003 has the numbers and what closing
the gap would take.

## Composition modes

| Mode | What it does |
| --- | --- |
| `auto` | BEP 19 default. Single-file: a URL ending in `/` gets `name` appended, otherwise the URL is the complete resource. Multi-file: `name` and `path` are appended per file. Matches `aria2`, so migrating a script is mechanical. |
| `exact` | The URL is the complete resource. Nothing is appended. For a mirror whose layout does not match the torrent's, or a file renamed on the server. |
| `prefix` | Appends `path` but not `name`. For mirrors hosting the contents at the root rather than inside a directory named after the torrent. |
| `template` | The URL carries placeholders expanded per request: `{name}` `{path}` `{filename}` `{index}` `{piece}` `{offset}` `{length}` `{end}` `{piece_offset}` `{piece_length}` `{infohash}`. Everything is percent-encoded unless written `{raw:path}`. |

## Scope selectors

```
*                    every file
3                    file index 3
3-7                  file indices 3 through 7, inclusive
3,5,9-               an index list and an open-ended range
path/to/file.iso     an exact path within the torrent
*.iso                a glob against the file path
!*.nfo               a negated glob, subtracted from the selection
piece:0-511          a piece index range
byte:0-1MiB          a byte range within the whole payload
file:3:byte:0-4MiB   a byte range within one file
```

Selectors are checked against the metainfo before any request goes out. A
selector matching nothing is an error, not a silent no-op.

## A local path as a source

A source URL may be `file:`. The bytes for a torrent are often already on the
disk under a different name, in a different directory, or inside a finished
copy of a different torrent that happens to hold the same file. Naming that
path is how they get reused instead of fetched again.

```bash
bit-cli download release.torrent \
  --web-seed-for 'file:0=file:///srv/archive/a3f1-blob.dat' \
  --web-seed-mode exact
```

Everything else about a source still applies: scope, composition, chunk size,
rate limit, retries, per-source accounting, and the same loopback bridge. In
particular the source is not trusted. `--web-seed-verify piece` is on by
default, so a local file of exactly the right length holding the wrong bytes is
refused with the path and the piece named, the same way a wrong mirror is.

`auto` composition works against a directory, so a tree you already have is one
flag:

```bash
bit-cli download album.torrent --web-seed file:///mnt/backup/
```

That resolves to `/mnt/backup/album/disc 1/a.flac` and so on, exactly as the
BEP 19 composition does over HTTP. `webseed list` shows the resolved paths
before anything is read.

A `..` in a resolved path is refused. `auto` and `prefix` composition append
the torrent's own `name` and `path` to the source URL, so the tail of it is
written by the `.torrent` rather than by you, and a hostile one naming
`../../../Windows/win.ini` would otherwise make a source rooted at one
directory read out of another.

`file:` is not in BEP 17 or BEP 19 and is never offered to a swarm. It is a
source for one invocation, like every other source here.

```bash
pwsh scripts/check-local-source.ps1
```

That drives eight cases with no server running and no port bound, including the
one this exists for: three torrents with three info hashes and three piece
lengths (2 MiB, 1 MiB, 512 KiB) sharing one 64 MiB file. The file is fetched
once and lands in three output directories with one distinct hash between them.

## Several torrents that hold the same file

A binding normally applies to every torrent in the invocation. When the same
file sits at a different index in each, say which one you mean by prefixing the
selector with that torrent's info hash:

```bash
bit-cli download c.torrent a.torrent b.torrent --dir out -j 1 \
  --web-seed-mode exact \
  --web-seed-for 'e608e60a…:file:0=https://cdn.example.com/blob' \
  --web-seed-for '00c47ee9…:file:0=file:///out/payload_c/a/b/c/file.blob' \
  --web-seed-for '17eb3674…:file:1=file:///out/payload_c/a/b/c/file.blob'
```

Torrent C fetches the file from the CDN. A and B read the copy C wrote. One
invocation, one trip to the CDN, three output directories, and the payload
hashes equal in all three. `-j 1` is what makes that safe: sources start in the
order they were given, so C has finished before A looks for its file.

Exactly forty hexadecimal characters followed by a colon is read as an info
hash. A hash naming no torrent in the run is a usage error, not a binding that
quietly does nothing. The binding table takes the same thing as a `torrent`
field on a `[[source]]`.

Nothing is trusted here. Every piece a `file:` source serves is hash-checked
against the torrent that asked for it, so a wrong binding costs a failed source
rather than a corrupt payload.

## The same thing with nothing written by you

Those three bindings are what the run can work out for itself, and it does:

```bash
bit-cli download c.torrent a.torrent b.torrent --dir out -j 1
```

Before the session starts, every pair of torrents in the invocation is compared
by the piece hashes covering each file. Where the hashes prove two files are
the same bytes, the later torrent gets a `file:` source pointing at the copy the
earlier one wrote, as soon as the earlier one has finished. No path, no info
hash, no flag.

```
torrent   finished over http from disk resumed  shared proven    hash
payload_c     True 20.00 MiB 20.00 MiB 0.00 B        0 0.00 B    42ee6db050db50ce
payload_a     True 0.00 B    16.00 MiB 3.00 MiB      1 16.00 MiB 42ee6db050db50ce
payload_b     True 0.00 B    16.00 MiB 3.00 MiB      1 16.00 MiB 42ee6db050db50ce
```

```bash
pwsh scripts/check-shared-files.ps1
```

That is the run above, measured: three info hashes, the shared file at a
different path and index in each, 16 MiB fetched once over HTTP and read off
the disk twice, one distinct hash across three output directories.

`--json` reports it under `shared`, per torrent, naming the file, the torrent
it came from, and how much of it the piece hashes proved:

```json
"shared": [{
  "index": 0,
  "path": "deep/nested/dirs/file.blob",
  "from_info_hash": "a0f16220418c110ee3b5dba0a689c2c1b4791ca5",
  "from_path": "out/payload_c/a/b/c/file.blob",
  "pieces_compared": 16,
  "bytes_proven": { "bytes": 16777216, "human": "16.00 MiB" }
}]
```

Three things bound it. Only a piece-hash proof counts, never a matching length.
Only a torrent that has already finished donates, so `-j 1` is what makes the
order true and above it nothing is donated. And the source is checked per piece
on the way in like any other, so a proof that was somehow wrong costs a retry
rather than a payload. `--no-share-files` turns the whole thing off.

## Which files two torrents actually share

```bash
bit-cli files a.torrent --against b.torrent --against c.torrent
```

```
INDEX  EVIDENCE  PROVEN  OTHER       OTHER PATH
0      length    -       c2806b5a:1  media/file.blob
0      length    -       31084dc6:0  a/b/c/file.blob
1      length    -       31084dc6:1  a/extra.bin
2      length    -       c2806b5a:2  notes/changelog.txt
```

`piece-hashes` means the pieces line up and their hashes agree, which proves
the bytes equal. `length` means the sizes match and nothing else could be
checked, which proves nothing: two of the four rows above are files that have
the same size and different contents.

A `.torrent` hashes fixed-size pieces of the whole payload, not files, so two
files can be compared by hash only where the pieces cover the same bytes of
each. That needs the same piece length and the same offset modulo it. The three
torrents above have three piece lengths, so nothing among them is provable.
Against a pair that lines up, the same command proves the whole file:

```
INDEX  EVIDENCE      PROVEN     OTHER       OTHER PATH
0      piece-hashes  64.00 MiB  c3dabcae:0  file.blob
```

## Which failures are worth retrying

Whether an HTTP status is worth retrying is a property of the server, not of
the code. `bit-cli` treats 401, 403, 404, 410 and 416 as permanent and retries
the rest. Two flags move a code across that line, per source:

```bash
bit-cli download release.torrent \
  --web-seed-for 'file:0=https://cdn.example.com/blobs/a3f1/payload.iso' \
  --web-seed-retry-status 403
```

**A permanent status on one file does not retire the source.** The file's
pieces are dropped from what that source announces and it goes on serving the
rest, so a mirror holding eleven files of twelve stays a mirror for eleven of
them. A source with nothing left is retired, and the reason says it ran out
rather than naming one file. `--json` carries `gone_files` and
`pieces_dropped` per source, both omitted when nothing was lost.

A source addressed by piece rather than by file, which is BEP 17, has no
per-file request to attribute a failure to, so a permanent status retires it
whole. See `TODO/webseed.md` under T-005.

A CDN that signs its URLs answers 403 when a signature expires, and the next
request to the stable URL is redirected to a fresh one and succeeds. Without
the flag the first expiry ends the source. With it the run rides them out:

```bash
pwsh scripts/check-signed-source.ps1
```

That drives nine cases against a loopback server that signs, redirects,
expires, and falls over. The pair that matters here is the same server and the
same signature window run twice, differing only in the flag. In the recorded
run, 22 signatures expired over 64 MiB: without the flag the run downloaded
nothing and exited 1, with it the payload completed byte for byte. The report
is `bench/signed-source-20260820T132602637Z.json`. The count varies with
timing; whether the run completes does not.

`--web-seed-fatal-status` is the other direction, same spelling: a code it
names is treated as permanent even though the built-in classification would
retry it, so it narrows the source or, on a request that names no file, retires
it. Both take codes and inclusive ranges (`403`, `403,429`, `500-599`). A code
in both lists is a usage error, because there is no defensible answer.

The retries are reported per source, in the text output and in `--json`:

```
source               http://127.0.0.1:57581/cdn/a3f1b2c4-signed-blob.dat
  scope              file:0
  state              active
  served             64.00 MiB
  retries            10 (10 on 403)
```

What bounds a retried source is `--web-seed-retries`, the attempts one request
gets, and `--web-seed-max-errors`, the consecutive failed requests a source
gets before it is out. A request that fails transiently after spending its
retries drops the connection and reconnects, so a mirror that restarts
mid-download is not lost. At the defaults that is four attempts per request and
five requests: measured against a mirror answering 503 forever, the source is
retired and the run exits 1 after 33.4 seconds.

**A mirror that stops answering is not a mirror answering badly, and the two
are handled differently.** A request that runs out of time is a stall: the
mirror is holding the connection open and will hold the retry the same way, so
a stall spends neither the retry ladder nor four more requests. It retires the
source at once. Against a backend that sends 64 KiB and then hangs, that is
**10.1 seconds and two requests** where spending the whole budget was 133
seconds and 21. `--web-seed-timeout` is what says how long is too long, and
`--web-seed-cooldown` still brings the source back.

## Giving a mirror another chance

A source that spends that budget is out for the rest of the run.
`--web-seed-cooldown` puts it back to work instead:

```bash
bit-cli download release.torrent \
  --web-seed https://mirror-a.example.com/pub/ \
  --web-seed-cooldown 30s --timeout 10m
```

The source sleeps for that long, then reconnects with the error run cleared. A
mirror that is down for five minutes is usable again at minute six instead of
lost at second seventeen.

It is zero by default, which means the source does not come back. That is what
keeps an unattended run against one dead mirror failing in half a minute rather
than sitting on a timer, and it is why the flag is opt-in: a caller who wants
patience says how much.

While it sleeps the source reports `"state": "cooling"` rather than `failed`,
with `cooldown_until` and `cooldown_remaining_ms` beside it, and `cooldowns`
counts how many times it has been out. A cooling source is not a dead one, so
`--web-seed-require` and the "every source is dead" stop condition keep waiting
for it. Bound that with `--timeout` or `--stop-timeout`.

Measured, one mirror down for twenty seconds and two runs differing only in the
cooldown:

| cooldown | exit | downloaded | state | cooldowns |
| --- | --- | --- | --- | --- |
| 5s | 0 | 64.00 MiB | active | 4 |
| 300s | 9 | 3.00 MiB | cooling | 1 |

Both were given `--timeout 60s`. The first woke into a mirror that was still
down twice, then into one that was back, and finished in 23.5 seconds. The
second was still asleep with 241.1 seconds left when the deadline fired.

## Checking the addressing before you download

`webseed list` resolves every binding and prints the exact URL each file maps
to. It touches no network.

```bash
bit-cli webseed list album.torrent --web-seed https://mirror.example.com/pub/
```

```
torrent              album
info hash            6700edefb64af8f2cf692179ae5b0092f824bda6
size                 43.95 KiB
sources              1
coverage             43.95 KiB of 43.95 KiB (100.00%)

[0] https://mirror.example.com/pub/
  scope              * (100.00%, 2 files, 3 whole pieces, 0 partial)
  composition        auto / auto / priority 0
  origin             command_line
  FILE  IN SCOPE   PATH           URL
  0     39.06 KiB  disc 1/a.flac  https://mirror.example.com/pub/album/disc%201/a.flac
  1     4.88 KiB   notes.nfo      https://mirror.example.com/pub/album/notes.nfo
```

Note the space in `disc 1` came back as `%20` and the `/` separators did not.
Getting that wrong is the most common way a web seed silently serves nothing.

Then check the mirrors answer:

```bash
bit-cli webseed test album.torrent --web-seed https://mirror.example.com/pub/
```

This reports range support, the entity length against what the torrent says,
the redirect chain hop by hop, the negotiated TLS version and cipher suite, the
latency, and the response headers that say where the bytes came from. One
request per source, one byte of payload at most.

```
source               https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases/x86_64/
  requested          https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases/x86_64/latest-releases.yaml
  scope              *
  status             206
  ranges             yes
  length             3.24 KiB (matches the torrent)
  server             nginx/1.29.0
  age                8
  etag               "6a2d8918-cf8"
  last-modified      Sat, 13 Jun 2026 16:45:12 GMT
  via                1.1 varnish, 1.1 varnish
  x-cache            HIT, HIT
  x-served-by        cache-ams-eham8680082-AMS, cache-bom-vanm7210091-BOM
  http               HTTP/1.1
  tls                TLSv1_3 TLS13_AES_128_GCM_SHA256
  handshake          connect 49ms, tls 52ms
  alpn               h2
  ttfb               269ms
  total              371ms
```

**`x-cache HIT, HIT` is the line that decides what a mirror costs.** A payload
served from cache costs the CDN's rate; one that misses costs an origin request
per range, and the difference between those two is the whole reason to put a
CDN in front of a bucket. It is also what makes a slow source diagnosable:
`ttfb 269ms` says the request was slow and `x-cache MISS` says why.

These are the headers of the exchange the rest of the report describes,
received rather than requested again. Asking a second time would answer a
different question, over a different connection, possibly from a different
cache node.

The reported set is fixed, because a report is a thing people paste and a
header set can carry a signed URL or a session cookie:

| header | what it answers |
| --- | --- |
| `age`, `x-cache`, `cf-cache-status`, `x-served-by`, `via` | was this served from cache, in four CDNs' spellings |
| `x-amz-request-id`, `x-amz-id-2` | the two values an S3 support ticket asks for, neither recoverable afterwards |
| `etag`, `last-modified` | whether `If-Range` resumption survives a deploy |
| `content-encoding` | whether a proxy changed what a byte range means |
| `cache-control`, `cf-ray` | the context for the rest |

Anything else is one flag away, and it is matched without case:

```bash
bit-cli webseed test album.torrent --web-seed https://mirror.example.com/pub/ --web-seed-report-header X-Cache-Hits
```

A header named that way whose value is a credential is still printed as
`<redacted>` unless `--no-redact` is given. `server` keeps its own field and is
not in the map.

## Fetching one piece from one mirror

```bash
bit-cli webseed fetch album.torrent \
  --url https://mirror.example.com/pub/ \
  --piece 42 --verify --json
```

Writes nothing unless `--output` is given, exits non-zero on a hash mismatch,
and reports full timing. Under `--trace http` it also prints the equivalent
`curl` command for every request it made, which is the standard the trace is
held to: if you cannot reproduce a failing request by hand from the log, the
trace is not detailed enough.

## What the bytes are checked against

Per-piece verification against the torrent's own hashes is not optional and is
not a mode. [`integrity.md`](integrity.md) states the whole guarantee, and
[`examples/cloudflare-webseed.md`](examples/cloudflare-webseed.md) walks a real
origin end to end.
