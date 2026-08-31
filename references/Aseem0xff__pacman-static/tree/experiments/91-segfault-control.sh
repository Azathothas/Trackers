#!/bin/sh
# 91-segfault-control.sh
#
# ⛔⛔ SUPERSEDED BY 92-segfault-rate.sh. KEPT ON PURPOSE, NOT LIVE.
#
# This script asks a 2x2 factor question about a fault that turned out to be
# INTERMITTENT. Two runs per cell cannot distinguish "this cell is safe" from
# "this cell got lucky twice", so every cell passed and the table said
# nothing. Its numbers are still in out/91-segfault-control.txt and they are
# real; they just do not answer the question the file claims to ask.
#
# Deleting it would orphan revision 1's numbers, so it stays. Run
# 92-segfault-rate.sh instead: a rate over many identical trials, plus a
# backtrace from a real crash.
#
# QUESTION: what actually causes the "error: segmentation fault" that ends an
# otherwise successful `pacman -S base` in 90-bootstrap-arch.sh?
#
# ⛔ THIS SCRIPT EXISTS BECAUSE THE FIRST ANSWER WAS WRONG. The crash lands in
# the scriptlet/hook phase, the Manjaro fork carries a patch whose commit
# message is "Touching that data inside the child makes it crash", and the
# obvious conclusion was that the missing patch was the cause. It was applied,
# rebuilt, and the crash reproduced identically. A cause published on that
# reasoning would have been withdrawn.
#
# ⚠ THE SECOND OBSERVATION was that a `--debug` run exited 0 -- but that run
# also had a WARM PACKAGE CACHE, so two things differed at once. This script
# holds each one still.
#
# THE MATRIX. Two factors, two levels, four cells, each run twice:
#   patch  : the libalpm curl-in-child patch applied, or not
#   cache   : package cache empty (downloads happen) or warm (they do not)
#
# ⭐ EACH CELL RUNS TWICE. A control run once is a coincidence you have not
# noticed yet, and this is the second time in this sweep that a single run
# produced a confident wrong answer.
#
# EXIT CODES
#   0  the matrix ran and every cell agreed across its two runs
#   1  the matrix ran and at least one cell was not reproducible
#   2  the measurement could not run
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
WORK=${WORK:-/home/user/work}
OUT="$HERE/out/91-segfault-control.txt"
PATCHED=$WORK/pacman-patched
UNPATCHED=$WORK/pacman-unpatched
CONF=$WORK/rootfs/conf/pacman.conf
WARMCACHE=$WORK/segctl/warmcache
PKGSET=${PKGSET:-base}

[ -x "$PATCHED" ] && [ -x "$UNPATCHED" ] || {
  echo "91: need both binaries. Build them with:" >&2
  echo "    NO_PATCHES=1 ./80-build-pacman.sh x86_64-linux-musl && cp \$WORK/pacman/x86_64/build/pacman \$WORK/pacman-unpatched" >&2
  echo "                 ./80-build-pacman.sh x86_64-linux-musl && cp \$WORK/pacman/x86_64/build/pacman \$WORK/pacman-patched" >&2
  exit 2; }
[ -r "$CONF" ] || { echo "91: no pacman.conf at $CONF -- run 90-bootstrap-arch.sh once" >&2; exit 2; }
[ "$(id -u)" = 0 ] || { echo "91: needs root" >&2; exit 2; }
mkdir -p "$HERE/out" "$WARMCACHE" || exit 2

run_cell() {  # binary  cachedir  tag
  _bin=$1; _cache=$2; _tag=$3
  _r=$WORK/segctl/root-$_tag
  rm -rf "$_r"
  mkdir -p "$_r/var/lib/pacman" "$_r/etc/pacman.d/hooks" "$_r/etc/pacman.d/gnupg" "$_r/var/log" "$_cache"
  "$_bin" --config "$CONF" --root "$_r" --dbpath "$_r/var/lib/pacman" \
     --cachedir "$_cache" --hookdir "$_r/etc/pacman.d/hooks" \
     --gpgdir "$_r/etc/pacman.d/gnupg" --logfile "$_r/var/log/pacman.log" \
     -Sy --noconfirm $PKGSET > "$WORK/segctl/$_tag.log" 2>&1
  _e=$?
  _n=$(ls -1 "$_r/var/lib/pacman/local" 2>/dev/null | grep -vc '^ALPM_DB_VERSION$' || echo 0)
  rm -rf "$_r"
  # 139 = SIGSEGV. Report the signal, not just "failed".
  case $_e in
    0)   printf 'ok'      ;;
    139) printf 'SIGSEGV' ;;
    *)   printf 'exit%s' "$_e" ;;
  esac
  printf ' %s' "$_n"
}

# ⚠ WARM THE CACHE ONCE, with a binary that is not on trial, so that the warm
# cells really do skip every download.
if [ -z "$(ls -A "$WARMCACHE" 2>/dev/null)" ]; then
  run_cell "$PATCHED" "$WARMCACHE" warmup >/dev/null 2>&1 || true
fi

rc=0
{
  echo "# 91-segfault-control"
  echo "date      : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host      : $(uname -srm)"
  echo "package   : $PKGSET"
  echo "patch     : 0001-libalpm-invalidate-curl-data-in-child.patch"
  echo "warm cache: $(ls -1 "$WARMCACHE" 2>/dev/null | wc -l | tr -d ' ') files"
  echo
  printf '%-10s %-8s %-16s %-16s %s\n' PATCH CACHE 'RUN 1 (pkgs)' 'RUN 2 (pkgs)' AGREE
} > "$OUT"

for p in unpatched patched; do
  case $p in patched) bin=$PATCHED ;; *) bin=$UNPATCHED ;; esac
  for c in cold warm; do
    if [ "$c" = warm ]; then cache=$WARMCACHE; else cache=$WORK/segctl/cold-$p; rm -rf "$cache"; fi
    r1=$(run_cell "$bin" "$cache" "$p-$c-1")
    if [ "$c" = cold ]; then rm -rf "$cache"; fi
    r2=$(run_cell "$bin" "$cache" "$p-$c-2")
    a=yes; [ "${r1%% *}" = "${r2%% *}" ] || { a=NO; rc=1; }
    printf '%-10s %-8s %-16s %-16s %s\n' "$p" "$c" "$r1" "$r2" "$a" >> "$OUT"
  done
done

{
  echo
  echo "  'ok N'      = exit 0, N packages in the local database"
  echo "  'SIGSEGV N' = killed by signal 11 after installing N packages"
  echo
  echo "verdict: $([ $rc -eq 0 ] && echo 'every cell reproduced across both runs' \
                                 || echo 'a cell did not reproduce -- do not draw a cause from this')"
} >> "$OUT"
cat "$OUT"
exit $rc
