# The two deep reviews, for the half a machine can do.
#
# TODO/RULES.md section 2 ends every session with two reviews: every claim
# against the code or the path it cites, and then a cold read looking for a doc
# contradicting another doc, an entry id that does not exist, a cited path that
# does not resolve, counts that no longer add up. Three of those four are
# mechanical, and doing them by hand is how they get skipped.
#
# On 2026-08-22 this would have caught two things that had been wrong for at
# least a session: TODO/INDEX.md's row for T-184 said `open` while its entry
# said `done`, and the priority table totalled 141 against 146 rows.
#
# What it checks:
#
#   1. Every INDEX row's status matches that entry's own `Status:` line.
#   2. Every entry in TODO/*.md has a row in INDEX.md, and every row has an
#      entry.
#   3. The counts prose and the priority table both agree with the rows.
#   4. Every `T-NNN` referenced from any TODO file is an entry that exists.
#   5. Every `TODO/<file>.md` and `(file.md)` link resolves.
#   6. Every `crates/...:NNN` citation resolves to a file with that many lines.
#   7. No file has a NUL byte in it. One got in on 2026-08-22 and `grep`
#      answered "Binary file TODO/trackers.md matches" instead of the line.
#   8. `patches/TASKS.md`'s table agrees with the entries it names: the same
#      priority and the same status, and its own counts add up.
#   9. `TODO/PROGRESS.md` carries what RULES.md section 2 step 2 says it must,
#      and its entry counts and patch count agree with what is on disk.
#
# 8 and 9 exist because of what happened on 2026-08-22. That session closed
# both P0 entries, wrote it into the entries and into PROGRESS.md, and pushed.
# `patches/TASKS.md` still said `T-020 | P0 | open` at HEAD for the whole of
# the next session, because the file was rewritten after the last push and
# nothing anywhere compared it to anything. `gates.ps1` runs this now, so a
# record that contradicts the tree cannot be pushed: the working tree at that
# moment had the entry saying done and the table saying open, which is the
# disagreement this reports.
#
# What it does not check: whether a claim is true. That is the review this does
# not replace, and the point of doing the mechanical half in one second is to
# leave the time for the half that needs reading.
#
# Usage:
#   pwsh -NoProfile -File scripts/check-todo.ps1
#   pwsh -NoProfile -File scripts/check-todo.ps1 -Json
#
# Exit codes: 0 everything agrees, 1 something does not, 2 could not run.

[CmdletBinding()]
param([switch]$Json)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$todo = Join-Path $repo "TODO"
if (-not (Test-Path $todo)) {
    [Console]::Error.WriteLine("check-todo: no TODO/ at $repo")
    exit 2
}

$problems = [System.Collections.ArrayList]::new()
function Problem([string]$kind, [string]$text) {
    [void]$problems.Add([ordered]@{ kind = $kind; detail = $text })
}

$files = @(Get-ChildItem -Path $todo -Filter *.md -File)

# The record is not only `TODO/`. `patches/TASKS.md` is the ordered list of
# vendored work, `patches/UPSTREAM.md` is the Apache-2.0 record of what was
# changed in somebody else's code, and `patches/README.md` says how both are
# worked on. All three name entries and cite paths, and none of them was
# compared to anything until 2026-08-22.
$patchDocs = @("TASKS.md", "UPSTREAM.md", "README.md") |
    ForEach-Object { Join-Path $repo "patches/$_" } |
    Where-Object { Test-Path $_ } |
    ForEach-Object { Get-Item $_ }

# The corpus index cites the corpus, and until 2026-08-24 nothing checked
# those citations at all: the pass below resolved a corpus path written in a
# `TODO/` entry and never one written in `reference/RESEARCH.md`, which is
# where almost all of them are. 327 of them, and a trim that moved a path
# would have broken every one silently.
#
# `reference/` is gitignored and absent on a fresh clone, so this adds nothing
# to a run that cannot see it.
$corpusDocs = @()
$corpusRoot = Join-Path $repo "reference"
if (Test-Path $corpusRoot -PathType Container) {
    foreach ($name in @("RESEARCH.md", "README.md")) {
        $candidate = Join-Path $corpusRoot $name
        if (Test-Path $candidate) { $corpusDocs += Get-Item $candidate }
    }
    $historyDir = Join-Path $corpusRoot "HISTORY"
    if (Test-Path $historyDir -PathType Container) {
        $corpusDocs += @(Get-ChildItem -Path $historyDir -Filter *.md -File)
    }
}

$scanFiles = @($files) + @($patchDocs) + @($corpusDocs)

# ---------------------------------------------------------------------------
# 0. Bytes, before anything reads these as text
# ---------------------------------------------------------------------------
#
# A NUL byte in a tracked Markdown file makes `grep` call it binary and skip
# it, makes a diff unreadable, and hides whatever is around it from every text
# tool including this one. It got in on 2026-08-22 by way of a backslash-x-0-0
# escape written into a Python string that then interpreted the escape, in a
# sentence quoting a tracker's error message. This is one line to check and it
# is checked first, because everything below reads these files as text.
#
# `gates.ps1` has a `text` gate over the whole tracked tree, so these files are
# covered twice. Deliberately: this script is the mechanical half of the two
# reviews and gets run on its own, and a review that reads a file `grep` would
# have skipped is the review this is meant to catch.

foreach ($file in $scanFiles) {
    $bytes = [System.IO.File]::ReadAllBytes($file.FullName)
    $at = [System.Array]::IndexOf($bytes, [byte]0)
    if ($at -ge 0) {
        Problem "nul-byte" "$($file.Name) has a NUL byte at offset $at, so every text tool will treat it as binary"
    }
}

# ---------------------------------------------------------------------------
# Read every entry
# ---------------------------------------------------------------------------

$entries = @{}
foreach ($file in $files) {
    $current = $null
    $lineNo = 0
    foreach ($line in [System.IO.File]::ReadAllLines($file.FullName)) {
        $lineNo++
        if ($line -match '^###\s+(T-\d+)\b') {
            $current = $Matches[1]
            if ($entries.ContainsKey($current)) {
                Problem "duplicate-entry" "$current is defined in both $($entries[$current].file) and $($file.Name)"
            }
            else {
                $entries[$current] = [ordered]@{ file = $file.Name; line = $lineNo; status = $null; priority = $null }
            }
            continue
        }
        if ($current -and $null -eq $entries[$current].status -and $line -match '^Status:\s*(.+)$') {
            $entries[$current].status = $Matches[1].Trim()
        }
        if ($current -and $null -eq $entries[$current].priority -and $line -match '^Priority:\s*(.+)$') {
            $entries[$current].priority = ($Matches[1] -replace '\*', '').Trim()
        }
    }
}

# A status line is prose, not a token. Normalise to the five words the index
# uses, taking the first one that appears: "**done**, with the premise
# corrected below" is done, and "open, blocked" is blocked.
function Normalize([string]$status) {
    if (-not $status) { return $null }
    $plain = ($status -replace '\*', '').Trim().ToLowerInvariant()
    foreach ($word in @('deferred', 'blocked', 'partial', 'done', 'open')) {
        if ($plain -match "(^|\W)$word(\W|$)") { return $word }
    }
    return $plain
}

# ---------------------------------------------------------------------------
# Read the index rows
# ---------------------------------------------------------------------------

$indexPath = Join-Path $todo "INDEX.md"
$indexText = [System.IO.File]::ReadAllText($indexPath)
$rows = @{}
$rowOrder = [System.Collections.ArrayList]::new()
foreach ($line in ($indexText -split "`r?`n")) {
    if ($line -match '^\|\s*\[(T-\d+)\]\(([^)]+)\)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|') {
        $id = $Matches[1]
        if ($rows.ContainsKey($id)) { Problem "duplicate-row" "$id has more than one row in INDEX.md" }
        $rows[$id] = [ordered]@{
            file     = $Matches[2]
            priority = $Matches[3].Trim()
            status   = ($Matches[5] -replace '\*', '').Trim()
        }
        [void]$rowOrder.Add($id)
    }
}

# ---------------------------------------------------------------------------
# 1 and 2: rows and entries agree, and both exist
# ---------------------------------------------------------------------------

foreach ($id in $rows.Keys) {
    if (-not $entries.ContainsKey($id)) {
        Problem "row-without-entry" "$id has a row in INDEX.md and no `### $id` anywhere in TODO/"
        continue
    }
    $entryStatus = Normalize $entries[$id].status
    $rowStatus = $rows[$id].status
    if (-not $entryStatus) {
        Problem "entry-without-status" "$id in $($entries[$id].file) has no Status: line"
        continue
    }
    if ($entryStatus -ne $rowStatus) {
        Problem "status-mismatch" "$id : INDEX.md says '$rowStatus', $($entries[$id].file):$($entries[$id].line) says '$($entries[$id].status)'"
    }
    $linked = $rows[$id].file
    if ($linked -ne $entries[$id].file) {
        Problem "wrong-link" "$id : INDEX.md links to $linked, the entry is in $($entries[$id].file)"
    }
}
foreach ($id in $entries.Keys) {
    if (-not $rows.ContainsKey($id)) {
        Problem "entry-without-row" "$id is defined in $($entries[$id].file):$($entries[$id].line) with no row in INDEX.md"
    }
}

# ---------------------------------------------------------------------------
# 3: the counts
# ---------------------------------------------------------------------------

$byState = @{}
$byPriority = @{}
foreach ($id in $rows.Keys) {
    $state = $rows[$id].status
    $priority = $rows[$id].priority
    if (-not $byState.ContainsKey($state)) { $byState[$state] = 0 }
    $byState[$state]++
    $key = "$priority/$state"
    if (-not $byPriority.ContainsKey($key)) { $byPriority[$key] = 0 }
    $byPriority[$key]++
}
function Count([string]$state) { if ($byState.ContainsKey($state)) { $byState[$state] } else { 0 } }

$total = $rows.Count
if ($indexText -match '(?m)^(\d+) items:\s*(\d+) to work through, and (\d+) deferred') {
    $claimTotal = [int]$Matches[1]
    $claimWork = [int]$Matches[2]
    $claimDeferred = [int]$Matches[3]
    if ($claimTotal -ne $total) { Problem "count-prose" "the prose says $claimTotal items, the rows say $total" }
    if ($claimDeferred -ne (Count 'deferred')) { Problem "count-prose" "the prose says $claimDeferred deferred, the rows say $(Count 'deferred')" }
    if ($claimWork -ne ($total - (Count 'deferred'))) { Problem "count-prose" "the prose says $claimWork to work through, the rows say $($total - (Count 'deferred'))" }
}
else { Problem "count-prose" "INDEX.md has no '<N> items: <N> to work through' line to check" }

if ($indexText -match '(?m)^(\d+) open, (\d+) partial, (\d+) blocked, (\d+) done\.') {
    $claimed = @{ open = [int]$Matches[1]; partial = [int]$Matches[2]; blocked = [int]$Matches[3]; done = [int]$Matches[4] }
    foreach ($state in $claimed.Keys) {
        if ($claimed[$state] -ne (Count $state)) {
            Problem "count-prose" "the prose says $($claimed[$state]) $state, the rows say $(Count $state)"
        }
    }
}
else { Problem "count-prose" "INDEX.md has no '<N> open, <N> partial, <N> blocked, <N> done.' line to check" }

# The priority table. `| P1 | 3 | 1 | 0 | 47 | 51 |` is open, partial, blocked,
# done, total.
$tableSeen = $false
foreach ($line in ($indexText -split "`r?`n")) {
    if ($line -match '^\|\s*(P[0-3])\s*\|\s*(\d+)\s*\|\s*(\d+)\s*\|\s*(\d+)\s*\|\s*(\d+)\s*\|\s*(\d+)\s*\|') {
        $tableSeen = $true
        $priority = $Matches[1]
        $want = @{
            open    = [int]$Matches[2]
            partial = [int]$Matches[3]
            blocked = [int]$Matches[4]
            done    = [int]$Matches[5]
        }
        $rowTotal = [int]$Matches[6]
        $sum = 0
        foreach ($state in $want.Keys) {
            $actual = if ($byPriority.ContainsKey("$priority/$state")) { $byPriority["$priority/$state"] } else { 0 }
            if ($want[$state] -ne $actual) {
                Problem "count-table" "$priority $state : the table says $($want[$state]), the rows say $actual"
            }
            $sum += $want[$state]
        }
        if ($sum -ne $rowTotal) { Problem "count-table" "$priority : the row sums to $sum and its total column says $rowTotal" }
    }
    elseif ($line -match '^\|\s*\*\*All\*\*\s*\|\s*\*\*(\d+)\*\*\s*\|\s*\*\*(\d+)\*\*\s*\|\s*\*\*(\d+)\*\*\s*\|\s*\*\*(\d+)\*\*\s*\|\s*\*\*(\d+)\*\*\s*\|') {
        $tableSeen = $true
        $want = @{ open = [int]$Matches[1]; partial = [int]$Matches[2]; blocked = [int]$Matches[3]; done = [int]$Matches[4] }
        foreach ($state in $want.Keys) {
            if ($want[$state] -ne (Count $state)) {
                Problem "count-table" "the All row says $($want[$state]) $state, the rows say $(Count $state)"
            }
        }
        if ([int]$Matches[5] -ne $total) { Problem "count-table" "the All row totals $($Matches[5]), the rows say $total" }
    }
}
if (-not $tableSeen) { Problem "count-table" "INDEX.md has no priority table to check" }

# ---------------------------------------------------------------------------
# 4, 5 and 6: references resolve
# ---------------------------------------------------------------------------

$known = [System.Collections.Generic.HashSet[string]]::new()
foreach ($id in $entries.Keys) { [void]$known.Add($id) }

# reference/ is gitignored and lives on the `references` branch, so a clone
# that has not fetched it cannot check a corpus citation. Absent is not a
# failure; it is one fewer thing this run can say.
$corpus = Join-Path $repo "reference"
$corpusPresent = Test-Path $corpus -PathType Container

# Every Rust file under crates/, indexed by bare name, so a citation written
# short resolves too. `cli.rs:2103` is how most of TODO/ cites this tree and
# the long-form check below never saw one: five citations had drifted 84 to
# 334 lines and every review passed them. A name two files share resolves to
# nothing on purpose, because guessing which one was meant is worse than
# saying nothing.
$byName = @{}
foreach ($rs in Get-ChildItem -Path (Join-Path $repo "crates") -Filter *.rs -Recurse -File) {
    if ($byName.ContainsKey($rs.Name)) { $byName[$rs.Name] = $null; continue }
    $byName[$rs.Name] = $rs.FullName
}

# The vendored trees are cited the same short way, and `patches/TASKS.md` names
# a seam as `tracker_comms.rs:293`. They go in a second index rather than the
# first, consulted only when `crates/` has no file of that name, so adding them
# cannot take coverage away from a citation into this repository's own source.
# Within the second index a shared name still resolves to nothing: `mod.rs`
# exists forty times over and guessing which was meant is worse than silence.
$byVendorName = @{}
$vendorRoot = Join-Path $repo "vendor"
if (Test-Path $vendorRoot) {
    foreach ($rs in Get-ChildItem -Path $vendorRoot -Filter *.rs -Recurse -File) {
        if ($byVendorName.ContainsKey($rs.Name)) { $byVendorName[$rs.Name] = $null; continue }
        $byVendorName[$rs.Name] = $rs.FullName
    }
}
function Resolve-ShortName([string]$name) {
    if ($byName.ContainsKey($name)) { return $byName[$name] }
    if ($byVendorName.ContainsKey($name)) { return $byVendorName[$name] }
    return ""
}

# Read once per file rather than once per citation.
$lineCache = @{}
function Get-Lines([string]$path) {
    if (-not $lineCache.ContainsKey($path)) {
        $lineCache[$path] = @([System.IO.File]::ReadAllLines($path))
    }
    $lineCache[$path]
}

# Whether a citation's line still holds the symbol the prose names beside it.
#
# Only a name that occurs exactly once in the file is judged: a name the file
# uses twice cannot say which occurrence was meant, and a wrong complaint about
# a citation is worse than a missing one. `$Cursor` is allowed a few lines of
# slack, because a citation often names the doc comment above a function.
function Test-Citation([string]$Path, [string]$Cited, [int]$Cursor, [string]$Prose, [string]$Where) {
    $lines = Get-Lines $Path
    foreach ($m in [regex]::Matches($Prose, '`(?<s>[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)`')) {
        $name = ($m.Groups['s'].Value -split '::')[-1]
        # A snake_case name of some length is a function or a field. Anything
        # shorter is as often an English word in backticks.
        if ($name.Length -lt 10 -or $name -notmatch '_') { continue }
        $pattern = "(?<![A-Za-z0-9_])" + [regex]::Escape($name) + "(?![A-Za-z0-9_])"
        $hits = @()
        for ($i = 0; $i -lt $lines.Count; $i++) {
            if ($lines[$i] -cmatch $pattern) { $hits += ($i + 1) }
        }
        if ($hits.Count -ne 1) { continue }
        if ([math]::Abs($hits[0] - $Cursor) -le 4) { continue }
        Problem "drifted-line" "$Where cites ${Cited}:$Cursor for ``$name``, which is at :$($hits[0])"
    }
}

foreach ($file in $scanFiles) {
    # A document under `reference/` cites somebody else's tree, so a path in it
    # is read differently below.
    $isCorpusDoc = $file.FullName.StartsWith((Join-Path $repo "reference"), [System.StringComparison]::OrdinalIgnoreCase)
    $text = [System.IO.File]::ReadAllText($file.FullName)
    $lineNo = 0
    # Inside a fenced block the text is quoted output, a command, or a
    # transcript. A path there is still checked, because a command a reader is
    # told to run has to exist. A drifted line is not: an entry that records
    # what a citation used to say, or quotes a checker naming a stale one, is
    # evidence and has to keep the number it was wrong at. T-193 is the worked
    # example and it reported itself seven times before this.
    $fenced = $false
    foreach ($line in ($text -split "`r?`n")) {
        $lineNo++
        if ($line -match '^\s*```') { $fenced = -not $fenced }
        # A T-NNN that names no entry. Anchors and links both.
        foreach ($m in [regex]::Matches($line, '\bT-(\d{3})\b')) {
            $id = "T-$($m.Groups[1].Value)"
            if (-not $known.Contains($id)) {
                Problem "unknown-entry" "$($file.Name):$lineNo references $id, which is not an entry"
            }
        }
        # A markdown link to another document, resolved against the file that
        # carries it. `patches/TASKS.md` links to `../TODO/peers.md` and
        # `TODO/INDEX.md` links to `peers.md`, and both have to resolve.
        foreach ($m in [regex]::Matches($line, '\]\((?<t>(?!https?:)[A-Za-z0-9._/-]+\.md)(?:#[^)]*)?\)')) {
            $target = $m.Groups['t'].Value
            if (-not (Test-Path (Join-Path $file.DirectoryName $target))) {
                Problem "dead-link" "$($file.Name):$lineNo links to $target, which does not resolve from $($file.Directory.Name)/"
            }
        }
        # A citation into this tree, as `crates/a/b.rs:123`. The lookbehind is
        # load-bearing: without it `TorrentNG/crates/rt-storage/src/x.rs` from
        # the corpus matches from `crates/` and is reported as a path this
        # repository does not have, which is true and not the question. It
        # excludes `/` and word characters and **not** `.`, because `.github`
        # starts with one; a corpus path ending in a directory called
        # `.github` would be a false positive nobody has ever written.
        #
        # `.github` was not in this list until 2026-08-23, so every citation
        # into a workflow resolved to nothing at all. T-161 named four lines of
        # `.github/workflows/ci.yml` and none was checked.
        foreach ($m in [regex]::Matches($line, '(?<![\w/-])(?<p>(?:crates|scripts|docs|vendor|patches|man|\.github)/[A-Za-z0-9._/-]+\.(?:rs|ps1|md|toml|json|jq|yml|patch))(?::(?<l>\d+))?')) {
            $cited = $m.Groups['p'].Value
            # A corpus document cites somebody else's tree, and `crates/` and
            # `docs/` are directory names inside those trees as well as in this
            # one. `crates/rt-utp/src/x.rs` is TorrentNG's and resolving it
            # here reports twenty paths this repository was never supposed to
            # have. So in a corpus document a path is only this tree's when its
            # first two components name a real directory here.
            if ($isCorpusDoc) {
                $head = ($cited -split '/')[0..1] -join '/'
                if (-not (Test-Path (Join-Path $repo $head) -PathType Container)) { continue }
            }
            # A path written with an ellipsis is deliberately abbreviated and
            # there is nothing to resolve.
            if ($cited -match '\.\.\.') { continue }
            $path = Join-Path $repo $cited
            if (-not (Test-Path $path)) {
                Problem "dead-path" "$($file.Name):$lineNo cites $cited, which is not there"
                continue
            }
            if ($m.Groups['l'].Success) {
                $count = (Get-Lines $path).Count
                if ([int]$m.Groups['l'].Value -gt $count) {
                    Problem "dead-line" "$($file.Name):$lineNo cites ${cited}:$($m.Groups['l'].Value) and that file has $count lines"
                } elseif (-not $fenced) {
                    Test-Citation $path $cited ([int]$m.Groups['l'].Value) $line "$($file.Name):$lineNo"
                }
            }
        }
        # The same citation written short, as `cli.rs:2103`. Resolved through
        # the bare-name index above, and skipped when the name is not unique.
        foreach ($m in [regex]::Matches($line, '(?<![\w./-])(?<p>[a-z0-9_]+\.rs):(?<l>\d+)')) {
            $short = $m.Groups['p'].Value
            $path = Resolve-ShortName $short
            if ([string]::IsNullOrEmpty($path)) { continue }
            $lines = Get-Lines $path
            if ([int]$m.Groups['l'].Value -gt $lines.Count) {
                Problem "dead-line" "$($file.Name):$lineNo cites ${short}:$($m.Groups['l'].Value) and that file has $($lines.Count) lines"
                continue
            }
            if (-not $fenced) {
                Test-Citation $path $short ([int]$m.Groups['l'].Value) $line "$($file.Name):$lineNo"
            }
        }
        # A citation into the corpus, as `TorrentNG/crates/a/b.rs:123`. Only
        # checkable when reference/ is on this machine, which is the case
        # TODO/RULES.md section 7 asks for: verify a path before citing it,
        # one Test-Path is the whole check.
        if ($corpusPresent) {
            # The path part must itself contain a directory. `torrent/x.rs`
            # is far more often this tree's `crates/bit-cli-core/src/torrent/`
            # written short than it is the corpus tree called `torrent`, and a
            # checker that cries wolf is a checker nobody runs.
            foreach ($m in [regex]::Matches($line, '(?<![\w./-])(?<r>[A-Za-z0-9_-]+)/(?<p>[A-Za-z0-9._-]+/[A-Za-z0-9._/-]+\.(?:rst|json|toml|patch|go|js|md|py|rs|ts))(?![A-Za-z0-9])(?::(?<l>\d+))?')) {
                $tree = $m.Groups['r'].Value
                $treeRoot = Join-Path $corpus $tree
                if (-not (Test-Path $treeRoot -PathType Container)) { continue }
                $cited = "$tree/$($m.Groups['p'].Value)"
                if ($cited -match '\.\.\.') { continue }
                $path = Join-Path $corpus $cited
                if (-not (Test-Path $path)) {
                    Problem "dead-corpus-path" "$($file.Name):$lineNo cites $cited, which is not in reference/"
                    continue
                }
                if ($m.Groups['l'].Success) {
                    # `Measure-Object -Line` does not count blank lines, an
                    # undercount of eight to ten percent that reads like a
                    # precise figure. It reported `herp_test.go` as 77 lines
                    # when it has 86, and called a correct citation at :80
                    # dead. Count the array instead.
                    $count = @(Get-Content -LiteralPath $path).Count
                    if ([int]$m.Groups['l'].Value -gt $count) {
                        Problem "dead-corpus-line" "$($file.Name):$lineNo cites ${cited}:$($m.Groups['l'].Value) and that file has $count lines"
                    }
                }
            }
        }
    }
}

# ---------------------------------------------------------------------------
# 8: patches/TASKS.md, the ordered list of vendored work
# ---------------------------------------------------------------------------
#
# Its table is a second copy of a status that lives in the entry, which is the
# shape that goes stale. It went stale for a whole session on 2026-08-22: both
# P0 rows said `open` while both entries said `done`, because the file was
# rewritten after the last push. Nothing compared them, so nothing said so.
#
# The row is `| [T-020](../TODO/peers.md) | **P0** | **done** | why |`.

$tasksPath = Join-Path $repo "patches/TASKS.md"
if (Test-Path $tasksPath) {
    $tasksText = [System.IO.File]::ReadAllText($tasksPath)
    $taskRows = [ordered]@{}
    foreach ($line in ($tasksText -split "`r?`n")) {
        if ($line -notmatch '^\|\s*\[(T-\d+)\]\(([^)]+)\)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|') { continue }
        $id = $Matches[1]
        if ($taskRows.Contains($id)) {
            Problem "tasks-duplicate-row" "$id has more than one row in patches/TASKS.md"
            continue
        }
        $taskRows[$id] = [ordered]@{
            link     = $Matches[2]
            priority = ($Matches[3] -replace '\*', '').Trim()
            status   = Normalize ($Matches[4])
        }
    }
    if ($taskRows.Count -eq 0) {
        Problem "tasks-table" "patches/TASKS.md has no table of entries to check"
    }

    foreach ($id in $taskRows.Keys) {
        # An id that names nothing is already reported by the reference pass.
        if (-not $entries.ContainsKey($id)) { continue }
        $row = $taskRows[$id]
        $entryStatus = Normalize $entries[$id].status
        if ($row.status -ne $entryStatus) {
            Problem "tasks-status-mismatch" "$id : patches/TASKS.md says '$($row.status)', $($entries[$id].file):$($entries[$id].line) says '$($entries[$id].status)'"
        }
        if ($entries[$id].priority -and $row.priority -ne $entries[$id].priority) {
            Problem "tasks-priority-mismatch" "$id : patches/TASKS.md says '$($row.priority)', $($entries[$id].file) says '$($entries[$id].priority)'"
        }
        $wantLink = "../TODO/$($entries[$id].file)"
        if ($row.link -ne $wantLink) {
            Problem "tasks-wrong-link" "$id : patches/TASKS.md links to $($row.link), the entry is in $wantLink"
        }
    }

    # The counting sentence over the table. This is the number two documents
    # disagree about when one of them is edited and the other is not.
    $taskStates = @{}
    foreach ($id in $taskRows.Keys) {
        $state = $taskRows[$id].status
        if (-not $taskStates.ContainsKey($state)) { $taskStates[$state] = 0 }
        $taskStates[$state]++
    }
    $taskCount = {
        param([string]$state)
        if ($taskStates.ContainsKey($state)) { $taskStates[$state] } else { 0 }
    }
    if ($tasksText -match '\*\*(\d+) entries: (\d+) done, (\d+) partial, (\d+) blocked, (\d+) open\.\*\*') {
        $claim = @{
            total   = [int]$Matches[1]
            done    = [int]$Matches[2]
            partial = [int]$Matches[3]
            blocked = [int]$Matches[4]
            open    = [int]$Matches[5]
        }
        if ($claim.total -ne $taskRows.Count) {
            Problem "tasks-count" "patches/TASKS.md says $($claim.total) entries, its table has $($taskRows.Count) rows"
        }
        foreach ($state in @('done', 'partial', 'blocked', 'open')) {
            $actual = & $taskCount $state
            if ($claim[$state] -ne $actual) {
                Problem "tasks-count" "patches/TASKS.md says $($claim[$state]) $state, its table says $actual"
            }
        }
    }
    else {
        Problem "tasks-count" "patches/TASKS.md has no '**<N> entries: <N> done, <N> partial, <N> blocked, <N> open.**' line to check"
    }

    # Every entry the table still calls unfinished has to be argued for
    # somewhere below it, or the table is a list of work with no work in it.
    foreach ($id in $taskRows.Keys) {
        if ($taskRows[$id].status -eq 'done') { continue }
        $mentions = ([regex]::Matches($tasksText, "(?<![\w-])$id(?![\w-])")).Count
        if ($mentions -lt 2) {
            Problem "tasks-unargued" "$id is in patches/TASKS.md's table as '$($taskRows[$id].status)' and appears nowhere else in the file"
        }
    }
}
else { Problem "tasks-table" "patches/TASKS.md is not there" }

# ---------------------------------------------------------------------------
# 8a: an open entry that names a workflow action nothing pins
# ---------------------------------------------------------------------------
#
# T-161 stayed open for a session after it was fixed, and nothing here noticed.
# Its Problem was that four jobs pinned `ilammy/setup-nasm@v1.5.2`, which was
# replaced by `scripts/setup-nasm.ps1` at every call site. The entry went on
# describing a workflow the tree does not have.
#
# Nothing mechanical can decide in general whether an entry describes a state
# the tree is in. This is the one shape that can be decided: an action pin is
# spelled `owner/name@ref` and nothing else in this record is, so a **backticked
# or fenced** pin in an **open or partial** entry that no workflow carries is an
# entry whose premise has moved.
#
# Restricted to open and partial deliberately. A closed entry quoting the pin it
# removed is evidence and has to keep it, which is the same rule the drifted-line
# check follows for a fenced citation.
#
# `bit-cli` is excluded because `Azathothas/bit-cli@v1` is this repository and
# not an action.

$workflowDir = Join-Path $repo ".github/workflows"
if (Test-Path $workflowDir) {
    # From `uses:` lines only, never from the raw text. `ci.yml` carries the
    # comment "Ours, not ilammy/setup-nasm: that action is unmaintained", so a
    # substring search over the file finds the very action the comment exists
    # to say is gone. That is how the first draft of this check passed T-161.
    $pinned = New-Object System.Collections.Generic.HashSet[string]
    foreach ($wf in Get-ChildItem -Path $workflowDir -Filter *.yml -File) {
        foreach ($wfLine in [System.IO.File]::ReadAllLines($wf.FullName)) {
            if ($wfLine -match '^\s*(?:-\s*)?uses:\s*(?<a>[^@\s]+)') {
                [void]$pinned.Add($Matches['a'].Trim())
            }
        }
    }
    foreach ($file in $files) {
        $current = $null
        $lineNo = 0
        foreach ($line in [System.IO.File]::ReadAllLines($file.FullName)) {
            $lineNo++
            if ($line -match '^###\s+(T-\d+)\b') { $current = $Matches[1]; continue }
            if (-not $current) { continue }
            $state = Normalize $entries[$current].status
            if ($state -ne 'open' -and $state -ne 'partial') { continue }
            foreach ($m in [regex]::Matches($line, '(?<![\w./-])(?<a>[A-Za-z0-9][A-Za-z0-9._-]*/[A-Za-z0-9][A-Za-z0-9._-]*)@(?<v>v?[0-9][A-Za-z0-9._-]*)')) {
                $action = $m.Groups['a'].Value
                if ($action -match '(?i)/bit-cli$') { continue }
                if ($pinned.Contains($action)) { continue }
                Problem "stale-premise" ("$($file.Name):$lineNo : $current is $state and names the action " +
                    "``$action@$($m.Groups['v'].Value)``, which no workflow uses. Either the entry is done " +
                    "or its premise moved.")
            }
        }
    }
}

# ---------------------------------------------------------------------------
# 8b: the pinned toolchain, and the jobs allowed to float off it
# ---------------------------------------------------------------------------
#
# T-150. `RUSTFLAGS: -D warnings` is set for the whole CI workflow, so every
# job that compiles is a lint gate, and a job that installs `stable` is a gate
# that moves on its own: a commit green when it was written goes red six weeks
# later with nobody having touched it. `RUST_GATE` is the named version they
# all take instead.
#
# Two things can quietly undo that and both are mechanical, so neither is left
# to a review:
#
#   1. Two workflows naming different versions. The pin lives in `ci.yml` and
#      in `release.yml` because a workflow cannot read another one's `env`, and
#      a number written twice is a number two files disagree about.
#   2. A new job pinning `stable` again. That is one line in a pull request and
#      it looks exactly like every other job.
#
# A floating toolchain is allowed in a job that carries `continue-on-error:
# true`, which is what `Clippy (tracking ...)` is: it reports what the next
# release will want and blocks nothing.

if (Test-Path $workflowDir) {
    $gates = @{}
    foreach ($wf in Get-ChildItem -Path $workflowDir -Filter *.yml -File) {
        $lines = [System.IO.File]::ReadAllLines($wf.FullName)

        for ($i = 0; $i -lt $lines.Count; $i++) {
            if ($lines[$i] -match '^\s*RUST_GATE:\s*"?(?<v>[0-9][A-Za-z0-9._-]*)"?\s*$') {
                $gates[$wf.Name] = $Matches['v']
            }
        }

        # Which jobs may float. A job starts at two spaces of indent and runs
        # until the next one, so `continue-on-error` is read per job rather
        # than per file: one exempt job must not exempt the file.
        $jobName = $null
        $floats = @{}
        $jobOf = @{}
        for ($i = 0; $i -lt $lines.Count; $i++) {
            $line = $lines[$i]
            if ($line -match '^  (?<j>[A-Za-z0-9_-]+):\s*$') {
                $jobName = $Matches['j']
                $floats[$jobName] = $false
                continue
            }
            if (-not $jobName) { continue }
            if ($line -match '^\s*continue-on-error:\s*true\s*$') { $floats[$jobName] = $true }
            if ($line -match '^\s*toolchain:\s*(?<t>\S+)') {
                $value = $Matches['t'].Trim('"', "'")
                if ($value -in @('stable', 'beta', 'nightly')) {
                    $jobOf["$($wf.Name):$($i + 1)"] = @{ job = $jobName; toolchain = $value }
                }
            }
        }
        foreach ($where in $jobOf.Keys) {
            $hit = $jobOf[$where]
            if (-not $floats[$hit.job]) {
                Problem "toolchain-pin" ("$where : job ``$($hit.job)`` installs ``$($hit.toolchain)``, which moves " +
                    "on its own, and does not carry ``continue-on-error: true``. Take the RUST_GATE pin " +
                    "or mark the job as not blocking. See TODO/cli-surface.md, T-150.")
            }
        }
    }

    $distinct = @($gates.Values | Sort-Object -Unique)
    if ($distinct.Count -gt 1) {
        $named = ($gates.Keys | Sort-Object | ForEach-Object { "$_ says $($gates[$_])" }) -join ', '
        Problem "toolchain-pin" "the workflows disagree about RUST_GATE: $named"
    }
}

# ---------------------------------------------------------------------------
# 9: TODO/PROGRESS.md, the only file the next session is told to read
# ---------------------------------------------------------------------------
#
# RULES.md section 2 step 2 lists what it must carry. A missing heading is a
# session that ends without saying where to resume; a stale count is a number
# the next session then quotes as measured. Both are mechanical.

$progressPath = Join-Path $todo "PROGRESS.md"
$progressText = [System.IO.File]::ReadAllText($progressPath)

foreach ($heading in @('## State', '## What the last session did', '## In progress', '## Start here next session', '## Open questions for the operator')) {
    if ($progressText -notmatch ("(?m)^" + [regex]::Escape($heading) + "\s*$")) {
        Problem "progress-shape" "PROGRESS.md has no '$heading' section, which RULES.md section 2 step 2 requires"
    }
}

if ($progressText -notmatch '\*\*Last session:\*\*\s*(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z)') {
    Problem "progress-shape" "PROGRESS.md's state line carries no start instant in ISO 8601 UTC, and every end-of-session measurement is taken from it"
}
if ($progressText -notmatch '\*\*Tests:\*\*\s*[\d,]+ passing') {
    Problem "progress-shape" "PROGRESS.md carries no '**Tests:** <N> passing' baseline"
}
if ($progressText -notmatch '\*\*CI:\*\*[\s\S]{0,400}?run \*\*(\d+)\*\*') {
    Problem "progress-shape" "PROGRESS.md's CI line does not name a run by id, and 'the latest' is not a run id"
}

# The counts, against the rows rather than against the last session's memory.
$workable = $total - (Count 'deferred')
if ($progressText -match '\*\*Entries:\*\*\s*(\d+) items\. (\d+) open, (\d+) partial, (\d+) blocked, (\d+) done, (\d+) deferred') {
    $claim = @{
        total    = [int]$Matches[1]
        open     = [int]$Matches[2]
        partial  = [int]$Matches[3]
        blocked  = [int]$Matches[4]
        done     = [int]$Matches[5]
        deferred = [int]$Matches[6]
    }
    if ($claim.total -ne $total) { Problem "progress-count" "PROGRESS.md says $($claim.total) items, INDEX.md has $total rows" }
    foreach ($state in @('open', 'partial', 'blocked', 'done', 'deferred')) {
        if ($claim[$state] -ne (Count $state)) {
            Problem "progress-count" "PROGRESS.md says $($claim[$state]) $state, the rows say $(Count $state)"
        }
    }
}
else { Problem "progress-count" "PROGRESS.md has no '**Entries:** <N> items. <N> open, ...' line to check" }

if ($progressText -match '(\d+) of (\d+) workable done, (\d+) left') {
    $doneClaim = [int]$Matches[1]
    $workClaim = [int]$Matches[2]
    $leftClaim = [int]$Matches[3]
    if ($doneClaim -ne (Count 'done')) { Problem "progress-count" "PROGRESS.md says $doneClaim workable done, the rows say $(Count 'done')" }
    if ($workClaim -ne $workable) { Problem "progress-count" "PROGRESS.md says $workClaim workable, the rows say $workable" }
    if ($leftClaim -ne ($workable - (Count 'done'))) { Problem "progress-count" "PROGRESS.md says $leftClaim left, the rows say $($workable - (Count 'done'))" }
}
else { Problem "progress-count" "PROGRESS.md has no '<N> of <N> workable done, <N> left' line to check" }

# The patch count, which vendor-status.ps1 prints and PROGRESS.md quotes back.
$patchDir = Join-Path $repo "patches"
$patchCount = @(Get-ChildItem -Path $patchDir -Filter *.patch -Recurse -File -ErrorAction SilentlyContinue).Count
if ($progressText -match '\*\*(\d+) patches\*\*') {
    if ([int]$Matches[1] -ne $patchCount) {
        Problem "progress-count" "PROGRESS.md says $($Matches[1]) patches, patches/ holds $patchCount"
    }
}
elseif ($patchCount -gt 0) {
    Problem "progress-count" "patches/ holds $patchCount patch(es) and PROGRESS.md's vendored line names no count"
}

# ---------------------------------------------------------------------------
# Say it
# ---------------------------------------------------------------------------

if ($Json) {
    [ordered]@{
        kind           = "check-todo"
        schema_version = "1"
        generated_at   = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
        entries        = $entries.Count
        rows           = $rows.Count
        by_state       = $byState
        ok             = ($problems.Count -eq 0)
        problems       = @($problems)
    } | ConvertTo-Json -Depth 6
    exit $(if ($problems.Count -eq 0) { 0 } else { 1 })
}

Write-Host ""
Write-Host ("check-todo: {0} entries, {1} rows" -f $entries.Count, $rows.Count)
$order = @('open', 'partial', 'blocked', 'done', 'deferred')
Write-Host ("  states: " + (($order | Where-Object { $byState.ContainsKey($_) } | ForEach-Object { "$_ $($byState[$_])" }) -join ', '))
if ($problems.Count -eq 0) {
    Write-Host "  everything agrees"
    Write-Host ""
    exit 0
}
Write-Host ""
Write-Host "$($problems.Count) problem(s):"
foreach ($item in $problems) {
    Write-Host ("  [{0}] {1}" -f $item.kind, $item.detail)
}
Write-Host ""
exit 1
