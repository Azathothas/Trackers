#!/bin/sh
# 50-zig-cross-targets.sh
#
# QUESTION: can `zig cc` stand in for five GCC+musl cross toolchains -- that
# is, does it produce a *statically linked* musl binary, for each of the five
# required targets, that resolves the libc calls pacman actually makes?
#
# Why it is worth asking: building mussel toolchains for five targets is the
# long pole of the whole project. If one 55 MB download replaces it, the build
# plan changes shape. loongarch64 is the deciding target, because mussel has
# no loongarch64 case at all (see 30-mussel-target-support.sh).
#
# ORACLE: the answer is read off the produced ELF with `readelf`, never from
# zig's own claim of support. `zig targets` listing a triple is evidence of
# intent; a linked binary with the right e_machine and no PT_INTERP is
# evidence of behaviour. Where qemu-user is present the binary is also RUN,
# which is the only check that the libc calls actually resolve.
#
# PINNED INPUTS
#   zig            0.16.0, x86_64-linux tarball
#   zig sha256     70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
#   fixture        fixtures/libc-surface.c (in this tree)
#
# EXIT CODES
#   0  the measurement ran
#   1  the measurement ran and at least one target failed
#   2  the measurement could not run
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
FIXTURE="$HERE/fixtures/libc-surface.c"
WORK=${WORK:-/home/user/work}
ZIG=${ZIG:-$WORK/zig/zig}
ZIG_SHA256=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
ZIG_URL=https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz
OUT="$HERE/out/50-zig-cross-targets.txt"

TARGETS='x86_64-linux-musl aarch64-linux-musl riscv64-linux-musl loongarch64-linux-musl powerpc64le-linux-musl'

[ -r "$FIXTURE" ] || { echo "50: fixture missing: $FIXTURE" >&2; exit 2; }
if [ ! -x "$ZIG" ]; then
  echo "50: zig not found at $ZIG" >&2
  echo "50: fetch it with:" >&2
  echo "    curl -fsSL -o zig.tar.xz $ZIG_URL" >&2
  echo "    echo '$ZIG_SHA256  zig.tar.xz' | sha256sum -c -" >&2
  echo "    mkdir -p $WORK/zig && tar xf zig.tar.xz -C $WORK/zig --strip-components=1" >&2
  exit 2
fi
command -v readelf >/dev/null 2>&1 || { echo "50: readelf not found" >&2; exit 2; }

TMP=$(mktemp -d) || exit 2
trap 'rm -rf "$TMP"' EXIT
rc=0

{
  echo "# 50-zig-cross-targets"
  echo "date            : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host            : $(uname -srm)"
  echo "host distro     : $(. /etc/os-release 2>/dev/null && echo "$PRETTY_NAME")"
  echo "cores           : $(nproc 2>/dev/null || echo '-')"
  echo "zig version     : $($ZIG version)"
  echo "zig path        : $ZIG"
  echo "fixture         : fixtures/libc-surface.c"
  echo "qemu-user       : $(command -v qemu-x86_64-static >/dev/null 2>&1 && echo present || echo ABSENT)"
  echo
  printf '%-26s %-8s %-10s %-9s %-8s %s\n' TARGET BUILD E_MACHINE LINKAGE BYTES RUN
} > "$OUT"

for t in $TARGETS; do
  bin="$TMP/probe-$t"
  log="$TMP/log-$t"
  if "$ZIG" cc -target "$t" -static -Os -o "$bin" "$FIXTURE" > "$log" 2>&1; then
    build=ok
  else
    build=FAIL; rc=1
  fi

  machine='-'; linkage='-'; bytes='-'; run='-'
  if [ "$build" = ok ] && [ -f "$bin" ]; then
    machine=$(readelf -h "$bin" 2>/dev/null | awk -F: '/Machine:/{gsub(/^ +| +$/,"",$2); print $2}')
    case $machine in
      'Advanced Micro Devices X86-64') machine=x86-64 ;;
      'AArch64')                       machine=aarch64 ;;
      'RISC-V')                        machine=riscv64 ;;
      'LoongArch')                     machine=loongarch ;;
      'PowerPC64')                     machine=ppc64 ;;
    esac
    # ⚠ STATIC IS ASSERTED ON THE PROGRAM HEADERS, NOT ON THE COMPILER FLAG.
    # `-static` was requested; PT_INTERP being absent is what proves it landed.
    if readelf -l "$bin" 2>/dev/null | grep -q INTERP; then linkage=dynamic; rc=1
    else linkage=static; fi
    bytes=$(wc -c < "$bin" | tr -d ' ')

    # RUN it where an emulator exists. A binary that links and cannot resolve
    # getpwnam is a failure this table must not report as a pass.
    # ⚠ THE EMULATOR NAME IS NOT THE TRIPLE PREFIX. Debian ships ppc64le as
    # `qemu-ppc64le-static` while the triple says `powerpc64le`; deriving the
    # name by cut(1) reported "no emulator" for an emulator that was installed,
    # and the run column read '-' rather than a result. Map it explicitly.
    qarch=$(printf '%s' "$t" | cut -d- -f1)
    case $qarch in
      powerpc64le) qarch=ppc64le ;;
      powerpc64)   qarch=ppc64 ;;
      powerpc)     qarch=ppc ;;
    esac
    for q in "qemu-$qarch-static" "qemu-$qarch"; do
      if command -v "$q" >/dev/null 2>&1; then
        if "$q" "$bin" > "$TMP/run-$t" 2>&1; then
          run=$(awk -F= '/^verdict=/{print $2}' "$TMP/run-$t")
        else
          run="EXIT$?"; rc=1
        fi
        break
      fi
    done
  fi

  printf '%-26s %-8s %-10s %-9s %-8s %s\n' "$t" "$build" "$machine" "$linkage" "$bytes" "$run" >> "$OUT"
  [ "$build" = FAIL ] && { echo; echo "--- $t build log ---"; sed -n '1,15p' "$log"; } >> "$OUT"
done

{
  echo
  echo "verdict: $([ $rc -eq 0 ] && echo 'every target built static' || echo 'at least one target failed')"
  echo "note   : RUN is '-' where no qemu-user emulator for that architecture is"
  echo "         installed. '-' means NOT MEASURED, never 'passed'."
} >> "$OUT"

cat "$OUT"
exit $rc
