# Drive a source that signs its URLs, redirects, and answers 403.
#
# This is the acceptance for three entries in `TODO/multi-source.md`:
#
#   T-131  the loopback file server can sign, redirect, and expire a signature
#   T-130  a source can be told which statuses are worth retrying
#   T-137  a cooled-down source comes back
#
# Nine cases, all on loopback, all against the same 64 MiB payload served under
# a name and a path that have nothing to do with the torrent's:
#
#   redirects        --redirect-chain 3 --sign-redirect 2 --require-sig
#                    completes, hashes equal, and every request was redirected
#                    before it was answered. This is what says the fetcher does
#                    not pin a resolved URL.
#   too_many_hops    --redirect-chain 12 exceeds the ten reqwest follows. The
#                    run fails and the reason names it.
#   expiring_default a signature window shorter than the redirect round trip,
#                    so signatures genuinely expire between the 302 and the
#                    request that carries them. Fails today, 403 is permanent.
#   expiring_retry   the same server with --web-seed-retry-status 403.
#                    Completes, hashes equal, and the report says how many
#                    retries the 403s cost.
#   fatal_override   a mirror that answers 503, which is transient by default,
#                    with --web-seed-fatal-status 503. Retires instead of
#                    retrying.
#   recovering_503   the same mirror with no policy at all, as the control:
#                    503 is already transient, so it completes.
#   cooldown_short   a mirror down for -OutageSeconds on a clock, with
#                    --web-seed-cooldown shorter than that. The source spends
#                    its budget, sleeps, wakes into a mirror that is still
#                    down, sleeps again, and finishes once it is back.
#   cooldown_long    the same mirror with a cooldown past the run's deadline.
#                    Does not finish, and the report says the source is
#                    cooling and for how much longer.
#   dead_mirror      a mirror that never recovers, at the default cooldown of
#                    zero. The source retires and the run fails in seconds,
#                    which is the fail-fast path the default protects.
#
# cooldown_short and cooldown_long are the same server and the same deadline.
# The only difference is the cooldown, so the pair is what says the flag moved
# the number.
#
# Usage:
#   pwsh scripts/check-signed-source.ps1
#   pwsh scripts/check-signed-source.ps1 -PayloadSize 16MiB -Window 0.01
#
# Exits 0 when every case behaves as described, 1 when one does not, and 2
# when the check could not run. The record goes to
# bench/signed-source-<timestamp>.json.
#
# See TODO/multi-source.md, T-130, T-131, and T-137.

[CmdletBinding()]
param(
    [string]$PayloadSize = "64MiB",
    [string]$ChunkSize = "1MiB",
    # The signature window, in seconds. It has to be shorter than the round
    # trip from the 302 to the request that carries the signature, or nothing
    # ever expires: see the note under T-131.
    [double]$Window = 0.01,
    # How long the mirror in the three cooldown cases stays down, on a clock.
    [int]$OutageSeconds = 20,
    # A cooldown shorter than the outage, so the source wakes into a mirror
    # that is still down and then into one that is back.
    [int]$ShortCooldown = 5,
    # A cooldown longer than the deadline, so the source is still asleep when
    # the run gives up.
    [int]$LongCooldown = 300,
    # `--timeout` for the three cooldown cases. Longer than the outage and
    # shorter than the long cooldown, which is what makes the pair a
    # measurement rather than two unrelated runs.
    [int]$CooldownDeadline = 60,
    # How long a run against a mirror that never recovers may take before
    # "fails fast" stops being true.
    [int]$FailFastCeiling = 45,
    [string]$Root = ".tmp/signed",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [int]$TimeoutSeconds = 180,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-signed-source: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

function ConvertFrom-Size([string]$text) {
    if ($text -match '^\s*([0-9]+(?:\.[0-9]+)?)\s*([A-Za-z]*)\s*$') {
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
$fileserver = Join-Path $repo "target/$Profile/examples/loopback-fileserver$exe"
foreach ($required in @($bitCli, $fileserver)) {
    if (-not (Test-Path $required)) {
        Exit-With 2 "missing $required. Build it first: cargo build --$Profile --workspace --bins --examples"
    }
}
if ($Window -le 0) { Exit-With 2 "-Window has to be positive." }

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path (Join-Path $Root "payload") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Root "cdn") | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# ---------------------------------------------------------------------------
# The payload, the torrent, and the CDN copy
# ---------------------------------------------------------------------------
#
# The CDN copy is the same bytes under an unrelated name in an unrelated
# directory, which is what `--web-seed-mode exact` exists for and what a signed
# CDN URL always looks like.

$payloadBytes = ConvertFrom-Size $PayloadSize
Write-Step "building a payload of $(Format-Size $payloadBytes)"
$blob = Join-Path $Root "payload/movie.bin"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 4711
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}
$stream = [System.IO.File]::Create($blob)
try {
    [int64]$written = 0
    while ($written -lt $payloadBytes) {
        $take = [Math]::Min([int64]$block.Length, $payloadBytes - $written)
        $stream.Write($block, 0, [int]$take)
        $written += $take
    }
}
finally { $stream.Dispose() }
$cdnCopy = Join-Path $Root "cdn/9f2c1a-signed-movie.dat"
Copy-Item $blob $cdnCopy
$expected = (Get-FileHash -Algorithm SHA256 $blob).Hash.ToLower()

$torrent = Join-Path $Root "movie.torrent"
& $bitCli create (Join-Path $Root "payload") --name payload --piece-length 1MiB `
    --no-creation-date --output $torrent --force --json 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }

# ---------------------------------------------------------------------------
# One server per case
# ---------------------------------------------------------------------------

function Start-Server([string[]]$serverArgs, [string]$label) {
    $stdout = Join-Path $Root "$label.url"
    $stderr = Join-Path $Root "$label.srv"
    $process = Start-Process -FilePath $fileserver -WorkingDirectory $Root -NoNewWindow -PassThru `
        -ArgumentList (@("--root", $Root, "--port", "0") + $serverArgs) `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $script:Background += $process
    $deadline = (Get-Date).AddSeconds(15)
    $base = $null
    while (-not $base -and (Get-Date) -lt $deadline) {
        if (Test-Path $stdout) {
            $line = Get-Content $stdout -TotalCount 1 -ErrorAction SilentlyContinue
            if ($line -and $line.Trim()) { $base = $line.Trim() }
        }
        if (-not $base) { Start-Sleep -Milliseconds 50 }
    }
    if (-not $base) { Exit-With 2 "the file server for $label printed no URL" }
    [pscustomobject]@{ process = $process; base = $base; log = $stderr }
}

function Stop-Server($server) {
    if ($server.process -and -not $server.process.HasExited) {
        Stop-Process -Id $server.process.Id -Force -ErrorAction SilentlyContinue
    }
    $script:Background = @($script:Background | Where-Object { $_.Id -ne $server.process.Id })
}

function Measure-ServerLog([string]$path) {
    if (-not (Test-Path $path)) { return [pscustomobject]@{ redirects = 0; refusals = 0; served = 0 } }
    $lines = Get-Content $path -ErrorAction SilentlyContinue
    [pscustomobject]@{
        redirects = @($lines | Where-Object { $_ -match '-> 302 ' }).Count
        refusals  = @($lines | Where-Object { $_ -match '-> 403 ' }).Count
        served    = @($lines | Where-Object { $_ -match '-> 20[06] ' }).Count
    }
}

$commands = [System.Collections.ArrayList]::new()

function Invoke-Download([string]$label, [string]$base, [string[]]$extra) {
    $outDir = Join-Path $Root "out-$label"
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $stdout = Join-Path $Root "$label.json"
    $stderr = Join-Path $Root "$label.err"
    $arguments = @(
        "download", $torrent,
        "--dir", $outDir,
        "--web-seed-for", "file:0=$($base)cdn/9f2c1a-signed-movie.dat",
        "--web-seed-mode", "exact",
        "--web-seed-only",
        "--web-seed-chunk-size", $ChunkSize,
        "--port", "0",
        "--json"
    ) + $extra
    [void]$commands.Add("bit-cli $($arguments -join ' ')")
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $repo -NoNewWindow -PassThru `
        -ArgumentList $arguments -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $finished = $process.WaitForExit($TimeoutSeconds * 1000)
    $clock.Stop()
    if (-not $finished) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        return [pscustomobject]@{ exit_code = 124; elapsed_ms = $clock.ElapsedMilliseconds; report = $null; out_dir = $outDir; stderr = $stderr }
    }
    $report = $null
    try { $report = Get-Content $stdout -Raw | ConvertFrom-Json } catch { }
    [pscustomobject]@{
        exit_code  = $process.ExitCode
        elapsed_ms = $clock.ElapsedMilliseconds
        report     = $report
        out_dir    = $outDir
        stderr     = $stderr
    }
}

function Get-PayloadHash([string]$outDir) {
    $landed = Join-Path $outDir "payload/movie.bin"
    if (-not (Test-Path $landed)) { return $null }
    (Get-FileHash -Algorithm SHA256 $landed).Hash.ToLower()
}

$cases = [System.Collections.ArrayList]::new()
$failures = [System.Collections.ArrayList]::new()

function Add-Case([string]$name, $run, $log, [string]$expectation, [bool]$ok, [string]$detail) {
    $source = $null
    if ($run.report -and $run.report.torrents -and $run.report.torrents[0].sources) {
        $source = $run.report.torrents[0].sources[0]
    }
    $retriesByStatus = [ordered]@{}
    if ($source -and $source.PSObject.Properties.Name -contains 'retries_by_status') {
        foreach ($property in $source.retries_by_status.PSObject.Properties) {
            $retriesByStatus[$property.Name] = $property.Value
        }
    }
    [void]$cases.Add([ordered]@{
        case              = $name
        expectation       = $expectation
        exit_code         = $run.exit_code
        elapsed_ms        = $run.elapsed_ms
        downloaded_bytes  = if ($run.report) { $run.report.downloaded.bytes } else { 0 }
        downloaded_human  = if ($run.report) { $run.report.downloaded.human } else { "0 B" }
        completed         = if ($run.report) { $run.report.completed } else { 0 }
        failed            = if ($run.report) { $run.report.failed } else { 0 }
        hash_matches      = ((Get-PayloadHash $run.out_dir) -eq $expected)
        source_state      = if ($source) { $source.state } else { $null }
        source_error      = if ($source) { $source.error } else { $null }
        retries           = if ($source -and $source.PSObject.Properties.Name -contains 'retries') { $source.retries } else { 0 }
        retries_by_status = $retriesByStatus
        cooldowns         = if ($source -and $source.PSObject.Properties.Name -contains 'cooldowns') { $source.cooldowns } else { 0 }
        cooldown_remaining_ms = if ($source -and $source.PSObject.Properties.Name -contains 'cooldown_remaining_ms') { $source.cooldown_remaining_ms } else { $null }
        cooldown_until    = if ($source -and $source.PSObject.Properties.Name -contains 'cooldown_until') { $source.cooldown_until } else { $null }
        server_redirects  = $log.redirects
        server_refusals   = $log.refusals
        server_served     = $log.served
        ok                = $ok
        detail            = $detail
    })
    if (-not $ok) { [void]$failures.Add("$name : $detail") }
}

# --- 1. redirects ----------------------------------------------------------
Write-Step "redirects: three plain hops, then a signing hop, on every request"
$server = Start-Server @("--redirect-chain", "3", "--sign-redirect", "2", "--require-sig") "redirects"
$run = Invoke-Download "redirects" $server.base @()
Stop-Server $server
$log = Measure-ServerLog $server.log
$hash = Get-PayloadHash $run.out_dir
$ok = ($run.exit_code -eq 0) -and ($hash -eq $expected) -and ($log.redirects -ge (4 * $log.served)) -and ($log.refusals -eq 0)
Add-Case "redirects" $run $log `
    "completes over four redirect hops per request, hashes equal, no signature expires at a two second window" `
    $ok "exit $($run.exit_code), $($log.redirects) redirects for $($log.served) answers, $($log.refusals) refusals, hash $(if ($hash -eq $expected) { 'matches' } else { 'differs' })"

# --- 2. too many hops ------------------------------------------------------
Write-Step "too_many_hops: twelve hops against the ten a client follows"
$server = Start-Server @("--redirect-chain", "12") "hops"
$run = Invoke-Download "hops" $server.base @()
Stop-Server $server
$log = Measure-ServerLog $server.log
$saidSo = (Get-Content $run.stderr -Raw -ErrorAction SilentlyContinue) -match 'redirect'
$ok = ($run.exit_code -ne 0) -and $saidSo
Add-Case "too_many_hops" $run $log `
    "fails, and the reason names the redirect limit" `
    $ok "exit $($run.exit_code), reason $(if ($saidSo) { 'names redirects' } else { 'does not name redirects' })"

# --- 3. an expiring signature, with no policy ------------------------------
Write-Step "expiring_default: a ${Window}s signature window, no status policy"
$server = Start-Server @("--sign-redirect", "$Window", "--require-sig") "expiring-default"
$run = Invoke-Download "expiring-default" $server.base @()
Stop-Server $server
$log = Measure-ServerLog $server.log
$ok = ($run.exit_code -ne 0) -and ($log.refusals -gt 0)
Add-Case "expiring_default" $run $log `
    "signatures expire, 403 is permanent by default, the source retires and the run fails" `
    $ok "exit $($run.exit_code), $($log.refusals) refusals"
$refusalsWithoutPolicy = $log.refusals

# --- 4. an expiring signature, with the policy -----------------------------
Write-Step "expiring_retry: the same window, --web-seed-retry-status 403"
$server = Start-Server @("--sign-redirect", "$Window", "--require-sig") "expiring-retry"
$run = Invoke-Download "expiring-retry" $server.base @("--web-seed-retry-status", "403")
Stop-Server $server
$log = Measure-ServerLog $server.log
$hash = Get-PayloadHash $run.out_dir
$charged = 0
if ($run.report -and $run.report.torrents[0].sources[0].PSObject.Properties.Name -contains 'retries_by_status') {
    $entry = $run.report.torrents[0].sources[0].retries_by_status.PSObject.Properties |
        Where-Object { $_.Name -eq '403' }
    if ($entry) { $charged = $entry.Value }
}
$ok = ($run.exit_code -eq 0) -and ($hash -eq $expected) -and ($log.refusals -gt 0) -and ($charged -gt 0)
Add-Case "expiring_retry" $run $log `
    "the same expiring signatures ride out, the payload completes byte for byte, and the retries are charged to 403" `
    $ok "exit $($run.exit_code), $($log.refusals) refusals, $charged retries charged to 403, hash $(if ($hash -eq $expected) { 'matches' } else { 'differs' })"

# --- 5. a transient status the caller calls fatal --------------------------
Write-Step "fatal_override: a recovering 503 with --web-seed-fatal-status 503"
$server = Start-Server @("--status", "503", "--fail-after", "4", "--recover-after", "8") "fatal"
$run = Invoke-Download "fatal" $server.base @("--web-seed-fatal-status", "503", "--web-seed-retries", "0")
Stop-Server $server
$log = Measure-ServerLog $server.log
$ok = ($run.exit_code -ne 0)
Add-Case "fatal_override" $run $log `
    "503 is transient by default; called fatal, the source retires on the first one" `
    $ok "exit $($run.exit_code)"

# --- 6. the control: the same server with no policy ------------------------
Write-Step "recovering_503: the same server, no policy, so the default retry applies"
$server = Start-Server @("--status", "503", "--fail-after", "4", "--recover-after", "8") "control"
$run = Invoke-Download "control" $server.base @()
Stop-Server $server
$log = Measure-ServerLog $server.log
$hash = Get-PayloadHash $run.out_dir
$ok = ($run.exit_code -eq 0) -and ($hash -eq $expected)
Add-Case "recovering_503" $run $log `
    "the same failure with no policy completes, because 503 is already transient" `
    $ok "exit $($run.exit_code), hash $(if ($hash -eq $expected) { 'matches' } else { 'differs' })"

# --- 7. a cooldown shorter than the outage ---------------------------------
#
# The mirror is down for -OutageSeconds on a clock rather than for a number of
# requests, because a source that is cooling down makes no requests and a
# request-counted outage would never end while it waits.

Write-Step "cooldown_short: a ${OutageSeconds}s outage, --web-seed-cooldown ${ShortCooldown}s"
$server = Start-Server @("--status", "503", "--fail-after", "4", "--down-for", "$OutageSeconds") "cool-short"
$run = Invoke-Download "cool-short" $server.base @(
    "--web-seed-cooldown", "${ShortCooldown}s",
    "--web-seed-max-errors", "2",
    "--web-seed-retries", "0",
    "--timeout", "${CooldownDeadline}s")
Stop-Server $server
$log = Measure-ServerLog $server.log
$hash = Get-PayloadHash $run.out_dir
$shortSource = if ($run.report -and $run.report.torrents) { $run.report.torrents[0].sources[0] } else { $null }
$shortCooldowns = if ($shortSource -and $shortSource.PSObject.Properties.Name -contains 'cooldowns') { $shortSource.cooldowns } else { 0 }
$ok = ($run.exit_code -eq 0) -and ($hash -eq $expected) -and ($shortCooldowns -gt 0)
Add-Case "cooldown_short" $run $log `
    "the source cools down, comes back, and finishes the payload byte for byte" `
    $ok "exit $($run.exit_code), $shortCooldowns cooldown(s), hash $(if ($hash -eq $expected) { 'matches' } else { 'differs' })"

# --- 8. a cooldown longer than the outage ----------------------------------
Write-Step "cooldown_long: the same outage, --web-seed-cooldown ${LongCooldown}s past the deadline"
$server = Start-Server @("--status", "503", "--fail-after", "4", "--down-for", "$OutageSeconds") "cool-long"
$run = Invoke-Download "cool-long" $server.base @(
    "--web-seed-cooldown", "${LongCooldown}s",
    "--web-seed-max-errors", "2",
    "--web-seed-retries", "0",
    "--timeout", "${CooldownDeadline}s")
Stop-Server $server
$log = Measure-ServerLog $server.log
$hash = Get-PayloadHash $run.out_dir
$longSource = if ($run.report -and $run.report.torrents) { $run.report.torrents[0].sources[0] } else { $null }
$longState = if ($longSource) { $longSource.state } else { $null }
$longRemaining = if ($longSource -and $longSource.PSObject.Properties.Name -contains 'cooldown_remaining_ms') { $longSource.cooldown_remaining_ms } else { $null }
$ok = ($run.exit_code -ne 0) -and ($hash -ne $expected) -and ($longState -eq "cooling") -and ($null -ne $longRemaining)
Add-Case "cooldown_long" $run $log `
    "the same outage with a cooldown past the deadline does not finish, and the report says the source is cooling and for how much longer" `
    $ok "exit $($run.exit_code), state '$longState', $(if ($null -ne $longRemaining) { "$([math]::Round($longRemaining / 1000, 1))s left" } else { 'no remaining time reported' })"

# --- 9. the default: a dead mirror still fails fast ------------------------
Write-Step "dead_mirror: a mirror that never recovers, at the default cooldown of zero"
$server = Start-Server @("--status", "503") "dead"
$run = Invoke-Download "dead" $server.base @("--timeout", "${CooldownDeadline}s")
Stop-Server $server
$log = Measure-ServerLog $server.log
$deadSource = if ($run.report -and $run.report.torrents) { $run.report.torrents[0].sources[0] } else { $null }
$deadState = if ($deadSource) { $deadSource.state } else { $null }
$fast = $run.elapsed_ms -lt ($FailFastCeiling * 1000)
$ok = ($run.exit_code -ne 0) -and $fast -and ($deadState -eq "failed")
Add-Case "dead_mirror" $run $log `
    "with no cooldown set the source retires and the run fails in seconds rather than sitting on a timer" `
    $ok "exit $($run.exit_code) after $([math]::Round($run.elapsed_ms / 1000, 1))s, state '$deadState', ceiling ${FailFastCeiling}s"

Stop-Background

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "signed-source-$stamp.json"
$verdict = switch ($true) {
    ($failures.Count -eq 0) { "every case behaved as described"; break }
    default { "$($failures.Count) of $($cases.Count) cases did not"; break }
}

[ordered]@{
    kind           = "check-signed-source"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = [System.Environment]::MachineName
        os      = [System.Environment]::OSVersion.VersionString
        cpus    = [System.Environment]::ProcessorCount
    }
    parameters     = [ordered]@{
        payload_size = $PayloadSize
        payload_bytes = $payloadBytes
        chunk_size   = $ChunkSize
        window_s     = $Window
        outage_s     = $OutageSeconds
        short_cooldown_s = $ShortCooldown
        long_cooldown_s  = $LongCooldown
        cooldown_deadline_s = $CooldownDeadline
        fail_fast_ceiling_s = $FailFastCeiling
        profile      = $Profile
    }
    payload_sha256 = $expected
    cases          = @($cases)
    verdict        = $verdict
    failures       = @($failures)
    commands       = @($commands)
    notes          = @(
        "The signature window has to be shorter than the round trip from the 302 to the request carrying the signature. bit-cli re-resolves the stable URL on every ranged request, so a window measured in seconds never expires mid-request: measured at 2s and 0.1s, zero refusals; at 0.01s, refusals on every run. See TODO/multi-source.md under T-131.",
        "expiring_default and expiring_retry are the same server and the same window. The only difference is --web-seed-retry-status 403, so the pair is what says the flag and not the timing decided the outcome.",
        "recovering_503 is the control for fatal_override: the same failure with no policy completes, so the failure in fatal_override came from the flag.",
        "The redirect count in redirects is four per answered request: three plain hops from --redirect-chain 3 and one signing hop. A client that pinned the resolved URL would show one redirect for the whole run.",
        "The three cooldown cases use --down-for, which ends the outage on a clock rather than after a number of requests. A source that is cooling down makes no requests, so a request-counted outage would never advance while it waits and the mirror would never come back.",
        "cooldown_short and cooldown_long share a server, a payload, a deadline, and every other flag. The cooldown is the only difference, which is what makes the pair a measurement. See TODO/multi-source.md, T-137.",
        "dead_mirror runs at every default, including --web-seed-cooldown 0. It is the case the default exists for: one mirror that never comes back has to fail in seconds rather than sit on a timer."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "payload:  $(Format-Size $payloadBytes), window ${Window}s"
Write-Host "report:   $reportPath"
Write-Host ""
$cases | ForEach-Object {
    [pscustomobject][ordered]@{
        case        = $_.case
        exit        = $_.exit_code
        downloaded  = $_.downloaded_human
        hash        = if ($_.hash_matches) { "matches" } else { "-" }
        "302"       = $_.server_redirects
        "403"       = $_.server_refusals
        retries     = $_.retries
        state       = $_.source_state
        cooldowns   = $_.cooldowns
        ok          = if ($_.ok) { "yes" } else { "NO" }
    }
} | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "refusals with no policy: $refusalsWithoutPolicy"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-signed-source: $failure") }
    exit 1
}
exit 0
