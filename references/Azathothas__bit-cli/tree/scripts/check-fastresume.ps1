# Whether a second seed of the same payload still re-hashes it.
#
# This is the acceptance for `TODO/disk-io.md` T-016. Seeding hash-checks the
# whole payload on every add, on every invocation, and never offers to skip it.
# Measured here: **0.32 s for 512 MiB**, about 1.6 GiB/s, so a 40 GiB seed
# spends about **25 seconds** of disk read before it announces anything.
#
# The entry read 6,087 ms for that same 512 MiB and inferred eight minutes for
# 40 GiB. Most of those six seconds was `--exit-when-idle 1s` waiting for a
# peer that never came, which is why this script stops at `--announce-only`
# instead. The correction is under the entry.
#
# `--fastresume` keeps the verified bitfield in a cache and reuses it. The
# entry's acceptance asks for two things and this measures both: a documented
# cache location, and that a stale cache is detected and discarded.
#
# Four runs against one payload:
#
#   cold        --fastresume with an empty cache. It hashes everything and
#               writes the cache. The baseline the two below are read against.
#   warm        --fastresume again, nothing touched. It must be faster, and
#               the cache files must still be there.
#   stale       one byte of the payload rewritten, which changes the file's
#               length or its modification time. The cache must be rejected
#               and the run must take about as long as `cold` did.
#   no_flag     no --fastresume at all, with a valid cache sitting there. It
#               must still hash everything: an opt-in flag that works when it
#               was not passed is not opt-in.
#
# `stale` is the case that matters most and it is the one an implementation
# gets wrong quietly. A resume cache that is used when it should not be serves
# bytes that do not match the torrent, and the peer on the other end finds out
# rather than this process.
#
# Usage:
#   pwsh scripts/check-fastresume.ps1
#   pwsh scripts/check-fastresume.ps1 -PayloadMiB 1024 -MinSaving 0.3
#
# Exits 0 when every case holds, 1 when one does not, 2 when it could not run.
# The record goes to bench/fastresume-<timestamp>.json.
#
# See TODO/disk-io.md, T-016.

[CmdletBinding()]
param(
    # Big enough that the hashing is visible over the fixed cost of a run.
    # 512 MiB is about a third of a second of hashing here, against a two
    # second settle that both runs pay, which is why the judging below is a
    # difference and not a ratio.
    [int]$PayloadMiB = 512,
    # How many seconds the warm run has to save over the cold one. A
    # difference rather than a ratio: see the judging at the bottom. 512 MiB
    # hashes in about a third of a second here, so this is half of that and
    # wants raising with -PayloadMiB.
    [double]$MinSaving = 0.15,
    [string]$Root = ".tmp/fastresume",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-fastresume: $message")
    exit $code
}

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
# A payload worth hashing
# ---------------------------------------------------------------------------

Write-Step "building a $PayloadMiB MiB payload"
$payloadDir = Join-Path $Root "data"
New-Item -ItemType Directory -Force -Path $payloadDir | Out-Null
$payload = Join-Path $payloadDir "big.bin"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 7
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}
$stream = [System.IO.File]::Create($payload)
try { for ($i = 0; $i -lt $PayloadMiB; $i++) { $stream.Write($block, 0, $block.Length) } }
finally { $stream.Dispose() }

$torrent = Join-Path $Root "p.torrent"
$create = Start-Process -FilePath $bitCli -ArgumentList @(
    "create", $payload, "--piece-length", "1MiB", "--no-creation-date",
    "--output", $torrent, "--force", "--json"
) -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $Root "create.out") `
    -RedirectStandardError (Join-Path $Root "create.err")
$create.WaitForExit(600000) | Out-Null
if ($create.ExitCode -ne 0) { Exit-With 2 "bit-cli create exited $($create.ExitCode)" }
$infoHash = (Get-Content (Join-Path $Root "create.out") -Raw | ConvertFrom-Json).info_hash
Write-Step "torrent $infoHash"

$cacheDir = Join-Path $payloadDir ".bit-cli-resume"

function Invoke-Seed([string]$label, [bool]$fastresume) {
    # `--announce-only` rather than a seeding run. It goes live, reports, and
    # stops, so the wall clock is process start plus the initial check plus a
    # fixed two second settle. `--exit-when-idle 1s` was tried first and hid
    # the whole measurement: five seconds of waiting for a peer that never
    # comes swamped the difference between hashing 512 MiB and not.
    $arguments = @(
        "seed", $torrent, "--data", $payloadDir,
        "--no-dht", "--no-lsd", "--no-tracker", "--port", "0",
        "--announce-only", "--jsonl"
    )
    if ($fastresume) { $arguments += "--fastresume" }
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $run = Start-Process -FilePath $bitCli -ArgumentList $arguments -PassThru -NoNewWindow `
        -RedirectStandardOutput (Join-Path $Root "$label.out") `
        -RedirectStandardError (Join-Path $Root "$label.err")
    $finished = $run.WaitForExit(600000)
    $clock.Stop()
    if (-not $finished) { Stop-Process -Id $run.Id -Force -ErrorAction SilentlyContinue }

    # What the run says it holds. This is the assertion that matters far more
    # than the clock: a resume cache that is wrong makes a seeder claim pieces
    # it does not have, and the peer on the other end is what finds out.
    #
    $complete = $null
    $have = $null
    $total = $null
    foreach ($line in (Get-Content (Join-Path $Root "$label.out") -ErrorAction SilentlyContinue)) {
        $event = $null
        try { $event = $line | ConvertFrom-Json } catch { continue }
        if ($null -ne $event.complete) {
            $complete = $event.complete
            $have = $event.have.bytes
            $total = $event.total.bytes
        }
    }

    [pscustomobject][ordered]@{
        case       = $label
        fastresume = $fastresume
        exit_code  = if ($finished) { $run.ExitCode } else { $null }
        elapsed_s  = [math]::Round($clock.Elapsed.TotalSeconds, 3)
        complete   = $complete
        have       = $have
        total      = $total
        cached     = (Test-Path (Join-Path $cacheDir "$infoHash.bitv"))
    }
}

$rows = @()
$failures = @()

Write-Step "cold: an empty cache"
$cold = Invoke-Seed "cold" $true
$rows += $cold
if (-not $cold.cached) { $failures += "cold left no cache at $cacheDir" }

Write-Step "warm: the same payload again"
$warm = Invoke-Seed "warm" $true
$rows += $warm

Write-Step "stale: one byte of the payload rewritten"
$fs = [System.IO.File]::Open($payload, "Open", "Write")
try {
    $fs.Seek(1024, "Begin") | Out-Null
    $fs.WriteByte(0xAB)
}
finally { $fs.Dispose() }
$stale = Invoke-Seed "stale" $true
$rows += $stale

Write-Step "no_flag: a valid cache and no --fastresume"
$refresh = Invoke-Seed "refresh" $true
$rows += $refresh
$noFlag = Invoke-Seed "no_flag" $false
$rows += $noFlag

# What each run has to say it holds.
#
# `cold` and `warm` run against an untouched payload and must both be complete.
# The three after the byte is rewritten must all report **incomplete**, and
# that is the assertion the whole entry rests on: a run that trusted a stale
# cache would say it holds the piece that changed, announce it, and serve
# something that does not hash to what the torrent says. The peer on the other
# end would be what found out.
foreach ($row in $rows) {
    $expected = ($row.case -in @("cold", "warm"))
    if ($row.complete -ne $expected) {
        $failures += ("{0} reported complete={1}, expected {2}: it holds {3} of {4} bytes" -f `
                $row.case, $row.complete, $expected, $row.have, $row.total)
    }
}

# And how long each took.
#
# **A difference and not a ratio**, because most of each run is a fixed cost
# this does not care about: `--announce-only` settles for two seconds whatever
# happened before it. Two runs that differ only in whether they hashed the
# payload differ by exactly the hashing, and the settle cancels. A ratio over
# the whole run would be 1.16 for a check that was skipped entirely, which
# says nothing.
#
# The saving is a property of the payload and the disk, so `-MinSaving` scales
# with `-PayloadMiB`. 512 MiB hashes in about a third of a second here.
$saving = [math]::Round($cold.elapsed_s - $warm.elapsed_s, 3)
if ($saving -lt $MinSaving) {
    $failures += ("warm took {0:N2}s against a {1:N2}s cold run, saving {2:N2}s, under the {3:N2}s this asserts" -f `
            $warm.elapsed_s, $cold.elapsed_s, $saving, $MinSaving)
}
# The stale run hashed again. Read against `refresh`, which is the warm run
# over the cache `stale` rewrote, so the two differ only in whether a check
# happened.
$staleCost = [math]::Round($stale.elapsed_s - $refresh.elapsed_s, 3)
if ($staleCost -lt $MinSaving) {
    $failures += ("stale took {0:N2}s against the {1:N2}s warm run after it, so the changed payload was served from the cache" -f `
            $stale.elapsed_s, $refresh.elapsed_s)
}
$noFlagCost = [math]::Round($noFlag.elapsed_s - $refresh.elapsed_s, 3)
if ($noFlagCost -lt $MinSaving) {
    $failures += ("no_flag took {0:N2}s against the {1:N2}s warm run, so the cache was used without the flag" -f `
            $noFlag.elapsed_s, $refresh.elapsed_s)
}

$verdict = if ($failures.Count -eq 0) { "pass" } else { "fail" }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "fastresume-$stamp.json"
[pscustomobject][ordered]@{
    kind           = "fastresume"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    }
    parameters     = [ordered]@{
        payload_mib = $PayloadMiB
        min_saving  = $MinSaving
        cache_dir   = $cacheDir
        info_hash   = $infoHash
        profile     = $Profile
    }
    cases          = @($rows)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "The cache lives beside the payload, at <data>/.bit-cli-resume/<info hash>.bitv, with a .meta sidecar naming every file's length and modification time.",
        "stale is the case that matters: a cache used when it should not be serves bytes that do not match the torrent, and the peer on the other end finds out rather than this process.",
        "The durations are judged as differences and not as ratios: most of each run is a fixed two second settle, so two runs that differ only in whether they hashed the payload differ by exactly the hashing.",
        "stale is read against the run after it rather than the run before it, because the stale run rewrites the cache it rejected, so the run after it is the warm one for that payload.",
        "no_flag exists because an opt-in flag that works when it was not passed is not opt-in."
    )
} | ConvertTo-Json -Depth 10 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
$rows | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
Write-Host "cache:   $cacheDir"
Write-Host "report:  $reportPath"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-fastresume: $failure") }
    exit 1
}
exit 0
