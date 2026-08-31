# Drive `bench swarm` against a live target, in both of its loads.
#
# This is the acceptance for `TODO/bench.md` T-092. `bench swarm` is the one
# subcommand that measures somebody else's process, and it has two loads
# because a target that is somebody else's process cannot be serving a torrent
# this run invented. The reasoning is under the entry; what this checks is that
# both loads behave as the entry says.
#
# Ten cases:
#
#   acceptance        The entry's own command, near enough to matter:
#                     `bench swarm <TARGET> --peers 100 --torrents 4
#                     --disk-budget 2GiB`. Completes, stays inside the budget,
#                     removes what it wrote, and none of the peers is served
#                     anything because the target does not have those torrents.
#   acceptance_cleanup  The same command with no --dir. The temp directory it
#                     owns does not survive the run.
#   leech_1           One peer against a torrent the target does serve. Every
#                     piece arrives and checks out against the torrent's own
#                     hashes.
#   leech_4           Four peers, same torrent. Each pulls the whole payload,
#                     and the pieces are held once between them rather than
#                     four times, which is what the budget counts.
#   leech_16          Sixteen. The three together are the serving curve.
#   budget            A budget smaller than the payload. Verified pieces past
#                     it are dropped and counted, the bytes on disk never
#                     cross it, and a piece that was dropped is not announced.
#   sources_ignored   A config file with every discovery mechanism on, and a
#                     second seeder for the same torrent announcing itself.
#                     The operating system's socket table says the run
#                     connected to the target and to nothing else.
#   listener_poisoned Leech, then the connect load, then leech again, against
#                     one seeder. The second leech must be served. The name is
#                     historical: this load did poison the listener until
#                     T-020 was fixed on 2026-08-22, and the case now holds
#                     the fix rather than recording the defect.
#   no_target         No target argument at all. Refused.
#   dead_target       A port nothing is listening on. Exit 6, and the report
#                     says every peer was refused rather than reporting a rate
#                     of zero as a measurement.
#
# Every case but the last two gets its own seeder. Two things made that
# necessary, and both were found by running this script rather than by reading
# it. The target's port is read from the target's own output, never chosen
# here, because a port this script picked could already be in use and dialling
# it would measure whatever else was listening. And the connect load used to
# leave the target unable to complete a handshake at all, so a case that reused
# the seeder before it measured that instead of itself. The second one is T-020,
# fixed on 2026-08-22, and `listener_poisoned` is where it is held on purpose.
#
# Usage:
#   pwsh scripts/check-swarm.ps1
#   pwsh scripts/check-swarm.ps1 -Peers 200 -PayloadMiB 32
#
# Exits 0 when every case behaves as described, 1 when one does not, and 2 when
# the check could not run. The record goes to bench/swarm-<timestamp>.json.
#
# See TODO/bench.md, T-092.

[CmdletBinding()]
param(
    [int]$Peers = 100,
    [int]$PayloadMiB = 8,
    [int[]]$LeechPeers = @(1, 4, 16),
    [string]$Root = ".tmp/swarm",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

$script:Seeder = $null
# The second seeder in the sources_ignored case. It outlives one Start-Seeder
# call, so it is killed separately.
$script:Decoy = $null

function Stop-Background {
    if ($script:Seeder -and -not $script:Seeder.HasExited) {
        try { $script:Seeder.Kill() } catch { }
    }
    $script:Seeder = $null
}

function Stop-Decoy {
    if ($script:Decoy -and -not $script:Decoy.HasExited) {
        try { $script:Decoy.Kill() } catch { }
    }
    $script:Decoy = $null
}

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-swarm: $message")
    Stop-Background
    Stop-Decoy
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

trap { Stop-Background; Stop-Decoy; throw }

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
[int64]$state = 11
for ($i = 0; $i -lt $payloadBytes.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $payloadBytes[$i] = [byte](($state -shr 16) -band 0xFF)
}
[System.IO.File]::WriteAllBytes((Join-Path $serve "payload.bin"), $payloadBytes)
$payloadLength = $payloadBytes.Length

$torrent = Join-Path $Root "payload.torrent"
# Through Start-Process with redirect files, like every other check script.
# Calling a native command directly under $ErrorActionPreference = 'Stop' makes
# whether a line on stderr ends the run depend on the host's pwsh version:
# $PSNativeCommandUseErrorActionPreference defaulted to $true in 7.2 and 7.3
# and to $false from 7.4. See TODO/windows.md under T-075.
$createProc = Start-Process -FilePath $bitCli -ArgumentList @(
    "create", (Join-Path $serve "payload.bin"), "--piece-length", "256KiB",
    "--no-creation-date", "--output", $torrent, "--force", "--json"
) -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $Root "create.out") `
    -RedirectStandardError (Join-Path $Root "create.err")
$createProc.WaitForExit(60000) | Out-Null
if ($createProc.ExitCode -ne 0) {
    Exit-With 2 "bit-cli create exited $($createProc.ExitCode): $(Get-Content (Join-Path $Root 'create.err') -Raw)"
}
$pieceCount = [math]::Ceiling($payloadLength / (256 * 1024))

# One seeder per case, not one for the whole script.
#
# The first version of this script started a seeder once and ran every case
# against it, with the connect load first. Every leech case after that
# reported zero peers handshaked, which read as a defect in `bench swarm` and
# is not one: the connect load leaves the target unable to complete a
# handshake for any info hash, including the one it is serving. That is T-020
# and it now has a case of its own, `listener_poisoned`. It is fixed, and the
# per-case seeder stays: a case that shares a seeder with the one before it can
# still measure the previous case's leftovers, and the isolation costs little.
$script:SeederIndex = 0

function Start-Seeder([string[]]$extra = @()) {
    Stop-Background
    $script:SeederIndex++
    $tag = "seed-$($script:SeederIndex)"
    $out = Join-Path $Root "$tag.out"
    $err = Join-Path $Root "$tag.err"
    # Port zero, and the port comes back out of the seeder's own event stream.
    # A port this script picked could already be in use, and dialling it would
    # measure whatever else was listening.
    $script:Seeder = Start-Process -FilePath $bitCli -ArgumentList (@(
        "--jsonl", "seed", $torrent, "--dir", $serve, "--port", "0",
        "--no-tracker", "--no-dht", "--no-lsd", "--stop-after", "600s"
    ) + $extra) -PassThru -NoNewWindow -RedirectStandardOutput $out -RedirectStandardError $err

    $addr = $null
    for ($attempt = 0; $attempt -lt 150; $attempt++) {
        Start-Sleep -Milliseconds 100
        if ($script:Seeder.HasExited) {
            Exit-With 2 "the seeder exited $($script:Seeder.ExitCode): $(Get-Content $err -Raw)"
        }
        foreach ($line in (Get-Content $out -ErrorAction SilentlyContinue)) {
            try { $event = $line | ConvertFrom-Json } catch { continue }
            if ($event.listen_addr) { $addr = $event.listen_addr }
        }
        if ($addr) { break }
    }
    if (-not $addr) { Exit-With 2 "the seeder never printed a listen address. stderr: $(Get-Content $err -Raw)" }
    $script:Target = "127.0.0.1:$(($addr -split ':')[-1])"
    Write-Step "seeder $($script:SeederIndex) serving $pieceCount pieces on $($script:Target)"
    $script:Target
}

$target = Start-Seeder

# ---------------------------------------------------------------------------
# Running one case
# ---------------------------------------------------------------------------

$cases = @()
$failures = @()

function Add-Failure([string]$name, [string]$message) {
    $script:failures += "${name}: $message"
}

function Invoke-Swarm([string]$name, [string[]]$extraArgs) {
    $reportPath = Join-Path $Root "$name.json"
    $stdout = Join-Path $Root "$name.out"
    $stderr = Join-Path $Root "$name.err"
    $arguments = @("bench", "swarm", $script:Target, "--report", $reportPath, "--format", "json") + $extraArgs
    $process = Start-Process -FilePath $bitCli -ArgumentList $arguments `
        -PassThru -NoNewWindow -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $exited = $process.WaitForExit(180000)
    if (-not $exited) {
        try { $process.Kill() } catch { }
        return [pscustomobject]@{ exit_code = -1; report = $null; stderr = "timed out" }
    }
    $parsed = $null
    if (Test-Path $reportPath) {
        try { $parsed = Get-Content $reportPath -Raw | ConvertFrom-Json } catch { }
    }
    [pscustomobject]@{
        exit_code = $process.ExitCode
        report    = $parsed
        stderr    = if (Test-Path $stderr) { (Get-Content $stderr -Raw) } else { "" }
    }
}

# Everything a run writes lives under its own directory, so "cleans up" is a
# question about a path rather than about the whole scratch tree.
function New-CaseDir([string]$name) {
    $path = Join-Path $Root "work/$name"
    New-Item -ItemType Directory -Force -Path $path | Out-Null
    $path
}

function Measure-Tree([string]$path) {
    if (-not (Test-Path $path)) { return 0 }
    $sum = 0
    foreach ($file in (Get-ChildItem -Recurse -File -Path $path -ErrorAction SilentlyContinue)) {
        $sum += $file.Length
    }
    $sum
}

# ---------------------------------------------------------------------------
# acceptance: the entry's own command
# ---------------------------------------------------------------------------

Write-Step "case acceptance ($Peers peers, 4 generated torrents, 2GiB budget)"
$target = Start-Seeder
$acceptDir = New-CaseDir "acceptance"
$run = Invoke-Swarm "acceptance" @(
    "--peers", "$Peers", "--torrents", "4", "--disk-budget", "2GiB",
    "--duration", "20s", "--warmup", "1s", "--connect-timeout", "5s", "--dir", $acceptDir
)
$acceptBytes = Measure-Tree $acceptDir
if ($run.exit_code -ne 0) { Add-Failure "acceptance" "exited $($run.exit_code), expected 0. stderr: $($run.stderr)" }
if (-not $run.report) { Add-Failure "acceptance" "no report was written" }
else {
    $swarm = $run.report.swarm
    if (-not $swarm) { Add-Failure "acceptance" "the report carries no swarm block" }
    else {
        if ($swarm.mode -ne "connect") { Add-Failure "acceptance" "mode=$($swarm.mode), expected connect with no --for" }
        if ($swarm.peers_dialled -ne $Peers) { Add-Failure "acceptance" "dialled $($swarm.peers_dialled) of $Peers peers" }
        if ($swarm.dialled -ne $target) { Add-Failure "acceptance" "dialled $($swarm.dialled), expected only $target" }
        # The target does not have any of the generated torrents, so nothing
        # can be served. A byte here would mean the info hashes collided with
        # something real.
        if ($swarm.bytes_received.bytes -ne 0) { Add-Failure "acceptance" "$($swarm.bytes_received.bytes) bytes arrived for torrents the target does not have" }
        if ($swarm.bytes_held.bytes -ne 0) { Add-Failure "acceptance" "$($swarm.bytes_held.bytes) bytes were held in connect mode, which fetches nothing" }
    }
    if ($run.report.parameters.disk_budget.bytes -ne 2GB) { Add-Failure "acceptance" "the budget was recorded as $($run.report.parameters.disk_budget.bytes)" }
}
if ($acceptBytes -gt 2GB) { Add-Failure "acceptance" "$acceptBytes bytes on disk, over the 2 GiB budget" }
# The generated torrents are the only thing this mode writes, and they are
# written into the directory the caller named, which is not removed.
if ($acceptBytes -eq 0) { Add-Failure "acceptance" "nothing was written, so the generated torrents are not reproducible" }
$cases += [pscustomobject][ordered]@{
    case            = "acceptance"
    exit_code       = $run.exit_code
    peers_dialled   = $run.report.swarm.peers_dialled
    peers_connected = $run.report.swarm.peers_connected
    peers_handshaked = $run.report.swarm.peers_handshaked
    bytes_on_disk   = $acceptBytes
    failures        = $run.report.swarm.failures
    fast_negotiated = $run.report.swarm.fast_extension.peers_negotiated
    have_all        = $run.report.swarm.fast_extension.have_all
}

# The default directory is removed. Same command, no --dir, and the temp
# directory it would have used must not survive.
Write-Step "case acceptance_cleanup (no --dir, so the run owns its directory)"
$target = Start-Seeder
$before = @(Get-ChildItem -Path ([System.IO.Path]::GetTempPath()) -Filter "bit-cli-bench-swarm-*" -Directory -ErrorAction SilentlyContinue).Count
$run = Invoke-Swarm "cleanup" @(
    "--peers", "4", "--torrents", "1", "--disk-budget", "16MiB",
    "--duration", "3s", "--warmup", "500ms", "--connect-timeout", "3s"
)
$after = @(Get-ChildItem -Path ([System.IO.Path]::GetTempPath()) -Filter "bit-cli-bench-swarm-*" -Directory -ErrorAction SilentlyContinue).Count
if ($run.exit_code -ne 0) { Add-Failure "acceptance_cleanup" "exited $($run.exit_code). stderr: $($run.stderr)" }
if ($after -gt $before) { Add-Failure "acceptance_cleanup" "$($after - $before) scratch directories were left behind" }
$cases += [pscustomobject][ordered]@{ case = "acceptance_cleanup"; exit_code = $run.exit_code; leftover_dirs = $after - $before }

# ---------------------------------------------------------------------------
# leech: the load that answers the entry's Relevance line
# ---------------------------------------------------------------------------

$curve = @()
foreach ($n in $LeechPeers) {
    $name = "leech_$n"
    Write-Step "case $name"
    $target = Start-Seeder
    $dir = New-CaseDir $name
    $run = Invoke-Swarm $name @(
        "--for", $torrent, "--peers", "$n", "--disk-budget", "512MiB",
        "--duration", "60s", "--warmup", "500ms", "--dir", $dir
    )
    $onDisk = Measure-Tree $dir
    if ($run.exit_code -ne 0) { Add-Failure $name "exited $($run.exit_code), expected 0. stderr: $($run.stderr)" }
    $swarm = if ($run.report) { $run.report.swarm } else { $null }
    if (-not $swarm) { Add-Failure $name "no swarm block" }
    else {
        if ($swarm.mode -ne "leech") { Add-Failure $name "mode=$($swarm.mode), expected leech" }
        if ($swarm.peers_handshaked -ne $n) { Add-Failure $name "$($swarm.peers_handshaked) of $n peers handshaked" }
        if ($swarm.peers_unchoked -ne $n) { Add-Failure $name "$($swarm.peers_unchoked) of $n peers were unchoked" }
        # Every peer is an independent leecher, so the whole payload arrives
        # once per peer.
        $want = $payloadLength * $n
        if ($swarm.bytes_received.bytes -ne $want) { Add-Failure $name "received $($swarm.bytes_received.bytes) bytes, expected $want" }
        if ($swarm.pieces_verified -ne ($pieceCount * $n)) { Add-Failure $name "verified $($swarm.pieces_verified) pieces, expected $($pieceCount * $n)" }
        if ($swarm.pieces_failed -ne 0) { Add-Failure $name "$($swarm.pieces_failed) pieces did not match the torrent's own hash" }
        # A piece is held once however many peers fetched it, so the payload
        # is on disk once and not n times.
        if ($swarm.bytes_held.bytes -ne $payloadLength) { Add-Failure $name "held $($swarm.bytes_held.bytes) bytes, expected the payload once at $payloadLength" }
        if ($onDisk -gt $payloadLength) { Add-Failure $name "$onDisk bytes on disk, more than the payload" }
        if ($run.report.summary.sustained_rate.bytes -le 0) { Add-Failure $name "sustained rate is $($run.report.summary.sustained_rate.bytes), and $($swarm.bytes_received.bytes) bytes arrived" }
        # The serving half. At a 512 MiB budget against an 8 MiB payload every
        # verified piece is kept, and a peer announces every piece it can
        # serve, so the two counts are the same number.
        if ($swarm.serving.pieces_announced -ne ($pieceCount * $n)) { Add-Failure $name "announced $($swarm.serving.pieces_announced) pieces, expected $($pieceCount * $n), one per piece each peer verified and kept" }
        if ($swarm.serving.requests_refused -ne 0) { Add-Failure $name "refused $($swarm.serving.requests_refused) requests for pieces it announced" }
        if ($swarm.serving.bytes_sent.bytes -ne 0 -and $swarm.serving.blocks_sent -eq 0) { Add-Failure $name "$($swarm.serving.bytes_sent.bytes) bytes sent in zero blocks" }
    }
    $curve += [pscustomobject][ordered]@{
        peers          = $n
        sustained      = $run.report.summary.sustained_rate.bytes
        sustained_human = $run.report.summary.sustained_rate.human
        received       = $swarm.bytes_received.bytes
        held           = $swarm.bytes_held.bytes
    }
    $cases += [pscustomobject][ordered]@{
        case      = $name
        exit_code = $run.exit_code
        peers     = $n
        sustained = $run.report.summary.sustained_rate.human
        received  = $swarm.bytes_received.bytes
        verified  = $swarm.pieces_verified
        held      = $swarm.bytes_held.bytes
        # What the peers gave back. `peers_asked` is the one to read: a
        # target that already holds the whole payload has nothing to ask a
        # synthetic peer for, because a synthetic peer can only hold what
        # that same target served it. Recorded rather than judged, because it
        # is a fact about the target.
        announced = $swarm.serving.pieces_announced
        peers_asked = $swarm.serving.peers_asked
        target_interested = $swarm.serving.peers_target_interested
        blocks_sent = $swarm.serving.blocks_sent
        bytes_sent = $swarm.serving.bytes_sent.bytes
        # Every synthetic peer offers the BEP 6 bit, so this is the target's
        # answer rather than the peer's offer. It was zero on every run until
        # 2026-08-23, which was `librqbit` saying it had no BEP 6 at all;
        # the vendored tree has it now and this is what says so from the wire.
        # Recorded rather than judged: what this script measures is the load
        # generator, and the entry that owns the number is
        # TODO/bep-coverage.md T-100.
        fast_negotiated = $swarm.fast_extension.peers_negotiated
        # And what the target sent in place of a bitfield. A complete seeder
        # that negotiated the extension says `have all` in two bytes, so this
        # is the half of the negotiation the target acted on rather than the
        # half it merely agreed to.
        have_all        = $swarm.fast_extension.have_all
    }
}

# ---------------------------------------------------------------------------
# budget: a cap smaller than the payload
# ---------------------------------------------------------------------------

Write-Step "case budget (a cap under the payload)"
$target = Start-Seeder
$budgetDir = New-CaseDir "budget"
$budget = [int]($payloadLength / 4)
$run = Invoke-Swarm "budget" @(
    "--for", $torrent, "--peers", "2", "--disk-budget", "$budget",
    "--duration", "60s", "--warmup", "500ms", "--dir", $budgetDir
)
$onDisk = Measure-Tree $budgetDir
$swarm = if ($run.report) { $run.report.swarm } else { $null }
if ($run.exit_code -ne 0) { Add-Failure "budget" "exited $($run.exit_code). stderr: $($run.stderr)" }
if (-not $swarm) { Add-Failure "budget" "no swarm block" }
else {
    if ($swarm.bytes_held.bytes -gt $budget) { Add-Failure "budget" "held $($swarm.bytes_held.bytes) bytes against a $budget byte budget" }
    if ($swarm.pieces_dropped_over_budget -le 0) { Add-Failure "budget" "nothing was dropped, and the payload is four times the budget" }
    # The download still completes: the budget bounds what is kept, not what
    # is fetched and checked.
    if ($swarm.pieces_verified -ne ($pieceCount * 2)) { Add-Failure "budget" "verified $($swarm.pieces_verified) pieces, expected $($pieceCount * 2): the budget must not stop the transfer" }
    # A piece the budget refused is a piece neither peer can serve, so neither
    # announces it. Both peers fetch the whole payload, so both reach every
    # piece that is on disk and each announces exactly those: twice the pieces
    # kept, and nothing for the three quarters that were dropped.
    $keptPieces = [math]::Floor($swarm.bytes_held.bytes / (256 * 1024))
    if ($swarm.serving.pieces_announced -ne ($keptPieces * 2)) {
        Add-Failure "budget" "announced $($swarm.serving.pieces_announced) pieces against $keptPieces on disk, expected $($keptPieces * 2): a piece the budget refused must not be announced"
    }
}
if ($onDisk -gt $budget) { Add-Failure "budget" "$onDisk bytes on disk against a $budget byte budget" }
$cases += [pscustomobject][ordered]@{
    case          = "budget"
    exit_code     = $run.exit_code
    budget_bytes  = $budget
    held_bytes    = $swarm.bytes_held.bytes
    on_disk_bytes = $onDisk
    dropped       = $swarm.pieces_dropped_over_budget
    announced     = $swarm.serving.pieces_announced
}

# ---------------------------------------------------------------------------
# sources_ignored: nothing but the target is ever contacted
# ---------------------------------------------------------------------------
#
# The entry's target model says `bench swarm` dials the target and nothing
# else, ever. Until now that was checked by reading the source and by trusting
# `swarm.dialled`, which is the tool's own claim about itself. This checks it
# from outside, against the operating system's socket table.
#
# Two things are arranged so there is something to find. A config file turns on
# every discovery mechanism `bit-cli` has a setting for, and a second seeder
# serves the same torrent on its own port with local service discovery left on,
# so it announces itself on this machine. If anything in the run were looking
# for peers, that is a peer to find.
#
# The entry asked for "a configuration file naming a different peer". There is
# no such setting: `ConfigFile` in crates/bit-cli-core/src/config.rs has no key
# that carries a peer address, so a config file cannot name one. Turning on the
# three mechanisms that discover peers is the same question asked in the form
# the configuration surface actually has.

Write-Step "case sources_ignored (discovery on in a config file, a second seeder to find)"
# Throttled, and that is what makes this case measurable at all. It reads the
# operating system's socket table while the run is connected, and on loopback
# an 8 MiB payload is served in about 90 ms, which is shorter than one
# Get-NetTCPConnection call. Capping the target's upload stretches the transfer
# past the run's own --duration, so the window is the duration rather than
# however fast this machine happens to be.
#
# It used to work by accident. The load was the connect load, whose peers held
# their connections "for the whole duration" only because the target could not
# answer them: that was T-020, and fixing it dropped this case from six samples
# to one and from 42 sightings to zero. A case that needs the target broken is
# not a case.
$target = Start-Seeder @("--max-overall-upload-rate", "512KiB")

$decoyOut = Join-Path $Root "decoy.out"
$decoyErr = Join-Path $Root "decoy.err"
# Local service discovery is deliberately left on for this one, so it announces
# itself. Every other seeder in this script has it off, which is what keeps the
# two from finding each other and giving this case a connection it cannot
# attribute.
$script:Decoy = Start-Process -FilePath $bitCli -ArgumentList @(
    "--jsonl", "seed", $torrent, "--dir", $serve, "--port", "0",
    "--no-tracker", "--no-dht", "--stop-after", "120s"
) -PassThru -NoNewWindow -RedirectStandardOutput $decoyOut -RedirectStandardError $decoyErr
$decoyAddr = $null
for ($attempt = 0; $attempt -lt 150; $attempt++) {
    Start-Sleep -Milliseconds 100
    if ($script:Decoy.HasExited) {
        Exit-With 2 "the decoy seeder exited $($script:Decoy.ExitCode): $(Get-Content $decoyErr -Raw)"
    }
    foreach ($line in (Get-Content $decoyOut -ErrorAction SilentlyContinue)) {
        try { $event = $line | ConvertFrom-Json } catch { continue }
        if ($event.listen_addr) { $decoyAddr = $event.listen_addr }
    }
    if ($decoyAddr) { break }
}
if (-not $decoyAddr) { Exit-With 2 "the decoy seeder never printed a listen address" }
$decoyPort = ($decoyAddr -split ':')[-1]
Write-Step "  decoy seeder serving the same torrent on 127.0.0.1:$decoyPort"

$cfg = Join-Path $Root "discovery.toml"
Set-Content -Path $cfg -Encoding utf8NoBOM -Value @(
    "enable_dht = true",
    "enable_pex = true",
    "enable_lsd = true",
    "max_peers = 200",
    "max_peers_total = 400"
)

# The config file has to be real and has to be read, or this case proves
# nothing at all: a mistyped path would leave the run with no config and pass.
$cfgShowOut = Join-Path $Root "config-show.json"
$cfgProc = Start-Process -FilePath $bitCli -ArgumentList @(
    "--config", $cfg, "config", "show", "--json"
) -PassThru -NoNewWindow -RedirectStandardOutput $cfgShowOut -RedirectStandardError (Join-Path $Root "config-show.err")
$cfgProc.WaitForExit(30000) | Out-Null
$cfgShown = $null
if (Test-Path $cfgShowOut) { try { $cfgShown = Get-Content $cfgShowOut -Raw | ConvertFrom-Json } catch { } }
$cfgRead = $false
if ($cfgShown) {
    $cfgRead = @($cfgShown.files_read) -contains $cfg
}
if (-not $cfgRead) { Add-Failure "sources_ignored" "the config file at $cfg was not read, so the case measured nothing" }

# The leech load against the throttled target above, so the peers stay
# connected for the run's whole duration and there is something to sample.
$sourcesReport = Join-Path $Root "sources_ignored.json"
$sourcesDir = New-CaseDir "sources_ignored"
$swarmProc = Start-Process -FilePath $bitCli -ArgumentList @(
    "--config", $cfg, "bench", "swarm", $target, "--for", $torrent, "--peers", "8",
    "--disk-budget", "64MiB", "--duration", "6s", "--warmup", "500ms",
    "--connect-timeout", "3s", "--dir", $sourcesDir,
    "--report", $sourcesReport, "--format", "json"
) -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $Root "sources_ignored.out") `
    -RedirectStandardError (Join-Path $Root "sources_ignored.err")

$sockets = $IsWindows -or $env:OS -eq "Windows_NT"
$remotes = @{}
$samples = 0
$targetSeen = 0
$udpSeen = 0
$listeners = 0
$decoyPeak = 0
if ($sockets) {
    while (-not $swarmProc.HasExited) {
        $samples++
        foreach ($conn in (Get-NetTCPConnection -OwningProcess $swarmProc.Id -ErrorAction SilentlyContinue)) {
            if ($conn.State -eq "Listen") { $listeners++; continue }
            if (-not $conn.RemotePort -or $conn.RemotePort -eq 0) { continue }
            $remote = "$($conn.RemoteAddress):$($conn.RemotePort)"
            $remotes[$remote] = $true
            if ($remote -eq $target) { $targetSeen++ }
        }
        $udpSeen += @(Get-NetUDPEndpoint -OwningProcess $swarmProc.Id -ErrorAction SilentlyContinue).Count
        # Named so it cannot collide with the $Peers parameter. PowerShell
        # variable names are case-insensitive, so a local called $peers here is
        # the script's own $Peers, and every later case built its argument list
        # from whatever this loop last measured. See RULES.md section 5.
        $decoyLive = @(Get-NetTCPConnection -OwningProcess $script:Decoy.Id -State Established -ErrorAction SilentlyContinue).Count
        if ($decoyLive -gt $decoyPeak) { $decoyPeak = $decoyLive }
    }
}
$swarmProc.WaitForExit(60000) | Out-Null
Stop-Decoy

$sourcesSwarm = $null
if (Test-Path $sourcesReport) { try { $sourcesSwarm = (Get-Content $sourcesReport -Raw | ConvertFrom-Json).swarm } catch { } }
if ($swarmProc.ExitCode -ne 0) { Add-Failure "sources_ignored" "exited $($swarmProc.ExitCode), expected 0" }
if (-not $sourcesSwarm) { Add-Failure "sources_ignored" "no swarm block" }
elseif ($sourcesSwarm.dialled -ne $target) { Add-Failure "sources_ignored" "the report says it dialled $($sourcesSwarm.dialled), expected $target" }

if ($sockets) {
    # The premise first. A run whose sockets were never seen proves nothing,
    # and passing on an empty sample is how a check like this rots.
    if ($targetSeen -eq 0) {
        Add-Failure "sources_ignored" "the socket table was sampled $samples times and never showed a connection to the target, so nothing was measured"
    }
    $strangers = @($remotes.Keys | Where-Object { $_ -ne $target })
    if ($strangers.Count -gt 0) {
        Add-Failure "sources_ignored" "connected to $($strangers -join ', '), and the only address it was given is $target"
    }
    if ($udpSeen -gt 0) { Add-Failure "sources_ignored" "bound $udpSeen UDP endpoints, and a run that speaks no DHT needs none" }
    if ($listeners -gt 0) { Add-Failure "sources_ignored" "bound a listening socket, and nothing is meant to connect to a load generator" }
    if ($decoyPeak -gt 0) { Add-Failure "sources_ignored" "the second seeder saw $decoyPeak peers, and nothing was told about it" }
}
$cases += [pscustomobject][ordered]@{
    case            = "sources_ignored"
    exit_code       = $swarmProc.ExitCode
    judged          = $sockets
    config_read     = $cfgRead
    decoy_port      = [int]$decoyPort
    dialled         = $sourcesSwarm.dialled
    socket_samples  = $samples
    target_sightings = $targetSeen
    remote_endpoints = @($remotes.Keys)
    udp_endpoints   = $udpSeen
    listening_sockets = $listeners
    decoy_peak_peers = $decoyPeak
}

# ---------------------------------------------------------------------------
# listener_poisoned: what the connect load does to the target
# ---------------------------------------------------------------------------
#
# This is T-020 measured through `bench swarm` instead of through
# `loopback-churn`, and it is why every other case here gets its own seeder.
#
# Three runs against one seeder: leech, then the connect load, then the same
# leech again. The first proves the target serves. The second is the
# acceptance's own command. The third is the question: can the target still
# complete a handshake for a torrent it holds?
#
# It records rather than judges. T-020 is open and this is the defect it
# names, so failing the build here would fail it for a defect that is already
# recorded, which is the `check-close-wait.ps1` rule.

Write-Step "case listener_poisoned (leech, connect load, leech again, one seeder)"
$target = Start-Seeder
$poisonBefore = Invoke-Swarm "poison_before" @(
    "--for", $torrent, "--peers", "1", "--disk-budget", "512MiB",
    "--duration", "30s", "--warmup", "500ms", "--dir", (New-CaseDir "poison_before")
)
$poisonLoad = Invoke-Swarm "poison_load" @(
    "--peers", "$Peers", "--torrents", "4", "--disk-budget", "2GiB",
    "--duration", "20s", "--warmup", "1s", "--connect-timeout", "5s",
    "--dir", (New-CaseDir "poison_load")
)
$poisonAfter = Invoke-Swarm "poison_after" @(
    "--for", $torrent, "--peers", "1", "--disk-budget", "512MiB",
    "--duration", "30s", "--warmup", "500ms", "--dir", (New-CaseDir "poison_after")
)
$beforeSwarm = if ($poisonBefore.report) { $poisonBefore.report.swarm } else { $null }
$loadSwarm = if ($poisonLoad.report) { $poisonLoad.report.swarm } else { $null }
$afterSwarm = if ($poisonAfter.report) { $poisonAfter.report.swarm } else { $null }

# The one thing this case does fail on is its own premise. If the seeder never
# served the first leech, the run measured nothing and says so.
if (-not $beforeSwarm -or $beforeSwarm.peers_handshaked -ne 1) {
    Add-Failure "listener_poisoned" "the target did not serve the first leech, so the case measured nothing"
}
# And that every one of the three runs happened at all. A run that fails
# before it starts writes no report, and this case reads only the reports: it
# recorded three nulls and passed, for a run that exited on its arguments and
# never opened a socket. What is judged is that each run produced a report,
# not what the report says, because what it says is T-020 and T-020 is open.
foreach ($stage in @(
    @{ name = "poison_before"; run = $poisonBefore },
    @{ name = "poison_load"; run = $poisonLoad },
    @{ name = "poison_after"; run = $poisonAfter }
)) {
    if (-not $stage.run.report) {
        Add-Failure "listener_poisoned" "$($stage.name) wrote no report and exited $($stage.run.exit_code): $($stage.run.stderr)"
    }
}
# The load has to have been a load. Zero connections is not a poisoned
# listener, it is a run that did not happen.
if ($loadSwarm -and $loadSwarm.peers_connected -lt 1) {
    Add-Failure "listener_poisoned" "the connect load reached $($loadSwarm.peers_connected) peers, so it poisoned nothing"
}
$poisoned = $null
if ($beforeSwarm -and $afterSwarm) {
    $poisoned = ($beforeSwarm.peers_handshaked -eq 1 -and $afterSwarm.peers_handshaked -eq 0)
    if ($poisoned) {
        Write-Step "  the target still accepts TCP and no longer handshakes"
    }
}
# Judged since 2026-08-22, where it carried judged: false for as long as T-020
# was open. The accept loop drained one queued handshake check per connection
# it accepted, so this load left the target accepting TCP and answering no
# handshake for any info hash, including the one it was serving. It is fixed in
# the vendored tree, so the same load must now leave the target still serving.
# The case keeps its name: TODO/bench.md and TODO/peers.md cite runs recorded
# under it, and those runs happened.
if ($null -ne $poisoned -and $poisoned) {
    Add-Failure "listener_poisoned" "the target served a peer before the load and none after it, so the accept loop is draining one entry per accept again. That is T-020."
}
$cases += [pscustomobject][ordered]@{
    case                = "listener_poisoned"
    judged              = $true
    todo                = "T-020"
    before_exit         = $poisonBefore.exit_code
    load_exit           = $poisonLoad.exit_code
    after_exit          = $poisonAfter.exit_code
    before_handshaked   = $beforeSwarm.peers_handshaked
    before_bytes        = $beforeSwarm.bytes_received.bytes
    load_connected      = $loadSwarm.peers_connected
    load_handshaked     = $loadSwarm.peers_handshaked
    load_failures       = $loadSwarm.failures
    after_connected     = $afterSwarm.peers_connected
    after_handshaked    = $afterSwarm.peers_handshaked
    after_bytes         = $afterSwarm.bytes_received.bytes
    seeder_still_alive  = (-not $script:Seeder.HasExited)
    reproduced          = $poisoned
}

# ---------------------------------------------------------------------------
# no_target and dead_target
# ---------------------------------------------------------------------------

Write-Step "case no_target"
$noTargetOut = Join-Path $Root "no-target.out"
$noTargetErr = Join-Path $Root "no-target.err"
$process = Start-Process -FilePath $bitCli -ArgumentList @("bench", "swarm", "--peers", "4") `
    -PassThru -NoNewWindow -RedirectStandardOutput $noTargetOut -RedirectStandardError $noTargetErr
$process.WaitForExit(30000) | Out-Null
if ($process.ExitCode -eq 0) { Add-Failure "no_target" "exited 0 with no target argument" }
$cases += [pscustomobject][ordered]@{ case = "no_target"; exit_code = $process.ExitCode }

Write-Step "case dead_target"
# A port with nothing on it. Bound and released here so the OS is unlikely to
# have handed it to anything else in between.
$probe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$probe.Start()
$deadPort = $probe.LocalEndpoint.Port
$probe.Stop()
$deadOut = Join-Path $Root "dead.json"
$deadErrFile = Join-Path $Root "dead.err"
$process = Start-Process -FilePath $bitCli -ArgumentList @(
    "bench", "swarm", "127.0.0.1:$deadPort", "--peers", "4", "--torrents", "1",
    "--duration", "5s", "--warmup", "500ms", "--connect-timeout", "3s",
    "--report", $deadOut, "--format", "json"
) -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $Root "dead.out") -RedirectStandardError $deadErrFile
$process.WaitForExit(60000) | Out-Null
$dead = $null
if (Test-Path $deadOut) { try { $dead = Get-Content $deadOut -Raw | ConvertFrom-Json } catch { } }
if ($process.ExitCode -ne 6) { Add-Failure "dead_target" "exited $($process.ExitCode), expected 6 for a target nothing answers" }
if ($dead -and $dead.swarm) {
    if ($dead.swarm.peers_connected -ne 0) { Add-Failure "dead_target" "$($dead.swarm.peers_connected) peers connected to a dead port" }
    if (@($dead.swarm.failures).Count -eq 0) { Add-Failure "dead_target" "no failure class was reported" }
}
else { Add-Failure "dead_target" "no swarm block in the report" }
$cases += [pscustomobject][ordered]@{
    case      = "dead_target"
    exit_code = $process.ExitCode
    failures  = $dead.swarm.failures
}

# ---------------------------------------------------------------------------
# The record
# ---------------------------------------------------------------------------

Stop-Background

$verdict = if ($failures.Count -eq 0) { "pass" } else { "fail" }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "swarm-$stamp.json"

[pscustomobject][ordered]@{
    kind           = "swarm_check"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    }
    parameters     = [ordered]@{
        peers          = $Peers
        leech_peers    = @($LeechPeers)
        payload_mib    = $PayloadMiB
        payload_bytes  = $payloadLength
        piece_count    = $pieceCount
        target         = $target
        profile        = $Profile
    }
    serving_curve  = @($curve)
    cases          = @($cases)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "The target's port comes out of the target's own event stream. A port this script picked could already be in use, and dialling it would measure whatever else was listening; that happened while this was written and looked exactly like a defect in bench swarm.",
        "In connect mode the target does not have the generated torrents, so zero bytes served is the correct result and any byte would mean an info hash collided with something real.",
        "In leech mode every peer is an independent leecher, so the payload arrives once per peer, and it is held on disk once between them.",
        "Every case but no_target and dead_target starts its own seeder. The connect load used to leave the target unable to complete a handshake for any info hash, so a case sharing a seeder with the one before it measured the previous case. That was T-020, now fixed, and listener_poisoned is where it is held rather than tripped over.",
        "listener_poisoned is judged since 2026-08-22. It carried judged: false while T-020 was open; the accept loop is fixed in the vendored tree and the case now fails the build if the target stops serving after the load.",
        "fast_negotiated is the target's answer to a BEP 6 offer every synthetic peer makes, and have_all is what it then sent in place of a bitfield. Recorded rather than judged. Both were zero on every run until 2026-08-23, which was librqbit having no BEP 6 at all; the entry that owns the numbers is TODO/bep-coverage.md T-100.",
        "A synthetic peer announces every piece it verified and kept and answers requests for those pieces. peers_asked is zero against a seeder and that is the only result that load can produce: a synthetic peer holds only what the target served it, so it can never offer the target a piece the target is missing. What the serving side changes is what the target sees, and pieces_announced is the number that says it happened.",
        "sources_ignored reads the operating system's socket table rather than the report, because the report is the tool's own claim about itself. It is judged only on Windows, where Get-NetTCPConnection is; on any other platform it records judged: false."
    )
} | ConvertTo-Json -Depth 10 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "serving curve:"
foreach ($point in $curve) {
    Write-Host ("  {0,3} peers  {1}" -f $point.peers, $point.sustained_human)
}
Write-Host ""
Write-Host "report:  $reportPath"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-swarm: $failure") }
    exit 1
}
exit 0
