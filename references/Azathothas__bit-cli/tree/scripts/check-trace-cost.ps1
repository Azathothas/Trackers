<#
.SYNOPSIS
    What `--trace http` costs a measurement.

.DESCRIPTION
    `TODO/bench.md` T-094 is the rule that tracing must never change a
    measurement without saying so. Nobody had measured it.

    It measures `download --web-seed-only`, and **not** `bench webseed`, which
    is what the entry proposed. `bench webseed` builds its own
    `reqwest::Client` at `crates/bit-cli-core/src/bench/webseed.rs:383` and
    never goes through `webseed::fetch`, where the `bit_cli::http` trace lives,
    so `--trace http` produces no records there at all: a comparison of two
    identical runs. Measured, not reasoned about: a traced `bench webseed` run
    writes one line of stderr, the one naming the report it wrote, and a traced
    `download` of the same payload writes one per ranged GET.

    Traced and untraced runs alternate so drift on the machine falls on both.
    Two blocks of five runs measure the first block against whatever else the
    machine was doing during the second.

    The minimum is what is compared, and the spread is printed beside it. The
    minimum of a set of timings is the least contaminated by everything else on
    the machine; the spread says how much to trust it.

        pwsh -NoProfile -File scripts/check-trace-cost.ps1
        pwsh -NoProfile -File scripts/check-trace-cost.ps1 -Runs 7 -PayloadSize 1GiB

    It judges nothing. What it measures is a property of this machine, and the
    entry records the numbers.
#>

[CmdletBinding()]
param(
    [string]$PayloadSize = "256MiB",
    [int]$Runs = 5,
    [int]$Concurrency = 8,
    [string]$RequestSize = "1MiB",
    [string]$Root = ".tmp/trace-cost",
    [string]$Json,
    [int]$TimeoutSeconds = 300
)

$ErrorActionPreference = 'Stop'

$exe = if ($IsWindows -or $env:OS -eq 'Windows_NT') { ".exe" } else { "" }
$bitCli = "target/release/bit-cli$exe"
$fileserver = "target/release/examples/loopback-fileserver$exe"
foreach ($path in @($bitCli, $fileserver)) {
    if (-not (Test-Path $path)) {
        throw "no binary at $path. Build one: cargo build --release --bins --examples"
    }
}
$bitCli = (Resolve-Path $bitCli).Path
$fileserver = (Resolve-Path $fileserver).Path

if (Test-Path $Root) { Remove-Item -Recurse -Force $Root }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path

function Write-Step([string]$text) {
    Write-Output ("{0}Z check-trace-cost: {1}" -f (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fff"), $text)
}

# The same deterministic payload generator bench-webseed.ps1 uses, so the two
# scripts measure the same bytes.
$payloadBytes = [int64]256MB
if ($PayloadSize -match '^(\d+)(MiB|GiB|MB|GB)?$') {
    $n = [int64]$Matches[1]
    $payloadBytes = switch ($Matches[2]) {
        'GiB' { $n * 1GB }
        'GB' { $n * 1GB }
        default { $n * 1MB }
    }
}

Write-Step "building a $([math]::Round($payloadBytes / 1MB)) MiB payload"
$payloadDir = Join-Path $Root "payload"
New-Item -ItemType Directory -Force -Path $payloadDir | Out-Null
$block = 1MB
$buffer = [byte[]]::new($block)
[int64]$state = 12345
for ($i = 0; $i -lt $block; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $buffer[$i] = [byte](($state -shr 16) -band 0xFF)
}
$stream = [System.IO.File]::Create((Join-Path $payloadDir "movie.bin"))
try {
    $written = 0
    while ($written -lt $payloadBytes) {
        $want = [math]::Min($block, $payloadBytes - $written)
        $stream.Write($buffer, 0, $want)
        $written += $want
    }
} finally { $stream.Dispose() }

$torrent = Join-Path $Root "target.torrent"
Write-Step "creating the torrent"
& $bitCli create $payloadDir --name payload --piece-length 1MiB --no-creation-date --output $torrent --force | Out-Null
if ($LASTEXITCODE -ne 0) { throw "bit-cli create exited $LASTEXITCODE" }

$serverOut = Join-Path $Root "fileserver.out"
$serverErr = Join-Path $Root "fileserver.err"
$server = Start-Process -FilePath $fileserver -ArgumentList @("--root", $Root) -NoNewWindow -PassThru `
    -RedirectStandardOutput $serverOut -RedirectStandardError $serverErr

try {
    $webSeed = $null
    $deadline = (Get-Date).AddSeconds(20)
    while (-not $webSeed -and (Get-Date) -lt $deadline) {
        if (Test-Path $serverOut) {
            $line = (Get-Content $serverOut -TotalCount 1 -ErrorAction SilentlyContinue)
            if ($line -match 'http://\S+') { $webSeed = $Matches[0] }
        }
        if (-not $webSeed) { Start-Sleep -Milliseconds 100 }
    }
    if (-not $webSeed) { throw "the file server printed no URL within 20 seconds" }
    Write-Step "web seed at $webSeed"

    # One run, returning what the report says about it.
    #
    # stderr goes to a file rather than to a console. That is the cheap
    # destination, and it is the honest one to measure: a caller redirecting a
    # trace to a file is the normal case, and a console would measure the
    # console.
    function Invoke-Run([bool]$traced, [int]$index) {
        $label = if ($traced) { "traced" } else { "plain" }
        $report = Join-Path $Root ("run-{0}-{1}.json" -f $label, $index)
        $out = Join-Path $Root ("dl-{0}-{1}" -f $label, $index)
        if (Test-Path $out) { Remove-Item -Recurse -Force $out }
        $argv = [System.Collections.Generic.List[string]]::new()
        if ($traced) { $argv.AddRange([string[]]@("--trace", "http")) }
        $argv.AddRange([string[]]@(
                "download", $torrent,
                "--dir", $out,
                "--web-seed", "$webSeed/payload/",
                "--web-seed-mode", "prefix",
                "--no-torrent-web-seed",
                "--web-seed-only",
                "--web-seed-concurrency", "$Concurrency",
                "--web-seed-chunk-size", $RequestSize,
                "--port", "0",
                "--json"
            ))
        $errFile = "$report.err"
        $process = Start-Process -FilePath $bitCli -ArgumentList $argv -NoNewWindow -PassThru `
            -RedirectStandardOutput $report -RedirectStandardError $errFile
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            $process.Kill()
            throw "a run did not finish within $TimeoutSeconds seconds"
        }
        if ($process.ExitCode -ne 0) {
            throw "download exited $($process.ExitCode): $(Get-Content $errFile -Raw)"
        }
        $doc = Get-Content $report -Raw | ConvertFrom-Json
        $lines = @(Get-Content $errFile -ErrorAction SilentlyContinue).Count
        [pscustomobject]@{
            traced      = $traced
            bytes_per_s = [double]$doc.torrents[0].mean_rate.bytes
            peak_rss    = [int64]$doc.process.peak_rss_bytes
            requests    = $lines
            tracing     = $traced
        }
    }

    # Alternating, so drift on the machine falls on both arms.
    $results = @()
    for ($i = 1; $i -le $Runs; $i++) {
        Write-Step "run $i of $Runs, plain"
        $results += Invoke-Run $false $i
        Write-Step "run $i of $Runs, --trace http"
        $results += Invoke-Run $true $i
    }
}
finally {
    if ($server -and -not $server.HasExited) { $server.Kill() }
}

function Summarise([object[]]$rows, [string]$label) {
    $rates = @($rows | ForEach-Object { $_.bytes_per_s })
    $rss = @($rows | ForEach-Object { $_.peak_rss })
    [pscustomobject]@{
        arm               = $label
        runs              = $rows.Count
        best_bytes_per_s  = [int64]($rates | Measure-Object -Maximum).Maximum
        median_bytes_per_s = [int64](($rates | Sort-Object)[[math]::Floor($rates.Count / 2)])
        spread_bytes_per_s = [int64](($rates | Measure-Object -Maximum).Maximum - ($rates | Measure-Object -Minimum).Minimum)
        peak_rss_bytes    = [int64]($rss | Measure-Object -Maximum).Maximum
        tracing_enabled   = [bool]($rows[0].tracing)
    }
}

$plain = Summarise @($results | Where-Object { -not $_.traced }) "plain"
$traced = Summarise @($results | Where-Object { $_.traced }) "--trace http"

$throughputCost = 0.0
if ($plain.best_bytes_per_s -gt 0) {
    $throughputCost = 100.0 * ($plain.best_bytes_per_s - $traced.best_bytes_per_s) / $plain.best_bytes_per_s
}
$rssCost = $traced.peak_rss_bytes - $plain.peak_rss_bytes

$report = [ordered]@{
    generated_at    = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    payload_bytes   = $payloadBytes
    concurrency     = $Concurrency
    request_size    = $RequestSize
    runs_per_arm    = $Runs
    plain           = $plain
    traced          = $traced
    throughput_cost_percent = [math]::Round($throughputCost, 2)
    peak_rss_cost_bytes     = $rssCost
    stderr_lines_plain      = [int64](@($results | Where-Object { -not $_.traced } | ForEach-Object { $_.requests }) | Measure-Object -Maximum).Maximum
    stderr_lines_traced     = [int64](@($results | Where-Object { $_.traced } | ForEach-Object { $_.requests }) | Measure-Object -Maximum).Maximum
}

Write-Output ""
Write-Output ("{0,-14} {1,16} {2,16} {3,14}" -f 'arm', 'best B/s', 'median B/s', 'peak RSS')
foreach ($arm in @($plain, $traced)) {
    Write-Output ("{0,-14} {1,16} {2,16} {3,14}" -f $arm.arm, $arm.best_bytes_per_s, $arm.median_bytes_per_s, $arm.peak_rss_bytes)
}
Write-Output ""
Write-Output ("throughput cost: {0}% of the best plain run" -f $report.throughput_cost_percent)
Write-Output ("peak RSS cost:   {0} bytes" -f $rssCost)
Write-Output ("spread:          plain {0}, traced {1}" -f $plain.spread_bytes_per_s, $traced.spread_bytes_per_s)
# The line count is what says the trace fired at all. A comparison against a
# trace that emitted nothing is a comparison of a run with itself, which is
# what measuring `bench webseed` would have been.
$tracedLines = (@($results | Where-Object { $_.traced } | ForEach-Object { $_.requests }) | Measure-Object -Maximum).Maximum
$plainLines = (@($results | Where-Object { -not $_.traced } | ForEach-Object { $_.requests }) | Measure-Object -Maximum).Maximum
Write-Output ("stderr lines:    plain {0}, traced {1}" -f $plainLines, $tracedLines)
if ($tracedLines -le $plainLines) {
    Write-Output "the traced arm wrote no more than the plain one: nothing was measured"
}
Write-Output ""

if ($Json) {
    $report | ConvertTo-Json -Depth 6 | Set-Content -Path $Json -Encoding utf8
    Write-Output "wrote $Json"
}
