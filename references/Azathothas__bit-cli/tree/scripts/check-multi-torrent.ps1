# Does running several torrents at once cost more than running them one after
# another?
#
# `-j` exists to run several sources in one invocation. If that is slower than
# running them serially for the same bytes, the flag is a trap. `TODO/performance.md`
# T-030 asks for three runs with the same total payload and the wall time,
# peak RSS, and CPU time of each.
#
# Four runs, not three, because three cannot separate two things:
#
#   one       one torrent alone. The per-torrent rate with nothing to share.
#   serial    N torrents, N invocations, one after another. The honest baseline
#             for "run them one at a time", process startup included, because
#             that is what a caller who avoided -j would actually pay.
#   j1        N torrents, one invocation, -j 1. Same session, same process, one
#             download at a time. Against `serial` it isolates what the shared
#             session costs from what the extra processes cost.
#   jN        N torrents, one invocation, -j N. All at once.
#
# Every run moves the same total bytes off the same loopback server, so the
# wire is not the variable. The server serves out of the page cache at over a
# gigabyte a second, well above anything the download path reaches, so what is
# measured is the tool.
#
# Each torrent gets its own payload with its own seed, so no two torrents share
# a piece and nothing is served from another torrent's cache.
#
# Usage:
#   pwsh scripts/check-multi-torrent.ps1
#   pwsh scripts/check-multi-torrent.ps1 -Torrents 6 -PayloadSize 256MiB -Runs 3
#
# Exits 0 when every run completed, 1 when one did not, and 2 when the check
# could not run. The record goes to bench/multi-torrent-<timestamp>.json.
#
# See TODO/performance.md, T-030.

[CmdletBinding()]
param(
    [int]$Torrents = 4,
    [string]$PayloadSize = "256MiB",
    [string]$PieceLength = "1MiB",
    [int]$Runs = 3,
    [int]$Connections = 4,
    [string]$Root = ".tmp/multi-torrent",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [int]$TimeoutSeconds = 900,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-multi-torrent: $message")
    Stop-Background
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

function Format-Rate([double]$bytesPerSecond) { "$(Format-Size $bytesPerSecond)/s" }

function Get-Median([double[]]$values) {
    if ($null -eq $values -or $values.Count -eq 0) { return 0 }
    $sorted = @($values | Sort-Object)
    $mid = [math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2 -eq 1) { return $sorted[$mid] }
    return ($sorted[$mid - 1] + $sorted[$mid]) / 2
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
if ($Torrents -lt 2) { Exit-With 2 "-Torrents has to be at least 2: the question is about several." }
if ($Runs -lt 1) { Exit-With 2 "-Runs has to be at least 1." }

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# ---------------------------------------------------------------------------
# Payloads
# ---------------------------------------------------------------------------
#
# One payload per torrent, each from its own seed, so no two torrents share a
# piece and no torrent is served out of another one's window cache.

$payloadBytes = ConvertFrom-Size $PayloadSize
$commands = [System.Collections.ArrayList]::new()
$torrentPaths = @()

Write-Step "building $Torrents payloads of $(Format-Size $payloadBytes)"
for ($t = 0; $t -lt $Torrents; $t++) {
    $dir = Join-Path $Root "payload$t"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $block = 1MB
    $buffer = [byte[]]::new($block)
    [int64]$state = 12345 + ($t * 7919)
    for ($i = 0; $i -lt $block; $i++) {
        $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
        $buffer[$i] = [byte](($state -shr 16) -band 0xFF)
    }
    $stream = [System.IO.File]::Create((Join-Path $dir "movie.bin"))
    try {
        $written = 0
        while ($written -lt $payloadBytes) {
            $want = [math]::Min($block, $payloadBytes - $written)
            $stream.Write($buffer, 0, $want)
            $written += $want
        }
    } finally { $stream.Dispose() }

    $torrent = Join-Path $Root "t$t.torrent"
    $arguments = @(
        "create", "payload$t", "--name", "payload$t", "--piece-length", $PieceLength,
        "--no-creation-date", "--output", $torrent, "--force", "--json"
    )
    [void]$commands.Add("$bitCli $($arguments -join ' ')")
    $process = Start-Process -FilePath $bitCli -ArgumentList $arguments -WorkingDirectory $Root `
        -NoNewWindow -Wait -PassThru -RedirectStandardOutput (Join-Path $Root "create$t.out") `
        -RedirectStandardError (Join-Path $Root "create$t.err")
    if ($process.ExitCode -ne 0) {
        Exit-With 2 "bit-cli create for payload$t exited $($process.ExitCode): $(Get-Content (Join-Path $Root "create$t.err") -Raw)"
    }
    $torrentPaths += $torrent
}

$server = Start-Process -FilePath $fileserver -ArgumentList @("--root", $Root) `
    -WorkingDirectory $Root -NoNewWindow -PassThru `
    -RedirectStandardOutput (Join-Path $Root "fileserver.out") `
    -RedirectStandardError (Join-Path $Root "fileserver.err")
$script:Background += $server
$webSeed = $null
$deadline = (Get-Date).AddSeconds(15)
while ((Get-Date) -lt $deadline -and -not $webSeed) {
    $line = Get-Content (Join-Path $Root "fileserver.out") -TotalCount 1 -ErrorAction SilentlyContinue
    if ($line -and $line.Trim()) { $webSeed = $line.Trim() }
    if (-not $webSeed) { Start-Sleep -Milliseconds 100 }
}
if (-not $webSeed) { Exit-With 2 "the loopback file server printed no URL" }
Write-Step "web seed at $webSeed"

# ---------------------------------------------------------------------------
# One download invocation
# ---------------------------------------------------------------------------

$script:outIndex = 0

function Invoke-Download([string]$label, [string[]]$sources, [int]$jobs, [string]$outDir, [int]$connections = 0) {
    $script:outIndex++
    $tag = "$label-$($script:outIndex)"
    if ($connections -le 0) { $connections = $Connections }
    if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null

    $arguments = @("download") + $sources + @(
        "--dir", $outDir,
        "--web-seed", $webSeed,
        "--web-seed-only",
        "--web-seed-connections", "$connections",
        "--port", "0",
        "--no-dht", "--no-lsd",
        "--file-allocation", "sparse",
        "--max-concurrent-downloads", "$jobs",
        "--json"
    )
    [void]$commands.Add("$bitCli $($arguments -join ' ')")
    $stdout = Join-Path $Root "$tag.out"
    $stderr = Join-Path $Root "$tag.err"
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $bitCli -ArgumentList $arguments -WorkingDirectory $Root `
        -NoNewWindow -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $clock.Stop()
        return [pscustomobject]@{
            ok = $false; elapsed_ms = [int64]$clock.Elapsed.TotalMilliseconds
            exit_code = 124; report = $null
        }
    }
    $clock.Stop()
    $report = $null
    try { $report = Get-Content $stdout -Raw | ConvertFrom-Json } catch { }
    [pscustomobject]@{
        ok = ($process.ExitCode -eq 0 -and $null -ne $report)
        elapsed_ms = [int64]$clock.Elapsed.TotalMilliseconds
        exit_code = $process.ExitCode
        report = $report
        stderr_path = $stderr
    }
}

# ---------------------------------------------------------------------------
# The ceiling
# ---------------------------------------------------------------------------
#
# Everything below reads off one loopback file server, so that server's own
# throughput is an upper bound on every mode and a rate that approaches it says
# nothing about `bit-cli`. `bench webseed` measures it with no bridge, no
# hashing, and no disk, at a concurrency well past what any mode below asks
# for.

Write-Step "ceiling: what the file server serves with no bridge, no hashing, no disk"
$ceilingArgs = @(
    "bench", "webseed", $torrentPaths[0],
    "--web-seed", $webSeed,
    "--duration", "10s", "--warmup", "2s",
    "--concurrency", "32", "--request-size", "1MiB",
    "--format", "json"
)
[void]$commands.Add("$bitCli $($ceilingArgs -join ' ')")
$ceilingOut = Join-Path $Root "ceiling.out"
$ceilingErr = Join-Path $Root "ceiling.err"
$ceilingProcess = Start-Process -FilePath $bitCli -ArgumentList $ceilingArgs -WorkingDirectory $Root `
    -NoNewWindow -Wait -PassThru -RedirectStandardOutput $ceilingOut -RedirectStandardError $ceilingErr
if ($ceilingProcess.ExitCode -ne 0) {
    Exit-With 2 "bench webseed exited $($ceilingProcess.ExitCode): $(Get-Content $ceilingErr -Raw)"
}
$ceilingReport = Get-Content $ceilingOut -Raw | ConvertFrom-Json
$ceilingBytes = [int64]$ceilingReport.summary.sustained_rate.bytes
Write-Step "ceiling is $(Format-Rate $ceilingBytes)"

# ---------------------------------------------------------------------------
# The runs
# ---------------------------------------------------------------------------

$results = [System.Collections.ArrayList]::new()
$failures = [System.Collections.ArrayList]::new()

# The -j values to step through, 1 up to the torrent count in powers of two,
# with the torrent count itself always included. Anything past it would be a
# permit nothing can take.
$jobSweep = @(1)
$next = 2
while ($next -lt $Torrents) { $jobSweep += $next; $next *= 2 }
if ($Torrents -gt 1) { $jobSweep += $Torrents }

function Record([string]$mode, [int]$iteration, [int64]$elapsedMs, $reports, [int]$processes) {
    $downloaded = 0
    $peakRss = 0
    $cpuMs = 0
    $handles = 0
    foreach ($report in $reports) {
        $downloaded += [int64]$report.downloaded.bytes
        $peakRss = [math]::Max($peakRss, [int64]$report.process.peak_rss_bytes)
        $cpuMs += [int64]$report.process.cpu_ms
        $handles = [math]::Max($handles, [int64]$report.process.open_handles)
        if ($report.failed -gt 0) {
            [void]$failures.Add("${mode} iteration ${iteration}: $($report.failed) torrent(s) did not finish")
        }
    }
    [void]$results.Add([ordered]@{
        mode          = $mode
        iteration     = $iteration
        processes     = $processes
        elapsed_ms    = $elapsedMs
        bytes         = $downloaded
        rate_bytes    = if ($elapsedMs -gt 0) { [int64]($downloaded * 1000 / $elapsedMs) } else { 0 }
        peak_rss      = $peakRss
        cpu_ms        = $cpuMs
        open_handles  = $handles
    })
}

for ($iteration = 1; $iteration -le $Runs; $iteration++) {
    Write-Step "iteration ${iteration}: one torrent alone"
    $single = Invoke-Download "one" @($torrentPaths[0]) 1 (Join-Path $Root "out-one")
    if (-not $single.ok) {
        Exit-With 1 "the single-torrent run exited $($single.exit_code): $(Get-Content $single.stderr_path -Raw)"
    }
    Record "one" $iteration $single.elapsed_ms @($single.report) 1

    Write-Step "iteration ${iteration}: $Torrents torrents, one invocation each, in turn"
    $serialMs = 0
    $serialReports = @()
    for ($t = 0; $t -lt $Torrents; $t++) {
        $run = Invoke-Download "serial" @($torrentPaths[$t]) 1 (Join-Path $Root "out-serial-$t")
        if (-not $run.ok) {
            Exit-With 1 "the serial run for torrent $t exited $($run.exit_code): $(Get-Content $run.stderr_path -Raw)"
        }
        $serialMs += $run.elapsed_ms
        $serialReports += $run.report
    }
    Record "serial" $iteration $serialMs $serialReports $Torrents

    # The sweep order flips between iterations, because a volume that has just
    # taken gigabytes is slower than one that has not, and a fixed order would
    # hand that to whichever step always runs last.
    $order = @($jobSweep)
    if ($iteration % 2 -eq 0) { [array]::Reverse($order) }
    foreach ($jobs in $order) {
        Write-Step "iteration ${iteration}: $Torrents torrents, one invocation, -j $jobs"
        $run = Invoke-Download "j$jobs" $torrentPaths $jobs (Join-Path $Root "out-j$jobs")
        if (-not $run.ok) {
            Exit-With 1 "the -j $jobs run exited $($run.exit_code): $(Get-Content $run.stderr_path -Raw)"
        }
        Record "j$jobs" $iteration $run.elapsed_ms @($run.report) 1
    }

    # The control. `-j N` runs N torrents at once and each keeps its own
    # connections, so the deepest sweep step has N times as many connections in
    # flight as the shallowest. This puts that same total on one torrent at a
    # time. If it reaches what `-j N` reaches, the flag is buying connections
    # rather than concurrency, and the two have to be told apart.
    $totalConnections = $Connections * $Torrents
    Write-Step "iteration ${iteration}: control, -j 1 with --web-seed-connections $totalConnections"
    $control = Invoke-Download "control" $torrentPaths 1 (Join-Path $Root "out-control") $totalConnections
    if (-not $control.ok) {
        Exit-With 1 "the control run exited $($control.exit_code): $(Get-Content $control.stderr_path -Raw)"
    }
    Record "control" $iteration $control.elapsed_ms @($control.report) 1
}

Stop-Background

# ---------------------------------------------------------------------------
# The verdict
# ---------------------------------------------------------------------------

$modes = @("one", "serial") + ($jobSweep | ForEach-Object { "j$_" }) + @("control")
$summary = [System.Collections.ArrayList]::new()
foreach ($mode in $modes) {
    $matching = @($results | Where-Object { $_.mode -eq $mode })
    if ($matching.Count -eq 0) { continue }
    $elapsed = [int64](Get-Median ([double[]]@($matching | ForEach-Object { [double]$_.elapsed_ms })))
    $bytes = [int64](Get-Median ([double[]]@($matching | ForEach-Object { [double]$_.bytes })))
    $rate = if ($elapsed -gt 0) { [int64]($bytes * 1000 / $elapsed) } else { 0 }
    [void]$summary.Add([ordered]@{
        mode          = $mode
        elapsed_ms    = $elapsed
        elapsed_human = "{0:N2}s" -f ($elapsed / 1000)
        bytes         = $bytes
        bytes_human   = Format-Size $bytes
        rate_bytes    = $rate
        rate_human    = Format-Rate $rate
        share_of_ceiling = if ($ceilingBytes -gt 0) { "{0:N2}%" -f (100 * $rate / $ceilingBytes) } else { "n/a" }
        peak_rss      = [int64](Get-Median ([double[]]@($matching | ForEach-Object { [double]$_.peak_rss })))
        cpu_ms        = [int64](Get-Median ([double[]]@($matching | ForEach-Object { [double]$_.cpu_ms })))
        open_handles  = [int64](Get-Median ([double[]]@($matching | ForEach-Object { [double]$_.open_handles })))
    })
}

$serial = $summary | Where-Object { $_.mode -eq "serial" } | Select-Object -First 1
$j1 = $summary | Where-Object { $_.mode -eq "j1" } | Select-Object -First 1
$jn = $summary | Where-Object { $_.mode -eq "j$Torrents" } | Select-Object -First 1
$one = $summary | Where-Object { $_.mode -eq "one" } | Select-Object -First 1

$control = $summary | Where-Object { $_.mode -eq "control" } | Select-Object -First 1
$jnOverSerial = if ($jn.elapsed_ms -gt 0) { [math]::Round([double]$serial.elapsed_ms / [double]$jn.elapsed_ms, 3) } else { 0 }
$jnOverJ1 = if ($jn.elapsed_ms -gt 0) { [math]::Round([double]$j1.elapsed_ms / [double]$jn.elapsed_ms, 3) } else { 0 }
$sessionCost = if ($serial.elapsed_ms -gt 0) { [math]::Round([double]$j1.elapsed_ms / [double]$serial.elapsed_ms, 3) } else { 0 }
$controlOverJn = if ($jn.rate_bytes -gt 0) { [math]::Round([double]$control.rate_bytes / [double]$jn.rate_bytes, 3) } else { 0 }
$idealMs = [double]$one.elapsed_ms * $Torrents
$ceilingShare = if ($ceilingBytes -gt 0) { [math]::Round([double]$jn.rate_bytes / [double]$ceilingBytes, 3) } else { 0 }

$verdict = if ($jnOverSerial -lt 1.0) {
    "-j is a trap here: $Torrents torrents at -j $Torrents took ${jnOverSerial}x the wall time of running them one invocation at a time"
} elseif ($jnOverJ1 -lt 1.05) {
    "-j buys nothing: -j $Torrents is ${jnOverJ1}x -j 1 in the same process, so the torrents are taking turns whatever the flag says"
} elseif ($ceilingShare -ge 0.9) {
    "-j holds and the server is what stops it: -j $Torrents is ${jnOverSerial}x one invocation at a time and reaches $($jn.share_of_ceiling) of what the file server serves, so the measurement is bounded by the server rather than by bit-cli"
} elseif ($controlOverJn -ge 0.95) {
    "-j holds, and it is buying connections rather than concurrency: -j $Torrents is ${jnOverSerial}x one invocation at a time, and one torrent at a time with the same total connections reaches ${controlOverJn}x what -j $Torrents reaches"
} else {
    "-j holds: $Torrents torrents at -j $Torrents finish in ${jnOverSerial}x less wall time than one invocation at a time, and ${jnOverJ1}x less than -j 1 in the same process, at $($jn.share_of_ceiling) of the file server's own rate. One torrent at a time with the same total connections reaches ${controlOverJn}x of it."
}

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "multi-torrent-$stamp.json"

[ordered]@{
    schema_version = "1"
    kind           = "multi-torrent"
    todo           = "T-030"
    generated_at   = Get-Timestamp
    parameters     = [ordered]@{
        torrents     = $Torrents
        payload_size = $PayloadSize
        piece_length = $PieceLength
        connections  = $Connections
        runs         = $Runs
        job_sweep    = @($jobSweep)
        profile      = $Profile
        web_seed     = $webSeed
    }
    verdict        = $verdict
    ceiling        = [ordered]@{
        what        = "bit-cli bench webseed against the same file server: no bridge, no hashing, no disk"
        rate_bytes  = $ceilingBytes
        rate_human  = Format-Rate $ceilingBytes
        concurrency = 32
    }
    ratios         = [ordered]@{
        serial_over_jn      = $jnOverSerial
        j1_over_jn          = $jnOverJ1
        j1_over_serial      = $sessionCost
        jn_share_of_ceiling = $ceilingShare
        control_over_jn     = $controlOverJn
        one_torrent_times_n = [int64]$idealMs
    }
    summary        = @($summary)
    runs           = @($results)
    commands       = @($commands)
    failures       = @($failures)
    notes          = @(
        "Every mode moves the same total bytes off the same loopback server, so the wire is not the variable. The server reads out of the page cache well above anything the download path reaches.",
        "Each torrent has its own payload from its own seed, so no two share a piece and no torrent is served out of another one's window cache.",
        "serial is N separate invocations, so its wall time includes N process startups and N sessions. j1 is one invocation with one download at a time, so the difference between the two is what the shared session costs rather than what the extra processes cost.",
        "one_torrent_times_n is the wall time N torrents would take if each cost exactly what one alone costs and nothing overlapped. It is a reference, not a target: a run faster than it overlapped something.",
        "ceiling is what the same file server serves through bit-cli's own HTTP path with no bridge, no hashing, and no disk. Every mode reads off that one server, so it is an upper bound on all of them, and a mode approaching it says more about the server than about bit-cli.",
        "control puts the same total connection count as the deepest -j step onto one torrent at a time. -j N runs N torrents at once and each keeps its own connections, so without the control the sweep cannot say whether the flag bought concurrency or bought connections."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "torrents: $Torrents of $(Format-Size $payloadBytes), $Runs iterations"
Write-Host "report:   $reportPath"
Write-Host ""
Write-Host "ceiling:  $(Format-Rate $ceilingBytes) through bit-cli's own HTTP path, no bridge, no hashing, no disk"
Write-Host ""
$summary | ForEach-Object {
    [pscustomobject][ordered]@{
        mode       = $_.mode
        wall       = $_.elapsed_human
        bytes      = $_.bytes_human
        rate       = $_.rate_human
        "of ceiling" = $_.share_of_ceiling
        "peak RSS" = Format-Size $_.peak_rss
        "CPU ms"   = $_.cpu_ms
        handles    = $_.open_handles
    }
} | Format-Table -AutoSize | Out-String | Write-Host
Write-Host ("one torrent x $Torrents would be {0:N2}s if nothing overlapped" -f ($idealMs / 1000))
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-multi-torrent: $failure") }
    exit 1
}
exit 0
