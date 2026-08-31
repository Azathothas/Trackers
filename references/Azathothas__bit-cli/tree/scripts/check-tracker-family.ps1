# Which of this host's addresses an HTTP tracker is told about.
#
# This is the acceptance for the half of `TODO/peers.md` T-022 that was left
# open: `bit-cli trackers` announces once per address family already, and the
# **session** did not. A seeder on a dual-stack host registered one address
# with an HTTP tracker, so peers on the other family learned nothing reachable,
# connected, failed, and retried. UDP trackers were already told about both,
# at `vendor/rqbit/crates/tracker_comms/src/tracker_comms.rs`, which is what
# made the HTTP path the odd one out rather than a design.
#
# Two cases, and the second is what says the first can fail:
#
#   dual_host     The tracker is named `http://localhost:<port>/announce`, so
#                 the host resolves in both families. The seeder must announce
#                 twice, once from 127.0.0.1 and once from ::1.
#   literal_host  The same tracker named `http://127.0.0.1:<port>/announce`.
#                 There is nothing to resolve and nothing to choose, so exactly
#                 one announce, over IPv4. A check that cannot tell these two
#                 apart is measuring that the tracker is up.
#
# The subject is `bit-cli seed`, which runs a `librqbit` session, rather than
# `bit-cli trackers`, which uses this repository's own tracker client and was
# announcing per family before any of this.
#
# The tracker is `loopback-tracker`, bound on 127.0.0.1 and [::1] at one port,
# keying its peer records by (peer id, family) the way a BEP 7 tracker does.
# It logs every announce with the source address and the family, and that log
# is what is read here.
#
# Usage:
#   pwsh scripts/check-tracker-family.ps1
#   pwsh scripts/check-tracker-family.ps1 -TimeoutSeconds 60 -Keep
#
# Exits 0 when both cases hold, 1 when one does not, and 2 when the check could
# not run, which includes a host with no IPv6 loopback.
#
# See TODO/peers.md, T-022.

[CmdletBinding()]
param(
    [int]$TimeoutSeconds = 45,
    [string]$Root = ".tmp/trackerfamily",
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

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-tracker-family: $message")
    exit $code
}

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
$trackerExe = Join-Path $repo "target/$Profile/examples/loopback-tracker$exe"
foreach ($needed in @($bitCli, $trackerExe)) {
    if (-not (Test-Path $needed)) {
        Exit-With 2 "missing $needed. Build it first: cargo build --$Profile --workspace --bins --examples"
    }
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

$background = [System.Collections.ArrayList]::new()
function Start-Background([string]$file, [string[]]$fileArgs, [string]$name) {
    $process = Start-Process -FilePath $file -ArgumentList $fileArgs -PassThru -NoNewWindow `
        -RedirectStandardOutput (Join-Path $Root "$name.out") `
        -RedirectStandardError (Join-Path $Root "$name.err")
    [void]$background.Add($process)
    $process
}
function Stop-Background {
    foreach ($process in $background) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
}

# ---------------------------------------------------------------------------
# A payload and a torrent. Neither matters; only the announce is measured.
# ---------------------------------------------------------------------------

$payload = Join-Path $Root "payload.bin"
[System.IO.File]::WriteAllBytes($payload, [byte[]]::new(262144))
$torrent = Join-Path $Root "p.torrent"
$create = Start-Process -FilePath $bitCli -ArgumentList @(
    "create", $payload, "--piece-length", "16KiB", "--no-creation-date",
    "--output", $torrent, "--force", "--json"
) -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $Root "create.out") `
    -RedirectStandardError (Join-Path $Root "create.err")
$create.WaitForExit(60000) | Out-Null
if ($create.ExitCode -ne 0) { Exit-With 2 "bit-cli create exited $($create.ExitCode)" }

$infoHash = (Get-Content (Join-Path $Root "create.out") -Raw | ConvertFrom-Json).info_hash
if (-not $infoHash) { Exit-With 2 "bit-cli create did not report an info_hash" }
Write-Step "torrent $infoHash"

# ---------------------------------------------------------------------------
# The tracker, on both families at one port
# ---------------------------------------------------------------------------

$tracker = Start-Background $trackerExe @("--port", "0", "--interval", "5") "tracker"
$trackerOut = Join-Path $Root "tracker.out"
$trackerLog = Join-Path $Root "tracker.err"

$deadline = (Get-Date).AddSeconds(15)
$urls = @()
while ((Get-Date) -lt $deadline) {
    if (Test-Path $trackerOut) {
        $urls = @(Get-Content $trackerOut -ErrorAction SilentlyContinue | Where-Object { $_ -match '^http' })
        if ($urls.Count -ge 2) { break }
    }
    Start-Sleep -Milliseconds 100
}
if ($urls.Count -eq 0) { Stop-Background; Exit-With 2 "loopback-tracker printed no announce URL" }
if ($urls.Count -lt 2) {
    Stop-Background
    Exit-With 2 "loopback-tracker bound IPv4 only, so this host has no IPv6 loopback and there is nothing to measure"
}
if ($urls[0] -notmatch ':(\d+)/announce') { Stop-Background; Exit-With 2 "cannot read the port out of '$($urls[0])'" }
$port = [int]$Matches[1]
Write-Step "tracker on 127.0.0.1:$port and [::1]:$port"

# ---------------------------------------------------------------------------
# One seed run per case
# ---------------------------------------------------------------------------
#
# The wait is on the condition, never on a duration: the run stops as soon as
# the tracker has logged an announce for this info hash from every family the
# case expects, and the deadline is only there so a failure ends.

function Read-Families([string]$since) {
    $lines = @(Get-Content $trackerLog -ErrorAction SilentlyContinue |
            Where-Object { $_ -match "announce info_hash=$infoHash" -and $_ -gt $since })
    $seen = [ordered]@{}
    foreach ($line in $lines) {
        if ($line -match 'from=(\S+) family=(ipv[46])') {
            $seen["$($Matches[2])"] = $Matches[1]
        }
    }
    $seen
}

$cases = @(
    [ordered]@{ name = "dual_host"; host = "localhost"; expect = @("ipv4", "ipv6") }
    [ordered]@{ name = "literal_host"; host = "127.0.0.1"; expect = @("ipv4") }
)

$rows = @()
$failures = @()
foreach ($case in $cases) {
    $mark = Get-Timestamp
    $url = "http://$($case.host):$port/announce"
    Write-Step "$($case.name): seeding, announcing to $url"
    $seed = Start-Background $bitCli @(
        "seed", $torrent, "--data", $Root, "--tracker", $url,
        "--no-dht", "--no-lsd", "--no-pex", "--tracker-interval", "5s",
        "--seed-time", "$($TimeoutSeconds)s", "--jsonl"
    ) "seed-$($case.name)"

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $seen = [ordered]@{}
    while ((Get-Date) -lt $deadline) {
        $seen = Read-Families $mark
        if (@($case.expect | Where-Object { -not $seen.Contains($_) }).Count -eq 0) { break }
        if ($seed.HasExited) { break }
        Start-Sleep -Milliseconds 250
    }
    # The negative case has to be given the same chance to announce twice as
    # the positive one, or "one family" only says the run was short. It gets a
    # second announce interval after the family it does expect turns up.
    if ($case.name -eq "literal_host" -and $seen.Contains("ipv4")) {
        Start-Sleep -Seconds 8
        $seen = Read-Families $mark
    }

    if (-not $seed.HasExited) { Stop-Process -Id $seed.Id -Force -ErrorAction SilentlyContinue }
    $seed.WaitForExit(15000) | Out-Null

    $families = @($seen.Keys)
    $rows += [pscustomobject][ordered]@{
        case     = $case.name
        url      = $url
        expected = ($case.expect -join ",")
        families = ($families -join ",")
        sources  = (($seen.Keys | ForEach-Object { "$_=$($seen[$_])" }) -join " ")
    }

    foreach ($want in $case.expect) {
        if (-not $seen.Contains($want)) {
            $failures += "$($case.name) never announced over $want, only [$($families -join ',')]"
        }
    }
    foreach ($got in $families) {
        if ($case.expect -notcontains $got) {
            $failures += "$($case.name) announced over $got, which it was not asked to publish"
        }
    }
}

Stop-Background

$verdict = if ($failures.Count -eq 0) { "pass" } else { "fail" }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "tracker-family-$stamp.json"
[pscustomobject][ordered]@{
    kind           = "tracker_announce_family"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    }
    parameters     = [ordered]@{
        timeout_seconds = $TimeoutSeconds
        tracker_port    = $port
        info_hash       = $infoHash
        profile         = $Profile
    }
    cases          = @($rows)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "The subject is bit-cli seed, which runs a librqbit session. bit-cli trackers uses this repository's own tracker client and announced per family before any of this.",
        "literal_host is the control: a URL naming an address has no resolution to override, so one announce is correct there and two would be wrong.",
        "The tracker keys its peer records by (peer id, family). Keyed by peer id alone the second announce overwrites the first and one host ends up reachable on one family.",
        "The two announces go in sequence rather than concurrently, so which family a BEP 3 tracker keeps is the same every run instead of a race."
    )
} | ConvertTo-Json -Depth 10 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
$rows | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
Write-Host "report:  $reportPath"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-tracker-family: $failure") }
    exit 1
}
exit 0
