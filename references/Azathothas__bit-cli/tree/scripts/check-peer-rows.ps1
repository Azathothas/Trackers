# What one peer costs a seeder that will never see it again.
#
# This is the attribution measurement `TODO/memory.md` T-040 is open on. The
# slope there is 0.804 MiB an hour under the `steady` workload, linear, and
# collinear with the leech completion rate of 228.5 an hour, so wall clock and
# completions cannot be told apart by running for longer. This tells them
# apart by holding time almost constant and moving the peer count instead.
#
# The candidate: `librqbit` keeps a row for every peer it has ever accepted and
# never reclaims one. Measured separately at 24 handshakes and 24 rows, all in
# `not needed`, with `live` and `dead` both zero. If that row is what the slope
# is made of, RSS is linear in the peer count and the intercept is the seeder.
#
# `loopback-churn` is the driver: it connects, completes a BEP 3
# handshake, and closes, with no payload and no tracker, so a peer row is the
# only thing each connection leaves behind. RSS and the row count both come
# out of the seeder's own `progress` events, because sampling from outside
# samples a process that may already have exited.
#
# Usage:
#   pwsh scripts/check-peer-rows.ps1
#   pwsh scripts/check-peer-rows.ps1 -Step 500 -Steps 20
#
# The default reaches 2,000 rows in steps of 200, which is the same order as
# the 398 the soak in T-040 reached in 1.76 hours. Going much higher measures
# something else: every `progress` event serialises the whole peer list, so a
# run holding tens of thousands of rows is paying for the reporting as well as
# for the rows, and that is a regime the soak never entered.
#
# It also carries the acceptance for `--max-rss`, which is the backstop this
# growth gets until a peer row can be reclaimed: a ceiling any process is over
# must stop the run with exit 16 and `"stopped": "rss_ceiling"`, and one
# nothing is near must not.
#
# Exits 0 when the run completed and wrote its record, 1 when the fit says the
# candidate is wrong or the ceiling did not behave, and 2 when the check could
# not run. The record goes to
# bench/peer-rows-<timestamp>.json.
#
# See TODO/memory.md, T-040, and TODO/peers.md, T-020.

[CmdletBinding()]
param(
    [int]$Step = 200,
    [int]$Steps = 10,
    [int]$Concurrency = 16,
    [int]$PayloadMiB = 2,
    # MAX_PEER_RECORDS in
    # vendor/rqbit/crates/librqbit/src/torrent_state/live/peers/mod.rs. Kept as
    # a parameter rather than read from the source, because this script has to
    # be able to say the tree disagrees with it rather than agree by
    # construction. -Step * -Steps has to go past it or the bound is not
    # exercised and this says so.
    [int]$PeerRecordBound = 1024,
    [string]$Root = ".tmp/peerrows",
    [string]$ReportDir = "bench",
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$script:Seeder = $null

function Stop-Background {
    if ($script:Seeder -and -not $script:Seeder.HasExited) {
        try { $script:Seeder.Kill() } catch { }
    }
    $script:Seeder = $null
}

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-peer-rows: $message")
    Stop-Background
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

trap { Stop-Background; throw }

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/release/bit-cli$exe"
$churn = Join-Path $repo "target/release/examples/loopback-churn$exe"
foreach ($needed in @($bitCli, $churn)) {
    if (-not (Test-Path $needed)) {
        Exit-With 2 "missing $needed. Build it first: cargo build --release --workspace --bins --examples"
    }
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

$serve = Join-Path $Root "serve"
New-Item -ItemType Directory -Force -Path $serve | Out-Null
Write-Step "building a $PayloadMiB MiB payload"
$payloadBytes = [byte[]]::new($PayloadMiB * 1024 * 1024)
[int64]$state = 13
for ($i = 0; $i -lt $payloadBytes.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $payloadBytes[$i] = [byte](($state -shr 16) -band 0xFF)
}
[System.IO.File]::WriteAllBytes((Join-Path $serve "payload.bin"), $payloadBytes)

$torrent = Join-Path $Root "payload.torrent"
$createProc = Start-Process -FilePath $bitCli -ArgumentList @(
    "create", (Join-Path $serve "payload.bin"), "--piece-length", "256KiB",
    "--no-creation-date", "--output", $torrent, "--force", "--json"
) -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $Root "create.out") `
    -RedirectStandardError (Join-Path $Root "create.err")
$createProc.WaitForExit(60000) | Out-Null
if ($createProc.ExitCode -ne 0) {
    Exit-With 2 "bit-cli create exited $($createProc.ExitCode)"
}
$infoHash = (Get-Content (Join-Path $Root "create.out") -Raw | ConvertFrom-Json).info_hash
if (-not $infoHash) { Exit-With 2 "the created torrent reported no info hash" }

$out = Join-Path $Root "seed.out"
$err = Join-Path $Root "seed.err"
$script:Seeder = Start-Process -FilePath $bitCli -ArgumentList @(
    "--jsonl", "seed", $torrent, "--dir", $serve, "--port", "0",
    "--no-tracker", "--no-dht", "--no-lsd", "--stop-after", "3600s",
    "--report-interval", "5s"
) -PassThru -NoNewWindow -RedirectStandardOutput $out -RedirectStandardError $err

$addr = $null
for ($attempt = 0; $attempt -lt 200; $attempt++) {
    Start-Sleep -Milliseconds 100
    if ($script:Seeder.HasExited) { Exit-With 2 "the seeder exited $($script:Seeder.ExitCode)" }
    foreach ($line in (Get-Content $out -ErrorAction SilentlyContinue)) {
        try { $event = $line | ConvertFrom-Json } catch { continue }
        if ($event.listen_addr) { $addr = $event.listen_addr }
    }
    if ($addr) { break }
}
if (-not $addr) { Exit-With 2 "the seeder never printed a listen address" }
$target = "127.0.0.1:$(($addr -split ':')[-1])"
Write-Step "seeder on $target, info hash $infoHash"

# The seeder's own last `progress` event: its RSS, its handle count, and how
# many peer rows it is holding.
function Get-Sample {
    $last = $null
    foreach ($line in (Get-Content $out -ErrorAction SilentlyContinue)) {
        try { $event = $line | ConvertFrom-Json } catch { continue }
        if ($null -ne $event.uploaded_bytes) { $last = $event }
    }
    if (-not $last) { return $null }
    [pscustomobject]@{
        rss_bytes = $last.process.rss_bytes
        handles   = $last.process.open_handles
        rows      = @($last.peer_detail).Count
        seen      = $last.peers.seen
    }
}

# One sample after the seeder has settled and before any peer has touched it.
Start-Sleep -Seconds 12
$baseline = Get-Sample
if (-not $baseline) { Exit-With 2 "the seeder wrote no progress event" }
Write-Step ("baseline: rss {0:N2} MiB, handles {1}, rows {2}" -f ($baseline.rss_bytes / 1MB), $baseline.handles, $baseline.rows)

$rows = @([pscustomobject][ordered]@{
        connections = 0
        peer_rows   = $baseline.rows
        peers_seen  = $baseline.seen
        rss_bytes   = $baseline.rss_bytes
        handles     = $baseline.handles
    })

$total = 0
for ($k = 1; $k -le $Steps; $k++) {
    if ($script:Seeder.HasExited) { Exit-With 2 "the seeder exited during step $k" }
    $churnOut = Join-Path $Root "churn$k.out"
    # Handshaking is loopback-churn's default and there is no --handshake to
    # ask for it. Passing one is an unknown argument and exit 2, which is how
    # this script first measured a seeder nothing had connected to.
    $proc = Start-Process -FilePath $churn -ArgumentList @(
        "--peer", $target, "--info-hash", $infoHash, "--connections", "$Step",
        "--concurrency", "$Concurrency"
    ) -PassThru -NoNewWindow -RedirectStandardOutput $churnOut `
        -RedirectStandardError (Join-Path $Root "churn$k.err")
    if (-not $proc.WaitForExit(600000)) {
        try { $proc.Kill() } catch { }
        Exit-With 2 "loopback-churn did not finish step $k"
    }
    if ($proc.ExitCode -ne 0) {
        Exit-With 2 "loopback-churn exited $($proc.ExitCode) on step ${k}: $(Get-Content (Join-Path $Root "churn$k.err") -Raw)"
    }
    $total += $Step
    # Two report intervals and a margin, so the sample read below is one the
    # seeder took after the last connection closed rather than during the
    # burst.
    Start-Sleep -Seconds 12
    $sample = Get-Sample
    $rows += [pscustomobject][ordered]@{
        connections = $total
        peer_rows   = $sample.rows
        peers_seen  = $sample.seen
        rss_bytes   = $sample.rss_bytes
        handles     = $sample.handles
    }
    Write-Step ("{0,6} connections: rows {1,6}  rss {2,7:N2} MiB  handles {3}" -f `
            $total, $sample.rows, ($sample.rss_bytes / 1MB), $sample.handles)
}

# One more sample after a minute of nothing, so a rise that is transient
# allocation can be told from one that is retained. Rows that are held cost
# what they cost; a number that falls away was the reporting.
Write-Step "settling for 60s with no traffic"
Start-Sleep -Seconds 60
$settled = Get-Sample
Write-Step ("settled:  rows {0,6}  rss {1,7:N2} MiB  handles {2}" -f `
        $settled.rows, ($settled.rss_bytes / 1MB), $settled.handles)

Stop-Background

# ---------------------------------------------------------------------------
# The backstop: --max-rss
# ---------------------------------------------------------------------------
#
# Nothing here frees a peer row, so what is carried is a bound. Two runs: a
# ceiling under any real process, which must stop on the first sample, and one
# nothing is near, which must reach its own deadline instead. The second is
# what proves the first stopped for the ceiling rather than for anything else.

function Invoke-Ceiling([string]$name, [string]$ceiling) {
    $so = Join-Path $Root "$name.out"
    $se = Join-Path $Root "$name.err"
    $proc = Start-Process -FilePath $bitCli -ArgumentList @(
        "--json", "seed", $torrent, "--dir", $serve, "--port", "0",
        "--no-tracker", "--no-dht", "--no-lsd", "--stop-after", "15s",
        "--report-interval", "1s", "--max-rss", $ceiling
    ) -PassThru -NoNewWindow -RedirectStandardOutput $so -RedirectStandardError $se
    if (-not $proc.WaitForExit(120000)) { try { $proc.Kill() } catch { }; return $null }
    $report = $null
    try { $report = Get-Content $so -Raw | ConvertFrom-Json } catch { }
    [pscustomobject]@{ exit_code = $proc.ExitCode; stopped = $report.stopped; rss = $report.process.rss_bytes }
}

Write-Step "case ceiling (--max-rss 1MiB, which any real process is over)"
$over = Invoke-Ceiling "ceiling_over" "1MiB"
Write-Step "case headroom (--max-rss 4GiB, which nothing is near)"
$under = Invoke-Ceiling "ceiling_under" "4GiB"

$ceilingFailures = @()
if ($over.exit_code -ne 16) { $ceilingFailures += "--max-rss 1MiB exited $($over.exit_code), expected 16" }
if ($over.stopped -ne "rss_ceiling") { $ceilingFailures += "--max-rss 1MiB stopped=$($over.stopped), expected rss_ceiling" }
if ($under.exit_code -ne 9) { $ceilingFailures += "--max-rss 4GiB exited $($under.exit_code), expected 9" }
if ($under.stopped -ne "deadline") { $ceilingFailures += "--max-rss 4GiB stopped=$($under.stopped), expected deadline" }
foreach ($failure in $ceilingFailures) { [Console]::Error.WriteLine("check-peer-rows: $failure") }

# The bound, and it is what makes the fit below need a subset.
#
# Until 2026-08-22 the session kept a row for every peer it had ever accepted
# and never gave one back, which is what this script was written to measure and
# is T-040. The vendored tree reclaims a `NotNeeded` or `Dead` row now, so the
# row count tracks connections up to MAX_PEER_RECORDS and is flat above it. Both
# halves are asserted: one row per handshake below the bound is still the
# attribution this script exists for, and flat above it is the fix.
$bounded = @($rows | Where-Object { $_.peer_rows -lt $PeerRecordBound })
$aboveBound = @($rows | Where-Object { $_.connections -gt $PeerRecordBound })
$boundFailures = @()
foreach ($row in $bounded) {
    if ($row.peer_rows -ne $row.connections) {
        $boundFailures += "at $($row.connections) connections the session held $($row.peer_rows) rows, and below the bound it is one per handshake"
    }
}
foreach ($row in $aboveBound) {
    if ($row.peer_rows -gt $PeerRecordBound) {
        $boundFailures += "at $($row.connections) connections the session held $($row.peer_rows) rows, past the bound of $PeerRecordBound"
    }
}
if ($aboveBound.Count -lt 2) {
    $boundFailures += "only $($aboveBound.Count) step(s) went past $PeerRecordBound connections, so the bound was not exercised"
}
foreach ($failure in $boundFailures) { [Console]::Error.WriteLine("check-peer-rows: $failure") }

# Least squares of rss against peer rows, over the steps below the bound only.
# Above it the row count is constant, so those points measure the intercept
# several times and flatten the slope toward nothing.
$n = $bounded.Count
$sumX = 0.0; $sumY = 0.0; $sumXX = 0.0; $sumXY = 0.0
foreach ($row in $bounded) {
    $x = [double]$row.peer_rows
    $y = [double]$row.rss_bytes
    $sumX += $x; $sumY += $y; $sumXX += $x * $x; $sumXY += $x * $y
}
$denominator = ($n * $sumXX) - ($sumX * $sumX)
$slope = if ($denominator -ne 0) { (($n * $sumXY) - ($sumX * $sumY)) / $denominator } else { 0 }
$intercept = ($sumY - ($slope * $sumX)) / $n
$meanY = $sumY / $n
$ssTot = 0.0; $ssRes = 0.0
foreach ($row in $bounded) {
    $predicted = $intercept + ($slope * [double]$row.peer_rows)
    $ssTot += [math]::Pow([double]$row.rss_bytes - $meanY, 2)
    $ssRes += [math]::Pow([double]$row.rss_bytes - $predicted, 2)
}
$r2 = if ($ssTot -gt 0) { 1 - ($ssRes / $ssTot) } else { 0 }

# T-040's own numbers, so the two measurements are compared here rather than
# by hand later: 0.804 MiB an hour against 228.5 completions an hour.
$soakSlopeBytesPerHour = 0.804 * 1MB
$soakCompletionsPerHour = 228.5
$impliedBytesPerPeer = $soakSlopeBytesPerHour / $soakCompletionsPerHour

$rowsHeld = ($rows | Select-Object -Last 1).peer_rows
$verdict = if ($ceilingFailures.Count -gt 0) {
    "rejected: --max-rss did not behave"
}
elseif ($boundFailures.Count -gt 0) {
    "rejected: peer rows are not bounded the way T-040 says"
}
elseif ($slope -le 0) {
    "rejected: rss does not rise with the peer count below the bound"
}
else { "measured" }

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "peer-rows-$stamp.json"
[pscustomobject][ordered]@{
    kind           = "peer_row_cost"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    }
    parameters     = [ordered]@{
        step        = $Step
        steps       = $Steps
        concurrency = $Concurrency
        payload_mib = $PayloadMiB
    }
    samples        = @($rows)
    settled        = [ordered]@{
        peer_rows = $settled.rows
        rss_bytes = $settled.rss_bytes
        handles   = $settled.handles
    }
    ceiling        = [ordered]@{
        over_exit      = $over.exit_code
        over_stopped   = $over.stopped
        under_exit     = $under.exit_code
        under_stopped  = $under.stopped
    }
    fit            = [ordered]@{
        bytes_per_peer_row = [math]::Round($slope, 1)
        intercept_bytes    = [math]::Round($intercept, 0)
        r_squared          = [math]::Round($r2, 4)
        fitted_over_steps  = $n
    }
    bound          = [ordered]@{
        max_peer_records = $PeerRecordBound
        rows_at_end      = $rowsHeld
        steps_past_bound = $aboveBound.Count
        failures         = @($boundFailures)
    }
    against_t040   = [ordered]@{
        slope_bytes_per_hour       = [math]::Round($soakSlopeBytesPerHour, 0)
        completions_per_hour       = $soakCompletionsPerHour
        implied_bytes_per_peer_row = [math]::Round($impliedBytesPerPeer, 1)
    }
    verdict        = $verdict
    notes          = @(
        "loopback-churn --handshake leaves a peer row and nothing else: no payload moves, no tracker announces, and the handshake is for the info hash the seeder holds.",
        "RSS and the row count are the seeder's own, read out of its progress events. A sampler outside the process measures a different thing and cannot read the row count at all.",
        "The comparison against T-040 is arithmetic on that entry's recorded numbers, not a second soak. What it answers is whether a peer row is the right size to be the slope.",
        "The settled sample is a minute of no traffic at the end. RSS that falls back was the reporting rather than the rows.",
        "The fit is over the steps below max_peer_records only. Above the bound the row count is constant, so those points measure the intercept again and flatten the slope toward nothing.",
        "Rows being flat above the bound is T-040 fixed, and one row per handshake below it is the attribution this script was written for. Both are asserted, because a bound that reclaimed a live peer would also make the count flat."
    )
} | ConvertTo-Json -Depth 10 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
$rows | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
Write-Host ("bytes per peer row: {0:N1}  (r squared {1:N4}, intercept {2:N2} MiB)" -f $slope, $r2, ($intercept / 1MB))
Write-Host ("T-040 implies:      {0:N1} per completion, from 0.804 MiB/h over 228.5/h" -f $impliedBytesPerPeer)
Write-Host "report:  $reportPath"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
if ($verdict -ne "measured") { exit 1 }
exit 0
