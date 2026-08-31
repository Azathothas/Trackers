# check-no-secrets.ps1 - does any file in this tree carry something that must
# not be published?
#
# ⭐ THE TWIN OF check-no-secrets.sh. Same schema, same categories, same exit
# codes. check-twins.ps1 is what stops the two drifting.
#
# ⚠ THE SCOPE IS TRACKED PLUS UNTRACKED-BUT-NOT-IGNORED, not tracked alone.
# `git ls-files` cannot see a file that has never been staged, which is exactly
# when a new file is most likely to carry a credential and exactly what the
# next `git add -A` would take.
#
# The defect this exists to catch is a credential, or a fingerprint of a private
# system, reaching a remote. Once it does, a history rewrite does not undo it:
# the value was readable, and it may be cached, mirrored or already indexed.
# Rotation is the fix; this is what stops it needing one.
#
# ⛔ IT FINDS THE SHAPES IT KNOWS, AND A GREEN RUN IS NOT A CLEARANCE.
# It cannot find a password that looks like a word, a hostname that reads as
# prose, or a page of correct-looking examples that happens to describe a real
# system. It narrows the reading. It does not replace it.
#
# -Public adds the rules that only matter for a repository that is or will be
# public: emails, absolute home paths, long hex identifiers. In a private
# project those are legitimate content, which is why they are not the default.
#
# Usage:
#   pwsh -NoProfile -File scripts/common/check-no-secrets.ps1
#   pwsh -NoProfile -File scripts/common/check-no-secrets.ps1 -Public
#   pwsh -NoProfile -File scripts/common/check-no-secrets.ps1 -Json
#
# Exit codes: 0 nothing found, 1 something found, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

[CmdletBinding()]
param(
    [switch]$Public,
    [switch]$Json
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine('check-no-secrets: git not found')
    exit 2
}
$root = (& git rev-parse --show-toplevel 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $root) {
    [Console]::Error.WriteLine('check-no-secrets: not a git repository')
    exit 2
}
$root = ($root | Select-Object -First 1).Trim()

Push-Location $root
try {
    $tracked = @(& git ls-files 2>$null)
    $untracked = @(& git ls-files --others --exclude-standard 2>$null)
}
finally { Pop-Location }

$files = @($tracked + $untracked | ForEach-Object { $_.Trim() } | Where-Object { $_ } | Sort-Object -Unique)

$script:found = 0
$script:report = New-Object System.Collections.ArrayList

function Add-Hit([string]$Name, $Lines) {
    # ⚠ COERCE TO AN ARRAY FIRST. Under Set-StrictMode -Version Latest,
    # reading .Count on a scalar or on $null throws "The property 'Count'
    # cannot be found on this object", and a pattern that matched exactly once
    # returns a scalar. So the failure appeared only when a rule fired, which
    # is the one path a green run never exercises.
    # ⛔ AND FILTER THE EMPTIES. A PowerShell function returning an EMPTY
    # collection returns nothing, and `@($null)` has a Count of ONE, not zero.
    # So every category reported a hit with an empty body and this check failed
    # over a clean tree: ten findings, all of them nothing. ⚠ The sh twin has
    # no equivalent trap, which is exactly why the two are compared on the same
    # tree rather than trusted separately.
    $arr = @($Lines | Where-Object { $_ })
    if ($arr.Count -eq 0) { return }
    $script:found++
    [void]$script:report.Add('')
    [void]$script:report.Add("== $Name ==")
    $arr | ForEach-Object { [void]$script:report.Add($_) }
}

# ⚠ A binary file is skipped, matching `grep -I` in the sh twin.
function Read-TextOrNull([string]$Path) {
    try { $bytes = [System.IO.File]::ReadAllBytes($Path) } catch { return $null }
    $limit = [Math]::Min($bytes.Length, 8000)
    for ($i = 0; $i -lt $limit; $i++) { if ($bytes[$i] -eq 0) { return $null } }
    return [System.Text.Encoding]::UTF8.GetString($bytes)
}

# Read every file once. ⚠ The sh twin spawns one grep per pattern; doing that
# here would be one process per pattern per file, on the slowest host there is.
$texts = [ordered]@{}
foreach ($rel in $files) {
    $full = Join-Path $root $rel
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { continue }
    $t = Read-TextOrNull $full
    if ($null -ne $t) { $texts[$rel] = $t }
}

function Find-Pattern([string]$Pattern) {
    $hits = New-Object System.Collections.ArrayList
    foreach ($rel in $texts.Keys) {
        $n = 0
        foreach ($line in ($texts[$rel] -split "`r?`n")) {
            $n++
            if ($line -cmatch $Pattern) { [void]$hits.Add(($rel + ':' + $n + ':' + $line)) }
        }
    }
    return $hits
}

# --- 1. a credential FILE is tracked -----------------------------------------
# The strongest signal there is: not a value that looks like a secret, but a
# file whose whole purpose is to hold one.
$credRe = '(^|/)(\.env(\..+)?|\.dev\.vars(\..+)?|.*\.(pem|key|p12|pfx|keystore|jks)|id_rsa|id_ed25519|id_ecdsa|credentials\.json|service-account.*\.json)$'
$credExempt = '\.(example|sample|template)$'
Add-Hit 'a credential file is tracked' @($files | Where-Object { $_ -match $credRe -and $_ -notmatch $credExempt })

# --- 2. secret-shaped strings ------------------------------------------------
# Each pattern is a vendor's documented token shape. A generic "high entropy"
# rule is deliberately absent: it fires on hashes, minified code and base64
# fixtures, and a check that cries wolf is a check somebody switches off.
$scans = [ordered]@{}
$scans['a private key block']  = 'BEGIN (RSA |OPENSSH |EC |DSA |PGP )?PRIVATE KEY'
$scans['an aws access key id'] = 'AKIA[0-9A-Z]{16}'
$scans['a github token']       = 'gh[pousr]_[A-Za-z0-9]{30,}'
$scans['a slack token']        = 'xox[abprs]-[0-9A-Za-z-]{10,}'
$scans['a google api key']     = 'AIza[0-9A-Za-z_-]{35}'
$scans['a stripe key']         = 'sk_(live|test)_[0-9A-Za-z]{16,}'
$scans['a npm token']          = 'npm_[A-Za-z0-9]{36}'
$scans['a bearer literal']     = 'Bearer [A-Za-z0-9._-]{24,}'
$scans['a password in a url']  = '://[A-Za-z0-9._%+-]+:[^@/\s]{6,}@'

foreach ($name in $scans.Keys) { Add-Hit $name (Find-Pattern $scans[$name]) }

# --- 3. public-only: fingerprints of a private system ------------------------
if ($Public) {
    Add-Hit 'an email address' (Find-Pattern '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}')

    # ⚠ Narrowed, not switched off. A pinned GitHub Action is a 40-hex commit on
    # a PUBLIC repository, and pinning is the SAFE practice this template asks
    # for: a tag moves and a moved tag runs unreviewed code. A rule that fires
    # on correct hardening is a rule somebody disables, so the uses: form is
    # excluded by shape rather than the whole hex rule being dropped.
    #
    # ⚠ A DECLARED PIN is the second such shape: a commit and a SHA-256 written
    # into a script that fetches and verifies code before executing it, so 40
    # hex and 64 hex, both public by construction, both the SAFE practice.
    # ⚠ THE WRAPPER THAT FIRST PRODUCED THIS SHAPE HAS LEFT THIS TREE, and the
    # exclusion stays because docs/containers.md still tells a project to write
    # one. It is an exclusion for a shape this template TEACHES, not for a file
    # it ships.
    # ⛔ Excluded by NAME, narrowly. The hex has to be assigned to an identifier
    # that says it is a pin, because a credential is not assigned to something
    # called PinnedSha256. ⛔ Keep this identical to the sh twin.
    $hex = @(Find-Pattern '\b[0-9a-f]{24,}\b' |
        Where-Object { $_ -notmatch 'uses:\s*[A-Za-z0-9._-]+/[A-Za-z0-9._-]+@[0-9a-f]{40}' } |
        Where-Object { $_ -cnotmatch '[Pp]inned(Ref|Sha256|Commit|Digest)|PINNED_(REF|SHA256)' })
    Add-Hit 'a long hex identifier' $hex

    # ⚠ Narrowed rather than switched off. These are well-known generic paths,
    # not a fingerprint of anybody's machine, and a check that fires on them is
    # one somebody disables. Whenever this produces a false positive, add the
    # generic path here; do not widen the exclusion to the whole rule.
    $homes = @(Find-Pattern '([A-Za-z]:[\/]Users[\/]|/home/|/Users/)[A-Za-z0-9._-]+' |
        Where-Object { $_ -notmatch '/home/(linuxbrew|runner|user|vagrant|ubuntu|node)/' } |
        Where-Object { $_ -notmatch '/Users/(runner|user)/' })
    Add-Hit 'an absolute home path' $homes
}

if ($Json) {
    $pub = 'false'
    if ($Public) { $pub = 'true' }
    Write-Output ('{"schema":"check-no-secrets/1","findings":' + $script:found + ',"public_rules":' + $pub + ',"history_scanned":false}')
    if ($script:found -gt 0) { exit 1 }
    exit 0
}

if ($script:found -gt 0) {
    $script:report | ForEach-Object { Write-Output $_ }
    Write-Output ''
    Write-Output ('⛔ ' + $script:found + ' category/categories matched.')
    Write-Output ''
    Write-Output 'If any of it is a real credential, IN THIS ORDER:'
    Write-Output '  1. ROTATE IT. Now, before anything else. It is compromised from the'
    Write-Output '     moment it was written, and removing the file does not change that.'
    Write-Output '  2. Tell the operator. They own the account.'
    Write-Output '  3. Remove it from the tree, and add the ignore rule.'
    Write-Output '  4. A history rewrite is the operator call and the operator action.'
    Write-Output '     It is tidying after the fix, not the fix.'
    Write-Output ''
    Write-Output 'If it is a false positive, narrow the pattern in this script rather than'
    Write-Output 'switching the check off. See docs/security/secrets.md.'
    exit 1
}

$suffix = ''
if ($Public) { $suffix = ' (public rules included)' }
Write-Output ('no secret shapes found in ' + $files.Count + ' files (tracked plus untracked-not-ignored)' + $suffix)
Write-Output '⚠ This finds the shapes it knows. It is not a clearance: read the diff.'
exit 0
