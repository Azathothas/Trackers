# Three torrents holding one file, in one invocation, with no binding written.
#
# This is the acceptance for `TODO/multi-source.md` T-140, and the last part of
# the operator's Scenario 2. [T-133](../TODO/multi-source.md) made the same
# thing work with `--web-seed-for '<HASH>:file:N=file:///...'` per torrent,
# which needs the caller to read the proof and write the binding. Here nothing
# is written: the run computes the equivalence from the metadata it already
# has, and reads the file from whichever torrent already wrote it.
#
# The fixture is `scripts/make-scenario-fixture.ps1` with one piece length
# instead of three. That matters and is the whole reason for the flag: two
# files can be compared by hash only where whole pieces cover the same bytes of
# each, so a shared file under three different piece lengths is not provable
# from the metadata at all. With one piece length and the file at a congruent
# offset in all three, every whole piece inside it lines up and its hash is the
# proof.
#
# What the run looks like:
#
#   torrent C   its own url-list points at a loopback mirror, so it fetches
#               the shared file and its own extra file over HTTP.
#   torrent A   every file except the shared one is already on disk. Its only
#               source is the copy C just wrote.
#   torrent B   the same, with the shared file at a different path and a
#               different index.
#
# `-j 1` runs them in the order given, which is what makes "already wrote it"
# true. The mirror is the only HTTP source in the run and only C can reach it,
# so "fetched once" is a number rather than a claim: the bytes crossing HTTP
# are C's, and A and B report their bytes against a `shared_file` source.
#
# Usage:
#   pwsh scripts/check-shared-files.ps1
#   pwsh scripts/check-shared-files.ps1 -BlobSizeMiB 64
#   pwsh scripts/check-shared-files.ps1 -Keep
#
# Exits 0 when every check holds, 1 when one does not, and 2 when the check
# could not run. The record goes to bench/shared-files-<timestamp>.json.
#
# See TODO/multi-source.md, T-140.

[CmdletBinding()]
param(
    [int]$BlobSizeMiB = 16,
    [int]$OtherSizeMiB = 2,
    [string]$Root = ".tmp/shared-files",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [int]$TimeoutSeconds = 300,
    # How many torrents run at once. `-j 1` is T-140's case and makes "an
    # earlier one has already written it" true by construction. Anything above
    # 1 is T-143's: the donor finishes while the takers are already running, so
    # the source has to attach to a torrent that has already started. See
    # TODO/multi-source.md, T-143.
    [int]$Jobs = 1,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-shared-files: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

function Format-Size([double]$bytes) {
    $units = @("B", "KiB", "MiB", "GiB", "TiB")
    $index = 0
    while ($bytes -ge 1024 -and $index -lt $units.Count - 1) { $bytes /= 1024; $index++ }
    "{0:N2} {1}" -f $bytes, $units[$index]
}

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
$serverExe = Join-Path $repo "target/$Profile/examples/loopback-fileserver$exe"
foreach ($required in @($bitCli, $serverExe)) {
    if (-not (Test-Path $required)) {
        Exit-With 2 "missing $required. Build it first: cargo build --$Profile --workspace --bins --examples"
    }
}
if ($BlobSizeMiB -lt 1) { Exit-With 2 "-BlobSizeMiB has to be at least 1." }

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

$fixture = Join-Path $Root "fixture"
New-Item -ItemType Directory -Force -Path $fixture | Out-Null

$script:Server = $null
function Stop-Server {
    if ($script:Server -and -not $script:Server.HasExited) {
        Stop-Process -Id $script:Server.Id -Force -ErrorAction SilentlyContinue
    }
    $script:Server = $null
}
trap { Stop-Server; throw }

# ---------------------------------------------------------------------------
# The mirror
# ---------------------------------------------------------------------------
#
# It starts before the fixture is built, because torrent C's url-list has to
# carry the URL it is created with and the port is the OS's choice. The
# fixture script recreates the directory underneath it, which the server does
# not mind: it resolves every request against the path it was given rather
# than holding the directory open.

Write-Step "starting the mirror"
$serverOut = Join-Path $Root "server.out"
$script:Server = Start-Process -FilePath $serverExe -WorkingDirectory $repo -NoNewWindow -PassThru `
    -ArgumentList @("--root", $fixture, "--port", "0") `
    -RedirectStandardOutput $serverOut -RedirectStandardError (Join-Path $Root "server.err")
$mirror = $null
$deadline = (Get-Date).AddSeconds(15)
while (-not $mirror -and (Get-Date) -lt $deadline) {
    $line = Get-Content $serverOut -TotalCount 1 -ErrorAction SilentlyContinue
    if ($line -and $line.Trim()) { $mirror = $line.Trim() }
    if (-not $mirror) { Start-Sleep -Milliseconds 100 }
}
if (-not $mirror) { Stop-Server; Exit-With 2 "the mirror never printed its URL" }
Write-Step "mirror at $mirror"

# ---------------------------------------------------------------------------
# The fixture
# ---------------------------------------------------------------------------

Write-Step "building the three-torrent fixture, $BlobSizeMiB MiB shared file, one piece length"
& (Join-Path $PSScriptRoot "make-scenario-fixture.ps1") `
    -BlobSizeMiB $BlobSizeMiB -OtherSizeMiB $OtherSizeMiB -Partial 0 `
    -PieceLength "1MiB" -WebSeed $mirror -Root $fixture -Profile $Profile | Out-Null
if ($LASTEXITCODE -ne 0) { Stop-Server; Exit-With 2 "make-scenario-fixture.ps1 exited $LASTEXITCODE" }

$blob = Join-Path $fixture "payload_a/deep/nested/dirs/file.blob"
if (-not (Test-Path $blob)) { Stop-Server; Exit-With 2 "the fixture has no shared file at $blob" }
$expected = (Get-FileHash -Algorithm SHA256 $blob).Hash.ToLower()
$blobBytes = (Get-Item $blob).Length

$torrents = [ordered]@{
    payload_c = Join-Path $fixture "torrent_c.torrent"
    payload_a = Join-Path $fixture "torrent_a.torrent"
    payload_b = Join-Path $fixture "torrent_b.torrent"
}
# Where the shared file sits in each torrent, and what else each one holds.
$sharedPath = @{
    payload_a = "deep/nested/dirs/file.blob"
    payload_b = "media/file.blob"
    payload_c = "a/b/c/file.blob"
}

# ---------------------------------------------------------------------------
# The state the run starts from
# ---------------------------------------------------------------------------
#
# A and B have everything except the shared file, so the only bytes the run has
# to find for them are the ones this is about. C has nothing and fetches its
# own files from the mirror.

$out = Join-Path $Root "out"
New-Item -ItemType Directory -Force -Path $out | Out-Null
foreach ($pair in @(
    @{ from = "payload_a/deep/other.bin"; to = "payload_a/deep/other.bin" },
    @{ from = "payload_a/readme.txt"; to = "payload_a/readme.txt" },
    @{ from = "payload_b/media/cover.png"; to = "payload_b/media/cover.png" },
    @{ from = "payload_b/notes/changelog.txt"; to = "payload_b/notes/changelog.txt" }
)) {
    $target = Join-Path $out $pair.to
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
    Copy-Item -LiteralPath (Join-Path $fixture $pair.from) -Destination $target -Force
}
$placedBytes = @{
    payload_a = (Get-Item (Join-Path $out "payload_a/deep/other.bin")).Length +
                (Get-Item (Join-Path $out "payload_a/readme.txt")).Length
    payload_b = (Get-Item (Join-Path $out "payload_b/media/cover.png")).Length +
                (Get-Item (Join-Path $out "payload_b/notes/changelog.txt")).Length
}

# ---------------------------------------------------------------------------
# One invocation
# ---------------------------------------------------------------------------

$arguments = @(
    "download", $torrents.payload_c, $torrents.payload_a, $torrents.payload_b,
    "--dir", $out,
    "-j", "$Jobs",
    "--no-tracker", "--no-dht", "--no-lsd",
    "--port", "0",
    "--report-interval", "500ms",
    "--stop-after", "$($TimeoutSeconds)s",
    "--json"
)
$command = "bit-cli $($arguments -join ' ')"
Write-Step "downloading three torrents in one invocation, no --web-seed-for"
$stdout = Join-Path $Root "run.json"
$stderr = Join-Path $Root "run.err"
$clock = [System.Diagnostics.Stopwatch]::StartNew()
$process = Start-Process -FilePath $bitCli -WorkingDirectory $repo -NoNewWindow -PassThru `
    -ArgumentList $arguments -RedirectStandardOutput $stdout -RedirectStandardError $stderr
# The run's own `--stop-after` is $TimeoutSeconds, so waiting exactly that long
# races it: a run that stops on its own deadline and writes its report is killed
# at the same instant and looks like a run that wrote nothing. The margin is
# what separates "it stopped and said so" from "we killed it".
$finished = $process.WaitForExit(($TimeoutSeconds + 30) * 1000)
$clock.Stop()
if (-not $finished) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
Stop-Server

$exitCode = if ($finished) { $process.ExitCode } else { 124 }
$report = $null
try { $report = Get-Content $stdout -Raw | ConvertFrom-Json } catch { }
if (-not $report) {
    Exit-With 2 "the run wrote no JSON report. stderr:`n$(Get-Content $stderr -Raw)"
}

# ---------------------------------------------------------------------------
# What it says
# ---------------------------------------------------------------------------

$failures = [System.Collections.ArrayList]::new()
function Require([bool]$ok, [string]$message) {
    if (-not $ok) { [void]$failures.Add($message) }
}

Require ($exitCode -eq 0) "the run exited $exitCode, not 0"

$rows = [System.Collections.ArrayList]::new()
foreach ($name in $torrents.Keys) {
    $torrent = $report.torrents | Where-Object { $_.name -eq $name } | Select-Object -First 1
    if (-not $torrent) {
        [void]$failures.Add("$name is not in the report")
        continue
    }
    $landed = Join-Path $out "$name/$($sharedPath[$name])"
    $hash = if (Test-Path $landed) { (Get-FileHash -Algorithm SHA256 $landed).Hash.ToLower() } else { $null }
    # `shared` is absent when it is empty, and `@($null)` in PowerShell is an
    # array of one null rather than an empty one, which counts as a donation
    # that never happened.
    $shared = @()
    if ($torrent.shared) { $shared = @($torrent.shared) }
    $httpBytes = 0
    foreach ($source in @($torrent.sources)) {
        if ($source -and $source.origin -ne "shared_file") { $httpBytes += $source.http_bytes }
    }
    [void]$rows.Add([ordered]@{
        torrent          = $name
        info_hash        = $torrent.info_hash
        finished         = $torrent.finished
        downloaded_bytes = $torrent.downloaded.bytes
        from_web_seeds   = $torrent.from_web_seeds.bytes
        from_peers       = $torrent.from_peers.bytes
        from_resume      = $torrent.from_resume.bytes
        http_bytes       = $httpBytes
        shared_count     = $shared.Count
        shared_from      = if ($shared.Count -gt 0) { $shared[0].from_info_hash } else { $null }
        shared_from_path = if ($shared.Count -gt 0) { $shared[0].from_path } else { $null }
        pieces_compared  = if ($shared.Count -gt 0) { $shared[0].pieces_compared } else { 0 }
        bytes_proven     = if ($shared.Count -gt 0) { $shared[0].bytes_proven.bytes } else { 0 }
        landed           = $landed
        sha256           = $hash
        hash_matches     = ($hash -eq $expected)
    })

    Require ([bool]$torrent.finished) "$name did not finish"
    Require ($hash -eq $expected) "$name's copy of the shared file does not hash equal"
}

$donor = $rows | Where-Object { $_.torrent -eq "payload_c" } | Select-Object -First 1
$takers = @($rows | Where-Object { $_.torrent -ne "payload_c" })

if ($donor) {
    # The donor is the only torrent that can reach the mirror, and it fetched
    # the shared file plus its own extra file over HTTP.
    Require ($donor.shared_count -eq 0) "the donor took a shared file from somewhere, which nothing had written yet"
    Require ($donor.http_bytes -ge $blobBytes) "the donor fetched $($donor.http_bytes) bytes over HTTP, less than the shared file's $blobBytes"
}
foreach ($taker in $takers) {
    Require ($taker.shared_count -eq 1) "$($taker.torrent) reported $($taker.shared_count) shared file(s), not 1"
    Require ($taker.shared_from -eq $donor.info_hash) "$($taker.torrent) named $($taker.shared_from) as the donor, not the torrent that fetched it"
    Require ($taker.http_bytes -eq 0) "$($taker.torrent) fetched $($taker.http_bytes) bytes over HTTP; the shared file was supposed to come off the disk"
    Require ($taker.from_web_seeds -eq $blobBytes) "$($taker.torrent) took $($taker.from_web_seeds) bytes from sources, not the shared file's $blobBytes"
    Require ($taker.from_peers -eq 0) "$($taker.torrent) charged $($taker.from_peers) bytes to peers in a run with no swarm"
    Require ($taker.from_resume -eq $placedBytes[$taker.torrent]) "$($taker.torrent) resumed $($taker.from_resume) bytes, not the $($placedBytes[$taker.torrent]) already on disk"
    Require ($taker.bytes_proven -ge $blobBytes) "$($taker.torrent) proved only $($taker.bytes_proven) bytes of the shared file"
}

$distinct = @($rows | ForEach-Object { $_.sha256 } | Where-Object { $_ } | Sort-Object -Unique)
Require ($distinct.Count -eq 1) "the shared file landed with $($distinct.Count) distinct hashes across the three output directories"

# The number this exists for: the payload crossed HTTP once, not three times.
$httpTotal = 0
foreach ($row in $rows) { $httpTotal += $row.http_bytes }
Require ($httpTotal -lt ($blobBytes * 2)) "the run pulled $httpTotal bytes over HTTP, which is more than one copy of the shared file"

# ---------------------------------------------------------------------------
# The record
# ---------------------------------------------------------------------------

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$jsonPath = Join-Path $ReportDir "shared-files-$stamp.json"
[ordered]@{
    kind           = "shared-files"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = [System.Environment]::MachineName
        os      = [System.Environment]::OSVersion.VersionString
        cpus    = [System.Environment]::ProcessorCount
    }
    parameters     = [ordered]@{
        blob_size_mib  = $BlobSizeMiB
        other_size_mib = $OtherSizeMiB
        piece_length   = "1MiB"
        profile        = $Profile
        jobs           = $Jobs
    }
    command        = $command
    mirror         = $mirror
    exit_code      = $exitCode
    elapsed_ms     = $clock.ElapsedMilliseconds
    shared_file    = [ordered]@{
        bytes  = $blobBytes
        human  = Format-Size $blobBytes
        sha256 = $expected
    }
    http_bytes_total = $httpTotal
    torrents       = @($rows)
    ok             = ($failures.Count -eq 0)
    failures       = @($failures)
    notes          = @(
        "No --web-seed-for anywhere. The bindings A and B read from are computed from the metadata: every whole piece inside the shared file has the same SHA-1 in both torrents, which is the same evidence `bit-cli files --against` reports as piece-hashes.",
        "One piece length for all three torrents, which the default fixture deliberately does not use. Two files can be compared by hash only where whole pieces cover the same bytes of each.",
        "-j $Jobs. At 1 a torrent can only read what an earlier one has already written and the order is the command line's, which is T-140. Above 1 the donor finishes while the takers are running, so the source attaches to a torrent that has already started, which is T-143.",
        "http_bytes counts only sources that are not shared_file, so it is the traffic that crossed the mirror rather than the bytes that moved."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $jsonPath -Encoding utf8NoBOM

Write-Host ""
$rows | ForEach-Object {
    [pscustomobject][ordered]@{
        torrent    = $_.torrent
        finished   = $_.finished
        "over http" = Format-Size $_.http_bytes
        "from disk" = Format-Size $_.from_web_seeds
        resumed    = Format-Size $_.from_resume
        shared     = $_.shared_count
        proven     = Format-Size $_.bytes_proven
        hash       = if ($_.sha256) { $_.sha256.Substring(0, 16) } else { "-" }
    }
} | Format-Table -AutoSize | Out-String | Write-Host

Write-Host "shared file:  $(Format-Size $blobBytes), sha256 $expected"
Write-Host "over http:    $(Format-Size $httpTotal) for the whole run"
Write-Host "distinct hashes across three output directories: $($distinct.Count)"
Write-Host "report:       $jsonPath"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-shared-files: $failure") }
    exit 1
}
Write-Host "verdict: one fetch, three copies, one hash"
exit 0
