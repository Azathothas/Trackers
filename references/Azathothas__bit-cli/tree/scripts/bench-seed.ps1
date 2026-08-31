# Measure a seeder: what leaves, per peer, and what the disk cost to send it.
#
# `bench leech` measures a download. This is the same envelope with every
# counter facing the other way: `uploaded_bytes` per peer rather than
# `downloaded_bytes`, and positioned reads rather than writes, because a
# seeder's storage cost is reading the payload back.
#
# One seeder and N leechers on loopback, all in one session each, so what is
# measured is `bit-cli` serving rather than the network. Three things the run
# reports that a rate on its own cannot say:
#
#   per peer      which leecher took what, and how fast. A seeder serving one
#                 peer well and another badly looks the same in the total.
#   the disk      bytes read, reads, and read time over the measured window.
#                 Against the bytes sent, that is the read amplification: a
#                 seeder reading more than it sends is re-reading.
#   the check     with -IncludeHashCheck, what the hash check on add cost
#                 before the clock started. A seeder with a 40 GB payload
#                 spends minutes there and none of it is serving.
#
# The leechers are rate capped so the run lasts long enough to sample. Without
# a cap a loopback transfer finishes inside one metrics interval and the series
# has one point in it.
#
# **So the default run measures whether the seeder keeps up with N capped
# leechers, not how fast the seeder can go.** The sustained rate is bounded by
# `-Leechers` times `-Rate`, and reading it as a capacity number would be
# reading the cap. For a capacity number, raise `-PayloadSize` until the
# transfer outlasts several metrics intervals on its own and drop the cap:
#
#   pwsh scripts/bench-seed.ps1 -PayloadSize 8GiB -Leechers 4 -Rate 0
#
# Usage:
#   pwsh scripts/bench-seed.ps1
#   pwsh scripts/bench-seed.ps1 -PayloadSize 512MiB -Leechers 4 -Rate 20MiB/s
#
# Exits 0 when the seeder served every leecher and the report carries the
# metrics, 1 when it did not, and 2 when the check could not run. The report
# goes to bench/seed-<timestamp>.json.
#
# See TODO/bench.md, T-090.

[CmdletBinding()]
param(
    [string]$PayloadSize = "256MiB",
    [string]$PieceLength = "1MiB",
    # Leechers pulling at once. Each is its own process and its own session.
    [int]$Leechers = 2,
    # Per-leecher download cap, so the run lasts long enough to sample.
    [string]$Rate = "8MiB/s",
    [string]$Duration = "120s",
    # Stop this long after the last leecher leaves. Without it the seeder waits
    # out --duration with nobody connected and the sustained rate is diluted by
    # the idle tail rather than describing the serving.
    [string]$ExitWhenIdle = "5s",
    [string]$Warmup = "2s",
    [string]$MetricsInterval = "1s",
    [switch]$IncludeHashCheck,
    [string]$Root = ".tmp/bench-seed",
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
    [Console]::Error.WriteLine("bench-seed: $message")
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

function Format-Size([double]$bytes) {
    $units = @("B", "KiB", "MiB", "GiB", "TiB")
    $index = 0
    while ($bytes -ge 1024 -and $index -lt $units.Count - 1) { $bytes /= 1024; $index++ }
    "{0:N2} {1}" -f $bytes, $units[$index]
}

trap { Stop-Background; throw }

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --workspace --bins --examples"
}
if ($Leechers -lt 1) { Exit-With 2 "-Leechers has to be at least 1." }

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path (Join-Path $Root "payload") | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# ---------------------------------------------------------------------------
# The payload
# ---------------------------------------------------------------------------

$payloadBytes = ConvertFrom-Size $PayloadSize
Write-Step "building a payload of $(Format-Size $payloadBytes)"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 31337
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
$expected = (Get-FileHash -Algorithm SHA256 (Join-Path $Root "payload/movie.bin")).Hash.ToLower()

$torrent = Join-Path $Root "movie.torrent"
& $bitCli create (Join-Path $Root "payload") --name payload --piece-length $PieceLength `
    --no-creation-date --output $torrent --force --json 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }

# ---------------------------------------------------------------------------
# The seeder
# ---------------------------------------------------------------------------
#
# The port is chosen once by the OS on a socket that is then closed, because
# the leechers have to be told an address before the seeder is up and
# `--port 0` would not tell them one they can use.

$probe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$probe.Start()
$port = $probe.LocalEndpoint.Port
$probe.Stop()

$reportPath = Join-Path $ReportDir "seed-$((Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssfffZ')).json"
$seedArgs = @(
    "bench", "seed", $torrent,
    "--data", $Root,
    "--port", "$port",
    "--no-dht", "--no-lsd", "--no-tracker",
    "--duration", $Duration,
    "--warmup", $Warmup,
    "--metrics-interval", $MetricsInterval,
    "--exit-when-idle", $ExitWhenIdle,
    "--report", $reportPath,
    "--format", "json"
)
if ($IncludeHashCheck) { $seedArgs += "--include-hash-check" }
$commands = [System.Collections.ArrayList]::new()
[void]$commands.Add("bit-cli $($seedArgs -join ' ')")

Write-Step "seeding $(Format-Size $payloadBytes) on 127.0.0.1:$port for $Duration"
$seedOut = Join-Path $Root "seed.out"
$seedErr = Join-Path $Root "seed.err"
$seeder = Start-Process -FilePath $bitCli -WorkingDirectory $repo -NoNewWindow -PassThru `
    -ArgumentList $seedArgs -RedirectStandardOutput $seedOut -RedirectStandardError $seedErr
$script:Background += $seeder

$deadline = (Get-Date).AddSeconds(120)
$listening = $false
while (-not $listening -and (Get-Date) -lt $deadline) {
    if ($seeder.HasExited) { Exit-With 2 "the seeder exited before it listened; see $seedErr" }
    $listening = $null -ne (Get-NetTCPConnection -State Listen -OwningProcess $seeder.Id -ErrorAction SilentlyContinue |
            Where-Object { $_.LocalPort -eq $port } | Select-Object -First 1)
    if (-not $listening) { Start-Sleep -Milliseconds 250 }
}
if (-not $listening) { Exit-With 2 "the seeder never listened on $port" }
Write-Step "seeder listening, starting $Leechers leecher(s) at $Rate each"

# ---------------------------------------------------------------------------
# The leechers
# ---------------------------------------------------------------------------

$leechProcs = @()
for ($i = 0; $i -lt $Leechers; $i++) {
    $out = Join-Path $Root "leech-$i"
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    $arguments = @(
        "download", $torrent,
        "--dir", $out,
        "--peer", "127.0.0.1:$port",
        "--no-dht", "--no-lsd", "--no-tracker", "--no-web-seed",
        "--port", "0",
        "--json"
    )
    # `-Rate 0` means no cap, for the capacity run described in the header.
    if ($Rate -and $Rate -ne "0") { $arguments += @("--max-download-rate", $Rate) }
    if ($i -eq 0) { [void]$commands.Add("bit-cli $($arguments -join ' ')") }
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $repo -NoNewWindow -PassThru `
        -ArgumentList $arguments `
        -RedirectStandardOutput (Join-Path $Root "leech-$i.json") `
        -RedirectStandardError (Join-Path $Root "leech-$i.err")
    $script:Background += $process
    $leechProcs += [pscustomobject]@{ index = $i; process = $process; out = $out }
}

$clock = [System.Diagnostics.Stopwatch]::StartNew()
foreach ($leecher in $leechProcs) {
    $left = $TimeoutSeconds * 1000 - $clock.ElapsedMilliseconds
    if ($left -lt 1000) { $left = 1000 }
    if (-not $leecher.process.WaitForExit([int]$left)) {
        Stop-Process -Id $leecher.process.Id -Force -ErrorAction SilentlyContinue
    }
}
$clock.Stop()
Write-Step "every leecher finished after $([math]::Round($clock.Elapsed.TotalSeconds, 1))s"

# The seeder runs out its --duration. Waiting for it rather than killing it is
# what makes the report complete.
if (-not $seeder.WaitForExit($TimeoutSeconds * 1000)) {
    Stop-Process -Id $seeder.Id -Force -ErrorAction SilentlyContinue
    Exit-With 1 "the seeder did not finish within ${TimeoutSeconds}s"
}
$seedCode = $seeder.ExitCode
Stop-Background

# ---------------------------------------------------------------------------
# What each side says
# ---------------------------------------------------------------------------

$report = $null
try { $report = Get-Content $reportPath -Raw | ConvertFrom-Json } catch { }
if (-not $report) { Exit-With 1 "the seeder wrote no report to $reportPath" }

$leechResults = @()
foreach ($leecher in $leechProcs) {
    $doc = $null
    try { $doc = Get-Content (Join-Path $Root "leech-$($leecher.index).json") -Raw | ConvertFrom-Json } catch { }
    $landed = Join-Path $leecher.out "payload/movie.bin"
    $hash = if (Test-Path $landed) { (Get-FileHash -Algorithm SHA256 $landed).Hash.ToLower() } else { $null }
    $leechResults += [ordered]@{
        index        = $leecher.index
        exit_code    = $leecher.process.ExitCode
        downloaded   = if ($doc) { [int64]$doc.downloaded.bytes } else { 0 }
        from_peers   = if ($doc) { [int64]$doc.from_peers.bytes } else { 0 }
        elapsed_ms   = if ($doc) { [int64]$doc.elapsed_ms } else { 0 }
        sha256       = $hash
        hash_matches = ($hash -eq $expected)
    }
}

$peerRows = @($report.sources)
$sentTotal = 0
foreach ($row in $peerRows) { $sentTotal += [int64]$row.bytes.bytes }
$pulled = 0
foreach ($result in $leechResults) { $pulled += [int64]$result.from_peers }

$failures = [System.Collections.ArrayList]::new()
if ($seedCode -ne 0) {
    [void]$failures.Add("the seeder exited $seedCode")
}
foreach ($result in $leechResults) {
    if ($result.exit_code -ne 0 -or -not $result.hash_matches) {
        [void]$failures.Add("leecher $($result.index) exited $($result.exit_code) and its payload $(if ($result.hash_matches) { 'matches' } else { 'differs' })")
    }
}
if ($peerRows.Count -lt $Leechers) {
    [void]$failures.Add("$($peerRows.Count) peer row(s) for $Leechers leecher(s): a seeder that cannot say who pulled from it has not measured anything")
}
if ($report.summary.bytes.bytes -le 0) {
    [void]$failures.Add("the seeder reported sending nothing")
}
if (-not $report.summary.disk -or $report.summary.disk.read_bytes.bytes -le 0) {
    [void]$failures.Add("the seeder reported no disk reads, so nothing measured what the payload cost to serve")
}
if (-not $report.environment.process -or $report.environment.process.peak_rss_bytes -le 0) {
    [void]$failures.Add("the report carries no peak RSS")
}

# Read amplification: bytes off the disk against bytes onto the wire. Above
# one means the seeder re-read something, which is what a cache that is too
# small looks like.
$amplification = $null
if ($report.summary.bytes.bytes -gt 0 -and $report.summary.disk) {
    $amplification = [math]::Round($report.summary.disk.read_bytes.bytes / $report.summary.bytes.bytes, 3)
}

$verdict = switch ($true) {
    ($failures.Count -eq 0) {
        "the seeder sent $(Format-Size $report.summary.bytes.bytes) to $($peerRows.Count) peer(s) at $($report.summary.sustained_rate.human), reading $(Format-Size $report.summary.disk.read_bytes.bytes) to do it"
        break
    }
    default { "$($failures.Count) thing(s) did not hold"; break }
}

$summaryPath = Join-Path $ReportDir "bench-seed-$((Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssfffZ')).json"
[ordered]@{
    kind           = "bench-seed"
    schema_version = "1"
    generated_at   = Get-Timestamp
    parameters     = [ordered]@{
        payload_size     = $PayloadSize
        payload_bytes    = $payloadBytes
        piece_length     = $PieceLength
        leechers         = $Leechers
        rate             = $Rate
        duration         = $Duration
        warmup           = $Warmup
        metrics_interval = $MetricsInterval
        exit_when_idle   = $ExitWhenIdle
        include_hash_check = [bool]$IncludeHashCheck
        port             = $port
        profile          = $Profile
    }
    payload_sha256 = $expected
    seed_report    = $reportPath
    seed_exit_code = $seedCode
    seeder         = [ordered]@{
        bytes_sent      = $report.summary.bytes
        sustained_rate  = $report.summary.sustained_rate
        peak_rate       = $report.summary.peak_rate
        peak_peers      = $report.summary.peak_peers
        disk            = $report.summary.disk
        hashing         = $report.summary.hashing
        samples         = @($report.series).Count
        peers           = @($peerRows | ForEach-Object {
                [ordered]@{ label = $_.label; kind = $_.kind; bytes = $_.bytes; rate = $_.rate }
            })
        peak_rss_bytes  = $report.environment.process.peak_rss_bytes
        cpu_ms          = $report.environment.process.cpu_ms
        open_handles    = $report.environment.process.open_handles
    }
    leechers_seen  = $peerRows.Count
    leech_results  = @($leechResults)
    bytes_sent     = $sentTotal
    bytes_pulled   = $pulled
    read_amplification = $amplification
    verdict        = $verdict
    failures       = @($failures)
    commands       = @($commands)
    notes          = @(
        "Every counter faces the other way from bench leech: uploaded_bytes per peer rather than downloaded_bytes, and positioned reads rather than writes.",
        "The leechers are rate capped so the run lasts long enough for the metrics interval to sample it. Without a cap a loopback transfer finishes inside one interval and the series has one point in it.",
        "read_amplification is bytes off the disk over bytes onto the wire. One means every byte read was sent once; above one means the seeder re-read something.",
        "The sustained rate covers the whole measured window including any time with no peer connected, so it is bounded below by how long the leechers took to arrive and to leave. peak_rate is the best single interval and is the one to compare between runs.",
        "bytes_sent is the seeder's own accounting and bytes_pulled is the sum of what the leechers say they took from peers. They are two measurements of the same transfer taken at opposite ends."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $summaryPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "payload:  $(Format-Size $payloadBytes), $Leechers leecher(s) at $Rate"
Write-Host "reports:  $reportPath"
Write-Host "          $summaryPath"
Write-Host ""
$peerRows | ForEach-Object {
    [pscustomobject][ordered]@{
        peer   = $_.label
        kind   = $_.kind
        sent   = $_.bytes.human
        rate   = $_.rate.human
    }
} | Format-Table -AutoSize | Out-String | Write-Host
Write-Host ("sent {0} at {1} sustained, {2} peak; read {3} off the disk over {4} reads" -f `
        $report.summary.bytes.human, $report.summary.sustained_rate.human,
    $report.summary.peak_rate.human,
    $report.summary.disk.read_bytes.human, $report.summary.disk.read_ops)
if ($null -ne $amplification) { Write-Host "read amplification: $amplification" }
if ($report.summary.hashing) {
    Write-Host ("hash check on add: {0} pieces, {1} in {2}ms at {3}" -f `
            $report.summary.hashing.pieces, $report.summary.hashing.bytes.human,
        $report.summary.hashing.total.ms, $report.summary.hashing.rate.human)
}
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("bench-seed: $failure") }
    exit 1
}
exit 0
