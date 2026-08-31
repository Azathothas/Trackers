# What a UDP tracker that does not answer actually costs.
#
# This is the acceptance for `TODO/trackers.md` T-064. `bit-cli` diverges from
# BEP 15 on purpose: the spec's ladder is `15 * 2^n` for n up to 8, which is up
# to 62 minutes, and this is a foreground tool. What it does instead is three
# attempts inside `--tracker-timeout`. What the entry owed was the **total**,
# which is what a caller setting a deadline needs and what the two corpus
# implementations both state.
#
# The total is not one number, because a UDP announce is two exchanges,
# connect then announce, and either can be the one that dies:
#
#   dead          Nothing answers. The connect exchange spends all three
#                 attempts and the announce is never sent. Three attempts.
#   half_dead     Connect is answered at once and the announce is not. Three.
#   slow_connect  Connect is answered on its third attempt and the announce is
#                 not. Five, and this is the worst case there is: a connect
#                 that is not answered by its third attempt gives up, so six
#                 cannot happen.
#
# So the worst case is **five attempts of `max(--tracker-timeout / 3, 1s)`**,
# which is `5/3` of the timeout and never less than five seconds. The floor is
# why a timeout under three seconds buys nothing.
#
# The cases are measured against purpose-built loopback sockets rather than
# against a real tracker: one that happens to answer measures nothing, and one
# that happens to be down measures the network.
#
# Usage:
#   pwsh scripts/check-udp-retry.ps1
#   pwsh scripts/check-udp-retry.ps1 -Timeouts 3s,12s
#
# Exits 0 when both budgets hold, 1 when one does not, and 2 when the check
# could not run. The record goes to bench/udp-retry-<timestamp>.json.
#
# See TODO/trackers.md, T-064.

[CmdletBinding()]
param(
    [string[]]$Timeouts = @("1s", "3s", "6s"),
    [double]$Tolerance = 0.6,
    [string]$Root = ".tmp/udpretry",
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
    [Console]::Error.WriteLine("check-udp-retry: $message")
    exit $code
}

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --workspace --bins --examples"
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# A torrent to hang the announce off. The payload is irrelevant; only the
# announce is being measured.
$payload = Join-Path $Root "payload.bin"
[System.IO.File]::WriteAllBytes($payload, [byte[]]::new(65536))
$torrent = Join-Path $Root "p.torrent"
$create = Start-Process -FilePath $bitCli -ArgumentList @(
    "create", $payload, "--piece-length", "16KiB", "--no-creation-date",
    "--output", $torrent, "--force", "--json"
) -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $Root "create.out") `
    -RedirectStandardError (Join-Path $Root "create.err")
$create.WaitForExit(60000) | Out-Null
if ($create.ExitCode -ne 0) { Exit-With 2 "bit-cli create exited $($create.ExitCode)" }

# ---------------------------------------------------------------------------
# The two sockets
# ---------------------------------------------------------------------------
#
# A bound socket that reads and never answers, not a closed port: Windows
# answers a closed UDP port with ICMP unreachable, which the client reads as an
# error rather than a timeout, and that measures the wrong thing.

# Every socket here binds loopback explicitly rather than the wildcard. A
# wildcard-bound socket's reply was not accepted by the client at all: the
# client connects its own socket to the target, so it takes datagrams only from
# that exact address, and the source the kernel picks for a wildcard socket is
# not guaranteed to be the one the client dialled. Bound to loopback it is.

$dead = [System.Net.Sockets.UdpClient]::new(
    [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Loopback, 0))
$deadPort = ([System.Net.IPEndPoint]$dead.Client.LocalEndPoint).Port

# A responder that answers BEP 15 connect after ignoring the first `$Ignore` of
# them, and never answers anything else. Connect is 16 bytes: protocol id,
# action 0, transaction id. The reply is 16: action 0, the same transaction id,
# and a connection id.
$responderScript = Join-Path $Root "responder.ps1"
@'
param([int]$Ignore = 0)
# Byte arithmetic rather than BinaryPrimitives, whose overloads take Span<byte>
# and cannot be handed a byte[] from PowerShell.
$socket = [System.Net.Sockets.UdpClient]::new(
    [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Loopback, 0))
$port = ([System.Net.IPEndPoint]$socket.Client.LocalEndPoint).Port
Write-Host "port=$port"
$from = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 0)
$seen = 0
while ($true) {
    $datagram = $socket.Receive([ref]$from)
    if ($datagram.Length -lt 16) { continue }
    $action = ([int]$datagram[8] -shl 24) -bor ([int]$datagram[9] -shl 16) -bor `
        ([int]$datagram[10] -shl 8) -bor [int]$datagram[11]
    if ($action -ne 0) { continue }
    $seen++
    if ($seen -le $Ignore) { continue }
    $reply = [byte[]]::new(16)
    [System.Array]::Copy($datagram, 12, $reply, 4, 4)
    $reply[15] = 42
    $null = $socket.Send($reply, $reply.Length, $from)
}
'@ | Set-Content -Path $responderScript -Encoding utf8NoBOM

$script:Responders = @()

function Start-Responder([string]$name, [int]$ignore) {
    $out = Join-Path $Root "$name.out"
    $proc = Start-Process -FilePath "pwsh" -ArgumentList @(
        "-NoProfile", "-File", $responderScript, "-Ignore", "$ignore"
    ) -PassThru -NoNewWindow -RedirectStandardOutput $out `
        -RedirectStandardError (Join-Path $Root "$name.err")
    $script:Responders += $proc
    for ($attempt = 0; $attempt -lt 200; $attempt++) {
        Start-Sleep -Milliseconds 100
        if ($proc.HasExited) {
            Exit-With 2 "the $name responder exited $($proc.ExitCode): $(Get-Content (Join-Path $Root "$name.err") -Raw)"
        }
        $line = (Get-Content $out -ErrorAction SilentlyContinue) |
            Where-Object { $_ -match '^port=(\d+)$' } | Select-Object -First 1
        if ($line -and $line -match '^port=(\d+)$') { return [int]$Matches[1] }
    }
    Exit-With 2 "the $name responder never printed a port"
}

$halfPort = Start-Responder "half" 0
Write-Step "dead on 127.0.0.1:$deadPort, half-dead on 127.0.0.1:$halfPort"

function Stop-Background {
    foreach ($proc in $script:Responders) {
        if ($proc -and -not $proc.HasExited) { try { $proc.Kill() } catch { } }
    }
    $script:Responders = @()
    if ($dead) { try { $dead.Close() } catch { } }
}
trap { Stop-Background; throw }

# ---------------------------------------------------------------------------
# Measuring
# ---------------------------------------------------------------------------

function Measure-Announce([string]$name, [int]$port, [string]$timeout) {
    $tag = "$name-$timeout"
    $so = Join-Path $Root "$tag.out"
    $se = Join-Path $Root "$tag.err"
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $proc = Start-Process -FilePath $bitCli -ArgumentList @(
        "--json", "trackers", $torrent,
        "--tracker", "udp://127.0.0.1:$port/announce", "--replace-trackers",
        "--tracker-timeout", $timeout
    ) -PassThru -NoNewWindow -RedirectStandardOutput $so -RedirectStandardError $se
    if (-not $proc.WaitForExit(300000)) { try { $proc.Kill() } catch { } }
    $watch.Stop()
    $report = $null
    try { $report = Get-Content $so -Raw | ConvertFrom-Json } catch { }
    [pscustomobject]@{
        elapsed = $watch.Elapsed.TotalSeconds
        exit    = $proc.ExitCode
        failure = if ($report.trackers) { $report.trackers[0].failure } else { "" }
    }
}

# One attempt is the timeout divided by three with a one second floor. That
# floor is why 1s and 3s cost the same.
function Get-Attempt([string]$timeout) {
    $seconds = switch -Regex ($timeout) {
        '^(\d+(\.\d+)?)ms$' { [double]$Matches[1] / 1000 }
        '^(\d+(\.\d+)?)s$' { [double]$Matches[1] }
        '^(\d+(\.\d+)?)m$' { [double]$Matches[1] * 60 }
        default { Exit-With 2 "cannot read the timeout $timeout" }
    }
    [math]::Max($seconds / 3, 1.0)
}

$rows = @()
$failures = @()
foreach ($timeout in $Timeouts) {
    $attempt = Get-Attempt $timeout

    # A responder per case per timeout, because `slow_connect` counts the
    # connects it has ignored and one that carried a previous case's count
    # would answer the first attempt of this one.
    $slowPort = Start-Responder "slow-$timeout" 2

    $cases = @(
        @{ name = "dead"; port = $deadPort; attempts = 3 },
        @{ name = "half_dead"; port = $halfPort; attempts = 3 },
        @{ name = "slow_connect"; port = $slowPort; attempts = 5 }
    )
    $row = [ordered]@{ tracker_timeout = $timeout; attempt_s = $attempt }
    foreach ($case in $cases) {
        $run = Measure-Announce $case.name $case.port $timeout
        $budget = $attempt * $case.attempts
        $row["$($case.name)_elapsed_s"] = [math]::Round($run.elapsed, 2)
        $row["$($case.name)_budget_s"] = [math]::Round($budget, 2)
        $row["$($case.name)_exit"] = $run.exit
        Write-Step ("--tracker-timeout {0,-3} {1,-12} {2,6:N2}s against {3,5:N2}s ({4} attempts)" -f `
                $timeout, $case.name, $run.elapsed, $budget, $case.attempts)

        if ($run.exit -ne 6) {
            $failures += "$($case.name) at $timeout exited $($run.exit), expected 6"
        }
        if ($run.failure -notmatch "timed out") {
            $failures += "$($case.name) at $timeout failed with '$($run.failure)', expected a timeout"
        }
        # Both sides. Over the budget means the budget is not the budget; under
        # it means an attempt was skipped, which is the same defect read the
        # other way round.
        if ($run.elapsed -gt ($budget + $Tolerance)) {
            $failures += ("$($case.name) at $timeout took {0:N2}s, over the {1:N2}s budget" -f $run.elapsed, $budget)
        }
        if ($run.elapsed -lt ($budget - $Tolerance)) {
            $failures += ("$($case.name) at $timeout took {0:N2}s, under the {1:N2}s budget, so an attempt was skipped" -f $run.elapsed, $budget)
        }
    }
    $rows += [pscustomobject]$row
}

Stop-Background

$verdict = if ($failures.Count -eq 0) { "pass" } else { "fail" }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "udp-retry-$stamp.json"
[pscustomobject][ordered]@{
    kind           = "udp_retry_budget"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    }
    parameters     = [ordered]@{
        timeouts    = @($Timeouts)
        tolerance_s = $Tolerance
        profile     = $Profile
    }
    samples        = @($rows)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "One attempt is max(--tracker-timeout / 3, 1s) and one exchange is three attempts, so a timeout under three seconds buys nothing.",
        "A UDP announce is two exchanges and the worst case is five attempts, not six: a connect that is not answered by its third attempt gives up, so the announce that would spend three more is never sent.",
        "The sockets are built here rather than borrowed from a real tracker: a tracker that happens to answer measures nothing, and one that happens to be down measures the network.",
        "Each responder binds loopback rather than the wildcard. The client connects its own UDP socket to the target and takes datagrams only from that address, and a wildcard-bound reply was not accepted."
    )
} | ConvertTo-Json -Depth 10 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
$rows | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
Write-Host "report:  $reportPath"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-udp-retry: $failure") }
    exit 1
}
exit 0
