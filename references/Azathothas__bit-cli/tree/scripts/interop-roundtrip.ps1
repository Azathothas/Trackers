# Prove the create round trip against a different BitTorrent client.
#
# A torrent nobody else can read is not a torrent. This script builds a
# payload, creates a .torrent with bit-cli, verifies it with bit-cli, seeds it
# with bit-cli, and then downloads it with a second client over loopback. It
# passes only when the second client's output is byte-identical to the input.
#
# Four cases run:
#   v1        a plain multi-file torrent, served by bit-cli seed over BitTorrent
#   private   the same with --private, so the private flag is exercised
#   webseed   --web-seed with no peer at all, so the second client has to
#             resolve the url-list and fetch over HTTP alone
#   magnet    a magnet resolved off a bit-cli seeder with `magnet --output`,
#             then opened by the second client. The property is cross-tool: a
#             torrent this tree writes out of metadata it pulled over BEP 9 is
#             only worth writing if another client will open it. See
#             TODO/metainfo.md, T-241.
#
# Nothing here touches the network. The tracker, the web seed, the seeder, and
# the second client all bind 127.0.0.1.
#
# Usage:
#   pwsh scripts/interop-roundtrip.ps1
#   pwsh scripts/interop-roundtrip.ps1 -Client aria2c -Keep
#   pwsh scripts/interop-roundtrip.ps1 -Profile release
#
# Exits 0 when every case round trips, 1 when a case fails, and 2 when the
# check could not run. The full record, with exact commands, exit codes,
# timings, and hashes, is written to <Root>/report.json.
#
# See TODO/create-seed.md, T-084.

[CmdletBinding()]
param(
    # The second client. Must be able to download a .torrent from the command
    # line. aria2c, rqbit, and transmission-cli are the ones this was written
    # against; only aria2c is wired up so far.
    [string]$Client = "aria2c",
    # Where payloads, torrents, logs, and the report go. Gitignored.
    [string]$Root = ".tmp/interop",
    # Which bit-cli build to drive.
    [ValidateSet("debug", "release")]
    [string]$Profile = "debug",
    # Seconds to let a single download run before giving up.
    [int]$TimeoutSeconds = 120,
    # Keep the payloads and downloads. Off by default because they are large.
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

# Write-Error is a terminating error under `Stop`, so a `Write-Error` followed
# by `exit 2` never reaches the exit and the caller sees 1. The exit codes in
# the header above are the contract, so failures go out this way instead.
function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("interop-roundtrip: $message")
    exit $code
}

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

function Resolve-Tool($name, $hint) {
    $found = Get-Command $name -ErrorAction SilentlyContinue
    if (-not $found) {
        Exit-With 2 "$name not found on PATH. $hint"
    }
    $found.Source
}

# ---------------------------------------------------------------------------
# Tools
# ---------------------------------------------------------------------------

$exe = if ($IsWindows) { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
$tracker = Join-Path $repo "target/$Profile/examples/loopback-tracker$exe"
$fileserver = Join-Path $repo "target/$Profile/examples/loopback-fileserver$exe"

foreach ($required in @($bitCli, $tracker, $fileserver)) {
    if (-not (Test-Path $required)) {
        Exit-With 2 "missing $required. Build it first: cargo build --workspace --bins --examples"
    }
}

$clientPath = Resolve-Tool $Client "Install it, or pass -Client with the path to another BitTorrent client."
$clientKind = switch -Regex (Split-Path -Leaf $clientPath) {
    '^aria2c' { 'aria2c' }
    '^rqbit'  { 'rqbit' }
    default {
        Exit-With 2 "-Client $Client is not wired up. Add a branch for it to Get-ClientArgs and Get-ShowArgs in this script."
    }
}
$clientVersion = (& $clientPath --version 2>&1 | Select-Object -First 1)

# How each client is asked to parse a torrent and print what it found, without
# downloading, and what its output has to contain for the parse to count.
#
# The two clients print different things. aria2c prints the info hash, so that
# is what is checked. rqbit prints the file list and not the hash, so the file
# names are checked instead. Agreement on the info hash is proven either way by
# the transfer itself: the tracker keys its swarm on the hash, so a client that
# computed a different one never finds the seeder and the case fails.
function Get-ShowArgs($torrent) {
    switch ($clientKind) {
        'aria2c' { @("-S", $torrent) }
        'rqbit'  { @("--disable-dht", "--disable-lsd", "download", "--list", $torrent) }
    }
}

function Get-ShowExpectation($infoHash) {
    switch ($clientKind) {
        'aria2c' { , @($infoHash) }
        'rqbit'  { , @("a.flac", "b.flac", "notes.nfo", "tiny.bin") }
    }
}

# How each client is asked to download one torrent to one directory, with every
# means of finding a peer disabled except the tracker in the torrent itself.
function Get-DownloadArgs($torrent, $outDir, $timeout) {
    switch ($clientKind) {
        'aria2c' {
            @(
                "--dir=$outDir",
                "--enable-dht=false", "--enable-dht6=false", "--bt-enable-lpd=false",
                "--seed-time=0", "--allow-overwrite=true",
                "--console-log-level=info", "--summary-interval=0",
                "--listen-port=6881",
                "--bt-stop-timeout=$timeout",
                $torrent
            )
        }
        'rqbit' {
            # rqbit has no BEP 19 support, so the web seed case is skipped for
            # it rather than reported as a failure. See Invoke-Case.
            @(
                "--disable-dht", "--disable-dht-persistence", "--disable-lsd",
                "--disable-upnp-port-forward", "--http-api-listen-addr", "127.0.0.1:0",
                "--listen-port", "6882",
                "download", "--exit-on-finish", "--overwrite",
                "--output-folder", $outDir,
                $torrent
            )
        }
    }
}

# ---------------------------------------------------------------------------
# Workspace
# ---------------------------------------------------------------------------

if (-not [System.IO.Path]::IsPathRooted($Root)) {
    $Root = Join-Path $repo $Root
}
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path

# Deterministic payload bytes, so the info hashes below are reproducible and
# can be quoted as evidence.
#
# The generator is the ANSI C LCG on a 31-bit state, taking bits 16 to 23 of
# each step. PowerShell has no wrapping unsigned arithmetic, so the state is
# kept narrow enough that `state * 1103515245` cannot overflow Int64. The low
# bits of an LCG are poor, which is why the byte comes from the middle.
function New-PayloadFile($path, $length, [int64]$seed) {
    $parent = Split-Path -Parent $path
    if ($parent -and -not (Test-Path $parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    $bytes = [byte[]]::new($length)
    [int64]$state = $seed
    for ($i = 0; $i -lt $length; $i++) {
        $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
        $bytes[$i] = [byte](($state -shr 16) -band 0xFF)
    }
    [System.IO.File]::WriteAllBytes($path, $bytes)
}

$payload = Join-Path $Root "payload"
Write-Step "building payload under $payload"
# A space in a directory name and a nested path, because percent-encoding of
# the path is the most common way a web seed silently serves nothing.
New-PayloadFile (Join-Path $payload "disc 1/a.flac")     300000 1
New-PayloadFile (Join-Path $payload "disc 1/b.flac")     150000 2
New-PayloadFile (Join-Path $payload "extras/notes.nfo")   40000 3
New-PayloadFile (Join-Path $payload "tiny.bin")              12 4

# Hash a file that a just-killed client may still hold open.
#
# A sharing violation here is transient by construction: the only thing that
# had the file is the client this script started and has already stopped. It is
# waited out on the condition, with a ceiling, so a slow teardown is a slow
# hash rather than a red job whose message names neither the client nor the
# timeout that caused it. See `TODO/create-seed.md`, T-225.
function Get-FileHashWhenReadable($path, $timeoutSeconds = 30) {
    $deadline = (Get-Date).AddSeconds($timeoutSeconds)
    while ($true) {
        try {
            return (Get-FileHash -Algorithm SHA256 -LiteralPath $path -ErrorAction Stop).Hash.ToLower()
        }
        catch {
            if ((Get-Date) -ge $deadline) {
                throw "cannot read $path after $timeoutSeconds seconds: $($_.Exception.Message)"
            }
            Start-Sleep -Milliseconds 200
        }
    }
}

function Get-TreeHashes($dir) {
    $dir = (Resolve-Path $dir).Path
    $out = [ordered]@{}
    Get-ChildItem -Recurse -File $dir | Sort-Object FullName | ForEach-Object {
        $relative = $_.FullName.Substring($dir.Length + 1).Replace('\', '/')
        $out[$relative] = @{
            sha256 = Get-FileHashWhenReadable $_.FullName
            bytes  = $_.Length
        }
    }
    $out
}

$sourceHashes = Get-TreeHashes $payload
$sourceBytes = ($sourceHashes.Values | Measure-Object -Property bytes -Sum).Sum
Write-Step "payload is $($sourceHashes.Count) files, $sourceBytes bytes"

# ---------------------------------------------------------------------------
# Process helpers
# ---------------------------------------------------------------------------

$script:Background = @()

function Start-Background($name, $path, $arguments, $workDir) {
    $stdout = Join-Path $Root "$name.out"
    $stderr = Join-Path $Root "$name.err"
    $process = Start-Process -FilePath $path -ArgumentList $arguments `
        -WorkingDirectory $workDir -NoNewWindow -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $script:Background += [pscustomobject]@{ Name = $name; Process = $process }
    [pscustomobject]@{ Process = $process; Stdout = $stdout; Stderr = $stderr }
}

# The tracker and the file server print their URL on the first line of stdout
# before serving anything, so a caller never has to guess a port.
function Wait-ForUrl($file, $seconds = 15) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $file) {
            $line = (Get-Content $file -TotalCount 1 -ErrorAction SilentlyContinue)
            if ($line -and $line.Trim()) { return $line.Trim() }
        }
        Start-Sleep -Milliseconds 100
    }
    throw "no URL on stdout of $file after ${seconds}s"
}

function Stop-Background {
    foreach ($entry in $script:Background) {
        if (-not $entry.Process.HasExited) {
            Stop-Process -Id $entry.Process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $script:Background = @()
}

function Invoke-Recorded($label, $path, $arguments, $workDir, $timeout) {
    $stdout = Join-Path $Root "$label.out"
    $stderr = Join-Path $Root "$label.err"
    $started = Get-Timestamp
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $path -ArgumentList $arguments `
        -WorkingDirectory $workDir -NoNewWindow -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    if (-not $process.WaitForExit($timeout * 1000)) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        # `Stop-Process -Force` returns before Windows has finished tearing the
        # process down, and a client that was mid-download still holds its
        # output files open for a moment after it. Everything below this hashes
        # those files, so waiting for the process to be gone is the difference
        # between a timeout reported as a timeout and a timeout reported as
        # `Get-FileHash: the process cannot access the file`. Waited on the
        # condition with a ceiling, not slept. See `TODO/create-seed.md`, T-225.
        $null = $process.WaitForExit(30 * 1000)
        $clock.Stop()
        return [pscustomobject]@{
            label = $label; command = "$path $($arguments -join ' ')"
            started_at = $started; exit_code = $null; timed_out = $true
            elapsed_ms = $clock.ElapsedMilliseconds; stdout = $stdout; stderr = $stderr
        }
    }
    $clock.Stop()
    [pscustomobject]@{
        label = $label; command = "$path $($arguments -join ' ')"
        started_at = $started; exit_code = $process.ExitCode; timed_out = $false
        elapsed_ms = $clock.ElapsedMilliseconds; stdout = $stdout; stderr = $stderr
    }
}

# ---------------------------------------------------------------------------
# One case
# ---------------------------------------------------------------------------

function Invoke-Case {
    param(
        [string]$Name,
        # Extra flags for bit-cli create.
        [string[]]$CreateArgs = @(),
        # When set, no tracker and no seeder run: the second client has only
        # the url-list to work from.
        [switch]$WebSeedOnly
    )

    Write-Step "case $Name"
    $failures = [System.Collections.ArrayList]@()
    $steps = [System.Collections.ArrayList]@()
    $torrent = Join-Path $Root "$Name.torrent"
    $outDir = Join-Path $Root "out-$Name"
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null

    try {
        $announce = $null
        $webSeed = $null

        if ($WebSeedOnly) {
            # BEP 19 appends the torrent name to a URL ending in `/`, so the
            # server root is the directory holding `payload`, not `payload`.
            $server = Start-Background "$Name-fileserver" $fileserver @("--root", $Root) $Root
            $webSeed = Wait-ForUrl $server.Stdout
            Write-Step "  web seed at $webSeed"
            $CreateArgs += @("--web-seed", $webSeed)
        } else {
            $trk = Start-Background "$Name-tracker" $tracker @("--port", "0", "--interval", "5") $Root
            $announce = Wait-ForUrl $trk.Stdout
            Write-Step "  tracker at $announce"
            $CreateArgs += @("--announce", $announce)
        }

        # create
        $create = Invoke-Recorded "$Name-create" $bitCli (@(
            "create", "payload",
            "--piece-length", "32KiB",
            "--no-creation-date",
            "--output", $torrent,
            "--force", "--json"
        ) + $CreateArgs) $Root 60
        [void]$steps.Add($create)
        if ($create.exit_code -ne 0) {
            [void]$failures.Add("bit-cli create exited $($create.exit_code)")
            return [pscustomobject]@{ name = $Name; passed = $false; failures = $failures; steps = $steps }
        }
        $created = Get-Content $create.stdout -Raw | ConvertFrom-Json
        Write-Step "  info hash $($created.info_hash), $($created.piece_count) pieces"

        # verify
        $verify = Invoke-Recorded "$Name-verify" $bitCli @(
            "verify", $torrent, "--dir", $Root, "--json"
        ) $Root 60
        [void]$steps.Add($verify)
        if ($verify.exit_code -ne 0) {
            [void]$failures.Add("bit-cli verify exited $($verify.exit_code)")
        }

        # The second client has to agree on the info hash before anything else
        # is worth testing. This parses and prints, it does not download.
        $show = Invoke-Recorded "$Name-client-show" $clientPath (Get-ShowArgs $torrent) $Root 30
        [void]$steps.Add($show)
        # Both streams, because rqbit logs its listing and aria2c prints it.
        $shown = (Get-Content $show.stdout -Raw) + (Get-Content $show.stderr -Raw)
        foreach ($wanted in (Get-ShowExpectation $created.info_hash)) {
            if ($shown -notmatch [regex]::Escape($wanted)) {
                [void]$failures.Add("$Client parsed the torrent but did not report ``$wanted``")
            }
        }

        # seed
        $seed = $null
        if (-not $WebSeedOnly) {
            $seed = Start-Background "$Name-seed" $bitCli @(
                "seed", $torrent, "--data", $Root, "--port", "51413",
                "--no-dht", "--no-lsd", "--no-pex",
                "--report-interval", "1s", "--exit-when-idle", "5s",
                "--stop-after", "$($TimeoutSeconds)s", "--jsonl"
            ) $Root
            # The seeder hash-checks the payload before it announces, so give
            # the tracker time to see it rather than racing the client to it.
            $deadline = (Get-Date).AddSeconds(30)
            while ((Get-Date) -lt $deadline) {
                if ((Test-Path $trk.Stderr) -and
                    (Select-String -Path $trk.Stderr -Pattern 'left=0' -Quiet)) { break }
                Start-Sleep -Milliseconds 200
            }
            Write-Step "  seeder announced"
        }

        # download with the second client
        $download = Invoke-Recorded "$Name-client-download" $clientPath `
            (Get-DownloadArgs $torrent $outDir $TimeoutSeconds) $Root $TimeoutSeconds
        [void]$steps.Add($download)
        if ($download.timed_out) {
            [void]$failures.Add("$Client did not finish within ${TimeoutSeconds}s")
        } elseif ($download.exit_code -ne 0) {
            [void]$failures.Add("$Client exited $($download.exit_code)")
        }

        # The web seed case has no peer at all, so every byte has to be
        # accounted for by the HTTP server. Counting them here is what stops
        # the case passing for some other reason.
        $servedBytes = $null
        if ($WebSeedOnly) {
            $servedBytes = 0
            Select-String -Path $server.Stderr -Pattern '-> 20[06] (\d+) byte' |
                ForEach-Object { $servedBytes += [int64]$_.Matches[0].Groups[1].Value }
            Write-Step "  web seed served $servedBytes bytes"
            if ($servedBytes -lt $created.total.bytes) {
                [void]$failures.Add("the web seed served $servedBytes bytes, less than the $($created.total.bytes) byte payload")
            }
        }

        # The seeder's own account of what it sent, so this is bit-cli's number
        # rather than an inference from the file sizes.
        #
        # It is the object whose `kind` is `seed`, found by walking the stream
        # backwards, not simply the last line: under `--jsonl` every run now
        # ends with a `session_end` event, and before that it ended with the
        # report. Reading the last line was right until it was not, which is
        # what this comment exists to stop happening again.
        $seedReport = $null
        if ($seed) {
            if ($seed.Process.WaitForExit(30 * 1000)) {
                $lines = @(Get-Content $seed.Stdout -ErrorAction SilentlyContinue)
                for ($index = $lines.Count - 1; $index -ge 0 -and -not $seedReport; $index--) {
                    if (-not $lines[$index].Trim()) { continue }
                    $parsed = $null
                    try { $parsed = $lines[$index] | ConvertFrom-Json } catch { continue }
                    if ($parsed.kind -eq "seed") { $seedReport = $parsed }
                }
            } else {
                [void]$failures.Add("bit-cli seed did not exit after the client finished")
            }
            if ($seedReport) {
                Write-Step "  seed uploaded $($seedReport.uploaded.bytes) bytes to $($seedReport.peers_served) peer(s), ratio $($seedReport.ratio)"
                if ($seedReport.peers_served -lt 1) {
                    [void]$failures.Add("bit-cli seed served no peer, so the payload did not come from it")
                }
                if ($seedReport.uploaded.bytes -lt $created.total.bytes) {
                    [void]$failures.Add("bit-cli seed accounted for $($seedReport.uploaded.bytes) bytes uploaded, less than the $($created.total.bytes) byte payload")
                }
            } else {
                [void]$failures.Add("bit-cli seed produced no final report")
            }
        }

        # byte-for-byte comparison
        $result = Join-Path $outDir "payload"
        $resultHashes = @{}
        if (Test-Path $result) {
            $resultHashes = Get-TreeHashes $result
            foreach ($relative in $sourceHashes.Keys) {
                if (-not $resultHashes.Contains($relative)) {
                    [void]$failures.Add("missing from the download: $relative")
                } elseif ($resultHashes[$relative].sha256 -ne $sourceHashes[$relative].sha256) {
                    [void]$failures.Add("hash mismatch on ${relative}: expected $($sourceHashes[$relative].sha256), got $($resultHashes[$relative].sha256)")
                }
            }
            foreach ($relative in $resultHashes.Keys) {
                if (-not $sourceHashes.Contains($relative)) {
                    [void]$failures.Add("extra file in the download: $relative")
                }
            }
        } else {
            [void]$failures.Add("no payload directory under $outDir")
        }

        $passed = $failures.Count -eq 0
        Write-Step ("  {0} in {1} ms" -f $(if ($passed) { "round trip matched" } else { "FAILED" }), $download.elapsed_ms)

        [pscustomobject]@{
            name          = $Name
            passed        = $passed
            failures      = $failures
            info_hash     = $created.info_hash
            piece_count   = $created.piece_count
            piece_length  = $created.piece_length.bytes
            total_bytes   = $created.total.bytes
            private       = $created.private
            announce      = $announce
            web_seed      = $webSeed
            seed_report   = $seedReport
            web_seed_bytes = $servedBytes
            steps         = $steps
            source_files  = $sourceHashes
            result_files  = $resultHashes
        }
    } finally {
        Stop-Background
    }
}

# ---------------------------------------------------------------------------
# The magnet case
# ---------------------------------------------------------------------------
#
# Not `Invoke-Case`, because the property is a different one. The three cases
# above prove the payload survives a round trip; this proves the **metainfo**
# does. A magnet carries an info hash and nothing else, so a client that pulls
# the metadata over BEP 9 and writes it back out has produced a file nobody has
# checked against anything: the hash it claims is the hash of the bytes it
# assembled, and the only real test is whether another client opens it.
#
# Nothing here touches the network either. `--no-dht --no-lsd --no-tracker` on
# the seeder and on the resolver leaves a swarm of one loopback address.

function Invoke-MagnetCase {
    Write-Step "case magnet"
    $failures = [System.Collections.ArrayList]@()
    $steps = [System.Collections.ArrayList]@()
    $torrent = Join-Path $Root "magnet.torrent"
    $written = Join-Path $Root "from-magnet.torrent"

    try {
        $create = Invoke-Recorded "magnet-create" $bitCli @(
            "create", "payload", "--piece-length", "32KiB", "--no-creation-date",
            "--output", $torrent, "--force", "--json"
        ) $Root 60
        [void]$steps.Add($create)
        if ($create.exit_code -ne 0) {
            [void]$failures.Add("bit-cli create exited $($create.exit_code)")
            return [pscustomobject]@{ name = "magnet"; passed = $false; failures = $failures; steps = $steps }
        }
        $created = Get-Content $create.stdout -Raw | ConvertFrom-Json

        $uri = Invoke-Recorded "magnet-uri" $bitCli @("magnet", $torrent) $Root 30
        [void]$steps.Add($uri)
        if ($uri.exit_code -ne 0) {
            [void]$failures.Add("bit-cli magnet exited $($uri.exit_code)")
            return [pscustomobject]@{ name = "magnet"; passed = $false; failures = $failures; steps = $steps }
        }
        $magnet = (Get-Content $uri.stdout -Raw).Trim()
        Write-Step "  magnet $magnet"

        # A fixed port, the way the seeding cases above use 51413. This one
        # is its own so the two can never be in flight together.
        $port = 51414
        $seeder = Start-Background "magnet-seed" $bitCli @(
            "seed", $torrent, "--data", $Root, "--port", "$port",
            "--no-dht", "--no-lsd", "--no-tracker", "--seed-time", "120s",
            "--report-interval", "1s", "--jsonl"
        ) $Root

        # Waited on the condition, and the condition is the seeder's own first
        # progress event rather than a socket table. A bound port is not a
        # session ready to answer for this info hash, which is T-221, and
        # `Get-NetTCPConnection` is Windows only, which is what turned
        # `Create round trip (ubuntu-latest)` red the first time this case ran.
        # The seeder emits `progress` once it is live, after the hash check.
        $deadline = (Get-Date).AddSeconds(60)
        $serving = $false
        while ((Get-Date) -lt $deadline) {
            if ((Test-Path $seeder.Stdout) -and
                (Select-String -Path $seeder.Stdout -Pattern '"type":"progress"' -Quiet)) {
                $serving = $true
                break
            }
            if ($seeder.Process.HasExited) { break }
            Start-Sleep -Milliseconds 200
        }
        if (-not $serving) {
            [void]$failures.Add("the seeder never reported serving; see $($seeder.Stderr)")
            return [pscustomobject]@{ name = "magnet"; passed = $false; failures = $failures; steps = $steps }
        }
        Write-Step "  seeder serving on 127.0.0.1:$port"

        $resolve = Invoke-Recorded "magnet-resolve" $bitCli @(
            "magnet", $magnet, "--peer", "127.0.0.1:$port",
            "--no-dht", "--no-lsd", "--no-tracker",
            "--output", $written, "--force"
        ) $Root 90
        [void]$steps.Add($resolve)
        if ($resolve.exit_code -ne 0) {
            [void]$failures.Add("bit-cli magnet --output exited $($resolve.exit_code)")
        }

        # The hash the written file actually has, read back through the tool
        # rather than trusted from the run that wrote it.
        $reread = Invoke-Recorded "magnet-reread" $bitCli @("info", $written, "--json") $Root 30
        [void]$steps.Add($reread)
        $writtenHash = $null
        if ($reread.exit_code -eq 0) {
            $writtenHash = (Get-Content $reread.stdout -Raw | ConvertFrom-Json).info_hash
            if ($writtenHash -ne $created.info_hash) {
                [void]$failures.Add("the written torrent has info hash $writtenHash, the original has $($created.info_hash)")
            }
        } else {
            [void]$failures.Add("bit-cli info on the written torrent exited $($reread.exit_code)")
        }

        # And the point of the case: another client opens it.
        $opened = Invoke-Recorded "magnet-client" $clientPath (Get-ShowArgs $written) $Root 60
        [void]$steps.Add($opened)
        if ($opened.exit_code -ne 0) {
            [void]$failures.Add("$Client could not open the written torrent, exit $($opened.exit_code)")
        } else {
            $said = (Get-Content $opened.stdout -Raw) + (Get-Content $opened.stderr -Raw)
            foreach ($wanted in (Get-ShowExpectation $created.info_hash)) {
                if ($said -notmatch [regex]::Escape($wanted)) {
                    [void]$failures.Add("$Client opened the written torrent and did not report ``$wanted``")
                }
            }
        }

        $passed = $failures.Count -eq 0
        Write-Step ("  {0}" -f $(if ($passed) { "the written torrent round tripped and $Client opened it" } else { "FAILED" }))
        [pscustomobject]@{
            name         = "magnet"
            passed       = $passed
            failures     = $failures
            info_hash    = $created.info_hash
            written_hash = $writtenHash
            magnet       = $magnet
            steps        = $steps
        }
    } finally {
        Stop-Background
    }
}

# ---------------------------------------------------------------------------
# Run
# ---------------------------------------------------------------------------

$startedAt = Get-Timestamp
$cases = @()
$skipped = @()
try {
    $cases += Invoke-Case -Name "v1"
    $cases += Invoke-Case -Name "private" -CreateArgs @("--private")
    # The web seed case asks the second client to resolve a `url-list` and
    # fetch over HTTP with no peer at all. A client that does not implement
    # BEP 19 cannot do that, and running it anyway would record a failure that
    # says nothing about bit-cli. Skipped and named, never silently dropped.
    if ($clientKind -eq 'rqbit') {
        $skipped += [pscustomobject]@{
            name = "webseed"
            why  = "rqbit does not implement BEP 19 web seeding, which is the gap bit-cli exists to fill"
        }
    } else {
        $cases += Invoke-Case -Name "webseed" -WebSeedOnly
    }
    $cases += Invoke-MagnetCase
} finally {
    Stop-Background
}

$passed = @($cases | Where-Object { $_.passed }).Count
$report = [ordered]@{
    schema_version = "1"
    started_at     = $startedAt
    finished_at    = Get-Timestamp
    repository     = $repo
    bit_cli        = $bitCli
    bit_cli_version = (& $bitCli version 2>$null | Select-Object -First 1)
    client         = $clientPath
    client_version = $clientVersion
    os             = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription.Trim()
    payload_bytes  = $sourceBytes
    cases_total    = $cases.Count
    cases_passed   = $passed
    cases          = $cases
    cases_skipped  = $skipped
}
$reportPath = Join-Path $Root "report.json"
$report | ConvertTo-Json -Depth 8 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Output ""
Write-Output "client:  $clientVersion"
Write-Output "report:  $reportPath"
Write-Output ""
Write-Output ("{0,-10} {1,-8} {2,-42} {3}" -f "CASE", "RESULT", "INFO HASH", "DETAIL")
foreach ($case in $cases) {
    # The magnet case matches a torrent rather than a payload, so it says what
    # it matched rather than borrowing a byte count it never took.
    $detail = if (-not $case.passed) { $case.failures -join "; " }
    elseif ($null -ne $case.total_bytes) { "$($case.total_bytes) bytes matched" }
    else { "info hash survived the write, and $Client opened it" }
    Write-Output ("{0,-10} {1,-8} {2,-42} {3}" -f $case.name, $(if ($case.passed) { "pass" } else { "FAIL" }), $case.info_hash, $detail)
}
foreach ($case in $skipped) {
    Write-Output ("{0,-10} {1,-8} {2,-42} {3}" -f $case.name, "skip", "", $case.why)
}

if (-not $Keep) {
    Remove-Item -Recurse -Force $payload -ErrorAction SilentlyContinue
    Get-ChildItem -Directory $Root -Filter "out-*" | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output ""
if ($passed -eq $cases.Count) {
    Write-Output "$passed of $($cases.Count) cases round tripped byte for byte"
    exit 0
}
Exit-With 1 "$($cases.Count - $passed) of $($cases.Count) cases failed"
