# check-control-bytes.ps1 - is there a literal control byte in any text file?
#
# ⭐ THE TWIN OF check-control-bytes.sh. Same schema, same exit codes, same
# scope. scripts/README.md says why every check here has two implementations,
# and check-twins.ps1 is what stops them drifting.
#
# The defect this exists to catch is a file that is invisible to review. A
# literal control byte makes a file unreadable to BOTH review tools at once:
# `grep` calls it binary and SKIPS it, and `git diff` prints "Binary files
# differ", so a code review of the file shows no diff at all. `git diff --text`
# renders it fine, which is the proof that only reviewability was ever at stake.
#
# ⭐ The runtime value is identical either way. Write the escape, not the byte.
# Because correctness never depends on it, this survives a long time unnoticed.
#
# -- THE THREE BLIND SPOTS THIS SCOPE WAS PAID FOR ---------------------------
#
# 1. ⛔ TRACKED ALONE IS NOT ENOUGH. `git ls-files` cannot see a file that has
#    never been staged, which is exactly when a new file is most likely to
#    acquire a stray byte. A brand-new test file carried a literal NUL where a
#    trailing space belonged; grep called it binary, an assertion went green for
#    the wrong reason, and the guard reported clean because the file was not
#    tracked yet.
# 2. ⛔ `git ls-files` IS RELATIVE TO THE PROCESS WORKING DIRECTORY, so this
#    guard's scope used to depend on who called it: 1071 files from the root,
#    391 from one package directory, which is where the gate actually invoked
#    it. ⭐ A guard cannot prove itself. It is pinned to the repository root.
# 3. ⚠ BINARIES ARE OUT OF SCOPE BY CONSTRUCTION. The extension list says what
#    IS text. An allowlist of "binaries that are fine" is the kind of list that
#    quietly absorbs a real finding.
#
# Usage:
#   pwsh -NoProfile -File scripts/common/check-control-bytes.ps1
#   pwsh -NoProfile -File scripts/common/check-control-bytes.ps1 -Json
#
# Exit codes: 0 clean, 1 a byte was found, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

[CmdletBinding()]
param(
    [switch]$Json
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine('check-control-bytes: git not found')
    exit 2
}

# ⚠ git writes progress to stderr on success and
# $PSNativeCommandUseErrorActionPreference is false by default from pwsh 7.4,
# so every git call here is judged on $LASTEXITCODE rather than on stderr.
$root = (& git rev-parse --show-toplevel 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $root) {
    [Console]::Error.WriteLine('check-control-bytes: not a git repository')
    exit 2
}
$root = ($root | Select-Object -First 1).Trim()

# Extensions asserted to be TEXT. Anything else is out of scope by construction.
$textRe = '\.(ts|tsx|js|mjs|cjs|jsx|json|md|sql|css|scss|html|toml|yaml|yml|sh|ps1|py|rs|go|c|h|cpp|hpp|java|rb|php|txt|cfg|ini|conf)$'

Push-Location $root
try {
    $tracked = @(& git ls-files 2>$null)
    $untracked = @(& git ls-files --others --exclude-standard 2>$null)
}
finally { Pop-Location }

$files = @($tracked + $untracked |
    ForEach-Object { $_.Trim() } |
    Where-Object { $_ -and $_ -match $textRe } |
    Sort-Object -Unique)

if ($files.Count -eq 0) {
    [Console]::Error.WriteLine('check-control-bytes: no text files in scope')
    exit 2
}

# C0 controls except the three that are legitimately in text: tab (09), newline
# (0a) and carriage return (0d). NUL is included here: unlike the sh twin, a
# .NET byte array has no trouble holding one.
function Test-BadByte([int]$b) {
    if ($b -eq 9 -or $b -eq 10 -or $b -eq 13) { return $false }
    if ($b -lt 32) { return $true }
    return $false
}

$problems = 0
$nfiles = 0
$report = New-Object System.Collections.ArrayList

foreach ($rel in $files) {
    $full = Join-Path $root $rel
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { continue }
    $nfiles++

    # ⚠ READ BYTES, NOT TEXT. Get-Content without -AsByteStream decodes, and a
    # decoder can turn an invalid sequence into U+FFFD, which would hide the
    # very byte this is looking for.
    $bytes = [System.IO.File]::ReadAllBytes($full)
    $line = 1
    $found = $null
    for ($i = 0; $i -lt $bytes.Length; $i++) {
        if ($bytes[$i] -eq 10) { $line++; continue }
        if (Test-BadByte $bytes[$i]) {
            $found = [pscustomobject]@{ Line = $line; Byte = $bytes[$i] }
            break
        }
    }
    if ($null -ne $found) {
        $problems++
        $hex = '0x{0:x2}' -f $found.Byte
        [void]$report.Add(("  {0}:{1} a control byte {2}" -f $rel, $found.Line, $hex))
    }
}

if ($Json) {
    # ⚠ CONCATENATED, NOT `-f`. PowerShell's format operator needs a doubled
    # brace to emit a literal one, so the JSON template would have to be written
    # brace, which is exactly the shape check-placeholders.sh looks
    # for, and it fired on it. Concatenation keeps the source honest and keeps
    # the output byte-identical to the sh twin.
    Write-Output ('{"schema":"check-control-bytes/1","problems":' + $problems + ',"files":' + $nfiles + '}')
    if ($problems -gt 0) { exit 1 }
    exit 0
}

if ($problems -gt 0) {
    Write-Output ("literal control bytes in {0} file(s):" -f $problems)
    Write-Output ''
    $report | ForEach-Object { Write-Output $_ }
    Write-Output ''
    Write-Output 'Write the ESCAPE, not the byte. The escape is the same character at'
    Write-Output 'runtime, and the byte is what makes the file invisible to grep and'
    Write-Output 'unreviewable in git diff. docs/conventions/shell.md section 6.'
    exit 1
}

Write-Output ("no literal control bytes in {0} text files (tracked plus untracked-not-ignored)" -f $nfiles)
exit 0
