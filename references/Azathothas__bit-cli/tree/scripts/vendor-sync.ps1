# Put an upstream tree under vendor/, and move it to a later release without
# losing what we changed.
#
# The model, decided 2026-08-22 and written down in patches/README.md: the
# vendored tree is the truth. We edit it in place like any other source in this
# repository, and the patch series under patches/ is derived from it by
# scripts/vendor-diff.ps1. Nothing is applied at build time and there is no
# step to forget.
#
# What this script does is the hard half of that model: when upstream ships a
# new release, our tree and theirs have both moved from a common base, and the
# two have to be reconciled file by file. `vendor/upstream.json` records the
# base, so a real three-way merge is possible rather than a copy that silently
# throws our work away.
#
#   -Init     first vendoring. Copies a pristine tree in. Refuses if the
#             directory already holds one, because that is the operation that
#             loses work.
#   -Check    say what a merge would do and change nothing.
#   default   three-way merge upstream's new release onto our tree.
#
# Usage:
#   pwsh scripts/vendor-sync.ps1 -Init
#   pwsh scripts/vendor-sync.ps1 -Upstream rqbit -Ref v9.1.0 -Check
#   pwsh scripts/vendor-sync.ps1 -Upstream rqbit -Ref v9.1.0
#
# Exits 0 when everything reconciled cleanly, 1 when a file conflicted and
# needs a person, and 2 when the run could not start.
#
# See patches/README.md for the whole workflow and docs/vendoring.md for why
# the dependencies are vendored at all.

[CmdletBinding()]
param(
    [string]$Upstream = "all",
    [string]$Ref,
    [switch]$Init,
    [switch]$Check,
    [string]$Manifest = "vendor/upstream.json",
    [string]$CacheRoot = ".tmp/vendor-pristine"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

function Say([string]$text) {
    $at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    Write-Host "$at vendor-sync: $text"
}

function Exit-With([int]$code, [string]$text) {
    Say $text
    exit $code
}

# A native command that writes to stderr does not terminate under
# $ErrorActionPreference = 'Stop' from pwsh 7.4, so every git call checks its
# own exit code. See TODO/RULES.md section 5.
function Invoke-Git([string[]]$Arguments, [string]$What) {
    $output = & git @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        Exit-With 2 "$What failed: git $($Arguments -join ' ')`n$output"
    }
    $output
}

if (-not (Test-Path $Manifest)) { Exit-With 2 "$Manifest is not there" }
$doc = Get-Content $Manifest -Raw | ConvertFrom-Json

$selected = @($doc.upstreams | Where-Object { $Upstream -eq "all" -or $_.name -eq $Upstream })
if ($selected.Count -eq 0) {
    Exit-With 2 "no upstream named '$Upstream' in $Manifest. Known: $(($doc.upstreams | ForEach-Object { $_.name }) -join ', ')"
}
if ($Ref -and $selected.Count -ne 1) {
    Exit-With 2 "-Ref names one upstream's release, so pass -Upstream with it"
}

# Whether a repository-relative path is excluded from the vendored tree.
#
# Matched on the first path component, which is all the exclusions this
# manifest needs and keeps the rule readable: a name is either vendored or it
# is not, wherever it appears at the root.
function Test-Excluded([string]$Relative, [string[]]$Exclude) {
    $head = ($Relative -split '[\\/]')[0]
    foreach ($name in $Exclude) {
        if ($head -eq $name) { return $true }
        if ($Relative -eq $name) { return $true }
    }
    $false
}

# Paths a `.gitignore` **inside the tree** ignores, which is upstream saying
# they are generated rather than source. Building the vendored workspace leaves
# `target/`, `node_modules/` and `crates/librqbit/webui/dist/` behind, and a
# fresh clone of the base has none of them.
#
# The rule is "ignored by a .gitignore inside the tree" rather than "ignored"
# flat, and the difference is the whole point here: Get-Swallowed below has to
# keep reporting a file that this repository's own root .gitignore would eat,
# which is the `.vscode/` case, while a file upstream's own .gitignore names is
# not vendored at all and must never reach it. Same function in
# scripts/vendor-diff.ps1. See TODO/cli-surface.md, T-197.
function Get-TreeIgnored([string]$Root, [System.Collections.Generic.List[string]]$Relatives) {
    $ignored = [System.Collections.Generic.HashSet[string]]::new()
    if ($Relatives.Count -eq 0) { return $ignored }
    $prefix = $Root.Replace([char]92, [char]47).TrimEnd('/')
    # Not $input: inside a function that is the automatic pipeline enumerator.
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

# Every vendored file of a tree, repository-relative, forward slashes.
function Get-TreeFiles([string]$Root, [string[]]$Exclude) {
    $files = [System.Collections.Generic.List[string]]::new()
    if (-not (Test-Path $Root)) { return $files }
    $prefix = (Resolve-Path $Root).Path
    foreach ($item in Get-ChildItem -Path $Root -Recurse -File -Force) {
        $relative = $item.FullName.Substring($prefix.Length).TrimStart('\', '/') -replace '\\', '/'
        if (Test-Excluded $relative $Exclude) { continue }
        $files.Add($relative)
    }
    $ignored = Get-TreeIgnored $Root $files
    if ($ignored.Count -eq 0) { return $files }
    $kept = [System.Collections.Generic.List[string]]::new()
    foreach ($f in $files) { if (-not $ignored.Contains($f)) { $kept.Add($f) } }
    $kept
}

# A pristine checkout of one upstream at one ref, cached under .tmp/.
#
# Cloned once per ref and reused, because a merge wants two of them at the same
# time and a reconciliation is run more than once before it is right. The cache
# is gitignored and disposable: delete .tmp/vendor-pristine to force a refetch.
function Get-Pristine([object]$Up, [string]$AtRef) {
    $safe = $AtRef -replace '[^A-Za-z0-9._-]', '_'
    $path = Join-Path $CacheRoot "$($Up.name)@$safe"
    if (Test-Path (Join-Path $path ".git")) {
        Say "pristine $($Up.name) at $AtRef is already fetched"
        return $path
    }
    New-Item -ItemType Directory -Force -Path $CacheRoot | Out-Null
    if (Test-Path $path) { Remove-Item -Recurse -Force $path }
    Say "fetching $($Up.repository) at $AtRef"
    # Not --depth 1: a commit sha cannot be cloned shallow by name, and the
    # merge needs two arbitrary commits of the same repository anyway.
    Invoke-Git @("clone", "--quiet", $Up.repository, $path) "clone $($Up.name)" | Out-Null
    Invoke-Git @("-C", $path, "checkout", "--quiet", $AtRef) "checkout $AtRef" | Out-Null
    $path
}

function Copy-Tree([string]$From, [string]$To, [string[]]$Exclude) {
    foreach ($relative in Get-TreeFiles $From $Exclude) {
        $target = Join-Path $To $relative
        $parent = Split-Path -Parent $target
        if (-not (Test-Path $parent)) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
        Copy-Item -LiteralPath (Join-Path $From $relative) -Destination $target -Force
    }
}

$conflicted = [System.Collections.ArrayList]::new()
$swallowed = [System.Collections.ArrayList]::new()
$touched = $false

# Files this repository's own .gitignore would keep out of a commit.
#
# A vendored tree is only a copy of upstream if every file in it is tracked. An
# ignore rule written for our source, `.vscode/` for instance, silently applies
# to somebody else's tree too: the file lands on this machine, never reaches a
# commit, and a fresh clone builds from a different tree than this one. Then
# every later reconciliation reports it as newly added upstream, forever.
#
# So this is checked rather than assumed, and the answer is either to exclude
# the path in vendor/upstream.json or to un-ignore it. Found the day the trees
# went in, by `.vscode/`.
function Get-Swallowed([string]$Root, [string[]]$Relatives) {
    if ($Relatives.Count -eq 0) { return @() }
    $out = [System.Collections.ArrayList]::new()
    # `git check-ignore` takes paths on stdin and answers with the ignored
    # ones. One call, because a call per file over a few hundred files is
    # several seconds of process creation on Windows.
    $paths = ($Relatives | ForEach-Object { "$Root/$_" }) -join "`n"
    $answer = $paths | & git check-ignore --stdin 2>$null
    if ($LASTEXITCODE -gt 1) { return @() }
    foreach ($line in @($answer)) {
        if ($line) { [void]$out.Add($line.Replace([char]92, [char]47)) }
    }
    $out
}

foreach ($up in $selected) {
    $dir = $up.directory
    $exclude = @($up.exclude)

    # -----------------------------------------------------------------------
    # First vendoring
    # -----------------------------------------------------------------------
    if ($Init) {
        if ((Test-Path $dir) -and @(Get-ChildItem $dir -Force -ErrorAction SilentlyContinue).Count -gt 0) {
            Say "$($up.name): $dir already holds a tree, skipping. -Init never writes over one."
            continue
        }
        $at = if ($Ref) { $Ref } else { $up.ref }
        $pristine = Get-Pristine $up $at
        $resolved = (Invoke-Git @("-C", $pristine, "rev-parse", "HEAD") "rev-parse").Trim()
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        Copy-Tree $pristine $dir $exclude
        $vendored = @(Get-TreeFiles $dir $exclude)
        Say "$($up.name): vendored $($vendored.Count) file(s) from $at ($($resolved.Substring(0,12))) into $dir"
        $up.base = $resolved
        $up.ref = $at
        $touched = $true
        continue
    }

    # -----------------------------------------------------------------------
    # Reconciling a new release
    # -----------------------------------------------------------------------
    if (-not (Test-Path $dir)) {
        Say "$($up.name): $dir is not there. Run with -Init first."
        continue
    }
    $to = if ($Ref) { $Ref } else { $up.ref }
    $basePristine = Get-Pristine $up $up.base
    $newPristine = Get-Pristine $up $to
    $newBase = (Invoke-Git @("-C", $newPristine, "rev-parse", "HEAD") "rev-parse").Trim()

    if ($newBase -eq $up.base) {
        Say "$($up.name): already at $($up.base.Substring(0,12)), nothing to reconcile"
        continue
    }

    $baseFiles = Get-TreeFiles $basePristine $exclude
    $newFiles = Get-TreeFiles $newPristine $exclude
    $ourFiles = Get-TreeFiles $dir $exclude
    $all = [System.Collections.Generic.SortedSet[string]]::new()
    foreach ($set in @($baseFiles, $newFiles, $ourFiles)) { foreach ($f in $set) { [void]$all.Add($f) } }

    $clean = 0; $added = 0; $removed = 0; $ours = 0; $conflicts = 0
    foreach ($relative in $all) {
        $inBase = $baseFiles.Contains($relative)
        $inNew = $newFiles.Contains($relative)
        $inOurs = $ourFiles.Contains($relative)
        $basePath = Join-Path $basePristine $relative
        $newPath = Join-Path $newPristine $relative
        $ourPath = Join-Path $dir $relative

        # Upstream added a file. Nothing of ours can conflict with it.
        if (-not $inBase -and $inNew -and -not $inOurs) {
            if (-not $Check) {
                $parent = Split-Path -Parent $ourPath
                if (-not (Test-Path $parent)) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
                Copy-Item -LiteralPath $newPath -Destination $ourPath -Force
            }
            $added++
            continue
        }
        # Upstream deleted a file. Ours goes only if we never touched it: a
        # file we changed and upstream removed is a decision, not a copy.
        if ($inBase -and -not $inNew -and $inOurs) {
            $sameAsBase = (Get-FileHash $ourPath).Hash -eq (Get-FileHash $basePath).Hash
            if ($sameAsBase) {
                if (-not $Check) { Remove-Item -LiteralPath $ourPath -Force }
                $removed++
            } else {
                [void]$conflicted.Add("$($up.name): $relative was changed here and deleted upstream")
                $conflicts++
            }
            continue
        }
        # Ours only: a file this repository added to the vendored tree.
        if (-not $inNew -and -not $inBase -and $inOurs) { $ours++; continue }
        if (-not $inOurs) { continue }
        if (-not $inNew) { continue }

        # Present on both sides. Identical files are the common case and cost
        # nothing to skip.
        if ((Get-FileHash $ourPath).Hash -eq (Get-FileHash $newPath).Hash) { $clean++; continue }
        if ($inBase -and (Get-FileHash $ourPath).Hash -eq (Get-FileHash $basePath).Hash) {
            # Untouched here, changed upstream: take theirs.
            if (-not $Check) { Copy-Item -LiteralPath $newPath -Destination $ourPath -Force }
            $clean++
            continue
        }
        if (-not $inBase) {
            [void]$conflicted.Add("$($up.name): $relative was added here and added upstream")
            $conflicts++
            continue
        }

        # Changed on both sides. `git merge-file` is the same three-way merge
        # git itself runs, and it marks conflicts in place with the usual
        # markers so an editor and a review both understand them.
        $scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("vendor-merge-" + [System.Guid]::NewGuid().ToString("N"))
        Copy-Item -LiteralPath $ourPath -Destination $scratch -Force
        & git merge-file --marker-size=32 -L "ours ($($up.name))" -L "base $($up.base.Substring(0,12))" -L "upstream $to" $scratch $basePath $newPath 2>&1 | Out-Null
        $mergeCode = $LASTEXITCODE
        if ($mergeCode -lt 0 -or $mergeCode -gt 127) {
            Remove-Item $scratch -Force -ErrorAction SilentlyContinue
            Exit-With 2 "$($up.name): git merge-file could not merge $relative"
        }
        if ($mergeCode -eq 0) {
            if (-not $Check) { Copy-Item -LiteralPath $scratch -Destination $ourPath -Force }
            $clean++
        } else {
            [void]$conflicted.Add("$($up.name): $relative has $mergeCode conflict(s)")
            $conflicts++
            if (-not $Check) { Copy-Item -LiteralPath $scratch -Destination $ourPath -Force }
        }
        Remove-Item $scratch -Force -ErrorAction SilentlyContinue
    }

    Say "$($up.name): $($up.base.Substring(0,12)) -> $($newBase.Substring(0,12)) [$to]"
    Say "  merged $clean, added $added, removed $removed, ours only $ours, conflicted $conflicts"

    if (-not $Check) {
        if ($conflicts -eq 0) {
            $up.base = $newBase
            $up.ref = $to
            $up.vendored_at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
            $touched = $true
        } else {
            Say "  base NOT advanced: resolve the conflicts, then run again to record it"
        }
    }
}

# Every selected upstream, whatever else this run did. A run that found nothing
# to reconcile still has to say the tree on disk is the tree a clone would get.
foreach ($up in $selected) {
    if (-not (Test-Path $up.directory)) { continue }
    foreach ($path in Get-Swallowed $up.directory (Get-TreeFiles $up.directory @($up.exclude))) {
        [void]$swallowed.Add($path)
    }
}

if ($touched -and -not $Check) {
    $doc | ConvertTo-Json -Depth 12 | Set-Content -Path $Manifest -Encoding utf8
    Say "recorded the new base in $Manifest"
}

if ($swallowed.Count -gt 0) {
    Write-Host ""
    Write-Host "these vendored files are ignored by this repository's .gitignore, so they"
    Write-Host "would never be committed and a fresh clone would build a different tree:"
    foreach ($path in $swallowed) { Write-Host "  $path" }
    Write-Host ""
    Write-Host "Either exclude the path in vendor/upstream.json, or un-ignore it."
    exit 1
}

if ($conflicted.Count -gt 0) {
    Write-Host ""
    Write-Host "conflicts, in the vendored tree with markers in place:"
    foreach ($line in $conflicted) { Write-Host "  $line" }
    Write-Host ""
    Write-Host "Resolve them, then:"
    Write-Host "  pwsh scripts/vendor-diff.ps1        # regenerate the patch series"
    Write-Host "  pwsh scripts/gates.ps1              # the tree still has to build"
    Write-Host "  pwsh scripts/vendor-sync.ps1 ...    # again, to record the new base"
    exit 1
}

Say "done"
exit 0
