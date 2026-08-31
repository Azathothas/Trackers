# Put NASM on PATH for a Windows build.
#
# `aws-lc-sys` assembles its own primitives and needs `nasm`, which is not on the
# GitHub Windows runner by default. It arrives under two parents, so removing
# either does not remove the need: `rustls`, and `librqbit-sha1-wrapper`, the
# SHA-1 backend every piece hash goes through. `cargo tree -i aws-lc-rs` shows
# both.
#
# **Why this is not `ilammy/setup-nasm`.** That action is unmaintained: v1.5.2
# is its newest release, it still runs on node20, and GitHub warns about the
# deprecation on every run. It also downloads NASM and checks nothing about
# what it got. This does the same job in about thirty lines, verifies the
# archive against a pinned SHA-256, and is a file in this repository that a
# reader can audit.
#
# Usage:
#   pwsh -NoProfile -File scripts/setup-nasm.ps1
#   pwsh -NoProfile -File scripts/setup-nasm.ps1 -Version 2.16.03 -Sha256 <hex>
#
# On a GitHub runner it appends the install directory to $env:GITHUB_PATH so
# later steps see it. Run locally, it prints the directory to add.
#
# Exits 0 when nasm is available afterwards, and 2 when it could not be
# installed. It is a no-op when nasm is already on PATH, so running it on a
# machine that has one costs nothing.

[CmdletBinding()]
param(
    [string]$Version = "2.16.03",
    # The SHA-256 of nasm-$Version-win64.zip as published by nasm.us, verified
    # by downloading it. Pinned rather than trusted: an archive that changes
    # under a released version number is exactly what a checksum is for.
    [string]$Sha256 = "3ee4782247bcb874378d02f7eab4e294a84d3d15f3f6ee2de2f47a46aa7226e6",
    [string]$Destination = "$env:RUNNER_TEMP",
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

function Say([string]$text) {
    Write-Host "setup-nasm: $text"
}

function Exit-With([int]$code, [string]$text) {
    [Console]::Error.WriteLine("setup-nasm: $text")
    exit $code
}

if (-not ($IsWindows -or $env:OS -eq "Windows_NT")) {
    Say "not Windows, nothing to do"
    exit 0
}

if (-not $Force) {
    $existing = Get-Command nasm -ErrorAction SilentlyContinue
    if ($existing) {
        Say "already on PATH at $($existing.Source)"
        exit 0
    }
}

if (-not $Destination) { $Destination = Join-Path ([System.IO.Path]::GetTempPath()) "nasm" }
New-Item -ItemType Directory -Force -Path $Destination | Out-Null

$url = "https://www.nasm.us/pub/nasm/releasebuilds/$Version/win64/nasm-$Version-win64.zip"
$archive = Join-Path $Destination "nasm-$Version-win64.zip"

Say "downloading $url"
try {
    # -UseBasicParsing for the runner's constrained PowerShell, and the
    # progress bar off because it makes Invoke-WebRequest an order of
    # magnitude slower on a large file.
    $previous = $ProgressPreference
    $ProgressPreference = 'SilentlyContinue'
    Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing -MaximumRetryCount 3 -RetryIntervalSec 5
    $ProgressPreference = $previous
} catch {
    Exit-With 2 "could not download $url : $_"
}

$actual = (Get-FileHash -Path $archive -Algorithm SHA256).Hash.ToLowerInvariant()
$expected = $Sha256.ToLowerInvariant()
if ($actual -ne $expected) {
    Exit-With 2 "checksum mismatch for nasm-$Version-win64.zip`n  expected $expected`n  got      $actual"
}
Say "sha256 ok"

$extracted = Join-Path $Destination "nasm-$Version"
if (Test-Path $extracted) { Remove-Item -Recurse -Force $extracted }
Expand-Archive -Path $archive -DestinationPath $Destination -Force

if (-not (Test-Path (Join-Path $extracted "nasm.exe"))) {
    Exit-With 2 "the archive did not contain nasm.exe at $extracted"
}

# GITHUB_PATH is how a step hands PATH to the steps after it. Locally there is
# no such file, so say what to add instead of failing.
if ($env:GITHUB_PATH) {
    Add-Content -Path $env:GITHUB_PATH -Value $extracted
    Say "added $extracted to GITHUB_PATH"
} else {
    Say "add this to PATH: $extracted"
    $env:PATH = "$extracted;$env:PATH"
}

$version = & (Join-Path $extracted "nasm.exe") -v 2>&1 | Out-String
Say $version.Trim()
exit 0
