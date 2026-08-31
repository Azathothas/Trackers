# twins-and-scripts.md

Two retirements, both about `scripts/`, both from 2026-08-30.

---

## 1. "Only the probe has a twin"

⛔ **This was the rule, and it was wrong.** Every check gained a PowerShell twin
because a native PowerShell session cannot be assumed to have the tools a POSIX
check needs.

⚠ **The retired wording did not survive a change; it shipped BESIDE the rule
that replaced it, in the same commit.** `3191d08`, 2026-08-25, carries both the
header below and the section 7 that says the opposite, in as many words. So the
file whose whole job is to stop two implementations drifting was telling its
next maintainer not to write the second one, from the day it existed, with the
correction two hundred lines further down.

⛔ **It then survived a maintenance pass.** `6eaf4b5`, 2026-08-28, edited this
same file and did not reconcile the two. That is the failure mode
[`../conventions/prose.md`](../conventions/prose.md) records: a reader takes the
first paragraph and acts on it, and nobody reads far enough to find the
contradiction. ⭐ It is also the argument for reading a file's header against
its body rather than trusting that a file agrees with itself.

Kept verbatim:

> ```text
> -- WHY ONLY THE PROBE HAS A TWIN, AND WHY THAT IS NOT AN OVERSIGHT
>
> Every other check here is POSIX sh alone, deliberately. Two implementations
> of one rule is two places for that rule to be wrong, so a twin has to earn
> itself. The probe earns it and nothing else does:
>
>   scripts/doctor/  RUNS BEFORE YOU KNOW WHAT IS INSTALLED. That is its whole
>                    job. It cannot require a POSIX layer, because "is there a
>                    POSIX layer" is one of the questions it answers. So it
>                    needs a native implementation per host family.
>
>   everything else  runs AFTER the probe has reported. By then sh is known to
>                    be present or known to be absent, and on Windows that means
>                    Git Bash, WSL or msys, all of which the probe reports. A
>                    second implementation would add a drift surface and buy
>                    nothing.
> ```

**What replaced it**, and the measurement that forced the change:
[`../../scripts/README.md`](../../scripts/README.md), the section on what two
implementations cost. On one Windows 11 machine, in a native PowerShell session
with Git Bash off `PATH`, `sed` was absent and `sort` resolved to
`Sort-Object`, which deduplicates case-insensitively and dropped two of four
distinct values while exiting 0.

⭐ **The half of the retired rule that survived** is its last sentence, and it
is now the live rule: wherever a twin exists, `check-twins.sh` covers it, and
adding a twin without adding it there is how drift starts.

---

## 2. The helpers that moved to `Azathothas/ToolKit`

⛔ **Removed from this repository on 2026-08-30**: `deslop`, `git-sync`,
`fill-license` and `write-file.mjs`, both halves of each, plus the
`scripts/powershell-windows/` wrapper.

The reasoning is in [`../agent-tooling.md`](../agent-tooling.md) and it is one
sentence: a tool kept in two repositories acquires two sets of defects and one
of them never gets fixed. What is kept here is what a project must be able to
run with no network, which is the probe and the checks.

⚠ **Two of the four were behind their upstream on the day they were removed**,
which is the argument rather than a footnote. The other two had not drifted at
all, and saying so is what makes the first two mean something:

| helper | how it compared to upstream, measured 2026-08-30 |
| --- | --- |
| ⛔ `git-sync.ps1` | **behind.** No `PositionalBinding = $false`, so a list argument overflows onto the next free parameters. Reproduced on the file itself, recovered from `HEAD` and instrumented to print its bindings rather than commit: `-Message "fix: thing" -Gate A,B,C,D` bound `-Gate` to `A` and put `B`, `C` and `D` into `-BodyFile`, `-Name` and `-Email`. A commit would have landed with a shell command as its author name and another as its email. ⚠ The post-commit check cannot catch it: it compares the landed commit against the same mis-bound values, so it prints `identity verified`. Upstream had fixed it. |
| ⛔ `deslop` | **behind.** Both halves printed the number of files they had PLANNED to remove and never read the directory back, so a file something held open reported as deleted. That is the row [`../conventions/forbidden-patterns.md`](../conventions/forbidden-patterns.md) already carried about a delete that reported success. Upstream had fixed it. |
| `fill-license` | **identical**, byte for byte, both halves. Removed for the one-copy reason, not because it had drifted. |
| `write-file.mjs` | **identical except for two spaces in one comment line.** It was also the only dependency on `node` anything under `scripts/` had. |

⚠ **The two that had not drifted are the honest half of this.** "A second copy
drifts" is a claim about tendency, not about every file, and two of four is
what it actually looked like here after five days.

⭐ **What this repository kept and upstream does not have** is
`scripts/common/mine-repo`, on the operator's ruling: it encodes this
methodology's reference-sweep procedure rather than a general job.

### The two comparisons that were removed with them

`check-twins.sh` lost its `git-sync --check` pair row and its licence
byte-for-byte comparison. Both are recorded here because the second one caught
a real defect and the reason it existed should not be lost with the row:

> ```text
> fill-license is compared on its OUTPUT, not on a status line, because its
> output IS the artefact and a corrupted licence exits 0. The over-replacement
> that bit this repository produced a valid-looking file.
> ```

⚠ **That defect is the reason `scripts/README.md` asks every new check to be
mutation-proved**, and that instruction is live. Only the comparison moved.
