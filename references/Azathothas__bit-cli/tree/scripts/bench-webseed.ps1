# Measure the web seed path against a raw curl ceiling.
#
# `bit-cli` presents an HTTP source to the torrent session as a peer over a
# loopback TCP connection. That costs a round trip, a second copy of every
# byte, peer protocol framing, and a peer slot. This script measures what it
# costs, by taking the same payload from the same server four ways in the same
# session on the same machine:
#
#   serial    curl, one connection, one request for the whole file. What a
#             single stream gets, and nothing else.
#   parallel  curl, N connections, one contiguous slice each. The ceiling the
#             two bit-cli stages are compared against, because they open N
#             connections too. Comparing eight connections against one and
#             calling the ratio an overhead would be wrong in bit-cli's favour,
#             which is the wrong direction to be wrong in.
#   fetch     bit-cli bench webseed. bit-cli's own HTTP fetch path, ranged, at
#             the same concurrency, with no bridge, no hashing, and no disk.
#   download  bit-cli download --web-seed-only. The whole path: HTTP fetch,
#             loopback bridge, peer protocol framing, piece verification, and
#             the write to disk.
#
# The gap between `parallel` and `fetch` is what bit-cli's HTTP client costs.
# The gap between `fetch` and `download` is what the bridge, the hashing, and
# the disk cost together. Reporting one ratio would say "slower" without
# saying where, which is not a result anybody can act on.
#
# Two failure cases run after the timed ones, because a number taken from a
# healthy mirror says nothing about what happens to an unhealthy one:
#
#   stall     the server sends part of a response and then stops without
#             closing. The run has to end, not hang.
#   416       the server refuses every range. The run has to fail fast and
#             name the reason.
#
# Usage:
#   pwsh scripts/bench-webseed.ps1
#   pwsh scripts/bench-webseed.ps1 -PayloadSize 512MiB -Runs 7
#   pwsh scripts/bench-webseed.ps1 -Mirror https://geo.mirror.pkgbuild.com/iso/2026.08.01/ `
#                                  -TorrentUrl https://geo.mirror.pkgbuild.com/iso/2026.08.01/archlinux-2026.08.01-x86_64.iso.torrent
#
# Exits 0 when every stage produced a number, 1 when a stage failed, and 2 when
# the check could not run. The report goes to bench/webseed-<timestamp>.json
# with every command, every exit code, and every timing.
#
# See TODO/webseed.md, T-001.

[CmdletBinding()]
param(
    # Payload to generate for the loopback case. Ignored with -Mirror.
    [string]$PayloadSize = "256MiB",
    # Timed runs per stage. The minimum is reported along with the spread,
    # because the minimum is the least contaminated by everything else on the
    # machine and the spread says how much to trust it.
    [int]$Runs = 5,
    # Ranged requests in flight, held the same across every stage that has a
    # concurrency to hold.
    [int]$Concurrency = 8,
    # Bytes per ranged request.
    [string]$RequestSize = "1MiB",
    # A real mirror to measure instead of the loopback server. Needs
    # -TorrentUrl as well.
    [string]$Mirror = "",
    # Where the .torrent for -Mirror comes from.
    [string]$TorrentUrl = "",
    # Working directory. Gitignored.
    [string]$Root = ".tmp/bench-webseed",
    # Where the report goes.
    [string]$ReportDir = "bench",
    # Which bit-cli build to drive. A debug build measures a debug build.
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    # Seconds before a single run is abandoned.
    [int]$TimeoutSeconds = 600,
    # Keep the payload and the downloads.
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("bench-webseed: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

# Binary units, the same ones bit-cli parses.
function ConvertFrom-Size([string]$text) {
    if ($text -match '^\s*([0-9]+(?:\.[0-9]+)?)\s*([A-Za-z]*)\s*$') {
        $value = [double]$Matches[1]
        $unit = $Matches[2].ToLower()
        $scale = switch ($unit) {
            ''    { 1 }
            'b'   { 1 }
            'k'   { 1KB } 'kib' { 1KB }
            'm'   { 1MB } 'mib' { 1MB }
            'g'   { 1GB } 'gib' { 1GB }
            default { Exit-With 2 "cannot parse size '$text'" }
        }
        return [int64]($value * $scale)
    }
    Exit-With 2 "cannot parse size '$text'"
}

function Format-Size([double]$bytes) {
    if ($bytes -ge 1GB) { return "{0:N2} GiB" -f ($bytes / 1GB) }
    if ($bytes -ge 1MB) { return "{0:N2} MiB" -f ($bytes / 1MB) }
    if ($bytes -ge 1KB) { return "{0:N2} KiB" -f ($bytes / 1KB) }
    "{0} B" -f [int64]$bytes
}

function Format-Rate([double]$bytesPerSecond) {
    "$(Format-Size $bytesPerSecond)/s"
}

# ---------------------------------------------------------------------------
# Tools
# ---------------------------------------------------------------------------

$exe = if ($IsWindows) { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
$fileserver = Join-Path $repo "target/$Profile/examples/loopback-fileserver$exe"
foreach ($required in @($bitCli, $fileserver)) {
    if (-not (Test-Path $required)) {
        Exit-With 2 "missing $required. Build it first: cargo build --workspace --bins --examples"
    }
}
$curl = (Get-Command curl -ErrorAction SilentlyContinue)
if (-not $curl) {
    Exit-With 2 "curl not found on PATH. It is the ceiling this measures against."
}
$curlPath = $curl.Source
$curlVersion = (& $curlPath --version 2>&1 | Select-Object -First 1)

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
#
# PeakWorkingSet64 and TotalProcessorTime come off the process handle that
# -PassThru keeps open, so they are the operating system's own figures for the
# child rather than anything the child reports about itself.
function Invoke-Timed($label, $path, $arguments, $timeout) {
    $stdout = Join-Path $Root "$label.out"
    $stderr = Join-Path $Root "$label.err"
    $startedAt = Get-Timestamp
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $path -ArgumentList $arguments `
        -WorkingDirectory $Root -NoNewWindow -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $finished = $process.WaitForExit($timeout * 1000)
    $clock.Stop()
    if (-not $finished) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        return [pscustomobject]@{
            label = $label; command = "$path $($arguments -join ' ')"
            started_at = $startedAt; exit_code = $null; timed_out = $true
            elapsed_ms = $clock.ElapsedMilliseconds
            peak_rss_bytes = $null; cpu_ms = $null
            stdout = $stdout; stderr = $stderr
        }
    }
    $peak = $null
    $cpu = $null
    try { $peak = $process.PeakWorkingSet64 } catch {}
    try { $cpu = [int64]$process.TotalProcessorTime.TotalMilliseconds } catch {}
    [pscustomobject]@{
        label = $label; command = "$path $($arguments -join ' ')"
        started_at = $startedAt; exit_code = $process.ExitCode; timed_out = $false
        elapsed_ms = $clock.ElapsedMilliseconds
        peak_rss_bytes = $peak; cpu_ms = $cpu
        stdout = $stdout; stderr = $stderr
    }
}

# The minimum, the median, and the spread of a series of run times.
#
# The minimum is the least contaminated by everything else on the machine. The
# spread is what says how much to trust it: a 5 percent spread is a
# measurement, a 60 percent spread is a machine doing something else.
function Get-Stats($values) {
    if (-not $values -or $values.Count -eq 0) { return $null }
    $sorted = @($values | Sort-Object)
    $min = $sorted[0]
    $max = $sorted[-1]
    $median = if ($sorted.Count % 2 -eq 1) {
        $sorted[[int](($sorted.Count - 1) / 2)]
    } else {
        ($sorted[$sorted.Count / 2 - 1] + $sorted[$sorted.Count / 2]) / 2
    }
    $mean = ($sorted | Measure-Object -Average).Average
    [pscustomobject]@{
        runs = $sorted.Count
        min = [int64]$min
        median = [double]$median
        mean = [double]$mean
        max = [int64]$max
        spread = [int64]($max - $min)
        spread_percent = if ($min -gt 0) { [math]::Round((($max - $min) / $min) * 100, 2) } else { $null }
        values = @($sorted)
    }
}

# ---------------------------------------------------------------------------
# Target
# ---------------------------------------------------------------------------

$payloadBytes = 0
$torrent = ""
$webSeed = ""
$fileUrl = ""
$targetKind = ""

if ($Mirror) {
    $targetKind = "mirror"
    Write-Step "fetching the torrent from $TorrentUrl"
    $torrent = Join-Path $Root "target.torrent"
    $fetch = Invoke-Timed "fetch-torrent" $curlPath @(
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
    # the same one interop-roundtrip.ps1 uses.
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
    ) 300
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

# The single URL curl reads, resolved by bit-cli itself so the ceiling and the
# candidate ask for exactly the same resource. Asking curl for a URL nobody
# else uses would compare two different things.
# Only the mirror named on the command line is measured. A real torrent
# carries its own url-list, and the Arch Linux ISO's carries 468 entries: left
# in, the download stage would spread its requests across all of them and the
# number would describe the internet rather than the mirror under test.
$onlyThisMirror = @("--web-seed", $webSeed, "--no-torrent-web-seed")

$listing = & $bitCli webseed list $torrent @onlyThisMirror --json | ConvertFrom-Json
$fileUrl = $listing.sources[0].urls[0].url
if (-not $fileUrl) { Exit-With 2 "bit-cli webseed list resolved no URL for $webSeed" }
Write-Step "reading from $fileUrl"

# ---------------------------------------------------------------------------
# Stages
# ---------------------------------------------------------------------------

$steps = [System.Collections.ArrayList]@()
$failures = [System.Collections.ArrayList]@()

# Both curl stages throw the payload away, because the two bit-cli stages they
# are the ceiling for do too. A ceiling that writes to disk while the thing
# measured against it does not is not a ceiling, it is a different question.
# The `download` stage does write to disk, and that cost lands in the gap
# between `fetch` and `download` where it belongs.
$nullDevice = if ($IsWindows) { "NUL" } else { "/dev/null" }

# One warm run before anything is timed, for the loopback case only. The
# payload was written moments ago and the server has never been asked for it,
# so the first read pays for both. Against a real mirror there is no local
# cache to warm and the mirror's is not ours to warm, so a warm-up would only
# spend somebody else's bandwidth.
if (-not $Mirror) {
    Write-Step "warming the server and the page cache"
    $null = & $curlPath -sS --http1.1 -o $nullDevice $fileUrl 2>&1
}

# 1a. curl, one connection, the whole file. What a single stream gets.
Write-Step "stage 1 of 4: curl ceiling, one connection, $Runs runs"
$serialRuns = @()
$serialCost = @()
for ($run = 1; $run -le $Runs; $run++) {
    $result = Invoke-Timed "curl-serial-$run" $curlPath @(
        "-sS", "--http1.1", "-o", $nullDevice, $fileUrl
    ) $TimeoutSeconds
    [void]$steps.Add($result)
    if ($result.exit_code -ne 0) {
        [void]$failures.Add("curl run $run exited $($result.exit_code)")
        break
    }
    $serialRuns += $result.elapsed_ms
    if ($null -ne $result.cpu_ms) { $serialCost += $result.cpu_ms }
    Write-Step "  run $run  $($result.elapsed_ms) ms  $(Format-Rate ($payloadBytes / ($result.elapsed_ms / 1000)))"
}
$serial = Get-Stats $serialRuns
$serialRate = if ($serial) { [int64]($payloadBytes / ($serial.min / 1000)) } else { 0 }

# 1b. curl, N connections, one contiguous slice each. The fair ceiling.
#
# The two bit-cli stages both open $Concurrency connections, and a single
# stream is not the same question. Comparing eight connections against one and
# calling the ratio an overhead would be wrong in bit-cli's favour, which is
# the wrong direction to be wrong in.
#
# One curl process with `--parallel`, not N curl processes. Starting eight
# processes on Windows costs more than the transfer does at these sizes, and a
# ceiling that measures process startup is not a ceiling. The ranges go in a
# config file because a command line with N `--next` blocks is unreadable.
Write-Step "stage 2 of 4: curl ceiling, $Concurrency connections, $Runs runs"
$sliceBytes = [int64][math]::Ceiling($payloadBytes / $Concurrency)
$curlConfig = Join-Path $Root "curl-parallel.cfg"
$blocks = @()
for ($slice = 0; $slice -lt $Concurrency; $slice++) {
    $from = $slice * $sliceBytes
    $to = [math]::Min($from + $sliceBytes, $payloadBytes) - 1
    if ($from -gt $to) { continue }
    $blocks += "--range $from-$to`n--output $nullDevice`n--url $fileUrl"
}
Set-Content -Path $curlConfig -Value ($blocks -join "`n--next`n") -Encoding utf8NoBOM

$parallelRuns = @()
$parallelCost = @()
for ($run = 1; $run -le $Runs; $run++) {
    $result = Invoke-Timed "curl-parallel-$run" $curlPath @(
        "-sS", "--http1.1", "-Z", "--parallel-max", $Concurrency, "-K", $curlConfig
    ) $TimeoutSeconds
    [void]$steps.Add($result)
    if ($result.exit_code -ne 0) {
        [void]$failures.Add("parallel curl run $run exited $($result.exit_code)")
        break
    }
    $parallelRuns += $result.elapsed_ms
    if ($null -ne $result.cpu_ms) { $parallelCost += $result.cpu_ms }
    Write-Step "  run $run  $($result.elapsed_ms) ms  $(Format-Rate ($payloadBytes / ($result.elapsed_ms / 1000)))"
}
$parallel = Get-Stats $parallelRuns
$parallelRate = if ($parallel) { [int64]($payloadBytes / ($parallel.min / 1000)) } else { 0 }

# The fair ceiling is the parallel one, because that is the shape both bit-cli
# stages run in.
$ceiling = $parallel
$ceilingRate = $parallelRate

# 2. bit-cli's own HTTP fetch path, no bridge.
Write-Step "stage 3 of 4: bit-cli bench webseed, $Runs runs"
$fetchRuns = @()
$fetchReports = @()
for ($run = 1; $run -le $Runs; $run++) {
    $reportPath = Join-Path $Root "fetch-$run.json"
    # The duration is set from the ceiling so this stage moves roughly the same
    # number of bytes as one curl run, rather than running for a fixed wall
    # time that has nothing to do with the payload.
    $seconds = [math]::Max(3, [math]::Ceiling(($ceiling.min / 1000) * 3))
    $result = Invoke-Timed "fetch-$run" $bitCli @(
        "bench", "webseed", $torrent,
        "--web-seed", $webSeed, "--no-torrent-web-seed",
        "--web-seed-only",
        "--duration", "${seconds}s", "--warmup", "1s",
        "--concurrency", $Concurrency,
        "--request-size", $RequestSize,
        "--metrics-interval", "250ms",
        "--report", $reportPath, "--format", "json",
        "--quiet"
    ) $TimeoutSeconds
    [void]$steps.Add($result)
    if ($result.exit_code -ne 0 -or -not (Test-Path $reportPath)) {
        [void]$failures.Add("bench webseed run $run exited $($result.exit_code)")
        break
    }
    $report = Get-Content $reportPath -Raw | ConvertFrom-Json
    $fetchReports += $report
    $fetchRuns += $report.summary.sustained_rate.bytes
    Write-Step "  run $run  $($report.summary.sustained_rate.human)/s  $($report.summary.requests) requests, $($report.summary.errors.total) failed"
}
$fetchRate = if ($fetchRuns.Count -gt 0) { ($fetchRuns | Measure-Object -Maximum).Maximum } else { 0 }

# 3. The whole path: fetch, bridge, verify, write.
Write-Step "stage 4 of 4: bit-cli download --web-seed-only, $Runs runs"
$downloadRuns = @()
$downloadCost = @()
$downloadRss = @()
$downloadHandles = @()
$firstPieceMs = @()
$bridgeBytes = @()
$bridgeBlocks = @()
for ($run = 1; $run -le $Runs; $run++) {
    $runDir = Join-Path $Root "out-$run"
    if (Test-Path $runDir) { Remove-Item -Recurse -Force $runDir }
    New-Item -ItemType Directory -Force -Path $runDir | Out-Null
    $result = Invoke-Timed "download-$run" $bitCli @(
        "download", $torrent,
        "--web-seed", $webSeed, "--no-torrent-web-seed",
        "--web-seed-only",
        "--dir", $runDir,
        "--web-seed-concurrency", $Concurrency,
        "--web-seed-chunk-size", $RequestSize,
        "--report-interval", "100ms",
        "--jsonl"
    ) $TimeoutSeconds
    [void]$steps.Add($result)
    if ($result.exit_code -ne 0) {
        [void]$failures.Add("download run $run exited $($result.exit_code)")
        break
    }
    $events = @(Get-Content $result.stdout | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json })
    $sessionStart = $events | Where-Object { $_.type -eq 'session_start' } | Select-Object -First 1
    $firstVerified = $events | Where-Object { $_.type -eq 'piece_verified' } | Select-Object -First 1
    $document = $events | Where-Object { $_.kind -eq 'download' } | Select-Object -First 1
    if (-not $document -or -not $document.torrents[0].finished) {
        [void]$failures.Add("download run $run did not finish")
        break
    }
    $downloadRuns += $result.elapsed_ms
    # The process reports its own cost. Reading PeakWorkingSet64 off the
    # handle after the child has exited gives zero, because the working set is
    # gone by then; only the process itself can report its high-water mark.
    if ($document.process) {
        $downloadCost += $document.process.cpu_ms
        $downloadRss += $document.process.peak_rss_bytes
        $downloadHandles += $document.process.open_handles
    }
    if ($sessionStart -and $firstVerified) {
        $firstPieceMs += [int64](([datetime]$firstVerified.at) - ([datetime]$sessionStart.at)).TotalMilliseconds
    }
    # Summed across sources, not taken from the first: the ratio has to cover
    # every byte that crossed the bridge, and a run with more than one source
    # would understate it otherwise.
    $source = [pscustomobject]@{
        served_bytes = ($document.torrents[0].sources | Measure-Object -Property served_bytes -Sum).Sum
        blocks = ($document.torrents[0].sources | Measure-Object -Property blocks -Sum).Sum
    }
    if ($source) {
        $bridgeBytes += $source.served_bytes
        $bridgeBlocks += $source.blocks
    }
    Write-Step "  run $run  $($result.elapsed_ms) ms  $(Format-Rate ($payloadBytes / ($result.elapsed_ms / 1000)))  peak RSS $(Format-Size $document.process.peak_rss_bytes)"
}
$download = Get-Stats $downloadRuns
$downloadRate = if ($download) { [int64]($payloadBytes / ($download.min / 1000)) } else { 0 }

# Bytes over the loopback socket per payload byte.
#
# Every block the bridge hands the session crosses loopback inside a BEP 3
# `piece` message: a four byte length prefix, a one byte id, a four byte piece
# index, and a four byte offset, so thirteen bytes of framing per block on top
# of the payload. The handshake and the bitfield are one-off and are not
# counted; on any payload worth measuring they round to nothing.
$framingPerBlock = 13
$loopbackBytes = $null
$loopbackRatio = $null
if ($bridgeBytes.Count -gt 0) {
    $served = ($bridgeBytes | Measure-Object -Maximum).Maximum
    $blocks = ($bridgeBlocks | Measure-Object -Maximum).Maximum
    $loopbackBytes = [int64]($served + $blocks * $framingPerBlock)
    $loopbackRatio = [math]::Round($loopbackBytes / $payloadBytes, 6)
}

# ---------------------------------------------------------------------------
# Failure cases
# ---------------------------------------------------------------------------

$failureCases = [System.Collections.ArrayList]@()

if (-not $Mirror) {
    Write-Step "failure case: a source that stalls mid transfer"
    $stallServer = Start-Background "fileserver-stall" $fileserver @(
        "--root", $Root, "--stall-after", "65536", "--fail-after", "2"
    )
    $stallUrl = Wait-ForUrl $stallServer.Stdout
    $runDir = Join-Path $Root "out-stall"
    New-Item -ItemType Directory -Force -Path $runDir | Out-Null
    $stall = Invoke-Timed "download-stall" $bitCli @(
        "download", $torrent, "--web-seed", $stallUrl, "--web-seed-only",
        "--dir", $runDir, "--web-seed-timeout", "5s",
        "--web-seed-concurrency", "2", "--timeout", "45s", "--json"
    ) 90
    [void]$steps.Add($stall)
    [void]$failureCases.Add([pscustomobject]@{
        case = "stall"
        description = "the server sends 64 KiB of a response and then stops without closing"
        exit_code = $stall.exit_code
        timed_out = $stall.timed_out
        elapsed_ms = $stall.elapsed_ms
        ended = -not $stall.timed_out
    })
    Write-Step "  exited $($stall.exit_code) after $($stall.elapsed_ms) ms (timed out: $($stall.timed_out))"

    Write-Step "failure case: a source that refuses every range with 416"
    $refuseServer = Start-Background "fileserver-416" $fileserver @(
        "--root", $Root, "--status", "416"
    )
    $refuseUrl = Wait-ForUrl $refuseServer.Stdout
    $runDir = Join-Path $Root "out-416"
    New-Item -ItemType Directory -Force -Path $runDir | Out-Null
    $refuse = Invoke-Timed "download-416" $bitCli @(
        "download", $torrent, "--web-seed", $refuseUrl, "--web-seed-only",
        "--dir", $runDir, "--timeout", "45s", "--json"
    ) 90
    [void]$steps.Add($refuse)
    [void]$failureCases.Add([pscustomobject]@{
        case = "416"
        description = "the server answers every ranged request with 416"
        exit_code = $refuse.exit_code
        timed_out = $refuse.timed_out
        elapsed_ms = $refuse.elapsed_ms
        ended = -not $refuse.timed_out
    })
    Write-Step "  exited $($refuse.exit_code) after $($refuse.elapsed_ms) ms (timed out: $($refuse.timed_out))"
}

Stop-Background

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

$environment = if ($fetchReports.Count -gt 0) { $fetchReports[0].environment } else { $null }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "webseed-$stamp.json"

# The fastest link this machine has, so a rate can be read as "would this
# saturate the wire" rather than only as a share of a loopback ceiling.
$linkBps = 0
$linkHuman = $null
if ($environment -and $environment.host.network) {
    $fastest = $environment.host.network | Sort-Object -Property link_speed_bps -Descending | Select-Object -First 1
    if ($fastest) {
        $linkBps = [int64]$fastest.link_speed_bps
        $linkHuman = "$($fastest.name) at $($fastest.link_speed_human)"
    }
}

$share = {
    param($rate)
    if ($ceilingRate -gt 0 -and $rate -gt 0) {
        "{0:N2}%" -f (($rate / $ceilingRate) * 100)
    } else { $null }
}

$report = [ordered]@{
    schema_version = "1"
    kind = "bench_webseed_comparison"
    todo = "T-001"
    generated_at = Get-Timestamp
    bit_cli_version = $meta.bit_cli_version
    environment = $environment
    tool_versions = [ordered]@{
        curl = $curlVersion
        bit_cli = "$(& $bitCli --version)"
    }
    target = [ordered]@{
        kind = $targetKind
        name = $meta.name
        info_hash = $meta.info_hash
        total_bytes = $payloadBytes
        total_human = Format-Size $payloadBytes
        piece_count = $pieceCount
        web_seed = $webSeed
        file_url = $fileUrl
    }
    parameters = [ordered]@{
        runs = $Runs
        concurrency = $Concurrency
        request_size = $RequestSize
        profile = $Profile
    }
    stages = [ordered]@{
        ceiling_serial = [ordered]@{
            what = "curl, one connection, one request for the whole file"
            wall_ms = $serial
            rate_bytes_per_sec = $serialRate
            rate_human = Format-Rate $serialRate
            cpu_ms = Get-Stats $serialCost
        }
        ceiling_parallel = [ordered]@{
            what = "curl, $Concurrency connections, one contiguous slice each. The ceiling the bit-cli stages are compared against, because they run in the same shape."
            connections = $Concurrency
            slice_bytes = $sliceBytes
            wall_ms = $parallel
            rate_bytes_per_sec = $parallelRate
            rate_human = Format-Rate $parallelRate
            cpu_ms = Get-Stats $parallelCost
            speedup_over_serial = if ($serialRate -gt 0) {
                [math]::Round($parallelRate / $serialRate, 3)
            } else { $null }
        }
        fetch = [ordered]@{
            what = "bit-cli bench webseed: bit-cli's HTTP fetch path, no bridge, no hashing, no disk"
            rate_bytes_per_sec = $fetchRate
            rate_human = Format-Rate $fetchRate
            share_of_ceiling = (& $share $fetchRate)
            runs = @($fetchRuns)
            peak_rss_bytes = if ($fetchReports.Count -gt 0) {
                ($fetchReports | ForEach-Object { $_.environment.process.peak_rss_bytes } | Measure-Object -Maximum).Maximum
            } else { $null }
            cpu_ms = if ($fetchReports.Count -gt 0) {
                ($fetchReports | ForEach-Object { $_.environment.process.cpu_ms } | Measure-Object -Maximum).Maximum
            } else { $null }
        }
        download = [ordered]@{
            what = "bit-cli download --web-seed-only: fetch, loopback bridge, verify, write"
            wall_ms = $download
            rate_bytes_per_sec = $downloadRate
            rate_human = Format-Rate $downloadRate
            share_of_ceiling = (& $share $downloadRate)
            share_of_fetch = if ($fetchRate -gt 0 -and $downloadRate -gt 0) {
                "{0:N2}%" -f (($downloadRate / $fetchRate) * 100)
            } else { $null }
            peak_rss_bytes = if ($downloadRss.Count -gt 0) { ($downloadRss | Measure-Object -Maximum).Maximum } else { $null }
            open_handles = if ($downloadHandles.Count -gt 0) { ($downloadHandles | Measure-Object -Maximum).Maximum } else { $null }
            cpu_ms = Get-Stats $downloadCost
            time_to_first_verified_piece_ms = Get-Stats $firstPieceMs
            time_to_first_verified_piece_resolution_ms = 100
            loopback_bytes = $loopbackBytes
            loopback_bytes_per_payload_byte = $loopbackRatio
            framing_bytes_per_block = $framingPerBlock
        }
    }
    failure_cases = @($failureCases)
    steps = @($steps)
    failures = @($failures)
    link = [ordered]@{
        fastest_bps = $linkBps
        fastest_human = $linkHuman
        download_bits_per_sec = $downloadRate * 8
        download_human = if ($downloadRate -gt 0) { "{0:N2} Gbit/s" -f (($downloadRate * 8) / 1e9) } else { $null }
        saturates_fastest_link = if ($linkBps -gt 0) { ($downloadRate * 8) -ge $linkBps } else { $null }
    }
    notes = @(
        "A loopback run has no network cost, so the share of the ceiling is the worst case for bit-cli and the best case for curl. On a real link the network is the bottleneck long before either of them is, which is what the `link` object is for: it says whether the measured rate would saturate this machine's fastest interface.",
        "The page cache is not dropped between runs: Windows has no supported way to do it. Both the ceiling and the candidates read the same file through the same server, so the cache helps each of them equally.",
        "time_to_first_verified_piece is derived from the --jsonl piece_verified event, which is emitted on the --report-interval poll, so its resolution is that interval and not finer. See TODO/cli-surface.md, T-111.",
        "loopback_bytes counts the BEP 3 piece message framing the bridge writes over loopback: 13 bytes per block on top of the payload. The handshake and the bitfield are one-off and are not counted."
    )
}

$report | ConvertTo-Json -Depth 12 | Set-Content -Path $reportPath -Encoding utf8NoBOM

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

Write-Host ""
Write-Host "target:  $($meta.name), $(Format-Size $payloadBytes), $pieceCount pieces, $targetKind"
Write-Host "report:  $reportPath"
Write-Host ""
$rows = @(
    [pscustomobject]@{ STAGE = "curl, 1 connection"; RATE = (Format-Rate $serialRate); "OF CEILING" = (& $share $serialRate); "WALL MIN" = "$($serial.min) ms"; SPREAD = "$($serial.spread_percent)%" }
    [pscustomobject]@{ STAGE = "curl, $Concurrency connections"; RATE = (Format-Rate $parallelRate); "OF CEILING" = "100.00%"; "WALL MIN" = "$($parallel.min) ms"; SPREAD = "$($parallel.spread_percent)%" }
    [pscustomobject]@{ STAGE = "bit-cli fetch, no bridge"; RATE = (Format-Rate $fetchRate); "OF CEILING" = (& $share $fetchRate); "WALL MIN" = "-"; SPREAD = "-" }
    [pscustomobject]@{ STAGE = "bit-cli download, bridge"; RATE = (Format-Rate $downloadRate); "OF CEILING" = (& $share $downloadRate); "WALL MIN" = "$($download.min) ms"; SPREAD = "$($download.spread_percent)%" }
)
$rows | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "The ceiling is curl at $Concurrency connections, which is the shape both bit-cli stages run in."
# A share above 100 percent means the reference was not a ceiling for this
# target. That is a result, not an error, and saying so is better than leaving
# a reader to work out why a percentage of a maximum exceeds the maximum.
foreach ($over in @(
    @{ name = "bit-cli fetch"; rate = $fetchRate }
    @{ name = "bit-cli download"; rate = $downloadRate }
) | Where-Object { $ceilingRate -gt 0 -and $_.rate -gt $ceilingRate }) {
    Write-Host ("{0} beat the reference, so curl at {1} connections was not the limit here." -f $over.name, $Concurrency)
}

if ($loopbackRatio) {
    Write-Host "loopback bytes per payload byte: $loopbackRatio"
}
if ($linkBps -gt 0) {
    $verdict = if (($downloadRate * 8) -ge $linkBps) { "saturates" } else { "does not saturate" }
    Write-Host "download rate is $("{0:N2} Gbit/s" -f (($downloadRate * 8) / 1e9)), which $verdict $linkHuman"
}
if ($firstPieceMs.Count -gt 0) {
    $ttfvp = Get-Stats $firstPieceMs
    Write-Host "time to first verified piece:    $($ttfvp.min) ms minimum, $($ttfvp.max) ms worst (resolution 100 ms)"
}
foreach ($case in $failureCases) {
    $verdict = if ($case.ended) { "ended after $($case.elapsed_ms) ms with exit $($case.exit_code)" } else { "DID NOT END" }
    Write-Host "failure case $($case.case): $verdict"
}

if (-not $Keep) {
    Remove-Item -Recurse -Force (Join-Path $Root "payload") -ErrorAction SilentlyContinue
    Get-ChildItem -Directory $Root -Filter "out-*" | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -Force (Join-Path $Root "curl.bin") -ErrorAction SilentlyContinue
}

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("bench-webseed: $failure") }
    exit 1
}
$notEnded = @($failureCases | Where-Object { -not $_.ended })
if ($notEnded.Count -gt 0) {
    [Console]::Error.WriteLine("bench-webseed: $($notEnded.Count) failure case(s) never ended")
    exit 1
}
exit 0
