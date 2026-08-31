# Prove the licence gate rejects what it says it rejects.
#
# `deny.toml` allows a fixed list of permissive licences and denies everything
# else, and `cargo deny check licenses` passes today. That says the current
# dependency tree is clean; it does not say the gate would catch a copyleft
# dependency arriving tomorrow, which is the thing it exists for.
#
# So this builds a throwaway crate that depends on a local crate declaring
# `GPL-3.0-or-later`, points `cargo deny` at the repository's own `deny.toml`,
# and requires it to fail. The GPL crate is a local path dependency with an
# empty `lib.rs`: nothing is downloaded, no network is touched, and the licence
# is a string in a manifest, which is exactly what `cargo deny` reads.
#
# Two checks, in this order:
#
#   real     `cargo deny check` over this repository. Has to pass.
#   probe    the same configuration over a tree with one GPL dependency. Has to
#            fail, naming the licence.
#
# Usage:
#   pwsh scripts/check-licence-gate.ps1
#
# Exits 0 when both hold, 1 when one does not, and 2 when the check could not
# run.
#
# See TODO/licensing.md, T-120 and T-121.

[CmdletBinding()]
param(
    [string]$Root = ".tmp/licence-gate",
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-licence-gate: $message")
    exit $code
}

function Write-Step($message) {
    Write-Host "$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.fffZ')) $message"
}

if (-not (Get-Command cargo-deny -ErrorAction SilentlyContinue)) {
    Exit-With 2 "cargo-deny is not installed. Install it with: cargo install cargo-deny --locked"
}
$denyConfig = Join-Path $repo "deny.toml"
if (-not (Test-Path $denyConfig)) { Exit-With 2 "no deny.toml at $denyConfig" }

if (-not [System.IO.Path]::IsPathRooted($Root)) { $Root = Join-Path $repo $Root }
if (Test-Path $Root) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path (Join-Path $Root "probe/gpl-crate/src") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Root "probe/src") | Out-Null
$Root = (Resolve-Path $Root).Path
$probe = Join-Path $Root "probe"

$failures = [System.Collections.ArrayList]::new()

# --- 1. The real tree passes ------------------------------------------------

Write-Step "checking this repository against its own deny.toml"
Push-Location $repo
try {
    $real = & cargo deny check 2>&1
    $realOk = $LASTEXITCODE -eq 0
}
finally { Pop-Location }
if (-not $realOk) {
    [void]$failures.Add("cargo deny check failed on this repository")
    $real | Select-Object -Last 20 | ForEach-Object { Write-Host "  $_" }
}
else {
    Write-Step "  passes"
}

# --- 2. A GPL dependency fails ----------------------------------------------

Set-Content -Path (Join-Path $probe "Cargo.toml") -Encoding utf8NoBOM -Value @'
[package]
name = "licence-gate-probe"
version = "0.0.0"
edition = "2024"
license = "MIT"

[dependencies]
gpl-crate = { path = "gpl-crate" }

[workspace]
'@
Set-Content -Path (Join-Path $probe "gpl-crate/Cargo.toml") -Encoding utf8NoBOM -Value @'
[package]
name = "gpl-crate"
version = "0.0.0"
edition = "2024"
license = "GPL-3.0-or-later"
'@
Set-Content -Path (Join-Path $probe "src/lib.rs") -Encoding utf8NoBOM -Value ""
Set-Content -Path (Join-Path $probe "gpl-crate/src/lib.rs") -Encoding utf8NoBOM -Value ""
Copy-Item -Path $denyConfig -Destination (Join-Path $probe "deny.toml") -Force

Write-Step "checking a tree with one GPL-3.0-or-later dependency"
Push-Location $probe
try {
    $probeOutput = & cargo deny check licenses 2>&1
    $probeFailed = $LASTEXITCODE -ne 0
}
finally { Pop-Location }

$text = ($probeOutput | Out-String)
if (-not $probeFailed) {
    [void]$failures.Add("cargo deny accepted a GPL-3.0-or-later dependency")
}
elseif ($text -notmatch "GPL-3\.0-or-later") {
    [void]$failures.Add("cargo deny failed without naming the licence it rejected")
}
else {
    $line = ($probeOutput | Where-Object { $_ -match "rejected: license is not explicitly allowed" } |
        Select-Object -First 1)
    Write-Step "  rejected: $($line -replace '\s+', ' ')"
}

if (-not $Keep) { Remove-Item -Recurse -Force $Root -ErrorAction SilentlyContinue }

Write-Host ""
if ($failures.Count -gt 0) {
    foreach ($failure in $failures) { [Console]::Error.WriteLine("check-licence-gate: $failure") }
    exit 1
}
Write-Host "verdict: the tree is clean and a GPL dependency is refused"
exit 0
