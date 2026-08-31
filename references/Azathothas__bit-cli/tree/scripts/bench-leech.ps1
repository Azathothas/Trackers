# Separate what a download costs into the pipeline, the hash, and the disk.
#
# `TODO/webseed.md` T-001 measured the whole path and found it reaches about a
# sixth of what `bit-cli`'s own HTTP fetch gets, at both ends of a 24-fold
# bandwidth range. A share that stays roughly constant as the network slows is
# a pipeline-depth limit rather than a per-byte cost, so the next question is
# which of the three the missing five sixths is. This script answers it, by
# taking the same payload from the same server two ways in one session:
#
#   fetch   bit-cli bench webseed. The HTTP path on its own: ranged requests
#           at a fixed concurrency, no bridge, no hashing, no disk.
#   leech   bit-cli bench leech. The whole path, with the counters on: how many
#           blocks the session kept outstanding, how long the bridge took to
#           answer one, how long every piece check took, and how long the
#           writes underneath took.
#
# The gap between the two is what the bridge, the hash, and the disk cost
# together. `bench leech` then says how that gap divides, because those three
# numbers are measured rather than modelled: the hash from the wall time of
# every piece read back and checked, the disk from the positioned writes, and
# the pipeline from the bridge's own view of the session's request window.
#
# Usage:
#   pwsh scripts/bench-leech.ps1
#   pwsh scripts/bench-leech.ps1 -PayloadSize 1GiB -Runs 5 -ConnectionSweep "1,2,4,8"
#   pwsh scripts/bench-leech.ps1 -Mirror https://geo.mirror.pkgbuild.com/iso/2026.08.01/ `
#                                -TorrentUrl https://geo.mirror.pkgbuild.com/iso/2026.08.01/archlinux-2026.08.01-x86_64.iso.torrent
#
# Exits 0 when every stage produced a number, 1 when a stage failed, and 2 when
# the check could not run. The report goes to bench/leech-<timestamp>.json with
# every command, every exit code, and every report the runs wrote.
#
# See TODO/bench.md, T-090.

[CmdletBinding()]
param(
    # Payload to generate for the loopback case. Ignored with -Mirror.
    [string]$PayloadSize = "256MiB",
    # Timed leech runs per connection count. The median is reported with the
    # spread.
    [int]$Runs = 5,
    # How many peer connections the one source is presented over, stepped.
    #
    # Each connection is one peer, and a peer's received blocks are written,
    # hashed, and accounted for one at a time on that connection's own task. So
    # the number of connections is the number of those paths running at once,
    # and stepping it is what separates "the source is slow" from "one peer is
    # as fast as one peer gets".
    [string]$ConnectionSweep = "1,2,4",
    # Ranged requests in flight for the fetch stage, and the per-source
    # concurrency the leech stage's bridge is given.
    [int]$Concurrency = 8,
    # Bytes per ranged request in the fetch stage.
    [string]$RequestSize = "1MiB",
    # A real mirror to measure instead of the loopback server. Needs
    # -TorrentUrl as well.
    [string]$Mirror = "",
    # Where the .torrent for -Mirror comes from.
    [string]$TorrentUrl = "",
    # Working directory. Gitignored.
    [string]$Root = ".tmp/bench-leech",
    # Where the report goes.
    [string]$ReportDir = "bench",
    # Which bit-cli build to drive. A debug build measures a debug build.
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    # How long one leech run gets before it stops and reports what it moved.
    # A loopback run completes long before this; a mirror run is capped by it,
    # which is how the measurement stays time-boxed and stops short of pulling
    # a whole ISO once per run per step.
    [string]$LeechDuration = "600s",
    # Seconds before a single run is abandoned.
    [int]$TimeoutSeconds = 900,
    # Keep the payload and the downloads.
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    Write-Host $message
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "==> $message"
}

function ConvertFrom-Size([string]$text) {
    if ($text -match '^\s*(\d+(?:\.\d+)?)\s*([KMGT]?i?B?)\s*$') {
        $value = [double]$Matches[1]
        switch ($Matches[2].ToUpperInvariant()) {
            "KIB" { return [int64]($value * 1024) }
            "MIB" { return [int64]($value * 1024 * 1024) }
            "GIB" { return [int64]($value * 1024 * 1024 * 1024) }
            "TIB" { return [int64]($value * 1024 * 1024 * 1024 * 1024) }
            default { return [int64]$value }
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

function Format-Rate([double]$bytesPerSecond) {
    "$(Format-Size $bytesPerSecond)/s"
}

function Format-Percent([double]$fraction) {
    "{0:N2}%" -f ($fraction * 100)
}

# ---------------------------------------------------------------------------
# Tools
# ---------------------------------------------------------------------------

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
$fileserver = Join-Path $repo "target/$Profile/examples/loopback-fileserver$exe"
foreach ($required in @($bitCli, $fileserver)) {
    if (-not (Test-Path $required)) {
        Exit-With 2 "missing $required. Build it first: cargo build --workspace --bins --examples --release"
    }
}
if ($Mirror -and -not $TorrentUrl) {
    Exit-With 2 "-Mirror needs -TorrentUrl: the .torrent says which bytes to ask for."
}

# ---------------------------------------------------------------------------
# Workspace
# ---------------------------------------------------------------------------

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path

if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

$script:Background = @()

function Start-Background($name, $path, $arguments) {
    $stdout = Join-Path $Root "$name.out"
    $stderr = Join-Path $Root "$name.err"
    $process = Start-Process -FilePath $path -ArgumentList $arguments `
        -WorkingDirectory $Root -NoNewWindow -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $script:Background += $process
    [pscustomobject]@{ Process = $process; Stdout = $stdout; Stderr = $stderr }
}

function Wait-ForUrl($file, $seconds = 15) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $file) {
            $line = (Get-Content $file -TotalCount 1 -ErrorAction SilentlyContinue)
            if ($line -and $line.Trim()) { return $line.Trim() }
        }
        Start-Sleep -Milliseconds 100
    }
    Exit-With 2 "no URL on stdout of $file after ${seconds}s"
}

function Stop-Background {
    foreach ($process in $script:Background) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $script:Background = @()
}

# Run one process to completion, recording what it cost.
function Invoke-Timed($label, $path, $arguments, $timeout) {
    $stdout = Join-Path $Root "$label.out"
    $stderr = Join-Path $Root "$label.err"
    $startedAt = Get-Timestamp
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $path -ArgumentList $arguments `
        -WorkingDirectory $Root -NoNewWindow -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    if (-not $process.WaitForExit($timeout * 1000)) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $clock.Stop()
        return [pscustomobject]@{
            label = $label; command = "$path $($arguments -join ' ')"
            started_at = $startedAt; elapsed_ms = [int64]$clock.Elapsed.TotalMilliseconds
            exit_code = 124; timed_out = $true; stdout = $stdout; stderr = $stderr
        }
    }
    $clock.Stop()
    [pscustomobject]@{
        label = $label; command = "$path $($arguments -join ' ')"
        started_at = $startedAt; elapsed_ms = [int64]$clock.Elapsed.TotalMilliseconds
        exit_code = $process.ExitCode; timed_out = $false; stdout = $stdout; stderr = $stderr
    }
}

function Get-Stats($values) {
    if (-not $values -or $values.Count -eq 0) { return $null }
    $sorted = $values | Sort-Object
    $min = $sorted[0]
    $max = $sorted[-1]
    $median = $sorted[[int][math]::Floor($sorted.Count / 2)]
    [pscustomobject]@{
        runs = $values.Count; min = $min; median = $median; max = $max
        spread_percent = if ($min -gt 0) { [math]::Round((($max - $min) / $min) * 100, 2) } else { 0 }
    }
}

trap { Stop-Background; throw }

# ---------------------------------------------------------------------------
# Target
# ---------------------------------------------------------------------------

$torrent = ""
$webSeed = ""
$targetKind = ""

if ($Mirror) {
    $targetKind = "mirror"
    Write-Step "fetching the torrent from $TorrentUrl"
    $curl = (Get-Command curl -ErrorAction SilentlyContinue)
    if (-not $curl) { Exit-With 2 "curl not found on PATH, and -Mirror needs it to fetch the .torrent" }
    $torrent = Join-Path $Root "target.torrent"
    $fetch = Invoke-Timed "fetch-torrent" $curl.Source @(
        "-sS", "-L", "-A", "bit-cli-bench/0.1", "-o", $torrent, $TorrentUrl
    ) 120
    if ($fetch.exit_code -ne 0 -or -not (Test-Path $torrent)) {
        Exit-With 2 "could not fetch $TorrentUrl (curl exited $($fetch.exit_code)). $(Get-Content $fetch.stderr -Raw)"
    }
    $webSeed = $Mirror
} else {
    $targetKind = "loopback"
    $payloadBytes = ConvertFrom-Size $PayloadSize
    Write-Step "building a $(Format-Size $payloadBytes) payload"
    $payloadDir = Join-Path $Root "payload"
    New-Item -ItemType Directory -Force -Path $payloadDir | Out-Null
    # Deterministic bytes, written in blocks so a large payload does not need a
    # large allocation. The generator is the ANSI C LCG taking bits 16 to 23,
    # the same one the other scripts here use.
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
    $create = Invoke-Timed "create" $bitCli @(
        "create", "payload", "--name", "payload", "--piece-length", "1MiB",
        "--no-creation-date", "--output", $torrent, "--force", "--json"
    ) 600
    if ($create.exit_code -ne 0) {
        Exit-With 2 "bit-cli create exited $($create.exit_code). $(Get-Content $create.stderr -Raw)"
    }

    $server = Start-Background "fileserver" $fileserver @("--root", $Root)
    $webSeed = Wait-ForUrl $server.Stdout
    Write-Step "web seed at $webSeed"
}

$meta = & $bitCli info $torrent --json | ConvertFrom-Json
$payloadBytes = $meta.total.bytes
$pieceCount = $meta.piece_count
Write-Step "target is $($meta.name), $(Format-Size $payloadBytes), $pieceCount pieces"

$script:commands = @()

# ---------------------------------------------------------------------------
# Stage 1: the HTTP path on its own
# ---------------------------------------------------------------------------
#
# Long enough to be a rate rather than a burst, and no longer: this stage has
# no completion of its own, it reads until the clock runs out.

Write-Step "fetch: bit-cli bench webseed, no bridge, no hashing, no disk"
$fetchReport = Join-Path $Root "fetch.json"
$fetchRun = Invoke-Timed "fetch" $bitCli @(
    "bench", "webseed", $torrent,
    "--web-seed", $webSeed, "--web-seed-only", "--no-torrent-web-seed",
    "--concurrency", "$Concurrency", "--request-size", $RequestSize,
    "--duration", "20s", "--warmup", "3s", "--metrics-interval", "1s",
    "--report", $fetchReport, "--format", "json"
) $TimeoutSeconds
$commands += $fetchRun
if ($fetchRun.exit_code -ne 0) {
    Stop-Background
    Exit-With 1 "bench webseed exited $($fetchRun.exit_code). $(Get-Content $fetchRun.stderr -Raw)"
}
$fetchDoc = Get-Content $fetchReport -Raw | ConvertFrom-Json
$fetchRate = [int64]$fetchDoc.summary.sustained_rate.bytes
Write-Step "  $(Format-Rate $fetchRate)"

# ---------------------------------------------------------------------------
# Stage 2: the whole path, with the counters on
# ---------------------------------------------------------------------------

Write-Step "leech: bit-cli bench leech, bridge, verification, and disk"
$bridgeCounts = @($ConnectionSweep -split ',' | ForEach-Object { [int]$_.Trim() } | Where-Object { $_ -ge 1 })
if ($bridgeCounts.Count -eq 0) { Exit-With 2 "-ConnectionSweep needs at least one count" }

# Every run writes into a directory of its own that did not exist before it.
#
# Reusing one directory and emptying it between runs looks tidier and is a
# trap: Windows keeps a handle on a freshly written payload for a while after
# the process that wrote it exits, the delete fails, and the next run finds a
# complete payload, hash-checks it, and finishes having fetched nothing. That
# is not a slow run, it is no run, and it would report the hash checker's rate.
# `bench leech` refuses to measure a payload that is already there; using a new
# directory each time means it never has to.
#
# The old directories are swept before each run on a best-effort basis and
# again at the end. A sweep that cannot finish costs disk, not correctness.
function Remove-Old($keep) {
    Get-ChildItem -Path $Root -Directory -Filter "leech-out-*" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -ne $keep } |
        ForEach-Object { Remove-Item -Recurse -Force $_.FullName -ErrorAction SilentlyContinue }
}

function Measure-Leech($label, $bridges, $concurrency, $repeatUrl = $false) {
    # --no-torrent-web-seed everywhere, because only the mirror named on the
    # command line is under test. A real torrent carries its own url-list and
    # the Arch Linux ISO's carries 468 entries; left in, the run would spread
    # its requests across all of them and the number would describe the
    # internet rather than the mirror.
    #
    # One source over N connections, which is what --web-seed-connections
    # does: N peers sharing one fetcher, so one window cache and one
    # concurrency budget between them.
    $seedArgs = @("--web-seed", $webSeed, "--web-seed-connections", "$bridges", "--no-torrent-web-seed")
    if ($repeatUrl) {
        # The same URL named N times instead: N separate sources, each with
        # its own fetcher and its own window cache. Same number of peers, and
        # the same window is fetched once per source rather than once. This is
        # the comparison that says what sharing the fetcher is worth.
        $seedArgs = @("--no-torrent-web-seed")
        for ($i = 0; $i -lt $bridges; $i++) { $seedArgs += @("--web-seed", $webSeed) }
    }

    $docs = @()
    $rates = @()
    for ($run = 1; $run -le $Runs; $run++) {
        $out = Join-Path $Root "leech-out-$label-$run"
        Remove-Old $out
        $report = Join-Path $Root "$label-$run.json"
        $result = Invoke-Timed "$label-$run" $bitCli (@(
            "bench", "leech", $torrent, "--dir", $out
        ) + $seedArgs + @(
            "--web-seed-only", "--web-seed-concurrency", "$concurrency",
            "--port", "0",
            "--duration", $LeechDuration, "--warmup", "0s", "--metrics-interval", "250ms",
            "--report", $report, "--format", "json"
        )) $TimeoutSeconds
        $script:commands += $result
        if ($result.exit_code -ne 0) {
            Stop-Background
            Exit-With 1 "bench leech exited $($result.exit_code). $(Get-Content $result.stderr -Raw)"
        }
        $document = Get-Content $report -Raw | ConvertFrom-Json
        $docs += $document
        $rates += [int64]$document.summary.sustained_rate.bytes
        Write-Step "    run $run  $(Format-Rate $document.summary.sustained_rate.bytes)  verify $($document.summary.hashing.total.ms)ms  write $($document.summary.disk.write_time.ms)ms"
    }

    # The median run, not the fastest one. Picking the fastest of seven runs
    # reports the luckiest scheduling rather than what the path does. The
    # fastest and the slowest are both in the report beside it.
    $order = 0..($rates.Count - 1) | Sort-Object { $rates[$_] }
    $middle = $order[[int][math]::Floor($order.Count / 2)]
    [pscustomobject]@{
        bridges = $bridges
        concurrency = $concurrency
        median = $docs[$middle]
        rate = $rates[$middle]
        stats = Get-Stats $rates
    }
}

$curve = @()
foreach ($bridges in $bridgeCounts) {
    Write-Step "  --web-seed-connections $bridges"
    $curve += Measure-Leech "leech-$bridges" $bridges $Concurrency
}

$widest = ($bridgeCounts | Measure-Object -Maximum).Maximum

# The control.
#
# Every extra connection is an extra peer and could also be an extra set of
# HTTP requests in flight, so a sweep on its own cannot say which of the two
# the gain came from. This holds the HTTP concurrency at what the whole sweep
# had and puts it all on one connection. If the rate follows the concurrency
# the gain was HTTP; if it stays where one connection was, the gain was the
# receive paths.
$control = $null
if ($widest -gt 1) {
    Write-Step "  control: 1 connection at $($Concurrency * $widest) requests in flight"
    $control = Measure-Leech "control" 1 ($Concurrency * $widest)
}

# The comparison.
#
# The same number of peers built the other way: the URL named N times, so N
# sources, each with its own fetcher and its own window cache. It answers what
# sharing the fetcher between connections is worth, in rate and in bytes
# pulled off the mirror.
$repeated = $null
if ($widest -gt 1) {
    Write-Step "  comparison: the URL named $widest times, $widest separate sources"
    $repeated = Measure-Leech "repeated" $widest $Concurrency $true
}

Stop-Background

$leech = $curve[-1].median
$leechRate = $curve[-1].rate
$leechStats = $curve[-1].stats
$oneBridge = $curve | Where-Object { $_.bridges -eq 1 } | Select-Object -First 1

# ---------------------------------------------------------------------------
# Attribution
# ---------------------------------------------------------------------------

$wallMs = [int64]$leech.summary.duration.ms
$verifyMs = if ($leech.summary.hashing) { [int64]$leech.summary.hashing.total.ms } else { 0 }
$writeMs = if ($leech.summary.disk) { [int64]$leech.summary.disk.write_time.ms } else { 0 }
$readMs = if ($leech.summary.disk) { [int64]$leech.summary.disk.read_time.ms } else { 0 }

# A block is written, and at a piece boundary read back and hashed, inline on
# the connection that received it. So those times are per receive path and add
# up across paths: with four bridges the writes can total more than the wall
# clock without anything being wrong. The budget they come out of is the wall
# time multiplied by the number of paths, and that is what they are a share of.
$paths = [int]$curve[-1].bridges
$pathMs = $wallMs * [math]::Max($paths, 1)
$share = { param($ms) if ($pathMs -gt 0) { $ms / $pathMs } else { 0 } }

$pipeline = $leech.summary.pipeline

Write-Host ""
Write-Host "Stage                              median       slowest      fastest      share of fetch"
Write-Host "---------------------------------------------------------------------------------------"
Write-Host ("{0,-34} {1,-12} {2,-12} {3,-12} {4}" -f "fetch, no bridge", (Format-Rate $fetchRate), "", "", "100.00%")
foreach ($step in $curve) {
    Write-Host ("{0,-34} {1,-12} {2,-12} {3,-12} {4}" -f "leech, $($step.bridges) connection(s)",
        (Format-Rate $step.rate), (Format-Rate $step.stats.min), (Format-Rate $step.stats.max),
        (Format-Percent ($step.rate / [math]::Max($fetchRate, 1))))
}
if ($control) {
    Write-Host ("{0,-34} {1,-12} {2,-12} {3,-12} {4}" -f "control: 1 conn, $($control.concurrency) HTTP",
        (Format-Rate $control.rate), (Format-Rate $control.stats.min), (Format-Rate $control.stats.max),
        (Format-Percent ($control.rate / [math]::Max($fetchRate, 1))))
}
if ($repeated) {
    Write-Host ("{0,-34} {1,-12} {2,-12} {3,-12} {4}" -f "URL named $widest times",
        (Format-Rate $repeated.rate), (Format-Rate $repeated.stats.min), (Format-Rate $repeated.stats.max),
        (Format-Percent ($repeated.rate / [math]::Max($fetchRate, 1))))
}
if ($oneBridge -and $curve.Count -gt 1) {
    Write-Host ""
    Write-Host "Scaling against one connection"
    Write-Host "---------------------------------------------------------------------"
    foreach ($step in $curve) {
        Write-Host ("{0,-34} {1}" -f "$($step.bridges) connection(s)",
            ("{0:N2}x" -f ($step.rate / [math]::Max($oneBridge.rate, 1))))
    }
    if ($control) {
        Write-Host ("{0,-34} {1}" -f "1 connection, $($control.concurrency) HTTP",
            ("{0:N2}x" -f ($control.rate / [math]::Max($oneBridge.rate, 1))))
    }
    if ($repeated) {
        Write-Host ("{0,-34} {1}" -f "URL named $widest times",
            ("{0:N2}x" -f ($repeated.rate / [math]::Max($oneBridge.rate, 1))))
    }
}

# What each form pulled off the mirror to move the payload once. Sources
# sharing a fetcher share its window cache; separate sources at the same URL
# do not, and fetch the same window once each.
$amplification = {
    param($step)
    if (-not $step) { return $null }
    $served = 0
    $http = 0
    foreach ($row in $step.median.sources) {
        $served += [int64]$row.bytes.bytes
        if ($row.http_bytes) { $http += [int64]$row.http_bytes.bytes }
    }
    if ($served -le 0) { return $null }
    [pscustomobject]@{ served = $served; http = $http; ratio = [math]::Round($http / $served, 3) }
}
$widestAmp = & $amplification ($curve | Where-Object { $_.bridges -eq $widest } | Select-Object -First 1)
$repeatedAmp = & $amplification $repeated
if ($widestAmp -and $repeatedAmp) {
    Write-Host ""
    Write-Host "Bytes pulled off the mirror to move the payload once"
    Write-Host "---------------------------------------------------------------------"
    Write-Host ("{0,-34} {1,-14} {2}" -f "--web-seed-connections $widest", (Format-Size $widestAmp.http), "$($widestAmp.ratio)x")
    Write-Host ("{0,-34} {1,-14} {2}" -f "URL named $widest times", (Format-Size $repeatedAmp.http), "$($repeatedAmp.ratio)x")
}
Write-Host ""
Write-Host "Where the time went, at $paths receive path$(if ($paths -ne 1) { 's' })"
Write-Host "-------------------------------------------------------------"
Write-Host ("{0,-34} {1,-12} {2}" -f "wall", "${wallMs}ms", "")
Write-Host ("{0,-34} {1,-12} {2}" -f "path time available", "${pathMs}ms", "100.00%")
Write-Host ("{0,-34} {1,-12} {2}" -f "piece checks (read plus hash)", "${verifyMs}ms", (Format-Percent (& $share $verifyMs)))
Write-Host ("{0,-34} {1,-12} {2}" -f "  of which reading them back", "${readMs}ms", (Format-Percent (& $share $readMs)))
Write-Host ("{0,-34} {1,-12} {2}" -f "writing the payload", "${writeMs}ms", (Format-Percent (& $share $writeMs)))
Write-Host ""
if ($pipeline) {
    Write-Host "The block request pipeline"
    Write-Host "-------------------------------------------------------------"
    Write-Host ("{0,-34} {1}" -f "blocks outstanding at peak", $pipeline.peak_in_flight)
    Write-Host ("{0,-34} {1}" -f "blocks outstanding on average", $pipeline.mean_in_flight)
    Write-Host ("{0,-34} {1}" -f "mean time to answer one", "$($pipeline.mean_service_us)us")
    Write-Host ("{0,-34} {1}" -f "block size", (Format-Size $pipeline.block_size.bytes))
    Write-Host ("{0,-34} {1}" -f "what that peak depth allows", (Format-Rate $pipeline.window_ceiling.bytes))
    Write-Host ("{0,-34} {1}" -f "  measured, as a share of it", (Format-Percent ($leechRate / [math]::Max($pipeline.window_ceiling.bytes, 1))))
    Write-Host ""
}

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "leech-$stamp.json"
$report = [ordered]@{
    schema_version = "1"
    kind = "bench-leech"
    generated_at = Get-Timestamp
    bit_cli_version = ($leech.environment.build.version)
    build = $leech.environment.build
    host = $leech.environment.host
    target = [ordered]@{
        kind = $targetKind
        name = $meta.name
        info_hash = $meta.info_hash
        total_bytes = $payloadBytes
        piece_count = $pieceCount
        piece_length_bytes = $meta.piece_length.bytes
        web_seed = $webSeed
    }
    parameters = [ordered]@{
        runs = $Runs
        connection_sweep = $bridgeCounts
        leech_duration = $LeechDuration
        concurrency = $Concurrency
        request_size = $RequestSize
        profile = $Profile
    }
    stages = [ordered]@{
        fetch = [ordered]@{
            what = "bit-cli bench webseed: the HTTP path, no bridge, no hashing, no disk"
            sustained_rate_bytes = $fetchRate
            sustained_rate_human = Format-Rate $fetchRate
            report = $fetchDoc.summary
        }
        leech = [ordered]@{
            what = "bit-cli bench leech: HTTP fetch, loopback bridge, piece verification, disk"
            sustained_rate_bytes = $leechRate
            sustained_rate_human = Format-Rate $leechRate
            share_of_fetch = Format-Percent ($leechRate / [math]::Max($fetchRate, 1))
            wall_stats_ms = $leechStats
            report = $leech.summary
        }
    }
    connection_curve = @($curve | ForEach-Object {
        [ordered]@{
            connections = $_.bridges
            web_seed_concurrency = $_.concurrency
            sustained_rate_bytes = $_.rate
            sustained_rate_human = Format-Rate $_.rate
            share_of_fetch = Format-Percent ($_.rate / [math]::Max($fetchRate, 1))
            speedup_over_one_connection = if ($oneBridge) { [math]::Round($_.rate / [math]::Max($oneBridge.rate, 1), 3) } else { $null }
            rate_stats_bytes = $_.stats
            amplification = & $amplification $_
            summary = $_.median.summary
            sources = $_.median.sources
        }
    })
    control = if ($control) {
        [ordered]@{
            what = "one connection carrying the same total HTTP concurrency as the widest step, so the sweep's gain can be attributed to the receive paths rather than to the requests in flight"
            connections = 1
            web_seed_concurrency = $control.concurrency
            sustained_rate_bytes = $control.rate
            sustained_rate_human = Format-Rate $control.rate
            speedup_over_one_connection = if ($oneBridge) { [math]::Round($control.rate / [math]::Max($oneBridge.rate, 1), 3) } else { $null }
            rate_stats_bytes = $control.stats
            summary = $control.median.summary
        }
    } else { $null }
    repeated_url = if ($repeated) {
        [ordered]@{
            what = "the same URL named N times, so N separate sources with N fetchers and N window caches, against --web-seed-connections N which is N connections sharing one"
            connections = $repeated.bridges
            web_seed_concurrency = $repeated.concurrency
            sustained_rate_bytes = $repeated.rate
            sustained_rate_human = Format-Rate $repeated.rate
            speedup_over_one_connection = if ($oneBridge) { [math]::Round($repeated.rate / [math]::Max($oneBridge.rate, 1), 3) } else { $null }
            rate_stats_bytes = $repeated.stats
            amplification = $repeatedAmp
            summary = $repeated.median.summary
            sources = $repeated.median.sources
        }
    } else { $null }
    attribution = [ordered]@{
        bridges = $paths
        wall_ms = $wallMs
        path_time_ms = $pathMs
        verify_ms = $verifyMs
        verify_share = Format-Percent (& $share $verifyMs)
        disk_read_ms = $readMs
        disk_read_share = Format-Percent (& $share $readMs)
        disk_write_ms = $writeMs
        disk_write_share = Format-Percent (& $share $writeMs)
        notes = @(
            "verify_ms covers reading each piece back and hashing it, so disk_read_ms is inside it rather than beside it",
            "path_time_ms is wall_ms times the number of bridges. A block is written, and at a piece boundary read back and hashed, inline on the connection that received it, so those times are per receive path and add up across paths."
        )
    }
    commands = $commands
    notes = @(
        "The page cache is not dropped between stages. Windows has no supported way to do it, and both stages read the same file through the same server, so the cache helps each of them equally.",
        "The fetch stage runs for a fixed duration; the leech stage runs until the torrent completes. A leech that finishes in under a second is a burst rather than a rate, which is why the payload defaults to 256 MiB.",
        "peak_in_flight covers the whole run including any warmup, because a high-water mark cannot be narrowed to a window after the fact.",
        "The same URL attached N times is N bindings and so N bridges. The per-source rows add up to the payload rather than to N copies of it, which is what says the pieces were divided rather than fetched twice.",
        "The attribution block is read off the last step of the bridge sweep."
    )
}
$report | ConvertTo-Json -Depth 12 | Set-Content -Path $reportPath -Encoding utf8NoBOM
Write-Step "report written to $reportPath"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
exit 0
