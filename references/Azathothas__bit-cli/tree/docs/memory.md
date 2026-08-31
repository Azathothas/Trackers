# Memory and handles

What bounds each, what is reported, and what a long run has actually measured.

The entries behind this are in [`TODO/memory.md`](../TODO/memory.md).

## What is reported

`download` and `seed` report peak resident memory, CPU time and handle count,
in the final report and in every `progress` event. Nothing has to be sampled
from outside the process to know what a run cost.

## What is bounded, and by which flag

| bound | flag | what happens at the limit |
| --- | --- | --- |
| open file descriptors | `--max-open-files` | files are closed and reopened on demand |
| total process handles | `--max-handles` | the run exits 16, loudly |
| resident memory | `--max-rss` | the run exits, loudly |
| per-source window cache | `--web-seed-chunk-size` and the source count | the budget is reported by `webseed list --json` before the run |

`--max-handles` and `--max-rss` are backstops rather than bounds: they stop the
process rather than the thing consuming the resource. A bound that is a real
bound is named in the entry that measured it.

## What a long run measured

`scripts/soak.ps1` samples a long-lived seeder under one of six workloads and
writes the series to a CSV under `bench/`. It is run by the operator in a
foreground terminal, never inside a session, because a session ending kills the
process it started.

```bash
pwsh -NoProfile -File scripts/soak.ps1 -ReadCsv bench/soak-20260823T154716064Z.csv
```

Three things that run taught, and all three are about the instrument rather
than the tool. A soak killed mid-append leaves a NUL-filled tail that reads as
one more sample of zeros, so the reader validates its own input now. A report
that judges only its named ceilings will report a pass over a workload that
stopped: the harness judges the workload too, and its leech failure threshold
defaults to 5 percent. And a run given `-ListenerCheck` records what the check
saw, not only that it was asked for, so a run whose workload stops says which
side stopped.

`self_reported.listener` in the report and the three `listener_` columns in the
CSV are that record. They are null and empty on a run without `-ListenerCheck`,
which is how a reader tells a listener nobody watched from one that answered
every probe. `probes` and `failed` are totals for the run;
`worst_consecutive_failures` and `first_unhealthy_elapsed_s` sit beside them
because a listener that failed in the middle and recovered ends the run looking
healthy.
