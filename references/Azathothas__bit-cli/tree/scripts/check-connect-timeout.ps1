# Prove that --web-seed-connect-timeout is the bound, and --web-seed-timeout is not.
#
# `TODO/webseed.md` T-141 recorded that `--web-seed-connect-timeout` bounds
# nothing: halving it did not move the wall clock and raising
# `--web-seed-timeout` moved it exactly. That measurement used
# `http://127.0.0.1:9/`, and port 9 on Windows is the **discard** service. It
# accepts the connection and never answers. So the connect succeeded in
# microseconds and the run was correctly bounded by the request timeout. The
# flag was never exercised.
#
# The two timeouts bound two different things and a check has to drive both:
#
#   blackhole   a route that never answers a SYN. The connect never completes,
#               so `--web-seed-connect-timeout` is the bound and
#               `--web-seed-timeout` should not move the number at all.
#   discard     a listener that accepts and never sends a byte. The connect
#               completes at once, so `--web-seed-timeout` is the bound and
#               `--web-seed-connect-timeout` should not move the number.
#
# The discard side is served here rather than borrowed from the machine: a
# `TcpListener` that accepts and holds is four lines of PowerShell and is the
# same on every host, where `TCPSVCS` is a Windows optional feature that may
# not be installed.
#
# The blackhole side cannot be served, because it is the absence of an answer
# rather than an answer. `-Blackhole` defaults to `192.0.2.1:80`, which RFC 5737
# reserves as TEST-NET-1 and no network is supposed to route. That is a
# property of the network rather than of this repository, so the script proves
# it before it measures anything: a raw connect to the address must still be
# pending after -ProbeSeconds. If the network answers, this exits 2 and says
# so, because a check that silently measured a refused connection would report
# a pass it had not earned.
#
# Usage:
#   pwsh scripts/check-connect-timeout.ps1
#   pwsh scripts/check-connect-timeout.ps1 -Blackhole 10.255.255.1:80
#
# Exits 0 when both bounds hold, 1 when one does not, and 2 when the check
# could not run. The record goes to bench/connect-timeout-<timestamp>.json.
#
# See TODO/webseed.md, T-141.

[CmdletBinding()]
param(
    # An address that drops the SYN. TEST-NET-1 by default: RFC 5737 reserves
    # 192.0.2.0/24 for documentation and no network routes it.
    [string]$Blackhole = "192.0.2.1:80",
    # How long a raw connect to $Blackhole must stay pending before the address
    # counts as a blackhole. Two seconds is far longer than any answer takes.
    [int]$ProbeSeconds = 2,
    # The connect timeouts to sweep, in seconds.
    [int[]]$ConnectSeconds = @(2, 5),
    # The request timeouts to sweep them against. Both are far longer than
    # every connect timeout, which is what makes the comparison mean something.
    [int[]]$RequestSeconds = @(30, 45),
    # How far a run may sit past the bound it should be honouring. Process
    # start, metainfo parsing, and the session coming up are inside this.
    [double]$SlackSeconds = 1.5,
    [string]$Root = ".tmp/connect-timeout",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

$script:Listener = $null
$script:Accepted = @()

function Stop-Background {
    foreach ($client in $script:Accepted) {
        if ($client) { $client.Close() }
    }
    $script:Accepted = @()
    if ($script:Listener) {
        $script:Listener.Stop()
        $script:Listener = $null
    }
}

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-connect-timeout: $message")
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
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --workspace --bins --examples"
}
foreach ($value in $ConnectSeconds) {
    foreach ($limit in $RequestSeconds) {
        if ($value -ge $limit) {
            Exit-With 2 "-ConnectSeconds $value is not shorter than -RequestSeconds $limit, so neither run could tell the two apart."
        }
    }
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# ---------------------------------------------------------------------------
# The address has to actually be a blackhole
# ---------------------------------------------------------------------------

$parts = $Blackhole -split ':'
if ($parts.Count -ne 2) { Exit-With 2 "-Blackhole has to be host:port, not '$Blackhole'" }
$blackholeHost = $parts[0]
$blackholePort = [int]$parts[1]

Write-Step "checking that $Blackhole drops the SYN"
$probe = [System.Net.Sockets.TcpClient]::new()
try {
    $pending = $probe.BeginConnect($blackholeHost, $blackholePort, $null, $null)
    $answered = $pending.AsyncWaitHandle.WaitOne([timespan]::FromSeconds($ProbeSeconds))
    if ($answered) {
        $what = "refused or accepted"
        try { $probe.EndConnect($pending); $what = "accepted the connection" }
        catch { $what = "answered: $($_.Exception.InnerException.Message)" }
        Exit-With 2 "$Blackhole $what within ${ProbeSeconds}s, so it is not a blackhole on this network. Pass -Blackhole with an address that is."
    }
}
finally { $probe.Close() }
Write-Step "$Blackhole is still pending after ${ProbeSeconds}s"

# ---------------------------------------------------------------------------
# A listener that accepts and never answers, for the other half
# ---------------------------------------------------------------------------

$script:Listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$script:Listener.Start()
$discardPort = $script:Listener.LocalEndpoint.Port
$discard = "http://127.0.0.1:$discardPort/"
Write-Step "discard listener on $discard"

# Accepting has to happen while `bit-cli` is connecting, and this script is
# single threaded, so the accept runs on a runspace of its own. It holds every
# connection open and reads nothing, which is what makes the far side wait.
$accepting = [powershell]::Create()
$accepting.AddScript({
        param($listener)
        $held = @()
        while ($true) {
            try { $held += $listener.AcceptTcpClient() }
            catch { break }
        }
    }).AddArgument($script:Listener) | Out-Null
$acceptHandle = $accepting.BeginInvoke()

# ---------------------------------------------------------------------------
# A torrent to point the sources at
# ---------------------------------------------------------------------------
#
# The payload never moves, so its content does not matter and its size only has
# to be enough for a torrent to exist.

$payload = Join-Path $Root "payload"
New-Item -ItemType Directory -Force -Path $payload | Out-Null
$block = [byte[]]::new(256 * 1024)
[int64]$state = 41
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}
[System.IO.File]::WriteAllBytes((Join-Path $payload "target.bin"), $block)

$torrent = Join-Path $Root "target.torrent"
& $bitCli create $payload --name target --piece-length 64KiB --no-creation-date `
    --output $torrent --force --json 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }

# ---------------------------------------------------------------------------
# The sweep
# ---------------------------------------------------------------------------

function Measure-Run([string]$url, [int]$connectSeconds, [int]$requestSeconds) {
    $arguments = @(
        "--json", "webseed", "test", $torrent,
        "--no-torrent-web-seed",
        "--web-seed", $url,
        "--web-seed-connect-timeout", "${connectSeconds}s",
        "--web-seed-timeout", "${requestSeconds}s"
    )
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $out = & $bitCli @arguments 2>$null
    $watch.Stop()
    $code = $LASTEXITCODE
    $error = ""
    try {
        $document = $out | Out-String | ConvertFrom-Json
        $error = $document.sources[0].error
    }
    catch { $error = "" }
    [pscustomobject][ordered]@{
        url             = $url
        connect_seconds = $connectSeconds
        request_seconds = $requestSeconds
        wall_ms         = [int]$watch.Elapsed.TotalMilliseconds
        exit_code       = $code
        error           = $error
    }
}

$runs = @()
$failures = @()

Write-Step "sweeping the blackhole at $Blackhole"
foreach ($connect in $ConnectSeconds) {
    foreach ($request in $RequestSeconds) {
        $run = Measure-Run "http://$Blackhole/" $connect $request
        $runs += $run
        $bound = $connect * 1000
        $ceiling = $bound + ($SlackSeconds * 1000)
        if ($run.wall_ms -gt $ceiling) {
            $failures += "blackhole at connect=${connect}s request=${request}s took $($run.wall_ms) ms, over the $ceiling ms the connect timeout allows. The request timeout is bounding it."
        }
        Write-Host ("  blackhole connect={0,2}s request={1,2}s  {2,7} ms  {3}" -f $connect, $request, $run.wall_ms, $run.error)
    }
}

Write-Step "sweeping the discard listener at $discard"
foreach ($connect in $ConnectSeconds) {
    foreach ($request in $RequestSeconds) {
        $run = Measure-Run $discard $connect $request
        $runs += $run
        # The other direction: the connect completes, so the connect timeout
        # must not be what ends the run. Anything at or under it would mean the
        # flag is bounding a connect that already succeeded.
        $floor = ($connect * 1000) + ($SlackSeconds * 1000)
        if ($run.wall_ms -lt $floor) {
            $failures += "discard at connect=${connect}s request=${request}s took $($run.wall_ms) ms, under the $floor ms floor. The connect timeout is ending a connect that succeeded."
        }
        Write-Host ("  discard   connect={0,2}s request={1,2}s  {2,7} ms  {3}" -f $connect, $request, $run.wall_ms, $run.error)
    }
}

# The two flags have to be independent, and the sweep is what shows it. Group
# the blackhole runs by connect timeout: within a group the request timeout
# changed and the wall clock should not have.
$blackholeRuns = $runs | Where-Object { $_.url -eq "http://$Blackhole/" }
foreach ($group in ($blackholeRuns | Group-Object connect_seconds)) {
    $spread = ($group.Group | Measure-Object wall_ms -Maximum -Minimum)
    $delta = $spread.Maximum - $spread.Minimum
    if ($delta -gt ($SlackSeconds * 1000)) {
        $failures += "at connect=$($group.Name)s the request timeout moved the wall clock by $delta ms, so it is not the connect timeout that bounds a blackhole."
    }
}

$discardRuns = $runs | Where-Object { $_.url -eq $discard }
foreach ($group in ($discardRuns | Group-Object request_seconds)) {
    $spread = ($group.Group | Measure-Object wall_ms -Maximum -Minimum)
    $delta = $spread.Maximum - $spread.Minimum
    if ($delta -gt ($SlackSeconds * 1000)) {
        $failures += "at request=$($group.Name)s the connect timeout moved the wall clock by $delta ms, so it is reaching a connect that succeeded."
    }
}

# The message has to say which timeout expired, or the reader turns the wrong
# knob. This is the reporting half of T-141.
foreach ($run in $blackholeRuns) {
    if ($run.error -notmatch 'connect timed out') {
        $failures += "the blackhole run at connect=$($run.connect_seconds)s reported `"$($run.error)`", which does not name the connect timeout."
    }
}
foreach ($run in $discardRuns) {
    if ($run.error -notmatch 'timed out waiting for the response') {
        $failures += "the discard run at request=$($run.request_seconds)s reported `"$($run.error)`", which does not name the request timeout."
    }
}

Stop-Background
$accepting.Stop()
$accepting.Dispose() | Out-Null

$verdict = if ($failures.Count -eq 0) { "pass" } else { "fail" }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "connect-timeout-$stamp.json"

[pscustomobject][ordered]@{
    kind           = "connect_timeout"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    }
    parameters     = [ordered]@{
        blackhole        = $Blackhole
        discard          = $discard
        connect_seconds  = @($ConnectSeconds)
        request_seconds  = @($RequestSeconds)
        slack_seconds    = $SlackSeconds
        profile          = $Profile
    }
    runs           = @($runs)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "The blackhole address is not served by this script. It is an address the network does not route, and the script proves a raw connect to it is still pending before it measures anything.",
        "The discard listener accepts and never writes, which is the case the earlier T-141 measurement mistook for a blackhole: 127.0.0.1:9 on Windows is the TCPSVCS discard service, so the connect succeeded and the request timeout was correctly the bound.",
        "The two sweeps are the two directions. On a blackhole the connect timeout has to be the bound and the request timeout has to move nothing. On a discard listener the reverse."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "report:  $reportPath"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-connect-timeout: $failure") }
    exit 1
}
exit 0
