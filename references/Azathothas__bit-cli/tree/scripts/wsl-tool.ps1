# Run Azathothas/ToolKit's WSL2 tooling at the revision this repository pinned.
#
# The defect it exists to catch: a script that wants a throwaway distro fetches
# `wsl-ephemeral.ps1` itself, and every one of them then carries its own copy of
# the rules about doing that. Pin a commit rather than a branch. Verify the
# bytes. Do not pipe a download into a shell. Keep the fetched copy somewhere a
# stale sibling cannot shadow it.
#
# **That last one is not hypothetical and it is why this file exists.** The
# launcher resolves in three steps and the first hit wins: an explicit local
# path, then a `wsl-ephemeral.ps1` sitting **beside** the launcher, then the
# pinned ref. Measured on 2026-08-30: with last session's copy left in `.tmp/`,
# a run passing both `-LauncherRef` and `-LauncherSha256` printed
# `Using the copy beside this launcher`, ran the stale file, and verified
# nothing. The pin was accepted and ignored. Nothing said so beyond that one
# line, and the stale copy had no `-Action HostAddress`, so the failure
# surfaced as a `ValidateSet` error about an argument that does exist upstream.
#
# So the cache directory here holds the launcher and nothing else, and this
# checks that before running it.
#
# Everything else is forwarded unchanged, so this page does not restate the
# tool's parameters. `docs/containers.md` is the procedure and the tool's own
# documentation is at Azathothas/ToolKit.
#
# Usage:
#   pwsh scripts/wsl-tool.ps1 -Action HostAddress
#   pwsh scripts/wsl-tool.ps1 -Action List
#   pwsh scripts/wsl-tool.ps1 -Action New -Image debian:bookworm-slim -Ephemeral -Force -CommandB64 <B64>
#   pwsh scripts/wsl-tool.ps1 -Json          # report what it resolved, run nothing
#
# Exit 0 and the wrapped tool's own code otherwise, 2 when this could not run:
# no Windows, no pin, a digest that does not match, or no network on a first
# fetch.
#
# See TODO/cli-surface.md, T-264, and docs/containers.md.

[CmdletBinding()]
param(
    # Report what would be used and run nothing.
    [switch]$Json,
    # Everything else goes to the launcher untouched.
    [Parameter(ValueFromRemainingArguments = $true)]
    [object[]]$Rest
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("wsl-tool: $message")
    exit $code
}

if (-not $IsWindows) {
    Exit-With 2 "this drives wsl.exe, so it is Windows only"
}

$pinPath = Join-Path $PSScriptRoot "toolkit-pin.json"
if (-not (Test-Path $pinPath)) { Exit-With 2 "$pinPath is missing" }
try {
    $pin = Get-Content -Raw $pinPath | ConvertFrom-Json
} catch {
    Exit-With 2 "$pinPath is not JSON: $($_.Exception.Message)"
}
foreach ($field in @('repository', 'ref')) {
    if (-not $pin.$field) { Exit-With 2 "$pinPath carries no '$field'" }
}
if ($pin.ref -notmatch '^[0-9a-f]{40}$') {
    Exit-With 2 "$pinPath names '$($pin.ref)', which is not a 40 character commit. A branch moves, and a moved reference runs code nobody reviewed."
}

# The cache holds the launcher alone. A `wsl-ephemeral.ps1` here would be
# resolved ahead of the pinned ref and would run unverified; see the header.
$cache = Join-Path $repo ".tmp/toolkit"
New-Item -ItemType Directory -Force -Path $cache | Out-Null
$sibling = Join-Path $cache "wsl-ephemeral.ps1"
if (Test-Path $sibling) {
    Remove-Item -Force $sibling
    [Console]::Error.WriteLine("wsl-tool: removed a wsl-ephemeral.ps1 from the cache; it would have shadowed the pinned revision")
}

$launcher = Join-Path $cache "wsl-ephemeral-launcher.ps1"
$wantLauncher = $pin.files.launcher.sha256
$wantTool = $pin.files.tool.sha256

function Get-Sha256([string]$path) {
    (Get-FileHash -Algorithm SHA256 -Path $path).Hash.ToLowerInvariant()
}

$haveLauncher = $false
if (Test-Path $launcher) {
    $have = Get-Sha256 $launcher
    if (-not $wantLauncher -or $have -eq $wantLauncher) {
        $haveLauncher = $true
    } else {
        [Console]::Error.WriteLine("wsl-tool: the cached launcher does not match the pin, fetching again")
        Remove-Item -Force $launcher
    }
}

if (-not $haveLauncher) {
    $uri = "https://raw.githubusercontent.com/$($pin.repository)/$($pin.ref)/$($pin.files.launcher.path)"
    try {
        Invoke-WebRequest -Uri $uri -OutFile $launcher -UseBasicParsing
    } catch {
        Exit-With 2 "cannot fetch $uri : $($_.Exception.Message)"
    }
    if ($wantLauncher) {
        $have = Get-Sha256 $launcher
        if ($have -ne $wantLauncher) {
            Remove-Item -Force $launcher
            Exit-With 2 "the launcher's digest is $have and the pin says $wantLauncher. Nothing was run and the download is deleted."
        }
    }
    # A file fetched on Windows can carry a Zone.Identifier stream, and an
    # execution policy that would run a local script refuses the same bytes
    # with that stream on them, naming the policy rather than the stream.
    Unblock-File -Path $launcher -ErrorAction SilentlyContinue
}

$resolved = [ordered]@{
    schema     = "wsl-tool/1"
    repository = $pin.repository
    ref        = $pin.ref
    launcher   = $launcher
    launcher_sha256 = Get-Sha256 $launcher
    tool_sha256_expected = $wantTool
    cache      = $cache
}

if ($Json) {
    $resolved | ConvertTo-Json -Depth 5 | Write-Output
    exit 0
}

# An ordinary array built with `+=`. A splatted array is re-parsed as a command
# line, and that property does not survive every way of building one: an
# ArrayList's `ToArray()` binds `-Action` positionally instead of as a
# parameter name. The launcher's own page carries the measurement.
$argv = @('-NoProfile', '-File', $launcher, '-LauncherRef', $pin.ref)
if ($wantTool) { $argv += @('-LauncherSha256', $wantTool) }
foreach ($item in $Rest) { $argv += $item }

& pwsh @argv
exit $LASTEXITCODE
