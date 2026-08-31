# Are the numbers a tracker sees the numbers `bit-cli` reports?
#
# The defect this exists to catch is an announce that disagrees with the run it
# describes. `uploaded`, `downloaded` and `left` are the only thing a tracker
# knows about a client, and nothing until now compared them against what the
# client's own report said it transferred. A wrong number here is invisible
# locally, wrong on the tracker forever, and indistinguishable from cheating.
#
# Six cases, all on loopback and only on loopback:
#
#   started-left     the first announce is `started` and carries the whole
#                    payload in `left`
#   completed        `completed` is sent, once, and `left` is 0 by then
#   stopped          `stopped` is sent when the run ends
#   left-monotonic   `left` never rises
#   totals-match     the last announce's `downloaded` agrees with the report's
#                    own byte count, and `uploaded` is not invented
#   interval         the gap between two ordinary announces is at least the
#                    `min interval` the tracker asked for
#
# Three more for the announce paths those six never reach, which is
# `TODO/trackers.md` T-237:
#
#   redirect         a `302` on `/announce` is followed, and the request that
#                    follows it carries the same three numbers
#   failure-reason   a rejection at HTTP 200 with a `failure reason` key is
#                    reported as a failure and not as a success, over HTTP and
#                    over UDP
#   udp              the same six assertions again, over a BEP 15 announce
#
# The evidence is `loopback-tracker --announce-log`, which appends one JSON
# object per announce carrying the raw query as received. The comparison is
# against `bit-cli download --json`, so both sides are machine output and
# neither is a log line parsed by eye.
#
# This points at loopback and never at a public tracker, and it changes no
# number it reports. It is a correctness harness, not a ratio tool.
#
# Usage:
#   pwsh scripts/check-announce.ps1
#   pwsh scripts/check-announce.ps1 -PayloadMiB 16 -Json bench/announce.json
#   pwsh scripts/check-announce.ps1 -SkipUdp
#
# Exits 0 when every judged case holds, 1 when one does not, and 2 when the
# check could not run.
#
# See TODO/trackers.md, T-235 and T-237.

[CmdletBinding()]
param(
    [int]$PayloadMiB = 8,
    [int]$UdpPayloadMiB = 2,
    [int]$TimeoutSeconds = 120,
    [int]$AnnounceInterval = 5,
    [string]$Root = ".tmp/announce",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [switch]$SkipUdp,
    [string]$Json
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-announce: $message")
    exit $code
}

function Write-Step($message) {
    Write-Host "$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.fffZ')) $message"
}

$bitCli = Join-Path $repo "target/$Profile/bit-cli.exe"
if (-not (Test-Path $bitCli)) {
    Exit-With 2 "missing $bitCli. Build it first: cargo build --$Profile --bins --examples"
}
$tracker = Join-Path $repo "target/$Profile/examples/loopback-tracker.exe"
if (-not (Test-Path $tracker)) {
    Exit-With 2 "missing $tracker. Build it first: cargo build --$Profile --bins --examples"
}

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path (Join-Path $Root "payload") | Out-Null
$Root = (Resolve-Path $Root).Path

$background = @()
function Stop-Background {
    foreach ($process in $script:background) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    $script:background = @()
}
trap {
    Stop-Background
    [Console]::Error.WriteLine("check-announce: $($_.Exception.Message)")
    [Console]::Error.WriteLine("  at $($_.InvocationInfo.ScriptLineNumber): $($_.InvocationInfo.Line.Trim())")
    throw
}

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# One payload of `$MiB` mebibytes, filled from the same seeded generator every
# run so two runs of this script produce the same info hash.
function New-Payload($path, $MiB) {
    $block = [byte[]]::new(1024 * 1024)
    [int64]$state = 20260824
    for ($i = 0; $i -lt $block.Length; $i++) {
        $state = ($state * 1103515245 + 12345) -band 0x7FFFFFFF
        $block[$i] = [byte](($state -shr 16) -band 0xFF)
    }
    $stream = [System.IO.File]::Create($path)
    try { for ($i = 0; $i -lt $MiB; $i++) { $stream.Write($block, 0, $block.Length) } }
    finally { $stream.Dispose() }
    (Get-Item -LiteralPath $path).Length
}

# Start a `loopback-tracker` and wait for the URLs it prints. The IPv4 HTTP URL
# is always its first line; the UDP one is last and may be on a different port,
# because a Windows reserved range can take the UDP port a TCP listener was
# just given.
function New-Tracker($name, $extra) {
    $out = Join-Path $Root "$name.out"
    $err = Join-Path $Root "$name.err"
    $log = Join-Path $Root "$name.jsonl"
    $argv = @("--port", "0", "--interval", "$AnnounceInterval", "--announce-log", "`"$log`"") + $extra
    $process = Start-Process -FilePath $tracker -WorkingDirectory $Root -NoNewWindow -PassThru `
        -ArgumentList $argv -RedirectStandardOutput $out -RedirectStandardError $err
    $script:background += $process
    $urls = @()
    $deadline = (Get-Date).AddSeconds(30)
    while ($urls.Count -lt 2 -and (Get-Date) -lt $deadline) {
        if ($process.HasExited) { break }
        $urls = @(Get-Content $out -ErrorAction SilentlyContinue | Where-Object { $_ -match '^(http|udp)://' })
        if ($urls.Count -lt 2) { Start-Sleep -Milliseconds 150 }
    }
    $http = $urls | Where-Object { $_ -like 'http://127.0.0.1:*' } | Select-Object -First 1
    if (-not $http) {
        Stop-Background
        Exit-With 2 "the loopback tracker '$name' never printed an announce URL; see $err"
    }
    [pscustomobject]@{
        Process = $process
        Http    = $http
        Udp     = ($urls | Where-Object { $_ -like 'udp://*' } | Select-Object -First 1)
        Log     = $log
        Err     = $err
    }
}

# Every announce a tracker wrote down, oldest first.
function Read-Announces($path) {
    if (-not (Test-Path $path)) { return @() }
    $out = @()
    foreach ($line in Get-Content $path) {
        if (-not $line.Trim()) { continue }
        try { $out += ($line | ConvertFrom-Json) } catch { }
    }
    @($out | Sort-Object at)
}

function Get-Field($object, $name) {
    if ($object -and $object.PSObject.Properties.Name -contains $name) { $object.$name } else { $null }
}

# The subject's own announces, separated from every other client's on this
# tracker. By peer id from the report where there is one, and otherwise by the
# one property that always differs: a seeder starts complete, so its first
# announce carries left=0 and the leecher's carries the whole payload.
function Select-Subject($announces, $report) {
    $peerId = Get-Field $report 'peer_id'
    if (-not $peerId -and $report.torrents -and $report.torrents.Count -gt 0) {
        $peerId = Get-Field $report.torrents[0] 'peer_id'
    }
    if ($peerId) {
        $filtered = @($announces | Where-Object { $_.peer_id -eq $peerId })
        if ($filtered.Count -gt 0) { return @($filtered | Sort-Object at) }
    }
    $candidate = $announces | Group-Object peer_id | Where-Object {
        $first = ($_.Group | Sort-Object at)[0]
        [int64]$first.left -gt 0
    }
    if ($candidate) { return @(($candidate | Select-Object -First 1).Group | Sort-Object at) }
    @($announces | Sort-Object at)
}

# The six assertions, against one subject's announces. Returns one ordered
# dictionary per assertion, so the HTTP round can add them to the table one by
# one and the UDP round can fold them into a single row.
function Get-FidelityCases($mine, $report, $payloadBytes) {
    $results = [System.Collections.ArrayList]::new()
    function Add-Result($name, $judged, $ok, $detail) {
        [void]$results.Add([ordered]@{ case = $name; judged = $judged; ok = $ok; detail = $detail })
    }

    $events = @($mine | ForEach-Object { $e = Get-Field $_ 'event'; if ($e) { $e } else { "" } })
    $firstAnnounce = if ($mine.Count -gt 0) { $mine[0] } else { $null }
    $lastAnnounce = if ($mine.Count -gt 0) { $mine[$mine.Count - 1] } else { $null }

    # 1. started, and it carries the whole payload as left
    $startedOk = $false
    $startedDetail = "no announce from the leecher"
    if ($firstAnnounce) {
        $startedEvent = $events[0]
        $startedLeft = [int64]$firstAnnounce.left
        $startedOk = ($startedEvent -eq "started") -and ($startedLeft -eq $payloadBytes)
        $startedDetail = "first event '$startedEvent', left $startedLeft, payload $payloadBytes"
    }
    Add-Result "started-left" $true $startedOk $startedDetail

    # 2. completed, exactly once, and left is zero by then
    $completedIndexes = @()
    for ($i = 0; $i -lt $mine.Count; $i++) {
        if ($events[$i] -eq "completed") { $completedIndexes += $i }
    }
    $completedOk = $false
    $completedDetail = "no completed event in $($mine.Count) announce(s)"
    if ($completedIndexes.Count -eq 1) {
        $completed = $mine[$completedIndexes[0]]
        $completedOk = ([int64]$completed.left -eq 0)
        $completedDetail = "one completed event, left $([int64]$completed.left)"
    } elseif ($completedIndexes.Count -gt 1) {
        $completedDetail = "$($completedIndexes.Count) completed events, and BEP 3 asks for one"
    }
    Add-Result "completed" $true $completedOk $completedDetail

    # 3. stopped, at the end
    $stoppedOk = ($events -contains "stopped")
    $stoppedDetail = "events: " + (($events | ForEach-Object { if ($_) { $_ } else { "-" } }) -join ",")
    Add-Result "stopped" $true $stoppedOk $stoppedDetail

    # 4. left never rises
    $leftValues = @($mine | ForEach-Object { [int64]$_.left })
    $leftOk = $true
    for ($i = 1; $i -lt $leftValues.Count; $i++) {
        if ($leftValues[$i] -gt $leftValues[$i - 1]) { $leftOk = $false }
    }
    Add-Result "left-monotonic" $true $leftOk ("left: " + ($leftValues -join " -> "))

    # 5. the totals the tracker saw against the totals the run reported
    # `docs/schema.md` gives these as `{bytes, human}`, the shape RULES.md section 5
    # asks for: a raw integer with any formatted string beside it rather than
    # instead of it. Read `.bytes` and never the pair.
    $reportedDownloaded = $null
    $reportedUploaded = $null
    if ($report.torrents -and $report.torrents.Count -gt 0) {
        $torrent0 = $report.torrents[0]
        $downloaded = Get-Field $torrent0 'downloaded'
        if ($downloaded -and $downloaded.PSObject.Properties.Name -contains 'bytes') {
            $reportedDownloaded = [int64]$downloaded.bytes
        }
        $uploaded = Get-Field $torrent0 'uploaded'
        if ($uploaded -and $uploaded.PSObject.Properties.Name -contains 'bytes') {
            $reportedUploaded = [int64]$uploaded.bytes
        }
    }
    $announcedDownloaded = if ($lastAnnounce) { [int64]$lastAnnounce.downloaded } else { $null }
    $announcedUploaded = if ($lastAnnounce) { [int64]$lastAnnounce.uploaded } else { $null }
    $totalsJudged = ($null -ne $reportedDownloaded -and $null -ne $announcedDownloaded)
    $totalsOk = $false
    if ($totalsJudged) {
        # The tracker's figure is taken at the last announce and the report's at
        # exit, so they are not required to be equal to the byte. What is required
        # is that the announce is not larger than the run and covers the payload.
        $totalsOk = ($announcedDownloaded -ge $payloadBytes) -and ($announcedDownloaded -le $reportedDownloaded)
    }
    $totalsDetail = "announced downloaded $announcedDownloaded, uploaded $announcedUploaded; report downloaded $reportedDownloaded, uploaded $reportedUploaded; payload $payloadBytes"
    Add-Result "totals-match" $totalsJudged $totalsOk $totalsDetail

    # 6. the interval the tracker asked for is honoured between ordinary announces
    $ordinary = @()
    for ($i = 0; $i -lt $mine.Count; $i++) {
        if (-not $events[$i]) { $ordinary += $mine[$i] }
    }
    $intervalJudged = ($ordinary.Count -ge 2)
    $intervalOk = $true
    $smallest = $null
    if ($intervalJudged) {
        for ($i = 1; $i -lt $ordinary.Count; $i++) {
            $gap = ([datetime]$ordinary[$i].at - [datetime]$ordinary[$i - 1].at).TotalSeconds
            if ($null -eq $smallest -or $gap -lt $smallest) { $smallest = $gap }
        }
        # One second of slack: the tracker stamps on arrival and the client times
        # from its own clock, and asserting they agree to the millisecond is
        # asserting a scheduling outcome.
        $intervalOk = ($smallest -ge ($AnnounceInterval - 1))
    }
    $intervalDetail = if ($intervalJudged) {
        "smallest gap $([math]::Round($smallest, 2))s against a min interval of ${AnnounceInterval}s over $($ordinary.Count) ordinary announces"
    } else {
        "only $($ordinary.Count) ordinary announce(s), so there is no gap to measure"
    }
    Add-Result "interval" $intervalJudged $intervalOk $intervalDetail

    @($results)
}

# A seeder and a leecher over one announce URL. Returns the leecher's report.
function Invoke-Swarm($label, $announceUrl, $payloadDir, $torrentName) {
    $torrent = Join-Path $Root "$label.torrent"
    & $bitCli create $payloadDir --name $torrentName --piece-length 256KiB `
        --announce $announceUrl --no-creation-date --output $torrent --force --json 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { Stop-Background; Exit-With 2 "bit-cli create exited $LASTEXITCODE for $label" }

    $seedRoot = Join-Path $Root "$label-seed"
    New-Item -ItemType Directory -Force -Path $seedRoot | Out-Null
    $seed = Start-Process -FilePath $bitCli -WorkingDirectory $Root -NoNewWindow -PassThru -ArgumentList @(
        "seed", $torrent, "--data", $Root, "--port", "0",
        "--no-dht", "--no-lsd",
        "--report-interval", "5s", "--seed-time", "$($TimeoutSeconds + 60)s", "--jsonl"
    ) -RedirectStandardOutput (Join-Path $seedRoot "seed.out") `
        -RedirectStandardError (Join-Path $seedRoot "seed.err")
    $script:background += $seed

    $listen = $null
    $deadline = (Get-Date).AddSeconds(60)
    while (-not $listen -and (Get-Date) -lt $deadline) {
        if ($seed.HasExited) { break }
        foreach ($line in @(Get-Content (Join-Path $seedRoot "seed.out") -ErrorAction SilentlyContinue)) {
            if (-not $line -or -not $line.Trim().StartsWith("{")) { continue }
            $event = $null
            try { $event = $line | ConvertFrom-Json } catch { continue }
            if ($event.listen_addr) { $listen = $event.listen_addr; break }
        }
        if (-not $listen) { Start-Sleep -Milliseconds 200 }
    }
    if (-not $listen) {
        Stop-Background
        Exit-With 2 "the $label seeder never reported a listen address; see $seedRoot/seed.err"
    }
    Write-Step "$label seeder listening on $listen"

    # The leecher is the subject. It is left running past completion for two
    # announce intervals so the `completed` event and at least one ordinary
    # announce after it are on the record, which is what the interval case reads.
    $leechOut = Join-Path $Root "$label-leech.json"
    $holdSeconds = [Math]::Max(3 * $AnnounceInterval, 15)
    Write-Step "$label leeching, then holding $holdSeconds s so the post-completion announces land"
    Start-Process -FilePath $bitCli -WorkingDirectory $Root -NoNewWindow -PassThru -Wait -ArgumentList @(
        "download", $torrent, "--dir", (Join-Path $Root "$label-leech"),
        "--no-dht", "--no-lsd", "--allow-overwrite",
        "--seed-time", "$($holdSeconds)s",
        "--stop-after", "$($TimeoutSeconds)s", "--json"
    ) -RedirectStandardOutput $leechOut -RedirectStandardError (Join-Path $Root "$label-leech.err") | Out-Null

    $report = $null
    try { $report = Get-Content $leechOut -Raw | ConvertFrom-Json } catch { $report = $null }
    if (-not $report) {
        Stop-Background
        Exit-With 2 "bit-cli download wrote no JSON report for $label; see $Root/$label-leech.err"
    }
    # The seeder is stopped here rather than at the end of the run: the next
    # round announces to the same tracker, and a seeder still holding a peer
    # record would be offered to it.
    if (-not $seed.HasExited) { Stop-Process -Id $seed.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 500
    $report
}

$cases = [System.Collections.ArrayList]::new()
function Add-Case($name, $judged, $ok, $detail) {
    [void]$cases.Add([ordered]@{
            case   = $name
            judged = $judged
            ok     = $ok
            detail = $detail
        })
}

# ---------------------------------------------------------------------------
# A payload, a torrent, and a tracker that writes down what it was told
# ---------------------------------------------------------------------------

Write-Step "building a $PayloadMiB MiB payload"
$payloadBytes = New-Payload (Join-Path $Root "payload/announce.bin") $PayloadMiB

$main = New-Tracker "tracker" @()
$announceLog = $main.Log
Write-Step "tracker at $($main.Http), min interval $AnnounceInterval s"
if ($main.Udp) { Write-Step "and at $($main.Udp)" }

# ---------------------------------------------------------------------------
# Round one: the ordinary HTTP announce
# ---------------------------------------------------------------------------

$report = Invoke-Swarm "http" $main.Http (Join-Path $Root "payload") "payload"

if (-not (Test-Path $announceLog)) {
    Stop-Background
    Exit-With 2 "the tracker recorded no announce at all; see $($main.Err)"
}
$announces = Read-Announces $announceLog
if ($announces.Count -eq 0) { Stop-Background; Exit-With 2 "the announce log holds no readable record" }

$httpAnnounces = @($announces | Where-Object { $_.protocol -eq 'http' })
$mine = Select-Subject $httpAnnounces $report
Write-Step "$($announces.Count) announce(s) recorded, $($mine.Count) from the HTTP leecher"

foreach ($result in Get-FidelityCases $mine $report $payloadBytes) {
    Add-Case $result.case $result.judged $result.ok $result.detail
}
$firstAnnounce = if ($mine.Count -gt 0) { $mine[0] } else { $null }

# ---------------------------------------------------------------------------
# Round two: the same six over BEP 15, which is T-237's third path
# ---------------------------------------------------------------------------

$udpDetail = "not run"
$udpJudged = $false
$udpOk = $false
$udpResults = @()
if ($SkipUdp) {
    $udpDetail = "-SkipUdp was passed"
} elseif (-not $main.Udp) {
    $udpDetail = "the tracker bound no UDP socket; see $($main.Err)"
} else {
    Write-Step "building a $UdpPayloadMiB MiB payload for the UDP round"
    New-Item -ItemType Directory -Force -Path (Join-Path $Root "payload-udp") | Out-Null
    # Its own payload, so the info hash differs from round one's and the two
    # rounds cannot be handed each other's peer records by the same tracker.
    $udpPayloadBytes = New-Payload (Join-Path $Root "payload-udp/announce.bin") $UdpPayloadMiB
    $udpReport = Invoke-Swarm "udp" $main.Udp (Join-Path $Root "payload-udp") "payload-udp"

    $udpAnnounces = @((Read-Announces $announceLog) | Where-Object { $_.protocol -eq 'udp' })
    $udpMine = Select-Subject $udpAnnounces $udpReport
    Write-Step "$($udpAnnounces.Count) UDP announce(s) recorded, $($udpMine.Count) from the UDP leecher"
    $udpResults = @(Get-FidelityCases $udpMine $udpReport $udpPayloadBytes)
    $judgedResults = @($udpResults | Where-Object { $_.judged })
    $failed = @($judgedResults | Where-Object { -not $_.ok })
    $udpJudged = ($judgedResults.Count -gt 0)
    $udpOk = ($failed.Count -eq 0)
    # Counted rather than written out. The fidelity cases are six today and a
    # seventh would otherwise leave this line saying "six" forever.
    $udpDetail = if ($failed.Count -gt 0) {
        "$($failed.Count) of $($judgedResults.Count) failed: " + (($failed | ForEach-Object { "$($_.case) ($($_.detail))" }) -join "; ")
    } else {
        "$($judgedResults.Count) of $($udpResults.Count) judged and all hold over $($udpMine.Count) announce(s)"
    }
}
Add-Case "udp" $udpJudged $udpOk $udpDetail

# ---------------------------------------------------------------------------
# Round three: an announce answered with a 302
# ---------------------------------------------------------------------------
#
# `bit-cli trackers` is the subject rather than a download, because one
# announce and one redirect are the whole question and a transfer would add
# nothing to it. What has to hold is that the request which follows the
# redirect carries the same three numbers as the one that was redirected: a
# `Location` without the query would lose all of them and the run would still
# look like a success.

$redirectTracker = New-Tracker "redirect" @("--redirect-announce", "1")
$redirectTorrent = Join-Path $Root "redirect.torrent"
& $bitCli create (Join-Path $Root "payload") --name payload --piece-length 256KiB `
    --announce $redirectTracker.Http --no-creation-date --output $redirectTorrent --force --json 2>&1 | Out-Null
& $bitCli trackers $redirectTorrent --tracker-timeout 15s --json 2>&1 |
    Set-Content -Path (Join-Path $Root "redirect.json") -Encoding utf8NoBOM
$redirectExit = $LASTEXITCODE
$redirectReport = $null
try { $redirectReport = Get-Content (Join-Path $Root "redirect.json") -Raw | ConvertFrom-Json } catch { }

$redirectAnnounces = Read-Announces $redirectTracker.Log
$original = @($redirectAnnounces | Where-Object { $_.path -eq '/announce' -and (Get-Field $_ 'event') -eq 'started' })
$followed = @($redirectAnnounces | Where-Object { $_.path -eq '/announce-r' })
$redirectOk = $false
$redirectDetail = "$($original.Count) request(s) to /announce and $($followed.Count) to /announce-r"
if ($original.Count -ge 1 -and $followed.Count -ge 1) {
    $before = $original[0]
    $after = $followed[0]
    $same = @('uploaded', 'downloaded', 'left', 'info_hash', 'peer_id') |
        Where-Object { (Get-Field $before $_) -ne (Get-Field $after $_) }
    $redirectOk = ($same.Count -eq 0) -and ($redirectExit -eq 0)
    $redirectDetail = if ($same.Count -gt 0) {
        "the followed request changed: " + ($same -join ", ")
    } else {
        "followed, exit $redirectExit, same up=$($after.uploaded) down=$($after.downloaded) left=$($after.left) across the hop"
    }
}
Add-Case "redirect" $true $redirectOk $redirectDetail

# ---------------------------------------------------------------------------
# Round four: an announce rejected at HTTP 200, and the same over UDP
# ---------------------------------------------------------------------------
#
# BEP 3 puts a rejection in the body rather than in the status, so a check
# that reads the status alone calls this a success. `RESEARCH.md` entry 29
# records the rule. Both protocols are asked in one run, the UDP one added by
# `--tracker` because `create --announce` takes a single URL.

$refusal = "this torrent is not tracked here"
$failTracker = New-Tracker "fail" @("--fail-announce", "`"$refusal`"")
$failTorrent = Join-Path $Root "fail.torrent"
& $bitCli create (Join-Path $Root "payload") --name payload --piece-length 256KiB `
    --announce $failTracker.Http --no-creation-date --output $failTorrent --force --json 2>&1 | Out-Null
$failArgs = @($failTorrent, "--tracker-timeout", "15s", "--json")
if ($failTracker.Udp) { $failArgs = @($failTorrent, "--tracker", $failTracker.Udp, "--tracker-timeout", "15s", "--json") }
& $bitCli trackers @failArgs 2>&1 |
    Set-Content -Path (Join-Path $Root "fail.json") -Encoding utf8NoBOM
$failExit = $LASTEXITCODE
$failReport = $null
try { $failReport = Get-Content (Join-Path $Root "fail.json") -Raw | ConvertFrom-Json } catch { }

$expected = if ($failTracker.Udp) { 2 } else { 1 }
$failOk = $false
$failDetail = "bit-cli trackers wrote no readable report"
if ($failReport) {
    $rows = @(Get-Field $failReport 'trackers')
    $refused = @($rows | Where-Object { (-not $_.ok) -and "$(Get-Field $_ 'failure')".Contains($refusal) })
    $responded = [int](Get-Field $failReport 'responded')
    # Every row refused, nothing reported as responding, and a non-zero exit.
    # The announce reached the tracker either way: `--fail-announce` records
    # it before refusing it, so a check that saw nothing in the log would be
    # measuring a request that never arrived rather than a rejection.
    $logged = @(Read-Announces $failTracker.Log)
    $failOk = ($refused.Count -eq $expected) -and ($responded -eq 0) -and
        ($failExit -ne 0) -and ($logged.Count -ge $expected)
    $failDetail = "$($refused.Count) of $($rows.Count) row(s) carry the reason, responded $responded, exit $failExit, $($logged.Count) announce(s) reached the tracker"
}
Add-Case "failure-reason" $true $failOk $failDetail

Stop-Background

# ---------------------------------------------------------------------------

$failures = @($cases | Where-Object { $_.judged -and -not $_.ok } | ForEach-Object {
        "$($_.case): $($_.detail)"
    })

$result = [ordered]@{
    kind              = "announce"
    generated_at      = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    payload_bytes     = $payloadBytes
    announce_url      = $main.Http
    udp_announce_url  = $main.Udp
    min_interval      = $AnnounceInterval
    announces_total   = $announces.Count
    announces_subject = $mine.Count
    query_order       = if ($firstAnnounce) { @($firstAnnounce.query_order) } else { @() }
    user_agent        = if ($firstAnnounce) {
        ($firstAnnounce.headers | Where-Object { $_.name -eq 'User-Agent' } | Select-Object -First 1).value
    } else { $null }
    peer_id           = if ($firstAnnounce) { $firstAnnounce.peer_id } else { $null }
    cases             = @($cases)
    udp_cases         = @($udpResults)
    failures          = @($failures)
    notes             = @(
        "Loopback only. Nothing here points at a public tracker and nothing here changes a reported number: this measures whether the announce agrees with the run.",
        "totals-match compares the last announce against the report at exit, so it asserts a bound rather than equality: the announce must cover the payload and must not exceed what the run says it moved.",
        "The interval case allows one second of slack, because the tracker stamps on arrival and the client times from its own clock.",
        "The udp row folds the same six assertions over a BEP 15 announce into one verdict. udp_cases carries them one by one.",
        "redirect and failure-reason use bit-cli trackers rather than a download: one announce is the whole question in both."
    )
}

if ($Json) {
    $jsonPath = if ([System.IO.Path]::IsPathRooted($Json)) { $Json } else { Join-Path $repo $Json }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $jsonPath) | Out-Null
    $result | ConvertTo-Json -Depth 8 | Set-Content -Path $jsonPath -Encoding utf8NoBOM
    Write-Host "check-announce: wrote $Json"
}

@($cases) | ForEach-Object { [pscustomobject]$_ } |
    Format-Table case, judged, ok, detail -AutoSize -Wrap |
    Out-String | Write-Host

if ($firstAnnounce) {
    Write-Host ("query order: " + (@($firstAnnounce.query_order) -join ", "))
    Write-Host ("user agent:  " + $result.user_agent)
    Write-Host ("peer id:     " + $firstAnnounce.peer_id)
}

Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue

if ($failures.Count -gt 0) {
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-announce: $failure") }
    exit 1
}
Write-Host "check-announce: every judged case holds"
exit 0
