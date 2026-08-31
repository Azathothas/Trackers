# Whether --init-timeout bounds a magnet that cannot resolve.
#
# `TODO/cli-surface.md` T-196. `bit-cli download <magnet>` has two paths into
# the session and only one of them was bounded. A file selection that needs a
# file count resolves the metadata first, inside a `--init-timeout`; without
# one, `engine.add` does the same resolution with no bound at all, and the
# `wait_until_initialized_within` that carries the same budget is on the next
# line and never reached. An ordinary invocation takes the unbounded branch.
#
# It cost ten minutes of a measurement to find: a magnet against a seeder that
# could not send its bitfield ran until the harness killed it, and the seeder
# had logged the reason in the first second.
#
# Two cases, and the first is the control:
#
#   selection     `--exclude-file` forces the bounded branch. This one always
#                 worked, and it is here so a failure in the other one cannot
#                 be blamed on the fixture.
#   no_selection  No file selection, which is what an ordinary run looks like.
#                 This is the branch that had no bound.
#
# Both point at a peer that is not there and have the DHT, LSD and trackers
# off, so there is nothing anywhere that could resolve the metadata. Both must
# exit non-zero inside the budget and name `phase: resolving_metadata`, because
# an exit that does not say which phase gave up sends a reader to the wrong
# flag.
#
# Usage:
#   pwsh scripts/check-init-timeout.ps1
#   pwsh scripts/check-init-timeout.ps1 -InitTimeoutSeconds 3 -Slack 6
#
# Exits 0 when both cases hold, 1 when one does not, 2 when it could not run.
#
# See TODO/cli-surface.md, T-196.

[CmdletBinding()]
param(
    [int]$InitTimeoutSeconds = 4,
    # How far past the budget a run may take before it counts as unbounded.
    # Five seconds, which is process start plus shutdown and nothing else: the
    # path this replaced stopped at 10.04 seconds under `librqbit`'s own peer
    # timeout, so anything at or past nine seconds has fallen back to it.
    [int]$Slack = 5,
    [string]$Root = ".tmp/inittimeout",
    [string]$ReportDir = "bench",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Get-Timestamp {
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
}

function Write-Step($message) {
    Write-Host "$(Get-Timestamp) $message"
}

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-init-timeout: $message")
    exit $code
}

$exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
$bitCli = Join-Path $repo "target/$Profile/bit-cli$exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --workspace --bins --examples"
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path
if (-not [System.IO.Path]::IsPathRooted($ReportDir)) { $ReportDir = Join-Path $repo $ReportDir }
New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null

# ---------------------------------------------------------------------------
# A magnet nothing can resolve
# ---------------------------------------------------------------------------
#
# Built from a real torrent so the info hash is well formed, then the torrent
# is never given to the downloader. Two files, because `--exclude-file` has to
# have something to exclude.

$payload = Join-Path $Root "payload"
New-Item -ItemType Directory -Force -Path $payload | Out-Null
[System.IO.File]::WriteAllBytes((Join-Path $payload "a.bin"), [byte[]]::new(65536))
[System.IO.File]::WriteAllBytes((Join-Path $payload "b.bin"), [byte[]]::new(65536))

$torrent = Join-Path $Root "p.torrent"
$create = Start-Process -FilePath $bitCli -ArgumentList @(
    "create", $payload, "--piece-length", "16KiB", "--no-creation-date",
    "--output", $torrent, "--force", "--json"
) -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $Root "create.out") `
    -RedirectStandardError (Join-Path $Root "create.err")
$create.WaitForExit(60000) | Out-Null
if ($create.ExitCode -ne 0) { Exit-With 2 "bit-cli create exited $($create.ExitCode)" }

$infoHash = (Get-Content (Join-Path $Root "create.out") -Raw | ConvertFrom-Json).info_hash
if (-not $infoHash) { Exit-With 2 "bit-cli create did not report an info_hash" }

$magnetOut = Join-Path $Root "magnet.out"
$magnet = Start-Process -FilePath $bitCli -ArgumentList @("magnet", $torrent) `
    -PassThru -NoNewWindow -RedirectStandardOutput $magnetOut `
    -RedirectStandardError (Join-Path $Root "magnet.err")
$magnet.WaitForExit(60000) | Out-Null
if ($magnet.ExitCode -ne 0) { Exit-With 2 "bit-cli magnet exited $($magnet.ExitCode)" }
$link = (Get-Content $magnetOut | Where-Object { $_ -match '^magnet:' } | Select-Object -First 1)
if (-not $link) { Exit-With 2 "bit-cli magnet printed no link" }
Write-Step "magnet built"

# The peer: it completes the BitTorrent handshake and then never says
# anything else.
#
# **Two simpler fixtures were tried first and both measure the wrong thing.**
#
#   A closed port. The connection fails at once, the initial peer list is
#   exhausted, and `librqbit` stops on its own with "input address stream
#   exhausted, no way to discover torrent metainfo" in **two seconds**.
#
#   A listener that accepts and never writes. The handshake read waits out the
#   peer read/write timeout and then the same exhaustion fires, at **ten
#   seconds**. Measured, with the fix stashed.
#
# So the peer here completes the handshake and then says nothing. It is 68
# fixed bytes: a length byte, "BitTorrent protocol", eight reserved bytes with
# BEP 10's bit set so the session treats this peer as able to serve metadata,
# the info hash, and a peer id.
#
# **That is still ten seconds rather than forever, and the ten seconds is the
# measurement this check rests on.** With one address and one peer, `librqbit`
# eventually exhausts the list whatever the peer does; keep-alives were tried
# on top of the handshake and moved the number by nothing. What made T-194's
# run last ten minutes was a tracker and a DHT still handing out addresses, so
# nothing ever exhausted. A fixture cannot reach that without the network.
#
# What this does prove, and it is what the entry asks for: before the fix the
# unbounded branch **ignored a 4 second `--init-timeout`** and stopped at 10.04
# seconds under someone else's timeout with `code: source_resolution`; after
# it, it stops at 4.04 seconds with `code: timeout` and
# `phase: resolving_metadata`. The flag is the bound now, and the default
# `-Slack` is set so a run that falls back to the ten second path fails on the
# clock as well as on the code.
$script:Listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$script:Listener.Start()
$deadPort = ([System.Net.IPEndPoint]$script:Listener.LocalEndpoint).Port

# Accepting has to happen while `bit-cli` is connecting and this script is
# single threaded, so it runs on a runspace of its own.
$accepting = [powershell]::Create()
$accepting.AddScript({
        param($listener, $infoHashHex)
        $hash = [byte[]]::new(20)
        for ($i = 0; $i -lt 20; $i++) {
            $hash[$i] = [Convert]::ToByte($infoHashHex.Substring($i * 2, 2), 16)
        }
        $reply = [System.Collections.Generic.List[byte]]::new()
        $reply.Add(19)
        $reply.AddRange([System.Text.Encoding]::ASCII.GetBytes("BitTorrent protocol"))
        # Reserved. Byte 5 bit 0x10 is BEP 10, the extension protocol, which is
        # how metadata is asked for. Setting it is what makes this peer look
        # like one worth waiting on.
        $reply.AddRange([byte[]]@(0, 0, 0, 0, 0, 0x10, 0, 0))
        $reply.AddRange($hash)
        $reply.AddRange([System.Text.Encoding]::ASCII.GetBytes("-XX0000-stallstall00"))
        $held = @()
        while ($true) {
            try {
                $client = $listener.AcceptTcpClient()
                $held += $client
                $stream = $client.GetStream()
                $their = [byte[]]::new(68)
                $read = 0
                while ($read -lt 68) {
                    $n = $stream.Read($their, $read, 68 - $read)
                    if ($n -le 0) { break }
                    $read += $n
                }
                if ($read -eq 68) {
                    $stream.Write($reply.ToArray(), 0, $reply.Count)
                    $stream.Flush()
                }
            }
            catch { break }
        }
    }).AddArgument($script:Listener).AddArgument($infoHash) | Out-Null
$acceptHandle = $accepting.BeginInvoke()
Write-Step "offering one peer at 127.0.0.1:$deadPort, which handshakes and then stalls"

# ---------------------------------------------------------------------------
# The two cases
# ---------------------------------------------------------------------------

$budget = $InitTimeoutSeconds + $Slack
$rows = @()
$failures = @()

foreach ($case in @(
        [ordered]@{ name = "selection"; extra = @("--exclude-file", "2") },
        [ordered]@{ name = "no_selection"; extra = @() }
    )) {
    $outDir = Join-Path $Root "out-$($case.name)"
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $stdout = Join-Path $Root "$($case.name).json"
    $arguments = @(
        "download", $link, "--dir", $outDir,
        "--init-timeout", "$($InitTimeoutSeconds)s",
        "--no-dht", "--no-lsd", "--no-tracker",
        "--peer", "127.0.0.1:$deadPort", "--port", "0", "--json"
    ) + $case.extra

    Write-Step "$($case.name): downloading with --init-timeout $($InitTimeoutSeconds)s"
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    $run = Start-Process -FilePath $bitCli -ArgumentList $arguments -PassThru -NoNewWindow `
        -RedirectStandardOutput $stdout -RedirectStandardError (Join-Path $Root "$($case.name).err")
    $finished = $run.WaitForExit($budget * 1000)
    $clock.Stop()
    if (-not $finished) {
        Stop-Process -Id $run.Id -Force -ErrorAction SilentlyContinue
        $run.WaitForExit(10000) | Out-Null
    }

    # `--json` prints one pretty-printed document, so it is read whole rather
    # than line by line. The per-torrent entry is where a failure that belongs
    # to one source lands.
    $phase = $null
    $code = $null
    $message = $null
    if (Test-Path $stdout) {
        try {
            $report = Get-Content $stdout -Raw | ConvertFrom-Json
            $torrent = @($report.torrents)[0]
            if ($torrent) {
                $phase = $torrent.phase
                $code = $torrent.code
                $message = $torrent.error
            }
        }
        catch { }
    }

    $rows += [pscustomobject][ordered]@{
        case       = $case.name
        finished   = $finished
        elapsed_s  = [math]::Round($clock.Elapsed.TotalSeconds, 2)
        exit_code  = if ($finished) { $run.ExitCode } else { $null }
        error_code = $code
        phase      = $phase
        message    = $message
    }

    if (-not $finished) {
        $failures += "$($case.name) was still running after $budget s, so --init-timeout bounds nothing on that path"
        continue
    }
    if ($run.ExitCode -eq 0) {
        $failures += "$($case.name) exited 0 against a magnet nothing could resolve"
    }
    if ($clock.Elapsed.TotalSeconds -gt $budget) {
        $failures += ("$($case.name) took {0:N2}s against a {1}s budget" -f $clock.Elapsed.TotalSeconds, $budget)
    }
    if ($code -ne "timeout") {
        $failures += "$($case.name) reported code '$code', expected timeout"
    }
    if ($phase -ne "resolving_metadata") {
        $failures += "$($case.name) reported phase '$phase', expected resolving_metadata"
    }
}

$script:Listener.Stop()
$accepting.EndInvoke($acceptHandle) | Out-Null
$accepting.Dispose()

$verdict = if ($failures.Count -eq 0) { "pass" } else { "fail" }
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
$reportPath = Join-Path $ReportDir "init-timeout-$stamp.json"
[pscustomobject][ordered]@{
    kind           = "init_timeout_bound"
    schema_version = "1"
    generated_at   = Get-Timestamp
    host           = [ordered]@{
        machine = $env:COMPUTERNAME
        os      = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    }
    parameters     = [ordered]@{
        init_timeout_seconds = $InitTimeoutSeconds
        slack_seconds        = $Slack
        dead_port            = $deadPort
        profile              = $Profile
    }
    cases          = @($rows)
    verdict        = $verdict
    failures       = @($failures)
    notes          = @(
        "selection is the control: --exclude-file forces the branch that already had the bound, so a failure in no_selection cannot be blamed on the fixture.",
        "The peer completes the handshake and then sends nothing. A closed port stops the run in two seconds and an accept-and-hold listener in ten, both by exhausting the address list rather than by the flag, so either would have passed before the fix.",
        "The phase is asserted, not only the exit code. An exit that does not say which phase gave up sends a reader to the wrong flag."
    )
} | ConvertTo-Json -Depth 10 | Set-Content -Path $reportPath -Encoding utf8NoBOM

Write-Host ""
$rows | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
Write-Host "report:  $reportPath"
Write-Host "verdict: $verdict"

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
if ($failures.Count -gt 0) {
    Write-Host ""
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-init-timeout: $failure") }
    exit 1
}
exit 0
