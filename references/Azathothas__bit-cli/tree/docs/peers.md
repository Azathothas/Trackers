# Peers

What `bit-cli` reports about a swarm, what it does when the swarm goes away,
and what it cannot see.

The entries behind this are in [`TODO/peers.md`](../TODO/peers.md).

```bash
bit-cli peers album.torrent --duration 30s --sort speed:desc --json
```

Joins the swarm, watches for `--duration` or until `--count` distinct peers
have been seen, and reports every peer with the address, the state, the
direction, the bytes each way, the pieces it verified, and its mean piece
time. The client string and the connection type come from the peer's extended
handshake, so they are there while it is connected and gone once it is not.

It joins as a real member, so payload arrives. That is what makes
`--sort speed` mean anything: the rows are bytes that actually came from each
peer. What arrives goes to a temporary directory that the process removes when
it exits, and nothing is written where you are standing. Bound it with
`--duration`, `--count`, or `--max-download-rate`.

`--peer HOST:PORT` dials a known member whether or not anything else answers,
and with `--no-tracker --no-dht --no-lsd` the sample is exactly the members
named on the command line:

```bash
bit-cli peers album.torrent --peer 127.0.0.1:51413 \
  --no-tracker --no-dht --no-lsd --duration 5s --json
```

Exit 6 when nobody was seen, which is a real answer rather than a failure to
produce one, and a script tells the two apart by the code.

## When the port is open and nobody is answered

The stranded sockets are the visible half. The other half is worse: the same
accept loop clears **one** queued handshake check per connection it accepts, so
a run of peers that close before they handshake leaves a backlog, and every
peer that arrives afterwards waits behind it. Measured one for one: twenty such
connections, then single peers one at a time, and **the twentieth was the first
to be served**. Time cleared nothing; connections did, one each.

Nothing a supervisor normally watches sees that. The process is alive, the port
accepts, the log is silent, and the ratio in the report is history. So `seed`
can watch its own listener from the outside of the socket:

```bash
bit-cli seed release.torrent --seed-time 7d --listener-check 60s
```

Each check dials this run's own listen port over loopback and completes a real
handshake for a torrent it is serving. Three failures in a row stop the run
with `"stopped": "listener_unhealthy"` and exit 17. Three is derived rather
than picked: one failure means a backlog a real peer would have cleared by
arriving, and three means the backlog outlived three connections, so the next
three peers get nothing either.

The check is off by default and it is not free. A completed handshake is a peer
as far as the session is concerned, so each check leaves one peer row that
`librqbit` keeps and never reclaims: 24 checks, 24 rows, measured. Those rows
are dropped from `peer_detail` and from the report, by the loopback port the
check dialled from, the same way a web seed bridge's connection is told from a
swarm member. An unknown info hash would leave no row at all and is the wrong
measurement: it resolves to an error inside the session, which **adds** an
entry to the backlog it is measuring.

```bash
pwsh scripts/check-listener.ps1
```

## Downloading through an outage

A download whose peers all go away recovers when they come back, but not
immediately. A dropped peer is retried at about 10 seconds, then 70, then 430,
a factor of six each time, so an outage that ends between two attempts waits
for the next one however long the network has been back.

That matters for `--stop-timeout`, which is how long with no progress a run
waits before giving up with exit 9 and `"stopped": "stalled"`. Set shorter than
the next retry, it turns a recoverable outage into a failure. Measured: a 40
second outage is caught by the 70 second attempt and the download completes
byte for byte; a 120 second outage is not, and a run given 180 seconds of
patience still exits 9 because the next attempt was not due for another four
minutes.

```bash
pwsh scripts/check-peer-recovery.ps1
```

So pick `--stop-timeout` deliberately. For an unattended run that a supervisor
retries, short is right: fail in seconds and start again. For one that has to
finish on its own, leave it off or set it past ten minutes. The numbers are in
`TODO/peers.md` under T-021.

`--redial-after` stops the waiting instead of budgeting for it:

```bash
bit-cli download release.torrent \
  --peer 203.0.113.9:51413 \
  --stop-timeout 300s --redial-after 30s
```

After that long with no progress, the torrent is paused and started again. That
throws away every peer connection and the backoff counters behind them, then
dials `--peer` and the trackers from scratch. Piece state is kept and nothing is
re-hashed, so the cost is the live connections and not the disk.

Measured on the same 120 second outage: without it the run exits 9 with 17.00
MiB of 128 after 300 seconds of patience, and with it the run re-dials four
times and completes byte for byte, finishing 55.6 seconds after the peer came
back.

```bash
pwsh scripts/check-peer-recovery.ps1 -OutageSeconds 120 -StopTimeout 60 -PatientTimeout 300
```

It is off by default, and it should stay off for a healthy swarm: the trigger is
no progress at all, and a swarm where every peer is choking is exactly the case
where dropping every connection every thirty seconds can make things worse. Set
`--max-redials` to cap how many times it fires, ten by default. Each one is in
the report as `redials[]` and on the event stream as `peer_redial`, with how
long the run had been stalled and how many live connections it cost.

## Transport and encryption

`--transport tcp|utp|both` chooses the peer transport and defaults to `tcp`,
which is what every run did before the flag existed. `--encryption` chooses
message stream encryption.

One combination does not work today and it is recorded rather than hidden:
`--transport utp` with encryption on stalls after the handshake, which is
T-233 in [`TODO/peers.md`](../TODO/peers.md). `--transport utp --encryption
off` completes.

```bash
pwsh -NoProfile -File scripts/check-transport.ps1
```

## What bit-cli advertises about itself

**One identity, whatever the command.** The peer id is Azureus style, per BEP
20: `-CL`, then three characters of `bit-cli`'s own version, then the build
slot, then `-`, then twelve random printable characters. Version 0.2.0
announces as `-CL0200-`.

`CL` is this client's two character code. It is not in libtorrent's client
table, nor in any of the five other implementations of that table that were
checked, in either case: a tracker files the announce under an unknown client
rather than under somebody else's. That is the correct answer until the table
gains a row, and it is what the check below prints.

Every path uses it. `download`, `seed`, `trackers` and `bench probe` all
announce and hand shake under the same eight bytes, and the three loopback
connections this process makes to itself, the web seed bridge, the swarm
bench's synthetic peer and the listener health check, carry the same code with
a role in the version slot so a log says which of the three dialled.

```bash
pwsh -NoProfile -File scripts/check-announce.ps1
```
