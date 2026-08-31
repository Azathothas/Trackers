# Is every tracked file one of the kinds this repository keeps?
#
# On 2026-08-23 a 1,000 byte payload reached the remote as `under/inner.bin`.
# Nothing was wrong with the push. It was left in the working tree by an
# acceptance run for T-226, `download --out`, whose third case is
# `--dir .tmp/t226b --out under`: the resolution that case was written to
# demonstrate made a relative `--out` absolute against the working directory,
# so the payload landed at `<repo>/under/inner.bin` instead of under `.tmp/`.
# `git-sync.ps1` stages with `git add -A`, `.gitignore` had no rule that
# covered it, and nothing anywhere compared the result against what this
# repository is supposed to contain. It was committed, pushed, and read by
# nobody for eight commits.
#
# The defect that wrote it is fixed. The reason it reached the remote is not,
# because it was never about that defect: any run that writes into the working
# tree gets the same ride. So this is the check that says what belongs here,
# and it is written so that both halves of that mishap are caught on their own.
#
# Two rules, and either one alone would have stopped this file:
#
#   top level   The first component of a tracked path is one of a fixed set.
#               `under/` was a new top level directory and this repository
#               gains one about once a month, on purpose.
#   kind        Outside `vendor/`, a tracked file's name is one this tree
#               keeps: a known extension or a known exact name. `inner.bin`
#               is neither, wherever it lands.
#
# `vendor/` is exempt from the second rule and only the second. It is
# upstream's tree, it legitimately holds `.bin`, `.torrent`, `.png` and `.svg`
# fixtures, and a reconciliation that had to add each one here would be a
# reconciliation nobody runs. The first rule still applies to it, because
# `vendor/` itself is on the list.
#
# The lists are measured rather than imagined: they are what `git ls-files`
# holds. Adding to them is one line and is meant to be a decision somebody
# makes on purpose, which is the whole point of the check.
#
# A NUL check comes with the kind rule for free. `gates.ps1`'s `text` gate
# reads six extensions; this reads every non-vendor file the kind rule already
# admits, so a control byte in a `.json`, a `.patch` or a `.csv` is caught too.
# That is a superset of what the older gate covers and it costs one pass over
# files that are all text by construction.
#
# It reads the index, not the working tree, so `git-sync.ps1` can run it after
# `git add -A` and before the commit: at that moment the index is exactly what
# the commit will contain. In CI and in `gates.ps1` the index is HEAD, so the
# same call answers "what is in this tree" with no second mode to keep honest.
#
# Usage:
#   pwsh scripts/check-tree.ps1
#   pwsh scripts/check-tree.ps1 -Json bench/tree.json
#
# Exits 0 when every tracked path is accounted for, 1 when one is not, and 2
# when the check could not run.
#
# See TODO/cli-surface.md, T-230.

[CmdletBinding()]
param(
    [string]$Json
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

# The top level of this repository. Everything here is either a directory with
# a reason or a root file something reads by name. A path whose first
# component is not on this list is not a file somebody meant to commit.
$TopLevel = @(
    '.cargo', '.codegraph', '.github',
    '.gitattributes', '.gitignore',
    'CHANGELOG.md', 'Cargo.lock', 'Cargo.toml', 'LICENSE', 'README.md',
    'THIRD_PARTY.md', 'about.hbs', 'about.toml', 'deny.toml',
    'TODO', 'bench', 'crates', 'docs', 'man', 'patches', 'scripts', 'vendor',
    # What this client puts on the wire, captured off it rather than
    # asserted. It is not `bench/`: those are runs, ignored by default and
    # force-added one at a time, and a golden a check reads on every run
    # has to be tracked normally. See `TODO/cli-surface.md`, T-244.
    'fingerprints'
)

# What a file outside `vendor/` may be. Every one of these is text and every
# one of them is here because the tree holds it: `.1` is the man page, `.hbs`
# is the `cargo about` template, `.jq` is the triage filter, `.patch` is the
# vendored series, `.csv` and `.json` are committed benchmark evidence.
#
# `yaml`, `txt` and `sh` are on the list without being in the tree, because
# each is a file this repository could gain in the ordinary course of work and
# none of them can be a payload.
$TextExtensions = @(
    '1', 'csv', 'gitattributes', 'gitignore', 'hbs', 'jq', 'json', 'lock',
    'md', 'patch', 'ps1', 'rs', 'sh', 'toml', 'txt', 'yaml', 'yml'
)

# Files with no extension, by exact name.
$TextNames = @('LICENSE', 'Makefile', 'Dockerfile')

# The bytes that are not text. Tab, newline and carriage return are, and
# nothing else below 32 is. Written as codes rather than as characters, which
# is the rule in TODO/RULES.md section 5 that this gate's older half enforces.
$AllowedControl = @([byte]9, [byte]10, [byte]13)

function Fail([string]$message) {
    [Console]::Error.WriteLine("check-tree: $message")
    exit 2
}

Push-Location $repo
try {
    $tracked = @(& git ls-files 2>$null | Where-Object { $_ -and $_.Trim() })
}
finally {
    Pop-Location
}
if ($LASTEXITCODE -ne 0 -or $tracked.Count -eq 0) {
    Fail "git ls-files returned nothing, so there is no index to check."
}

$problems = [System.Collections.ArrayList]::new()
function Problem([string]$rule, [string]$path, [string]$why) {
    [void]$problems.Add([ordered]@{ rule = $rule; path = $path; why = $why })
}

$checkedForControl = 0
foreach ($relative in $tracked) {
    $normalised = $relative -replace '\\', '/'
    $top = ($normalised -split '/')[0]
    if ($TopLevel -notcontains $top) {
        # No backticks in these strings. PowerShell reads one as an escape, so
        # a quoted flag written the way Markdown writes it loses its backticks
        # at best and gains a newline at worst.
        Problem "top-level" $normalised "'$top' is not one of this repository's top level entries. A run that writes into the working tree lands here and git add -A takes it. If it belongs, add it to the TopLevel list in this script on purpose."
        continue
    }

    if ($normalised -like 'vendor/*') { continue }

    $leaf = ($normalised -split '/')[-1]
    $extension = if ($leaf -match '\.([A-Za-z0-9_+-]+)$') { $Matches[1] } else { $null }
    $known = if ($extension) { $TextExtensions -contains $extension } else { $TextNames -contains $leaf }
    if (-not $known) {
        $what = if ($extension) { "extension '.$extension'" } else { "no extension" }
        Problem "kind" $normalised "$what is not a kind this tree keeps outside vendor/. Payloads and fixtures a run produces belong under .tmp/. If this file is meant to be here, add its kind to this script on purpose."
        continue
    }

    # Known kind, so it is text by construction and a control byte in it is a
    # defect rather than a format. Read once, stop at the first offender.
    $path = Join-Path $repo $normalised
    if (-not (Test-Path -LiteralPath $path)) { continue }
    $checkedForControl++
    $bytes = [System.IO.File]::ReadAllBytes($path)
    for ($i = 0; $i -lt $bytes.Length; $i++) {
        $b = $bytes[$i]
        if ($b -lt 32 -and $AllowedControl -notcontains $b) {
            Problem "control-byte" $normalised ("byte $i is 0x{0:x2}, which is invisible in every editor and makes text tools treat the file as binary" -f $b)
            break
        }
    }
}

if ($Json) {
    $report = [ordered]@{
        kind           = "check-tree"
        generated_at   = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
        tracked        = $tracked.Count
        scanned_bytes  = $checkedForControl
        problems       = @($problems)
        ok             = ($problems.Count -eq 0)
    }
    $jsonPath = if ([System.IO.Path]::IsPathRooted($Json)) { $Json } else { Join-Path $repo $Json }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $jsonPath) | Out-Null
    $report | ConvertTo-Json -Depth 6 | Set-Content -Path $jsonPath -Encoding utf8NoBOM
}

if ($problems.Count -gt 0) {
    foreach ($problem in $problems) {
        [Console]::Error.WriteLine("check-tree: [$($problem.rule)] $($problem.path)")
        [Console]::Error.WriteLine("             $($problem.why)")
    }
    [Console]::Error.WriteLine("check-tree: $($problems.Count) tracked path(s) this repository does not account for.")
    exit 1
}

Write-Host "check-tree: $($tracked.Count) tracked paths, $checkedForControl read for control bytes"
Write-Host "  every one is a kind this repository keeps"
exit 0
