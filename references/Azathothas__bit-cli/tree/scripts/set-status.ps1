# Move one entry's status and make every count agree with the rows again.
#
# `scripts/check-todo.ps1` is the reader: it fails a gate when INDEX.md's counts
# disagree with INDEX.md's rows, or when PROGRESS.md quotes a count that no
# longer holds. This is the writer for the same numbers.
#
# The defect it exists to catch is arithmetic done by hand at the end of a
# session. Closing one entry moves seven numbers: the prose total line, the
# open and done figures beside it, one row of the priority table, its total
# column, the All row, and PROGRESS.md's two count lines. Every session has had
# to change all of them together, and a session that gets one wrong fails the
# `record` gate at the push, after the work is done and the message is written.
#
# It writes nothing that check-todo.ps1 does not then verify, which is the point
# of having both: this derives the numbers from the rows, and that one asserts
# them against the rows independently. Run both.
#
# Usage:
#   pwsh scripts/set-status.ps1 -Entry T-232 -Status done
#   pwsh scripts/set-status.ps1 -Entry T-251 -Status partial -Item "New text *(what changed)*"
#   pwsh scripts/set-status.ps1 -Recount
#   pwsh scripts/set-status.ps1 -Recount -Check
#
# `done` is written as `**done**`, which is the emphasis INDEX.md uses for an
# entry closed by this project's own work. Pass `-Plain` for the unemphasised
# form the imported entries carry.
#
# What it does NOT do: the entry's own `Status:` line, in TODO/<category>.md.
# That line sits in prose a session is writing anyway, often with a closing
# date and a clause beside it, and rewriting prose from a script is how a
# closing sentence loses its meaning. It prints what that line currently says
# so the disagreement is visible here rather than at the gate.
#
# Exits 0 when it wrote or when -Check found nothing, 1 when -Check found a
# count that disagrees, and 2 when it could not run.
#
# See TODO/RULES.md section 4a.

[CmdletBinding()]
param(
    # The entry to move, as T-NNN. Omit with -Recount to re-derive the counts
    # without touching a row.
    [string]$Entry,
    [ValidateSet("open", "partial", "blocked", "done", "deferred")]
    [string]$Status,
    # Replace the row's item text. The entry's title, not its status.
    [string]$Item,
    # Re-derive every count from the rows and change nothing else.
    [switch]$Recount,
    # Report what would change and write nothing.
    [switch]$Check,
    # Write `done` rather than `**done**`.
    [switch]$Plain,
    [switch]$Json
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$indexPath = Join-Path $repo "TODO/INDEX.md"
$progressPath = Join-Path $repo "TODO/PROGRESS.md"

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("set-status: $message")
    exit $code
}

if (-not $Entry -and -not $Recount) {
    Exit-With 2 "pass -Entry T-NNN with -Status, or -Recount on its own."
}
if ($Entry -and -not $Status -and -not $Item) {
    Exit-With 2 "-Entry needs -Status, -Item, or both."
}
if ($Entry -notmatch '^(T-\d{3})?$') { Exit-With 2 "-Entry is a T-NNN, not '$Entry'." }
foreach ($required in @($indexPath, $progressPath)) {
    if (-not (Test-Path $required)) { Exit-With 2 "missing $required" }
}

# Read and write as UTF-8 with no BOM and LF endings, which is what every file
# under TODO/ is and what scripts/check-tree.ps1 asserts. Set-Content would
# write CRLF on Windows and turn a one-line change into a whole-file diff.
function Read-Text([string]$path) { [System.IO.File]::ReadAllText($path) }
function Write-Text([string]$path, [string]$text) {
    [System.IO.File]::WriteAllText($path, ($text -replace "`r`n", "`n"),
        [System.Text.UTF8Encoding]::new($false))
}

$index = Read-Text $indexPath
$changes = [System.Collections.ArrayList]::new()

# ---------------------------------------------------------------------------
# The row
# ---------------------------------------------------------------------------
#
# A row is `| [T-NNN](file.md) | P1 | memory | **done** | Item text |`. The
# status and the item are the fourth and fifth cells, and the first three are
# left alone: a priority or a category move is a judgement, and this script
# does arithmetic.

if ($Entry) {
    $rowPattern = "(?m)^(\| \[" + [regex]::Escape($Entry) + "\]\([^)]+\) \| [^|]*\| [^|]*\| )([^|]*?)( \| )([^|]*?)( \|)$"
    $match = [regex]::Match($index, $rowPattern)
    if (-not $match.Success) { Exit-With 2 "no INDEX.md row for $Entry" }

    $wasStatus = $match.Groups[2].Value.Trim()
    $wasItem = $match.Groups[4].Value.Trim()
    $nowStatus = $wasStatus
    if ($Status) {
        $nowStatus = if ($Status -eq "done" -and -not $Plain) { "**done**" } else { $Status }
    }
    $nowItem = if ($PSBoundParameters.ContainsKey("Item")) { $Item } else { $wasItem }

    if ($nowStatus -ne $wasStatus) { [void]$changes.Add("$Entry status $wasStatus -> $nowStatus") }
    if ($nowItem -ne $wasItem) { [void]$changes.Add("$Entry item rewritten") }
    $index = $index.Remove($match.Index, $match.Length).Insert(
        $match.Index, ($match.Groups[1].Value + $nowStatus + $match.Groups[3].Value + $nowItem + $match.Groups[5].Value))

    # And what the entry itself says, which this script does not write. A
    # disagreement here is what check-todo.ps1 fails on, so it is printed at
    # the moment somebody can still fix it in the same edit.
    $file = [regex]::Match($match.Groups[1].Value, '\]\(([^)]+)\)').Groups[1].Value
    $entryPath = Join-Path (Join-Path $repo "TODO") $file
    if (Test-Path $entryPath) {
        $entryText = Read-Text $entryPath
        $head = [regex]::Match($entryText, "(?m)^### " + [regex]::Escape($Entry) + " .*$")
        if ($head.Success) {
            $after = $entryText.Substring($head.Index)
            $line = [regex]::Match($after, "(?m)^Status:\s*(.+)$")
            if ($line.Success) {
                $said = $line.Groups[1].Value.Trim()
                $agrees = ($said -replace '\*', '') -match ("^" + [regex]::Escape(($nowStatus -replace '\*', '')))
                $note = if ($agrees) { "agrees" } else { "DISAGREES, edit it" }
                [void]$changes.Add("TODO/$file says 'Status: $said' ($note)")
            }
        }
    }
}

# ---------------------------------------------------------------------------
# The counts, every one of them derived from the rows
# ---------------------------------------------------------------------------

$rows = [regex]::Matches($index, '(?m)^\| \[(T-\d+)\]\([^)]+\) \| ([^|]*?)\s*\| [^|]*\| ([^|]*?)\s*\|')
if ($rows.Count -eq 0) { Exit-With 2 "parsed no rows out of INDEX.md" }

$states = @("open", "partial", "blocked", "done", "deferred")
$counts = @{}
foreach ($state in $states) { $counts[$state] = 0 }
$byPriority = @{}
foreach ($row in $rows) {
    $state = ($row.Groups[3].Value -replace '\*', '').Trim()
    if ($state -notin $states) { Exit-With 2 "row $($row.Groups[1].Value) has an unknown status '$state'" }
    $counts[$state]++
    $priority = $row.Groups[2].Value.Trim()
    if ($priority -eq "n/a") { $priority = "Phase C" }
    if (-not $byPriority.ContainsKey($priority)) {
        $byPriority[$priority] = @{}
        foreach ($state in $states) { $byPriority[$priority][$state] = 0 }
    }
    $byPriority[$priority][($row.Groups[3].Value -replace '\*', '').Trim()]++
}
$total = $rows.Count
$workable = $total - $counts["deferred"]

# The prose two lines under "## Counts".
$index = [regex]::Replace($index, '(?m)^\d+ items: \d+ to work through, and \d+ deferred to Phase C\.$',
    "$total items: $workable to work through, and $($counts['deferred']) deferred to Phase C.")
$index = [regex]::Replace($index, '(?m)^\d+ open, \d+ partial, \d+ blocked, \d+ done\.$',
    "$($counts['open']) open, $($counts['partial']) partial, $($counts['blocked']) blocked, $($counts['done']) done.")

# The priority table. One row per level that has entries, plus Phase C and All.
foreach ($level in @("P0", "P1", "P2", "P3")) {
    $c = if ($byPriority.ContainsKey($level)) { $byPriority[$level] } else { @{ open = 0; partial = 0; blocked = 0; done = 0 } }
    $levelTotal = $c["open"] + $c["partial"] + $c["blocked"] + $c["done"]
    $pattern = "(?m)^\| $level \| \d+ \| \d+ \| \d+ \| \d+ \| \d+ \|$"
    if (-not [regex]::IsMatch($index, $pattern)) { Exit-With 2 "INDEX.md has no $level row in the priority table" }
    $index = [regex]::Replace($index, $pattern,
        "| $level | $($c['open']) | $($c['partial']) | $($c['blocked']) | $($c['done']) | $levelTotal |")
}
$index = [regex]::Replace($index, '(?m)^\| Phase C \| \| \| \| \d+ deferred \| \d+ \|$',
    "| Phase C | | | | $($counts['deferred']) deferred | $($counts['deferred']) |")
$index = [regex]::Replace($index, '(?m)^\| \*\*All\*\* \| \*\*\d+\*\* \| \*\*\d+\*\* \| \*\*\d+\*\* \| \*\*\d+\*\* \| \*\*\d+\*\* \|$',
    "| **All** | **$($counts['open'])** | **$($counts['partial'])** | **$($counts['blocked'])** | **$($counts['done'])** | **$total** |")

# ---------------------------------------------------------------------------
# PROGRESS.md quotes the same numbers back
# ---------------------------------------------------------------------------
#
# Two fixed lines, in the shapes RULES.md section 5 requires and
# check-todo.ps1 parses. Nothing else in that file is touched: what a session
# did is prose and is the session's to write.

$progress = Read-Text $progressPath
$entriesLine = "- **Entries:** $total items. $($counts['open']) open, $($counts['partial']) partial, " +
"$($counts['blocked']) blocked, $($counts['done']) done, $($counts['deferred']) deferred"
if (-not [regex]::IsMatch($progress, '(?m)^- \*\*Entries:\*\* \d+ items\. \d+ open, \d+ partial, \d+ blocked, \d+ done, \d+ deferred')) {
    Exit-With 2 "PROGRESS.md has no '- **Entries:** <N> items. ...' line to rewrite"
}
$progress = [regex]::Replace($progress,
    '(?m)^- \*\*Entries:\*\* \d+ items\. \d+ open, \d+ partial, \d+ blocked, \d+ done, \d+ deferred',
    $entriesLine)
if (-not [regex]::IsMatch($progress, '\d+ of \d+ workable done, \d+ left')) {
    Exit-With 2 "PROGRESS.md has no '<N> of <N> workable done, <N> left' line to rewrite"
}
$progress = [regex]::Replace($progress, '\d+ of \d+ workable done, \d+ left',
    "$($counts['done']) of $workable workable done, $($workable - $counts['done']) left")

# ---------------------------------------------------------------------------
# Write, or say what would have been written
# ---------------------------------------------------------------------------

$indexWas = Read-Text $indexPath
$progressWas = Read-Text $progressPath
$indexMoved = ($index -ne $indexWas)
$progressMoved = ($progress -ne $progressWas)

$result = [ordered]@{
    kind      = "set_status"
    entry     = $(if ($Entry) { $Entry } else { $null })
    counts    = [ordered]@{
        total    = $total
        workable = $workable
        open     = $counts["open"]
        partial  = $counts["partial"]
        blocked  = $counts["blocked"]
        done     = $counts["done"]
        deferred = $counts["deferred"]
    }
    changes   = @($changes)
    # Always an array, including when it is empty: a consumer that reads
    # `.rewritten.Count` must not be handed a null because nothing moved.
    rewritten = @(@(
            $(if ($indexMoved) { "TODO/INDEX.md" }),
            $(if ($progressMoved) { "TODO/PROGRESS.md" })
        ) | Where-Object { $_ })
    checked   = [bool]$Check
}

if (-not $Check) {
    if ($indexMoved) { Write-Text $indexPath $index }
    if ($progressMoved) { Write-Text $progressPath $progress }
}

if ($Json) { $result | ConvertTo-Json -Depth 5 | Write-Output }
else {
    foreach ($change in $changes) { Write-Host "  $change" }
    $verb = if ($Check) { "would read" } else { "counts" }
    Write-Host "set-status: $verb $total items, $($counts['open']) open, $($counts['partial']) partial, $($counts['blocked']) blocked, $($counts['done']) done, $($counts['deferred']) deferred"
    if ($result.rewritten.Count -eq 0) { Write-Host "set-status: nothing to change" }
    elseif ($Check) { Write-Host "set-status: $($result.rewritten -join ' and ') would be rewritten" }
    else {
        Write-Host "set-status: rewrote $($result.rewritten -join ' and ')"
        Write-Host "set-status: now run  pwsh -NoProfile -File scripts/check-todo.ps1"
    }
}

if ($Check -and $result.rewritten.Count -gt 0) { exit 1 }
exit 0
