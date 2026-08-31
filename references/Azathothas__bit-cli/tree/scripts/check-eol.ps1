# Does every tracked file's working tree line ending match what .gitattributes
# says it should be?
#
# The defect this exists to catch is a stray carriage return read by something
# that is not git. `.gitattributes` normalises the **index** to LF, so a file
# written with CRLF commits as LF and `git diff` shows nothing: the drift is
# invisible to every git command and to a review. It is not invisible to the
# things in this repository that read the working tree directly.
#
# `check-todo.ps1`, `check-docs.ps1` and the schema generator all parse these
# files with `(?m)^...$`, and in .NET that anchor matches before the newline
# and leaves the carriage return inside the capture. A status cell read as
# "done`r" is a status that matches nothing. Every PowerShell script that writes
# one of these files with `Set-Content` writes CRLF on Windows by default, so
# the drift arrives from this repository's own tooling rather than from an
# editor.
#
# It was measured before this check existed: on 2026-08-25, four `TODO/` files
# and `CHANGELOG.md` were CRLF in the working tree and `TODO/create-seed.md`
# was **mixed**, all of them LF in the index.
#
# What it reads is git's own answer rather than a table repeated here.
# `git ls-files --eol` prints, per tracked file, the index ending, the working
# tree ending, and the attributes git resolved for it. The expected working
# tree ending is therefore whatever `.gitattributes` says, including its one
# exception: `*.ps1` is `eol=crlf`, because Windows PowerShell 5.1 mis-parses a
# here-string whose terminator arrives with a bare LF.
#
# `vendor/` is reported and never rewritten. `scripts/vendor-diff.ps1` derives
# the patch series with `git diff --no-index`, which compares two files on disk
# with no attribute normalisation at all, so changing a vendored file's endings
# would rewrite the whole series against a base that still has the old ones.
# The index is already LF there, which is the half that decides what a commit
# contains.
#
# Usage:
#   pwsh scripts/check-eol.ps1
#   pwsh scripts/check-eol.ps1 -Fix
#   pwsh scripts/check-eol.ps1 -Json bench/eol.json
#
# `-Fix` rewrites the offenders and produces no git diff: the index is already
# LF on both sides, so normalising the working tree changes nothing a commit
# would carry. `scripts/gates.ps1` runs this as the `eol` gate and passes `-Fix`
# through, so it is never a step anybody runs by hand.
#
# There is deliberately no CI job for it, unlike `tree`, `docs` and `record`.
# A fresh checkout gets its endings from `.gitattributes` by construction, so a
# job asserting that they match `.gitattributes` would assert a tautology. The
# drift this catches only exists in a working tree something has written into,
# which is a local machine.
#
# Exits 0 when every tracked file agrees, 1 when one does not, and 2 when the
# check could not run.
#
# See TODO/RULES.md section 5, "Output and prose".

[CmdletBinding()]
param(
    # Rewrite the offenders rather than failing on them.
    [switch]$Fix,
    # Write the report here as well as printing it.
    [string]$Json
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-eol: $message")
    exit $code
}

Push-Location $repo
try {
    $listed = & git ls-files --eol 2>&1
    if ($LASTEXITCODE -ne 0) { Exit-With 2 "git ls-files --eol failed: $listed" }

    # A line is `i/<eol>  w/<eol>  attr/<attributes><TAB><path>`. The attribute
    # column contains spaces, so the split is on the tab before the path and
    # never on whitespace.
    $wrong = [System.Collections.ArrayList]::new()
    $skipped = [System.Collections.ArrayList]::new()
    $fixed = [System.Collections.ArrayList]::new()
    $indexWrong = [System.Collections.ArrayList]::new()
    $seen = 0

    foreach ($line in $listed) {
        $text = "$line"
        if (-not $text.Trim()) { continue }
        $tab = $text.IndexOf("`t")
        if ($tab -lt 0) { continue }
        $columns = $text.Substring(0, $tab)
        $relative = $text.Substring($tab + 1).Trim()
        if (-not $relative) { continue }
        $seen++

        $indexEol = [regex]::Match($columns, 'i/(\S+)').Groups[1].Value
        $treeEol = [regex]::Match($columns, 'w/(\S+)').Groups[1].Value
        $attributes = [regex]::Match($columns, 'attr/(.*)$').Groups[1].Value.Trim()

        # Binary by attribute, or by git's own detection. Nothing to normalise
        # and nothing that could be wrong.
        if ($attributes -match '(^|\s)-text($|\s)' -or $indexEol -eq "-text" -or $treeEol -eq "-text") {
            continue
        }
        # A file with no line endings at all: empty, or one line with no final
        # newline. Neither ending is present, so neither can be wrong.
        if ($treeEol -eq "none") { continue }

        $want = switch -Regex ($attributes) {
            'eol=crlf' { "crlf"; break }
            default { "lf" }
        }

        # The index is the half that decides what a commit carries, and it is
        # LF for every text file whatever the working tree gets. `eol=crlf`
        # asks for a checkout, not for a commit: a `.ps1` is `i/lf w/crlf` when
        # everything is right. So this compares against LF rather than against
        # $want, and a file that fails it is one committed with the endings
        # still in it, which -Fix cannot repair without a re-add.
        if ($indexEol -ne "none" -and $indexEol -ne "lf") {
            [void]$indexWrong.Add("${relative}: the index is $indexEol and every tracked text file is committed as lf")
        }

        if ($treeEol -eq $want) { continue }

        if ($relative -like "vendor/*") {
            [void]$skipped.Add("${relative}: $treeEol, left alone because the patch series is derived with git diff --no-index")
            continue
        }

        [void]$wrong.Add("${relative}: $treeEol where .gitattributes asks for $want")
        if (-not $Fix) { continue }

        # Bytes in, bytes out. A BOM and every other byte survive: the only
        # thing rewritten is the ending, and CRLF is collapsed first so a mixed
        # file converges rather than gaining a second carriage return.
        $path = Join-Path $repo $relative
        if (-not (Test-Path -LiteralPath $path)) { continue }
        $bytes = [System.IO.File]::ReadAllBytes($path)
        $content = [System.Text.Encoding]::UTF8.GetString($bytes)
        $normalised = $content -replace "`r`n", "`n"
        if ($want -eq "crlf") { $normalised = $normalised -replace "`n", "`r`n" }
        if ($normalised -ne $content) {
            [System.IO.File]::WriteAllBytes($path, [System.Text.Encoding]::UTF8.GetBytes($normalised))
            [void]$fixed.Add($relative)
        }
    }

    if ($seen -eq 0) { Exit-With 2 "git ls-files --eol listed nothing" }

    $outstanding = if ($Fix) { @($wrong.Count - $fixed.Count) } else { $wrong.Count }
    $ok = ($outstanding -eq 0) -and ($indexWrong.Count -eq 0)

    $report = [ordered]@{
        kind         = "eol"
        generated_at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
        tracked      = $seen
        fixed        = @($fixed)
        wrong        = @($wrong)
        index_wrong  = @($indexWrong)
        skipped      = @($skipped)
        ok           = $ok
    }
    if ($Json) {
        $jsonPath = if ([System.IO.Path]::IsPathRooted($Json)) { $Json } else { Join-Path $repo $Json }
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $jsonPath) | Out-Null
        $report | ConvertTo-Json -Depth 5 | Set-Content -Path $jsonPath -Encoding utf8NoBOM
    }

    foreach ($entry in $skipped) { Write-Host "  skipped $entry" }
    foreach ($entry in $indexWrong) { Write-Host "  index   $entry" }
    if ($Fix) {
        foreach ($entry in $fixed) { Write-Host "  rewrote $entry" }
        Write-Host "check-eol: $seen tracked, $($fixed.Count) rewritten, $($skipped.Count) left under vendor/"
    }
    else {
        foreach ($entry in $wrong) { Write-Host "  wrong   $entry" }
        Write-Host "check-eol: $seen tracked, $($wrong.Count) wrong, $($skipped.Count) left under vendor/"
    }

    if (-not $ok) {
        if ($indexWrong.Count -gt 0) {
            [Console]::Error.WriteLine("check-eol: $($indexWrong.Count) file(s) are committed with the wrong endings. Re-add them: git add --renormalize .")
        }
        if ($outstanding -gt 0) {
            [Console]::Error.WriteLine("check-eol: $outstanding file(s) disagree with .gitattributes. Run with -Fix, or: pwsh -NoProfile -File scripts/gates.ps1 -Fix")
        }
        exit 1
    }
    exit 0
}
finally {
    Pop-Location
}
