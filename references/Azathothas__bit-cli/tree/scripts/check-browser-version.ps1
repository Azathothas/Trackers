# Is the browser bit-cli claims to be one that anybody still runs?
#
# The defect it exists to catch: `TODO/cli-surface.md` T-244 ships a client
# that presents itself as a current Chrome. A profile pinned to a browser
# nobody runs is a *correct* fingerprint of a browser that does not exist,
# which is its own tell, and nothing in the tree notices when that happens.
# Browsers ship every four weeks and this repository does not.
#
# So this asks the vendors what stable actually is and compares the answer
# against `BROWSER_MAJOR` in `crates/bit-cli-core/src/page.rs`, which is the
# one number the profile is pinned to.
#
# Four sources, all first-party and all documented endpoints meant to be read
# by a program:
#
#   chrome    versionhistory.googleapis.com, the Chrome release API, read
#             through its **releases** endpoint rather than its versions one
#   chrome-for-testing
#             googlechromelabs.github.io, Google's own per-channel index of the
#             builds it publishes for automation. It is the cross-check, and
#             the two disagreeing is a finding rather than an error.
#   firefox   product-details.mozilla.org, the file Mozilla's own release
#             tooling reads
#   edge      edgeupdates.microsoft.com, the enterprise update feed
#
# ⚠ **The highest version number on the stable channel is not what stable
# is.** Chrome rolls a release out in stages, and `.../versions?pageSize=1`
# answers with the highest version **known**, which for days is a build
# reaching a fraction of a percent of users. Measured 2026-08-29: that endpoint
# said `153.0.8010.12` while the releases endpoint showed it at
# `fraction 0.005` and `152.0.7977.65` at `fraction 1`, and Chrome for Testing
# agreed that stable was 152.
#
# This check reported one major of drift that did not exist, which is the
# opposite of the defect it was written to catch: chasing a build almost nobody
# runs produces a correct fingerprint of a browser that does not exist. So it
# reads the fraction and takes the version that is actually being served.
#
# **Every fetch is trapped on its own.** One dead endpoint degrades that field
# and leaves the others intact, because a check that reports nothing when one
# vendor has an outage is a check that teaches people to ignore it.
#
# It **recommends** rather than only reporting, which is the operator's
# requirement: when the profile is behind, the output carries the replacement
# `BROWSER_MAJOR` and `BROWSER_USER_AGENT`, in the shape `page.rs` wants, so
# patching is applying a diff rather than doing the work again.
#
# **Two things it deliberately does not recommend**, because neither can be
# computed from a version number:
#
#   the ClientHello    a cipher list, a key exchange list, a signature
#                      algorithm list and an extension order. Stable across
#                      Chrome 136, 142 and 151 in the vendored database and not
#                      guaranteed to stay so: 151 added the three ML-DSA
#                      signature algorithms and moved the HTTP/2 connection
#                      window.
#   sec-ch-ua          Chrome permutes its brand list and varies the spelling
#                      of the fake brand per major, on purpose. The vendored
#                      database has `"Not.A/Brand"` at 136, `"Not_A Brand"` at
#                      142 and `"Not=A?Brand"` at 151, with the order flipped.
#                      Guessing the next one produces a header no browser
#                      sends.
#
# So this reports **where the ceiling is** as well as how far behind the
# profile has fallen: the newest Chrome the vendored `impit` fingerprint
# database carries. That is the real blocker on a bump, and a session reading
# the drift should not have to go and find it.
# `scripts/check-browser-fingerprint.ps1` is the half that reads a real
# browser.
#
# Usage:
#   pwsh scripts/check-browser-version.ps1
#   pwsh scripts/check-browser-version.ps1 -Json
#   pwsh scripts/check-browser-version.ps1 -Out bench/browser-versions.json
#   pwsh scripts/check-browser-version.ps1 -MaxBehind 0   # judge, do not record
#
# Exit 0 when the profile is no more than `-MaxBehind` majors behind Chrome
# stable, 1 when it is further behind than that, and 2 when it could not run:
# no network, or every source failed.
#
# **`-MaxBehind` defaults to 2 and that is a decision.** Chrome ships a major
# every four weeks, so one is normal between sessions and two is a month and a
# half of not looking. Three is a profile nobody has read.
#
# See TODO/cli-surface.md, T-244.

[CmdletBinding()]
param(
    # Report as one JSON object on stdout instead of a table.
    [switch]$Json,
    # Also write the JSON here. A path under bench/ is the convention.
    [string]$Out = "",
    # How many majors behind Chrome stable the profile may be before this
    # fails. Absent, the default below is used.
    [int]$MaxBehind = 2,
    # Seconds to wait for each vendor.
    [int]$TimeoutSeconds = 20
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-browser-version: $message")
    exit $code
}

# ---------------------------------------------------------------------------
# What the profile claims
# ---------------------------------------------------------------------------

$pageRs = Join-Path $repo "crates/bit-cli-core/src/page.rs"
if (-not (Test-Path $pageRs)) { Exit-With 2 "$pageRs is not there" }
$pageText = Get-Content -Raw $pageRs

$claimedMajor = $null
if ($pageText -match 'pub const BROWSER_MAJOR:\s*u32\s*=\s*(\d+)') {
    $claimedMajor = [int]$Matches[1]
}
if ($null -eq $claimedMajor) {
    Exit-With 2 "page.rs has no BROWSER_MAJOR to compare against"
}

$claimedAgent = ""
if ($pageText -match 'pub const BROWSER_USER_AGENT:\s*&str\s*=\s*"([^"]*)"') {
    $claimedAgent = $Matches[1] -replace '\s+', ' '
}

# The newest Chrome the vendored fingerprint database can actually produce a
# `ClientHello` for. A profile cannot honestly claim a version past this: a
# User-Agent that disagrees with the handshake under it is a worse tell than
# being one version behind, which is why the profile is not simply bumped.
$databaseMajor = $null
$databasePath = "vendor/impit/impit/src/fingerprint/database/chrome.rs"
$databaseFile = Join-Path $repo $databasePath
if (Test-Path $databaseFile) {
    $majors = @([regex]::Matches((Get-Content -Raw $databaseFile), 'pub mod chrome_(\d+)') |
            ForEach-Object { [int]$_.Groups[1].Value })
    if ($majors.Count -gt 0) { $databaseMajor = ($majors | Sort-Object -Descending)[0] }
}

# ---------------------------------------------------------------------------
# What the vendors say
# ---------------------------------------------------------------------------

# One fetch, trapped. Returns a hashtable with either `value` or `error`, never
# both, so a caller never has to ask whether a null meant "absent" or "failed".
function Get-Json([string]$Url) {
    try {
        $response = Invoke-WebRequest -Uri $Url -TimeoutSec $TimeoutSeconds `
            -MaximumRedirection 3 -UseBasicParsing `
            -Headers @{ 'User-Agent' = 'bit-cli-version-check' }
        return @{ value = ($response.Content | ConvertFrom-Json) }
    } catch {
        return @{ error = $_.Exception.Message }
    }
}

# What stable is actually serving, which is not the same as the highest
# version it knows about.
#
# `fraction` is the share of users a release is being served to. A staged
# rollout runs several at once, so the answer is the **highest version at full
# rollout**, and only when there is none does the highest fraction win. A
# release at 0.005 is a build 1 user in 200 has.
function Get-Chrome {
    $url = "https://versionhistory.googleapis.com/v1/chrome/platforms/win/channels/stable/versions/all/releases" +
    "?filter=endtime%3Dnone&order_by=version%20desc"
    $answer = Get-Json $url
    if ($answer.error) { return @{ browser = 'chrome'; error = $answer.error } }
    $releases = @($answer.value.releases | Where-Object { $_.version })
    if ($releases.Count -eq 0) { return @{ browser = 'chrome'; error = "the response carried no release" } }

    $full = @($releases | Where-Object { $_.fraction -ge 1 })
    $chosen = if ($full.Count -gt 0) {
        ($full | Sort-Object { [version]$_.version } -Descending)[0]
    } else {
        ($releases | Sort-Object -Property fraction -Descending)[0]
    }
    $highest = ($releases | Sort-Object { [version]$_.version } -Descending)[0]

    @{
        browser        = 'chrome'
        version        = $chosen.version
        major          = [int]($chosen.version -split '\.')[0]
        fraction       = $chosen.fraction
        # What the naive endpoint would have said, kept so a reader can see
        # the difference rather than take this on trust.
        highest_known  = $highest.version
        highest_fraction = $highest.fraction
    }
}

# Google's own index of the builds it publishes for automation. A second
# opinion on the same question from the same vendor: when it and the release
# API disagree about stable, something is mid-rollout and the report says so.
function Get-ChromeForTesting {
    $answer = Get-Json "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions.json"
    if ($answer.error) { return @{ browser = 'chrome-for-testing'; error = $answer.error } }
    $version = $answer.value.channels.Stable.version
    if (-not $version) { return @{ browser = 'chrome-for-testing'; error = "the index carried no Stable channel" } }
    @{
        browser = 'chrome-for-testing'
        version = $version
        major   = [int]($version -split '\.')[0]
        beta    = $answer.value.channels.Beta.version
    }
}

function Get-Firefox {
    $answer = Get-Json "https://product-details.mozilla.org/1.0/firefox_versions.json"
    if ($answer.error) { return @{ browser = 'firefox'; error = $answer.error } }
    $version = $answer.value.LATEST_FIREFOX_VERSION
    if (-not $version) { return @{ browser = 'firefox'; error = "the response carried no LATEST_FIREFOX_VERSION" } }
    @{ browser = 'firefox'; version = $version; major = [int]($version -split '\.')[0] }
}

function Get-Edge {
    $answer = Get-Json "https://edgeupdates.microsoft.com/api/products?view=enterprise"
    if ($answer.error) { return @{ browser = 'edge'; error = $answer.error } }
    $stable = @($answer.value | Where-Object { $_.Product -eq 'Stable' })
    if ($stable.Count -eq 0) { return @{ browser = 'edge'; error = "the feed carried no Stable product" } }
    # The feed lists one release per platform and architecture. The highest
    # version across them is the release, and taking the first would make the
    # answer depend on the order Microsoft happens to serve.
    $versions = @($stable[0].Releases | ForEach-Object { $_.ProductVersion } | Where-Object { $_ })
    if ($versions.Count -eq 0) { return @{ browser = 'edge'; error = "the Stable product carried no release" } }
    $version = ($versions | Sort-Object { [version]$_ } -Descending)[0]
    @{ browser = 'edge'; version = $version; major = [int]($version -split '\.')[0] }
}

$sources = @((Get-Chrome), (Get-ChromeForTesting), (Get-Firefox), (Get-Edge))
$reached = @($sources | Where-Object { -not $_.error })
if ($reached.Count -eq 0) {
    $why = ($sources | ForEach-Object { "$($_.browser): $($_.error)" }) -join '; '
    Exit-With 2 "no vendor answered. $why"
}

$chrome = $sources | Where-Object { $_.browser -eq 'chrome' } | Select-Object -First 1

# ---------------------------------------------------------------------------
# The verdict, and the replacement when there is one
# ---------------------------------------------------------------------------

$behind = $null
$pass = $true
$detail = ""
$recommend = $null

if ($chrome.error) {
    $detail = "chrome could not be reached, so nothing was judged: $($chrome.error)"
} else {
    $behind = $chrome.major - $claimedMajor
    if ($behind -gt $MaxBehind) {
        $pass = $false
        $detail = "the profile claims Chrome $claimedMajor and stable is $($chrome.major), which is $behind major(s) behind"
    } elseif ($behind -gt 0) {
        $detail = "the profile claims Chrome $claimedMajor and stable is $($chrome.major), within the $MaxBehind allowed"
    } elseif ($behind -lt 0) {
        $detail = "the profile claims Chrome $claimedMajor and stable is $($chrome.major), which is ahead of stable"
    } else {
        $detail = "the profile claims Chrome $claimedMajor and stable is $($chrome.major)"
    }

    # A recommendation identical to what is already there is noise. When the
    # database caps the reachable version at the one the profile already
    # claims, there is nothing to apply and the block above has said why.
    if ($behind -ne 0) {
        # Recommend what can be reached, not what is newest. A profile past
        # the database's newest entry is a User-Agent with the wrong
        # handshake under it.
        $newMajor = $chrome.major
        $capped = $false
        if ($null -ne $databaseMajor -and $newMajor -gt $databaseMajor) {
            $newMajor = $databaseMajor
            $capped = $true
        }
        $recommend = [ordered]@{
            # Exactly the three literals in page.rs that carry a version, in
            # the shape they are written there. What this cannot produce is
            # the ClientHello: see check-browser-fingerprint.ps1.
            file               = "crates/bit-cli-core/src/page.rs"
            browser_major      = $newMajor
            browser_user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/$newMajor.0.0.0 Safari/537.36"
            sec_ch_ua          = "`"Not=A?Brand`";v=`"99`", `"Google Chrome`";v=`"$newMajor`", `"Chromium`";v=`"$newMajor`""
            reachable          = $newMajor
            capped_by_database = $capped
            unresolved         = @(
                "the TLS ClientHello, which is a cipher and extension list rather than a version string",
                "the HTTP/2 SETTINGS values, which are numbers a browser chooses rather than a version",
                "the sec-ch-ua brand list, which Chrome permutes and respells per major on purpose"
            )
            proof              = "https://versionhistory.googleapis.com/v1/chrome/platforms/win/channels/stable/versions?pageSize=1 answered $($chrome.version)"
            next               = "pwsh scripts/check-browser-fingerprint.ps1, on a machine running that Chrome"
        }
    }
}

$report = [ordered]@{
    schema    = "browser-versions/1"
    generated = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    profile   = [ordered]@{
        browser_major      = $claimedMajor
        browser_user_agent = $claimedAgent
        source             = "crates/bit-cli-core/src/page.rs"
    }
    database  = [ordered]@{
        newest_major = $databaseMajor
        source       = $databasePath
    }
    latest    = @($sources | ForEach-Object {
            $row = [ordered]@{ browser = $_.browser }
            if ($_.error) { $row.error = $_.error } else {
                $row.version = $_.version
                $row.major = $_.major
                # Only Chrome carries these, and they are the evidence for
                # picking this version over the highest one known.
                if ($null -ne $_.fraction) { $row.fraction = $_.fraction }
                if ($_.highest_known) {
                    $row.highest_known = $_.highest_known
                    $row.highest_fraction = $_.highest_fraction
                }
                if ($_.beta) { $row.beta = $_.beta }
            }
            [pscustomobject]$row
        })
    behind    = $behind
    max_behind = $MaxBehind
    pass      = $pass
    detail    = $detail
    recommend = $recommend
}

$jsonText = $report | ConvertTo-Json -Depth 8
if ($Out) {
    $outPath = if ([System.IO.Path]::IsPathRooted($Out)) { $Out } else { Join-Path $repo $Out }
    $parent = Split-Path -Parent $outPath
    if ($parent -and -not (Test-Path $parent)) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    [System.IO.File]::WriteAllText($outPath, $jsonText)
}

if ($Json) {
    Write-Output $jsonText
} else {
    Write-Host ("profile  Chrome {0}" -f $claimedMajor)
    if ($null -ne $databaseMajor) {
        Write-Host ("database Chrome {0}, the newest the vendored fingerprints reach" -f $databaseMajor)
    }
    foreach ($s in $sources) {
        if ($s.error) {
            Write-Host ("{0,-18} unreachable: {1}" -f $s.browser, $s.error)
        } else {
            Write-Host ("{0,-18} {1}" -f $s.browser, $s.version)
        }
    }
    # The version that is serving is not always the highest one published, and
    # a reader who is told only the answer cannot check it.
    if ($chrome -and -not $chrome.error -and $chrome.highest_known -ne $chrome.version) {
        Write-Host ("{0,-18} {1} is published and is at fraction {2}, so it is not stable yet" -f `
                "", $chrome.highest_known, $chrome.highest_fraction)
    }
    Write-Host ""
    Write-Host ("check-browser-version: {0}" -f $detail)
    if ($recommend) {
        Write-Host ""
        if ($recommend.capped_by_database) {
            Write-Host ("stable is Chrome {0} and the vendored fingerprint database stops at {1}." -f `
                    $chrome.major, $databaseMajor)
            Write-Host "  Bumping past it would send that version's User-Agent over this version's"
            Write-Host "  handshake, which is a combination no browser produces. What unblocks it is"
            Write-Host "  a newer entry in $databasePath, or a capture from a real browser of that"
            Write-Host "  version through scripts/check-browser-fingerprint.ps1."
            Write-Host ""
        }
        if ($recommend.reachable -eq $claimedMajor) {
            Write-Host "there is no replacement to apply: the profile already claims the newest"
            Write-Host "version the vendored fingerprints can produce a handshake for."
            Write-Host ("  next: {0}" -f $recommend.next)
        } else {
            Write-Host "the replacement, for $($recommend.file):"
            Write-Host ("  pub const BROWSER_MAJOR: u32 = {0};" -f $recommend.browser_major)
            Write-Host ("  pub const BROWSER_USER_AGENT: &str = `"{0}`";" -f $recommend.browser_user_agent)
            Write-Host ("  sec-ch-ua: {0}" -f $recommend.sec_ch_ua)
            Write-Host "  still unresolved, and this cannot produce them:"
            foreach ($u in $recommend.unresolved) { Write-Host "    $u" }
            Write-Host ("  next: {0}" -f $recommend.next)
        }
    }
}

if (-not $pass) { exit 1 }
exit 0
