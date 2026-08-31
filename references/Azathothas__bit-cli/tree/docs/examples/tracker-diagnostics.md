# A tracker that will not answer

`bit-cli trackers` announces or scrapes for real, reports what every tracker
said, and exits on what it found. It is the command to reach for when a
download finds no peers and you do not yet know whose fault that is.

## A tracker that works

```bash
bit-cli trackers album.torrent --timeout 15s
```

```text
info hash            ae3ee993bf4fd886e98f15b899664c4d212085b2
name                 album
action               announce
announced port       6881
withdrawn from       1
trackers             1
responded            1
failed               0
seeders              0
leechers             1
left                 1543000 bytes
peers                0

TIER  TRACKER                          FAMILY  STATUS  RTT   SEED  LEECH  INTERVAL  PEERS  REASON
0     http://127.0.0.1:55146/announce  v4      ok      15ms  0     1      5s        0
```

Four columns are the diagnosis and the rest is context.

**`TIER`** is the BEP 12 tier the tracker is in. Tiers are announced in order,
not in parallel: a tracker in tier 1 is only asked when every tracker in tier 0
failed.

**`FAMILY`** is the address family the announce actually went over. A
dual-stack tracker records the source address of the connection it was
announced over, so a client reachable on one family and announced over the
other is invisible to half the swarm.

**`INTERVAL`** is what the tracker asked for. Announcing more often than this
is how an account gets rate limited.

**`withdrawn from`** counts the trackers this run sent a `stopped` event to on
the way out. A command that announces and does not withdraw leaves a phantom
peer in the swarm until the tracker times it out.

## A tracker that does not answer

```bash
bit-cli trackers album.torrent \
  --tracker "http://127.0.0.1:1/announce" --replace-trackers --timeout 5s
```

```text
responded            0
failed               1

TIER  TRACKER                      FAMILY  STATUS  RTT     SEED  LEECH  INTERVAL  PEERS  REASON
0     http://127.0.0.1:1/announce  v4      failed  2035ms  -     -      -         0      error sending request for url (http://127.0.0.1:1/announce?info_hash=%AE%3E%E9%93%BF...&peer_id=-CL0200-8um319nbelzj&port=6881&uploaded=0&downloaded=0&left=1543000&compact=1&no_peer_id=1&numwant=50&key=893be13d&event=started)
```

Exit **6**, no usable sources.

**The `REASON` column carries the whole request URL**, which is the point. A
tracker that answers `400` because it did not like a parameter, or one that
answers nothing because the host is unreachable, look identical from the
outside until the request is visible. With the URL you can run the same request
by hand.

`--replace-trackers` is what makes this a diagnostic rather than an experiment:
without it the torrent's own trackers are announced to as well, and a
successful one masks the failing one.

## Reading it from a script

```bash
bit-cli trackers album.torrent --json
```

Every row is in `trackers[]` with `ok`, `elapsed_ms`, `tier`, `protocol` and
the failure reason. The report's top-level counts are the highest figure any
tracker gave rather than a sum: trackers disagree constantly, and the highest
is the most informative single number with every tracker's own figure in the
table below it.

## Scrape instead of announce

```bash
bit-cli trackers album.torrent --scrape
```

A scrape asks for the counts without joining the swarm, so it does not create a
peer record and does not need withdrawing.

Only the BEP 48 convention is implemented, which derives the scrape URL by
replacing the last path segment `announce` with `scrape`. A tracker that uses
another convention reports as not supporting scrape, which is accurate: no
implementation in the thirty-nine tree corpus implements another one either.
`--scrape-url` overrides the derivation when you know the tracker's real one.

## What the peer id says about this command

The `REASON` line above carries `peer_id=-CL0200-...`. That is `bit-cli`'s own
Azureus-style identity, per BEP 20: `-CL` is this client, `0200` is version
0.2.0, and the twelve characters after it differ between runs.

`bit-cli trackers` and `bit-cli download` announce under the same eight bytes,
so a tracker's client statistics count both as one client. They did not until
T-236: `trackers` announced as `-BC0100-`, which libtorrent's table maps to
BitComet, and `download` announced as the vendored engine's `-rQ9010-`.
[`../peers.md`](../peers.md) is what the identity is now.

## When the tracker is fine and the download still finds nothing

Work down, in this order:

1. `bit-cli trackers <T>` says the tracker answered and how many peers it
   returned. Zero peers from a healthy tracker is a dead swarm, not a defect.
2. `bit-cli peers <T> --duration 30s` connects to what the tracker returned and
   reports what each peer did. See [`../peers.md`](../peers.md).
3. If peers are found and nothing transfers, the transport or the encryption
   setting is the next suspect: `scripts/check-transport.ps1` measures every
   combination and one of them is a known defect.
