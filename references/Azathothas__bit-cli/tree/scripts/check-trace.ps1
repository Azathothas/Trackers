<#
.SYNOPSIS
    What every `--trace` subsystem writes, counted per target.

.DESCRIPTION
    `TODO/cli-surface.md` T-219 is the entry. Eleven subsystems were
    documented and ten of them raised a `tracing` target nothing wrote to, so
    a caller who turned one on got nothing and read that as "there were no
    writes".

    `crates/bit-cli/tests/trace_subsystems.rs` is what holds the fix, and CI
    runs it. This script is the measurement beside it: the same runs, with the
    records counted per target rather than asserted, so the numbers an entry
    quotes can be taken again on any machine.

    It also takes the before number the entry rests on: one run tracing every
    subsystem except `http`, whose stderr was empty before this work.

        pwsh -NoProfile -File scripts/check-trace.ps1
        pwsh -NoProfile -File scripts/check-trace.ps1 -Json bench/trace.json

    It judges one thing and only one: a subsystem that writes on none of the
    targets it raises is a failure, because that is the defect. Everything else
    is reported.
#>

[CmdletBinding()]
param(
    [string]$Root = ".tmp/trace-subsystems",
    [string]$PayloadSize = "2MiB",
    [string]$PieceLength = "256KiB",
    [string]$Json,
    [int]$TimeoutSeconds = 120
)

$ErrorActionPreference = 'Stop'

$exe = if ($IsWindows -or $env:OS -eq 'Windows_NT') { ".exe" } else { "" }
$bitCli = "target/release/bit-cli$exe"
if (-not (Test-Path $bitCli)) {
    throw "no binary at $bitCli. Build one: cargo build --release --bins"
}
$bitCli = (Resolve-Path $bitCli).Path

if (Test-Path $Root) { Remove-Item -Recurse -Force $Root }
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Root = (Resolve-Path $Root).Path

function Write-Step([string]$text) {
    Write-Output ("{0}Z check-trace: {1}" -f (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fff"), $text)
}

$payloadBytes = 2MB
if ($PayloadSize -match '^(\d+)(MiB|KiB|GiB)?$') {
    $n = [int64]$Matches[1]
    $payloadBytes = switch ($Matches[2]) {
        'KiB' { $n * 1KB }
        'GiB' { $n * 1GB }
        default { $n * 1MB }
    }
}

$src = Join-Path $Root "src"
New-Item -ItemType Directory -Force -Path $src | Out-Null
$bytes = [byte[]]::new($payloadBytes)
[int64]$state = 987
for ($i = 0; $i -lt $bytes.Length; $i++) {
    $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
    $bytes[$i] = [byte](($state -shr 16) -band 0xFF)
}
[System.IO.File]::WriteAllBytes((Join-Path $src "movie.bin"), $bytes)
Write-Step "built a $([math]::Round($payloadBytes / 1MB, 2)) MiB payload"

$torrent = Join-Path $Root "fixture.torrent"
& $bitCli create $src --name payload --piece-length $PieceLength --no-creation-date --output $torrent --force | Out-Null
if ($LASTEXITCODE -ne 0) { throw "bit-cli create exited $LASTEXITCODE" }

# A `file:` source takes the local branch of the fetcher and everything above
# it is the same code an HTTP source runs, so the whole path is exercised with
# no server. See `docs/trace.md`.
$seedUrl = "file:///" + ($src.Replace('\', '/').Replace(' ', '%20')) + "/"
Write-Step "web seed at $seedUrl"

# A loopback server that answers every request 503. `retry` needs a status the
# classifier calls transient so the ladder runs, and `tracker` needs an
# exchange to report on.
$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
$stubPort = $listener.LocalEndpoint.Port
$stub = Start-ThreadJob -ScriptBlock {
    param($l)
    while ($true) {
        try {
            $client = $l.AcceptTcpClient()
            $stream = $client.GetStream()
            $stream.ReadTimeout = 2000
            $buffer = [byte[]]::new(4096)
            try { $null = $stream.Read($buffer, 0, $buffer.Length) } catch {}
            $reply = [System.Text.Encoding]::ASCII.GetBytes("HTTP/1.1 503 Service Unavailable`r`nContent-Length: 0`r`nConnection: close`r`n`r`n")
            $stream.Write($reply, 0, $reply.Length)
            $stream.Flush()
            $client.Close()
        }
        catch { break }
    }
} -ArgumentList $listener
Write-Step "503 stub on 127.0.0.1:$stubPort"

# One run, returning the record count per target. The exit code is reported
# and not judged: two of the cases below are failures on purpose, and what is
# measured is what the run said it was doing.
function Invoke-Traced([string]$label, [string[]]$argv) {
    $errFile = Join-Path $Root "$label.err"
    $outFile = Join-Path $Root "$label.out"
    $process = Start-Process -FilePath $bitCli -ArgumentList $argv -NoNewWindow -PassThru `
        -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
        $process.Kill()
        $process.WaitForExit()
        throw "$label did not finish within $TimeoutSeconds seconds"
    }
    $counts = @{}
    $lines = 0
    foreach ($line in (Get-Content $errFile -ErrorAction SilentlyContinue)) {
        $lines++
        if (-not $line.StartsWith('{')) { continue }
        try { $doc = $line | ConvertFrom-Json } catch { continue }
        if ($doc.target) {
            $counts[$doc.target] = 1 + ($counts[$doc.target] ?? 0)
        }
    }
    [pscustomobject]@{
        label       = $label
        exit_code   = $process.ExitCode
        stderr_lines = $lines
        targets     = $counts
    }
}

$download = @(
    "download", $torrent,
    "--web-seed", $seedUrl,
    "--web-seed-mode", "prefix",
    "--no-torrent-web-seed",
    "--web-seed-only",
    "--port", "0",
    "--json"
)

# Subsystem, the targets it raises, and the run that puts it in the path.
# The targets are the ones `crates/bit-cli/src/logging.rs` carries; a copy
# rather than a read, because this script runs against a built binary and the
# test beside it is what holds the two together.
$cases = @(
    @{ name = "peer"; targets = @("bit_cli::peer", "librqbit::peer_connection"); argv = $download + @("--dir", (Join-Path $Root "d-peer")) }
    @{ name = "handshake"; targets = @("bit_cli::handshake", "librqbit::handshake"); argv = $download + @("--dir", (Join-Path $Root "d-handshake")) }
    @{ name = "tracker"; targets = @("bit_cli::tracker", "librqbit_tracker_comms"); argv = @("trackers", $torrent, "--tracker", "http://127.0.0.1:$stubPort/announce", "--replace-trackers", "--json") }
    @{ name = "dht"; targets = @("bit_cli::dht", "librqbit_dht"); argv = $download + @("--dir", (Join-Path $Root "d-dht")) }
    @{ name = "http"; targets = @("bit_cli::http"); argv = $download + @("--dir", (Join-Path $Root "d-http")) }
    @{ name = "piece"; targets = @("bit_cli::piece", "librqbit::piece"); argv = $download + @("--dir", (Join-Path $Root "d-piece")) }
    @{ name = "picker"; targets = @("bit_cli::picker", "librqbit::picker"); argv = $download + @("--dir", (Join-Path $Root "d-picker"), "--piece-selector", "in-order") }
    @{ name = "disk"; targets = @("bit_cli::disk"); argv = $download + @("--dir", (Join-Path $Root "d-disk")) }
    @{ name = "ratelimit"; targets = @("bit_cli::ratelimit"); argv = $download + @("--dir", (Join-Path $Root "d-ratelimit"), "--web-seed-speed-limit", "2MiB") }
    @{ name = "retry"; targets = @("bit_cli::retry"); argv = @(
            "download", $torrent,
            "--web-seed", "http://127.0.0.1:$stubPort/payload/",
            "--web-seed-mode", "prefix",
            "--no-torrent-web-seed",
            "--web-seed-only",
            "--web-seed-retries", "1",
            "--port", "0",
            "--stop-after", "8s",
            "--dir", (Join-Path $Root "d-retry"),
            "--json"
        )
    }
    @{ name = "config"; targets = @("bit_cli::config"); argv = @("config", "show", "--json") }
)

$rows = @()
$silent = @()
try {
    foreach ($case in $cases) {
        $argv = @("--trace", $case.name, "--log-format", "json") + $case.argv
        $run = Invoke-Traced $case.name $argv
        $hits = [ordered]@{}
        foreach ($target in ($run.targets.Keys | Sort-Object)) {
            foreach ($want in $case.targets) {
                if ($target -eq $want -or $target.StartsWith("$want::")) {
                    $hits[$target] = $run.targets[$target]
                }
            }
        }
        if ($hits.Count -eq 0) { $silent += $case.name }
        $rows += [pscustomobject]@{
            subsystem = $case.name
            raises    = $case.targets
            exit_code = $run.exit_code
            records   = [int](($hits.Values | Measure-Object -Sum).Sum)
            per_target = $hits
            other_targets = @($run.targets.Keys | Where-Object { -not $hits.Contains($_) } | Sort-Object)
        }
        Write-Output ("{0,-11} exit {1,-4} {2,6} record(s)  {3}" -f $case.name, $run.exit_code, $rows[-1].records, (($hits.Keys | ForEach-Object { "$_=$($hits[$_])" }) -join ", "))
    }

    # The before number, taken the way T-219 took it: every name except
    # `http`, which was the only one that ever matched anything.
    $ten = "peer,handshake,tracker,dht,piece,picker,disk,ratelimit,retry,config"
    $tenRun = Invoke-Traced "ten" (@("--trace", $ten, "--log-format", "json") + $download + @("--dir", (Join-Path $Root "d-ten")))
    $httpRun = Invoke-Traced "http-only" (@("--trace", "http", "--log-format", "json") + $download + @("--dir", (Join-Path $Root "d-http-only")))
    $plainRun = Invoke-Traced "untraced" (@("--log-format", "json") + $download + @("--dir", (Join-Path $Root "d-untraced")))
}
finally {
    $listener.Stop()
    $stub | Stop-Job -PassThru | Remove-Job -Force | Out-Null
}

Write-Output ""
Write-Output ("--trace {0}" -f $ten)
Write-Output ("    {0} stderr line(s)" -f $tenRun.stderr_lines)
Write-Output ("--trace http")
Write-Output ("    {0} stderr line(s)" -f $httpRun.stderr_lines)
Write-Output ("no --trace")
Write-Output ("    {0} stderr line(s)" -f $plainRun.stderr_lines)
Write-Output ""

if ($Json) {
    $report = [ordered]@{
        generated_at  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
        payload_bytes = $payloadBytes
        piece_length  = $PieceLength
        subsystems    = $rows
        all_but_http  = [ordered]@{
            traced       = $ten
            stderr_lines = $tenRun.stderr_lines
            per_target   = $tenRun.targets
        }
        http_only     = [ordered]@{ stderr_lines = $httpRun.stderr_lines; per_target = $httpRun.targets }
        untraced      = [ordered]@{ stderr_lines = $plainRun.stderr_lines }
        silent        = $silent
    }
    $report | ConvertTo-Json -Depth 8 | Set-Content -Path $Json -Encoding utf8
    Write-Output "wrote $Json"
}

if ($silent.Count -gt 0) {
    Write-Output ("{0} subsystem(s) wrote on none of the targets they raise: {1}" -f $silent.Count, ($silent -join ", "))
    exit 1
}
Write-Output ("every one of the {0} documented subsystems wrote on a target it raises" -f $rows.Count)
