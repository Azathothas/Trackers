#!/bin/sh
# 85-feature-matrix.sh
#
# QUESTION: which of pacman's optional features are actually compiled into
# each architecture's binary, and which were dropped?
#
# ⭐ TWO KINDS OF EVIDENCE, REPORTED SEPARATELY, because they answer different
# questions and only one of them is about the shipped binary:
#
#   BUILD-TIME  the version meson resolved for each dependency, read out of
#               the setup log. Says the library was found and linked.
#   RUNTIME     the binary executed under qemu-user. Says it works.
#
# ⛔ A DEPENDENCY MESON "FOUND" IS NOT A WORKING FEATURE. libseccomp is
# `required: false` in pacman's meson: a target where it is missing builds
# fine and silently loses the download sandbox. That is exactly the kind of
# difference this table exists to make visible.
#
# EXIT CODES
#   0  every architecture reports the same feature set
#   1  the architectures disagree, or a binary is missing
#   2  the measurement could not run
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
WORK=${WORK:-/home/user/work}
OUT="$HERE/out/85-feature-matrix.txt"
ARCHES=${ARCHES:-'x86_64 aarch64 riscv64 loongarch64 powerpc64le'}
mkdir -p "$HERE/out" || exit 2

qemu_for() {
  case $1 in
    x86_64) echo qemu-x86_64-static ;; aarch64) echo qemu-aarch64-static ;;
    riscv64) echo qemu-riscv64-static ;; loongarch64) echo qemu-loongarch64-static ;;
    powerpc64le) echo qemu-ppc64le-static ;; *) echo '' ;;
  esac
}

# dep name as meson prints it | what it gives pacman
DEPS='libarchive libcurl gpgme libcrypto libseccomp'

rc=0
sig=''
{
  echo "# 85-feature-matrix"
  echo "date : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host : $(uname -srm)"
  echo
  echo "## build-time: the version meson resolved, per architecture"
  echo
  printf '%-14s %-12s %-10s %-9s %-10s %-11s %s\n' ARCH libarchive libcurl gpgme libcrypto libseccomp landlock
} > "$OUT"

for a in $ARCHES; do
  log=$WORK/pacman/$a/setup.log
  if [ ! -r "$log" ]; then
    printf '%-14s %s\n' "$a" 'NOT BUILT' >> "$OUT"; rc=1; continue
  fi
  row=$(printf '%-14s' "$a")
  line=''
  for d in $DEPS; do
    v=$(grep -oE "Run-time dependency $d found: YES [0-9][0-9.]*" "$log" | head -1 | awk '{print $NF}')
    [ -n "$v" ] || v='-'
    line="$line $v"
  done
  ll=$(grep -c 'Has header "linux/landlock.h" : YES' "$log" 2>/dev/null)
  [ "$ll" -gt 0 ] 2>/dev/null && ll=yes || ll=no
  # shellcheck disable=SC2086
  printf '%-14s %-12s %-10s %-9s %-10s %-11s %s\n' "$a" $line "$ll" >> "$OUT"
  this="$line $ll"
  if [ -z "$sig" ]; then sig=$this; elif [ "$sig" != "$this" ]; then rc=1; fi
done

{
  echo
  echo "## runtime: the binary itself"
  echo
  printf '%-14s %-9s %-11s %-22s %s\n' ARCH LINKAGE BYTES 'REPORTS' EMULATOR
} >> "$OUT"

for a in $ARCHES; do
  bin=$WORK/pacman/$a/build/pacman
  q=$(qemu_for "$a")
  if [ ! -f "$bin" ]; then
    printf '%-14s %s\n' "$a" 'NOT BUILT' >> "$OUT"; rc=1; continue
  fi
  lk=$(readelf -l "$bin" 2>/dev/null | grep -q INTERP && echo dynamic || echo static)
  [ "$lk" = static ] || rc=1
  sz=$(wc -c < "$bin" | tr -d ' ')
  if [ -n "$q" ] && command -v "$q" >/dev/null 2>&1; then
    ver=$("$q" "$bin" --version 2>&1 | grep -oE 'libalpm v[0-9.]+' | head -1)
    [ -n "$ver" ] || { ver='NO BANNER'; rc=1; }
  else
    ver='NOT MEASURED'; q='(absent)'
  fi
  printf '%-14s %-9s %-11s %-22s %s\n' "$a" "$lk" "$sz" "$ver" "$q" >> "$OUT"
done

{
  echo
  echo "## deliberately off"
  echo
  printf '  %-22s %s\n' 'i18n'   'meson -Di18n=false; see docs/FEATURES.md'
  printf '  %-22s %s\n' 'doc'    'meson -Ddoc=disabled; needs asciidoc/a2x'
  printf '  %-22s %s\n' 'nettle' 'meson -Dcrypto=openssl; nettle is the alternative, not an addition'
  echo
  echo "verdict: $([ $rc -eq 0 ] && echo 'every architecture reports the same feature set' \
                                 || echo 'architectures disagree, or a binary is missing')"
} >> "$OUT"
cat "$OUT"
exit $rc
