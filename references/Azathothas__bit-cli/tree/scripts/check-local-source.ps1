# Drive a `file:` source: bytes already on the disk, named as a web seed.
#
# This is the acceptance for `TODO/multi-source.md` T-133 layers 1 and 2, and
# for the operator's Scenario 2. Nothing here touches the network and no server
# runs: every byte comes off the local filesystem.
#
# It builds the three-torrent fixture (one bit-identical `file.blob` under
# three info hashes and three piece lengths) and runs eight cases:
#
#   exact          torrent C's file.blob from the CDN copy, a name and a path
#                  with no relation to the torrent's. Completes, hash matches.
#   auto           torrent C again, from the directory holding the payload,
#                  with the BEP 19 composition appending name and path.
#   shared_a       torrent A's file.blob from the copy torrent C just finished.
#   shared_b       torrent B's file.blob from the same copy. Its piece length
#                  is 512 KiB against C's 2 MiB, so the boundaries do not line
#                  up and the bytes still land.
#   wrong_bytes    a file of the right length holding the wrong bytes. The
#                  source is refused and the report names the path and the
#                  piece.
#   missing        a path that is not there. The source is refused and the
#                  report names it.
#   one_invocation the same three torrents in one command instead of three.
#                  C reads the CDN copy, A and B read what C wrote, and all
#                  three land byte for byte.
#   equivalence    what the metadata alone proves: nothing across the three
#                  piece lengths, and the shared file byte for byte across a
#                  pair built to line up.
#
# The shared cases are the point: one 64 MiB payload fetched once and written
# into three output directories under three different info hashes, all four
# copies hashing equal.
#
# `one_invocation` is the same result in one command, which needs two things
# the earlier cases do not. A binding has to name one torrent, because
# `file.blob` is index 0 in A and C and index 1 in B, so
# `--web-seed-for '<INFOHASH>:file:N=URL'` says which. And `-j 1` has to start
# the sources in the order they were given, because A and B read the file C
# writes. Every other file in each torrent is put in place first, so the only
# bytes the run fetches are the shared file's and "fetched once" is a number
# rather than a claim: exactly one source reads the CDN copy.
#
# Usage:
#   pwsh scripts/check-local-source.ps1
#   pwsh scripts/check-local-source.ps1 -BlobSizeMiB 16
#
# Exits 0 when every case behaves as described, 1 when one does not, and 2 when
# the check could not run. The record goes to bench/local-source-<timestamp>.json.
#
# See TODO/multi-source.md, T-133.

[CmdletBinding()]
param(
    [int]$BlobSizeMiB = 64,
    [string]$Root = ".tmp/local-source",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [int]$TimeoutSeconds = 300,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-local-source: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

function Format-Size([double]$bytes) {
    $units = @("B", "KiB", "MiB", "GiB", "TiB")
    $index = 0
    while ($bytes -ge 1024 -and $index -lt $units.Count - 1) { $bytes /= 1024; $index++ }
    "{0:N2} {1}" -f $bytes, $units[$index]
}

# A path as a file: URL. Separators and the drive colon stay; everything else
# outside the unreserved set is percent-encoded, which is what
# crates/bit-cli-core/src/webseed/local.rs does on the other side.
function ConvertTo-FileUrl([string]$path) {
    $text = (Resolve-Path $path).Path -replace '\\', '/'
    $out = [System.Text.StringBuilder]::new("file://")
    if (-not $text.StartsWith("/")) { [void]$out.Append("/") }
    foreach ($byte in [System.Text.Encoding]::UTF8.GetBytes($text)) {
        $char = [char]$byte
        if (($char -ge 'A' -and $char -le 'Z') -or ($char -ge 'a' -and $char -le 'z') -or
            ($char -ge '0' -and $char -le '9') -or "-_.~/:".Contains($char)) {
            [void]$out.Append($char)
        }
        else {
            [void]$out.AppendFormat("%{0:X2}", $byte)
        }
    }
    $out.ToString()
}

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --workspace --bins --examples"
}
if ($BlobSizeMiB -lt 1) { Exit-With 2 "-BlobSizeMiB has to be at least 1." }

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# ---------------------------------------------------------------------------
# The fixture
# ---------------------------------------------------------------------------

Write-Step "building the three-torrent fixture, $BlobSizeMiB MiB shared file"
$fixture = Join-Path $Root "fixture"
& (Join-Path $PSScriptRoot "make-scenario-fixture.ps1") `
    -BlobSizeMiB $BlobSizeMiB -OtherSizeMiB 4 -Partial 0 -Root $fixture -Profile $Profile |
    Out-Null
if ($LASTEXITCODE -ne 0) { Exit-With 2 "make-scenario-fixture.ps1 exited $LASTEXITCODE" }

$cdnCopy = Join-Path $fixture "cdn/a3f1b2c4-signed-blob.dat"
if (-not (Test-Path $cdnCopy)) { Exit-With 2 "the fixture has no CDN copy at $cdnCopy" }
$expected = (Get-FileHash -Algorithm SHA256 $cdnCopy).Hash.ToLower()
$blobBytes = (Get-Item $cdnCopy).Length

# A file of the right length holding the wrong bytes, which is what a length
# check misses and the per-piece check is for.
$wrong = Join-Path $Root "wrong-bytes.dat"
$block = [byte[]]::new(1024 * 1024)
[int64]$state = 20260820
for ($i = 0; $i -lt $block.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $block[$i] = [byte](($state -shr 16) -band 0xFF)
}
$stream = [System.IO.File]::Create($wrong)
try {
    [int64]$written = 0
    while ($written -lt $blobBytes) {
        $take = [Math]::Min([int64]$block.Length, $blobBytes - $written)
        $stream.Write($block, 0, [int]$take)
        $written += $take
    }
}
finally { $stream.Dispose() }

# ---------------------------------------------------------------------------
# Cases
# ---------------------------------------------------------------------------

$commands = [System.Collections.ArrayList]::new()
$cases = [System.Collections.ArrayList]::new()
$failures = [System.Collections.ArrayList]::new()

function Invoke-Download([string]$label, [string]$torrent, [string[]]$extra) {
    $outDir = Join-Path $Root "out-$label"
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $stdout = Join-Path $Root "$label.json"
    $stderr = Join-Path $Root "$label.err"
    $arguments = @(
        "download", $torrent,
        "--dir", $outDir,
        "--web-seed-only",
        "--no-torrent-web-seed",
        "--port", "0",
        "--json"
    ) + $extra
    [void]$commands.Add("bit-cli $($arguments -join ' ')")
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $bitCli -WorkingDirectory $repo -NoNewWindow -PassThru `
        -ArgumentList $arguments -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $finished = $process.WaitForExit($TimeoutSeconds * 1000)
    $clock.Stop()
    if (-not $finished) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        return [pscustomobject]@{ exit_code = 124; elapsed_ms = $clock.ElapsedMilliseconds; report = $null; out_dir = $outDir; stderr = $stderr }
    }
    $report = $null
    try { $report = Get-Content $stdout -Raw | ConvertFrom-Json } catch { }
    [pscustomobject]@{
        exit_code  = $process.ExitCode
        elapsed_ms = $clock.ElapsedMilliseconds
        report     = $report
        out_dir    = $outDir
        stderr     = $stderr
    }
}

function Add-Case([string]$name, $run, [string]$landed, [string]$expectation, [bool]$ok, [string]$detail) {
    $hash = $null
    if ($landed -and (Test-Path $landed)) {
        $hash = (Get-FileHash -Algorithm SHA256 $landed).Hash.ToLower()
    }
    $source = $null
    if ($run.report -and $run.report.torrents -and $run.report.torrents[0].sources) {
        $source = $run.report.torrents[0].sources[0]
    }
    [void]$cases.Add([ordered]@{
        case             = $name
        expectation      = $expectation
        exit_code        = $run.exit_code
        elapsed_ms       = $run.elapsed_ms
        downloaded_bytes = if ($run.report) { $run.report.downloaded.bytes } else { 0 }
        downloaded_human = if ($run.report) { $run.report.downloaded.human } else { "0 B" }
        landed_path      = $landed
        sha256           = $hash
        hash_matches     = ($hash -eq $expected)
        source_url       = if ($source) { $source.url } else { $null }
        source_state     = if ($source) { $source.state } else { $null }
        source_error     = if ($source) { $source.error } else { $null }
        ok               = $ok
        detail           = $detail
    })
    if (-not $ok) { [void]$failures.Add("$name : $detail") }
}

$torrentA = Join-Path $fixture "torrent_a.torrent"
$torrentB = Join-Path $fixture "torrent_b.torrent"
$torrentC = Join-Path $fixture "torrent_c.torrent"

# --- 1. exact: the CDN copy, renamed and in an unrelated directory ---------
Write-Step "exact: torrent C's file.blob from a local copy under an unrelated name"
$run = Invoke-Download "exact" $torrentC @(
    "--select-file", "0",
    "--web-seed-for", "file:0=$(ConvertTo-FileUrl $cdnCopy)",
    "--web-seed-mode", "exact"
)
$landed = Join-Path $run.out_dir "payload_c/a/b/c/file.blob"
$hash = if (Test-Path $landed) { (Get-FileHash -Algorithm SHA256 $landed).Hash.ToLower() } else { $null }
$ok = ($run.exit_code -eq 0) -and ($hash -eq $expected)
Add-Case "exact" $run $landed `
    "a file: source with exact composition completes a torrent with no network" `
    $ok "exit $($run.exit_code), hash $(if ($hash -eq $expected) { 'matches' } else { 'differs' })"

# The copy torrent C just finished is the source for the other two, which is
# the two-step form of Scenario 2.
$finishedCopy = $landed

# --- 2. auto: the payload directory, BEP 19 composition --------------------
Write-Step "auto: torrent C from the payload tree, name and path appended"
$run = Invoke-Download "auto" $torrentC @("--web-seed", (ConvertTo-FileUrl $fixture))
$landed = Join-Path $run.out_dir "payload_c/a/b/c/file.blob"
$hash = if (Test-Path $landed) { (Get-FileHash -Algorithm SHA256 $landed).Hash.ToLower() } else { $null }
$ok = ($run.exit_code -eq 0) -and ($hash -eq $expected)
Add-Case "auto" $run $landed `
    "a file: source with auto composition appends the torrent name and path, as BEP 19 does over HTTP" `
    $ok "exit $($run.exit_code), hash $(if ($hash -eq $expected) { 'matches' } else { 'differs' })"

# --- 3 and 4. the same bytes into two other torrents -----------------------
foreach ($case in @(
        @{ name = "shared_a"; torrent = $torrentA; index = 0; landed = "payload_a/deep/nested/dirs/file.blob"; pieces = "1 MiB" },
        @{ name = "shared_b"; torrent = $torrentB; index = 1; landed = "payload_b/media/file.blob"; pieces = "512 KiB" }
    )) {
    Write-Step "$($case.name): the copy torrent C finished, into a torrent with $($case.pieces) pieces"
    if (-not (Test-Path $finishedCopy)) {
        Add-Case $case.name ([pscustomobject]@{ exit_code = 2; elapsed_ms = 0; report = $null; out_dir = $Root; stderr = $null }) $null `
            "the shared file comes from the copy torrent C finished" $false `
            "torrent C left no copy to share, so this could not run"
        continue
    }
    $run = Invoke-Download $case.name $case.torrent @(
        "--select-file", "$($case.index)",
        "--web-seed-for", "file:$($case.index)=$(ConvertTo-FileUrl $finishedCopy)",
        "--web-seed-mode", "exact"
    )
    $landed = Join-Path $run.out_dir $case.landed
    $hash = if (Test-Path $landed) { (Get-FileHash -Algorithm SHA256 $landed).Hash.ToLower() } else { $null }
    $ok = ($run.exit_code -eq 0) -and ($hash -eq $expected)
    Add-Case $case.name $run $landed `
        "the same bytes serve a torrent with a different info hash and a $($case.pieces) piece length" `
        $ok "exit $($run.exit_code), hash $(if ($hash -eq $expected) { 'matches' } else { 'differs' })"
}

# --- 5. the right length, the wrong bytes ----------------------------------
Write-Step "wrong_bytes: a local file of the right length holding something else"
$run = Invoke-Download "wrong" $torrentC @(
    "--select-file", "0",
    "--web-seed-for", "file:0=$(ConvertTo-FileUrl $wrong)",
    "--web-seed-mode", "exact"
)
$said = (Get-Content $run.stderr -Raw -ErrorAction SilentlyContinue)
$named = $said -match 'wrong-bytes\.dat' -and $said -match 'hash'
$ok = ($run.exit_code -ne 0) -and $named
Add-Case "wrong_bytes" $run $null `
    "the per-piece check refuses it and the report names the path and the piece" `
    $ok "exit $($run.exit_code), reason $(if ($named) { 'names the path and the hash' } else { 'does not' })"

# --- 6. a path that is not there -------------------------------------------
Write-Step "missing: a path that does not exist"
$run = Invoke-Download "missing" $torrentC @(
    "--select-file", "0",
    "--web-seed-for", "file:0=$(ConvertTo-FileUrl $Root)/not-here.dat",
    "--web-seed-mode", "exact"
)
$said = (Get-Content $run.stderr -Raw -ErrorAction SilentlyContinue)
$named = $said -match 'not-here\.dat' -and $said -match 'no such file'
$ok = ($run.exit_code -ne 0) -and $named
Add-Case "missing" $run $null `
    "the source is refused and the report names the path" `
    $ok "exit $($run.exit_code), reason $(if ($named) { 'names the path' } else { 'does not' })"

# --- 7. all three torrents in one invocation -------------------------------
#
# The two-step above is three invocations. This is the same result in one,
# which is what T-133 layer 2 asks for, and it needs two things that did not
# exist: a binding that names one torrent, because `file.blob` is index 0 in
# A and C and index 1 in B, and `-j 1` starting the sources in the order they
# were given, because A and B read the file C writes.
#
# Every other file in each torrent is put in place first, so the only bytes
# this run fetches are the shared file's. That is what makes "fetched once"
# a number rather than a claim: torrent C reads the CDN copy, and A and B read
# what C wrote.

Write-Step "one_invocation: three torrents, one command, C first and A and B from its output"
$hashes = @{}
foreach ($pair in @(@{ k = "a"; t = $torrentA }, @{ k = "b"; t = $torrentB }, @{ k = "c"; t = $torrentC })) {
    $info = & $bitCli info $pair.t --json | ConvertFrom-Json
    if (-not $info.info_hash) { Exit-With 2 "could not read the info hash of $($pair.t)" }
    $hashes[$pair.k] = $info.info_hash
}

$oneOut = Join-Path $Root "out-one"
New-Item -ItemType Directory -Force -Path $oneOut | Out-Null
# Everything except the shared file, so the run has nothing else to fetch.
foreach ($name in @("payload_a", "payload_b", "payload_c")) {
    $from = Join-Path $fixture $name
    $to = Join-Path $oneOut $name
    Copy-Item -Recurse -Force -LiteralPath $from -Destination $to
    Get-ChildItem -Recurse -File -LiteralPath $to |
        Where-Object { $_.Name -eq "file.blob" } |
        Remove-Item -Force
}
$sharedFromC = Join-Path $oneOut "payload_c/a/b/c/file.blob"

$oneArgs = @(
    "download", $torrentC, $torrentA, $torrentB,
    "--dir", $oneOut,
    "-j", "1",
    "--web-seed-only",
    "--no-torrent-web-seed",
    "--web-seed-mode", "exact",
    "--port", "0",
    "--web-seed-for", "$($hashes['c']):file:0=$(ConvertTo-FileUrl $cdnCopy)",
    "--web-seed-for", "$($hashes['a']):file:0=file:///$(($sharedFromC -replace '\\', '/'))",
    "--web-seed-for", "$($hashes['b']):file:1=file:///$(($sharedFromC -replace '\\', '/'))",
    "--json"
)
[void]$commands.Add("bit-cli $($oneArgs -join ' ')")
$oneStdout = Join-Path $Root "one.json"
$oneStderr = Join-Path $Root "one.err"
$clock = [System.Diagnostics.Stopwatch]::StartNew()
$process = Start-Process -FilePath $bitCli -WorkingDirectory $repo -NoNewWindow -PassThru `
    -ArgumentList $oneArgs -RedirectStandardOutput $oneStdout -RedirectStandardError $oneStderr
$finished = $process.WaitForExit($TimeoutSeconds * 1000)
$clock.Stop()
if (-not $finished) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
$oneReport = $null
try { $oneReport = Get-Content $oneStdout -Raw | ConvertFrom-Json } catch { }

$landedOne = @(
    (Join-Path $oneOut "payload_c/a/b/c/file.blob"),
    (Join-Path $oneOut "payload_a/deep/nested/dirs/file.blob"),
    (Join-Path $oneOut "payload_b/media/file.blob")
)
$oneHashes = @($landedOne | ForEach-Object {
        if (Test-Path $_) { (Get-FileHash -Algorithm SHA256 $_).Hash.ToLower() } else { $null }
    })
$oneDistinct = @($oneHashes | Where-Object { $_ } | Sort-Object -Unique)
# Exactly one torrent read the CDN copy. The other two read what it wrote,
# which is the "fetched once" the scenario is about.
$fromCdn = 0
if ($oneReport -and $oneReport.torrents) {
    foreach ($torrent in $oneReport.torrents) {
        foreach ($source in @($torrent.sources)) {
            if ($source -and $source.url -like "*a3f1b2c4-signed-blob.dat") { $fromCdn++ }
        }
    }
}
$oneExit = if ($finished) { $process.ExitCode } else { 124 }
$oneOk = ($oneExit -eq 0) -and ($oneDistinct.Count -eq 1) -and ($oneDistinct[0] -eq $expected) -and
    ($oneHashes.Count -eq 3) -and (-not ($oneHashes -contains $null)) -and ($fromCdn -eq 1)
[void]$cases.Add([ordered]@{
    case             = "one_invocation"
    expectation      = "three torrents in one command: C reads the CDN copy, A and B read what C wrote, and all three land byte for byte"
    exit_code        = $oneExit
    elapsed_ms       = $clock.ElapsedMilliseconds
    downloaded_bytes = if ($oneReport) { $oneReport.downloaded.bytes } else { 0 }
    downloaded_human = if ($oneReport) { $oneReport.downloaded.human } else { "0 B" }
    landed_path      = ($landedOne -join "; ")
    sha256           = $oneDistinct[0]
    hash_matches     = ($oneDistinct.Count -eq 1 -and $oneDistinct[0] -eq $expected)
    source_url       = "one binding per torrent, by info hash"
    source_state     = $null
    source_error     = $null
    sources_from_cdn = $fromCdn
    info_hashes      = @($hashes["c"], $hashes["a"], $hashes["b"])
    ok               = $oneOk
    detail           = "exit $oneExit, $($oneDistinct.Count) distinct hash across three torrents, $fromCdn source read the CDN copy"
})
if (-not $oneOk) {
    [void]$failures.Add("one_invocation : exit $oneExit, $($oneDistinct.Count) distinct hash across three torrents, $fromCdn source read the CDN copy")
}

# --- 8. what the metadata alone can prove ----------------------------------
#
# T-133 layer 3 asks for the equivalence to be derived rather than declared.
# A `.torrent` hashes fixed-size pieces of the whole payload, not files, so two
# files can be compared by hash only where the pieces cover the same bytes of
# each: the same piece length, and the same offset modulo it.
#
# The three-torrent fixture is built from three different piece lengths, which
# is precisely the case where that cannot hold. So this case measures both
# halves: nothing is provable there, and something is provable in a pair built
# to line up. Without the second half the first is an untested "no".

Write-Step "equivalence: what the metadata proves across the fixture, and across an aligned pair"

$againstJson = & $bitCli files $torrentA --against $torrentB --against $torrentC --json | ConvertFrom-Json
$fixtureMatches = @()
foreach ($file in $againstJson.files) {
    if ($file.PSObject.Properties.Name -contains 'shared' -and $file.shared) {
        foreach ($match in $file.shared) {
            $fixtureMatches += [ordered]@{
                index = $file.index
                path = $file.path
                other_index = $match.index
                other_path = $match.path
                evidence = $match.evidence
                proven = $match.proven
            }
        }
    }
}
$fixtureProven = @($fixtureMatches | Where-Object { $_.proven }).Count

# An aligned pair: the same shared file at offset zero in both, the same piece
# length, and a second file that differs in length so it cannot match.
$alignedRoot = Join-Path $Root "aligned"
New-Item -ItemType Directory -Force -Path (Join-Path $alignedRoot "dx") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $alignedRoot "dy") | Out-Null
$sharedBlob = Join-Path $fixture "payload_a/deep/nested/dirs/file.blob"
Copy-Item -LiteralPath $sharedBlob -Destination (Join-Path $alignedRoot "dx/file.blob") -Force
Copy-Item -LiteralPath $sharedBlob -Destination (Join-Path $alignedRoot "dy/file.blob") -Force
# Named to sort after `file.blob`, so the shared file is at offset zero in both.
foreach ($tail in @(@{ dir = "dx"; name = "z1.bin"; mib = 2; seed = 777 },
        @{ dir = "dy"; name = "z2.bin"; mib = 3; seed = 888 })) {
    $block = [byte[]]::new(1024 * 1024)
    [int64]$state = $tail.seed
    for ($i = 0; $i -lt $block.Length; $i++) {
        $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
        $block[$i] = [byte](($state -shr 16) -band 0xFF)
    }
    $stream = [System.IO.File]::Create((Join-Path $alignedRoot "$($tail.dir)/$($tail.name)"))
    try { for ($i = 0; $i -lt $tail.mib; $i++) { $stream.Write($block, 0, $block.Length) } }
    finally { $stream.Dispose() }
}
foreach ($pair in @(@{ dir = "dx" }, @{ dir = "dy" })) {
    & $bitCli create (Join-Path $alignedRoot $pair.dir) --name $pair.dir --piece-length 1MiB `
        --no-creation-date --output (Join-Path $alignedRoot "$($pair.dir).torrent") --force --json 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Exit-With 2 "bit-cli create exited $LASTEXITCODE building the aligned pair" }
}
[void]$commands.Add("bit-cli files $torrentA --against $torrentB --against $torrentC --json")
[void]$commands.Add("bit-cli files $(Join-Path $alignedRoot 'dx.torrent') --against $(Join-Path $alignedRoot 'dy.torrent') --json")

$alignedJson = & $bitCli files (Join-Path $alignedRoot "dx.torrent") `
    --against (Join-Path $alignedRoot "dy.torrent") --json | ConvertFrom-Json
$alignedMatches = @()
foreach ($file in $alignedJson.files) {
    if ($file.PSObject.Properties.Name -contains 'shared' -and $file.shared) {
        foreach ($match in $file.shared) {
            $alignedMatches += [ordered]@{
                index = $file.index
                path = $file.path
                other_index = $match.index
                other_path = $match.path
                evidence = $match.evidence
                proven = $match.proven
                bytes_proven = $match.bytes_proven.bytes
            }
        }
    }
}
$alignedProven = @($alignedMatches | Where-Object { $_.proven })
$equivOk = ($fixtureProven -eq 0) -and ($alignedProven.Count -eq 1) -and
    ($alignedProven[0].bytes_proven -eq $blobBytes) -and ($alignedMatches.Count -eq 1)
[void]$cases.Add([ordered]@{
    case             = "equivalence"
    expectation      = "nothing is provable across three piece lengths, and the shared file is proven byte for byte across an aligned pair"
    exit_code        = 0
    elapsed_ms       = 0
    downloaded_bytes = 0
    downloaded_human = "0 B"
    landed_path      = $null
    sha256           = $null
    hash_matches     = $true
    source_url       = $null
    source_state     = $null
    source_error     = $null
    fixture_matches  = @($fixtureMatches)
    fixture_proven   = $fixtureProven
    aligned_matches  = @($alignedMatches)
    ok               = $equivOk
    detail           = "$($fixtureMatches.Count) candidate(s) across the fixture and $fixtureProven proven; $($alignedMatches.Count) match across the aligned pair and $($alignedProven.Count) proven"
})
if (-not $equivOk) {
    [void]$failures.Add("equivalence : $($fixtureMatches.Count) candidates and $fixtureProven proven across the fixture; $($alignedMatches.Count) matches and $($alignedProven.Count) proven across the aligned pair")
}

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

$sharedHashes = @($cases |
        Where-Object { $_.case -in @("exact", "shared_a", "shared_b") -and $_.sha256 } |
        ForEach-Object { $_.sha256 } |
        Sort-Object -Unique)
$sharedOk = ($sharedHashes.Count -eq 1 -and $sharedHashes[0] -eq $expected)
if (-not $sharedOk) {
    [void]$failures.Add("the shared file did not land identically in all three output directories")
}

$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "local-source-$stamp.json"
$verdict = switch ($true) {
    ($failures.Count -eq 0) { "every case behaved as described"; break }
    default { "$($failures.Count) checks did not"; break }
}

[ordered]@{
    kind           = "check-local-source"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = [System.Environment]::MachineName
        os      = [System.Environment]::OSVersion.VersionString
        cpus    = [System.Environment]::ProcessorCount
    }
    parameters     = [ordered]@{
        blob_size_mib = $BlobSizeMiB
        blob_bytes    = $blobBytes
        profile       = $Profile
    }
    payload_sha256 = $expected
    cases          = @($cases)
    shared_file    = [ordered]@{
        torrents         = 3
        piece_lengths    = @("2 MiB", "1 MiB", "512 KiB")
        distinct_hashes  = $sharedHashes.Count
        matches_source   = $sharedOk
        one_invocation   = [ordered]@{
            info_hashes      = @($hashes["c"], $hashes["a"], $hashes["b"])
            sources_from_cdn = $fromCdn
            distinct_hashes  = $oneDistinct.Count
            from_web_seeds   = if ($oneReport) { $oneReport.from_web_seeds.human } else { $null }
            from_peers       = if ($oneReport) { $oneReport.from_peers.human } else { $null }
            from_resume      = if ($oneReport -and $oneReport.PSObject.Properties.Name -contains 'from_resume') { $oneReport.from_resume.human } else { $null }
            elapsed_ms       = $clock.ElapsedMilliseconds
        }
    }
    verdict        = $verdict
    failures       = @($failures)
    commands       = @($commands)
    notes          = @(
        "No server runs and nothing binds a port. Every byte comes off the local filesystem, so a failure here is bit-cli and not the network.",
        "The three torrents have three info hashes and three piece lengths, 2 MiB, 1 MiB, and 512 KiB. The shared file's piece boundaries therefore line up in none of them, which is what makes the shared cases worth running.",
        "shared_a and shared_b read the copy torrent C wrote in the exact case, so the 64 MiB is fetched once and lands three times.",
        "wrong_bytes uses a file of exactly the right length, so nothing but the per-piece hash check can catch it. That check is --web-seed-verify piece and it is the default.",
        "one_invocation is the same result in one command. It counts how many sources read the CDN copy and requires exactly one: the other two read what the first torrent wrote, which is what 'fetched once' means when the CDN is a real one.",
        "The equivalence in one_invocation is declared by the caller and verified by bit-cli rather than trusted. Every piece a file: source serves is hash-checked against the torrent that asked for it before the session sees it, which is what wrong_bytes measures with a file of exactly the right length.",
        "The equivalence case measures both halves of what the metadata can decide. A .torrent hashes fixed-size pieces of the whole payload rather than files, so two files can be compared by hash only where the pieces cover the same bytes of each: the same piece length, and the same offset modulo it. The three-torrent fixture has three piece lengths, so nothing there is provable and the report says `length` rather than claiming a match. The aligned pair has one piece length and the shared file at offset zero in both, so its hashes line up and the whole file is proven.",
        "Two of the fixture's `length` candidates are files that are not the same bytes at all: deep/other.bin against a/extra.bin, and readme.txt against notes/changelog.txt. Equal length is not evidence, which is exactly why the evidence is reported rather than a yes or no."
    )
} | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
Write-Host "shared file: $(Format-Size $blobBytes), three torrents, three piece lengths"
Write-Host "report:      $reportPath"
Write-Host ""
$cases | ForEach-Object {
    [pscustomobject][ordered]@{
        case       = $_.case
        exit       = $_.exit_code
        downloaded = $_.downloaded_human
        hash       = if ($_.hash_matches) { "matches" } else { "-" }
        ok         = if ($_.ok) { "yes" } else { "NO" }
    }
} | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "the shared file landed with $($sharedHashes.Count) distinct hash across three info hashes"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-local-source: $failure") }
    exit 1
}
exit 0
