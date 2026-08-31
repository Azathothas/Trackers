# Does the profile bit-cli impersonates still match a real browser?
#
# The defect it exists to catch: `TODO/cli-surface.md` T-244 ships a client
# that presents itself as a current Chrome, and `fingerprints/*.json` records
# what that client puts on the wire. Nothing compares either against a
# **browser**. A browser that changes its cipher list, its extension order, its
# HTTP/2 settings or its header set leaves this repository claiming a
# fingerprint nobody has, and the golden goes on passing because the golden is
# a record of ourselves.
#
# So this drives the browser this machine has at `loopback-tlsprobe`, reads
# what it emits, and compares it against `fingerprints/bit-cli-browser.json`
# field by field. The browser is the authority and the golden is the claim.
#
# **It recommends with proof**, which is the operator's requirement rather than
# a nicety: where the two disagree, the output carries the browser's own value
# in the shape the file that has to change wants, and names the browser and
# version it came from. A check that only says "your fingerprint changed" is
# half a tool.
#
# **With no browser it exits 2 and says so.** Most CI runners have none, and a
# check that fails a build because a machine has no Chrome is a check somebody
# disables. `crates/bit-cli-core/src/browser.rs` is the search and it names
# every path it looked at.
#
# **`-Container` captures from a browser this machine does not have.** The host
# path can only ever read the Chrome that is installed here, so a profile can
# never get ahead of it, and on 2026-08-29 that left this repository a major
# behind stable with no way to close the gap. So `-Container` puts a Chrome for
# Testing build of the channel asked for into a throwaway WSL2 distro, drives it
# at the probe, and destroys the distro in the same run. `-Channel Beta` is how
# the next bump is ready the day it ships. `docs/containers.md` is the
# procedure and `scripts/wsl-tool.ps1` is the pinned tooling.
#
# **In NAT mode the distro cannot reach the Windows loopback at all**, so the
# container path binds the probe to the address the distro reaches this host at
# and the failure without that is silent: the fixture simply never receives a
# connection. `wsl-tool.ps1 -Action HostAddress` is what answers, and the
# probe's `--bind` is what takes it.
#
# **Header values are read here and nowhere else.** The probe records header
# names by default; `--header-values` is passed only for this one capture,
# where the client is a browser this script launched itself, into a throwaway
# profile, at a loopback port, having visited nothing. `cookie` and
# `authorization` are dropped even then. Nothing else in this repository ever
# asks for values.
#
# **A difference an open entry already names is recorded and not judged.**
# That is `scripts/check-close-wait.ps1`'s pattern: a check must not fail a
# build for a defect that is already written down and being worked on, and the
# other half of the rule is that the exemption comes off when the entry closes.
# `-Strict` judges every difference, which is what a session verifying a fix
# passes.
#
# Usage:
#   pwsh scripts/check-browser-fingerprint.ps1
#   pwsh scripts/check-browser-fingerprint.ps1 -Json
#   pwsh scripts/check-browser-fingerprint.ps1 -Out bench/browser-fingerprint.json
#   pwsh scripts/check-browser-fingerprint.ps1 -BrowserPath /path/to/chrome
#   pwsh scripts/check-browser-fingerprint.ps1 -Strict
#   pwsh scripts/check-browser-fingerprint.ps1 -Container
#   pwsh scripts/check-browser-fingerprint.ps1 -Container -Channel Beta
#
# Exit 0 when the profile matches the browser apart from what an entry already
# names, 1 when it does not, and 2 when it could not run: no browser, no build,
# or the probe captured nothing. With `-Container` and no WSL2 or no container
# engine it exits 2 naming the missing piece, the same way the host path exits
# 2 with no browser.
#
# See TODO/cli-surface.md, T-244 and T-264.

[CmdletBinding()]
param(
    [switch]$Json,
    [string]$Out = "",
    # An explicit browser, tried first and alone.
    [string]$BrowserPath = "",
    [ValidateSet("debug", "release")]
    [string]$Build = "release",
    [string]$GoldenDir = "fingerprints",
    # Seconds to let the browser run before it is killed.
    [int]$TimeoutSeconds = 25,
    # Judge every difference, including the ones an open entry already names.
    [switch]$Strict,
    # Capture from a browser in a throwaway WSL2 distro rather than from this
    # host. Everything it creates is removed in the same run.
    [switch]$Container,
    # Which Chrome for Testing channel the container installs. Stable is what
    # the profile may claim; the others are captured so a bump is ready early.
    [ValidateSet("Stable", "Beta", "Dev", "Canary")]
    [string]$Channel = "Stable",
    # Where the container gets its browser.
    #
    # `cft` is Chrome for Testing, which is addressable by channel and is the
    # only way to capture a build before it ships. It is **unbranded**: its
    # `sec-ch-ua` carries no "Google Chrome" entry at all, measured on
    # 2026-08-30 as `"Not?A_Brand";v="24", "Chromium";v="152"`.
    # `apt` is Google's own branded stable package, which is the only source
    # here that can supply that header, and it reaches stable and nothing else.
    [ValidateSet("cft", "apt")]
    [string]$Source = "cft",
    # The distro the container path builds from.
    [string]$Image = "debian:bookworm-slim",
    # Minutes to let the distro install a browser and drive it.
    [int]$ContainerMinutes = 12
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-browser-fingerprint: $message")
    exit $code
}

$exeDir = Join-Path $repo "target/$Build/examples"
$probe = Join-Path $exeDir "loopback-tlsprobe.exe"
if (-not (Test-Path $probe)) { $probe = Join-Path $exeDir "loopback-tlsprobe" }
$finder = Join-Path $exeDir "browser-capture.exe"
if (-not (Test-Path $finder)) { $finder = Join-Path $exeDir "browser-capture" }
foreach ($p in @($probe, $finder)) {
    if (-not (Test-Path $p)) {
        Exit-With 2 "$p is missing; run: cargo build --$Build --bins --examples"
    }
}

$scratch = Join-Path $repo ".tmp/browser-fingerprint"
New-Item -ItemType Directory -Force -Path $scratch | Out-Null

# Differences an open entry already names, so this check records them rather
# than failing a build for them. One row per entry, and the row goes when the
# entry closes.
#
# **It is empty, and that is the state to keep it in.** T-262's row was the
# only one: the Akamai PRIORITY field, where Chrome opened stream 1 with a
# block and `h2` sent none. T-262 closed, so the row came off, which is the
# other half of the rule about a check that measures an open defect. Adding one
# back means naming the entry that owns the difference.
$known = @()

# Whether a difference is one an entry already names. Nothing is exempt today;
# this is the shape a row would be judged by. For the Akamai fingerprint an
# exemption would mean every field but the third agrees, because anything else
# is a real disagreement even on the same line.
function Test-KnownAkamai([string]$claim, [string]$browser) {
    $a = $claim -split '\|'
    $b = $browser -split '\|'
    if ($a.Count -ne 4 -or $b.Count -ne 4) { return $false }
    return ($a[0] -ceq $b[0]) -and ($a[1] -ceq $b[1]) -and ($a[3] -ceq $b[3])
}

# ---------------------------------------------------------------------------
# Is there a browser at all? This is the case that has to work everywhere.
#
# The container path answers the same question a different way: Chrome for
# Testing publishes an exact build per channel, so the version is known before
# anything is installed, and the guest asserts it against what the binary
# actually reports.
# ---------------------------------------------------------------------------

$wslTool = Join-Path $PSScriptRoot "wsl-tool.ps1"
$distro = $null
$distroName = $null
$guestVersion = $null
$hostAddr = $null

function Invoke-WslTool {
    param([string[]]$ToolArgs, [string]$StdOut, [string]$StdErr, [int]$WaitMinutes = 5)
    $argv = @('-NoProfile', '-File', $wslTool) + $ToolArgs
    $proc = Start-Process -FilePath 'pwsh' -ArgumentList $argv -PassThru -NoNewWindow `
        -RedirectStandardOutput $StdOut -RedirectStandardError $StdErr
    if (-not $proc.WaitForExit($WaitMinutes * 60 * 1000)) {
        try { $proc | Stop-Process -Force -ErrorAction SilentlyContinue } catch {}
        return 124
    }
    return $proc.ExitCode
}

# Nothing this script creates outlives it, and the removal is checked rather
# than assumed. Registered in a `finally` so a failure anywhere below still
# reaches it.
function Remove-Distro {
    if (-not $distro) { return }
    $rmOut = Join-Path $scratch "remove.out"
    $rmErr = Join-Path $scratch "remove.err"
    $code = Invoke-WslTool -ToolArgs @('-Action', 'Remove', '-Name', $distro, '-Force') `
        -StdOut $rmOut -StdErr $rmErr -WaitMinutes 5
    if ($code -ne 0) {
        [Console]::Error.WriteLine("check-browser-fingerprint: could not remove $distro (exit $code); run: pwsh scripts/wsl-tool.ps1 -Action Purge -Force")
    }
    $script:distro = $null
}

if ($Container) {
    if (-not $IsWindows) { Exit-With 2 "-Container drives wsl.exe, so it is Windows only" }
    if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
        Exit-With 2 "-Container needs WSL2 and wsl.exe is not on PATH"
    }
    $engine = @('podman', 'docker') | Where-Object { Get-Command $_ -ErrorAction SilentlyContinue } | Select-Object -First 1
    if (-not $engine) {
        Exit-With 2 "-Container needs podman or docker to pull $Image and neither is on PATH"
    }
    if (-not (Test-Path $wslTool)) { Exit-With 2 "$wslTool is missing" }
}

if ($Container -and $Source -eq 'cft') {
    # Which build, from Google's own per-channel index. Read before anything is
    # installed, so a failure to reach it is a clean exit rather than a distro
    # to clean up.
    $cftUrl = "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json"
    try {
        $cft = Invoke-RestMethod -Uri $cftUrl -UseBasicParsing -TimeoutSec 30
    } catch {
        Exit-With 2 "cannot read $cftUrl : $($_.Exception.Message)"
    }
    $picked = $cft.channels.$Channel
    if (-not $picked) { Exit-With 2 "Chrome for Testing's index carries no $Channel channel" }
    $zip = @($picked.downloads.chrome | Where-Object { $_.platform -eq 'linux64' }) | Select-Object -First 1
    if (-not $zip) { Exit-With 2 "Chrome for Testing's $Channel channel publishes no linux64 build" }
    $browser = [ordered]@{
        path    = "chrome-for-testing $Channel, in a throwaway $Image distro"
        version = $picked.version
        major   = [int]($picked.version -split '\.')[0]
        channel = $Channel
        branded = $false
        source  = $zip.url
    }
} elseif ($Container) {
    if ($Channel -ne 'Stable') {
        Exit-With 2 "-Source apt is Google's stable package and reaches no other channel; use -Source cft for $Channel"
    }
    # The version is not knowable before the install here, unlike the index the
    # `cft` path reads, so it is filled in from the binary afterwards.
    $browser = [ordered]@{
        path    = "google-chrome-stable, in a throwaway $Image distro"
        version = $null
        major   = $null
        channel = 'Stable'
        branded = $true
        source  = "https://dl.google.com/linux/chrome/deb stable main"
    }
} else {
    $findArgs = @('--json')
    if ($BrowserPath) { $findArgs += @('--path', $BrowserPath) }
    $findOut = Join-Path $scratch "find.json"
    $findErr = Join-Path $scratch "find.err"
    $find = Start-Process -FilePath $finder -ArgumentList $findArgs -PassThru -NoNewWindow -Wait `
        -RedirectStandardOutput $findOut -RedirectStandardError $findErr
    if ($find.ExitCode -ne 0) {
        $why = (Get-Content -Raw $findErr -ErrorAction SilentlyContinue)
        if (-not $why) { $why = "no browser was found" }
        Exit-With 2 "$($why.Trim())"
    }
    $browser = Get-Content -Raw $findOut | ConvertFrom-Json
}

# ---------------------------------------------------------------------------
# What the browser puts on the wire
# ---------------------------------------------------------------------------

# In NAT mode a distro cannot reach the Windows loopback and the failure is
# silent, so the container path binds the probe where the distro can reach it.
$probeBind = @()
if ($Container) {
    $addrOut = Join-Path $scratch "hostaddr.out"
    $addrErr = Join-Path $scratch "hostaddr.err"
    $code = Invoke-WslTool -ToolArgs @('-Action', 'HostAddress') -StdOut $addrOut -StdErr $addrErr -WaitMinutes 3
    if ($code -ne 0) {
        $why = (Get-Content -Raw $addrErr -ErrorAction SilentlyContinue)
        Exit-With 2 "cannot work out the address a distro reaches this host at: $($why.Trim())"
    }
    $hostAddr = (Get-Content -Raw $addrOut).Trim()
    if ($hostAddr -notmatch '^[0-9a-fA-F:.]+$') {
        Exit-With 2 "the host address came back as '$hostAddr', which is not an address"
    }
    $probeBind = @('--bind', $hostAddr)
}

$probeOut = Join-Path $scratch "probe.txt"
$probeErr = Join-Path $scratch "probe.err"
# The raw ClientHello, kept as evidence rather than only as a hash. JA4 sorts
# and JA4_r sorts, so neither can say what order the browser put its extensions
# in or which codepoints are new; this is the only artefact that can.
$helloPath = Join-Path $scratch "clienthello.hex"
if (Test-Path $probeOut) { Remove-Item -Force $probeOut }
if (Test-Path $helloPath) { Remove-Item -Force $helloPath }
$p = Start-Process -FilePath $probe -PassThru -NoNewWindow `
    -ArgumentList (@('--until-h2', '--json', '--port', '0', '--header-values', '--hello-out', $helloPath) + $probeBind) `
    -RedirectStandardOutput $probeOut -RedirectStandardError $probeErr

# Wait on the fixture's own first line, never on a guessed duration.
$url = $null
for ($i = 0; $i -lt 100; $i++) {
    Start-Sleep -Milliseconds 100
    if (Test-Path $probeOut) {
        $first = Get-Content $probeOut -TotalCount 1 -ErrorAction SilentlyContinue
        if ($first) { $url = "$first".Trim(); break }
    }
    if ($p.HasExited) { break }
}
if (-not $url) {
    if (-not $p.HasExited) { $p | Stop-Process -Force -ErrorAction SilentlyContinue }
    Exit-With 2 "the probe never announced itself"
}

$driveOut = Join-Path $scratch "drive.out"
$driveErr = Join-Path $scratch "drive.err"

if ($Container) {
    # `/bin/sh` is dash on Debian, so nothing here is a bashism. The two URLs
    # are substituted rather than passed, because the command channel sources
    # the text and a sourced script has no positional arguments of its own.
    #
    # `--ignore-certificate-errors` and `--test-type` go to the browser and to
    # nothing that ships, for the reason `browser-capture` already records: the
    # probe's authority is minted per run and a browser that refuses it aborts
    # before it sends one HTTP/2 frame, which is exactly the half this is for.
    # It changes what the browser accepts after the handshake, not the
    # ClientHello it sends.
    #
    # **Trusting the authority properly was tried first and does not work
    # here.** Measured on 2026-08-30: the CA was added to `/root/.pki/nssdb`
    # with `certutil -t "C,,"`, `certutil -L` listed it, and Chrome 152 still
    # answered `CertificateUnknown`. Chrome on Linux uses its own root store
    # for server authentication and does not consult that database.
    #
    # `timeout` bounds the browser, because the failure mode being bounded is
    # a browser that never finishes rather than one that errors: without it a
    # Chrome that cannot complete a handshake sat until the distro was killed.
    $install = if ($Source -eq 'apt') { @'
echo "==> google-chrome-stable, from Google's own repository"
apt-get install -y -qq --no-install-recommends ca-certificates curl gnupg >/dev/null
curl -fsSL https://dl.google.com/linux/linux_signing_key.pub | gpg --dearmor -o /usr/share/keyrings/google-chrome.gpg
echo "deb [arch=amd64 signed-by=/usr/share/keyrings/google-chrome.gpg] https://dl.google.com/linux/chrome/deb/ stable main" > /etc/apt/sources.list.d/google-chrome.list
apt-get update -qq
apt-get install -y -qq google-chrome-stable >/dev/null
CHROME=/usr/bin/google-chrome-stable
'@ } else { @'
echo "==> chrome for testing"
apt-get install -y -qq --no-install-recommends \
  ca-certificates curl unzip \
  libnss3 libatk1.0-0 libatk-bridge2.0-0 libcups2 libdrm2 libxkbcommon0 \
  libxcomposite1 libxdamage1 libxfixes3 libxrandr2 libgbm1 libpango-1.0-0 \
  libcairo2 libasound2 libatspi2.0-0 libx11-6 libxcb1 libxext6 libexpat1 \
  fonts-liberation >/dev/null
curl -fsSL "@@CFT@@" -o /tmp/chrome.zip
unzip -q /tmp/chrome.zip -d /opt
CHROME=/opt/chrome-linux64/chrome
chmod +x "$CHROME"
'@ }

    $guest = @'
set -e
URL="@@URL@@"
export DEBIAN_FRONTEND=noninteractive
echo "==> apt"
apt-get update -qq
@@INSTALL@@
echo "==> version"
"$CHROME" --version
echo "==> drive"
timeout @@DRIVESECS@@ "$CHROME" --headless=new --no-sandbox --user-data-dir=/tmp/profile \
  --no-first-run --no-default-browser-check \
  --disable-search-engine-choice-screen --disable-gpu --test-type \
  --ignore-certificate-errors --dump-dom "$URL" >/dev/null 2>/tmp/chrome.err || true
echo "==> chrome stderr"
tail -5 /tmp/chrome.err 2>/dev/null || true
echo "==> done"
'@
    $guest = $guest.Replace('@@INSTALL@@', $install).
        Replace('@@URL@@', "$url/").
        Replace('@@CFT@@', "$($browser.source)").
        Replace('@@DRIVESECS@@', "$TimeoutSeconds")
    # A `.ps1` is CRLF in this working tree by `.gitattributes`, so a here-string
    # in this file carries carriage returns. `/bin/sh` would read each one as
    # part of the last word on its line. The command channel is byte-exact and
    # will not fix this for anybody.
    $guest = $guest -replace "`r`n", "`n"
    $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($guest))

    $distro = "eph-bitcli-fp-$([guid]::NewGuid().ToString('N').Substring(0,6))"
    $distroName = $distro
    try {
        # Not `-Ephemeral`: the guest's output is read after it exits and the
        # removal is checked separately, so the two stay legible when one fails.
        $code = Invoke-WslTool -WaitMinutes $ContainerMinutes -StdOut $driveOut -StdErr $driveErr `
            -ToolArgs @('-Action', 'New', '-Image', $Image, '-Name', $distro, '-Force', '-CommandB64', $b64)
    } finally {
        Remove-Distro
    }
    if (-not $p.HasExited) { $p.WaitForExit(10000) | Out-Null }
    if (-not $p.HasExited) { $p | Stop-Process -Force -ErrorAction SilentlyContinue }

    $guestText = (Get-Content -Raw $driveOut -ErrorAction SilentlyContinue)
    if ($guestText -match 'Chrome[^\d]*(\d+\.\d+\.\d+\.\d+)') { $guestVersion = $Matches[1] }
    if ($code -eq 124) {
        Exit-With 2 "the distro did not finish inside $ContainerMinutes minute(s); raise -ContainerMinutes"
    }
    if ($code -ne 0) {
        $why = (Get-Content -Raw $driveErr -ErrorAction SilentlyContinue)
        Exit-With 2 "the distro exited $code. $($why.Trim())"
    }
    if (-not $guestVersion) {
        Exit-With 2 "the distro installed a browser that did not report a version"
    }
    if ($null -eq $browser.version) {
        # The apt path cannot know the version before installing it, so the
        # binary is the only source and there is nothing to cross-check.
        $browser.version = $guestVersion
        $browser.major = [int]($guestVersion -split '\.')[0]
    } elseif ($guestVersion -ne $browser.version) {
        # The index said which build this would be before it was installed. If
        # the binary disagrees, the index is not describing what ran and every
        # number below is attributed to the wrong version.
        Exit-With 2 "Chrome for Testing's index says $Channel is $($browser.version) and the binary reports $guestVersion"
    }
} else {
    $driveArgs = @('--url', "$url/", '--timeout', "$TimeoutSeconds")
    if ($BrowserPath) { $driveArgs += @('--path', $BrowserPath) }
    Start-Process -FilePath $finder -ArgumentList $driveArgs -NoNewWindow -Wait `
        -RedirectStandardOutput $driveOut -RedirectStandardError $driveErr | Out-Null

    $p.WaitForExit(10000) | Out-Null
    if (-not $p.HasExited) { $p | Stop-Process -Force -ErrorAction SilentlyContinue }
}

# Everything the run created is gone, and this reads the state back rather than
# trusting the removal. A session that leaves a distro behind has left a VHDX of
# a few hundred MiB on somebody's disk.
$containerClean = $null
if ($Container) {
    $listOut = Join-Path $scratch "list.out"
    $listErr = Join-Path $scratch "list.err"
    Invoke-WslTool -ToolArgs @('-Action', 'List') -StdOut $listOut -StdErr $listErr -WaitMinutes 3 | Out-Null
    $listText = (Get-Content -Raw $listErr -ErrorAction SilentlyContinue) + (Get-Content -Raw $listOut -ErrorAction SilentlyContinue)
    $containerClean = ($listText -notmatch [regex]::Escape($distroName)) -and ($listText -notmatch 'eph-bitcli-fp-')
    if (-not $containerClean) {
        Exit-With 1 "a distro or a rootfs tarball from this run is still registered; run: pwsh scripts/wsl-tool.ps1 -Action Purge -Force"
    }
}

$lines = @(Get-Content $probeOut -ErrorAction SilentlyContinue)
if ($lines.Count -lt 2) { Exit-With 2 "the probe captured nothing from the browser" }

# **The capture to take is the first one that reached HTTP/2, and neither the
# first nor the last connection is reliably it.** Measured on 2026-08-30
# driving Chrome 152 at the probe: 13 connections, the first carrying no HTTP/2
# at all because a browser opens sockets it then abandons, and every one after
# the second carrying `pre_shared_key` because the session resumed. Taking the
# first would report a failed handshake; taking the last would record an
# extension a cold client never sends.
$captures = @()
foreach ($line in $lines) {
    if ($line -notlike '{*') { continue }
    try { $captures += ($line | ConvertFrom-Json) } catch { }
}
if ($captures.Count -eq 0) { Exit-With 2 "the probe's output carried no capture: $($lines[-1])" }
$observed = $captures | Where-Object { $_.akamai } | Select-Object -First 1
if (-not $observed) {
    $why = (Get-Content -Raw $probeErr -ErrorAction SilentlyContinue)
    Exit-With 2 "the browser completed no HTTP/2 request in $($captures.Count) connection(s), so there is nothing to compare. $($why.Trim())"
}

# ---------------------------------------------------------------------------
# What this repository claims
# ---------------------------------------------------------------------------

$goldenRoot = if ([System.IO.Path]::IsPathRooted($GoldenDir)) { $GoldenDir } else { Join-Path $repo $GoldenDir }
$goldenPath = Join-Path $goldenRoot "bit-cli-browser.json"
if (-not (Test-Path $goldenPath)) {
    Exit-With 2 "$goldenPath is not there; run scripts/check-fingerprint.ps1 -Update first"
}
$claim = (Get-Content -Raw $goldenPath | ConvertFrom-Json).fingerprint

$pageRs = Join-Path $repo "crates/bit-cli-core/src/page.rs"
$pageText = Get-Content -Raw $pageRs
$claimedMajor = $null
if ($pageText -match 'pub const BROWSER_MAJOR:\s*u32\s*=\s*(\d+)') { $claimedMajor = [int]$Matches[1] }

# ---------------------------------------------------------------------------
# Compare, field by field
# ---------------------------------------------------------------------------

$problems = @()

if ($claim.ja4 -cne $observed.ja4) {
    $problems += [ordered]@{
        field = "ja4"
        claim = $claim.ja4
        browser = $observed.ja4
        where = "crates/bit-cli-core/src/fetch.rs, through impit's fingerprint database"
    }
}
if ($claim.akamai -cne $observed.akamai) {
    $row = [ordered]@{
        field = "akamai"
        claim = $claim.akamai
        browser = $observed.akamai
        where = "crates/bit-cli-core/src/page.rs, BROWSER_H2_* and impit's fingerprint database"
        known = $null
    }
    $exempt = @($known | Where-Object { $_.field -eq 'akamai' }) | Select-Object -First 1
    if (-not $Strict -and $exempt -and (Test-KnownAkamai $claim.akamai $observed.akamai)) {
        $row.known = $exempt
        $row.where = "$($exempt.entry): $($exempt.why)"
    }
    $problems += $row
}
$claimHeaders = @($claim.h2_headers)
$browserHeaders = @($observed.headers)
if (($claimHeaders -join '|') -cne ($browserHeaders -join '|')) {
    $problems += [ordered]@{
        field = "header order"
        claim = ($claimHeaders -join ', ')
        browser = ($browserHeaders -join ', ')
        where = "crates/bit-cli-core/src/page.rs, BROWSER_HEADERS"
    }
}

$browserMajor = $browser.major
if ($null -ne $browserMajor -and $null -ne $claimedMajor -and $browserMajor -ne $claimedMajor) {
    $problems += [ordered]@{
        field = "browser major"
        claim = "$claimedMajor"
        browser = "$browserMajor"
        where = "crates/bit-cli-core/src/page.rs, BROWSER_MAJOR"
    }
}

# The replacement, in the shape page.rs wants. Written whenever a capture
# happened, not only on a failure: a passing run's block is what a reader
# checks the file against by eye.
# A headless capture says `HeadlessChrome/151.0.0.0` where the browser a
# person runs says `Chrome/151.0.0.0`, and the same substitution reaches
# `sec-ch-ua` on some builds. Pasting the capture verbatim would ship a
# User-Agent that announces automation, which is the one thing this profile
# exists not to do, so the replacement is normalised and the substitution is
# reported beside it.
$headless = @()
$pairs = @($observed.header_pairs | ForEach-Object {
        $name = $_[0]
        $value = $_[1]
        if ($value -match 'HeadlessChrome') {
            $headless += $name
            $value = $value -replace 'HeadlessChrome', 'Chrome'
        }
        , @($name, $value)
    })
$rustHeaders = ($pairs | ForEach-Object {
        $name = $_[0]
        $value = ($_[1] -replace '\\', '\\\\') -replace '"', '\"'
        "    (`"$name`", `"$value`"),"
    }) -join "`n"

$recommend = [ordered]@{
    from             = [ordered]@{
        path    = $browser.path
        version = $browser.version
        major   = $browser.major
    }
    ja4              = $observed.ja4
    ja4_r            = $observed.ja4_r
    akamai           = $observed.akamai
    header_order     = $browserHeaders
    browser_headers  = "pub const BROWSER_HEADERS: &[(&str, &str)] = &[`n$rustHeaders`n];"
    headless_rewritten = $headless
    hello            = $helloPath
    note             = "Every value of the profile is in crates/bit-cli-core/src/page.rs, including the cipher list, the extension order and the HTTP/2 settings, so a JA4 that has moved is that file to edit and not a vendored one to reconcile. A codepoint impit's ExtensionType cannot name is a finding to record rather than a value to drop."
}
if ($Container -and $Source -eq 'cft') {
    $recommend.headers_caveat = "Chrome for Testing is unbranded and this capture is Linux, so sec-ch-ua carries no Google Chrome entry and sec-ch-ua-platform and the User-Agent name the wrong platform. Use -Source apt for the brand list."
}

$judged = @($problems | Where-Object { -not $_.known })
$pass = $judged.Count -eq 0

$report = [ordered]@{
    schema    = "browser-fingerprint/1"
    generated = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    browser   = $browser
    observed  = [ordered]@{
        ja4          = $observed.ja4
        ja4_r        = $observed.ja4_r
        ja3          = $observed.ja3
        akamai       = $observed.akamai
        headers      = $browserHeaders
        header_pairs = $pairs
    }
    claim     = [ordered]@{
        ja4        = $claim.ja4
        akamai     = $claim.akamai
        h2_headers = $claimHeaders
        major      = $claimedMajor
    }
    pass      = $pass
    strict    = [bool]$Strict
    problems  = $problems
    judged    = $judged.Count
    recommend = $recommend
}

if ($Container) {
    $report.container = [ordered]@{
        image        = $Image
        channel      = $Channel
        distro       = $distroName
        host_address = $hostAddr
        binary_version = $guestVersion
        removed      = [bool]$containerClean
    }
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
    Write-Host ("browser  {0}" -f $browser.path)
    Write-Host ("version  {0}" -f $browser.version)
    if ($Container) {
        Write-Host ("distro   {0} on {1}, removed: {2}" -f $Image, $hostAddr, $containerClean)
    }
    Write-Host ""
    Write-Host ("  JA4     browser {0}" -f $observed.ja4)
    Write-Host ("          bit-cli {0}" -f $claim.ja4)
    Write-Host ("  akamai  browser {0}" -f $observed.akamai)
    Write-Host ("          bit-cli {0}" -f $claim.akamai)
    Write-Host ("  headers browser {0}" -f ($browserHeaders -join ', '))
    Write-Host ("          bit-cli {0}" -f ($claimHeaders -join ', '))
    Write-Host ""
    foreach ($problem in $problems) {
        $mark = if ($problem.known) { "note" } else { "FAIL" }
        Write-Host ("{0} {1}" -f $mark, $problem.field)
        Write-Host ("       claim   {0}" -f $problem.claim)
        Write-Host ("       browser {0}" -f $problem.browser)
        Write-Host ("       change  {0}" -f $problem.where)
    }
    if ($problems.Count -gt 0) { Write-Host "" }
    if ($pass) {
        $tail = if ($problems.Count -gt 0) { ", apart from what an entry already names" } else { "" }
        Write-Host "check-browser-fingerprint: the profile matches this browser$tail"
    } else {
        Write-Host "check-browser-fingerprint: $($judged.Count) field(s) disagree"
    }
    if (-not $pass -or $problems.Count -gt 0) {
        Write-Host ""
        Write-Host "the replacement, from $($browser.version):"
        Write-Host $recommend.browser_headers
        if ($headless.Count -gt 0) {
            Write-Host ""
            Write-Host ("HeadlessChrome was rewritten to Chrome in: {0}" -f ($headless -join ', '))
        }
        Write-Host ""
        Write-Host $recommend.note
    }
}

if (-not $pass) { exit 1 }
exit 0
