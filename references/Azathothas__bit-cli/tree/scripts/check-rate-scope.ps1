# Which rate cap bounds which source.
#
# `TODO/multi-source.md` T-132: `--max-download-rate` and
# `--max-overall-download-rate` go into `librqbit`'s limiter, HTTP sources
# reach the session as peers over loopback, and `--web-seed-speed-limit` caps
# HTTP only. So two of the three directions exist and the third, capping peers
# alone, does not. The entry says the asymmetry is not documented; this is what
# documents it, in numbers rather than in prose.
#
# The report already splits what arrived: `from_peers` and `from_web_seeds` are
# top-level fields of `bit-cli download --json`, so a run with both kinds of
# source says which cap bit which one without inference.
#
# Ten phases against one payload, one mirror, and one seeder:
#
#   http_ceiling          HTTP only, uncapped. What the mirror can do.
#   http_session_cap      HTTP only, --max-overall-download-rate. This was
#                         T-132's premise: a session cap bounds HTTP too.
#   http_webseed_cap      HTTP only, --web-seed-speed-limit. The per-source
#                         bucket, which is T-035 and already closed.
#   http_peer_cap         HTTP only, --max-peer-rate. **Must exceed it.** The
#                         bridge is not a swarm peer, so the swarm cap does
#                         not reach it. This is the decisive row.
#   peer_ceiling          Peers only, uncapped. What the seeder can do, and
#                         what says the row below measured something.
#   peer_peer_cap         Peers only, --max-peer-rate. Must hold.
#   hybrid_ceiling        Both sources, uncapped. Recorded, not judged: the
#                         split between them is a scheduling outcome.
#   hybrid_webseed_cap    Both sources, --web-seed-speed-limit only. HTTP is
#                         bounded and the run is not, because nothing was
#                         asked to bound the peer.
#   hybrid_session_cap    Both sources, --max-overall-download-rate. The total
#                         is bounded and neither side is bounded on its own.
#   hybrid_both_caps      Both sources, --max-peer-rate and
#                         --web-seed-speed-limit at different rates. Each side
#                         stays under its own, in one run and one report.
#
# **What is deliberately not judged, and why the entry's acceptance is not
# taken literally.** T-132 asks for "peer bytes within 10% of 10 MiB/s and HTTP
# bytes within 10% of 50 MiB/s". The upper half of that is a cap and is judged.
# The lower half asks each source to be *at* its cap, which is a scheduling
# outcome: the picker decides how much each source is asked for, and TODO/
# RULES.md section 5 says a fixture must not assert one. It is arranged instead,
# which is what that rule says to do: http_peer_cap and peer_peer_cap each make
# one source the only supplier, so "the cap binds peers" and "the cap does not
# bind HTTP" are invariants rather than races.
#
# What is judged and what is only recorded matters here. A cap is an invariant
# and is judged. A split between two sources racing each other is not: TODO/
# RULES.md section 5 says a fixture asserting that two things both did some
# work is asserting a scheduling outcome it does not control, so the splits are
# recorded and the caps are judged.
#
# Usage:
#   pwsh scripts/check-rate-scope.ps1
#   pwsh scripts/check-rate-scope.ps1 -Rate 4MiB/s -PayloadMiB 256
#
# Exits 0 when every cap holds, 1 when one does not, and 2 when the check could
# not run. The record goes to bench/rate-scope-<timestamp>.json.
#
# See TODO/multi-source.md, T-132.

[CmdletBinding()]
param(
    [string]$Rate = "8MiB/s",
    # The web seed cap in the two-cap phase. Different from -Rate on purpose:
    # one report has to show each side held to its own number, and two equal
    # numbers cannot show which cap did which.
    [string]$WebSeedRate = "24MiB/s",
    [int]$PayloadMiB = 128,
    # Fraction a capped run may exceed its cap by. A limiter that lets a burst
    # through and then pauses is still a limiter.
    [double]$Tolerance = 0.15,
    [string]$Root = ".tmp/rate-scope",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
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

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-rate-scope: $message")
    Stop-Background
    exit $code
}

trap { Stop-Background; throw }

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

function Format-Rate([double]$bytesPerSecond) {
    "{0:N2} MiB/s" -f ($bytesPerSecond / 1MB)
}

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
$fileserver = Join-Path $repo "target/$Profile/examples/loopback-fileserver$exe"
foreach ($required in @($bitCli, $fileserver)) {
    if (-not (Test-Path $required)) {
        Exit-With 2 "missing $required. Build it first: cargo build --$Profile --workspace --bins --examples"
    }
}

$rateBytes = ConvertFrom-Size $Rate
$webSeedRateBytes = ConvertFrom-Size $WebSeedRate
if ($rateBytes -lt 1) { Exit-With 2 "-Rate has to be positive." }
if ($webSeedRateBytes -lt 1) { Exit-With 2 "-WebSeedRate has to be positive." }

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# ---------------------------------------------------------------------------
# A payload, a torrent, a mirror, and a seeder
# ---------------------------------------------------------------------------
#
# Pseudo-random rather than zeroes: a run of zeroes is what a filesystem or an
# HTTP layer elides, and either would measure the shortcut instead of the cap.

$serve = Join-Path $Root "payload"
New-Item -ItemType Directory -Force -Path $serve | Out-Null
Write-Step "building a $PayloadMiB MiB payload"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 8675309
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}
# Two files, so the torrent is unambiguously multi-file and the web seed
# composition is the directory form rather than the single-file one.
foreach ($file in @("movie.bin", "notes.txt")) {
    $want = if ($file -eq "notes.txt") { [int64]4096 } else { [int64]$PayloadMiB * 1MB - 4096 }
    $stream = [System.IO.File]::Create((Join-Path $serve $file))
    try {
        [int64]$written = 0
        while ($written -lt $want) {
            $take = [Math]::Min([int64]$block.Length, $want - $written)
            $stream.Write($block, 0, [int]$take)
            $written += $take
        }
    }
    finally { $stream.Dispose() }
}

$torrent = Join-Path $Root "payload.torrent"
Push-Location $Root
try {
    & $bitCli create payload --name payload --piece-length 1MiB --no-creation-date `
        --output $torrent --force --json 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }
}
finally { Pop-Location }

Write-Step "starting the mirror"
$serverOut = Join-Path $Root "fileserver.out"
$server = Start-Process -FilePath $fileserver -ArgumentList @("--root", $Root) `
    -PassThru -NoNewWindow -RedirectStandardOutput $serverOut `
    -RedirectStandardError (Join-Path $Root "fileserver.err")
$script:Background += $server
$webSeed = $null
for ($attempt = 0; $attempt -lt 150; $attempt++) {
    Start-Sleep -Milliseconds 100
    if ($server.HasExited) { Exit-With 2 "the mirror exited $($server.ExitCode)" }
    $line = (Get-Content $serverOut -ErrorAction SilentlyContinue) |
        Where-Object { $_ -match '^http' } | Select-Object -First 1
    if ($line) { $webSeed = $line.Trim(); break }
}
if (-not $webSeed) { Exit-With 2 "the mirror never printed a URL" }
Write-Step "  mirror at $webSeed"

# The seeder's port comes out of its own event stream, never chosen here: a
# port this script picked could already be in use, and dialling it would
# measure whatever else was listening.
Write-Step "starting the seeder"
$seedOut = Join-Path $Root "seed.out"
$seeder = Start-Process -FilePath $bitCli -ArgumentList @(
    "--jsonl", "seed", $torrent, "--data", $Root, "--port", "0",
    "--no-tracker", "--no-dht", "--no-lsd", "--stop-after", "1800s"
) -PassThru -NoNewWindow -RedirectStandardOutput $seedOut `
    -RedirectStandardError (Join-Path $Root "seed.err")
$script:Background += $seeder
$peer = $null
for ($attempt = 0; $attempt -lt 300; $attempt++) {
    Start-Sleep -Milliseconds 100
    if ($seeder.HasExited) { Exit-With 2 "the seeder exited $($seeder.ExitCode): $(Get-Content (Join-Path $Root 'seed.err') -Raw)" }
    foreach ($line in (Get-Content $seedOut -ErrorAction SilentlyContinue)) {
        try { $event = $line | ConvertFrom-Json } catch { continue }
        if ($event.listen_addr) { $peer = "127.0.0.1:$(($event.listen_addr -split ':')[-1])" }
    }
    if ($peer) { break }
}
if (-not $peer) { Exit-With 2 "the seeder never printed a listen address" }
Write-Step "  seeder at $peer"

# ---------------------------------------------------------------------------
# Running one phase
# ---------------------------------------------------------------------------

$commands = [System.Collections.ArrayList]::new()

function Invoke-Phase([string]$label, [bool]$withPeer, [bool]$withHttp, [string[]]$extra) {
    $outDir = Join-Path $Root "out-$label"
    if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $stdout = Join-Path $Root "$label.json"

    $arguments = @(
        "download", $torrent, "--dir", $outDir,
        "--no-torrent-web-seed",
        "--no-dht", "--no-lsd", "--no-tracker", "--port", "0", "--json"
    )
    if ($withHttp) { $arguments += @("--web-seed", $webSeed) }
    if ($withPeer) { $arguments += @("--peer", $peer) }
    $arguments += $extra
    [void]$commands.Add("bit-cli $($arguments -join ' ')")

    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $Root -NoNewWindow -PassThru `
        -ArgumentList $arguments -RedirectStandardOutput $stdout `
        -RedirectStandardError (Join-Path $Root "$label.err")
    $finished = $process.WaitForExit(900000)
    $clock.Stop()
    if (-not $finished) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }

    $report = $null
    try { $report = Get-Content $stdout -Raw | ConvertFrom-Json } catch { }
    $ms = [Math]::Max(1, $clock.ElapsedMilliseconds)
    $total = if ($report) { [int64]$report.downloaded.bytes } else { 0 }
    $http = if ($report) { [int64]$report.from_web_seeds.bytes } else { 0 }
    $peers = if ($report) { [int64]$report.from_peers.bytes } else { 0 }
    [pscustomobject][ordered]@{
        phase           = $label
        exit_code       = if ($finished) { $process.ExitCode } else { 124 }
        elapsed_ms      = $clock.ElapsedMilliseconds
        bytes           = $total
        from_web_seeds  = $http
        from_peers      = $peers
        # From the wall clock and the bytes the report says landed, never from
        # the report's own mean, so the limiter is not measured by the thing it
        # is limiting.
        rate            = [int64]($total * 1000.0 / $ms)
        web_seed_rate   = [int64]($http * 1000.0 / $ms)
        peer_rate       = [int64]($peers * 1000.0 / $ms)
        rate_human      = Format-Rate ($total * 1000.0 / $ms)
    }
}

$ceiling = [Math]::Max(1, [int64]($rateBytes * (1 + $Tolerance)))
$webCeiling = [Math]::Max(1, [int64]($webSeedRateBytes * (1 + $Tolerance)))

# What a token bucket is allowed to have passed, as a rate, over a run of a
# given length.
#
# A bucket is a rate **and a burst**, not a rate alone. `governor`'s
# `Quota::per_second(n)` refills n per second and holds n, so a run of t
# seconds may legitimately pass `n * t + n` bytes. Amortised that is
# `n + n / t`, which is 16% over on a four second run and 2% over on a sixty
# second one. Judging the plain rate would fail a limiter that is working
# because the run it bounded was short, and the entry's acceptance says "over
# sixty seconds" for this reason. This says the same thing without needing the
# run to last that long.
function Get-Allowance([double]$rate, [int64]$elapsedMs) {
    $seconds = [Math]::Max(0.001, $elapsedMs / 1000.0)
    [int64](($rate * (1 + $Tolerance)) + ($rate / $seconds))
}
$phases = @()
$failures = [System.Collections.ArrayList]::new()

foreach ($spec in @(
        @{ label = "http_ceiling"; peer = $false; http = $true; extra = @() },
        @{ label = "http_session_cap"; peer = $false; http = $true; extra = @("--max-overall-download-rate", $Rate) },
        @{ label = "http_webseed_cap"; peer = $false; http = $true; extra = @("--web-seed-speed-limit", $Rate) },
        @{ label = "http_peer_cap"; peer = $false; http = $true; extra = @("--max-peer-rate", $Rate) },
        @{ label = "peer_ceiling"; peer = $true; http = $false; extra = @() },
        @{ label = "peer_peer_cap"; peer = $true; http = $false; extra = @("--max-peer-rate", $Rate) },
        @{ label = "hybrid_ceiling"; peer = $true; http = $true; extra = @() },
        @{ label = "hybrid_webseed_cap"; peer = $true; http = $true; extra = @("--web-seed-speed-limit", $Rate) },
        @{ label = "hybrid_session_cap"; peer = $true; http = $true; extra = @("--max-overall-download-rate", $Rate) },
        @{ label = "hybrid_both_caps"; peer = $true; http = $true; extra = @("--max-peer-rate", $Rate, "--web-seed-speed-limit", $WebSeedRate) }
    )) {
    Write-Step "phase $($spec.label)"
    $run = Invoke-Phase $spec.label $spec.peer $spec.http $spec.extra
    Write-Step ("  {0} total, {1} http, {2} peers" -f `
            $run.rate_human, (Format-Rate $run.web_seed_rate), (Format-Rate $run.peer_rate))
    $phases += $run
    if ($run.exit_code -ne 0) {
        [void]$failures.Add("$($spec.label) exited $($run.exit_code)")
    }
    if ($run.bytes -ne ($PayloadMiB * 1MB)) {
        [void]$failures.Add("$($spec.label) fetched $($run.bytes) bytes, expected $($PayloadMiB * 1MB)")
    }
}

Stop-Background

$by = @{}
foreach ($phase in $phases) { $by[$phase.phase] = $phase }

# Judged: a cap is an invariant.
if ($by["http_session_cap"].rate -gt (Get-Allowance $rateBytes $by["http_session_cap"].elapsed_ms)) {
    [void]$failures.Add("the session cap let HTTP run at $(Format-Rate $by['http_session_cap'].rate) against $Rate")
}
if ($by["http_webseed_cap"].rate -gt (Get-Allowance $rateBytes $by["http_webseed_cap"].elapsed_ms)) {
    [void]$failures.Add("the web seed cap let HTTP run at $(Format-Rate $by['http_webseed_cap'].rate) against $Rate")
}
if ($by["hybrid_session_cap"].rate -gt (Get-Allowance $rateBytes $by["hybrid_session_cap"].elapsed_ms)) {
    [void]$failures.Add("the session cap let the run reach $(Format-Rate $by['hybrid_session_cap'].rate) against $Rate")
}
if ($by["hybrid_webseed_cap"].web_seed_rate -gt (Get-Allowance $rateBytes $by["hybrid_webseed_cap"].elapsed_ms)) {
    [void]$failures.Add("the web seed cap let HTTP reach $(Format-Rate $by['hybrid_webseed_cap'].web_seed_rate) against $Rate in the hybrid run")
}
# Judged the other way: with only the web seed cap set, the run has to exceed
# it, because nothing was asked to bound the peer in that phase. If this ever
# fails, either the peer got nothing or a cap is reaching further than it was
# told to.
if ($by["hybrid_webseed_cap"].rate -le $ceiling) {
    [void]$failures.Add("the hybrid run stayed under the web seed cap with nothing capping the peer, so this measured nothing")
}

# --------------------------------------------------------------------------
# The peer cap, arranged so each assertion is an invariant
# --------------------------------------------------------------------------

# It binds peers.
if ($by["peer_peer_cap"].rate -gt (Get-Allowance $rateBytes $by["peer_peer_cap"].elapsed_ms)) {
    [void]$failures.Add("the peer cap let the swarm run at $(Format-Rate $by['peer_peer_cap'].rate) against $Rate")
}
# And the seeder could have exceeded it, so the line above measured a cap
# rather than a slow peer.
if ($by["peer_ceiling"].rate -le $ceiling) {
    [void]$failures.Add("the uncapped peer run only reached $(Format-Rate $by['peer_ceiling'].rate), at or under $Rate, so capping it measured nothing")
}
# It does not bind an attached HTTP source. This is the row the whole entry
# turns on: the bridge dials in as an ordinary peer, and a swarm cap that
# reached it would be the defect T-132 describes.
if ($by["http_peer_cap"].rate -le $ceiling) {
    [void]$failures.Add("the peer cap held HTTP to $(Format-Rate $by['http_peer_cap'].rate), at or under $Rate, so it is still capping the bridge")
}
# Both caps in one run, each side under its own.
if ($by["hybrid_both_caps"].peer_rate -gt (Get-Allowance $rateBytes $by["hybrid_both_caps"].elapsed_ms)) {
    [void]$failures.Add("with both caps set the peers ran at $(Format-Rate $by['hybrid_both_caps'].peer_rate) against $Rate")
}
if ($by["hybrid_both_caps"].web_seed_rate -gt (Get-Allowance $webSeedRateBytes $by["hybrid_both_caps"].elapsed_ms)) {
    [void]$failures.Add("with both caps set HTTP ran at $(Format-Rate $by['hybrid_both_caps'].web_seed_rate) against $WebSeedRate")
}

# Not judged: the split in the hybrid phases, and whether either source
# reached its own cap. Two sources racing is a scheduling outcome.

$verdict = if ($failures.Count -eq 0) { "pass" } else { "fail" }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "rate-scope-$stamp.json"
[pscustomobject][ordered]@{
    kind           = "rate_scope"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    }
    parameters     = [ordered]@{
        rate                 = $Rate
        rate_bytes           = $rateBytes
        web_seed_rate        = $WebSeedRate
        web_seed_rate_bytes  = $webSeedRateBytes
        payload_mib          = $PayloadMiB
        tolerance            = $Tolerance
        profile              = $Profile
    }
    phases         = @($phases)
    commands       = @($commands)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "from_peers and from_web_seeds are top-level fields of the download report, so the split is read rather than inferred.",
        "The caps are judged and the splits are recorded. Two sources racing each other is a scheduling outcome, and a fixture that asserts one is asserting something it does not control.",
        "hybrid_webseed_cap is judged in both directions: HTTP must stay under the cap, and the run as a whole must not, because nothing was asked to bound the peer there.",
        "http_peer_cap is the row T-132 turns on. The web seed bridge dials into the session as an ordinary peer, so a swarm cap that reached it would be the defect. It must exceed the cap.",
        "peer_ceiling exists so peer_peer_cap means something: a cap that holds because the peer was slow has measured nothing.",
        "The entry's acceptance asks for each source to be within 10% of its cap. The upper half is a cap and is judged; the lower half is a scheduling outcome and is arranged rather than asserted, per TODO/RULES.md section 5.",
        "Rates come from the wall clock and the bytes the report says landed, never from the report's own mean, so a limiter is not measured by the thing it limits.",
        "A cap is judged as rate plus burst over the run's own length, because a token bucket is a rate and a burst. governor holds one second of quota, which is 16% over on a four second run and 2% over on a sixty second one."
    )
} | ConvertTo-Json -Depth 10 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
$phases | Select-Object phase, rate_human,
@{ n = "http"; e = { Format-Rate $_.web_seed_rate } },
@{ n = "peers"; e = { Format-Rate $_.peer_rate } } |
    Format-Table -AutoSize | Out-String -Width 200 | Write-Host
Write-Host "report:  $reportPath"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-rate-scope: $failure") }
    exit 1
}
exit 0
