# Does `seed --listener-check` see its own listener stop answering?
#
# This is the acceptance for `TODO/peers.md` T-020's second finding. The first
# finding was a panic and is fixed. The second is that `librqbit` 9.0.0's
# accept loop clears one queued handshake check per connection it accepts, so
# a run of peers that close before they handshake leaves a backlog and every
# peer after it waits behind one. The target then accepts TCP, answers no
# handshake, and goes on reporting itself as seeding. Nothing a supervisor
# watches says so: the process is alive and the port is open.
#
# `--listener-check` is what says so. Four cases:
#
#   healthy       A seeder nobody has poisoned. Every probe is answered, the
#                 run is not stopped, and the probes leave nothing in
#                 `peer_detail`: a probe completes a real handshake, so the
#                 session keeps a peer row for it, and those rows are this
#                 process talking to itself.
#   survives_load The same seeder, then `bench swarm --peers N --torrents 1`,
#                 which is the load that used to leave the backlog. The run
#                 must NOT stop, and the listener must report itself healthy
#                 with zero consecutive failures.
#   off           No `--listener-check`, same load, and the run carries on to
#                 its own `--stop-after`, which is exit 9 and
#                 `"stopped": "deadline"`. It shows the load does not stop a
#                 run by itself, with the probe out of the picture entirely.
#   recovery      How many incoming connections it takes to clear whatever the
#                 load left. One: the first peer after the load is served.
#
# Three of those four asserted the opposite until 2026-08-22, because the
# accept loop drained one queued handshake check per connection it accepted and
# this script was written to characterise that. The fix is in the vendored
# tree; the cases are inverted rather than deleted, so the defect cannot come
# back unnoticed. What the old `poisoned` case uniquely proved, that
# `--listener-check` can stop a real run with exit 17, is not reachable from a
# healthy listener any more; the threshold logic behind it is covered by
# `three_unanswered_probes_in_a_row_stop_the_run` and its two neighbours in
# `crates/bit-cli/src/swarm.rs`.
#
# Usage:
#   pwsh scripts/check-listener.ps1
#   pwsh scripts/check-listener.ps1 -Poison 40
#
# Exits 0 when every case behaves as described, 1 when one does not, and 2
# when the check could not run. The record goes to
# bench/listener-<timestamp>.json.
#
# See TODO/peers.md, T-020.

[CmdletBinding()]
param(
    [int]$Poison = 20,
    [int]$PayloadMiB = 4,
    [string]$Interval = "2s",
    [string]$Root = ".tmp/listener",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
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
    [Console]::Error.WriteLine("check-listener: $message")
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

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# ---------------------------------------------------------------------------
# A payload, a torrent, and a seeder serving it
# ---------------------------------------------------------------------------

$serve = Join-Path $Root "serve"
New-Item -ItemType Directory -Force -Path $serve | Out-Null
Write-Step "building a $PayloadMiB MiB payload"
$payloadBytes = [byte[]]::new($PayloadMiB * 1024 * 1024)
[int64]$state = 29
for ($i = 0; $i -lt $payloadBytes.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $payloadBytes[$i] = [byte](($state -shr 16) -band 0xFF)
}
[System.IO.File]::WriteAllBytes((Join-Path $serve "payload.bin"), $payloadBytes)

# Through Start-Process with redirect files, like every other check script:
# whether a line on stderr ends the run otherwise depends on the host's pwsh
# version. See TODO/windows.md under T-075.
$torrent = Join-Path $Root "payload.torrent"
$createProc = Start-Process -FilePath $bitCli -ArgumentList @(
    "create", (Join-Path $serve "payload.bin"), "--piece-length", "256KiB",
    "--no-creation-date", "--output", $torrent, "--force", "--json"
) -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $Root "create.out") `
    -RedirectStandardError (Join-Path $Root "create.err")
$createProc.WaitForExit(60000) | Out-Null
if ($createProc.ExitCode -ne 0) {
    Exit-With 2 "bit-cli create exited $($createProc.ExitCode): $(Get-Content (Join-Path $Root 'create.err') -Raw)"
}

$script:SeederIndex = 0

# Port zero, and the port comes back out of the seeder's own event stream. A
# port this script picked could already be in use, and dialling it would
# measure whatever else was listening.
function Start-Seeder([string[]]$extra, [string]$stopAfter) {
    Stop-Background
    $script:SeederIndex++
    $tag = "seed-$($script:SeederIndex)"
    $script:SeedOut = Join-Path $Root "$tag.out"
    $script:SeedErr = Join-Path $Root "$tag.err"
    $arguments = @(
        "--jsonl", "seed", $torrent, "--dir", $serve, "--port", "0",
        "--no-tracker", "--no-dht", "--no-lsd", "--stop-after", $stopAfter,
        "--report-interval", "1s"
    ) + $extra
    $script:Seeder = Start-Process -FilePath $bitCli -ArgumentList $arguments `
        -PassThru -NoNewWindow -RedirectStandardOutput $script:SeedOut `
        -RedirectStandardError $script:SeedErr

    $addr = $null
    for ($attempt = 0; $attempt -lt 150; $attempt++) {
        Start-Sleep -Milliseconds 100
        if ($script:Seeder.HasExited) {
            Exit-With 2 "the seeder exited $($script:Seeder.ExitCode): $(Get-Content $script:SeedErr -Raw)"
        }
        foreach ($line in (Get-Content $script:SeedOut -ErrorAction SilentlyContinue)) {
            try { $event = $line | ConvertFrom-Json } catch { continue }
            if ($event.listen_addr) { $addr = $event.listen_addr }
        }
        if ($addr) { break }
    }
    if (-not $addr) { Exit-With 2 "the seeder never printed a listen address. stderr: $(Get-Content $script:SeedErr -Raw)" }
    $script:Target = "127.0.0.1:$(($addr -split ':')[-1])"
    Write-Step "seeder $($script:SeederIndex) on $($script:Target)"
    $script:Target
}

# Every `progress` event the seeder has written so far.
function Get-Progress {
    $events = @()
    foreach ($line in (Get-Content $script:SeedOut -ErrorAction SilentlyContinue)) {
        try { $event = $line | ConvertFrom-Json } catch { continue }
        if ($null -ne $event.uploaded_bytes) { $events += $event }
    }
    $events
}

# Wait for a condition rather than for a duration. A test that waits out a
# guessed number of seconds is asserting a scheduling outcome it does not
# control; see TODO/RULES.md section 5.
function Wait-For([scriptblock]$condition, [int]$seconds, [string]$what) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if (& $condition) { return $true }
        Start-Sleep -Milliseconds 250
    }
    Write-Step "  timed out after ${seconds}s waiting for $what"
    $false
}

function Invoke-Poison([string]$name) {
    $report = Join-Path $Root "$name.json"
    $work = Join-Path $Root "work/$name"
    New-Item -ItemType Directory -Force -Path $work | Out-Null
    $arguments = @(
        "bench", "swarm", $script:Target, "--report", $report, "--format", "json",
        "--peers", "$Poison", "--torrents", "1", "--disk-budget", "256MiB",
        "--duration", "12s", "--warmup", "500ms", "--connect-timeout", "5s",
        "--dir", $work
    )
    $process = Start-Process -FilePath $bitCli -ArgumentList $arguments -PassThru -NoNewWindow `
        -RedirectStandardOutput (Join-Path $Root "$name.out") `
        -RedirectStandardError (Join-Path $Root "$name.err")
    if (-not $process.WaitForExit(120000)) { try { $process.Kill() } catch { } }
    if (Test-Path $report) { return (Get-Content $report -Raw | ConvertFrom-Json) }
    $null
}

$cases = @()
$failures = @()
function Add-Failure([string]$name, [string]$message) {
    $script:failures += "${name}: $message"
}

# ---------------------------------------------------------------------------
# healthy: nobody has poisoned it, so every probe is answered
# ---------------------------------------------------------------------------

Write-Step "case healthy (--listener-check $Interval, no load)"
# 240s, not 90s: this seeder is reused by the case below, whose load runs for
# about 85 seconds and which then has to find it still alive. At 90s the
# deadline fired first and the seeder looked like it had been stopped by the
# load.
Start-Seeder @("--listener-check", $Interval) "240s" | Out-Null

# Three answered probes is the same number the stop condition needs to see
# fail, so this is the exact counterpart of the case below.
$sawThree = Wait-For {
    $last = (Get-Progress) | Select-Object -Last 1
    $last -and $last.listener -and $last.listener.probes -ge 3
} 40 "three probes"
$healthySample = (Get-Progress) | Select-Object -Last 1
if (-not $sawThree) {
    Add-Failure "healthy" "the seeder made fewer than three probes in 40s, so the case measured nothing"
}
elseif (-not $healthySample.listener.healthy) {
    Add-Failure "healthy" "an unpoisoned seeder reported its own listener unhealthy: $($healthySample.listener | ConvertTo-Json -Compress)"
}
if ($healthySample.listener.consecutive_failures -ne 0) {
    Add-Failure "healthy" "$($healthySample.listener.consecutive_failures) consecutive failures against a seeder nothing had touched"
}
# The probes each complete a real handshake, so the session records a peer row
# for each. Those rows are this process, and the reported peer list drops them.
$probeRows = @($healthySample.peer_detail).Count
if ($probeRows -ne 0) {
    Add-Failure "healthy" "$probeRows peer rows after $($healthySample.listener.probes) probes; the probe's own rows are not being dropped"
}
if ($script:Seeder.HasExited) {
    Add-Failure "healthy" "the seeder exited $($script:Seeder.ExitCode) with nothing wrong with it"
}
$cases += [pscustomobject][ordered]@{
    case                 = "healthy"
    probes               = $healthySample.listener.probes
    failed               = $healthySample.listener.failed
    consecutive_failures = $healthySample.listener.consecutive_failures
    last_rtt_ms          = $healthySample.listener.last_rtt_ms
    peer_rows            = $probeRows
    peers_seen           = $healthySample.peers.seen
    still_running        = (-not $script:Seeder.HasExited)
}

# ---------------------------------------------------------------------------
# poisoned: the same seeder, and the load that leaves the backlog
# ---------------------------------------------------------------------------

Write-Step "case survives_load ($Poison connections for a torrent the seeder does not have)"
$load = Invoke-Poison "poison"
# The seeder must NOT stop. Until 2026-08-22 this same load poisoned the
# listener and the case asserted exit 17: the accept loop drained one queued
# handshake check per connection it accepted, so twenty checks that resolved to
# an error cost the next twenty peers their handshake. That is fixed in the
# vendored tree, patches/UPSTREAM.md under "librqbit: one failed handshake
# check stops the accept loop draining", and the case is inverted rather than
# deleted: it is now what stops the defect coming back.
$stoppedEarly = $script:Seeder.WaitForExit(20000)
$loadExit = if ($stoppedEarly) { $script:Seeder.ExitCode } else { $null }
$final = $null
foreach ($line in (Get-Content $script:SeedOut -ErrorAction SilentlyContinue)) {
    try { $event = $line | ConvertFrom-Json } catch { continue }
    if ($event.stopped) { $final = $event }
}
$afterLoad = (Get-Progress) | Select-Object -Last 1
if ($stoppedEarly) {
    Add-Failure "survives_load" "the seeder stopped with $loadExit ($($final.stopped)) after $Poison connections that closed without handshaking; the accept loop is not draining"
}
elseif (-not $afterLoad.listener.healthy) {
    Add-Failure "survives_load" "the listener reported itself unhealthy after the load: $($afterLoad.listener | ConvertTo-Json -Compress)"
}
elseif ($afterLoad.listener.consecutive_failures -ne 0) {
    Add-Failure "survives_load" "$($afterLoad.listener.consecutive_failures) consecutive probe failures after the load, expected 0"
}
if ($load.swarm.peers_connected -lt $Poison) {
    Add-Failure "survives_load" "only $($load.swarm.peers_connected) of $Poison connections landed, so the case measured less than it says"
}
$cases += [pscustomobject][ordered]@{
    case                 = "survives_load"
    load_connected       = $load.swarm.peers_connected
    load_handshaked      = $load.swarm.peers_handshaked
    stopped_early        = $stoppedEarly
    exit_code            = $loadExit
    stopped              = $final.stopped
    probes               = $afterLoad.listener.probes
    failed               = $afterLoad.listener.failed
    consecutive_failures = $afterLoad.listener.consecutive_failures
    last_rtt_ms          = $afterLoad.listener.last_rtt_ms
    still_running        = (-not $script:Seeder.HasExited)
}
Stop-Background

# ---------------------------------------------------------------------------
# off: the load does not stop a run by itself, with no probe in the picture
# ---------------------------------------------------------------------------

Write-Step "case off (no --listener-check, same load)"
Start-Seeder @() "30s" | Out-Null
Invoke-Poison "poison_off" | Out-Null
$exitedOff = $script:Seeder.WaitForExit(90000)
$offExit = if ($exitedOff) { $script:Seeder.ExitCode } else { $null }
$offFinal = $null
foreach ($line in (Get-Content $script:SeedOut -ErrorAction SilentlyContinue)) {
    try { $event = $line | ConvertFrom-Json } catch { continue }
    if ($event.stopped) { $offFinal = $event }
}
if (-not $exitedOff) {
    Add-Failure "off" "the seeder never reached its own 30s deadline"
    Stop-Background
}
# 9 rather than 0: reaching `--stop-after` is `Stopped::Deadline`, which is a
# deadline that passed. What matters here is that it is not 17.
elseif ($offExit -ne 9) {
    Add-Failure "off" "exited $offExit, expected 9: without the flag the poison must not stop the run early"
}
elseif ($offFinal.stopped -ne "deadline") {
    Add-Failure "off" "stopped=$($offFinal.stopped), expected deadline"
}
# Absent rather than null, so a consumer selects on the key.
$offSample = (Get-Progress) | Select-Object -Last 1
if ($offSample -and $offSample.PSObject.Properties.Name -contains "listener" -and $null -ne $offSample.listener) {
    Add-Failure "off" "a run that did not ask for the check still reported a listener block"
}
$cases += [pscustomobject][ordered]@{
    case          = "off"
    exit_code     = $offExit
    stopped       = $offFinal.stopped
    listener_key  = ($null -ne $offSample.listener)
}
Stop-Background

# ---------------------------------------------------------------------------
# recovery: how much traffic it takes to clear the backlog
# ---------------------------------------------------------------------------
#
# This is the derivation for the threshold of three, so it is measured rather
# than asserted in prose. No `--listener-check` here: the probe would clear the
# backlog itself and this case is about what a real peer meets.
#
# `librqbit`'s accept loop used to drain its pending set through a `select!`
# arm whose pattern was `Some(Ok(..))`. A check that resolved to an error failed
# that pattern, which disabled the arm for the rest of that `select!` call, so
# the loop could not come round again until `accept` fired: one queued error
# cost one incoming connection, and this case measured $Poison of them costing
# $Poison. The vendored tree matches every outcome now, so the expected answer
# here is one.

Write-Step "case recovery (how many connections clear a $Poison connection backlog)"
Start-Seeder @() "300s" | Out-Null
$recoveryProbeDir = Join-Path $Root "work/recovery"
New-Item -ItemType Directory -Force -Path $recoveryProbeDir | Out-Null

function Invoke-Probe([int]$index) {
    $report = Join-Path $Root "recover$index.json"
    $work = Join-Path $recoveryProbeDir "$index"
    New-Item -ItemType Directory -Force -Path $work | Out-Null
    $process = Start-Process -FilePath $bitCli -ArgumentList @(
        "bench", "swarm", $script:Target, "--report", $report, "--format", "json",
        "--for", $torrent, "--peers", "1", "--disk-budget", "64MiB",
        "--duration", "5s", "--warmup", "200ms", "--dir", $work
    ) -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $Root "recover$index.out") `
        -RedirectStandardError (Join-Path $Root "recover$index.err")
    if (-not $process.WaitForExit(60000)) { try { $process.Kill() } catch { }; return -1 }
    if (-not (Test-Path $report)) { return -1 }
    (Get-Content $report -Raw | ConvertFrom-Json).swarm.peers_handshaked
}

$servedBefore = Invoke-Probe 0
if ($servedBefore -ne 1) {
    Add-Failure "recovery" "the seeder did not serve a peer before the load, so the case measured nothing"
}
Invoke-Poison "poison_recovery" | Out-Null
$ceiling = 3 * $Poison + 10
$recovered = -1
for ($k = 1; $k -le $ceiling; $k++) {
    if ($script:Seeder.HasExited) { break }
    if ((Invoke-Probe $k) -ge 1) { $recovered = $k; break }
}
# One is the pass. Before the accept loop was fixed this read the other way
# round: a backlog of $Poison took $Poison connections to clear, one each, and
# `$recovered -eq 1` meant the load had not landed and the case had measured
# nothing. Now the first peer after the load gets served, so anything above one
# is the drain rate regressing. The load landing is checked separately in
# survives_load, so a 1 here cannot be a load that never arrived.
if ($recovered -lt 0) {
    Add-Failure "recovery" "$ceiling connections did not clear a $Poison connection backlog"
}
elseif ($recovered -gt 1) {
    Add-Failure "recovery" "the first peer after the load was not served; it took $recovered connections, so the accept loop is draining one entry per accept again"
}
Write-Step "  $recovered connections cleared a $Poison connection backlog"
$cases += [pscustomobject][ordered]@{
    case                    = "recovery"
    served_before           = $servedBefore
    poison_connections      = $Poison
    connections_to_recover  = $recovered
    ceiling                 = $ceiling
}
Stop-Background

# ---------------------------------------------------------------------------
# The record
# ---------------------------------------------------------------------------

$verdict = if ($failures.Count -eq 0) { "pass" } else { "fail" }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "listener-$stamp.json"

[pscustomobject][ordered]@{
    kind           = "listener_check"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    }
    parameters     = [ordered]@{
        poison_connections = $Poison
        payload_mib        = $PayloadMiB
        interval           = $Interval
        profile            = $Profile
    }
    cases          = @($cases)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "The probe completes a real handshake for a torrent the seeder holds rather than an unknown one. An unknown info hash is cheaper, and it is the wrong measurement: it resolves to an error inside the session, which adds an entry to the same backlog it is measuring. A completed handshake takes one off instead.",
        "That costs one peer row per probe, which librqbit keeps in a terminal state and never reclaims. The reported peer list drops them by the port the probe dialled from, which is the mechanism the web seed bridge already uses.",
        "Three failures in a row is derived, not picked. The accept loop clears one queued check per connection it accepts, so one failure means a backlog a real peer would have cleared by arriving, and three means the backlog outlived three connections.",
        "survives_load and recovery asserted the opposite until 2026-08-22. The accept loop drained one queued handshake check per accepted connection, so twenty connections that closed before handshaking cost the next twenty peers their handshake. Fixed in the vendored tree; see patches/UPSTREAM.md and TODO/peers.md T-020.",
        "The off case keeps the probe out of the picture entirely, so the load is shown not to stop a run on its own.",
        "recovery runs without the flag on purpose. The probe clears one queued check per answered handshake, so a seeder being probed is a seeder being repaired, and this case is about what a peer that is not us meets."
    )
} | ConvertTo-Json -Depth 10 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
$cases | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
Write-Host "report:  $reportPath"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-listener: $failure") }
    exit 1
}
exit 0
