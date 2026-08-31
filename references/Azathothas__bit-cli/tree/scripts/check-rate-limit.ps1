# Does the session rate limit move a number.
#
# `TODO/performance.md` T-031: `--max-download-rate` and `--max-upload-rate` go
# straight into `librqbit`'s `LimitsConfig`, which was reported upstream not to
# take effect. Closed upstream is not verified here, and rule 0.10 says a knob
# that does not move a number does not ship.
#
# The measurement is a paired run. One seeder, one payload, two downloads: one
# capped and one not, alternating so neither always gets the disk in the same
# state. The capped run has to sustain within -Tolerance of the cap, and the
# uncapped run has to be meaningfully faster, or the flag is decorative.
#
# Peers only. `--no-web-seed` and `--no-tracker` are set, so the only source is
# the seeder named with `--peer`, and the number measured is the session's own
# limiter rather than the per-source token bucket that `--web-seed-speed-limit`
# already has.
#
# Usage:
#   pwsh scripts/check-rate-limit.ps1
#   pwsh scripts/check-rate-limit.ps1 -Rate 2MiB/s -PayloadSize 256MiB -Runs 3
#
# Exits 0 when the cap holds, 1 when it does not, and 2 when the check could
# not run. The record goes to bench/rate-limit-<timestamp>.json.
#
# See TODO/performance.md, T-031.

[CmdletBinding()]
param(
    [string]$Rate = "4MiB/s",
    [string]$PayloadSize = "128MiB",
    [int]$Runs = 3,
    # Fraction the sustained rate may exceed the cap by. A limiter that lets a
    # burst through and then pauses is still a limiter; one that is 50% over is
    # not.
    [double]$Tolerance = 0.15,
    [string]$Root = ".tmp/rate-limit",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [int]$TimeoutSeconds = 600,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-rate-limit: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

function ConvertFrom-Size([string]$text) {
    if ($text -match '^\s*([0-9]+(?:\.[0-9]+)?)\s*([A-Za-z]*)\s*(?:/s)?\s*$') {
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

function Format-Size([double]$bytes) {
    $units = @("B", "KiB", "MiB", "GiB", "TiB")
    $index = 0
    while ($bytes -ge 1024 -and $index -lt $units.Count - 1) { $bytes /= 1024; $index++ }
    "{0:N2} {1}" -f $bytes, $units[$index]
}

function Get-Median([double[]]$values) {
    if ($null -eq $values -or $values.Count -eq 0) { return 0 }
    $sorted = @($values | Sort-Object)
    $mid = [math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2 -eq 1) { return $sorted[$mid] }
    return ($sorted[$mid - 1] + $sorted[$mid]) / 2
}

$script:Background = @()

function Stop-Background {
    foreach ($process in $script:Background) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $script:Background = @()
}

trap { Stop-Background; throw }

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --workspace --bins --examples"
}
if ($Runs -lt 1) { Exit-With 2 "-Runs has to be at least 1." }

$rateBytes = ConvertFrom-Size $Rate
if ($rateBytes -lt 1) { Exit-With 2 "-Rate has to be positive." }

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path (Join-Path $Root "payload") | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

$payloadBytes = ConvertFrom-Size $PayloadSize
Write-Step "building a payload of $(Format-Size $payloadBytes)"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 8675309
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}
$stream = [System.IO.File]::Create((Join-Path $Root "payload/movie.bin"))
try {
    [int64]$written = 0
    while ($written -lt $payloadBytes) {
        $take = [Math]::Min([int64]$block.Length, $payloadBytes - $written)
        $stream.Write($block, 0, [int]$take)
        $written += $take
    }
}
finally { $stream.Dispose() }

$torrent = Join-Path $Root "movie.torrent"
& $bitCli create (Join-Path $Root "payload") --name payload --piece-length 1MiB `
    --no-creation-date --output $torrent --force --json 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }

function Start-Seed([string]$tag, [string]$uploadCap) {
    $arguments = @(
        "seed", $torrent, "--dir", $Root, "--port", "0",
        "--no-dht", "--no-lsd", "--seed-time", "60m", "--json"
    )
    if ($uploadCap) { $arguments += @("--max-upload-rate", $uploadCap) }
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $Root -NoNewWindow -PassThru `
        -ArgumentList $arguments `
        -RedirectStandardOutput (Join-Path $Root "$tag.out") `
        -RedirectStandardError (Join-Path $Root "$tag.err")
    $script:Background += $process
    $deadline = (Get-Date).AddSeconds(60)
    $bound = $null
    while (-not $bound -and (Get-Date) -lt $deadline) {
        if ($process.HasExited) { Exit-With 2 "the seeder exited; see $Root/$tag.err" }
        $bound = (Get-NetTCPConnection -State Listen -OwningProcess $process.Id -ErrorAction SilentlyContinue |
                Select-Object -First 1).LocalPort
        if (-not $bound) { Start-Sleep -Milliseconds 250 }
    }
    if (-not $bound) { Exit-With 2 "the seeder never opened a listening socket" }
    [pscustomobject]@{ process = $process; port = $bound }
}

function Stop-Seed($running) {
    if ($running.process -and -not $running.process.HasExited) {
        Stop-Process -Id $running.process.Id -Force -ErrorAction SilentlyContinue
    }
    $script:Background = @($script:Background | Where-Object { $_.Id -ne $running.process.Id })
    # Windows will not free a port the instant the process dies.
    Start-Sleep -Seconds 1
}

Write-Step "starting the seeder"
$seeder = Start-Seed "seed" $null
$port = $seeder.port
Write-Step "seeder listening on 127.0.0.1:$port"

$commands = [System.Collections.ArrayList]::new()
$results = [System.Collections.ArrayList]::new()

function Invoke-Download([string]$label, [bool]$capped) {
    # A fresh directory per run. Reusing one lets the hash check on add find
    # the payload already there and report a rate that measures the disk.
    $outDir = Join-Path $Root "out-$label"
    if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $stdout = Join-Path $Root "$label.json"
    $arguments = @(
        "download", $torrent, "--dir", $outDir,
        "--peer", "127.0.0.1:$port",
        "--no-dht", "--no-lsd", "--no-tracker", "--no-web-seed",
        "--port", "0", "--json"
    )
    if ($capped) { $arguments += @("--max-download-rate", $Rate) }
    [void]$commands.Add("bit-cli $($arguments -join ' ')")
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $repo -NoNewWindow -PassThru `
        -ArgumentList $arguments -RedirectStandardOutput $stdout `
        -RedirectStandardError (Join-Path $Root "$label.err")
    $finished = $process.WaitForExit($TimeoutSeconds * 1000)
    $clock.Stop()
    if (-not $finished) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        return [pscustomobject]@{ exit_code = 124; elapsed_ms = $clock.ElapsedMilliseconds; bytes = 0; rate = 0 }
    }
    $report = $null
    try { $report = Get-Content $stdout -Raw | ConvertFrom-Json } catch { }
    $bytes = if ($report) { [int64]$report.downloaded.bytes } else { 0 }
    $ms = [Math]::Max(1, $clock.ElapsedMilliseconds)
    [pscustomobject]@{
        exit_code  = $process.ExitCode
        elapsed_ms = $clock.ElapsedMilliseconds
        bytes      = $bytes
        # From the wall clock rather than the report's own mean, so the
        # limiter cannot be measured by the thing it is limiting.
        rate       = [double]$bytes * 1000.0 / $ms
    }
}

for ($run = 1; $run -le $Runs; $run++) {
    # Alternate, so the volume's own state is not always handed to the same
    # side of the pair.
    $order = if ($run % 2 -eq 1) { @($true, $false) } else { @($false, $true) }
    foreach ($capped in $order) {
        $label = if ($capped) { "capped-$run" } else { "uncapped-$run" }
        Write-Step "run $run of $Runs, $label"
        $outcome = Invoke-Download $label $capped
        [void]$results.Add([ordered]@{
            run         = $run
            capped      = $capped
            exit_code   = $outcome.exit_code
            elapsed_ms  = $outcome.elapsed_ms
            bytes       = $outcome.bytes
            bytes_human = Format-Size $outcome.bytes
            rate        = [int64]$outcome.rate
            rate_human  = "$(Format-Size $outcome.rate)/s"
        })
    }
}

# --- the other direction ---------------------------------------------------
#
# `--max-upload-rate` is the same `LimitsConfig` field seen from the other end,
# so it gets the same treatment: cap the seeder and leave the downloader
# uncapped. If the download comes out at the cap, the upload limiter is real.

Write-Step "restarting the seeder with --max-upload-rate $Rate"
Stop-Seed $seeder
$seeder = Start-Seed "seed-capped" $Rate
$port = $seeder.port
$uploadRuns = [System.Collections.ArrayList]::new()
for ($run = 1; $run -le $Runs; $run++) {
    Write-Step "upload cap run $run of $Runs"
    $outcome = Invoke-Download "upcapped-$run" $false
    [void]$uploadRuns.Add([ordered]@{
        run         = $run
        exit_code   = $outcome.exit_code
        elapsed_ms  = $outcome.elapsed_ms
        bytes       = $outcome.bytes
        bytes_human = Format-Size $outcome.bytes
        rate        = [int64]$outcome.rate
        rate_human  = "$(Format-Size $outcome.rate)/s"
    })
}

Stop-Background

$uploadMedian = Get-Median @($uploadRuns | ForEach-Object { [double]$_.rate })
$cappedRates = @($results | Where-Object { $_.capped } | ForEach-Object { [double]$_.rate })
$uncappedRates = @($results | Where-Object { -not $_.capped } | ForEach-Object { [double]$_.rate })
$cappedMedian = Get-Median $cappedRates
$uncappedMedian = Get-Median $uncappedRates
$overBy = if ($rateBytes -gt 0) { ($cappedMedian - $rateBytes) / $rateBytes } else { 0 }

$failures = [System.Collections.ArrayList]::new()
if (@($results | Where-Object { $_.exit_code -ne 0 }).Count -gt 0) {
    [void]$failures.Add("a run did not exit 0")
}
if ($cappedMedian -gt $rateBytes * (1.0 + $Tolerance)) {
    [void]$failures.Add(
        "the capped median $(Format-Size $cappedMedian)/s is $([math]::Round($overBy * 100, 2))% over the $Rate cap")
}
# A cap that is not below what the link does measures nothing.
if ($uncappedMedian -le $rateBytes * 1.5) {
    [void]$failures.Add(
        "the uncapped median $(Format-Size $uncappedMedian)/s is not meaningfully above the cap; lower -Rate")
}
if ($uploadMedian -gt $rateBytes * (1.0 + $Tolerance)) {
    [void]$failures.Add(
        "with the seeder capped at $Rate the download reached $(Format-Size $uploadMedian)/s, so --max-upload-rate did not hold")
}

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "rate-limit-$stamp.json"
$verdict = switch ($true) {
    ($failures.Count -eq 0) {
        "the cap holds: $(Format-Size $cappedMedian)/s against a $Rate cap, and $(Format-Size $uncappedMedian)/s uncapped"
        break
    }
    default { "$($failures.Count) checks did not hold"; break }
}

[ordered]@{
    kind            = "check-rate-limit"
    schema_version  = "1"
    generated_at    = Get-Timestamp
    host            = [ordered]@{
        machine = [System.Environment]::MachineName
        os      = [System.Environment]::OSVersion.VersionString
        cpus    = [System.Environment]::ProcessorCount
    }
    parameters      = [ordered]@{
        rate          = $Rate
        rate_bytes    = $rateBytes
        payload_size  = $PayloadSize
        payload_bytes = $payloadBytes
        runs          = $Runs
        tolerance     = $Tolerance
        profile       = $Profile
    }
    runs            = @($results)
    capped_median   = [int64]$cappedMedian
    capped_human    = "$(Format-Size $cappedMedian)/s"
    uncapped_median = [int64]$uncappedMedian
    uncapped_human  = "$(Format-Size $uncappedMedian)/s"
    upload_capped_median = [int64]$uploadMedian
    upload_capped_human  = "$(Format-Size $uploadMedian)/s"
    upload_runs     = @($uploadRuns)
    over_cap_share  = [math]::Round($overBy, 4)
    ratio           = if ($cappedMedian -gt 0) { [math]::Round($uncappedMedian / $cappedMedian, 3) } else { $null }
    verdict         = $verdict
    failures        = @($failures)
    commands        = @($commands)
    notes           = @(
        "Peers only: --no-web-seed and --no-tracker are set, so the only source is the seeder named with --peer. --web-seed-speed-limit is a different limiter and is measured elsewhere.",
        "The rate is computed from the wall clock and the bytes the report says landed, not from the report's own mean, so the limiter is not measured by the thing it limits.",
        "Each run gets a fresh output directory. Reusing one lets the hash check on add find the payload already there and report a rate that measures the disk.",
        "The pair alternates order between runs, so the volume's own state is not always handed to the same side.",
        "upload_runs is the other direction of the same LimitsConfig field: the seeder is restarted with --max-upload-rate and the downloader runs uncapped, so what bounds the transfer is the seeder's limiter."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "payload: $(Format-Size $payloadBytes), cap $Rate, $Runs paired runs"
Write-Host "report:  $reportPath"
Write-Host ""
$results | ForEach-Object {
    [pscustomobject][ordered]@{
        run    = $_.run
        mode   = if ($_.capped) { "capped" } else { "uncapped" }
        exit   = $_.exit_code
        wall   = "{0:N1}s" -f ($_.elapsed_ms / 1000)
        bytes  = $_.bytes_human
        rate   = $_.rate_human
    }
} | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "with the seeder capped at $Rate and the downloader uncapped: $(Format-Size $uploadMedian)/s"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-rate-limit: $failure") }
    exit 1
}
exit 0
