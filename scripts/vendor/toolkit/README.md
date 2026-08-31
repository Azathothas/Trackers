# scripts/vendor/toolkit

Four files fetched verbatim from
[`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) at the commit
[`PIN.json`](PIN.json) records. They are not this project's code.

| file | what it is |
| --- | --- |
| `doctor.sh`, `doctor.ps1` | the environment probe. What host this is, what shell, what tools resolve, and what the repository looks like. A probe, not a gate: a missing tool is data, so it exits 0 either way. |
| `git-sync.sh`, `git-sync.ps1` | commit and push with the identity and attribution rules enforced rather than remembered. [`../../../docs/conventions/git.md`](../../../docs/conventions/git.md) is what it enforces. |

Run whichever half the host has. On Windows use the `.ps1` with
`pwsh -NoProfile -File`: a native PowerShell session may have no `sed` at all,
and its `sort` resolves to a cmdlet alias that answers differently.

## Why these are here and the checks are not

A tool kept in two repositories acquires two sets of defects, and one of the
two never gets fixed. These two do a job that has nothing to do with tracker
measurement, they are maintained upstream, and copying them unchanged means a
later version is a re-fetch rather than a merge.

The checks under [`../..`](..) went the other way and were written in Python
instead of taken as shell pairs, because RULES 15.5 makes a `.sh` that a gate
depends on a platform requirement in disguise, and because one implementation
cannot disagree with itself.

## Keeping the pin honest

`python3 scripts/check-vendor-pin.py` compares the bytes on disk against the
recorded digests. It never fetches: a gate that reaches the network is red
whenever somebody else's host is.

Taking a newer version is a deliberate act, not a sync that happens on its own.
[`../../../docs/methodology/template-sync.md`](../../../docs/methodology/template-sync.md)
is the procedure, and
[`../../../docs/methodology/vendoring.md`](../../../docs/methodology/vendoring.md)
is what to do if a defect here has to be fixed here.

⛔ **Read a new digest from the raw endpoint, never from a working tree.**
`docs/methodology/vendoring.md` carries the reason, and
`scripts/check-vendor-pin.py` normalises line endings before comparing so that
a Windows checkout and a Linux one answer the same.
