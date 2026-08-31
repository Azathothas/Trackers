# Do the clients still build their identity the way we reproduce it?
#
# The defect this exists to catch is a client profile that was true when it was
# derived. `scripts/make-client-profile.ps1` reads a client's own source at a
# tag and refuses to emit anything when the construction it knows how to read
# is no longer there. That refusal is only useful if somebody runs it, and
# nothing runs it: a profile derived once and committed goes stale silently,
# which is the whole failure mode T-234 in `TODO/peers.md` exists to avoid.
#
# So this is the canary, and it is the same instrument as
# `scripts/upstream-scan.ps1`: a periodic read of somebody else's repository
# that says whether our record of it still holds. It runs the derivation for
# every client at its newest stable release and at its newest prerelease, and
# it fails when a guard fails.
#
# It fails on a guard rather than on a changed value. A new release is normal
# and produces a new peer id prefix, which is not a failure. A release whose
# CMakeLists.txt no longer builds the prefix from BASE62, or whose session
# implementation no longer calls `generate_fingerprint` with the four version
# constants, is: it means the profile this repository would build is a
# description of a client that does not exist.
#
# It needs the network and it reads two public repositories through `gh`. It
# writes nothing anywhere but the record path, and it announces nothing.
#
# Usage:
#   pwsh -NoProfile -File scripts/check-client-profile.ps1
#   pwsh -NoProfile -File scripts/check-client-profile.ps1 -Client transmission
#   pwsh -NoProfile -File scripts/check-client-profile.ps1 -Json
#
# Exit codes: 0 every guard held, 1 a guard failed, 2 could not run, which here
# means `gh` is missing or a repository could not be read. It is not in
# `scripts/gates.ps1` for that reason: a gate that fails when a network is
# down is a gate people learn to ignore.

[CmdletBinding()]
param(
    [ValidateSet('all', 'qbittorrent', 'transmission')]
    [string]$Client = 'all',
    # Derive only this release kind. Both by default, because a prerelease is
    # where a construction changes first.
    [ValidateSet('both', 'stable', 'beta')]
    [string]$Kind = 'both',
    [string]$Out = 'patches/profiles',
    [switch]$Json
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$maker = Join-Path $PSScriptRoot 'make-client-profile.ps1'

function Say([string]$text) {
    if (-not $Json) { Write-Host "check-client-profile: $text" }
}

if (-not (Test-Path $maker)) {
    [Console]::Error.WriteLine("check-client-profile: scripts/make-client-profile.ps1 is not there")
    exit 2
}
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine("check-client-profile: gh is not on PATH, so no source can be read")
    exit 2
}

# The arithmetic first. A self-test failure means the peer ids this would
# derive are wrong whatever upstream says, so there is no point reading
# upstream until it passes.
$selfTest = & pwsh -NoProfile -File $maker -SelfTest 2>&1
if ($LASTEXITCODE -ne 0) {
    foreach ($line in $selfTest) { [Console]::Error.WriteLine($line) }
    [Console]::Error.WriteLine("check-client-profile: the derivation's own self-test fails, so nothing was read")
    exit 1
}
Say "self-test passes"

$clients = if ($Client -eq 'all') { @('qbittorrent', 'transmission') } else { @($Client) }
$kinds = if ($Kind -eq 'both') { @('stable', 'beta') } else { @($Kind) }

$results = @()
$failed = 0
$couldNotRun = 0

foreach ($name in $clients) {
    foreach ($kind in $kinds) {
        $raw = & pwsh -NoProfile -File $maker -Client $name -Latest $kind -Json 2>&1
        $code = $LASTEXITCODE
        $text = ($raw | Out-String)

        # A prerelease behind the newest stable is not a failure and not a
        # profile. It is the ordinary state of a project between betas.
        if ($code -eq 1 -and $text -match 'no prerelease ahead of its newest stable') {
            Say "$name $kind : none ahead of stable"
            $results += [ordered]@{
                client = $name; kind = $kind; state = 'none'
                detail = 'no prerelease ahead of the newest stable release'
            }
            continue
        }

        if ($code -eq 2) {
            $couldNotRun++
            Say "$name $kind : could not read the source"
            $results += [ordered]@{
                client = $name; kind = $kind; state = 'unreadable'; detail = $text.Trim()
            }
            continue
        }

        $doc = $null
        $brace = $text.IndexOf('{')
        if ($brace -ge 0) {
            try { $doc = $text.Substring($brace) | ConvertFrom-Json } catch { $doc = $null }
        }

        if ($code -ne 0) {
            $failed++
            $guards = if ($doc -and $doc.guard_failures) { @($doc.guard_failures) } else { @($text.Trim()) }
            Say "$name $kind : GUARD FAILED"
            foreach ($g in $guards) { Say "    $g" }
            $results += [ordered]@{
                client = $name; kind = $kind; state = 'guard-failed'; guards = $guards
            }
            continue
        }

        if ($null -eq $doc) {
            $failed++
            Say "$name $kind : the derivation exited 0 and wrote nothing a parser could read"
            $results += [ordered]@{ client = $name; kind = $kind; state = 'unparseable' }
            continue
        }

        Say ("{0} {1} : {2} {3} ua {4}" -f $name, $kind, $doc.version, $doc.peer_id.prefix, $doc.tracker_http.user_agent)
        $results += [ordered]@{
            client   = $name
            kind     = $kind
            state    = 'ok'
            version  = $doc.version
            release  = $doc.release
            ref      = $doc.derived_from.ref
            prefix   = $doc.peer_id.prefix
            agent    = $doc.tracker_http.user_agent
            prerelease_visible = $doc.peer_id.prerelease_visible
        }
    }
}

$record = [ordered]@{
    kind          = 'client_profile_scan'
    generated_at  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
    clients       = $clients
    kinds         = $kinds
    guard_failures = $failed
    unreadable     = $couldNotRun
    results       = $results
}

if ($Out) {
    $dir = if ([System.IO.Path]::IsPathRooted($Out)) { $Out } else { Join-Path $repo $Out }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $stamp = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssfffZ')
    $path = Join-Path $dir "profiles-$stamp.json"
    $record | ConvertTo-Json -Depth 8 | Set-Content -Path $path -Encoding utf8NoBOM
    Say "wrote $path"
}

if ($Json) { $record | ConvertTo-Json -Depth 8 }

if ($couldNotRun -gt 0 -and $failed -eq 0) {
    [Console]::Error.WriteLine("check-client-profile: $couldNotRun source(s) could not be read")
    exit 2
}
if ($failed -gt 0) {
    [Console]::Error.WriteLine("check-client-profile: $failed guard failure(s). Read the files each one names before editing make-client-profile.ps1")
    exit 1
}
Say "every guard held"
exit 0
