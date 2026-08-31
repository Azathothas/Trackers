# Watch one long-lived process for a slope.
#
# `TODO/memory.md` T-040 is a report of RSS and open descriptors climbing until
# the process failed, over a run measured in days. `bit-cli seed` is the shape
# that reaches: a single process holding a payload, a listener, a tracker
# announce timer, and whatever peers turn up.
#
# The subject is one seeder. Everything else here exists to give it something
# to do, because an idle process is flat by construction and a flat line from
# an idle process says nothing about a busy one.
#
# Six workloads, so a slope names a subsystem rather than "the process":
#
#   idle      a seeder with no tracker and nothing connecting. The control.
#             Any slope here is the session's own timers or the sampler.
#   announce  a loopback tracker at a short interval. The reporter's growth
#             started after changing trackers, so this is the announce path on
#             its own. The tracker never expires a peer, so the peer list the
#             seeder is handed grows for the whole run, which is the shape a
#             busy public tracker has.
#   leech     real downloads against the seeder, one finishing and another
#             starting. Peer sessions arriving and leaving, with payload
#             moving and files opening.
#   steady    announce and leech together. The deployment shape, and the
#             default, because those are the two paths a seeder runs for days.
#   churn     connections that open and close without handshaking. This is
#             T-020's shape and the known positive: it strands sockets, so a
#             run with it should show a slope, which is what says the sampler
#             can see one.
#   all       steady plus churn.
#
# `all` is not the default and should not be the six-hour run. Churn strands
# sockets at about 30,000 handles an hour (measured, see TODO/memory.md), which
# is T-020 rather than T-040 and swamps every other series in the same chart.
# It no longer starves the leechers, and that line used to say it did. The claim
# that carries is the failure count rather than the cycle count: measured on
# 2026-08-25 at two leechers with -ListenerCheck on, **no cycle failed either
# way**, 22 completed over two minutes with no churn and 26 over three minutes
# with the default churn beside them. The old figures, 1 completed and 2 failed,
# were taken before T-020 closed. Starving them now takes -ChurnConnections
# 20000 with -ChurnConcurrency 256 or more, which is what the two runs behind
# T-232's attribution use.
#
# Three series, sampled every -SampleSeconds from outside the process:
# resident memory, handle count, and TCP socket states. The seeder reports the
# first two itself in every `progress` event under `--jsonl`, and the summary
# checks the two against each other, because a sampler that disagrees with the
# subject is measuring something else.
#
# Usage:
#   pwsh scripts/soak.ps1                             six hours, the deployment
#   pwsh scripts/soak.ps1 -Minutes 20 -Workload churn
#   pwsh scripts/soak.ps1 -Minutes 360 -RssCeilingMiBPerHour 8
#
# Writes bench/soak-<timestamp>.csv with one row per sample and
# bench/soak-<timestamp>.json with the parameters, the slopes, and the verdict.
#
# Exits 0 when the run completed and every named ceiling held, 1 when a ceiling
# was passed or the seeder died, and 2 when the check could not run. With no
# ceiling named the slopes are recorded rather than judged, because T-040 is
# open and this script is what measures it.
#
# See TODO/memory.md, T-040.

[CmdletBinding()]
param(
    # Wall clock. T-040's acceptance is six hours, which is the default.
    [int]$Minutes = 360,
    [int]$SampleSeconds = 30,
    [ValidateSet("steady", "all", "idle", "announce", "leech", "churn")]
    [string]$Workload = "steady",
    # Small on purpose: the leech cycle rate is what matters, not the bytes.
    [int]$PayloadMiB = 16,
    # Downloads in flight against the seeder. Each one is a peer session that
    # connects, transfers, and leaves.
    [int]$Leechers = 2,
    # Connections per churn burst, and how many bursts run at once.
    [int]$ChurnConnections = 500,
    [int]$ChurnConcurrency = 32,
    # Slope ceilings. Zero records the number without judging it.
    [double]$RssCeilingMiBPerHour = 0,
    [double]$HandleCeilingPerHour = 0,
    [double]$CloseWaitCeilingPerHour = 0,
    # How many leech cycles may fail, as a percentage of the cycles attempted,
    # before the run stops being a measurement of the workload it names.
    #
    # This is not a ceiling like the three above and it is judged whether or
    # not one is named, because it is not about the subject. It is about
    # whether the subject was doing anything. The run of 2026-08-23T15:47:16Z
    # completed 298 cycles, failed 1,080, and stopped completing any at
    # t+1.29h; its seeder then sat at a flat 168 handles and no CPU for the
    # remaining 4.7 hours and the report said "every named ceiling held over 6
    # hours". Every number in it is true and the run measured an idle process.
    #
    # Zero turns the judgement off for a run that is deliberately hostile to
    # its own leechers, which -Workload churn is.
    [double]$LeechFailurePercent = 5,
    # Pass a duration to give the seeder --listener-check.
    #
    # Off by default so the two committed six hour runs stay comparable: the
    # check costs one loopback connection and one peer row per interval, which
    # are two of the series this script measures. Turn it on for a run that is
    # asking whether the seeder is still answering, which is the question the
    # run above left open. See TODO/memory.md, T-232.
    [string]$ListenerCheck,
    [string]$Root = ".tmp/soak",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [switch]$Keep,
    # Read a CSV a finished run left behind and print its fits, without
    # starting anything. A soak is six hours and its numbers are read many
    # times after it, most recently by hand into TODO/memory.md, T-224. Every
    # other parameter but -ReadJson is ignored.
    [string]$ReadCsv,
    # Where -ReadCsv writes the same fits as JSON. The table beside it is for a
    # person; this is what scripts/check-soak-fit.ps1 asserts on, so the check
    # reads this script's own numbers rather than computing its own.
    [string]$ReadJson
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("soak: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

$script:Background = @()

function Start-Child($path, $arguments, $tag) {
    $process = Start-Process -FilePath $path -WorkingDirectory $Root -NoNewWindow -PassThru `
        -ArgumentList $arguments `
        -RedirectStandardOutput (Join-Path $Root "$tag.out") `
        -RedirectStandardError (Join-Path $Root "$tag.err")
    $script:Background += $process
    $process
}

function Stop-Background {
    foreach ($process in $script:Background) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $script:Background = @()
}

# ---------------------------------------------------------------------------
# Slopes
# ---------------------------------------------------------------------------
#
# Least squares against elapsed hours, so the slope reads as "per hour" and a
# six-hour run and a twenty-minute one are comparable. R squared is reported
# beside it because a slope through noise is not a trend, and the two numbers
# together are what say whether the line is real.
#
# They are still not enough, and the run of 2026-08-23T09:01:32Z is why. Its
# `rss_bytes` slope was 3.708 MiB/h at r squared 0.717, against a ceiling of 4,
# and that line was fitted across a single interval where resident memory rose
# 11.61 MiB in eight seconds and never came back. Either side of it the slope
# is 1.02 and 1.69. A fit describes a trend; nothing in a slope and an r
# squared can say that most of the run's rise arrived at once, and a reader
# with only those two numbers reads a step as growth.
#
# So the largest single-interval change in each direction is reported beside
# the fit, with when it happened, and `step_share` is what fraction of the
# run's whole rise that one interval carried. `step_share` reads high on a
# short run with little total movement, where it means nothing; it is the
# **magnitude** that separates a step from a sawtooth, and both are printed.
# See `TODO/memory.md`, T-224.

function Get-Slope($rows, $column) {
    $n = $rows.Count
    if ($n -lt 2) { return $null }
    $sumX = 0.0; $sumY = 0.0; $sumXY = 0.0; $sumXX = 0.0
    foreach ($row in $rows) {
        $x = [double]$row.elapsed_s / 3600.0
        $y = [double]$row.$column
        $sumX += $x; $sumY += $y; $sumXY += ($x * $y); $sumXX += ($x * $x)
    }
    $denominator = ($n * $sumXX) - ($sumX * $sumX)
    if ([math]::Abs($denominator) -lt 1e-12) { return $null }
    $slope = (($n * $sumXY) - ($sumX * $sumY)) / $denominator
    $intercept = ($sumY - ($slope * $sumX)) / $n
    $meanY = $sumY / $n
    $ssTot = 0.0; $ssRes = 0.0
    foreach ($row in $rows) {
        $x = [double]$row.elapsed_s / 3600.0
        $y = [double]$row.$column
        $ssTot += [math]::Pow($y - $meanY, 2)
        $ssRes += [math]::Pow($y - ($intercept + ($slope * $x)), 2)
    }
    $r2 = if ($ssTot -gt 0) { 1.0 - ($ssRes / $ssTot) } else { $null }
    $values = @($rows | ForEach-Object { [double]$_.$column })

    # The largest move between two consecutive samples, each way, and when.
    # Walked once over the same rows the fit used, so the two cannot describe
    # different windows.
    $largestRise = 0.0; $largestRiseAt = $null
    $largestFall = 0.0; $largestFallAt = $null
    for ($i = 1; $i -lt $n; $i++) {
        $delta = $values[$i] - $values[$i - 1]
        $at = [double]$rows[$i].elapsed_s / 3600.0
        if ($delta -gt $largestRise) { $largestRise = $delta; $largestRiseAt = $at }
        if ($delta -lt $largestFall) { $largestFall = $delta; $largestFallAt = $at }
    }
    $totalRise = $values[$n - 1] - $values[0]
    $stepShare = if ($totalRise -gt 0) { [math]::Round($largestRise / $totalRise, 3) } else { $null }

    [ordered]@{
        column             = $column
        samples            = $n
        first              = $values[0]
        last               = $values[$n - 1]
        min                = ($values | Measure-Object -Minimum).Minimum
        max                = ($values | Measure-Object -Maximum).Maximum
        mean               = [math]::Round(($values | Measure-Object -Average).Average, 2)
        slope_per_hour     = [math]::Round($slope, 3)
        r_squared          = if ($null -eq $r2) { $null } else { [math]::Round($r2, 4) }
        largest_rise       = $largestRise
        largest_rise_hours = if ($null -eq $largestRiseAt) { $null } else { [math]::Round($largestRiseAt, 3) }
        largest_fall       = $largestFall
        largest_fall_hours = if ($null -eq $largestFallAt) { $null } else { [math]::Round($largestFallAt, 3) }
        step_share         = $stepShare
    }
}

# ---------------------------------------------------------------------------
# Reading a run that already happened
# ---------------------------------------------------------------------------
#
# Placed immediately after Get-Slope and before four things, each of which
# would break it in a different way. Before the `trap`, because `exit` at
# script scope is a terminating error and the trap rethrows it. Before every
# `Start-Child`, because a read-only mode below those is a soak with a report
# on the end of it. Before the `Get-NetTCPConnection` platform guard, because
# reading a CSV needs no sockets and `scripts/check-soak-fit.ps1` runs this on
# Linux in CI. And after `Get-Slope`, because the table has to be the one a
# live run prints, from the same function, so a number read here and a number
# read there cannot differ.

# Is this line a sample, or is it what a killed run left behind?
#
# A soak that is killed leaves the file it was appending to extended and not
# written: NTFS flushes the size before the bytes, so the tail is zero fill.
# `bench/soak-20260821T012428252Z.csv` carried 176 such bytes for three days.
# `Import-Csv` turns them into one more record whose every field is the empty
# string, `[double]""` is 0 in PowerShell, and `Get-Slope` then fits a line
# through a final sample of zeros: that file read as `last 0.00 MiB` for every
# series, "532 samples over 0 hours", and a largest fall of -20.75 MiB that
# nothing measured. Nothing said anything was wrong.
#
# So a row has to look like a row. `sample`, `elapsed_s` and `rss_bytes` are
# counters the sampler writes as integers, and `iso` is an instant. A record
# missing any of those is not a sample, whatever produced it.
#
# It is dropped and counted rather than dropped quietly, because a truncated
# file is itself worth knowing about: the 531 samples before the tail are real
# and the run they came from ended in a way its report never mentioned.
function Test-SoakRow($row) {
    if (-not $row) { return $false }
    foreach ($field in @("sample", "elapsed_s", "rss_bytes")) {
        if ("$($row.$field)" -notmatch '^\d+$') { return $false }
    }
    return ("$($row.iso)" -match '^\d{4}-\d{2}-\d{2}T')
}

if ($ReadCsv) {
    if (-not (Test-Path $ReadCsv)) { Exit-With 2 "no such CSV: $ReadCsv" }
    $parsed = @(Import-Csv $ReadCsv)
    $read = @($parsed | Where-Object { Test-SoakRow $_ })
    $dropped = $parsed.Count - $read.Count
    if ($read.Count -lt 2) { Exit-With 2 "$ReadCsv has $($read.Count) sample(s), which fits nothing" }
    $readHours = [double]$read[-1].elapsed_s / 3600.0

    # The workload, from the two counters the sampler writes into every row.
    #
    # A finished run is read many times after it and the fits below are all
    # about the seeder. Whether anything was talking to the seeder is the
    # question to answer before any of them mean something, and it is in the
    # CSV already. See TODO/memory.md, T-232.
    $readDone = [int]$read[-1].leech_completed
    $readFailed = [int]$read[-1].leech_failed
    $readAttempted = $readDone + $readFailed
    $readShare = if ($readAttempted -gt 0) { [math]::Round(100.0 * $readFailed / $readAttempted, 2) } else { 0 }
    # The last sample at which a cycle completed. A run whose workload stopped
    # has flat series after this point and they are flat because nothing was
    # happening.
    $lastProgress = $null
    for ($i = $read.Count - 1; $i -gt 0; $i--) {
        if ([int]$read[$i].leech_completed -gt [int]$read[$i - 1].leech_completed) {
            $lastProgress = $read[$i]
            break
        }
    }

    # The listener columns, when the run that wrote the file had them. A CSV
    # from before 2026-08-25 has no such columns and reads as a run that was
    # not watched, which is what it was. See TODO/memory.md, T-232.
    $readListener = $null
    $readListenerBadAt = $null
    foreach ($sampleRow in $read) {
        if ("$($sampleRow.listener_probes)" -notmatch '^\d+$') { continue }
        $readListener = $sampleRow
        if ("$($sampleRow.listener_healthy)" -eq "0" -and $null -eq $readListenerBadAt) {
            $readListenerBadAt = [int]$sampleRow.elapsed_s
        }
    }

    Write-Host ""
    Write-Host "csv:       $ReadCsv"
    Write-Host "samples:   $($read.Count) over $([math]::Round($readHours, 3)) hours"
    if ($readListener) {
        $readListenerState = if ($null -ne $readListenerBadAt) {
            "first unhealthy at t+${readListenerBadAt}s"
        }
        else { "healthy at every sample" }
        Write-Host "listener:  $($readListener.listener_probes) probes, $($readListener.listener_failed) failed, $readListenerState"
    }
    if ($dropped -gt 0) {
        Write-Host "truncated: $dropped line(s) are not samples and are excluded from every fit below."
        Write-Host "           A soak killed mid-write leaves the file extended and zero filled."
    }
    if ($readAttempted -gt 0) {
        Write-Host "workload:  $readDone leech cycles completed, $readFailed failed ($readShare percent)"
        if ($readShare -gt $LeechFailurePercent -and $LeechFailurePercent -gt 0) {
            $stoppedAt = if ($lastProgress) { "t+$($lastProgress.elapsed_s)s, $([math]::Round([double]$lastProgress.elapsed_s / 3600.0, 3)) hours" } else { "before the first sample" }
            Write-Host "           OVER the $LeechFailurePercent percent this run treats as still measuring its workload."
            Write-Host "           The last cycle completed at $stoppedAt. Every fit below is mostly of an idle process."
        }
    }
    Write-Host ""
    $readRows = [System.Collections.ArrayList]::new()
    $readFits = [ordered]@{}
    foreach ($column in @("rss_bytes", "peak_rss_bytes", "handles", "threads",
            "tcp_total", "tcp_close_wait", "tcp_established")) {
        $entry = Get-Slope $read $column
        if (-not $entry) { continue }
        $readFits[$column] = $entry
        $scale = if ($column -like "*rss*") { 1MB } else { 1 }
        $unit = if ($column -like "*rss*") { "MiB" } else { "" }
        [void]$readRows.Add([pscustomobject][ordered]@{
                series      = $column
                first       = [math]::Round($entry.first / $scale, 2)
                last        = [math]::Round($entry.last / $scale, 2)
                max         = [math]::Round($entry.max / $scale, 2)
                "per hour"  = [math]::Round($entry.slope_per_hour / $scale, 3)
                "r2"        = $entry.r_squared
                "step up"   = [math]::Round($entry.largest_rise / $scale, 2)
                "at h"      = $entry.largest_rise_hours
                "step down" = [math]::Round($entry.largest_fall / $scale, 2)
                unit        = $unit
            })
    }
    $readRows | Format-Table -AutoSize | Out-String | Write-Host
    if ($ReadJson) {
        $readReport = [ordered]@{
            kind         = "soak_read"
            generated_at = Get-Timestamp
            csv          = $ReadCsv
            samples      = $read.Count
            dropped_rows = $dropped
            hours        = [math]::Round($readHours, 4)
            workload     = [ordered]@{
                leech_completed      = $readDone
                leech_failed         = $readFailed
                leech_failed_percent = $readShare
                last_progress_s      = if ($lastProgress) { [int]$lastProgress.elapsed_s } else { $null }
                measured_its_workload = -not (($LeechFailurePercent -gt 0) -and ($readShare -gt $LeechFailurePercent))
            }
            listener     = $(if ($readListener) {
                    [ordered]@{
                        probes                    = [int64]$readListener.listener_probes
                        failed                    = [int64]$readListener.listener_failed
                        healthy                   = ("$($readListener.listener_healthy)" -eq "1")
                        first_unhealthy_elapsed_s = $readListenerBadAt
                    }
                }
                else { $null })
            slopes       = $readFits
        }
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent (Join-Path $repo $ReadJson)) | Out-Null
        $readReport | ConvertTo-Json -Depth 6 | Set-Content -Path (Join-Path $repo $ReadJson) -Encoding utf8
        Write-Host "soak: wrote $ReadJson"
    }
    exit 0
}

trap { Stop-Background; throw }

if (-not ($IsWindows -or $env:OS -eq "Windows_NT")) {
    Exit-With 2 "the socket series reads Get-NetTCPConnection, which is Windows only. On Linux read `ss -tan` instead."
}
$exe = ".exe"
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
$trackerExe = Join-Path $repo "target/$Profile/examples/loopback-tracker$exe"
$churnExe = Join-Path $repo "target/$Profile/examples/loopback-churn$exe"
foreach ($required in @($bitCli, $trackerExe, $churnExe)) {
    if (-not (Test-Path $required)) {
        Exit-With 2 "missing $required. Build it first: cargo build --$Profile --workspace --bins --examples"
    }
}
if ($Minutes -lt 1) { Exit-With 2 "-Minutes has to be at least 1." }
if ($SampleSeconds -lt 1) { Exit-With 2 "-SampleSeconds has to be at least 1." }

$wantAnnounce = $Workload -in @("steady", "all", "announce")
$wantLeech = $Workload -in @("steady", "all", "leech")
$wantChurn = $Workload -in @("all", "churn")

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path (Join-Path $Root "payload") | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# A six-hour run holds target/release/bit-cli.exe for six hours, and Windows
# will not let cargo replace a running executable, so a soak in the background
# would block every rebuild for as long as it lasts. It runs from its own copy
# instead. The binaries are statically linked, which is what makes a lone .exe
# enough: see scripts/check-static.ps1.
$bin = Join-Path $Root "bin"
New-Item -ItemType Directory -Force -Path $bin | Out-Null
foreach ($source in @($bitCli, $trackerExe, $churnExe)) {
    Copy-Item -Path $source -Destination $bin -Force
}
$bitCli = Join-Path $bin (Split-Path -Leaf $bitCli)
$trackerExe = Join-Path $bin (Split-Path -Leaf $trackerExe)
$churnExe = Join-Path $bin (Split-Path -Leaf $churnExe)

# ---------------------------------------------------------------------------
# A payload to serve
# ---------------------------------------------------------------------------

Write-Step "building a $PayloadMiB MiB payload"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 90210
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}
$stream = [System.IO.File]::Create((Join-Path $Root "payload/soak.bin"))
try { for ($i = 0; $i -lt $PayloadMiB; $i++) { $stream.Write($block, 0, $block.Length) } }
finally { $stream.Dispose() }

$torrent = Join-Path $Root "soak.torrent"
$announce = $null
$trackerProcess = $null
if ($wantAnnounce) {
    $trackerProcess = Start-Child $trackerExe @("--port", "0", "--interval", "5") "tracker"
    $deadline = (Get-Date).AddSeconds(15)
    while (-not $announce -and (Get-Date) -lt $deadline) {
        $line = Get-Content (Join-Path $Root "tracker.out") -TotalCount 1 -ErrorAction SilentlyContinue
        if ($line -and $line.Trim()) { $announce = $line.Trim() }
        if (-not $announce) { Start-Sleep -Milliseconds 100 }
    }
    if (-not $announce) { Exit-With 2 "the loopback tracker never printed its URL" }
    Write-Step "tracker at $announce"
}

$createArgs = @("create", (Join-Path $Root "payload"), "--name", "payload", "--piece-length", "1MiB",
    "--no-creation-date", "--output", $torrent, "--force", "--json")
if ($announce) { $createArgs += @("--announce", $announce) }
& $bitCli @createArgs 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }
$infoHash = (& $bitCli info $torrent --json | ConvertFrom-Json).info_hash
if (-not $infoHash) { Exit-With 2 "could not read the info hash" }

# ---------------------------------------------------------------------------
# The subject
# ---------------------------------------------------------------------------
#
# --seed-time outlives the sampling window, so the run ends because this
# script stops it rather than because the seeder gave up partway.

$seedTime = [int]($Minutes * 60) + 300
$seedArgs = @(
    "seed", $torrent, "--data", $Root, "--port", "0",
    "--no-dht", "--no-lsd",
    "--report-interval", "$($SampleSeconds)s",
    "--seed-time", "$($seedTime)s",
    "--jsonl"
)
if (-not $wantAnnounce) { $seedArgs += "--no-tracker" }
# A seeder that stops answering handshakes while its listening socket stays
# bound looks exactly like a healthy idle one from outside: the process is
# alive, the port is open, and the ratio is still reported. --listener-check
# dials this run's own port and completes a real handshake, and three failures
# in a row stop the run with exit 17. See TODO/memory.md, T-232.
if ($ListenerCheck) { $seedArgs += @("--listener-check", $ListenerCheck) }
Write-Step "starting the seeder: $Workload for $Minutes minutes, sampling every ${SampleSeconds}s"
$seed = Start-Child $bitCli $seedArgs "seed"

$port = $null
$deadline = (Get-Date).AddSeconds(60)
while (-not $port -and (Get-Date) -lt $deadline) {
    if ($seed.HasExited) { Exit-With 2 "the seeder exited before it listened; see $Root/seed.err" }
    $port = (Get-NetTCPConnection -State Listen -OwningProcess $seed.Id -ErrorAction SilentlyContinue |
            Select-Object -First 1).LocalPort
    if (-not $port) { Start-Sleep -Milliseconds 250 }
}
if (-not $port) { Exit-With 2 "the seeder never opened a listening socket" }
Write-Step "seeder listening on 127.0.0.1:$port, pid $($seed.Id)"

# ---------------------------------------------------------------------------
# Load
# ---------------------------------------------------------------------------

$leechSlots = @{}
$leechDone = 0
$leechFailed = 0
$script:LeechFailures = [System.Collections.ArrayList]::new()
$churnRuns = 0
$churnProcess = $null

$script:LoadErrors = 0

# Start a process, and treat a failure to start as load rather than as the end
# of the run.
#
# A six hour soak that dies at hour two has measured two hours, and the reasons
# it dies are not the reasons it is running: a redirected output file that the
# previous process has not finished releasing, a directory removal racing the
# next creation, a machine briefly out of handles. Windows releases a process
# handle some time after `HasExited` goes true, so restarting a leecher into
# the same output file is exactly that race, and it fired once here at 2.2
# hours into a six hour run under a parallel `cargo build`.
#
# So: three attempts with a short wait, and then a counted failure. The count
# is in the summary, because a run with a hundred of them is measuring
# something else.
function Start-Counted($block, $what) {
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try { return & $block }
        catch {
            if ($attempt -eq 3) {
                $script:LoadErrors++
                Write-Step "  could not start $what after 3 attempts: $($_.Exception.Message)"
                return $null
            }
            Start-Sleep -Milliseconds (200 * $attempt)
        }
    }
}

function Start-Leech($slot) {
    Start-Counted {
        $out = Join-Path $Root "leech-$slot"
        if (Test-Path $out) { Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue }
        New-Item -ItemType Directory -Force -Path $out | Out-Null
        Start-Process -FilePath $bitCli -WorkingDirectory $Root -NoNewWindow -PassThru -ArgumentList @(
            "download", $torrent, "--dir", $out,
            "--peer", "127.0.0.1:$port",
            "--no-dht", "--no-lsd", "--no-tracker",
            "--allow-overwrite", "--stop-after", "120s", "--json"
        ) -RedirectStandardOutput (Join-Path $Root "leech-$slot.out") `
            -RedirectStandardError (Join-Path $Root "leech-$slot.err")
    } "leecher $slot"
}

function Start-Churn {
    Start-Counted {
        Start-Process -FilePath $churnExe -WorkingDirectory $Root -NoNewWindow -PassThru -ArgumentList @(
            "--peer", "127.0.0.1:$port",
            "--connections", "$ChurnConnections",
            "--concurrency", "$ChurnConcurrency",
            "--no-handshake"
        ) -RedirectStandardOutput (Join-Path $Root "churn.out") `
            -RedirectStandardError (Join-Path $Root "churn.err")
    } "churn"
}

# What the seeder says it cost, so the sampler can be checked against the
# subject. A sampler that disagrees with the process is measuring something
# else.
#
# This reads forward from where the last call stopped rather than re-reading
# the whole file, because a six hour run writes 720 progress events and
# re-parsing every one of them on every sample is work charged to the machine
# under measurement. A chunk can end mid-line, so the tail is held back until
# its newline arrives.

$script:SelfStream = $null
$script:SelfReader = $null
$script:SelfPending = ""
$script:SelfPeakRss = $null
$script:SelfHandles = $null
$script:SelfEvents = 0

# What --listener-check found, out of the same events.
#
# A finished run used to say the flag was on and nothing about what it saw:
# bench/soak-20260824T164609340Z.json carries parameters.listener_check "60s",
# no listener key anywhere, and no listener column in the CSV. The seeder
# reports it in every progress event and $Root is deleted when the run ends, so
# the only place it existed was a file the run then destroyed. See
# TODO/memory.md, T-232.
#
# probes and failed are counters the seeder accumulates, so the last event
# carries the totals. healthy and consecutive_failures are levels, so the
# worst one seen is kept beside the last: a listener that failed for an hour
# and recovered is a run whose middle cannot be read off the final values.
$script:SelfListener = $null
$script:SelfListenerWorst = 0
$script:SelfListenerUnhealthy = 0
$script:SelfListenerFirstBad = $null

function Update-SelfReported {
    try {
        if (-not $script:SelfReader) {
            $selfPath = Join-Path $Root "seed.out"
            if (-not (Test-Path $selfPath)) { return }
            $script:SelfStream = [System.IO.File]::Open(
                $selfPath, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read,
                [System.IO.FileShare]::ReadWrite)
            $script:SelfReader = [System.IO.StreamReader]::new($script:SelfStream)
        }
        $chunk = $script:SelfReader.ReadToEnd()
        if (-not $chunk) { return }
        $lines = ($script:SelfPending + $chunk) -split "`n"
        $script:SelfPending = $lines[-1]
        for ($index = 0; $index -lt $lines.Count - 1; $index++) {
            $text = $lines[$index].Trim()
            if (-not $text) { continue }
            $reported = $null
            try { $reported = $text | ConvertFrom-Json } catch { continue }
            if ($reported.type -ne "progress" -or -not $reported.process) { continue }
            $script:SelfEvents++
            if ($null -eq $script:SelfPeakRss -or $reported.process.peak_rss_bytes -gt $script:SelfPeakRss) {
                $script:SelfPeakRss = $reported.process.peak_rss_bytes
            }
            if ($null -eq $script:SelfHandles -or $reported.process.open_handles -gt $script:SelfHandles) {
                $script:SelfHandles = $reported.process.open_handles
            }
            # Absent unless --listener-check was asked for, which is what lets
            # a reader tell "watched and fine" from "not watched".
            if ($reported.listener) {
                $script:SelfListener = $reported.listener
                if ($reported.listener.consecutive_failures -gt $script:SelfListenerWorst) {
                    $script:SelfListenerWorst = $reported.listener.consecutive_failures
                }
                if (-not $reported.listener.healthy) {
                    $script:SelfListenerUnhealthy++
                    if ($null -eq $script:SelfListenerFirstBad) {
                        $script:SelfListenerFirstBad = [int]($clock.Elapsed.TotalSeconds)
                    }
                }
            }
        }
    } catch { }
}

# The reader holds seed.out open, and -Keep off deletes the root at the end.
# Windows will not delete a file another handle is on, so this is called
# before the cleanup rather than left to the process exiting.
function Close-SelfReported {
    if ($script:SelfReader) { $script:SelfReader.Dispose(); $script:SelfReader = $null }
    if ($script:SelfStream) { $script:SelfStream.Dispose(); $script:SelfStream = $null }
}

# The summary is written after every sample, not only when the window ends, so
# a run killed at hour four leaves a report of four hours rather than a CSV
# somebody has to fit a line through by hand. `complete` is what says which of
# the two a reader is holding; nothing else about the shape changes, and the
# last write of a run that finished is the object this file always carried.
#
# Returns the slopes, the failures, and the verdict, so the caller prints what
# was written rather than computing it a second time.
function Write-SoakSummary([bool]$Complete) {
    $summaryRows = @($samples)
    $summarySlopes = [ordered]@{}
    foreach ($column in @("rss_bytes", "peak_rss_bytes", "handles", "threads",
            "tcp_total", "tcp_close_wait", "tcp_established")) {
        $summarySlopes[$column] = Get-Slope $summaryRows $column
    }

    $summaryHours = $clock.Elapsed.TotalHours
    $summaryFailures = [System.Collections.ArrayList]::new()
    if ($seedDied) { [void]$summaryFailures.Add("the seeder exited before the sampling window ended; see $Root/seed.err") }

    $summaryRss = if ($summarySlopes["rss_bytes"]) { [math]::Round($summarySlopes["rss_bytes"].slope_per_hour / 1MB, 3) } else { $null }
    if ($RssCeilingMiBPerHour -gt 0 -and $null -ne $summaryRss -and $summaryRss -gt $RssCeilingMiBPerHour) {
        [void]$summaryFailures.Add("resident memory grew $summaryRss MiB/hour, over the ceiling of $RssCeilingMiBPerHour")
    }
    if ($HandleCeilingPerHour -gt 0 -and $summarySlopes["handles"] -and
        $summarySlopes["handles"].slope_per_hour -gt $HandleCeilingPerHour) {
        [void]$summaryFailures.Add("handles grew $($summarySlopes["handles"].slope_per_hour)/hour, over the ceiling of $HandleCeilingPerHour")
    }
    if ($CloseWaitCeilingPerHour -gt 0 -and $summarySlopes["tcp_close_wait"] -and
        $summarySlopes["tcp_close_wait"].slope_per_hour -gt $CloseWaitCeilingPerHour) {
        [void]$summaryFailures.Add("CLOSE_WAIT grew $($summarySlopes["tcp_close_wait"].slope_per_hour)/hour, over the ceiling of $CloseWaitCeilingPerHour")
    }

    # The workload, before the subject.
    #
    # Every ceiling above is a statement about the seeder, and a seeder nobody
    # is talking to holds all of them. The run of 2026-08-23T15:47:16Z stopped
    # completing leech cycles at t+1.29h, failed 1,080 of the 1,378 it
    # attempted, and reported "every named ceiling held over 6 hours" with an
    # empty failures list. Its last 4.7 hours measured an idle process and
    # nothing in the report said so.
    #
    # Judged whether or not a ceiling is named, because this is not a ceiling:
    # it is whether the run measured what it says it measured. See
    # TODO/memory.md, T-232.
    $leechAttempted = $leechDone + $leechFailed
    $leechFailShare = if ($leechAttempted -gt 0) { [math]::Round(100.0 * $leechFailed / $leechAttempted, 2) } else { 0 }
    if ($wantLeech -and $LeechFailurePercent -gt 0 -and $leechAttempted -gt 0 -and
        $leechFailShare -gt $LeechFailurePercent) {
        # And who to blame, when the run was asked to watch the listener.
        #
        # T-232's two candidates are a seeder that stopped accepting and
        # leechers that stopped connecting, and nothing in a finished run
        # distinguished them. A listener probe completes a real handshake
        # against this run's own port, so a seeder that answers it while every
        # leech cycle fails is not the one at fault. That sentence is the
        # entry's first branch and it is written at the instant the share
        # trips rather than left for somebody to cross-read two files for.
        $blame = ""
        if ($script:SelfListener) {
            if ($script:SelfListenerUnhealthy -gt 0) {
                $blame = ". The seeder stopped answering its own listener probe at t+$($script:SelfListenerFirstBad)s, so the fault is the seeder's"
            }
            else {
                $blame = ". The seeder answered its own listener probe throughout, $($script:SelfListener.probes) probes and $($script:SelfListener.failed) failed, so the fault is not the seeder's accept path"
            }
        }
        [void]$summaryFailures.Add(
            "$leechFailed of $leechAttempted leech cycles failed, $leechFailShare percent, over the $LeechFailurePercent percent this run treats as still measuring its workload$blame")
    }

    # A run asked to watch the listener and given nothing to read has not done
    # what it was asked. The seeder refuses --listener-check when it bound no
    # listen port and says so on stderr, which a finished report never carried:
    # this is that case, named in the report rather than in a file the run
    # deletes.
    if ($ListenerCheck -and $script:SelfEvents -gt 0 -and -not $script:SelfListener) {
        [void]$summaryFailures.Add(
            "-ListenerCheck $ListenerCheck was passed and none of the $($script:SelfEvents) progress events carried a listener block, so this run cannot say whether the seeder was still answering")
    }

    $summaryJudged = ($RssCeilingMiBPerHour -gt 0) -or ($HandleCeilingPerHour -gt 0) -or ($CloseWaitCeilingPerHour -gt 0)
    $summaryVerdict = switch ($true) {
        ($summaryFailures.Count -gt 0) { "$($summaryFailures.Count) ceiling(s) or the run itself did not hold"; break }
        (-not $Complete) { "in flight: $($summaryRows.Count) samples over $([math]::Round($summaryHours, 2)) of the $([math]::Round($Minutes / 60.0, 2)) hours asked for"; break }
        ($summaryJudged) { "every named ceiling held over $([math]::Round($summaryHours, 2)) hours"; break }
        default { "recorded, not judged: no ceiling was named"; break }
    }

    [ordered]@{
        kind             = "soak"
        schema_version   = "1"
        generated_at     = Get-Timestamp
        complete         = $Complete
        host             = [ordered]@{
            machine = [System.Environment]::MachineName
            os      = [System.Environment]::OSVersion.VersionString
            cpus    = [System.Environment]::ProcessorCount
        }
        parameters       = [ordered]@{
            minutes           = $Minutes
            sample_seconds    = $SampleSeconds
            workload          = $Workload
            payload_mib       = $PayloadMiB
            leechers          = $Leechers
            churn_connections = $ChurnConnections
            churn_concurrency = $ChurnConcurrency
            profile           = $Profile
            ceilings          = [ordered]@{
                rss_mib_per_hour    = $RssCeilingMiBPerHour
                handles_per_hour    = $HandleCeilingPerHour
                close_wait_per_hour = $CloseWaitCeilingPerHour
                leech_failure_percent = $LeechFailurePercent
            }
            listener_check    = $ListenerCheck
        }
        info_hash        = $infoHash
        csv              = $csvPath
        elapsed_hours    = [math]::Round($summaryHours, 4)
        samples          = $summaryRows.Count
        cycles           = [ordered]@{
            leech_completed         = $leechDone
            leech_failed            = $leechFailed
            leech_failed_percent    = $leechFailShare
            # Why, the first five times. Empty on a run whose workload held.
            leech_failures          = @($script:LeechFailures)
            churn_runs              = $churnRuns
            churn_connections_total = $churnRuns * $ChurnConnections
            progress_events         = $script:SelfEvents
            # Samples or process starts that failed and were carried past
            # rather than ending the run.
            load_errors             = $script:LoadErrors
        }
        slopes           = $summarySlopes
        rss_mib_per_hour = $summaryRss
        self_reported    = [ordered]@{
            peak_rss_bytes = $script:SelfPeakRss
            open_handles   = $script:SelfHandles
            # Null when -ListenerCheck was not passed, so a reader tells "not
            # watched" from "watched and fine" without reading parameters.
            listener       = $(if ($script:SelfListener) {
                    [ordered]@{
                        healthy                    = [bool]$script:SelfListener.healthy
                        probes                     = [int64]$script:SelfListener.probes
                        failed                     = [int64]$script:SelfListener.failed
                        consecutive_failures       = [int]$script:SelfListener.consecutive_failures
                        last_rtt_ms                = $script:SelfListener.last_rtt_ms
                        last_failure               = $script:SelfListener.last_failure
                        worst_consecutive_failures = [int]$script:SelfListenerWorst
                        unhealthy_events           = [int]$script:SelfListenerUnhealthy
                        first_unhealthy_elapsed_s  = $script:SelfListenerFirstBad
                    }
                }
                else { $null })
        }
        seed_exited_early = $seedDied
        verdict          = $summaryVerdict
        failures         = @($summaryFailures)
        commands         = @(
            "$bitCli $($seedArgs -join ' ')",
            $(if ($wantChurn) { "$churnExe --peer 127.0.0.1:$port --connections $ChurnConnections --concurrency $ChurnConcurrency --no-handshake" } else { $null }),
            $(if ($wantLeech) { "$bitCli download $torrent --dir leech-N --peer 127.0.0.1:$port --no-dht --no-lsd --no-tracker --allow-overwrite --stop-after 120s --json" } else { $null }),
            $(if ($wantAnnounce) { "$trackerExe --port 0 --interval 5" } else { $null })
        ) | Where-Object { $_ }
        notes            = @(
            "The subject is the seeder. rss_bytes and handles are read from outside with Get-Process, and the seeder's own progress events carry the same two figures, so self_reported is the cross-check rather than a second measurement.",
            "slope_per_hour is least squares against elapsed hours. r_squared beside it says whether the line is a trend or noise: a large slope with a low r squared is a spike, not growth.",
            "largest_rise and largest_fall are the biggest move between two consecutive samples, with the elapsed hour each happened at, and step_share is largest_rise over the run's whole rise. They are here because a slope and an r squared cannot say that a fit spans a step: the run of 2026-08-23T09:01:32Z read 3.708 MiB/h at r squared 0.717 across one interval that rose 11.61 MiB and never came back. Read the magnitude first; step_share is high and meaningless on a run that barely moved. See TODO/memory.md, T-224.",
            "peak_rss_bytes is a high-water mark rather than a level, so its slope is bounded below by zero and says nothing on its own. rss_bytes is the series that can fall as well as rise, and it is the one a leak shows in.",
            "The loopback tracker never expires a peer, so under -Workload announce or all the peer list handed to the seeder grows for the whole run. That is deliberate: it is the shape a busy tracker has, and it is the path T-040's report points at.",
            "complete is false while the run is still sampling. This file is rewritten after every sample, so a run that is killed leaves the report it had reached rather than nothing at all.",
            "leech_failed_percent is judged whether or not a ceiling is named, because every ceiling here is a statement about the seeder and a seeder nobody is talking to holds all of them. leech_failures says why the first few failed, which is the thing a finished run cannot be asked afterwards. See TODO/memory.md, T-232.",
            "self_reported.listener is null unless -ListenerCheck was passed. probes and failed are the seeder's own counters, so they are totals for the run; consecutive_failures and healthy are the last event's levels, and worst_consecutive_failures with unhealthy_events say whether the middle of the run differed from its end. The same three figures are the last columns of the CSV, one per sample. See TODO/memory.md, T-232."
        )
        # Written beside the report and renamed over it, never into it. A
        # `Set-Content` straight onto $jsonPath truncates first and fills
        # after, so a process killed between the two leaves a file of NUL
        # bytes. That is what happened to the steady run of 2026-08-21T01:24Z:
        # 531 CSV samples survived because the CSV is appended, and the JSON
        # this rule exists to preserve was destroyed. See TODO/memory.md,
        # T-157.
    } | ConvertTo-Json -Depth 8 | Set-Content -Path "$jsonPath.tmp" -Encoding utf8NoBOM
    Move-Item -LiteralPath "$jsonPath.tmp" -Destination $jsonPath -Force

    [ordered]@{
        slopes           = $summarySlopes
        failures         = $summaryFailures
        verdict          = $summaryVerdict
        hours            = $summaryHours
        rss_mib_per_hour = $summaryRss
    }
}

# ---------------------------------------------------------------------------
# Sampling
# ---------------------------------------------------------------------------

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$csvPath = Join-Path $ReportDir "soak-$stamp.csv"
$jsonPath = Join-Path $ReportDir "soak-$stamp.json"
# The three listener columns are empty on a run without -ListenerCheck, and on
# every CSV written before 2026-08-25. Import-Csv reads by name, so an older
# file still reads: the columns are absent rather than wrong.
$header = "sample,iso,elapsed_s,rss_bytes,peak_rss_bytes,handles,threads,cpu_ms," +
"tcp_total,tcp_established,tcp_listen,tcp_close_wait,tcp_time_wait,tcp_other," +
"leech_completed,leech_failed,churn_runs," +
"listener_probes,listener_failed,listener_healthy"
Set-Content -Path $csvPath -Value $header -Encoding utf8NoBOM

$samples = [System.Collections.ArrayList]::new()
$clock = [System.Diagnostics.Stopwatch]::StartNew()
$endAt = (Get-Date).AddMinutes($Minutes)
$sample = 0
$seedDied = $false

while ((Get-Date) -lt $endAt) {
    if ($seed.HasExited) { $seedDied = $true; break }

    # One sample is not the run. A transient failure here, a file still held
    # by a process that has exited or a directory removal racing its own
    # recreation, used to end a six hour soak at hour two: everything in this
    # loop ran under `$ErrorActionPreference = 'Stop'` with a trap above it.
    # It is counted and the loop carries on, and the count is in the summary,
    # because a run with a hundred of them is measuring something else.
    try {
        # Top up the load before sampling, so a sample never lands in the gap
        # between one leecher exiting and the next starting.
        if ($wantLeech) {
            for ($slot = 0; $slot -lt $Leechers; $slot++) {
                $running = $leechSlots[$slot]
                if ($running -and $running.HasExited) {
                    if ($running.ExitCode -eq 0) { $leechDone++ }
                    else {
                        $leechFailed++
                        # Why it failed, the first few times, because the
                        # answer is gone otherwise. Both redirect files are
                        # overwritten by the next cycle and $Root is deleted at
                        # the end, so the run of 2026-08-23T15:47:16Z failed
                        # 1,080 cycles and left nothing that said what any of
                        # them hit. Capped, because a run that fails a thousand
                        # times fails the same way a thousand times.
                        if ($script:LeechFailures.Count -lt 5) {
                            $errText = ""
                            foreach ($tail in @("leech-$slot.err", "leech-$slot.out")) {
                                $lines = @(Get-Content (Join-Path $Root $tail) -Tail 4 -ErrorAction SilentlyContinue |
                                        Where-Object { $_ -and $_.Trim() })
                                if ($lines.Count -gt 0) { $errText = ($lines -join " | "); break }
                            }
                            [void]$script:LeechFailures.Add([ordered]@{
                                    sample    = $sample
                                    elapsed_s = [int]($clock.Elapsed.TotalSeconds)
                                    slot      = $slot
                                    exit_code = $running.ExitCode
                                    said      = $errText
                                })
                            Write-Step "  leecher $slot exited $($running.ExitCode): $errText"
                        }
                    }
                    $leechSlots[$slot] = $null
                    $running = $null
                }
                if (-not $running) { $leechSlots[$slot] = Start-Leech $slot }
            }
        }
        if ($wantChurn -and (-not $churnProcess -or $churnProcess.HasExited)) {
            if ($churnProcess) { $churnRuns++ }
            $churnProcess = Start-Churn
        }
    
        # Before the row is built rather than after it, so the listener columns
        # in a sample are the events that arrived before that sample and not
        # the ones before the previous one.
        Update-SelfReported

        $seed.Refresh()
        $states = @{}
        foreach ($group in (Get-NetTCPConnection -OwningProcess $seed.Id -ErrorAction SilentlyContinue |
                    Group-Object State)) {
            $states[$group.Name] = $group.Count
        }
        $total = 0
        foreach ($count in $states.Values) { $total += $count }
        $named = 0
        foreach ($key in @("Established", "Listen", "CloseWait", "TimeWait")) {
            if ($states.ContainsKey($key)) { $named += $states[$key] }
        }
    
        $row = [ordered]@{
            sample           = $sample
            iso              = Get-Timestamp
            elapsed_s        = [int]($clock.Elapsed.TotalSeconds)
            rss_bytes        = $seed.WorkingSet64
            peak_rss_bytes   = $seed.PeakWorkingSet64
            handles          = $seed.HandleCount
            threads          = $seed.Threads.Count
            cpu_ms           = [int64]$seed.TotalProcessorTime.TotalMilliseconds
            tcp_total        = $total
            tcp_established  = if ($states.ContainsKey("Established")) { $states["Established"] } else { 0 }
            tcp_listen       = if ($states.ContainsKey("Listen")) { $states["Listen"] } else { 0 }
            tcp_close_wait   = if ($states.ContainsKey("CloseWait")) { $states["CloseWait"] } else { 0 }
            tcp_time_wait    = if ($states.ContainsKey("TimeWait")) { $states["TimeWait"] } else { 0 }
            tcp_other        = $total - $named
            leech_completed  = $leechDone
            leech_failed     = $leechFailed
            churn_runs       = $churnRuns
            listener_probes  = if ($script:SelfListener) { $script:SelfListener.probes } else { "" }
            listener_failed  = if ($script:SelfListener) { $script:SelfListener.failed } else { "" }
            listener_healthy = if ($script:SelfListener) { [int][bool]$script:SelfListener.healthy } else { "" }
        }
        [void]$samples.Add($row)
        Add-Content -Path $csvPath -Encoding utf8NoBOM -Value (($row.Values | ForEach-Object { "$_" }) -join ",")
    
        # Rewrite the report now rather than only when the window ends, so a run
        # that is killed at hour four leaves four hours of slopes. See
        # Write-SoakSummary.
        [void](Write-SoakSummary $false)
    
        if ($sample % 10 -eq 0) {
            Write-Step ("  t+{0,6}s  rss {1,7:N1} MiB  handles {2,5}  sockets {3,5}  CW {4,5}  leech {5}" -f `
                    $row.elapsed_s, ($row.rss_bytes / 1MB), $row.handles, $row.tcp_total, $row.tcp_close_wait, $leechDone)
        }
        $sample++
    }
    catch {
        $script:LoadErrors++
        Write-Step "  sample $sample failed: $($_.Exception.Message)"
        $sample++
    }

    $nextAt = $endAt
    $due = (Get-Date).AddSeconds($SampleSeconds)
    if ($due -lt $nextAt) { $nextAt = $due }
    $wait = ($nextAt - (Get-Date)).TotalMilliseconds
    if ($wait -gt 0) { Start-Sleep -Milliseconds ([int]$wait) }
}

$clock.Stop()
# Floor rather than `[int]`, which rounds: 59.6 minutes printed as 60 and read
# as a run that reached its hour. See scripts/session-report.ps1.
Write-Step "sampling finished after $([math]::Floor($clock.Elapsed.TotalMinutes)) minutes, $($samples.Count) samples"

if (-not $seed.HasExited) { Stop-Process -Id $seed.Id -Force -ErrorAction SilentlyContinue }
Start-Sleep -Milliseconds 500
Update-SelfReported
Close-SelfReported
Stop-Background

$summary = Write-SoakSummary $true
$slopes = $summary.slopes
$failures = $summary.failures
$hours = $summary.hours

Write-Host ""
Write-Host "workload:  $Workload for $([math]::Round($hours, 2)) hours, $($samples.Count) samples"
Write-Host "csv:       $csvPath"
Write-Host "report:    $jsonPath"
Write-Host ""
@($slopes.Keys) | ForEach-Object {
    $entry = $slopes[$_]
    if (-not $entry) { return }
    [pscustomobject][ordered]@{
        series      = $_
        first       = $entry.first
        last        = $entry.last
        max         = $entry.max
        "per hour"  = $entry.slope_per_hour
        "r2"        = $entry.r_squared
        "step up"   = $entry.largest_rise
        "at h"      = $entry.largest_rise_hours
        "step down" = $entry.largest_fall
    }
} | Format-Table -AutoSize | Out-String | Write-Host
$closingAttempted = $leechDone + $leechFailed
$closingShare = if ($closingAttempted -gt 0) { [math]::Round(100.0 * $leechFailed / $closingAttempted, 1) } else { 0 }
Write-Host "leech cycles: $leechDone completed, $leechFailed failed ($closingShare percent). churn bursts: $churnRuns."
foreach ($why in $script:LeechFailures) {
    Write-Host "  first failures: t+$($why.elapsed_s)s slot $($why.slot) exit $($why.exit_code) $($why.said)"
}
if ($script:LoadErrors -gt 0) {
    Write-Host "load errors carried past: $script:LoadErrors"
}
if ($null -ne $script:SelfPeakRss) {
    Write-Host "self reported: peak RSS $([math]::Round($script:SelfPeakRss / 1MB, 2)) MiB, $script:SelfHandles handles, over $($script:SelfEvents) progress events"
}
if ($script:SelfListener) {
    $listenerState = if ($script:SelfListenerUnhealthy -gt 0) {
        "went unhealthy at t+$($script:SelfListenerFirstBad)s, worst run of $script:SelfListenerWorst"
    }
    else { "healthy throughout" }
    Write-Host "listener:      $($script:SelfListener.probes) probes, $($script:SelfListener.failed) failed, $listenerState"
}
elseif ($ListenerCheck) {
    Write-Host "listener:      -ListenerCheck $ListenerCheck was passed and no progress event carried a listener block"
}
Write-Host "verdict: $($summary.verdict)"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("soak: $failure") }
    exit 1
}
exit 0
