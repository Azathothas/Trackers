# check-one-home.ps1 - does any sentence appear in two documents?
#
# ⭐ THE TWIN OF check-one-home.sh. Same schema, same exit codes, same
# threshold, same exemptions.
#
# ⛔ THE DEFECT: one fact with two homes. docs/conventions/prose.md has always
# said every fact lives in exactly one document, and nothing checked it, so it
# drifted the way an unchecked rule always drifts. The copy a reader trusts is
# whichever they saw first, and the wrong one is invisible until somebody
# notices the two disagree.
#
# ⭐ WHAT IT COST, MEASURED. A project built from this template accumulated 8
# sentences appearing verbatim in two documents and 3 whole sections that were
# near-copies of a convention, in a file that opened by saying it restated
# nothing. This template's own tree, checked for the first time on 2026-08-28,
# held 42 duplicated sentences of 8 words or more, 5 of them in the very
# skeleton it ships for that job.
#
# -- ⚠ THE FIRST RUN OF THE INSTRUMENT REPORTED ZERO, AND WAS WRONG ----------
#
# ⛔ It reported no duplicates at any threshold over a 60-file document set,
# because its file collector matched NOTHING: a quoted pathspec reached git
# through a shell that treats a quote as an ordinary character. Zero duplicates
# over zero files reads exactly like a clean tree.
#
# ⭐ That is why this refuses to report success over an empty scope.
#
# -- THE EXEMPTIONS ----------------------------------------------------------
#
# ⛔ THE ENTRY-POINT ROUTERS ARE EXEMPT FROM EACH OTHER, AND ONLY FROM EACH
# OTHER. Each states the absolutes in full on purpose, because a session may be
# handed exactly one of them. ⚠ A sentence shared between a router and any
# OTHER file is still refused, so the exemption cannot seed a copy into the
# tree. Verified by planting exactly that.
#
# ⛔ docs/history/ IS EXEMPT ENTIRELY: a superseded page states things the live
# pages now state differently, which is the point of it.
#
# ⚠ WHAT IT CANNOT SEE: a fact restated in different words. That is a reading.
# Verbatim duplication is what copy-and-paste actually produces, and that is
# what this holds.
#
# Usage:
#   pwsh -NoProfile -File scripts/common/check-one-home.ps1
#   pwsh -NoProfile -File scripts/common/check-one-home.ps1 -Json
#
# Exit codes: 0 clean, 1 a sentence has two homes, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

# ⛔ PositionalBinding IS OFF. A stray expanded argument must fail to bind
# rather than land on the next free parameter.
[CmdletBinding(PositionalBinding = $false)]
param(
    [switch]$Json
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ⚠ A CONSTANT, NOT A FLAG, for the reason check-markers gives about its own
# ceiling: a threshold anybody can raise from a command line gets raised
# instead of met.
$minWords = 12

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine('check-one-home: git not found')
    exit 2
}
$root = (& git rev-parse --show-toplevel 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $root) {
    [Console]::Error.WriteLine('check-one-home: not a git repository')
    exit 2
}
$root = ($root | Select-Object -First 1).Trim()

Push-Location $root
try {
    $tracked = @(& git ls-files 2>$null)
    $untracked = @(& git ls-files --others --exclude-standard 2>$null)
}
finally { Pop-Location }

# ⛔ NO QUOTED PATHSPEC HANDED TO GIT. The filter is applied here. See header.
# ⚠ -cnotmatch: PowerShell's default comparison is case-INSENSITIVE, and this
# trap has already made an exclusion in a sibling check swallow every finding.
$files = @($tracked + $untracked |
    ForEach-Object { $_.Trim() } |
    Where-Object { $_ -and $_ -cmatch '\.md$' -and $_ -cnotmatch '^docs/history/' } |
    Sort-Object -Unique)

if ($files.Count -lt 2) {
    [Console]::Error.WriteLine("check-one-home: only $($files.Count) file(s) in scope; nothing to compare")
    exit 2
}

$routers = @{ 'AGENTS.md' = $true; 'ROUTE.md' = $true; 'docs/templates/AGENTS.md' = $true }

function Get-SentenceList([string]$Text) {
    $sb = New-Object System.Text.StringBuilder
    $fence = $false
    foreach ($line in ($Text -split "`r?`n")) {
        if ($line -match '^[ \t]*```') { $fence = -not $fence; continue }
        if ($fence) { continue }
        if ($line -match '^[ \t]*\|') { continue }   # a table row is not a sentence
        if ($line -match '^[ \t]*#') { continue }    # nor is a heading
        $l = [regex]::Replace($line, '`[^`]*`', ' ')
        $l = [regex]::Replace($l, '\]\([^)]*\)', ' ')
        $l = $l -replace '\[', ' '
        [void]$sb.Append(' ').Append($l)
    }
    $out = New-Object System.Collections.ArrayList
    foreach ($part in [regex]::Split($sb.ToString(), '[.:!?]+[ \t]+')) {
        $s = ([regex]::Replace($part.ToLowerInvariant(), '[^a-z0-9 ]', ' '))
        $s = ([regex]::Replace($s, ' +', ' ')).Trim()
        if (-not $s) { continue }
        if (($s -split ' ').Count -lt $minWords) { continue }
        [void]$out.Add($s)
    }
    return $out
}

$seen = @{}
$nfiles = 0
foreach ($rel in $files) {
    $full = Join-Path $root $rel
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { continue }
    $nfiles++
    $text = [System.IO.File]::ReadAllText($full, [System.Text.UTF8Encoding]::new($false))
    foreach ($s in (Get-SentenceList $text)) {
        if (-not $seen.ContainsKey($s)) { $seen[$s] = New-Object System.Collections.Generic.HashSet[string] }
        [void]$seen[$s].Add($rel)
    }
}

if ($nfiles -lt 2) {
    [Console]::Error.WriteLine("check-one-home: only $nfiles file(s) readable; nothing to compare")
    exit 2
}

$dups = New-Object System.Collections.ArrayList
foreach ($k in $seen.Keys) {
    $where = $seen[$k]
    if ($where.Count -lt 2) { continue }
    $allRouters = $true
    foreach ($f in $where) { if (-not $routers.ContainsKey($f)) { $allRouters = $false } }
    if ($allRouters) { continue }
    [void]$dups.Add([pscustomobject]@{ Sentence = $k; Files = @($where | Sort-Object) })
}

$count = $dups.Count

if ($Json) {
    Write-Output ('{"schema":"check-one-home/1","problems":' + $count + ',"files":' + $nfiles + ',"min_words":' + $minWords + '}')
    if ($count -gt 0) { exit 1 }
    exit 0
}

if ($count -gt 0) {
    Write-Output ("one fact, one home: {0} sentence(s) appear in more than one document:" -f $count)
    Write-Output ''
    foreach ($d in $dups) {
        $shown = $d.Sentence
        if ($shown.Length -gt 88) { $shown = $shown.Substring(0, 88) }
        Write-Output ('  "' + $shown + '"')
        foreach ($f in $d.Files) { Write-Output ('      ' + $f) }
        Write-Output ''
    }
    Write-Output 'Keep the fact in the document that owns it and make the other a pointer.'
    Write-Output 'docs/conventions/prose.md, "one fact, one home".'
    exit 1
}

Write-Output ("one fact one home: {0} documents, no sentence of {1}+ words in two of them" -f $nfiles, $minWords)
exit 0
