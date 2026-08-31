# Does a download survive its peers going away and coming back.
#
# `TODO/peers.md` T-021 is a report that disabling and re-enabling a network
# adapter mid-download drops the rate to zero and it never recovers. That is
# the failure that makes an unattended download useless: a cron job starts a
# 40 GB transfer and comes back to a stalled process at 60 percent.
#
# The adapter is not the variable and cannot be touched here anyway, because
# disabling one is a change to the machine. What the client sees is the same
# either way: every peer connection dies at once, nothing is reachable for a
# while, and then everything is reachable again. So the outage is the seeder
# being killed and restarted on the same port.
#
# Three scenarios, because they ask different questions:
#
#   patient    `--stop-timeout` longer than the outage, so the run is still
#              alive when the peer returns. Does the client re-dial on its own?
#              This is the one that says whether T-021 reproduces at all.
#   impatient  `--stop-timeout` shorter than the outage. Does the run give up
#              and say so, rather than sitting at 60 percent forever? This is
#              the one that says whether an unattended caller can retry.
#   redial     the patient run again with `--redial-after`, which throws the
#              peer state away instead of waiting the backoff out. This is
#              T-138's acceptance, and the pair with `patient` is what says the
#              flag moves a number.
#
# `impatient` has to exit 9 with "stopped": "stalled" inside -StopTimeout of
# the cut, and `redial` has to complete with the payload hashing equal.
#
# `patient` is held to completing only when the outage is short enough for
# librqbit's own backoff to catch it. That backoff is 10 seconds minimum with a
# factor of 6, so attempts land at about 10s, 70s, 430s. An outage that ends
# before the 70 second attempt is caught by it; one that ends after it waits
# for the 430 second attempt, which is exactly what T-021 measured and what
# T-138 exists to fix. Past that reach `patient` is recorded rather than
# judged, so this script does not fail the build for a defect that is open.
#
# The transfer is held open with `--max-download-rate`, measured and holding in
# TODO/performance.md under T-031, so the outage lands in the middle of it
# rather than after it.
#
# Usage:
#   pwsh scripts/check-peer-recovery.ps1
#   pwsh scripts/check-peer-recovery.ps1 -OutageSeconds 120 -StopTimeout 60 -PatientTimeout 300
#
# Exits 0 when every judged scenario behaved as described, 1 when one did not,
# and 2 when the check could not run. The record goes to
# bench/peer-recovery-<timestamp>.json.
#
# See TODO/peers.md, T-021 and T-138.

[CmdletBinding()]
param(
    [string]$PayloadSize = "128MiB",
    [string]$Rate = "2MiB/s",
    # When the outage starts, as seconds into the download.
    [int]$CutAfterSeconds = 8,
    # How long nothing is reachable.
    [int]$OutageSeconds = 40,
    # `--stop-timeout`: how long with no progress before the run gives up. It
    # has to be shorter than the outage, or the impatient scenario never
    # reaches its deadline.
    [int]$StopTimeout = 20,
    # `--stop-timeout` for the two scenarios that are meant to outlive the
    # outage. Zero derives it from the outage and -StopTimeout.
    [int]$PatientTimeout = 0,
    # `--redial-after` for the third scenario. It has to be shorter than the
    # patient timeout or the run gives up before it re-dials.
    [int]$RedialAfter = 30,
    [string]$Root = ".tmp/peer-recovery",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [int]$TimeoutSeconds = 900,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-peer-recovery: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

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

function Format-Size([double]$bytes) {
    $units = @("B", "KiB", "MiB", "GiB", "TiB")
    $index = 0
    while ($bytes -ge 1024 -and $index -lt $units.Count - 1) { $bytes /= 1024; $index++ }
    "{0:N2} {1}" -f $bytes, $units[$index]
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

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --workspace --bins --examples"
}

# The impatient scenario only tests anything when the run gives up before the
# peer comes back, and the patient one only when it is still there after.
if ($StopTimeout -ge $OutageSeconds) {
    Exit-With 2 "-StopTimeout $StopTimeout has to be shorter than -OutageSeconds $OutageSeconds, or the impatient scenario never reaches its deadline."
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path (Join-Path $Root "payload") | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

$payloadBytes = ConvertFrom-Size $PayloadSize
Write-Step "building a payload of $(Format-Size $payloadBytes)"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 271828
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}
$stream = [System.IO.File]::Create((Join-Path $Root "payload/movie.bin"))
try {
    [int64]$written = 0
    while ($written -lt $payloadBytes) {
        $take = [Math]::Min([int64]$block.Length, $payloadBytes - $written)
        $stream.Write($block, 0, [int]$take)
        $written += $take
    }
}
finally { $stream.Dispose() }
$expected = (Get-FileHash -Algorithm SHA256 (Join-Path $Root "payload/movie.bin")).Hash.ToLower()

$torrent = Join-Path $Root "movie.torrent"
& $bitCli create (Join-Path $Root "payload") --name payload --piece-length 1MiB `
    --no-creation-date --output $torrent --force --json 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }

# ---------------------------------------------------------------------------
# A port the seeder can come back on
# ---------------------------------------------------------------------------
#
# The whole point is that the peer returns at the address the client already
# knows, so the port has to survive the restart. The OS picks it once, on a
# socket that is then closed, which is the nearest thing to `--port 0` that a
# restart can reuse.

$probe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$probe.Start()
$port = $probe.LocalEndpoint.Port
$probe.Stop()
Write-Step "the seeder will use 127.0.0.1:$port across the outage"

function Start-Seed([string]$tag) {
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $Root -NoNewWindow -PassThru `
        -ArgumentList @(
            "seed", $torrent, "--dir", $Root, "--port", "$port",
            "--no-dht", "--no-lsd", "--seed-time", "60m", "--json"
        ) `
        -RedirectStandardOutput (Join-Path $Root "$tag.out") `
        -RedirectStandardError (Join-Path $Root "$tag.err")
    $script:Background += $process
    $deadline = (Get-Date).AddSeconds(60)
    while ((Get-Date) -lt $deadline) {
        if ($process.HasExited) { Exit-With 2 "the seeder exited; see $Root/$tag.err" }
        $listening = Get-NetTCPConnection -State Listen -OwningProcess $process.Id -ErrorAction SilentlyContinue |
            Where-Object { $_.LocalPort -eq $port }
        if ($listening) { return $process }
        Start-Sleep -Milliseconds 250
    }
    Exit-With 2 "the seeder never listened on $port"
}

function Stop-Seed($process) {
    if ($process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
    $script:Background = @($script:Background | Where-Object { $_.Id -ne $process.Id })
    # Windows does not free a listening port the instant the process dies, and
    # the restart needs the same one.
    Start-Sleep -Seconds 1
}

# ---------------------------------------------------------------------------
# One scenario: a download with an outage in the middle
# ---------------------------------------------------------------------------

$commands = [System.Collections.ArrayList]::new()

function Invoke-Scenario([string]$name, [int]$stopTimeout, [int]$redialAfter = 0) {
    $with = if ($redialAfter -gt 0) { ", --redial-after ${redialAfter}s" } else { "" }
    Write-Step "$name : --stop-timeout ${stopTimeout}s against a ${OutageSeconds}s outage$with"
    $seed = Start-Seed "$name-seed-before"

    $outDir = Join-Path $Root "out-$name"
    if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $stdout = Join-Path $Root "$name.json"
    $arguments = @(
        "download", $torrent, "--dir", $outDir,
        "--peer", "127.0.0.1:$port",
        "--no-dht", "--no-lsd", "--no-tracker", "--no-web-seed",
        "--port", "0",
        "--max-download-rate", $Rate,
        "--stop-timeout", "${stopTimeout}s",
        "--report-interval", "2s",
        "--json"
    )
    if ($redialAfter -gt 0) { $arguments += @("--redial-after", "${redialAfter}s") }
    [void]$commands.Add("bit-cli $($arguments -join ' ')")

    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $download = Start-Process -FilePath $bitCli -WorkingDirectory $repo -NoNewWindow -PassThru `
        -ArgumentList $arguments -RedirectStandardOutput $stdout `
        -RedirectStandardError (Join-Path $Root "$name.err")
    $script:Background += $download

    $timeline = [System.Collections.ArrayList]::new()
    function Note([string]$what) {
        [void]$timeline.Add([ordered]@{
            at    = Get-Timestamp
            t_ms  = $clock.ElapsedMilliseconds
            event = $what
        })
        Write-Step ("  t+{0,6:N1}s {1}" -f ($clock.ElapsedMilliseconds / 1000), $what)
    }
    Note "download started"

    Start-Sleep -Seconds $CutAfterSeconds
    if ($download.HasExited) {
        Stop-Background
        Exit-With 2 "the download finished before the outage; raise -PayloadSize or lower -Rate"
    }
    Note "cutting the seeder"
    Stop-Seed $seed
    $cutAtMs = $clock.ElapsedMilliseconds

    # Polled rather than slept through, because when the run gives up is the
    # number the acceptance is about.
    $exitedAtMs = $null
    $outageEnd = (Get-Date).AddSeconds($OutageSeconds)
    while ((Get-Date) -lt $outageEnd) {
        if ($download.HasExited) {
            $exitedAtMs = $clock.ElapsedMilliseconds
            Note "the download exited $($download.ExitCode) during the outage"
            break
        }
        Start-Sleep -Milliseconds 500
    }
    if (-not $exitedAtMs) {
        $remaining = ($outageEnd - (Get-Date)).TotalSeconds
        if ($remaining -gt 0) { Start-Sleep -Seconds ([Math]::Ceiling($remaining)) }
        Note "outage over, the download is still running"
    }

    $seed = Start-Seed "$name-seed-after"
    Note "the seeder is back on the same port"

    $finished = $download.WaitForExit($TimeoutSeconds * 1000)
    $clock.Stop()
    if (-not $finished) { Stop-Process -Id $download.Id -Force -ErrorAction SilentlyContinue }
    $endedWith = if ($finished) { "$($download.ExitCode)" } else { "124 (timed out)" }
    Note "the download ended, exit $endedWith"
    Stop-Seed $seed

    $report = $null
    try { $report = Get-Content $stdout -Raw | ConvertFrom-Json } catch { }
    $exitCode = if ($finished) { $download.ExitCode } else { 124 }
    $stopped = if ($report -and $report.torrents) { $report.torrents[0].stopped } else { $null }
    $landed = Join-Path $outDir "payload/movie.bin"
    $hash = if (Test-Path $landed) { (Get-FileHash -Algorithm SHA256 $landed).Hash.ToLower() } else { $null }
    $gaveUpAfterMs = if ($exitedAtMs) { $exitedAtMs - $cutAtMs } else { $null }

    # What the run did about the stall, from its own report rather than from
    # this script's clock. T-138's acceptance asks for both numbers.
    $redials = @()
    if ($report -and $report.torrents -and $report.torrents[0].redials) {
        $redials = @($report.torrents[0].redials)
    }
    $recoveredAfterMs = $null
    if ($redials.Count -gt 0) {
        $last = $redials[$redials.Count - 1]
        $recoveredAfterMs = [int64]$last.at_ms - $cutAtMs
    }

    [pscustomobject]@{
        scenario           = $name
        stop_timeout_s     = $stopTimeout
        redial_after_s     = $redialAfter
        exit_code          = $exitCode
        stopped            = $stopped
        downloaded_bytes   = if ($report) { [int64]$report.downloaded.bytes } else { 0 }
        downloaded_human   = if ($report) { $report.downloaded.human } else { "0 B" }
        cut_at_ms          = $cutAtMs
        exited_at_ms       = $exitedAtMs
        gave_up_after_ms   = $gaveUpAfterMs
        elapsed_ms         = $clock.ElapsedMilliseconds
        redials            = $redials
        redial_count       = $redials.Count
        last_redial_after_cut_ms = $recoveredAfterMs
        sha256             = $hash
        hash_matches       = ($hash -eq $expected)
        timeline           = @($timeline)
    }
}

# `patient` gets a stop-timeout well past the outage, so the run is still there
# when the peer comes back and the only question is whether it re-dials. It has
# to clear librqbit's own peer reconnect backoff too, which is 10s minimum with
# a factor of 6: attempts land at roughly 10s, 70s, and 430s after a peer
# drops, so an outage that ends between two of them waits for the next.
$patientTimeout = if ($PatientTimeout -gt 0) { $PatientTimeout } else { $OutageSeconds + $StopTimeout + 60 }
if ($RedialAfter -ge $patientTimeout) {
    Exit-With 2 "-RedialAfter $RedialAfter has to be shorter than the patient timeout of $patientTimeout, or the redial scenario gives up before it re-dials."
}

# The second attempt of librqbit's own backoff, in seconds after a peer drops.
# An outage that ends before it is caught without any help; one that ends after
# it waits for the third attempt, at about 430s. See TODO/peers.md, T-021.
$backoffReach = 70

$patient = Invoke-Scenario "patient" $patientTimeout
$impatient = Invoke-Scenario "impatient" $StopTimeout
$redial = Invoke-Scenario "redial" $patientTimeout $RedialAfter
Stop-Background

$failures = [System.Collections.ArrayList]::new()
$notes = [System.Collections.ArrayList]::new()
$recovered = ($patient.exit_code -eq 0) -and ($patient.stopped -eq "completed") -and $patient.hash_matches
if (-not $recovered) {
    $message = "patient: exit $($patient.exit_code), stopped '$($patient.stopped)', downloaded $($patient.downloaded_human). Given ${patientTimeout}s of patience against a ${OutageSeconds}s outage, the run did not re-dial and finish."
    if ($OutageSeconds -le $backoffReach) {
        [void]$failures.Add($message)
    }
    else {
        # Recorded, not judged: a ${OutageSeconds}s outage is past the second
        # backoff attempt, so waiting is the documented behaviour and T-138 is
        # the entry for it. The `redial` scenario is what has to pass here.
        [void]$notes.Add("$message This is T-021's recorded behaviour past the ${backoffReach}s backoff attempt, not a regression.")
    }
}
$saidSo = ($impatient.exit_code -eq 9) -and ($impatient.stopped -eq "stalled")
$inTime = ($null -ne $impatient.gave_up_after_ms) -and
    ($impatient.gave_up_after_ms -le ($StopTimeout + 15) * 1000)
if (-not $saidSo) {
    [void]$failures.Add(
        "impatient: exit $($impatient.exit_code), stopped '$($impatient.stopped)'. A run that cannot continue has to give up and say so.")
}
elseif (-not $inTime) {
    $late = [math]::Round($impatient.gave_up_after_ms / 1000, 1)
    [void]$failures.Add(
        "impatient: gave up ${late}s after the cut, past --stop-timeout ${StopTimeout}s plus slack.")
}
$redialed = ($redial.exit_code -eq 0) -and ($redial.stopped -eq "completed") -and $redial.hash_matches
if (-not $redialed) {
    [void]$failures.Add(
        "redial: exit $($redial.exit_code), stopped '$($redial.stopped)', downloaded $($redial.downloaded_human) after $($redial.redial_count) re-dial(s). --redial-after ${RedialAfter}s has to finish a run that a ${OutageSeconds}s outage would otherwise strand.")
}
elseif ($redial.redial_count -eq 0) {
    [void]$failures.Add(
        "redial: completed without re-dialling once, so the flag was not what finished it. Raise -OutageSeconds or lower -RedialAfter.")
}

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "peer-recovery-$stamp.json"
$gaveUpIn = if ($impatient.gave_up_after_ms) { [math]::Round($impatient.gave_up_after_ms / 1000, 1) } else { "no" }
$verdict = switch ($true) {
    ($failures.Count -eq 0) {
        "--redial-after finished a ${OutageSeconds}s outage in $($redial.redial_count) re-dial(s), and the run gave up in ${gaveUpIn}s when told to be less patient"
        break
    }
    default { "$($failures.Count) of 3 scenarios did not behave as described"; break }
}

[ordered]@{
    kind           = "check-peer-recovery"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = [System.Environment]::MachineName
        os      = [System.Environment]::OSVersion.VersionString
        cpus    = [System.Environment]::ProcessorCount
    }
    parameters     = [ordered]@{
        payload_size      = $PayloadSize
        payload_bytes     = $payloadBytes
        rate              = $Rate
        cut_after_seconds = $CutAfterSeconds
        outage_seconds    = $OutageSeconds
        stop_timeout      = $StopTimeout
        patient_timeout   = $patientTimeout
        redial_after      = $RedialAfter
        backoff_reach     = $backoffReach
        port              = $port
        profile           = $Profile
    }
    payload_sha256 = $expected
    scenarios      = @($patient, $impatient, $redial)
    verdict        = $verdict
    failures       = @($failures)
    recorded       = @($notes)
    commands       = @($commands)
    notes          = @(
        "The outage is the seeder being killed and restarted on the same port. Disabling a network adapter is a change to the machine and is not done here; what the client sees is the same either way, every peer connection dying at once and nothing reachable for a while.",
        "The port is chosen once by the OS on a socket that is then closed, because the peer has to come back at the address the client already knows and --port 0 would move it.",
        "The transfer is held open with --max-download-rate so the outage lands in the middle of it. That cap is measured and holding in TODO/performance.md under T-031.",
        "gave_up_after_ms is measured from the cut, polled every 500ms, so it is when the run decided rather than when this script noticed.",
        "The impatient scenario is allowed 15s of slack over --stop-timeout, because the stall clock starts at the last progress rather than at the cut and the report interval quantises when it is checked.",
        "patient and redial differ in exactly one flag, so the pair is the measurement: same payload, same rate, same outage, same stop timeout. See TODO/peers.md, T-138.",
        "patient is only failed when the outage is inside librqbit's backoff reach, backoff_reach seconds. Past that, waiting is what T-021 measured and recorded, and failing the build for it would fail the build for an open defect."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "payload: $(Format-Size $payloadBytes) at $Rate, ${OutageSeconds}s outage after ${CutAfterSeconds}s"
Write-Host "report:  $reportPath"
Write-Host ""
@($patient, $impatient, $redial) | ForEach-Object {
    [pscustomobject][ordered]@{
        scenario        = $_.scenario
        "stop-timeout"  = "$($_.stop_timeout_s)s"
        "redial-after"  = if ($_.redial_after_s -gt 0) { "$($_.redial_after_s)s" } else { "off" }
        exit            = $_.exit_code
        stopped         = $_.stopped
        downloaded      = $_.downloaded_human
        hash            = if ($_.hash_matches) { "matches" } else { "-" }
        "re-dials"      = $_.redial_count
        "gave up after" = if ($_.gave_up_after_ms) { "{0:N1}s" -f ($_.gave_up_after_ms / 1000) } else { "-" }
    }
} | Format-Table -AutoSize | Out-String | Write-Host
foreach ($note in $notes) { Write-Host "recorded: $note" }
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-peer-recovery: $failure") }
    exit 1
}
exit 0
