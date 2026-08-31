# check-docs.ps1 - do the documents still resolve, and are they written the way
# this repository writes documents?
#
# ⭐ THE TWIN OF check-docs.sh. Same schema, same exit codes, same exemptions.
# check-twins.ps1 is what stops the two drifting.
#
# The defect this exists to catch is a document that was true when it was
# written. Three shapes of it, and every one is invisible to every other check:
#
#   - a link or a path that stopped resolving when something was renamed;
#   - a fenced shell block that does not parse, which is a block nobody can
#     copy and paste;
#   - an angle-bracket placeholder inside a shell block: a human reads it as
#     "fill this in" and bash reads it as a redirect, so the reader gets a
#     cryptic syntax error instead of an obvious instruction.
#
# ⚠ CONTROL BYTES ARE NOT CHECKED HERE. That rule scanned markdown only while
# every .ts, .py, .rs and .sh in the tree went unchecked, so it moved to
# check-control-bytes.ps1, which reads every text file. Run both.
#
# ⚠ THE CHARACTER HALF OF THE PROSE RULE IS NOT HERE. No em dash and no
# character outside the five belong to check-markers.ps1, which reads every
# tracked text file rather than markdown alone. Run both. What stays here is
# what is specific to a document: links, fenced blocks, placeholders, banned
# vocabulary and orphan pages.
#
# ⛔ WHAT IT DOES NOT CHECK IS WHETHER A CLAIM IS TRUE. That is a reading, and
# it belongs to the review pass. A guard that tried to verify prose would
# either pass vacuously or refuse legitimate writing, and both are worse than
# an honest scope.
#
# ⚠ THE SHELL-BLOCK PARSE NEEDS A POSIX SHELL, AND THIS HOST MAY NOT HAVE ONE.
# When no `sh` is on PATH the blocks are still COUNTED, so the schema matches
# the sh twin, and the parse rule is reported as SKIPPED on stderr rather than
# silently passing. ⛔ A rule that cannot run must say so: reporting green for
# a check that never executed is the failure this whole repository is built to
# avoid.
#
# Usage:
#   pwsh -NoProfile -File scripts/common/check-docs.ps1
#   pwsh -NoProfile -File scripts/common/check-docs.ps1 -Json
#   pwsh -NoProfile -File scripts/common/check-docs.ps1 -Path docs
#
# Exit codes: 0 clean, 1 something is wrong, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

[CmdletBinding()]
param(
    [switch]$Json,
    [string]$Path = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine('check-docs: git not found')
    exit 2
}
$root = (& git rev-parse --show-toplevel 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $root) {
    [Console]::Error.WriteLine('check-docs: not a git repository')
    exit 2
}
$root = ($root | Select-Object -First 1).Trim()

Push-Location $root
try {
    $tracked = @(& git ls-files 2>$null)
    $untracked = @(& git ls-files --others --exclude-standard 2>$null)
}
finally { Pop-Location }

$all = @($tracked + $untracked | ForEach-Object { $_.Trim() } | Where-Object { $_ } | Sort-Object -Unique)
$files = @($all | Where-Object { $_ -match '\.md$' })
if ($Path) {
    $prefix = $Path.TrimEnd('/', '\').Replace('\', '/')
    $files = @($files | Where-Object { $_ -like "$prefix/*" -or $_ -eq $prefix })
}
if ($files.Count -eq 0) {
    [Console]::Error.WriteLine('check-docs: no markdown files in scope')
    exit 2
}

$problems = New-Object System.Collections.ArrayList
$count = 0
$nlinks = 0
$nblocks = 0
function Add-Problem([string]$Text) {
    [void]$script:problems.Add('  ' + $Text)
    $script:count++
}
$script:problems = $problems
$script:count = 0

# A POSIX shell, if this host has one. See the header: absence is reported,
# never silently treated as a pass.
$shell = $null
foreach ($c in 'sh', 'bash') {
    $g = Get-Command $c -ErrorAction SilentlyContinue
    if ($g -and $g.CommandType -in 'Application', 'ExternalScript') { $shell = $g.Source; break }
}
$skippedParse = 0

# ⚠ THE TEMPLATE DIRECTORIES ARE EXEMPT FROM THE LINK CHECK, AND MUST BE.
# A template's links are written relative to where the file will live in the
# PROJECT, not where it lives here: docs/templates/AGENTS.md links to
# docs/methodology/gate.md because in a project that file sits at the root.
# Checking those here reports thirty-odd failures on a correct tree, and a
# check that fails on a correct tree gets switched off within a week.
# ⭐ The PROSE rules still apply to templates. Only link resolution is exempt,
# because only that one is position-dependent.
function Test-LinkExempt([string]$Rel) {
    return ($Rel -like 'docs/templates/*' -or $Rel -like 'bootstrap/prompts/*')
}

$linked = New-Object System.Collections.Generic.HashSet[string]

function Get-LinkTarget([string]$Text) {
    # Strip fenced blocks, then code spans, then take every ](...) target.
    # ⚠ Stripping code spans is why a backticked expression is not reported as
    # a broken link. Markdown does not linkify a code span, and an earlier
    # version of this check reported exactly that as broken.
    $out = New-Object System.Collections.ArrayList
    $fence = $false
    $n = 0
    foreach ($line in ($Text -split "`r?`n")) {
        $n++
        if ($line -match '^[ \t]*```') { $fence = -not $fence; continue }
        if ($fence) { continue }
        $clean = [regex]::Replace($line, '`[^`]*`', '')
        foreach ($m in [regex]::Matches($clean, '\]\(([^)\s]+)')) {
            [void]$out.Add([pscustomobject]@{ Line = $n; Target = $m.Groups[1].Value })
        }
    }
    return $out
}

foreach ($rel in $files) {
    $full = Join-Path $root $rel
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { continue }
    $text = [System.IO.File]::ReadAllText($full)
    # ⛔ FORWARD SLASHES, ALWAYS. Split-Path returns a WINDOWS separator, so
    # `docs\conventions` has no `/` in it, and the `..` collapse below then
    # treats the whole thing as ONE segment: `docs\conventions/../../x`
    # collapsed to `../x`, which resolves outside the repository and reported
    # thirty-one correct links as broken. git speaks forward slashes and so
    # does every link in a markdown file; the only thing that did not was this
    # one call.
    $dir = (Split-Path -Parent $rel).Replace([char]92, '/')
    if (-not $dir) { $dir = '.' }
    $linkCheck = -not (Test-LinkExempt $rel)

    # -- links ---------------------------------------------------------------
    foreach ($t in (Get-LinkTarget $text)) {
        $target = $t.Target
        if ($target -match '^(https?:|mailto:)' -or -not $target) { continue }
        # ⚠ COUNTED BEFORE THE EMPTY TEST, to match the sh twin exactly. A
        # pure-anchor link like the section links in this repository's own
        # documents has no path part, so it is counted as examined and then
        # skipped. Counting it after instead put the two implementations one
        # apart on a clean tree, which check-twins reports as drift and which
        # is a real disagreement about what the number means.
        $bare = ($target -split '#')[0]
        if (-not $linkCheck) { continue }
        $script:nlinks++
        if (-not $bare) { continue }
        # Normalise to a repo-relative path so a link from a subdirectory and
        # one from the root name the same file.
        $joined = if ($dir -eq '.') { $bare } else { $dir + '/' + $bare }
        $norm = $joined -replace '/\./', '/'
        while ($norm -match '[^/]+/\.\./') { $norm = $norm -replace '[^/]+/\.\./', '' }
        $norm = $norm -replace '^\./', ''
        [void]$linked.Add($norm)
        if (-not (Test-Path -LiteralPath (Join-Path $root $norm))) {
            Add-Problem ($rel + ':' + $t.Line + ' broken link -> ' + $target)
        }
    }
}

# -- fenced shell blocks -----------------------------------------------------
foreach ($rel in $files) {
    $full = Join-Path $root $rel
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { continue }
    $lines = [System.IO.File]::ReadAllText($full) -split "`r?`n"
    $inBlock = $false
    $start = 0
    $buf = New-Object System.Collections.ArrayList
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        if (-not $inBlock -and $line -match '^[ \t]*```(bash|sh)[ \t]*$') {
            $inBlock = $true; $start = $i + 1; [void]$buf.Clear(); continue
        }
        if ($inBlock -and $line -match '^[ \t]*```') {
            $inBlock = $false
            $nblocks++
            $body = ($buf -join "`n")

            if ($body -match '<[a-z][a-z0-9-]*>') {
                Add-Problem ($rel + ':' + $start + ' shell-unsafe placeholder. bash reads it as a redirect; use UPPER_SNAKE')
            }

            if ($shell) {
                # ⛔ A TEMP FILE, NOT stdin. docs/conventions/shell.md: from
                # PowerShell a native command's stdin is not byte-exact, and a
                # trailing CRLF gets appended. For a syntax check that is the
                # difference between a real answer and a fabricated one.
                $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ('checkdocs-' + [guid]::NewGuid().ToString('N') + '.sh')
                try {
                    [System.IO.File]::WriteAllText($tmp, ($body -replace "`r", '') + "`n")
                    $prev = $ErrorActionPreference
                    $ErrorActionPreference = 'Continue'
                    try { & $shell -n $tmp 2>$null | Out-Null } finally { $ErrorActionPreference = $prev }
                    if ($LASTEXITCODE -ne 0) {
                        Add-Problem ($rel + ':' + $start + ' shell block does not parse')
                    }
                }
                finally { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue }
            }
            else { $skippedParse++ }
            continue
        }
        if ($inBlock) { [void]$buf.Add($line) }
    }
}

# -- a page nothing links to -------------------------------------------------
# ⛔ AN UNLINKED PAGE IS NOT READ, SO IT IS NOT CORRECTED, and that is the state
# every stale document passes through on the way to being wrong.
# Roots are exempt: a README is an entry point, and the files at the repository
# root are what a reader or a raw URL arrives at directly.
foreach ($rel in $files) {
    if ($rel -match '(^|/)README\.md$') { continue }
    if ($rel -notmatch '/') { continue }
    if (-not $linked.Contains($rel)) {
        Add-Problem ($rel + ' is linked from nowhere. An unlinked page is not read, so it is not corrected.')
    }
}

# -- the character rule moved, it was NOT dropped -------------------------
# ⛔ THE FIVE-CHARACTER ALLOWLIST AND THE EM-DASH RULE NOW LIVE IN
# check-markers.ps1, over EVERY tracked text file rather than over markdown
# alone. Two checks enforcing one rule is two places for it to be wrong, and
# these two would have been wrong differently: this one strips fenced blocks
# and code spans before it looks and a whole-tree scan that did not would
# refuse the page that names the character it bans.
#
# ⚠ It is the same move the control-byte rule made out of this file, for
# the same reason. ⛔ Run both: this one for documents, that one for the
# whole tree.

$count = $script:count

if ($Json) {
    Write-Output ('{"schema":"check-docs/1","problems":' + $count + ',"files":' + $files.Count + ',"links":' + $nlinks + ',"shell_blocks":' + $nblocks + '}')
    if ($skippedParse -gt 0) {
        [Console]::Error.WriteLine('⚠ no POSIX shell on PATH: ' + $skippedParse + ' shell block(s) counted but NOT parsed')
    }
    if ($count -gt 0) { exit 1 }
    exit 0
}

if ($count -gt 0) {
    Write-Output ('documentation check failed, ' + $count + ' problem(s):')
    Write-Output ''
    $problems | ForEach-Object { Write-Output $_ }
    Write-Output ''
    if ($skippedParse -gt 0) {
        Write-Output ('⚠ no POSIX shell on PATH: ' + $skippedParse + ' shell block(s) counted but NOT parsed')
    }
    exit 1
}

Write-Output ('docs ok: ' + $files.Count + ' files, ' + $nlinks + ' relative links, ' + $nblocks + ' shell blocks. Links and prose clean.')
if ($skippedParse -gt 0) {
    Write-Output ('⚠ no POSIX shell on PATH: ' + $skippedParse + ' shell block(s) counted but NOT parsed')
}
exit 0
