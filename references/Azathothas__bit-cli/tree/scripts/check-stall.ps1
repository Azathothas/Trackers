# Run the same download hundreds of times and look at the tail.
#
# `TODO/performance.md` T-037 is one run in about seventy that took 274,546 ms
# where the same command usually takes about 3,200 ms. It completed and every
# byte arrived, and CPU time over that run was 5,155 ms, so the process was
# waiting rather than working for four and a half minutes. It has never been
# reproduced deliberately.
#
# A rare event needs a lot of trials, and a mean says nothing about a tail. So
# this runs one fixed command -Runs times and reports the distribution: median,
# p95, p99, maximum, and the ratio of the maximum to the median. That last one
# is the acceptance's number.
#
# What makes this different from re-running a benchmark is that every run's
# per-source reconnect counters are kept, and the slowest runs are reported in
# full. A bridge that loses its connection waits on a delay that doubles from
# one second to thirty, so thirteen consecutive failures is 271 seconds, which
# is the shape of the run T-037 recorded. If that is what happens, the slow run
# says so in `reconnects` and `reconnect_wait_ms` rather than looking like a
# slow mirror.
#
# The payload is small and the source is loopback, so a run is a second or two
# and hundreds of them are minutes rather than hours. That is deliberate: what
# is being sampled is the setup and teardown path, which is where a rare stall
# has to live, and not the transfer.
#
# Usage:
#   pwsh scripts/check-stall.ps1
#   pwsh scripts/check-stall.ps1 -Runs 500 -Torrents 4 -Jobs 2
#
# Exits 0 when every run completed and the slowest was inside -Ratio times the
# median, 1 when one was not, and 2 when the check could not run. The record
# goes to bench/stall-<timestamp>.json.
#
# See TODO/performance.md, T-037.

[CmdletBinding()]
param(
    [int]$Runs = 200,
    # Torrents per invocation, and how many of them run at once. The shape
    # T-037 was seen in is four torrents at -j 2.
    [int]$Torrents = 4,
    [int]$Jobs = 2,
    # Connections per source. check-multi-torrent.ps1 defaults to four, and the
    # run T-037 recorded came from it, so this matches.
    [int]$Connections = 4,
    [string]$PayloadSize = "16MiB",
    [string]$PieceLength = "1MiB",
    # How many times the median a run may take before it counts as a stall.
    [double]$Ratio = 5.0,
    # Slowest runs kept in full in the report.
    [int]$KeepSlowest = 5,
    [string]$Root = ".tmp/stall",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [int]$TimeoutSeconds = 600,
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
    [Console]::Error.WriteLine("check-stall: $message")
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
if ($Runs -lt 2) { Exit-With 2 "-Runs has to be at least 2 for a distribution to mean anything." }

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# ---------------------------------------------------------------------------
# One payload per torrent, each with its own seed
# ---------------------------------------------------------------------------
#
# No two torrents share a piece, so nothing is served out of another torrent's
# window cache and each source does its own work.

$payloadBytes = ConvertFrom-Size $PayloadSize
Write-Step "building $Torrents payloads of $PayloadSize"
$torrentFiles = @()
for ($t = 0; $t -lt $Torrents; $t++) {
    $dir = Join-Path $Root "serve/payload-$t"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $block = [byte[]]::new(1024 * 1024)
    [int64]$state = 9001 + ($t * 7919)
    for ($i = 0; $i -lt $block.Length; $i++) {
        $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
        $block[$i] = [byte](($state -shr 16) -band 0xFF)
    }
    $stream = [System.IO.File]::Create((Join-Path $dir "movie.bin"))
    try {
        [int64]$written = 0
        while ($written -lt $payloadBytes) {
            $take = [Math]::Min([int64]$block.Length, $payloadBytes - $written)
            $stream.Write($block, 0, [int]$take)
            $written += $take
        }
    }
    finally { $stream.Dispose() }

    $torrent = Join-Path $Root "payload-$t.torrent"
    & $bitCli create $dir --name "payload-$t" --piece-length $PieceLength `
        --no-creation-date --output $torrent --force --json 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }
    $torrentFiles += $torrent
}

$serveRoot = Join-Path $Root "serve"
$stdout = Join-Path $Root "server.url"
$server = Start-Process -FilePath $fileserver -WorkingDirectory $Root -NoNewWindow -PassThru `
    -ArgumentList @("--root", $serveRoot, "--port", "0") `
    -RedirectStandardOutput $stdout -RedirectStandardError (Join-Path $Root "server.log")
$script:Background += $server
$deadline = (Get-Date).AddSeconds(15)
$base = $null
while (-not $base -and (Get-Date) -lt $deadline) {
    $line = Get-Content $stdout -TotalCount 1 -ErrorAction SilentlyContinue
    if ($line -and $line.Trim()) { $base = $line.Trim() }
    if (-not $base) { Start-Sleep -Milliseconds 50 }
}
if (-not $base) { Exit-With 2 "the file server printed no URL" }
Write-Step "server at $base, $Runs runs of $Torrents torrents at -j $Jobs, $Connections connections each"

# ---------------------------------------------------------------------------
# The runs
# ---------------------------------------------------------------------------

$arguments = @("download") + $torrentFiles + @(
    "--dir", (Join-Path $Root "out"),
    "--web-seed", $base,
    "--web-seed-only",
    "--web-seed-connections", "$Connections",
    "--allow-overwrite",
    "-j", "$Jobs",
    "--port", "0",
    "--json"
)
$command = "bit-cli $($arguments -join ' ')"

$results = [System.Collections.ArrayList]::new()
$clock = [System.Diagnostics.Stopwatch]::StartNew()
for ($run = 0; $run -lt $Runs; $run++) {
    # A fresh output directory per run, so nothing resumes and every run does
    # the same work. The volume's own state carries between runs otherwise.
    $out = Join-Path $Root "out"
    if (Test-Path $out) { Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force -Path $out | Out-Null

    $runOut = Join-Path $Root "run.json"
    $runErr = Join-Path $Root "run.err"
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $repo -NoNewWindow -PassThru `
        -ArgumentList $arguments -RedirectStandardOutput $runOut -RedirectStandardError $runErr
    $finished = $process.WaitForExit($TimeoutSeconds * 1000)
    $watch.Stop()
    if (-not $finished) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }

    $report = $null
    try { $report = Get-Content $runOut -Raw | ConvertFrom-Json } catch { }
    $reconnects = 0
    $reconnectWaitMs = 0
    $reasons = [ordered]@{}
    if ($report -and $report.torrents) {
        foreach ($torrent in $report.torrents) {
            foreach ($source in @($torrent.sources)) {
                if (-not $source) { continue }
                if ($source.PSObject.Properties.Name -contains 'reconnects') {
                    $reconnects += [int64]$source.reconnects
                }
                if ($source.PSObject.Properties.Name -contains 'reconnect_wait_ms') {
                    $reconnectWaitMs += [int64]$source.reconnect_wait_ms
                }
                if ($source.PSObject.Properties.Name -contains 'reconnect_reasons') {
                    foreach ($property in $source.reconnect_reasons.PSObject.Properties) {
                        if (-not $reasons.Contains($property.Name)) { $reasons[$property.Name] = 0 }
                        $reasons[$property.Name] += [int64]$property.Value
                    }
                }
            }
        }
    }

    [void]$results.Add([ordered]@{
        run                = $run
        exit_code          = if ($finished) { $process.ExitCode } else { 124 }
        wall_ms            = $watch.ElapsedMilliseconds
        reported_ms        = if ($report) { [int64]$report.elapsed_ms } else { $null }
        cpu_ms             = if ($report -and $report.process) { [int64]$report.process.cpu_ms } else { $null }
        peak_rss_bytes     = if ($report -and $report.process) { [int64]$report.process.peak_rss_bytes } else { $null }
        open_handles       = if ($report -and $report.process) { [int64]$report.process.open_handles } else { $null }
        completed          = if ($report) { [int]$report.completed } else { 0 }
        failed             = if ($report) { [int]$report.failed } else { $Torrents }
        reconnects         = $reconnects
        reconnect_wait_ms  = $reconnectWaitMs
        reconnect_reasons  = $reasons
    })

    if (($run + 1) % 25 -eq 0) {
        $so_far = @($results | ForEach-Object { $_.wall_ms }) | Sort-Object
        $mid = $so_far[[int]($so_far.Count / 2)]
        Write-Step ("  {0,4} runs, median {1,6} ms, slowest {2,7} ms" -f ($run + 1), $mid, $so_far[-1])
    }
}
$clock.Stop()
Stop-Background

# ---------------------------------------------------------------------------
# The distribution
# ---------------------------------------------------------------------------

function Get-Percentile($sorted, [double]$fraction) {
    if ($sorted.Count -eq 0) { return $null }
    $index = [int][math]::Floor($fraction * ($sorted.Count - 1))
    $sorted[[math]::Min($index, $sorted.Count - 1)]
}

$sorted = @($results | ForEach-Object { [int64]$_.wall_ms }) | Sort-Object
$median = Get-Percentile $sorted 0.5
$p95 = Get-Percentile $sorted 0.95
$p99 = Get-Percentile $sorted 0.99
$slowest = $sorted[-1]
$fastest = $sorted[0]
$maxOverMedian = if ($median -gt 0) { [math]::Round($slowest / $median, 3) } else { $null }

$slowRuns = @($results | Sort-Object { -[int64]$_.wall_ms } | Select-Object -First $KeepSlowest)
$notCompleted = @($results | Where-Object { $_.exit_code -ne 0 -or $_.completed -lt $Torrents })
$withReconnects = @($results | Where-Object { $_.reconnects -gt 0 })
$totalReconnects = 0
$totalReconnectWaitMs = 0
foreach ($entry in $results) {
    $totalReconnects += [int64]$entry.reconnects
    $totalReconnectWaitMs += [int64]$entry.reconnect_wait_ms
}

$failures = [System.Collections.ArrayList]::new()
if ($notCompleted.Count -gt 0) {
    [void]$failures.Add("$($notCompleted.Count) of $Runs runs did not complete every torrent; the first was run $($notCompleted[0].run) with exit $($notCompleted[0].exit_code)")
}
if ($null -ne $maxOverMedian -and $maxOverMedian -gt $Ratio) {
    [void]$failures.Add("the slowest run took ${slowest} ms against a median of ${median} ms, a ratio of $maxOverMedian over the ceiling of $Ratio")
}

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "stall-$stamp.json"
$verdict = switch ($true) {
    ($failures.Count -eq 0) {
        "$Runs runs, median ${median} ms, slowest ${slowest} ms, a ratio of $maxOverMedian inside the ceiling of $Ratio"
        break
    }
    default { "$($failures.Count) thing(s) did not hold over $Runs runs"; break }
}

[ordered]@{
    kind           = "check-stall"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = [System.Environment]::MachineName
        os      = [System.Environment]::OSVersion.VersionString
        cpus    = [System.Environment]::ProcessorCount
    }
    parameters     = [ordered]@{
        runs          = $Runs
        torrents      = $Torrents
        jobs          = $Jobs
        connections   = $Connections
        payload_size  = $PayloadSize
        payload_bytes = $payloadBytes
        piece_length  = $PieceLength
        ratio_ceiling = $Ratio
        profile       = $Profile
    }
    command        = $command
    elapsed_ms     = $clock.ElapsedMilliseconds
    distribution   = [ordered]@{
        runs        = $Runs
        fastest_ms  = $fastest
        median_ms   = $median
        p95_ms      = $p95
        p99_ms      = $p99
        slowest_ms  = $slowest
        max_over_median = $maxOverMedian
        mean_ms     = [math]::Round((($sorted | Measure-Object -Average).Average), 1)
    }
    reconnects     = [ordered]@{
        runs_with_any = $withReconnects.Count
        total         = $totalReconnects
        total_wait_ms = $totalReconnectWaitMs
    }
    slowest_runs   = @($slowRuns)
    runs           = @($results)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "Every run writes into a fresh output directory, so nothing resumes and each run does the same work.",
        "The source is one loopback file server serving out of the page cache, so the wire is not the variable and what is sampled is the client's setup, transfer, and teardown path.",
        "reconnects and reconnect_wait_ms come from the run's own report, summed over every source of every torrent. A bridge waits between attempts on a delay that doubles from 1s to 30s, so thirteen consecutive failures is 271 seconds. A slow run with zero reconnects was slow for some other reason, and that is the point of carrying both.",
        "max_over_median is the acceptance's number. A tail is not a mean: T-037's run was 85 times the median, which no average over the same sixty runs would have shown."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "runs:    $Runs of $Torrents torrents at -j $Jobs, $PayloadSize each"
Write-Host "command: $command"
Write-Host "report:  $reportPath"
Write-Host ""
[pscustomobject][ordered]@{
    fastest    = "$fastest ms"
    median     = "$median ms"
    p95        = "$p95 ms"
    p99        = "$p99 ms"
    slowest    = "$slowest ms"
    "max/median" = $maxOverMedian
} | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "runs with a reconnect: $($withReconnects.Count) of $Runs, $totalReconnectWaitMs ms waited in total"
Write-Host ""
$slowRuns | ForEach-Object {
    [pscustomobject][ordered]@{
        run          = $_.run
        "wall ms"    = $_.wall_ms
        "cpu ms"     = $_.cpu_ms
        exit         = $_.exit_code
        completed    = $_.completed
        reconnects   = $_.reconnects
        "waited ms"  = $_.reconnect_wait_ms
    }
} | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-stall: $failure") }
    exit 1
}
exit 0
