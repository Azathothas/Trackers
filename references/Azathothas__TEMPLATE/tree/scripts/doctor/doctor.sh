#!/bin/sh
# doctor.sh - what host is this, what is installed, and what is this repo?
#
# The defect this exists to catch is an agent that assumes its environment.
# A session that assumes node is present writes a node script and finds out at
# the gate. A session that assumes it is on Linux reaches for `pkill` on a
# machine that wants `taskkill`. A session that takes "most tools are available"
# on trust plans around a tool that is not there. This answers all of it in one
# read-only pass, before any of it costs a task.
#
# It is a PROBE, not a gate. A missing tool is data, not a failure, so it exits
# 0 whenever it ran. It exits 2 only when it could not run at all.
#
# It is read-only: no installer, no config change, no network call unless --net
# is passed, and the only file it writes is a temp file it removes.
#
# Redundancy is the point. Every tool is looked for on PATH first, then in the
# install directories a shell PATH misses. A machine-wide scoop install under
# C:/ProgramData/scoop is the worked example: checking only ~/scoop reports
# msys2 as absent on a machine that has it.
#
# Usage:
#   sh scripts/doctor/doctor.sh              human-readable report
#   sh scripts/doctor/doctor.sh --json       machine-readable, schema agent-doctor/1
#   sh scripts/doctor/doctor.sh --fast       presence only, skip version probes
#   sh scripts/doctor/doctor.sh --net        also test outbound reachability
#   sh scripts/doctor/doctor.sh --group vcs  probe one group only
#
# Exit codes: 0 it ran, 2 it could not run.
#
# The PowerShell twin is scripts/doctor/doctor.ps1 and emits the same schema.
# Changing a field here means changing it there; scripts/doctor/README.md says
# how the two are kept in step and carries the measured runtimes.

set -u

SCHEMA="agent-doctor/1"
MODE=text
FAST=0
NET=0
ONLY=""

while [ $# -gt 0 ]; do
  case "$1" in
    # ⚠ CONTRADICTORY OUTPUT FLAGS ARE REFUSED, NOT SILENTLY RESOLVED.
    # Last-one-wins is what an arg loop does by default, and it is not what the
    # PowerShell twin can reproduce: PowerShell binds parameters into a bag with
    # no order in it. Two probes answering the same command line differently is
    # the drift check-twins.sh exists to stop, so both refuse instead.
    --json) [ "$MODE" = text_explicit ] && { printf 'doctor: --json and --text are contradictory. Pass one.\n' >&2; exit 2; }
            MODE=json ;;
    --text) [ "$MODE" = json ] && { printf 'doctor: --json and --text are contradictory. Pass one.\n' >&2; exit 2; }
            MODE=text_explicit ;;
    --fast) FAST=1 ;;
    --net)  NET=1 ;;
    --group) shift; ONLY="${1:-}" ;;
    -h|--help)
      awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"
      exit 0 ;;
    *) printf 'doctor: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

# ---------------------------------------------------------------- helpers ---

# Escape a value for a JSON string, into the global ESC.
#
# ⛔ IT SETS A GLOBAL RATHER THAN PRINTING, AND THAT IS A MEASURED DECISION, NOT
# A STYLE ONE. The first version was `jstr() { printf '"%s"' "$(json_escape ...)"; }`
# with json_escape piping through sed and two trs. That is six process spawns
# per value, four values per tool, eighty-two tools: about two thousand spawns,
# and it took 67 s on Windows where a fork is expensive. The rewrite runs the
# pipeline only for values that actually contain a backslash, a quote or a
# control byte, which on a real machine is almost none of them.
# Measured on the same host: 67 s -> under 10 s.
esc() {
  case "$1" in
    *\\*|*\"*|*"$NL"*|*"$TAB"*|*"$CR"*)
      ESC=$(printf '%s' "$1" \
        | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' \
        | tr -d '\000-\010\013\014\016-\037' \
        | tr '\n\r\t' '   ') ;;
    *) ESC="$1" ;;
  esac
}

NL='
'
TAB=$(printf '\t')
CR=$(printf '\r')

jbool() { if [ "${1:-0}" = "1" ]; then printf 'true'; else printf 'false'; fi; }

# Is this binary callable? Answers into the global WP_PATH; returns 0 or 1.
which_path() {
  WP_PATH=$(command -v "$1" 2>/dev/null) || WP_PATH=""
  [ -n "$WP_PATH" ] && return 0

  # PATH missed it. Look where installers actually put things, which is the
  # half a bare `command -v` cannot see: a shell started before an install, a
  # machine-wide install absent from a user PATH, a tool behind a shim dir.
  for _wp_dir in $FALLBACK_DIRS; do
    for _wp_ext in "" ".exe" ".cmd" ".bat" ".ps1"; do
      if [ -f "$_wp_dir/$1$_wp_ext" ]; then
        WP_PATH="$_wp_dir/$1$_wp_ext"; return 0
      fi
    done
  done
  WP_PATH=""
  return 1
}

# First version-looking token from a tool's own version output.
# Both streams are captured: java and several JVM tools print to stderr.
# stdin is closed so nothing can block waiting for input.
#
# The extraction splits the output into tokens and takes the FIRST that reads
# as a version, rather than matching a regex across the whole line. A greedy
# regex reports the wrong half of a version and does it confidently: the first
# draft turned `git version 2.51.0.windows.3` into `5.0.windows.3`, `v22.11.0`
# into `7.0`, and `rustc 1.83.0` into `8.0`. A wrong version is worse than a
# blank one, because a blank one gets checked.
# The name may be joined to the number by a hyphen or an underscore, which is
# why the pattern allows one: `jq-1.8.2` read as no version at all until it did.
#
# ⛔ EVERY PROBE IS TIME-LIMITED, and that is not a nicety. Several tools block
# for as long as you let them: `kubectl version` without --client contacts a
# cluster, `gradle --version` starts a daemon, a cloud CLI can sit on an update
# check. The first draft had no limit and did not finish in two minutes, which
# is a probe nobody runs twice.
#
# Returns 124 when the tool never answered. ⛔ Do NOT record that in a variable
# here: this function is called inside $( ), which is a subshell, so any
# assignment is discarded the moment it returns. The caller reads the exit code.
probe_version() {
  _pv_path="$1"; _pv_flag="$2"
  [ "$FAST" = "1" ] && { printf ''; return 0; }
  [ "$_pv_flag" = "@none" ] && { printf ''; return 0; }

  _pv_rc=0
  # shellcheck disable=SC2086
  # $_pv_flag is deliberately unquoted: a flag field may be two words, as
  # `kubectl version --client` is, and the values come from the table below
  # rather than from anything a caller supplies.
  if [ -n "$TIMEOUT_BIN" ]; then
    _pv_raw=$("$TIMEOUT_BIN" "$PROBE_TIMEOUT" "$_pv_path" $_pv_flag 2>&1 </dev/null) || _pv_rc=$?
  else
    _pv_raw=$(run_watchdog "$PROBE_TIMEOUT" "$_pv_path" $_pv_flag) || _pv_rc=$?
  fi

  # 124 is the coreutils timeout verdict; 137 is SIGKILL, which the watchdog
  # uses. Both mean "it never answered", which is a different fact from "it has
  # no version" and is reported separately.
  if [ "$_pv_rc" = 124 ] || [ "$_pv_rc" = 137 ]; then
    printf ''
    return 124
  fi

  [ -z "$_pv_raw" ] && { printf ''; return 0; }
  printf '%s' "$_pv_raw" \
    | head -n 5 \
    | tr -cs '0-9A-Za-z.+_-' '\n' \
    | grep -E '^[A-Za-z]*[-_]?[0-9]+\.[0-9]+' \
    | head -n 1 \
    | sed 's/^[A-Za-z]*[-_]\{0,1\}//'
}

# The fallback for a host with no `timeout` on PATH, which is stock macOS.
# One-second granularity is enough for a limit measured in seconds, and it
# avoids `sleep 0.1`, which POSIX does not require a shell to accept.
run_watchdog() {
  _rw_secs="$1"; shift
  _rw_out="${TMPDIR_OK:-/tmp}/.doctor.$$"
  ( "$@" >"$_rw_out" 2>&1 </dev/null ) &
  _rw_pid=$!
  _rw_n=0
  while kill -0 "$_rw_pid" 2>/dev/null; do
    if [ "$_rw_n" -ge "$_rw_secs" ]; then
      kill -9 "$_rw_pid" 2>/dev/null
      wait "$_rw_pid" 2>/dev/null
      rm -f "$_rw_out"
      return 137
    fi
    _rw_n=$((_rw_n + 1))
    sleep 1
  done
  wait "$_rw_pid" 2>/dev/null
  _rw_rc=$?
  [ -f "$_rw_out" ] && cat "$_rw_out"
  rm -f "$_rw_out"
  return "$_rw_rc"
}

NOTES=""
note() { NOTES="$NOTES$1$NL"; }

PROBE_TIMEOUT=6
TIMEOUT_BIN=""
if command -v timeout >/dev/null 2>&1; then TIMEOUT_BIN=timeout
elif command -v gtimeout >/dev/null 2>&1; then TIMEOUT_BIN=gtimeout   # macOS, via coreutils
fi
TIMEDOUT=""
STUBS=""

# ------------------------------------------------------------------- host ---

UNAME_S=$(uname -s 2>/dev/null) || UNAME_S=""
UNAME_R=$(uname -r 2>/dev/null) || UNAME_R=""
UNAME_M=$(uname -m 2>/dev/null) || UNAME_M=""

OS=unknown
FLAVOR=native
case "$UNAME_S" in
  Linux*)       OS=linux ;;
  Darwin*)      OS=macos ;;
  MINGW*|MSYS*) OS=windows; FLAVOR=msys ;;
  CYGWIN*)      OS=windows; FLAVOR=cygwin ;;
  FreeBSD*)     OS=freebsd ;;
  OpenBSD*)     OS=openbsd ;;
  NetBSD*)      OS=netbsd ;;
  SunOS*)       OS=solaris ;;
esac

# uname said nothing useful. Two more ways to recognise Windows, because a
# stripped busybox or a restricted shell can lack uname entirely.
if [ "$OS" = unknown ]; then
  if [ -n "${SYSTEMROOT:-}" ] || [ -n "${WINDIR:-}" ]; then
    OS=windows; FLAVOR=unknown
  elif [ -d /proc/self ] && [ -f /etc/os-release ]; then
    OS=linux
  fi
fi

# WSL is Linux that can see a Windows filesystem, and the difference decides
# which path separators, which line endings and which package manager apply.
WSL=0
if [ "$OS" = linux ]; then
  if [ -n "${WSL_DISTRO_NAME:-}" ] || [ -n "${WSL_INTEROP:-}" ]; then
    WSL=1
  elif [ -r /proc/version ] && grep -qiE 'microsoft|wsl' /proc/version 2>/dev/null; then
    WSL=1
  elif [ -d /mnt/c/Windows ]; then
    WSL=1
  fi
  [ "$WSL" = "1" ] && FLAVOR=wsl
fi

CONTAINER=0
if [ -f /.dockerenv ] || [ -f /run/.containerenv ]; then
  CONTAINER=1
elif [ -r /proc/1/cgroup ] && grep -qE 'docker|containerd|lxc|kubepods' /proc/1/cgroup 2>/dev/null; then
  CONTAINER=1
fi

DISTRO=""
DISTRO_VER=""
if [ -r /etc/os-release ]; then
  # shellcheck source=/dev/null
  DISTRO=$(. /etc/os-release 2>/dev/null; printf '%s' "${ID:-}")
  # shellcheck source=/dev/null
  DISTRO_VER=$(. /etc/os-release 2>/dev/null; printf '%s' "${VERSION_ID:-}")
elif [ -r /etc/redhat-release ]; then
  DISTRO=$(head -n 1 /etc/redhat-release)
elif [ "$OS" = macos ]; then
  DISTRO=macos
  DISTRO_VER=$(sw_vers -productVersion 2>/dev/null || printf '')
elif [ "$OS" = windows ]; then
  DISTRO=windows
  DISTRO_VER=$(cmd //c ver 2>/dev/null | tr -d '\r' | sed -n 's/.*\[Version \(.*\)\]/\1/p') || DISTRO_VER=""
fi

# Which shell is actually interpreting this, not which one is configured.
SHELL_NAME="sh"
if [ -n "${BASH_VERSION:-}" ]; then SHELL_NAME="bash ${BASH_VERSION}"
elif [ -n "${ZSH_VERSION:-}" ]; then SHELL_NAME="zsh ${ZSH_VERSION}"
elif [ -n "${KSH_VERSION:-}" ]; then SHELL_NAME="ksh"
fi

# Where fallback lookups search. Non-existent entries are pruned once here, so
# the inner loop never stats a directory that is not there.
case "$OS" in
  windows)
    _CAND="
      $HOME/scoop/shims
      /c/ProgramData/scoop/shims
      /c/ProgramData/scoop/apps/msys2/current/usr/bin
      /c/ProgramData/chocolatey/bin
      $HOME/AppData/Local/Microsoft/WindowsApps
      $HOME/AppData/Local/Microsoft/WinGet/Links
      $HOME/.cargo/bin
      $HOME/go/bin
      $HOME/.local/bin
      /c/Program Files/Git/cmd
      /c/Program Files/nodejs
      /c/Program Files/PowerShell/7
      /c/Program Files/WinGet/Links
      /c/Windows/System32
    " ;;
  macos)
    _CAND="
      /opt/homebrew/bin /usr/local/bin /usr/bin /bin
      $HOME/.cargo/bin $HOME/go/bin $HOME/.local/bin
      /opt/local/bin /nix/var/nix/profiles/default/bin
    " ;;
  *)
    _CAND="
      /usr/local/bin /usr/bin /bin /usr/sbin /sbin
      $HOME/.cargo/bin $HOME/go/bin $HOME/.local/bin
      /snap/bin /opt/bin /nix/var/nix/profiles/default/bin
      /home/linuxbrew/.linuxbrew/bin
    " ;;
esac
FALLBACK_DIRS=""
for _d in $_CAND; do
  [ -d "$_d" ] && FALLBACK_DIRS="$FALLBACK_DIRS $_d"
done

# A temp directory that is actually writable. An agent that plans a scratch
# file into a directory it cannot write finds out at the worst moment.
TMPDIR_OK=""
for _t in "${TMPDIR:-}" /tmp "$HOME/tmp" .; do
  [ -z "$_t" ] && continue
  if [ -d "$_t" ] && [ -w "$_t" ]; then TMPDIR_OK="$_t"; break; fi
done
[ -z "$TMPDIR_OK" ] && note "no writable temp directory among TMPDIR, /tmp, ~/tmp, ."

# ------------------------------------------------------------------- repo ---

IS_GIT=0; GIT_ROOT=""; GIT_BRANCH=""; GIT_REMOTE=""; GIT_DIRTY=0
GIT_COMMITS=0; HAS_CODEGRAPH=0; REMOTE_IS_TEMPLATE=0
if command -v git >/dev/null 2>&1; then
  if GIT_ROOT=$(git rev-parse --show-toplevel 2>/dev/null); then
    IS_GIT=1
    GIT_BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || printf '')
    GIT_REMOTE=$(git remote get-url origin 2>/dev/null || printf '')
    [ -n "$(git status --porcelain 2>/dev/null)" ] && GIT_DIRTY=1
    GIT_COMMITS=$(git rev-list --count HEAD 2>/dev/null || printf '0')
    case "$GIT_REMOTE" in
      *[Tt][Ee][Mm][Pp][Ll][Aa][Tt][Ee]*) REMOTE_IS_TEMPLATE=1 ;;
    esac
  fi
fi
[ -d .codegraph ] && HAS_CODEGRAPH=1

# Which ecosystems does this tree already declare? Read from manifests, which
# is evidence, rather than from a directory name, which is a guess.
ECOSYSTEMS=""
add_eco() { case " $ECOSYSTEMS " in *" $1 "*) ;; *) ECOSYSTEMS="$ECOSYSTEMS $1" ;; esac; }
[ -f package.json ] && add_eco node
{ [ -f deno.json ] || [ -f deno.jsonc ]; } && add_eco deno
{ [ -f bun.lockb ] || [ -f bunfig.toml ]; } && add_eco bun
[ -f Cargo.toml ] && add_eco rust
[ -f go.mod ] && add_eco go
{ [ -f pyproject.toml ] || [ -f requirements.txt ] || [ -f setup.py ]; } && add_eco python
[ -f Gemfile ] && add_eco ruby
[ -f composer.json ] && add_eco php
[ -f pom.xml ] && add_eco java-maven
{ [ -f build.gradle ] || [ -f build.gradle.kts ]; } && add_eco java-gradle
[ -f CMakeLists.txt ] && add_eco cmake
{ [ -f Makefile ] || [ -f makefile ]; } && add_eco make
{ [ -f Dockerfile ] || [ -f compose.yaml ] || [ -f docker-compose.yml ]; } && add_eco container
{ [ -f wrangler.toml ] || [ -f wrangler.jsonc ] || [ -f wrangler.json ]; } && add_eco cloudflare-workers
{ [ -f flake.nix ] || [ -f default.nix ]; } && add_eco nix
[ -f Package.swift ] && add_eco swift
ls ./*.csproj ./*.sln >/dev/null 2>&1 && add_eco dotnet
ECOSYSTEMS="${ECOSYSTEMS# }"

# ------------------------------------------------------------------ tools ---

TOOLS_JSON=""
TOOLS_TEXT=""
FOUND_COUNT=0
MISSING_COUNT=0

probe() {
  _p_id="$1"; _p_group="$2"; _p_bin="$3"; _p_flag="$4"
  [ -n "$ONLY" ] && [ "$ONLY" != "$_p_group" ] && return 0

  if which_path "$_p_bin"; then
    _p_path="$WP_PATH"
    _p_ver=$(probe_version "$_p_path" "$_p_flag")
    _p_rc=$?
    [ "$_p_rc" = 124 ] && TIMEDOUT="$TIMEDOUT $_p_id"
    FOUND_COUNT=$((FOUND_COUNT + 1))

    # On PATH but answering nothing, and it did not time out. Usually a shim
    # standing in for a tool that is not installed: the Windows Store python3
    # alias is the common one, a zero-byte stub that prints "Python was not
    # found". Reported rather than listed as present, because present is what
    # it is not.
    if [ -z "$_p_ver" ] && [ "$FAST" = "0" ] && [ "$_p_flag" != "@none" ] && [ "$_p_rc" != 124 ]; then
      STUBS="$STUBS $_p_id"
    fi

    if [ "$MODE" = json ]; then
      esc "$_p_id";    _e_id="$ESC"
      esc "$_p_group"; _e_gr="$ESC"
      esc "$_p_path";  _e_pa="$ESC"
      esc "$_p_ver";   _e_ve="$ESC"
      TOOLS_JSON="$TOOLS_JSON    {\"id\":\"$_e_id\",\"group\":\"$_e_gr\",\"found\":true,\"path\":\"$_e_pa\",\"version\":\"$_e_ve\"},$NL"
    else
      TOOLS_TEXT="$TOOLS_TEXT  yes  $(printf '%-16s %-10s %s' "$_p_id" "${_p_ver:--}" "$_p_path")$NL"
    fi
  else
    MISSING_COUNT=$((MISSING_COUNT + 1))
    if [ "$MODE" = json ]; then
      esc "$_p_id";    _e_id="$ESC"
      esc "$_p_group"; _e_gr="$ESC"
      TOOLS_JSON="$TOOLS_JSON    {\"id\":\"$_e_id\",\"group\":\"$_e_gr\",\"found\":false,\"path\":\"\",\"version\":\"\"},$NL"
    else
      TOOLS_TEXT="$TOOLS_TEXT  no   $_p_id$NL"
    fi
  fi
}

# id|group|binary|version-flag   (@none = the tool has no version flag)
while IFS='|' read -r t_id t_group t_bin t_flag; do
  case "$t_id" in ''|\#*) continue ;; esac
  probe "$t_id" "$t_group" "$t_bin" "$t_flag"
done <<'TOOLTABLE'
git|vcs|git|--version
gh|vcs|gh|--version
git-lfs|vcs|git-lfs|--version
jj|vcs|jj|--version
hg|vcs|hg|--version
svn|vcs|svn|--version
node|runtime|node|--version
deno|runtime|deno|--version
bun|runtime|bun|--version
python3|runtime|python3|--version
python|runtime|python|--version
ruby|runtime|ruby|--version
php|runtime|php|--version
java|runtime|java|-version
dotnet|runtime|dotnet|--version
go|runtime|go|version
rustc|runtime|rustc|--version
zig|runtime|zig|version
perl|runtime|perl|--version
lua|runtime|lua|-v
gcc|compiler|gcc|--version
clang|compiler|clang|--version
cl|compiler|cl|@none
npm|pkg-lang|npm|--version
pnpm|pkg-lang|pnpm|--version
yarn|pkg-lang|yarn|--version
pip|pkg-lang|pip|--version
pipx|pkg-lang|pipx|--version
uv|pkg-lang|uv|--version
poetry|pkg-lang|poetry|--version
cargo|pkg-lang|cargo|--version
rustup|pkg-lang|rustup|--version
gem|pkg-lang|gem|--version
composer|pkg-lang|composer|--version
maven|pkg-lang|mvn|--version
gradle|pkg-lang|gradle|--version
scoop|pkg-system|scoop|@none
choco|pkg-system|choco|--version
winget|pkg-system|winget|--version
brew|pkg-system|brew|--version
apt|pkg-system|apt|--version
dnf|pkg-system|dnf|--version
pacman|pkg-system|pacman|--version
apk|pkg-system|apk|--version
zypper|pkg-system|zypper|--version
nix|pkg-system|nix|--version
docker|container|docker|--version
podman|container|podman|--version
kubectl|container|kubectl|version --client
wsl|container|wsl|@none
make|build|make|--version
cmake|build|cmake|--version
ninja|build|ninja|--version
just|build|just|--version
msbuild|build|msbuild|-version
task|build|task|--version
shellcheck|quality|shellcheck|--version
shfmt|quality|shfmt|--version
ruff|quality|ruff|--version
eslint|quality|eslint|--version
prettier|quality|prettier|--version
golangci-lint|quality|golangci-lint|--version
jq|cli|jq|--version
yq|cli|yq|--version
rg|cli|rg|--version
fd|cli|fd|--version
curl|cli|curl|--version
wget|cli|wget|--version
aria2c|cli|aria2c|--version
tar|cli|tar|--version
7z|cli|7z|@none
sqlite3|cli|sqlite3|--version
scc|cli|scc|--version
tokei|cli|tokei|--version
hyperfine|cli|hyperfine|--version
wrangler|cloud|wrangler|--version
aws|cloud|aws|--version
gcloud|cloud|gcloud|--version
az|cloud|az|--version
flyctl|cloud|flyctl|version
terraform|cloud|terraform|--version
bash|shell|bash|--version
zsh|shell|zsh|--version
pwsh|shell|pwsh|--version
powershell|shell|powershell|@none
codegraph|agent|codegraph|--version
TOOLTABLE

TOOLS_JSON=$(printf '%s' "$TOOLS_JSON" | sed '$ s/,$//')

if [ -n "$TIMEDOUT" ]; then
  note "no answer within ${PROBE_TIMEOUT}s:$TIMEDOUT - present, version unknown. Not the same fact as absent."
fi
if [ -n "$STUBS" ]; then
  note "on PATH but reported no version:$STUBS - probably a shim or a stub rather than a working install. Confirm before planning on one."
fi

# --------------------------------------------------------------- outbound ---

NET_OK=unknown
if [ "$NET" = "1" ]; then
  NET_OK=no
  if command -v curl >/dev/null 2>&1; then
    curl -sS -m 8 -o /dev/null https://example.com 2>/dev/null && NET_OK=yes
  elif command -v wget >/dev/null 2>&1; then
    wget -q -T 8 -O /dev/null https://example.com 2>/dev/null && NET_OK=yes
  else
    NET_OK=untested
    note "--net was requested but neither curl nor wget is available"
  fi
fi

NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || printf 'unknown')

# ----------------------------------------------------------------- output ---

if [ "$MODE" = json ]; then
  NOTES_JSON=""
  if [ -n "$NOTES" ]; then
    _first=1
    printf '%s' "$NOTES" > "${TMPDIR_OK:-/tmp}/.doctor-notes.$$"
    while IFS= read -r _n; do
      [ -z "$_n" ] && continue
      esc "$_n"
      if [ "$_first" = 1 ]; then NOTES_JSON="\"$ESC\""; _first=0
      else NOTES_JSON="$NOTES_JSON,\"$ESC\""; fi
    done < "${TMPDIR_OK:-/tmp}/.doctor-notes.$$"
    rm -f "${TMPDIR_OK:-/tmp}/.doctor-notes.$$"
  fi
  ECO_JSON=""
  for e in $ECOSYSTEMS; do
    esc "$e"
    if [ -z "$ECO_JSON" ]; then ECO_JSON="\"$ESC\""; else ECO_JSON="$ECO_JSON,\"$ESC\""; fi
  done

  esc "$NOW";         E_NOW="$ESC"
  esc "$ONLY";        E_ONLY="$ESC"
  esc "$OS";          E_OS="$ESC"
  esc "$FLAVOR";      E_FLAVOR="$ESC"
  esc "$UNAME_R";     E_KERNEL="$ESC"
  esc "$UNAME_M";     E_ARCH="$ESC"
  esc "$DISTRO";      E_DISTRO="$ESC"
  esc "$DISTRO_VER";  E_DISTROV="$ESC"
  esc "$SHELL_NAME";  E_SHELL="$ESC"
  esc "$TMPDIR_OK";   E_TMP="$ESC"
  esc "$NET_OK";      E_NET="$ESC"
  esc "$GIT_ROOT";    E_ROOT="$ESC"
  esc "$GIT_BRANCH";  E_BRANCH="$ESC"
  esc "$GIT_REMOTE";  E_REMOTE="$ESC"

  cat <<JSONEOF
{
  "schema": "$SCHEMA",
  "generated": "$E_NOW",
  "probe": { "impl": "doctor.sh", "fast": $(jbool "$FAST"), "group": "$E_ONLY" },
  "host": {
    "os": "$E_OS",
    "flavor": "$E_FLAVOR",
    "wsl": $(jbool "$WSL"),
    "container": $(jbool "$CONTAINER"),
    "kernel": "$E_KERNEL",
    "arch": "$E_ARCH",
    "distro": "$E_DISTRO",
    "distro_version": "$E_DISTROV",
    "shell": "$E_SHELL",
    "writable_tmp": "$E_TMP",
    "network": "$E_NET"
  },
  "repo": {
    "is_git": $(jbool "$IS_GIT"),
    "root": "$E_ROOT",
    "branch": "$E_BRANCH",
    "remote": "$E_REMOTE",
    "dirty": $(jbool "$GIT_DIRTY"),
    "commits": ${GIT_COMMITS:-0},
    "remote_looks_like_template": $(jbool "$REMOTE_IS_TEMPLATE"),
    "has_codegraph": $(jbool "$HAS_CODEGRAPH"),
    "ecosystems": [$ECO_JSON]
  },
  "summary": { "tools_found": $FOUND_COUNT, "tools_missing": $MISSING_COUNT },
  "tools": [
$TOOLS_JSON
  ],
  "notes": [$NOTES_JSON]
}
JSONEOF
  exit 0
fi

printf 'doctor  %s  (%s)\n\n' "$SCHEMA" "$NOW"
printf 'HOST\n'
printf '  os            %s (%s)\n' "$OS" "$FLAVOR"
printf '  arch          %s\n' "${UNAME_M:-unknown}"
printf '  kernel        %s\n' "${UNAME_R:-unknown}"
[ -n "$DISTRO" ] && printf '  distro        %s %s\n' "$DISTRO" "$DISTRO_VER"
printf '  wsl           %s\n' "$(if [ "$WSL" = 1 ]; then echo yes; else echo no; fi)"
printf '  container     %s\n' "$(if [ "$CONTAINER" = 1 ]; then echo yes; else echo no; fi)"
printf '  shell         %s\n' "$SHELL_NAME"
printf '  writable tmp  %s\n' "${TMPDIR_OK:-NONE}"
[ "$NET" = 1 ] && printf '  network       %s\n' "$NET_OK"

printf '\nREPO\n'
if [ "$IS_GIT" = 1 ]; then
  printf '  git root      %s\n' "$GIT_ROOT"
  printf '  branch        %s (%s commits)\n' "${GIT_BRANCH:-none}" "$GIT_COMMITS"
  printf '  origin        %s\n' "${GIT_REMOTE:-none}"
  printf '  tree          %s\n' "$(if [ "$GIT_DIRTY" = 1 ]; then echo dirty; else echo clean; fi)"
  [ "$REMOTE_IS_TEMPLATE" = 1 ] && printf '  %s origin still points at a template remote. Detach before committing project work.\n' "$(printf '\342\232\240')"
else
  printf '  not a git repository\n'
fi
printf '  codegraph     %s\n' "$(if [ "$HAS_CODEGRAPH" = 1 ]; then echo indexed; else echo absent; fi)"
printf '  ecosystems    %s\n' "${ECOSYSTEMS:-none detected}"

printf '\nTOOLS  (%s found, %s missing)\n' "$FOUND_COUNT" "$MISSING_COUNT"
printf '%s' "$TOOLS_TEXT"

if [ -n "$NOTES" ]; then
  printf '\nNOTES\n'
  printf '%s' "$NOTES" | sed 's/^/  /'
fi

printf '\nThis is a probe, not a gate. A missing tool is data.\n'
printf 'Machine-readable: sh %s --json\n' "$0"
exit 0
