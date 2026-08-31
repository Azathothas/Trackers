# The only sanctioned way to commit and push in this repository.
#
# Every rule this script enforces has cost a session at least once. They are
# written down in TODO/RULES.md section 4; this file is what makes them
# mechanical instead of remembered.
#
# What it enforces:
#
#   1. Author and committer are Azathothas <AjamX101@gmail.com>, set per
#      invocation with `-c`, so a machine with different global config still
#      produces the right commits.
#   2. No AI attribution: no Co-Authored-By naming a model or tool, no
#      "generated with" line, no tool name in the body. Refused, not stripped,
#      because silently editing a commit message is worse than refusing one.
#   3. Nothing under reference/ reaches main, even with -Force.
#   4. The gates run before the push, not after.
#   5. The corpus is pushed to the `references` branch so it survives a lost
#      machine without entering main's history.
#
# Usage:
#
#   # stage everything, commit, run the gates, push, sync the corpus
#   pwsh -NoProfile -File scripts/git-sync.ps1 -Message "Subject" -Body "..."
#
#   # commit only, no push
#   pwsh -NoProfile -File scripts/git-sync.ps1 -Message "Subject" -NoPush
#
#   # push what is already committed
#   pwsh -NoProfile -File scripts/git-sync.ps1 -PushOnly
#
#   # stage specific paths rather than everything
#   pwsh -NoProfile -File scripts/git-sync.ps1 -Message "Subject" -Path README.md,TODO/INDEX.md
#
#   # add one benchmark that IS the evidence for an entry
#   pwsh -NoProfile -File scripts/git-sync.ps1 -Message "Subject" -Evidence bench/soak-20260821T012428252Z.json
#
#   # check the rules without doing anything
#   pwsh -NoProfile -File scripts/git-sync.ps1 -Check
#
#   # get reference/ onto a fresh clone from the references branch
#   pwsh -NoProfile -File scripts/git-sync.ps1 -FetchReferences
#
#   # a body with apostrophes, backticks or dollars in it: pass a file, never
#   # a shell string
#   pwsh -NoProfile -File scripts/git-sync.ps1 -Message "Subject" -BodyFile msg.txt
#
#   # a documentation-only push, with no CI run to pay for
#   pwsh -NoProfile -File scripts/git-sync.ps1 -Message "Subject" -NoCi
#
#   # and print what the session did on the way out
#   pwsh -NoProfile -File scripts/git-sync.ps1 -Message "Subject" -Summary -Since 2026-08-22T01:11:24Z
#
# Exit codes: 0 all good, 1 a rule was broken or a gate failed, 2 the script
# could not run (not a git repository, no message, git missing).
#
# See TODO/RULES.md.

[CmdletBinding()]
param(
    # Commit subject. Required unless -PushOnly, -Check or -FetchReferences.
    [string]$Message,

    # Commit body. Blank line inserted between subject and body.
    #
    # Prefer -BodyFile. A body typed into a shell has to survive that shell's
    # quoting, and a body with an apostrophe in it does not: a PowerShell
    # here-string passed from bash ends at the first `'`, and the rest of the
    # message becomes commands. That has cost this repository two failed
    # pushes.
    [string]$Body,

    # Read the commit body from a UTF-8 file. Nothing is interpreted, so an
    # apostrophe, a backtick, a `$`, or a heredoc marker is just text.
    [string]$BodyFile,

    # Paths to stage. Default is everything tracked and untracked, minus
    # whatever .gitignore excludes.
    [string[]]$Path,

    # Paths to force-add past .gitignore. For a benchmark that IS the evidence
    # for a TODO entry. Refused for anything under reference/.
    [string[]]$Evidence,

    # Commit but do not push.
    [switch]$NoPush,

    # Push what is already committed. No staging, no commit.
    [switch]$PushOnly,

    # Run every check and report, change nothing.
    [switch]$Check,

    # Restore reference/ from the references branch into the working tree.
    [switch]$FetchReferences,

    # Skip the gates. For a documentation-only change where the tree is known
    # green. Recorded in the output so it is visible in a transcript.
    [switch]$SkipGates,

    # Do not sync the corpus to the references branch on this push.
    [switch]$NoReferences,

    # Mark the commit so GitHub Actions does not run. For a push that changes
    # nothing a job could fail on: documentation, TODO entries, a script the
    # workflow does not call. A CI run costs sixteen jobs and about five
    # minutes, and pushing again while one is in flight cancels it, so a push
    # that cannot go red is a push worth not paying for.
    #
    # Refused unless the staged paths are all in the safe set, because a
    # "documentation-only" push that carries a source file is exactly the one
    # that needed CI. -Force overrides, and says so.
    [switch]$NoCi,

    # Print what the session did: files, lines, entries closed, and how long it
    # took. Needs -Since to measure elapsed time.
    [switch]$Summary,

    # ISO 8601 UTC instant the session started, for -Summary.
    [string]$Since,

    # Override a refusal that is a judgement rather than a rule. Today only
    # -NoCi has one. Never overrides the reference/ rule or the identity rule.
    [switch]$Force,

    [string]$Branch = "main",
    [string]$ReferenceBranch = "references"
)

$ErrorActionPreference = 'Stop'

# `git` writes progress to stderr on success, and
# $PSNativeCommandUseErrorActionPreference is false by default from pwsh 7.4,
# so stderr alone never terminates. Every git call below is checked on
# $LASTEXITCODE instead.
$script:RepoRoot = Split-Path -Parent $PSScriptRoot

$AuthorName = "Azathothas"
$AuthorEmail = "AjamX101@gmail.com"

function Write-Step([string]$text) {
    $stamp = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    Write-Host "$stamp git-sync: $text"
}

function Exit-With([int]$code, [string]$text) {
    [Console]::Error.WriteLine("git-sync: $text")
    exit $code
}

# Run git and return its stdout. Terminates on a non-zero exit unless
# -AllowFail. Named $gitArgs rather than $args, because $args inside a function
# is an automatic variable that silently swallows a parameter of that name.
function Invoke-Git {
    param([string[]]$gitArgs, [switch]$AllowFail)
    $output = & git @gitArgs 2>&1
    $code = $LASTEXITCODE
    if ($code -ne 0 -and -not $AllowFail) {
        Exit-With 1 "git $($gitArgs -join ' ') failed with exit $code`n$output"
    }
    $script:LastGitExit = $code
    return ($output | Out-String)
}

# Same, with the identity pinned. Author and committer both, because
# `git commit --author` sets only the author.
function Invoke-GitAs {
    param([string[]]$gitArgs)
    $prefix = @(
        "-c", "user.name=$AuthorName",
        "-c", "user.email=$AuthorEmail",
        "-c", "committer.name=$AuthorName",
        "-c", "committer.email=$AuthorEmail"
    )
    return (Invoke-Git -gitArgs ($prefix + $gitArgs))
}

Set-Location $script:RepoRoot
Invoke-Git -gitArgs @("rev-parse", "--git-dir") | Out-Null

# `pwsh -File` hands each shell word to PowerShell as one string, so `-Path
# a,b,c` typed in bash arrives as the single path "a,b,c" and git reports a
# pathspec that matches nothing. Splitting here makes the flag mean the same
# thing from either shell, which is what a flag has to do.
function Split-List {
    param([string[]]$values)
    if (-not $values) { return @() }
    return @($values | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
}
$Path = Split-List $Path
$Evidence = Split-List $Evidence

# ---------------------------------------------------------------------------
# Rule 2: no AI attribution
# ---------------------------------------------------------------------------
#
# -cmatch throughout where case is not the signal we want to ignore. These are
# case-insensitive on purpose: "co-authored-by" and "Co-Authored-By" are the
# same violation. The tool names are matched with word boundaries so a legitimate
# sentence mentioning a file called `claude.rs` would not trip, but an
# attribution line would.
$AttributionPatterns = @(
    '(?im)^\s*co-authored-by:',
    '(?i)generated\s+with\s+\[?claude',
    '(?i)\bgenerated\s+by\s+(claude|chatgpt|gpt-|copilot|cursor|codex|gemini|llm|an?\s+ai\b)',
    '(?i)\bwritten\s+by\s+(claude|chatgpt|gpt-|copilot|an?\s+ai\b)',
    '(?i)\bwith\s+assistance\s+from\s+(claude|chatgpt|copilot|an?\s+ai\b)',
    '(?i)\bclaude\s+(code|opus|sonnet|haiku)\b',
    '(?i)\banthropic\b',
    '(?i)^\s*(assisted|authored)-by:\s*(claude|chatgpt|copilot)',
    '(?i)\bnoreply@anthropic\.com\b',
    '(?i)🤖'
)

function Test-Attribution([string]$text) {
    $hits = @()
    foreach ($pattern in $AttributionPatterns) {
        if ($text -match $pattern) { $hits += $Matches[0].Trim() }
    }
    return $hits
}

# ---------------------------------------------------------------------------
# Rule 3: nothing under reference/ reaches main
# ---------------------------------------------------------------------------

function Get-ForbiddenStaged {
    $staged = (Invoke-Git -gitArgs @("diff", "--cached", "--name-only")) -split "`r?`n" |
        Where-Object { $_ -and $_.Trim() }
    return @($staged | Where-Object { $_ -match '^reference/' -or $_ -eq 'reference' })
}

# ---------------------------------------------------------------------------
# Rule 4: a CI skip is deliberate or it is not there
# ---------------------------------------------------------------------------
#
# Every marker GitHub Actions honours in a commit message. They are matched
# case-insensitively and anywhere in the message, which is exactly how GitHub
# reads them, and why a sentence about one is one.

$script:CiSkipPatterns = @(
    '(?i)\[skip[ _-]?ci\]',
    '(?i)\[ci[ _-]?skip\]',
    '(?i)\[no[ _-]?ci\]',
    '(?i)\[skip[ _-]?actions\]',
    '(?i)\[actions[ _-]?skip\]'
)

# ---------------------------------------------------------------------------
# Rule 5: -NoCi only where CI could not have caught anything
# ---------------------------------------------------------------------------
#
# The safe set is named by what it is rather than by what it is not, so a new
# kind of file defaults to needing CI. Anything outside it is a path some job
# builds, runs, or lints, and "it is only a small change" is the sentence that
# precedes a red matrix.
#
# `.github/` is deliberately **not** safe: a workflow edit is the one change
# whose effect is only visible in a run.

# `patches/` is safe for the same reason `TODO/` is: nothing in it is a build
# input. The `.patch` files are DERIVED from `vendor/` by
# scripts/vendor-diff.ps1 and are never applied to anything, so editing one
# changes no byte cargo compiles. `vendor/` itself is **not** in this list and
# must not be: that is source, and it is the source CI exists to build.

$script:SafeForNoCi = @(
    '^TODO/',
    '^docs/',
    '^bench/',
    '^patches/',
    '^scripts/',
    '^README\.md$',
    '^LICENSE',
    '^\.gitignore$',
    '^\.gitattributes$'
)

# `scripts/` is safe except for the scripts a workflow actually runs, and which
# those are is derived from the workflows rather than listed here. A workflow
# that starts calling a new script makes that script unsafe on the same commit,
# with nothing to remember.
function Get-ScriptsCiRuns {
    $dir = Join-Path $script:RepoRoot ".github/workflows"
    if (-not (Test-Path $dir)) { return @() }
    $names = [System.Collections.Generic.HashSet[string]]::new()
    foreach ($file in (Get-ChildItem -Path $dir -File)) {
        foreach ($m in [regex]::Matches([System.IO.File]::ReadAllText($file.FullName), 'scripts/[A-Za-z0-9._-]+')) {
            [void]$names.Add($m.Value)
        }
    }
    return @($names)
}

function Test-NeedsCi {
    param([string[]]$staged)
    $ciRuns = Get-ScriptsCiRuns
    return @($staged | Where-Object {
            $path = $_
            if ($ciRuns -contains $path) { return $true }
            -not ($script:SafeForNoCi | Where-Object { $path -match $_ })
        })
}

# ---------------------------------------------------------------------------
# -FetchReferences
# ---------------------------------------------------------------------------

if ($FetchReferences) {
    Write-Step "fetching $ReferenceBranch from origin"
    Invoke-Git -gitArgs @("fetch", "origin", "${ReferenceBranch}:refs/remotes/origin/$ReferenceBranch") -AllowFail | Out-Null
    if ($script:LastGitExit -ne 0) {
        Exit-With 1 "origin has no '$ReferenceBranch' branch yet. Push one first with a normal run of this script."
    }
    Write-Step "restoring reference/ into the working tree"
    Invoke-Git -gitArgs @("checkout", "origin/$ReferenceBranch", "--", "reference") | Out-Null
    # `git checkout <ref> -- <path>` stages what it restores. reference/ must
    # never be staged on main, so unstage it immediately and leave the files.
    Invoke-Git -gitArgs @("reset", "--", "reference") -AllowFail | Out-Null
    $count = (Get-ChildItem -Recurse -File -Path (Join-Path $script:RepoRoot "reference") -ErrorAction SilentlyContinue).Count
    Write-Step "reference/ restored, $count files, unstaged"
    exit 0
}

# ---------------------------------------------------------------------------
# -Check
# ---------------------------------------------------------------------------

if ($Check) {
    $problems = 0

    $forbidden = Get-ForbiddenStaged
    if ($forbidden.Count -gt 0) {
        [Console]::Error.WriteLine("git-sync: staged paths under reference/: $($forbidden -join ', ')")
        $problems++
    }
    else { Write-Step "no staged path under reference/" }

    if ($Message) {
        $hits = Test-Attribution "$Message`n$Body"
        if ($hits.Count -gt 0) {
            [Console]::Error.WriteLine("git-sync: attribution in the message: $($hits -join '; ')")
            $problems++
        }
        else { Write-Step "message carries no attribution" }
    }

    # The last commit on this branch, so a bad one that landed some other way
    # is still caught.
    $subject = (Invoke-Git -gitArgs @("log", "-1", "--pretty=%an <%ae>%n%B")).Trim()
    $hits = Test-Attribution $subject
    if ($hits.Count -gt 0) {
        [Console]::Error.WriteLine("git-sync: HEAD commit carries attribution: $($hits -join '; ')")
        $problems++
    }
    else { Write-Step "HEAD commit is clean" }

    $who = (Invoke-Git -gitArgs @("log", "-1", "--pretty=%an <%ae>|%cn <%ce>")).Trim()
    $expected = "$AuthorName <$AuthorEmail>"
    if ($who -ne "$expected|$expected") {
        [Console]::Error.WriteLine("git-sync: HEAD identity is '$who', expected '$expected|$expected'")
        $problems++
    }
    else { Write-Step "HEAD identity is $expected, author and committer" }

    if ($problems -gt 0) { exit 1 }
    Write-Step "all checks pass"
    exit 0
}

# ---------------------------------------------------------------------------
# The gates
# ---------------------------------------------------------------------------

function Invoke-Gates {
    if ($SkipGates) {
        Write-Step "GATES SKIPPED by -SkipGates. The push carries no proof the tree is green."
        return
    }

    Write-Step "cargo fmt --all --check"
    & cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { Exit-With 1 "cargo fmt --all --check failed. Run 'cargo fmt --all'." }

    Write-Step "cargo clippy --workspace --all-targets --all-features -- -D warnings"
    & cargo clippy --workspace --all-targets --all-features -- -D warnings
    if ($LASTEXITCODE -ne 0) { Exit-With 1 "clippy failed." }

    Write-Step "cargo test --workspace"
    # Per-run, not per-machine. A fixed name means two pushes at once collide
    # on it and the second dies naming a locked file rather than the other run.
    # See TODO/cli-surface.md, T-228.
    $testLog = Join-Path ([System.IO.Path]::GetTempPath()) "bit-cli-git-sync-tests-$PID.txt"
    & cargo test --workspace 2>&1 | Tee-Object -FilePath $testLog | Out-Null
    $testExit = $LASTEXITCODE

    # Filter for the test name, not the summary line: -match is
    # case-insensitive, so 'FAILED' would match "0 failed" and a flake's name
    # would be lost. -cmatch and the leading 'test ' is the signal.
    $failed = @(Select-String -Path $testLog -Pattern '^test \S+ \.\.\. FAILED' -CaseSensitive |
        ForEach-Object { $_.Line.Trim() })
    if ($failed.Count -gt 0) {
        Exit-With 1 "$($failed.Count) test(s) failed:`n  $($failed -join "`n  ")"
    }
    if ($testExit -ne 0) { Exit-With 1 "cargo test --workspace exited $testExit with no named failure. See $testLog." }

    $passed = 0
    foreach ($line in (Select-String -Path $testLog -Pattern '^test result: ok\. (\d+) passed')) {
        $passed += [int]$line.Matches[0].Groups[1].Value
    }
    Write-Step "$passed tests passed, 0 failed"
}

# ---------------------------------------------------------------------------
# Sync the corpus to the references branch
# ---------------------------------------------------------------------------
#
# An orphan branch holding reference/ and nothing else. When it is pushed it is
# force-pushed, because it is a mirror of a working directory rather than a
# history: the corpus has no commits worth bisecting and a growing history of
# a 52 MB tree is a cost with no reader. It is pushed only when the tree hash
# differs from what the remote already holds, which for a corpus that changes
# about once a month is almost never.
#
# Built in a temporary index so the working tree's index is never touched, and
# so a failure here cannot leave reference/ staged on main.

function Sync-References {
    $corpus = Join-Path $script:RepoRoot "reference"
    if (-not (Test-Path $corpus)) {
        Write-Step "no reference/ on disk, nothing to sync"
        return
    }

    $tempIndex = Join-Path ([System.IO.Path]::GetTempPath()) "bit-cli-references-index-$PID"
    if (Test-Path $tempIndex) { Remove-Item -Force $tempIndex }
    $previousIndex = $env:GIT_INDEX_FILE
    $env:GIT_INDEX_FILE = $tempIndex
    try {
        Write-Step "building the $ReferenceBranch tree from reference/"
        # --force because reference/ is gitignored on main, which is the point.
        # This index is thrown away, so it cannot leak into a main commit.
        Invoke-Git -gitArgs @("add", "--force", "--", "reference") | Out-Null
        $tree = (Invoke-Git -gitArgs @("write-tree")).Trim()
        if (-not $tree) { Exit-With 1 "could not write a tree for reference/" }

        $stamp = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
        $head = (Invoke-Git -gitArgs @("rev-parse", "--short", "HEAD")).Trim()
        $files = (Get-ChildItem -Recurse -File -Path $corpus).Count
        $commitMessage = @"
Corpus as of $stamp

Mirror of reference/ at main $head. $files files.
Not a history: this branch is force-pushed and holds only the current corpus.
See TODO/reference-map.md and reference/RESEARCH.md.
"@
        # Nothing to do when the remote already holds these exact bytes. The
        # tree hash is the whole comparison: it is the content of reference/
        # and nothing else, so an equal hash means an equal corpus. Before
        # this, every push force-pushed 52 MB whether or not a single file had
        # changed, which is most pushes.
        #
        # `ls-remote` is one round trip and no objects. The commit it names is
        # local whenever this script pushed it, which is the ordinary case; on
        # a fresh clone it is not, and then there is nothing to compare and the
        # push happens.
        $remote = (Invoke-Git -gitArgs @("ls-remote", "origin", "refs/heads/$ReferenceBranch") -AllowFail).Trim()
        $remoteHead = if ($remote) { ($remote -split "\s+")[0] } else { $null }
        if ($remoteHead) {
            $remoteTree = (Invoke-Git -gitArgs @("rev-parse", "--verify", "--quiet", "${remoteHead}^{tree}") -AllowFail).Trim()
            if ($remoteTree -eq $tree) {
                Write-Step "origin/$ReferenceBranch already holds this tree ($files files), nothing to push"
                return
            }
        }

        $commit = (Invoke-GitAs -gitArgs @("commit-tree", $tree, "-m", $commitMessage)).Trim()
        if (-not $commit) { Exit-With 1 "could not create the $ReferenceBranch commit" }

        Write-Step "pushing $files files to origin/$ReferenceBranch"
        Invoke-Git -gitArgs @("push", "--force", "origin", "${commit}:refs/heads/$ReferenceBranch") | Out-Null
        Write-Step "origin/$ReferenceBranch now holds $files files at $($commit.Substring(0,7))"
    }
    finally {
        if ($null -eq $previousIndex) { Remove-Item Env:GIT_INDEX_FILE -ErrorAction SilentlyContinue }
        else { $env:GIT_INDEX_FILE = $previousIndex }
        if (Test-Path $tempIndex) { Remove-Item -Force $tempIndex -ErrorAction SilentlyContinue }
    }
}

# ---------------------------------------------------------------------------
# Commit
# ---------------------------------------------------------------------------

if (-not $PushOnly) {
    if (-not $Message) { Exit-With 2 "-Message is required unless -PushOnly, -Check or -FetchReferences." }

    if ($BodyFile) {
        if ($Body) { Exit-With 2 "-Body and -BodyFile are two ways to say the same thing. Pass one." }
        if (-not (Test-Path -LiteralPath $BodyFile)) { Exit-With 2 "-BodyFile '$BodyFile' does not exist." }
        $Body = [System.IO.File]::ReadAllText((Resolve-Path -LiteralPath $BodyFile)).TrimEnd()
    }

    $hits = Test-Attribution "$Message`n$Body"
    if ($hits.Count -gt 0) {
        Exit-With 1 "the commit message carries AI attribution and will not be rewritten for you: $($hits -join '; '). Remove it and run again. See TODO/RULES.md section 4."
    }

    # A commit that says `[skip ci]` skips CI, and GitHub does not care whether
    # the sentence around it meant it. The commit that introduced -NoCi
    # explained the marker in prose, and that push shipped without a run:
    # sixteen jobs skipped, silently, on the one commit that changed the push
    # tool. Checked here rather than after the gates, because finding it out
    # after a five minute test run is finding it out late. Refused rather than
    # rewritten, for the same reason an attribution line is.
    if (-not $NoCi) {
        $skips = @($script:CiSkipPatterns | Where-Object { "$Message`n$Body" -match $_ })
        if ($skips.Count -gt 0) {
            Exit-With 1 ("the commit message carries a CI skip marker and -NoCi was not passed, so this push would " +
                "silently start no run. Write the marker some other way, or pass -NoCi if you meant it. " +
                "Matched: $($skips -join '; '). See TODO/RULES.md section 4.")
        }
    }

    $onBranch = (Invoke-Git -gitArgs @("rev-parse", "--abbrev-ref", "HEAD")).Trim()
    if ($onBranch -ne $Branch) {
        Write-Step "on '$onBranch', not '$Branch'. Committing there."
    }

    if ($Path) {
        Write-Step "staging $($Path.Count) path(s)"
        Invoke-Git -gitArgs (@("add", "--") + $Path) | Out-Null
    }
    else {
        Write-Step "staging everything not ignored"
        Invoke-Git -gitArgs @("add", "-A") | Out-Null
    }

    if ($Evidence) {
        foreach ($item in $Evidence) {
            if ($item -match '^reference[/\\]') {
                Exit-With 1 "-Evidence '$item' is under reference/. That never enters main. See TODO/RULES.md section 4."
            }
            Write-Step "force-adding evidence: $item"
            Invoke-Git -gitArgs @("add", "--force", "--", $item) | Out-Null
        }
    }

    $forbidden = Get-ForbiddenStaged
    if ($forbidden.Count -gt 0) {
        Invoke-Git -gitArgs (@("reset", "--") + $forbidden) -AllowFail | Out-Null
        Exit-With 1 "refusing to commit paths under reference/: $($forbidden -join ', '). They have been unstaged. reference/ belongs on the '$ReferenceBranch' branch. See TODO/RULES.md section 4."
    }

    $staged = (Invoke-Git -gitArgs @("diff", "--cached", "--name-only")) -split "`r?`n" |
        Where-Object { $_ -and $_.Trim() }
    if ($staged.Count -eq 0) {
        Exit-With 1 "nothing staged, so there is nothing to commit."
    }
    Write-Step "$($staged.Count) file(s) staged"

    # What is staged is what the commit will contain, so this is the moment to
    # ask whether all of it belongs here.
    #
    # `under/inner.bin` reached the remote on 2026-08-23 through this exact
    # path: an acceptance run left a 1,000 byte payload in the working tree,
    # `git add -A` above took it, and no rule anywhere compared the result
    # against what this repository holds. `check-tree.ps1` reads the index,
    # which right now is the staged tree, so the answer is about this commit
    # rather than about the last one.
    #
    # Not behind -SkipGates. That switch is for a documentation change on a
    # tree known green, and a stray payload is likelier in exactly that push
    # than in one that ran the gates. It costs about a second.
    $treeCheck = Join-Path $PSScriptRoot "check-tree.ps1"
    $treeOut = (& pwsh -NoProfile -File $treeCheck 2>&1 | Out-String)
    if ($LASTEXITCODE -ne 0) {
        Invoke-Git -gitArgs @("reset") -AllowFail | Out-Null
        [Console]::Error.WriteLine($treeOut.TrimEnd())
        Exit-With 1 ("refusing to commit: the staged tree holds a path this repository does not account for. " +
            "Nothing has been committed and the index is reset. Move a run's output under .tmp/, or add the " +
            "kind to scripts/check-tree.ps1 on purpose. See TODO/cli-surface.md, T-230.")
    }
    Write-Step "tree ok, every staged path is a kind this repository keeps"

    if ($NoCi) {
        $risky = Test-NeedsCi $staged
        if ($risky.Count -gt 0 -and -not $Force) {
            Exit-With 1 ("-NoCi refused: these staged paths are ones CI is the check for: " +
                ($risky -join ', ') +
                ". Push them with CI, or pass -Force if you have a reason and can say what it is.")
        }
        if ($risky.Count -gt 0) {
            Write-Step "-NoCi -Force: skipping CI on a push that touches $($risky.Count) path(s) CI checks"
        }
    }

    Invoke-Gates

    $full = if ($Body) { "$Message`n`n$Body" } else { $Message }

    if ($NoCi) {
        # GitHub Actions reads this out of the head commit's message and skips
        # the run. It goes on its own line at the end so the subject stays
        # readable in a log, and so a reader can see in `git log` which pushes
        # were never checked.
        $full = "$full`n`n[skip ci]"
    }
    $messageFile = Join-Path ([System.IO.Path]::GetTempPath()) "bit-cli-commit-$PID.txt"
    # UTF-8 without a BOM: pwsh 7 defaults to that, and a BOM ends up as three
    # bytes at the front of the subject line.
    [System.IO.File]::WriteAllText($messageFile, $full, (New-Object System.Text.UTF8Encoding($false)))
    try {
        Invoke-GitAs -gitArgs @("commit", "--file", $messageFile) | Out-Null
    }
    finally {
        Remove-Item -Force $messageFile -ErrorAction SilentlyContinue
    }

    $head = (Invoke-Git -gitArgs @("log", "-1", "--pretty=%h %s")).Trim()
    Write-Step "committed $head"

    $who = (Invoke-Git -gitArgs @("log", "-1", "--pretty=%an <%ae>|%cn <%ce>")).Trim()
    $expected = "$AuthorName <$AuthorEmail>"
    if ($who -ne "$expected|$expected") {
        Exit-With 1 "the commit landed with identity '$who' rather than '$expected'. Something overrode -c."
    }
}
else {
    Invoke-Gates
}

# ---------------------------------------------------------------------------
# Push
# ---------------------------------------------------------------------------

if ($NoPush) {
    Write-Step "-NoPush, stopping before the push"
    exit 0
}

$onBranch = (Invoke-Git -gitArgs @("rev-parse", "--abbrev-ref", "HEAD")).Trim()
Write-Step "pushing $onBranch to origin"
Invoke-Git -gitArgs @("push", "origin", $onBranch) | Out-Null
Write-Step "pushed"

if (-not $NoReferences) { Sync-References }

# The run the push started. A push that leaves CI red without an entry naming
# why is not finished, so print the handle rather than making the caller find it.
$gh = Get-Command gh -ErrorAction SilentlyContinue
if ($NoCi) {
    Write-Step "[skip ci] on the commit, so no run was started. Nothing to read."
}
elseif ($gh) {
    Write-Step "the run this push started, once it registers:"
    & gh run list --limit 1
    Write-Host ""
    Write-Host "  Watch it:  gh run watch `$(gh run list --limit 1 --json databaseId --jq '.[0].databaseId')"
    Write-Host "  Read it:   gh run view `$(gh run list --limit 1 --json databaseId --jq '.[0].databaseId')"
}
else {
    Write-Step "gh is not on PATH. Check CI by hand before calling this finished."
}

# What the session did, measured. Printed after the push so the numbers include
# it. See scripts/session-report.ps1.
if ($Summary) {
    $report = Join-Path $PSScriptRoot "session-report.ps1"
    $reportArgs = @("-NoProfile", "-File", $report)
    if ($Since) { $reportArgs += @("-Since", $Since) }
    & pwsh @reportArgs
}

exit 0
