# Drive a Metalink from a real mirror network, over the real internet.
#
# `scripts/check-metalink.ps1` proves the behaviour against a loopback server.
# This proves it against a document nobody here wrote: a MirrorBrain instance
# generating an RFC 5854 `.meta4` on demand, with dozens of real mirrors, real
# priorities, real `location` codes, a `<pieces>` block, an OpenPGP
# `<signature>`, and a published checksum this repository did not compute.
#
# It is the acceptance's last clause for `TODO/cli-surface.md` T-113: "Run
# against a real `.meta4`".
#
# **One thing no real instance serves.** MirrorBrain emits
# `<metaurl mediatype="torrent">` only when its operator has configured
# torrents, and no reachable instance had in August 2026: neither
# `download.documentfoundation.org` nor `download.opensuse.org` puts a
# `<metaurl>` in any document they generate. So the real document alone has
# nothing to download from and case `real_as_served` records exactly that: exit
# 4, naming how many mirrors it did have.
#
# Case `real_with_torrent` closes the gap without faking anything that can be
# real. It takes the real document, downloads the real payload once with
# `curl`, builds a `.torrent` over those exact bytes, serves only that torrent
# from loopback, and adds the one `<metaurl>` line. Everything else is the
# document the mirror generated: the payload comes down over the public
# internet from the mirror list the mirror chose, and the checksum it is
# verified against is the one The Document Foundation published.
#
# Five cases:
#
#   real_as_served     The `.meta4` exactly as the mirror generated it. Exit 4,
#                      and the message names the mirror count.
#   real_by_url        The same document named by its URL rather than by the
#                      saved copy, which is how a MirrorBrain document is
#                      normally met. Same exit code and same reason as
#                      real_as_served. This is T-154's acceptance.
#   real_v3_as_served  The same file as Metalink 3, which the same instance
#                      also generates. The version 3 path against a document
#                      nobody here wrote.
#   real_dry_run       `--dry-run` over the real document: every mirror, the
#                      published size and checksum, no network at all.
#   real_with_torrent  The real document plus one `<metaurl>`. Downloads from
#                      the real mirrors and verifies the published sha-256.
#
# Usage:
#   pwsh scripts/check-metalink-real.ps1
#   pwsh scripts/check-metalink-real.ps1 -Meta4Url <URL>
#
# Exits 0 when every case behaves as described, 1 when one does not, and 2 when
# the check could not run, which includes the mirror being unreachable. A
# network failure is reported as "could not run" and never as a pass: this
# check is about a real mirror, so a run that did not reach one proved nothing.
# The record goes to bench/metalink-real-<timestamp>.json.
#
# See TODO/cli-surface.md, T-113.

[CmdletBinding()]
param(
    # A MirrorBrain-generated RFC 5854 document. The default is a LibreOffice
    # help pack: 3.8 MiB, small enough to fetch twice in a check and large
    # enough to be a real multi-piece torrent.
    [string]$Meta4Url = "https://download.documentfoundation.org/libreoffice/stable/25.8.7/win/x86_64/LibreOffice_25.8.7_Win_x86-64_helppack_ast.msi.meta4",
    [string]$Root = ".tmp/metalink-real",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [int]$TimeoutSeconds = 300,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

$script:Server = $null

function Stop-Background {
    if ($script:Server -and -not $script:Server.HasExited) {
        try { $script:Server.Kill() } catch { }
    }
    $script:Server = $null
}

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-metalink-real: $message")
    Stop-Background
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

trap { Stop-Background; throw }

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
$fileServer = Join-Path $repo "target/$Profile/examples/loopback-fileserver$exe"
foreach ($needed in @($bitCli, $fileServer)) {
    if (-not (Test-Path $needed)) {
        Exit-With 2 "missing $needed. Build it first: cargo build --$Profile --workspace --bins --examples"
    }
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# A browser user agent, because some mirrors refuse anything else. The testing
# policy in TODO/RULES.md: if a site blocks this, say so and name it. That is
# what the exit-2 paths below do.
$userAgent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0 Safari/537.36"

# ---------------------------------------------------------------------------
# The documents, as the mirror generates them
# ---------------------------------------------------------------------------

$metalinkUrl = $Meta4Url -replace '\.meta4$', '.metalink'
$asServed = Join-Path $Root "real.meta4"
$asServedV3 = Join-Path $Root "real.metalink"

# `-OutFile` rather than reading `.Content`: in pwsh 7 that property is a
# string for a text content type, and a metalink round-tripped through a string
# is no longer the bytes the mirror sent.
function Get-Remote([string]$url, [string]$destination) {
    try {
        Invoke-WebRequest -Uri $url -UserAgent $userAgent -TimeoutSec 120 `
            -MaximumRedirection 5 -OutFile $destination | Out-Null
    }
    catch {
        Exit-With 2 "cannot fetch $url : $($_.Exception.Message). The mirror is unreachable or is refusing this client; this check proves nothing without it."
    }
    if (-not (Test-Path $destination)) {
        Exit-With 2 "$url wrote nothing to $destination, so there is no real document to check against."
    }
    (Get-Item $destination).Length
}

Write-Step "fetching $Meta4Url"
$meta4Bytes = Get-Remote $Meta4Url $asServed
Write-Step "fetching $metalinkUrl"
$metalinkBytes = Get-Remote $metalinkUrl $asServedV3

# What the document itself says, read with an XML parser rather than by
# pattern, so the expectations below come from the document and not from this
# script's idea of it.
[xml]$document = Get-Content $asServed -Raw
$namespace = New-Object System.Xml.XmlNamespaceManager($document.NameTable)
$namespace.AddNamespace("m", "urn:ietf:params:xml:ns:metalink")
$fileNode = $document.SelectSingleNode("//m:file", $namespace)
if (-not $fileNode) { Exit-With 2 "$Meta4Url has no <file> element, so it is not the document this expects." }
$fileName = $fileNode.GetAttribute("name")
$publishedSize = [int64]$fileNode.SelectSingleNode("m:size", $namespace).InnerText
$publishedSha256 = ($fileNode.SelectNodes("m:hash", $namespace) |
    Where-Object { $_.GetAttribute("type") -eq "sha-256" } |
    Select-Object -First 1).InnerText.Trim().ToLower()
$mirrorNodes = @($fileNode.SelectNodes("m:url", $namespace))
$httpMirrors = @($mirrorNodes | Where-Object { $_.InnerText.Trim() -match '^https?://' })
$torrentNodes = @($fileNode.SelectNodes("m:metaurl", $namespace) |
    Where-Object { $_.GetAttribute("mediatype") -eq "torrent" })
$hasPieces = [bool]$fileNode.SelectSingleNode("m:pieces", $namespace)
$hasSignature = [bool]$fileNode.SelectSingleNode("m:signature", $namespace)

if (-not $publishedSha256) { Exit-With 2 "$Meta4Url carries no sha-256, so there is nothing to verify against." }
Write-Step ("the document names {0}, {1} bytes, {2} HTTP mirrors, {3} torrents, sha-256 {4}" -f
    $fileName, $publishedSize, $httpMirrors.Count, $torrentNodes.Count, $publishedSha256.Substring(0, 16))

$cases = @()
$failures = @()

function Add-Failure([string]$name, [string]$message) {
    $script:failures += "${name}: $message"
}

function Invoke-Download([string]$name, [string]$documentPath, [string[]]$extraArgs) {
    $outputDirectory = Join-Path $Root "out/$name"
    New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
    $stdout = Join-Path $Root "$name.out"
    $stderr = Join-Path $Root "$name.err"
    $arguments = @("--json", "download", $documentPath, "--dir", $outputDirectory,
        "--timeout", "${TimeoutSeconds}s") + $extraArgs
    $process = Start-Process -FilePath $bitCli -ArgumentList $arguments `
        -PassThru -NoNewWindow -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $exited = $process.WaitForExit($TimeoutSeconds * 1000 + 60000)
    if (-not $exited) {
        try { $process.Kill() } catch { }
        return [pscustomobject]@{ exit_code = -1; report = $null; stderr = "timed out"; directory = $outputDirectory }
    }
    $parsed = $null
    $text = if (Test-Path $stdout) { Get-Content $stdout -Raw } else { "" }
    if ($text.Trim()) { try { $parsed = $text | ConvertFrom-Json } catch { } }
    [pscustomobject]@{
        exit_code = $process.ExitCode
        report    = $parsed
        stderr    = if (Test-Path $stderr) { (Get-Content $stderr -Raw) } else { "" }
        directory = $outputDirectory
    }
}

# ---------------------------------------------------------------------------
# real_as_served: what the mirror actually gives you
# ---------------------------------------------------------------------------

Write-Step "case real_as_served"
$run = Invoke-Download "real_as_served" $asServed @()
if ($torrentNodes.Count -gt 0) {
    # The instance grew torrent support. That is a better world and this case
    # has to change with it rather than assert the old one.
    if ($run.exit_code -ne 0) { Add-Failure "real_as_served" "the document now carries a torrent and the run exited $($run.exit_code). stderr: $($run.stderr)" }
}
else {
    if ($run.exit_code -ne 4) { Add-Failure "real_as_served" "exited $($run.exit_code), expected 4 for a document with no torrent. stderr: $($run.stderr)" }
    if ($run.stderr -notmatch 'no torrent') { Add-Failure "real_as_served" "the message does not say the document lists no torrent: $($run.stderr)" }
    if ($run.stderr -notmatch "$($httpMirrors.Count) HTTP mirror") {
        Add-Failure "real_as_served" "the message does not name the $($httpMirrors.Count) mirrors the document did have: $($run.stderr)"
    }
}
$cases += [pscustomobject][ordered]@{ case = "real_as_served"; exit_code = $run.exit_code; note = ($run.stderr -split "`n")[0] }

# ---------------------------------------------------------------------------
# real_by_url: the same document, named by its URL rather than by a saved copy
# ---------------------------------------------------------------------------
#
# This is T-154's acceptance, and it is the strongest form available: the URL
# is a live MirrorBrain instance generating the document per request, so the
# run fetches a document nobody here wrote or saved.
#
# What is asserted is that it behaves as the saved copy does. Both reach the
# same outcome for the same reason, and `real_as_served` two cases up is the
# saved copy of the same URL, so the comparison is against a run in this same
# report rather than against a remembered one.
#
# The document is generated per request, so the two are not guaranteed to be
# byte-identical: an instance may rotate its mirror list between two requests.
# The exit code and the reason are what must match, and the mirror count is
# reported rather than judged for exactly that reason.

Write-Step "case real_by_url"
$byUrl = Invoke-Download "real_by_url" $Meta4Url @()
if ($byUrl.exit_code -ne $run.exit_code) {
    Add-Failure "real_by_url" "exited $($byUrl.exit_code) and the saved copy exited $($run.exit_code). stderr: $($byUrl.stderr)"
}
if ($torrentNodes.Count -eq 0) {
    # The URL was recognised as a metalink if and only if the message is about
    # the document. Classified as a torrent URL, which is what it was before
    # T-154, the failure is a bencode parse error instead.
    if ($byUrl.stderr -notmatch 'no torrent') {
        Add-Failure "real_by_url" "the message is not about the metalink, so the URL was not recognised as one: $($byUrl.stderr)"
    }
    if ($byUrl.stderr -match 'bencode') {
        Add-Failure "real_by_url" "the URL was handed to the session as a .torrent: $($byUrl.stderr)"
    }
}
$cases += [pscustomobject][ordered]@{ case = "real_by_url"; exit_code = $byUrl.exit_code; note = ($byUrl.stderr -split "`n")[0] }

Write-Step "case real_v3_as_served"
$runV3 = Invoke-Download "real_v3_as_served" $asServedV3 @()
if ($torrentNodes.Count -eq 0) {
    if ($runV3.exit_code -ne 4) { Add-Failure "real_v3_as_served" "exited $($runV3.exit_code), expected 4. stderr: $($runV3.stderr)" }
    if ($runV3.stderr -notmatch 'no torrent') { Add-Failure "real_v3_as_served" "the message does not say the document lists no torrent: $($runV3.stderr)" }
}
$cases += [pscustomobject][ordered]@{ case = "real_v3_as_served"; exit_code = $runV3.exit_code; note = ($runV3.stderr -split "`n")[0] }

# ---------------------------------------------------------------------------
# real_dry_run: the whole document, read, with no network
# ---------------------------------------------------------------------------

Write-Step "case real_dry_run"
$dryOut = Join-Path $Root "dry.out"
$dryErr = Join-Path $Root "dry.err"
$dryProcess = Start-Process -FilePath $bitCli `
    -ArgumentList @("--json", "download", $asServed, "--dir", (Join-Path $Root "out/dry"), "--dry-run") `
    -PassThru -NoNewWindow -RedirectStandardOutput $dryOut -RedirectStandardError $dryErr
$dryProcess.WaitForExit(60000) | Out-Null
$dry = $null
$dryText = if (Test-Path $dryOut) { Get-Content $dryOut -Raw } else { "" }
if ($dryText.Trim()) { try { $dry = $dryText | ConvertFrom-Json } catch { } }
if ($dryProcess.ExitCode -ne 0) { Add-Failure "real_dry_run" "exited $($dryProcess.ExitCode). stderr: $(Get-Content $dryErr -Raw)" }
$dryMirrors = 0
if (-not $dry) { Add-Failure "real_dry_run" "no JSON document" }
else {
    $row = $dry.torrents[0]
    $dryMirrors = @($row.web_seeds).Count
    if ($row.metalink.size -ne $publishedSize) { Add-Failure "real_dry_run" "reported size $($row.metalink.size), the document says $publishedSize" }
    if ($row.metalink.checksum.expected -ne $publishedSha256) { Add-Failure "real_dry_run" "reported checksum $($row.metalink.checksum.expected), the document says $publishedSha256" }
    if ($row.metalink.mirrors_listed -ne $httpMirrors.Count) { Add-Failure "real_dry_run" "reported $($row.metalink.mirrors_listed) mirrors, the document lists $($httpMirrors.Count)" }
    if ($dryMirrors -ne $httpMirrors.Count) { Add-Failure "real_dry_run" "built $dryMirrors sources from $($httpMirrors.Count) mirrors" }
}
$cases += [pscustomobject][ordered]@{ case = "real_dry_run"; exit_code = $dryProcess.ExitCode; note = "$dryMirrors sources from the document" }

# ---------------------------------------------------------------------------
# real_with_torrent: the real mirrors, over the real internet
# ---------------------------------------------------------------------------
#
# The payload is fetched once with curl so a torrent can exist over exactly
# those bytes. Everything the run then does is against the mirror list in the
# document, and the digest it is checked against is the published one.

Write-Step "fetching the payload once so a torrent can describe it"
$fetched = Join-Path $Root "payload/$fileName"
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $fetched) | Out-Null
$firstMirror = $httpMirrors[0].InnerText.Trim()
Get-Remote $firstMirror $fetched | Out-Null
$fetchedSha256 = (Get-FileHash -Algorithm SHA256 -Path $fetched).Hash.ToLower()
if ($fetchedSha256 -ne $publishedSha256) {
    Exit-With 2 "the payload fetched from $firstMirror hashes to $fetchedSha256 and the document publishes $publishedSha256. The mirror and the document disagree, so this check cannot use either as ground truth."
}
Write-Step "the payload matches the published sha-256"

$serve = Join-Path $Root "serve"
New-Item -ItemType Directory -Force -Path $serve | Out-Null
$torrent = Join-Path $serve "payload.torrent"
& $bitCli create $fetched --piece-length 256KiB --no-creation-date --output $torrent --force --json 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE" }

$serverOut = Join-Path $Root "fileserver.out"
$serverErr = Join-Path $Root "fileserver.err"
$script:Server = Start-Process -FilePath $fileServer `
    -ArgumentList @("--root", $serve, "--port", "0") `
    -PassThru -NoNewWindow -RedirectStandardOutput $serverOut -RedirectStandardError $serverErr
$base = $null
for ($attempt = 0; $attempt -lt 100; $attempt++) {
    Start-Sleep -Milliseconds 100
    if (Test-Path $serverOut) {
        $printed = (Get-Content $serverOut -Raw)
        if ($printed -match 'http://\S+') { $base = $Matches[0].Trim().TrimEnd('/'); break }
    }
}
if (-not $base) { Exit-With 2 "the file server printed no base URL. stderr: $(Get-Content $serverErr -Raw)" }

# One line added to the document the mirror generated, and nothing else
# touched: the closing `</file>` gets a `<metaurl>` in front of it.
$withTorrent = Join-Path $Root "real-with-torrent.meta4"
$original = Get-Content $asServed -Raw
$patched = $original -replace '(?s)</file>', "  <metaurl mediatype=`"torrent`">$base/payload.torrent</metaurl>`n  </file>"
if ($patched -eq $original) { Exit-With 2 "could not add a <metaurl> to the document: no </file> in it." }
Set-Content -Path $withTorrent -Value $patched -Encoding utf8NoBOM

Write-Step "case real_with_torrent (payload from the document's own mirrors)"
# --web-seed-only, so every byte comes from the mirror list rather than from a
# swarm, which is what makes "the document's mirrors served it" a measurement.
$run = Invoke-Download "real_with_torrent" $withTorrent @("--web-seed-only")
$metalink = if ($run.report -and $run.report.torrents) { $run.report.torrents[0].metalink } else { $null }
$torrentReport = if ($run.report -and $run.report.torrents) { $run.report.torrents[0] } else { $null }
if ($run.exit_code -ne 0) { Add-Failure "real_with_torrent" "exited $($run.exit_code), expected 0. stderr: $($run.stderr)" }
if (-not $metalink) { Add-Failure "real_with_torrent" "the report carries no metalink block" }
else {
    if ($metalink.version -ne "4") { Add-Failure "real_with_torrent" "version $($metalink.version), expected 4" }
    if ($metalink.mirrors_listed -ne $httpMirrors.Count) { Add-Failure "real_with_torrent" "listed $($metalink.mirrors_listed) mirrors, the document has $($httpMirrors.Count)" }
    if ($metalink.mirrors_registered -ne $httpMirrors.Count) { Add-Failure "real_with_torrent" "registered $($metalink.mirrors_registered) of $($httpMirrors.Count) mirrors" }
    if ($metalink.checksum.algorithm -ne "sha256") { Add-Failure "real_with_torrent" "checked $($metalink.checksum.algorithm), expected the document's sha-256" }
    if ($metalink.checksum.expected -ne $publishedSha256) { Add-Failure "real_with_torrent" "verified against $($metalink.checksum.expected), the document publishes $publishedSha256" }
    if ($metalink.checksum.matched -ne $true) { Add-Failure "real_with_torrent" "the published checksum did not match: $($metalink.checksum | ConvertTo-Json -Compress)" }
    if ($metalink.agreement.size_agrees -ne $true) { Add-Failure "real_with_torrent" "the document's size and the torrent's disagree: $($metalink.agreement | ConvertTo-Json -Compress)" }
}
$servedBytes = 0
if ($torrentReport) {
    if (-not $torrentReport.finished) { Add-Failure "real_with_torrent" "the download did not finish" }
    $servedBytes = $torrentReport.from_web_seeds.bytes
    if ($servedBytes -ne $publishedSize) { Add-Failure "real_with_torrent" "HTTP sources served $servedBytes of $publishedSize bytes" }
    $fromMetalink = @($torrentReport.sources | Where-Object { $_.origin -eq "metalink" })
    if ($fromMetalink.Count -ne $httpMirrors.Count) { Add-Failure "real_with_torrent" "$($fromMetalink.Count) sources carried origin=metalink, expected $($httpMirrors.Count)" }
    $servedFrom = @($fromMetalink | Where-Object { $_.served_bytes -gt 0 })
    Write-Step ("{0} of {1} mirrors served bytes" -f $servedFrom.Count, $fromMetalink.Count)
}
$landed = Join-Path $run.directory $fileName
if (-not (Test-Path $landed)) { Add-Failure "real_with_torrent" "no payload at $landed" }
elseif ((Get-FileHash -Algorithm SHA256 -Path $landed).Hash.ToLower() -ne $publishedSha256) {
    Add-Failure "real_with_torrent" "the payload on disk does not hash to the published sha-256"
}
$cases += [pscustomobject][ordered]@{
    case         = "real_with_torrent"
    exit_code    = $run.exit_code
    served_bytes = $servedBytes
    metalink     = $metalink
}

# ---------------------------------------------------------------------------
# The record
# ---------------------------------------------------------------------------

Stop-Background

$verdict = if ($failures.Count -eq 0) { "pass" } else { "fail" }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "metalink-real-$stamp.json"

[pscustomobject][ordered]@{
    kind           = "metalink_real"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    }
    parameters     = [ordered]@{
        meta4_url    = $Meta4Url
        metalink_url = $metalinkUrl
        profile      = $Profile
    }
    document       = [ordered]@{
        meta4_bytes       = $meta4Bytes
        metalink_bytes    = $metalinkBytes
        file              = $fileName
        published_size    = $publishedSize
        published_sha256  = $publishedSha256
        http_mirrors      = $httpMirrors.Count
        all_url_elements  = $mirrorNodes.Count
        torrents          = $torrentNodes.Count
        has_pieces_block  = $hasPieces
        has_pgp_signature = $hasSignature
    }
    cases          = @($cases)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "No MirrorBrain instance reachable in August 2026 emits <metaurl mediatype=`"torrent`">, so a real document alone has nothing to download from. real_as_served records that as exit 4 naming the mirror count, which is the behaviour a user of a real document gets.",
        "real_with_torrent adds one <metaurl> line to the document the mirror generated and changes nothing else. The payload comes from the document's own mirror list over the public internet and is verified against the checksum the publisher published.",
        "A network failure exits 2, never 0. This check is about a real mirror, so a run that did not reach one proved nothing."
    )
} | ConvertTo-Json -Depth 10 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "report:  $reportPath"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-metalink-real: $failure") }
    exit 1
}
exit 0
