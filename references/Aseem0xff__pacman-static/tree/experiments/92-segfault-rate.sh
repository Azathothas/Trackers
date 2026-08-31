#!/bin/sh
# 92-segfault-rate.sh
#
# QUESTION: how often does `pacman -S base` end in SIGSEGV after a fully
# successful install, and where in the code does it happen?
#
# ⛔ THIS SUPERSEDES 91-segfault-control.sh, WHICH ASKED THE WRONG QUESTION.
# 91- is a 2x2 of two factors (patch applied, cache warm) with each cell run
# twice. Every cell passed, and two other small samples had already produced
# two confident, wrong causes:
#
#   "it is the missing libalpm curl patch"  -- disproved by applying it
#   "it is path-specific"                   -- disproved by --debug at the
#                                              same path exiting 0
#   "it needs sync+install in one process"  -- disproved by an 8-cell matrix
#                                              in which all 8 passed
#
# ⭐ THE FAULT IS INTERMITTENT, so a factor matrix with two runs per cell
# cannot see it: any cell can pass twice by luck. The right instrument for an
# intermittent fault is a RATE over many identical trials, plus a backtrace
# from a real crash. That is what this is.
#
# 91- is kept in the tree, labelled superseded, because revision 1's numbers
# have to stay traceable to whatever produced them.
#
# ⚠ WHAT IS ALREADY KNOWN AND IS NOT IN DOUBT: when it crashes, the install
# has ALREADY COMPLETED. 137 packages are in the local database, the root is
# ~700 MB, it chroots, and its own pacman runs. Only the exit status is
# wrong. That is still a blocker -- no caller can tell it from a real
# failure -- but it is not data loss.
#
# USAGE
#   ./92-segfault-rate.sh [TRIALS]        default 10
#
# EXIT CODES
#   0  every trial exited 0
#   1  at least one trial crashed (the rate and any backtrace are in the report)
#   2  the measurement could not run
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
WORK=${WORK:-/home/user/work}
TRIALS=${1:-10}
PACMAN=${PACMAN:-$WORK/pacman/x86_64/build/pacman}
CONF=${CONF:-$WORK/rootfs/conf/pacman.conf}
BASE=$WORK/segrate
OUT="$HERE/out/92-segfault-rate.txt"

[ -x "$PACMAN" ] || { echo "92: no pacman at $PACMAN -- run 80- first" >&2; exit 2; }
[ -r "$CONF" ]   || { echo "92: no pacman.conf at $CONF -- run 90- once" >&2; exit 2; }
[ "$(id -u)" = 0 ] || { echo "92: needs root" >&2; exit 2; }
mkdir -p "$HERE/out" "$BASE" || exit 2

# ⭐ CORE DUMPS ON. A backtrace ends the guessing that produced three wrong
# causes above. The binary is built unstripped with debug_info, so gdb can
# name the function without a separate symbol file.
ulimit -c unlimited 2>/dev/null
COREDIR=$BASE/cores; mkdir -p "$COREDIR"
core_pattern=$(cat /proc/sys/kernel/core_pattern 2>/dev/null || echo '?')

crashes=0; ok=0; pkgs_on_crash=''; bt=''
rows=''

i=1
while [ "$i" -le "$TRIALS" ]; do
  R=$BASE/t$i
  rm -rf "$R"
  mkdir -p "$R/var/lib/pacman" "$R/var/cache/pacman/pkg" \
           "$R/etc/pacman.d/hooks" "$R/etc/pacman.d/gnupg" "$R/var/log"
  ( cd "$COREDIR" && ulimit -c unlimited 2>/dev/null
    "$PACMAN" --config "$CONF" --root "$R" --dbpath "$R/var/lib/pacman" \
      --cachedir "$R/var/cache/pacman/pkg" --hookdir "$R/etc/pacman.d/hooks" \
      --gpgdir "$R/etc/pacman.d/gnupg" --logfile "$R/var/log/pacman.log" \
      -Sy --noconfirm base ) > "$BASE/t$i.log" 2>&1
  e=$?
  n=$(ls -1 "$R/var/lib/pacman/local" 2>/dev/null | grep -vc '^ALPM_DB_VERSION$' || echo 0)
  case $e in
    0)   ok=$((ok+1)); st=ok ;;
    139) crashes=$((crashes+1)); st=SIGSEGV; pkgs_on_crash="$pkgs_on_crash $n" ;;
    *)   crashes=$((crashes+1)); st="exit$e" ;;
  esac
  rows="$rows$(printf '%-7s %-9s %s' "$i" "$st" "$n")
"
  # First crash with a core: get the backtrace and stop collecting more.
  if [ "$st" = SIGSEGV ] && [ -z "$bt" ]; then
    c=$(ls -t "$COREDIR"/core* 2>/dev/null | head -1)
    if [ -n "$c" ] && command -v gdb >/dev/null 2>&1; then
      bt=$(gdb -batch -q -ex 'bt 25' "$PACMAN" "$c" 2>/dev/null | sed -n '1,40p')
    fi
  fi
  rm -rf "$R"
  i=$((i+1))
done

rate=$(( crashes * 100 / TRIALS ))
{
  echo "# 92-segfault-rate"
  echo "date         : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host         : $(uname -srm), $(nproc 2>/dev/null) cores"
  echo "binary       : $PACMAN"
  echo "trials       : $TRIALS, each into a fresh empty root, cold cache"
  echo "command      : pacman -Sy --noconfirm base"
  echo "core_pattern : $core_pattern"
  echo
  printf '%-7s %-9s %s\n' TRIAL RESULT 'PKGS INSTALLED'
  printf '%s' "$rows"
  echo
  printf '%-22s %s\n' 'exited 0'  "$ok"
  printf '%-22s %s\n' 'crashed'   "$crashes"
  printf '%-22s %s%%\n' 'crash rate' "$rate"
  echo
  echo "⚠ PKGS INSTALLED is the count in the local database when the process"
  echo "  ended. A crash row showing the full count is the install having"
  echo "  COMPLETED before the crash."
  echo
  if [ -n "$bt" ]; then
    echo "## backtrace from the first crash"
    echo
    printf '%s\n' "$bt" | sed 's/^/  /'
  else
    echo "## no backtrace"
    echo
    echo "  Either no trial crashed, or no core file was produced."
    echo "  core_pattern is '$core_pattern'; a pattern beginning with '|' pipes"
    echo "  cores to a handler and no file lands in the working directory."
    echo "  Set it to 'core' and re-run to collect one."
  fi
  echo
  echo "verdict: $([ "$crashes" -eq 0 ] && echo "no crash in $TRIALS trials" \
                                       || echo "$crashes/$TRIALS crashed -- intermittent")"
} > "$OUT"
cat "$OUT"
[ "$crashes" -eq 0 ] || exit 1
exit 0
