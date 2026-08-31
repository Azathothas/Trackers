# Measuring a mirror before trusting it

A mirror that answers a browser quickly can still be a poor web seed: it may
ignore `Range`, rate limit per object, collapse under concurrency, or sit
behind a redirect chain that costs more than the transfer.

Four commands, in this order. Each one answers a question the next one assumes.

## 1. Does it serve what the torrent needs

No network:

```bash
bit-cli webseed list album.torrent --web-seed "https://mirror.example.com/pub/"
```

This prints the exact URL every file maps to. If those URLs are wrong, nothing
below matters. The commonest failure is a base URL without a trailing slash,
which BEP 19 treats as naming the payload itself rather than a directory.

## 2. Does it behave like a web seed

One request per source, one byte of payload at most:

```bash
bit-cli webseed test album.torrent --web-seed "https://mirror.example.com/pub/"
```

Reports range support, the entity length against what the torrent says, the
redirect chain hop by hop, the negotiated TLS version and cipher suite, and the
latency.

**Range support is the pass or fail.** An origin that answers `200` to a ranged
request is refused rather than read, because reading it as if it were the
requested range serves wrong bytes at every offset.

A length that disagrees with the torrent is the second thing to look at: it
usually means the mirror holds a different build, and every piece will fail.

## 3. How does it behave under concurrency

```bash
bit-cli webseed probe album.torrent \
  --web-seed "https://mirror.example.com/pub/" --concurrency-sweep 1,2,4,8,16
```

Latency percentiles and throughput as concurrency rises. The number where the
curve flattens is the number to pass to `--web-seed-connections`, and it is
usually smaller than the connection would suggest, because most mirrors rate
limit per object rather than per client.

## 4. What does the whole path cost

```bash
pwsh -NoProfile -File scripts/bench-webseed.ps1
```

This is the one that attributes rather than measures. It takes the number in
four stages, so a slow run can be blamed on the right layer:

| stage | what it isolates |
| --- | --- |
| raw `curl` against the mirror | the network and the origin, with nothing of ours in the path |
| `bit-cli`'s own HTTP path | our fetcher, without the torrent session |
| one source as one peer | the bridge, and the per-connection receive path |
| the same source over N connections | whether the bridge or the mirror is the wall |

**The bridge costs about five sixths of the available throughput, and the
reason is that one source is one peer.** A block arriving from a peer is
written, and at a piece boundary the whole piece is read back and hashed,
inline on that connection's own task before the next block from that peer is
processed. One connection reaches 18.18% of `bit-cli`'s own HTTP path on
loopback; the same source over two reaches 34.90%, which is 1.92x.

Three things it is **not**, each ruled out by measurement rather than argument:

- **Not the requests in flight.** The same 64 requests on one connection reach
  0.81x, slightly worse than 8 on the same connection.
- **Not the request window.** The bridge sees the 128 block window reached, and
  the run sits at 40% of what that peak would allow.
- **Not hashing.** Piece checks are 11% of a one-connection run.
- **Not the disk.** Storage moves 1.31 GiB/s at eight writers on one file,
  which is 3.3 times what an eight-bridge run asks of it.

For comparison, `bit-cli`'s own HTTP path beats `curl` over a real network, at
156.71% of eight parallel `curl` slices. The bridge is the cost, and
`--web-seed-connections` is the lever.

## Comparing two mirrors, or the same mirror twice

```bash
bit-cli webseed probe album.torrent --web-seed "https://a.example/pub/" \
  --json > a.json
bit-cli webseed probe album.torrent --web-seed "https://b.example/pub/" \
  --json --baseline a.json
```

`--baseline` prints a delta per metric. `--fail-under` exits 14 when a named
metric falls below a floor, which is what turns a benchmark into a check a CI
job can run.

## Running against a real mirror

The mirrors this repository uses for real measurements are
`fosstorrents.com`, `dl-cdn.alpinelinux.org` and `geo.mirror.pkgbuild.com`.

Pass `--no-torrent-web-seed` when measuring one named mirror, or the torrent's
own `url-list` sources join in and the number is of the set rather than of the
one.

The failure matrix was run against all 468 web seeds in the Arch torrent, and
it found two defects that had made `webseed test` unusable against any HTTPS
mirror. A matrix that has never been run against a real mirror set is a matrix
that has not been tested.
