# check-changelog.ps1 - does CHANGELOG.md still obey the four rules that a
# machine can hold?
#
# ⭐ THE TWIN OF check-changelog.sh. Same schema, same exit codes, same rules.
#
# The defect this exists to catch is a changelog that stopped being orderable.
# docs/conventions/docs.md states four rules and says in as many words that each
# is mechanical enough to check. Nothing checked them, which is the exact shape
# this template warns about: a rule stated in a document and enforced by nobody
# is a preference, and a preference stated as a rule is what makes an agent stop
# believing the rules that matter.
#
# -- WHAT IT CHECKS ----------------------------------------------------------
#   1. ⛔ NEWEST FIRST. Dates inside a section descend. This is the rule that
#      breaks most often, because appending is what an editor does by default.
#   2. ⛔ Every entry heading carries a date, ISO 8601.
#   3. ⛔ Every entry names its record. An entry with no record is a claim.
#   4. ⛔ Every entry says whether it deployed. Silence is not an answer.
#
# ⛔ WHAT IT DELIBERATELY DOES NOT CHECK IS WHETHER AN ENTRY IS TRUE. That is a
# reading and it belongs to the claim audit, docs/methodology/reviews.md lens 3.
#
# ⚠ NO CHANGELOG IS "COULD NOT RUN", NOT "PASS". A project with no CHANGELOG.md
# has not broken these rules and has not satisfied them either, and reporting
# green over an absent file is how a check quietly stops applying. Exit 2.
#
# Usage:
#   pwsh -NoProfile -File scripts/common/check-changelog.ps1
#   pwsh -NoProfile -File scripts/common/check-changelog.ps1 -Json
#   pwsh -NoProfile -File scripts/common/check-changelog.ps1 -File path/to/CHANGELOG.md
#
# Exit codes: 0 clean, 1 a rule was broken, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

[CmdletBinding()]
param(
    [switch]$Json,
    [string]$File = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine('check-changelog: git not found')
    exit 2
}
$root = (& git rev-parse --show-toplevel 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $root) {
    [Console]::Error.WriteLine('check-changelog: not a git repository')
    exit 2
}
$root = ($root | Select-Object -First 1).Trim()

if (-not $File) { $File = 'CHANGELOG.md' }
$target = if ([System.IO.Path]::IsPathRooted($File)) { $File } else { Join-Path $root $File }

if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
    [Console]::Error.WriteLine("check-changelog: no $File in this repository.")
    [Console]::Error.WriteLine('  That is "could not run", not "passed": a project with no changelog')
    [Console]::Error.WriteLine('  has neither broken these rules nor satisfied them.')
    exit 2
}

$lines = [System.IO.File]::ReadAllText($target) -split "`r?`n"

$problems = 0
$entries = 0
$report = New-Object System.Collections.ArrayList

# State for the entry currently being read.
$started = $false
$entryLine = 0
$hasRecord = $false
$hasDeploy = $false
# ⚠ prev resets per SECTION. "Unreleased" above "1.0.0" is correct, and
# comparing dates across a section boundary would report that as backwards.
$prev = ''

$dateRe = '[0-9]{4}-[0-9]{2}-[0-9]{2}(T[0-9]{2}:[0-9]{2}:[0-9]{2}Z)?'

function Complete-Entry {
    if (-not $script:started) { return }
    if (-not $script:hasRecord) {
        [void]$script:report.Add(("  {0}: the entry at line {1} names no record. An entry with no record is a claim." -f $script:relName, $script:entryLine))
        $script:problems++
    }
    if (-not $script:hasDeploy) {
        [void]$script:report.Add(("  {0}: the entry at line {1} does not say whether it deployed. Silence is not an answer." -f $script:relName, $script:entryLine))
        $script:problems++
    }
    $script:started = $false
}

$script:relName = $File
$script:report = $report
$script:problems = 0
$script:started = $false
$script:entryLine = 0
$script:hasRecord = $false
$script:hasDeploy = $false

$n = 0
foreach ($line in $lines) {
    $n++
    if ($line -match '^## ') {
        Complete-Entry
        $prev = ''
        continue
    }
    if ($line -match '^### ') {
        Complete-Entry
        $entries++
        $script:entryLine = $n
        if ($line -match $dateRe) {
            $d = $Matches[0]
        } else {
            [void]$report.Add(("  {0}:{1} no date in the heading. Nothing can order it." -f $File, $n))
            $script:problems++
            $d = ''
        }
        # Rule 1: newest first, within the section. ⚠ ISO 8601 sorts correctly
        # as a plain string, which is why the date FORMAT is a rule and not a
        # preference.
        if ($d -and $prev -and ([string]::CompareOrdinal($d, $prev) -gt 0)) {
            [void]$report.Add(("  {0}:{1} out of order: {2} comes after {3}. Newest first." -f $File, $n, $d, $prev))
            $script:problems++
        }
        if ($d) { $prev = $d }
        $script:hasRecord = $false
        $script:hasDeploy = $false
        $script:started = $true
        continue
    }
    if ($script:started) {
        $low = $line.ToLowerInvariant()
        if ($low -match 'record:') { $script:hasRecord = $true }
        if ($low -match 'deploy')  { $script:hasDeploy = $true }
    }
}
Complete-Entry

$problems = $script:problems

if ($Json) {
    Write-Output ('{"schema":"check-changelog/1","problems":' + $problems + ',"entries":' + $entries + '}')
    if ($problems -gt 0) { exit 1 }
    exit 0
}

if ($problems -gt 0) {
    Write-Output ("changelog check failed, {0} problem(s):" -f $problems)
    Write-Output ''
    $report | ForEach-Object { Write-Output $_ }
    Write-Output ''
    Write-Output 'The rules are in docs/conventions/docs.md. ⛔ Fix the entry; do not'
    Write-Output 'reorder the whole file in the commit that adds to it. Tidying is its'
    Write-Output 'own commit, or both become unreviewable.'
    exit 1
}

Write-Output ("changelog ok: {0} entries, in order, each dated with a record and a deploy line" -f $entries)
exit 0
