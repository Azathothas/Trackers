# Does the whole-run rate cap move a number, across several torrents at once.
#
# `TODO/cli-surface.md` T-181: `--max-overall-download-rate` and
# `--max-overall-upload-rate` parsed, reached no code, and capped nothing. The
# per-torrent pair beside them did work, and was reaching `librqbit`'s
# *session* limit, so capping one torrent capped the whole run and capping the
# whole run did nothing. `librqbit` 9.0.0 has two fields, one on
# `SessionOptions` and one on `AddTorrentOptions`, and each flag now goes to
# the one it names.
#
# T-031 already measured `--max-download-rate` and it did so with one torrent,
# where per-torrent and whole-run are the same number. This is the measurement
# that tells them apart: four torrents in one invocation, so a session cap has
# to hold across all four together and a per-torrent cap has to not.
#
# The sources are HTTP web seeds rather than peers, deliberately. A web seed
# reaches the session as a peer, so the session limiter is what bounds it, and
# [T-132](../TODO/multi-source.md) is that `--max-overall-*` and
# `--web-seed-speed-limit` therefore interact. Measuring the cap over HTTP
# sources is the case that interaction is about.
#
# **It does not measure the two flags together.** An earlier revision of this
# header said phase 3 did, and phase 3 is `--max-download-rate`, the
# per-torrent cap. `--web-seed-speed-limit` appears in no phase here. Composing
# the two is [T-132](../TODO/multi-source.md)'s and belongs with the rest of
# that entry rather than bolted on here.
#
# Usage:
#   pwsh scripts/check-overall-rate.ps1
#   pwsh scripts/check-overall-rate.ps1 -Rate 4MiB/s -PayloadSize 64MiB -Torrents 4
#
# Exits 0 when the caps hold, 1 when one does not, and 2 when the check could
# not run. The record goes to bench/overall-rate-<timestamp>.json.
#
# See TODO/cli-surface.md, T-181.

[CmdletBinding()]
param(
    [string]$Rate = "4MiB/s",
    # Total across every torrent, split evenly between them.
    [string]$PayloadSize = "64MiB",
    [int]$Torrents = 4,
    # Fraction the sustained rate may exceed the cap by. The acceptance in
    # T-181 asks for ten per cent; the default here is that number.
    [double]$Tolerance = 0.10,
    [string]$Root = ".tmp/overall-rate",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [int]$TimeoutSeconds = 900,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-overall-rate: $message")
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

function Start-Background($name, $path, $arguments, $workdir) {
    $stdout = Join-Path $Root "$name.out"
    $stderr = Join-Path $Root "$name.err"
    $process = Start-Process -FilePath $path -ArgumentList $arguments `
        -WorkingDirectory $workdir -NoNewWindow -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $script:Background += $process
    [pscustomobject]@{ Process = $process; Stdout = $stdout; Stderr = $stderr }
}

function Stop-Background {
    foreach ($process in $script:Background) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $script:Background = @()
}

function Wait-ForLine($file, $seconds, $what) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $file) {
            $line = (Get-Content $file -TotalCount 1 -ErrorAction SilentlyContinue)
            if ($line -and $line.Trim()) { return $line.Trim() }
        }
        Start-Sleep -Milliseconds 100
    }
    Exit-With 2 "timed out waiting for $what"
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
if ($Torrents -lt 2) { Exit-With 2 "-Torrents has to be at least 2, or this measures the same thing T-031 already did." }

$rateBytes = ConvertFrom-Size $Rate
if ($rateBytes -lt 1) { Exit-With 2 "-Rate has to be positive." }

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

$payloadBytes = ConvertFrom-Size $PayloadSize
$perTorrent = [int64]($payloadBytes / $Torrents)
if ($perTorrent -lt 1MB) { Exit-With 2 "-PayloadSize split $Torrents ways is under a megabyte each, which measures startup rather than throughput." }

# One block of pseudo-random bytes, written repeatedly. Random rather than
# zeroes because a run of zeroes is what a filesystem or an HTTP layer
# compresses or elides, and either would measure the shortcut.
Write-Step "building $Torrents payloads of $(Format-Size $perTorrent) each"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 8675309
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}

$torrentPaths = @()
for ($t = 1; $t -le $Torrents; $t++) {
    $name = "payload$t"
    $dir = Join-Path $Root $name
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    # A block of its own per torrent, so no two torrents share a piece and
    # nothing can be donated between them. A donated file would be served from
    # disk rather than over HTTP and would not be rate limited at all, which
    # would read as the cap failing. See TODO/multi-source.md, T-140.
    $mine = [byte[]]::new($block.Length)
    for ($i = 0; $i -lt $block.Length; $i++) { $mine[$i] = [byte]($block[$i] -bxor $t) }

    # Two files, so every torrent is unambiguously multi-file and the web seed
    # composition is the directory form rather than the single-file one.
    foreach ($file in @("movie.bin", "notes.txt")) {
        $want = if ($file -eq "notes.txt") { [int64]4096 } else { $perTorrent - 4096 }
        $stream = [System.IO.File]::Create((Join-Path $dir $file))
        try {
            [int64]$written = 0
            while ($written -lt $want) {
                $take = [Math]::Min([int64]$mine.Length, $want - $written)
                $stream.Write($mine, 0, [int]$take)
                $written += $take
            }
        }
        finally { $stream.Dispose() }
    }

    $torrent = Join-Path $Root "$name.torrent"
    Push-Location $Root
    try {
        & $bitCli create $name --name $name --piece-length 1MiB --no-creation-date `
            --output $torrent --force --json 2>&1 | Out-Null
        if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE for $name" }
    }
    finally { Pop-Location }
    $torrentPaths += $torrent
}

Write-Step "starting the mirror"
$server = Start-Background "fileserver" $fileserver @("--root", $Root) $Root
$webSeed = Wait-ForLine $server.Stdout 15 "URL on the file server's stdout"
Write-Step "  mirror at $webSeed"

$commands = [System.Collections.ArrayList]::new()

# One invocation, every torrent, `-j` wide enough that all of them run at once.
# A cap that holds only because the torrents ran one after another would prove
# nothing.
function Invoke-Run([string]$label, [string[]]$extra) {
    $outDir = Join-Path $Root "out-$label"
    if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $stdout = Join-Path $Root "$label.json"

    $arguments = @("download") + $torrentPaths + @(
        "--dir", $outDir,
        "-j", "$Torrents",
        "--no-torrent-web-seed", "--web-seed", $webSeed,
        "--no-dht", "--no-lsd", "--no-tracker",
        "--port", "0", "--json"
    ) + $extra
    [void]$commands.Add("bit-cli $($arguments -join ' ')")

    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $Root -NoNewWindow -PassThru `
        -ArgumentList $arguments -RedirectStandardOutput $stdout `
        -RedirectStandardError (Join-Path $Root "$label.err")
    $finished = $process.WaitForExit($TimeoutSeconds * 1000)
    $clock.Stop()
    if (-not $finished) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        return [pscustomobject]@{ exit_code = 124; elapsed_ms = $clock.ElapsedMilliseconds; bytes = 0; rate = 0; torrents = 0 }
    }

    $report = $null
    try { $report = Get-Content $stdout -Raw | ConvertFrom-Json } catch { }
    $bytes = if ($report) { [int64]$report.downloaded.bytes } else { 0 }
    $count = if ($report -and $report.torrents) { @($report.torrents).Count } else { 0 }
    $ms = [Math]::Max(1, $clock.ElapsedMilliseconds)
    [pscustomobject]@{
        exit_code  = $process.ExitCode
        elapsed_ms = $clock.ElapsedMilliseconds
        bytes      = $bytes
        torrents   = $count
        # From the wall clock and the bytes the report says landed, never from
        # the report's own mean, so the limiter is not measured by the thing it
        # is limiting.
        rate       = [double]$bytes * 1000.0 / $ms
    }
}

$phases = [System.Collections.ArrayList]::new()

Write-Step "phase 1 of 3: uncapped, $Torrents torrents at once"
$uncapped = Invoke-Run "uncapped" @()
Write-Step "  $(Format-Size $uncapped.rate)/s over $([math]::Round($uncapped.elapsed_ms / 1000, 1))s"

Write-Step "phase 2 of 3: --max-overall-download-rate $Rate"
$overall = Invoke-Run "overall" @("--max-overall-download-rate", $Rate)
Write-Step "  $(Format-Size $overall.rate)/s over $([math]::Round($overall.elapsed_ms / 1000, 1))s"

# The per-torrent flag at the same number. It is the same arithmetic seen from
# the other side: $Torrents torrents each capped at $Rate can move up to
# $Torrents * $Rate together, so a run that comes out near the session cap
# instead is the old behaviour, where both flags reached the session field.
Write-Step "phase 3 of 3: --max-download-rate $Rate, per torrent"
$perTorrentRun = Invoke-Run "per-torrent" @("--max-download-rate", $Rate)
Write-Step "  $(Format-Size $perTorrentRun.rate)/s over $([math]::Round($perTorrentRun.elapsed_ms / 1000, 1))s"

Stop-Background

foreach ($pair in @(
        @{ name = "uncapped"; run = $uncapped },
        @{ name = "overall"; run = $overall },
        @{ name = "per_torrent"; run = $perTorrentRun })) {
    [void]$phases.Add([ordered]@{
            phase       = $pair.name
            exit_code   = $pair.run.exit_code
            elapsed_ms  = $pair.run.elapsed_ms
            bytes       = $pair.run.bytes
            bytes_human = Format-Size $pair.run.bytes
            torrents    = $pair.run.torrents
            rate        = [int64]$pair.run.rate
            rate_human  = "$(Format-Size $pair.run.rate)/s"
        })
}

$overBy = ($overall.rate - $rateBytes) / $rateBytes
$failures = [System.Collections.ArrayList]::new()

foreach ($phase in $phases) {
    if ($phase.exit_code -ne 0) {
        [void]$failures.Add("the $($phase.phase) run exited $($phase.exit_code)")
    }
    if ($phase.torrents -ne $Torrents) {
        [void]$failures.Add("the $($phase.phase) run reported $($phase.torrents) torrents, not $Torrents")
    }
}

# The claim: a whole-run cap holds across every torrent together.
if ($overall.rate -gt $rateBytes * (1.0 + $Tolerance)) {
    [void]$failures.Add(
        "--max-overall-download-rate $Rate let $(Format-Size $overall.rate)/s through, $([math]::Round($overBy * 100, 2))% over")
}
# A cap that is not below what the link does measures nothing.
if ($uncapped.rate -le $rateBytes * 1.5) {
    [void]$failures.Add(
        "the uncapped run reached only $(Format-Size $uncapped.rate)/s, which is not meaningfully above the $Rate cap; lower -Rate or raise -PayloadSize")
}
# The two scopes are different fields, and this is what says so from the
# outside: the same number as a per-torrent cap has to let more through than
# as a whole-run cap, because there are $Torrents torrents.
if ($perTorrentRun.rate -le $overall.rate * 1.3) {
    [void]$failures.Add(
        "--max-download-rate $Rate over $Torrents torrents reached $(Format-Size $perTorrentRun.rate)/s, no more than the whole-run cap's $(Format-Size $overall.rate)/s, so the two flags are still reaching one field")
}

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "overall-rate-$stamp.json"
$verdict = if ($failures.Count -eq 0) {
    "both scopes hold: $(Format-Size $overall.rate)/s whole-run against a $Rate cap, $(Format-Size $perTorrentRun.rate)/s with the same number per torrent, $(Format-Size $uncapped.rate)/s uncapped"
}
else { "$($failures.Count) checks did not hold" }

[ordered]@{
    kind             = "check-overall-rate"
    schema_version   = "1"
    generated_at     = Get-Timestamp
    host             = [ordered]@{
        machine = [System.Environment]::MachineName
        os      = [System.Environment]::OSVersion.VersionString
        cpus    = [System.Environment]::ProcessorCount
    }
    parameters       = [ordered]@{
        rate               = $Rate
        rate_bytes         = $rateBytes
        payload_size       = $PayloadSize
        payload_bytes      = $payloadBytes
        bytes_per_torrent  = $perTorrent
        torrents           = $Torrents
        tolerance          = $Tolerance
        profile            = $Profile
    }
    phases           = @($phases)
    over_cap_share   = [math]::Round($overBy, 4)
    uncapped_ratio   = if ($overall.rate -gt 0) { [math]::Round($uncapped.rate / $overall.rate, 3) } else { $null }
    per_torrent_ratio = if ($overall.rate -gt 0) { [math]::Round($perTorrentRun.rate / $overall.rate, 3) } else { $null }
    verdict          = $verdict
    failures         = @($failures)
    commands         = @($commands)
    notes            = @(
        "The sources are HTTP web seeds, not peers. A web seed reaches the session as a peer, so the session limiter bounds it, which is the interaction TODO/multi-source.md T-132 is about.",
        "Every torrent runs at once: -j is the torrent count, so a cap that held only because the torrents ran in sequence would not pass.",
        "The rate is computed from the wall clock and the bytes the report says landed, not from the report's own mean.",
        "Each phase gets a fresh output directory. Reusing one lets the hash check on add find the payload already there and report a rate that measures the disk.",
        "Phase 3 is the discriminator. Before T-181 both flags reached SessionOptions::ratelimits, so --max-download-rate and --max-overall-download-rate at the same number produced the same run. They now reach two different librqbit fields and the per-torrent one has to let more through."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "$Torrents torrents, $(Format-Size $perTorrent) each, cap $Rate"
Write-Host "report:  $reportPath"
Write-Host ""
$phases | ForEach-Object {
    [pscustomobject][ordered]@{
        phase = $_.phase
        exit  = $_.exit_code
        wall  = "{0:N1}s" -f ($_.elapsed_ms / 1000)
        bytes = $_.bytes_human
        rate  = $_.rate_human
    }
} | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-overall-rate: $failure") }
    exit 1
}
exit 0
