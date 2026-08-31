# Measure what each --file-allocation method actually does to the disk.
#
# `sparse` and `prealloc` are the same size in a directory listing and are not
# the same thing on the volume. The difference is whether a half-finished
# 40 GiB torrent has reserved 40 GiB or has reserved nothing, which is the
# difference between a capacity plan that holds and one that does not. This
# script measures it: apparent size, allocated size, and the sparse flag, for
# every method, against a real download over loopback.
#
# Usage:
#   pwsh scripts/check-allocation.ps1
#   pwsh scripts/check-allocation.ps1 -PayloadSize 1GiB
#
# Exits 0 when every method behaved as documented, 1 when one did not, and 2
# when the check could not run. The record goes to
# bench/allocation-<timestamp>.json.
#
# See TODO/disk-io.md, T-012.

[CmdletBinding()]
param(
    [string]$PayloadSize = "256MiB",
    [string]$Root = ".tmp/allocation",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [int]$TimeoutSeconds = 300,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-allocation: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

function Format-Size([double]$bytes) {
    if ($bytes -ge 1GB) { return "{0:N2} GiB" -f ($bytes / 1GB) }
    if ($bytes -ge 1MB) { return "{0:N2} MiB" -f ($bytes / 1MB) }
    if ($bytes -ge 1KB) { return "{0:N2} KiB" -f ($bytes / 1KB) }
    "{0} B" -f [int64]$bytes
}

function ConvertFrom-Size([string]$text) {
    if ($text -match '^\s*([0-9]+(?:\.[0-9]+)?)\s*([A-Za-z]*)\s*$') {
        $value = [double]$Matches[1]
        $scale = switch ($Matches[2].ToLower()) {
            ''    { 1 } 'b' { 1 }
            'k'   { 1KB } 'kib' { 1KB }
            'm'   { 1MB } 'mib' { 1MB }
            'g'   { 1GB } 'gib' { 1GB }
            default { Exit-With 2 "cannot parse size '$text'" }
        }
        return [int64]($value * $scale)
    }
    Exit-With 2 "cannot parse size '$text'"
}

# ---------------------------------------------------------------------------
# How much of the volume a file actually occupies
# ---------------------------------------------------------------------------
#
# A directory listing shows the apparent size, which is the same for a sparse
# file and a preallocated one. `GetCompressedFileSize` is the allocated size,
# which is the number this script exists to read. `fsutil file layout` shows
# the same thing in more detail and needs elevation, so it is not used here.

if (-not ('BitCli.Volume' -as [type])) {
    Add-Type -Namespace BitCli -Name Volume -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError = true, CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
public static extern uint GetCompressedFileSizeW(string name, out uint high);
'@
}

function Get-AllocatedSize([string]$path) {
    $high = 0
    $low = [BitCli.Volume]::GetCompressedFileSizeW($path, [ref]$high)
    if ($low -eq 0xFFFFFFFF -and [System.Runtime.InteropServices.Marshal]::GetLastWin32Error() -ne 0) {
        return $null
    }
    ([int64]$high -shl 32) -bor [int64]$low
}

function Get-SparseFlag([string]$path) {
    $output = & fsutil sparse queryflag $path 2>&1
    if ($LASTEXITCODE -ne 0) { return $null }
    -not ($output -join ' ').Contains('NOT set as sparse')
}

# ---------------------------------------------------------------------------
# Tools and workspace
# ---------------------------------------------------------------------------

$exe = if ($IsWindows) { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
$fileserver = Join-Path $repo "target/$Profile/examples/loopback-fileserver$exe"
foreach ($required in @($bitCli, $fileserver)) {
    if (-not (Test-Path $required)) {
        Exit-With 2 "missing $required. Build it first: cargo build --$Profile --workspace --bins --examples"
    }
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

$payloadBytes = ConvertFrom-Size $PayloadSize
Write-Step "building a $(Format-Size $payloadBytes) payload"
New-Item -ItemType Directory -Force -Path (Join-Path $Root "payload") | Out-Null
$block = 1MB
$buffer = [byte[]]::new($block)
[int64]$state = 4242
for ($i = 0; $i -lt $block; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $buffer[$i] = [byte](($state -shr 16) -band 0xFF)
}
$stream = [System.IO.File]::Create((Join-Path $Root "payload/movie.bin"))
try {
    $written = 0
    while ($written -lt $payloadBytes) {
        $want = [math]::Min($block, $payloadBytes - $written)
        $stream.Write($buffer, 0, $want)
        $written += $want
    }
} finally { $stream.Dispose() }

# The torrent is built from a directory, so it is a multi-file torrent named
# `payload` and its one file lands under a directory of that name. Both paths
# below carry it. They did not until 2026-08-22, and `Test-Path` on a path that
# is not there returns a length of zero and a hash of nothing, so the script
# reported the payload as not matching the source and nothing reserved, on all
# four methods, while every download was byte for byte correct. See
# `TODO/disk-io.md`, T-190.
$torrent = Join-Path $Root "payload.torrent"
& $bitCli create (Join-Path $Root "payload") --name payload --piece-length 1MiB `
    --no-creation-date --output $torrent --force --json 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }

$serverOut = Join-Path $Root "fileserver.out"
$server = Start-Process -FilePath $fileserver -ArgumentList @("--root", $Root) `
    -WorkingDirectory $Root -NoNewWindow -PassThru `
    -RedirectStandardOutput $serverOut -RedirectStandardError (Join-Path $Root "fileserver.err")
$deadline = (Get-Date).AddSeconds(15)
$webSeed = $null
while (-not $webSeed -and (Get-Date) -lt $deadline) {
    if (Test-Path $serverOut) {
        $line = Get-Content $serverOut -TotalCount 1 -ErrorAction SilentlyContinue
        if ($line -and $line.Trim()) { $webSeed = $line.Trim() }
    }
    if (-not $webSeed) { Start-Sleep -Milliseconds 100 }
}
if (-not $webSeed) { Exit-With 2 "the file server printed no URL" }
Write-Step "web seed at $webSeed"

# ---------------------------------------------------------------------------
# Two runs per method
# ---------------------------------------------------------------------------
#
# The question this answers is "does a half-finished torrent occupy the space
# it will need", so the measurement that matters is taken before the payload
# arrives, not after. Each method therefore runs twice:
#
#   reserved   the torrent is added against a source that answers nothing, so
#              the files are created and sized and no byte of payload lands.
#              Volume free space is read either side of that, which is the
#              number a capacity plan is made from.
#   complete   the same torrent downloaded for real, and the result hashed
#              against the source, because an allocation method that loses
#              data is worse than one that reserves nothing.

$volume = (Get-Item $Root).PSDrive.Name

function Get-FreeSpace {
    (Get-PSDrive -Name $volume).Free
}

$sourceHash = (Get-FileHash -Algorithm SHA256 (Join-Path $Root "payload/movie.bin")).Hash.ToLower()
$results = [System.Collections.ArrayList]@()
$failures = [System.Collections.ArrayList]@()

function Invoke-Download($method, $outDir, $seed, $stopAfter, $label) {
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $stdout = Join-Path $Root "$label.json"
    $stderr = Join-Path $Root "$label.err"
    $arguments = @(
        "download", $torrent, "--web-seed", $seed, "--web-seed-only",
        "--dir", $outDir, "--file-allocation", $method, "--port", "0", "--json"
    )
    if ($stopAfter) { $arguments += @("--stop-after", $stopAfter) }
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $repo -NoNewWindow -PassThru `
        -ArgumentList $arguments -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $finished = $process.WaitForExit($TimeoutSeconds * 1000)
    $clock.Stop()
    if (-not $finished) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        return [pscustomobject]@{ exit_code = $null; elapsed_ms = $clock.ElapsedMilliseconds; stderr = $stderr }
    }
    [pscustomobject]@{ exit_code = $process.ExitCode; elapsed_ms = $clock.ElapsedMilliseconds; stderr = $stderr }
}

foreach ($method in @("none", "sparse", "prealloc", "falloc")) {
    Write-Step "--file-allocation $method"

    # 1. Reserved and empty. Nothing answers on port 1, so the add sizes the
    #    files and the run then hits its deadline with no payload.
    $reservedDir = Join-Path $Root "reserved-$method"
    [System.GC]::Collect()
    $before = Get-FreeSpace
    $reserved = Invoke-Download $method $reservedDir "http://127.0.0.1:1/" "6s" "reserve-$method"
    $after = Get-FreeSpace
    $reservedFile = Join-Path $reservedDir "payload/movie.bin"
    $reservedApparent = if (Test-Path $reservedFile) { (Get-Item $reservedFile).Length } else { 0 }
    $reservedAllocated = if (Test-Path $reservedFile) { Get-AllocatedSize $reservedFile } else { $null }
    $reservedSparse = if (Test-Path $reservedFile) { Get-SparseFlag $reservedFile } else { $null }
    $freeDelta = $before - $after
    $warnings = @(Get-Content $reserved.stderr -ErrorAction SilentlyContinue |
        Where-Object { $_ -match 'warning:' } | ForEach-Object { $_.Trim() })

    # 2. Downloaded for real, and checked.
    $completeDir = Join-Path $Root "complete-$method"
    $complete = Invoke-Download $method $completeDir $webSeed $null "complete-$method"
    $completeFile = Join-Path $completeDir "payload/movie.bin"
    $completeHash = if (Test-Path $completeFile) {
        (Get-FileHash -Algorithm SHA256 $completeFile).Hash.ToLower()
    } else { $null }

    if ($complete.exit_code -ne 0) {
        [void]$failures.Add("$method : the download exited $($complete.exit_code)")
    }
    if ($completeHash -ne $sourceHash) {
        [void]$failures.Add("$method : the payload does not match the source")
    }
    if ($reservedApparent -ne $payloadBytes) {
        [void]$failures.Add("$method : reserved $reservedApparent bytes, not $payloadBytes")
    }
    if ($method -eq "sparse" -and $reservedSparse -eq $false) {
        [void]$failures.Add("sparse: the file is not marked sparse")
    }
    if ($method -ne "sparse" -and $method -ne "none" -and $freeDelta -lt ($payloadBytes / 2)) {
        [void]$failures.Add("$method : reserving freed only $freeDelta bytes of volume space, so it did not reserve")
    }

    [void]$results.Add([pscustomobject]@{
        method = $method
        reserved_apparent_bytes = $reservedApparent
        reserved_apparent_human = Format-Size $reservedApparent
        reserved_allocated_bytes = $reservedAllocated
        reserved_allocated_human = if ($null -ne $reservedAllocated) { Format-Size $reservedAllocated } else { $null }
        reserved_sparse_flag = $reservedSparse
        volume_free_delta_bytes = $freeDelta
        volume_free_delta_human = Format-Size ([math]::Max(0, $freeDelta))
        reserve_ms = $reserved.elapsed_ms
        download_ms = $complete.elapsed_ms
        payload_matches_source = ($completeHash -eq $sourceHash)
        warnings = $warnings
    })
    Write-Step ("  reserved {0}, allocated {1}, sparse {2}, volume gave up {3}, payload {4}" -f `
        (Format-Size $reservedApparent),
        $(if ($null -ne $reservedAllocated) { Format-Size $reservedAllocated } else { 'unknown' }),
        $reservedSparse,
        (Format-Size ([math]::Max(0, $freeDelta))),
        $(if ($completeHash -eq $sourceHash) { 'matches' } else { 'DOES NOT MATCH' }))
}

if (-not $server.HasExited) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue }

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "allocation-$stamp.json"
[ordered]@{
    schema_version = "1"
    kind = "allocation_check"
    todo = "T-012"
    generated_at = Get-Timestamp
    payload_bytes = $payloadBytes
    payload_human = Format-Size $payloadBytes
    volume = $volume
    filesystem = (Get-Volume -DriveLetter $volume -ErrorAction SilentlyContinue).FileSystemType
    source_sha256 = $sourceHash
    methods = @($results)
    failures = @($failures)
    notes = @(
        "The measurement that separates the methods is taken before any payload arrives: the torrent is added against a source that answers nothing, so the files are created and sized and nothing is downloaded. Volume free space either side of that is the number a capacity plan is made from.",
        "GetCompressedFileSize reports zero for a sparse NTFS file even when it holds data, so it is recorded and not asserted on. `fsutil sparse queryflag` is the reliable per-file signal and volume free space is the reliable per-volume one.",
        "Every method is also run to completion and the result hashed against the source, because an allocation method that loses data is worse than one that reserves nothing."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "payload: $(Format-Size $payloadBytes)  on $volume ($((Get-Volume -DriveLetter $volume -ErrorAction SilentlyContinue).FileSystemType))"
Write-Host "report:  $reportPath"
Write-Host ""
$results | Format-Table -Property method, reserved_apparent_human, reserved_allocated_human,
    reserved_sparse_flag, volume_free_delta_human, reserve_ms, payload_matches_source -AutoSize |
    Out-String | Write-Host

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-allocation: $failure") }
    exit 1
}
exit 0
