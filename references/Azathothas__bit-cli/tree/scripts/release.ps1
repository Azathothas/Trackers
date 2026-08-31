# Move the version, and write the changelog section that goes with it.
#
# The version lives in one place, `[workspace.package]` in the root Cargo.toml,
# and the released version is driven from the git tag by .github/workflows/
# release.yml. So a release is three things that have to agree: the manifest,
# a `CHANGELOG.md` heading, and a tag. This moves the first two together and
# prints the third for a person to run, because tagging and pushing a release
# is not something a script should decide to do.
#
# The changelog section it writes carries the vendored upstream pins as well as
# the commits. That is the point of writing it here rather than by hand: with
# the dependencies vendored, "what changed" includes which upstream commit the
# binary was built from, and nothing else in the repository puts those two
# facts next to each other.
#
# Usage:
#   pwsh scripts/release.ps1 -Bump minor
#   pwsh scripts/release.ps1 -Version 1.0.0
#   pwsh scripts/release.ps1 -Check
#   pwsh scripts/release.ps1 -Release          # unreleased -> today, prints the tag
#
# Exits 0 on success, 1 when -Check finds a disagreement, 2 when it could not
# run.

[CmdletBinding()]
param(
    [ValidateSet("major", "minor", "patch")]
    [string]$Bump,
    [string]$Version,
    [switch]$Check,
    [switch]$Release,
    [string]$Manifest = "vendor/upstream.json"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

function Say([string]$text) {
    $at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    Write-Host "$at release: $text"
}
function Exit-With([int]$code, [string]$text) { Say $text; exit $code }

$cargoPath = "Cargo.toml"
$changelogPath = "CHANGELOG.md"
foreach ($path in @($cargoPath, $changelogPath)) {
    if (-not (Test-Path $path)) { Exit-With 2 "$path is not there" }
}

$cargo = [System.IO.File]::ReadAllText($cargoPath)
# The workspace version, not any other `version =` in the file. Anchored on the
# [workspace.package] table so a dependency's version can never be mistaken
# for it.
# Every group is named. .NET numbers named groups after unnamed ones, so a
# mixed pattern makes ${3} the version rather than the quote after it, and the
# replacement writes the new version straight into the old one.
$versionPattern = '(?ms)(?<head>\[workspace\.package\].*?^version = ")(?<v>[0-9]+\.[0-9]+\.[0-9]+)(?<tail>")'
$found = [regex]::Match($cargo, $versionPattern)
if (-not $found.Success) { Exit-With 2 "no [workspace.package] version in $cargoPath" }
$current = $found.Groups['v'].Value

$changelog = [System.IO.File]::ReadAllText($changelogPath).Replace("`r`n", "`n")

# -------------------------------------------------------------------------
# -Check: the manifest, the changelog and the vendored pins agree
# -------------------------------------------------------------------------
if ($Check) {
    $problems = [System.Collections.ArrayList]::new()
    $headings = [regex]::Matches($changelog, '(?m)^## (?<v>[0-9]+\.[0-9]+\.[0-9]+)(?<rest>[^\n]*)$')
    if ($headings.Count -eq 0) { [void]$problems.Add("$changelogPath has no version heading") }
    else {
        $newest = $headings[0].Groups['v'].Value
        if ($newest -ne $current) {
            [void]$problems.Add("$cargoPath says $current and the newest $changelogPath heading says $newest")
        }
    }
    if (Test-Path $Manifest) {
        $doc = Get-Content $Manifest -Raw | ConvertFrom-Json
        $section = if ($headings.Count -gt 0) {
            $start = $headings[0].Index
            $end = if ($headings.Count -gt 1) { $headings[1].Index } else { $changelog.Length }
            $changelog.Substring($start, $end - $start)
        } else { "" }
        foreach ($up in $doc.upstreams) {
            $short = $up.base.Substring(0, 12)
            if ($section -notmatch [regex]::Escape($short)) {
                [void]$problems.Add("the newest $changelogPath section does not name $($up.name) at $short, which is what $Manifest pins")
            }
        }
    }
    if ($problems.Count -gt 0) {
        foreach ($problem in $problems) { Say "  $problem" }
        Exit-With 1 "$($problems.Count) disagreement(s)"
    }
    Exit-With 0 "version $current, changelog and vendored pins agree"
}

# -------------------------------------------------------------------------
# -Release: mark the newest section released
# -------------------------------------------------------------------------
if ($Release) {
    $today = (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd")
    $pattern = "(?m)^## $([regex]::Escape($current)), unreleased$"
    if ($changelog -notmatch $pattern) {
        Exit-With 2 "no `"## $current, unreleased`" heading to release"
    }
    $changelog = [regex]::Replace($changelog, $pattern, "## $current, $today")
    [System.IO.File]::WriteAllText($changelogPath, $changelog)
    Say "marked $current released on $today"
    Write-Host ""
    Write-Host "Commit that, then tag and push it yourself:"
    Write-Host "  pwsh scripts/git-sync.ps1 -Message `"Release $current`" -BodyFile <path>"
    Write-Host "  git tag -a v$current -m `"v$current`""
    Write-Host "  git push origin v$current"
    exit 0
}

# -------------------------------------------------------------------------
# A new version
# -------------------------------------------------------------------------
if (-not $Bump -and -not $Version) {
    Exit-With 2 "pass -Bump major|minor|patch, or -Version X.Y.Z, or -Check, or -Release"
}
if ($Bump -and $Version) { Exit-With 2 "-Bump and -Version are two ways to say the same thing" }

if ($Version) {
    if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') { Exit-With 2 "-Version must be X.Y.Z" }
    $next = $Version
} else {
    $parts = $current -split '\.'
    $major = [int]$parts[0]; $minor = [int]$parts[1]; $patch = [int]$parts[2]
    switch ($Bump) {
        "major" { $major++; $minor = 0; $patch = 0 }
        "minor" { $minor++; $patch = 0 }
        "patch" { $patch++ }
    }
    $next = "$major.$minor.$patch"
}
if ($next -eq $current) { Exit-With 2 "already at $next" }

# Where the last section started, so the commit list covers this version only.
# Recorded in the changelog itself as a "Since `<sha>`." line, because there
# are no tags until the first release and a section written by hand has to be
# able to say where it began too.
$sinceMatch = [regex]::Match($changelog, '(?m)^Since `(?<sha>[0-9a-f]{7,40})`\.')
$sinceSha = if ($sinceMatch.Success) { $sinceMatch.Groups['sha'].Value } else { $null }
$head = (& git rev-parse HEAD 2>$null)
if ($LASTEXITCODE -ne 0) { Exit-With 2 "not a git repository" }
$head = $head.Trim()

$subjects = @()
if ($sinceSha) {
    $log = & git log --format="%h %s" "$sinceSha..HEAD" 2>$null
    if ($LASTEXITCODE -eq 0) { $subjects = @($log) }
}
if ($subjects.Count -eq 0 -and -not $sinceSha) {
    Say "no `"Since`" marker in $changelogPath, so this section lists nothing and records where it starts"
}

$lines = [System.Collections.ArrayList]::new()
[void]$lines.Add("## $next, unreleased")
[void]$lines.Add("")
[void]$lines.Add("Since ``$($head.Substring(0,12))``.")
[void]$lines.Add("")

if (Test-Path $Manifest) {
    $doc = Get-Content $Manifest -Raw | ConvertFrom-Json
    [void]$lines.Add("### Vendored upstreams")
    [void]$lines.Add("")
    [void]$lines.Add("The binary is built from these trees, not from crates.io. See")
    [void]$lines.Add("``docs/vendoring.md``.")
    [void]$lines.Add("")
    foreach ($up in $doc.upstreams) {
        [void]$lines.Add("- ``$($up.name)`` at ``$($up.ref)``, commit ``$($up.base.Substring(0,12))``, from $($up.repository)")
    }
    [void]$lines.Add("")
}

if ($subjects.Count -gt 0) {
    [void]$lines.Add("### Changes")
    [void]$lines.Add("")
    [void]$lines.Add("Every commit since the previous section, newest first. A subject in this")
    [void]$lines.Add("repository is a sentence rather than a category, so they are listed as")
    [void]$lines.Add("written rather than sorted into headings that would have to be invented.")
    [void]$lines.Add("")
    foreach ($subject in $subjects) {
        $parts = $subject -split ' ', 2
        if ($parts.Count -eq 2) { [void]$lines.Add("- ``$($parts[0])`` $($parts[1])") }
    }
    [void]$lines.Add("")
}

# Inserted above the newest existing heading, which keeps "newest first".
$firstHeading = [regex]::Match($changelog, '(?m)^## ')
if (-not $firstHeading.Success) { Exit-With 2 "$changelogPath has no `"## `" heading to insert above" }
$section = ($lines -join "`n") + "`n"
$changelog = $changelog.Substring(0, $firstHeading.Index) + $section + $changelog.Substring($firstHeading.Index)

$cargo = [regex]::Replace($cargo, $versionPattern, "`${head}$next`${tail}")

# A workspace member that another member depends on carries the version twice:
# once as its own, and once in the [workspace.dependencies] entry that pins it.
# Cargo refuses to resolve when they disagree, and the message names the new
# version, so it reads as though the bump itself were the problem.
$internalPattern = '(?m)^(?<head>bit-cli-core = \{ path = "crates/bit-cli-core", version = ")(?<v>[0-9]+\.[0-9]+\.[0-9]+)(?<tail>" \})$'
if ([regex]::IsMatch($cargo, $internalPattern)) {
    $cargo = [regex]::Replace($cargo, $internalPattern, "`${head}$next`${tail}")
    Say "bit-cli-core's [workspace.dependencies] pin moved with it"
}
[System.IO.File]::WriteAllText($cargoPath, $cargo)
[System.IO.File]::WriteAllText($changelogPath, $changelog)

Say "$current -> $next in $cargoPath and $changelogPath"
Write-Host ""
Write-Host "Three things follow a version, and CI fails on each of them separately:"
Write-Host "  cargo update --workspace --offline                 # Cargo.lock"
Write-Host "  cargo about generate --config about.toml --output-file THIRD_PARTY.md about.hbs"
Write-Host "  pwsh scripts/gates.ps1"
Write-Host "  pwsh scripts/release.ps1 -Check"
Write-Host ""
Write-Host "THIRD_PARTY.md carries every crate version including this one, and the"
Write-Host "notices job regenerates and diffs it. Bumping without it is a red run."
exit 0
