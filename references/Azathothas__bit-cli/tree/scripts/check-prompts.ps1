# Does every path and script a kickoff prompt names still exist?
#
# A prompt is the first thing a session reads, so a prompt naming a script that
# was renamed is worse than a stale citation in TODO/: it sends the session
# down a path that does not exist before it has read anything that could
# correct it. `check-todo.ps1` does this for the record; nothing did it for the
# prompts, which live on the `references` branch and so are never touched by a
# commit that renames a script.
#
# It checks two things and nothing else, because those are the two that rot:
#
#   1. every `scripts/<name>.ps1` a prompt names exists;
#   2. every repository path a prompt names exists.
#
# It does not check prose, and it does not check that a prompt is a good
# prompt.
#
# Usage:
#   pwsh scripts/check-prompts.ps1
#   pwsh scripts/check-prompts.ps1 -Path reference/PROMPT-SAMPLEs
#
# Exits 0 when everything resolves, 1 when something does not, and 2 when the
# prompts are not on this machine, which is the case on a fresh clone that has
# not fetched the corpus.

[CmdletBinding()]
param(
    [string]$Path = "reference/PROMPT-SAMPLEs"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

function Say([string]$text) { Write-Host "check-prompts: $text" }

if (-not (Test-Path $Path)) {
    Say "$Path is not here. It lives on the ``references`` branch:"
    Say "  pwsh -NoProfile -File scripts/git-sync.ps1 -FetchReferences"
    exit 2
}

$files = @(Get-ChildItem -Path $Path -Filter *.md -File)
if ($files.Count -eq 0) { Say "no prompts in $Path"; exit 2 }

$problems = [System.Collections.ArrayList]::new()
$checked = 0

foreach ($file in $files) {
    $lineNo = 0
    foreach ($line in [System.IO.File]::ReadAllLines($file.FullName)) {
        $lineNo++

        # A script the prompt tells somebody to run.
        foreach ($m in [regex]::Matches($line, '(?<![\w./-])scripts/(?<n>[A-Za-z0-9._-]+\.ps1)')) {
            $checked++
            $target = Join-Path "scripts" $m.Groups['n'].Value
            if (-not (Test-Path $target)) {
                [void]$problems.Add("$($file.Name):$lineNo names $target, which is not there")
            }
        }

        # A repository path the prompt tells somebody to read. Restricted to
        # the directories that actually exist at the root, so a sentence
        # mentioning `TODO/` in prose is not mistaken for a file.
        foreach ($m in [regex]::Matches($line, '(?<![\w./-])(?<p>(?:TODO|docs|patches|vendor|crates|bench)/[A-Za-z0-9._/-]+\.(?:md|json|toml|rs|ps1))')) {
            $checked++
            $cited = $m.Groups['p'].Value
            if ($cited -match '<|>') { continue }
            if (-not (Test-Path $cited)) {
                [void]$problems.Add("$($file.Name):$lineNo names $cited, which is not there")
            }
        }

        # A sibling prompt linked from another one.
        foreach ($m in [regex]::Matches($line, '\]\((?<t>PROMPT_[A-Za-z0-9_]+\.md)\)')) {
            $checked++
            $target = Join-Path $Path $m.Groups['t'].Value
            if (-not (Test-Path $target)) {
                [void]$problems.Add("$($file.Name):$lineNo links to $($m.Groups['t'].Value), which is not in $Path")
            }
        }
    }
}

Say "$($files.Count) prompt(s), $checked reference(s) checked"
if ($problems.Count -gt 0) {
    foreach ($problem in $problems) { Say "  $problem" }
    Say "$($problems.Count) problem(s)"
    exit 1
}
Say "everything resolves"
exit 0
