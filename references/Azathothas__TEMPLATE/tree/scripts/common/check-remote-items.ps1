# check-remote-items.ps1 - what is open against this repository, and does it
# say anything that survives being checked?
#
# ⭐ THE TWIN OF check-remote-items.sh, and the one to prefer on Windows: it
# drives the native gh.exe and git.exe rather than ones inside an msys layer.
#
# The defect this exists to catch is a change accepted on the strength of its
# own description. A bot's pull request title says what it believes it is
# doing. A contributor's issue says what they believe is wrong. Both are
# CLAIMS, and both are usually right, which is exactly what makes the wrong one
# expensive: nobody is looking by the hundredth bump.
#
# ⭐ THIS WAS PAID FOR ON THIS REPOSITORY, TWICE, IN ONE HOUR.
#   1. `actions/checkout` was pinned to v4, and v4 targets Node 20, which the
#      platform had deprecated. The runs were being force-migrated with a
#      warning in a log nobody reads. Resolving a tag is not the same as
#      checking what it declares.
#   2. The replacement pin was v5, chosen by looking only at v5 and v4. v7
#      already existed. A tag resolving cleanly says nothing about whether it
#      is current.
#
# -- WHAT IT VERIFIES, AND IT DOES NOT TAKE THE ITEM'S WORD ------------------
# For every pinned action a pull request proposes:
#   the commit exists AND belongs to the repository the ref names, so a
#   lookalike SHA cannot ride in; the tag in the trailing comment really
#   resolves to that commit; ⭐ the runtime it DECLARES is not one the platform
#   has deprecated; and whether a newer release is already out.
#
# ⛔ IT IS READ ONLY. It never merges, never closes, never comments, never
# approves. Deciding is the operator's. docs/security/remote-ops.md.
#
# ⚠ IT CANNOT TELL YOU WHETHER A CHANGE IS A GOOD IDEA. It checks the facts an
# item asserts about the world. Whether you want the change is a reading.
#
# Usage:
#   pwsh -NoProfile -File scripts/common/check-remote-items.ps1
#   pwsh -NoProfile -File scripts/common/check-remote-items.ps1 -Json
#   pwsh -NoProfile -File scripts/common/check-remote-items.ps1 -Repo OWNER/NAME
#
# Exit codes: 0 nothing open, or nothing open failed a check;
#             1 an item's claim did not survive checking;
#             2 could not run.
#
# ⚠ AN UNREAD ITEM IS NOT A FAILED CHECK, and this used to exit 1 for one. Any
# repository with an open issue was then permanently red, which is how a check
# stops being read: the one state it cannot report is the one it exists for.
# An item needing a reading is counted, named, and exits 0. Only a claim that
# was checked and did not hold exits 1.
#
# ⛔ `-Json` PUTS THE JSON DOCUMENT ON STDOUT AND NOTHING ELSE. It used to
# print the whole human report there first, so piping into a JSON parser failed
# and every other check in this directory was machine-readable while this one
# was not. The report still goes out, on stderr.
#
# ⛔ Read the exit code from this process, unpiped.

[CmdletBinding()]
param(
    [switch]$Json,
    [string]$Repo = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

foreach ($t in 'gh', 'git') {
    if (-not (Get-Command $t -ErrorAction SilentlyContinue)) {
        [Console]::Error.WriteLine("check-remote-items: $t not found")
        exit 2
    }
}
& gh auth status *> $null
if ($LASTEXITCODE -ne 0) {
    [Console]::Error.WriteLine('check-remote-items: gh is not authenticated')
    exit 2
}

$ghArgs = @()
if ($Repo) { $ghArgs = @('--repo', $Repo) }

$script:problems = 0
$script:needsHuman = 0

# ⛔ IN JSON MODE, STDOUT IS RESERVED FOR THE DOCUMENT. Every line the body
# writes for a person goes through here, so the choice of stream is made once
# rather than at fourteen call sites. The report still reaches a terminal; a
# gate runner reading stdout gets the document alone.
function Write-Report([string]$T) {
    if ($Json) { [Console]::Error.WriteLine($T) } else { Write-Output $T }
}

function Write-Note([string]$T)  { Write-Report ('  ' + $T) }
function Write-Bad([string]$T)   { Write-Report ('  ⛔ ' + $T); $script:problems++ }
function Write-Human([string]$T) { Write-Report ('  ⚠ ' + $T); $script:needsHuman++ }

# ⚠ gh on Windows emits CRLF, and a carriage return riding on a value is
# invisible until something types it. Every value read out of gh is stripped.
function Get-Clean($V) {
    if ($null -eq $V) { return '' }
    return (($V | Out-String) -replace "`r", '').Trim()
}

# -- open issues -------------------------------------------------------------
# ⚠ Reported, not judged. An issue is a person's account of a problem and
# nothing here can verify it. What this can do is stop one going unnoticed.
Write-Report ''
Write-Report 'OPEN ISSUES'
$issuesRaw = & gh issue list @ghArgs --state open --limit 50 --json number,title,author,createdAt 2>$null
if ($LASTEXITCODE -ne 0) {
    [Console]::Error.WriteLine('check-remote-items: could not list issues')
    exit 2
}
$issues = @()
$t = Get-Clean $issuesRaw
if ($t) { $issues = @($t | ConvertFrom-Json) }
if ($issues.Count -eq 0) { Write-Note 'none' }
else {
    foreach ($i in $issues) { Write-Report ('  #' + $i.number + ' [' + $i.author.login + '] ' + $i.title) }
    Write-Human ($issues.Count.ToString() + ' open issue(s). Read them; nothing here can verify a report.')
}

# -- open pull requests ------------------------------------------------------
Write-Report ''
Write-Report 'OPEN PULL REQUESTS'
$prsRaw = & gh pr list @ghArgs --state open --limit 50 --json number,title,author,headRefName,files 2>$null
if ($LASTEXITCODE -ne 0) {
    [Console]::Error.WriteLine('check-remote-items: could not list pull requests')
    exit 2
}
$prs = @()
$t = Get-Clean $prsRaw
if ($t) { $prs = @($t | ConvertFrom-Json) }

if ($prs.Count -eq 0) { Write-Note 'none' }
else {
    foreach ($pr in $prs) {
        $n = $pr.number
        Write-Report ''
        Write-Report ('  #' + $n + ' [' + $pr.author.login + '] ' + $pr.title)

        $diff = & gh pr diff @ghArgs $n 2>$null
        if ($LASTEXITCODE -ne 0) { Write-Human ('#' + $n + ': could not read the diff'); continue }
        $diffText = ($diff | Out-String) -replace "`r", ''

        # Every action pin the diff ADDS. The trailing comment is captured too,
        # because a pin whose label disagrees with it is its own defect.
        $pins = New-Object System.Collections.ArrayList
        foreach ($line in ($diffText -split "`n")) {
            if ($line -notmatch '^\+') { continue }
            foreach ($m in [regex]::Matches($line, 'uses:\s*([A-Za-z0-9._-]+/[A-Za-z0-9._-]+)@([0-9a-f]{40})(\s*#\s*(\S+))?')) {
                [void]$pins.Add([pscustomobject]@{
                    Action = $m.Groups[1].Value
                    Sha    = $m.Groups[2].Value
                    Tag    = $m.Groups[4].Value
                })
            }
        }

        if ($pins.Count -eq 0) {
            $paths = ''
            if ($pr.files) { $paths = (($pr.files | ForEach-Object { $_.path }) -join ', ') }
            Write-Note ('touches: ' + $paths)
            Write-Human ('#' + $n + ': nothing mechanically checkable here. Read it.')
            continue
        }

        foreach ($pin in $pins) {
            $action = $pin.Action
            $sha = $pin.Sha
            $tag = $pin.Tag
            $label = 'no label'
            if ($tag) { $label = $tag }
            Write-Report ('    ' + $action + '@' + $sha.Substring(0, 12) + '  (labelled ' + $label + ')')

            # 1. does the commit exist, and in THAT repository?
            & gh api ("repos/$action/commits/$sha") --jq '.sha' *> $null
            if ($LASTEXITCODE -ne 0) {
                Write-Bad ($action + '@' + $sha + ' does not exist in that repository. A pin naming a commit the repo does not have is not a bump.')
                continue
            }
            Write-Note ('      commit exists in ' + $action)

            # 2. does the label resolve to that same commit?
            if ($tag) {
                $tSha = Get-Clean (& gh api ("repos/$action/git/ref/tags/$tag") --jq '.object.sha' 2>$null)
                $tTyp = Get-Clean (& gh api ("repos/$action/git/ref/tags/$tag") --jq '.object.type' 2>$null)
                if ($tTyp -eq 'tag') { $tSha = Get-Clean (& gh api ("repos/$action/git/tags/$tSha") --jq '.object.sha' 2>$null) }
                if (-not $tSha) { Write-Human ('      the label ' + $tag + ' is not a tag in ' + $action) }
                elseif ($tSha -ne $sha) {
                    Write-Bad ('the label says ' + $tag + ' but that tag is ' + $tSha.Substring(0, 12) + ', not the pinned commit. The comment has drifted from the pin.')
                }
                else { Write-Note ('      label ' + $tag + ' matches the pin') }
            }
            else { Write-Human '      no tag comment beside the pin. A bare SHA tells a reader nothing.' }

            # 3. ⭐ what runtime does the PINNED COMMIT declare?
            #    This is the check the Node 20 deprecation got past.
            $rt = ''
            try {
                $yml = Invoke-WebRequest -Uri ("https://raw.githubusercontent.com/$action/$sha/action.yml") -TimeoutSec 20 -UseBasicParsing
                $inRuns = $false
                foreach ($line in (($yml.Content) -split "`r?`n")) {
                    if ($line -match '^runs:') { $inRuns = $true; continue }
                    if ($inRuns -and $line -match '^[^ ]') { break }
                    if ($inRuns -and $line -match '^\s*using:\s*(.+)$') { $rt = $Matches[1]; break }
                }
            }
            catch { $rt = '' }

            # ⚠ THE DECLARED VALUE MAY BE QUOTED, AND THE TEST BELOW MATCHES
            # BARE WORDS. `using: "node24"` is valid YAML and real actions write
            # it that way: astral-sh/setup-uv does. Before this line the raw
            # capture kept its quotes, so a quoted "node20" matched no arm, fell
            # through to the catch-all, and was reported as "unrecognised"
            # instead of the ⛔ this whole check exists to raise. A deprecated
            # runtime evaded the one rule written for it by being spelled the
            # other legal way. Found by running this against a real pull request.
            $rt = ($rt -replace '["'']', '').Trim()

            switch -Regex ($rt) {
                '^$'                       { Write-Human '      could not read action.yml at that commit; runtime unverified' }
                '^(node12|node16|node20)$' { Write-Bad ('it declares ' + $rt + ', which the platform has deprecated. It will run under a forced newer runtime, with a warning nobody reads, until it does not.') }
                '^(node24|docker|composite)$' { Write-Note ('      runtime: ' + $rt) }
                default                    { Write-Human ('      runtime: ' + $rt + ' (unrecognised; check it)') }
            }

            # 4. is anything newer already out?
            $latest = Get-Clean (& gh api ("repos/$action/releases/latest") --jq '.tag_name' 2>$null)
            if ($latest -and $tag -and $latest -ne $tag) {
                Write-Human ('      ' + $latest + ' is already released; this proposes ' + $tag)
            }
            elseif ($latest) { Write-Note ('      ' + $latest + ' is the latest release') }
        }
    }
}

# ⛔ THE TWO MODES REPORT THE SAME VERDICT. They differed once: text exited 1
# whenever anything needed a reading and -Json exited 0 over the same tree, so
# a gate runner saw green where a person saw red. Both twins carried it, so
# check-twins compared them and passed. One exit expression, computed here, is
# what stops that returning.
Write-Report ''
if ($script:problems -gt 0) {
    Write-Report ('⛔ ' + $script:problems + ' claim(s) did not survive checking. Do not merge on the description.')
    $rc = 1
}
elseif ($script:needsHuman -gt 0) {
    Write-Report ('⚠ ' + $script:needsHuman + ' item(s) need a reading. Nothing failed a check; nothing was verified either.')
    $rc = 0
}
else {
    Write-Report '✅ every mechanically checkable claim held.'
    Write-Report '⚠ That is not approval. Whether you want a change is a reading, not a check.'
    $rc = 0
}

if ($Json) {
    Write-Output ('{"schema":"check-remote-items/1","problems":' + $script:problems + ',"needs_human":' + $script:needsHuman + ',"open_prs":' + $prs.Count + '}')
}
exit $rc
