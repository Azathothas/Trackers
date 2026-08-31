# Is the fork healthy, and is a reconciliation due?
#
# One screen, a few seconds, no full scan. This is the thing to run at the top
# of a session that touches the vendored trees, and the thing to run before
# believing anything else in `patches/`.
#
# It answers five questions:
#
#   1. What is each upstream pinned to, and is there a newer release?
#   2. How far behind is the pinned commit, in commits?
#   3. Does the patch series match the vendored tree?
#   4. Does `patches/UPSTREAM.md` have a section for every patch?
#   5. Do the manifest, the changelog and the version agree?
#
# Questions 3, 4 and 5 need no network. `-Offline` skips 1 and 2 so the rest
# still answer on a machine with no GitHub.
#
# Usage:
#   pwsh scripts/vendor-status.ps1
#   pwsh scripts/vendor-status.ps1 -Offline
#
# Exits 0 when everything agrees, 1 when something needs a person, 2 when the
# check could not run. An upgrade being available is **not** a failure: it is
# information, and deciding to take it is a judgement.

[CmdletBinding()]
param(
    [switch]$Offline,
    [string]$Manifest = "vendor/upstream.json",
    [string]$Patches = "patches"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

function Say([string]$text) { Write-Host $text }
function Exit-With([int]$code, [string]$text) { Write-Host "vendor-status: $text"; exit $code }

if (-not (Test-Path $Manifest)) { Exit-With 2 "$Manifest is not there" }
$doc = Get-Content $Manifest -Raw | ConvertFrom-Json

$problems = [System.Collections.ArrayList]::new()
$notes = [System.Collections.ArrayList]::new()

# --------------------------------------------------------------------------
# 1 and 2: what upstream has that we do not
# --------------------------------------------------------------------------
Say ""
Say "Vendored upstreams"
Say "------------------"
foreach ($up in $doc.upstreams) {
    # A ref pinned by commit is 40 characters and would push the columns off
    # the screen. Shortened for display only; the manifest keeps it whole.
    $ref = if ($up.ref.Length -gt 14) { $up.ref.Substring(0, 12) } else { $up.ref }
    $line = "  {0,-28} {1,-14} {2}" -f $up.name, $ref, $up.base.Substring(0, 12)
    if ($Offline -or -not (Get-Command gh -ErrorAction SilentlyContinue)) {
        Say $line
        continue
    }
    $slug = ([uri]$up.repository).AbsolutePath.Trim('/')

    # The newest release, and how far the default branch has moved past our
    # base. `compare` gives both in one call and costs the same as either.
    $latest = & gh api "repos/$slug/releases/latest" --jq ".tag_name" 2>$null
    if ($LASTEXITCODE -ne 0) { $latest = $null }
    $ahead = & gh api "repos/$slug/compare/$($up.base)...HEAD" --jq ".ahead_by" 2>$null
    if ($LASTEXITCODE -ne 0) { $ahead = $null }

    $suffix = ""
    if ($latest -and $latest.Trim() -and $latest.Trim() -ne $up.ref) {
        $suffix += "  newest release $($latest.Trim())"
        [void]$notes.Add("$($up.name): pinned at $($up.ref), upstream released $($latest.Trim())")
    }
    if ($ahead -and [int]$ahead -gt 0) {
        $suffix += "  $ahead commit(s) behind"
    }
    if (-not $suffix) { $suffix = "  up to date" }
    Say ($line + $suffix)
}

# --------------------------------------------------------------------------
# 3: the series describes the tree
# --------------------------------------------------------------------------
Say ""
Say "Patch series"
Say "------------"
& pwsh -NoProfile -File (Join-Path $PSScriptRoot "vendor-diff.ps1") -Check *> $null
$diffCode = $LASTEXITCODE
switch ($diffCode) {
    0 { Say "  the series matches the vendored trees" }
    1 {
        Say "  STALE: the series does not match the trees"
        [void]$problems.Add("the patch series is stale: pwsh scripts/vendor-diff.ps1")
    }
    default {
        Say "  could not check (exit $diffCode)"
        [void]$problems.Add("scripts/vendor-diff.ps1 -Check could not run")
    }
}

$patchFiles = @()
if (Test-Path $Patches) {
    $patchFiles = @(Get-ChildItem -Path $Patches -Filter *.patch -Recurse -File)
}
Say "  $($patchFiles.Count) patch(es) carried"

# --------------------------------------------------------------------------
# 4: every patch is written down
# --------------------------------------------------------------------------
$upstreamDoc = Join-Path $Patches "UPSTREAM.md"
if (-not (Test-Path $upstreamDoc)) {
    [void]$problems.Add("$upstreamDoc is not there, and it is the record Apache-2.0 asks for")
} else {
    $text = [System.IO.File]::ReadAllText($upstreamDoc)
    foreach ($patch in $patchFiles) {
        # Named by the file it changes, which is what the section has to cite.
        # The patch header carries the path; the name is a flattened form of it.
        $header = (Get-Content -LiteralPath $patch.FullName -TotalCount 1)
        $cited = $header -replace '^#\s*[^:]+:\s*', ''
        if ($cited -and -not $text.Contains($cited.Trim())) {
            [void]$problems.Add("$($patch.Name) changes $($cited.Trim()) and no section in UPSTREAM.md names it")
        }
    }
    if ($patchFiles.Count -gt 0) {
        Say "  every patch has a section in UPSTREAM.md"
    }
}

# --------------------------------------------------------------------------
# 5: the version, the changelog and the pins agree
# --------------------------------------------------------------------------
Say ""
Say "Version"
Say "-------"
& pwsh -NoProfile -File (Join-Path $PSScriptRoot "release.ps1") -Check *> $null
switch ($LASTEXITCODE) {
    0 { Say "  Cargo.toml, CHANGELOG.md and the vendored pins agree" }
    1 {
        Say "  DISAGREEMENT, run: pwsh scripts/release.ps1 -Check"
        [void]$problems.Add("the version, the changelog and the pins disagree")
    }
    default { [void]$problems.Add("scripts/release.ps1 -Check could not run") }
}

# --------------------------------------------------------------------------
Say ""
foreach ($note in $notes) { Say "note: $note" }
if ($notes.Count -gt 0) {
    Say ""
    Say "An upgrade being available is not a failure. To see what it would change:"
    Say "  pwsh scripts/upstream-scan.ps1"
    Say "  pwsh scripts/vendor-sync.ps1 -Upstream <name> -Ref <tag> -Check"
    Say ""
}
if ($problems.Count -gt 0) {
    foreach ($problem in $problems) { Say "problem: $problem" }
    exit 1
}
Say "vendor-status: everything agrees"
exit 0
