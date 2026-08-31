# DHT and peer discovery

The entries behind this are in [`TODO/dht.md`](../TODO/dht.md).

## What is on by default, and how to turn it off

DHT, peer exchange and local service discovery are on unless the torrent is
private or a flag says otherwise. `--no-dht`, `--no-pex` and `--no-lsd` each
turn one off, and a private torrent under BEP 27 turns all three off by itself.

A run that must contact nothing but one named peer needs all three:

```bash
bit-cli download <TORRENT> --peer 127.0.0.1:6881 --no-dht --no-lsd --no-tracker
```

That is the shape every acceptance script in `scripts/` uses, so a measurement
is of the thing being measured rather than of whatever else the swarm supplied.

## A magnet with no DHT and no trackers

It fails, and it says so rather than hanging. `--init-timeout` bounds metadata
resolution on every path, and a run that cannot resolve exits non-zero naming
the phase.

## Resolving a magnet from one peer

Metadata comes over BEP 9 from any peer that has it, so a magnet resolves with
no tracker, no DHT and no web seed as long as one peer is reachable. Measured
against a loopback seeder: exit 0, the whole payload, and the metainfo pulled
from that one peer. The command is in
[`examples/multi-source.md`](examples/multi-source.md) and the run is recorded
under T-241 in [`../TODO/metainfo.md`](../TODO/metainfo.md).

## What is not reported yet

The DHT's own state, node count and bucket health are not in any report. That
is T-052 in [`TODO/dht.md`](../TODO/dht.md), open.
