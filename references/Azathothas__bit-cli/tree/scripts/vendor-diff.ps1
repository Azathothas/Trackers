# What this repository changed in a vendored upstream, as a patch series.
#
# The vendored tree is the truth and this is derived from it, which is the
# model patches/README.md describes. Nothing here is applied to anything: the
# series exists so a change to somebody else's code is reviewable on its own,
# so Apache-2.0's "state the changes" obligation is met by a file rather than
# by memory, and so a reconciliation can be read patch by patch.
#
# One patch per file changed, named after the file, because a vendored change
# is usually one seam in one place and a series grouped by intent would need a
# grouping nobody maintains. The header of each says which upstream commit it
# is against.
#
# Usage:
#   pwsh scripts/vendor-diff.ps1                 # regenerate every series
#   pwsh scripts/vendor-diff.ps1 -Upstream rqbit
#   pwsh scripts/vendor-diff.ps1 -Check          # fail if the series is stale
#
# Exits 0 when the series matches the tree, 1 under -Check when it does not,
# and 2 when the run could not start.

[CmdletBinding()]
param(
    [string]$Upstream = "all",
    [switch]$Check,
    [string]$Manifest = "vendor/upstream.json",
    [string]$Out = "patches",
    [string]$CacheRoot = ".tmp/vendor-pristine"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

function Say([string]$text) {
    $at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    Write-Host "$at vendor-diff: $text"
}
function Exit-With([int]$code, [string]$text) { Say $text; exit $code }

if (-not (Test-Path $Manifest)) { Exit-With 2 "$Manifest is not there" }
$doc = Get-Content $Manifest -Raw | ConvertFrom-Json
$selected = @($doc.upstreams | Where-Object { $Upstream -eq "all" -or $_.name -eq $Upstream })
if ($selected.Count -eq 0) { Exit-With 2 "no upstream named '$Upstream'" }

function Test-Excluded([string]$Relative, [string[]]$Exclude) {
    $head = ($Relative -split '[\\/]')[0]
    foreach ($name in $Exclude) { if ($head -eq $name -or $Relative -eq $name) { return $true } }
    $false
}

# Paths a `.gitignore` **inside the tree** ignores, which is upstream saying
# they are generated rather than source. Building the vendored workspace leaves
# `target/`, `node_modules/` and `crates/librqbit/webui/dist/` behind, none of
# which was ever vendored and none of which can be a local change: a fresh
# clone of the base has none of them.
#
# Derived rather than listed, because upstream already wrote it down. It is not
# in upstream.json's exclude list either, because that list is what this
# repository decided not to vendor and this is not a decision.
#
# The rule has to be "ignored by a .gitignore inside the tree" rather than
# "ignored" flat: a file this repository's own root .gitignore would swallow is
# exactly what scripts/vendor-sync.ps1 has to keep reporting, which is the
# `.vscode/` case docs/vendoring.md describes.
#
# Without this the walk hashed 7.2 GB across 9,894 files and produced 14,964
# patches, and the script looked hung. See TODO/cli-surface.md, T-197.
function Get-TreeIgnored([string]$Root, [System.Collections.Generic.List[string]]$Relatives) {
    $ignored = [System.Collections.Generic.HashSet[string]]::new()
    if ($Relatives.Count -eq 0) { return $ignored }
    $prefix = $Root.Replace([char]92, [char]47).TrimEnd('/')
    # Not $input: inside a function that is the automatic pipeline enumerator,
    # and assigning to it silently breaks the pipe. Same trap as $args.
    $stdinText = ($Relatives | ForEach-Object { "$prefix/$_" }) -join "`n"
    $lines = $stdinText | & git check-ignore -v --no-index --stdin 2>$null
    if ($LASTEXITCODE -gt 1) { return $ignored }
    foreach ($line in @($lines)) {
        $text = ([string]$line).Replace([char]92, [char]47)
        # "<source>:<line>:<pattern>\t<path>"
        $tab = $text.IndexOf("`t")
        if ($tab -lt 0) { continue }
        $source = $text.Substring(0, $tab)
        $path = $text.Substring($tab + 1)
        if (-not $source.StartsWith("$prefix/")) { continue }
        if ($path.StartsWith("$prefix/")) {
            [void]$ignored.Add($path.Substring($prefix.Length + 1))
        }
    }
    $ignored
}

function Get-TreeFiles([string]$Root, [string[]]$Exclude) {
    $files = [System.Collections.Generic.List[string]]::new()
    if (-not (Test-Path $Root)) { return $files }
    $prefix = (Resolve-Path $Root).Path
    foreach ($item in Get-ChildItem -Path $Root -Recurse -File -Force) {
        $relative = $item.FullName.Substring($prefix.Length).TrimStart('\', '/').Replace([char]92, [char]47)
        if (Test-Excluded $relative $Exclude) { continue }
        $files.Add($relative)
    }
    $ignored = Get-TreeIgnored $Root $files
    if ($ignored.Count -eq 0) { return $files }
    $kept = [System.Collections.Generic.List[string]]::new()
    foreach ($f in $files) { if (-not $ignored.Contains($f)) { $kept.Add($f) } }
    $kept
}

$stale = $false

foreach ($up in $selected) {
    $pristine = Join-Path $CacheRoot "$($up.name)@$($up.base)"
    if (-not (Test-Path (Join-Path $pristine ".git"))) {
        # The alias the sync script writes when it fetched by tag rather than
        # by sha. Either is the same tree; whichever is on disk will do.
        $byRef = Join-Path $CacheRoot "$($up.name)@$(($up.ref -replace '[^A-Za-z0-9._-]', '_'))"
        if (Test-Path (Join-Path $byRef ".git")) { $pristine = $byRef }
    }
    if (-not (Test-Path (Join-Path $pristine ".git"))) {
        Exit-With 2 "no pristine copy of $($up.name) at $($up.base). Run: pwsh scripts/vendor-sync.ps1 -Upstream $($up.name) -Check"
    }

    $exclude = @($up.exclude)
    $dir = $up.directory
    $ourFiles = Get-TreeFiles $dir $exclude
    $baseFiles = Get-TreeFiles $pristine $exclude
    $all = [System.Collections.Generic.SortedSet[string]]::new()
    foreach ($set in @($ourFiles, $baseFiles)) { foreach ($f in $set) { [void]$all.Add($f) } }

    $seriesDir = Join-Path $Out $up.name
    $written = [System.Collections.ArrayList]::new()
    $index = 0

    foreach ($relative in $all) {
        $ourPath = Join-Path $dir $relative
        $basePath = Join-Path $pristine $relative
        $inOurs = $ourFiles.Contains($relative)
        $inBase = $baseFiles.Contains($relative)
        if ($inOurs -and $inBase -and (Get-FileHash $ourPath).Hash -eq (Get-FileHash $basePath).Hash) { continue }

        # `git diff --no-index` produces a patch `git apply` understands, from
        # two files neither of which git tracks. It exits 1 when they differ,
        # which is the normal case here and not a failure.
        $left = if ($inBase) { $basePath } else { "/dev/null" }
        $right = if ($inOurs) { $ourPath } else { "/dev/null" }
        $body = & git diff --no-index --no-color -- $left $right 2>&1
        if ($LASTEXITCODE -gt 1) { Exit-With 2 "git diff failed for $relative" }
        if (-not $body) { continue }

        # `--no-index` names both sides by the path it was handed, which here
        # is a cache directory on this machine and a vendor path, spelled with
        # whatever separator the platform uses. A patch has to name the file,
        # not where two copies of it happened to sit, or it reads differently
        # on every machine and `git apply -p1` cannot place it. The three
        # header lines are rewritten to the repository-relative path.
        $rewritten = [System.Collections.Generic.List[string]]::new()
        foreach ($line in @($body)) {
            $text = [string]$line
            if ($text.StartsWith("diff --git ")) {
                $rewritten.Add("diff --git a/$relative b/$relative"); continue
            }
            if ($text.StartsWith("--- ")) {
                $rewritten.Add($(if ($inBase) { "--- a/$relative" } else { "--- /dev/null" })); continue
            }
            if ($text.StartsWith("+++ ")) {
                $rewritten.Add($(if ($inOurs) { "+++ b/$relative" } else { "+++ /dev/null" })); continue
            }
            $rewritten.Add($text)
        }
        $body = $rewritten

        $index++
        $slug = ($relative -replace '[\\/]', '-') -replace '[^A-Za-z0-9._-]', '_'
        $name = "{0:d4}-{1}.patch" -f $index, $slug
        $header = @(
            "# $($up.name): $relative",
            "#",
            "# Against $($up.repository) at $($up.base).",
            "# Generated by scripts/vendor-diff.ps1 from vendor/$($up.name). Do not edit:",
            "# the vendored tree is the truth and this is derived from it. What the",
            "# change is for is patches/UPSTREAM.md.",
            ""
        ) -join "`n"
        [void]$written.Add(@{ Name = $name; Text = $header + (($body -join "`n")) + "`n" })
    }

    # Compare before writing, so -Check can answer and a regeneration that
    # changes nothing does not churn the working tree.
    $existing = @{}
    if (Test-Path $seriesDir) {
        foreach ($file in Get-ChildItem -Path $seriesDir -Filter *.patch -File) {
            $existing[$file.Name] = [System.IO.File]::ReadAllText($file.FullName)
        }
    }
    $differs = $existing.Count -ne $written.Count
    if (-not $differs) {
        foreach ($patch in $written) {
            if (-not $existing.ContainsKey($patch.Name) -or $existing[$patch.Name] -ne $patch.Text) { $differs = $true; break }
        }
    }

    if (-not $differs) {
        Say "$($up.name): $($written.Count) patch(es), unchanged"
        continue
    }
    if ($Check) {
        Say "$($up.name): the series is stale, $($existing.Count) on disk against $($written.Count) from the tree"
        $stale = $true
        continue
    }

    if (Test-Path $seriesDir) { Remove-Item -Path (Join-Path $seriesDir "*.patch") -Force -ErrorAction SilentlyContinue }
    if ($written.Count -eq 0) {
        Say "$($up.name): no local changes, series is empty"
        continue
    }
    New-Item -ItemType Directory -Force -Path $seriesDir | Out-Null
    foreach ($patch in $written) {
        [System.IO.File]::WriteAllText((Join-Path $seriesDir $patch.Name), $patch.Text)
    }
    Say "$($up.name): wrote $($written.Count) patch(es) to $seriesDir"
}


# ---------------------------------------------------------------------------
# The citations in UPSTREAM.md follow the numbering
# ---------------------------------------------------------------------------
#
# Every patch is named `NNNN-<path>.patch` and the number is its position in
# the series, so adding one file renumbers every file after it. `UPSTREAM.md`
# names each patch under `Files:`, and those names then all point at nothing.
#
# That happened three times in one session on 2026-08-22 and cost a `record`
# gate failure each time, with eleven dead paths on the last of them. The
# number is derived, so keeping the citations pointing at it is derived work
# too, and it belongs here rather than in somebody's memory.
#
# Rewritten by suffix: `0009-crates-librqbit-src-limits.rs.patch` and
# `0004-crates-librqbit-src-limits.rs.patch` are the same patch at different
# positions, so the part after the number is the identity. A citation whose
# suffix names no patch on disk is left alone, because that is a real dead
# path and `check-todo.ps1` should keep reporting it.

function Update-PatchCitations {
    # Not `$doc`: that is `upstream.json` in the enclosing scope, and a local
    # of the same name shadows it. TODO/RULES.md section 5 has this exact
    # hazard written down and it was walked into anyway.
    $record = Join-Path $repo "patches/UPSTREAM.md"
    if (-not (Test-Path $record)) { return 0 }

    $bySuffix = @{}
    foreach ($upstream in $doc.upstreams) {
        $dir = Join-Path $repo "patches/$($upstream.name)"
        if (-not (Test-Path $dir)) { continue }
        foreach ($file in Get-ChildItem -Path $dir -Filter *.patch -File) {
            if ($file.Name -match '^(\d{4})-(.+)$') {
                $bySuffix["$($upstream.name)/$($Matches[2])"] = "$($upstream.name)/$($file.Name)"
            }
        }
    }
    if ($bySuffix.Count -eq 0) { return 0 }

    $text = [System.IO.File]::ReadAllText($record)
    $moved = 0
    $updated = [regex]::Replace($text, 'patches/([A-Za-z0-9._-]+)/(\d{4})-([A-Za-z0-9._-]+\.patch)', {
            param($m)
            $key = "$($m.Groups[1].Value)/$($m.Groups[3].Value)"
            if (-not $bySuffix.ContainsKey($key)) { return $m.Value }
            $want = "patches/$($bySuffix[$key])"
            if ($want -ne $m.Value) { $script:CitationsMoved++ }
            $want
        })
    if ($updated -ne $text) {
        [System.IO.File]::WriteAllText($record, $updated)
    }
    return $script:CitationsMoved
}

$script:CitationsMoved = 0
if (-not $Check) {
    $moved = Update-PatchCitations
    if ($moved -gt 0) {
        Say "UPSTREAM.md: $moved patch citation(s) renumbered"
    }
}

if ($stale) {
    Write-Host ""
    Write-Host "Regenerate with: pwsh scripts/vendor-diff.ps1"
    exit 1
}
Say "done"
exit 0
