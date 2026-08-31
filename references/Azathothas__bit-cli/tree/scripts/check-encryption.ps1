# Does `--encryption` reach the peers it says it reaches, and refuse the ones
# it says it refuses?
#
# `TODO/peers.md` T-163: a peer configured to require encryption will not
# exchange a byte with a plaintext-only client, so the swarm a plaintext client
# can reach is smaller than the swarm that exists. MSE is what closes that, and
# this is what says it did.
#
# Three seeders, one payload, one port each, and they differ only in
# `--encryption`:
#
#   prefer    the default: accepts either, dials with MSE
#   require   MSE or nothing, in both directions
#   off       neither offers nor accepts it
#
# Seven phases, and every one of them is an invariant rather than a race.
#
#   prefer_seeder_default    no mode flag at all. Completes, and settles on rc4.
#   prefer_seeder_off        completes, and settles on plaintext.
#   prefer_seeder_require    completes, and settles on rc4.
#   require_seeder_default   the entry's first half: a peer that requires
#                            encryption, reached with no mode flag. Completes.
#   require_seeder_off       the control. **Must fetch nothing.** Without it
#                            every pass above could be a `require` that quietly
#                            accepts plaintext.
#   off_seeder_default       the entry's second half: the same leecher, no mode
#                            flag, against a peer that has encryption off.
#                            Completes, on plaintext, which is the fallback
#                            redial working.
#   off_seeder_require       the control in the other direction. Fetches
#                            nothing.
#
# **One listening port and no mode flag.** The first three phases are the same
# seeder process on the same port, so an accepting end telling MSE from
# plaintext by looking at the first twenty bytes is what is being measured, and
# it is measured three times without restarting it. Nothing on the wire says
# which mode either end is in.
#
# **Why the negative phases assert bytes and not a duration.** A run that must
# not complete has no condition to wait on, so `--stop-after` ends it. The
# assertion is that it fetched zero bytes, which is true however long it ran,
# and never that it failed inside some number of seconds. See
# `TODO/RULES.md` section 5 under "Testing".
#
# The mode each end settled on is read from `bit-cli peers --json`, which
# carries `peers[].encryption` per peer. It is a separate short run per phase
# rather than a field of the download report, because the download report has
# no peer rows.
#
# Usage:
#   pwsh scripts/check-encryption.ps1
#   pwsh scripts/check-encryption.ps1 -PayloadMiB 32 -Keep
#
# Exits 0 when every phase holds, 1 when one does not, and 2 when the check
# could not run. The record goes to bench/encryption-<timestamp>.json.
#
# See TODO/peers.md, T-163.

[CmdletBinding()]
param(
    [int]$PayloadMiB = 8,
    # How long a run that must not complete is given before it is ended. It is
    # a deadline, not a threshold: nothing asserts the failure happened inside
    # it, only that no byte arrived.
    [string]$NegativeDeadline = "25s",
    # How long the peer sampling run watches for. Long enough for one dial and
    # one handshake over loopback.
    [string]$SampleDuration = "6s",
    [string]$Root = ".tmp/encryption",
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
    [Console]::Error.WriteLine("check-encryption: $message")
    Stop-Background
    exit $code
}

trap { Stop-Background; throw }

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --bins"
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# ---------------------------------------------------------------------------
# A payload and a torrent
# ---------------------------------------------------------------------------
#
# Pseudo-random rather than zeroes, so nothing between here and the socket can
# elide it and turn a throughput measurement into a measurement of the
# shortcut. Here it also matters for a second reason: RC4 over a run of zeroes
# is the keystream itself, and a fixture whose ciphertext is the keystream is
# not a fixture for a cipher.

$serve = Join-Path $Root "payload"
New-Item -ItemType Directory -Force -Path $serve | Out-Null
Write-Step "building a $PayloadMiB MiB payload"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 20260823
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}
$stream = [System.IO.File]::Create((Join-Path $serve "payload.bin"))
try {
    for ($written = 0; $written -lt $PayloadMiB; $written++) {
        $stream.Write($block, 0, $block.Length)
    }
}
finally { $stream.Dispose() }

$torrent = Join-Path $Root "payload.torrent"
Push-Location $Root
try {
    & $bitCli create payload --name payload --piece-length 1MiB --no-creation-date `
        --output $torrent --force --json 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }
}
finally { Pop-Location }
$payloadBytes = [int64]$PayloadMiB * 1MB

# ---------------------------------------------------------------------------
# Three seeders, one per mode
# ---------------------------------------------------------------------------
#
# The port comes out of each seeder's own event stream and is never chosen
# here: a port this script picked could already be in use, and dialling it
# would measure whatever else was listening.

$commands = [System.Collections.ArrayList]::new()

function Start-Seeder([string]$mode) {
    $out = Join-Path $Root "seed-$mode.out"
    $arguments = @(
        "--jsonl", "seed", $torrent, "--data", $Root, "--port", "0",
        "--encryption", $mode,
        "--no-tracker", "--no-dht", "--no-lsd", "--stop-after", "1800s"
    )
    [void]$commands.Add("bit-cli $($arguments -join ' ')")
    $process = Start-Process -FilePath $bitCli -ArgumentList $arguments `
        -PassThru -NoNewWindow -RedirectStandardOutput $out `
        -RedirectStandardError (Join-Path $Root "seed-$mode.err")
    $script:Background += $process
    for ($attempt = 0; $attempt -lt 600; $attempt++) {
        Start-Sleep -Milliseconds 100
        if ($process.HasExited) {
            Exit-With 2 "the $mode seeder exited $($process.ExitCode): $(Get-Content (Join-Path $Root "seed-$mode.err") -Raw)"
        }
        foreach ($line in (Get-Content $out -ErrorAction SilentlyContinue)) {
            try { $event = $line | ConvertFrom-Json } catch { continue }
            if ($event.listen_addr) {
                return "127.0.0.1:$(($event.listen_addr -split ':')[-1])"
            }
        }
    }
    Exit-With 2 "the $mode seeder never printed a listen address"
}

$seeders = [ordered]@{}
foreach ($mode in @("prefer", "require", "off")) {
    Write-Step "starting the $mode seeder"
    $seeders[$mode] = Start-Seeder $mode
    Write-Step "  $mode seeder at $($seeders[$mode])"
}

# ---------------------------------------------------------------------------
# One phase: a download, then a peer sample
# ---------------------------------------------------------------------------

function Invoke-Phase([string]$label, [string]$seederMode, [string]$leecherMode, [bool]$expectComplete) {
    $peer = $seeders[$seederMode]
    $outDir = Join-Path $Root "out-$label"
    if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null

    $arguments = @(
        "download", $torrent, "--dir", $outDir, "--peer", $peer,
        "--no-torrent-web-seed", "--no-dht", "--no-lsd", "--no-tracker",
        "--port", "0", "--json"
    )
    if ($leecherMode) { $arguments += @("--encryption", $leecherMode) }
    if (-not $expectComplete) { $arguments += @("--stop-after", $NegativeDeadline) }
    [void]$commands.Add("bit-cli $($arguments -join ' ')")

    $stdout = Join-Path $Root "$label.json"
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $Root -NoNewWindow -PassThru `
        -ArgumentList $arguments -RedirectStandardOutput $stdout `
        -RedirectStandardError (Join-Path $Root "$label.err")
    $finished = $process.WaitForExit(600000)
    $clock.Stop()
    if (-not $finished) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }

    $report = $null
    try { $report = Get-Content $stdout -Raw | ConvertFrom-Json } catch { }
    $bytes = if ($report -and $report.downloaded) { [int64]$report.downloaded.bytes } else { 0 }

    # What each end settled on, from the run that reports peer rows. A separate
    # short run: the download report carries no peers.
    $sampleArgs = @(
        "peers", $torrent, "--peer", $peer, "--duration", $SampleDuration,
        "--no-dht", "--no-lsd", "--no-tracker", "--port", "0", "--json"
    )
    if ($leecherMode) { $sampleArgs += @("--encryption", $leecherMode) }
    [void]$commands.Add("bit-cli $($sampleArgs -join ' ')")
    $sampleOut = Join-Path $Root "$label-peers.json"
    $sample = Start-Process -FilePath $bitCli -WorkingDirectory $Root -NoNewWindow -PassThru `
        -ArgumentList $sampleArgs -RedirectStandardOutput $sampleOut `
        -RedirectStandardError (Join-Path $Root "$label-peers.err")
    $sample.WaitForExit(120000) | Out-Null

    $negotiated = $null
    $peerState = $null
    try {
        $peersReport = Get-Content $sampleOut -Raw | ConvertFrom-Json
        $row = @($peersReport.peers) | Where-Object { $_.addr -eq $peer } | Select-Object -First 1
        if ($row) {
            $negotiated = $row.encryption
            $peerState = $row.state
        }
    }
    catch { }

    [pscustomobject][ordered]@{
        phase            = $label
        seeder           = $seederMode
        leecher          = if ($leecherMode) { $leecherMode } else { "default" }
        expect_complete  = $expectComplete
        exit_code        = if ($finished) { $process.ExitCode } else { 124 }
        elapsed_ms       = $clock.ElapsedMilliseconds
        bytes            = $bytes
        negotiated       = $negotiated
        peer_state       = $peerState
    }
}

$phases = @()
$failures = [System.Collections.ArrayList]::new()

$specs = @(
    @{ label = "prefer_seeder_default"; seeder = "prefer"; leecher = ""; complete = $true; mode = "rc4" },
    @{ label = "prefer_seeder_off"; seeder = "prefer"; leecher = "off"; complete = $true; mode = "plaintext" },
    @{ label = "prefer_seeder_require"; seeder = "prefer"; leecher = "require"; complete = $true; mode = "rc4" },
    @{ label = "require_seeder_default"; seeder = "require"; leecher = ""; complete = $true; mode = "rc4" },
    @{ label = "require_seeder_off"; seeder = "require"; leecher = "off"; complete = $false; mode = $null },
    @{ label = "off_seeder_default"; seeder = "off"; leecher = ""; complete = $true; mode = "plaintext" },
    @{ label = "off_seeder_require"; seeder = "off"; leecher = "require"; complete = $false; mode = $null }
)

foreach ($spec in $specs) {
    Write-Step "phase $($spec.label)"
    $run = Invoke-Phase $spec.label $spec.seeder $spec.leecher $spec.complete
    Write-Step ("  exit {0}, {1} bytes, settled on {2}" -f $run.exit_code, $run.bytes, ($run.negotiated ?? "nothing"))
    $phases += $run

    if ($spec.complete) {
        if ($run.exit_code -ne 0) {
            [void]$failures.Add("$($spec.label) exited $($run.exit_code)")
        }
        if ($run.bytes -ne $payloadBytes) {
            [void]$failures.Add("$($spec.label) fetched $($run.bytes) bytes, expected $payloadBytes")
        }
        if ($run.negotiated -ne $spec.mode) {
            [void]$failures.Add("$($spec.label) settled on '$($run.negotiated)', expected '$($spec.mode)'")
        }
    }
    else {
        # The invariant is that nothing crossed. Not that it failed quickly.
        if ($run.bytes -ne 0) {
            [void]$failures.Add("$($spec.label) fetched $($run.bytes) bytes and should have fetched none")
        }
        if ($run.exit_code -eq 0) {
            [void]$failures.Add("$($spec.label) exited 0 and should not have completed")
        }
    }
}

Stop-Background

$verdict = if ($failures.Count -eq 0) { "pass" } else { "fail" }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "encryption-$stamp.json"
[pscustomobject][ordered]@{
    kind           = "encryption"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    }
    parameters     = [ordered]@{
        payload_mib       = $PayloadMiB
        payload_bytes     = $payloadBytes
        negative_deadline = $NegativeDeadline
        sample_duration   = $SampleDuration
        profile           = $Profile
    }
    seeders        = $seeders
    phases         = @($phases)
    commands       = @($commands)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "The first three phases are the same seeder process on the same port, so one listening port serving both kinds of peer is measured three times without a restart.",
        "require_seeder_off and off_seeder_require are the controls. Without them a `require` that quietly accepted plaintext would pass every other phase.",
        "A phase that must not complete asserts zero bytes, which is true however long it ran. The deadline only ends the run.",
        "The negotiated mode is read from bit-cli peers --json, which carries peers[].encryption. The download report has no peer rows.",
        "off_seeder_default is the fallback redial: the leecher offers MSE, the seeder does not answer it, and the leecher dials again in plaintext inside the same attempt."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8

Write-Step "wrote $reportPath"
foreach ($phase in $phases) {
    "{0,-24} {1,-8} -> {2,-8} exit {3,-4} {4,10} bytes  {5}" -f `
        $phase.phase, $phase.seeder, $phase.leecher, $phase.exit_code, $phase.bytes, ($phase.negotiated ?? "-")
}

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-encryption: $failure") }
    exit 1
}
Write-Step "check-encryption: pass"
exit 0
