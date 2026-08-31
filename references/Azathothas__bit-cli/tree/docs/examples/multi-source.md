# Several sources for one payload

Pointing a swarm, a CDN, a signed URL and a copy already on disk at the same
torrent, and getting the bytes without fetching any of them twice.

The working record for this, including what was measured and what is still
open, is [`../../TODO/multi-source.md`](../../TODO/multi-source.md). This page
is what the tool does today.

## There is no separate concept for a direct download link

A "direct download link for one file" is already expressible, and that is why
several of these arrangements need no flags at all. A binding is a triple:

- **source**: an HTTP or HTTPS URL with its own headers, auth, user agent,
  timeouts, concurrency, connections and rate cap, or a `file:` URL naming
  bytes already on disk.
- **scope**: which part of the torrent that source may serve.
- **composition**: how the request URL is built.

A direct link to one file is a source with scope `1` and composition `exact`.
Nothing about the term reaches the command line.

## A CDN copy under a different name

The CDN holds one file of the torrent, renamed, in a different directory. The
scope says which file and `exact` says to use the URL unchanged rather than
appending the torrent's own path:

```bash
bit-cli download album.torrent \
  --web-seed-for "0=https://cdn.example/a3f1b2c4-signed-blob.dat" \
  --web-seed-mode exact
```

Check it before running it. `webseed list` resolves every binding and prints
the exact URL each file maps to, and touches the network not at all:

```bash
bit-cli webseed list album.torrent \
  --web-seed-for "0=https://cdn.example/a3f1b2c4-signed-blob.dat" \
  --web-seed-mode exact
```

## A signed URL that expires

A signature usually expires. `bit-cli` re-resolves the stable source URL on
every request rather than caching a signed one, so a signature at a realistic
window does not expire mid-download.

What matters when it does expire is which status retires the source:

```bash
bit-cli download album.torrent \
  --web-seed "https://cdn.example/signed/?token=..." \
  --web-seed-retry-status 429,500,502,503,504 \
  --web-seed-fatal-status 401,403 \
  --web-seed-max-errors 5 \
  --web-seed-cooldown 60s
```

`--web-seed-cooldown` is the one that is easy to leave out and expensive to
leave out: without it a source retired on a burst of failures is lost for the
whole run.

```bash
pwsh -NoProfile -File scripts/check-signed-source.ps1
```

drives nine cases against a loopback server that signs, redirects, expires a
signature and fails on a clock.

## Bytes that are already on this disk

A source URL may be `file:`, so a copy already on disk is a source like any
other. No server, no port:

```bash
bit-cli download album.torrent \
  --web-seed-for "0=file:///D:/archive/album.flac" \
  --web-seed-mode exact
```

```bash
pwsh -NoProfile -File scripts/check-local-source.ps1
```

is the acceptance, eight cases, no server and no bound port.

## Two torrents that hold the same file

This one needs no flags at all.

```bash
bit-cli download c.torrent a.torrent b.torrent -j 1
```

Before the session starts, every pair of torrents is compared by the piece
hashes covering each file. A file another torrent has already written becomes a
`file:` source for the torrents that still need it, at whatever path and index
it has in each.

Measured over three info hashes with the shared file at a different path and
index in each: 16 MiB fetched once over HTTP, read off the disk twice, one
distinct hash across three output directories, in 511 ms.

```bash
pwsh -NoProfile -File scripts/check-shared-files.ps1
```

**`-j 1` is load bearing.** Above it nothing has finished yet when the later
torrents start, so nothing is donated. Attaching a source to a torrent that has
already started is T-143 in
[`../../TODO/multi-source.md`](../../TODO/multi-source.md), done, and the
ordering constraint is what is left.

## Asking what two torrents actually share

```bash
bit-cli files a.torrent --against b.torrent
```

It decides from the metadata alone and says what the answer rests on:
`piece-hashes` when the pieces line up and agree, `length` when only the size
matches.

**Three torrents with three different piece lengths is exactly the case where
piece hashes cannot be compared**, so the answer is honest rather than
optimistic: nothing is provable, and two of the length-only candidates may not
be the same bytes at all. A `length` answer is a candidate, not a fact.

## Capping the swarm and the HTTP sources separately

```bash
bit-cli download album.torrent \
  --web-seed "https://mirror.example/pub/" \
  --max-overall-download-rate 50MiB \
  --web-seed-speed-limit 10MiB
```

The swarm and the HTTP sources have separate caps, and the overall cap bounds
both together. That is the arrangement for a mirror you are allowed to use but
not allowed to hammer.

## Making one source do more of the work

```bash
bit-cli download album.torrent --web-seed "https://fast.example/pub/" \
  --prefer-web-seed --web-seed-connections 2
```

`--prefer-web-seed` moves the HTTP share of a hybrid run from a mean of 46.72%
to 62.60% across five paired runs. It works by doubling a source's connections
rather than its request budget, because a source is one peer and a peer's
blocks are hash-checked on that connection's own task: two connections is two
receive paths and it is worth 1.92x, while the same requests in flight on one
connection is worth 0.81x.

The curve is flat after two connections. `--web-seed-connections 8` is not
eight times anything.

```bash
pwsh -NoProfile -File scripts/check-prefer.ps1
```

## What is not there yet

Steering which source answers a given piece at run time is T-135, open. Today
the levers are the scope, the priority and the connection count, all set before
the run starts.
