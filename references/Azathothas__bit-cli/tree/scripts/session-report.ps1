# What a session did, measured rather than remembered.
#
# Every session ends by writing down what it changed, and every session has so
# far counted it by hand. The numbers are all derivable: git knows what moved,
# `scc` knows how big the tree is, and TODO/INDEX.md knows how many entries
# there are and what state each is in. Deriving them costs one command and
# removes a class of doc that is wrong the moment it is written.
#
# Usage:
#   pwsh -NoProfile -File scripts/session-report.ps1 -Since 2026-08-22T01:11:24Z
#   pwsh -NoProfile -File scripts/session-report.ps1 -Base 76e33e8
#   pwsh -NoProfile -File scripts/session-report.ps1 -Since ... -Json
#
# `-Since` is the ISO 8601 UTC instant the session started, which
# TODO/PROGRESS.md carries on its state line. `-Base` names the commit to
# measure from instead, and wins when both are given.
#
# Exit codes: 0 always, unless the script could not run (2). This reports; it
# does not judge.
#
# See TODO/RULES.md section 2.

[CmdletBinding()]
param(
    [string]$Since,
    [string]$Base,
    [switch]$Json
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("session-report: $message")
    exit $code
}

function Invoke-Git {
    param([string[]]$gitArgs)
    $out = & git -C $repo @gitArgs 2>&1
    if ($LASTEXITCODE -ne 0) { return $null }
    return ($out | Out-String)
}

if (-not (Invoke-Git @("rev-parse", "--git-dir"))) {
    Exit-With 2 "not a git repository: $repo"
}

# ---------------------------------------------------------------------------
# Which commit to measure from
# ---------------------------------------------------------------------------

if (-not $Base) {
    if (-not $Since) { Exit-With 2 "pass -Since <ISO 8601 UTC> or -Base <commit>." }
    $first = (Invoke-Git @("log", "--since=$Since", "--format=%H", "--reverse"))
    $first = ($first -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -First 1)
    if (-not $first) {
        # Nothing committed since then. The base is HEAD and every number below
        # is zero, which is a true answer rather than a missing one.
        $Base = "HEAD"
    }
    else {
        $parent = (Invoke-Git @("rev-parse", "--verify", "--quiet", "$first^"))
        # A first commit with no parent means the session started the history.
        $Base = if ($parent) { $parent.Trim() } else { $first }
    }
}
$Base = (Invoke-Git @("rev-parse", "--short", $Base))
if (-not $Base) { Exit-With 2 "could not resolve the base commit." }
$Base = $Base.Trim()
$head = (Invoke-Git @("rev-parse", "--short", "HEAD")).Trim()

# ---------------------------------------------------------------------------
# What moved
# ---------------------------------------------------------------------------

$commits = @((Invoke-Git @("log", "--format=%h %s", "$Base..HEAD")) -split "`r?`n" |
        Where-Object { $_.Trim() })

$numstat = @((Invoke-Git @("diff", "--numstat", "$Base..HEAD")) -split "`r?`n" |
        Where-Object { $_.Trim() })
$added = 0
$removed = 0
$files = 0
foreach ($line in $numstat) {
    $parts = $line -split "`t"
    if ($parts.Count -lt 3) { continue }
    $files++
    # A binary file shows `-` for both counts.
    if ($parts[0] -ne '-') { $added += [int]$parts[0] }
    if ($parts[1] -ne '-') { $removed += [int]$parts[1] }
}

# ---------------------------------------------------------------------------
# How big the tree is
# ---------------------------------------------------------------------------
#
# `scc` over crates/ only. The whole repository would count TODO/ prose, which
# is a real part of the work and not a line count anybody compares.

$rust = [ordered]@{ files = 0; code = 0; comments = 0 }
if (Get-Command scc -ErrorAction SilentlyContinue) {
    $scc = & scc --no-cocomo --format json (Join-Path $repo "crates") 2>$null
    if ($LASTEXITCODE -eq 0 -and $scc) {
        try {
            foreach ($row in ($scc | ConvertFrom-Json)) {
                if ($row.Name -eq 'Rust') {
                    $rust.files = [int]$row.Count
                    $rust.code = [int]$row.Code
                    $rust.comments = [int]$row.Comment
                }
            }
        }
        catch { }
    }
}

# ---------------------------------------------------------------------------
# What the entry list says
# ---------------------------------------------------------------------------

$rowPattern = '^\|\s*\[(T-\d+)\]\([^)]+\)\s*\|\s*([^|]+?)\s*\|\s*[^|]*\|\s*([^|]+?)\s*\|'

function Read-Entries([string]$text) {
    $states = @{}
    foreach ($line in ($text -split "`r?`n")) {
        if ($line -match $rowPattern) {
            $states[$Matches[1]] = ($Matches[3] -replace '\*', '').Trim()
        }
    }
    return $states
}

$indexPath = Join-Path $repo "TODO/INDEX.md"
if (-not (Test-Path $indexPath)) { Exit-With 2 "TODO/INDEX.md is not there." }
$now = Read-Entries ([System.IO.File]::ReadAllText($indexPath))
$before = Read-Entries ((Invoke-Git @("show", "${Base}:TODO/INDEX.md")) ?? "")

$byState = @{}
foreach ($state in $now.Values) {
    if (-not $byState.ContainsKey($state)) { $byState[$state] = 0 }
    $byState[$state]++
}

# An entry counts as advanced when its state changed at all, so a `partial`
# that was `open` is progress and is not silently a non-event.
$advanced = @()
$filed = @()
foreach ($id in $now.Keys) {
    # A row that did not exist at the base is filed, and its state matters:
    # an entry filed and closed in one session is not the same as one filed and
    # left open, and both happen.
    if (-not $before.ContainsKey($id)) { $filed += "$id ($($now[$id]))"; continue }
    if ($before[$id] -ne $now[$id]) { $advanced += "$id ($($before[$id]) -> $($now[$id]))" }
}
$closed = @($advanced | Where-Object { $_ -match '-> done\)$' })

$total = $now.Count
$deferred = if ($byState.ContainsKey('deferred')) { $byState['deferred'] } else { 0 }
$done = if ($byState.ContainsKey('done')) { $byState['done'] } else { 0 }
$workable = $total - $deferred
$left = $workable - $done

# ---------------------------------------------------------------------------
# How long it took
# ---------------------------------------------------------------------------

$endedAt = (Get-Date).ToUniversalTime()
$elapsed = $null
if ($Since) {
    try {
        $startedAt = [datetimeoffset]::Parse($Since).UtcDateTime
        $elapsed = $endedAt - $startedAt
    }
    catch { }
}
# `[math]::Floor`, not `[int]`. PowerShell's `[int]` on a double rounds to
# nearest, so `[int](2.65)` is 3 and a session of 2h 39m reported itself as
# "3h 39m": the hour came from the minutes and was then printed again beside
# them. Every session past the half hour was an hour too long, and the number
# goes into PROGRESS.md's state line.
$elapsedText = if ($elapsed) {
    "{0}h {1}m" -f [math]::Floor($elapsed.TotalHours), $elapsed.Minutes
}
else { "unknown, pass -Since" }

# ---------------------------------------------------------------------------
# Say it
# ---------------------------------------------------------------------------

if ($Json) {
    [ordered]@{
        kind           = "session-report"
        schema_version = "1"
        generated_at   = $endedAt.ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
        started_at     = $Since
        elapsed_ms     = if ($elapsed) { [int64]$elapsed.TotalMilliseconds } else { $null }
        base           = $Base
        head           = $head
        commits        = $commits.Count
        files_changed  = $files
        lines_added    = $added
        lines_removed  = $removed
        rust           = $rust
        entries        = [ordered]@{
            total     = $total
            deferred  = $deferred
            workable  = $workable
            done      = $done
            left      = $left
            by_state  = $byState
            closed    = @($closed)
            advanced  = @($advanced)
            filed     = @($filed)
        }
    } | ConvertTo-Json -Depth 6
    exit 0
}

Write-Host ""
Write-Host "session $Base..$head"
Write-Host ("  elapsed        {0}" -f $elapsedText)
Write-Host ("  commits        {0}" -f $commits.Count)
Write-Host ("  files changed  {0}, +{1} -{2}" -f $files, $added, $removed)
if ($rust.code -gt 0) {
    Write-Host ("  rust in tree   {0} files, {1} code, {2} comment" -f $rust.files, $rust.code, $rust.comments)
}
else {
    Write-Host "  rust in tree   scc is not on PATH, so this is unmeasured"
}
Write-Host ""
Write-Host ("  entries done   {0}/{1}, {2} left" -f $done, $workable, $left)
$order = @('open', 'partial', 'blocked', 'done', 'deferred')
$line = ($order | Where-Object { $byState.ContainsKey($_) } | ForEach-Object { "$_ $($byState[$_])" }) -join ', '
Write-Host ("  entry states   {0} (of {1} rows)" -f $line, $total)

if ($closed.Count -gt 0) {
    Write-Host ""
    Write-Host "  closed this session:"
    foreach ($item in $closed) { Write-Host "    $item" }
}
$other = @($advanced | Where-Object { $_ -notmatch '-> done\)$' })
if ($other.Count -gt 0) {
    Write-Host "  advanced:"
    foreach ($item in $other) { Write-Host "    $item" }
}
if ($filed.Count -gt 0) {
    Write-Host "  filed:"
    foreach ($item in $filed) { Write-Host "    $item" }
}
if ($commits.Count -gt 0) {
    Write-Host ""
    Write-Host "  commits:"
    foreach ($item in $commits) { Write-Host "    $item" }
}
Write-Host ""
exit 0
