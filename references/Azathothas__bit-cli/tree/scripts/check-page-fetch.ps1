# Measure whether the pages bit-cli has to read are served to a plain client.
#
# The defect it exists to catch: T-244 ships static extraction from a web page,
# and the size of that work turns on one question nobody had measured. If the
# pages a person actually meets a torrent on refuse a plain HTTP client, the
# static tier needs a browser-shaped TLS and HTTP/2 fingerprint underneath it,
# which is a second TLS stack or a vendored fork. If they do not, the static
# tier needs an HTML parser and nothing else, and impersonation is a documented
# contingency rather than a dependency.
#
# So this fetches each page in the list below **once**, with `bit-cli`'s own
# User-Agent, and records what came back. It is a measurement, not a crawler:
#
#   - robots.txt is fetched for each host and honoured. A path that is
#     disallowed for our agent or for `*` is skipped and recorded as skipped,
#     never fetched.
#   - one GET per page, in sequence, with a pause between hosts. Nothing is
#     followed out of the page and no second page on any host is read.
#   - nothing is kept. Bodies land under `.tmp/` for the classifier and the
#     report holds counts, statuses and marker names, not content.
#
# A CAPTCHA or a bot check is recorded as `bot_check` and the run moves on. It
# is never retried, never worked around, and nothing here tries to look like a
# browser: the whole point is to find out what a plain client is served.
#
# The list is derived rather than invented, in three groups:
#
#   mirror       the three public mirrors TODO/RULES.md section 5 already
#                permits this repository to use
#   distro       the download pages of major Linux distributions, which is
#                where a person meets a torrent almost every time
#   index        public torrent indexes that publish a browsable page with no
#                account, all of them carrying freely redistributable content
#
# **`-Extract` runs the shipping extractor over each body it saved**, through
# `loopback-fileserver` so no second request reaches anybody, and records the
# link inventory per page: how many links, and which of the three rules took
# each. That is what turns this from "was the page served" into "and is there
# anything on it we can read", and it is the only place where the extractor
# meets markup nobody in this repository wrote.
#
# Usage:
#   pwsh scripts/check-page-fetch.ps1
#   pwsh scripts/check-page-fetch.ps1 -Json -Out bench/page-fetch.json
#   pwsh scripts/check-page-fetch.ps1 -MaxBlocked 0     # judge, do not record
#
# Exit 0 when the run completed, 1 when more pages were blocked than
# `-MaxBlocked` allows, and 2 when it could not run at all. Without
# `-MaxBlocked` it records the count and does not judge it, which is the
# pattern scripts/check-close-wait.ps1 set: a script measuring something
# outside this tree must not fail a build for it.
#
# See TODO/cli-surface.md, T-244.

[CmdletBinding()]
param(
    # Report as one JSON object on stdout instead of a table.
    [switch]$Json,
    # Also write the JSON here. The path is created if its directory exists.
    [string]$Out = "",
    # Fail when more than this many pages are blocked. Omitted means record
    # the number and judge nothing.
    [int]$MaxBlocked = -1,
    # Only the pages whose group matches this. One of mirror, distro, index.
    [ValidateSet("", "mirror", "distro", "index")]
    [string]$Group = "",
    # Seconds to wait between requests, so one host is never hit twice quickly.
    [double]$Pause = 1.5,
    # Per-request deadline in seconds.
    [int]$TimeoutSeconds = 30,
    # Where bodies are written for the classifier to read.
    [string]$Root = ".tmp/page-fetch",
    # Run the shipping extractor over the saved bodies and record what it found.
    [switch]$Extract,
    [ValidateSet("debug", "release")]
    [string]$Build = "release"
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-page-fetch: $message")
    exit $code
}

# bit-cli's own default User-Agent, from
# crates/bit-cli-core/src/webseed/fetch.rs `default_user_agent`. It names the
# tool and its version and impersonates nothing, which is the client this
# measurement is about.
$version = "0.2.0"
$manifest = Join-Path $repo "Cargo.toml"
if (Test-Path -LiteralPath $manifest) {
    $line = Select-String -LiteralPath $manifest -Pattern '^version = "([0-9][^"]*)"' |
        Select-Object -First 1
    if ($line) { $version = $line.Matches[0].Groups[1].Value }
}
$agent = "bit-cli/$version"

# The pages. Every one is named here and nothing else is fetched.
$pages = @(
    # The three mirrors RULES.md section 5 permits.
    @{ id = "fosstorrents";     group = "mirror"; url = "https://fosstorrents.com/distributions/";                          note = "torrent index for distribution images" }
    @{ id = "alpine-cdn";       group = "mirror"; url = "https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/";     note = "autoindex of release images" }
    @{ id = "arch-mirror";      group = "mirror"; url = "https://geo.mirror.pkgbuild.com/iso/latest/";                      note = "autoindex carrying the release torrent" }

    # Distribution download pages.
    @{ id = "debian-torrent";   group = "distro"; url = "https://www.debian.org/CD/torrent-cd/";                            note = "the page Debian points a torrent user at" }
    @{ id = "debian-cdimage";   group = "distro"; url = "https://cdimage.debian.org/debian-cd/current/amd64/bt-cd/";        note = "the directory the torrents are actually in" }
    @{ id = "ubuntu-alt";       group = "distro"; url = "https://ubuntu.com/download/alternative-downloads";                note = "where Ubuntu publishes its torrents" }
    @{ id = "archlinux";        group = "distro"; url = "https://archlinux.org/download/";                                  note = "carries both a torrent link and a magnet" }
    @{ id = "linuxmint";        group = "distro"; url = "https://linuxmint.com/download.php";                               note = "download page linking per-edition torrents" }
    @{ id = "manjaro";          group = "distro"; url = "https://manjaro.org/products/download/x86";                        note = "a download page built largely in script" }
    @{ id = "tails";            group = "distro"; url = "https://tails.net/install/download/";                              note = "offers a torrent beside the direct image" }
    @{ id = "kali";             group = "distro"; url = "https://www.kali.org/get-kali/";                                   note = "torrent links behind a tabbed layout" }
    @{ id = "fedora-torrent";   group = "distro"; url = "https://torrent.fedoraproject.org/";                               note = "Fedora's own torrent index" }

    # Public indexes that publish a page with no account.
    @{ id = "academictorrents"; group = "index";  url = "https://academictorrents.com/browse.php";                          note = "public index of research datasets" }
    @{ id = "linuxtracker";     group = "index";  url = "https://linuxtracker.org/";                                        note = "public tracker index for distributions" }
    @{ id = "webtorrent-free";  group = "index";  url = "https://webtorrent.io/free-torrents";                              note = "a page of magnet links, no account" }
)

if ($Group) { $pages = @($pages | Where-Object { $_.group -eq $Group }) }
if ($pages.Count -eq 0) { Exit-With 2 "no pages match -Group $Group" }

$outRoot = if ([System.IO.Path]::IsPathRooted($Root)) { $Root } else { Join-Path $repo $Root }
try {
    New-Item -ItemType Directory -Force -Path $outRoot | Out-Null
} catch {
    Exit-With 2 "cannot create $outRoot : $($_.Exception.Message)"
}

# Markers that say a challenge page arrived instead of the page asked for.
# Named individually so the report says which one matched rather than "blocked".
$challengeMarkers = @(
    @{ name = "cf-just-a-moment";  pattern = 'Just a moment\.\.\.' }
    @{ name = "cf-chl";            pattern = '__cf_chl_' }
    @{ name = "cf-attention";      pattern = 'Attention Required!' }
    @{ name = "checking-browser";  pattern = 'Checking your browser' }
    @{ name = "enable-js-cookies"; pattern = 'Enable JavaScript and cookies to continue' }
    @{ name = "ddos-guard";        pattern = '(?i)ddos-guard' }
    @{ name = "captcha";           pattern = '(?i)(h-captcha|g-recaptcha|recaptcha/api\.js)' }
)

# Recorded and never acted on. Cloudflare injects
# `/cdn-cgi/challenge-platform/scripts/jsd/main.js` into pages it serves
# normally, as bot-management telemetry rather than as a challenge, so its
# presence says the origin is behind Cloudflare and nothing more. The first
# run of this script counted it as a challenge and reported two pages blocked
# that were both served in full: kali.org's real download page, 135 KB of it,
# and linuxtracker.org's real index. Keep the two lists apart.
$advisoryMarkers = @(
    @{ name = "cf-challenge-platform"; pattern = '/cdn-cgi/challenge-platform/' }
)

# `href` in all three HTML5 framings: double-quoted, single-quoted, and
# unquoted. The third is not exotic. kali.org serves minified HTML and writes
# every one of its torrent links as `href=https://...iso.torrent>torrent`, so a
# quoted-only pattern reports zero links on a page carrying eight.
$hrefPattern = 'href\s*=\s*(?:"([^"]*)"|''([^'']*)''|([^\s"''=<>`]+))'

function Get-HrefValues([string]$body) {
    $values = @()
    foreach ($m in [regex]::Matches($body, $hrefPattern, 'IgnoreCase')) {
        for ($g = 1; $g -le 3; $g++) {
            if ($m.Groups[$g].Success) { $values += $m.Groups[$g].Value; break }
        }
    }
    return $values
}

# The href's path, with any query and fragment cut off, is what decides whether
# it names a torrent. `?download=1` after the extension is common and does not
# make the link something else.
function Test-TorrentHref([string]$value) {
    $path = ($value -split '[?#]', 2)[0]
    return $path.ToLowerInvariant().EndsWith('.torrent')
}

function Get-Origin([string]$url) {
    $u = [System.Uri]$url
    return "$($u.Scheme)://$($u.Authority)"
}

# Fetch one URL and return a hashtable, never throwing. `error` is set when
# nothing came back at all.
function Invoke-OneGet([string]$url, [string]$ua, [int]$timeout) {
    $result = @{ status = 0; body = ""; contentType = ""; error = ""; finalUrl = $url; bytes = 0 }
    try {
        $response = Invoke-WebRequest -Uri $url -UserAgent $ua -TimeoutSec $timeout `
            -MaximumRedirection 5 -SkipHttpErrorCheck -ErrorAction Stop
        $result.status = [int]$response.StatusCode
        $result.contentType = "$($response.Headers['Content-Type'])"
        $body = $response.Content
        if ($body -is [byte[]]) { $body = [System.Text.Encoding]::UTF8.GetString($body) }
        $result.body = "$body"
        $result.bytes = $result.body.Length
        if ($response.BaseResponse -and $response.BaseResponse.RequestMessage) {
            $result.finalUrl = "$($response.BaseResponse.RequestMessage.RequestUri)"
        }
    } catch {
        $result.error = $_.Exception.Message
    }
    return $result
}

# Parse robots.txt into the rules that apply to our agent, longest match wins.
# Returns $null when robots.txt could not be read, which is treated as "no
# rules published" the way every crawler does.
function Get-RobotRules([string]$text, [string]$agentToken) {
    $groups = @{}
    $current = @()
    $lastWasAgent = $false
    foreach ($raw in ($text -split "`r?`n")) {
        $line = ($raw -replace '#.*$', '').Trim()
        if (-not $line) { continue }
        $parts = $line -split ':', 2
        if ($parts.Count -ne 2) { continue }
        $field = $parts[0].Trim().ToLowerInvariant()
        $value = $parts[1].Trim()
        if ($field -eq 'user-agent') {
            if (-not $lastWasAgent) { $current = @() }
            $current += $value.ToLowerInvariant()
            $lastWasAgent = $true
            foreach ($name in $current) { if (-not $groups.ContainsKey($name)) { $groups[$name] = @() } }
            continue
        }
        $lastWasAgent = $false
        if ($field -ne 'disallow' -and $field -ne 'allow') { continue }
        foreach ($name in $current) { $groups[$name] += , @{ rule = $field; path = $value } }
    }
    $token = $agentToken.ToLowerInvariant()
    foreach ($name in $groups.Keys) {
        if ($token.StartsWith($name) -and $name -ne '*') { return $groups[$name] }
    }
    if ($groups.ContainsKey('*')) { return $groups['*'] }
    return @()
}

# Does robots.txt allow this path? Longest matching rule wins; Allow wins a tie,
# which is what Google's and the RFC 9309 draft's resolution both say.
function Test-RobotAllows($rules, [string]$path) {
    if ($null -eq $rules) { return $true }
    $best = $null
    $bestLen = -1
    foreach ($rule in $rules) {
        $pattern = $rule.path
        if ($pattern -eq '') { continue }
        # Only the literal prefix form and a trailing * are handled. Anything
        # more exotic is treated as a prefix, which errs toward not fetching.
        $prefix = $pattern.TrimEnd('*')
        if ($path.StartsWith($prefix)) {
            $len = $prefix.Length
            if ($len -gt $bestLen -or ($len -eq $bestLen -and $rule.rule -eq 'allow')) {
                $bestLen = $len
                $best = $rule
            }
        }
    }
    if ($null -eq $best) { return $true }
    return $best.rule -eq 'allow'
}

$robotsByOrigin = @{}
$results = @()
$first = $true

foreach ($page in $pages) {
    if (-not $first) { Start-Sleep -Seconds $Pause }
    $first = $false

    $origin = Get-Origin $page.url
    if (-not $robotsByOrigin.ContainsKey($origin)) {
        $robots = Invoke-OneGet "$origin/robots.txt" $agent $TimeoutSeconds
        if ($robots.status -eq 200 -and $robots.body) {
            $robotsByOrigin[$origin] = Get-RobotRules $robots.body $agent
        } else {
            $robotsByOrigin[$origin] = $null
        }
        Start-Sleep -Seconds $Pause
    }

    $path = ([System.Uri]$page.url).AbsolutePath
    $row = [ordered]@{
        id           = $page.id
        group        = $page.group
        url          = $page.url
        note         = $page.note
        robots       = if ($null -eq $robotsByOrigin[$origin]) { "none published" } else { "read" }
        verdict      = ""
        http_status  = 0
        content_type = ""
        bytes        = 0
        torrent_links = 0
        magnet_links = 0
        markers      = @()
        advisory     = @()
        detail       = ""
    }

    if (-not (Test-RobotAllows $robotsByOrigin[$origin] $path)) {
        $row.verdict = "skipped"
        $row.detail = "robots.txt disallows $path for $agent"
        $results += [pscustomobject]$row
        Write-Host ("{0,-18} {1}" -f $page.id, "skipped, robots.txt disallows it")
        continue
    }

    $got = Invoke-OneGet $page.url $agent $TimeoutSeconds
    $row.http_status = $got.status
    $row.content_type = $got.contentType
    $row.bytes = $got.bytes

    if ($got.error) {
        $row.verdict = "error"
        $row.detail = $got.error
    } else {
        $matched = @()
        foreach ($marker in $challengeMarkers) {
            if ($got.body -match $marker.pattern) { $matched += $marker.name }
        }
        $advisory = @()
        foreach ($marker in $advisoryMarkers) {
            if ($got.body -match $marker.pattern) { $advisory += $marker.name }
        }
        $row.markers = $matched
        $row.advisory = $advisory
        # A .torrent href or a magnet: URI. This is not the extractor and does
        # not resolve anything against the document; it says whether there is
        # something on the page to extract, so a page that is served and
        # carries nothing is told apart from a page that is served and carries
        # links.
        $hrefs = Get-HrefValues $got.body
        $row.torrent_links = @($hrefs | Where-Object { Test-TorrentHref $_ }).Count
        $row.magnet_links = @($hrefs | Where-Object { $_.ToLowerInvariant().StartsWith('magnet:') }).Count

        if ($got.status -ge 200 -and $got.status -lt 300) {
            if ($matched.Count -gt 0) {
                $row.verdict = "bot_check"
                $row.detail = "200 carrying a challenge page: $($matched -join ', ')"
            } else {
                $row.verdict = "served"
                $row.detail = "$($row.torrent_links) torrent href(s), $($row.magnet_links) magnet URI(s)"
                if ($advisory.Count -gt 0) { $row.detail += " [$($advisory -join ', ')]" }
            }
        } elseif ($got.status -in 401, 403, 429, 503) {
            $row.verdict = "bot_check"
            $row.detail = "HTTP $($got.status)$(if ($matched.Count) { ": $($matched -join ', ')" })"
        } else {
            $row.verdict = "refused"
            $row.detail = "HTTP $($got.status)"
        }

        $bodyFile = Join-Path $outRoot "$($page.id).html"
        try { [System.IO.File]::WriteAllText($bodyFile, $got.body) } catch { }
    }

    $results += [pscustomobject]$row
    Write-Host ("{0,-18} {1,-10} {2}" -f $page.id, $row.verdict, $row.detail)
}

$served = @($results | Where-Object { $_.verdict -eq 'served' }).Count
$blocked = @($results | Where-Object { $_.verdict -in 'bot_check', 'refused' }).Count
$errored = @($results | Where-Object { $_.verdict -eq 'error' }).Count
$skipped = @($results | Where-Object { $_.verdict -eq 'skipped' }).Count

# Every page failing to connect is a machine with no network, not a finding.
if ($errored -eq $results.Count -and $results.Count -gt 0) {
    Exit-With 2 "every page failed to connect; this machine has no route to any of them"
}

$judged = $MaxBlocked -ge 0
$pass = (-not $judged) -or ($blocked -le $MaxBlocked)

# ---------------------------------------------------------------------------
# What the shipping extractor makes of the bodies that were saved
# ---------------------------------------------------------------------------
#
# Through `loopback-fileserver` and `bit-cli` itself, so this is the extractor
# that ships rather than a second implementation of it in PowerShell. The
# counting above is a regex and is deliberately kept: it answers "is there
# something on this page at all", and a disagreement between the two is worth
# seeing.
#
# No request leaves this machine. The bodies were fetched once, above.
$extraction = $null
if ($Extract) {
    $exeDir = Join-Path $repo "target/$Build"
    $bit = Join-Path $exeDir "bit-cli.exe"
    if (-not (Test-Path $bit)) { $bit = Join-Path $exeDir "bit-cli" }
    $fileServer = Join-Path $exeDir "examples/loopback-fileserver.exe"
    if (-not (Test-Path $fileServer)) { $fileServer = Join-Path $exeDir "examples/loopback-fileserver" }
    if (-not (Test-Path $bit) -or -not (Test-Path $fileServer)) {
        Write-Host "  -Extract needs a build: cargo build --$Build --bins --examples"
    } else {
        $serverOut = Join-Path $outRoot "server.txt"
        $server = Start-Process -FilePath $fileServer -PassThru -NoNewWindow `
            -ArgumentList @('--root', $outRoot, '--port', '0') `
            -RedirectStandardOutput $serverOut -RedirectStandardError (Join-Path $outRoot "server.err")
        $base = $null
        for ($i = 0; $i -lt 100; $i++) {
            Start-Sleep -Milliseconds 100
            $first = Get-Content $serverOut -TotalCount 1 -ErrorAction SilentlyContinue
            if ($first) { $base = "$first".Trim().TrimEnd('/'); break }
            if ($server.HasExited) { break }
        }
        if (-not $base) {
            Write-Host "  -Extract could not start the file server"
            if (-not $server.HasExited) { $server | Stop-Process -Force -ErrorAction SilentlyContinue }
        } else {
            $rows = @()
            try {
                foreach ($row in $results) {
                    $bodyFile = Join-Path $outRoot "$($row.id).html"
                    if (-not (Test-Path $bodyFile)) { continue }
                    $runOut = Join-Path $outRoot "$($row.id).extract.json"
                    Start-Process -FilePath $bit -NoNewWindow -Wait `
                        -ArgumentList @('info', "$base/$($row.id).html", '--json', '--timeout', '20s') `
                        -RedirectStandardOutput $runOut -RedirectStandardError "$runOut.err" | Out-Null
                    $links = @()
                    $single = $false
                    try {
                        $doc = Get-Content -Raw $runOut | ConvertFrom-Json
                        # `@($null)` is a one-element array in PowerShell, so a
                        # page with no links would count as one without this.
                        $links = @($doc.context.page_links | Where-Object { $_ })
                        # A page with exactly **one** link is not refused: it
                        # resolves, and the link never appears in a list. The
                        # run then reports on the torrent behind it, so the
                        # page's own URL is not what the context names.
                        if ($links.Count -eq 0 -and $doc.context.url -and
                            $doc.context.url -ne "$base/$($row.id).html") {
                            $single = $true
                        }
                    } catch { }
                    $byRule = @{}
                    foreach ($link in $links) {
                        $rule = if ($link.matched) { $link.matched } else { "unknown" }
                        $byRule[$rule] = 1 + [int]$byRule[$rule]
                    }
                    $rows += [pscustomobject][ordered]@{
                        id        = $row.id
                        links     = if ($single) { 1 } else { $links.Count }
                        single    = $single
                        extension = [int]$byRule['extension']
                        type      = [int]$byRule['type']
                        label     = [int]$byRule['label']
                    }
                }
            } finally {
                if (-not $server.HasExited) { $server | Stop-Process -Force -ErrorAction SilentlyContinue }
            }
            $extraction = $rows
            Write-Host ""
            Write-Host "  what the extractor found, by rule:"
            foreach ($r in $rows) {
                $how = if ($r.single) { "  (one link, resolved rather than listed)" } else { "" }
                Write-Host ("    {0,-18} {1,4} link(s)   extension {2,3}  type {3,3}  label {4,3}{5}" -f `
                        $r.id, $r.links, $r.extension, $r.type, $r.label, $how)
            }
        }
    }
}

$report = [ordered]@{
    schema      = "page-fetch/2"
    generated   = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    user_agent  = $agent
    pages       = $results.Count
    served      = $served
    blocked     = $blocked
    errored     = $errored
    skipped     = $skipped
    judged      = $judged
    max_blocked = if ($judged) { $MaxBlocked } else { $null }
    pass        = $pass
    extraction  = $extraction
    results     = $results
}

$jsonText = $report | ConvertTo-Json -Depth 6
if ($Out) {
    $outPath = if ([System.IO.Path]::IsPathRooted($Out)) { $Out } else { Join-Path $repo $Out }
    [System.IO.File]::WriteAllText($outPath, $jsonText)
}
if ($Json) {
    Write-Output $jsonText
} else {
    Write-Host ""
    Write-Host ("check-page-fetch: {0} page(s), {1} served, {2} blocked, {3} errored, {4} skipped" -f `
            $results.Count, $served, $blocked, $errored, $skipped)
    Write-Host ("  user agent: {0}" -f $agent)
    if (-not $judged) { Write-Host "  recorded, not judged; pass -MaxBlocked to judge it" }
}

if (-not $pass) { exit 1 }
exit 0
