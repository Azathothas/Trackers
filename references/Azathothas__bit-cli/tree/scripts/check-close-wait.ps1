# Count the sockets a seeder leaves behind when peers come and go.
#
# `TODO/peers.md` T-020 is a report of twenty thousand sockets stuck in
# CLOSE_WAIT after two days as a service. CLOSE_WAIT means the peer sent FIN
# and the local side never called close, so the way to produce it is a peer
# that connects and leaves, thousands of times. Time is not the variable,
# connections are.
#
# Two modes, because which one leaks says where:
#
#   handshake     a BEP 3 handshake, the reply read, then close. The far side
#                 accepted a peer and the peer left, which is an ordinary
#                 disconnect.
#   no-handshake  connect and close without a byte. The far side accepted a
#                 connection whose handshake check then fails, which is what a
#                 port scan, a health check, or a half-open NAT looks like.
#
# For each mode it reports the socket state counts at four moments: before, the
# moment the churn stops, after a settle, and after a handful of ordinary
# connections. The fourth is the one that says what kind of defect it is. A
# residue that time does not clear but later traffic does is a queue the accept
# loop only drains when a connection arrives; a residue nothing clears is a
# leak. It also reports whether the listener is still there, because a listener
# that stops answering is worse than either: the process stays up, keeps
# reporting itself as seeding, and serves nobody.
#
# Usage:
#   pwsh scripts/check-close-wait.ps1
#   pwsh scripts/check-close-wait.ps1 -Connections 5000 -Concurrency 64 -Settle 60
#
# Exits 0 when the listener survived both modes, 1 when it did not, and 2 when
# the check could not run. The stuck socket count is recorded rather than
# judged unless -Ceiling names a number, because the leak behind it is upstream
# and open: see T-020. The listener dying is a regression in something bit-cli
# did fix, so that one always fails the run. The record goes to
# bench/close-wait-<timestamp>.json.
#
# See TODO/peers.md, T-020.

[CmdletBinding()]
param(
    [int]$Connections = 2000,
    [int]$Concurrency = 64,
    # Seconds to wait after the churn before the last count. A socket that is
    # going to be released is released well inside this.
    [int]$Settle = 60,
    # Stuck sockets allowed at the end. Zero, the default, records the count
    # without judging it, because the leak it measures is upstream and open.
    # T-020's acceptance is this script with -Ceiling 100, and it fails today.
    [int]$Ceiling = 0,
    # Handshaked connections to send after the settle, to see whether later
    # traffic clears the residue or adds to it. Zero skips it.
    [int]$Drain = 50,
    [string]$Root = ".tmp/close-wait",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-close-wait: $message")
    exit $code
}

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

trap { Stop-Background; throw }

if (-not ($IsWindows -or $env:OS -eq "Windows_NT")) {
    Exit-With 2 "this reads Get-NetTCPConnection, which is Windows only. On Linux use `ss -tan state close-wait`."
}
$exe = ".exe"
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
$churn = Join-Path $repo "target/$Profile/examples/loopback-churn$exe"
foreach ($required in @($bitCli, $churn)) {
    if (-not (Test-Path $required)) {
        Exit-With 2 "missing $required. Build it first: cargo build --$Profile --workspace --bins --examples"
    }
}
if ($Connections -lt 1) { Exit-With 2 "-Connections has to be at least 1." }

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path (Join-Path $Root "payload") | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# ---------------------------------------------------------------------------
# A payload to seed
# ---------------------------------------------------------------------------
#
# Small on purpose. Nothing here transfers a byte of payload: what is being
# counted is sockets, and a large payload would only make the seeder's hash
# check slow.

Write-Step "building a 4 MiB payload to seed"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 5150
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}
$stream = [System.IO.File]::Create((Join-Path $Root "payload/movie.bin"))
try { foreach ($i in 1..4) { $stream.Write($block, 0, $block.Length) } }
finally { $stream.Dispose() }

$torrent = Join-Path $Root "movie.torrent"
& $bitCli create (Join-Path $Root "payload") --name payload --piece-length 1MiB `
    --no-creation-date --output $torrent --force --json 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }
$infoHash = (& $bitCli info $torrent --json | ConvertFrom-Json).info_hash
if (-not $infoHash) { Exit-With 2 "could not read the info hash" }

function Get-SocketStates($processId) {
    $counts = [ordered]@{}
    Get-NetTCPConnection -OwningProcess $processId -ErrorAction SilentlyContinue |
        Group-Object State |
        Sort-Object Name |
        ForEach-Object { $counts[$_.Name] = $_.Count }
    $counts
}

function Get-CloseWait($processId) {
    @(Get-NetTCPConnection -OwningProcess $processId -State CloseWait -ErrorAction SilentlyContinue).Count
}

function Test-Listening($processId) {
    $null -ne (Get-NetTCPConnection -State Listen -OwningProcess $processId -ErrorAction SilentlyContinue |
            Select-Object -First 1)
}

function Start-Seed($tag) {
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $Root -NoNewWindow -PassThru `
        -ArgumentList @(
            "seed", $torrent, "--dir", $Root, "--port", "0",
            "--no-dht", "--no-lsd", "--seed-time", "30m", "--json"
        ) `
        -RedirectStandardOutput (Join-Path $Root "$tag.out") `
        -RedirectStandardError (Join-Path $Root "$tag.err")
    $script:Background += $process
    $deadline = (Get-Date).AddSeconds(30)
    $port = $null
    while (-not $port -and (Get-Date) -lt $deadline) {
        if ($process.HasExited) { Exit-With 2 "the seeder exited before it listened; see $Root/$tag.err" }
        $port = (Get-NetTCPConnection -State Listen -OwningProcess $process.Id -ErrorAction SilentlyContinue |
                Select-Object -First 1).LocalPort
        if (-not $port) { Start-Sleep -Milliseconds 200 }
    }
    if (-not $port) { Exit-With 2 "the seeder never opened a listening socket" }
    [pscustomobject]@{ process = $process; port = $port; log = (Join-Path $Root "$tag.err") }
}

function Stop-Seed($seed) {
    if ($seed.process -and -not $seed.process.HasExited) {
        Stop-Process -Id $seed.process.Id -Force -ErrorAction SilentlyContinue
    }
    $script:Background = @($script:Background | Where-Object { $_.Id -ne $seed.process.Id })
}

$rounds = [System.Collections.ArrayList]::new()
$failures = [System.Collections.ArrayList]::new()
$commands = [System.Collections.ArrayList]::new()

foreach ($mode in @("handshake", "no-handshake")) {
    Write-Step "$mode : $Connections connections, $Concurrency at a time"
    $seed = Start-Seed $mode
    $before = Get-SocketStates $seed.process.Id
    $seed.process.Refresh()
    $handlesBefore = $seed.process.HandleCount

    $churnArgs = @("--peer", "127.0.0.1:$($seed.port)", "--connections", "$Connections",
        "--concurrency", "$Concurrency")
    if ($mode -eq "no-handshake") { $churnArgs += "--no-handshake" }
    else { $churnArgs += @("--info-hash", $infoHash) }
    [void]$commands.Add("loopback-churn $($churnArgs -join ' ')")

    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $churnOut = Join-Path $Root "$mode-churn.json"
    $churnProcess = Start-Process -FilePath $churn -NoNewWindow -PassThru -ArgumentList $churnArgs `
        -RedirectStandardOutput $churnOut -RedirectStandardError (Join-Path $Root "$mode-churn.err")
    $finished = $churnProcess.WaitForExit(600 * 1000)
    $clock.Stop()
    if (-not $finished) {
        Stop-Process -Id $churnProcess.Id -Force -ErrorAction SilentlyContinue
    }

    # Taken the moment the churn stops, so it is what was held rather than what
    # is left.
    $duringCloseWait = Get-CloseWait $seed.process.Id
    Start-Sleep -Seconds $Settle
    $after = Get-SocketStates $seed.process.Id
    $stuck = Get-CloseWait $seed.process.Id
    $seed.process.Refresh()
    $handlesAfter = $seed.process.HandleCount
    $listening = Test-Listening $seed.process.Id

    # Does later traffic clear what is stuck, or add to it? The accept loop
    # drains its pending set inside the same `select!` that accepts, so a
    # residue that only clears when a new connection arrives is a stalled
    # queue rather than a leak, and the two need different fixes.
    $drained = $null
    $statesAfterDrain = $null
    $handlesAfterDrain = $null
    if ($Drain -gt 0 -and $listening) {
        & $churn --peer "127.0.0.1:$($seed.port)" --info-hash $infoHash `
            --connections $Drain --concurrency 4 2>$null | Out-Null
        Start-Sleep -Seconds 5
        $drained = Get-CloseWait $seed.process.Id
        $statesAfterDrain = Get-SocketStates $seed.process.Id
        $seed.process.Refresh()
        $handlesAfterDrain = $seed.process.HandleCount
    }

    $churnReport = $null
    try { $churnReport = Get-Content $churnOut -Raw | ConvertFrom-Json } catch { }
    $panicked = (Select-String -Path $seed.log -Pattern 'panicked' -Quiet -ErrorAction SilentlyContinue) -eq $true
    Stop-Seed $seed

    # The listener dying is a regression in something bit-cli fixed and fails
    # the run. The stuck sockets are a defect it has not, so they fail only
    # when a caller names a ceiling to hold them to.
    $ok = $listening -and (-not $panicked) -and ($Ceiling -le 0 -or $stuck -le $Ceiling)
    [void]$rounds.Add([ordered]@{
        mode                  = $mode
        connections_asked     = $Connections
        connections_completed = if ($churnReport) { $churnReport.completed } else { 0 }
        connections_failed    = if ($churnReport) { $churnReport.failed } else { $Connections }
        elapsed_ms            = $clock.ElapsedMilliseconds
        states_before         = $before
        states_after          = $after
        close_wait_during     = $duringCloseWait
        close_wait_after      = $stuck
        handles_before        = $handlesBefore
        handles_after         = $handlesAfter
        settle_s              = $Settle
        close_wait_after_drain = $drained
        states_after_drain    = $statesAfterDrain
        handles_after_drain   = $handlesAfterDrain
        drain_connections     = $Drain
        listening_after       = $listening
        panicked              = $panicked
        ok                    = $ok
    })
    if ($panicked) {
        [void]$failures.Add("$mode : the seeder panicked; the listener is gone and the process is still running")
    }
    elseif (-not $listening) {
        [void]$failures.Add("$mode : the seeder stopped listening")
    }
    elseif ($Ceiling -gt 0 -and $stuck -gt $Ceiling) {
        [void]$failures.Add("$mode : $stuck sockets in CLOSE_WAIT after ${Settle}s, over the ceiling of $Ceiling")
    }
}

Stop-Background

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "close-wait-$stamp.json"
$verdict = switch ($true) {
    ($failures.Count -eq 0 -and $Ceiling -gt 0) { "both modes ended under $Ceiling stuck sockets with the listener alive"; break }
    ($failures.Count -eq 0) { "the listener survived both modes; the stuck socket counts are recorded, not judged"; break }
    default { "$($failures.Count) of $($rounds.Count) modes did not"; break }
}

[ordered]@{
    kind           = "check-close-wait"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = [System.Environment]::MachineName
        os      = [System.Environment]::OSVersion.VersionString
        cpus    = [System.Environment]::ProcessorCount
    }
    parameters     = [ordered]@{
        connections = $Connections
        concurrency = $Concurrency
        settle_s    = $Settle
        ceiling     = $Ceiling
        profile     = $Profile
    }
    info_hash      = $infoHash
    rounds         = @($rounds)
    verdict        = $verdict
    failures       = @($failures)
    commands       = @($commands)
    notes          = @(
        "CLOSE_WAIT means the peer sent FIN and the local side never called close. A socket in TIME_WAIT is the opposite and is normal, which is why the state counts are reported in full rather than as one number.",
        "Each mode gets its own seeder, so one mode's residue cannot be charged to the other.",
        "close_wait_during is read the moment the churn stops and close_wait_after once the settle has passed, so the pair says whether time alone releases anything. close_wait_after_drain is read after a few ordinary connections, and the drop from the settled count to that one is what says the residue is a stalled queue rather than a leak.",
        "panicked reads the seeder's own stderr. librqbit's accept loop panics when its pending handshake check set is full and one of those checks fails, which kills the listener while the process keeps running. See TODO/peers.md, T-020."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "connections: $Connections at $Concurrency at a time, settle ${Settle}s"
Write-Host "report:      $reportPath"
Write-Host ""
$rounds | ForEach-Object {
    [pscustomobject][ordered]@{
        mode          = $_.mode
        completed     = $_.connections_completed
        failed        = $_.connections_failed
        "CW during"   = $_.close_wait_during
        "CW after"    = $_.close_wait_after
        "CW drained"  = $_.close_wait_after_drain
        "handles"     = "$($_.handles_before) -> $($_.handles_after)"
        listening     = if ($_.listening_after) { "yes" } else { "NO" }
        panicked      = if ($_.panicked) { "YES" } else { "no" }
        ok            = if ($_.ok) { "yes" } else { "NO" }
    }
} | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-close-wait: $failure") }
    exit 1
}
exit 0
