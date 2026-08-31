# Measure how many payload handles a seed actually holds.
#
# A torrent with twenty thousand files is one torrent, and ten of them in one
# process is two hundred thousand descriptors if every file stays open. This
# script builds a many-file torrent, seeds it at several `--max-open-files`
# caps, and samples the process handle count while it runs.
#
# The number that matters is the difference between two caps, not the absolute
# count: a process holds handles for threads, sockets, and libraries as well as
# for payload, and only the payload part is what the cap controls. Two caps
# whose handle counts differ by exactly the difference between them is the
# proof.
#
# Usage:
#   pwsh scripts/check-handles.ps1
#   pwsh scripts/check-handles.ps1 -Files 2000 -Caps 8,64,256
#
# Exits 0 when the caps hold, 1 when one does not, and 2 when the check could
# not run. The record goes to bench/handles-<timestamp>.json.
#
# See TODO/disk-io.md, T-011.

[CmdletBinding()]
param(
    [int]$Files = 300,
    [int]$FileBytes = 16384,
    [int[]]$Caps = @(8, 64, 128),
    [string]$Root = ".tmp/handles",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [string]$SeedFor = "12s",
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-handles: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

$exe = if ($IsWindows) { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --workspace --bins --examples"
}
if ($Caps.Count -lt 2) {
    Exit-With 2 "-Caps needs at least two values: the measurement is the difference between them."
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root }
New-Item -ItemType Directory -Force -Path (Join-Path $Root "many") | Out-Null
$Root = (Resolve-Path $Root).Path

Write-Step "building $Files files of $FileBytes bytes"
$buffer = [byte[]]::new($FileBytes)
[int64]$state = 99
for ($i = 0; $i -lt $FileBytes; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $buffer[$i] = [byte](($state -shr 16) -band 0xFF)
}
for ($i = 1; $i -le $Files; $i++) {
    $name = "f{0:D5}.bin" -f $i
    [System.IO.File]::WriteAllBytes((Join-Path $Root "many/$name"), $buffer)
}

$torrent = Join-Path $Root "many.torrent"
& $bitCli create (Join-Path $Root "many") --piece-length 16KiB --no-creation-date `
    --output $torrent --force --json 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }

# ---------------------------------------------------------------------------
# One seed per cap, sampling the handle count while it runs
# ---------------------------------------------------------------------------

$results = [System.Collections.ArrayList]@()
$failures = [System.Collections.ArrayList]@()

foreach ($cap in $Caps) {
    Write-Step "--max-open-files $cap"
    $stdout = Join-Path $Root "seed-$cap.json"
    $stderr = Join-Path $Root "seed-$cap.err"
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $repo -NoNewWindow -PassThru `
        -ArgumentList @(
            "seed", $torrent, "--data", $Root,
            "--max-open-files", "$cap", "--port", "0",
            "--no-dht", "--no-lsd", "--no-tracker",
            "--stop-after", $SeedFor, "--json"
        ) -RedirectStandardOutput $stdout -RedirectStandardError $stderr

    $peak = 0
    $samples = 0
    while (-not $process.HasExited) {
        try {
            $process.Refresh()
            $peak = [Math]::Max($peak, $process.HandleCount)
            $samples++
        } catch {}
        Start-Sleep -Milliseconds 200
    }

    if (-not (Test-Path $stdout)) {
        [void]$failures.Add("cap $cap wrote no report")
        continue
    }
    $doc = Get-Content $stdout -Raw | ConvertFrom-Json
    if (-not $doc.complete) {
        [void]$failures.Add("cap $cap : the seed reported an incomplete payload")
    }
    [void]$results.Add([pscustomobject]@{
        cap = $cap
        peak_process_handles = $peak
        reported_open_handles = $doc.process.open_handles
        samples = $samples
        complete = [bool]$doc.complete
        have_bytes = $doc.have.bytes
    })
    Write-Step "  peak $peak process handles over $samples samples, complete $($doc.complete)"
}

# ---------------------------------------------------------------------------
# The cap is what the difference has to equal
# ---------------------------------------------------------------------------
#
# Every other handle the process holds is the same whatever the cap is, so it
# cancels. What is left is one handle per payload file the cap allows, and a
# tolerance because a live process opens and closes sockets while it runs.

$tolerance = 8
$ordered = @($results | Sort-Object cap)
for ($i = 1; $i -lt $ordered.Count; $i++) {
    $low = $ordered[$i - 1]
    $high = $ordered[$i]
    $expected = $high.cap - $low.cap
    $observed = $high.peak_process_handles - $low.peak_process_handles
    $off = [math]::Abs($observed - $expected)
    if ($off -gt $tolerance) {
        [void]$failures.Add(
            "caps $($low.cap) and $($high.cap) differ by $observed handles, expected $expected (tolerance $tolerance)")
    }
}
if ($ordered.Count -gt 0) {
    $smallest = $ordered[0]
    if ($smallest.peak_process_handles -ge $Files) {
        [void]$failures.Add(
            "cap $($smallest.cap) held $($smallest.peak_process_handles) handles for $Files files, which is not a cap")
    }
}

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "handles-$stamp.json"
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null
$reportPath = Join-Path $ReportDir "handles-$stamp.json"

[ordered]@{
    schema_version = "1"
    kind = "handle_cap_check"
    todo = "T-011"
    generated_at = Get-Timestamp
    files = $Files
    file_bytes = $FileBytes
    caps = @($Caps)
    seed_for = $SeedFor
    tolerance = $tolerance
    results = @($ordered)
    failures = @($failures)
    notes = @(
        "The absolute handle count includes everything the process holds that is not payload: threads, sockets, and libraries. That part is the same whatever the cap is, so the measurement is the difference between two caps, which has to equal the difference between the caps themselves."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "files:  $Files"
Write-Host "report: $reportPath"
Write-Host ""
$ordered | Format-Table -Property cap, peak_process_handles, complete -AutoSize | Out-String | Write-Host
for ($i = 1; $i -lt $ordered.Count; $i++) {
    $low = $ordered[$i - 1]
    $high = $ordered[$i]
    Write-Host ("cap {0} to {1}: {2} more handles, cap grew by {3}" -f `
        $low.cap, $high.cap,
        ($high.peak_process_handles - $low.peak_process_handles),
        ($high.cap - $low.cap))
}

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-handles: $failure") }
    exit 1
}
exit 0
