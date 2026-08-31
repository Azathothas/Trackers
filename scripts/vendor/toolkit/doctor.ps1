# doctor.ps1 - what host is this, what is installed, and what is this repo?
#
# The defect this exists to catch is an agent that assumes its environment.
# It is the PowerShell twin of scripts/doctor/doctor.sh and emits the same
# schema, agent-doctor/1. On Windows this is the one to prefer: it needs no
# POSIX layer, so it answers correctly on a machine with no Git Bash, no WSL
# and no msys.
#
# It is a PROBE, not a gate. A missing tool is data, not a failure, so it exits
# 0 whenever it ran. It exits 2 only when it could not run at all.
#
# It is read-only: no installer, no config change, no network call unless -Net
# is passed, and the only file it writes is a temp file it removes.
#
# Runs on Windows PowerShell 5.1 and on PowerShell 7+, on any OS 7+ supports.
# There are deliberately NO here-strings in this file: 5.1 mis-parses one whose
# terminator arrives with a bare LF, and this file is checked out as LF on a
# non-Windows machine.
#
# Usage:
#   pwsh -NoProfile -File scripts/doctor/doctor.ps1
#   pwsh -NoProfile -File scripts/doctor/doctor.ps1 -Json
#   pwsh -NoProfile -File scripts/doctor/doctor.ps1 -Fast
#   pwsh -NoProfile -File scripts/doctor/doctor.ps1 -Net
#   pwsh -NoProfile -File scripts/doctor/doctor.ps1 -Group vcs
#
# Exit codes: 0 it ran, 2 it could not run.
#
# ⛔ Read the exit code from the process that produced it, unpiped. Piping this
# into anything reports the pipeline's status, not this script's.

[CmdletBinding()]
param(
    # Emit the agent-doctor/1 JSON document instead of the human report.
    [switch]$Json,
    # Select the human report explicitly. It is already the default, so this is
    # a no-op that exists for one reason: doctor.sh accepts --text, and a twin
    # whose CLI surface differs is drift a schema comparison cannot see.
    # `doctor.ps1 -Text` exited 1 with a parameter-binding error while
    # `doctor.sh --text` exited 0, and check-twins.sh compared only the JSON
    # output, so nothing caught it. The flag comparison in check-twins.sh is
    # the other half of this fix.
    [switch]$Text,
    # Presence only. Skips every version probe, which is where the time goes.
    [switch]$Fast,
    # Also test outbound reachability. Off by default: a probe makes no network
    # call unless it is asked to.
    [switch]$Net,
    # Probe one group only: vcs, runtime, compiler, pkg-lang, pkg-system,
    # container, build, quality, cli, cloud, shell, agent.
    [string]$Group = ''
)

$ErrorActionPreference = 'Stop'

# ⚠ CONTRADICTORY OUTPUT FLAGS ARE REFUSED, NOT SILENTLY RESOLVED.
# doctor.sh resolves `--json --text` by last-one-wins, because a POSIX arg loop
# reads them in order. PowerShell hands over a bag of bound parameters with no
# order in it, so the twin CANNOT reproduce that answer. Guessing would make the
# two probes return different documents for the same command line, which is the
# exact drift check-twins.sh exists to stop. Refusing is the one answer both can
# give. Exit 2: the probe could not run, as opposed to running and finding fault.
if ($Json -and $Text) {
    [Console]::Error.WriteLine('doctor: -Json and -Text are contradictory. Pass one.')
    exit 2
}

$Schema = 'agent-doctor/1'
$ProbeTimeoutMs = 6000

$notes = New-Object System.Collections.ArrayList
function Add-Note([string]$Text) { [void]$notes.Add($Text) }

# ------------------------------------------------------------------- host ---

# 5.1 has no $IsWindows: those automatic variables arrived in PowerShell 6, and
# 5.1 only ever runs on Windows. Asking for the variable rather than assuming
# is what makes this file work under both.
function Test-AutoVar([string]$Name) {
    return [bool](Get-Variable -Name $Name -ErrorAction SilentlyContinue)
}
if (Test-AutoVar 'IsWindows') {
    $onWindows = $IsWindows
    $onLinux   = $IsLinux
    $onMac     = $IsMacOS
} else {
    $onWindows = $true; $onLinux = $false; $onMac = $false
}

$os = 'unknown'; $flavor = 'native'
if ($onWindows) { $os = 'windows' } elseif ($onLinux) { $os = 'linux' } elseif ($onMac) { $os = 'macos' }

$isWsl = $false
if ($os -eq 'linux') {
    if ($env:WSL_DISTRO_NAME -or $env:WSL_INTEROP) { $isWsl = $true }
    elseif ((Test-Path '/proc/version') -and ((Get-Content '/proc/version' -Raw) -match 'microsoft|WSL')) { $isWsl = $true }
    elseif (Test-Path '/mnt/c/Windows') { $isWsl = $true }
    if ($isWsl) { $flavor = 'wsl' }
}

$inContainer = $false
if (Test-Path '/.dockerenv') { $inContainer = $true }
elseif (Test-Path '/run/.containerenv') { $inContainer = $true }
elseif (Test-Path '/proc/1/cgroup') {
    if ((Get-Content '/proc/1/cgroup' -Raw -ErrorAction SilentlyContinue) -match 'docker|containerd|lxc|kubepods') { $inContainer = $true }
}

$arch = ''
try { $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() } catch { $arch = '' }
if (-not $arch -and $env:PROCESSOR_ARCHITECTURE) { $arch = $env:PROCESSOR_ARCHITECTURE }
# Normalised to what `uname -m` says, so the two twins report one vocabulary
# for one machine. .NET answers `X64` where uname answers `x86_64`, and a
# consumer comparing the two fields would read one host as two.
switch -Regex ($arch) {
    '^(X64|AMD64|x86_64)$'      { $arch = 'x86_64';  break }
    '^(Arm64|ARM64|aarch64)$'   { $arch = 'aarch64'; break }
    '^(X86|x86)$'               { $arch = 'i686';    break }
    '^(Arm|ARM)$'               { $arch = 'armv7l';  break }
}

# Three ways to name the OS build, because each one is absent somewhere.
# CIM is unavailable in a locked-down or non-Windows session, the registry key
# is Windows-only, and [Environment] is the floor that always answers something.
$distro = ''; $distroVer = ''; $kernel = ''
if ($os -eq 'windows') {
    $distro = 'windows'
    try {
        $cim = Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop
        $distroVer = $cim.Version
        # Win32_OperatingSystem stops at the build; the update-build-revision
        # lives only in the registry. Without it this reads 10.0.26200 where the
        # sh twin reads 10.0.26200.9168, and a consumer diffing two runs of one
        # machine sees a change that did not happen.
        try {
            $ubr = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction Stop).UBR
            if ($null -ne $ubr) { $distroVer = ('{0}.{1}' -f $distroVer, $ubr) }
        } catch { $null = $_ }
        $kernel = $cim.Caption
    } catch {
        try {
            $cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction Stop
            $distroVer = ('{0}.{1}' -f $cv.CurrentMajorVersionNumber, $cv.CurrentBuildNumber)
            $kernel = $cv.ProductName
        } catch {
            $distroVer = [System.Environment]::OSVersion.Version.ToString()
            $kernel = [System.Environment]::OSVersion.VersionString
        }
    }
} elseif (Test-Path '/etc/os-release') {
    foreach ($line in (Get-Content '/etc/os-release' -ErrorAction SilentlyContinue)) {
        if ($line -match '^ID=(.*)$')         { $distro    = $Matches[1].Trim('"') }
        if ($line -match '^VERSION_ID=(.*)$') { $distroVer = $Matches[1].Trim('"') }
    }
    if (Test-Path '/proc/sys/kernel/osrelease') { $kernel = (Get-Content '/proc/sys/kernel/osrelease' -Raw).Trim() }
} elseif ($os -eq 'macos') {
    $distro = 'macos'
    try { $distroVer = (& sw_vers -productVersion 2>$null) } catch { $distroVer = '' }
}

$shellName = ('PowerShell {0} ({1})' -f $PSVersionTable.PSVersion, $PSVersionTable.PSEdition)

# Where fallback lookups search. Non-existent entries are pruned once, so the
# inner loop never stats a directory that is not there.
$candidateDirs = @()
if ($os -eq 'windows') {
    $candidateDirs = @(
        (Join-Path $env:USERPROFILE 'scoop\shims'),
        'C:\ProgramData\scoop\shims',
        'C:\ProgramData\scoop\apps\msys2\current\usr\bin',
        'C:\ProgramData\chocolatey\bin',
        (Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps'),
        (Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Links'),
        (Join-Path $env:USERPROFILE '.cargo\bin'),
        (Join-Path $env:USERPROFILE 'go\bin'),
        (Join-Path $env:USERPROFILE '.local\bin'),
        (Join-Path $env:ProgramFiles 'Git\cmd'),
        (Join-Path $env:ProgramFiles 'nodejs'),
        (Join-Path $env:ProgramFiles 'PowerShell\7'),
        (Join-Path $env:ProgramFiles 'WinGet\Links'),
        (Join-Path $env:SystemRoot 'System32')
    )
} elseif ($os -eq 'macos') {
    $candidateDirs = @('/opt/homebrew/bin','/usr/local/bin','/usr/bin','/bin',
        "$HOME/.cargo/bin","$HOME/go/bin","$HOME/.local/bin",
        '/opt/local/bin','/nix/var/nix/profiles/default/bin')
} else {
    $candidateDirs = @('/usr/local/bin','/usr/bin','/bin','/usr/sbin','/sbin',
        "$HOME/.cargo/bin","$HOME/go/bin","$HOME/.local/bin",
        '/snap/bin','/opt/bin','/nix/var/nix/profiles/default/bin',
        '/home/linuxbrew/.linuxbrew/bin')
}
$fallbackDirs = @()
foreach ($d in $candidateDirs) {
    if ($d -and (Test-Path -LiteralPath $d -PathType Container)) { $fallbackDirs += $d }
}

$writableTmp = ''
foreach ($t in @($env:TEMP, $env:TMPDIR, '/tmp', (Join-Path $HOME 'tmp'), '.')) {
    if (-not $t) { continue }
    if (-not (Test-Path -LiteralPath $t -PathType Container)) { continue }
    $probeFile = Join-Path $t ('.doctor-w-{0}' -f ([guid]::NewGuid().ToString('N')))
    try {
        [System.IO.File]::WriteAllText($probeFile, 'x')
        Remove-Item -LiteralPath $probeFile -Force -ErrorAction SilentlyContinue
        $writableTmp = $t
        break
    } catch { continue }
}
if (-not $writableTmp) { Add-Note 'no writable temp directory among TEMP, TMPDIR, /tmp, ~/tmp, .' }

# ---------------------------------------------------------------- lookups ---

$exeExts = if ($os -eq 'windows') { @('', '.exe', '.cmd', '.bat', '.ps1') } else { @('') }

function Resolve-Tool([string]$Name) {
    # Get-Command is the PATH answer and also sees functions and aliases, so
    # the result is filtered to real executables and scripts.
    try {
        $cmd = Get-Command -Name $Name -ErrorAction Stop |
               Where-Object { $_.CommandType -in @('Application','ExternalScript') } |
               Select-Object -First 1
        if ($cmd) { if ($cmd.Source) { return $cmd.Source } else { return $cmd.Name } }
    } catch { $null = $_ }

    # PATH missed it. Look where installers actually put things, which is the
    # half a PATH lookup cannot see: a session started before an install, a
    # machine-wide install absent from a user PATH, a tool behind a shim dir.
    foreach ($dir in $fallbackDirs) {
        foreach ($ext in $exeExts) {
            $candidate = Join-Path $dir ($Name + $ext)
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
        }
    }
    return $null
}

# First version-looking token from a tool's own version output.
#
# The extraction splits the output into tokens and takes the FIRST that reads
# as a version, rather than matching a regex across the whole line. A greedy
# regex reports the wrong half of a version and does it confidently: the sh
# twin's first draft turned `git version 2.51.0.windows.3` into
# `5.0.windows.3` and `v22.11.0` into `7.0`. A wrong version is worse than a
# blank one, because a blank one gets checked.
# The name may be joined to the number by a hyphen or an underscore, which is
# why the pattern allows one: `jq-1.8.2` read as no version at all until it did.
function Get-VersionToken([string]$Text) {
    if (-not $Text) { return '' }
    foreach ($tok in [regex]::Split($Text, '[^0-9A-Za-z.+_-]+')) {
        if ($tok -match '^[A-Za-z]*[-_]?[0-9]+\.[0-9]+') {
            return ($tok -replace '^[A-Za-z]*[-_]?', '')
        }
    }
    return ''
}

# Run a program with a hard time limit. Returns a hashtable: Output, TimedOut.
#
# ⛔ THE STREAMS ARE READ BEFORE THE WAIT, AND THAT ORDER IS THE WHOLE TRICK.
# Calling WaitForExit first deadlocks any child that fills the 4 KB pipe
# buffer: the child blocks on write, the parent blocks on wait, and neither
# moves until the timeout fires. `pnpm --version` is small enough to hide this
# and a compiler's `--version` banner is not.
#
# ⛔ AND THE TIME LIMIT IS NOT A NICETY. Several tools block for as long as you
# let them: `kubectl version` without --client contacts a cluster, `gradle
# --version` starts a daemon, a cloud CLI can sit on an update check. The sh
# twin without a limit did not finish in two minutes.
function Invoke-Limited([string]$FilePath, [string[]]$Arguments, [int]$TimeoutMs) {
    # ⛔ A SHIM IS NOT AN EXECUTABLE, AND Process.Start WILL NOT PRETEND IT IS.
    # With UseShellExecute = $false there is no shell to interpret a script, so
    # a .ps1 throws "not a valid application for this OS platform" and a .cmd
    # is refused outright. This matters far more than it sounds: on Windows the
    # node ecosystem ships shims, and scoop's are .ps1 - npm, pnpm, yarn,
    # wrangler and codegraph all resolved to one here and every one of them was
    # reported as an uninstalled stub until this branch existed.
    # A .ps1 goes to the PowerShell that is already running, which is the one
    # host guaranteed to be present; a .cmd or .bat goes to cmd.exe.
    $exe = $FilePath
    $argList = @()
    switch -Regex ([System.IO.Path]::GetExtension($FilePath)) {
        '^\.ps1$' {
            $host_exe = $null
            try { $host_exe = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName } catch { $null = $_ }
            if (-not $host_exe) { $host_exe = 'powershell.exe' }
            $exe = $host_exe
            $argList = @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',('"' + $FilePath + '"')) + $Arguments
            break
        }
        '^\.(cmd|bat)$' {
            $exe = (Join-Path $env:SystemRoot 'System32\cmd.exe')
            $argList = @('/d','/c',('"' + $FilePath + '"')) + $Arguments
            break
        }
        default { $argList = $Arguments }
    }

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    if ($argList -and $argList.Count -gt 0) { $psi.Arguments = ($argList -join ' ') }
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardInput = $true

    $proc = $null
    try {
        $proc = [System.Diagnostics.Process]::Start($psi)
    } catch {
        return @{ Out = ''; Err = ''; Combined = ''; Code = -1; TimedOut = $false; Failed = $true }
    }

    $outTask = $proc.StandardOutput.ReadToEndAsync()
    $errTask = $proc.StandardError.ReadToEndAsync()
    # Close stdin so nothing can block waiting for input it will never get.
    try { $proc.StandardInput.Close() } catch { $null = $_ }

    if (-not $proc.WaitForExit($TimeoutMs)) {
        try { $proc.Kill() } catch { $null = $_ }
        try { $proc.WaitForExit(2000) | Out-Null } catch { $null = $_ }
        return @{ Out = ''; Err = ''; Combined = ''; Code = -1; TimedOut = $true; Failed = $false }
    }

    # ⛔ THE TWO STREAMS ARE KEPT APART, and a caller that merges them must mean
    # to. A version probe merges, because java and several JVM tools print their
    # version to stderr. Anything reading a VALUE must not: `git rev-parse
    # --abbrev-ref HEAD` in a repository with no commits prints `HEAD` to stdout
    # AND a three-line fatal to stderr, and merging them put that fatal into
    # this report's `branch` field.
    $o = ''; $e = ''
    try { $o = $outTask.Result } catch { $o = '' }
    try { $e = $errTask.Result } catch { $e = '' }
    return @{ Out = $o; Err = $e; Combined = ($o + ' ' + $e); Code = $proc.ExitCode; TimedOut = $false; Failed = $false }
}

# ------------------------------------------------------------------- repo ---

$repo = [ordered]@{
    is_git = $false; root = ''; branch = ''; remote = ''
    dirty = $false; commits = 0
    remote_looks_like_template = $false
    has_codegraph = $false; ecosystems = @()
}
$gitPath = Resolve-Tool 'git'
if ($gitPath) {
    # Every one of these reads .Out and checks .Code. A non-zero git is a git
    # that answered on stderr, and stderr is not a value.
    function Get-GitValue([string[]]$GitArgs, [int]$Ms = 5000) {
        $r = Invoke-Limited $gitPath $GitArgs $Ms
        if ($r.TimedOut -or $r.Failed -or $r.Code -ne 0) { return '' }
        return ($r.Out).Trim()
    }
    $root = Get-GitValue @('rev-parse','--show-toplevel')
    if ($root) {
        $repo.is_git  = $true
        $repo.root    = $root
        $repo.branch  = Get-GitValue @('rev-parse','--abbrev-ref','HEAD')
        $repo.remote  = Get-GitValue @('remote','get-url','origin')
        $repo.dirty   = [bool](Get-GitValue @('status','--porcelain') 8000)
        $cnt          = Get-GitValue @('rev-list','--count','HEAD')
        if ($cnt -match '^[0-9]+$') { $repo.commits = [int]$cnt }
        if ($repo.remote -match 'template') { $repo.remote_looks_like_template = $true }
    }
}
if (Test-Path -LiteralPath '.codegraph' -PathType Container) { $repo.has_codegraph = $true }

# Which ecosystems does this tree already declare? Read from manifests, which
# is evidence, rather than from a directory name, which is a guess.
$ecoMap = @(
    @{ n = 'node';               f = @('package.json') },
    @{ n = 'deno';               f = @('deno.json','deno.jsonc') },
    @{ n = 'bun';                f = @('bun.lockb','bunfig.toml') },
    @{ n = 'rust';               f = @('Cargo.toml') },
    @{ n = 'go';                 f = @('go.mod') },
    @{ n = 'python';             f = @('pyproject.toml','requirements.txt','setup.py') },
    @{ n = 'ruby';               f = @('Gemfile') },
    @{ n = 'php';                f = @('composer.json') },
    @{ n = 'java-maven';         f = @('pom.xml') },
    @{ n = 'java-gradle';        f = @('build.gradle','build.gradle.kts') },
    @{ n = 'cmake';              f = @('CMakeLists.txt') },
    @{ n = 'make';               f = @('Makefile','makefile') },
    @{ n = 'container';          f = @('Dockerfile','compose.yaml','docker-compose.yml') },
    @{ n = 'cloudflare-workers'; f = @('wrangler.toml','wrangler.jsonc','wrangler.json') },
    @{ n = 'nix';                f = @('flake.nix','default.nix') },
    @{ n = 'swift';              f = @('Package.swift') }
)
$ecosystems = @()
foreach ($e in $ecoMap) {
    foreach ($f in $e.f) {
        if (Test-Path -LiteralPath $f -PathType Leaf) { $ecosystems += $e.n; break }
    }
}
if (@(Get-ChildItem -Path . -Filter '*.csproj' -File -ErrorAction SilentlyContinue).Count -gt 0 -or
    @(Get-ChildItem -Path . -Filter '*.sln'    -File -ErrorAction SilentlyContinue).Count -gt 0) {
    $ecosystems += 'dotnet'
}
$repo.ecosystems = @($ecosystems)

# ------------------------------------------------------------------ tools ---
# id, group, binary, version arguments. An empty argument array means the tool
# has no version flag; presence is all that can be established.

$toolTable = @(
    @('git','vcs','git',@('--version')),
    @('gh','vcs','gh',@('--version')),
    @('git-lfs','vcs','git-lfs',@('--version')),
    @('jj','vcs','jj',@('--version')),
    @('hg','vcs','hg',@('--version')),
    @('svn','vcs','svn',@('--version')),
    @('node','runtime','node',@('--version')),
    @('deno','runtime','deno',@('--version')),
    @('bun','runtime','bun',@('--version')),
    @('python3','runtime','python3',@('--version')),
    @('python','runtime','python',@('--version')),
    @('ruby','runtime','ruby',@('--version')),
    @('php','runtime','php',@('--version')),
    @('java','runtime','java',@('-version')),
    @('dotnet','runtime','dotnet',@('--version')),
    @('go','runtime','go',@('version')),
    @('rustc','runtime','rustc',@('--version')),
    @('zig','runtime','zig',@('version')),
    @('perl','runtime','perl',@('--version')),
    @('lua','runtime','lua',@('-v')),
    @('gcc','compiler','gcc',@('--version')),
    @('clang','compiler','clang',@('--version')),
    @('cl','compiler','cl',@()),
    @('npm','pkg-lang','npm',@('--version')),
    @('pnpm','pkg-lang','pnpm',@('--version')),
    @('yarn','pkg-lang','yarn',@('--version')),
    @('pip','pkg-lang','pip',@('--version')),
    @('pipx','pkg-lang','pipx',@('--version')),
    @('uv','pkg-lang','uv',@('--version')),
    @('poetry','pkg-lang','poetry',@('--version')),
    @('cargo','pkg-lang','cargo',@('--version')),
    @('rustup','pkg-lang','rustup',@('--version')),
    @('gem','pkg-lang','gem',@('--version')),
    @('composer','pkg-lang','composer',@('--version')),
    @('maven','pkg-lang','mvn',@('--version')),
    @('gradle','pkg-lang','gradle',@('--version')),
    @('scoop','pkg-system','scoop',@()),
    @('choco','pkg-system','choco',@('--version')),
    @('winget','pkg-system','winget',@('--version')),
    @('brew','pkg-system','brew',@('--version')),
    @('apt','pkg-system','apt',@('--version')),
    @('dnf','pkg-system','dnf',@('--version')),
    @('pacman','pkg-system','pacman',@('--version')),
    @('apk','pkg-system','apk',@('--version')),
    @('zypper','pkg-system','zypper',@('--version')),
    @('nix','pkg-system','nix',@('--version')),
    @('docker','container','docker',@('--version')),
    @('podman','container','podman',@('--version')),
    @('kubectl','container','kubectl',@('version','--client')),
    # wsl.exe writes UTF-16LE, which a redirected stdout reads as empty or as
    # mojibake, so no version can honestly be taken from it. Presence only.
    @('wsl','container','wsl',@()),
    @('make','build','make',@('--version')),
    @('cmake','build','cmake',@('--version')),
    @('ninja','build','ninja',@('--version')),
    @('just','build','just',@('--version')),
    @('task','build','task',@('--version')),
    @('msbuild','build','msbuild',@('-version')),
    @('shellcheck','quality','shellcheck',@('--version')),
    @('shfmt','quality','shfmt',@('--version')),
    @('ruff','quality','ruff',@('--version')),
    @('eslint','quality','eslint',@('--version')),
    @('prettier','quality','prettier',@('--version')),
    @('golangci-lint','quality','golangci-lint',@('--version')),
    @('jq','cli','jq',@('--version')),
    @('yq','cli','yq',@('--version')),
    @('rg','cli','rg',@('--version')),
    @('fd','cli','fd',@('--version')),
    @('curl','cli','curl',@('--version')),
    @('wget','cli','wget',@('--version')),
    @('aria2c','cli','aria2c',@('--version')),
    @('tar','cli','tar',@('--version')),
    @('7z','cli','7z',@()),
    @('sqlite3','cli','sqlite3',@('--version')),
    @('scc','cli','scc',@('--version')),
    @('tokei','cli','tokei',@('--version')),
    @('hyperfine','cli','hyperfine',@('--version')),
    @('wrangler','cloud','wrangler',@('--version')),
    @('aws','cloud','aws',@('--version')),
    @('gcloud','cloud','gcloud',@('--version')),
    @('az','cloud','az',@('--version')),
    @('flyctl','cloud','flyctl',@('version')),
    @('terraform','cloud','terraform',@('--version')),
    @('bash','shell','bash',@('--version')),
    @('zsh','shell','zsh',@('--version')),
    @('pwsh','shell','pwsh',@('--version')),
    @('powershell','shell','powershell',@('-NoProfile','-Command','$PSVersionTable.PSVersion.ToString()')),
    @('codegraph','agent','codegraph',@('--version'))
)

# PSScriptAnalyzer is a MODULE, not an executable. Resolve-Tool filters to
# applications and external scripts, so listing it in the table above would
# have reported it missing on every machine that has it. It is worth knowing
# about because it is the PowerShell linter a gate would call.
function Get-ModuleTool([string]$Id, [string]$ModuleName) {
    try {
        $m = Get-Module -ListAvailable -Name $ModuleName -ErrorAction SilentlyContinue |
             Sort-Object Version -Descending | Select-Object -First 1
        if ($m) { return [ordered]@{ id=$Id; group='quality'; found=$true; path=$m.ModuleBase; version=$m.Version.ToString() } }
    } catch { $null = $_ }
    return [ordered]@{ id=$Id; group='quality'; found=$false; path=''; version='' }
}

$tools = New-Object System.Collections.ArrayList
$timedOut = New-Object System.Collections.ArrayList
$stubs    = New-Object System.Collections.ArrayList
$found = 0; $missing = 0

foreach ($row in $toolTable) {
    $id = $row[0]; $grp = $row[1]; $bin = $row[2]; $vargs = $row[3]
    if ($Group -and $Group -ne $grp) { continue }

    $path = Resolve-Tool $bin
    if ($path) {
        $found++
        $version = ''
        if (-not $Fast -and $vargs.Count -gt 0) {
            $res = Invoke-Limited $path $vargs $ProbeTimeoutMs
            if ($res.TimedOut) {
                [void]$timedOut.Add($id)
            } else {
                # Merged here on purpose: java and several JVM tools print the
                # version to stderr, so a probe reading stdout alone finds none.
                $version = Get-VersionToken $res.Combined
                # On PATH but answering nothing. Usually a shim standing in for
                # a tool that is not installed: the Windows Store python3 alias
                # is the common one, a stub that prints "Python was not found".
                # Reported rather than listed as present, because present is
                # what it is not.
                if (-not $version) { [void]$stubs.Add($id) }
            }
        }
        [void]$tools.Add([ordered]@{ id=$id; group=$grp; found=$true; path=$path; version=$version })
    } else {
        $missing++
        [void]$tools.Add([ordered]@{ id=$id; group=$grp; found=$false; path=''; version='' })
    }
}

if (-not $Group -or $Group -eq 'quality') {
    $psa = Get-ModuleTool 'psscriptanalyzer' 'PSScriptAnalyzer'
    [void]$tools.Add($psa)
    if ($psa.found) { $found++ } else { $missing++ }
}

if ($timedOut.Count -gt 0) {
    Add-Note ('no answer within {0}s: {1} - present, version unknown. Not the same fact as absent.' -f ($ProbeTimeoutMs/1000), ($timedOut -join ' '))
}
if ($stubs.Count -gt 0) {
    Add-Note ('on PATH but reported no version: {0} - probably a shim or a stub rather than a working install. Confirm before planning on one.' -f ($stubs -join ' '))
}

# --------------------------------------------------------------- outbound ---

$network = 'unknown'
if ($Net) {
    $network = 'no'
    try {
        $req = [System.Net.WebRequest]::Create('https://example.com')
        $req.Method = 'HEAD'
        $req.Timeout = 8000
        $resp = $req.GetResponse()
        $resp.Close()
        $network = 'yes'
    } catch { $network = 'no' }
}

# ----------------------------------------------------------------- output ---

$doc = [ordered]@{
    schema    = $Schema
    generated = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    probe     = [ordered]@{ impl = 'doctor.ps1'; fast = [bool]$Fast; group = $Group }
    host      = [ordered]@{
        os = $os; flavor = $flavor; wsl = $isWsl; container = $inContainer
        kernel = $kernel; arch = $arch
        distro = $distro; distro_version = $distroVer
        shell = $shellName; writable_tmp = $writableTmp; network = $network
    }
    repo      = $repo
    summary   = [ordered]@{ tools_found = $found; tools_missing = $missing }
    tools     = @($tools)
    notes     = @($notes)
}

if ($Json) {
    # ConvertTo-Json rather than hand-built strings: it is present in 5.1 and
    # it escapes what has to be escaped. Depth 6 covers repo.ecosystems and
    # the tools array; the 5.1 default of 2 silently renders nested objects as
    # the literal text "System.Collections.Hashtable".
    $doc | ConvertTo-Json -Depth 6
    exit 0
}

function Write-Row([string]$Label, $Value) {
    Write-Output ('  {0,-14}{1}' -f $Label, $Value)
}

Write-Output ('doctor  {0}  ({1})' -f $Schema, $doc.generated)
Write-Output ''
Write-Output 'HOST'
Write-Row 'os'           ('{0} ({1})' -f $os, $flavor)
Write-Row 'arch'         $arch
if ($kernel)    { Write-Row 'kernel' $kernel }
if ($distro)    { Write-Row 'distro' ('{0} {1}' -f $distro, $distroVer) }
Write-Row 'wsl'          $(if ($isWsl) { 'yes' } else { 'no' })
Write-Row 'container'    $(if ($inContainer) { 'yes' } else { 'no' })
Write-Row 'shell'        $shellName
Write-Row 'writable tmp' $(if ($writableTmp) { $writableTmp } else { 'NONE' })
if ($Net) { Write-Row 'network' $network }

Write-Output ''
Write-Output 'REPO'
if ($repo.is_git) {
    Write-Row 'git root' $repo.root
    Write-Row 'branch'   ('{0} ({1} commits)' -f $repo.branch, $repo.commits)
    Write-Row 'origin'   $(if ($repo.remote) { $repo.remote } else { 'none' })
    Write-Row 'tree'     $(if ($repo.dirty) { 'dirty' } else { 'clean' })
    if ($repo.remote_looks_like_template) {
        Write-Output '  origin still points at a template remote. Detach before committing project work.'
    }
} else {
    Write-Output '  not a git repository'
}
Write-Row 'codegraph'  $(if ($repo.has_codegraph) { 'indexed' } else { 'absent' })
Write-Row 'ecosystems' $(if ($repo.ecosystems.Count) { ($repo.ecosystems -join ' ') } else { 'none detected' })

Write-Output ''
Write-Output ('TOOLS  ({0} found, {1} missing)' -f $found, $missing)
foreach ($t in $tools) {
    if ($t.found) {
        $v = $(if ($t.version) { $t.version } else { '-' })
        Write-Output ('  yes  {0,-16} {1,-10} {2}' -f $t.id, $v, $t.path)
    } else {
        Write-Output ('  no   {0}' -f $t.id)
    }
}

if ($notes.Count -gt 0) {
    Write-Output ''
    Write-Output 'NOTES'
    foreach ($n in $notes) { Write-Output ('  {0}' -f $n) }
}

Write-Output ''
Write-Output 'This is a probe, not a gate. A missing tool is data.'
Write-Output ('Machine-readable: pwsh -NoProfile -File {0} -Json' -f $PSCommandPath)
exit 0
