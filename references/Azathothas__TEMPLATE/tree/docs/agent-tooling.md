# agent-tooling.md

⭐ **Read this before you install anything, write your own, or decide a job
cannot be done here.** It is a catalogue of tools that already exist, with
where each one lives.

⛔ **It carries names, links and one line each, and nothing else.** No flags, no
options, no exit codes, no worked invocations. Every one of those is upstream's
to change, and a page that copies them is a page that becomes wrong without
anybody editing it. ⚠ Read the tool's own documentation at the link for how to
call it.

---

## The three reflexes this page exists to stop

| the reflex | what it costs |
| --- | --- |
| **installing something** | a system change nobody asked for, on somebody else's machine, that outlives the session |
| **writing your own** | a second implementation of a solved problem, with its own defects, that nobody else will ever fix |
| ⛔ **refusing, because a tool "is not available"** | the most expensive of the three. [`methodology/sessions.md`](methodology/sessions.md) is the rule: a missing tool closes one route, not the question. |

⚠ **A tool being absent is a measurement, not a verdict.** Run the probe, say
what is missing, then find another route. Three routes considered and rejected
is a finding; one route tried is a stop.

---

## What this repository ships

⭐ Everything here runs with **no network**, which is why it is here rather than
upstream. A gate that has to fetch a check is a gate that is red when somebody
else's host is down, and a check fetched at gate time is code nobody reviewed
judging the tree.

| tool | what it does |
| --- | --- |
| [`../scripts/doctor/`](../scripts/doctor/) | the environment probe. What host, what shell, what tools, what the repository is. A probe, not a gate. |
| [`../scripts/common/check-gate`](../scripts/common/) | runs every check below and prints one verdict |
| [`../scripts/common/check-docs`](../scripts/common/) | links resolve, fenced blocks parse, banned vocabulary, orphan pages |
| [`../scripts/common/check-markers`](../scripts/common/) | only the five defined characters, and not too many of them |
| [`../scripts/common/check-one-home`](../scripts/common/) | one fact, one home: no long sentence in two documents |
| [`../scripts/common/check-placeholders`](../scripts/common/) | did a template placeholder survive into a real file |
| [`../scripts/common/check-control-bytes`](../scripts/common/) | a literal control byte in a tracked text file |
| [`../scripts/common/check-changelog`](../scripts/common/) | the four changelog rules a machine can hold |
| [`../scripts/common/check-no-secrets`](../scripts/common/) | does anything in the tree carry something that must not be published |
| [`../scripts/common/check-remote-items`](../scripts/common/) | do the open items against this repository say anything that survives being checked |
| [`../scripts/common/check-twins`](../scripts/common/) | do both halves of every pair still answer the same way |
| ⭐ [`../scripts/common/mine-repo`](../scripts/common/) | fetch everything a reference sweep needs, and keep it. [`methodology/references.md`](methodology/references.md) is the procedure. |

---

## What lives upstream

⛔ **These were here and were removed.** A tool kept in two repositories
acquires two sets of defects, and one of the two never gets fixed. Two of the
four below were carrying a defect their upstream had already fixed on the day
they left. [`history/twins-and-scripts.md`](history/twins-and-scripts.md) has
the comparison, including the two that had not drifted.

⚠ **Fetch by a pinned commit or a release tag, never a branch.** A moving
reference runs code nobody reviewed. [`containers.md`](containers.md) has the
worked shape of a pinned wrapper and what it cost to get right.

| tool | upstream | what it does |
| --- | --- | --- |
| `wsl-toolkit` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | creates a throwaway Linux distro on a Windows host, runs a command in it, and destroys it. [`containers.md`](containers.md) is the procedure. |
| `git-sync` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | commit and push with [`conventions/git.md`](conventions/git.md)'s rules enforced rather than remembered |
| `fill-license` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | writes a `LICENSE` from a canonical text with the holder filled in, and refuses the ones whose notice is not yours to alter. [`../LICENSES/README.md`](../LICENSES/README.md) says which. |
| `deslop` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | inventories the files in a tree that address a reader as an agent. [`methodology/lean-adoption.md`](methodology/lean-adoption.md) is the procedure. |
| `write-file` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | writes or patches a file without the shell touching the payload. [`conventions/shell.md`](conventions/shell.md) section 1 is why that matters. |

---

## The general-purpose ones, which are somebody else's entirely

⚠ **Presence is not capability.** The probe reports what resolves on `PATH`,
and a name that resolves can still be the wrong program: measured on one
Windows 11 machine, `sort` resolved to PowerShell's own `Sort-Object` alias and
`python3` resolved to a Microsoft Store stub that exits 49 without running
anything. ⛔ Probe by RUNNING the tool, not by finding it.

| job | reach for | why not the obvious thing |
| --- | --- | --- |
| talk to a code host's API | [`gh`](https://cli.github.com/) | reads only, and never against somebody else's repository. [`security/remote-ops.md`](security/remote-ops.md). |
| fetch a URL | `curl`, or the host's own client | in Windows PowerShell 5.1 `curl` is an ALIAS for a cmdlet that takes different arguments. [`conventions/shell.md`](conventions/shell.md). |
| read or reshape JSON | [`jq`](https://jqlang.github.io/jq/) | ⛔ never a regular expression over JSON. A bracket inside a string value is how this repository's own page joiner lost an entire comment corpus. |
| read or reshape YAML | [`yq`](https://github.com/mikefarah/yq) | the same reason |
| lint POSIX shell | [`shellcheck`](https://www.shellcheck.net/) | it finds the quoting and exit-code traps [`conventions/shell.md`](conventions/shell.md) documents, before they ship |
| lint PowerShell | [`PSScriptAnalyzer`](https://github.com/PowerShell/PSScriptAnalyzer) | the same, on the half a POSIX linter cannot see |
| time a command honestly | [`hyperfine`](https://github.com/sharkdp/hyperfine) | a single `time` run is not a measurement. [`methodology/experiments.md`](methodology/experiments.md) says what one owes. |
| count lines of code | [`scc`](https://github.com/boyter/scc) or [`tokei`](https://github.com/XAMPPRocky/tokei) | ⚠ counters disagree about blank and comment lines, so name which one produced a number |
| search a tree | [`rg`](https://github.com/BurntSushi/ripgrep) | it locates; it does not confirm. Open the file. |
| run something on Linux from Windows | `wsl-toolkit`, above | never install a distro by hand and leave it registered. [`containers.md`](containers.md). |

---

## Adding a row

1. **The tool has to already exist and be reachable.** This is a catalogue, not
   a wish list.
2. **One line, and no behaviour.** If the row needs a flag to be useful, the
   flag belongs in upstream's documentation and the row belongs in
   [`containers.md`](containers.md) or nowhere.
3. **Say where it lives**, as a link a reader can open.
4. **A row for a tool nothing in this repository uses is a row somebody
   maintains for nothing.** Delete it instead.
