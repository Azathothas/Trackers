# What has happened upstream since the commit we vendored, and which of it
# matters here.
#
# Run this on every version bump and before every reconciliation. A vendored
# dependency stops being visible: nobody sees a release note, nobody sees the
# issue that names the bug we worked around, and the fork drifts from upstream
# with no record of what it drifted past. This is that record.
#
# For each upstream in vendor/upstream.json it fetches **everything**: every
# release, every issue and every pull request the repository has, open and
# closed, plus the commits since our base. Then it triages, because 614 items
# nobody reads is the same as no scan at all.
#
# The vocabulary it triages against is derived from `TODO/INDEX.md` rather than
# written here, so it cannot go stale: the terms are the nouns in the titles of
# entries that are still open, partial or blocked. A short curated list carries
# what a title never says, which is protocol names and upstream type names.
# A curated hit is worth three, a derived hit one, an open item one more, and
# the tiers follow from the total.
#
# It also says whether a newer release exists than the one we are pinned to,
# which is the fact that decides whether a reconciliation is due and the one
# nobody sees once a dependency has no version left to update.
#
# It never writes to TODO/. What it produces is a candidate list, and turning a
# candidate into an entry is a judgement: see patches/TASKS.md.
#
# Usage:
#   pwsh scripts/upstream-scan.ps1
#   pwsh scripts/upstream-scan.ps1 -Upstream rqbit -All
#   pwsh scripts/upstream-scan.ps1 -Since 2026-01-01   # bound issues and PRs too
#
# Exits 0 when the scan completed, 2 when it could not run. A scan that finds
# things is not a failure: it is the normal case and the whole point.

[CmdletBinding()]
param(
    [string]$Upstream = "all",
    # Bound the scan to what changed after this instant. Omitted, the scan
    # covers the repository's whole history of issues and pull requests, and
    # only the commits since our base, because a commit older than the tree we
    # vendored is already in it.
    [string]$Since,
    # Print every item rather than only the ones triage marked. The JSON
    # record always holds everything.
    [switch]$All,
    # Every page, by default. 100 items each, so 200 pages is 20,000 items and
    # is a ceiling rather than a plan. It exists so a mistake cannot make ten
    # thousand calls, not to decide what is worth reading.
    [int]$MaxPages = 200,
    # How many entries a derived term may name before it is noise. A word in
    # half the titles in the record flags everything and says nothing.
    [int]$TermCeiling = 6,
    [string]$Index = "TODO/INDEX.md",
    [string]$Manifest = "vendor/upstream.json",
    [string]$Out = "patches/scan"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

function Say([string]$text) {
    $at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    Write-Host "$at upstream-scan: $text"
}
function Exit-With([int]$code, [string]$text) { Say $text; exit $code }

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    Exit-With 2 "gh is not on PATH. This scan is entirely GitHub API calls."
}
if (-not (Test-Path $Manifest)) { Exit-With 2 "$Manifest is not there" }

# One `gh api` call, with backoff, because this makes hundreds of them.
#
# Three failures are worth retrying and no others. A 403 carrying "rate limit"
# is the secondary limit, which is not the hourly quota and clears in tens of
# seconds. A 429 is the same thing said properly. A 5xx is GitHub. Anything
# else, a 404 or a bad query, is a mistake here and retrying it just makes the
# same mistake more slowly.
#
# The wait doubles from two seconds and is capped, so a scan that hits the
# limit finishes late rather than hammering through it. `gh` itself does not
# back off: it returns the error and exits.
function Invoke-GhApi([string]$Path, [int]$Attempts = 6) {
    $wait = 2
    for ($try = 1; $try -le $Attempts; $try++) {
        $text = & gh api $Path 2>&1
        if ($LASTEXITCODE -eq 0) {
            try { return ($text | ConvertFrom-Json) }
            catch { Exit-With 2 "gh api $Path returned something that is not JSON" }
        }
        $message = ($text | Out-String)
        $retryable = $message -match "rate limit" -or $message -match "429" -or
                     $message -match "was submitted too quickly" -or $message -match "HTTP 5\d\d"
        if (-not $retryable) {
            Say "  gh api $Path failed and is not worth retrying: $($message.Trim())"
            return $null
        }
        if ($try -eq $Attempts) {
            Say "  gh api $Path still rate limited after $Attempts attempts, giving up on this call"
            return $null
        }
        Say "  rate limited, waiting $wait s (attempt $try of $Attempts)"
        Start-Sleep -Seconds $wait
        $wait = [math]::Min($wait * 2, 60)
    }
    $null
}

# A paged endpoint, to a bounded number of pages.
#
# Bounded on purpose. An upstream with ten years of closed issues is not a
# reason to make a thousand calls: what this scan is for is what changed since
# the base, and -Since narrows it further.
function Invoke-GhPaged([string]$Path, [int]$Pages) {
    $all = [System.Collections.ArrayList]::new()
    for ($page = 1; $page -le $Pages; $page++) {
        $joiner = if ($Path.Contains("?")) { "&" } else { "?" }
        $batch = Invoke-GhApi "$Path${joiner}per_page=100&page=$page"
        if ($null -eq $batch) { break }
        $items = @($batch)
        if ($items.Count -eq 0) { break }
        foreach ($item in $items) { [void]$all.Add($item) }
        if ($items.Count -lt 100) { break }
    }
    $all
}

# What this repository cares about, derived from its own record.
#
# The first version of this was a hand-written list of terms, and that is the
# wrong shape: a term list written once describes the entries that existed the
# day it was written, and an entry filed afterwards is invisible to it. The
# vocabulary comes out of `TODO/INDEX.md` now, so it cannot go stale, and the
# curated list below carries only what a title cannot say: protocol names and
# upstream type names that no entry title spells out.
#
# Nothing is filtered away by it. Every item upstream produced is listed and
# recorded; the terms decide what is marked for attention, not what is fetched.
function Get-Interest([string]$IndexPath) {
    $interest = [System.Collections.ArrayList]::new()

    # Words that appear in an entry title and say nothing about the subject.
    # Everything else in a title is a noun this repository has a stake in.
    $stop = @(
        "the", "a", "an", "and", "or", "of", "to", "in", "is", "are", "was",
        "were", "not", "no", "for", "on", "at", "by", "with", "from", "that",
        "this", "it", "its", "has", "have", "had", "does", "do", "did", "can",
        "cannot", "than", "then", "when", "what", "which", "who", "why", "how",
        "one", "two", "every", "any", "all", "some", "more", "most", "own",
        "same", "other", "another", "into", "out", "over", "under", "up",
        "down", "off", "but", "as", "be", "been", "being", "so", "if", "else",
        "each", "per", "there", "here", "still", "yet", "only", "also", "just",
        "measure", "measured", "implemented", "implement", "missing", "used",
        "using", "use", "uses", "says", "say", "said", "make", "makes", "made",
        "take", "takes", "taken", "give", "gives", "given", "goes", "went",
        "run", "runs", "ran", "work", "works", "worked", "need", "needs",
        "file", "files", "line", "lines", "test", "tests", "case", "cases",
        "bit", "cli", "first", "second", "third", "last", "next", "new", "old"
    )
    $stopSet = [System.Collections.Generic.HashSet[string]]::new(
        [string[]]$stop, [System.StringComparer]::OrdinalIgnoreCase)

    $byTerm = @{}
    if (Test-Path $IndexPath) {
        foreach ($line in [System.IO.File]::ReadAllLines($IndexPath)) {
            $row = [regex]::Match($line, '^\|\s*\[(?<id>T-\d{3})\]\([^)]*\)\s*\|[^|]*\|[^|]*\|(?<state>[^|]*)\|(?<title>[^|]*)\|')
            if (-not $row.Success) { continue }
            # A closed entry needs nothing from upstream. Taking terms from one
            # is how a scan ends up flagging four fifths of what it fetched:
            # 152 entry titles produce a vocabulary broad enough to match
            # anything, and 55 open ones produce a vocabulary that means
            # something. `deferred` is Phase C, which by decision 7.4 is not
            # worked on.
            $state = $row.Groups['state'].Value.Replace('*', '').Trim().ToLowerInvariant()
            if ($state -notin @("open", "partial", "blocked", "open, blocked")) { continue }
            $id = $row.Groups['id'].Value
            foreach ($word in ($row.Groups['title'].Value -split '[^A-Za-z0-9_-]+')) {
                $term = $word.Trim().ToLowerInvariant()
                # Four characters is where a word stops being a preposition and
                # starts being a noun often enough to be worth matching.
                if ($term.Length -lt 4) { continue }
                if ($stopSet.Contains($term)) { continue }
                if (-not $byTerm.ContainsKey($term)) { $byTerm[$term] = [System.Collections.Generic.SortedSet[string]]::new() }
                [void]$byTerm[$term].Add($id)
            }
        }
    }

    # What a title never says: the protocol and the upstream type names. These
    # are the words a release note uses and an entry title does not.
    $curated = @{
        "mse" = "T-163"; "obfusc" = "T-163"; "rc4" = "T-163"
        "bep 55" = "T-102"; "holepunch" = "T-102"; "hole punch" = "T-102"
        "bep 54" = "T-167"; "donthave" = "T-167"
        "bep 9" = "T-100"; "bep 6" = "T-100"; "bep 19" = "webseed"
        "peerconnectionhandler" = "T-100, T-102, T-167"
        "torrentstorage" = "T-132"; "storagefactory" = "T-132"
        "close_wait" = "T-020"; "select!" = "T-020"; "accept loop" = "T-020"
        "session_persistence" = "T-016"; "fastresume" = "T-016"
        "cap_lints" = "vendoring"; "msrv" = "vendoring"
        "yanked" = "vendoring"; "rustsec" = "vendoring"; "cve" = "vendoring"
        "breaking change" = "vendoring"; "semver" = "vendoring"
    }

    foreach ($term in $byTerm.Keys) {
        [void]$interest.Add(@{ term = $term; entries = ($byTerm[$term] -join ", "); source = "index" })
    }
    foreach ($term in $curated.Keys) {
        [void]$interest.Add(@{ term = $term; entries = $curated[$term]; source = "curated" })
    }
    $interest
}

# A term that names half the entries in the record is noise. One that names a
# handful is a signal. This keeps the second kind.
function Select-UsefulTerms($Interest, [int]$Ceiling) {
    @($Interest | Where-Object {
        $_.source -eq "curated" -or (($_.entries -split ", ").Count -le $Ceiling)
    })
}

# Which terms an item matches, and where each is allowed to match.
#
# A term derived from an entry title is matched against the upstream **title**
# only. Bodies carry URLs, stack traces and quoted logs, and a word like "http"
# or "listen" appears in all of them: matching a derived term against a body
# flags nearly everything and so says nothing. A curated term is different. It
# is a protocol name or an upstream type name that a title often omits and a
# body spells out, so those are matched against both.
# What a row's flags are worth, and the tier that follows from it.
#
# A curated term is a protocol or type name and is worth three; a term derived
# from an entry title is worth one, because a common noun matches a lot. An
# open item is worth one more than a closed one: a closed pull request is
# history and an open one is a decision somebody can still influence.
#
#   high    a curated hit, or four points of anything
#   medium  two points
#   low     one point
#   none    nothing matched
function Get-Tier($Flags, [string]$State) {
    if ($Flags.Count -eq 0) { return @{ tier = "none"; score = 0 } }
    $score = 0
    $curated = $false
    foreach ($flag in $Flags) {
        if ($flag.weight -eq 3) { $curated = $true }
        $score += $flag.weight
    }
    if ($State -eq "open") { $score += 1 }
    $tier = if ($curated -or $score -ge 4) { "high" } elseif ($score -ge 2) { "medium" } else { "low" }
    @{ tier = $tier; score = $score }
}

function Get-Flags([string]$Title, [string]$Body, $Interest) {
    $title = if ($Title) { $Title.ToLowerInvariant() } else { "" }
    $both = if ($Body) { "$title`n$($Body.ToLowerInvariant())" } else { $title }
    $hits = [System.Collections.ArrayList]::new()
    foreach ($row in $Interest) {
        $haystack = if ($row.source -eq "curated") { $both } else { $title }
        if ($haystack.Contains($row.term)) {
            [void]$hits.Add([ordered]@{
                term = $row.term
                entries = $row.entries
                weight = $(if ($row.source -eq "curated") { 3 } else { 1 })
            })
        }
    }
    $hits
}

# An instant as ISO 8601 UTC, whatever ConvertFrom-Json made of it.
#
# It parses an ISO timestamp into a [DateTime], and interpolating one of those
# into a string uses the current culture: "02/10/2026 16:44:41" on this
# machine. Putting that in a `since=` query gives GitHub something it cannot
# read, and the retry loop then spends two and a half minutes on it. Every
# instant that reaches a URL or the record goes through here. TODO/RULES.md
# section 5 already says ISO 8601 UTC everywhere; this is where it was easiest
# to lose.
function Format-Iso($value) {
    if ($null -eq $value) { return $null }
    if ($value -is [datetime]) { return $value.ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ") }
    $parsed = [datetime]::MinValue
    if ([datetime]::TryParse([string]$value, [ref]$parsed)) {
        return $parsed.ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    }
    [string]$value
}

$doc = Get-Content $Manifest -Raw | ConvertFrom-Json
$selected = @($doc.upstreams | Where-Object { $Upstream -eq "all" -or $_.name -eq $Upstream })
if ($selected.Count -eq 0) { Exit-With 2 "no upstream named '$Upstream'" }

$interest = Select-UsefulTerms (Get-Interest $Index) $TermCeiling
$derived = @($interest | Where-Object { $_.source -eq "index" }).Count
Say "$($interest.Count) term(s) to triage against: $derived from $Index, $($interest.Count - $derived) curated"

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$record = [ordered]@{
    scanned_at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    since      = $Since
    upstreams  = [System.Collections.ArrayList]::new()
}

foreach ($up in $selected) {
    $slug = ([uri]$up.repository).AbsolutePath.Trim('/')
    Say "$($up.name): $slug, base $($up.base.Substring(0,12))"

    # The base commit's own date bounds every other query. Without it a scan
    # of a ten year old repository asks for everything.
    $baseCommit = Invoke-GhApi "repos/$slug/commits/$($up.base)"
    $baseDate = if ($baseCommit) { Format-Iso $baseCommit.commit.committer.date } else { $null }
    $cutoff = if ($Since) { Format-Iso $Since } elseif ($baseDate) { $baseDate } else { $null }
    if (-not $cutoff) {
        Say "  could not date the base commit; pass -Since to bound the scan"
        continue
    }
    Say "  looking at everything after $cutoff"

    # String comparison, and it is correct because both sides are ISO 8601 UTC
    # of the same width, which sorts the same way the instants do. A [DateTime]
    # against a string would compare a date to text.
    # Every release, not only the ones after the base, so the report can say
    # what this tree is behind as well as what is new.
    $allReleases = @(Invoke-GhPaged "repos/$slug/releases" $MaxPages)
    $releases = @($allReleases | Where-Object { $_.published_at -and (Format-Iso $_.published_at) -gt $cutoff })

    # Commits are bounded by the base: one older than the tree we vendored is
    # already in it and nothing here can act on it.
    $commits = @(Invoke-GhPaged "repos/$slug/commits?since=$cutoff" $MaxPages)

    # Issues and pull requests are NOT bounded by the base unless the caller
    # says so. An issue filed two years ago and still open is exactly the kind
    # of thing this repository worked around and needs to know about, and
    # `since` on this endpoint means "updated since", which silently drops it.
    # `issues` returns pull requests too, which is why each is separated on
    # `pull_request` rather than asked for twice.
    $issueQuery = "repos/$slug/issues?state=all&sort=updated&direction=desc"
    if ($Since) { $issueQuery += "&since=$cutoff" }
    $issuesAndPulls = @(Invoke-GhPaged $issueQuery $MaxPages)
    $pulls = @($issuesAndPulls | Where-Object { $null -ne $_.pull_request })
    $issues = @($issuesAndPulls | Where-Object { $null -eq $_.pull_request })

    $rows = [System.Collections.ArrayList]::new()
    foreach ($item in $releases) {
        [void]$rows.Add([ordered]@{ kind = "release"; ref = $item.tag_name; title = $item.name; state = "published"; at = (Format-Iso $item.published_at); url = $item.html_url; tier = "none"; score = 0; flags = @(Get-Flags $item.name $item.body $interest) })
    }
    foreach ($item in $commits) {
        $subject = ($item.commit.message -split "`n")[0]
        [void]$rows.Add([ordered]@{ kind = "commit"; ref = $item.sha.Substring(0, 12); title = $subject; state = "merged"; at = (Format-Iso $item.commit.committer.date); url = $item.html_url; tier = "none"; score = 0; flags = @(Get-Flags $subject $item.commit.message $interest) })
    }
    foreach ($item in $pulls) {
        [void]$rows.Add([ordered]@{ kind = "pr"; ref = "#$($item.number)"; title = $item.title; state = $item.state; at = (Format-Iso $item.updated_at); url = $item.html_url; tier = "none"; score = 0; flags = @(Get-Flags $item.title $item.body $interest) })
    }
    foreach ($item in $issues) {
        [void]$rows.Add([ordered]@{ kind = "issue"; ref = "#$($item.number)"; title = $item.title; state = $item.state; at = (Format-Iso $item.updated_at); url = $item.html_url; tier = "none"; score = 0; flags = @(Get-Flags $item.title $item.body $interest) })
    }

    foreach ($row in $rows) {
        $verdict = Get-Tier $row.flags $row.state
        $row.tier = $verdict.tier
        $row.score = $verdict.score
    }
    $flagged = @($rows | Where-Object { $_.tier -in @("high", "medium") })
    $openIssues = @($issues | Where-Object { $_.state -eq "open" }).Count
    $openPulls = @($pulls | Where-Object { $_.state -eq "open" }).Count
    $high = @($rows | Where-Object { $_.tier -eq "high" }).Count
    $medium = @($rows | Where-Object { $_.tier -eq "medium" }).Count
    Say "  $($commits.Count) commit(s) since the base; $($issues.Count) issue(s) ($openIssues open), $($pulls.Count) pull request(s) ($openPulls open)"
    Say "  of $($rows.Count) item(s): $high need attention, $medium are worth a look, $($rows.Count - $high - $medium) are neither"

    # Is there a newer release than the one this tree is pinned to?
    #
    # Asked here rather than in a script of its own because the releases are
    # already fetched. A tag newer than ours is the single fact that decides
    # whether a reconciliation is due, and it is the thing nobody sees once a
    # dependency has no version left to update.
    $newest = @($allReleases | Sort-Object { Format-Iso $_.published_at } -Descending | Select-Object -First 1)
    $upgrade = $null
    if ($newest.Count -gt 0 -and $newest[0].tag_name -and $newest[0].tag_name -ne $up.ref) {
        $upgrade = $newest[0].tag_name
        Say "  UPGRADE AVAILABLE: pinned at $($up.ref), newest release is $upgrade"
    } elseif ($releases.Count -eq 0 -and $commits.Count -gt 0) {
        Say "  no newer release, but $($commits.Count) commit(s) have landed since the base"
    }

    [void]$record.upstreams.Add([ordered]@{
        name = $up.name; repository = $up.repository; base = $up.base
        base_committed_at = $baseDate; cutoff = $cutoff
        pinned_ref = $up.ref
        newest_release = if ($newest.Count -gt 0) { $newest[0].tag_name } else { $null }
        upgrade_available = $upgrade
        counts = [ordered]@{
            releases_after_base = $releases.Count; releases_total = $allReleases.Count
            commits = $commits.Count
            pulls = $pulls.Count; pulls_open = $openPulls
            issues = $issues.Count; issues_open = $openIssues
            high = $high; medium = $medium; total = $rows.Count
        }
        rows = @($rows)
    })
}

$path = Join-Path $Out "upstream-$stamp.json"
$record | ConvertTo-Json -Depth 12 | Set-Content -Path $path -Encoding utf8
Say "record: $path"

Write-Host ""
foreach ($up in $record.upstreams) {
    Write-Host "$($up.name): $($up.counts.high) need attention, $($up.counts.medium) worth a look, of $($up.counts.total)"
    # `high` and `medium` by default. Everything is in the JSON record either
    # way: this decides what a person reads, not what was kept.
    $show = if ($All) {
        @($up.rows)
    } else {
        @($up.rows | Where-Object { $_.tier -in @("high", "medium") })
    }
    foreach ($row in ($show | Sort-Object -Property @{ Expression = "score"; Descending = $true }, @{ Expression = "at"; Descending = $true })) {
        $mark = switch ($row.tier) { "high" { "!!" } "medium" { " *" } default { "  " } }
        $title = if ($row.title.Length -gt 62) { $row.title.Substring(0, 59) + "..." } else { $row.title }
        Write-Host ("  {0} {1,-7} {2,-9} {3,-6} {4}" -f $mark, $row.kind, $row.ref, $row.state, $title)
        foreach ($flag in $row.flags) { Write-Host "          $($flag.term) -> $($flag.entries)" }
    }
    Write-Host ""
}
$upgrades = @($record.upstreams | Where-Object { $_.upgrade_available })
if ($upgrades.Count -gt 0) {
    Write-Host "Upgrades available:"
    foreach ($up in $upgrades) {
        Write-Host "  $($up.name): pinned at $($up.pinned_ref), newest is $($up.upgrade_available)"
        Write-Host "    pwsh scripts/vendor-sync.ps1 -Upstream $($up.name) -Ref $($up.upgrade_available) -Check"
    }
    Write-Host ""
}
Write-Host "A flag says a person should look, not that anything is wrong."
Write-Host "Turning one into an entry is patches/TASKS.md's job."
exit 0
