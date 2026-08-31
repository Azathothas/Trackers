# Drive a Metalink end to end: resolve, register, download, verify.
#
# This is the acceptance for `TODO/cli-surface.md` T-113. A Metalink is two
# independent descriptions of one payload in one file: a `.torrent` named by
# URL, and a list of HTTP mirrors with a whole-file checksum over the same
# bytes. `bit-cli download release.meta4` has to read the document, fetch the
# torrent it names, register the mirrors as web seed sources, download, and
# then check the payload against the document's own checksum.
#
# The part that matters is the last one, and it is not "did the download
# work". The session has already verified every piece against the torrent's
# own SHA-1 hashes, so a Metalink checksum that then disagrees says the
# **Metalink** is the document that is wrong. Saying which of the two is wrong
# is the reason to carry both.
#
# Twelve cases, all served from a loopback file server:
#
#   v4_ok               RFC 5854 `.meta4`. The torrent comes from the
#                       `<metaurl>`, the `<url>` mirrors serve every byte, and
#                       the sha-256 matches. An `ftp:` mirror in the same
#                       document is counted and not registered.
#   v3_ok               The same facts as Metalink 3: `<url type="bittorrent">`
#                       for the torrent, `<verification><hash type="sha256">`
#                       for the checksum, and `preference` running the other
#                       way from `priority`.
#   url_source          The same v4_ok document named by URL rather than by
#                       path. The report is compared field by field with the
#                       saved copy's. See T-154.
#   bad_checksum        The document's sha-256 is wrong. The payload completes
#                       and passes the torrent's piece hashes, so the run exits
#                       7 and the report carries both digests.
#   bad_size            The document's `<size>` disagrees with the torrent. It
#                       is warned before a byte is fetched and the run exits 7.
#   md5_only            A weaker algorithm is still checked.
#   sha512_only         An algorithm this cannot compute is reported as not
#                       checked, with the reason, and does not fail the run. A
#                       checksum that was not computed is not one that passed.
#   no_torrent          A document with mirrors and no torrent. Exit 4, and the
#                       message says how many mirrors it did have.
#   torrent_fallback    Two `<metaurl>`s where the preferred one 404s. The
#                       second is used and the report records the first.
#   truncated           A document that stops mid-element. Refused, exit 4.
#   hash_check_only     --hash-check-only over the payload v4_ok already
#                       downloaded. The metalink block is reported at that exit
#                       too, with the size comparison and either a checked
#                       digest or a not_checked reason. See T-155.
#   dry_run             No network at all: the document's own claims are
#                       reported and the torrent is not fetched. The file
#                       server is stopped first, so a run that reached for it
#                       would fail rather than pass quietly.
#
# Usage:
#   pwsh scripts/check-metalink.ps1
#   pwsh scripts/check-metalink.ps1 -PayloadMiB 8 -Keep
#
# Exits 0 when every case behaves as described, 1 when one does not, and 2 when
# the check could not run. The record goes to bench/metalink-<timestamp>.json.
#
# See TODO/cli-surface.md, T-113.

[CmdletBinding()]
param(
    [int]$PayloadMiB = 2,
    [string]$Root = ".tmp/metalink",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [int]$TimeoutSeconds = 120,
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
    [Console]::Error.WriteLine("check-metalink: $message")
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

# ---------------------------------------------------------------------------
# A payload, a torrent for it, and a server that holds both
# ---------------------------------------------------------------------------

$serve = Join-Path $Root "serve"
New-Item -ItemType Directory -Force -Path $serve | Out-Null

Write-Step "building a $PayloadMiB MiB payload"
$payloadBytes = [byte[]]::new($PayloadMiB * 1024 * 1024)
[int64]$state = 7
for ($i = 0; $i -lt $payloadBytes.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $payloadBytes[$i] = [byte](($state -shr 16) -band 0xFF)
}
$payload = Join-Path $serve "release.bin"
[System.IO.File]::WriteAllBytes($payload, $payloadBytes)
$payloadLength = $payloadBytes.Length
$sha256 = (Get-FileHash -Algorithm SHA256 -Path $payload).Hash.ToLower()
$md5 = (Get-FileHash -Algorithm MD5 -Path $payload).Hash.ToLower()
$sha512 = (Get-FileHash -Algorithm SHA512 -Path $payload).Hash.ToLower()

$torrent = Join-Path $serve "release.bin.torrent"
& $bitCli create $payload --piece-length 256KiB --no-creation-date --output $torrent --force --json 2>&1 | Out-Null
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
Write-Step "file server at $base"

$payloadUrl = "$base/release.bin"
$torrentUrl = "$base/release.bin.torrent"
$missingUrl = "$base/not-here.torrent"

# ---------------------------------------------------------------------------
# The documents
# ---------------------------------------------------------------------------

function New-Document([string]$name, [string]$text) {
    $path = Join-Path $Root $name
    Set-Content -Path $path -Value $text -Encoding utf8NoBOM
    $path
}

function New-V4([string]$name, [string]$size, [string]$hashes, [string]$extra) {
    New-Document $name @"
<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <published>2026-08-21T00:00:00Z</published>
  <file name="release.bin">
    <size>$size</size>
$hashes
    <url priority="1">$payloadUrl</url>
$extra
  </file>
</metalink>
"@
}

$torrentEntry = "    <metaurl mediatype=`"torrent`">$torrentUrl</metaurl>"

$documents = @{
    v4_ok            = New-V4 "v4-ok.meta4" $payloadLength `
        "    <hash type=`"sha-256`">$sha256</hash>" `
        "    <url priority=`"5`">ftp://ftp.example.com/release.bin</url>`n$torrentEntry"
    bad_checksum     = New-V4 "bad-checksum.meta4" $payloadLength `
        "    <hash type=`"sha-256`">$('0' * 64)</hash>" $torrentEntry
    bad_size         = New-V4 "bad-size.meta4" ($payloadLength + 1) `
        "    <hash type=`"sha-256`">$sha256</hash>" $torrentEntry
    md5_only         = New-V4 "md5-only.meta4" $payloadLength `
        "    <hash type=`"md5`">$md5</hash>" $torrentEntry
    sha512_only      = New-V4 "sha512-only.meta4" $payloadLength `
        "    <hash type=`"sha-512`">$sha512</hash>" $torrentEntry
    no_torrent       = New-V4 "no-torrent.meta4" $payloadLength `
        "    <hash type=`"sha-256`">$sha256</hash>" ""
    torrent_fallback = New-V4 "torrent-fallback.meta4" $payloadLength `
        "    <hash type=`"sha-256`">$sha256</hash>" `
        "    <metaurl mediatype=`"torrent`" priority=`"1`">$missingUrl</metaurl>`n    <metaurl mediatype=`"torrent`" priority=`"2`">$torrentUrl</metaurl>"
}

# Metalink 3 puts the same facts one level deeper, spells the torrent as a
# `<url type="bittorrent">`, and ranks mirrors with `preference`, where a
# higher number is preferred. The parser normalises that to version 4's rule,
# and this case is what proves the whole path does and not only the parser.
$documents.v3_ok = New-Document "v3-ok.metalink" @"
<?xml version="1.0" encoding="UTF-8"?>
<metalink version="3.0" xmlns="http://www.metalinker.org/">
  <files>
    <file name="release.bin">
      <size>$payloadLength</size>
      <verification>
        <hash type="sha256">$sha256</hash>
      </verification>
      <resources>
        <url type="http" preference="90">$payloadUrl</url>
        <url type="bittorrent">$torrentUrl</url>
      </resources>
    </file>
  </files>
</metalink>
"@

$documents.truncated = New-Document "truncated.meta4" @"
<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="release.bin">
    <url priority="1">$payloadUrl</url>
"@

# ---------------------------------------------------------------------------
# Running one case
# ---------------------------------------------------------------------------

$cases = @()
$failures = @()

# PowerShell variable names are case-insensitive, so a local named `$document`
# here would be the `[string]` parameter and the parsed JSON assigned to it
# would be converted back to a string. Every local in this function is named so
# it cannot collide with a parameter.
function Invoke-Download([string]$name, [string]$documentPath, [string[]]$extraArgs) {
    $outputDirectory = Join-Path $Root "out/$name"
    New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
    $stdout = Join-Path $Root "$name.out"
    $stderr = Join-Path $Root "$name.err"
    $arguments = @("--json", "download", $documentPath, "--dir", $outputDirectory,
        "--web-seed-only", "--timeout", "${TimeoutSeconds}s") + $extraArgs
    # Under $ErrorActionPreference='Stop' a native command writing to stderr is
    # a terminating error in pwsh 7, and every one of these cases writes there.
    $process = Start-Process -FilePath $bitCli -ArgumentList $arguments `
        -PassThru -NoNewWindow -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $exited = $process.WaitForExit($TimeoutSeconds * 1000 + 30000)
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

function Add-Failure([string]$name, [string]$message) {
    $script:failures += "${name}: $message"
}

function Get-Metalink($run) {
    if (-not $run.report) { return $null }
    if (-not $run.report.torrents) { return $null }
    $run.report.torrents[0].metalink
}

# --- the two that should simply work ---------------------------------------

foreach ($pair in @(@("v4_ok", "4"), @("v3_ok", "3"))) {
    $name = $pair[0]
    $wantVersion = $pair[1]
    Write-Step "case $name"
    $run = Invoke-Download $name $documents[$name] @()
    $metalink = Get-Metalink $run
    $torrentReport = if ($run.report -and $run.report.torrents) { $run.report.torrents[0] } else { $null }
    if ($run.exit_code -ne 0) { Add-Failure $name "exited $($run.exit_code), expected 0. stderr: $($run.stderr)" }
    if (-not $metalink) { Add-Failure $name "the report carries no metalink block" }
    else {
        if ($metalink.version -ne $wantVersion) { Add-Failure $name "reported version $($metalink.version), expected $wantVersion" }
        if ($metalink.torrent_url -ne $torrentUrl) { Add-Failure $name "resolved the torrent from $($metalink.torrent_url), expected $torrentUrl" }
        if ($metalink.mirrors_registered -lt 1) { Add-Failure $name "registered $($metalink.mirrors_registered) mirrors, expected at least 1" }
        if ($metalink.checksum.matched -ne $true) { Add-Failure $name "the checksum did not match: $($metalink.checksum | ConvertTo-Json -Compress)" }
        if ($metalink.agreement.size_agrees -ne $true) { Add-Failure $name "the sizes did not agree: $($metalink.agreement | ConvertTo-Json -Compress)" }
    }
    if ($torrentReport) {
        if (-not $torrentReport.finished) { Add-Failure $name "the download did not finish" }
        if ($torrentReport.from_web_seeds.bytes -ne $payloadLength) {
            Add-Failure $name "HTTP sources served $($torrentReport.from_web_seeds.bytes) of $payloadLength bytes"
        }
        $fromMetalink = @($torrentReport.sources | Where-Object { $_.origin -eq "metalink" })
        if ($fromMetalink.Count -lt 1) { Add-Failure $name "no source carried origin=metalink" }
    }
    $landed = Join-Path $run.directory "release.bin"
    if (-not (Test-Path $landed)) { Add-Failure $name "no payload at $landed" }
    elseif ((Get-FileHash -Algorithm SHA256 -Path $landed).Hash.ToLower() -ne $sha256) {
        Add-Failure $name "the payload on disk does not hash to the source payload"
    }
    $cases += [pscustomobject][ordered]@{
        case      = $name
        exit_code = $run.exit_code
        metalink  = $metalink
    }
}

# --- the same document, named by URL ----------------------------------------
#
# Every real Metalink is served over HTTP: MirrorBrain generates one per
# request, so a URL is how a caller normally meets one and a saved `.meta4` is
# what you get after keeping a copy. Before T-154 the http:// prefix was
# checked before the extension, so this was classified as a torrent URL, handed
# to the session as a `.torrent`, and failed on the bencode parse with a
# message about the torrent.
#
# What is asserted is that it behaves **exactly** as the same bytes on disk do:
# the same block, field by field, against the v4_ok case above.

Write-Step "case url_source (the v4_ok document, served)"
Copy-Item -LiteralPath $documents.v4_ok -Destination (Join-Path $serve "release.meta4") -Force
$meta4Url = "$base/release.meta4"
$run = Invoke-Download "url_source" $meta4Url @()
$urlMetalink = Get-Metalink $run
$urlTorrent = if ($run.report -and $run.report.torrents) { $run.report.torrents[0] } else { $null }
if ($run.exit_code -ne 0) { Add-Failure "url_source" "exited $($run.exit_code), expected 0. stderr: $($run.stderr)" }
if (-not $urlMetalink) { Add-Failure "url_source" "the report carries no metalink block, so the URL was not recognised as a metalink" }
if ($urlTorrent) {
    if ($urlTorrent.source -ne $meta4Url) { Add-Failure "url_source" "source=$($urlTorrent.source), expected $meta4Url" }
    if (-not $urlTorrent.finished) { Add-Failure "url_source" "the download did not finish" }
    if ($urlTorrent.from_web_seeds.bytes -ne $payloadLength) {
        Add-Failure "url_source" "HTTP sources served $($urlTorrent.from_web_seeds.bytes) of $payloadLength bytes"
    }
}
$landed = Join-Path $run.directory "release.bin"
if (-not (Test-Path $landed)) { Add-Failure "url_source" "no payload at $landed" }
elseif ((Get-FileHash -Algorithm SHA256 -Path $landed).Hash.ToLower() -ne $sha256) {
    Add-Failure "url_source" "the payload on disk does not hash to the source payload"
}
# Field by field against the saved copy, which is what "behaves exactly as the
# same document saved to disk does" means. Compared as compressed JSON so a
# field added later is compared too, without this list going stale.
#
# `checksum.path` is the one field that must differ and is not a behavioural
# difference: each case downloads into its own `out/<name>`, so the two point
# at two copies of the same bytes. It is blanked for the comparison and checked
# on its own, which is stronger than leaving it out: a run whose payload landed
# somewhere else entirely would pass the comparison and fail here.
function Get-ComparableMetalink($block) {
    $json = $block | ConvertTo-Json -Depth 8
    $copy = $json | ConvertFrom-Json
    if ($copy.checksum) { $copy.checksum.path = "<output directory>" }
    $copy | ConvertTo-Json -Depth 8 -Compress
}
$savedMetalink = ($cases | Where-Object { $_.case -eq "v4_ok" }).metalink
if ($savedMetalink -and $urlMetalink) {
    $a = Get-ComparableMetalink $savedMetalink
    $b = Get-ComparableMetalink $urlMetalink
    if ($a -ne $b) { Add-Failure "url_source" "the metalink block differs from the saved copy's.`n  saved: $a`n  url:   $b" }
    $checkedPath = $urlMetalink.checksum.path
    if ($checkedPath -ne $landed) {
        Add-Failure "url_source" "the checksum was computed over $checkedPath, expected the payload this case wrote at $landed"
    }
}
$cases += [pscustomobject][ordered]@{ case = "url_source"; exit_code = $run.exit_code; metalink = $urlMetalink }

# The ftp: mirror belongs to the v4 document only, and it must have been
# counted rather than registered: a source this cannot fetch from is worse
# than one it never had.
$v4 = ($cases | Where-Object { $_.case -eq "v4_ok" }).metalink
if ($v4) {
    if ($v4.mirrors_listed -ne 1) { Add-Failure "v4_ok" "listed $($v4.mirrors_listed) usable mirrors, expected 1" }
    if (@($v4.mirrors_unsupported).Count -ne 1) {
        Add-Failure "v4_ok" "reported $(@($v4.mirrors_unsupported).Count) unsupported mirrors, expected the one ftp: entry"
    }
}

# --- the checksum that disagrees -------------------------------------------

Write-Step "case bad_checksum"
$run = Invoke-Download "bad_checksum" $documents.bad_checksum @()
$metalink = Get-Metalink $run
if ($run.exit_code -ne 7) { Add-Failure "bad_checksum" "exited $($run.exit_code), expected 7. stderr: $($run.stderr)" }
if (-not $metalink) { Add-Failure "bad_checksum" "the report carries no metalink block" }
else {
    if ($metalink.checksum.matched -ne $false) { Add-Failure "bad_checksum" "matched=$($metalink.checksum.matched), expected false" }
    if ($metalink.checksum.expected -ne ('0' * 64)) { Add-Failure "bad_checksum" "expected digest not reported" }
    if ($metalink.checksum.actual -ne $sha256) { Add-Failure "bad_checksum" "actual digest was $($metalink.checksum.actual), expected $sha256" }
}
# The payload is still correct: it passed the torrent's own piece hashes. That
# is what makes this evidence about the metalink rather than about the bytes.
$landed = Join-Path $run.directory "release.bin"
if (-not (Test-Path $landed)) { Add-Failure "bad_checksum" "the payload was not written" }
elseif ((Get-FileHash -Algorithm SHA256 -Path $landed).Hash.ToLower() -ne $sha256) {
    Add-Failure "bad_checksum" "the payload on disk is not the source payload, so the torrent is what disagreed"
}
if ($run.stderr -notmatch 'metalink') { Add-Failure "bad_checksum" "stderr does not mention the metalink: $($run.stderr)" }
$cases += [pscustomobject][ordered]@{ case = "bad_checksum"; exit_code = $run.exit_code; metalink = $metalink }

# --- the size that disagrees ------------------------------------------------

Write-Step "case bad_size"
$run = Invoke-Download "bad_size" $documents.bad_size @()
$metalink = Get-Metalink $run
if ($run.exit_code -ne 7) { Add-Failure "bad_size" "exited $($run.exit_code), expected 7. stderr: $($run.stderr)" }
if ($metalink) {
    if ($metalink.agreement.size_agrees -ne $false) { Add-Failure "bad_size" "size_agrees=$($metalink.agreement.size_agrees), expected false" }
    if ($metalink.agreement.metalink_size -ne ($payloadLength + 1)) { Add-Failure "bad_size" "the document's size was not reported" }
    if ($metalink.agreement.torrent_size -ne $payloadLength) { Add-Failure "bad_size" "the torrent's size was not reported" }
}
else { Add-Failure "bad_size" "the report carries no metalink block" }
if ($run.stderr -notmatch 'the torrent says') { Add-Failure "bad_size" "the disagreement was not warned before the download: $($run.stderr)" }
$cases += [pscustomobject][ordered]@{ case = "bad_size"; exit_code = $run.exit_code; metalink = $metalink }

# --- a weaker algorithm is still checked ------------------------------------

Write-Step "case md5_only"
$run = Invoke-Download "md5_only" $documents.md5_only @()
$metalink = Get-Metalink $run
if ($run.exit_code -ne 0) { Add-Failure "md5_only" "exited $($run.exit_code), expected 0. stderr: $($run.stderr)" }
if ($metalink) {
    if ($metalink.checksum.algorithm -ne "md5") { Add-Failure "md5_only" "checked $($metalink.checksum.algorithm), expected md5" }
    if ($metalink.checksum.matched -ne $true) { Add-Failure "md5_only" "the md5 did not match" }
}
else { Add-Failure "md5_only" "the report carries no metalink block" }
$cases += [pscustomobject][ordered]@{ case = "md5_only"; exit_code = $run.exit_code; metalink = $metalink }

# --- an algorithm nothing here computes -------------------------------------

Write-Step "case sha512_only"
$run = Invoke-Download "sha512_only" $documents.sha512_only @()
$metalink = Get-Metalink $run
if ($run.exit_code -ne 0) { Add-Failure "sha512_only" "exited $($run.exit_code), expected 0. stderr: $($run.stderr)" }
if ($metalink) {
    if ($metalink.checksum.matched -ne $null) { Add-Failure "sha512_only" "matched=$($metalink.checksum.matched), and nothing was hashed" }
    if (-not $metalink.checksum.not_checked) { Add-Failure "sha512_only" "nothing said why the checksum was not checked" }
    elseif ($metalink.checksum.not_checked -notmatch 'sha512') { Add-Failure "sha512_only" "the reason does not name the algorithm: $($metalink.checksum.not_checked)" }
}
else { Add-Failure "sha512_only" "the report carries no metalink block" }
$cases += [pscustomobject][ordered]@{ case = "sha512_only"; exit_code = $run.exit_code; metalink = $metalink }

# --- a document with nothing to download ------------------------------------

Write-Step "case no_torrent"
$run = Invoke-Download "no_torrent" $documents.no_torrent @()
if ($run.exit_code -ne 4) { Add-Failure "no_torrent" "exited $($run.exit_code), expected 4. stderr: $($run.stderr)" }
if ($run.stderr -notmatch 'no torrent') { Add-Failure "no_torrent" "the message does not say the document lists no torrent: $($run.stderr)" }
if ($run.stderr -notmatch '1 HTTP mirror') { Add-Failure "no_torrent" "the message does not say how many mirrors it did have: $($run.stderr)" }
$cases += [pscustomobject][ordered]@{ case = "no_torrent"; exit_code = $run.exit_code; metalink = $null }

# --- the preferred torrent is gone ------------------------------------------

Write-Step "case torrent_fallback"
$run = Invoke-Download "torrent_fallback" $documents.torrent_fallback @()
$metalink = Get-Metalink $run
if ($run.exit_code -ne 0) { Add-Failure "torrent_fallback" "exited $($run.exit_code), expected 0. stderr: $($run.stderr)" }
if ($metalink) {
    if ($metalink.torrent_url -ne $torrentUrl) { Add-Failure "torrent_fallback" "used $($metalink.torrent_url), expected the second entry $torrentUrl" }
    $fallbacks = @($metalink.torrent_fallbacks)
    if ($fallbacks.Count -ne 1) { Add-Failure "torrent_fallback" "recorded $($fallbacks.Count) failed torrent URLs, expected 1" }
    elseif ($fallbacks[0].url -ne $missingUrl) { Add-Failure "torrent_fallback" "recorded $($fallbacks[0].url) as the failure, expected $missingUrl" }
}
else { Add-Failure "torrent_fallback" "the report carries no metalink block" }
$cases += [pscustomobject][ordered]@{ case = "torrent_fallback"; exit_code = $run.exit_code; metalink = $metalink }

# --- half a mirror list is not a mirror list --------------------------------

Write-Step "case truncated"
$run = Invoke-Download "truncated" $documents.truncated @()
if ($run.exit_code -eq 0) { Add-Failure "truncated" "exited 0, and the document stops mid-element" }
if ($run.stderr -notmatch 'truncated') { Add-Failure "truncated" "the message does not say the document is truncated: $($run.stderr)" }
$cases += [pscustomobject][ordered]@{ case = "truncated"; exit_code = $run.exit_code; metalink = $null }

# --- --hash-check-only over a payload that is already complete --------------
#
# It runs against `out/v4_ok`, which the first case downloaded and verified, so
# the payload on disk is complete before this starts. That is the case worth
# checking: the hash check proves the bytes against the torrent, and the
# metalink block then proves the same bytes against the document, which is the
# strongest thing this flag can report.
#
# Before T-155 this reported nothing about the document at all, because
# `one_inner` returned for `--hash-check-only` above the block that builds it.
# The size comparison in particular is computed before that return and was then
# thrown away.
#
# The file server is still up here, deliberately: `--hash-check-only` still has
# to fetch the `.torrent` the `<metaurl>` names before it can check anything
# against it. Stopping the server is the next case's business.

Write-Step "case hash_check_only (over the completed v4_ok payload)"
$hcoOut = Join-Path $Root "hash_check_only.out"
$hcoErr = Join-Path $Root "hash_check_only.err"
$hcoProcess = Start-Process -FilePath $bitCli `
    -ArgumentList @("--json", "download", $documents.v4_ok, "--dir", (Join-Path $Root "out/v4_ok"),
        "--hash-check-only", "--timeout", "${TimeoutSeconds}s") `
    -PassThru -NoNewWindow -RedirectStandardOutput $hcoOut -RedirectStandardError $hcoErr
$hcoProcess.WaitForExit($TimeoutSeconds * 1000 + 30000) | Out-Null
$hcoCode = $hcoProcess.ExitCode
$hco = $null
$hcoText = if (Test-Path $hcoOut) { Get-Content $hcoOut -Raw } else { "" }
if ($hcoText.Trim()) { try { $hco = $hcoText | ConvertFrom-Json } catch { } }
$hcoMetalink = $null
if ($hcoCode -ne 0) { Add-Failure "hash_check_only" "exited $hcoCode, expected 0. stderr: $(Get-Content $hcoErr -Raw)" }
if (-not $hco) { Add-Failure "hash_check_only" "no JSON document" }
else {
    $row = $hco.torrents[0]
    $hcoMetalink = $row.metalink
    if (-not $row.finished) { Add-Failure "hash_check_only" "finished=$($row.finished), and the payload was downloaded and verified by the v4_ok case" }
    if (-not $hcoMetalink) { Add-Failure "hash_check_only" "the report carries no metalink block, which is the whole of T-155" }
    else {
        if ($hcoMetalink.agreement.size_agrees -ne $true) {
            Add-Failure "hash_check_only" "agreement.size_agrees=$($hcoMetalink.agreement.size_agrees), expected true"
        }
        if ($hcoMetalink.agreement.metalink_size -ne $payloadLength) {
            Add-Failure "hash_check_only" "agreement.metalink_size=$($hcoMetalink.agreement.metalink_size), expected $payloadLength"
        }
        if (-not $hcoMetalink.torrent_url) { Add-Failure "hash_check_only" "no torrent_url, so the document's own claims were not reported" }
        if ($hcoMetalink.mirrors_listed -lt 1) { Add-Failure "hash_check_only" "mirrors_listed=$($hcoMetalink.mirrors_listed), expected at least 1" }
        # Either a digest that was computed, or a reason it was not. A checksum
        # that was not computed is not a checksum that passed, which is the
        # rule the whole metalink block is written under.
        $checksum = $hcoMetalink.checksum
        if (-not $checksum) { Add-Failure "hash_check_only" "no checksum block" }
        elseif ($null -eq $checksum.matched -and -not $checksum.not_checked) {
            Add-Failure "hash_check_only" "the checksum was neither checked nor given a not_checked reason"
        }
        elseif ($checksum.matched -eq $false) {
            Add-Failure "hash_check_only" "the checksum did not match, and v4_ok's document is the correct one"
        }
    }
}
$cases += [pscustomobject][ordered]@{ case = "hash_check_only"; exit_code = $hcoCode; metalink = $hcoMetalink }

# --- a dry run reads the document and touches nothing -----------------------
#
# The file server is stopped first. A dry run that reached for the torrent
# would fail here rather than pass quietly against a server that happened to
# still be up, which is the difference between checking the behaviour and
# checking that it usually holds.

Write-Step "case dry_run (file server stopped)"
Stop-Background
$dryOut = Join-Path $Root "dry.out"
$dryErr = Join-Path $Root "dry.err"
$dryProcess = Start-Process -FilePath $bitCli `
    -ArgumentList @("--json", "download", $documents.v4_ok, "--dir", (Join-Path $Root "out/dry"), "--dry-run") `
    -PassThru -NoNewWindow -RedirectStandardOutput $dryOut -RedirectStandardError $dryErr
$dryProcess.WaitForExit(60000) | Out-Null
$dryCode = $dryProcess.ExitCode
$dry = $null
$dryText = if (Test-Path $dryOut) { Get-Content $dryOut -Raw } else { "" }
if ($dryText.Trim()) { try { $dry = $dryText | ConvertFrom-Json } catch { } }
if ($dryCode -ne 0) { Add-Failure "dry_run" "exited $dryCode, expected 0. stderr: $(Get-Content $dryErr -Raw)" }
if (-not $dry) { Add-Failure "dry_run" "no JSON document" }
else {
    $row = $dry.torrents[0]
    if ($row.kind -ne "metalink") { Add-Failure "dry_run" "kind=$($row.kind), expected metalink" }
    if ($row.needs_network -ne $true) { Add-Failure "dry_run" "needs_network=$($row.needs_network), and the torrent is named by URL" }
    if (-not $row.metalink) { Add-Failure "dry_run" "no metalink block" }
    else {
        if ($row.metalink.size -ne $payloadLength) { Add-Failure "dry_run" "size=$($row.metalink.size), expected $payloadLength" }
        if ($row.metalink.checksum.expected -ne $sha256) { Add-Failure "dry_run" "the document's checksum was not reported" }
        if (@($row.metalink.torrents).Count -ne 1) { Add-Failure "dry_run" "the torrent URL was not reported" }
    }
    if ($row.info_hash) { Add-Failure "dry_run" "an info hash was reported, so the torrent was fetched" }
}
$cases += [pscustomobject][ordered]@{ case = "dry_run"; exit_code = $dryCode; metalink = $null }

# ---------------------------------------------------------------------------
# The record
# ---------------------------------------------------------------------------

Stop-Background

$verdict = if ($failures.Count -eq 0) { "pass" } else { "fail" }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "metalink-$stamp.json"

[pscustomobject][ordered]@{
    kind           = "metalink"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    }
    parameters     = [ordered]@{
        payload_mib     = $PayloadMiB
        payload_bytes   = $payloadLength
        payload_sha256  = $sha256
        profile         = $Profile
        timeout_seconds = $TimeoutSeconds
    }
    cases          = @($cases)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "Every case is served from a loopback file server, so the numbers are about bit-cli and not about a mirror.",
        "bad_checksum is the case that matters: the payload passes the torrent's own piece hashes and fails the metalink's digest, so the metalink is the document that is wrong. The script checks the payload on disk against the source bytes to prove it.",
        "dry_run runs with the file server stopped, so a dry run that reached for the torrent fails here rather than passing against a server that happened to still be up."
    )
} | ConvertTo-Json -Depth 10 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "report:  $reportPath"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-metalink: $failure") }
    exit 1
}
exit 0
