# check-markers.ps1 - only the five defined characters, and not too many of them.
#
# ⭐ THE TWIN OF check-markers.sh. Same schema, same exit codes, same scope,
# same ceiling. scripts/README.md says why every check here has two
# implementations, and check-twins.sh is what stops them drifting.
#
# Two rules, one subject, one home. docs/conventions/prose.md is the rule.
#
#   1. THE CHARACTER SET. Every tracked text file is ASCII, with the three
#      prose markers and the two status glyphs as the only exception.
#   2. THE DENSITY. A file carrying more markers than the ceiling below is
#      refused, because a page where every paragraph shouts has no markers at
#      all.
#
# -- WHY THIS OWNS THE CHARACTER RULE AND check-docs NO LONGER DOES ----------
#
# ⛔ THE RULE USED TO SCAN MARKDOWN ALONE, which left every .sh, .ps1, .c,
# .yml and .mjs in the tree unchecked. Measured on this repository on
# 2026-08-28, before this check existed: 2290 characters outside the five, in
# 22 files, and every one of them was in a script rather than a document.
#
# ⛔ Two checks enforcing one rule is two places for it to be wrong, so the
# rule moved here entire, the same way the control-byte rule moved out of
# check-docs into its own file, and for the same reason.
#
# -- THE DENSITY CEILING, AND WHERE THE NUMBER CAME FROM --------------------
#
# ⭐ prose.md has always said to use markers "sparingly enough that they are
# still visible" and nothing checked it, so an agent kept strictly to the five
# allowed characters and spammed them until the documents were unreadable.
#
# Markers per 100 non-blank lines, measured 2026-08-28 on one Windows 11 Pro
# 26200 machine, over the tracked markdown of three trees:
#
#   pkgforge-dev/docker-bsd            38.6 overall, worst file 53.3
#   Azathothas/TEMPLATE (this tree)     9.0 overall, worst file 26.3
#   pkgforge-dev/cross-libc-dlopen      8.6 overall, worst file 21.8
#
# ⭐ The two ADOPTER trees were ranked by eye before any of this was counted,
# and the ranking came out in that order. ⚠ Only those two were ranked; this
# tree was not placed against them and its number simply falls between.
#
# The ceiling is 30. It passes every file in the two trees that read well and
# refuses 7 of the 12 files in the one that does not.
#
# ⚠ IT IS A CONSTANT, NOT A FLAG. A ceiling anybody can raise from a command
# line is a ceiling that gets raised instead of met.
#
# ⚠ WHAT IT CANNOT SEE. Density is a count, and a marker used wrongly is a
# reading: a status glyph carrying a rule passes this check and fails a review.
#
# -- THE EXEMPTIONS ----------------------------------------------------------
#
# ⛔ LICENSES/*.txt IS EXEMPT. Canonical SPDX texts, two of which carry
# typographic quotes and a copyright sign, and four of which must never have
# their notice altered because the copyright line is somebody else's. A check
# that asked anybody to edit them would be asking for a corruption.
#
# ⚠ A LEADING BYTE-ORDER MARK IS EXEMPT, and only a leading one. Every .ps1
# here begins with one. A BOM anywhere else is a real defect and is reported.
#
# ⭐ A SPECIMEN INSIDE A CODE SPAN OR A FENCED BLOCK IS PERMITTED, in markdown,
# because a page that bans a character cannot otherwise show which one.
#
# Usage:
#   pwsh -NoProfile -File scripts/common/check-markers.ps1
#   pwsh -NoProfile -File scripts/common/check-markers.ps1 -Json
#
# Exit codes: 0 clean, 1 a character or a density was refused, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

# ⛔ PositionalBinding IS OFF. A .ps1 invoked through `-File` receives whatever
# the calling shell expanded as separate arguments, and a stray one binds
# positionally onto the next free parameter. That shipped a commit under a
# fabricated author in a sibling script in this directory. Off, a stray
# argument fails to bind and nothing runs.
[CmdletBinding(PositionalBinding = $false)]
param(
    [switch]$Json
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ceiling = 30

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine('check-markers: git not found')
    exit 2
}

# ⚠ git writes progress to stderr on success, so every git call here is judged
# on $LASTEXITCODE rather than on stderr.
$root = (& git rev-parse --show-toplevel 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $root) {
    [Console]::Error.WriteLine('check-markers: not a git repository')
    exit 2
}
$root = ($root | Select-Object -First 1).Trim()

$textRe = '\.(ts|tsx|js|mjs|cjs|jsx|json|md|sql|css|scss|html|toml|yaml|yml|sh|ps1|py|rs|go|c|h|cpp|hpp|java|rb|php|txt|cfg|ini|conf)$'

Push-Location $root
try {
    $tracked = @(& git ls-files 2>$null)
    $untracked = @(& git ls-files --others --exclude-standard 2>$null)
}
finally { Pop-Location }

# ⛔ -cnotmatch, NOT -notmatch. PowerShell's default comparison operators are
# case-INSENSITIVE, and this exact trap once made an exclusion swallow every
# real finding in a sibling check here: `[a-z]` matched an upper-case letter
# and the check reported clean over a file that was not.
$files = @($tracked + $untracked |
    ForEach-Object { $_.Trim() } |
    Where-Object { $_ -and $_ -match $textRe -and $_ -cnotmatch '^LICENSES/.*\.txt$' } |
    Sort-Object -Unique)

if ($files.Count -eq 0) {
    [Console]::Error.WriteLine('check-markers: no text files in scope')
    exit 2
}

# U+26D4 stop, U+2B50 star, U+26A0 warning, U+2705 pass, U+274C fail.
# ⛔ WRITTEN AS CODEPOINTS, NOT AS LITERALS. A file that carries the characters
# it is deciding about is a file whose own rule cannot be read from its source,
# and this repository has a documented habit of a page about a character
# containing the character it warns of.
$allowed = @(
    [char]0x26D4, [char]0x2B50, [char]0x26A0, [char]0x2705, [char]0x274C
)
$allowedSet = @{}
foreach ($c in $allowed) { $allowedSet[$c] = $true }

$problems = 0
$nfiles = 0
$markers = 0
$worst = 0
$worstF = '-'
$report = New-Object System.Collections.ArrayList

foreach ($rel in $files) {
    $full = Join-Path $root $rel
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { continue }
    $nfiles++

    # ⚠ ReadAllText WITH A UTF8 DECODER STRIPS A LEADING BOM AND NOTHING ELSE,
    # which is exactly the exemption: a BOM in the middle of a file survives
    # into the text and is still reported.
    $text = [System.IO.File]::ReadAllText($full, [System.Text.UTF8Encoding]::new($false))
    $lines = $text -split "`n"

    $isMd = $rel -match '\.md$'
    $fence = $false
    $fmark = 0
    $fnon = 0
    $ln = 0

    foreach ($raw in $lines) {
        $ln++
        $line = $raw -replace "`r", ''
        if ($line -match '\S') { $fnon++ }

        # Markers are counted BEFORE anything is stripped, because the density
        # rule is about what a reader sees on the page.
        $kept = New-Object System.Text.StringBuilder
        foreach ($ch in $line.ToCharArray()) {
            if ($allowedSet.ContainsKey($ch)) { $fmark++ } else { [void]$kept.Append($ch) }
        }
        $rest = $kept.ToString()

        # ⭐ THE SPECIMEN EXEMPTION, markdown only.
        if ($isMd) {
            if ($line -match '^[ \t]*```') { $fence = -not $fence; continue }
            if ($fence) { continue }
            $rest = [regex]::Replace($rest, '`[^`]*`', '')
        }

        # Whatever survives must be ASCII. First offender per line only; a line
        # with one wrong character usually has several of the same.
        #
        # ⭐ THE CODEPOINT IS REPORTED, not just the position. This check took
        # over the em-dash rule from check-docs, which named that one character
        # in its message, and "something non-ASCII on line 12" would have been
        # a step backwards.
        #
        # ⚠ A SURROGATE PAIR IS ONE CODEPOINT AND TWO .NET CHARS. Reading the
        # high surrogate alone gives D83D rather than 1F600, which is a number
        # that exists nowhere and cannot be searched for. ConvertToUtf32 reads
        # the pair, which is what makes this agree with the sh twin's decoder.
        for ($i = 0; $i -lt $rest.Length; $i++) {
            if ([int]$rest[$i] -lt 128) { continue }
            $cp = if ([char]::IsHighSurrogate($rest[$i]) -and $i + 1 -lt $rest.Length) {
                [char]::ConvertToUtf32($rest[$i], $rest[$i + 1])
            } else { [int]$rest[$i] }
            $problems++
            [void]$report.Add(("  {0}:{1} U+{2:X4} is outside the five. docs/conventions/prose.md" -f $rel, $ln, $cp))
            break
        }
    }

    if ($fnon -lt 1) { $fnon = 1 }
    $markers += $fmark

    # ⚠ INTEGER DIVISION, to match the sh twin exactly. [math]::Floor on a
    # double would agree here and disagree at a boundary, and the twins are
    # compared on the number rather than on the verdict.
    $dens = [int][math]::Floor(($fmark * 100) / $fnon)
    if ($dens -gt $worst) { $worst = $dens; $worstF = $rel }
    if ($dens -gt $ceiling) {
        $problems++
        [void]$report.Add(("  {0} {1} markers in {2} non-blank lines, {3} per 100. The ceiling is {4}. docs/conventions/prose.md" -f $rel, $fmark, $fnon, $dens, $ceiling))
    }
}

if ($Json) {
    # ⚠ CONCATENATED, NOT `-f`. PowerShell's format operator needs a doubled
    # brace to emit a literal one, which is the shape check-placeholders looks
    # for, and it has fired on exactly that before.
    Write-Output ('{"schema":"check-markers/1","problems":' + $problems + ',"files":' + $nfiles + ',"markers":' + $markers + ',"ceiling":' + $ceiling + ',"worst_density":' + $worst + '}')
    if ($problems -gt 0) { exit 1 }
    exit 0
}

if ($problems -gt 0) {
    Write-Output ("marker check failed, {0} problem(s):" -f $problems)
    Write-Output ''
    $report | ForEach-Object { Write-Output $_ }
    Write-Output ''
    Write-Output 'The five are the three prose markers and the two status glyphs.'
    Write-Output 'Everything else is ASCII. docs/conventions/prose.md is the rule.'
    exit 1
}

Write-Output ("markers ok: {0} files, {1} markers, densest {2} per 100 non-blank lines ({3}), ceiling {4}" -f $nfiles, $markers, $worst, $worstF, $ceiling)
exit 0
