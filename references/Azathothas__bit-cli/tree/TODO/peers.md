# Peer connections

Forty issues in the corpus touch peer handling: handshakes, connection churn,
connection limits, choke logic, and slow peers.

---

### T-020 Connections accumulate in CLOSE_WAIT until TCP is unusable

Source:      https://github.com/ikatson/rqbit/issues/311 (open)
Category:    peers
Priority:    P0
Effort:      L
Status:      **done**, 2026-08-22T14:47Z

Problem:     After about two days as a service, a reporter saw 20,000 sockets
             in CLOSE_WAIT and FIN_WAIT, which degraded TCP for the whole
             machine.
Relevance:   The netdisk deployment is exactly this shape: a long-lived process
             with many torrents. `bit-cli` is a one-shot foreground tool, which
             bounds the exposure to one invocation, but a `seed` run with
             `--seed-time 7d` is a two-day process.
Approach:    CLOSE_WAIT means the local side never called close after the peer
             sent FIN, so a task holding the socket is not being dropped.
             Reproduce with a long `bit-cli seed` run and a peer that connects
             and disconnects in a loop, watching `netstat -an` bucket counts.
             If it reproduces, the fix is upstream; carry a connection-count
             ceiling here in the meantime.
Acceptance:  A four-hour `bit-cli seed` run against a peer that reconnects
             every second ends with fewer than 100 sockets in CLOSE_WAIT,
             measured with `Get-NetTCPConnection -State CloseWait` and recorded
             here with the count at start, middle, and end.

**Reproduced, and it is two defects, not one. One is fixed here; the other is
upstream and open, with a ceiling carried here in the meantime.**

Time is not the variable, connections are, so the harness replaces four hours
with a burst.
`crates/bit-cli-core/examples/loopback-churn.rs` connects, optionally
handshakes, and closes, thousands of times.
`pwsh -NoProfile -File scripts/check-close-wait.ps1` drives it against a
seeder and counts the socket states at four moments.

```
mode         completed failed CW during CW after CW drained handles     listening panicked
handshake         2000      0         0        0          0 188 -> 228  yes       no
no-handshake      2000      0       986      986         92 188 -> 1210 yes       no
```

**Defect one: the accept loop panics and the listener silently dies.** Fixed.

`librqbit` 9.0.0's `task_listener` (`session.rs:970-1013`) is a
`tokio::select!` over two branches, both with preconditions. Accepting is
enabled only while the pending handshake-check set is under
`max_pending_incoming_handshake_checks`, and draining that set is enabled only
while it is not empty. A pending check that resolves to `Err` fails the second
branch's `Some(Ok(..))` pattern, which disables it for that iteration, and
when the set is at the cap the first branch is already disabled. Every branch
disabled panics:

```
thread 'tokio-rt-worker' panicked at librqbit-9.0.0/src/session.rs:980:13:
all branches are disabled and there is no else branch
```

A connection that closes before it handshakes is exactly that `Err`. Measured
at the 256 default: 3000 such connections at 64 at a time killed a seeder's
listener in 79 seconds, 2411 of the 3000 then failed to connect at all, and
**the process carried on reporting itself as seeding**. That is worse than a
leak, because nothing in the run says anything is wrong.

`bit-cli` sets `max_pending_incoming_handshake_checks` to `usize::MAX`
(`crates/bit-cli-core/src/engine.rs`, `PENDING_HANDSHAKE_CHECKS`). That is not
papering over it: it removes the branch that carries it, because the first
branch's precondition never goes false and the pair can never both be
disabled. The same flood now finishes in 8.8 seconds with 0 failures and the
listener alive.

**Defect two: a connection that closes before handshaking strands a socket.**
Open, upstream.

With the panic gone the residue is visible. About 0.5 stuck sockets per
no-handshake connection, and it accumulates:

| no-handshake connections | CLOSE_WAIT | handles |
| --- | --- | --- |
| 1000 | 560 | 772 |
| 2000 | 1075 | 1303 |
| 3000 | 1534 | 1776 |
| 4000 | 2053 | 2312 |
| then 100 ordinary connections | **96** | 2339 |

Three things that says:

- **Time releases nothing.** 986 stuck at the moment the churn stopped and 986
  after a 30 second settle. An earlier run held 545 flat for 107 seconds.
- **Ordinary traffic releases almost all of it.** 100 handshaked connections
  took 2053 down to 96. So it is a queue the accept loop only drains inside
  the same `select!` that accepts, not a leak: an idle seeder holds whatever
  the last burst left, and a busy one clears it.
- **A completed handshake strands nothing.** 25,000 handshaked connect and
  close cycles left the seeder holding exactly one socket, its listener, with
  the handle count flat at 228 from 10,000 onward. A handshake for an info
  hash the seeder does not have strands about 6% of the time, so the failing
  read is where nearly all of it is.

The reporter's twenty thousand after two days is this: about forty thousand
connections that never handshaked, against a seeder with too little other
traffic to drain them. Closing it means the accept loop draining its pending
set to empty rather than one item per iteration, which is upstream.

**What is carried here: `--max-handles <N>`.** Off by default. Sampled once
per `--report-interval`, and when the process holds more than that many
handles the run stops with `"stopped": "handle_ceiling"` and exit 16. It does
not close a socket. What it does is turn an unbounded stranding inside a
`seed --seed-time 7d` into a loud exit that a supervisor restarts, which is
what the Approach above asks for.

```
$ bit-cli seed t.torrent --dir . --port 0 --seed-time 30s --max-handles 50 --json
exit=16
  "open_handles": 187,
  "stopped": "handle_ceiling",
```

Status stays **open**: the stranding is not fixed, and `scripts/check-close-wait.ps1
-Ceiling 100`, which is this entry's acceptance as written, still fails. What
the script does assert, and what will now fail the run, is the listener
surviving, so defect one cannot come back unnoticed.


**What the soak adds, 2026-08-20, extended 2026-08-21.** `CLOSE_WAIT` is
**zero at every one of 1,064 samples** across a 4.605 hour `steady` run and a
4.617 hour `idle` one, with handles flat in both and exactly 189 at every
`idle` sample. So this needs the churn shape: connections that close before
they handshake. A seeder under a deployment-shaped load, with real downloads
and a tracker announcing, strands nothing over four and a half hours. See
[T-040](memory.md) for the runs.

**The stranding also stops the target serving, 2026-08-21.** This was known as
a socket count. It is worse than that: while the pending set is full the target
**cannot complete a handshake for any info hash, including one it is
serving**, and it goes on reporting itself as seeding. Found by
[T-092](bench.md)'s acceptance, which used one seeder for every case and read
as a broken handshake in `bench swarm` until the order was changed.

Three runs against one `bit-cli seed`, from
`bench/swarm-20260821T063418798Z.json`, case `listener_poisoned`:

| step | connected | handshaked | bytes |
| --- | --- | --- | --- |
| `bench swarm <T> --for p.torrent --peers 1` | 1 | 1 | 8,388,608 |
| `bench swarm <T> --peers 100 --torrents 4` | 100 | 0 | 0 |
| `bench swarm <T> --for p.torrent --peers 1` | 1 | **0** | **0** |

99 of the 100 ended in `handshake_timeout` and one in
`closed_before_handshake`, and `seeder_still_alive` is true at the end. So the
target accepts the TCP connection, answers no handshake, and never says so.

That changes what this entry costs. A stranded socket is a resource; a
listener that accepts and never answers is an outage that no health check
looking at the process, the port, or the log will see. The `--max-handles`
ceiling carried here was the only mitigation when this was written, and it now
has a second reason to exist. The check below is the second mitigation.

Reproduce:

```powershell
pwsh -NoProfile -File scripts/check-swarm.ps1
```

Case `listener_poisoned`, which carries `judged: false` because this entry is
open and an acceptance script does not fail the build for a defect that is
already recorded.

**The mechanism, and the sentence above it is wrong, 2026-08-22.** The line
"while the pending set is full the target cannot complete a handshake" names
the wrong cause. The set is never full: `PENDING_HANDSHAKE_CHECKS` is
`usize::MAX`, which is what removed defect one. The cap has nothing to do
with it, and a reader who fixed the cap would have fixed nothing.

What it is is the **drain rate**, which is one entry per accepted connection.
`task_listener`'s second `select!` arm is
`Some(Ok((live, checked))) = futs.next(), if !futs.is_empty()`
(`session.rs:1005`, the same file as defect one). A check that resolves to
`Err` fails that pattern, so `tokio::select!` disables the arm for the rest of
that call and waits on `l.accept()`, which on an idle seeder is forever. The
loop cannot come round again, and nothing in `futs` is polled until the next
connection arrives. A check that resolves to `Ok` matches, so it ends an iteration
without consuming an accept and the queued successes drain for free.

Measured, and it is one for one. Twenty connections that handshaked for an
info hash the seeder does not have, then single peers one at a time for a
torrent it does: **the twentieth got a handshake and the nineteen before it
got nothing.** `bench/listener-20260822T045550230Z.json`, case `recovery`,
`connections_to_recover` 20 against `poison_connections` 20, with a peer
served before the load to prove the seeder was working. An earlier run of the
same shape recovered on the thirteenth, and the difference is the load's own
duration: eight of that twenty ended in `closed_before_handshake`, which is
the target having already got to them. Nothing recovers on a timer. What
clears the queue is connections, one each.

**A peer row is kept for every completed handshake, and never reclaimed.**
Twenty-four handshake-and-close connections from loopback left twenty-four
rows, `live 0` and `dead 0` at every sample, all in `not needed`. That is not
a T-020 defect on its own, but it decides the shape of anything that watches
the listener by handshaking with it, and it is a candidate for the linear
slope [T-040](memory.md) is attributing.

**What is carried here, second: `--listener-check <DUR>`.** Off by default,
and on `seed` only. Not on `download`, and that is a decision rather than an
omission: the probe watches one listener, and a `-j` run has one session behind
several watch loops, so the flag would either probe once per torrent per
interval or need somewhere above the loop to live. A `download --seed-time 7d`
is the shape that would want it, and it can have it when the flag has a second
caller asking. The reason is on the `listener: None` line in
`crates/bit-cli/src/cmd/download.rs` as well, so it is not only here. Every interval it dials this run's own listen port over loopback
and completes a real handshake for a torrent the run is serving. Three
failures in a row stop the run with `"stopped": "listener_unhealthy"` and exit
17.

From the acceptance's `poisoned` case, which is a seeder given
`--listener-check 2s` and then the twenty connection load:

```
exit=17
  "stopped": "listener_unhealthy",
  "listener": { "probes": 6, "failed": 3, "consecutive_failures": 3,
                "last_failure": "handshake_timeout" }
```

Three is derived from the drain rate above rather than picked. One failure
means a backlog of at least one, which a real peer clears for itself by
arriving; three means the backlog outlived three connections, so the next
three peers get nothing either.

The probe uses a real info hash rather than an unknown one, and that costs
something. An unknown hash is rejected before the session records a peer, so
it would leave no row. It is also the wrong measurement: it resolves to `Err`,
so it **adds** an entry to the backlog it is measuring, and a backlog of one
becomes a backlog of two while the probe reports an outage on a listener that
a real peer would have got through. A completed handshake takes an entry off
instead. What it costs is one peer row per probe, and those rows
are dropped from `peer_detail` and from the report by the loopback port the
probe dialled from, which is the mechanism the web seed bridge already uses.
They come out of `peers.seen` and `peers.live` too, because `seed` exits 14
when it stopped idle having seen no peer and `--exit-when-idle` measures how
long it has had none live, and a probe a minute would answer both wrong.

Acceptance, four cases, and `recovery` is the drain-rate measurement above:

```powershell
pwsh -NoProfile -File scripts/check-listener.ps1
```

```
case      probes failed consecutive exit stopped             other
healthy        3      0           0    -  -                  peer_rows 0, peers_seen 0, rtt 15 ms
poisoned       6      3           3   17  listener_unhealthy last_failure handshake_timeout
off            -      -           -    9  deadline           no listener key at all
recovery       -      -           -    -  -                  20 connections cleared a 20 backlog
```

Status stays **open**, for the same reason as above and now for a second
mitigation rather than one. Nothing here drains the queue for a peer that is
not us, `scripts/check-close-wait.ps1 -Ceiling 100` still fails, and the fix is
still the accept loop draining its pending set to empty. What has changed is
that the outage is now loud: a supervisor gets exit 17 instead of a process
that reports a ratio and serves nobody.

**Closed 2026-08-22T14:47Z, by one match arm in the vendored tree.**

The mechanism the section above names is the whole defect, and the fix is what
that section says it would be. `task_listener`'s second `select!` arm matched
`Some(Ok((live, checked)))`. A `select!` arm whose pattern fails is disabled
for the rest of that call, so a handshake check resolving to `Err` left the
loop waiting on `l.accept()` alone. The arm now binds the whole result and
handles it inside, so no outcome can disable it. `patches/UPSTREAM.md` under
"librqbit: one failed handshake check stops the accept loop draining" carries
the diff and the reason.

**This entry's own acceptance, as written, and it had never passed:**

```
$ pwsh -NoProfile -File scripts/check-close-wait.ps1 -Ceiling 100

mode         completed failed CW during CW after CW drained handles    listening panicked ok
handshake         2000      0         0        0          0 188 -> 226 yes       no       yes
no-handshake      2000      0         0        0          0 188 -> 194 yes       no       yes

verdict: both modes ended under 100 stuck sockets with the listener alive
```

Against what the same command measured before, in the table at the top of this
entry:

| | before | after |
| --- | --- | --- |
| `no-handshake`, CLOSE_WAIT while the churn ran | 986 | **0** |
| `no-handshake`, CLOSE_WAIT after a 30 s settle | 986 | **0** |
| `no-handshake`, handles | 188 to 1210 | **188 to 194** |
| 4,000 connections, CLOSE_WAIT | 2,053 | not reproduced |

`bench/close-wait-20260822T144628230Z.json` is the run.

**And the outage, which was the worse half.** The stranding stopped the target
serving anything at all, for any info hash, while it went on reporting itself
as seeding.

```
$ pwsh -NoProfile -File scripts/check-listener.ps1
   1 connections cleared a 20 connection backlog
verdict: pass
```

| | before | after |
| --- | --- | --- |
| connections to clear a 20 connection backlog | 20 | **1** |
| probes / failed under the same load | 6 / 3 | **13 / 0** |
| the seeder under that load | exit 17, `listener_unhealthy` | still serving |

`bench/listener-20260822T144737688Z.json`.

**Three of `check-listener.ps1`'s four cases asserted the defect**, so they are
inverted rather than deleted and now hold the fix: `poisoned`, which required
exit 17, is `survives_load` and requires the run to carry on with the listener
healthy; `recovery` required more than one connection to clear the backlog and
now requires exactly one. `check-swarm.ps1`'s `listener_poisoned` case carried
`judged: false` because this entry was open, and is judged now. Both changes
mean the defect cannot come back unnoticed, which is what the cases are for.

**What that costs, said plainly.** The old `poisoned` case was the only
end-to-end proof that `--listener-check` can stop a real run with exit 17, and
there is no longer a way to poison a listener to produce one. The decision
behind the exit is covered by three unit tests in `crates/bit-cli/src/swarm.rs`:
`one_unanswered_probe_does_not_stop_a_seeder`,
`three_unanswered_probes_in_a_row_stop_the_run`, and
`an_answered_probe_clears_the_run_of_failures_before_it`. What is no longer
covered anywhere is the wiring between a real seeder's probe and a real exit.

**The two backstops carried here stay, and one of them needs its reasoning
rewritten.** `--max-handles` and `--listener-check` are both still off by
default and both still do what they did. But the threshold of three was
**derived from the drain rate**: "one failure means a backlog of at least one,
which a real peer clears for itself by arriving; three means the backlog
outlived three connections". There is no backlog now, so that derivation is
gone. Three is still the right number for a different reason, which is that a
single probe can time out on a loaded machine without the listener being
unreachable, and it is no longer measured. Said here rather than left as a
number whose stated justification no longer holds.

**What is not fixed.** A peer row is still kept for every completed handshake
and never reclaimed, which the section above notes and which belongs to
[T-040](memory.md). The `handshake` mode's 188 to 226 handles is that, not this.

### T-021 A temporary network drop stops the download permanently

Source:      https://github.com/ikatson/rqbit/issues/363 (open)
Category:    peers
Priority:    P0
Effort:      M
Status:      **done**

Problem:     Disabling and re-enabling a network adapter mid-download drops the
             rate to zero and it never recovers, even after the adapter is
             back.
Relevance:   This is the failure that makes an unattended download useless. A
             cron job that starts a 40 GB download and comes back to a stalled
             process at 60 percent has failed silently.
Approach:    `bit-cli` covers the symptom, not the cause: `--stop-timeout`
             turns a permanent stall into exit 9 with the stall named, so a
             caller can retry rather than wait forever. The cause is that
             `librqbit` does not re-announce or re-dial after every peer dies.
             Reproduce on Windows with `Disable-NetAdapter`, then decide
             whether a retry belongs in `bit-cli` (re-add the torrent to a
             fresh session and resume) or upstream.
Acceptance:  `bit-cli download <TORRENT> --stop-timeout 60s` through a
             two-minute adapter outage either recovers and completes, or exits
             9 within 60 seconds of the stall with `"stopped": "stalled"`.
             Whichever it does is recorded here with the timeline.

**It does both, and which one depends on a number nobody had looked at.**

The adapter is not the variable and cannot be touched here anyway: disabling
one is a change to the machine. What the client sees is the same either way,
every peer connection dying at once and nothing reachable for a while, so the
outage is the seeder being killed and restarted on the same port.
`pwsh -NoProfile -File scripts/check-peer-recovery.ps1` does that, twice:

```
scenario  stop-timeout exit stopped   downloaded hash    gave up after
patient   120s            0 completed 128.00 MiB matches -
impatient 20s             9 stalled   17.00 MiB  -       19.4s
```

`--stop-timeout 20s` against a 40 second outage exits 9 with `"stopped":
"stalled"` **19.4 seconds after the cut**, which is the acceptance's second
branch and inside the timeout it was given. Left alone for longer, the same
download re-dials the peer and completes with the payload hashing equal, which
is the first branch.

**What decides which is `librqbit`'s peer reconnect backoff, and it is steep.**
`torrent_state/live/peer/stats/atomic.rs:52`: 10 second minimum, **factor 6**,
one hour maximum. So a peer that drops is retried at roughly 10s, 70s, 430s,
and then 36 minutes. An outage that ends between two of those attempts waits
for the next one, however long the network has been back.

That is what makes the entry's own two-minute case look like "never recovers".
Measured directly: a 120 second outage with `--stop-timeout 180s` had the
seeder back at t+129s and the download still sat at 17.00 MiB until its stall
timeout fired at t+189s, because the next attempt was not due until t+438s. The
same shape with a 40 second outage is caught by the 70 second attempt and
completes.

So the report is accurate as an observation and wrong as a diagnosis. Nothing
is stuck. The client is waiting, and the wait grows by six every time.

**What `bit-cli` does about it.** The backoff is not reachable: it is built in
`pub(crate)` code from constants, `SessionOptions` does not carry it, and
`add_peer_if_not_seen` is `pub(crate)` and refuses a peer it has already seen,
so there is no public route to force a re-dial either. What is reachable is
saying so, and `--stop-timeout` already does: a run that cannot continue exits
9 and names the stall, which is what lets an unattended caller retry rather
than wait. `README.md` now states the interaction under "Seeding for days",
because a `--stop-timeout` shorter than the next backoff attempt turns a
recoverable outage into a failure, and that is a choice a caller has to make
deliberately rather than discover.

The residue, forcing a re-dial rather than waiting out the backoff, is
[T-138](#t-138-a-peer-that-comes-back-waits-out-a-backoff-that-grows-by-six),
and it is now **done**. `--redial-after 30s` finishes the same 120 second
outage this entry could not, in four re-dials. The paragraph above stays as
written because it is what happens with the flag off, which is the default.

### T-138 A peer that comes back waits out a backoff that grows by six

Source:      came out of closing T-021
Category:    peers
Priority:    P2
Effort:      M
Status:      **done**

Problem:     `librqbit`'s peer reconnect backoff is 10 seconds minimum with a
             factor of 6, so attempts land at about 10s, 70s, 430s, and then
             36 minutes. A peer that comes back one second after an attempt
             fails is not tried again for six times as long as the last wait.
             On a swarm of one peer, which is what `--peer` builds and what a
             private tracker often is, that is the difference between a
             download finishing and a download timing out.
Relevance:   [T-021](#t-021-a-temporary-network-drop-stops-the-download-permanently)
             measured it: a 120 second outage with the peer back at t+129s left
             the run at 17 of 128 MiB until its stall timeout fired, because
             the next attempt was due at t+438s.
Approach:    Three, none of them free.

             1. **Re-add the torrent on a stall.** `bit-cli` already knows the
                source, the output directory, and the peer list. On a stall it
                could remove the torrent from the session and add it again,
                which resets peer state and re-dials `initial_peers`. The hash
                check on add makes it safe and is what makes it expensive: a
                full read of the payload every time. Bounded by only doing it
                once per stall and by a cap on how many times.
             2. **A second session.** Heavier, same shape, no advantage.
             3. **Reach the backoff.** It is four constants in `pub(crate)`
                code. Making it configurable is the small change upstream and
                the one that fixes it properly, and it is the same fork
                question [T-002](webseed.md) priced.
Acceptance:  A 120 second outage with `--stop-timeout 300s` completes, and the
             report says how long the run waited and how many times it
             re-dialled. Today the same run exits 9 at t+189s with 17.00 MiB
             of 128, recorded under
             [T-021](#t-021-a-temporary-network-drop-stops-the-download-permanently).

**Option 1, and it turned out to cost nothing rather than a hash check.**

The entry priced option 1 as "remove the torrent and add it again", with a full
read of the payload every time. That is not what is needed. `librqbit` 9.0.0
exports `Session::pause` and `Session::unpause`, and the pair does exactly the
job:

- `ManagedTorrent::pause` on a live torrent calls `TorrentStateLive::pause`,
  which takes the piece tracker out and hands back a `TorrentStatePaused`
  holding the chunk tracker (`torrent_state/live/mod.rs:767`). The peer map and
  its backoff counters live in `TorrentStateLive` and are dropped with it.
- `Session::unpause` calls `make_peer_rx_managed_torrent(handle, true)`, which
  rebuilds the peer stream from `initial_peers`, the trackers, the DHT, and
  LSD, then `start`s the torrent (`session.rs:1511` and `session.rs:1610`).
- `Paused` to `Live` is a direct transition. Only a fresh add or an error goes
  through `Initializing`, which is the state that hash checks. So no payload is
  re-read.

So the cost is the live connections, not the disk. Option 3, reaching the
backoff constants, is still the change that fixes it at the source, and it is
still the fork question [T-002](webseed.md) priced. It is not needed for this.

**`--redial-after <DUR>`, off by default, with `--max-redials <N>` at 10.**

`bit_cli_core::engine::Engine::redial` is the pause and unpause pair.
`cmd::download::watch` calls it when the byte count has been flat for
`--redial-after` and the last re-dial was at least that long ago, checked after
the stop conditions so a run that was going to give up this tick does. Every
re-dial goes into the report as `redials[]` with the attempt number, the
milliseconds into the run, how long the run had been stalled, and how many live
peer connections it threw away, and out as a `peer_redial` event under
`--jsonl`.

Off by default because the trigger is a stall and the cost is every live
connection: a swarm where one peer is slow and the rest are working is not a
stall, but a swarm where every peer is choking is, and tearing that down every
thirty seconds is a way to make it worse. A caller who wants an unattended run
to survive an outage says how long to wait first. `bit-cli` warns when
`--redial-after` is not shorter than `--stop-timeout`, because in that order the
run gives up before it ever re-dials.

**The measurement, 2026-08-20T13:01:50.325Z**, in
`bench/peer-recovery-20260820T130150325Z.json`. Three scenarios, the first two
and the third differing in exactly one flag:

```
$ pwsh -NoProfile -File scripts/check-peer-recovery.ps1 \
    -OutageSeconds 120 -StopTimeout 60 -PatientTimeout 300 -RedialAfter 30
```

```
scenario  stop-timeout redial-after exit stopped   downloaded hash    re-dials
patient   300s         off             9 stalled   17.00 MiB  -              0
impatient 60s          off             9 stalled   17.00 MiB  -              0
redial    300s         30s             0 completed 128.00 MiB matches        4
```

`patient` is the acceptance's "today" line reproduced: 300 seconds of patience
against a 120 second outage, and it still exits 9 with 17.00 MiB of 128.
`redial` is the same run with `--redial-after 30s` and it completes with the
payload hashing equal.

The four re-dials, from the report:

| attempt | at | stalled for | peers dropped |
| --- | --- | --- | --- |
| 1 | t+38.2s | 30.1s | 0 |
| 2 | t+68.3s | 60.3s | 0 |
| 3 | t+98.4s | 90.4s | 0 |
| 4 | t+128.5s | 120.5s | 0 |

The seeder was cut at t+9.0s and came back at t+129.4s. The run finished at
t+185.0s, which is 55.6s after the peer returned and is what 111 MiB of 128 at
`--max-download-rate 2MiB/s` takes. So it resumed as soon as there was
something to resume from.

**What actually recovers it is the reset, not the re-dial.** The fourth
re-dial at t+128.5s was still during the outage, one second before the seeder
was back, so its own dial failed like the three before it. What it left behind
was a fresh `TorrentStateLive` whose backoff was back at its 10 second minimum,
so the next automatic attempt was due at about t+138.5s rather than at t+438s.
That is the whole mechanism: the flag does not have to land on the moment the
network returns, it only has to keep the wait bounded by `--redial-after` plus
10 seconds instead of letting it multiply by six.

`peers_dropped` is 0 on all four because there was nothing live to drop during
an outage. It is in the report for the case where a re-dial fires against a
swarm that is connected but not moving, which is where the cost is real.

`pwsh -NoProfile -File scripts/check-peer-recovery.ps1` is the acceptance and
now drives all three scenarios. `patient` is failed only when the outage is
inside the backoff's second attempt at about 70 seconds; past that its stalling
is what [T-021](#t-021-a-temporary-network-drop-stops-the-download-permanently)
recorded, and failing the build for it would fail the build for behaviour that
is documented. `redial` is failed whenever it does not complete, and also when
it completes without re-dialling at all, because a scenario the flag did not
change proves nothing.

Two unit tests cover the plumbing without a network:
`a_stalled_run_redials_up_to_the_cap_and_reports_each_one` holds `--max-redials`
to its cap and checks the interval between attempts, and
`a_stalled_run_without_the_flag_never_redials` checks that the report says
nothing when the flag is off.

### T-022 Peer connections churn on IPv6-only swarms

Source:      https://github.com/ikatson/rqbit/issues/537 (open)
Category:    peers
Priority:    P1
Effort:      M
Status:      **done**, 2026-08-22T17:26Z

Problem:     A session bound to `[::]` announces one address to the tracker.
             On a dual-stack host that means IPv4 peers may never learn a
             reachable address, so they connect, fail, and retry.
Relevance:   `bit-cli` binds `[::]` by default and relies on `librqbit`
             clearing `IPV6_V6ONLY` for a genuine dual-stack socket, which it
             does. The announce side is separate and still single-address.
Approach:    `bit-cli`'s own tracker client (`crates/bit-cli-core/src/tracker.rs`)
             announces one port and lets the tracker take the source address,
             which is right for one family at a time. Announcing both families
             needs two announces, one over each. Decide whether `bit-cli
             trackers` should do that, and whether the session should too.
Acceptance:  `bit-cli trackers <TORRENT> --json` on a dual-stack host reports
             the peers each family's announce returned, separately.

**The decision the Approach asks for, taken 2026-08-22 in an unattended
session.** `bit-cli trackers` announces once per family. It is a diagnostic
whose whole job is to report what a tracker said, and "which of my addresses
did this tracker take" is the question this entry is about. The session is a
separate answer and is below.

**Half of the Approach's premise is wrong, and the pinned dependency is where
to read it.** "Announcing both families needs two announces, one over each.
Decide whether the session should too" reads as though the session announces
once. For **UDP trackers it already announces twice**:
`librqbit-tracker-comms-9.0.0/src/tracker_comms.rs:374-387` resolves the first
IPv4 and the first IPv6 address into `UdpTrackerResolveResult::Two(v4, v6)` and
fires both with `tokio::join!`. For **HTTP trackers it announces once**:
`tracker_comms.rs:293` is a single `reqwest` GET and the family is whatever the
connector picks. So the session half is already done for UDP and is blocked on
`librqbit` for HTTP, at that line. That is the pinned dependency `bit-cli`
actually runs rather than a corpus tree, so it is evidence about `bit-cli`.

**What `bit-cli trackers` did before this, which was worse than either.**
`udp_target` took `to_socket_addrs().next()`, the first address the resolver
happened to return. On a dual-stack host that is not a choice, it is an
ordering, and it can differ between two runs against the same tracker.

**Built.** `Client::announce_on` takes a family, `announce` keeps the old
behaviour by passing `None`, and `bit-cli trackers` grows `--family` with
`auto`, `v4` and `v6`. `auto` resolves the tracker and announces once per
family it has an address in.

- **UDP** filters the resolution to the family and binds the local socket to
  match.
- **HTTP** overrides the resolution. `ClientBuilder::local_address` does **not**
  pin a family, which is worth recording because it is the obvious thing to
  reach for: `hyper-util-0.1.20/src/client/legacy/connect/http.rs:794-820`
  binds the local address only when it already matches the destination's family
  and otherwise falls through to the unspecified address **of the
  destination's own family**, so setting `0.0.0.0` still connects over IPv6.
  `resolve_to_addrs`, with the host resolved and filtered here, is what works,
  because then there is no address of the other family left to choose.
- The announced port is bound on **both** families now. It was IPv4 only, and
  an IPv6 announce naming a port listening only on IPv4 registers exactly the
  black hole [T-061](trackers.md) added that listener to prevent. Two separate
  listeners rather than one dual-stack socket, for
  [T-023](#t-023-the-listen-port-is-chosen-without-checking-both-address-families)'s
  reason.
- `stopped` goes out over the family the announce that succeeded used. Sent
  over the other one it names a different source address and leaves the record
  it meant to remove.

**One tracker's two announces go in sequence, and finding out why is the
measurement worth keeping.** They were concurrent first. `loopback-tracker`
keyed its peer records by peer id alone, as a plain BEP 3 tracker does, so the
second announce **overwrote** the first and one peer announcing over both
families ended up with a single record. Which family survived was whichever
announce landed last, measured at `127.0.0.1:7100` with no `[::1]:7100`:

```
one peer announces over both families on port 7100 and stays
a second peer asks what the swarm holds:
  peers: 127.0.0.1:7100
  count: 1
```

So two announces is what it takes to **tell** a tracker about both addresses,
and whether it **keeps** both is the tracker's choice. That is the whole reason
BEP 7 exists. `loopback-tracker` keys by `(peer id, family)` now, which is what
a tracker holding BEP 7's peer lists does, and it answers with `peers6` beside
`peers`. The same measurement then reads:

```
  peers: 127.0.0.1:7100, [::1]:7100
  count: 2
```

That is the entry's Problem, gone: one host, both addresses registered, and an
IPv4 peer learns a reachable one. Sequencing the two announces also makes the
outcome deterministic against the other kind of tracker, where the last family
in the list wins every time instead of a race deciding it.

**What the two families return is usually the same list, and reporting them
apart is still right.** Measured against the fixture: a peer announcing over
both is told about the same two peers either way, because what the family
decides is what the tracker records **about the announcer**, not what it hands
back. Trackers that answer only same-family peers are common enough that the
report should be able to show it, and this is what shows it.

Acceptance, run 2026-08-22 against `loopback-tracker` bound on `127.0.0.1` and
`[::1]` at one port, announcing to `http://localhost:<port>/announce` so both
families resolve:

```
=== auto (exit 0) ===
trackers=1 announces=2 responded=1 failed=0
  family v4: announces=1 responded=1
  family v6: announces=1 responded=1
  http://localhost:53414/announce family=v4 endpoint=127.0.0.1:53414 ok=True
  http://localhost:53414/announce family=v6 endpoint=[::1]:53414 ok=True
```

and the tracker's own log, which is the other side of it:

```
08:08:54.895Z announce ... from=127.0.0.1 family=ipv4 port=6881 event=started
08:08:54.907Z announce ... from=::1       family=ipv6 port=6881 event=started
08:08:54.917Z announce ... from=127.0.0.1 family=ipv4 port=6881 event=stopped
08:08:54.930Z announce ... from=::1       family=ipv6 port=6881 event=stopped
```

`--family v4` and `--family v6` each send one announce and name the endpoint,
and a family a tracker has no address in fails with the family named rather
than falling back to the other one, which would publish an address the caller
did not ask to publish.

**Closed 2026-08-22 in the vendored tree, which is what the paragraph below
said would be needed.** It said the session's half was "upstream's to make".
The trees were vendored the same day, so it was made here.

`vendor/rqbit/crates/tracker_comms/src/tracker_comms.rs` now resolves an HTTP
tracker the same way it already resolved a UDP one, keeps a `reqwest` client
per address family with the resolution pinned, and announces once over each in
sequence. `librqbit`'s session hands it a factory that rebuilds the session's
own client, so the proxy, the bound interface and the user agent are configured
in one place; behind a proxy it hands `None` and nothing changes, because the
proxy resolves and the local family is not ours to choose.
[`patches/UPSTREAM.md`](../patches/UPSTREAM.md) carries the full section.

**Measured, one `bit-cli seed` against `loopback-tracker` on both loopback
addresses at one port.** The tracker logs the source address of every announce,
which is the thing a tracker actually records about a peer:

| case | tracker URL | before | after |
| --- | --- | --- | --- |
| `dual_host` | `http://localhost:<port>/announce` | **ipv6 only**, from `::1` | **ipv4 from 127.0.0.1 and ipv6 from ::1** |
| `literal_host` | `http://127.0.0.1:<port>/announce` | ipv4, from `127.0.0.1` | ipv4, from `127.0.0.1` |

```bash
pwsh -NoProfile -File scripts/check-tracker-family.ps1
```

`bench/tracker-family-20260822T172231576Z.json` is the before, taken with the
two vendored files stashed and the tree rebuilt, and
`bench/tracker-family-20260822T172549738Z.json` is the after.

**Which family the old code picked was not a choice.** The before run says
`ipv6`, and nothing in `bit-cli` asked for that: it is the order the resolver
returned addresses in. An IPv4-only peer reading that tracker got no address it
could dial, which is this entry's Problem exactly.

**`literal_host` is the control and it has to keep passing.** A URL naming an
address has no resolution to override, so that case takes the fallback path,
which is the old code, and one announce there is correct. A check that reported
two families for both cases would be reporting that something announces twice
regardless.

**What is still one announce, deliberately.** A tracker whose host resolves in
one family only, a tracker named by address, and a session behind a proxy. Each
falls back to the client the session built, so none of them is a new path.

### T-023 The listen port is chosen without checking both address families

Source:      carried from the first session
Category:    peers
Priority:    P1
Effort:      S
Status:      done

Problem:     Probing a candidate port by binding `[::]` alone says nothing
             about IPv4 on Windows, where the standard library leaves
             `IPV6_V6ONLY` on. A port free on IPv6 and taken on IPv4 was
             reported free, and the dual-stack bind `librqbit` then makes fails.
Relevance:   It cost the whole session, not the port.
Approach:    `engine::choose_listen_addr` now requires a port to be free on
             both families before choosing it for a dual-stack listener, falls
             back to a single family with a warning naming which, and then to
             an OS-assigned port. The probe is injected, so the tests describe
             which ports are taken rather than binding sockets.
Acceptance:  `cargo test -p bit-cli-core engine::tests` passes, including
             `a_port_taken_on_ipv4_alone_is_not_chosen_for_a_dual_stack_listener`.
             Done: 2026-08-19.

### T-024 Per-peer choke and unchoke history is not reported

Source:      the operator's brief
Category:    peers
Priority:    P2
Effort:      M
Status:      **done**, 2026-08-23T05:19Z

Problem:     `bit-cli seed --json` reports per-peer address, client, direction,
             bytes in each direction, verified pieces, chunks, errors, and
             connect time. It does not report choke and unchoke events or a
             disconnect reason, because `librqbit`'s `PeerStats` snapshot does
             not carry them.
Relevance:   A3.4b names both. Without them "why did this peer stop taking
             bytes" has no answer in the report.
Approach:    `PeerCounters` in `librqbit` carries `times_stolen_from_me` and
             `times_i_stole` but no choke history and no disconnect cause. Add
             them upstream, or infer disconnects from a peer leaving the
             snapshot between two ticks and record that as the weaker answer it
             is.
Acceptance:  `bit-cli seed --json` carries a `disconnects` array per peer with
             a timestamp and a reason, and the reason is a real one rather than
             "gone".

**Closed 2026-08-23, and the Approach's second option was not needed.** It said
to add the counters upstream "or infer disconnects from a peer leaving the
snapshot between two ticks and record that as the weaker answer it is". The
trees are vendored, so the first option was available, and the weaker answer
was not taken.

**The reason was already in hand and was being thrown away.**
`on_peer_died(&self, error: Option<crate::Error>)` had it, set the state to
`Dead`, and dropped it. So a report could say a peer was `dead`, which is a
fact about the row rather than about what happened.

- **`peers[].disconnects`**, newest last, each with `at` in ISO 8601 UTC and
  `reason`. Bounded at four per peer: a flapping peer produces one per flap and
  the session keeps 1,024 peer rows, so this is the second factor in a product
  that has to stay small, and the reason is truncated at 200 bytes because an
  `anyhow` chain can be a paragraph.
- **`peers[].choked` and `peers[].unchoked`**, counted in `on_i_am_choked` and
  `on_i_am_unchoked`. A peer that chokes goes quiet and looks exactly like one
  that is slow; these are the two numbers that tell them apart.

**A connection the peer closed cleanly reports `closed by the peer`** rather
than an empty string. That is a real reason and it is a different fact from a
reason nobody recorded, which is why `librqbit` keeps it as `None` and this
crate names it.

**Measured, and the reason is the one the read actually failed with.**
`a_peer_that_leaves_is_reported_with_a_reason_and_a_time` in
`crates/bit-cli/src/cmd/seed.rs`: a raw socket completes a BEP 3 handshake
against a running `bit-cli seed --json` and closes.

```json
{"at":"2026-08-23T05:17:45.446Z","reason":"error writing: An established connection was aborted by the software in your host machine. (os error 10053)"}
```

The info hash comes from `bit-cli info --json` rather than from a literal, so a
fixture change cannot leave the test handshaking for the wrong torrent, and the
peer thread returns whether it completed the exchange so a run where nothing
connected fails as that rather than as a missing field.

**What this does not carry.** A choke **history** with timestamps, which the
title asks for and the Acceptance does not. Two counters answer "did this peer
choke us, and how often"; they do not answer "when". Nothing here needed the
second, and adding a bounded event list per peer for it is the same shape as
`disconnects` if something ever does. Said here rather than left as a title
that promises more than the entry delivered.

### T-025 PeerStatsFilterState is not exported, so the filter is built by JSON

Source:      `librqbit` 9.0.0 API gap
Category:    peers
Priority:    P3
Effort:      S
Status:      **done**, 2026-08-22T19:38Z

Problem:     `librqbit` exports `PeerStatsFilter` through `http_api_types` but
             not the enum its one field holds, so the value that asks for every
             peer rather than only the connected ones cannot be named in Rust.
Relevance:   `bit-cli` needs every peer, including ones that took two gigabytes
             and left. It builds the filter through the type's own
             `Deserialize` from a fixed literal, which works and reads badly.
Approach:    One line upstream: re-export `PeerStatsFilterState` alongside
             `PeerStatsFilter`. Until then the literal is pinned by a comment
             at `engine::all_peers_filter`.
Acceptance:  `engine::all_peers_filter` constructs the filter with a named
             enum variant and no JSON.

**Closed 2026-08-22, and it was the one line the Approach said it was.**
`http_api_types` re-exports `PeerStatsFilterState` beside `PeerStatsFilter` in
the vendored tree, and `all_peers_filter` is

```rust
PeerStatsFilter {
    state: PeerStatsFilterState::All,
}
```

with no `serde_json`, no literal, and no `unwrap_or_default` whose fallback
would have quietly narrowed the report to live peers if the literal had ever
stopped parsing. Worth doing because it is the smallest possible demonstration
of what owning the fork is for: this sat open as an upstream API gap while the
fix was one line in a file this repository now ships.

### T-142 bit-cli peers never joined the swarm it was sampling

Source:      found building [T-117](cli-surface.md)'s `peers` fixture
Category:    peers
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `bit-cli peers` added its torrent with `paused: true`, and
             `librqbit` 9.0.0 hands a torrent its peer stream only when it
             starts: `ManagedTorrent::start` takes `peer_rx` and documents
             that it must be set unless `start_paused`. So the command never
             announced, never dialled, and reported an empty swarm however
             long it watched. Every run said `seen: 0`, `peers: []`, and exit
             9.
Relevance:   P1 by the definition in [INDEX.md](INDEX.md): a documented
             capability that does not work. `README.md` says "Connect, sample
             the swarm, report peers, exit" and the command could not do the
             first of those.
Approach:    Start the torrent. The comment said paused "keeps the torrent
             connected to the swarm for peer discovery without pulling any
             payload", and neither half of that was true.
Acceptance:  A seeder on loopback and `bit-cli peers` pointed at it report
             that peer, with the bytes that came from it.

**Done, with two other gaps closed alongside it because the fix could not be
proven without them.**

The measurement that found it, 2026-08-20T16:15Z. `loopback-tracker` logs
every announce it gets, one seeder was serving, and the tracker saw exactly
one client:

```
16:15:20.141Z announce ... port=55502 left=0 event=started -> 0 peer(s)     the seeder
                                                                            nothing from `peers`
16:16:11.238Z announce ... port=57261 left=2000 event=started -> 1 peer(s)  `download`, for contrast
```

`bit-cli peers ... --duration 10s` between those two announces reported
`seen: 0` with the seeder up and registered. `bit-cli download` against the
same torrent announced and found it.

**Nothing selected is not the same as nothing wanted.** The first fix tried
was `paused: false` with `only_files: Some(vec![])`, which announces and
dials and still pulls no payload. Measured against the loopback seeder, that
reports the peer and nothing else: `state: "not needed"`, `errors: 1`,
`downloaded_bytes: 0`, and no client string, because neither side wants
anything from the other and the connection is dropped on the handshake. With
the file selection left alone the same fixture reports
`downloaded_bytes: 2000`, `verified_pieces: 2`, `chunks: 2`,
`mean_piece_ms: 10`, and `errors: 0`.

The report is built on the second of those. `--sort speed` orders peers by
bytes that arrived, and `PeersReport` carries `downloaded` and per-peer
`downloaded_bytes`, so a sample that transfers nothing cannot answer what the
command is asked. What the sample pulls goes to a temporary directory that the
process removes when it exits, which is unchanged, and `--duration`,
`--count`, and now `--max-download-rate` are what bound how much moves.

**The command could not be driven offline, which is why this survived.**
`peers` built `TrackerArgs::default()`, hardcoded `no_dht: false` and
`no_lsd: false`, and had no `--peer`. So it could not be pointed at a known
peer, could not be told to stay off the DHT, and could not be tested without
the network. It now flattens `TrackerArgs` and `LimitArgs` and takes `--peer`,
`--no-dht`, and `--no-lsd`, which is the same set `download` and `seed` carry.
`peers --peer <ADDR> --no-tracker --no-dht --no-lsd` samples a swarm of
exactly the members named on the command line and reaches nothing else.

BEP 27 was never at risk here: `librqbit` builds neither the DHT nor the LSD
receiver for a private torrent, in `session.rs` around line 1537, whatever the
session's own settings say.

`cmd::peers::tests::a_sampled_swarm_carries_what_came_from_each_peer` is the
regression test: a real seeder on a thread, `--peer` pointed at it, and
assertions on the bytes that arrived and on the working directory being left
empty. It fails on the old code with `seen: 0`.

```
$ cargo test -p bit-cli --lib peers
test result: ok. 11 passed; 0 failed
```

---

### T-163 MSE/PE peer encryption is not implemented

Source:      `reference/RESEARCH.md` section D, 2026-08-21
Category:    peers
Priority:    P2
Effort:      L
Status:      **done**, 2026-08-23T03:05Z

Problem:     `bit-cli` speaks plaintext BitTorrent only. There is no MSE/PE
             (message stream encryption, protocol encryption) in the tree, and
             no way to require it, prefer it, or accept it.
Relevance:   This is an interoperability cost before it is a privacy feature.
             A peer configured to **require** encryption will not exchange
             traffic with a plaintext-only client at all, which superseedr
             [Issue 297](https://github.com/Jagalite/superseedr/issues/297)
             states plainly from the other side of the same gap. So the swarm
             `bit-cli` can reach is smaller than the swarm that exists, and
             nothing in the output says so.
Approach:    Three sources, in the order they are worth reading.

             `mtorrent/mtorrent-core/src/pe/` is the cleanest standalone
             implementation. `key_exchange.rs` carries the 768-bit MSE DH
             prime with generator 2 and `KEY_SIZE = 96`.
             `mtorrent/mtorrent-core/src/pe/handshake.rs:12-17` fixes
             `MODE_PLAINTEXT = 1`, `MODE_RC4 = 2`, `MODE_ANY = 3`,
             `MAX_PADDING_LEN = 512` and `VC_LEN = 8`, with `max_pe3_len` and
             `max_pe4_len` just below so a reader can bound its buffers. `:41`
             `outbound_handshake` and `:164` `inbound_handshake` are the two
             directions.

             `mtorrent/mtorrent-core/src/pe/utils.rs:17` `detect_encryption`
             is the piece that matters most for `bit-cli`'s shape. It reads
             exactly `PROTOCOL_STRING.len()` bytes, compares, and returns the
             stream **with those bytes pushed back**, so one listening port
             serves plaintext and encrypted peers with no second port and no
             mode flag.

             `nanotorrent` is the librqbit-specific route, and it is the one
             that matters, because `bit-cli` builds on librqbit and does not
             fork it. Patches `0003-stream-transform-seam.patch` and
             `0005-incoming-stream-transform-seam.patch` add a
             `StreamTransform` trait plus `SessionOptions::stream_transform`
             for outgoing streams, and an `IncomingStreamTransform` for the
             accept path. The non-obvious half is in 0005: the incoming
             transform is handed **every active info hash**, because the hash
             is not known until the possibly-encrypted handshake has been
             read, and the MSE responder resolves the peer's SKEY against
             them. `nanotorrent/src/bittorrent/mse.rs` is 819 lines of
             implementation against those two seams, and its module doc states
             the policy choice outright: RC4 only, advertise only RC4 in
             `crypto_provide`, drop a peer that will not do RC4, because that
             is what "require encryption" means.
Blocker:     The seams do not exist in `librqbit` 9.0.0. This is the same wall
             [T-002](webseed.md) measured and [T-102](bep-coverage.md)
             records: the connect and accept paths and `PeerConnectionHandler`
             are implemented inside `librqbit` by the torrent state, not by
             anything a dependent crate can supply. What would unblock it is
             two upstream visibility changes of the shape nanotorrent's 0003
             and 0005 make, or a vendored `librqbit`, which decision 7.3 does
             not take. It stays open with the cost named.
Acceptance:  A `bit-cli download` against a peer configured to require
             encryption completes, and the same run against the same peer with
             encryption off completes too, from one listening port with no
             mode flag. `--encryption off|prefer|require` reports which mode
             each peer settled on in `--json`. Both runs recorded here.

**Closed 2026-08-23. The blocker was real and the fork removed it.** The
Blocker line said the seams do not exist in `librqbit` 9.0.0 and that what
would unblock it is "two upstream visibility changes of the shape nanotorrent's
0003 and 0005 make, or a vendored `librqbit`, which decision 7.3 does not
take". The trees were vendored on 2026-08-22, so the second option is now the
one available, and what it took is one trait with two methods rather than the
two separate seams the Blocker line expected.

**The seam is in the vendored tree and the encryption is not.**
`librqbit::StreamTransform` is called once per peer connection in each
direction, before any protocol byte crosses it, and hands back the two halves
to use from then on. `patches/UPSTREAM.md` carries it under "a peer connection
cannot be wrapped before the handshake". Everything else is this repository's
own code in `crates/bit-cli-core/src/mse/`, which is where
`cargo test --workspace` can reach it: the vendored crates' tests are not in
that run and the workspace's are.

**What was written, and what it was checked against.** Nothing cryptographic
came from a dependency and none of the corpus was copied. One dependency was
added, `rand`, and it is a random source rather than an implementation: the
private exponent and the two padding fields. It was already in the lock file
through `librqbit`.

| file | what | checked against |
| --- | --- | --- |
| `mse/dh768.rs` | the 768 bit exchange, twelve `u64` limbs, Montgomery reduction | `pow(2, x, P)` from an arbitrary precision implementation, three exponents |
| `mse/rc4.rs` | RC4 with the 1,024 byte MSE discard | RFC 6229, three keys, offsets 0, 16 and 240 |
| `mse/handshake.rs` | both directions of the five message handshake | both ends over one in-memory duplex, twenty runs for the padding search |
| `mse/stream.rs` | the encrypting halves and the pushback | round trips at four write sizes, and one duplex narrower than a write |
| `mse/mod.rs` | the policy, and what each peer settled on | the bound on the outcome map |

**One 768 bit exponentiation costs 51.4 microseconds** and a handshake needs
two, so MSE adds about a tenth of a millisecond to a peer connection. The
measurement is the ignored `exponentiation_cost` test in `dh768.rs`, which is
there so a reader has a command rather than a remembered number.

The reduction is Montgomery's. The one hand-rolled implementation this was read
against, `FluxDown`'s, uses binary long division instead, which walks the 1,536
bits of the product once per multiply where Montgomery walks the twelve limbs
once; `mtorrent` delegates the arithmetic to a big integer crate and does not
have the question. No comparison between the two is claimed here, because only
one of them was built.

```bash
cargo test -p bit-cli-core --release --lib mse::dh768 -- --ignored --nocapture
```

**The acceptance ran, all seven phases.**
`bench/encryption-20260823T030511908Z.json`, from
`scripts/check-encryption.ps1`. Three seeders differing only in
`--encryption`, one payload, and two phases that are controls rather than
cases.

| phase | seeder | leecher | bytes | settled on |
| --- | --- | --- | --- | --- |
| `prefer_seeder_default` | prefer | no flag | **8,388,608** | `rc4` |
| `prefer_seeder_off` | prefer | `off` | **8,388,608** | `plaintext` |
| `prefer_seeder_require` | prefer | `require` | **8,388,608** | `rc4` |
| `require_seeder_default` | require | no flag | **8,388,608** | `rc4` |
| `require_seeder_off` | require | `off` | **0** | control |
| `off_seeder_default` | off | no flag | **8,388,608** | `plaintext` |
| `off_seeder_require` | off | `require` | **0** | control |

The first three are the same seeder process on the same port, which is the
"one listening port with no mode flag" half of the acceptance: an accepting end
tells MSE from plaintext by reading the first twenty bytes, and it did it three
times without restarting. The two controls are what say the rest measured
something: a `require` that quietly accepted plaintext would pass every other
row.

**`--encryption` defaults to `prefer`, which changes what a default run does.**
It dials with MSE and dials again in plaintext when the peer does not answer,
which is what mainline clients do. The redial is the reason `prefer` is
usable at all: without it, a plaintext peer would be lost until `librqbit`'s
own backoff dialled it again, and that backoff is [T-138](peers.md)'s.

**A premise that was wrong, and the measurement that showed it.** The first
implementation let a responder with `--encryption off` complete the
Diffie-Hellman exchange and refuse afterwards, on the reasoning that the policy
check belongs where the outcome is known. That is backwards. The dialling end
had by then been told its handshake worked, so it never fell back, and
`off_seeder_default` did not complete at all: it looped, dialling with MSE
against a seeder that answered and then hung up, until the run's deadline. The
refusal is at the first twenty bytes now, before the exchange, and
`encryption_off_refuses_before_the_key_exchange` in `handshake.rs` holds the
ordering.

**What is deliberately not offered.** `crypto_provide` names RC4 and nothing
else. MSE's other method is plaintext-after-handshake, and offering it would
buy no peer that RC4 does not already reach while adding a third state to
report and to test. A peer that offers plaintext only is refused with a message
saying so.

**What this is not.** RC4 with a 768 bit exchange is not confidentiality
against a serious attacker, and nothing here claims it. What it buys is that a
middlebox cannot classify the stream by reading the protocol header off the
front of it, and that a peer which refuses plaintext will talk to us. That was
always the entry's argument: the Relevance line calls it an interoperability
cost before it is a privacy feature.

### T-164 A peer that sends garbage keeps its connection slot

Source:      `reference/RESEARCH.md` section D, 2026-08-21
Category:    peers
Priority:    P2
Effort:      M
Status:      **partial**. Part 1, `--block-peer`, done 2026-08-22T02:20Z.
             Parts 2 and 3 blocked on `librqbit` 9.0.0, both named below.

Problem:     `bit-cli` has `--web-seed-fatal-status` and
             `--web-seed-max-errors`, so an HTTP source that misbehaves is
             retired and stays retired. There is no equivalent for a peer. A
             peer that fails a piece hash, sends a malformed message, or
             breaks the protocol is dropped and then redialled.
Relevance:   vortex [Issue 125](https://github.com/Nehliin/vortex/issues/125)
             is that failure with the crash already fixed: once the process
             stopped dying on the malformed response, the same peer
             reconnected and kept sending garbage, burning a connection slot,
             and the DHT rediscovered it **every 20 seconds**. Fixing a crash
             without adding a blocklist turns a hard failure into a slow one,
             which is harder to diagnose. The asymmetry is the argument on its
             own: `bit-cli` already decided that a source which misbehaves gets
             retired, and applies that decision to only one of its two kinds
             of source.
Approach:    The proposal in that issue is the shape. Auto-block on a protocol
             violation, check the blocklist **before completing a handshake**
             rather than after, and expose add, remove and query. Persistence
             is optional there, which suits `bit-cli`: decision 7.4 allows no
             state file, so the blocklist lives for the invocation and
             `--block-peer <ADDR>` covers the case a user wants to carry
             across runs.
             `aria2_rust/aria2-core/src/engine/bt_peer_storage/` holds a
             `rejection_state.rs` with blocklist tests beside it, for a second
             opinion on the bookkeeping.

             What makes a violation attributable rather than guessed is
             [T-179](webseed.md), smart ban: with several sources filling one
             piece, a failed hash names a set of peers and not one peer. Build
             that first, or this blocks whichever peer is convenient.

             **T-179 is done, and it built the half that is not peer-specific.**
             `webseed/ledger.rs` records a hash of every block against whoever
             supplied it and convicts every supplier whose hash differs from
             the bytes the session went on to verify, reading those bytes back
             off the disk rather than fetching them again. It is keyed on a
             `usize` source index rather than a URL for this entry's sake, so a
             peer key fits without changing the type. What is missing on the
             peer side is the recording hook: `bit-cli` sees a bridge put a
             block on the wire and does not see a peer's block arrive, because
             that path is inside `librqbit`. Name that seam here before pricing
             this, the way [T-167](bep-coverage.md) had to.
Acceptance:  A synthetic peer that fails a piece hash twice is not redialled
             for the rest of the run, `bit-cli peers --json` names it with the
             reason, and the freed slot measurably goes to another peer.
             `bench swarm` drives it, because it already builds peers that
             misbehave on purpose.

**The seam is named, 2026-08-22T02:10Z, and it splits this entry into three
parts rather than one.** Read before writing any of it, which is what the
paragraph above asked for. Two of the three are blocked and one is not blocked
at all, which is not what "effort M, blocked on a librqbit seam" would have
said.

### 1. A blocklist exists upstream, and it is checked in exactly the right place

This is the part the entry did not know. `librqbit` 9.0.0 has a blocklist and an
allowlist, and both are consulted in both directions:

- **Incoming**, `session.rs:917`, `if self.blocklist.has(incoming_ip)`, and it
  is above the `read_handshake` at `:934`. That is the vortex proposal's
  "check the blocklist **before completing a handshake**", already true.
- **Outgoing**, `torrent_state/live/mod.rs:629`, in the peer-stream loop, before
  a permit is taken or a connection task is spawned.
- Both bump `session_stats` counters, `blocked_incoming` and `blocked_outgoing`.

`SessionOptions::blocklist_url` (`session.rs:461`) is how it is populated, once,
at `Session::new_with_opts` (`session.rs:739-748`). `IpRanges::load_from_url`
(`ip_ranges.rs:61`) takes a **`file:` URL** as well as an HTTP one
(`ip_ranges.rs:64-70`), and the format is PeerGuardian's: `name:start-end` per
line, `#` for a comment, plain or gzip, parsed at `ip_ranges.rs:152`.

**So `--block-peer <ADDR>` is not blocked.** `bit-cli` writes the ranges it was
given to a scratch file and points `blocklist_url` at it before the session
starts. `cmd::peers` already makes a `tempfile::tempdir()` per invocation, so
the pattern exists and decision 7.4 is not touched: this is a scratch file for
the length of one process, not state anything reads back.

### 2. Adding to that blocklist during a run is blocked, and it is a near miss

`Session.blocklist` (`session.rs:141`) is a plain `IpRanges` field, not a lock
and not an `ArcSwap`. `bit-cli` holds an `Arc<Session>` through
`Engine::session`, so there is no `&mut` to be had and no interior mutability to
use.

`IpRanges::new` (`ip_ranges.rs:47`) is `pub` and takes the ranges directly, so
the value could be built. It cannot be named: `lib.rs:60` declares
`mod ip_ranges;` with no `pub`, so `pub` inside it reaches nothing outside the
crate. That is the same shape as [T-167](bep-coverage.md)'s `update_bitfield`,
and it is recorded here for the same reason: so nobody re-derives it.

### 3. Attributing a bad piece to the right peer is blocked, and upstream
### already gets it wrong

This is the half [T-179](webseed.md) built for HTTP sources, and the seam is
`TorrentStorage`.

`file_ops.rs:310`, `write_chunk(&self, who_sent: PeerHandle, data, chunk_info)`,
**has** the peer: `PeerHandle` is `SocketAddr` (`type_aliases.rs:13`). It drops
it one line later. The trait `bit-cli` implements is
`storage/mod.rs:136`, `pwrite_all_vectored(&self, file_id, offset, bufs)`, and
there is no peer in it. `SafeStorage` therefore sees every byte a peer sends and
never sees who sent it. `mod file_ops;` and `mod torrent_state;` are both
private, so there is no second place to look.

And `librqbit` already convicts a peer, incorrectly.
`torrent_state/live/mod.rs:1965-1972`: when `check_piece` returns false it warns
with `?addr`, marks the piece failed, and

```rust
anyhow::bail!("i am probably a bogus peer. dying.")
```

which drops the connection of whichever peer delivered the **last** chunk of
that piece. With several peers filling one piece that is the peer that finished
it, not the peer that broke it. That is exactly the wrong answer T-179 was
written to stop giving, present upstream, and it is why smart ban for peers
cannot be built beside `librqbit`: the conviction happens inside it, before
anything `bit-cli` owns is told.

`webseed/ledger.rs` is still the right machinery and still fits. It is keyed on
a `usize`, and a `SocketAddr` maps to one through a table `bit-cli` would keep.
What is missing is the one call that would fill it.

**What would unblock parts 2 and 3**, smallest upstream change first:

1. `TorrentStorage` gains a `who_sent: Option<PeerHandle>` on the write methods,
   or a separate `fn on_chunk_written(&self, who_sent, file_id, offset, len)`
   with a default empty body. `write_chunk` already holds the value; this is
   passing it on. That alone unblocks part 3 and lets `bit-cli` convict the
   right peer with the ledger it already has.
2. `Session.blocklist` becomes an `ArcSwap<IpRanges>` or a `RwLock`, with a
   `Session::block_ip` beside it, and `pub mod ip_ranges`. That unblocks part 2.
3. Failing 1, `librqbit` stops convicting on the last chunk and takes a
   per-block record of its own. That is the larger change and it is upstream's
   to want.

**Re-priced.** Part 1 was effort S and is **done, 2026-08-22T02:20Z**. Parts 2
and 3 stay open and blocked, with the lines above as the blocker. The entry
keeps its P2 and stays at the height of its value, which is the rule in
[INDEX.md](INDEX.md).

### Part 1, as built

`--block-peer <ADDR>` on `download`, `seed` and `peers`, because it lives in
`LimitArgs` and every command that has a session has those. It takes an
address, an inclusive `START-END` range, or a CIDR block, in either family.
`swarm::blocked_ranges` parses it. Three decisions in it are worth stating:

- **A `HOST:PORT` is refused**, with the address to write instead. The session
  blocks an address, so silently dropping the port would block every port on
  that host without saying so.
- **Nothing is resolved.** `--peer` takes a name because a caller naming a peer
  wants to reach it. A blocklist entry that resolved would block whatever the
  name pointed at when the run started, which is not what a block means.
- **A `/0` and a `/32` are both exact**, because a shift by the full width is
  undefined and the widest block is the one a caller reaches for to test the
  flag.

`Engine::start` writes the ranges to a scratch file in PeerGuardian format and
points `blocklist_url` at its `file:` URL. The file is a `NamedTempFile` held
for that one call and deleted when it drops, so decision 7.4 is untouched: it
is not state, nothing reads it back, and a run that blocks nothing writes no
file at all.

**Measured**, against `target/release/bit-cli`, one loopback seeder holding an
8 KiB payload, the same command twice:

```
$ bit-cli peers blk.torrent --peer 127.0.0.1:51955 --no-tracker --no-dht --no-lsd --duration 4s --port 0
live 0  connecting 0  queued 0  seen 1  dead 0
ADDRESS          STATE       DIR       DOWN      PIECES
127.0.0.1:51955  not needed  outgoing  8.00 KiB  8

$ bit-cli peers blk.torrent --peer 127.0.0.1:51955 --block-peer 127.0.0.1 ...
live 0  connecting 0  queued 1  seen 1  dead 0
blocked              0 incoming, 1 outgoing
ADDRESS          STATE   DIR       DOWN  PIECES
127.0.0.1:51955  queued  outgoing  0 B   0
```

8 KiB and eight pieces against the peer, or nothing and a refusal counted. The
number the flag moves is `blocked_outgoing`, which is the session's own counter
rather than one this tree keeps, read through `Api::api_session_stats`. It is
reported as `blocked` on `peers`, absent when nothing was refused so an
ordinary sample carries no extra field, and it is in `docs/schema.md`.

**`seen` counts a blocked address, and that is recorded rather than
corrected.** `task_peer_adder` registers the address when it is queued and
checks the blocklist when it takes it off the queue
(`torrent_state/live/mod.rs:629`), so a blocked peer sits at `queued` for the
whole run with nothing against it. Subtracting a refusal count from a peer
count would be arithmetic nobody can check: the counter counts refusals, not
addresses, and one address refused twice moves it by two. The two numbers are
reported side by side instead.

Six tests. `a_blocked_peer_is_never_dialled_and_never_joins_the_swarm` is the
acceptance and uses the same loopback-seeder rig as
[T-142](#t-142-bit-cli-peers-never-joined-the-swarm-it-was-sampling)'s.

### T-165 The peer's reqq is ignored, so the queue depth is a fixed 128

Source:      `reference/RESEARCH.md` section D, 2026-08-21
Category:    peers
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T18:12Z, premise disproved

Problem:     A peer's BEP 10 extended handshake carries `reqq`, the number of
             block requests it will queue. `bit-cli bench leech` reports a
             queue depth of 128 whatever the peer said, and nothing reads the
             advertised value.
Relevance:   mtorrent [Issue 17](https://github.com/DanglingPointer/mtorrent/issues/17)
             carries the whole argument: exceeding `reqq` either wastes every
             request past the limit or gets the connection dropped, depending
             on the peer, and both look like a slow peer from this side. It
             also makes a number `bench leech` prints wrong rather than merely
             unbounded, which matters more here than upstream, because that
             number is evidence under [T-041](memory.md) and
             [T-018](disk-io.md). A fixed constant reported as a measurement is
             the mistake [T-032](performance.md) and [T-141](webseed.md) both
             closed by disproving.
Approach:    `vortex/bittorrent/src/peer_comm/extended_protocol.rs:60`
             `extension_handshake_msg` shows `reqq` beside `m`, `v`, `p`,
             `metadata_size` and `upload_only` in the same handshake
             `bit-cli`'s bridge already builds, so the field is one key away on
             the send side. On the receive side the value bounds the pipeline.
             `seedchamp/docs/design.md:197` is what to bound it *with*: a
             BDP-sized depth from an EMA of that peer's own wire rate,
             `desired = 5 s * rate / 16 KiB`, capped rather than fixed, with a
             20 s request stall and 4 s in endgame. The bridge is the place to
             start because it is `bit-cli`'s own peer implementation. The
             session side needs `librqbit`.
Acceptance:  `bench leech` reports the peer's advertised `reqq` and the depth
             actually used, and the two agree when the peer advertises less
             than the cap. A synthetic peer advertising `reqq = 8` receives no
             more than 8 outstanding requests, asserted in a test rather than
             observed in a report.

**Done 2026-08-23T18:12Z, and both halves of the Problem are disproved. There
is nothing left in this entry to build.**

**`reqq` is read.** `librqbit` 9.0.1's `on_extended_handshake` at
`vendor/rqbit/crates/librqbit/src/torrent_state/live/mod.rs:1241` takes the
peer's `reqq`, computes `reqq.min(DEFAULT_PEER_REQUEST_WINDOW)` and assigns it
to that peer's `flow.request_window`. It is upstream's own code: no patch in
`patches/rqbit/` touches it and `patches/UPSTREAM.md` has no section for it, so
it has been true since the version this repository vendored.
`bit-cli bench probe` reads the key as well, at
`crates/bit-cli-core/src/bench/probe.rs:417`.

**And the reported depth is observed rather than fixed.** `peak_queue_depth`
comes from `pipeline.peak_in_flight`, which the bridge counts at
`crates/bit-cli-core/src/webseed/bridge.rs:432`. The other feeder,
`Recorder::observe_choke`, has no caller anywhere outside its own test.

### Run against the claim

The bridge advertises `reqq: 250`, at `bridge.rs:72`, so the session's window
is `min(250, 128)` and a run reports 128. That is the number the entry is named
for, and it is the cap being reached rather than a constant being printed. The
way to tell the two apart is to change what the bridge says and look:

| `REQUEST_QUEUE` the bridge advertises | peak in flight | mean in flight | leech rate |
| --- | --- | --- | --- |
| 250 | **128** | 19 | 120.30 MiB/s |
| 32 | **32** | 7 | 122.14 MiB/s |

The peak follows the advertisement. `bit-cli bench leech`, 32 MiB payload, one
connection, loopback file server, one run each. Both reports are committed:
`bench/leech-20260823T180307071Z.json` and
`bench/leech-20260823T180645783Z.json`.

```bash
pwsh -NoProfile -File scripts/bench-leech.ps1 -PayloadSize 32MiB -Runs 1 -ConnectionSweep "1"
```

### And the Approach is disproved as well, by the same two runs

The Approach proposes replacing the fixed window with a BDP-sized depth from an
EMA of the peer's own rate, citing `seedchamp/docs/design.md:197`. **Nothing on
this path is short of window.** Mean in flight is 19 blocks against a window of
128, and quartering the window to 32 left throughput where it was: 120.30
MiB/s against 122.14, which is noise on a single run either way.

A rewrite that moves no number does not ship, which is the same rule
[RULES.md](RULES.md) section 5 applies to a flag. So this closes with no
residual entry behind it rather than with a smaller version of itself: the
depth is not what this path is limited by, and the measurement that would
justify sizing it dynamically is the one that says otherwise.

**What that leaves is the other three numbers in the same report**, and they
are already somebody's: at a window of 128 the run reached 15.37 percent of
what that depth allows, and the gap between `fetch` at 961.97 MiB/s and
`leech` at 120.30 is [T-090](bench.md)'s question, not this one.

### T-166 BEP 10 extension ids are not proven to map in both directions

Source:      `reference/RESEARCH.md` section D, 2026-08-21
Category:    peers
Priority:    P1
Effort:      S
Status:      **done**

Problem:     The web seed bridge implements BEP 10 (`webseed/bridge.rs:83`,
             `:708`) and nothing in the tree asserts that it keeps **our**
             extension ids and **the peer's** apart. They are two independent
             numberings.
Relevance:   vortex [PR 103](https://github.com/Nehliin/vortex/pull/103) is
             the best interop finding in the corpus and it is exactly this
             mistake. The extension map was keyed by the local id and then
             tested against the peer's: `if self.extensions.contains_key(&id)
             { continue; }`. When qBittorrent assigned `ut_metadata = 2` and
             the local side used `1`, incoming id 2 was skipped as "already
             initialised", because the local `upload_only` happened to be 2.
             The stated consequence is that extensions had never once worked
             against qBittorrent. A defect of this shape is silent, is
             invisible against any peer that happens to number its extensions
             the same way, and `bit-cli`'s bridge sits on both ends of a
             loopback pair in every test it has, which is precisely the
             arrangement that hides it.
Approach:    The rule is one sentence: map **peer id to handler** in one
             direction and **name to our id** in the other, as two separate
             tables, and never index one with the other's key. Read the bridge
             against that rule, then write a test whose peer deliberately
             numbers `ut_metadata` and `upload_only` differently from the
             bridge and asserts both are routed.

             Two ordering rules from the same repository are worth asserting
             while that test is being written.
             [PR 156](https://github.com/Nehliin/vortex/pull/156): messages
             arriving in the same TCP read as the handshake were processed
             before the bitfield was queued, so `Interested` could precede
             `Bitfield`, and **the bitfield must be the first message after
             the handshake**. `webseed/bridge.rs:674` already says the order
             matters; a test is what keeps it true.
             [PR 155](https://github.com/Nehliin/vortex/pull/155) is the
             `Have` handling for peers without BEP 6.
Acceptance:  A test in which the peer's extension numbering differs from the
             bridge's, the bridge routes an incoming `ut_metadata` and an
             incoming `upload_only` to the right handlers, and the first
             message after the handshake is asserted to be the bitfield.

**Read against that rule, the bridge had neither table, and the missing one
cost a connection.** The premise of this entry needed correcting before the
test could be written, and the correction is what found the defect.

`bit-cli`'s bridge advertises an **empty** `m` (`webseed/bridge.rs`
`extended_handshake`), which is the honest thing: it seeds and implements no
extension messages. So there is no "name to our id" table, and because every
extension message fell through the receive loop's catch-all there was no "peer
id to handler" table either. A map keyed the wrong way round, which is the
literal vortex PR 103 defect, could not exist here because there was no map.

**What did exist is the same mistake one level down.** The receive loop called
`Message::deserialize`, and `librqbit-peer-protocol` 9.0.0 routes an incoming
extension id against **its own** constants:
`MY_EXTENDED_UT_METADATA = 3` at `librqbit-peer-protocol/src/lib.rs:52` and
`MY_EXTENDED_UT_PEX = 1` at `:55`, dispatched at `src/extended/mod.rs`. Those
are the ids that crate advertises. This bridge
advertises neither, and it was still reading incoming ids through them. That is
an incoming id looked up in a table the two ends never agreed on, which is
exactly the direction confusion this entry names.

The cost is a dropped connection. `UtMetadata::deserialize` refuses a body that
is not a ut_metadata message, `ExtendedHandshake` refuses one with no `m`, and
a deserialize error becomes `BridgeError::Link`, which ends the connection and
starts the reconnect backoff. Measured across the whole id space, with the fix
reverted:

```
EXT ID 0: LINK DIED: early eof      <- decoded as an extended handshake
EXT ID 1: link survived             <- decoded as ut_pex; an empty dict happens to parse
EXT ID 2: link survived
EXT ID 3: LINK DIED: early eof      <- decoded as ut_metadata
EXT ID 4: link survived
EXT ID 7: link survived
EXT ID 9: link survived
EXT ID 200: link survived
```

Two ids out of the sample, and both of them `librqbit`'s. Every id the bridge
had actually advertised, which is none of them, was fine. **Id 1 surviving is
the more instructive result**: it was decoded as `ut_pex` too, and it lived
only because an empty bencode dictionary happens to satisfy that type. It was
never routed correctly, it was routed to the wrong type and got away with it.
That is the silence this entry predicted.

**Fixed by deciding the question against our own map and nowhere else.**
`OUR_EXTENSIONS` is the table of `(name, our id)` pairs the bridge advertises,
`is_our_extension` is the only thing that reads it, and the receive loop drops
an extension frame whose id is not in it before `Message::deserialize` ever
sees the bytes. The table is empty today and the wire form says the same thing,
which a unit test asserts as one claim: an empty table and an empty `1:mde` are
the same statement, so an entry added to one without the other fails.

That is also the seam [T-167](bep-coverage.md) needs. `lt_donthave` adds one
entry to `OUR_EXTENSIONS` and one handler, and the receive direction is right
by construction because the lookup is against the advertised map. The **send**
direction is the second table and does not exist yet: it has to be read out of
the peer's own extended handshake, and T-167 is the first thing that will need
it, because the bridge is the end that sends `lt_donthave`.

**The test is a session written by hand, which is what this entry was for.**
`crates/bit-cli-core/tests/bridge_protocol.rs` speaks the peer protocol byte by
byte, declares the message ids as its own constants rather than importing the
bridge's, and never calls the serializer the bridge calls. Nothing in it can
agree with the bridge by construction. Every other bridge test puts a real
`librqbit` session on the far end, and both ends of that pair number their
extensions identically, which is the arrangement the entry named as the one
that hides this.

The session advertises `ut_metadata = 2`, `upload_only = 4`, `lt_donthave = 7`,
none of which is `librqbit`'s number for any of them, and then sends messages
under those ids **and** under 1 and 3. `no_extension_id_can_end_the_connection`
walks all 256 ids on one connection, then sends a well-formed `ut_metadata`
request under id 3, which is precisely what a peer that got the direction
backwards would send. The assertion in both is behavioural: after all of it the
bridge still answers a `request` with the source's bytes at the offset the
request named.

**On the ordering rule, PR 156 is right and its one-line summary is not the
rule here.** vortex's finding is that a message arriving in the same TCP read
as the handshake was processed before the bitfield had been queued, so
`Interested` could precede `Bitfield`. `bit-cli`'s bridge writes the extended
handshake, the bitfield and `unchoke` as one concatenated buffer in a single
`write_all`, before the receive loop starts, so nothing can interleave with
them. The order on the wire is extended handshake, bitfield, unchoke, and the
extended handshake being first is deliberate rather than an exception: BEP 10
puts it in the handshaking sequence, and it is what carries the BEP 21
`upload_only` flag that tells the session it is looking at a partial seed
rather than a leecher. `the_bitfield_precedes_every_peer_message_after_the_handshake`
asserts that reading, which is the rule that survives contact with a peer that
also speaks BEP 10: **no ordinary peer message precedes the bitfield.**

PR 155, `Have` handling for peers without BEP 6, is not applicable. The bridge
sends a bitfield and then never revises it, so it sends no `Have` at all, and
it ignores every `Have` the session sends because it only seeds. That changes
with [T-167](bep-coverage.md), which is the first message the bridge will send
to revise what it holds.

**Proven by reverting the fix.**

```
$ cargo test -p bit-cli-core --test bridge_protocol    # with the frame skip removed
test the_bitfield_precedes_every_peer_message_after_the_handshake ... ok
test no_extension_id_can_end_the_connection ... FAILED
test a_peer_that_numbers_its_extensions_differently_is_still_served ... FAILED
test result: FAILED. 1 passed; 2 failed

$ cargo test -p bit-cli-core --test bridge_protocol    # with the fix
test the_bitfield_precedes_every_peer_message_after_the_handshake ... ok
test a_peer_that_numbers_its_extensions_differently_is_still_served ... ok
test no_extension_id_can_end_the_connection ... ok
test result: ok. 3 passed; 0 failed

$ cargo test -p bit-cli-core --lib webseed::bridge
test webseed::bridge::tests::an_incoming_extension_id_is_only_read_against_our_own_map ... ok
test result: ok. 17 passed; 0 failed
```

**One note on how nearly this stayed hidden.** The first draft of the
hand-written session sent a malformed extended handshake: two bencode string
lengths were wrong, `12:lt_donthave` for an eleven byte name and `9:fake/1.0`
for an eight byte value. Every id "died", which reads as a much larger defect
than the real one. The lesson is the one [RULES.md](RULES.md) already carries
from T-032 and T-141: the first reading was of the fixture rather than of the
thing. The fixture is now the part of this test worth reading twice.


### T-194 A torrent past 131,960 pieces cannot be served or fetched at all

Source:      [rqbit#637](https://github.com/ikatson/rqbit/issues/637), item 0 of
             `patches/TASKS.md`, measured 2026-08-22
Category:    peers
Priority:    **P0**
Effort:      M
Status:      **done**, 2026-08-22T13:52Z, with a residual ceiling in
             [T-195](peers.md)

Problem:     `Message::Bitfield` is serialized into the fixed per connection
             write buffer, which is `MAX_MSG_LEN` bytes. A bitfield is one bit
             per piece, so its length is a property of the torrent and not of
             the protocol. Past 131,960 pieces it does not fit, `serialize`
             returns `NoSpaceInBuffer`, and the connection is dropped before a
             single piece is served. Both directions fail: a seeder cannot
             answer, and a leecher fetching metadata for such a torrent by
             magnet never resolves it.
Relevance:   This is not a slowdown. A torrent past the threshold does not
             work at all, in either role, against any peer. Nothing in
             `bit-cli` reported it as anything: the seeder logged
             `error managing peer: not enough space in buffer` at DEBUG and
             carried on, and the leecher waited.
Approach:    Stop routing the bitfield through the shared fixed buffer. The
             handler sizes its own buffer, because only it knows the piece
             count. `Message::bitfield_message_len` is the one thing the
             protocol crate has to expose for it.
Acceptance:  A torrent above the old threshold resolves by magnet from a local
             seeder and its file is created.

**Where the number comes from.** `MAX_MSG_LEN` is 16,500 bytes, built in
`peer_binary_protocol/src/lib.rs` for a `ut_metadata` data message: a 16,384
byte chunk plus its bencode header plus 64 bytes of slack. A bitfield message
is `5 + ceil(pieces / 8)` bytes, so it fits while `ceil(pieces / 8) <= 16,495`,
which is 131,960 pieces. The comment above the constant said the `ut_metadata`
request was "the largest known message", and that was the whole mistake.

**Measured, and it is exact to one piece.** Every case is a torrent of 1 KiB
pieces, seeded on loopback with trackers and DHT off, fetched by magnet by a
second process given only `--peer 127.0.0.1:<port>`:

| pieces | `.torrent` | bitfield | before | after |
| --- | --- | --- | --- | --- |
| 131,952 | 2,639,179 B | 16,499 B | resolves | resolves |
| **131,960** | 2,639,339 B | **16,500 B** | **resolves** | resolves |
| **131,961** | 2,639,359 B | **16,501 B** | **no space in buffer** | resolves |
| 131,968 | 2,639,499 B | 16,501 B | no space in buffer | resolves |
| 163,840 | 3,276,939 B | 20,485 B | no space in buffer | resolves |

The two middle rows are one piece apart and 16,500 is `MAX_MSG_LEN` exactly.

**The `.torrent` size is a red herring, and that matters for the upstream
report.** rqbit#637 is titled "rqbit faill to add torrent larger than 2MB" and
has an empty body. Both 2.64 MB torrents in the table above are "larger than
2MB" and one of them works, so the size of the file is not the variable. The
piece count is. A 2 GiB payload at 16 KiB pieces makes a 2,621,581 byte
`.torrent` with 131,072 pieces, and that one seeds, verifies and downloads
fine. Whether the upstream report is this defect cannot be established from an
empty issue body; it is the same neighbourhood and the same order of magnitude,
and that is as far as the evidence goes.

**Adding is not what fails.** `bit-cli create`, `info`, `verify` and `seed` all
handle a 3.13 MiB `.torrent` with no trouble, and `create` builds one from
160 MiB of payload in 0.195 s. Item 0 of `patches/TASKS.md` asked whether
`bit-cli` could make such a fixture quickly enough to test with, and it can.
What fails is the wire.

**The fix**, in `patches/UPSTREAM.md` under "librqbit: a bitfield larger than
MAX_MSG_LEN cannot be sent":

- `PeerConnectionHandler::serialize_bitfield_message_to_buf` takes a
  `&mut Vec<u8>` rather than a `&mut [u8]`, so the implementor sizes it.
- The send site uses a buffer of its own rather than the shared `write_buf`,
  allocated once per connection and dropped after the bitfield is written.
- `Message::bitfield_message_len` is the exact length `serialize` needs.

```
$ pwsh -NoProfile -File scripts/check-bitfield.ps1
bitfield: 163840 pieces, 3276939 B torrent, metadata resolved, file created
bitfield: ok
```

Upstream's own tests still pass, 139 of them, and the new one is
`test_bitfield_larger_than_max_msg_len` in `peer_binary_protocol`.

### T-195 The read side caps the same message at 262,104 pieces

Source:      measured while closing [T-194](peers.md), 2026-08-22
Category:    peers
Priority:    P2
Effort:      M
Status:      **done**, 2026-08-22T18:57Z

Problem:     `ReadBuf` is a ring buffer of `BUFLEN`, 32,768 bytes, in
             `vendor/rqbit/crates/librqbit/src/read_buf.rs:12`. A message that
             cannot fit in it fails with `read buffer is full`. For a bitfield
             that is `5 + ceil(pieces / 8) <= 32,768`, which is 262,104 pieces.
Relevance:   [T-194](peers.md)
             moved the send side off a fixed buffer entirely, so this is now
             the binding limit and the two halves agree on it. It is twice what
             it was and it is still a limit.
Approach:    Not attempted. The ring buffer needs an overflow path for a
             message larger than itself, and `read_message` holds an unsafe
             reborrow with a miri test around it, so this is a larger change to
             somebody else's code than the send side was. Growing `BUFLEN`
             moves the number without removing it.
Acceptance:  A torrent above 262,104 pieces resolves by magnet from a local
             seeder.

**Measured, and exact to one piece**, same harness as T-194, after the T-194
fix:

| pieces | `.torrent` | bitfield | result |
| --- | --- | --- | --- |
| **262,104** | 5,242,219 B | **32,768 B** | resolves |
| **262,105** | 5,242,239 B | **32,769 B** | `read buffer is full. need_additional_bytes=1` |

32,768 is `BUFLEN` exactly, and the client says how far over it is: one byte.

**What this costs in practice.** A torrent needs more than 262,104 pieces to
hit it, which is a 4 GiB payload at 16 KiB pieces and 1 TiB at 4 MiB. Real
clients raise the piece length as the payload grows, so this is reachable but
uncommon. `bit-cli create` refuses to build one above 100,000 pieces without
`--allow piece-count`, which is not a fix and does not help a torrent somebody
else made.

**Closed 2026-08-22, and the Approach's worry was the right one to have.** It
said the ring buffer needs an overflow path and that `read_message` holds an
unsafe reborrow with a miri test around it. Both are true and neither stopped
it.

**The buffer grows.** `buf` is a `Box<[u8]>` rather than a `Box<[u8; BUFLEN]>`,
every use of `BUFLEN` in the ring arithmetic reads the current capacity, and
`grow` doubles into a new allocation, copying the two halves contiguously to
the front. It is called from exactly one place: the `NotEnoughData` arm, when
the buffer is full and the message is not finished. `BUFLEN` is still what a
connection starts with.

**What stops a peer using it to make this process allocate.** Growth is bounded
by `max_len`, and `max_len` is never taken from the length prefix the peer
sent, which is the number a hostile peer picks. It comes from
`PeerConnectionHandler::max_incoming_message_len`, a new trait method whose
default is the old buffer, so an implementor that does not answer behaves
exactly as before:

- **A live torrent answers from its own piece count**, one bitfield plus
  `MAX_MSG_LEN` of slack. A peer can make the buffer as large as one bitfield
  for the torrent it is talking about and no larger.
- **`peer_info_reader` cannot**, and that is the interesting case. A seeder
  sends its bitfield immediately after the handshake, before this side has the
  metadata, so the message that arrives is as large as the torrent makes it
  while the piece count is the exact thing not known yet. It answers with a
  constant, `MAX_BITFIELD_BEFORE_METADATA` = 1 MiB, which is 8,388,568 pieces:
  128 GiB at a 16 KiB piece length and 32 TiB at 4 MiB.

**That second one is why the first attempt did not work end to end.** The unit
test passed and `check-bitfield.ps1` still failed at 262,105, because a magnet
resolves through `peer_info_reader` and it was still holding the default. The
bitfield it choked on was one it had no use for.

**Measured.** `scripts/check-bitfield.ps1`, a seeder and a magnet fetch on
loopback with trackers and DHT off:

| pieces | `.torrent` | bitfield | before | after |
| --- | --- | --- | --- | --- |
| 262,104 | 5,242,219 B | 32,768 B | resolves | resolves |
| **262,105** | 5,242,239 B | 32,769 B | `read buffer is full` | **resolves** |
| **524,288** | 10,485,900 B | 65,541 B | `read buffer is full` | **resolves** |
| **1,048,576** | 20,971,661 B | 131,077 B | `read buffer is full` | **resolves** |

```bash
pwsh -NoProfile -File scripts/check-bitfield.ps1
```

The default cases are now 131,961 and 262,105, which are the two counts this
repository has measured a client dying on, one per side.

**The unsafe reborrow is still sound, and the growth path is inside what proves
it.** `test_read_buf_miri` now reads an oversized bitfield as well as a piece,
so the reallocation happens under miri while the reborrow is in play:

```bash
cargo +nightly miri test --manifest-path vendor/rqbit/Cargo.toml -p librqbit --features miri test_read_buf_miri -- --ignored
```

**Two things about running that on Windows**, because both cost time.
`cargo-miri` fails with "cargo uses an argfile to invoke rustc" when the
command line gets long, and a short `CARGO_TARGET_DIR` is the way past it. And
`with_timeout` is a no-op only under `--features miri`, so a test that reaches
it cannot run outside miri without a tokio runtime; the growth test is a
`#[tokio::test]` for the ordinary suite and the miri one covers the same path.

**What is left, and it is a different shape.** The pre-metadata ceiling is a
constant rather than a fact about the torrent, so it is a limit, not the
absence of one. Removing it properly means skipping a message this side has no
use for rather than buffering it, which changes `read_message`'s contract from
"return a message" to "may drop one". Nothing in this repository needs it: a
torrent past 8,388,568 pieces is 128 GiB at the smallest piece length anyone
uses.

---

### T-210 An incoming peer is recorded under this session's own peer id

Source:      found closing [T-132](multi-source.md), 2026-08-22
Category:    peers
Priority:    P1
Effort:      S
Status:      **done**, 2026-08-22T17:55Z

Problem:     `manage_peer_incoming` builds the handshake it is about to send,
             writes it, and then hands **that** handshake to
             `on_handshake` and asks **it** whether extended messages are
             supported. Both answers are about this session rather than about
             the peer. The outgoing path a few lines below reads the peer's
             handshake off the wire and uses that, which is what says this is
             a slip rather than a design.
Relevance:   Two things follow, and the second is a wire behaviour. Every
             incoming peer is recorded under our own peer id, so anything
             asking "who is this peer" gets ourselves. And
             `Handshake::new` always sets the BEP 10 extension bit, so every
             incoming peer is assumed to speak the extension protocol whether
             or not it said so.
Approach:    Use `incoming.handshake`, which is the peer's, already read and
             already validated for info hash and self-connection eight lines
             above.
Acceptance:  A peer-scoped rate limit keyed on the peer id reaches an outgoing
             peer and not an exempt incoming one, which is
             `scripts/check-rate-scope.ps1`'s `http_peer_cap` row.

**Found by a limiter that did not limit.** [T-132](multi-source.md) needed the
session's download limit to skip one peer, identified by its peer id prefix.
The exemption matched nothing, and the reason was that the peer id every
incoming peer was filed under was this session's own. `bit-cli`'s web seed
bridge dials **in**, so it was exactly the case that took the wrong path.

The fix is three lines in
`vendor/rqbit/crates/librqbit/src/peer_connection.rs`: the handshake built to
send is named `ours`, and the peer's handshake is what reaches
`supports_extended` and `on_handshake`.

**How it is held.** `scripts/check-rate-scope.ps1`'s `http_peer_cap` phase caps
peers and attaches an HTTP source. Before, the source was capped with them at
**8.40 MiB/s**, because its identity was ours; after, it runs at
**151.84 MiB/s** against the same cap. `bench/rate-scope-20260822T175543220Z.json`.

**The second half is not directly measured here and is not left silent.**
Nothing in this repository speaks the extension protocol badly enough to notice
being sent an extended message it did not ask for, and building a peer that
refuses BEP 10 to prove it is [T-166](#t-166-bep-10-extension-ids-are-not-proven-to-map-in-both-directions)'s
shape of work rather than this one's. What is certain from reading is that the
bit came from a constructor rather than from the wire.

### T-233 MSE over uTP stalls after the handshake

Source:      measured while building [T-101](bep-coverage.md)'s
             `--transport` flag, 2026-08-24
Category:    peers
Priority:    P1
Effort:      M
Status:      open

Problem:     A torrent does not move between two `bit-cli` sessions when the
             transport is uTP **and** message stream encryption is in use.
             Every other combination of the two works, which is what says this
             is about the pair rather than about either half.

             `scripts/check-transport.ps1`, 32 MiB over loopback, committed at
             `bench/transport-20260824T033000Z.json`:

             | transport | encryption | result |
             | --- | --- | --- |
             | tcp | prefer | 152.38 MiB/s |
             | tcp | require | 160.00 MiB/s |
             | utp | off | 76.19 MiB/s |
             | utp | prefer | **stalls** |
             | utp | require | **stalls** |

             **It stalls in one direction, after the connection is working.**
             Traced on both ends at `--log-level trace`:

             - the leecher connects over uTP and the MSE handshake succeeds,
               with no fallback: nothing logs
               "encrypted handshake failed, dialling again in plaintext";
             - the seeder reads the BitTorrent handshake and decodes it
               correctly, info hash and all, so the cipher is in step;
             - the seeder sends its extended handshake, `HaveAll` and
               `Unchoke`, and the leecher **receives and decodes all three**;
             - the leecher logs `about to send: Message(Interested)` and about
               sixty `Message(Request(..))`, and its uTP stream sends them:
               `sent ST_DATA payload_size=528`, `991`, `662`;
             - the seeder's uTP stream receives **nothing** after the
               handshake, and ten seconds later its inactivity timer fires:
               `reader is dead, could not send UtpMesage to it`.

             So bytes leave one uTP stream and do not arrive at the other,
             after several hundred bytes have crossed in both directions
             successfully.
Relevance:   **It is this repository's own code, not upstream's.** The only
             thing that differs between the working and the failing case is
             whether `MseTransform` wraps the connection:
             `crates/bit-cli-core/src/mse/mod.rs:179`, `StreamTransform`, is a
             fork addition, and `Encryption::Off` returns the streams
             unwrapped while every other policy returns `Prefixed` over the
             read half and `EncryptedWrite` over the write half
             (`crates/bit-cli-core/src/mse/stream.rs`). uTP is what exposes it:
             the same wrappers over TCP move 160 MiB/s.

             **It is P1 rather than P3 because of what it implies about TCP.**
             The two streams differ in how readily they return `Poll::Pending`
             from a write: `vendor/librqbit-utp/src/stream_tx.rs:163` yields
             deliberately every 8,192 bytes, and its `poll_write` returns
             `Pending` rather than `Ok(0)` when its ring is full. A loopback
             TCP socket almost never does either. If the defect is in how the
             wrappers handle a partial or deferred write, then TCP is not
             immune, it is merely never asked, and a congested peer or a slow
             link is the same condition.

             It also blocks half of [T-101](bep-coverage.md): `--transport utp`
             cannot be recommended while the default `--encryption prefer`
             makes it stall.
Approach:    Reproduce it below the session, then bisect the wrapper.

             The two candidates named by the trace, in order:

             - **`EncryptedWrite::poll_write`**,
               `crates/bit-cli-core/src/mse/stream.rs:172`. It buffers the
               ciphertext in `pending` and reports plaintext bytes consumed
               only after `poll_drain` has pushed all of it, which is correct
               for a caller that retries with the same buffer. What it is not
               is cancellation safe: a `poll_write` that returns `Pending` and
               is then dropped leaves `pending` holding ciphertext the caller
               believes was never written, and the next write with a different
               buffer drains the stale bytes and reports the wrong count.
               `a_round_trip_survives_any_write_size` covers partial writes and
               does not cover a cancelled one.
             - **`Prefixed::poll_read_vectored`**, same file. The reader
               (`vendor/rqbit/crates/librqbit/src/read_buf.rs:258`) reads into
               a ring buffer's `unfilled_ioslices()`, which can be two slices
               and can include an empty one, and the decrypt loop assumes the
               inner filled them in order and contiguously.

             **Write the reproduction against a stream that behaves like uTP
             rather than against a duplex pipe**, because a duplex pipe is what
             the existing unit tests already use and they pass. A pair of real
             `librqbit_utp` streams in one process is the fixture, and
             `vendor/librqbit-utp`'s own tests have the setup for it.
Acceptance:  A test below the session moves bytes through `EncryptedWrite` and
             `Prefixed` over a stream that defers writes the way uTP does, and
             fails before the fix. Then
             `mse_over_utp_does_not_carry_a_torrent` in
             `crates/bit-cli-core/tests/transport_e2e.rs` is inverted to assert
             the transfer completes, and `scripts/check-transport.ps1`'s
             `utp-mse` case expects `finished`.

**Pinned, not left silent.** `mse_over_utp_does_not_carry_a_torrent` asserts
the failure and names this entry, beside `mse_over_tcp_carries_a_torrent`,
which is what says the pin is about the pair. A change that makes the transfer
complete fails that test and is read as progress, which is the shape
[T-173](metainfo.md) used.

```bash
cargo test -p bit-cli-core --test transport_e2e
```

## Measured the same day, and it eliminates the Approach's first candidate

**Four paired traces, both ends at `--log-level trace`, and a probe added to
`EncryptedWrite`.** What they establish is narrower and more useful than the
Approach above guessed.

### `EncryptedWrite` is not stuck, and that is measured rather than argued

The probe reports every `poll_write` and what the writer below accepted. On a
failing run the leecher's sequence is:

```
encrypted write accepted written=68     the BitTorrent handshake
encrypted write accepted written=112    the extended handshake
encrypted write accepted written=5      unchoke
encrypted write accepted written=17     a request, and twenty-two more
```

**Every byte handed to the wrapper was accepted by the stream below it, in
order, with no deferral and no error.** So "a `poll_write` that returns
`Pending` and is then dropped leaves stale ciphertext in `pending`" is not what
is happening here: nothing ever returned `Pending`.

The probe is kept, under `--trace handshake`, because it is what the next
attempt will want first.

```bash
bit-cli download <torrent> --peer HOST:PORT --transport utp --encryption require --trace handshake
```

### No bytes are lost, in either direction

Every paired trace agrees to the byte on what the leecher's uTP stream sent and
what the seeder's received: 1,305 and 1,305, then 1,109 and 1,109. **uTP is
delivering everything it is given.** So the failure is not a dropped segment
and not a stream that stops carrying.

### And where it stalls is not the same twice

This is the part that matters for the next attempt, and it is why the trace
above should not be read as one story:

| run | leecher put on the wire | seeder decoded |
| --- | --- | --- |
| `require` | 1,305 B, the MSE handshake only | nothing, "timeout reading" |
| `require` | 1,109 B, the MSE handshake only | nothing, "timeout reading" |
| `require`, probe build | 3,486 B, handshake **and** all the requests | not traced |
| `prefer` | handshake, then the requests | the BitTorrent handshake, the extended handshake, `HaveAll`, `Unchoke`, then nothing |

So sometimes the MSE handshake itself does not complete over uTP, and sometimes
it completes and the peer wire messages that follow are never acted on. **A
first reading of this entry said "the bytes never leave the leecher". That is
true of one run and not of another, and it is corrected here rather than left
standing.**

### What that leaves, and what to do first

The write side is eliminated and the transport is eliminated. What is left is
the **read** side, and the shape of the evidence points at it: bytes arrive,
nothing is lost, and the reader does not make progress on them.

- **`mse::handshake::Buffered`**, `crates/bit-cli-core/src/mse/handshake.rs:146`.
  It reads into a 512 byte chunk and loops until it has what it needs, which
  handles a short read. What it has never been driven by is a stream that
  delivers the same bytes in a different number of pieces, which is exactly
  what uTP does and TCP on loopback does not.
- **`Prefixed::poll_read_vectored`**, `crates/bit-cli-core/src/mse/stream.rs`,
  reached through `read_buf.rs`'s ring buffer, which can hand it two slices and
  can hand it an empty one.

**The fixture is still the thing to build first**, and the Approach's last
paragraph stands: a pair of real `librqbit_utp` streams in one process, driven
through the MSE handshake and then through peer wire traffic. A duplex pipe is
what the existing unit tests use and they pass, so it is not the fixture that
will show this.

---

### T-234 bit-cli cannot present itself as a client a restrictive peer will talk to

Source:      the operator, 2026-08-24, from their own experience of peers that
             answer well known clients and drop everything else. Corpus:
             `RESEARCH.md` entries 23 to 28, and `libtorrent`
             `src/http_tracker_connection.cpp:138` at `v2.0.11` and
             `libtransmission/session.cc:196-206` at `4.1.0`
Category:    peers
Priority:    P2
Effort:      L
Status:      open

Problem:     `bit-cli` announces and handshakes as itself. Some peers and some
             trackers use client identity as an access filter, so a client they
             do not recognise loses swarm it could otherwise reach. There is no
             flag today that changes what is advertised: `man/bit-cli.json`
             carries `--web-seed-user-agent`, which is the HTTP source path
             only, and nothing for the peer id, the tracker request, or the
             handshake.

             **What this is not.** It is not a ratio faker and it does not
             become one. `bit-cli` does not misreport `uploaded`, `downloaded`
             or `left`, does not circumvent a tracker rule, and does not
             inflate a statistic. What it changes is which name it gives, not
             what it says it did. [T-235](trackers.md) exists to hold the other
             half of that line: the numbers a tracker sees are the numbers
             `bit-cli` reports, and it is checked.

Premise:     A client profile is not a string, and an entry that treats it as
             one ships a mask that fails on the second check. Six surfaces
             carry an identity and all six are visible to somebody:

             | surface | what it carries | where it is seen |
             | --- | --- | --- |
             | peer id | the 8 byte prefix and the 12 byte suffix, and the suffix alphabet differs per client | tracker and every peer |
             | tracker HTTP | `User-Agent`, the header set **and order**, the query parameter **order**, `key`, `numwant`, `no_peer_id`, `supportcrypto`, `redundant`, `compact`, the event spelling, `ipv6` | tracker |
             | tracker UDP | `key` and `num_want` | tracker |
             | handshake | the 8 reserved bytes: DHT, fast extension, extension protocol | every peer |
             | BEP 10 | the `m` key **set**, `v`, `reqq`, `p`, `e`, `yourip`, `metadata_size`, `upload_only` | every peer that speaks it |
             | message order | extended handshake against bitfield, and `HaveAll` or `HaveNone` under the fast extension | every peer |

             Two more that are cheaper to get wrong than to get right: MSE
             `crypto_provide`, and the web seed request's `User-Agent`,
             `Accept-Encoding` and `Range` style.

             **The `m` dictionary's key order is not a free variable.** Bencode
             requires dictionary keys in lexicographic order, so the order is
             forced and only the key set and the values are chosen. A client
             that emits them unsorted is fingerprinted by that alone. This
             corrects the surface list this entry was drafted from.

             **The profile has to be derived from the client, never copied from
             another emulator.** `RESEARCH.md` entry 27 is the evidence and it
             is not close: four projects implement qBittorrent's `key` as
             "hash, no leading zero", and libtorrent writes `key=%08X`, so a
             real key starts with `0` one time in sixteen and none of the four
             can produce one. Each reimplementation inherited an algorithm
             named after a rule the client does not have.

Approach:    Three parts, and the first is done.

             **1. The generator, built 2026-08-24.**
             `scripts/make-client-profile.ps1` reads a client's own repository
             at a tag, extracts the version constants and the identity
             construction, and refuses when the construction is no longer the
             one it knows how to read. It derives qBittorrent from
             `src/base/version.h.in` plus the session implementation, and
             Transmission from `CMakeLists.txt` plus
             `libtransmission/session.cc`. Its `-SelfTest` covers the version
             alphabet over its whole range, the eight byte prefix width, the
             Transmission checksum invariant, and that a generated key can
             start with a zero.

             **`-Latest stable` and `-Latest beta` resolve the tag**, sorting
             the tag list by parsed version rather than by the order the API
             returned it, because GitHub does not document that order to be
             either chronological or semantic. A prerelease behind the newest
             stable release is refused rather than offered: a beta nobody would
             run is not a client to imitate.

             **The generator had the defect it exists to catch, and adding
             `-Latest beta` is what found it.** It hardcoded the fourth
             character of both prefixes as `0`. Both clients derive that
             character, and for a prerelease neither derives `0`:

             | | where | stable | beta | dev |
             | --- | --- | --- | --- | --- |
             | Transmission | `CMakeLists.txt:144-163` | `0` | **`B`** | **`Z`** |
             | qBittorrent | `sessionimpl.cpp:1726`, from `QBT_VERSION_BUILD` | `0` | `0` | `0` |

             So Transmission 4.1.0-beta.5 announces `-TR410B-` and the old code
             would have produced `-TR4130-`'s shape with a `0`. qBittorrent is
             the other way round: its peer id does not carry the status at all,
             and its User-Agent does, because `version.h.in:40-44` appends
             `QBT_VERSION_STATUS` to `PROJECT_VERSION` and
             `sessionimpl.cpp:128` builds the agent from that. A qBittorrent
             beta is `-qB5100-` with `qBittorrent/5.1.0beta1`, which is the
             stable release's peer id and the prerelease's agent.

             The profile records that difference as `prerelease_visible`, so a
             caller can tell whether a peer can see which build this is.

             All four values are read from the tag now, each behind a guard
             that asserts the construction producing it. A `Test-Profile` gate
             runs on the finished object before anything is written: eight byte
             Azureus prefix, the client's own two-letter code, every version
             character inside the alphabet, a User-Agent that carries the
             version it claims, and a non-empty record of which files it came
             from. A profile failing any of those is not written, whatever the
             guards said.

             **The canary, built the same day.**
             `scripts/check-client-profile.ps1` runs the derivation for both
             clients at their newest stable and newest prerelease and fails
             when a guard fails. A new release is not a failure; a release
             whose `CMakeLists.txt` no longer builds the prefix from `BASE62`
             is. It is the same instrument as `scripts/upstream-scan.ps1` and
             is deliberately **not** in `scripts/gates.ps1`: it needs the
             network, and a gate that fails when a network is down is a gate
             people learn to ignore.

             **2. The profile document, not built.** One JSON file per client
             and version, generated by the script above and committed the way
             `man/` is, so refreshing the set is a script run rather than a
             release. That is the answer to
             [rustatio Issue 111](https://github.com/takitsu21/rustatio/issues/111),
             where the maintainer refuses a user-set version because it is easy
             to misconfigure and the reporter proposes an API, a hosted file,
             or a plugin system. A committed generated set needs none of the
             three and cannot drift from the client silently, because
             regenerating it is a diff.

             **3. The flag surface, not built.** See the decision below.

Decision:    **The name is `--as-client`.** The operator floated
             `--announce-as` and `--advertise-as`. `--announce-as` loses
             because it is wrong about the scope: the announce is one of six
             surfaces and the smallest one, and a flag named for the announce
             would make a reader think the handshake is unaffected.
             `--advertise-as` is accurate and long, and `advertise` is already
             this repository's word for what a **seeder** does with pieces, so
             it collides in the reader's head. `--as-client <PROFILE>` says
             what it does, is short enough not to need an abbreviation, and
             reads correctly in the negative case, which is
             `--as-client bit-cli`.

             No short flag. `man/bit-cli.json` has ten short flags in use,
             `-O -V -c -d -j -l -o -q -u -v`, and `-c` is `--config`. The
             obvious letters are taken, and a mask is not a flag anybody types
             often enough to want one character for it. Recorded so the
             question is not reopened without a reason.

             **The default is honest, and this is not negotiable.** With no
             flag, `bit-cli` advertises `bit-cli` and its own version. A client
             that lies by default corrupts this repository's own interop and
             bench numbers, which is the one thing it cannot afford: every
             comparative claim here rests on a measured run, and a run taken
             under an undeclared mask measures something else.

             **Whatever is advertised appears in the machine output.** The
             `--json` and `--jsonl` documents carry the profile name, the peer
             id actually used, and the `User-Agent` actually sent, so no
             measurement is silently taken under a mask. `docs/schema.md` gains
             the fields.

             Component overrides, all of which compose over a profile:
             `--peer-id-prefix`, `--user-agent`, and `--peer-id` as the literal
             escape hatch. **`--peer-id` takes bytes**, because a peer id is
             twenty bytes and not a string: `RESEARCH.md` entry 26 records
             KTorrent's, which is `-KT26043-`, then a NUL, then ten characters.

             Discovery from the command line, so a script does not read the
             source: `bit-cli client list` and `bit-cli client show <PROFILE>`,
             which print the profile document. Neither touches the network.

             Naming checked against `man/bit-cli.json` on 2026-08-24: none of
             `--as-client`, `--peer-id`, `--peer-id-prefix` or `--user-agent`
             exists today, and there is no `client` subcommand.
             `docs/flags.md`'s conventions are followed: a noun for what is
             set, no abbreviation, and the value name in the manual.

Prove:       The generator, and this part runs today:

             ```
             pwsh -NoProfile -File scripts/make-client-profile.ps1 -SelfTest
             pwsh -NoProfile -File scripts/check-client-profile.ps1
             ```

             The canary, run on 2026-08-24, with both clients at both kinds:

             ```
             check-client-profile: self-test passes
             check-client-profile: qbittorrent stable : 5.2.3 -qB5230- ua qBittorrent/5.2.3
             check-client-profile: qbittorrent beta : none ahead of stable
             check-client-profile: transmission stable : 4.1.3 -TR4130- ua Transmission/4.1.3
             check-client-profile: transmission beta : none ahead of stable
             check-client-profile: every guard held
             ```

             It was proved able to fail by mutating one guard pattern, which
             produced exit 1 and "CMakeLists.txt no longer carries the BASE62
             table the prefix is built from" before the pattern was restored.

             The two prerelease constructions, derived by naming the tags:

             | tag | prefix | User-Agent | visible to a peer |
             | --- | --- | --- | --- |
             | `4.1.0-beta.5` | `-TR410B-` | `Transmission/4.1.0-beta.5` | yes |
             | `release-5.1.0beta1` | `-qB5100-` | `qBittorrent/5.1.0beta1` | no |

             It derives four qBittorrent versions that agree with joal's
             committed profiles, and refuses two that predate
             `src/base/version.h.in`:

             | version | derived | joal's profile |
             | --- | --- | --- |
             | 4.6.7 | `-qB4670-` | `-qB4670-` |
             | 5.0.0 | `-qB5000-` | `-qB5000-` |
             | 5.1.4 | `-qB5140-` | `-qB5140-` |
             | 5.2.3 | `-qB5230-` | `-qB5230-` |
             | 4.1.9 | exit 2, the file is not in that tag | `-qB4190-` |
             | 3.3.16 | exit 2, the file is not in that tag | `-qB33G0-` |

             The refusal is the designed behaviour and it was checked rather
             than assumed: both tags resolve and
             `repos/qbittorrent/qBittorrent/contents/src/base/version.h.in`
             returns 404 at each.

             For the parts that are not built, the acceptance is a property
             test rather than a golden string, because a golden string is what
             locks a wrong prefix in (`RESEARCH.md` entry 28):

             ```
             cargo test -p bit-cli-core client_profile
             ```

             It must assert, per profile: the peer id is exactly 20 bytes; the
             prefix matches the profile; the suffix alphabet is the profile's
             and not a default; a Transmission style suffix sums to a multiple
             of its base; a libtorrent style key is 8 upper case hex digits and
             **can** begin with `0` over enough draws; and the announce query
             parameters appear in the profile's order.

             Then, end to end, that the mask is what goes on the wire:

             ```
             pwsh -NoProfile -File scripts/check-announce.ps1 -Profile qbittorrent-5.2.3
             ```

             `scripts/check-announce.ps1` exists as of 2026-08-24
             ([T-235](trackers.md)) and records the exact query it received, so
             the profile's parameter order is checkable against a real request
             rather than against a unit test's idea of one.

Notes:       Licence. `joal` is Apache-2.0, not MIT.
             `scripts/make-client-profile.ps1` is an independent implementation
             written from the observed behaviour of
             `joal/scripts/bittorrent-client-update-detector/`, cited in that
             script's header with the SHA. Nothing is copied, so
             `THIRD_PARTY.md`, `about.toml` and `deny.toml` are untouched.

             What the generator does that the original does not, which is the
             "make it better" the operator asked for:

             - **The version to character encoding is table driven and tested
               over its whole range.** joal's qBittorrent script concatenates
               decimal (`qbittorrent_analyzer.sh:445`), so 3.3.13 gives a nine
               byte prefix; its Transmission script writes
               `BASE62=($(echo {0..9} {A..A} {a..z}))` (`transmission.sh:79`),
               which is 37 entries and not 62.
             - **Every value it extracts is used.** joal's qBittorrent script
               greps the user agent, the peer id line and the key format, then
               emits a fixed template that uses none of them
               (`qbittorrent_analyzer.sh:553`).
             - **A disagreement between the tag and the constants is a
               failure**, not something to paper over.
             - **The profile records what it did not derive.** The peer wire
               fields are marked as needing a live client rather than left
               absent, so a reader cannot mistake silence for zero.

             The guard discipline is joal's and is kept:
             `transmission.sh:66` asserts the upstream construction before
             trusting its own derivation, which is the difference between "the
             client did not change" and "I could not find the line".

Ruled:       **2026-08-24. All three points accepted as designed.**

             The default stays honest, so `bit-cli` advertises itself unless
             told otherwise. Whatever is advertised appears in the machine
             output, so a measurement is never silently taken under a mask. The
             flag is `--as-client`, and `--announce-as` and `--advertise-as`
             stay rejected for the reason already recorded: each names half of
             what moves, and a mask moves the peer id, the User-Agent, the
             query order and the key alphabet together.

             An option to suppress the mask line from `--json` was put to the
             operator and refused, which is the answer that keeps this entry
             from becoming a way to take an unattributable measurement.

---

### T-236 bit-cli announces under two peer ids and neither one is bit-cli

Source:      found by running [T-235](trackers.md)'s
             `scripts/check-announce.ps1`, 2026-08-24
Category:    peers
Priority:    P1
Effort:      S
Status:      **done**, 2026-08-24

Problem:     One binary announces under two different client identities, and
             one of them belongs to a real client that is not this one.

             | command | prefix | what a tracker reads it as |
             | --- | --- | --- |
             | `download`, `seed` | `-rQ9010-` | `rqbit` 9.0.1 |
             | `trackers`, `bench probe` | `-BC0100-` | **BitComet** 1.0.0 |

             `-rQ9010-` comes from the vendored session:
             `vendor/rqbit/crates/librqbit/src/session.rs:603` calls
             `generate_azereus_style(*b"rQ", crate_version!())` when
             `SessionOptions::peer_id` is `None`, and `bit-cli` never sets it.
             So the version in every announce this repository makes is
             `librqbit`'s version rather than `bit-cli`'s, and it moves when
             the vendored tree is bumped.

             `-BC0100-` is `bit-cli`'s own, at
             `crates/bit-cli/src/cmd/trackers.rs:570` and
             `crates/bit-cli/src/cmd/bench.rs:171`. libtorrent's registered
             prefix table maps it to BitComet:
             `src/identify_client.cpp:161`, tag `v2.0.11`, `{"BC", "BitComet"}`.

             The comment above the first one says the prefix exists "so a
             tracker's client statistics attribute the announce correctly
             rather than filing it under 'unknown'". **That is the premise this
             disproves**: it does attribute the announce, to BitComet.

             `the_peer_id_is_azureus_style_and_printable` at
             `crates/bit-cli/src/cmd/trackers.rs:766` asserts the prefix is
             exactly `-BC0100-`, so the wrong value is pinned by a passing
             test. That is the shape `RESEARCH.md` entry 28 records in somebody
             else's tree, arrived at here independently.

Premise:     Measured rather than read. `scripts/check-announce.ps1` prints the
             peer id the tracker received:

             ```
             peer id:     -rQ9010-i%a4\%a3%f3%06%fd%86%c9%d5%a2%fa
             ```

             The suffix is percent-escaped by the fixture's own `printable`
             helper because a peer id is twenty arbitrary bytes.

Approach:    Pick a two character code that is not taken, set it in one place,
             and make both paths read it.

             **The code is not free to choose.** libtorrent's table at
             `src/identify_client.cpp` is the closest thing to a registry, and
             `aquatic/crates/peer_id/src/lib.rs:100-120` is a second
             implementation of the same list. Any candidate is checked against
             both before it is used. `-bC` and `-BT` and `-BX` are taken;
             `-bt` should be assumed taken until checked.

             One constant, used by the session and by the two commands that
             roll their own, so a third path cannot appear with a fourth
             identity. `SessionOptions::peer_id` is `pub` and takes it, so no
             patch to `vendor/` is needed. `patches/UPSTREAM.md` gains nothing.

             The version digits are `bit-cli`'s own, encoded one component per
             character the way `RESEARCH.md` entry 23 records and
             `scripts/make-client-profile.ps1` implements, so a component of
             ten or more does not widen the prefix.

             This is the honest default that [T-234](peers.md)'s decision
             names. T-234 is what makes it optional; this is what makes the
             default true.

Prove:       ```
             pwsh -NoProfile -File scripts/check-announce.ps1
             ```

             Its printed peer id must carry the chosen prefix, and the same
             prefix must appear for `bit-cli trackers`:

             ```
             cargo test -p bit-cli one_peer_id_prefix_for_every_command
             ```

             The test asserts the two call sites and the session option resolve
             to the same eight bytes, so a third path added later fails rather
             than diverging quietly. The existing
             `the_peer_id_is_azureus_style_and_printable` is inverted to hold
             the new value rather than deleted, which is the rule
             [RULES.md](RULES.md) section 5 states under testing and
             [T-020](peers.md) is the worked example of.

Correction:  **Two undercounts, both found by grepping for the prefix rather
             than by trusting the table above.**

             It was **six** identities, not two, and five of the six claimed
             BitComet's code:

             | where | prefix |
             | --- | --- |
             | the session, `SessionOptions::peer_id` left `None` | `-rQ9010-` |
             | `bit-cli trackers` | `-BC0100-` |
             | `bit-cli bench probe` | `-BC0100-` |
             | the web seed bridge, `webseed/bridge.rs:48` | `-BCws01-` |
             | the swarm bench's synthetic peer, `bench/swarm.rs:96` | `-BCsw01-` |
             | the listener health check, `listener.rs:51` | `-BClc01-` |

             Only the first three reach a tracker or a remote peer. The other
             three are loopback inside one process, and they are fixed anyway:
             an identity that is wrong in a log is still wrong, and the point
             of one module is that a seventh cannot appear.

             There was a seventh, at `listener.rs:194`, and it is **not** ours:
             a test fixture standing in for whatever remote peer answers. It
             said `-BCzz01-` and now says so in a comment as well as in its
             bytes, because a fixture replying with this client's own prefix
             would hide a self-connect rather than exercise one.

             **And `-bC` is not taken.** The entry said it was. Checked
             against libtorrent `v2.0.11` `src/identify_client.cpp:148-250`,
             which carries **92** Azureus-style codes, against
             `aquatic/crates/peer_id/src/lib.rs:100-120`, and against the four
             other implementations of the same table in the corpus, in
             `seedchamp`, `torrust-actix`, `gosh-dl` and `superseedr`. `bC`,
             `bt`, `bl`, `bi`, `CL` and `cl` are free in all six.

Closed:      `crates/bit-cli-core/src/peer_id.rs` is the one place, and every
             one of the six reads it.

             **The code is `CL`**, and it was chosen on the one property that
             separated the candidates. Every code containing `b` has a case
             twin already in the registry, and the lookup is a byte comparison
             so a twin is legal but confusable: `bC` twins `BC`, which is the
             client this was being mistaken for, `bt` twins `BT` (BitTorrent
             mainline), `bl` twins `BL` (BitBlinder), `bi` twins `BI`
             (BiglyBT). `CL` has no twin in any of the six registries in
             either case, and it reads as the command line, which is the one
             thing that distinguishes this client from every entry in that
             table.

             The version is `bit-cli`'s own and is built at compile time from
             `CARGO_PKG_VERSION_*`, so `-CL0200-` moves when this crate does
             and never when the vendored tree does. Two compile-time
             assertions rather than runtime checks: a version component past
             61 has no single-character encoding and fails the build, and a
             prerelease version fails the build because the fourth character
             is still `0` and Transmission puts `B` or `Z` there. Both are
             raised in the release that would have needed them.

             **The suffix is printable now**, twelve characters from the
             operating system's generator. It was twelve raw bytes on the
             session path, which is why the peer id in the check's own output
             used to be half percent escapes. Two of the six generators seeded
             themselves from `SystemTime::now()` and one derived all twelve
             characters from a single nanosecond reading.

Prove:       ```
             pwsh -NoProfile -File scripts/check-announce.ps1
             ```

             Every judged case holds and the printed identity is
             `peer id:     -CL0200-nnznnl2zn5d2`, against `-rQ9010-` and a
             percent-escaped suffix before. `bit-cli trackers` against
             `loopback-tracker --announce-log` recorded `-CL0200-uk3i5zyavz6d`
             on both of its announces, which is the second half of the table
             the problem statement above names.

             ```
             cargo test -p bit-cli one_peer_id_prefix_for_every_command
             ```

             Five more in `bit_cli_core::peer_id`, and one of them is the
             guard that matters: `the_client_code_is_not_one_a_registry_already_names`
             carries all 92 of libtorrent's codes plus `rQ` and the corpus's
             extras, copied rather than fetched so it does not need a network,
             and fails if anybody moves `CLIENT_CODE` onto a taken one.

---

### T-238 NAT traversal beyond the BEPs, and what a relay would actually buy

Source:      the operator, 2026-08-24, reopening the ruling in
             [RULES.md](RULES.md) section 6. Corpus: `RESEARCH.md` entries 30
             to 37, and entry 1's `torrent/NOTES.md:15-31`
Category:    peers
Priority:    P2
Effort:      L
Status:      open, **and it needs an operator ruling before it is workable**

Problem:     `bit-cli` is reachable to a peer that can dial it. Behind a NAT it
             is reachable only to peers it dials first, and behind a symmetric
             NAT or carrier grade NAT it is often reachable to nobody. BEP 55
             is [T-102](bep-coverage.md) and is not implemented. Nothing beyond
             BEP 55 is implemented either, and the operator's ruling is that
             compliance is the floor rather than the ceiling.

Premise:     **What each mechanism does on each NAT shape**, from the corpus
             and stated per shape because that is where the differences are.

             | NAT shape | direct inbound | UPnP or NAT-PMP or PCP | BEP 55 holepunch | port prediction | relay |
             | --- | --- | --- | --- | --- | --- |
             | none, public | works | not needed | not needed | not needed | not needed |
             | full cone | fails until mapped | works when the gateway answers | works | not needed | works |
             | restricted cone | fails | works when the gateway answers | works | not needed | works |
             | port restricted | fails | works when the gateway answers | works | not needed | works |
             | symmetric | fails | works when the gateway answers | **fails** | sometimes | works |
             | carrier grade NAT | fails | **fails**, there is no gateway to ask | **fails** | sometimes | works |

             **BEP 55 fails on exactly two shapes and they are the two that
             matter.** It works by having a peer that can already see both ends
             tell each of them the other's `address:port`. On a symmetric NAT
             the external port is allocated per destination, so the port the
             rendezvous peer observed is not the port the target will see. On
             carrier grade NAT there is no gateway the client can ask for a
             mapping, so the port mapping protocols fail as well.

             **Port prediction is the only mechanism in the corpus that
             addresses the symmetric case without a relay**, and
             `RESEARCH.md` entry 31 is the worked example: probe the NAT
             several times, model `delta = public_port - local_port`, predict,
             widen by the observed deviation, and for a progressing allocator
             shift forward by an estimated rate. Its own README declines to
             claim a success rate and says results depend on the devices. Its
             cost is a burst of outbound connection attempts, which for a
             BitTorrent client is the shape [T-020](peers.md) already measured
             as the one that strands sockets.

             **A relay always works and is always the most expensive.**
             `RESEARCH.md` entry 30 ranks it sixth of six for that reason, and
             separates a relay used for **signalling** from a relay that
             carries the **data**. eD2k made the same separation in 2002:
             entry 33's server "only exchanges small address packets, it never
             relays file data".

Approach:    **The recommendation, and the operator's ruling is what this
             entry needs.**

             **Recommend: do not adopt `iroh`. Adopt the ladder instead, and
             implement its rungs here.**

             The measured cost of `iroh` 1.0.3, taken on 2026-08-24 by
             resolving it in a throwaway crate outside the tree:

             | | |
             | --- | --- |
             | crate | `iroh` |
             | version | 1.0.3 |
             | licence | `MIT OR Apache-2.0`, which `deny.toml` already allows |
             | direct required dependencies | 43, from the sparse index |
             | crates added to this tree | **113**, against a current 302 |
             | what it would replace | nothing |

             That last row is the argument. `iroh` does not replace TCP, uTP,
             the tracker, the DHT or PEX. It is additive, and what it adds is
             reachability **to other iroh endpoints**.

             **The reason is addressing, not size.** An `iroh` peer is an
             `EndpointId`, an ed25519 public key (`RESEARCH.md` entry 32,
             `iroh-fm/crates/server/src/iroh_rpc.rs:47-58`). A BitTorrent peer
             is an `IP:port` handed over by a tracker, the DHT or PEX. There is
             no form of `EndpointAddr` a qBittorrent peer could dial, and no
             way to publish an `EndpointId` through BEP 5, BEP 11 or a tracker
             response that any other client would understand. So a `bit-cli`
             that speaks `iroh` is reachable to other `bit-cli` instances and
             to nothing else in the swarm. `dig-nat` (entry 30) has the same
             shape with a different key, and so does Hollow (entry 34).

             **What a standards-only peer sees, per mechanism**, which the
             operator's ruling requires each one to state:

             | mechanism | what a peer that speaks only the BEPs sees |
             | --- | --- |
             | UPnP, NAT-PMP, PCP | an ordinary peer at an ordinary `IP:port`. The mapping is invisible on the wire. |
             | BEP 55 holepunch | an ordinary peer, and the `ut_holepunch` extension it may ignore |
             | port prediction | an ordinary inbound TCP connection. The prediction happens before the connection exists. |
             | relay for signalling only | an ordinary peer at an ordinary `IP:port` once the punch lands |
             | relay carrying data | an ordinary peer, at the relay's address rather than ours |
             | `iroh` transport | **nothing. It cannot connect at all.** |

             The last row is why it is refused. Every other mechanism degrades
             to plain BEP 55 and plain TCP or uTP; that one does not degrade,
             it disappears.

             **What to build instead, in this order**, each rung useful alone:

             1. **Port mapping.** NAT-PMP (RFC 6886) and PCP (RFC 6887) are
                small fixed-layout datagrams and are unit-testable with no
                network, which is entry 30's own argument at
                `dig-nat/Cargo.toml:7-14`. UPnP/IGD is the one that is large
                enough to want a crate.
             2. **BEP 55**, which is [T-102](bep-coverage.md) and unchanged by
                this entry. Its inline flow still looks right. Two things from
                entry 33 are worth folding in as design input rather than as a
                wire change: an explicit initiator role, and an enumerated
                failure reason.
             3. **The ladder.** Direct, then mapping, then BEP 55, then relay,
                ranked and each bounded by its own timeout, with a failure that
                carries every rung's reason. Entry 30's
                `src/strategy.rs:58-110` is the shape.
             4. **Port prediction**, last and behind a flag that is off by
                default, because its cost is a burst of connection attempts.

             **A relay is a separate ruling and is not recommended here.** It
             costs an operator, a trust assumption, and a new failure mode when
             the relay is the thing being censored. It is the rung to add when
             1 to 4 are measured and found insufficient, and not before.

Decision:    **Reopened, and this entry does not close it.** What the operator
             is being asked to rule on:

             a. Is the recommendation accepted: no `iroh`, build the ladder?
             b. Is a relay in scope at all, given that it needs somebody to run
                it and a trust assumption to state?
             c. Is port prediction acceptable given that its cost is a burst of
                connection attempts at a NAT?

             Recommended answers: a. yes. b. not yet, and revisit when the
             ladder is measured. c. yes, behind a flag that is off by default.

Ruled:       **2026-08-24. a. yes. b. yes, and more than one. c. yes.**

             b. is the one that went against the recommendation, so it is the
             one written out. The relay rung is in scope, it is **several
             relays rather than one**, and they are ranked by how widely
             deployed the provider is, on the operator's reasoning that a
             widely deployed provider is the one most likely to be reachable
             from a censored network and least likely to be the thing that
             disappears.

             That makes the protocol choice, not the vendor choice, the first
             piece of work. **TURN, RFC 8656**, is the rung to build against:
             it is the only relay protocol with more than one provider, it is
             a published standard rather than one project's wire format, and a
             client that speaks it can be pointed at whichever provider ranks
             highest without a code change. DERP, which is `n0`'s, is the
             counter-example and shows why the protocol comes first: it has one
             provider and it needs that project's client.

             **Speaking a relay protocol does not mean taking the `iroh`
             crate**, and this entry's refusal of `iroh` is unchanged and is
             for the reason it already gives: BitTorrent has nowhere to put a
             node id. A TURN allocation carries an ordinary transport address,
             which is exactly what the peer protocol already has a field for.

             **What a standards-only peer sees**, which
             [RULES.md](RULES.md) section 6 requires each mechanism to state:
             an ordinary TCP or uTP connection arriving from the relay's
             address. The relayed peer is not distinguishable from a peer
             behind a NAT that happened to work, and no BEP is extended,
             deprecated or reinterpreted to make it work.

             **The trust assumption, stated rather than implied.** A relay
             learns the pair of addresses it is joining and how many bytes pass
             between them. It does not learn the info hash or the payload while
             MSE is on, because it forwards ciphertext it has no key for. It is
             a metadata observer and not a content one, and that sentence is
             what has to appear in the user-facing documentation beside the
             flag, not only here.

             **Ranking needs a definition before it needs code.** "Most
             popular provider" is not a number a program can read. The rank
             this builds against is: reachability measured from this machine
             first, then provider count on the same protocol, then published
             deployment size. The first of those is the only one a run can
             measure, which is why [T-239](#t-239-nothing-says-what-shape-of-network-bit-cli-is-on-or-whether-a-peer-path-is-direct)
             stays the prerequisite for the whole entry.

Prove:       Nothing here is provable on loopback, which is the honest
             statement of why this entry is L and why it is not started.
             Loopback has no NAT. What the first rung needs is a measurement
             against a real gateway:

             ```
             bit-cli net probe --json
             ```

             That subcommand is [T-239](peers.md), and it is the prerequisite
             for this entry rather than a part of it: until `bit-cli` can say
             what shape of NAT it is behind, nothing here can be shown to have
             changed anything.

Notes:       [RULES.md](RULES.md) section 6's iroh paragraph is rewritten by
             this session and the superseded text is in
             `reference/HISTORY/RULES-section-6-iroh.md`. The old ruling said
             "do not reach for a NAT crate" and the new one says NAT crates are
             candidates. **The recommendation happens to land in the same
             place for `iroh` specifically, and it lands there for a different
             reason**: the old text said BEP 55 needs no NAT library, which is
             true and is not the whole question; this says `iroh` cannot carry
             a BitTorrent peer because BitTorrent has nowhere to put a node id.

             `reference/README.md`'s "BEP 55 holepunch, and the iroh question"
             is reconciled against this entry in the same push.

---

### T-239 Nothing says what shape of network bit-cli is on, or whether a peer path is direct

Source:      the operator, 2026-08-24, asking whether a network diagnostic
             subcommand is needed. Corpus: `RESEARCH.md` entries 30, 32 and 34,
             and [net4people/bbs issue 491](https://github.com/net4people/bbs/issues/491)
Category:    peers
Priority:    P2
Effort:      M
Status:      open

Problem:     Two questions `bit-cli` cannot answer today, and both block
             [T-238](peers.md) rather than following it.

             **What shape of NAT is this?** `bench probe` measures one peer or
             one HTTP endpoint. Nothing measures the local network. Without it
             a traversal mechanism cannot be shown to have changed anything,
             because the shape that decides whether it helps is unknown.

             **Is this peer path direct?** `bit-cli peers` reports that a peer
             is connected. After a hole punch, direct against relayed is the
             whole question and the report cannot express it.

Premise:     **No tree in the corpus classifies a NAT.** `dig-nat`'s
             `src/stun.rs` discovers a reflexive address and stops
             (`RESEARCH.md` entry 30); Hollow's 768 line `stun.rs` does the
             same (entry 34); iroh reports the path it selected but not the NAT
             it is behind. There is no RFC 3489 behaviour test anywhere in the
             thirty-seven trees. So this is new work rather than a port, and
             saying so is the reason the entry exists.

             **The classification is four STUN exchanges, not a library.**
             Bind one UDP socket and keep it:

             | test | what it shows |
             | --- | --- |
             | binding request to server A, port 1 | the reflexive address, and whether it equals the local address, which is "no NAT" |
             | binding request to server A, **port 2** | a different external port for the same socket means the mapping is address-and-port dependent, which is symmetric |
             | binding request to server B | a different external port again confirms it; the same port narrows it to a cone |
             | ask A to reply from a different address and port | whether an unsolicited source is accepted, which separates full cone from restricted |

             The first two are the ones that matter, because symmetric is the
             shape [T-238](peers.md) says BEP 55 cannot cross.

Approach:    **`bit-cli net probe`**, a new subcommand under a new `net` verb,
             read only, no torrent required, and one that touches the network
             by definition. Checked against `man/bit-cli.json` on 2026-08-24:
             there is no `net` command and no `probe` at the top level, and
             `bench probe` is a different thing that stays where it is.

             What it reports, in `--json` and as a table:

             - the local addresses per family, and which the OS would use
             - the reflexive address per family, and the STUN servers asked
             - the mapping shape from the tests above, named rather than scored
             - whether the gateway answers UPnP, NAT-PMP or PCP, and what
               mapping it offered, released immediately
             - whether the configured or default trackers answer, per protocol
             - whether the DHT bootstrap nodes answer
             - whether an inbound TCP and an inbound uTP connection arrive on
               the announced port, which needs a reflector and is the one part
               that cannot be done alone

             **Scope, and this is where the brief is challenged.** The operator
             asked about censorship and blocking behaviour and about a trace
             route style hop list. Both are declined here, with the reason.

             [net4people/bbs issue 491](https://github.com/net4people/bbs/issues/491)
             is a 2026 request for exactly that tool, and the answers in the
             thread are the evidence: OONI Probe already does the measurement
             and is deployed, Tor's `emma` does the narrower reachability
             version, and a commenter's summary of automatic protocol selection
             is that "the different tools/protocols are far too fragmented" for
             it to work yet. A BitTorrent client is not where that gets solved,
             and a half-built censorship scanner inside one is worse than none:
             it invites a user to trust a verdict it cannot support.

             What is in scope is narrower and is `bit-cli`'s own business:
             **can this client reach a swarm from here, and by what path.**
             That is answerable, bounded, and it is what [T-238](peers.md)
             needs. A `blocked` verdict is only ever reported per named
             endpoint that was actually tried.

             The hop list is declined for a different reason: a traceroute
             needs raw sockets or `SOCK_DGRAM` with `IP_TTL` and elevated
             privileges on Windows, and this repository's headless parity rule
             means a subcommand that needs administrator on one platform and
             not on another is a subcommand that behaves differently by
             platform.

             **The second half is the peer path label**, and it is separable
             and much cheaper. `RESEARCH.md` entry 32 has the shape at
             `iroh-fm/crates/server/src/iroh_rpc.rs:411-437`: per connection,
             report whether the selected path is direct or relayed, with its
             round trip time, and log it **only when it changes**. `bit-cli`
             should carry `path` on each peer row with `direct`, `holepunched`
             or `relayed`, and it applies to BEP 55 whether or not any relay is
             ever adopted.

Prove:       ```
             bit-cli net probe --json
             ```

             On this machine, with a known NAT, it must name the mapping shape
             and the reflexive address, and its exit code must be 0 when it
             could measure and non-zero when it could not reach any STUN
             server. A run with no network must fail loudly rather than
             reporting "no NAT".

             And a new check beside the existing ones, `check-net-probe.ps1`.
             It is named without its directory here because it does not exist
             yet: `scripts/check-todo.ps1` resolves every `scripts/...` path a
             `TODO/` file writes, so the resolvable form arrives with the file.

             The check has to work without a real NAT, so it drives a loopback
             STUN responder that answers with a scripted mapping and asserts
             the classifier names the shape that responder was configured to
             imitate. That is the same pattern `scripts/check-swarm.ps1` uses
             for a synthetic peer: the fixture is the thing that makes the
             classification checkable rather than the network.

Notes:       This is the prerequisite for [T-238](peers.md) rather than a part
             of it. It is also useful on its own: "my download finds no peers"
             is the most common thing a user cannot diagnose, and today
             `bit-cli` has nothing to say about it.
