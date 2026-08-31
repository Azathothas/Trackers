# git-sync.ps1 - the sanctioned way to commit and push in a project that
# started from this template.
#
# ⭐ THE TWIN OF git-sync.sh, and the one to prefer on Windows. Both pin the
# identity and enforce the same refusals; this one runs where no POSIX shell
# does, and it drives the native git.exe rather than one inside an msys layer.
#
# The defect this exists to catch is a rule that everybody agreed to and nobody
# enforces. docs/conventions/git.md states the identity rule and the
# attribution rule; before this script existed, the template DOCUMENTED both
# and ENFORCED neither, so the only thing standing between a project and a
# commit crediting a tool was whether the agent that session had read the file.
#
# ⭐ WHAT IT MAKES MECHANICAL, and each one has cost a real session:
#
#   1. Author AND committer are pinned per invocation with `git -c`, so a
#      machine whose global config says something else still produces the
#      right commit. ⚠ `git commit --author` sets only the author, which is
#      why both are set here: a commit can carry two different identities and
#      the one shown in a log is not the one a checker reads.
#   2. An AI-attribution line is REFUSED, never stripped. Silently rewriting
#      somebody's commit message is worse than declining to commit it: the
#      author never learns the rule and the next message has the same line.
#   3. A CI-skip marker is refused unless the flag was passed. A message that
#      merely MENTIONS a skip marker skips CI, because the platform does not
#      read the sentence around it. That shipped a commit with no run once.
#   4. The body is read from a FILE, never from a shell string. A body with an
#      apostrophe in it does not survive a shell, and the failure is silent:
#      see docs/conventions/shell.md section 1.
#   5. The gates run BEFORE the commit, not after the push. Finding out after
#      is finding out late, and a red remote is somebody else's problem by then.
#
# ⛔ NOTHING ABOUT THIS SCRIPT KNOWS WHO YOU ARE. The identity comes from
# -Name/-Email or from git config, and if neither has one the script refuses
# rather than guessing. A template must never carry a person baked into it.
#
# ⚠ IT IS A HELPER, NOT A CHECK. It writes: that is its job. -Check is the
# read-only half and satisfies the check contract in scripts/README.md; the
# rest of the script deliberately does not.
#
# Usage:
#   pwsh -NoProfile -File scripts/common/git-sync.ps1 -Check
#   pwsh -NoProfile -File scripts/common/git-sync.ps1 -Message "Subject" -BodyFile msg.txt
#   pwsh -NoProfile -File scripts/common/git-sync.ps1 -Message "Subject" -NoPush
#   pwsh -NoProfile -File scripts/common/git-sync.ps1 -PushOnly
#   pwsh -NoProfile -File scripts/common/git-sync.ps1 -Message "Subject" -Gate "sh scripts/common/check-docs.sh"
#
# Exit codes: 0 done, 1 a rule was broken or a gate failed, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

# -- PSScriptAnalyzer, suppressed per rule with the reason --------------------
# CI runs Invoke-ScriptAnalyzer over scripts/ at Error and Warning, so a
# suppression here is the difference between a red gate and a green one. Each
# is scoped to ONE rule and carries its justification. ⛔ Do not replace these
# with a settings file that switches the rule off everywhere: that weakens the
# gate for every future script to spare this one.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSReviewUnusedParameter', '',
    Justification = 'Gate and SkipGates are read by Invoke-Gates through script scope rather than as arguments. The analyzer does not follow that, and threading them through the call to satisfy it would make the code worse.')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '',
    Justification = 'Invoke-Gates runs every gate, plural, and Invoke-GitAs is a verb plus the preposition "as" rather than a plural noun. Renaming either to satisfy the rule would make the name describe the thing less accurately.')]
# ⛔ PositionalBinding OFF, and it is not tidiness. Called through -File, the
# CALLING shell evaluates an array and hands the child separate command-line
# arguments, so `-Gate "a","b","c","d"` gave the child four of them: one bound
# to -Gate and the other three bound POSITIONALLY, in declaration order, to
# -Name, -Email and -Branch. This script then made a commit whose author and
# committer were a shell command, printed "identity verified" one line under
# it, and tried to push a branch called `sh scripts/common/check-changelog.sh`.
# The push failing is what stopped it reaching a remote, which is luck rather
# than a guard. With this off, a stray positional argument does not bind at all
# and nothing runs. TOOL-03.
[CmdletBinding(PositionalBinding = $false)]
param(
    [string]$Message = '',
    [string]$BodyFile = '',
    [string]$Name = '',
    [string]$Email = '',
    [string]$Branch = '',
    [string[]]$Path = @(),
    [string[]]$Gate = @(),
    [switch]$NoPush,
    [switch]$PushOnly,
    [switch]$Check,
    [switch]$SkipGates,
    [switch]$NoCi,
    [switch]$Json
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine('git-sync: git not found')
    exit 2
}
$root = (& git rev-parse --show-toplevel 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $root) {
    [Console]::Error.WriteLine('git-sync: not a git repository')
    exit 2
}
$root = ($root | Select-Object -First 1).Trim()
Set-Location $root

function Write-Step([string]$Text) {
    Write-Output ((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ') + ' git-sync: ' + $Text)
}
function Exit-With([int]$Code, [string]$Text) {
    [Console]::Error.WriteLine('git-sync: ' + $Text)
    exit $Code
}

# ⛔ NOTHING IS INVENTED. If neither the flags nor git config name a person,
# the script refuses. Guessing an identity onto somebody's commit is worse than
# not committing, because it is a claim about who wrote something.
if (-not $Name)  { $Name  = (& git config user.name 2>$null);  if ($LASTEXITCODE -ne 0) { $Name = '' } }
if (-not $Email) { $Email = (& git config user.email 2>$null); if ($LASTEXITCODE -ne 0) { $Email = '' } }
if ($Name)  { $Name  = ($Name  | Select-Object -First 1).Trim() }
if ($Email) { $Email = ($Email | Select-Object -First 1).Trim() }
if (-not $Name -or -not $Email) {
    [Console]::Error.WriteLine('git-sync: no identity. Pass -Name and -Email, or set git config')
    [Console]::Error.WriteLine('  user.name and user.email. Nothing is guessed here.')
    exit 2
}
$ident = $Name + ' <' + $Email + '>'

# `git -c` on every invocation. Committer as well as author: --author sets only
# the author and the two can disagree.
function Invoke-GitAs {
    param([string[]]$GitArgs)
    $prefix = @(
        '-c', ('user.name=' + $Name),
        '-c', ('user.email=' + $Email),
        '-c', ('committer.name=' + $Name),
        '-c', ('committer.email=' + $Email)
    )
    & git @prefix @GitArgs
}

if (-not $Branch) {
    $Branch = (& git rev-parse --abbrev-ref HEAD 2>$null)
    if ($LASTEXITCODE -ne 0 -or -not $Branch) { $Branch = 'main' }
    $Branch = ($Branch | Select-Object -First 1).Trim()
}

# -- rule 1: no tool is credited in a commit ---------------------------------
# ⛔ REFUSED, NOT STRIPPED. Rewriting a message to make it pass is how the
# author never finds out, and the same line arrives again next time.
#
# ⚠ THE ANTHROPIC ADDRESS IS WRITTEN WITH A BRACKETED DOT ON PURPOSE. Spelled
# as a plain address it is a valid email, and check-no-secrets --public refuses
# a tracked email address, so this guard's own source would have failed the
# secret sweep. The bracket is a no-op to the regex engine and breaks the shape
# the sweep looks for. It costs one comment and saves a false red.
#
# ⚠ Case-insensitive on purpose. "Co-Authored-By" and "co-authored-by" are the
# same violation, and a guard that caught only one spelling would be a guard
# that catches whichever one nobody uses.
$attribution = @(
    '^\s*co-authored-by:',
    'generated\s+with\s+\[?claude',
    'generated\s+by\s+(claude|chatgpt|gpt-|copilot|cursor|codex|gemini|llm|an?\s+ai)',
    'written\s+by\s+(claude|chatgpt|gpt-|copilot|an?\s+ai)',
    'with\s+assistance\s+from\s+(claude|chatgpt|copilot|an?\s+ai)',
    'claude\s+(code|opus|sonnet|haiku)',
    'anthropic',
    '^\s*(assisted|authored)-by:\s*(claude|chatgpt|copilot)',
    'noreply@anthropic[.]com'
) -join '|'

# -- rule 2: a CI skip is deliberate or it is not there ----------------------
# Every marker the platform honours, matched the way it matches them:
# case-insensitively and anywhere in the message. That is why a sentence ABOUT
# one is one.
$ciSkip = '\[skip[ _-]?ci\]|\[ci[ _-]?skip\]|\[no[ _-]?ci\]|\[skip[ _-]?actions\]|\[actions[ _-]?skip\]'

# ⚠ EVERY CALLER WRAPS THIS IN @(). PowerShell's `return` UNROLLS a collection,
# so a pattern matching exactly once hands back a scalar, and `.Count` on a
# scalar throws under Set-StrictMode. The failure therefore appeared only when
# a rule FIRED, which is the one path a green run never exercises: the refusal
# crashed instead of refusing, and the exit code was 1 either way, so it looked
# correct from the outside.
function Find-Match([string]$Text, [string]$Pattern) {
    $hits = New-Object System.Collections.ArrayList
    $n = 0
    foreach ($line in ($Text -split "`r?`n")) {
        $n++
        if ($line -imatch $Pattern) { [void]$hits.Add($n.ToString() + ':' + $line) }
    }
    return @($hits | Where-Object { $_ })
}

# -- the message -------------------------------------------------------------
# ⛔ THE BODY COMES FROM A FILE. docs/conventions/shell.md section 1: a body
# passed as a shell string loses its quoting, and the way it fails is worse
# than an error. Nothing errors, and a fragment of the body is executed or
# dropped somewhere in the middle.
$msgText = ''
if ($Message) {
    $msgText = $Message + "`n"
    if ($BodyFile) {
        if (-not (Test-Path -LiteralPath $BodyFile -PathType Leaf)) {
            Exit-With 2 ("-BodyFile '" + $BodyFile + "' does not exist.")
        }
        $msgText += "`n" + [System.IO.File]::ReadAllText($BodyFile)
    }
}

# -- -Check: the read-only half ----------------------------------------------
if ($Check) {
    $problems = 0

    if ($msgText) {
        $hits = @(Find-Match $msgText $attribution)
        if ($hits.Count -gt 0) {
            [Console]::Error.WriteLine('git-sync: the message carries attribution:')
            $hits | ForEach-Object { [Console]::Error.WriteLine($_) }
            $problems++
        }
        else { Write-Step 'message carries no attribution' }
    }

    # ⭐ THE LAST COMMIT IS CHECKED TOO, so a bad one that landed some other way
    # is still caught. A guard that only inspects what it is asked to write
    # cannot see what somebody committed around it.
    $head = (& git log -1 --pretty='%an <%ae>%n%cn <%ce>%n%B' 2>$null) -join "`n"
    if ($LASTEXITCODE -eq 0 -and $head) {
        $hits = @(Find-Match $head $attribution)
        if ($hits.Count -gt 0) {
            [Console]::Error.WriteLine('git-sync: HEAD commit carries attribution:')
            $hits | ForEach-Object { [Console]::Error.WriteLine($_) }
            $problems++
        }
        else { Write-Step 'HEAD commit is clean' }

        $who = ((& git log -1 --pretty='%an <%ae>|%cn <%ce>' 2>$null) | Select-Object -First 1).Trim()
        if ($who -ne ($ident + '|' + $ident)) {
            [Console]::Error.WriteLine('git-sync: HEAD identity is ' + $who + ', expected ' + $ident + '|' + $ident)
            $problems++
        }
        else { Write-Step ('HEAD identity is ' + $ident + ', author and committer') }
    }

    if ($Json) { Write-Output ('{"schema":"git-sync/1","problems":' + $problems + '}') }
    if ($problems -gt 0) { exit 1 }
    Write-Step 'all checks pass'
    exit 0
}

# -- the gates, BEFORE the commit --------------------------------------------
# ⛔ IT SETS A FLAG; IT DOES NOT RETURN ONE. A PowerShell function returns
# EVERYTHING written to its output stream, so `return $true` after a gate that
# printed anything hands the caller an ARRAY, and a non-empty array is truthy.
# A FAILING GATE THEREFORE PASSED: the commit landed and the script exited 0.
# ⚠ It looked correct from the outside, because the gate's own output appeared
# in the transcript exactly as it would on a real failure. Caught by asserting
# the exit code of a run whose gate was `exit 3`, not by reading the output.
$script:gatesOk = $true
function Invoke-Gates {
    $script:gatesOk = $true
    if ($SkipGates) {
        Write-Step 'GATES SKIPPED by -SkipGates. This push carries no proof the tree is green.'
        return
    }
    if ($Gate.Count -eq 0) { Write-Step 'no -Gate given, nothing to run'; return }
    foreach ($g in $Gate) {
        if (-not $g) { continue }
        Write-Step ('gate: ' + $g)
        # ⛔ Unpiped, and the exit code is read from the process that produced
        # it. $PSNativeCommandUseErrorActionPreference is false by default from
        # pwsh 7.4, so a gate writing to stderr does not terminate on its own.
        # ⚠ The gate's output goes to the HOST, not down this function's output
        # stream, which is the other half of the bug above.
        $prev = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            if ($IsWindows) { & cmd.exe /c $g 2>&1 | Out-Host }
            else { & /bin/sh -c $g 2>&1 | Out-Host }
        }
        finally { $ErrorActionPreference = $prev }
        if ($LASTEXITCODE -ne 0) { $script:gatesOk = $false; return }
    }
}

# -- commit ------------------------------------------------------------------
if (-not $PushOnly) {
    if (-not $msgText) { Exit-With 2 '-Message is required unless -PushOnly or -Check.' }

    $hits = @(Find-Match $msgText $attribution)
    if ($hits.Count -gt 0) {
        [Console]::Error.WriteLine('git-sync: the commit message carries AI attribution and will NOT be')
        [Console]::Error.WriteLine('rewritten for you:')
        $hits | ForEach-Object { [Console]::Error.WriteLine($_) }
        [Console]::Error.WriteLine('')
        [Console]::Error.WriteLine('Remove it and run again. docs/conventions/git.md.')
        exit 1
    }

    # ⚠ Checked BEFORE the gates, not after. Finding this out after a long test
    # run is finding it out late.
    if (-not $NoCi) {
        $skips = @(Find-Match $msgText $ciSkip)
        if ($skips.Count -gt 0) {
            [Console]::Error.WriteLine('git-sync: the message carries a CI skip marker and -NoCi was not')
            [Console]::Error.WriteLine('passed, so this push would silently start no run:')
            $skips | ForEach-Object { [Console]::Error.WriteLine($_) }
            [Console]::Error.WriteLine('')
            [Console]::Error.WriteLine('Write the marker some other way, or pass -NoCi if you meant it.')
            exit 1
        }
    }

    if ($Path.Count -gt 0) {
        foreach ($p in $Path) {
            if (-not $p) { continue }
            & git add -- $p
            if ($LASTEXITCODE -ne 0) { Exit-With 1 ('git add failed for ' + $p) }
        }
        Write-Step 'staged the named path(s)'
    }
    else {
        & git add -A
        if ($LASTEXITCODE -ne 0) { Exit-With 1 'git add -A failed' }
        Write-Step 'staged everything not ignored'
    }

    $staged = @(& git diff --cached --name-only 2>$null | Where-Object { $_ })
    if ($staged.Count -eq 0) { Exit-With 1 'nothing staged, so there is nothing to commit.' }
    Write-Step ($staged.Count.ToString() + ' file(s) staged')

    Invoke-Gates
    if (-not $script:gatesOk) { Exit-With 1 'a gate failed. Nothing has been pushed.' }

    if ($NoCi) {
        # On its own line at the end, so the subject stays readable in a log and
        # a reader can see in `git log` which pushes were never checked.
        $msgText += "`n[skip ci]`n"
    }

    # ⛔ UTF-8 WITHOUT A BOM. A BOM on a commit-message file ends up as three
    # bytes at the front of the subject line, and the subject is what every
    # log, every release note and every search reads first.
    $msgFile = Join-Path ([System.IO.Path]::GetTempPath()) ('git-sync-commit-' + $PID + '.txt')
    [System.IO.File]::WriteAllText($msgFile, $msgText, (New-Object System.Text.UTF8Encoding($false)))
    try {
        Invoke-GitAs -GitArgs @('commit', '--file', $msgFile) | Out-Null
        if ($LASTEXITCODE -ne 0) { Exit-With 1 'git commit failed' }
    }
    finally { Remove-Item -LiteralPath $msgFile -Force -ErrorAction SilentlyContinue }

    Write-Step ('committed ' + ((& git log -1 --pretty='%h %s') | Select-Object -First 1))

    # ⭐ VERIFY RATHER THAN ASSUME. `-c` can be overridden by a hook or by an
    # environment variable, and a commit that landed with the wrong identity is
    # not fixed by having asked nicely.
    $who = ((& git log -1 --pretty='%an <%ae>|%cn <%ce>') | Select-Object -First 1).Trim()
    if ($who -ne ($ident + '|' + $ident)) {
        Exit-With 1 ("the commit landed as '" + $who + "', not '" + $ident + "'. Something overrode -c.")
    }
    Write-Step ('identity verified: ' + $ident + ', author and committer')
}
else {
    Invoke-Gates
    if (-not $script:gatesOk) { Exit-With 1 'a gate failed. Nothing has been pushed.' }
}

# -- push --------------------------------------------------------------------
if ($NoPush) {
    Write-Step '-NoPush, stopping before the push'
    exit 0
}

Write-Step ('pushing ' + $Branch + ' to origin')
& git push origin $Branch
if ($LASTEXITCODE -ne 0) { Exit-With 1 'git push failed' }
Write-Step 'pushed'
exit 0
