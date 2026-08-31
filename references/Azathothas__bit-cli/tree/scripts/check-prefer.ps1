# Measure what --prefer-web-seed moves.
#
# The flag is documented as biasing the download toward HTTP when both a peer
# and a source hold a piece. `bit-cli` cannot reach `librqbit`'s piece picker,
# so it cannot state the preference directly; what it can do is give the HTTP
# source more of whatever decides which answer arrives first. A flag that
# changes a number without changing the outcome is worse than no flag, so this
# script measures the outcome.
#
# The setup is a hybrid swarm built entirely on loopback:
#
#   the mirror   the loopback file server, holding the whole payload, reached
#                over HTTP as a web seed.
#   the peer     a second bit-cli seeding the same payload over loopback,
#                uncapped, so it is a genuine competitor for every piece.
#   the leecher  bit-cli download, given both, run once without the flag and
#                once with it.
#
# What is compared is the byte split the run reports: `from_web_seeds` against
# `from_peers`. Both runs fetch the same payload from the same two sources on
# the same machine back to back, so the split is the only thing that moved.
#
# Usage:
#   pwsh scripts/check-prefer.ps1
#   pwsh scripts/check-prefer.ps1 -PayloadSize 1GiB -Runs 5
#
# Exits 0 when the flag shifted the split toward HTTP in every run, 1 when it
# did not, and 2 when the check could not run. The record goes to
# bench/prefer-<timestamp>.json.
#
# See TODO/webseed.md, T-003.

[CmdletBinding()]
param(
    # Payload to build. Big enough that the split is a measurement rather than
    # a rounding, small enough that a run is seconds.
    [string]$PayloadSize = "256MiB",
    # Rate caps, empty for none.
    #
    # Both are empty by default and that is the point. A cap on either side
    # decides the split by itself: two sources capped at 24 and 8 MiB/s split
    # 75 to 25 whatever the client does, and a flag measured against that
    # measures the caps. Uncapped, the split is whichever side answers a block
    # sooner, which is what the preference is supposed to move.
    [string]$PeerRate = "",
    [string]$SeedRate = "",
    # Paired runs. Each pair is one run without the flag and one with it.
    [int]$Runs = 3,
    [string]$Root = ".tmp/prefer",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [int]$TimeoutSeconds = 300,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-prefer: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

function Format-Size([double]$bytes) {
    $units = @("B", "KiB", "MiB", "GiB", "TiB")
    $index = 0
    while ($bytes -ge 1024 -and $index -lt $units.Count - 1) { $bytes /= 1024; $index++ }
    "{0:N2} {1}" -f $bytes, $units[$index]
}

function Format-Percent([double]$fraction) {
    "{0:N2}%" -f ($fraction * 100)
}

function ConvertFrom-Size([string]$text) {
    if ($text -match '^\s*(\d+(?:\.\d+)?)\s*([KMGT]?i?B?)\s*$') {
        $value = [double]$Matches[1]
        switch ($Matches[2].ToUpperInvariant()) {
            "KIB" { return [int64]($value * 1024) }
            "MIB" { return [int64]($value * 1024 * 1024) }
            "GIB" { return [int64]($value * 1024 * 1024 * 1024) }
            default { return [int64]$value }
        }
    }
    Exit-With 2 "cannot read the size '$text'"
}

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
$fileserver = Join-Path $repo "target/$Profile/examples/loopback-fileserver$exe"
foreach ($required in @($bitCli, $fileserver)) {
    if (-not (Test-Path $required)) {
        Exit-With 2 "missing $required. Build it first: cargo build --workspace --bins --examples --release"
    }
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

$script:Background = @()

function Start-Background($name, $path, $arguments, $workdir) {
    $stdout = Join-Path $Root "$name.out"
    $stderr = Join-Path $Root "$name.err"
    $process = Start-Process -FilePath $path -ArgumentList $arguments `
        -WorkingDirectory $workdir -NoNewWindow -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $script:Background += $process
    [pscustomobject]@{ Process = $process; Stdout = $stdout; Stderr = $stderr }
}

function Stop-Background {
    foreach ($process in $script:Background) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $script:Background = @()
}

function Wait-ForLine($file, $seconds, $what) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $file) {
            $line = (Get-Content $file -TotalCount 1 -ErrorAction SilentlyContinue)
            if ($line -and $line.Trim()) { return $line.Trim() }
        }
        Start-Sleep -Milliseconds 100
    }
    Stop-Background
    Exit-With 2 "no $what after ${seconds}s: $file"
}

trap { Stop-Background; throw }

# ---------------------------------------------------------------------------
# The payload, the torrent, and the two sources
# ---------------------------------------------------------------------------

$payloadBytes = ConvertFrom-Size $PayloadSize
Write-Step "building a $(Format-Size $payloadBytes) payload"
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
# From $Root, because `payload` is the directory name inside the torrent and
# `create` reads it relative to where it runs.
Push-Location $Root
try {
    & $bitCli create payload --name payload --piece-length 1MiB --no-creation-date `
        --output $torrent --force --json 2>$null | Out-Null
    if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }
} finally { Pop-Location }

Write-Step "starting the mirror"
$server = Start-Background "fileserver" $fileserver @("--root", $Root) $Root
$webSeed = Wait-ForLine $server.Stdout 15 "URL on the file server's stdout"
Write-Step "  mirror at $webSeed"

# The peer seeds the same payload. It announces nothing and joins nothing: the
# leecher is given its address directly with --peer, so the swarm has exactly
# two members and no discovery can add a third.
#
# --dir is the payload directory rather than $Root, because the torrent's name
# is `payload` and `seed` is being told exactly where the files are rather than
# where a directory named after the torrent would go.
Write-Step "starting the peer$(if ($PeerRate) { ", capped at $PeerRate" })"
$peerPort = 0
$seedArgs = @(
    "seed", $torrent, "--dir", $payloadDir,
    "--no-tracker", "--no-dht", "--no-lsd", "--no-pex",
    "--port", "0", "--seed-time", "600s", "--jsonl"
)
if ($PeerRate) { $seedArgs += @("--max-upload-rate", $PeerRate) }
$seeder = Start-Background "seeder" $bitCli $seedArgs $Root
Start-Sleep -Milliseconds 500
$deadline = (Get-Date).AddSeconds(20)
while ((Get-Date) -lt $deadline -and $peerPort -eq 0) {
    foreach ($line in (Get-Content $seeder.Stdout -ErrorAction SilentlyContinue)) {
        if (-not $line.Trim()) { continue }
        try { $event = $line | ConvertFrom-Json } catch { continue }
        if ($event.listen_addr) {
            $peerPort = [int]($event.listen_addr -split ':')[-1]
            break
        }
    }
    if ($peerPort -eq 0) { Start-Sleep -Milliseconds 200 }
}
if ($peerPort -eq 0) {
    Stop-Background
    Exit-With 2 "the seeder never reported a listen address. $(Get-Content $seeder.Stderr -Raw)"
}
$peerAddr = "127.0.0.1:$peerPort"
Write-Step "  peer at $peerAddr"

# ---------------------------------------------------------------------------
# The paired runs
# ---------------------------------------------------------------------------

function Invoke-Download($label, $prefer) {
    $out = Join-Path $Root "out-$label"
    if (Test-Path $out) { Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue }
    $arguments = @(
        "download", $torrent, "--dir", $out,
        "--web-seed", $webSeed, "--no-torrent-web-seed",
        "--peer", $peerAddr, "--no-tracker", "--no-dht", "--no-lsd",
        "--port", "0", "--allow-overwrite", "--json"
    )
    if ($SeedRate) { $arguments += @("--web-seed-speed-limit", $SeedRate) }
    if ($prefer) { $arguments += "--prefer-web-seed" }

    $stdout = Join-Path $Root "$label.out"
    $stderr = Join-Path $Root "$label.err"
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $bitCli -ArgumentList $arguments `
        -WorkingDirectory $Root -NoNewWindow -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        Stop-Background
        Exit-With 2 "the $label run did not finish in ${TimeoutSeconds}s"
    }
    $clock.Stop()
    if ($process.ExitCode -ne 0) {
        Stop-Background
        Exit-With 1 "the $label run exited $($process.ExitCode). $(Get-Content $stderr -Raw)"
    }
    $document = Get-Content $stdout -Raw | ConvertFrom-Json
    if (-not $Keep) { Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue }

    $torrentReport = $document.torrents[0]
    $fromWeb = [int64]$torrentReport.from_web_seeds.bytes
    $fromPeers = [int64]$torrentReport.from_peers.bytes
    $total = $fromWeb + $fromPeers
    [pscustomobject]@{
        label = $label
        prefer = $prefer
        command = "$bitCli $($arguments -join ' ')"
        elapsed_ms = [int64]$clock.Elapsed.TotalMilliseconds
        from_web_seeds = $fromWeb
        from_peers = $fromPeers
        web_seed_share = if ($total -gt 0) { $fromWeb / $total } else { 0 }
        connections = [int]$torrentReport.sources[0].connections
        http_bytes = [int64]$torrentReport.sources[0].http_bytes
        downloaded = [int64]$torrentReport.downloaded.bytes
    }
}

$pairs = @()
for ($run = 1; $run -le $Runs; $run++) {
    Write-Step "run $run of $Runs"
    $off = Invoke-Download "off-$run" $false
    Write-Step "  without the flag  HTTP $(Format-Size $off.from_web_seeds) ($(Format-Percent $off.web_seed_share))  peer $(Format-Size $off.from_peers)"
    $on = Invoke-Download "on-$run" $true
    Write-Step "  with the flag     HTTP $(Format-Size $on.from_web_seeds) ($(Format-Percent $on.web_seed_share))  peer $(Format-Size $on.from_peers)"
    $pairs += [pscustomobject]@{
        run = $run
        off = $off
        on = $on
        shift = $on.web_seed_share - $off.web_seed_share
    }
}

Stop-Background

# ---------------------------------------------------------------------------
# The verdict
# ---------------------------------------------------------------------------

$shifted = @($pairs | Where-Object { $_.shift -gt 0 }).Count
$meanOff = ($pairs | Measure-Object -Property { $_.off.web_seed_share } -Average).Average
$meanOn = ($pairs | Measure-Object -Property { $_.on.web_seed_share } -Average).Average

Write-Host ""
Write-Host "RUN   WITHOUT      WITH         SHIFT"
Write-Host "--------------------------------------"
foreach ($pair in $pairs) {
    Write-Host ("{0,-5} {1,-12} {2,-12} {3}" -f $pair.run,
        (Format-Percent $pair.off.web_seed_share),
        (Format-Percent $pair.on.web_seed_share),
        (Format-Percent $pair.shift))
}
Write-Host ("{0,-5} {1,-12} {2,-12} {3}" -f "mean",
    (Format-Percent $meanOff), (Format-Percent $meanOn), (Format-Percent ($meanOn - $meanOff)))
Write-Host ""

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "prefer-$stamp.json"
$report = [ordered]@{
    schema_version = "1"
    kind = "check-prefer"
    generated_at = Get-Timestamp
    setup = [ordered]@{
        payload_bytes = $payloadBytes
        piece_length_bytes = 1048576
        web_seed = $webSeed
        web_seed_rate = if ($SeedRate) { $SeedRate } else { "unlimited" }
        peer = $peerAddr
        peer_rate = if ($PeerRate) { $PeerRate } else { "unlimited" }
        profile = $Profile
    }
    runs = $pairs
    summary = [ordered]@{
        pairs = $pairs.Count
        pairs_shifted_toward_http = $shifted
        mean_share_without = Format-Percent $meanOff
        mean_share_with = Format-Percent $meanOn
        mean_shift = Format-Percent ($meanOn - $meanOff)
    }
    notes = @(
        "Only the mirror named here is used: --no-torrent-web-seed drops the torrent's own url-list, which is empty for this generated torrent and would not be for a real one.",
        "The peer announces nothing and the leecher is given its address with --peer, so the swarm has exactly two members.",
        "Neither side is rate limited by default. A cap decides the split by itself, so a flag measured against capped sources measures the caps rather than the flag."
    )
}
$report | ConvertTo-Json -Depth 10 | Set-Content -Path $reportPath -Encoding utf8NoBOM
Write-Step "report written to $reportPath"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($shifted -eq $pairs.Count) {
    Write-Step "--prefer-web-seed shifted the split toward HTTP in all $($pairs.Count) pairs"
    exit 0
}
Write-Step "--prefer-web-seed shifted the split in $shifted of $($pairs.Count) pairs"
exit 1
