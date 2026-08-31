# Build the fixture the multi-source scenarios are measured against.
#
# `TODO/multi-source.md` describes five scenarios about pointing several kinds
# of source at one payload. All five are testable on loopback, and this builds
# what they need:
#
#   payload/deep/nested/dirs/file.blob   the file every scenario is about
#   payload/deep/other.bin               a second file, so the tree is real
#   payload/readme.txt                   a third, small
#   cdn/<random>-signed-blob.dat         the same file.blob under a name and a
#                                        path with no relation to the torrent
#   mirror/<torrent name>/...            the same tree under a second layout,
#                                        for the remapping scenario
#
#   torrent_a.torrent   the three files above, 1 MiB pieces
#   torrent_b.torrent   the same file.blob plus two different files, 512 KiB
#                       pieces, so the piece boundaries do not line up
#   torrent_c.torrent   the same file.blob plus a third set, 2 MiB pieces, and
#                       a web seed already in its url-list, from `-WebSeed`
#
# `-PieceLength` overrides all three piece lengths with one, which is what
# makes the shared file provable from the metadata rather than only assertable.
#
# Torrents A, B, and C hold a bit-identical `file.blob` under three different
# info hashes, which is scenario 2. Their piece lengths differ on purpose:
# equivalence that only works when the boundaries align is not equivalence.
#
# `-Partial` leaves a percentage of file.blob already on disk in each output
# directory, which is the state scenarios 1, 2, and 3 start from.
#
# Usage:
#   pwsh scripts/make-scenario-fixture.ps1
#   pwsh scripts/make-scenario-fixture.ps1 -BlobSizeMiB 256 -Partial 60
#
# Exits 0 when every torrent was created, and 2 when it could not run. The
# fixture stays where it is put; nothing here removes it.
#
# See TODO/multi-source.md.

[CmdletBinding()]
param(
    [int]$BlobSizeMiB = 64,
    [int]$OtherSizeMiB = 8,
    [int]$Partial = 70,
    # One piece length for all three torrents instead of three different ones.
    # The default is the three, because equivalence that only holds when the
    # boundaries line up is not equivalence. T-140 needs the other case: the
    # boundaries lining up is what lets the metadata prove the file is shared.
    [string]$PieceLength = "",
    # Torrent C's url-list entry. The default is a fixed port nothing is
    # listening on, so a caller that wants it served has to say where.
    [string]$WebSeed = "http://127.0.0.1:8080/",
    [string]$Root = ".tmp/scenario",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release"
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("make-scenario-fixture: $message")
    exit $code
}

function Write-Step($message) {
    Write-Host "$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.fffZ')) $message"
}

# Deterministic bytes from the ANSI C LCG, taking bits 16 to 23. The same
# generator the other scripts here use, so a payload built twice is identical.
function New-Blob([string]$path, [int]$mib, [int64]$seed) {
    $block = 1MB
    $buffer = [byte[]]::new($block)
    [int64]$state = $seed
    for ($i = 0; $i -lt $block; $i++) {
        $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
        $buffer[$i] = [byte](($state -shr 16) -band 0xFF)
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $path) | Out-Null
    $stream = [System.IO.File]::Create($path)
    try { for ($written = 0; $written -lt $mib; $written++) { $stream.Write($buffer, 0, $block) } }
    finally { $stream.Dispose() }
}

# Copy the first `$mib` MiB of a file, which is what a stalled download leaves.
function Copy-Prefix([string]$from, [string]$to, [int]$mib) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $to) | Out-Null
    $src = [System.IO.File]::OpenRead($from)
    $dst = [System.IO.File]::Create($to)
    try {
        $buffer = [byte[]]::new(1MB)
        for ($i = 0; $i -lt $mib; $i++) {
            $read = $src.Read($buffer, 0, 1MB)
            if ($read -le 0) { break }
            $dst.Write($buffer, 0, $read)
        }
    } finally { $src.Dispose(); $dst.Dispose() }
}

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --workspace --bins --examples"
}
if ($BlobSizeMiB -lt 4) { Exit-With 2 "-BlobSizeMiB has to be at least 4." }
if ($Partial -lt 0 -or $Partial -gt 100) { Exit-With 2 "-Partial is a percentage." }

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force -LiteralPath $Root }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path

# ---------------------------------------------------------------------------
# The payloads
# ---------------------------------------------------------------------------
#
# `file.blob` is byte-identical in all three torrents, which is the whole point.
# Everything around it differs, so the three info hashes differ.

Write-Step "building the shared file, $BlobSizeMiB MiB"
$blob = Join-Path $Root "payload_a/deep/nested/dirs/file.blob"
New-Blob $blob $BlobSizeMiB 111
New-Blob (Join-Path $Root "payload_a/deep/other.bin") $OtherSizeMiB 333
New-Blob (Join-Path $Root "payload_a/readme.txt") 1 222

Write-Step "building torrent B's tree around the same file"
New-Item -ItemType Directory -Force -Path (Join-Path $Root "payload_b/media") | Out-Null
Copy-Item -LiteralPath $blob -Destination (Join-Path $Root "payload_b/media/file.blob") -Force
New-Blob (Join-Path $Root "payload_b/notes/changelog.txt") 1 444
New-Blob (Join-Path $Root "payload_b/media/cover.png") 2 555

Write-Step "building torrent C's tree around the same file"
New-Item -ItemType Directory -Force -Path (Join-Path $Root "payload_c/a/b/c") | Out-Null
Copy-Item -LiteralPath $blob -Destination (Join-Path $Root "payload_c/a/b/c/file.blob") -Force
New-Blob (Join-Path $Root "payload_c/a/extra.bin") 4 666

# The CDN copy: same bytes, a name and a path with no relation to the torrent.
New-Item -ItemType Directory -Force -Path (Join-Path $Root "cdn") | Out-Null
$signed = Join-Path $Root "cdn/a3f1b2c4-signed-blob.dat"
Copy-Item -LiteralPath $blob -Destination $signed -Force

# The second mirror: the same tree under a different layout, with a space in a
# directory name, for the remapping and encoding scenario.
New-Item -ItemType Directory -Force -Path (Join-Path $Root "mirror/pub files/payload") | Out-Null
Copy-Item -LiteralPath $blob -Destination (Join-Path $Root "mirror/pub files/payload/file.blob") -Force

# ---------------------------------------------------------------------------
# The torrents
# ---------------------------------------------------------------------------
#
# Three piece lengths on purpose. Equivalence that only holds when the piece
# boundaries line up is not equivalence, and the fixture has to be able to tell
# the difference. `-PieceLength` gives all three the same one, which is the
# other case: T-140 needs the boundaries to line up so the metadata can prove
# the file is shared.

$created = @()
Push-Location $Root
try {
    foreach ($spec in @(
        @{ dir = "payload_a"; name = "payload_a"; piece = "1MiB";   out = "torrent_a.torrent"; seed = $null },
        @{ dir = "payload_b"; name = "payload_b"; piece = "512KiB"; out = "torrent_b.torrent"; seed = $null },
        @{ dir = "payload_c"; name = "payload_c"; piece = "2MiB";   out = "torrent_c.torrent"; seed = $WebSeed }
    )) {
        $piece = if ($PieceLength) { $PieceLength } else { $spec.piece }
        $arguments = @(
            "create", $spec.dir, "--name", $spec.name, "--piece-length", $piece,
            "--no-creation-date", "--output", $spec.out, "--force", "--json"
        )
        if ($spec.seed) { $arguments += @("--web-seed", $spec.seed) }
        $stdout = Join-Path $Root "create.out"
        $stderr = Join-Path $Root "create.err"
        $process = Start-Process -FilePath $bitCli -ArgumentList $arguments -WorkingDirectory $Root `
            -NoNewWindow -Wait -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        if ($process.ExitCode -ne 0) {
            Exit-With 2 "bit-cli create for $($spec.dir) exited $($process.ExitCode): $(Get-Content $stderr -Raw)"
        }
        $created += (Get-Content $stdout -Raw | ConvertFrom-Json)
        Remove-Item $stdout, $stderr -Force -ErrorAction SilentlyContinue
    }
} finally { Pop-Location }

# ---------------------------------------------------------------------------
# The partial state each scenario starts from
# ---------------------------------------------------------------------------

if ($Partial -gt 0) {
    $have = [int][math]::Floor($BlobSizeMiB * $Partial / 100)
    Write-Step "leaving $Partial% of file.blob on disk in each output directory"
    Copy-Prefix $blob (Join-Path $Root "out_a/payload_a/deep/nested/dirs/file.blob") $have
    Copy-Prefix $blob (Join-Path $Root "out_b/payload_b/media/file.blob") $have
    Copy-Prefix $blob (Join-Path $Root "out_c/payload_c/a/b/c/file.blob") $have
}

Write-Host ""
Write-Host "fixture: $Root"
Write-Host ""
foreach ($torrent in $created) {
    "{0,-12} {1}  {2,10}  pieces {3,4} of {4}" -f `
        $torrent.name, $torrent.info_hash, $torrent.total.human, $torrent.piece_count, $torrent.piece_length.human
}
Write-Host ""
Write-Host "the shared file, byte for byte the same in all three:"
foreach ($path in @("payload_a/deep/nested/dirs/file.blob", "payload_b/media/file.blob", "payload_c/a/b/c/file.blob", "cdn/a3f1b2c4-signed-blob.dat")) {
    $full = Join-Path $Root $path
    "  {0}  {1}" -f (Get-FileHash $full -Algorithm SHA256).Hash.Substring(0, 16), $path
}
exit 0
