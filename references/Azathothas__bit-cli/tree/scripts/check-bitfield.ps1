# Prove a torrent whose bitfield does not fit in one protocol message still works.
#
# `TODO/peers.md` T-194: `Message::Bitfield` used to be serialized into the
# fixed per connection write buffer, which is `MAX_MSG_LEN` = 16,500 bytes. A
# bitfield is one bit per piece, so its length is a property of the torrent.
# Past 131,960 pieces it did not fit, the serialize failed with "not enough
# space in buffer", and the connection was dropped before anything was served.
# A seeder could not answer and a magnet never resolved.
#
# The variable is the piece count, not the size of the `.torrent`. Two 2.64 MB
# torrents sixteen pieces apart sat on either side of the threshold, so this
# check is written in pieces and derives the payload from them.
#
# What it does, per case: build a payload of exactly N pieces of 1 KiB, create
# a `.torrent` from it, seed it on loopback with trackers and DHT off, then ask
# a second process to fetch the same torrent by magnet knowing nothing but
# `--peer 127.0.0.1:<port>`. Metadata resolving and a file appearing is the
# pass: both require the bitfield to have been sent.
#
# Usage:
#   pwsh scripts/check-bitfield.ps1
#   pwsh scripts/check-bitfield.ps1 -Pieces 262104
#   pwsh scripts/check-bitfield.ps1 -Pieces 131960,131961,163840
#
# Exits 0 when every case resolved, 1 when one did not, and 2 when the check
# could not run. The record goes to bench/bitfield-<timestamp>.json.
#
# The read side had a ceiling of its own until 2026-08-22 and no longer does.
# `ReadBuf` was a fixed 32,768 byte ring buffer, so 262,105 pieces failed with
# "read buffer is full" however well the send side behaved. It grows now, up to
# what the connection says the torrent could need, which is T-195. 1,048,576
# pieces resolve, four times the old ceiling and a 21 MB `.torrent`.
#
# The default cases below are the two thresholds this repository has actually
# measured a client dying on, one for each side.
#
# See TODO/peers.md, T-194 and T-195.

[CmdletBinding()]
param(
    # Piece counts to prove. The two the old code died on, one per side:
    # 131,961 is the first count the fixed write buffer could not hold, and
    # 262,105 the first the fixed read buffer could not.
    [int[]]$Pieces = @(131961, 262105),
    # Bytes per piece. 1 KiB keeps the payload small enough to build in a
    # second: the piece count is what is being tested, not the piece length.
    [int]$PieceLength = 1024,
    [int]$FirstPort = 6921,
    # Seconds to wait for metadata to resolve before calling it a failure.
    [int]$ResolveTimeout = 60,
    [string]$Root = ".tmp/bitfield",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-bitfield: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Get-Stamp {
    (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
}

$exe = Join-Path $repo "target/$Profile/bit-cli.exe"
if (-not (Test-Path $exe)) {
    $exe = Join-Path $repo "target/$Profile/bit-cli"
}
if (-not (Test-Path $exe)) {
    Exit-With 2 "no bit-cli binary at target/$Profile. Run: cargo build --profile $Profile --bins"
}

$root = Join-Path $repo $Root
if (Test-Path $root) { Remove-Item -Recurse -Force $root }
New-Item -ItemType Directory -Force -Path $root | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $repo $ReportDir) | Out-Null

# The bitfield message is a 4 byte length, a 1 byte id, then one bit per piece.
$PreambleLen = 5
function Get-BitfieldLen([int]$pieces) {
    $PreambleLen + [math]::Ceiling($pieces / 8)
}

# Wait on the condition, never on a duration: the seeder has to be accepting
# before the leecher is told to dial it. A fixed sleep produced one
# "connection actively refused" run that read as a negative result.
function Wait-ForListener([int]$port, [System.Diagnostics.Process]$proc, [int]$timeoutSeconds) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if ($proc.HasExited) { return $false }
        $probe = New-Object System.Net.Sockets.TcpClient
        try {
            $probe.Connect('127.0.0.1', $port)
            if ($probe.Connected) { return $true }
        } catch {
            # not listening yet
        } finally {
            $probe.Close()
        }
        Start-Sleep -Milliseconds 200
    }
    return $false
}

$cases = @()
$failed = 0
$port = $FirstPort

foreach ($pieceCount in $Pieces) {
    $name = "p$pieceCount"
    $dataDir = Join-Path $root "$name/data"
    $outDir = Join-Path $root "$name/out"
    New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null

    # Exactly $pieceCount full pieces.
    $payload = Join-Path $dataDir "data.bin"
    $total = [int64]$pieceCount * [int64]$PieceLength
    $fs = [System.IO.File]::Create($payload)
    try {
        $fs.SetLength($total)
    } finally {
        $fs.Close()
    }

    $torrent = Join-Path $root "$name.torrent"
    # --allow piece-count: bit-cli refuses to create anything above 100,000
    # pieces by default, which is the lint doing its job and not this check.
    $createJson = & $exe create $payload --piece-length "$PieceLength" --allow piece-count -o $torrent --json 2>$null | ConvertFrom-Json
    if (-not (Test-Path $torrent)) {
        Exit-With 2 "could not create a torrent of $pieceCount pieces"
    }
    if ($createJson.piece_count -ne $pieceCount) {
        Exit-With 2 "asked for $pieceCount pieces, got $($createJson.piece_count)"
    }

    $magnet = (& $exe magnet $torrent --json 2>$null | ConvertFrom-Json).magnet

    $seedOut = Join-Path $root "$name-seed.json"
    $seedErr = Join-Path $root "$name-seed.err"
    $dlOut = Join-Path $root "$name-dl.json"
    $dlErr = Join-Path $root "$name-dl.err"

    $seedFor = $ResolveTimeout + 30
    $seeder = Start-Process -FilePath $exe -PassThru -NoNewWindow `
        -RedirectStandardOutput $seedOut -RedirectStandardError $seedErr `
        -ArgumentList @('seed', $torrent, '--dir', $dataDir, '--port', "$port",
            '--seed-time', "${seedFor}s", '--no-tracker', '-vv', '--json')

    $resolved = $false
    $listened = Wait-ForListener $port $seeder 120
    $downloader = $null
    if ($listened) {
        $downloader = Start-Process -FilePath $exe -PassThru -NoNewWindow `
            -RedirectStandardOutput $dlOut -RedirectStandardError $dlErr `
            -ArgumentList @('download', $magnet, '--peer', "127.0.0.1:$port",
                '--no-dht', '--dir', $outDir, '-vv', '--json')

        # A magnet that resolves creates its files. That is the signal: it
        # cannot happen without the bitfield having been sent and read.
        $deadline = (Get-Date).AddSeconds($ResolveTimeout)
        while ((Get-Date) -lt $deadline -and -not $resolved) {
            if (@(Get-ChildItem -Path $outDir -Recurse -File -ErrorAction SilentlyContinue).Count -gt 0) {
                $resolved = $true
                break
            }
            if ($downloader.HasExited) { break }
            Start-Sleep -Milliseconds 500
        }
    }

    foreach ($p in @($downloader, $seeder)) {
        if ($null -ne $p -and -not $p.HasExited) { $p | Stop-Process -Force }
    }
    Start-Sleep -Milliseconds 500

    $seedText = if (Test-Path $seedErr) { Get-Content -Raw $seedErr } else { '' }
    $dlText = if (Test-Path $dlErr) { Get-Content -Raw $dlErr } else { '' }

    $case = [ordered]@{
        pieces            = $pieceCount
        piece_length      = $PieceLength
        payload_bytes     = $total
        torrent_bytes     = (Get-Item $torrent).Length
        bitfield_bytes    = Get-BitfieldLen $pieceCount
        seeder_listened   = $listened
        metadata_resolved = $resolved
        no_space_in_buffer = ($seedText -match 'not enough space in buffer')
        read_buffer_full  = (($seedText -match 'read buffer is full') -or ($dlText -match 'read buffer is full'))
        pass              = $resolved
    }
    $cases += $case

    $verdict = if ($resolved) { "resolved" } else { "DID NOT RESOLVE" }
    $why = ""
    if (-not $resolved) {
        if ($case.no_space_in_buffer) { $why = ", not enough space in buffer" }
        elseif ($case.read_buffer_full) { $why = ", read buffer is full" }
        elseif (-not $listened) { $why = ", the seeder never listened" }
    }
    Write-Host ("bitfield: {0} pieces, {1} B torrent, {2} B bitfield, {3}{4}" -f `
            $pieceCount, $case.torrent_bytes, $case.bitfield_bytes, $verdict, $why)

    if (-not $resolved) { $failed++ }
    $port++
}

$report = [ordered]@{
    schema_version = "1"
    kind           = "bitfield"
    generated_at   = Get-Timestamp
    max_msg_len    = 16500
    read_buflen    = 32768
    cases          = $cases
    failed         = $failed
}
$reportPath = Join-Path $repo "$ReportDir/bitfield-$(Get-Stamp).json"
$report | ConvertTo-Json -Depth 6 | Set-Content -Path $reportPath -Encoding utf8
Write-Host "bitfield: report $reportPath"

if (-not $Keep) { Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue }

if ($failed -gt 0) {
    Exit-With 1 "$failed of $($cases.Count) case(s) did not resolve"
}
Write-Host "bitfield: ok"
exit 0
