# agent-tooling.md

⭐ **Read this before installing anything, writing your own, or deciding a job
cannot be done here.** It is a catalogue of what already exists.

⛔ **It carries names, links and one line each, and nothing else.** No flags, no
exit codes, no worked invocations. Every one of those is the tool's own to
change, and a page that copies them becomes wrong without anybody editing it.

---

## The three reflexes this page exists to stop

| the reflex | what it costs |
| --- | --- |
| **installing something** | a system change nobody asked for, on somebody else's machine, that outlives the session |
| **writing your own** | a second implementation of a solved problem, with its own defects, that nobody else will ever fix |
| ⛔ **refusing, because a tool "is not available"** | the most expensive of the three. RULES 10.1a: a missing tool closes one route, not the question |

⚠ **A tool being absent is a measurement, not a verdict.** Say what is missing,
then find another route. ⭐ **Three routes considered and rejected is a finding;
one route tried is a stop.**

⚠ **Presence is not capability.** Measured on one Windows 11 host on
2026-08-31: `python3` resolved to a Microsoft Store stub that exits 49 without
running anything, while `python` was a working 3.13.15. ⛔ **Probe by running
the tool, not by finding it.**
[`conventions/shell.md`](conventions/shell.md) section 6.

---

## What this repository ships

⭐ Everything here runs with **no network** and needs nothing installed beyond
Python 3.11. A gate that has to fetch something is a gate that is red whenever
somebody else's host is down.

| tool | what it does |
| --- | --- |
| ⭐ [`../scripts/check-gate.py`](../scripts/check-gate.py) | runs every check below and prints one verdict, reading each exit code unpiped |
| [`../scripts/check-todo.py`](../scripts/check-todo.py) | the work record agrees with itself, and every count is re-derived from the rows |
| [`../scripts/check-citations.py`](../scripts/check-citations.py) | every citation resolves: paths, links, rule ids, claim ids, entry ids, decision ids, **line numbers**, and whether a load-bearing line still says what it is cited for |
| [`../scripts/check-corpus-integrity.py`](../scripts/check-corpus-integrity.py) | every file under `references/` is committed, and no ignore rule can reach one |
| [`../scripts/check-decision-record.py`](../scripts/check-decision-record.py) | the decision counts add up and every closed decision records its rejected alternatives |
| [`../scripts/check-no-third-party-imports.py`](../scripts/check-no-third-party-imports.py) | standard library only, parsed with `ast` rather than grepped |
| [`../scripts/check-docs.py`](../scripts/check-docs.py) | fenced shell blocks parse, no shell-unsafe placeholder, no banned vocabulary, no orphan page |
| [`../scripts/check-markers.py`](../scripts/check-markers.py) | only the five defined characters, and not too many of them |
| [`../scripts/check-control-bytes.py`](../scripts/check-control-bytes.py) | a literal control byte in any tracked text file |
| [`../scripts/check-one-home.py`](../scripts/check-one-home.py) | one fact, one home: no long sentence in two documents |
| [`../scripts/check-no-secrets.py`](../scripts/check-no-secrets.py) | anything in the tree that must not be published, including a private tracker's passkey |
| [`../scripts/check-vendor-pin.py`](../scripts/check-vendor-pin.py) | the vendored helpers still match their recorded digests |
| [`../scripts/check-vantage-metadata.py`](../scripts/check-vantage-metadata.py) | every health record carries its vantage. ⚠ Exits 2 until P2 exists, and that is the correct answer |
| [`../scripts/generate.py`](../scripts/generate.py) | build the dataset. `--offline` runs it against the pinned fixtures |
| [`../scripts/fetch-reference-comments.py`](../scripts/fetch-reference-comments.py) | capture an upstream's comment threads. Touches the network, so never in CI |
| [`../experiments/`](../experiments/) | the instruments. **Every measured number this project publishes came from one of these**, and each one prints its conditions |

[`../scripts/README.md`](../scripts/README.md) is the contract all of them are
held to.

---

## What is vendored, pinned

| tool | what it does |
| --- | --- |
| [`../scripts/vendor/toolkit/doctor.sh`](../scripts/vendor/toolkit/) and its `.ps1` twin | the environment probe. What host, what shell, what tools resolve, what the repository is. ⭐ A probe, not a gate: a missing tool is data, so it exits 0 either way |
| [`../scripts/vendor/toolkit/git-sync.sh`](../scripts/vendor/toolkit/) and its `.ps1` twin | commit and push with the identity and attribution rules enforced rather than remembered |

[`methodology/vendoring.md`](methodology/vendoring.md) is the rule and
[`../scripts/vendor/toolkit/README.md`](../scripts/vendor/toolkit/README.md)
says why these two are copied while the checks were rewritten.

---

## What lives upstream

⚠ **Fetch by a pinned commit, never a branch.** A moving reference runs code
nobody reviewed.

| tool | upstream | what it does |
| --- | --- | --- |
| `mine-repo` | [`Azathothas/TEMPLATE`](https://github.com/Azathothas/TEMPLATE) | fetches everything a reference sweep needs in one call, including the four tracker sources that get forgotten. ⚠ Shell only; this project's own comment fetcher covers the half it had a gap in |
| `wsl-toolkit` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | creates a throwaway Linux distro on a Windows host, runs a command in it, and destroys it. [`containers.md`](containers.md) |
| `fill-license` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | writes a `LICENSE` from a canonical text, and refuses the ones whose notice is not the writer's to alter |
| `deslop` | [`Azathothas/ToolKit`](https://github.com/Azathothas/ToolKit) | inventories the files in a tree that address a reader as an agent |

---

## The general-purpose ones

| job | reach for | why not the obvious thing |
| --- | --- | --- |
| talk to a code host's API | [`gh`](https://cli.github.com/) | ⛔ **never against somebody else's repository**, under any framing. Against *this* one, RULES 13.1 sanctions writes: releases, tags, the data branch, issues and workflow runs. This row used to say "reads only", which was stricter than the rule it points at and would have blocked [T-003](../TODO/claims.md). [`security/remote-ops.md`](security/remote-ops.md) |
| fetch a URL where the direct route is blocked | RULES 16's read-only proxies | they carry none of your credentials, which a rule about a route that can write does not give you |
| read or reshape JSON | [`jq`](https://jqlang.github.io/jq/) | ⛔ never a regular expression over JSON. A bracket inside a string value is how one page joiner lost an entire comment corpus |
| time a command honestly | [`hyperfine`](https://github.com/sharkdp/hyperfine) | a single timed run is not a measurement |
| search a tree | [`rg`](https://github.com/BurntSushi/ripgrep) | it locates; it does not confirm. Open the file |
| count characters outside ASCII | `rg` | ⚠ a byte class splits a three-byte character into three fragments, so a `grep -o` count comes out three times too high and looks like real output |
| run something on Linux from Windows | `wsl-toolkit`, or podman through `CONTAINER_ENGINE` | ⛔ never install a distro by hand and leave it registered |

---

## Adding a row

1. **The tool has to already exist and be reachable.** This is a catalogue, not
   a wish list.
2. **One line, and no behaviour.**
3. **Say where it lives**, as a link a reader can open.
4. **A row for a tool nothing here uses is a row somebody maintains for
   nothing.** Delete it instead.
