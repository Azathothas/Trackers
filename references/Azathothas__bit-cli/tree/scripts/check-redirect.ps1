<#
.SYNOPSIS
    What each way of capturing `--json` on Windows does to the bytes.

.DESCRIPTION
    `bit-cli` writes UTF-8 with no BOM to stdout whatever the console code page
    is. Getting those bytes into a file or a parser is the caller's half, and on
    Windows it is not one decision but two:

      [Console]::OutputEncoding   how this host decodes what a program wrote
      $OutputEncoding             how this host encodes what it sends into one

    Neither defaults to UTF-8. On this machine both hosts start at the console
    code page, IBM437, and Windows PowerShell 5.1 sends ASCII into a native
    command. So a name with a character outside that code page is corrupted on
    the way through the pipeline, and the JSON still parses: nothing says so.

    This builds a torrent whose name carries four characters no single code page
    holds, runs every documented form against it, and reports which ones give
    the bytes back. Run it under both hosts. It judges nothing and fails on
    nothing: what it measures is a property of the host, not of `bit-cli`.

        pwsh -NoProfile -File scripts/check-redirect.ps1
        powershell -NoProfile -File scripts/check-redirect.ps1

    See `TODO/windows.md`, T-075, and the README's "On Windows" section, which
    this is the evidence for.

.NOTES
    No non-ASCII byte appears in this file. Windows PowerShell 5.1 reads a UTF-8
    script with no BOM as ANSI, so a literal here would be corrupted before the
    measurement started. The name is built from code points instead.
#>

[CmdletBinding()]
param(
    [string]$Exe = "target/release/bit-cli.exe",
    [string]$Work = ".tmp/redirect-check",
    [string]$Json
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path $Exe)) {
    throw "no binary at $Exe. Build one: cargo build --release --bins"
}
$Exe = (Resolve-Path $Exe).Path

if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
New-Item -ItemType Directory -Force -Path $Work | Out-Null
$Work = (Resolve-Path $Work).Path

# "cafe" with an acute e, a lambda, and two CJK ideographs. IBM437 holds the
# first, cp1252 holds the first, and neither holds the rest.
$name = "caf" + [char]0x00E9 + "-" + [char]0x03BB + "-" + [char]0x65E5 + [char]0x672C + ".bin"
$expectedBytes = [System.Text.Encoding]::UTF8.GetBytes($name)

$payload = Join-Path $Work $name
[System.IO.File]::WriteAllBytes($payload, (New-Object byte[] 65536))
$torrent = Join-Path $Work "fixture.torrent"

# Arguments reach a program as UTF-16 whatever the code page is, so the name
# survives the call that creates the torrent. Only output is at risk.
& $Exe create $payload -o $torrent --no-creation-date --no-created-by | Out-Null
if ($LASTEXITCODE -ne 0) { throw "bit-cli create exited $LASTEXITCODE" }

# jq reads a file and writes the name. Its stdout is captured as raw bytes, so
# what is compared is what jq produced rather than what this host decoded.
$haveJq = [bool](Get-Command jq -ErrorAction SilentlyContinue)
function Test-File([string]$path) {
    if (-not (Test-Path $path)) { return [ordered]@{ wrote = $false; whole = $false; note = "nothing written" } }
    $bytes = [System.IO.File]::ReadAllBytes($path)
    $take = [Math]::Min(4, $bytes.Length)
    $head = ($bytes[0..($take - 1)] | ForEach-Object { $_.ToString('x2') }) -join ' '
    $row = [ordered]@{ wrote = $true; size = $bytes.Length; head = $head; whole = $false; note = '' }
    if (-not $haveJq) { $row.note = 'jq is not on PATH'; return $row }
    $out = Join-Path $Work 'jq-out.bin'
    $err = Join-Path $Work 'jq-err.txt'
    $p = Start-Process -FilePath 'jq' -ArgumentList @('-r', '.name', "`"$path`"") `
        -NoNewWindow -Wait -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
    if ($p.ExitCode -ne 0) {
        $row.note = (((Get-Content -Path $err -Raw -ErrorAction SilentlyContinue) -replace '\s+', ' ')).Trim()
        return $row
    }
    $got = [System.IO.File]::ReadAllBytes($out)
    while ($got.Length -gt 0 -and ($got[-1] -eq 10 -or $got[-1] -eq 13)) { $got = $got[0..($got.Length - 2)] }
    $row.whole = (@(Compare-Object $got $expectedBytes -SyncWindow 0).Count -eq 0)
    if (-not $row.whole) { $row.note = 'jq read it, and the name is not the name' }
    return $row
}

$forms = [ordered]@{}

# 1. The host's own redirection operator.
$f = Join-Path $Work 'redirect.json'
& $Exe info $torrent --json > $f
$forms['> file'] = Test-File $f

# 2. cmd's redirection, which copies bytes and decodes nothing.
$f = Join-Path $Work 'cmd.json'
& cmd /c "`"$Exe`" info `"$torrent`" --json > `"$f`"" | Out-Null
$forms['cmd /c "... > file"'] = Test-File $f

# 3. Out-File with the encoding named. utf8NoBOM arrived in PowerShell 6.
$f = Join-Path $Work 'outfile.json'
try {
    & $Exe info $torrent --json | Out-File -Encoding utf8NoBOM $f
    $forms['| Out-File -Encoding utf8NoBOM'] = Test-File $f
}
catch {
    $forms['| Out-File -Encoding utf8NoBOM'] = [ordered]@{
        wrote = $false; whole = $false
        note  = 'this host has no utf8NoBOM: ' + (($_.Exception.Message -replace '\s+', ' ').Trim())
    }
}

# 4. Set-Content, which exists on both hosts and means a different thing on each.
$f = Join-Path $Work 'setcontent.json'
& $Exe info $torrent --json | Set-Content -Path $f -Encoding utf8
$forms['| Set-Content -Encoding utf8'] = Test-File $f

# 5. The pipe straight into the host's parser, with no file at all.
$parsed = & $Exe info $torrent --json | ConvertFrom-Json
$forms['| ConvertFrom-Json'] = [ordered]@{
    wrote = $true; whole = ($parsed.name -eq $name)
    note  = $(if ($parsed.name -eq $name) { '' } else { 'it parsed, and the name is not the name' })
}

# 6. The same two forms with both encodings named first. This is the recipe.
$beforeConsole = [Console]::OutputEncoding
$beforeNative = $OutputEncoding
try {
    [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
    $OutputEncoding = New-Object System.Text.UTF8Encoding $false

    $parsed = & $Exe info $torrent --json | ConvertFrom-Json
    $forms['| ConvertFrom-Json, encodings set'] = [ordered]@{
        wrote = $true; whole = ($parsed.name -eq $name); note = ''
    }
    $f = Join-Path $Work 'setcontent-utf8.json'
    & $Exe info $torrent --json | Set-Content -Path $f -Encoding utf8
    $forms['| Set-Content -Encoding utf8, encodings set'] = Test-File $f
}
finally {
    [Console]::OutputEncoding = $beforeConsole
    $OutputEncoding = $beforeNative
}

$report = [ordered]@{
    generated_at            = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    host                    = $PSVersionTable.PSVersion.ToString()
    edition                 = "$($PSVersionTable.PSEdition)"
    console_output_encoding = [Console]::OutputEncoding.WebName
    to_native_encoding      = $OutputEncoding.WebName
    jq                      = $haveJq
    name_utf8_hex           = (($expectedBytes | ForEach-Object { $_.ToString('x2') }) -join '')
    forms                   = $forms
}

Write-Output ""
Write-Output "host $($report.host) $($report.edition), console reads $($report.console_output_encoding), writes $($report.to_native_encoding) into a program"
Write-Output ""
Write-Output ("{0,-44} {1,-6} {2}" -f 'form', 'whole', 'note')
foreach ($key in $forms.Keys) {
    $row = $forms[$key]
    Write-Output ("{0,-44} {1,-6} {2}" -f $key, $(if ($row.whole) { 'yes' } else { 'NO' }), $row.note)
}
Write-Output ""

if ($Json) {
    $report | ConvertTo-Json -Depth 6 | Set-Content -Path $Json -Encoding utf8
    Write-Output "wrote $Json"
}
