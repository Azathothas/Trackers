# doctor

What host is this, what is installed, and what is this repo. One read-only
pass, before any of it costs a task.

Two implementations, one schema. Run whichever the host can run.

```bash
sh scripts/doctor/doctor.sh
```

```bash
pwsh -NoProfile -File scripts/doctor/doctor.ps1
```

On Windows prefer the PowerShell one. It needs no POSIX layer, so it answers
on a machine with no Git Bash, no WSL and no msys.

## Why it exists

The defect it catches is an agent that assumes its environment. A session that
assumes node is present writes a node script and finds out at the gate. A
session that assumes Linux reaches for `pkill` on a machine that wants
`taskkill`. A session that takes "most tools are available" on trust plans
around a tool that is not there.

It is also the validator. When the operator states an environment, run this
and compare. A stated fact and a measured one that disagree is the finding.

## What it is not

It is a probe, not a gate. A missing tool is data, not a failure, so it exits
0 whenever it ran. It exits 2 only when it could not run at all. Nothing here
belongs in a gate chain.

It is read-only. No installer, no config change, no network call unless the
network flag is passed, and the only file it writes is a temp file it removes.

## Flags

| flag | sh | ps | what it does |
| --- | --- | --- | --- |
| json | `--json` | `-Json` | emit the schema document instead of the report |
| text | `--text` | `-Text` | select the human report explicitly. It is the default. |
| fast | `--fast` | `-Fast` | presence only, skip every version probe |
| net | `--net` | `-Net` | also test outbound reachability |
| group | `--group vcs` | `-Group vcs` | probe one group only |

Groups: `vcs`, `runtime`, `compiler`, `pkg-lang`, `pkg-system`, `container`,
`build`, `quality`, `cli`, `cloud`, `shell`, `agent`.

## Measured runtime

On one Windows 11 machine, 86 tools, 51 of them present:

| run | sh | ps |
| --- | --- | --- |
| full | 25 s | 16 s |
| fast | 5 s | 4 s |
| one group | 2 s | 2 s |

The numbers are from this machine on 2026-08-25 and are here so a session
knows what to expect, not as a claim about any other host. Windows is the slow
case: a process spawn costs more there than anywhere else, and this spawns one
per tool. Re-measure rather than quote these if the answer matters.

## The schema, agent-doctor/1

```
schema      the string "agent-doctor/1"
generated   ISO 8601 UTC
probe       impl, fast, group
host        os flavor wsl container kernel arch distro distro_version
            shell writable_tmp network
repo        is_git root branch remote dirty commits
            remote_looks_like_template has_codegraph ecosystems
summary     tools_found tools_missing
tools[]     id group found path version
notes[]     things the probe wants said out loud
```

Field notes that are easy to read wrong:

- `flavor` is the shell environment, not the OS. The sh twin under Git Bash
  reports `msys`; the ps twin on the same machine reports `native`. Both are
  right about the environment they are in.
- `kernel` is a best-effort build identifier and its shape differs by platform.
  Do not parse it.
- `ecosystems` is read from manifest files that are actually present. It is
  evidence, not a guess from a directory name.
- `remote_looks_like_template` is a warning, not a fact about the project. It
  fires when `origin` contains the word template, which is the state a fresh
  clone of this repository is in and must leave before any project work.
- `version` empty with `found` true means the tool answered nothing. That is
  reported in `notes` and it usually means a shim rather than an install.

## The two twins have to agree

Changing a field in one means changing it in the other. The check is to run
both on one machine and compare: same top-level keys, same section keys, same
values for `os`, `arch`, `wsl`, `container`, `distro_version`, and the same
verdict per tool.

⚠ Some disagreement is correct and must not be flattened away. Each twin
reports what its own host can actually reach, and on a Windows machine with
msys installed those differ honestly:

| id | sh sees | ps sees | why |
| --- | --- | --- | --- |
| `bash` | msys bash | the Windows PATH bash | two different binaries |
| `tar` | GNU tar | Windows bsdtar | two different binaries |
| `zsh` | present | absent | msys `/usr/bin/zsh` is not on the native PATH |
| `psscriptanalyzer` | not probed | probed | a PowerShell module, invisible to sh |

## Things this cost to learn

Each of these was a real defect in this script, found by running it.

- ⛔ A greedy regex over a version line reports the wrong half of the version
  and does it confidently. `git version 2.51.0.windows.3` came back as
  `5.0.windows.3`, `v22.11.0` as `7.0`. The fix is to split into tokens and
  take the first that reads as a version. A wrong number is worse than a blank
  one, because a blank one gets checked.
- ⛔ The name may be joined to the number by a hyphen. `jq-1.8.2` read as no
  version at all until the pattern allowed one.
- ⛔ Several tools block for as long as you let them. `kubectl version` without
  `--client` contacts a cluster. Every probe is time-limited at six seconds,
  and a timeout is reported as its own fact rather than as an absent tool.
- ⛔ In the sh twin, `probe_version` runs inside `$( )`, which is a subshell,
  so an assignment inside it is discarded. The caller reads the exit code.
- ⛔ In the ps twin, `Process.Start` with `UseShellExecute` false cannot run a
  `.ps1` or a `.cmd`. On Windows the node ecosystem ships shims and scoop's are
  `.ps1`, so npm, pnpm, yarn, wrangler and codegraph all reported as
  uninstalled stubs until the launcher handled them.
- ⛔ In the ps twin, reading a value from merged stdout and stderr put a git
  fatal into the `branch` field. A version probe merges the streams on purpose,
  because java prints its version to stderr. Anything reading a value must not.
- ⛔ `.NET` says `X64` where `uname -m` says `x86_64`. Normalised, or one
  machine reads as two.
- ⚠ `wsl.exe` writes UTF-16LE, which a redirected stdout reads as empty. It is
  probed for presence only.
- ⚠ `Invoke-ScriptAnalyzer` is a cmdlet, not an application, so a PATH lookup
  can never find it. It is checked as a module instead.
