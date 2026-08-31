# Does --piece-selector change the order pieces arrive in, and what does it cost?
#
# `TODO/performance.md` T-032 said the four `--piece-selector` values do not
# reach `librqbit`'s picker, "which is rarest-first and not configurable". Half
# of that is wrong. `librqbit` 9.0.0 is not rarest-first: nothing in
# `ChunkTracker::iter_queued_pieces` counts how many peers hold a piece. It
# walks the files in priority order and, within each file, yields the first
# piece, then the last, then the middle in ascending order. Near-sequential
# with the tail pulled forward.
#
# So the interesting number is not "is it ordered" but "where does it break
# order, and does the flag fix that". This measures both, on the same fixture,
# with the same source, changing one flag:
#
#   descents   how many times `piece_verified` reported a piece lower than the
#              one before it. The default's descent is the last piece arriving
#              third or fourth. Sequential's, when it has any, is two
#              concurrent transfers finishing out of the order they started in.
#   wall       what the ordering costs, because holding the priority window
#              open holds one permit from the session's blocking semaphore and
#              points every peer at the same part of the file.
#
# The loop variable is `$connectionCount` rather than `$connections`, because
# PowerShell resolves variable names case-insensitively and `$connections`
# **is** the `-Connections` parameter: the first iteration would overwrite the
# array it is iterating. See TODO/RULES.md on this, it has bitten here
# before.
#
# **Connections are the variable that matters.** A selector decides which piece
# is asked for next; it cannot decide the order in which N transfers already in
# flight finish. At `--web-seed-connections 1` the arrival order is the request
# order and the answer is exact. Above 1 it is the request order with local
# reordering, and that reordering is concurrency rather than selection. Both
# are measured here because reading them together is what separates the two.
#
# Usage:
#   pwsh scripts/check-piece-order.ps1
#   pwsh scripts/check-piece-order.ps1 -Runs 10 -Connections 1,2,4,8
#
# Exits 0 when sequential is strictly non-decreasing at one connection and no
# worse than the default above it, 1 when it is not, and 2 when the check could
# not run. The record goes to bench/piece-order-<timestamp>.json.
#
# See TODO/performance.md, T-032.

[CmdletBinding()]
param(
    # Runs per cell. The descent count at one connection is deterministic; the
    # wall clock is not, so the throughput comparison wants several.
    [int]$Runs = 5,
    # A comma-separated list rather than an `[int[]]`, because `pwsh -File`
    # hands `-Connections 1,2,4` over as the single string "1,2,4" and an
    # `[int[]]` parameter silently makes something else of it. A string that
    # this splits itself behaves the same under `-File` and `-Command`.
    [string]$Connections = "1,4",
    [string]$PayloadSize = "48MiB",
    [string]$PieceLength = "1MiB",
    # How much slower sequential may be than the default at the same
    # connection count before it counts as a regression.
    [double]$SlowdownCeiling = 1.6,
    [string]$Root = ".tmp/piece-order",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

$script:Background = @()

function Stop-Background {
    foreach ($process in $script:Background) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $script:Background = @()
}

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-piece-order: $message")
    Stop-Background
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

function ConvertFrom-Size([string]$text) {
    if ($text -match '^\s*([0-9]+(?:\.[0-9]+)?)\s*([A-Za-z]*)\s*$') {
        $value = [double]$Matches[1]
        switch ($Matches[2].ToUpperInvariant()) {
            "" { return [int64]$value }
            "B" { return [int64]$value }
            "KIB" { return [int64]($value * 1024) }
            "MIB" { return [int64]($value * 1024 * 1024) }
            "GIB" { return [int64]($value * 1024 * 1024 * 1024) }
            default { Exit-With 2 "cannot read the size '$text'" }
        }
    }
    Exit-With 2 "cannot read the size '$text'"
}

trap { Stop-Background; throw }

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
$fileserver = Join-Path $repo "target/$Profile/examples/loopback-fileserver$exe"
foreach ($required in @($bitCli, $fileserver)) {
    if (-not (Test-Path $required)) {
        Exit-With 2 "missing $required. Build it first: cargo build --$Profile --workspace --bins --examples"
    }
}
if ($Runs -lt 1) { Exit-With 2 "-Runs has to be at least 1." }
$connectionCounts = @(
    $Connections -split ',' | ForEach-Object {
        $text = $_.Trim()
        if ($text -notmatch '^[0-9]+$') { Exit-With 2 "-Connections has to be a comma-separated list of numbers, not '$Connections'" }
        [int]$text
    }
)
if ($connectionCounts.Count -eq 0) { Exit-With 2 "-Connections is empty" }
if (-not ($connectionCounts -contains 1)) {
    Exit-With 2 "-Connections has to include 1: it is the only setting where the arrival order is the request order, and so the only one where the selector can be checked exactly."
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# ---------------------------------------------------------------------------
# A payload with enough pieces for an order to be visible
# ---------------------------------------------------------------------------
#
# The torrent name and the served directory have to agree, because BEP 19
# composes `<url>/<name>/<path>` for a multi-file torrent. Serving the parent
# and naming the directory after the torrent is what makes one URL enough.

$payloadBytes = ConvertFrom-Size $PayloadSize
$served = Join-Path $Root "served"
$payloadDir = Join-Path $served "target"
New-Item -ItemType Directory -Force -Path $payloadDir | Out-Null

Write-Step "building a $PayloadSize payload"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 424242
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}
$stream = [System.IO.File]::Create((Join-Path $payloadDir "payload.bin"))
try {
    $written = 0
    while ($written -lt $payloadBytes) {
        $take = [Math]::Min($block.Length, $payloadBytes - $written)
        $stream.Write($block, 0, $take)
        $written += $take
    }
}
finally { $stream.Dispose() }

$torrent = Join-Path $Root "target.torrent"
& $bitCli create $payloadDir --name target --piece-length $PieceLength --no-creation-date `
    --output $torrent --force --json 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }
$pieceCount = [int]((& $bitCli info $torrent --json | ConvertFrom-Json).piece_count)
if ($pieceCount -lt 8) {
    Exit-With 2 "the fixture has $pieceCount pieces, which is too few for an order to mean anything"
}
Write-Step "$pieceCount pieces of $PieceLength"

# ---------------------------------------------------------------------------
# One server for every run
# ---------------------------------------------------------------------------

$server = Start-Process -FilePath $fileserver -WorkingDirectory $repo -NoNewWindow -PassThru `
    -ArgumentList @("--root", $served, "--port", "0") `
    -RedirectStandardOutput (Join-Path $Root "server.out") `
    -RedirectStandardError (Join-Path $Root "server.err")
$script:Background += $server

$base = $null
$deadline = (Get-Date).AddSeconds(15)
while (-not $base -and (Get-Date) -lt $deadline) {
    $line = Get-Content (Join-Path $Root "server.out") -TotalCount 1 -ErrorAction SilentlyContinue
    if ($line -and $line.Trim()) { $base = $line.Trim() }
    if (-not $base) { Start-Sleep -Milliseconds 100 }
}
if (-not $base) { Exit-With 2 "the loopback file server never printed its URL" }
Write-Step "serving at $base"

# ---------------------------------------------------------------------------
# The sweep
# ---------------------------------------------------------------------------

function Measure-Order([string]$selector, [int]$connectionCount, [int]$run) {
    $out = Join-Path $Root "out-$run"
    if (Test-Path $out) { Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue }
    $events = Join-Path $Root "events-$run.jsonl"
    $arguments = @(
        "--jsonl", "download", $torrent, "-d", $out,
        "--web-seed-only", "--no-torrent-web-seed",
        "--web-seed", $base,
        "--web-seed-connections", "$connectionCount",
        # Short, because `piece_verified` is derived from polling the bitfield
        # and a coarse interval would fold several pieces into one tick and
        # report them in index order whatever order they arrived in. That would
        # make every selector look sequential. See TODO/cli-surface.md, T-111.
        "--report-interval", "20ms",
        "--stop-timeout", "120s"
    )
    if ($selector) { $arguments += @("--piece-selector", $selector) }

    # Through `Start-Process` rather than the call operator, because under
    # `$ErrorActionPreference = 'Stop'` a native command writing to stderr is a
    # terminating error in PowerShell 7, and `bit-cli` writes warnings there by
    # design. Redirecting both streams to files is what the other check scripts
    # do for the same reason.
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $repo -NoNewWindow -PassThru `
        -ArgumentList $arguments `
        -RedirectStandardOutput $events `
        -RedirectStandardError (Join-Path $Root "run-$run.err")
    $process.WaitForExit()
    $watch.Stop()
    $code = $process.ExitCode

    $pieces = @(Get-Content $events | ForEach-Object {
            $document = $_ | ConvertFrom-Json
            if ($document.type -eq "piece_verified") { [int]$document.piece }
        })
    $descents = 0
    $firstDescent = $null
    for ($i = 1; $i -lt $pieces.Count; $i++) {
        if ($pieces[$i] -lt $pieces[$i - 1]) {
            $descents++
            if ($null -eq $firstDescent) { $firstDescent = $i }
        }
    }
    Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
    [pscustomobject][ordered]@{
        selector      = if ($selector) { $selector } else { "default" }
        connections   = $connectionCount
        run           = $run
        exit_code     = $code
        pieces        = $pieces.Count
        descents      = $descents
        first_descent = $firstDescent
        wall_ms       = [int]$watch.Elapsed.TotalMilliseconds
        order         = ($pieces -join ",")
    }
}

$results = @()
$failures = @()
foreach ($connectionCount in $connectionCounts) {
    foreach ($selector in @("", "sequential")) {
        for ($run = 1; $run -le $Runs; $run++) {
            $result = Measure-Order $selector $connectionCount $run
            $results += $result
            if ($result.exit_code -ne 0) {
                $failures += "$($result.selector) at $connectionCount connection(s) exited $($result.exit_code)"
            }
            if ($result.pieces -ne $pieceCount) {
                $failures += "$($result.selector) at $connectionCount connection(s) verified $($result.pieces) pieces, not $pieceCount"
            }
            Write-Host ("  {0,-10} conn={1,-2} run={2}  {3,3} pieces  {4,2} descents  {5,6} ms" -f
                $result.selector, $connectionCount, $run, $result.pieces, $result.descents, $result.wall_ms)
        }
    }
}

Stop-Background

# ---------------------------------------------------------------------------
# The verdict
# ---------------------------------------------------------------------------

function Cell([string]$selector, [int]$connectionCount) {
    $rows = $results | Where-Object { $_.selector -eq $selector -and $_.connections -eq $connectionCount }
    [pscustomobject][ordered]@{
        selector          = $selector
        connections       = $connectionCount
        runs              = ($rows | Measure-Object).Count
        descents_max      = ($rows | Measure-Object descents -Maximum).Maximum
        descents_mean     = [math]::Round((($rows | Measure-Object descents -Average).Average), 2)
        wall_ms_mean      = [int](($rows | Measure-Object wall_ms -Average).Average)
        wall_ms_min       = ($rows | Measure-Object wall_ms -Minimum).Minimum
    }
}

$cells = @()
foreach ($connectionCount in $connectionCounts) {
    foreach ($selector in @("default", "sequential")) {
        $cells += Cell $selector $connectionCount
    }
}

# The acceptance, and the reason it is stated at one connection. With one
# transfer in flight the arrival order is the request order, so a descent there
# is the selector's and nothing else's.
$sequentialAtOne = $cells | Where-Object { $_.selector -eq "sequential" -and $_.connections -eq 1 }
$defaultAtOne = $cells | Where-Object { $_.selector -eq "default" -and $_.connections -eq 1 }
if ($sequentialAtOne.descents_max -ne 0) {
    $failures += "sequential at one connection had up to $($sequentialAtOne.descents_max) descents; at one connection the arrival order is the request order, so it must have none."
}
if ($defaultAtOne.descents_max -eq 0) {
    $failures += "the default at one connection had no descents in any run, so there is nothing for the flag to fix and this check is measuring the wrong thing."
}

# And the cost, at every connection count.
foreach ($connectionCount in $connectionCounts) {
    $d = $cells | Where-Object { $_.selector -eq "default" -and $_.connections -eq $connectionCount }
    $s = $cells | Where-Object { $_.selector -eq "sequential" -and $_.connections -eq $connectionCount }
    if ($d.wall_ms_mean -gt 0) {
        $ratio = [math]::Round($s.wall_ms_mean / $d.wall_ms_mean, 3)
        if ($ratio -gt $SlowdownCeiling) {
            $failures += "sequential at $connectionCount connection(s) took ${ratio}x the default, over the ${SlowdownCeiling}x ceiling."
        }
    }
}

$verdict = if ($failures.Count -eq 0) { "pass" } else { "fail" }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "piece-order-$stamp.json"

[pscustomobject][ordered]@{
    kind           = "piece_order"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
        cpus    = [Environment]::ProcessorCount
    }
    parameters     = [ordered]@{
        runs             = $Runs
        connections      = @($connectionCounts)
        payload_size     = $PayloadSize
        piece_length     = $PieceLength
        piece_count      = $pieceCount
        slowdown_ceiling = $SlowdownCeiling
        profile          = $Profile
    }
    cells          = @($cells)
    runs_detail    = @($results)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "descents counts how many times piece_verified reported a lower index than the event before it. It is the number the acceptance turns on.",
        "At one connection the arrival order is the request order, so a descent is the selector's. Above one it is the request order with local reordering from concurrent transfers finishing out of turn, which no selector can prevent.",
        "The default is not rarest-first. librqbit 9.0.0 yields the first piece of a file, then the last, then the middle ascending, so its descent is the tail arriving early. Nothing in its picker counts how many peers hold a piece.",
        "--report-interval is 20ms on purpose. piece_verified is derived from polling the bitfield, so a coarse interval folds several pieces into one tick and reports them in index order whatever order they arrived in."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
$cells | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "report:  $reportPath"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-piece-order: $failure") }
    exit 1
}
exit 0
