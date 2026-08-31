# Does a transfer complete over each transport, and what does it cost?
#
# `--transport tcp|utp|both` is T-101, and the entry's rule is the repository's:
# a flag that does not move a number does not ship. So this is the number.
#
# One seeder and one leecher per case, both told the same transport, over
# loopback with no tracker, no DHT and no LSD, so the only way a byte moves is
# the peer connection this is measuring. The leecher is given the seeder's
# address directly with `--peer`.
#
# `utp` is the case that says whether BEP 29 works here at all. It is not a
# fallback: a `UtpOnly` leecher cannot reach a `TcpOnly` seeder, so a run that
# completes proves the uTP path carried the peer wire protocol end to end
# rather than quietly using TCP. `mixed` is the negative control and asserts
# exactly that: `utp` against `tcp` must NOT complete.
#
# What this cannot measure is the reason to want uTP. LEDBAT targets a fixed
# one-way queueing delay and yields to other traffic on the same link, and
# loopback has no bottleneck to queue at. Throughput here is a statement about
# this machine's loopback, not about either congestion controller. See the
# entry.
#
# Usage:
#   pwsh scripts/check-transport.ps1
#   pwsh scripts/check-transport.ps1 -PayloadMiB 64 -Json bench/transport.json
#
# Exits 0 when every case holds, 1 when one does not, and 2 when the check
# could not run.
#
# See TODO/bep-coverage.md, T-101.

[CmdletBinding()]
param(
    [int]$PayloadMiB = 32,
    [int]$TimeoutSeconds = 120,
    [string]$Root = ".tmp/transport",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [string]$Json
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-transport: $message")
    exit $code
}

function Write-Step($message) {
    Write-Host "$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.fffZ')) $message"
}

$bitCli = Join-Path $repo "target/$Profile/bit-cli.exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --bins"
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path (Join-Path $Root "payload") | Out-Null
$Root = (Resolve-Path $Root).Path

# ---------------------------------------------------------------------------
# A payload worth moving
# ---------------------------------------------------------------------------

Write-Step "building a $PayloadMiB MiB payload"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 20260824
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}
$payload = Join-Path $Root "payload/transport.bin"
$stream = [System.IO.File]::Create($payload)
try { for ($i = 0; $i -lt $PayloadMiB; $i++) { $stream.Write($block, 0, $block.Length) } }
finally { $stream.Dispose() }
$payloadHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $payload).Hash

$torrent = Join-Path $Root "transport.torrent"
& $bitCli create (Join-Path $Root "payload") --name payload --piece-length 1MiB `
    --no-creation-date --output $torrent --force --json 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }

$background = @()
function Stop-Background {
    foreach ($process in $script:background) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $script:background = @()
}
trap { Stop-Background; throw }

# One case: a seeder and a leecher, both on $transport, and what it cost.
#
# `-LeechTransport` differs from `-SeedTransport` only in the negative control,
# where the point is that the two cannot reach each other.
function Invoke-Case($tag, $seedTransport, $leechTransport, $encryption, $expectFinished) {
    $caseRoot = Join-Path $Root $tag
    New-Item -ItemType Directory -Force -Path $caseRoot | Out-Null

    $seed = Start-Process -FilePath $bitCli -WorkingDirectory $Root -NoNewWindow -PassThru -ArgumentList @(
        "seed", $torrent, "--data", $Root, "--port", "0",
        "--no-dht", "--no-lsd", "--no-tracker",
        "--transport", $seedTransport, "--encryption", $encryption,
        "--report-interval", "5s", "--seed-time", "$($TimeoutSeconds + 60)s", "--jsonl"
    ) -RedirectStandardOutput (Join-Path $caseRoot "seed.out") `
        -RedirectStandardError (Join-Path $caseRoot "seed.err")
    $script:background += $seed

    # The address comes from the seeder's own report rather than from a socket
    # table: a uTP listener is a UDP socket, so Get-NetTCPConnection cannot see
    # it and a check that looked there would find nothing and call it a
    # failure to listen.
    $listen = $null
    $deadline = (Get-Date).AddSeconds(60)
    while (-not $listen -and (Get-Date) -lt $deadline) {
        if ($seed.HasExited) { break }
        foreach ($line in @(Get-Content (Join-Path $caseRoot "seed.out") -ErrorAction SilentlyContinue)) {
            if (-not $line -or -not $line.Trim().StartsWith("{")) { continue }
            $event = $null
            try { $event = $line | ConvertFrom-Json } catch { continue }
            if ($event.listen_addr) { $listen = $event.listen_addr; break }
        }
        if (-not $listen) { Start-Sleep -Milliseconds 200 }
    }
    if (-not $listen) {
        Stop-Background
        Exit-With 2 "the $seedTransport seeder never reported a listen address; see $caseRoot/seed.err"
    }
    $port = $listen.Split(":")[-1]
    Write-Step "  $tag seeder listening on $listen, pid $($seed.Id)"

    $out = Join-Path $caseRoot "leech"
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $leechOut = Join-Path $caseRoot "leech.json"
    $leech = Start-Process -FilePath $bitCli -WorkingDirectory $Root -NoNewWindow -PassThru -Wait -ArgumentList @(
        "download", $torrent, "--dir", $out,
        "--peer", "127.0.0.1:$port",
        "--transport", $leechTransport, "--encryption", $encryption,
        "--no-dht", "--no-lsd", "--no-tracker", "--allow-overwrite",
        "--stop-after", "$($TimeoutSeconds)s", "--json"
    ) -RedirectStandardOutput $leechOut -RedirectStandardError (Join-Path $caseRoot "leech.err")
    $clock.Stop()

    $report = $null
    try { $report = Get-Content $leechOut -Raw | ConvertFrom-Json } catch { $report = $null }
    $finished = [bool]($report -and $report.torrents -and $report.torrents[0].finished)

    # Verify from the bytes rather than from the report, because the report is
    # the thing under test.
    $landed = Join-Path $out "payload/transport.bin"
    $hash = if (Test-Path $landed) { (Get-FileHash -Algorithm SHA256 -LiteralPath $landed).Hash } else { $null }
    $verified = ($hash -eq $payloadHash)

    if (-not $seed.HasExited) { Stop-Process -Id $seed.Id -Force -ErrorAction SilentlyContinue }

    $seconds = [math]::Round($clock.Elapsed.TotalSeconds, 2)
    $rate = if ($finished -and $seconds -gt 0) { [math]::Round($PayloadMiB / $seconds, 2) } else { $null }
    Write-Step ("  {0}: finished {1}, verified {2}, {3}s{4}" -f $tag, $finished, $verified, $seconds,
        $(if ($rate) { ", $rate MiB/s" } else { "" }))

    [ordered]@{
        case            = $tag
        seed_transport  = $seedTransport
        leech_transport = $leechTransport
        encryption      = $encryption
        expect_finished = $expectFinished
        finished        = $finished
        verified        = $verified
        seconds         = $seconds
        mib_per_second  = $rate
        exit_code       = $leech.ExitCode
        judged          = $true
        ok              = ($finished -eq $expectFinished) -and ($verified -eq $expectFinished)
    }
}

$cases = [System.Collections.ArrayList]::new()
[void]$cases.Add((Invoke-Case "tcp" "tcp" "tcp" "prefer" $true))
[void]$cases.Add((Invoke-Case "utp" "utp" "utp" "off" $true))
[void]$cases.Add((Invoke-Case "both" "both" "both" "prefer" $true))
# The negative control, and the reason the three above mean anything. A
# `UtpOnly` leecher cannot reach a `TcpOnly` seeder, so if this one completed
# it would mean the flag is not reaching the dialer and every case above was
# TCP.
[void]$cases.Add((Invoke-Case "mixed" "tcp" "utp" "off" $false))
# The pair that does not work, and the two that say it is neither the transport
# nor the encryption on its own. See TODO/peers.md, T-233.
[void]$cases.Add((Invoke-Case "utp-mse" "utp" "utp" "require" $false))
[void]$cases.Add((Invoke-Case "tcp-mse" "tcp" "tcp" "require" $true))

Stop-Background

$failures = @($cases | Where-Object { -not $_.ok } | ForEach-Object {
        "$($_.case): finished $($_.finished), verified $($_.verified), expected finished $($_.expect_finished)"
    })

$report = [ordered]@{
    kind         = "transport"
    generated_at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    payload_mib  = $PayloadMiB
    profile      = $Profile
    cases        = @($cases)
    failures     = @($failures)
    notes        = @(
        "Loopback has no bottleneck link, so nothing here measures what uTP is for: LEDBAT targets a fixed one-way queueing delay and yields to competing traffic, and there is no queue to build on loopback. The rates are a statement about this machine.",
        "The mixed case is the control. A UtpOnly leecher cannot reach a TcpOnly seeder, so its completing would mean the flag never reached the dialer and the other cases were TCP.",
        "utp-mse is expected NOT to finish and that is a defect rather than a design: MSE over uTP stalls after the handshake, and utp with encryption off and tcp-mse are the two cases that say it is neither the transport nor the encryption on its own. See TODO/peers.md, T-233."
    )
}
if ($Json) {
    $jsonPath = if ([System.IO.Path]::IsPathRooted($Json)) { $Json } else { Join-Path $repo $Json }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $jsonPath) | Out-Null
    $report | ConvertTo-Json -Depth 6 | Set-Content -Path $jsonPath -Encoding utf8NoBOM
    Write-Host "check-transport: wrote $Json"
}

@($cases) | ForEach-Object { [pscustomobject]$_ } |
    Format-Table case, seed_transport, leech_transport, encryption, finished, verified, seconds, mib_per_second, exit_code -AutoSize |
    Out-String | Write-Host

Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-transport: $failure") }
    exit 1
}
Write-Host "check-transport: every case holds"
exit 0
