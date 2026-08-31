# Check that a release build has no dynamic runtime dependency.
#
# A statically linked bit-cli runs on a machine with nothing installed. On
# Windows that means no Visual C++ redistributable, which is the whole point of
# +crt-static in .cargo/config.toml and is easy to lose: one dependency that
# links the dynamic CRT and the binary starts asking for VCRUNTIME140.dll on a
# machine that does not have it, which fails at process start with a dialog box
# rather than an error a script can read. On Linux it means no interpreter and
# no shared objects at all, which is what the musl targets are for.
#
# Both formats are checked here, chosen by the file's own magic bytes rather
# than by the host, because all three release targets make the same promise and
# only one of them was ever checked.
#
#   PE   the import table must carry no VCRUNTIME, MSVCP, MSVCR, UCRT, CONCRT,
#        or api-ms-win-crt-* entry. Read with dumpbin.
#   ELF  there must be no PT_INTERP program header and no DT_NEEDED dynamic
#        entry. Read here, from the file, with no readelf and no ldd: `ldd` on
#        a static binary prints "not a dynamic executable" on glibc and runs
#        the binary on some others, and neither is a thing to build a gate on.
#
# Usage:
#   pwsh scripts/check-static.ps1
#   pwsh scripts/check-static.ps1 -Path target/x86_64-pc-windows-msvc/release/bit-cli.exe
#   pwsh scripts/check-static.ps1 -Path target/x86_64-unknown-linux-musl/release/bit-cli
#
# With no -Path it checks whichever release binary exists, preferring the one
# an explicit --target produced. `+crt-static` is set per target triple in
# .cargo/config.toml, so a plain `cargo build --release` on the host gets it
# too and both paths are equally valid to check. Checking a path that a build
# never wrote would pass on a stale artifact, which is the one thing this
# script must not do.
#
# Exits 0 when the binary is self-contained, 1 when it is not, and 2 when the
# check could not run.

[CmdletBinding()]
param(
    [string]$Path = ""
)

$ErrorActionPreference = 'Stop'

# Write-Error is a terminating error under `Stop`, so a `Write-Error` followed
# by `exit 2` never reaches the exit and the caller sees 1. The exit codes in
# the header above are the contract, so failures go out this way instead.
function Exit-With([int]$code, [string]$message) {
    [Console]::Error.WriteLine("check-static: $message")
    exit $code
}

$root = Split-Path -Parent $PSScriptRoot

# With no -Path, take whichever release binary exists, preferring the one an
# explicit --target produced because that is what a release build writes.
if (-not $Path) {
    $candidates = @(
        (Join-Path $root "target/x86_64-pc-windows-msvc/release/bit-cli.exe"),
        (Join-Path $root "target/x86_64-unknown-linux-musl/release/bit-cli"),
        (Join-Path $root "target/aarch64-unknown-linux-musl/release/bit-cli"),
        (Join-Path $root "target/release/bit-cli.exe"),
        (Join-Path $root "target/release/bit-cli")
    )
    $Path = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $Path) {
        Exit-With 2 ("no release binary under target/. Build one first: " +
            "cargo build --release --locked")
    }
}

# Resolve a relative path against the repository root rather than the caller's
# working directory, so the script works from anywhere and from CI.
if (-not [System.IO.Path]::IsPathRooted($Path)) {
    $Path = Join-Path $root $Path
}

if (-not (Test-Path $Path)) {
    Exit-With 2 "no binary at $Path. Build it first: cargo build --release --locked"
}

# Which check to run is the file's business, not the host's. A CI job that
# built an ELF on a Linux runner and a developer checking a cross-built
# artifact from Windows should both get the right answer.
$magic = [byte[]]::new(4)
$stream = [System.IO.File]::OpenRead($Path)
try { $read = $stream.Read($magic, 0, 4) } finally { $stream.Dispose() }
if ($read -lt 4) { Exit-With 2 "$Path is $read bytes, which is not a binary" }

$isElf = $magic[0] -eq 0x7F -and $magic[1] -eq 0x45 -and $magic[2] -eq 0x4C -and $magic[3] -eq 0x46
$isPe = $magic[0] -eq 0x4D -and $magic[1] -eq 0x5A

if ($isElf) {
    # ---------------------------------------------------------------------
    # ELF: no interpreter and no shared library needed
    # ---------------------------------------------------------------------
    #
    # A dynamically linked executable carries a PT_INTERP program header
    # naming its loader, and one DT_NEEDED entry per shared object. A fully
    # static one has neither. Reading them takes the program header table and
    # the .dynamic section, both of which are at fixed offsets from the header.
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $class = $bytes[4]           # 1 = 32 bit, 2 = 64 bit
    $endian = $bytes[5]          # 1 = little
    if ($class -ne 2) { Exit-With 2 "$Path is a 32 bit ELF, which no release target produces" }
    if ($endian -ne 1) { Exit-With 2 "$Path is big endian, which no release target produces" }

    $phoff = [System.BitConverter]::ToUInt64($bytes, 0x20)
    $phentsize = [System.BitConverter]::ToUInt16($bytes, 0x36)
    $phnum = [System.BitConverter]::ToUInt16($bytes, 0x38)
    if ($phnum -eq 0) { Exit-With 2 "$Path has no program headers, which cannot be right" }

    $PT_DYNAMIC = 2
    $PT_INTERP = 3
    $interpreter = $null
    $dynamicOffset = 0
    $dynamicSize = 0
    for ($i = 0; $i -lt $phnum; $i++) {
        $at = [int]$phoff + ($i * $phentsize)
        $type = [System.BitConverter]::ToUInt32($bytes, $at)
        $offset = [System.BitConverter]::ToUInt64($bytes, $at + 0x08)
        $filesz = [System.BitConverter]::ToUInt64($bytes, $at + 0x20)
        switch ($type) {
            $PT_INTERP {
                $text = [System.Text.Encoding]::ASCII.GetString($bytes, [int]$offset, [int]$filesz)
                $interpreter = $text.TrimEnd([char]0)
            }
            $PT_DYNAMIC {
                $dynamicOffset = [int]$offset
                $dynamicSize = [int]$filesz
            }
        }
    }

    # DT_NEEDED is tag 1, and the tag list ends at DT_NULL, tag 0. The value is
    # an offset into the string table, which is not needed here: the count is
    # the answer, and a static binary's count is zero.
    $needed = 0
    if ($dynamicSize -gt 0) {
        for ($at = $dynamicOffset; $at -lt ($dynamicOffset + $dynamicSize); $at += 16) {
            $tag = [System.BitConverter]::ToUInt64($bytes, $at)
            if ($tag -eq 0) { break }
            if ($tag -eq 1) { $needed++ }
        }
    }

    Write-Output "binary:  $Path"
    Write-Output "format:  ELF64"
    Write-Output "size:    $((Get-Item $Path).Length) bytes"
    Write-Output "interp:  $(if ($interpreter) { $interpreter } else { 'none' })"
    Write-Output "needed:  $needed shared object(s)"

    $problems = @()
    if ($interpreter) { $problems += "it names the dynamic loader $interpreter" }
    if ($needed -gt 0) { $problems += "it needs $needed shared object(s)" }
    if ($problems) {
        Write-Output ""
        Exit-With 1 "the binary is not statically linked: $($problems -join ', ')"
    }

    Write-Output ""
    Write-Output "static confirmed: no PT_INTERP and no DT_NEEDED"
    exit 0
}

if (-not $isPe) {
    Exit-With 2 "$Path is neither a PE nor an ELF binary"
}

# -------------------------------------------------------------------------
# PE: no dynamic C runtime import
# -------------------------------------------------------------------------

# dumpbin ships with the MSVC build tools and is not on PATH by default.
$dumpbin = Get-ChildItem -Path @(
    "${env:ProgramFiles(x86)}\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe",
    "${env:ProgramFiles}\Microsoft Visual Studio\*\*\VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe"
) -ErrorAction SilentlyContinue | Select-Object -Last 1

if (-not $dumpbin) {
    Exit-With 2 "dumpbin not found. Install the Visual Studio build tools, or run this on a machine that has them."
}

$imports = & $dumpbin.FullName /dependents $Path |
    Select-String -Pattern '^\s+(\S+\.dll)\s*$' |
    ForEach-Object { $_.Matches[0].Groups[1].Value }

if (-not $imports) {
    Exit-With 2 "dumpbin reported no imports for $Path, which cannot be right"
}

# The C runtime, in every spelling that means "not statically linked".
# api-ms-win-crt-* are the CRT api-sets; api-ms-win-core-* are core OS
# api-sets and are fine.
$forbidden = $imports | Where-Object {
    $_ -match '^(vcruntime|msvcp|msvcr|ucrtbase|concrt)' -or $_ -match '^api-ms-win-crt-'
}

Write-Output "binary:  $Path"
Write-Output "size:    $((Get-Item $Path).Length) bytes"
Write-Output "imports:"
$imports | ForEach-Object { Write-Output "  $_" }

if ($forbidden) {
    Write-Output ""
    Exit-With 1 "the binary depends on the dynamic C runtime: $($forbidden -join ', ')"
}

Write-Output ""
Write-Output "static CRT confirmed: no VCRUNTIME, MSVCP, UCRT, or api-ms-win-crt-* import"
exit 0
