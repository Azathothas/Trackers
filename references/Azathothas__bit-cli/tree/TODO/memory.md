# Memory

Sixteen issues touch memory growth, per-torrent overhead, and buffer pooling.

---

### T-040 Memory and descriptors grow without bound over a long run

Source:      https://github.com/ikatson/rqbit/issues/525 (open)
Category:    memory
Priority:    P0
Effort:      L
Status:      **done**, 2026-08-22T22:50Z

Problem:     A reporter running `librqbit` inside a long-lived server saw both
             RSS and open descriptors climb until the process failed. It
             started after changing trackers, which points at the tracker or
             peer discovery path rather than at storage.
Relevance:   The netdisk deployment is a long-lived process. This is the
             failure mode that takes it down at 3am.
Approach:    Related to [T-011](disk-io.md) and [T-020](peers.md), and possibly
             the same defect seen from three angles. Do not guess: run one
             `bit-cli seed` for six hours with a sampler recording RSS, handle
             count, and socket state counts every 30 seconds, and plot it. A
             flat line closes this; a slope names the subsystem.
Acceptance:  `scripts/soak.ps1` writes `bench/soak-<timestamp>.csv` with the
             three series, and this entry records the slope of each over six
             hours.

**Where this stands, 2026-08-21.** Read this first; the rest of the entry is
the history in order, and its earlier summaries were true when they were
written.

- **Descriptors: disproved.** An idle seeder holds exactly 189 handles across
  533 samples over 4.6 hours, and a loaded one shows no trend. `CLOSE_WAIT` is
  zero at all 1,064 samples across both runs.
- **Memory: reproduced, quantified, linear.** 0.804 MiB an hour under `steady`,
  r squared 0.73 over 525 samples, and the last three hours give the same
  slope. Not a settling curve.
- **Attribution: answered, 2026-08-22, and not by the run this entry called
  for.** Most of the byte is the peer row `librqbit` keeps for every peer it
  has ever accepted and never reclaims. **2,891 bytes a row**, measured over
  2,000 rows at r squared 0.94, which at the soak's 228.5 completions an hour
  is 0.63 MiB an hour against the measured 0.804. See
  [the 2026-08-22 section](#session-of-2026-08-22-the-slope-is-peer-rows).
- **Bounded, and the bound is measured over six hours.** `MAX_PEER_RECORDS`,
  1,024 per torrent, in the vendored tree. The slope is **+0.909 MiB/h while
  the records accumulate and flat once they stop**, and the break is at the
  instant the map fills. See
  [the six hour run](#the-six-hour-run-2026-08-22-and-the-bound-holds), which
  is what closed this. `--max-rss` is still carried as a backstop.

The evidence and the fits are in
[the 2026-08-21 section](#session-of-2026-08-21-the-question-is-answered-and-the-answer-is-linear)
and [the 2026-08-22 one](#session-of-2026-08-22-the-slope-is-peer-rows).

---

**The harness is built and a 1.76 hour run is recorded. The six hour run the
acceptance asks for has not been completed, so this stays open.** (2026-08-20;
superseded by the section above.)

`scripts/soak.ps1` samples one long-lived `bit-cli seed` every
`-SampleSeconds` and writes `bench/soak-<timestamp>.csv` with resident memory,
peak resident memory, handles, threads, CPU time, and the TCP socket states
broken out by state. Six workloads, because a slope has to name a subsystem
rather than "the process":

| workload | what it drives |
| --- | --- |
| `idle` | a seeder with no tracker and nothing connecting. The control. |
| `announce` | a loopback tracker at a five second interval. The tracker never expires a peer, so the peer list handed to the seeder grows for the whole run, which is the path this entry's report points at. |
| `leech` | real downloads against the seeder, one finishing and another starting. |
| `steady` | announce and leech together. The deployment, and the default. |
| `churn` | connections that open and close without handshaking. T-020's shape, and the known positive. |
| `all` | steady plus churn. |

`all` is deliberately not the default. Churn strands sockets at about 30,000
handles an hour, which is [T-020](peers.md) rather than this entry and swamps
every other series in the same chart. It also starves the leechers: the same
run that completed 22 downloads in two minutes without churn completed 1 and
failed 2 with it.

Two things the harness does that are worth keeping. It runs from its own copy
of `target/release/bit-cli.exe`, because a six hour run would otherwise hold
that file for six hours and Windows will not let `cargo` replace a running
executable. And the seeder reports its own RSS and handle count in every
`progress` event under `--jsonl`, so the summary cross-checks the sampler
against the subject: a sampler that disagrees with the process is measuring
something else.

**The measurement so far**, `bench/soak-20260820T132757504Z.csv`, workload
`steady`, 16 MiB payload, two leechers, 30 second samples, **1.76 hours and 398
completed leech cycles**:

| series | first | last | max | per hour | r squared |
| --- | --- | --- | --- | --- | --- |
| `rss_bytes` | 14.81 MiB | 16.31 MiB | 17.03 MiB | **+0.58 MiB** | 0.63 |
| `peak_rss_bytes` | 14.94 MiB | 17.27 MiB | 17.27 MiB | +0.85 MiB | 0.85 |
| `handles` | 210 | 216 | 240 | **+0.77** | 0.004 |
| `threads` | 29 | 27 | 35 | -0.02 | 0.00 |
| `tcp_total` | 1 | 1 | 2 | +0.01 | 0.001 |
| `tcp_close_wait` | 0 | 0 | **0** | 0 | n/a |
| `cpu_ms` | 156 | 30,438 | | +17,156 | 0.9995 |

What that says, and what it does not.

- **Descriptors are flat.** 0.77 handles an hour at an r squared of 0.004 is
  noise, not a trend, and `CLOSE_WAIT` was zero at every one of the 200
  samples. So the half of this entry that names descriptors is not reproducing
  under a deployment-shaped load. [T-011](disk-io.md) bounding open files with
  `--max-open-files` is the likeliest reason.
- **CPU is flat as a rate.** 17,156 ms of CPU per hour is 4.8 ms per second of
  wall time, under 0.5% of one core, and the r squared of 0.9995 says it is a
  straight line rather than an acceleration.
- **Memory rises, slowly, and the fit is weak.** 0.58 MiB an hour at an r
  squared of 0.63 over 1.76 hours is about 14 MiB a day if it is linear, and
  1.76 hours is not long enough to say whether it is linear, a settling curve,
  or an allocator that has not returned pages yet. `peak_rss_bytes` is a
  high-water mark, so its slope is bounded below by zero and says less than its
  r squared suggests.

**What is left is the run, not the harness.** Six hours of `steady`, and an
`idle` control of the same length to separate the session's own timers from
the load:

```powershell
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 360 -Workload steady -PayloadMiB 16
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 360 -Workload idle
```

Once both are in, the ceilings turn the record into a check:
`-RssCeilingMiBPerHour`, `-HandleCeilingPerHour`, and
`-CloseWaitCeilingPerHour` each fail the run when the slope passes them, and
with none named the slopes are recorded rather than judged.

One residue in the harness itself: the summary JSON is written only when the
sampling window ends, so a run that is killed early leaves the CSV and no
summary. The numbers above were computed from the CSV by hand for that reason.
Writing the summary on every sample, or on a signal, is a small change and
would have saved that step. **Done in the session below, and it was needed
within the hour.**

**Session of 2026-08-20, second run: the harness is fixed, an idle control is
in, and the six hour run is still the thing that is missing.**

Two runs were started together, `steady` and `idle`, so the load could be
separated from the session's own timers. Neither reached six hours before the
session ended, and what they did reach is recorded here because the summary is
now written after every sample rather than only at the end. That change is the
harness residue this entry named, and it paid for itself on the first run: the
`steady` run died at 2.26 hours and its record survived.

| series | steady, 2.26 h, 258 samples | idle, 2.76 h, 315 samples |
| --- | --- | --- |
| `rss_bytes` per hour | **+0.93 MiB**, r squared 0.65 | **-0.15 MiB**, r squared 0.11 |
| `rss_bytes` first, last, max | 14.75, 18.23, 20.19 MiB | 13.14, 12.38, 13.67 MiB |
| `handles` per hour | +2.09, r squared 0.015 | **0.00**, and 188 at every sample |
| `tcp_close_wait` max | **0** | **0** |
| leech cycles | 514 | none by design |

**The idle control is the new fact.** A seeder with no tracker and nothing
connecting holds 188 handles at every one of 315 samples over 2.76 hours, and
its resident memory does not rise: the slope is slightly negative at an r
squared of 0.11, which is a flat line with noise on it. So whatever the `steady` run is doing, it
is the load doing it and not the session's timers, and this entry's report of
descriptors climbing on their own does not reproduce at all.

**The `steady` slope is still not a straight line.** 0.93 MiB an hour at an r
squared of 0.65, with a maximum of 20.19 MiB against a last reading of 18.23,
is a series that rises and falls rather than one that climbs. Two and a half
hours cannot separate a settling curve from a leak, which is exactly what the
six hour run is for and why this stays open.

*That reading was wrong, and the next section says why.* The maximum above the
last reading is one thread burst, not the series changing direction. Excluding
it the slope is 0.804 MiB an hour at an r squared of 0.73, and it is a line.

Both runs also shared the machine with a full `cargo build --release` and the
test suite, several times over. That is worth knowing when reading the RSS
series: the leech cycles compete with whatever else is running.

**One harness defect, found the hard way.** The first `steady` run ended at
2.26 hours with `ScriptHalted`. `Start-Process` for the next leecher threw,
almost certainly on the redirected output file the previous leecher had not
finished releasing, and with `$ErrorActionPreference = 'Stop'` and a trap above
it that one throw ended a six hour run. Fixed two ways: starting a process
retries three times before giving up, and the whole sampling body is inside a
`try` that counts a failure and carries on. The count is `cycles.load_errors`
in the summary, because a run with a hundred of them is measuring something
else.

**What the next session does.** Both commands, from a clean tree:

```powershell
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 360 -Workload steady -PayloadMiB 16 -Root .tmp/soak-steady
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 360 -Workload idle -Root .tmp/soak-idle
```

Start them first, before anything else, and leave the machine as quiet as the
rest of the work allows. `bench/soak-<timestamp>.json` is readable while the
run is going: `complete` is `false` until the window ends, and the slopes in it
are the slopes so far. When both are in, put the numbers in the table above,
answer the one open question, and set the ceilings:

```powershell
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 360 -Workload steady `
  -RssCeilingMiBPerHour <answer> -HandleCeilingPerHour <answer> -CloseWaitCeilingPerHour 1
```

The question is only whether the `steady` RSS slope is linear, a settling
curve, or an allocator holding pages. A slope that flattens after the first
hour is the second; one that holds 0.9 MiB an hour to hour six is the first and
names a leak worth chasing.

Runs recorded so far, all partial, all under `bench/`:
`soak-20260820T132757504Z` (steady, 1.76 h, the first),
`soak-20260820T155246381Z` (steady, 2.26 h, killed by the harness defect above),
`soak-20260820T155309362Z` (idle, 2.76 h, the control, stopped with the
session), and
`soak-20260820T181505020Z` (steady, restarted at 18:15 UTC and still running
when the session ended, so its files are not committed).

The restarted run is the one to look for: if `.tmp/soak-steady` and its
bench files are still on the machine, read the JSON before starting another,
because a run that reached five hours is worth more than a fresh one.

## Session of 2026-08-21: the question is answered, and the answer is linear

The pair started at 2026-08-21T01:24:28Z ran **4.61 hours of the six** and were
killed with the session, not by a defect. Between them they hold 1,064 samples,
which is more than the six hour run was ever going to need. Both are committed:

- `bench/soak-20260821T012428252Z.csv`, `steady`, 16 MiB payload, two leechers,
  531 samples over 4.605 hours, 1,060 completed leech cycles.
- `bench/soak-20260821T012429347Z.csv` and `.json`, `idle`, the control, 533
  samples over 4.617 hours.

**Six hours was not needed, and the reason is in the data rather than in the
schedule.** The discrimination the entry asks for is settled by comparing the
slope over the whole run against the slope over its last three hours. If those
agree, the series is a line. If the second is smaller, it is settling. They
agree, and they already agreed at three hours.

### The steady run

Fitted against elapsed hours, over the 525 samples that are not the one thread
burst described below:

| model | fit | r squared | rmse (MiB) |
| --- | --- | --- | --- |
| **linear in `t`** | 14.886 + **0.804** MiB/h | **0.733** | **0.652** |
| logarithmic, `log(1+t)` | 14.322 + 2.207 | 0.673 | 0.722 |
| square root, `sqrt(t)` | 13.857 + 2.018 | 0.673 | 0.723 |
| saturating, `1-exp(-t/2h)` | 14.300 + 4.016 | 0.645 | 0.752 |
| saturating, `1-exp(-t/5h)` | 14.626 + 6.105 | 0.712 | 0.678 |
| saturating, `1-exp(-t/8h)` | 14.721 + 8.423 | 0.723 | 0.665 |

Linear wins outright. Every saturating model fits worse, and they improve
monotonically as the time constant grows, which is the signature of a curve
that does not bend inside the window: at eight hours the exponential is a
straight line over four and a half. The last three hours on their own give
**0.744 MiB/h at r squared 0.52**, against 0.804 over the whole run. The slope
does not decay.

So the answer to the open question is **linear**. Not a settling curve, and not
an allocator holding pages and then releasing them.

The half-hourly shape, which is what a single whole-run slope hides:

| window | min | median | max | mean threads | mean handles |
| --- | --- | --- | --- | --- | --- |
| 0.0-0.5 h | 15.00 | 15.62 | 16.49 | 26.9 | 216 |
| 0.5-1.0 h | 14.43 | 14.74 | 16.03 | 26.6 | 216 |
| 1.0-1.5 h | 14.91 | 15.53 | **41.70** | 39.1 | 252 |
| 1.5-2.0 h | 15.29 | 16.34 | 20.63 | 27.9 | 219 |
| 2.0-2.5 h | 15.49 | 17.17 | 23.72 | 28.2 | 221 |
| 2.5-3.0 h | 15.55 | 17.40 | 17.77 | 26.9 | 217 |
| 3.0-3.5 h | 15.74 | 17.59 | 18.73 | 26.8 | 217 |
| 3.5-4.0 h | 16.08 | 17.94 | 18.62 | 26.7 | 216 |
| 4.0-4.5 h | 16.05 | 18.61 | 19.17 | 27.0 | 217 |

All figures MiB. The floor rises from 14.43 to 16.05 and the median from 14.74
to 18.61, over three and a half hours. The rise is in the level, not only in
the peaks.

### The one spike, and why it is not the trend

Resident memory's maximum of 41.70 MiB, the handle maximum of 1,150, and the
thread maximum of 352 are **all the same sample**, number 130, at 1.107 hours.
Resident memory's 99th percentile is 19.39 MiB, so the maximum is 2.15 times
the 99th. Three samples in the whole run are above 100 threads: 1.11, 1.12, and
2.02 hours.

The three series move together. `corr(threads, handles)` is **0.9984** and
`corr(threads, rss)` is 0.767, and a straight fit of resident memory against
thread count gives **14.645 MiB + 79.5 KiB per thread**, which is the size of a
thread's committed stack. So a handle spike is a thread spike, a thread spike
is a memory spike, and all three retire. That is a burst of blocking work, not
growth.

This is why the whole-run slope including the spikes is 0.732 MiB/h at r
squared 0.27 and the slope excluding them is 0.804 at r squared 0.73. Removing
three samples makes the trend clearer, not weaker, which is the opposite of
what removing evidence for a trend would do.

### The idle control

The control is what makes the steady number mean anything.

| series | over 533 samples and 4.617 hours |
| --- | --- |
| `handles` | **189 at every sample.** Minimum 189, maximum 189. |
| `threads` | 21 from hour two onward, no variation |
| `tcp_total` | 1 at every sample, which is the listener |
| `tcp_close_wait` | **0 at every sample** |
| `rss_bytes` | 13.75 MiB falling to 12.02, then flat within 0.03 MiB for the last 2.5 hours |
| `peak_rss_bytes` | last rose at hour 1, then flat |

A seeder with no tracker and nothing connecting does not move. So the sampler
is not the source, the session's own timers are not the source, and every
number in the `steady` run is the load.

### What this closes and what it does not

- **The descriptors half of this entry is disproved.** `idle` holds exactly 189
  handles across 533 samples. `steady` is -2.18 an hour at an r squared of
  0.003, which is noise. `CLOSE_WAIT` is **zero at all 1,064 samples across
  both runs**, so [T-020](peers.md) needs the churn shape and does not appear
  under a deployment-shaped load. That was already the reading at 2.76 hours
  and 4.6 hours does not change it.
- **The memory half reproduces and is now quantified.** 0.804 MiB an hour under
  `steady`, linear, r squared 0.73 over 525 samples. That is 19.3 MiB a day and
  579 MiB over thirty if it holds, which is the shape this entry's report
  describes.
- **What is not answered is what the byte is charged to.** Leech completions
  run at a constant 228.5 an hour at an r squared of 0.9999 for the whole run,
  so elapsed time and completed work are collinear in this data and cannot be
  separated by it. 0.804 MiB an hour and 3.6 KiB per completed leech fit the
  same points exactly as well.

**The next measurement is not a longer run.** It is two shorter ones at
different leech rates, because that is the only thing that separates per-hour
from per-download. **Superseded on 2026-08-22 by a third way that needs
neither**, and the pair below was never run; see
[the 2026-08-22 section](#session-of-2026-08-22-the-slope-is-peer-rows). Moving
the leech rate moves the peer count and the transferred bytes together, so it
would have separated per-hour from per-download and left per-download
ambiguous:

```powershell
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 120 -Workload steady -PayloadMiB 16 -Leechers 1 -Root .tmp/soak-rate1
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 120 -Workload steady -PayloadMiB 16 -Leechers 4 -Root .tmp/soak-rate4
```

Four times the completion rate against the same wall clock. If the MiB per hour
quadruples, it is per download and the leech path is where to look. If it does
not move, it is per hour and the announce and timer paths are. Two hours is
enough for both, because the discrimination above needed three.

### The ceilings, set

The slopes above are now the reference, so `scripts/soak.ps1` can judge rather
than record. These are the values a regression run should carry:

```powershell
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 120 -Workload steady -PayloadMiB 16 `
  -RssCeilingMiBPerHour 2 -HandleCeilingPerHour 32 -CloseWaitCeilingPerHour 1
```

- `-RssCeilingMiBPerHour 2` is 2.5 times the measured 0.804. Above it something
  new is happening; at it, this entry's own finding is not what fails the run,
  which is the [check-close-wait.ps1](../scripts/check-close-wait.ps1) rule.
- `-HandleCeilingPerHour 32` against a measured slope of zero in `idle` and
  -2.18 in `steady`. It has to clear the thread bursts, which reach 1,150
  handles for two samples and come straight back.
- `-CloseWaitCeilingPerHour 1` against zero at 1,064 samples. Anything at all
  here is [T-020](peers.md) arriving under a load that has never shown it.

Reproduce the analysis from the committed CSVs:

```powershell
pwsh -NoProfile -Command "Import-Csv bench/soak-20260821T012428252Z.csv | Where-Object { $_.iso -match '^\d{4}-' } | Measure-Object -Property rss_bytes -Minimum -Maximum -Average"
```

## Session of 2026-08-22: the slope is peer rows

The open question was attribution: 0.804 MiB an hour and 3.6 KiB per completed
leech fit the same points equally well, because completions ran at a constant
228.5 an hour for the whole soak. The entry's plan was two runs at different
leech rates. **A third measurement settles it and needs neither**, because it
moves the peer count with the wall clock held almost still.

**The candidate came out of [T-020](peers.md).** `librqbit` records a peer for
every completed handshake and never reclaims the row: 24 handshakes from
loopback left 24 rows, all in `not needed`, with `live` and `dead` both zero.
A leech cycle is a completed handshake, so the soak was accumulating one row
per completion.

`scripts/check-peer-rows.ps1` drives `loopback-churn` in steps against one
seeder and reads RSS and the row count out of the seeder's own `progress`
events. No payload moves, no tracker announces, and the handshake is for the
info hash the seeder holds, so a peer row is the only thing each connection
leaves behind.

```powershell
pwsh -NoProfile -File scripts/check-peer-rows.ps1
```

`bench/peer-rows-20260822T051423181Z.json`, 2,000 connections in steps of 200,
about three and a half minutes end to end:

| connections | peer rows | rss | handles |
| --- | --- | --- | --- |
| 0 | 0 | 11.91 MiB | 188 |
| 200 | 200 | 13.97 MiB | 212 |
| 600 | 600 | 15.07 MiB | 212 |
| 1000 | 1000 | 15.74 MiB | 216 |
| 1400 | 1400 | 17.03 MiB | 216 |
| 2000 | 2000 | 18.11 MiB | 216 |
| after 60 s of nothing | 2000 | 18.65 MiB | 216 |

**One row per connection, exactly, and nothing gives it back.** `peers_seen`
tracks the row count one for one at every step, and a minute of silence at the
end returns none of the memory, so this is retained rather than allocator
churn.

**2,890.8 bytes a peer row**, least squares over the eleven points, r squared
0.944, intercept 13.03 MiB. A pilot run of the same script an hour earlier gave
2,906.7, so the number is stable to half a percent.

### What that accounts for

The soak completed 228.5 leech cycles an hour. At 2,891 bytes a row that is
**0.63 MiB an hour against the 0.804 measured**, so peer rows are 78 percent of
the slope. Read off sub-ranges rather than the whole fit and the row cost is
2,327 bytes from 400 to 2,000, 2,478 from 1,000 to 2,000, and 3,250 across the
whole range, against the 3,689 bytes a completion the soak implies: 63 to 88
percent, whichever way it is cut. The first two hundred rows cost more than the
rest, which is the allocator finding its size rather than a bigger row.

Two things stop this being a closed identity, and both are worth saying:

- A soak leecher's row is not this row. It transferred 16 MiB, so it carries
  counters and a client string this one never sets, and it is the larger of the
  two. That pushes the accounted fraction up rather than down.
- One leech cycle is one handshake only if the leecher never reconnects. The
  soak did not record that, so 228.5 rows an hour is a floor.

So: **the slope is peer rows, to within the precision either measurement has.**
Not a timer, not the announce path, and not the sampler.

### What is carried here: `--max-rss <SIZE>`

Off by default, on `seed` and `download`, and the same shape as
[T-020](peers.md)'s `--max-handles` for the same reason: nothing in this tree
can free a peer row, so what it can do is bound the growth and make it loud.
Sampled once per `--report-interval`, from the same reading the handle ceiling
uses so the two cannot report different instants. Over it, the run stops with
`"stopped": "rss_ceiling"` and exit 16.

```
$ bit-cli seed t.torrent --dir . --port 0 --stop-after 15s --max-rss 1MiB --json
exit=16
  "stopped": "rss_ceiling",
```

Handles are checked before memory when both are set, because a process out of
descriptors has already stopped working and one over a memory line is still
serving. The acceptance is the last two cases of `check-peer-rows.ps1`: a
ceiling any process is over stops on the first sample, and a ceiling nothing is
near reaches the run's own deadline instead, which is what proves the first
stopped for the ceiling.

Status stays **partial**. The growth is attributed and bounded, and it is not
fixed: closing it means `librqbit` reclaiming a peer row that will not be used
again, which is upstream. The corpus has the shape of the answer in
`aria2_rust/aria2-core/src/engine/bt_peer_storage/constants.rs:4`, where
`MAX_PEER_LIST_SIZE` is 512 and `MAX_DROPPED_PEERS` is 50: aria2 bounds both
lists and evicts, rather than keeping every peer it has ever met.

### One harness defect this run found: T-157

The `steady` run's JSON is **4,833 NUL bytes**. Its CSV survived with all 531
samples because a CSV is appended and a summary is rewritten. The whole point
of rewriting the summary after every sample is that a killed run still leaves
its slopes, and the kill destroyed exactly that. Written up as
[T-157](#t-157-a-killed-soak-destroys-the-summary-it-was-rewriting) below, and
fixed.

### T-157 A killed soak destroys the summary it was rewriting

Source:      `bench/soak-20260821T012428252Z.json`, 2026-08-21
Category:    memory
Priority:    P2
Effort:      S
Status:      **done**

Problem:     `scripts/soak.ps1` rewrote `bench/soak-<timestamp>.json` with
             `Set-Content` straight onto the path. That truncates first and
             fills after, so a process killed between the two leaves a file of
             NUL bytes rather than the previous summary.
Relevance:   The rewrite exists so a killed run leaves its slopes. Doing it
             non-atomically means a killed run leaves less than nothing: a file
             that parses as an object with every field empty.
Approach:    Write to `<path>.tmp` and rename over the target. A rename within
             one directory is atomic on both NTFS and POSIX.
Acceptance:  A short run writes the summary, leaves no `.tmp` behind, and the
             summary parses.

The `steady` run of 2026-08-21T01:24:28Z is the worked example. 4.605 hours and
531 samples in the CSV, 4,833 NUL bytes in the JSON, and the slopes in this
entry had to be recomputed from the CSV. `Set-Content` now writes
`$jsonPath.tmp` and `Move-Item -Force` renames it over `$jsonPath`.

Acceptance, run:

```powershell
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 1 -Workload idle -Root .tmp/soak-atomic-check
```

`complete=True`, 2 samples, and zero `bench/*.tmp` left behind.

### T-212 Resolving a magnet can allocate 4 GiB across 128 peers

Source:      reading nzbd's `0016-limit-peer-metadata-before-allocation` and
             `0014-bound-discovery-pressure` against the vendored tree,
             2026-08-23
Category:    memory
Priority:    P2
Effort:      M
Status:      open

Problem:     Two bounds that multiply.
             `vendor/rqbit/crates/librqbit/src/dht_utils.rs:42` runs **128**
             metadata reads at once, and
             `vendor/rqbit/crates/librqbit/src/peer_info_reader/mod.rs:87`
             lets each one allocate whatever the peer says the metadata is,
             up to **32 MiB**, on the peer's word. 128 hostile peers
             answering one magnet is 4 GiB of allocation, held until the read
             timeout drops them.
Relevance:   Adding a magnet is the one operation that takes a number from a
             stranger and allocates it. `--max-rss` is the backstop and a
             backstop is not a bound: it stops the process rather than the
             peer. The per-peer 32 MiB is a sensible ceiling on its own, and
             it is the multiplication that is not bounded anywhere.
Approach:    Not the option nzbd's `0016` adds. That makes the per-peer cap
             configurable, which is a knob with no caller here and does not
             touch the product. Bound the **aggregate** instead: a byte budget
             shared across the resolution, acquired before the buffer is
             built, so 128 peers cannot each take 32 MiB. The check also
             belongs before the two writer sends in `on_extended_handshake`,
             which currently unchoke and declare interest to a peer that is
             about to be refused.
             `seen`, at `dht_utils.rs:39`, is the smaller half: one
             `SocketAddr` per address the DHT returns, retained for the whole
             resolution and handed on as the initial peer list. It is bounded
             by `--init-timeout` rather than by design.
Acceptance:  A magnet resolution against a fixture swarm where every peer
             advertises the maximum metadata size holds peak RSS under a named
             ceiling, and the same run with one honest peer still resolves. A
             `bench` run recorded here with both numbers.

**What is measured and what is arithmetic.** The two numbers above are read off
those two lines and multiplied. What has **not** been measured is a run that
reaches 4 GiB: it needs a fixture swarm of peers that answer an extended
handshake with a large `metadata_size` and then stall, and no such fixture
exists here. The entry is filed with the arithmetic and the citations rather
than with a measurement, and the acceptance is what would replace one with the
other.

**Why the per-peer cap is not the thing to lower.** A torrent of 1,048,576
pieces, which [T-195](peers.md) made resolvable, carries 20 MiB of piece hashes
in its info dictionary. 32 MiB is therefore a real ceiling with about 50 per
cent of headroom, not an absurd one, and lowering it would refuse torrents this
repository has gone out of its way to support.

### T-041 Per-source window cache is bounded but not measured

Source:      `bit-cli` design
Category:    memory
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T18:00Z

Problem:     Each HTTP source caches whole windows in memory. The bound is
             `cache_windows * chunk_size`, and `cmd::download::cache_windows`
             picks the count so the product stays near 16 MiB per source. With
             twelve mirrors that is 192 MiB, which is a real number nobody has
             measured.
Relevance:   `--web-seed-chunk-size 64MiB` with ten sources is 640 MiB of cache
             by construction, and nothing warns about it.
Approach:    Report the computed cache budget in `webseed list --json` so it is
             visible before the run, and cap the total across sources rather
             than per source.
Acceptance:  `bit-cli webseed list <TORRENT> --json` carries
             `"cache_budget_bytes"` per source and a total, and a run with ten
             sources at a 64 MiB chunk size warns when the total exceeds
             256 MiB.

**Done 2026-08-23T18:00Z. The premise held and the number in the Relevance
line is half the real one.**

`cache_windows` divides a 16 MiB per-source budget by the largest chunk size
any source asked for and **clamps the result to `[2, 16]`**. The Relevance line
reads "`--web-seed-chunk-size 64MiB` with ten sources is 640 MiB of cache by
construction", which is ten sources times one window of 64 MiB. The floor is
two, so it is **1.25 GiB**, and the acceptance asserts that figure rather than
only that something warned.

**The floor is right and it is the whole reason the budget is exceeded.** A
cache of one window cannot hold the window a read is being served from and the
next one at the same time, so a source with one window re-fetches every window
it just evicted. So a large chunk size costs eight times the budget by design,
and what was missing is anything that said so before the run.

### A test named for the budget asserted the case that breaks it

`the_window_cache_stays_inside_its_memory_budget` had three cases and the
middle one was `cache_windows == 2` for a 64 MiB chunk, commented "never below
two windows". That is 128 MiB against a per-source budget of 16, so the test's
name was a claim about the run it was pinning the opposite of. The floor stays;
the name went, and the **cost** the floor produces is now asserted beside the
count in `the_window_count_falls_as_the_chunk_size_rises_until_the_floor`.

### What ships

`cache_budget` returns the window count, the bytes per source, and the total,
and `CACHE_BUDGET_PER_SOURCE` and `CACHE_TOTAL_WARN` are named constants rather
than two literals in one expression.

`bit-cli webseed list --json` carries `sources[].cache_budget` per source and
`cache_budget_total` with `cache_windows` for the run, and the text form prints
both. It is computed by the same function the run calls, from the same specs,
so the listing is what a download of that torrent with those flags will hold
rather than a second estimate of it.

**The warning is on `download` as well as on `webseed list`**, which the
Acceptance did not ask for and the entry's own Relevance did: the memory is
held by the run, and `webseed list` is only where a caller looks first. On
`download` it is raised once per run rather than once per worker, and named by
source where the run has more than one torrent.

### Measured

| sources | chunk | windows | total | warns |
| --- | --- | --- | --- | --- |
| 1 | 4 MiB | 4 | 16 MiB | no |
| 16 | 4 MiB | 4 | 256 MiB | no, and it is exactly the ceiling |
| 1 | 64 MiB | 2 | 128 MiB | no |
| 10 | 64 MiB | 2 | **1.25 GiB** | yes |
| 1 | 64 KiB | 16 | 1 MiB | no |

The ceiling is 256 MiB because sixteen mirrors at the default chunk size is
the largest total the ordinary case reaches. Anything above it comes from a
chunk size the caller chose, and the message names that flag.

Four cases, two on the arithmetic and two driving the command, and the last of
them asserts the warning is on **stderr and not stdout**, which is the rule the
whole surface keeps.

```bash
cargo test -p bit-cli --lib the_window_count_falls the_total_budget_is the_listing_carries a_chunk_size_that_costs
```

### What this entry did not do, and it is filed rather than left

The Approach's second half, "cap the total across sources rather than per
source", is **not** taken here, and the reason is the rule about evidence
rather than a preference. Capping the total means dividing 16 MiB across the
sources, which for two mirrors halves each one's cache from four windows to
two. That is a throughput change and this entry has no measurement of it: the
window cache is what absorbs a re-read, so fewer windows may cost fetches. A
cap chosen without measuring what it costs is the shape [RULES.md](RULES.md)
section 5 refuses.

It is [T-227](#t-227-the-window-cache-budget-is-per-source-so-the-total-is-whatever-the-source-count-makes-it),
filed, with the measurement it needs named.

### T-042 Peak RSS is not captured in any report

Source:      the operator's brief
Category:    memory
Priority:    P1
Effort:      S
Status:      **done**

Problem:     A3.11 requires peak RSS, total CPU time, and open handle count in
             every `bench` report. None is collected.
Relevance:   "A benchmark without its environment recorded is not a result."
             Two throughput numbers with different memory ceilings are not
             comparable.
Approach:    On Windows, `GetProcessMemoryInfo` gives `PeakWorkingSetSize` and
             `GetProcessHandleCount` gives handles; on Linux, read
             `VmHWM` from `/proc/self/status` and count `/proc/self/fd`. Both
             are a few lines and need no new dependency.
Acceptance:  Every `bench` report carries `peak_rss_bytes`, `cpu_ms`, and
             `open_handles`, and `bit-cli bench webseed --format json` shows
             all three non-zero.

`bit_cli_core::sysinfo::Process::sample` reads all three, with no new
dependency: raw `extern "system"` declarations against `kernel32` on Windows
and `/proc` reads on Linux. It also splits CPU time into user and system,
because on a loopback benchmark the split is the result: the run below spent
29.9 s of CPU over 10 s of wall time and most of it in the kernel, which says
the ceiling is the socket rather than the client.

Every sample of the time series carries the three figures as well as the
summary, so a leak shows up as a slope rather than as one number at the end.
`Process::max` folds samples so a spike halfway through a run is not lost when
memory is released before the end.

Acceptance, 2026-08-19T23:13:33.253Z, release build:

```
$ bit-cli bench webseed .tmp/bench/payload.torrent --web-seed $URL \
    --format json --duration 10s --warmup 2s --concurrency 8 --request-size 1MiB
```

```
"process": {
  "peak_rss_bytes": 42074112,
  "rss_bytes": 33861632,
  "cpu_ms": 29859,
  "cpu_user_ms": 8609,
  "cpu_system_ms": 15234,
  "open_handles": 219
}
```

All three are non-zero. The unit tests in `sysinfo::tests` assert that a sample
reads every field, that CPU time only goes up, and that a delta never goes
negative:

```
$ cargo test -p bit-cli-core --lib sysinfo
test result: ok. 14 passed; 0 failed
```

The Linux path is written and compiles under `#[cfg(unix)]` but has not been
run here: this machine is Windows. See the same note under
[T-091](bench.md).

## Session of 2026-08-22, second: the rows are bounded

The slope was attributed and could not be fixed from here, which is what kept
this partial. The trees are vendored now, so it is fixed there.

`PeerStates::states` only ever grew: `drop_peer` was called on two paths, a bug
branch and backoff exhaustion, and a peer that hands over cleanly ends in
`NotNeeded` and stays. There is a bound now, `MAX_PEER_RECORDS` = 1,024 per
torrent, reclaiming `NotNeeded` and `Dead` rows before an insert and never a
`Live`, `Connecting` or `Queued` one. `patches/UPSTREAM.md` under "librqbit:
nothing ever reclaimed a peer row" carries the diff and the reasoning.

```powershell
pwsh -NoProfile -File scripts/check-peer-rows.ps1
```

| connections | rows before | rows after |
| --- | --- | --- |
| 1,000 | 1,000 | 1,000 |
| 1,200 | 1,200 | **1,024** |
| 2,000 | 2,000 | **1,024** |

Exactly 1,024 and flat. `bench/peer-rows-20260822T152743150Z.json`.

**One row per handshake below the bound is still asserted**, and separately,
because a bound that reclaimed a live peer would also make the count flat. The
fit that measures the row cost now runs over the steps below the bound only:
above it the row count is constant, so those points measure the intercept again
and flatten the slope toward nothing. 4,280.9 bytes a row over the six points
below 1,024, r squared 0.938, against the 3,689.5 this entry's soak implies.
The spread across fitted ranges was already known and is recorded above: 2,327
to 3,250 depending on where it is read.

**RSS at 2,000 connections did not move, and that is the expected result rather
than a disappointment.** Freeing a row returns it to the allocator, not to the
operating system. 976 reclaimed rows are inside the run-to-run variation at
this scale: the two runs of the bounded binary gave **17.75 MiB and 17.55
MiB** at 2,000 connections, and the unbounded record for the same step is
18.11 MiB, a spread the runs themselves cover. What the bound changes is that demand stops growing, which is what a
process that fails at 3am needs. A ten thousand connection run would show it
and was started and abandoned when the session was redirected.

**What this cost elsewhere, and it was nearly a self-inflicted bug.** A `Dead`
row can be in the dial queue when it is reclaimed, and
`task_manage_outgoing_peer` answered a missing row with
`Error::BugPeerNotFound`. A bound that logs "bug" for its own correct behaviour
is worse than no bound, so that path returns quietly now. Found by reading the
callers before running anything, not by the measurement.

**Status stays partial, and the reason is a measurement rather than a defect.**
This entry's acceptance is `scripts/soak.ps1` over **six hours** with the slope
of each series recorded. The rows are bounded and that is proved; the soak that
would show the memory series flat over six hours has not been run since the
change. That run is the whole of what is left.

---

## The six hour run, 2026-08-22, and the bound holds

This entry's Acceptance is `scripts/soak.ps1` over six hours with the slope of
each series recorded. It has been run, on the `steady` workload, and it closes
this entry.

`bench/soak-20260822T164952755Z.csv`, **687 samples over 6.00 hours**, 1,372
completed leech cycles and none failed.

| series | first | last | max | per hour | r squared |
| --- | --- | --- | --- | --- | --- |
| `rss_bytes` | 13.74 MiB | 18.72 MiB | 19.68 MiB | **+0.815 MiB** | 0.807 |
| `peak_rss_bytes` | 13.86 MiB | 21.16 MiB | 21.16 MiB | +1.064 MiB | 0.713 |
| `handles` | 210 | 213 | 345 | **-0.315** | 0.003 |
| `threads` | 29 | 26 | 80 | -0.145 | 0.005 |
| `tcp_total` | 2 | 1 | 3 | -0.075 | 0.065 |
| `tcp_close_wait` | 0 | 0 | **0** | 0 | n/a |

```bash
pwsh -NoProfile -File scripts/soak.ps1
```

**Read the whole-run RSS slope and you would conclude the bound did nothing.**
0.815 MiB an hour against the 0.804 measured before it. That number is an
average of two regimes and it describes neither.

**The bound engages part way through the run, and the slope breaks there.** It
is 1,024 rows per torrent and this workload completes about 229 leech cycles an
hour, so the map fills at **16,745 s, 4.65 hours in**, which was read live off
the seeder's own `progress` events: 1,024 rows against 1,079 peers seen, and
the row count never moved again. Fitting either side of that instant:

| window | samples | slope | r squared |
| --- | --- | --- | --- |
| **before**, 0 to 4.65 h | 531 | **+0.909 MiB/h** | 0.799 |
| **after**, 4.65 to 6.00 h | 156 | **-0.140 MiB/h** | 0.005 |

13.74 MiB to 18.61 MiB in the first window and 18.68 MiB to 18.72 MiB in the
second. A straight line for four and a half hours, then flat.

That is what the bound was built to do, measured end to end: **memory grows
while peer records accumulate and stops growing when they stop accumulating.**
The attribution this entry rested on, that most of the byte is the peer row, is
confirmed rather than merely inferred from a per-row size.

**An interim read at 5.06 hours said the opposite and was wrong.** It had 55
samples after the elbow and reported +1.45 MiB/h at r squared 0.107, which is
noise fitted to a line. The lesson is the one this entry has been about
throughout: a slope needs a window long enough to have a shape, and the window
has to start where the thing being measured starts.

**Descriptors: disproved for the third time, now over six hours.** Handles
trend at -0.315 an hour at r squared 0.003, which is no trend, and the maximum
of 345 against a mean of 216 is a burst that came back. This entry's report
named descriptors as well as memory and nothing here has ever reproduced that
half.

**`CLOSE_WAIT` was zero at all 687 samples**, minimum zero and maximum zero.
That is [T-020](peers.md)'s fix holding for six hours under load rather than
for the length of an acceptance script.

**What is not closed by this.** The `all` workload, which adds churn, was not
run: churn strands sockets at about 30,000 handles an hour, which is T-020's
shape and swamps every other series. And the bound is 1,024 rows for one
torrent; a session holding many torrents has that many times the torrent count,
which is bounded but not small. Nothing measures the multi-torrent case yet.

### T-224 The six hour soak's RSS slope is one step and a sawtooth, not a leak

Source:      the operator's six hour soak, 2026-08-23T09:01:32Z
Category:    memory
Priority:    P2
Effort:      M
Status:      **done**, 2026-08-24

Problem:     The first soak to reach its full six hour window reports
             **3.708 MiB/h at r squared 0.717** for `rss_bytes`, against a
             ceiling of 4. The verdict is "every named ceiling held", and it
             did, with a 7 percent margin.

             **That number is a line fitted through a step.** At sample 132,
             `t+1.161h`, resident memory goes from 15.8 MiB to 27.5 MiB in one
             eight second interval and never returns below 27.5 for the
             remaining 4.8 hours. Threads and handles do not step with it: 35
             to 28 and 209 to 188 across the same boundary, which is the
             ordinary oscillation both show all run. So roughly **11.7 MiB is
             allocated once and retained**, at leech cycle 264 of 1,360.

             Fitted either side of that step the slope is a different thing:

             ```
             whole run    n=681   3.708 MiB/h   r2 0.717
             before step  n=132   1.020 MiB/h   r2 0.484
             after step   n=549   1.690 MiB/h   r2 0.621
             from t+2h    n=457   1.371 MiB/h   r2 0.418
             ```

             **And what is left after the step is a sawtooth rather than
             growth.** From `t+2h` the series has mean 33.8 MiB, standard
             deviation 2.4, and range 27.6 to 39.2. Fifty-two samples fall by
             more than 3 MiB and forty-nine rise by more than 3. A series that
             gives back what it takes, 52 times, is an allocator or a cache
             with a high-water mark, not a leak.
Relevance:   Three things, and the third is why this is filed rather than
             noted.

             **The reported number is wrong about the mechanism**, and it is
             the same mistake [INDEX.md](INDEX.md) already records for the
             earlier `steady` soak: a slope fitted across a discontinuity
             describes neither side of it. That run was noise read as a trend;
             this one is a step read as a trend. `soak.ps1` reports one linear
             fit per series and has no way to say "there is a step here".

             **The margin is not what it looks like.** 3.708 against a ceiling
             of 4 reads as "close, watch it". Take the step out and the run is
             at 1.0 to 1.7 MiB/h, which is comfortably inside. Leave the step
             in and a run one hour longer would have reported a **lower**
             slope, because the step's contribution to the fit shrinks as the
             window grows. A ceiling a run passes or fails depending on how
             long it ran is not a ceiling.

             **The step itself is the finding.** 11.7 MiB retained, once, is
             larger than anything [T-040](#t-040-memory-and-descriptors-grow-without-bound-over-a-long-run)
             measured and it is not explained by handles, threads or sockets,
             all of which are flat across it.
Approach:    Two pieces, and the first is cheap.

             **Make `soak.ps1` report the step.** A single linear fit is the
             wrong summary for a series with a discontinuity in it. The
             cheapest honest addition is a largest-single-interval-change
             column beside each slope, and a note when that change is more
             than some fraction of the whole run's rise, which here would be
             11.6 of 22.7 MiB. The fit stays; what changes is that a reader is
             told not to trust it alone. That alone would have made this entry
             unnecessary to write by hand.

             **Then find the step.** It is at a wall clock rather than at a
             round number of cycles, so start by asking whether it reproduces:
             a two hour run at the same leech rate should cross it. If it
             does, the candidates in order of size are the piece cache, the
             window cache [T-041](#t-041-per-source-window-cache-is-bounded-but-not-measured)
             says is bounded but not measured, and whatever the vendored
             session allocates lazily on a threshold it crosses at around 260
             completed torrents.
Acceptance:  `soak.ps1` reports the largest single-interval change per series
             and says when a slope is fitted across one. And either the step
             reproduces at a known cause, named with a file, or two runs at
             different leech rates show it is not tied to completed work, in
             which case the entry says so and closes on the measurement.

**What did hold, and it is worth separating from the above.** Every ceiling
passed on its own terms, and two of the three passed with no argument at all.

| series | per hour | ceiling | verdict |
| --- | --- | --- | --- |
| `rss_bytes` | 3.708 MiB | 4 MiB | held, and this entry is about how |
| `handles` | 0.44 | 20 | held, r squared 0.00, flat |
| `tcp_close_wait` | 0.00 | 1 | **zero at every one of 681 samples** |

`tcp_close_wait` is [T-020](peers.md) staying fixed over six hours and 1,360
completed leech cycles, which is the longest window it has been held over.
Threads are flat at r squared 0.00. **1,360 leech cycles completed and none
failed.**

The run is `bench/soak-20260823T090132499Z.csv` and its summary is the `.json`
beside it, both committed.

## Half of the Acceptance is met, 2026-08-23T17:10Z, and the step did not reproduce

The entry stays open, because the Acceptance has two halves joined by "and"
and only the first is finished. What the second half now has is a measurement
rather than a plan.

### The first half is built: a report says when its slope spans a step

`Get-Slope` reports `largest_rise`, `largest_rise_hours`, `largest_fall`,
`largest_fall_hours` and `step_share` beside the fit, and the table a run
prints carries the first three. Reading the committed run's `rss_bytes` line
is now the whole of what this entry had to be computed by hand to say:

```
series      first   last    max per hour   r2 step up at h step down unit
rss_bytes   13.55  35.18  39.20     3.71 0.72   11.61 1.16     -7.23 MiB
```

**`step_share` is reported and is not the number to read first.** It is
`largest_rise` over the run's whole rise, 0.537 here, and it reads just as
high on a short run that barely moved, where it means nothing. The magnitude
is what separates a step from a sawtooth. The JSON notes say so where a reader
of the report will meet it.

**`soak.ps1 -ReadCsv <csv>` re-reads a finished run** through that same
`Get-Slope`, printing the table and writing nothing, and `-ReadJson <path>`
writes the fits beside it. A soak is six hours and its numbers are read many
times afterwards; until now there was no way to read one except by fitting a
line by hand, which is how this entry was written.

That block sits above the `trap`, above every `Start-Child`, and above the
`Get-NetTCPConnection` platform guard, and each of those matters: `exit` at
script scope is a terminating error the trap rethrows, a read-only mode below
the seeder is a soak with a report on the end of it, and the check below runs
on Linux in CI. It was written below all three to begin with.

```bash
pwsh -NoProfile -File scripts/soak.ps1 -ReadCsv bench/soak-20260823T090132499Z.csv
```

`scripts/check-soak-fit.ps1` is the acceptance, and it is a CI job, `Soak fit`,
because both fixtures are a CSV rather than a clock. Three cases: the mode
runs and prints the columns; the committed run's step is over 8 MiB, lands
between `t+1.0` and `t+1.3`, and is more than two hours' worth of the fitted
slope; and a generated series rising 128 KiB every sample, at four times the
slope and with no step, reports a largest rise of exactly one increment. That
third case is what stops the column being the slope reported twice.

**Every number it asserts on comes from `soak.ps1 -ReadJson`.** The check was
written computing its own fit first, which would have passed against a
`soak.ps1` that reported nothing.

**Run against the defect**: with the largest-rise walk disabled, the step case
fails on all three of its assertions and the other two still pass.

```bash
pwsh -NoProfile -File scripts/check-soak-fit.ps1
```

### The second half: the step did not happen again

The operator's reproduction run, `bench/soak-20260823T154716064Z`, started
2026-08-23T15:47:16Z from a release build of `d3bc6a5`, same workload and same
leech rate. It crossed `t+1.161h` and **nothing stepped**:

| | committed, 09:01:32Z | reproduction, 15:47:16Z |
| --- | --- | --- |
| samples read | 681 over 5.992 h | 161 over 1.39 h |
| `rss_bytes` first, last | 13.55, 35.18 MiB | 13.95, 15.57 MiB |
| slope | 3.708 MiB/h, r2 0.717 | 1.074 MiB/h, r2 0.609 |
| largest single rise | **11.61 MiB at t+1.161 h** | **1.48 MiB at t+1.187 h** |
| largest single fall | -7.23 MiB | -1.30 MiB |
| rss at t+1.3 h | 27.09 MiB | 16.22 MiB |

**So the step is not a property of the elapsed time or of the cycle count.**
The entry's Approach guessed "whatever the vendored session allocates lazily
on a threshold it crosses at around 260 completed torrents", and a second run
at the same rate reaching the same point without allocating it rules that out
as stated.

**And the two runs agree on what the tree actually does.** The reproduction's
whole-run slope, 1.074 MiB/h, is the committed run's **pre-step** slope, 1.020.
The number this entry says is the honest one is the number a second run
produces on its own.

### What is left, and it is smaller than the entry

The Acceptance's second half asks for the cause named with a file, **or** two
runs at different leech rates showing the step is not tied to completed work.
One run at the same rate is not the second of those. What would close it is
one more run with `-Leechers` changed, which is the operator's to start for
the reason every soak is.

The reproduction was still running when this was written, at 161 samples of
its 360 minutes, so it may yet step later at a point neither run has reached.
That would be a different finding from the one filed, and the column now
reports it without anybody fitting a line by hand.

## The reproduction finished, 2026-08-24, and most of it does not count

It ran its full six hours. **No step**, and the whole-run figures are the
flattest this repository has recorded:

```
series     first  last   max per hour   r2 step up at h step down unit
rss_bytes  13.95 17.71 17.72     0.46 0.54    1.99 3.02     -2.07 MiB
```

```bash
pwsh -NoProfile -File scripts/soak.ps1 -ReadCsv bench/soak-20260823T154716064Z.csv
```

**And 78 percent of it is a measurement of an idle process.** Its workload
stopped at `t+4653s`, 1.292 hours in: 298 leech cycles completed, 1,080 failed,
none completed after that point, and the seeder spent the remaining 4.7 hours
alive and using 47 milliseconds of CPU. That is
[T-232](#t-232-a-six-hour-soak-reported-a-pass-on-a-workload-that-stopped-after-78-minutes),
filed on this reading, and the same command prints it now.

**What survives, and it is the half this entry needed.** The step being
answered is at `t+1.161h`, and the workload was still running then: cycles were
completing right up to `t+1.292h`. So the reproduction **did** cross the step
point under load and **did not step**, with a largest single rise of 1.99 MiB
against 11.61, and the crossing is not in the part that is idle.

**What does not survive is everything after `t+1.3h`.** The flat 4.7 hours are
not evidence that a busy seeder is flat. This is why the entry does not close
on "two six hour runs and only one stepped": the second one is a busy run of
1.29 hours with a long flat tail, and its 0.461 MiB/h is fitted mostly through
the tail.

| | committed, 09:01:32Z | reproduction, 15:47:16Z |
| --- | --- | --- |
| samples | 681 over 5.992 h | 690 over 5.993 h |
| leech cycles | 1,360 completed, **0 failed** | 298 completed, **1,080 failed** |
| workload ran for | the whole run | **1.292 h of 5.993** |
| largest single rise | **11.61 MiB at t+1.161 h** | 1.99 MiB at t+3.02 h |
| rss at t+1.3 h | 27.09 MiB | 15.57 MiB |

`bench/soak-20260823T154716064Z.csv` and its `.json` are committed as the
evidence for both entries.

### So: is another soak needed? Yes, and it is one soak rather than two

The operator asked whether this is definitive. It is not, for two reasons that
the same run answers:

- **This entry** still has one run at one leech rate. The reproduction was
  meant to be the second data point and it is only one for the first 1.29
  hours of its window.
- **[T-232](#t-232-a-six-hour-soak-reported-a-pass-on-a-workload-that-stopped-after-78-minutes)**
  needs a run that can say whether the seeder stopped answering or the
  leechers stopped calling.

`-Leechers 4` is the different rate this entry wants and `-ListenerCheck 60s`
is what T-232 needs, and neither interferes with the other:

```bash
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 360 -Leechers 4 -ListenerCheck 60s -RssCeilingMiBPerHour 4 -HandleCeilingPerHour 20 -CloseWaitCeilingPerHour 1
```

**And the harness will not report a pass on a dead workload again**, which is
the thing that made this reading expensive: `-LeechFailurePercent` defaults to
5, the failing run is at 78.37, and `soak.ps1 -ReadCsv` says so on any finished
run without anybody counting a column by hand.


#### Closed 2026-08-24 on the measurement: the step does not reproduce, and what is there instead is the sawtooth

The Acceptance has two halves. The first was built on 2026-08-23 and is above.
The second offered two ways to finish: name the step's cause with a file, or
show with two runs at different leech rates that it is not tied to completed
work. **It is the second.** The run is the operator's
`bench/soak-20260824T164609340Z`, six hours at `-Leechers 4 -ListenerCheck 60s`,
and it completed **2,812 leech cycles with none failed** over 704 samples.

| | committed, 2 leechers | this run, 4 leechers |
| --- | --- | --- |
| samples | 681 over 5.992 h | 704 over 5.993 h |
| leech cycles | 1,360 completed, 0 failed | 2,812 completed, 0 failed |
| `rss_bytes` per hour | 3.71 MiB, r2 0.72 | 1.81 MiB, r2 0.65 |
| largest single rise | **11.61 MiB at t+1.16 h** | 7.82 MiB at t+4.92 h |
| largest single fall | -7.23 MiB | -7.13 MiB |

**There is no step in this run.** The committed run's is one move that stays:
15.68, 15.85, then **27.46**, and 27.51 and 27.63 after it. This run never does
that. What it does instead, from `t+1.045 h` to the end, is oscillate: **126**
single interval changes over 3 MiB, and every rise is matched by a fall of
nearly the same size within a sample or two.

```
t+  3761s   16.39 ->  19.79 MiB  (+3.41)  cycles 496
t+  3791s   19.79 ->  16.70 MiB  (-3.09)  cycles 500
...
t+ 20348s   26.05 ->  18.91 MiB  (-7.13)  cycles 2656
t+ 20378s   18.91 ->  25.17 MiB  (+6.26)  cycles 2660
```

The floor of that band holds between 16.5 and 19.3 MiB for the whole run while
its ceiling drifts from 20 to 26, so the amplitude grows from 3.4 MiB to about
7. That is a high-water mark rising slowly, which is what this entry proposed
before there was a second run to check it against: **an allocator or a cache,
not a leak.**

**The step is not tied to completed work, and the cycle counts are what say
so.** The committed run's step lands at sample 132, `t+4178s`, with **264 leech
cycles** completed. This run passed 264 cycles inside its first thirty-five
minutes and nothing happened there: its first change over 3 MiB is at `t+3761s`
and 496 cycles, and it is a tooth rather than a step. A threshold in completed
torrents would have to fire at a similar cycle count in both, and the two are
264 and 496 for the first movement and 264 against 2,332 for the largest.

**What this does not claim.** It does not name the allocation. Nothing here
identifies which cache or which pool holds the high-water mark, and the entry
closes without that because the Acceptance offered this branch instead: the
question it was filed to answer is whether the reported slope described a leak
tied to work, and the answer is measured and no.

**The reported number is still the thing to be careful about**, and that is the
part of this entry which outlives it. A single linear fit across a rise and a
fall of similar size describes neither, and it moves with the window: the same
run reports a different slope depending on how long it ran. The `step up`,
`at h` and `step down` columns exist so a reader is told not to trust the fit
alone, and they are what this closing is written from.

```bash
pwsh -NoProfile -File scripts/soak.ps1 -ReadCsv bench/soak-20260824T164609340Z.csv
```

[T-227](#t-227-the-window-cache-budget-is-per-source-so-the-total-is-whatever-the-source-count-makes-it)
is where a named cache with a budget is still open, and it is the entry to
reach for if the high-water mark is ever worth chasing to its source.

### T-227 The window cache budget is per source, so the total is whatever the source count makes it

Source:      measured while closing T-041, 2026-08-23
Category:    memory
Priority:    P2
Effort:      M
Status:      open

Problem:     `cmd::download::cache_windows` divides `CACHE_BUDGET_PER_SOURCE`,
             16 MiB, by the largest chunk size any source asked for, and every
             source gets that many windows. Nothing divides by the number of
             sources, so the run's total is the per-source budget multiplied by
             the mirror count: sixteen mirrors at the default chunk size hold
             256 MiB, and ten at `--web-seed-chunk-size 64MiB` hold 1.25 GiB.
Relevance:   [T-041](#t-041-per-source-window-cache-is-bounded-but-not-measured)
             closed on making that number visible and warning above 256 MiB,
             which is what its Acceptance asked for. Its Approach also proposed
             capping the total, and that half is here because it is a
             throughput change and T-041 had no measurement of one.

             The warning is honest but it is advice rather than a bound. A
             caller who wants sixteen mirrors and a large chunk size has no way
             to say "hold both, inside 256 MiB", and the tool has no way to
             give it to them.
Approach:    Divide the budget by the source count, with a floor of two windows
             per source, which is where the arithmetic stops helping: two
             windows is what stops a source re-fetching the window it just
             evicted, and below it the cache does more harm than none.

             So the cap is only reachable while
             `sources * 2 * chunk_size <= budget`, and past that the honest
             answer is the warning T-041 already ships plus a flag that lets a
             caller lower the ceiling themselves. Decide the two together: a
             `--web-seed-cache-budget` that defaults to today's behaviour is a
             smaller change than it looks and makes the bound the caller's.

             **Measure before building**, because this is a throughput change
             and the entry is filed without that measurement. Two mirrors at
             the default chunk size go from four windows each to two under a
             shared budget, and the window cache is what absorbs a re-read.
             `bit-cli bench webseed` against `loopback-fileserver` with two and
             with eight sources, at four windows and at two, is the curve. If
             the curve is flat the cap ships; if it is not, the flag ships and
             the default does not move.
Acceptance:  `bench/cache-windows-<timestamp>.json` shows throughput against
             window count at two source counts, and either the total is capped
             with the curve recorded here, or the cap is refused on that curve
             and a flag gives the caller the bound instead.

**Ruled on 2026-08-24: measure, then flag.** Both halves confirmed. The curve
is run before any default moves, and `--web-seed-cache-budget` ships defaulting
to today's behaviour whatever the curve says, so a caller who wants sixteen
mirrors inside a bound can ask for one. Capping the total without the
measurement was put and refused.

### T-231 A soak killed mid-write reads as a final sample of zeros

Source:      found by `scripts/check-tree.ps1` on the day it was written,
             2026-08-24, see [T-230](cli-surface.md)
Category:    memory
Priority:    P1
Effort:      S
Status:      **done** 2026-08-24

Problem:     `bench/soak-20260821T012428252Z.csv` is committed evidence and it
             ended in 176 NUL bytes. NTFS flushes a file's size before its
             bytes, so a soak killed while appending leaves the tail zero
             filled. `Import-Csv` reads that tail as one more record whose
             every field is the empty string, `[double]""` is 0 in PowerShell,
             and `Get-Slope` then fits its line through a final sample of
             zeros.

             What `soak.ps1 -ReadCsv` said about that file, against what the
             531 rows in it actually hold:

             | | reported | true |
             | --- | --- | --- |
             | samples, hours | 532 over **0** | 531 over 4.605 |
             | `rss_bytes` last | **0.00 MiB** | 19.27 MiB |
             | `handles` last | **0.00** | 241 |
             | `peak_rss_bytes` largest fall | **-42.19 MiB** | 0.00 |
             | `rss_bytes` slope | 0.77 MiB/h | 0.73 MiB/h |

             The fourth row is the one that gives it away without knowing
             anything else. `peak_rss_bytes` is a high-water mark. It cannot
             fall, and the report said it fell by its whole value.
Relevance:   Three things, and the second is why this is P1.

             **Nothing said anything was wrong.** The run printed a table and
             exited 0. Every number in it is wrong in the same direction and
             none of them is absurd enough to notice except the one nobody
             reads.

             **`Get-Slope` is what `scripts/check-soak-fit.ps1` asserts on and
             what [T-224](#t-224-the-six-hour-soaks-rss-slope-is-one-step-and-a-sawtooth-not-a-leak)
             is written from.** A reader that invents a terminal zero
             manufactures exactly the shape T-224 exists to detect: a large
             single-interval fall, and a `last` that has nothing to do with
             the run.

             **Somebody had already met it and worked around it in prose.**
             This file's own line in the entry above reads it with
             `Where-Object { $_.iso -match '^\d{4}-' }`. That filter is in the
             record because the rows are not all rows, and the fix went into
             the sentence rather than into the reader.

             It is the same family as [T-229](bench.md) and
             [T-103](bep-coverage.md): the defect is in the instrument, so
             everything the instrument said has to be re-read rather than
             trusted.

             **And this exact failure was already found and fixed once, on the
             other file of the same run.**
             [T-157](#t-157-a-killed-soak-destroys-the-summary-it-was-rewriting)
             is `bench/soak-20260821T012428252Z.json` left as NUL bytes by a
             kill during `Set-Content`, closed by writing to a temporary path
             and renaming. Its source line names the same timestamp this entry
             does. The `.json` is rewritten and could be made atomic; the
             `.csv` is appended and cannot, so nothing there was to fix and
             nothing was looked at. The fix for an append is on the reading
             side, and no reader had one.
Approach:    A row has to look like a row, and the file has to stop carrying
             bytes that are not data.
Acceptance:  A CSV with a zero-filled tail reports the samples it has and the
             last value it actually holds, says the file was truncated, and
             the committed file carries no NUL. Run against the defect.

#### Done 2026-08-24

**`Test-SoakRow` in `scripts/soak.ps1`.** `sample`, `elapsed_s` and
`rss_bytes` are counters the sampler writes as integers and `iso` is an
instant; a record missing any of the four is not a sample, whatever produced
it. Dropped rows are **counted and printed**, not dropped quietly, because a
truncated file is itself worth knowing: those 531 samples are real and the run
they came from ended in a way its own report never mentioned. `-ReadJson`
carries `dropped_rows` beside `samples`.

**The committed file was repaired rather than exempted.** The 176 NUL bytes
are a crash artefact, not a measurement, and removing them changes no sample:

```bash
pwsh -NoProfile -File scripts/soak.ps1 -ReadCsv bench/soak-20260821T012428252Z.csv
```

The working-tree bytes are identical to the old blob with the fill stripped.
The committed blob differs by more than that, and the reason is worth
recording: `.gitattributes` sets `* text=auto`, git classified the file as
binary **because of the NULs**, and so never normalised its line endings. With
the fill gone it is text, and the new blob is LF where the old one was CRLF.
A file that was quietly stored as a binary blob is the same fact as the one
this entry is about.

**A fourth case in `scripts/check-soak-fit.ps1`**, so the CI job holds it.
The fixture is the generated ramp with 176 NUL bytes appended, which makes the
expected numbers exact: the last real sample is the last one written. It
asserts `dropped_rows` is at least one, `last` is the last real value, the
sample count matches the same file without the fill, and the output says the
file was truncated.

**Run against the defect**: with `Test-SoakRow` returning true for everything,
the new case fails on all four assertions and the other three pass.

```bash
pwsh -NoProfile -File scripts/check-soak-fit.ps1
```

### T-232 A six hour soak reported a pass on a workload that stopped after 78 minutes

Source:      the operator's second six hour soak,
             `bench/soak-20260823T154716064Z`, read 2026-08-24
Category:    memory
Priority:    P1
Effort:      M
Status:      **done**, 2026-08-25

Problem:     The run's own summary line says
             `leech cycles: 298 completed, 1080 failed`, and its report says
             `"verdict": "every named ceiling held over 6 hours"` with
             `"failures": []`.

             Both are true. At sample 149, `t+4653s`, which is **1.293
             hours**, three things happen in the same interval and none of
             them ever comes back:

             | | s148, t+4616 | s149, t+4653 | rest of the run |
             | --- | --- | --- | --- |
             | `leech_completed` | 296 | 298 | **298** for 540 samples |
             | `cpu_ms` | 38,234 | 38,250 | **38,297** at t+21,303 |
             | `tcp_established` | 1 | 0 | 0 at every sample |
             | `handles` | 180 | 168 | **168**, not one sample off it |
             | `threads` | 26 | 22 | **22**, likewise |

             From there the seeder spends **4.7 hours** alive, listening,
             emitting its progress event every 30 seconds, and using
             **47 milliseconds of CPU**. Every leech cycle after it fails, two
             per sample, 1,080 of them, each inside one 30 second tick rather
             than waiting out its `--stop-after 120s`.

             `tcp_listen` is 1 the whole time. The socket is bound and nothing
             is accepted through it.
Relevance:   **The flat lines that follow are the report's evidence, and they
             are flat because nothing was happening.** `rss_bytes` holds
             between 15.4 and 17.7 MiB for the last 4.7 hours, `handles` does
             not move by one, and the run's 0.461 MiB/h is fitted mostly
             through that. A soak measures a busy seeder; this one measured a
             busy seeder for 1.29 hours and an idle process for 4.7.

             **It is the third instrument defect in two sessions**, after
             [T-229](bench.md) and [T-231](#t-231-a-soak-killed-mid-write-reads-as-a-final-sample-of-zeros),
             and it is the one that costs the most: the run was started to
             answer [T-224](#t-224-the-six-hour-soaks-rss-slope-is-one-step-and-a-sawtooth-not-a-leak),
             and 78 percent of it cannot be used for that.

             **What it is not.** It is not a cycle count. A `steady` run at
             four leechers and a five second sample interval, on the same
             binary, completed **552 cycles in 14 minutes with none failed**,
             nearly twice the 298 the failing run stopped at, at eight times
             the cycle rate. It is not the committed run either, which
             completed 1,360 and failed none over the full six hours at the
             failing run's own rate.

             ```bash
             pwsh -NoProfile -File scripts/soak.ps1 -Minutes 14 -SampleSeconds 5 -Leechers 4 -Workload steady
             ```

             `bench/soak-20260824T023232248Z` is that run. It is committed as
             the negative result, because "298 cycles is not the trigger" is
             the only thing about this that is settled.

             **What the shape suggests, and it is a suggestion.** A process
             holding a bound listening socket and accepting nothing is the
             shape `--listener-check` exists for: `crates/bit-cli/src/cli.rs:1581`
             calls it out in those words, "the process is alive, the port is
             open, and the ratio still gets reported", and
             [T-020](peers.md) is the accept loop defect it was written
             against. The soak does not pass that flag, so the seeder was
             never asked whether it still answered.

             It is equally consistent with the leechers never opening a
             connection. Nothing in the run distinguishes the two, because
             the leech process's own output is overwritten by the next cycle
             and `$Root` is deleted when the run ends.
Approach:    Two halves. Make the instrument able to answer the question, then
             ask it.

             **The instrument.** A verdict that reads as a pass on a run whose
             workload died is worse than no verdict. And a failing cycle has
             to leave behind why.

             **The question.** `--listener-check` on the seeder splits the two
             candidates in one run: if the seeder answers its own handshake
             while the leechers fail, the fault is the leecher's, and if it
             stops the run at exit 17 the fault is the seeder's and the run
             says so at the instant it happens rather than four hours later.
Acceptance:  A soak whose leech cycles stop completing reports that as a
             failure and names the exit code and the first line of output of
             the cycles that failed. **And** a run with `-ListenerCheck` set
             either reproduces the stop with the seeder still answering, which
             names the leecher, or stops at exit 17, which names the seeder.

#### The first half is built, 2026-08-24

**`soak.ps1` judges its workload, and does it whether or not a ceiling is
named.** Every ceiling the script takes is a statement about the seeder, and a
seeder nobody is talking to holds all three of them; whether the run measured
its workload is a different question and is not optional.
`-LeechFailurePercent` defaults to 5 and the failing run is at **78.4**. The
committed run of 2026-08-23T09:01:32Z is at 0, and so is the twelve minute
reproduction. Zero turns it off, for `-Workload churn`, which is hostile to
its own leechers on purpose.

`leech_failed_percent` and `leech_failures` are in the report, and the closing
summary prints the first failures with their exit code and what they said.

**A failing cycle leaves why behind now**, capped at five, because a run that
fails a thousand times fails the same way a thousand times. That is the line
the finished run cannot be asked for afterwards: 1,080 failures and not one
recorded exit code.

**`-ListenerCheck <DUR>` passes `--listener-check` to the seeder.** Off by
default, so the two committed six hour runs stay comparable: the check costs
one loopback connection and one peer row per interval, and those are two of
the series this script measures. The run that asks the question turns it on.

**Run against the defect** is the finished run itself. With
`-LeechFailurePercent 5`, `bench/soak-20260823T154716064Z` fails on
`1080 of 1378 leech cycles failed, 78.37 percent`, where it reported
"every named ceiling held over 6 hours". Nothing else about that run changes,
which is the point: the numbers were never wrong.

#### The second half is one run, and it is the operator's

Both open questions fit in one soak, at a leech rate the committed run did not
use, which is also what [T-224](#t-224-the-six-hour-soaks-rss-slope-is-one-step-and-a-sawtooth-not-a-leak)
has left:

```bash
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 360 -Leechers 4 -ListenerCheck 60s -RssCeilingMiBPerHour 4 -HandleCeilingPerHour 20 -CloseWaitCeilingPerHour 1
```

#### The second half ran, 2026-08-24, and the stop did not reproduce

The operator's run is `bench/soak-20260824T164609340Z`, six hours,
`-Leechers 4 -ListenerCheck 60s`, which is the command this entry and
[T-224](#t-224-the-six-hour-soaks-rss-slope-is-one-step-and-a-sawtooth-not-a-leak)
asked for between them. **704 samples over 5.9999 hours, 2,812 leech cycles
completed, none failed**, and the run's own verdict is "every named ceiling
held over 6 hours" with an empty failure list. This time that verdict describes
a busy seeder, which is what the first half of this entry exists to
distinguish.

**Neither branch of the Acceptance's second half triggers**, because there was
no stop: nothing to attribute to a leecher, and the seeder never exited 17.

**And the parameters were never the variable.** The two runs of 2026-08-23 are
the same configuration in every field:

| | 09:01:32Z | 15:47:16Z | 2026-08-24, this one |
| --- | --- | --- | --- |
| minutes | 360 | 360 | 360 |
| sample seconds | 30 | 30 | 30 |
| workload | steady | steady | steady |
| payload | 16 MiB | 16 MiB | 16 MiB |
| leechers | 2 | 2 | **4** |
| listener check | none | none | **60s** |
| leech cycles | **1,360 completed, 0 failed** | **298 completed, 1,080 failed** | **2,812 completed, 0 failed** |

Two runs with identical parameters, one healthy for six hours and one dead
after 78 minutes. So the stop is not a configuration this entry can choose, and
it is not the leech rate either: doubling the rate produced 2,812 clean cycles.
It is a race or something outside the run, and picking parameters cannot
reproduce it on demand.

#### What this run did find is a hole in the instrument, and it is the thing to fix next

**A finished soak says the listener check was on and never says what it saw.**
`bench/soak-20260824T164609340Z.json` carries `parameters.listener_check:
"60s"` and no result anywhere: there is no `listener` key, the CSV has no
listener column, and `self_reported` carries only `peak_rss_bytes` and
`open_handles`.

The seeder does report it. Read out of the running seeder's own progress
events at `t+4.3 h`, this run was at:

```json
"listener":{"consecutive_failures":0,"failed":0,"healthy":true,"last_rtt_ms":0,"probes":257}
```

That was read from `.tmp/soak/seed.out` **while the run was in flight**, and
that file is gone: `$Root` is deleted when the run ends, which this entry
already records as the reason the failing run could not be asked anything
afterwards.

**So the Acceptance's first branch could not have been answered even if the
stop had reproduced.** "Reproduces the stop with the seeder still answering"
needs the seeder's listener health in the record, and only the exit 17 branch
survives into a finished report. That is the same defect this entry is about,
one level up: the instrument reports a pass and cannot be asked what it was
measuring.

**The fix is small and the seam is already there.** `self_reported` is built
from the seeder's own progress events, which is where `listener` already
arrives. Carrying `listener` into it, and the last event's values into the
report, costs one field and makes the flag worth setting.

**The entry stays open** on that, rather than on waiting for a recurrence. What
it needs is no longer a lucky run: it is the listener figures in the report, so
that the next occurrence answers the question by itself. The harness half is
otherwise finished, and the failure output capture built on 2026-08-23 is what
will name the leecher when one happens.

**One thing this run does settle**: `--listener-check` at 60 seconds over six
hours costs nothing a ceiling notices. `tcp_close_wait` is 0 at every one of
704 samples and the handle series has an r squared of 0.06, so the flag can be
left on in future soaks rather than being the thing that makes two runs
incomparable.

#### Closed, 2026-08-25: the listener figures are in the report, and a stop now names its own side

**`self_reported.listener` exists.** `scripts/soak.ps1` reads the `listener`
block out of the seeder's own progress events, the same events
`peak_rss_bytes` and `open_handles` already came from, and carries it into the
report, into three new CSV columns, and into `-ReadCsv`. A run without
`-ListenerCheck` writes null and empty columns, so a reader tells "not watched"
from "watched and fine" without going to `parameters`.

**The last event's values are not enough, and one of the two runs below proves
it.** `probes` and `failed` are counters the seeder accumulates, so the last
event carries the totals; `healthy` and `consecutive_failures` are levels. The
heavy churn run ends at `"healthy": true` having failed three probes and been
unhealthy at `t+40s`. Reporting only the last event would have said the
listener was fine. `worst_consecutive_failures`, `unhealthy_events` and
`first_unhealthy_elapsed_s` sit beside the last values for that reason.

**And the run names which side stopped.** When the leech failure share trips,
the failure line says what the listener was doing, which is the entry's own
question written at the instant it can be answered rather than left for
somebody to cross-read two files for four hours later.

**Run against both branches**, three minutes each, because the spontaneous stop
of 2026-08-23T15:47:16Z cannot be summoned and the shape can: heavy churn
starves the seeder's accept path, moderate churn starves only the leechers.

```bash
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 3 -SampleSeconds 10 -Workload all -Leechers 2 -ChurnConnections 20000 -ChurnConcurrency 256 -ListenerCheck 20s
```

| run | listener | what the report says |
| --- | --- | --- |
| `bench/soak-20260825T013344925Z` | 13 probes, **3 failed**, first unhealthy at `t+40s` | `1 of 6 leech cycles failed, 16.67 percent ... The seeder stopped answering its own listener probe at t+40s, so the fault is the seeder's` |
| `bench/soak-20260825T014217900Z` | 7 probes, **0 failed** | `1 of 7 leech cycles failed, 14.29 percent ... The seeder answered its own listener probe throughout, 7 probes and 0 failed, so the fault is not the seeder's accept path` |

Both carry a non-empty `failures` list, which is what the script exits 1 on.
The moderate run's exit code was read unpiped and is 1; the heavy run's was
read through a `tail`, so what that reported is the pipeline's. Its CSV is
committed beside its report because it is where the three new columns are
visible per sample.

**A fourth failure case comes with it.** `-ListenerCheck` passed, progress
events arriving, and not one of them carrying a `listener` block is a run that
was asked to watch the listener and given nothing to read. The seeder refuses
the flag when it bound no listen port, on stderr, into a file the run deletes.
That is a failure in the report now.

**What this entry does not answer, and cannot.** The stop of
2026-08-23T15:47:16Z has no attribution and will not get one: its `$Root` was
deleted, its CSV has no listener columns, and the two six hour runs either side
of it did not reproduce it. The entry closes on the instrument, which is what
its own previous section said it was waiting for: a recurrence answers the
question by itself now, and the failure output capture built on 2026-08-23
names the leecher's exit code beside it.

**One claim in `scripts/soak.ps1`'s own header was disproved on the way.** It
said `-Workload all` starves the leechers, "the same run that completed 22
downloads in two minutes without churn completed 1 and failed 2 with it". At
the default churn now: **no cycle fails**, 26 completed over three minutes with
churn and 22 over two minutes without it. The two runs are different lengths,
so the cycle counts are not a comparison and the failure count is: it is zero
either way against 2 failures out of 3. Those figures were taken before
[T-020](peers.md) closed. The comment carries the new measurement and says what
starving them now takes.
