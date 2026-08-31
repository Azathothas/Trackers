# Hooks

Flags that run a command of yours at a point in a run. Two commands have them,
`download` and `seed`, and they do not mean quite the same thing.

## `bit-cli download`

| Flag | When it runs | How often |
| --- | --- | --- |
| `--on-complete <COMMAND>` | a torrent finished | once per finished torrent |
| `--on-error <COMMAND>` | a torrent did not finish | once per unfinished torrent |
| `--on-piece-verified <COMMAND>` | a piece verified | once per piece |

**Once per torrent, not once per run.** `bit-cli download a.torrent b.torrent
-j 2 --on-complete notify` runs `notify` twice, with `BIT_CLI_INFO_HASH`
differing. A run where one torrent finished and the other did not runs
`--on-complete` for the first and `--on-error` for the second, in the same run.
Every variable below that describes bytes or time describes **that torrent**;
the four that describe the whole run say so in their names.

## `bit-cli seed`

| Flag | When it runs | How often |
| --- | --- | --- |
| `--on-complete <COMMAND>` | the payload passed its hash check and the listener is up | once, before serving |
| `--on-error <COMMAND>` | the run failed to start, or died | once |

**A seeder does not complete**, so `--on-complete` there is the moment it
starts being useful rather than the moment it stops. It fires before the serve
loop and not again: a seeder that runs for a week runs the hook on the first
second of it.

**`BIT_CLI_FINISHED` says whether the payload is whole, not whether the run
succeeded.** A partial seed is a legitimate thing to be doing, so it still
fires `--on-complete`, with `BIT_CLI_FINISHED=false` and
`BIT_CLI_DOWNLOADED_BYTES` short of `BIT_CLI_TOTAL_BYTES`. On `download` the
two questions are the same one and the flag that fires says the answer.

**`BIT_CLI_STOPPED` is `serving`** on a seeder's `--on-complete`, which is the
one value `download` never sets. On `--on-error` it is `error`.

**Three things a seeder does not have.**

- **No `--on-piece-verified`.** A seeder verifies every piece once, during the
  hash check on add, so the hook would fire in a burst at startup and then be
  silent for days. What a seeder would want is a hook per peer, which is a new
  trigger rather than this one moved, and nothing has asked for it.
- **Nothing on `--announce-only`.** That run announces and stops without ever
  serving, so the moment `--on-complete` names does not happen in it.
- **No hook on `peers`, `bench leech` or `bench seed`.** All three **refuse**
  the hook flags with exit 2 rather than accepting them and running nothing.
  That is the difference between a caller learning at once and a caller waiting
  for a notification that was never coming. The flags reach those commands
  because five commands share one argument struct, which is why the refusal is
  explicit rather than absent. See `../TODO/cli-surface.md`, T-214.

## Nothing is interpolated into a command line

The command is run as you wrote it, through `cmd /C` on Windows and `sh -c`
elsewhere, and every fact arrives as an environment variable. A file named
`; rm -rf /` is a file name.

```bash
bit-cli download release.torrent --on-complete 'echo "$BIT_CLI_NAME landed in $BIT_CLI_DIR"'
```

That is the reason there is no `%f`-style substitution here and will not be
one: a torrent's own bytes decide the name, and a name that becomes part of a
command line is a name that can become a command.

## The variables

They share the `BIT_CLI_` prefix with the environment variables that set a
**configuration** value, and a name in this table is never read as one. A run
refuses a `BIT_CLI_*` name that is neither, because a typo in a deployment
script is how a production setting goes missing, and the list below is what
keeps a hook whose command is `bit-cli` from having the child refuse its
parent's variables. See `TODO/cli-surface.md`, T-222.

Set for every hook:

| Variable | What it holds |
| --- | --- |
| `BIT_CLI_VERSION` | The version of `bit-cli` that ran. |
| `BIT_CLI_HOOK` | Which hook this is: `on-complete`, `on-error`, or `on-piece-verified`. |
| `BIT_CLI_INFO_HASH` | The torrent's info hash, lower-case hex. |
| `BIT_CLI_NAME` | The torrent's name. |
| `BIT_CLI_DIR` | The directory this torrent's payload was written to. |

Set for `--on-complete` and `--on-error`:

| Variable | What it holds |
| --- | --- |
| `BIT_CLI_SOURCE` | The source as it was given on the command line. |
| `BIT_CLI_TOTAL_BYTES` | The torrent's total length. **This torrent's**, not the run's. |
| `BIT_CLI_DOWNLOADED_BYTES` | What arrived for this torrent, from every source. |
| `BIT_CLI_FROM_PEERS_BYTES` | What arrived from the swarm for this torrent. |
| `BIT_CLI_FROM_WEB_SEEDS_BYTES` | What arrived from HTTP sources for this torrent. |
| `BIT_CLI_FINISHED` | `true` when every selected piece verified, `false` otherwise. |
| `BIT_CLI_STOPPED` | Why this torrent stopped: `completed`, `timeout`, `stalled`, and so on. |
| `BIT_CLI_ELAPSED_MS` | How long this torrent took, in milliseconds. |
| `BIT_CLI_ERROR` | The failure, when there was one. **Absent** on success rather than empty, so `[ -n "$BIT_CLI_ERROR" ]` is a correct test. |
| `BIT_CLI_TORRENTS` | How many torrents the whole run was asked for. |
| `BIT_CLI_COMPLETED` | How many of them finished. |
| `BIT_CLI_FAILED` | How many did not. |
| `BIT_CLI_RUN_ELAPSED_MS` | How long the whole run took, in milliseconds. |

Set for `--on-piece-verified`:

| Variable | What it holds |
| --- | --- |
| `BIT_CLI_PIECE` | The piece index that just verified. |
| `BIT_CLI_PIECE_LENGTH` | That piece's length in bytes. The last piece of a torrent is usually shorter than the rest. |

`--on-piece-verified` deliberately carries nothing about progress. It fires
thousands of times and a progress figure read per piece is a figure that changed
before the hook could read it; `--jsonl` is the surface for watching a run.

## What `--on-piece-verified` costs

**One piece is one process, and a process is not free.** Measured on this
project's development machine, Windows 11 with a 20 core CPU: **1,025
invocations took 47.55 seconds**, which is 46 ms each.

Read that number for what it is. The command measured was `cmd /C rem`, and a
hook is already run through `cmd /C`, so each invocation started **two**
processes rather than one: about 23 ms per `cmd`. An ordinary hook is one
process and costs about half of it. Either way a 4 GiB torrent at a 1 MiB piece
length is 4,096 pieces, so a hook that does anything at all is minutes of
process startup. Two bounds stop that deciding how fast the download goes.

1. **It runs on its own thread.** The download never waits for a hook process
   to exit. Without this a hook taking 20 ms would cap the run at 50 pieces a
   second whatever the network could do.
2. **The queue is bounded at 1,024 invocations**, and what does not fit is
   **counted** rather than waited for. A hook slower than pieces arrive is
   yours to fix, and the run tells you what it cost rather than quietly
   dropping notifications or quietly slowing down.

The counts are in `--json` under `hooks`, and a run that skipped any says so on
stderr:

```json
"hooks": { "ran": 4096, "failed": 0, "skipped": 0 }
```

If you see `skipped`, the hook is slower than the download. Write to a queue and
return, or use `--jsonl` and read `piece_verified` events instead, which are
free.

## Exit codes

A hook that exits non-zero is **counted and warned about**, and does not change
the run's exit code. The download is what you asked for; the hook is a
notification about it. A hook that cannot be started at all is the same: a
warning on stderr and a count.

## Where this is checked

`crates/bit-cli/src/hooks.rs` holds the one list of variables. Two tests hold
this file to it: `every_hook_variable_is_documented` fails when a variable has
no row here, and `every_variable_a_hook_sets_is_in_the_list` fails when the
code and the list disagree in either direction. See `TODO/cli-surface.md`,
T-115.

| Claim | Held by |
| --- | --- |
| `--on-complete` fires once per finished torrent, with its own info hash | `on_complete_fires_once_per_torrent_with_its_own_info_hash` |
| A mixed run fires both hooks | `a_mixed_run_fires_on_complete_and_on_error` |
| `--on-piece-verified` fires once per piece | `on_piece_verified_fires_once_per_piece` |
| A seeder fires `--on-complete` once, when it is ready to serve | `on_complete_fires_once_when_the_seeder_is_ready_to_serve` |
| A hook that fails does not fail the seeding | `a_failing_hook_does_not_fail_the_seeding` |
| A hook that failed names the error and which hook it was | `a_failed_hook_names_the_error_and_says_which_hook_it_is` |

## Telling something else what happened

`--on-complete` and `--on-error` run a command of yours **once per torrent**, and
`--on-piece-verified` runs one per verified piece. Every fact arrives as a
`BIT_CLI_*` environment variable and nothing is interpolated into a command line,
so a file named `; rm -rf /` is a file name.

```bash
bit-cli download a.torrent b.torrent -j 2 --on-complete 'echo "$BIT_CLI_NAME landed in $BIT_CLI_DIR"'
```

That runs twice, with `BIT_CLI_INFO_HASH` differing. A run where one torrent
finished and the other did not runs `--on-complete` for the first and
`--on-error` for the second.

[`docs/hooks.md`](hooks.md) lists every variable, says what
`--on-piece-verified` costs and how it is bounded, and what an exit code does.
