# Every gate, in one command, with one answer.
#
# A session runs these at the start to establish a baseline and after every
# change to keep one. Run by hand they are four commands whose output has to be
# read four different ways: `fmt` says nothing when it passes, `clippy` buries
# its verdict in a build log, `test` needs its failures filtered by test name
# rather than by the summary line, and `deny` says "ok" four times. This says
# one thing.
#
# Usage:
#   pwsh -NoProfile -File scripts/gates.ps1
#   pwsh -NoProfile -File scripts/gates.ps1 -Fix         # cargo fmt --all first
#   pwsh -NoProfile -File scripts/gates.ps1 -Fast        # skip deny and the build
#   pwsh -NoProfile -File scripts/gates.ps1 -Build       # also build the binaries
#   pwsh -NoProfile -File scripts/gates.ps1 -Json
#
# Exit codes: 0 every gate passed, 1 one did not, 2 the script could not run.
#
# Three things it does that running the commands by hand does not:
#
#   - Kills stray `bit-cli` and loopback-* processes first. A release binary
#     left running by an acceptance script holds its own executable open, and
#     the next build fails on a locked file with an error that names neither.
#     One exception, and it is load-bearing: a process running out of `.tmp/`
#     is spared, because `soak.ps1` copies its binaries there so that a six
#     hour run holds no build output open. Killing it would end T-040's
#     acceptance silently, which is the one measurement a session cannot redo.
#   - Filters test failures with `^test \S+ \.\.\. FAILED` and -CaseSensitive.
#     `-match 'FAILED'` matches "0 failed" in the summary line, so a flake's
#     name is lost exactly when it is needed. TODO/RULES.md section 5.
#   - Builds with `--bins --examples` when asked. `--examples` alone builds the
#     examples and no binaries, which is how a script comes to run yesterday's
#     `bit-cli.exe`. TODO/RULES.md section 5.
#   - Fails on any C0 control byte except tab, newline and return, in any
#     tracked text file. Four were in this tree and none was noticed, because
#     a file with one in it is what `grep` calls binary and skips.
#   - Normalises line endings, under `-Fix`, and fails on them otherwise. A
#     carriage return in a file `.gitattributes` says is LF is invisible to
#     git, because the index is normalised either way, and visible to every
#     regex in this repository that reads the working tree. Wiring it here is
#     what makes it never a step anybody runs by hand.
#   - Runs `check-todo.ps1`, so a push cannot carry a record that contradicts
#     the tree. `patches/TASKS.md` said two P0 entries were open for a session
#     after both closed, because nothing compared the two files.
#   - Prints the toolchain and warns when the stable it is using is behind the
#     one CI would install. Clippy gains lints with every release, so a green
#     run here on an older rustc is not a green clippy job there. It warns
#     rather than fails: a stale toolchain is not a reason to stop working.
#
# See TODO/RULES.md.

[CmdletBinding()]
param(
    # Run `cargo fmt --all` before checking, rather than failing on formatting.
    [switch]$Fix,
    # Skip `cargo deny` and the build. For the inner loop.
    [switch]$Fast,
    # Also `cargo build --release --bins --examples`.
    [switch]$Build,
    [switch]$Json
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

$results = [System.Collections.Specialized.OrderedDictionary]::new()
$failures = [System.Collections.ArrayList]::new()
$started = [System.Diagnostics.Stopwatch]::StartNew()

function Write-Step([string]$text) {
    $stamp = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    Write-Host "$stamp gates: $text"
}

function Record([string]$name, [bool]$ok, [string]$detail) {
    $results[$name] = [ordered]@{ ok = $ok; detail = $detail }
    if (-not $ok) { [void]$failures.Add("$name`: $detail") }
    $verdict = if ($ok) { "ok" } else { "FAILED" }
    Write-Step "$name $verdict$(if ($detail) { " ($detail)" })"
}

# ---------------------------------------------------------------------------
# Stray processes
# ---------------------------------------------------------------------------

# A process running out of `.tmp/` is not stray and is not killed. `soak.ps1`
# copies the binaries it needs into `.tmp/soak/bin/` for exactly this reason:
# the copy holds no build output open, so nothing here is served by stopping
# it. T-040's acceptance is a six hour run, PROGRESS.md tells a session to
# start it early, and every gates run in between would otherwise end it. The
# run is the measurement, so losing it silently costs the whole session.
$tmpRoot = [System.IO.Path]::GetFullPath((Join-Path $repo ".tmp")) + [System.IO.Path]::DirectorySeparatorChar
$candidates = @(Get-Process bit-cli, loopback-fileserver, loopback-tracker, loopback-churn, loopback-tlsprobe -ErrorAction SilentlyContinue)
$spared = @($candidates | Where-Object {
        $path = try { $_.Path } catch { $null }
        $path -and $path.StartsWith($tmpRoot, [StringComparison]::OrdinalIgnoreCase)
    })
$stray = @($candidates | Where-Object { $spared -notcontains $_ })
if ($spared.Count -gt 0) {
    Write-Step "leaving $($spared.Count) process(es) under .tmp/ alone, they hold no build output"
}
if ($stray.Count -gt 0) {
    Write-Step "stopping $($stray.Count) stray process(es) that would lock the build output"
    $stray | Stop-Process -Force -ErrorAction SilentlyContinue
}

# ---------------------------------------------------------------------------
# The toolchain, which is not a gate but decides what the gates can see
# ---------------------------------------------------------------------------
#
# CI pins `stable`, which moves. Clippy gains lints with every release, so a
# local toolchain a release behind passes a clippy CI then fails. That has
# happened: `clippy::chunks_exact_to_as_chunks` arrived in 1.98 and a push that
# was green here was red there, on nothing but the age of this machine's
# rustc. This warns rather than fails, because a red gate for a toolchain
# nobody has updated yet would stop work that is otherwise fine.

$toolchain = (& rustc --version 2>&1 | Out-String).Trim()
Write-Step "toolchain $toolchain"
if (-not $Fast -and (Get-Command rustup -ErrorAction SilentlyContinue)) {
    $check = & rustup check 2>&1 | Out-String
    $stale = $check -split "`n" | Where-Object {
        $_ -match '^stable-' -and $_ -match 'update available'
    }
    if ($stale) {
        # Only the toolchain in use matters. `rustup check` lists every one
        # installed, and a stale `windows-gnu` beside a current `windows-msvc`
        # is not a problem anybody has.
        $inUse = (& rustup show active-toolchain 2>&1 | Out-String).Trim()
        foreach ($line in $stale) {
            $name = ($line -split ' ')[0]
            if ($inUse -like "$name*") {
                Write-Step "WARNING: $($line.Trim())"
                Write-Step "WARNING: CI builds on stable, so a lint this rustc cannot see can still fail there. rustup update stable"
            }
        }
    }
}

# ---------------------------------------------------------------------------
# text
# ---------------------------------------------------------------------------
#
# A control byte in a tracked text file is invisible and changes what the file
# means. A NUL makes every text tool treat the file as binary. `grep` answers "Binary file X matches" instead of the line, a diff is
# unreadable, and whatever is around it is invisible to a review.
#
# Two got in and neither was noticed. `crates/bit-cli-core/src/torrent/bencode.rs`
# had three, in a byte-string literal written with the bytes themselves rather
# than escapes, since 2026-08-21. `TODO/trackers.md` had one on 2026-08-22, from
# a Python escape interpreted on the way to the file. Both are one line to
# check, and the check is here rather than in `check-todo.ps1` because it is
# the source tree that had the older one.
#
# Tracked files only, and only the ones meant to be text: `git ls-files` knows
# what is tracked, and the extension list is what this tree actually holds.

# NUL is not the only one. On 2026-08-23 a 0x08 backspace reached
# `scripts/check-todo.ps1` the same way the `TODO/trackers.md` NUL did, from a
# Python `\b` escape interpreted on the way to the file. It landed inside a
# regex, so the pattern silently matched nothing and a check written that
# afternoon passed everything. This gate said `text ok` on the same run.
#
# So the set is every C0 control byte except the three that are text: tab,
# newline, and carriage return. A byte in that range is never something anybody
# typed on purpose into a source file, and it is invisible in every editor.
$allowed = @([byte]9, [byte]10, [byte]13)
$binaryish = [System.Collections.ArrayList]::new()
$tracked = & git ls-files -- "*.rs" "*.md" "*.ps1" "*.toml" "*.yml" "*.jq" 2>$null
foreach ($relative in $tracked) {
    if (-not $relative) { continue }
    $path = Join-Path $repo $relative
    if (-not (Test-Path $path)) { continue }
    $bytes = [System.IO.File]::ReadAllBytes($path)
    for ($i = 0; $i -lt $bytes.Length; $i++) {
        $b = $bytes[$i]
        if ($b -lt 32 -and $allowed -notcontains $b) {
            [void]$binaryish.Add("${relative}:$i is byte 0x$('{0:x2}' -f $b)")
            break
        }
    }
}
Record "text" ($binaryish.Count -eq 0) $(if ($binaryish.Count -eq 0) { "" }
    else { "control byte in $($binaryish -join ', ')" })

# ---------------------------------------------------------------------------
# eol
# ---------------------------------------------------------------------------
#
# Beside `text` because it is the same subject: bytes in a tracked text file
# that nobody typed and nobody can see. `.gitattributes` normalises the index,
# so a file written with CRLF commits as LF and `git diff` shows nothing at
# all. What it does not normalise is the working tree, which is what
# `check-todo.ps1`, `check-docs.ps1` and every `(?m)^...$` in this repository
# actually read: in .NET that anchor matches before the newline and leaves the
# carriage return inside the capture.
#
# It runs before `record` for the same reason `man` and `fmt` do: it rewrites
# files under -Fix, and a gate that reports on a tree the run then changes is
# T-220. See scripts/check-eol.ps1 for what it costs and why vendor/ is
# reported rather than rewritten.

$eolArgs = @("-NoProfile", "-File", (Join-Path $PSScriptRoot "check-eol.ps1"))
if ($Fix) { $eolArgs += "-Fix" }
$eolSaid = & pwsh @eolArgs 2>&1
$eolOk = ($LASTEXITCODE -eq 0)
# Counting the `wrong` lines is right when the check only reported, and wrong
# under -Fix, where a file it could not repair is labelled something else. That
# combination printed `eol FAILED (0 file(s) disagree)` on 2026-08-30 over two
# real files, which sends a reader looking at a count instead of at the check.
# So the count is used when there is one and the check's own last line
# otherwise: a gate must never report a failure as zero of anything.
$eolWrong = @($eolSaid | Select-String '^  wrong ').Count
Record "eol" $eolOk $(if ($eolOk) { "" }
    elseif ($eolWrong -gt 0) { "$eolWrong file(s) disagree with .gitattributes; run with -Fix" }
    else { "$(@($eolSaid)[-1])" })

# ---------------------------------------------------------------------------
# man
# ---------------------------------------------------------------------------
#
# `man/bit-cli.1`, `man/bit-cli.json` and `man/bit-cli.md` are generated from
# the clap definition and committed, so a reader can open them without building
# anything. A committed generated file is only worth having if something fails
# when it goes stale.
#
# The check that binds is `cargo test -p bit-cli --test man_is_current`, inside
# the `test` gate below: it renders from the crate being compiled, so it cannot
# compare against a stale binary, and it runs wherever CI builds. This line is
# here so a session that regenerates gets told what to run rather than reading
# a test name out of a failure, and it is skipped when there is no binary yet
# rather than failing on one that does not exist.
#
# -Fix regenerates them, the same as it formats.

$manExe = Join-Path $repo "target/release/bit-cli.exe"
if (-not (Test-Path $manExe)) { $manExe = Join-Path $repo "target/release/bit-cli" }
if ($Fast) {
    Write-Step "man skipped by -Fast"
}
elseif (-not (Test-Path $manExe)) {
    Write-Step "man skipped: no release binary yet, the test gate covers it"
}
else {
    $manArgs = @("-NoProfile", "-File", (Join-Path $PSScriptRoot "check-man.ps1"))
    if ($Fix) { $manArgs += "-Fix" }
    & pwsh @manArgs | Out-Null
    Record "man" ($LASTEXITCODE -eq 0) $(if ($LASTEXITCODE -eq 0) { "" }
        else { "run with -Fix, or: pwsh -NoProfile -File scripts/check-man.ps1 -Fix" })
}

# ---------------------------------------------------------------------------
# fmt
# ---------------------------------------------------------------------------

if ($Fix) {
    & cargo fmt --all
    Record "fmt" ($LASTEXITCODE -eq 0) "rewritten"
}
else {
    & cargo fmt --all --check | Out-Null
    Record "fmt" ($LASTEXITCODE -eq 0) $(if ($LASTEXITCODE -eq 0) { "" } else { "run with -Fix" })
}

# ---------------------------------------------------------------------------
# record
# ---------------------------------------------------------------------------
#
# `TODO/` is the authoritative record and `patches/TASKS.md` is the ordered
# list of vendored work. Both are second copies of a status that lives in an
# entry, and a second copy is the thing that goes stale.
#
# It went stale, and this gate is what it cost. The session of 2026-08-22
# closed both P0 entries, wrote it into the entries, into `INDEX.md` and into
# `PROGRESS.md`, and pushed. `patches/TASKS.md` was rewritten afterwards and
# never committed, so HEAD went on saying `T-020 | P0 | open` while the entry
# beside it said `done`. The next session read the stale one first.
#
# `check-todo.ps1` compares them: every row against the entry it names, every
# count against the rows, and PROGRESS.md against what RULES.md section 2 step
# 2 says it must carry. That is a second, and it runs here so that a push
# cannot carry a record contradicting the tree it describes. It is not skipped
# by -Fast: it costs about three seconds, and it is the one gate here that
# catches a claim rather than a defect.
#
# **It runs after `man` and `fmt`, and that ordering is load-bearing.** Both of
# those rewrite files under `-Fix`, and one of the things this checks is that a
# `TODO/` citation names the line its symbol is actually on. Run first, it
# checked line numbers that `cargo fmt --all` then moved: on 2026-08-23 a local
# `gates.ps1 -Fix` printed `record ok` and the same check failed in CI on the
# push that followed, because the formatting pass had added ten lines to the
# file a citation pointed into. A gate that reports on a tree the run then
# changes is the same defect `check-man.ps1` had. See `TODO/cli-surface.md`,
# T-220.

$todoArgs = @("-NoProfile", "-File", (Join-Path $PSScriptRoot "check-todo.ps1"))
$todoOut = (& pwsh @todoArgs 2>&1 | Out-String)
$todoOk = ($LASTEXITCODE -eq 0)
$todoDetail = ""
if (-not $todoOk) {
    $lines = @($todoOut -split "`r?`n" | Where-Object { $_ -match '^\s+\[' })
    $todoDetail = if ($lines.Count -gt 0) { ($lines[0].Trim()) } else { "see: pwsh -NoProfile -File scripts/check-todo.ps1" }
    if ($lines.Count -gt 1) { $todoDetail += " and $($lines.Count - 1) more" }
}
Record "record" $todoOk $todoDetail

# ---------------------------------------------------------------------------
# tree
# ---------------------------------------------------------------------------
#
# The `text` gate above reads six extensions. It cannot see a file that is not
# one of them, and on 2026-08-23 that is exactly what reached the remote: a
# 1,000 byte payload at `under/inner.bin`, left in the working tree by a T-226
# acceptance run and taken by `git add -A`. Eight commits later nobody had
# looked at it.
#
# `check-tree.ps1` says what belongs in this repository: a fixed top level, and
# outside `vendor/` a fixed set of file kinds, both measured from what the
# index already holds. It reads the index rather than the working tree, which
# is why `git-sync.ps1` can run the same script after staging and get an
# answer about the commit it is about to make.
#
# It found a second thing the day it was written. `bench/soak-20260821T012428252Z.csv`
# ended in 176 NUL bytes from a soak killed mid-append, and `soak.ps1 -ReadCsv`
# was reading them as a final sample of zeros. See `TODO/cli-surface.md` T-230
# and `TODO/memory.md` T-231.

$treeArgs = @("-NoProfile", "-File", (Join-Path $PSScriptRoot "check-tree.ps1"))
$treeOut = (& pwsh @treeArgs 2>&1 | Out-String)
$treeOk = ($LASTEXITCODE -eq 0)
$treeDetail = ""
if (-not $treeOk) {
    $lines = @($treeOut -split "`r?`n" | Where-Object { $_ -match '^check-tree: \[' })
    $treeDetail = if ($lines.Count -gt 0) { ($lines[0] -replace '^check-tree: ', '').Trim() } else { "see: pwsh -NoProfile -File scripts/check-tree.ps1" }
    if ($lines.Count -gt 1) { $treeDetail += " and $($lines.Count - 1) more" }
}
Record "tree" $treeOk $treeDetail

# ---------------------------------------------------------------------------
# docs
# ---------------------------------------------------------------------------
#
# `record` above holds `TODO/` and `patches/` to the tree. Nothing held
# `README.md` and `docs/` to anything: a renamed script, a renamed flag or a
# moved heading left a document that still read correctly and no longer
# described this tool.
#
# `check-docs.ps1` resolves every relative link and anchor, every `scripts/`
# path, and every flag and command an example names, against
# `man/bit-cli.json`. It also enforces the mechanical half of the prose rule.
#
# It is separate from `check-todo.ps1` rather than folded into it because the
# rules differ in two ways that matter: a `TODO/` entry is allowed to carry
# project history and a `docs/` page is not, and a `TODO/` entry may name a
# check script that does not exist yet while a `docs/` page may not.

$docsArgs = @("-NoProfile", "-File", (Join-Path $PSScriptRoot "check-docs.ps1"))
$docsOut = (& pwsh @docsArgs 2>&1 | Out-String)
$docsOk = ($LASTEXITCODE -eq 0)
$docsDetail = ""
if (-not $docsOk) {
    $lines = @($docsOut -split "`r?`n" | Where-Object { $_ -match '^\s+\[' })
    $docsDetail = if ($lines.Count -gt 0) { $lines[0].Trim() } else { "see: pwsh -NoProfile -File scripts/check-docs.ps1" }
    if ($lines.Count -gt 1) { $docsDetail += " and $($lines.Count - 1) more" }
}
Record "docs" $docsOk $docsDetail

# ---------------------------------------------------------------------------
# clippy
# ---------------------------------------------------------------------------

# One log per run, not one per machine.
#
# Both of these were a fixed name, so two `gates.ps1` runs at once collided on
# them and the second died with `out-file: The process cannot access the file
# ... because it is being used by another process`. Nothing in that message
# says "another gates run is going", so the next session debugs `Out-File`. A
# session does start a second run: one in the background and one in the
# foreground is the ordinary way an agent works. See TODO/cli-surface.md,
# T-228.
#
# `$PID` rather than a random suffix, so a log left behind by a run that was
# killed can still be tied to the process that wrote it.
$clippyLog = Join-Path ([System.IO.Path]::GetTempPath()) "bit-cli-gates-clippy-$PID.txt"
& cargo clippy --workspace --all-targets --all-features -- -D warnings 2>&1 |
    Tee-Object -FilePath $clippyLog | Out-Null
$clippyOk = $LASTEXITCODE -eq 0
$clippyCount = @(Select-String -Path $clippyLog -Pattern '^error' -CaseSensitive).Count
Record "clippy" $clippyOk $(if ($clippyOk) { "" } else { "$clippyCount error line(s), see $clippyLog" })

# ---------------------------------------------------------------------------
# test
# ---------------------------------------------------------------------------

$testLog = Join-Path ([System.IO.Path]::GetTempPath()) "bit-cli-gates-tests-$PID.txt"
& cargo test --workspace 2>&1 | Tee-Object -FilePath $testLog | Out-Null
$testExit = $LASTEXITCODE

$failed = @(Select-String -Path $testLog -Pattern '^test \S+ \.\.\. FAILED' -CaseSensitive |
    ForEach-Object { ($_.Line -split '\s+')[1] })
$passed = 0
foreach ($line in (Select-String -Path $testLog -Pattern '^test result: ok\. (\d+) passed')) {
    $passed += [int]$line.Matches[0].Groups[1].Value
}
$testOk = ($testExit -eq 0) -and ($failed.Count -eq 0)
$testDetail = if ($testOk) { "$passed passed" }
elseif ($failed.Count -gt 0) { "$($failed.Count) failed: $($failed -join ', ')" }
else { "exited $testExit with no named failure, see $testLog" }
Record "test" $testOk $testDetail

# ---------------------------------------------------------------------------
# deny
# ---------------------------------------------------------------------------

if (-not $Fast) {
    if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
        & cargo deny check 2>&1 | Out-Null
        Record "deny" ($LASTEXITCODE -eq 0) ""
    }
    else {
        Record "deny" $true "cargo-deny is not installed, so this is unmeasured"
    }
}

# ---------------------------------------------------------------------------
# build
# ---------------------------------------------------------------------------

if ($Build -and -not $Fast) {
    # --bins AND --examples. `--examples` alone builds no binaries, which is
    # how an acceptance script comes to measure a stale bit-cli.exe.
    & cargo build --release --bins --examples 2>&1 | Out-Null
    Record "build" ($LASTEXITCODE -eq 0) "release, bins and examples"
}

# ---------------------------------------------------------------------------
# Say it
# ---------------------------------------------------------------------------

$started.Stop()
$ok = $failures.Count -eq 0

# A passing run leaves nothing behind. A failing one leaves both logs, because
# the detail line above points a reader at them by path and a message naming a
# file that is gone is worse than no message. See TODO/cli-surface.md, T-228.
if ($ok) {
    foreach ($log in @($clippyLog, $testLog)) {
        if ($log) { Remove-Item -LiteralPath $log -Force -ErrorAction SilentlyContinue }
    }
}

if ($Json) {
    [ordered]@{
        kind           = "gates"
        schema_version = "1"
        generated_at   = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
        elapsed_ms     = $started.ElapsedMilliseconds
        ok             = $ok
        tests_passed   = $passed
        tests_failed   = @($failed)
        gates          = $results
        failures       = @($failures)
    } | ConvertTo-Json -Depth 6
    exit $(if ($ok) { 0 } else { 1 })
}

Write-Host ""
if ($ok) {
    Write-Host ("all gates pass: {0} tests, {1:n1}s" -f $passed, ($started.Elapsed.TotalSeconds))
    exit 0
}
Write-Host "gates failed:"
foreach ($item in $failures) { Write-Host "  $item" }
exit 1
