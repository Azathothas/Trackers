# Does a soak report say that its slope is fitted across a step?
#
# `scripts/soak.ps1` reported one least squares fit per series and nothing
# else, and on 2026-08-23T09:01:32Z that produced a number that is true and
# describes nothing: `rss_bytes` 3.708 MiB/h at r squared 0.717, against a
# ceiling of 4. The line is fitted across a single eight second interval where
# resident memory rose 11.61 MiB and never came back; either side of it the
# slope is 1.02 and 1.69. Nothing in a slope and an r squared can say that, so
# a reader with only those two numbers reads a step as growth, and the entry
# that said otherwise had to be computed by hand.
#
# `soak.ps1 -ReadCsv` now re-reads a finished run through the same `Get-Slope`
# a live run uses, and reports the largest single-interval change each way with
# the hour it happened at. This checks that it does, on the run that has the
# step and on a series that does not, because a column that always reports
# something large says as little as no column at all.
#
# Every number asserted here is read from `soak.ps1 -ReadJson`, which is that
# script's own output. A check that recomputes the fit itself passes when the
# script it is checking is wrong, and it was written that way first.
#
# Five cases:
#
#   the mode   `-ReadCsv` runs, exits 0, and prints the step columns. It sits
#              above the trap and above every `Start-Child` in `soak.ps1`, and
#              it has to: a read-only mode placed below them is a soak with a
#              report on the end of it, which is where the block was written.
#   step       the committed run of 2026-08-23T09:01:32Z. Its `rss_bytes` step
#              up must be over 8 MiB, must land between t+1.0 and t+1.3 hours,
#              and must be more than two hours' worth of the fitted slope,
#              which is the shape that says the fit spans a discontinuity.
#   no step    a generated CSV that rises by a fixed amount every sample and
#              nothing else. Four times the slope, no step: its largest
#              single-interval rise must be the per-sample increment and no
#              more.
#   truncated  that same ramp with a zero fill on the end, which is what NTFS
#              leaves when a soak is killed mid-append. Its last sample must be
#              the last one written rather than a row of zeros. T-231.
#   stalled    the committed run of 2026-08-23T15:47:16Z, whose workload
#              stopped at t+4653s and which reported a pass over six hours
#              anyway. It must not read as having measured its workload, and
#              the healthy six hour run must. T-232.
#
# Nothing here starts a soak or waits on a clock. Every fixture is a CSV and a
# read, so this runs in seconds and belongs in CI.
#
# Usage:
#   pwsh scripts/check-soak-fit.ps1
#   pwsh scripts/check-soak-fit.ps1 -Json bench/soak-fit.json
#
# Exits 0 when every case holds, 1 when one does not, and 2 when the check
# could not run.
#
# See TODO/memory.md, T-224, T-231 and T-232.

[CmdletBinding()]
param(
    # The committed run that carries the step. Named rather than discovered:
    # a check that picks up whichever soak ran last measures a different thing
    # every session.
    [string]$WithStep = "bench/soak-20260823T090132499Z.csv",
    # The run whose workload stopped 1.29 hours into its six, and which
    # reported "every named ceiling held over 6 hours" anyway. Named for the
    # same reason as the one above. See TODO/memory.md, T-232.
    [string]$WithStalledWorkload = "bench/soak-20260823T154716064Z.csv",
    [string]$Root = ".tmp/soak-fit",
    [string]$Json
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$soak = Join-Path $repo "scripts/soak.ps1"

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-soak-fit: $message")
    exit $code
}

if (-not (Test-Path $soak)) { Exit-With 2 "no scripts/soak.ps1 at $soak" }
if (-not (Test-Path (Join-Path $repo $WithStep))) { Exit-With 2 "no CSV at $WithStep" }
if (-not (Test-Path (Join-Path $repo $WithStalledWorkload))) { Exit-With 2 "no CSV at $WithStalledWorkload" }

$workDir = Join-Path $repo $Root
New-Item -ItemType Directory -Force -Path $workDir | Out-Null

$failures = [System.Collections.ArrayList]::new()
$cases = [System.Collections.ArrayList]::new()

# Run `soak.ps1 -ReadCsv` on one CSV and hand back what it printed and what it
# wrote. The relative path is what `soak.ps1` joins onto the repository root,
# so both sides agree on where the file is.
function Invoke-Read($csvRelative, $tag) {
    $jsonRelative = "$Root/$tag.json"
    $printed = & pwsh -NoProfile -File $soak -ReadCsv (Join-Path $repo $csvRelative) -ReadJson $jsonRelative 2>&1 | Out-String
    $code = $LASTEXITCODE
    $written = Join-Path $repo $jsonRelative
    $fits = if (Test-Path $written) { Get-Content $written -Raw | ConvertFrom-Json } else { $null }
    [pscustomobject]@{ printed = $printed; exit = $code; fits = $fits }
}

# ---------------------------------------------------------------------------
# The mode itself, and the run that has the step
# ---------------------------------------------------------------------------

$stepRun = Invoke-Read $WithStep "with-step"
if ($stepRun.exit -ne 0) { [void]$failures.Add("soak.ps1 -ReadCsv exited $($stepRun.exit)") }
foreach ($heading in @("step up", "at h", "step down", "rss_bytes")) {
    if ($stepRun.printed -notmatch [regex]::Escape($heading)) {
        [void]$failures.Add("the -ReadCsv table has no '$heading'")
    }
}
if (-not $stepRun.fits) { Exit-With 1 "soak.ps1 -ReadJson wrote nothing for $WithStep" }
[void]$cases.Add([ordered]@{
        case   = "read_csv_runs_and_prints_the_step_columns"
        exit   = $stepRun.exit
        judged = $true
        ok     = ($stepRun.exit -eq 0)
    })

$rss = $stepRun.fits.slopes.rss_bytes
if (-not $rss) { Exit-With 1 "the report for $WithStep carries no rss_bytes fit" }
$stepMiB = $rss.largest_rise / 1MB
$slopeMiB = $rss.slope_per_hour / 1MB
# How many hours of the fitted trend arrived in one sampling interval. A slope
# is a description of a run; a single interval carrying hours of it means the
# description is wrong about the mechanism even where it is right about the
# total.
$hoursOfTrend = if ($slopeMiB -gt 0) { $stepMiB / $slopeMiB } else { 0 }

if ($stepMiB -le 8) {
    [void]$failures.Add("$WithStep rss_bytes largest rise is $([math]::Round($stepMiB, 2)) MiB, expected over 8")
}
if ($rss.largest_rise_hours -lt 1.0 -or $rss.largest_rise_hours -gt 1.3) {
    [void]$failures.Add("$WithStep rss_bytes step is at t+$($rss.largest_rise_hours) h, expected between 1.0 and 1.3")
}
if ($hoursOfTrend -le 2) {
    [void]$failures.Add("$WithStep step is $([math]::Round($hoursOfTrend, 2)) hours of the fitted slope, expected over 2")
}
[void]$cases.Add([ordered]@{
        case           = "a_run_with_a_step_reports_it"
        csv            = $WithStep
        samples        = $stepRun.fits.samples
        slope_mib_hour = [math]::Round($slopeMiB, 3)
        r_squared      = $rss.r_squared
        largest_rise   = [math]::Round($stepMiB, 2)
        rise_hours     = $rss.largest_rise_hours
        hours_of_trend = [math]::Round($hoursOfTrend, 2)
        judged         = $true
        ok             = ($stepMiB -gt 8) -and ($hoursOfTrend -gt 2)
    })

# ---------------------------------------------------------------------------
# A series with a bigger slope and no step
# ---------------------------------------------------------------------------
#
# Generated rather than found, because the point is a series whose per-sample
# increment is known exactly. It rises 128 KiB every thirty seconds, which is
# 15 MiB/h, four times the run above, and its largest single-interval rise is
# still one increment. A column that reported something large here would be
# reporting the slope a second time.

$rampRelative = "$Root/ramp.csv"
$increment = 128 * 1024
$lines = [System.Collections.ArrayList]::new()
[void]$lines.Add("sample,iso,elapsed_s,rss_bytes,peak_rss_bytes,handles,threads,cpu_ms,tcp_total,tcp_established,tcp_listen,tcp_close_wait,tcp_time_wait,tcp_other,leech_completed,leech_failed,churn_runs")
for ($i = 0; $i -lt 400; $i++) {
    $rss = 14000000 + ($i * $increment)
    [void]$lines.Add("$i,2026-01-01T00:00:00.000Z,$($i * 30),$rss,$rss,180,30,0,1,1,1,0,0,0,0,0,0")
}
Set-Content -Path (Join-Path $repo $rampRelative) -Value $lines -Encoding utf8

$rampRun = Invoke-Read $rampRelative "ramp"
if (-not $rampRun.fits) { Exit-With 1 "soak.ps1 -ReadJson wrote nothing for the generated ramp" }
$rampRss = $rampRun.fits.slopes.rss_bytes
if ($rampRss.largest_rise -gt $increment) {
    [void]$failures.Add("a series with no step reported a rise of $($rampRss.largest_rise) bytes, expected no more than $increment")
}
if ($rampRss.slope_per_hour / 1MB -le $slopeMiB) {
    [void]$failures.Add("the ramp's slope is not above the stepped run's, so this case does not separate the two")
}
[void]$cases.Add([ordered]@{
        case           = "a_run_with_no_step_reports_none"
        samples        = $rampRun.fits.samples
        slope_mib_hour = [math]::Round($rampRss.slope_per_hour / 1MB, 3)
        r_squared      = $rampRss.r_squared
        largest_rise   = $rampRss.largest_rise
        increment      = $increment
        judged         = $true
        ok             = ($rampRss.largest_rise -le $increment)
    })

# ---------------------------------------------------------------------------
# A file a killed run left behind
# ---------------------------------------------------------------------------
#
# NTFS flushes a file's size before its bytes, so a soak killed while
# appending leaves the tail zero filled. `Import-Csv` reads that as one more
# record of empty strings, `[double]""` is 0, and the fit then runs through a
# final sample of zeros. `bench/soak-20260821T012428252Z.csv` carried 176 such
# bytes and read as `last 0.00 MiB` for every series over "0 hours", with a
# largest fall of -20.75 MiB that nothing measured and a fall in
# `peak_rss_bytes`, which is a high-water mark and cannot fall at all.
#
# The fixture is the ramp above with a zero fill on the end, so the expected
# numbers are known exactly: the last real sample is the last one written.

$truncatedRelative = "$Root/truncated.csv"
$rampBytes = [System.IO.File]::ReadAllBytes((Join-Path $repo $rampRelative))
$fill = [byte[]]::new(176)
[System.IO.File]::WriteAllBytes((Join-Path $repo $truncatedRelative), $rampBytes + $fill)

$truncatedRun = Invoke-Read $truncatedRelative "truncated"
if (-not $truncatedRun.fits) { Exit-With 1 "soak.ps1 -ReadJson wrote nothing for the truncated fixture" }
$truncatedRss = $truncatedRun.fits.slopes.rss_bytes
$lastReal = 14000000 + (399 * $increment)

if ($truncatedRun.fits.dropped_rows -lt 1) {
    [void]$failures.Add("a truncated CSV reported $($truncatedRun.fits.dropped_rows) dropped row(s), expected at least 1")
}
if ($truncatedRss.last -ne $lastReal) {
    [void]$failures.Add("a truncated CSV reported last $($truncatedRss.last), expected $lastReal, which is the last row that is a row")
}
if ($truncatedRss.samples -ne $rampRss.samples) {
    [void]$failures.Add("a truncated CSV fitted $($truncatedRss.samples) samples against the same file's $($rampRss.samples) without the fill")
}
if ($truncatedRun.printed -notmatch 'truncated:') {
    [void]$failures.Add("the -ReadCsv output does not say the file was truncated")
}
[void]$cases.Add([ordered]@{
        case         = "a_truncated_csv_is_not_read_as_a_sample_of_zeros"
        samples      = $truncatedRss.samples
        dropped_rows = $truncatedRun.fits.dropped_rows
        last         = $truncatedRss.last
        expected     = $lastReal
        judged       = $true
        ok           = ($truncatedRss.last -eq $lastReal) -and ($truncatedRun.fits.dropped_rows -ge 1)
    })

# ---------------------------------------------------------------------------
# A run whose workload stopped
# ---------------------------------------------------------------------------
#
# Every ceiling this script's subject is judged against is a statement about
# the seeder, and a seeder nobody is talking to holds all of them. The run of
# 2026-08-23T15:47:16Z stopped completing leech cycles at t+4653s and spent the
# remaining 4.7 hours alive, listening, and using 47 milliseconds of CPU. Its
# report says "every named ceiling held over 6 hours" with an empty failures
# list, and every number in it is true.
#
# Two runs, because a check that only sees the stalled one passes against a
# script that calls every run stalled. The healthy six hour run read at the top
# is the control: same length, same workload, 1,360 cycles and none failed.

$stalledRun = Invoke-Read $WithStalledWorkload "stalled"
if (-not $stalledRun.fits) { Exit-With 1 "soak.ps1 -ReadJson wrote nothing for $WithStalledWorkload" }
$stalled = $stalledRun.fits.workload
$healthy = $stepRun.fits.workload
if (-not $stalled -or -not $healthy) { Exit-With 1 "the -ReadJson report carries no workload block" }

if ($stalled.measured_its_workload) {
    [void]$failures.Add("$WithStalledWorkload reads as having measured its workload, and it failed $($stalled.leech_failed) of $($stalled.leech_failed + $stalled.leech_completed) cycles")
}
if ($stalled.leech_failed_percent -le 50) {
    [void]$failures.Add("$WithStalledWorkload reports $($stalled.leech_failed_percent) percent failed, expected over 50")
}
# The stop is at a wall clock inside the run rather than at its end, which is
# what separates "the workload stopped" from "the run ended".
if ($stalled.last_progress_s -lt 4000 -or $stalled.last_progress_s -gt 5200) {
    [void]$failures.Add("$WithStalledWorkload puts its last completed cycle at t+$($stalled.last_progress_s)s, expected between 4000 and 5200")
}
if ($stalledRun.printed -notmatch 'workload:') {
    [void]$failures.Add("the -ReadCsv output does not carry a workload line")
}
if (-not $healthy.measured_its_workload) {
    [void]$failures.Add("$WithStep reads as not having measured its workload, and it failed $($healthy.leech_failed) cycles")
}
[void]$cases.Add([ordered]@{
        case          = "a_run_whose_workload_stopped_does_not_read_as_a_pass"
        stalled_csv   = $WithStalledWorkload
        failed_percent = $stalled.leech_failed_percent
        last_progress_s = $stalled.last_progress_s
        control_failed = $healthy.leech_failed
        judged        = $true
        ok            = (-not $stalled.measured_its_workload) -and $healthy.measured_its_workload
    })

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

$report = [ordered]@{
    kind         = "soak_fit"
    generated_at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    with_step    = $WithStep
    cases        = @($cases)
    failures     = @($failures)
}
if ($Json) {
    $jsonPath = Join-Path $repo $Json
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $jsonPath) | Out-Null
    $report | ConvertTo-Json -Depth 6 | Set-Content -Path $jsonPath -Encoding utf8
    Write-Host "check-soak-fit: wrote $Json"
}

@($cases) | ForEach-Object { [pscustomobject]$_ } | Format-Table -AutoSize | Out-String | Write-Host

Remove-Item -Recurse -Force $workDir -ErrorAction SilentlyContinue

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-soak-fit: $failure") }
    exit 1
}
Write-Host "check-soak-fit: every case holds"
exit 0
