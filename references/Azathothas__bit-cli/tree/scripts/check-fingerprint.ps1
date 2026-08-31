# What does this client actually put on the wire?
#
# The defect it exists to catch: `TODO/cli-surface.md` T-244 fetches a source
# document while presenting as a browser, and every part of that presentation
# is invisible from inside the process. A client's own view of its handshake is
# the view it intended. The header set can be read from the code; the TLS
# `ClientHello` and the HTTP/2 SETTINGS frame are decided by `rustls` and `h2`
# and change when either is upgraded, silently and without a test failing.
#
# So this drives `bit-cli` at `loopback-tlsprobe`, reads the fingerprint off
# the wire, and compares it against a golden committed under `fingerprints/`.
# Nothing here touches the network.
#
# Three captures, because they need different things:
#
#   raw         no handshake is completed, so nothing has to be disabled to
#               reach it. This is where JA4 is read, and it is the JA4 that
#               ships: a client told to skip certificate verification can fall
#               back to a different `signature_algorithms` list, and the JA4
#               read through that handshake is not the one an origin sees.
#   plain       cleartext HTTP/1.1, which is where the header order of a
#               client that will not complete a handshake is still readable.
#   tls         a real handshake, ALPN picking `h2`. This is where the Akamai
#               HTTP/2 fingerprint and the HPACK-decoded header order are
#               read, and neither exists before it.
#
# **Nothing here weakens certificate verification, and that is the point.** The
# probe mints its own certificate authority per run, signs the leaf with it and
# writes the authority to a file; the run under test is given that file through
# `BIT_CLI_EXTRA_CA_FILE`, which **adds** a root to the usual ones. The chain
# is verified normally. A client told to skip verification is a different
# client on the wire, so a fingerprint read through one would be a fingerprint
# of something that never ships.
#
# **JA4 is asserted and JA3 is not.** JA4 sorts ciphers and extensions before
# hashing, so it survives a client that shuffles its extension order; JA3
# preserves wire order and flakes. JA3 is recorded for a reader and never
# compared.
#
# Usage:
#   pwsh scripts/check-fingerprint.ps1
#   pwsh scripts/check-fingerprint.ps1 -Update      # rewrite the goldens
#   pwsh scripts/check-fingerprint.ps1 -Json
#
# Exit 0 when every capture matched its golden, 1 when one did not, 2 when it
# could not run. With no golden present it records what it saw and exits 0,
# saying so: a check that has never been given an answer must not invent one.
#
# See TODO/cli-surface.md, T-244.

[CmdletBinding()]
param(
    [switch]$Json,
    # Rewrite the goldens from what was captured. A deliberate act: the point
    # of the check is that the fingerprint does not move without somebody
    # deciding it should.
    [switch]$Update,
    [ValidateSet("", "browser", "plain")]
    [string]$Profile = "",
    [string]$GoldenDir = "fingerprints",
    [ValidateSet("debug", "release")]
    [string]$Build = "release",
    # How many handshakes each capture makes. More than one because the
    # `ClientHello` draws two GREASE codepoints per connection, so one
    # handshake samples one draw of sixteen. Eight puts the odds of missing a
    # three-in-sixteen defect at about one in ten, where one handshake put them
    # at four in five. See TODO/cli-surface.md, T-263.
    [ValidateRange(1, 64)]
    [int]$Handshakes = 8
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-fingerprint: $message")
    exit $code
}

$exeDir = Join-Path $repo "target/$Build"
$bit = Join-Path $exeDir "bit-cli.exe"
if (-not (Test-Path $bit)) { $bit = Join-Path $exeDir "bit-cli" }
$probe = Join-Path $exeDir "examples/loopback-tlsprobe.exe"
if (-not (Test-Path $probe)) { $probe = Join-Path $exeDir "examples/loopback-tlsprobe" }
foreach ($p in @($bit, $probe)) {
    if (-not (Test-Path $p)) {
        Exit-With 2 "$p is missing; run: cargo build --$Build --bins --examples"
    }
}

$goldenRoot = if ([System.IO.Path]::IsPathRooted($GoldenDir)) { $GoldenDir } else { Join-Path $repo $GoldenDir }
if ($Update) { New-Item -ItemType Directory -Force -Path $goldenRoot | Out-Null }
$scratch = Join-Path $repo ".tmp/fingerprint"
New-Item -ItemType Directory -Force -Path $scratch | Out-Null

# Start the probe, point `$Handshakes` bit-cli runs at it, and return the
# capture they made. `$Mode` decides which half of the fingerprint is reachable.
#
# **More than one run, and that is a repair rather than thoroughness.** The
# `ClientHello` carries two GREASE codepoints drawn per connection from the
# sixteen RFC 8701 reserves, so one handshake samples one draw. T-263 shipped a
# defect that rejected three of those sixteen, which is one handshake in five,
# and this check made exactly one: it failed a CI run, passed the next one over
# the same defect, and would have been called noise by anybody reading a single
# green run. Every capture has to match, so a rate is caught rather than
# sampled.
function Get-Capture([string]$profileName, [string]$Mode) {
    $tag = "$profileName-$Mode"
    $out = Join-Path $scratch "$tag-out.txt"
    $err = Join-Path $scratch "$tag-err.txt"
    if (Test-Path $out) { Remove-Item -Force $out }
    $probeArgs = @('--json', '--port', '0')
    if ($Mode -eq 'raw') { $probeArgs += '--raw' }
    if ($Mode -eq 'plain') { $probeArgs += '--plain' }
    # The `tls` capture is the only one that completes a handshake, so it is
    # the only one that needs a certificate the run under test will accept.
    $caFile = $null
    if ($Mode -eq 'tls') {
        $caFile = Join-Path $scratch "$tag-ca.pem"
        if (Test-Path $caFile) { Remove-Item -Force $caFile }
        $probeArgs += @('--ca-out', $caFile)
    }

    $p = Start-Process -FilePath $probe -ArgumentList $probeArgs -PassThru -NoNewWindow `
        -RedirectStandardOutput $out -RedirectStandardError $err

    # Wait on the fixture's own first line, never on a guessed duration.
    $url = $null
    for ($i = 0; $i -lt 100; $i++) {
        Start-Sleep -Milliseconds 100
        if (Test-Path $out) {
            $first = Get-Content $out -TotalCount 1 -ErrorAction SilentlyContinue
            if ($first) { $url = "$first".Trim(); break }
        }
        if ($p.HasExited) { break }
    }
    if (-not $url) {
        if (-not $p.HasExited) { $p | Stop-Process -Force -ErrorAction SilentlyContinue }
        return @{ error = "the probe never announced itself" }
    }

    # The fetch always fails: the certificate is camouflage and in raw mode
    # there is no handshake at all. The ClientHello is on the wire before
    # either of those matters, which is the whole point.
    $runOut = Join-Path $scratch "$tag-run.txt"
    $argv = @('info', "$url/one.torrent", '--page-client', $profileName, '--timeout', '10s')
    $hadCa = $env:BIT_CLI_EXTRA_CA_FILE
    if ($caFile) { $env:BIT_CLI_EXTRA_CA_FILE = $caFile }
    try {
        for ($run = 0; $run -lt $Handshakes; $run++) {
            Start-Process -FilePath $bit -ArgumentList $argv -NoNewWindow -Wait `
                -RedirectStandardOutput $runOut -RedirectStandardError "$runOut.err" | Out-Null
        }
    } finally {
        if ($null -eq $hadCa) {
            Remove-Item Env:BIT_CLI_EXTRA_CA_FILE -ErrorAction SilentlyContinue
        } else {
            $env:BIT_CLI_EXTRA_CA_FILE = $hadCa
        }
    }

    # The probe serves until it is stopped now, because it is no longer told to
    # exit after one connection.
    if (-not $p.HasExited) { $p | Stop-Process -Force -ErrorAction SilentlyContinue }

    $captures = @()
    foreach ($line in @(Get-Content $out -ErrorAction SilentlyContinue)) {
        if ($line -notlike '{*') { continue }
        try { $captures += ($line | ConvertFrom-Json) } catch { }
    }
    if ($captures.Count -eq 0) { return @{ error = "the probe captured nothing" } }

    # **The cold capture is the one that counts, and it is the first.**
    # Measured on 2026-08-30 over eleven captures of one binary: eight carried
    # `session_ticket` and three carried `pre_shared_key` instead, because the
    # connection resumed, and the two produce different JA4s. That is the
    # client telling the truth rather than a defect, and it is the same thing a
    # real Chrome does. So the captures are **not** required to be identical: a
    # resumed hello legitimately differs, and comparing one against the golden
    # would fail for the wrong reason.
    #
    # What every capture does have to do in `tls` mode is reach HTTP/2, which
    # is the shape the T-263 defect took: a handshake that failed for three
    # draws in sixteen left an Akamai fingerprint that was simply absent, and
    # one handshake sampled it four times in five.
    if ($Mode -eq 'tls') {
        $withoutH2 = @($captures | Where-Object { -not $_.akamai }).Count
        if ($withoutH2 -gt 0) {
            return @{
                error = "$withoutH2 of $($captures.Count) handshake(s) reached no HTTP/2 at all, so something in the handshake fails at a rate rather than always; see $err"
            }
        }
    }
    try {
        return @{ capture = $captures[0] }
    } catch {
        return @{ error = "the probe's output is not JSON: $($lines[1])" }
    }
}

$profiles = if ($Profile) { @($Profile) } else { @('browser', 'plain') }
$results = @()

foreach ($name in $profiles) {
    $raw = Get-Capture $name 'raw'
    $plain = Get-Capture $name 'plain'
    $tls = Get-Capture $name 'tls'
    if ($raw.error) { Exit-With 2 "$name raw capture: $($raw.error)" }
    if ($plain.error) { Exit-With 2 "$name plain capture: $($plain.error)" }
    if ($tls.error) { Exit-With 2 "$name tls capture: $($tls.error)" }

    $observed = [ordered]@{
        profile    = $name
        # From the raw capture, which is the one that ships.
        ja4        = $raw.capture.ja4
        ja4_r      = $raw.capture.ja4_r
        # Recorded for a reader and never compared: JA3 preserves wire order.
        ja3        = $raw.capture.ja3
        # From the cleartext capture. `Host` is dropped: it carries the port
        # the probe happened to bind, so keeping it would make the golden
        # depend on a free port.
        headers    = @($plain.capture.headers | Where-Object { $_ -ne 'host' })
        # From the handshake capture, and neither exists without one. The
        # Akamai string is SETTINGS|WINDOW_UPDATE|PRIORITY|PSEUDO_HEADER_ORDER.
        akamai     = $tls.capture.akamai
        h2_headers = @($tls.capture.headers)
    }

    $goldenPath = Join-Path $goldenRoot "bit-cli-$name.json"
    $row = [ordered]@{
        profile  = $name
        ja4      = $observed.ja4
        headers  = $observed.headers.Count
        golden   = (Test-Path $goldenPath)
        pass     = $true
        detail   = ""
        problems = @()
    }

    if ($Update) {
        $doc = [ordered]@{
            schema      = "fingerprint/2"
            captured    = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
            note        = "Captured off the wire by loopback-tlsprobe. ja4 and ja4_r come from a --raw capture, headers from a --plain one, akamai and h2_headers from a real handshake the probe's own CA makes verifiable. ja3 is recorded and never asserted, because it preserves wire order and flakes."
            bit_cli     = (& $bit --version 2>$null | Select-Object -First 1)
            fingerprint = $observed
        }
        [System.IO.File]::WriteAllText($goldenPath, ($doc | ConvertTo-Json -Depth 6))
        $row.detail = "wrote $goldenPath"
        $row.golden = $true
    } elseif (-not (Test-Path $goldenPath)) {
        $row.detail = "no golden at $goldenPath, recorded only; pass -Update to write one"
    } else {
        $want = (Get-Content -Raw $goldenPath | ConvertFrom-Json).fingerprint
        $problems = @()
        if ($want.ja4 -cne $observed.ja4) {
            $problems += "ja4 want '$($want.ja4)' got '$($observed.ja4)'"
        }
        if ($want.ja4_r -cne $observed.ja4_r) {
            $problems += "ja4_r differs, which says where: want '$($want.ja4_r)' got '$($observed.ja4_r)'"
        }
        $wantHeaders = @($want.headers)
        if (($wantHeaders -join '|') -cne ($observed.headers -join '|')) {
            $problems += "header order want [$($wantHeaders -join ', ')] got [$($observed.headers -join ', ')]"
        }
        if ($want.akamai -cne $observed.akamai) {
            $problems += "akamai want '$($want.akamai)' got '$($observed.akamai)'"
        }
        $wantH2 = @($want.h2_headers)
        if (($wantH2 -join '|') -cne ($observed.h2_headers -join '|')) {
            $problems += "http/2 header order want [$($wantH2 -join ', ')] got [$($observed.h2_headers -join ', ')]"
        }
        $row.problems = $problems
        $row.pass = $problems.Count -eq 0
        $row.detail = if ($problems.Count -eq 0) { "matches the golden" } else { $problems[0] }
    }

    $results += [pscustomobject]$row
    $mark = if ($row.pass) { "ok  " } else { "FAIL" }
    Write-Host ("{0} {1,-8} {2}" -f $mark, $name, $row.detail)
    Write-Host ("       JA4     {0}" -f $observed.ja4)
    Write-Host ("       headers {0}" -f ($observed.headers -join ', '))
    foreach ($problem in ($row.problems | Select-Object -Skip 1)) {
        Write-Host "       $problem"
    }
}

$failed = @($results | Where-Object { -not $_.pass })
$report = [ordered]@{
    schema    = "fingerprint-check/1"
    generated = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    profiles  = $results.Count
    failed    = $failed.Count
    pass      = $failed.Count -eq 0
    results   = $results
}

if ($Json) {
    $report | ConvertTo-Json -Depth 6
} else {
    Write-Host ""
    Write-Host ("check-fingerprint: {0} profile(s), {1} failed" -f $results.Count, $failed.Count)
}

if ($failed.Count -gt 0) { exit 1 }
exit 0
