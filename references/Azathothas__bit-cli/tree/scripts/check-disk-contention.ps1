# Why does writing one payload file from several threads cost more than
# writing it from one?
#
# `bench leech` found that the same 1 GiB costs 995 ms of write time totalled
# across one receive path and 11,918 ms across eight. `TODO/disk-io.md` T-017
# named two possible causes and a download cannot tell them apart, because
# both are running at once: the file handle, or the session's own locking
# around a received chunk. This script takes the session out and separates
# them.
#
# Phase one, the layouts. `bit-cli bench disk` writes the same bytes in the
# same block size from the same number of threads, three ways:
#
#   shared   one file, one handle, every thread interleaving blocks into it.
#            The shape a torrent with one payload file and several peers has.
#   handles  one file opened once per thread, each writing through its own
#            handle, at the same offsets shared uses.
#   split    one file per thread, each writing only its own.
#
# Reading the three against each other answers it. If handles beats shared,
# the limit is per handle and giving a file more of them fixes it. If handles
# tracks shared and only split scales, the limit is the file itself and no
# number of handles helps. If shared scales on its own, storage was never the
# problem and the session is.
#
# Phase two, the block size. Whatever the limit turns out to be, this says
# whether it is charged per operation or per byte: the same bytes to the same
# file from the same threads, in blocks from 16 KiB up. A limit charged per
# operation gets cheaper with larger writes and one charged per byte does not.
#
# The layouts alternate inside each iteration and the order flips between
# iterations, so no layout always gets the disk in the same state. Each step
# drains its writeback before the next starts, or a step that filled the page
# cache would hand its cost to whichever step ran after it.
#
# Usage:
#   pwsh scripts/check-disk-contention.ps1
#   pwsh scripts/check-disk-contention.ps1 -PayloadSize 2GiB -Iterations 5 -ThreadSweep "1,2,4,8,16"
#   pwsh scripts/check-disk-contention.ps1 -BlockSizes "16KiB,1MiB"
#   pwsh scripts/check-disk-contention.ps1 -RunLength 64
#
# -RunLength is how many blocks one thread writes before the next takes over,
# under shared and handles. 1 strides block by block, which is what every
# record before 2026-08-22 was taken at and is the most contended arrangement
# there is. A receive path writes a whole fetched range at a time, so 64 at a
# 16 KiB block is the shape a download has. See TODO/disk-io.md, T-018.
#
# Exits 0 when the run produced a verdict, 1 when a step read back a block it
# did not write, and 2 when the check could not run. The record goes to
# bench/disk-contention-<timestamp>.json with every step of every iteration.
#
# See TODO/disk-io.md, T-017.

[CmdletBinding()]
param(
    [string]$PayloadSize = "1GiB",
    [string]$BlockSize = "16KiB",
    [string]$ThreadSweep = "1,2,4,8",
    [int]$RunLength = 1,
    [string]$BlockSizes = "16KiB,64KiB,256KiB,1MiB",
    [int]$Iterations = 3,
    [string]$FileAllocation = "sparse",
    [string]$Root = ".tmp/disk-contention",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-disk-contention: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

function Get-Median([double[]]$values) {
    if ($null -eq $values -or $values.Count -eq 0) { return 0 }
    $sorted = @($values | Sort-Object)
    $mid = [math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2 -eq 1) { return $sorted[$mid] }
    return ($sorted[$mid - 1] + $sorted[$mid]) / 2
}

function Format-Rate([double]$bytesPerSecond) {
    $units = @("B/s", "KiB/s", "MiB/s", "GiB/s", "TiB/s")
    $value = $bytesPerSecond
    $index = 0
    while ($value -ge 1024 -and $index -lt ($units.Count - 1)) {
        $value = $value / 1024
        $index++
    }
    "{0:N2} {1}" -f $value, $units[$index]
}

$exe = if ($IsWindows) { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --workspace --bins --examples"
}

$threads = @($ThreadSweep -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ } | ForEach-Object { [int]$_ })
if ($threads.Count -lt 2) {
    Exit-With 2 "-ThreadSweep needs at least two counts: the measurement is the shape of the curve."
}
$blocks = @($BlockSizes -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
if ($Iterations -lt 1) {
    Exit-With 2 "-Iterations has to be at least 1."
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path

$layouts = @("shared", "handles", "split")
$runs = [System.Collections.ArrayList]::new()
$blockRuns = [System.Collections.ArrayList]::new()
$commands = [System.Collections.ArrayList]::new()
$failures = [System.Collections.ArrayList]::new()

function Invoke-Bench([string]$layout, [string]$block, [string]$sweep) {
    $arguments = @(
        "bench", "disk",
        "--dir", $Root,
        "--payload-size", $PayloadSize,
        "--block-size", $block,
        "--concurrency-sweep", $sweep,
        "--run-length", $RunLength,
        "--layout", $layout,
        "--file-allocation", $FileAllocation,
        "--format", "json"
    )
    [void]$commands.Add("$bitCli $($arguments -join ' ')")
    $stdout = Join-Path $Root "out.json"
    $stderr = Join-Path $Root "err.txt"
    $process = Start-Process -FilePath $bitCli -ArgumentList $arguments -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    if ($process.ExitCode -ne 0) {
        $message = (Get-Content $stderr -Raw -ErrorAction SilentlyContinue)
        Exit-With 2 "bench disk --layout $layout --block-size $block exited $($process.ExitCode): $message"
    }
    $report = Get-Content $stdout -Raw | ConvertFrom-Json
    Remove-Item $stdout, $stderr -Force -ErrorAction SilentlyContinue
    $report
}

function Add-Steps($report, [System.Collections.ArrayList]$into, [int]$iteration, [string]$block) {
    foreach ($step in $report.disk_steps) {
        if ($step.verified -eq $false) {
            [void]$failures.Add("iteration $iteration, $($step.layout) at $($step.threads) threads in $block blocks read back a block it did not write")
        }
        [void]$into.Add([ordered]@{
            iteration            = $iteration
            layout               = $step.layout
            block_size           = $block
            run_length           = $step.run_length
            threads              = $step.threads
            files                = $step.files
            bytes                = $step.bytes.bytes
            elapsed_ms           = $step.elapsed.ms
            flush_ms             = $step.flush.ms
            rate_bytes           = $step.rate.bytes
            rate_human           = $step.rate.human
            total_write_time_ms  = $step.total_write_time.ms
            write_ops            = $step.write_ops
            write_calls          = $step.write_calls
            mean_write_us        = $step.mean_write_us
            concurrency_achieved = $step.concurrency_achieved
            verified             = $step.verified
        })
    }
}

# ---------------------------------------------------------------------------
# Phase one: where the limit lives
# ---------------------------------------------------------------------------

for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
    $order = if ($iteration % 2 -eq 1) { $layouts } else { @($layouts[2], $layouts[1], $layouts[0]) }
    foreach ($layout in $order) {
        Write-Step "iteration $iteration, layout $layout"
        Add-Steps (Invoke-Bench $layout $BlockSize $ThreadSweep) $runs $iteration $BlockSize
    }
}

$deepest = ($threads | Sort-Object)[-1]
$curve = [System.Collections.ArrayList]::new()
foreach ($count in ($threads | Sort-Object)) {
    $row = [ordered]@{ threads = $count }
    foreach ($layout in $layouts) {
        $matching = @($runs | Where-Object { $_.threads -eq $count -and $_.layout -eq $layout })
        $row["${layout}_rate_bytes"] = [int64](Get-Median ([double[]]@($matching | ForEach-Object { [double]$_.rate_bytes })))
        $row["${layout}_rate_human"] = Format-Rate $row["${layout}_rate_bytes"]
        $row["${layout}_wall_ms"] = [int64](Get-Median ([double[]]@($matching | ForEach-Object { [double]$_.elapsed_ms })))
        $row["${layout}_mean_write_us"] = [int64](Get-Median ([double[]]@($matching | ForEach-Object { [double]$_.mean_write_us })))
    }
    [void]$curve.Add($row)
}

$one = $curve[0]
foreach ($row in $curve) {
    foreach ($layout in $layouts) {
        $base = [double]$one["${layout}_rate_bytes"]
        $row["${layout}_speedup"] = if ($base -gt 0) {
            [math]::Round([double]$row["${layout}_rate_bytes"] / $base, 3)
        } else { 0 }
    }
}

# ---------------------------------------------------------------------------
# Phase two: whether the limit is charged per operation or per byte
# ---------------------------------------------------------------------------

$blockSweep = ($threads | Sort-Object) -join ','
for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
    foreach ($block in $blocks) {
        Write-Step "iteration $iteration, shared layout in $block blocks"
        Add-Steps (Invoke-Bench "shared" $block $blockSweep) $blockRuns $iteration $block
    }
}

$blockCurve = [System.Collections.ArrayList]::new()
foreach ($block in $blocks) {
    $row = [ordered]@{ block_size = $block }
    foreach ($count in ($threads | Sort-Object)) {
        $matching = @($blockRuns | Where-Object { $_.block_size -eq $block -and $_.threads -eq $count })
        $rate = [int64](Get-Median ([double[]]@($matching | ForEach-Object { [double]$_.rate_bytes })))
        $row["t${count}_rate_bytes"] = $rate
        $row["t${count}_rate_human"] = Format-Rate $rate
        $row["t${count}_mean_write_us"] = [int64](Get-Median ([double[]]@($matching | ForEach-Object { [double]$_.mean_write_us })))
        # The asks, not what reached the device. This phase is about the block
        # size, so the number that means something is how many writes the same
        # bytes were split into. Since the write buffer landed the two are not
        # one number, and reporting the device count here would say how well
        # the buffer coalesced rather than how large a write was.
        # See TODO/disk-io.md, T-018.
        $row["t${count}_write_calls"] = [int64](Get-Median ([double[]]@($matching | ForEach-Object { [double]$_.write_calls })))
        $row["t${count}_write_ops"] = [int64](Get-Median ([double[]]@($matching | ForEach-Object { [double]$_.write_ops })))
    }
    [void]$blockCurve.Add($row)
}

# ---------------------------------------------------------------------------
# The verdict
# ---------------------------------------------------------------------------
#
# All three layouts run the same code at one thread, so their one-thread rates
# agree and each layout's speedup is measured against its own.

$deep = $curve | Where-Object { $_.threads -eq $deepest } | Select-Object -First 1
$sharedSpeedup = [double]$deep["shared_speedup"]
$handlesSpeedup = [double]$deep["handles_speedup"]
$splitSpeedup = [double]$deep["split_speedup"]
$bestSplit = [double](($curve | ForEach-Object { [double]$_["split_speedup"] } | Sort-Object)[-1])
$bestHandles = [double](($curve | ForEach-Object { [double]$_["handles_speedup"] } | Sort-Object)[-1])

$verdict = if ($sharedSpeedup -ge 1.5) {
    "neither: one file scales to ${sharedSpeedup}x on its own at $deepest threads, so storage is not what caps a download"
} elseif ($bestHandles -ge 1.5) {
    "the handle: giving one file its own handle per writer reaches ${bestHandles}x where one handle reaches ${sharedSpeedup}x"
} elseif ($bestSplit -ge 1.5) {
    "the file, not the handle: more handles on one file reach ${bestHandles}x, the same writes over separate files reach ${bestSplit}x. Writes to one file serialise whatever handle they arrive on."
} else {
    "not storage: nothing scales past ${bestSplit}x, so the volume is saturated and the layout does not decide it"
}

$fastest = $blockCurve | Select-Object -Last 1
$slowest = $blockCurve | Select-Object -First 1
$perOp = if ([double]$slowest["t${deepest}_rate_bytes"] -gt 0) {
    [math]::Round([double]$fastest["t${deepest}_rate_bytes"] / [double]$slowest["t${deepest}_rate_bytes"], 3)
} else { 0 }
$charge = if ($perOp -ge 1.5) {
    "per operation: at $deepest threads, $($fastest.block_size) writes reach ${perOp}x what $($slowest.block_size) writes reach for the same bytes"
} else {
    "per byte: at $deepest threads, $($fastest.block_size) writes reach only ${perOp}x what $($slowest.block_size) writes reach, so the write size does not decide it"
}

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null
$reportPath = Join-Path $ReportDir "disk-contention-$stamp.json"

[ordered]@{
    schema_version = "1"
    kind           = "disk-contention"
    todo           = "T-017"
    generated_at   = Get-Timestamp
    parameters     = [ordered]@{
        payload_size    = $PayloadSize
        block_size      = $BlockSize
        thread_sweep    = @($threads | Sort-Object)
        block_sizes     = @($blocks)
        iterations      = $Iterations
        file_allocation = $FileAllocation
        profile         = $Profile
    }
    verdict        = $verdict
    charged        = $charge
    layout_curve   = @($curve)
    block_curve    = @($blockCurve)
    layout_runs    = @($runs)
    block_runs     = @($blockRuns)
    commands       = @($commands)
    failures       = @($failures)
    notes          = @(
        "shared is one payload file behind one handle with every thread writing interleaved blocks into it, which is the shape a torrent with one file and several peers has. handles is the same file and the same offsets opened once per thread. split is one file per thread.",
        "All three layouts run identical code at one thread, so the one-thread rates agree and each layout's speedup is measured against its own one-thread rate.",
        "rate covers the write phase only. Each step drains its writeback afterwards, reported as flush_ms and not counted in rate, so a step that filled the page cache does not hand its cost to the step after it.",
        "A payload that fits in the page cache measures the filesystem rather than the device, which is what this check is about. A payload several times larger measures the drive's own write cache running out, which is a real limit and a different one."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "payload: $PayloadSize, $Iterations iterations"
Write-Host "report:  $reportPath"
Write-Host ""
Write-Host "Where the limit lives, in $BlockSize blocks:"
$curve | ForEach-Object {
    [pscustomobject][ordered]@{
        threads   = $_.threads
        shared    = $_["shared_rate_human"]
        handles   = $_["handles_rate_human"]
        split     = $_["split_rate_human"]
        "shared x1"  = $_["shared_speedup"]
        "handles x1" = $_["handles_speedup"]
        "split x1"   = $_["split_speedup"]
    }
} | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "verdict: $verdict"
Write-Host ""
Write-Host "What one write costs, shared layout:"
$blockCurve | ForEach-Object {
    $row = [ordered]@{ block = $_.block_size }
    foreach ($count in ($threads | Sort-Object)) { $row["t$count"] = $_["t${count}_rate_human"] }
    $row["calls"] = $_["t${deepest}_write_calls"]
    $row["device"] = $_["t${deepest}_write_ops"]
    [pscustomobject]$row
} | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "charged: $charge"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-disk-contention: $failure") }
    exit 1
}
exit 0
