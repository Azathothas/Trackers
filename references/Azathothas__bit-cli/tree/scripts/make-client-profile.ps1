<#
.SYNOPSIS
    Derive a BitTorrent client identity profile from that client's own tagged
    source, and refuse to guess when the source no longer says what it said.

.DESCRIPTION
    A client profile is not a string. What a tracker and a peer see is a peer
    id, a User-Agent, a set of query parameters in a fixed order, a key whose
    alphabet and width are the client's own, and a set of headers. Getting one
    of those wrong is what makes a mask fail on the second check.

    The defect this exists to catch is a profile copied from another emulator
    rather than derived from the client. Five projects share one profile format
    and no two of them agree on what it means. Four of the five never emit a
    qBittorrent `key` with a leading zero; libtorrent writes `key=%08X`, so a
    real one starts with `0` once in sixteen. Every one of them reproduced the
    format faithfully and the client not at all.

    So this reads the client's own repository at a tag, extracts the version
    constants and the identity construction, and asserts that the construction
    is still the one it knows how to read. When an upstream file moves or a
    line changes, it exits 1 and names what moved, rather than emitting a
    profile that describes a client that no longer exists.

        pwsh -NoProfile -File scripts/make-client-profile.ps1 -Client qbittorrent -Version 5.2.3
        pwsh -NoProfile -File scripts/make-client-profile.ps1 -Client transmission -Latest stable
        pwsh -NoProfile -File scripts/make-client-profile.ps1 -Client transmission -Latest beta -Json
        pwsh -NoProfile -File scripts/make-client-profile.ps1 -SelfTest

    `-Latest` reads the tag list, sorts it by parsed version rather than by the
    order the API returned, and takes the highest of the kind asked for. A beta
    older than the newest stable is not offered, because a prerelease nobody
    would run is not a client to imitate.

    Exit codes follow the check-script contract: 0 the profile was derived and
    every guard held, 1 a guard failed or a value could not be extracted, 2 the
    run could not start, which here means `gh` is missing or the network is not
    reachable.

    Every fetch is a read of a public repository. Nothing is written anywhere
    but the path given by -Out, and nothing is announced anywhere.

.NOTES
    Ported from joal's `scripts/bittorrent-client-update-detector/`, Apache-2.0,
    at 90e710ba01ac6a8665eb352a612ce4e9581483c8. This is an independent
    implementation written from the observed behaviour of those two scripts;
    no line was copied. What it does differently is in `docs/reference-mining.md`
    and under T-234 in `TODO/peers.md`. The three that matter:

      - the version to character encoding is table driven and tested over its
        whole range, so a component of 10 or more produces `A` rather than two
        characters and a peer id one byte too long
      - every value the run extracts is used, rather than extracted, printed,
        and then replaced by a hardcoded template
      - the profile carries the peer wire surface as well as the announce, and
        says which fields were derived and which were left unknown

    **Nothing constant is assumed.** An earlier form of this script hardcoded
    the fourth character of both prefixes as `0`, and that is wrong for a
    prerelease of either client:

      Transmission  CMakeLists.txt derives TR_PEER_ID_PREFIX and sets the
                    seventh character to `Z` for a dev build, `B` for a beta
                    and `0` for a stable release. A beta announces `-TR410B-`.
      qBittorrent   sessionimpl.cpp passes QBT_VERSION_BUILD as the fourth
                    component, which is `0` today and is a constant in a file
                    rather than a constant of the format. Its User-Agent takes
                    QBT_VERSION_STATUS, so a beta is `qBittorrent/5.1.0beta1`
                    while its peer id is the one the stable release will use.

    Every one of those four values is read from the tag now, and the guard
    fails if the construction that produces it is no longer there.
#>

[CmdletBinding()]
param(
    [ValidateSet('qbittorrent', 'transmission')]
    [string]$Client = 'qbittorrent',
    [string]$Version,
    [ValidateSet('stable', 'beta')]
    [string]$Latest,
    [string]$Out,
    [switch]$Json,
    [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'

# The peer id version alphabet. libtorrent, libtorrent-rakshasa and
# Transmission all encode one version component as one character: 0 to 9 then
# A to Z then a to z. Transmission calls the same table BASE62.
$VersionAlphabet = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz'

function ConvertTo-VersionChar {
    param([int]$Value)
    if ($Value -lt 0 -or $Value -ge $VersionAlphabet.Length) {
        throw "version component $Value has no single-character encoding"
    }
    return $VersionAlphabet[$Value]
}

function Get-VersionParts {
    param([string]$Text)
    $parts = $Text -split '\.'
    if ($parts.Count -lt 3) {
        throw "version '$Text' does not carry three components"
    }
    return @([int]$parts[0], [int]$parts[1], [int]$parts[2])
}

function New-LibtorrentKey {
    # libtorrent v2.0.11 src/http_tracker_connection.cpp:138 writes "&key=%08X",
    # so the value is a 32 bit integer in upper case hex, zero padded to eight,
    # and a leading zero is ordinary rather than impossible.
    $bytes = New-Object byte[] 4
    [System.Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    $value = [System.BitConverter]::ToUInt32($bytes, 0)
    return $value.ToString('X8')
}

function New-TransmissionPeerId {
    # Transmission 4.1.3 libtransmission/session.cc:196-206. Eleven characters
    # drawn from the pool, then one checksum character chosen so the whole
    # suffix sums to a multiple of the base.
    param([string]$Prefix)
    $pool = '0123456789abcdefghijklmnopqrstuvwxyz'
    $base = $pool.Length
    $suffixLength = 20 - $Prefix.Length
    $bytes = New-Object byte[] ($suffixLength - 1)
    [System.Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    $sb = New-Object System.Text.StringBuilder
    $total = 0
    foreach ($b in $bytes) {
        $v = [int]$b % $base
        $total += $v
        [void]$sb.Append($pool[$v])
    }
    $check = if (($total % $base) -ne 0) { $base - ($total % $base) } else { 0 }
    [void]$sb.Append($pool[$check])
    return $Prefix + $sb.ToString()
}

function Get-RepoFile {
    param([string]$Repo, [string]$Path, [string]$Ref)
    $raw = & gh api "repos/$Repo/contents/$Path`?ref=$Ref" --jq '.content' 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $raw) { return $null }
    $joined = ($raw -join '') -replace '\s', ''
    try {
        return [System.Text.Encoding]::UTF8.GetString([System.Convert]::FromBase64String($joined))
    } catch {
        return $null
    }
}

# ---------------------------------------------------------------------------
# Tags, and picking the newest of a kind
# ---------------------------------------------------------------------------

# One tag, parsed. `Rank` orders a stable release above the prereleases that
# led to it, which is what makes "5.1.0 is newer than 5.1.0rc1" true here.
function ConvertTo-TagInfo {
    param([string]$Name, [string]$Pattern)
    $m = [regex]::Match($Name, $Pattern)
    if (-not $m.Success) { return $null }
    $pre = if ($m.Groups['pre'].Success) { $m.Groups['pre'].Value.ToLowerInvariant() } else { '' }
    $num = if ($m.Groups['num'].Success -and $m.Groups['num'].Value) { [int]$m.Groups['num'].Value } else { 0 }
    # beta before rc before the release itself.
    $rank = switch ($pre) {
        ''      { 3 }
        'rc'    { 2 }
        'beta'  { 1 }
        default { 0 }
    }
    if ($rank -eq 0) { return $null }
    return [pscustomobject]@{
        Name       = $Name
        Major      = [int]$m.Groups['maj'].Value
        Minor      = [int]$m.Groups['min'].Value
        Patch      = [int]$m.Groups['pat'].Value
        Kind       = if ($pre) { 'beta' } else { 'stable' }
        Rank       = $rank
        PreNumber  = $num
    }
}

function Get-TagList {
    param([string]$Repo, [string]$Pattern)
    $names = & gh api "repos/$Repo/tags?per_page=100" --paginate --jq '.[].name' 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $names) { return $null }
    $tags = @()
    foreach ($name in $names) {
        $info = ConvertTo-TagInfo -Name $name -Pattern $Pattern
        if ($info) { $tags += $info }
    }
    return $tags
}

# Sorting is by the parsed version and never by the order the API returned.
# GitHub's tag order is not documented to be either chronological or semantic.
function Select-NewestTag {
    param([object[]]$Tags, [string]$Kind)
    $sorted = $Tags | Sort-Object Major, Minor, Patch, Rank, PreNumber
    if ($sorted.Count -eq 0) { return $null }
    $newest = $sorted[-1]
    if ($Kind -eq 'stable') {
        $stable = @($sorted | Where-Object { $_.Kind -eq 'stable' })
        if ($stable.Count -eq 0) { return $null }
        return $stable[-1]
    }
    # A prerelease is only offered when it is ahead of every stable release.
    # One behind is a tag nobody would run, and imitating it would describe a
    # client that no longer exists in the wild.
    if ($newest.Kind -ne 'beta') { return $null }
    return $newest
}

# ---------------------------------------------------------------------------
# The gate every profile passes before it is written
# ---------------------------------------------------------------------------

# Structural invariants, checked against the finished object rather than
# against the code that built it. A profile that fails one of these is not
# written, whatever the guards said, because a guard proves the upstream
# construction is unchanged and this proves what we built from it is a client.
function Test-Profile {
    param([hashtable]$Profile, [string]$Code)
    $failures = @()
    $prefix = $Profile.peer_id.prefix

    if ($prefix.Length -ne 8) { $failures += "peer id prefix '$prefix' is $($prefix.Length) bytes, not 8" }
    if (-not $prefix.StartsWith('-') -or -not $prefix.EndsWith('-')) {
        $failures += "peer id prefix '$prefix' is not Azureus style"
    }
    if ($prefix.Length -ge 3 -and $prefix.Substring(1, 2) -cne $Code) {
        $failures += "peer id prefix '$prefix' does not carry the client code '$Code'"
    }
    foreach ($ch in $prefix.Substring(3, [Math]::Max(0, $prefix.Length - 4)).ToCharArray()) {
        if ($VersionAlphabet.IndexOf($ch) -lt 0) {
            $failures += "peer id prefix '$prefix' carries '$ch', which is not in the version alphabet"
        }
    }
    if ([string]::IsNullOrWhiteSpace($Profile.version)) { $failures += 'no version' }
    if ([string]::IsNullOrWhiteSpace($Profile.tracker_http.user_agent)) { $failures += 'no user agent' }
    if ($Profile.tracker_http.user_agent -notmatch [regex]::Escape($Profile.version)) {
        $failures += "user agent '$($Profile.tracker_http.user_agent)' does not carry the version '$($Profile.version)'"
    }
    if ($Profile.derived_from.files.Count -eq 0) { $failures += 'nothing records which files it was derived from' }
    foreach ($key in $Profile.Keys) {
        if ($Profile[$key] -is [string] -and $Profile[$key] -eq 'unknown') {
            $failures += "field '$key' is the string 'unknown'"
        }
    }
    if ($Profile.peer_id.Contains('sample')) {
        $sample = $Profile.peer_id.sample
        if ($sample.Length -ne 20) { $failures += "sample peer id is $($sample.Length) bytes, not 20" }
        if (-not $sample.StartsWith($prefix)) { $failures += 'sample peer id does not start with the prefix' }
    }
    return $failures
}

# ---------------------------------------------------------------------------

function Invoke-SelfTest {
    $failures = @()

    # The whole alphabet round trips, which is the property the ports lost.
    for ($i = 0; $i -lt $VersionAlphabet.Length; $i++) {
        if ((ConvertTo-VersionChar -Value $i) -ne $VersionAlphabet[$i]) {
            $failures += "alphabet index $i does not round trip"
        }
    }

    # Both ends of the table, and one past it.
    if ((ConvertTo-VersionChar 61) -ne 'z') { $failures += 'component 61 is not z' }
    $threw = $false
    try { [void](ConvertTo-VersionChar 62) } catch { $threw = $true }
    if (-not $threw) { $failures += 'component 62 did not throw, so a version can silently wrap' }

    # A component of ten or more is one character, not two. joal's qBittorrent
    # script concatenates decimal, so 3.3.13 becomes -qB33130- and the prefix
    # is nine bytes. The real one is -qB33D0-.
    $cases = @(
        @{ v = '5.2.3';  code = 'qB'; build = 0; want = '-qB5230-' },
        @{ v = '3.3.13'; code = 'qB'; build = 0; want = '-qB33D0-' },
        @{ v = '3.3.16'; code = 'qB'; build = 0; want = '-qB33G0-' },
        @{ v = '4.1.3';  code = 'TR'; build = 0; want = '-TR4130-' },
        @{ v = '3.0.0';  code = 'TR'; build = 0; want = '-TR3000-' }
    )
    foreach ($case in $cases) {
        $p = Get-VersionParts -Text $case.v
        $got = '-{0}{1}{2}{3}{4}-' -f $case.code,
            (ConvertTo-VersionChar $p[0]),
            (ConvertTo-VersionChar $p[1]),
            (ConvertTo-VersionChar $p[2]),
            (ConvertTo-VersionChar $case.build)
        if ($got -ne $case.want) {
            $failures += "$($case.code) $($case.v): got $got, want $($case.want)"
        }
        if ($got.Length -ne 8) {
            $failures += "$($case.code) $($case.v): prefix is $($got.Length) bytes, not 8"
        }
    }

    # A prerelease is not the release it precedes, and the two clients differ.
    # Transmission moves the seventh character; qBittorrent does not and moves
    # its User-Agent instead. CMakeLists.txt:144-163 and version.h.in are the
    # two sources, and both are asserted against upstream by the guards below.
    if ((New-TransmissionPrefix -Major 4 -Minor 1 -Patch 0 -Beta '5' -Dev $false) -ne '-TR410B-') {
        $failures += 'Transmission 4.1.0-beta.5 does not produce -TR410B-'
    }
    if ((New-TransmissionPrefix -Major 4 -Minor 1 -Patch 3 -Beta '' -Dev $false) -ne '-TR4130-') {
        $failures += 'Transmission 4.1.3 does not produce -TR4130-'
    }
    if ((New-TransmissionPrefix -Major 4 -Minor 0 -Patch 0 -Beta '' -Dev $true) -ne '-TR400Z-') {
        $failures += 'a Transmission dev build does not produce -TR400Z-'
    }
    if ((New-QbittorrentUserAgent -Major 5 -Minor 1 -Patch 0 -Build 0 -Status 'beta1') -ne 'qBittorrent/5.1.0beta1') {
        $failures += 'qBittorrent 5.1.0beta1 does not produce its documented User-Agent'
    }
    if ((New-QbittorrentUserAgent -Major 5 -Minor 2 -Patch 3 -Build 0 -Status '') -ne 'qBittorrent/5.2.3') {
        $failures += 'qBittorrent 5.2.3 does not produce its documented User-Agent'
    }
    # version.h.in:40-44 puts the build number in the string only when it is
    # not zero, which is the branch nothing has exercised in years.
    if ((New-QbittorrentUserAgent -Major 5 -Minor 2 -Patch 3 -Build 1 -Status '') -ne 'qBittorrent/5.2.3.1') {
        $failures += 'a qBittorrent build number does not reach the User-Agent'
    }

    # A key must be able to start with a zero. Every profile set read for T-234
    # guarantees it cannot, and libtorrent writes key=%08X.
    $sawLeadingZero = $false
    for ($i = 0; $i -lt 4096; $i++) {
        if ((New-LibtorrentKey).StartsWith('0')) { $sawLeadingZero = $true; break }
    }
    if (-not $sawLeadingZero) {
        $failures += "New-LibtorrentKey never produced a key with a leading zero in 4096 draws"
    }
    foreach ($i in 1..64) {
        $k = New-LibtorrentKey
        if ($k.Length -ne 8) { $failures += "key '$k' is not 8 characters"; break }
        if ($k -cmatch '[a-f]') { $failures += "key '$k' is not upper case"; break }
    }

    # Transmission's checksum digit makes the suffix sum a multiple of the base.
    foreach ($i in 1..64) {
        $id = New-TransmissionPeerId -Prefix '-TR4130-'
        if ($id.Length -ne 20) { $failures += "peer id '$id' is not 20 bytes"; break }
        $pool = '0123456789abcdefghijklmnopqrstuvwxyz'
        $total = 0
        foreach ($c in $id.Substring(8).ToCharArray()) { $total += $pool.IndexOf($c) }
        if (($total % 36) -ne 0) {
            $failures += "peer id '$id' suffix sums to $($total % 36) mod 36, not 0"
            break
        }
    }

    # Tag ordering, over a list deliberately shuffled out of order. The stable
    # release outranks the prereleases that led to it.
    $names = @('release-5.1.0beta1', 'release-5.2.3', 'release-4.6.7',
               'release-5.1.0', 'release-5.1.0rc1', 'release-5.2.0')
    $tags = @()
    foreach ($n in $names) {
        $tags += ConvertTo-TagInfo -Name $n -Pattern $QbitTagPattern
    }
    if ((Select-NewestTag -Tags $tags -Kind 'stable').Name -ne 'release-5.2.3') {
        $failures += 'newest stable is not release-5.2.3'
    }
    if ($null -ne (Select-NewestTag -Tags $tags -Kind 'beta')) {
        $failures += 'a beta behind the newest stable was offered'
    }
    $tags += ConvertTo-TagInfo -Name 'release-5.3.0beta1' -Pattern $QbitTagPattern
    if ((Select-NewestTag -Tags $tags -Kind 'beta').Name -ne 'release-5.3.0beta1') {
        $failures += 'a beta ahead of the newest stable was not offered'
    }
    if ((Select-NewestTag -Tags $tags -Kind 'stable').Name -ne 'release-5.2.3') {
        $failures += 'a beta changed which stable is newest'
    }
    if ($null -ne (ConvertTo-TagInfo -Name 'release-5.2' -Pattern $QbitTagPattern)) {
        $failures += 'a two-component tag was accepted'
    }
    if ((ConvertTo-TagInfo -Name '4.1.0-beta.5' -Pattern $TransTagPattern).PreNumber -ne 5) {
        $failures += 'a Transmission beta number was not read'
    }

    # The gate refuses what the guards would not have caught: a profile whose
    # prefix is the wrong length, carries the wrong code, or whose User-Agent
    # does not carry the version it claims.
    $bad = @(
        @{ why = 'short prefix'; p = (New-TestProfile -Prefix '-qB523-' -Version '5.2.3' -Agent 'qBittorrent/5.2.3') },
        @{ why = 'wrong code';   p = (New-TestProfile -Prefix '-TR5230-' -Version '5.2.3' -Agent 'qBittorrent/5.2.3') },
        @{ why = 'agent disagrees with version'; p = (New-TestProfile -Prefix '-qB5230-' -Version '5.2.3' -Agent 'qBittorrent/5.2.2') }
    )
    foreach ($case in $bad) {
        if ((Test-Profile -Profile $case.p -Code 'qB').Count -eq 0) {
            $failures += "the profile gate accepted a profile with a $($case.why)"
        }
    }
    $good = New-TestProfile -Prefix '-qB5230-' -Version '5.2.3' -Agent 'qBittorrent/5.2.3'
    if ((Test-Profile -Profile $good -Code 'qB').Count -ne 0) {
        $failures += 'the profile gate refused a correct profile'
    }

    if ($failures.Count -gt 0) {
        foreach ($f in $failures) { Write-Host "  fail: $f" }
        Write-Host "make-client-profile: $($failures.Count) self-test failure(s)"
        return 1
    }
    Write-Host "make-client-profile: self-test passes"
    return 0
}

function New-TestProfile {
    param([string]$Prefix, [string]$Version, [string]$Agent)
    return @{
        version      = $Version
        peer_id      = @{ prefix = $Prefix }
        tracker_http = @{ user_agent = $Agent }
        derived_from = @{ files = @('one') }
    }
}

# ---------------------------------------------------------------------------
# The two constructions, each reproduced from the file that defines it
# ---------------------------------------------------------------------------

# Transmission CMakeLists.txt:144-163. Three base 62 characters, then one that
# says which kind of build this is.
function New-TransmissionPrefix {
    param([int]$Major, [int]$Minor, [int]$Patch, [string]$Beta, [bool]$Dev)
    $kind = if ($Dev) { 'Z' } elseif ($Beta -ne '') { 'B' } else { '0' }
    return '-TR{0}{1}{2}{3}-' -f (ConvertTo-VersionChar $Major),
        (ConvertTo-VersionChar $Minor), (ConvertTo-VersionChar $Patch), $kind
}

# qBittorrent src/base/version.h.in:40-44 then sessionimpl.cpp:128. The build
# number reaches the string only when it is not zero, and the status is
# appended with no separator.
function New-QbittorrentUserAgent {
    param([int]$Major, [int]$Minor, [int]$Patch, [int]$Build, [string]$Status)
    $core = if ($Build -ne 0) { "$Major.$Minor.$Patch.$Build" } else { "$Major.$Minor.$Patch" }
    return "qBittorrent/$core$Status"
}

# The numeric triple of a version string, so `4.1.0-beta.5` and
# `5.1.0beta1` compare equal to the constants at those tags, which carry the
# release the prerelease is heading for and never the prerelease label.
function Get-NumericVersion {
    param([string]$Text)
    $m = [regex]::Match($Text, '^(\d+)\.(\d+)\.(\d+)')
    if (-not $m.Success) { return $null }
    return '{0}.{1}.{2}' -f $m.Groups[1].Value, $m.Groups[2].Value, $m.Groups[3].Value
}

$QbitTagPattern  = '^release-(?<maj>\d+)\.(?<min>\d+)\.(?<pat>\d+)(?<pre>beta|rc)?(?<num>\d*)$'
$TransTagPattern = '^(?<maj>\d+)\.(?<min>\d+)\.(?<pat>\d+)(?:-(?<pre>beta|rc)\.(?<num>\d+))?$'

# ---------------------------------------------------------------------------

if ($SelfTest) { exit (Invoke-SelfTest) }

if ($Version -and $Latest) {
    Write-Host "make-client-profile: pass -Version or -Latest, not both"
    exit 2
}

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    Write-Host "make-client-profile: gh is not on PATH, so no source can be read"
    exit 2
}

$guardFailures = @()
$derived = $null
$repo = if ($Client -eq 'qbittorrent') { 'qbittorrent/qBittorrent' } else { 'transmission/transmission' }
$pattern = if ($Client -eq 'qbittorrent') { $QbitTagPattern } else { $TransTagPattern }

if ($Latest) {
    $tags = Get-TagList -Repo $repo -Pattern $pattern
    if ($null -eq $tags) {
        Write-Host "make-client-profile: cannot read the tag list for $repo"
        exit 2
    }
    $picked = Select-NewestTag -Tags $tags -Kind $Latest
    if ($null -eq $picked) {
        if ($Latest -eq 'beta') {
            Write-Host "make-client-profile: $repo has no prerelease ahead of its newest stable release"
            Write-Host "  nothing was written. A prerelease behind a stable release is not a client to imitate."
            exit 1
        }
        Write-Host "make-client-profile: no tag in $repo matches the release pattern"
        exit 1
    }
    $Version = '{0}.{1}.{2}' -f $picked.Major, $picked.Minor, $picked.Patch
    $ref = $picked.Name
    if (-not $Json) { Write-Host "make-client-profile: latest $Latest is $ref" }
}

if ($Client -eq 'qbittorrent') {
    if (-not $Version) { $Version = '5.2.3' }
    if (-not $ref) { $ref = "release-$Version" }

    $versionFile = Get-RepoFile -Repo $repo -Path 'src/base/version.h.in' -Ref $ref
    if ($null -eq $versionFile) {
        Write-Host "make-client-profile: cannot read src/base/version.h.in at $ref"
        exit 2
    }

    # Read the version from the client rather than trusting the tag. joal's
    # script extracts these and then ignores them; here a disagreement is a
    # guard failure, because the tag and the constants naming different
    # versions is exactly the case a profile must not paper over.
    $constants = @{}
    foreach ($name in 'QBT_VERSION_MAJOR', 'QBT_VERSION_MINOR', 'QBT_VERSION_BUGFIX', 'QBT_VERSION_BUILD') {
        $m = [regex]::Match($versionFile, "(?m)^#define\s+$name\s+(\d+)")
        if (-not $m.Success) { $guardFailures += "$name is not in src/base/version.h.in at $ref" }
        else { $constants[$name] = [int]$m.Groups[1].Value }
    }
    # The status is what makes a prerelease a prerelease, and it is empty for a
    # stable release rather than absent.
    $statusMatch = [regex]::Match($versionFile, '(?m)^#define\s+QBT_VERSION_STATUS\s+"([^"]*)"')
    if (-not $statusMatch.Success) {
        $guardFailures += "QBT_VERSION_STATUS is not in src/base/version.h.in at $ref"
    }
    $status = if ($statusMatch.Success) { $statusMatch.Groups[1].Value } else { '' }

    # The construction itself, asserted rather than assumed. If the build
    # number stops being conditional, or the status stops being appended, the
    # User-Agent this script builds is no longer the one the client sends.
    if ($versionFile -notmatch '#if\s*\(QBT_VERSION_BUILD\s*!=\s*0\)') {
        $guardFailures += 'version.h.in no longer makes the build number conditional'
    }
    if ($versionFile -notmatch 'PROJECT_VERSION\s+QBT_STRINGIFY\([^)]*\)\s+QBT_VERSION_STATUS') {
        $guardFailures += 'version.h.in no longer appends QBT_VERSION_STATUS to PROJECT_VERSION'
    }

    if ($guardFailures.Count -eq 0) {
        $declared = '{0}.{1}.{2}' -f $constants['QBT_VERSION_MAJOR'], $constants['QBT_VERSION_MINOR'], $constants['QBT_VERSION_BUGFIX']
        $asked = Get-NumericVersion -Text $Version
        if ($null -eq $asked) {
            $guardFailures += "'$Version' does not begin with three numeric components"
        } elseif ($declared -ne $asked) {
            $guardFailures += "tag $ref carries version constants $declared"
        }
    }

    # qBittorrent moved the session implementation between 4.x and 5.x. Read
    # whichever exists and record which one answered.
    $sessionPath = $null
    $sessionText = $null
    foreach ($candidate in 'src/base/bittorrent/sessionimpl.cpp', 'src/base/bittorrent/session.cpp') {
        $text = Get-RepoFile -Repo $repo -Path $candidate -Ref $ref
        if ($null -ne $text) { $sessionPath = $candidate; $sessionText = $text; break }
    }
    if ($null -eq $sessionText) {
        $guardFailures += "no session implementation at either known path in $ref"
    } else {
        # The guards that matter, and each one names a value this script uses.
        $codeMatch = [regex]::Match($sessionText, 'PEER_ID\[\]\s*=\s*"([A-Za-z]{2})"')
        if (-not $codeMatch.Success) {
            $guardFailures += "$sessionPath no longer declares a two-letter PEER_ID"
        }
        if ($sessionText -notmatch 'generate_fingerprint\(\s*PEER_ID\s*,\s*QBT_VERSION_MAJOR\s*,\s*QBT_VERSION_MINOR\s*,\s*QBT_VERSION_BUGFIX\s*,\s*QBT_VERSION_BUILD\s*\)') {
            $guardFailures += "$sessionPath no longer builds the fingerprint from the four version constants"
        }
        if ($sessionText -notmatch 'USER_AGENT\s*=\s*QStringLiteral\("qBittorrent/"\s*QBT_VERSION_2\)') {
            $guardFailures += "$sessionPath no longer builds the User-Agent from qBittorrent/ and QBT_VERSION_2"
        }
    }

    if ($guardFailures.Count -eq 0) {
        $code = $codeMatch.Groups[1].Value
        $prefix = '-{0}{1}{2}{3}{4}-' -f $code,
            (ConvertTo-VersionChar $constants['QBT_VERSION_MAJOR']),
            (ConvertTo-VersionChar $constants['QBT_VERSION_MINOR']),
            (ConvertTo-VersionChar $constants['QBT_VERSION_BUGFIX']),
            (ConvertTo-VersionChar $constants['QBT_VERSION_BUILD'])
        $agent = New-QbittorrentUserAgent -Major $constants['QBT_VERSION_MAJOR'] `
            -Minor $constants['QBT_VERSION_MINOR'] -Patch $constants['QBT_VERSION_BUGFIX'] `
            -Build $constants['QBT_VERSION_BUILD'] -Status $status
        $fullVersion = $agent.Substring('qBittorrent/'.Length)
        $derived = [ordered]@{
            name    = "qbittorrent-$fullVersion"
            client  = 'qBittorrent'
            version = $fullVersion
            release = if ($status) { 'prerelease' } else { 'stable' }
            derived_from = [ordered]@{
                repo    = $repo
                ref     = $ref
                files   = @('src/base/version.h.in', $sessionPath)
                engine  = 'libtorrent'
            }
            peer_id = [ordered]@{
                style   = 'azureus'
                prefix  = $prefix
                suffix  = [ordered]@{
                    kind    = 'charset'
                    charset = 'A-Za-z0-9_~()!.*-'
                    length  = 12
                }
                refresh = 'never'
                # The status is not in the peer id and is in the User-Agent, so
                # a prerelease is indistinguishable from the release it
                # precedes to anything reading only the peer id.
                prerelease_visible = $false
            }
            tracker_http = [ordered]@{
                user_agent  = $agent
                headers     = @('User-Agent', 'Accept-Encoding: gzip', 'Connection: close')
                query_order = @('info_hash', 'peer_id', 'port', 'uploaded', 'downloaded',
                                'left', 'corrupt', 'key', 'event', 'numwant', 'compact',
                                'no_peer_id', 'supportcrypto', 'redundant')
                key         = [ordered]@{
                    width          = 8
                    case           = 'upper'
                    leading_zero   = $true
                    refresh        = 'per_torrent'
                    source         = 'libtorrent src/http_tracker_connection.cpp, key=%08X'
                }
                numwant         = 200
                numwant_on_stop = 0
                encoder         = [ordered]@{
                    unreserved = 'A-Za-z0-9_~()!.*-'
                    hex_case   = 'lower'
                }
            }
            peer_wire = [ordered]@{
                note = 'not derived by this run: reserved bytes, the extension handshake and the message order after the handshake are read from a live client, not from a tag'
            }
        }
        $gateFailures = Test-Profile -Profile @{
            version = $derived.version
            peer_id = @{ prefix = $derived.peer_id.prefix }
            tracker_http = @{ user_agent = $derived.tracker_http.user_agent }
            derived_from = @{ files = $derived.derived_from.files }
        } -Code $code
        foreach ($f in $gateFailures) { $guardFailures += "profile gate: $f" }
    }
}
elseif ($Client -eq 'transmission') {
    if (-not $Version) { $Version = '4.1.3' }
    if (-not $ref) { $ref = $Version }

    $cmake = Get-RepoFile -Repo $repo -Path 'CMakeLists.txt' -Ref $ref
    if ($null -eq $cmake) {
        Write-Host "make-client-profile: cannot read CMakeLists.txt at $ref"
        exit 2
    }

    $constants = @{}
    foreach ($name in 'TR_VERSION_MAJOR', 'TR_VERSION_MINOR', 'TR_VERSION_PATCH') {
        $m = [regex]::Match($cmake, "set\($name\s+`"(\d+)`"\)")
        if (-not $m.Success) { $guardFailures += "$name is not in CMakeLists.txt at $ref" }
        else { $constants[$name] = [int]$m.Groups[1].Value }
    }
    $betaMatch = [regex]::Match($cmake, 'set\(TR_VERSION_BETA_NUMBER\s+"(\d*)"\)')
    if (-not $betaMatch.Success) {
        $guardFailures += 'TR_VERSION_BETA_NUMBER is not in CMakeLists.txt, so a beta cannot be told from a release'
    }
    $beta = if ($betaMatch.Success) { $betaMatch.Groups[1].Value } else { '' }
    $devMatch = [regex]::Match($cmake, 'set\(TR_VERSION_DEV\s+(TRUE|FALSE)\)')
    if (-not $devMatch.Success) {
        $guardFailures += 'TR_VERSION_DEV is not in CMakeLists.txt'
    }
    $dev = $devMatch.Success -and $devMatch.Groups[1].Value -eq 'TRUE'

    if ($cmake -notmatch 'set\(TR_SEMVER\s+"\$\{TR_VERSION_MAJOR\}\.\$\{TR_VERSION_MINOR\}\.\$\{TR_VERSION_PATCH\}"\)') {
        $guardFailures += 'TR_SEMVER is no longer major.minor.patch, so the User-Agent is no longer derivable from these three'
    }
    if ($cmake -notmatch 'set\(TR_USER_AGENT_PREFIX\s+"\$\{TR_SEMVER\}"\)') {
        $guardFailures += 'TR_USER_AGENT_PREFIX is no longer TR_SEMVER'
    }
    # The peer id prefix is built here rather than in the C++, and the seventh
    # character is what says stable, beta or dev. Reproducing it without
    # asserting it is how a beta gets a stable release's peer id.
    if ($cmake -notmatch 'set\(BASE62\s+"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"\)') {
        $guardFailures += 'CMakeLists.txt no longer carries the BASE62 table the prefix is built from'
    }
    if ($cmake -notmatch '(?s)if\(TR_VERSION_DEV\).{0,120}TR_PEER_ID_PREFIX\s+"Z".{0,200}TR_PEER_ID_PREFIX\s+"B".{0,200}TR_PEER_ID_PREFIX\s+"0"') {
        $guardFailures += 'CMakeLists.txt no longer chooses the seventh character as Z, B or 0'
    }

    if ($guardFailures.Count -eq 0) {
        $declared = '{0}.{1}.{2}' -f $constants['TR_VERSION_MAJOR'], $constants['TR_VERSION_MINOR'], $constants['TR_VERSION_PATCH']
        $asked = Get-NumericVersion -Text $Version
        if ($null -eq $asked) {
            $guardFailures += "'$Version' does not begin with three numeric components"
        } elseif ($declared -ne $asked) {
            $guardFailures += "tag $ref carries version constants $declared"
        }
    }

    $session = Get-RepoFile -Repo $repo -Path 'libtransmission/session.cc' -Ref $ref
    if ($null -eq $session) {
        $guardFailures += 'libtransmission/session.cc is not at its known path'
    } else {
        # The checksum is the guard. Three of the four emulators read for T-234
        # get this wrong, and a tracker that validates it sees the difference.
        if ($session -notmatch '0123456789abcdefghijklmnopqrstuvwxyz') {
            $guardFailures += 'session.cc no longer carries the base 36 peer id pool'
        }
        if ($session -notmatch 'total\s*%\s*std::size\(Pool\)') {
            $guardFailures += 'session.cc no longer computes the peer id checksum the way this script reproduces'
        }
        if ($session -notmatch 'PEERID_PREFIX') {
            $guardFailures += 'session.cc no longer takes its prefix from PEERID_PREFIX, which CMakeLists.txt builds'
        }
    }

    if ($guardFailures.Count -eq 0) {
        $prefix = New-TransmissionPrefix -Major $constants['TR_VERSION_MAJOR'] `
            -Minor $constants['TR_VERSION_MINOR'] -Patch $constants['TR_VERSION_PATCH'] `
            -Beta $beta -Dev $dev
        $semver = $declared
        if ($dev -or $beta -ne '') {
            $semver += '-'
            if ($beta -ne '') { $semver += "beta.$beta" }
            if ($dev -and $beta -ne '') { $semver += '.' }
            if ($dev) { $semver += 'dev' }
        }
        $derived = [ordered]@{
            name    = "transmission-$semver"
            client  = 'Transmission'
            version = $semver
            release = if ($dev) { 'dev' } elseif ($beta -ne '') { 'prerelease' } else { 'stable' }
            derived_from = [ordered]@{
                repo   = $repo
                ref    = $ref
                files  = @('CMakeLists.txt', 'libtransmission/session.cc')
                engine = 'libtransmission'
            }
            peer_id = [ordered]@{
                style   = 'azureus'
                prefix  = $prefix
                suffix  = [ordered]@{
                    kind      = 'pool_with_checksum'
                    pool      = '0123456789abcdefghijklmnopqrstuvwxyz'
                    base      = 36
                    length    = 12
                    checksum  = 'the whole suffix sums to a multiple of the base'
                }
                refresh = 'per_session'
                # Unlike qBittorrent, the seventh character says which kind of
                # build this is, so a peer sees the difference.
                prerelease_visible = $true
                sample  = (New-TransmissionPeerId -Prefix $prefix)
            }
            tracker_http = [ordered]@{
                user_agent  = "Transmission/$semver"
                headers     = @('User-Agent', 'Accept: */*', 'Accept-Encoding: deflate, gzip')
                query_order = @('info_hash', 'peer_id', 'port', 'uploaded', 'downloaded',
                                'left', 'numwant', 'key', 'compact', 'supportcrypto',
                                'event', 'ipv6')
                key         = [ordered]@{
                    width        = 'variable'
                    case         = 'lower'
                    leading_zero = $false
                    refresh      = 'never'
                    source       = 'libtransmission announce_key, an integer rendered as hex'
                }
                numwant         = 80
                numwant_on_stop = 0
                encoder         = [ordered]@{
                    unreserved = 'A-Za-z0-9-'
                    hex_case   = 'lower'
                }
            }
            peer_wire = [ordered]@{
                note = 'not derived by this run: reserved bytes, the extension handshake and the message order after the handshake are read from a live client, not from a tag'
            }
        }
        $gateFailures = Test-Profile -Profile @{
            version = $derived.version
            peer_id = @{ prefix = $derived.peer_id.prefix; sample = $derived.peer_id.sample }
            tracker_http = @{ user_agent = $derived.tracker_http.user_agent }
            derived_from = @{ files = $derived.derived_from.files }
        } -Code 'TR'
        foreach ($f in $gateFailures) { $guardFailures += "profile gate: $f" }
    }
}

if ($guardFailures.Count -gt 0) {
    if ($Json) {
        [ordered]@{ ok = $false; client = $Client; version = $Version; ref = $ref; guard_failures = $guardFailures } |
            ConvertTo-Json -Depth 6
    } else {
        Write-Host "make-client-profile: $Client $Version could not be derived"
        foreach ($f in $guardFailures) { Write-Host "  guard: $f" }
        Write-Host "  nothing was written. The client changed how it builds its identity,"
        Write-Host "  or the tag does not exist. Read the files named above before editing this script."
    }
    exit 1
}

$text = ($derived | ConvertTo-Json -Depth 8)

if ($Out) {
    $dir = Split-Path -Parent $Out
    if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
    [System.IO.File]::WriteAllText($Out, $text + "`n", (New-Object System.Text.UTF8Encoding $false))
    if (-not $Json) { Write-Host "make-client-profile: wrote $Out" }
}

if ($Json -or -not $Out) { $text }

exit 0
