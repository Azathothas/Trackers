#!/bin/sh
# 80-build-pacman.sh
#
# QUESTION: with the dependency stack from 70- in place, does pacman itself
# cross-build and statically link with `zig cc`, and does the resulting
# binary RUN on its target architecture?
#
# ⭐ THIS IS THE CLAIM THE WHOLE PROJECT RESTS ON, so it is measured three
# ways and only the third is worth anything:
#   1. meson compiles it            -- says the build system was satisfied
#   2. the ELF has no PT_INTERP     -- says the link was really static
#   3. qemu-user runs `pacman -V`   -- says the binary WORKS on that target
# A build plan that stops at 1 has proved nothing; a statically linked
# binary that segfaults on riscv64 still passes 1 and 2.
#
# ORACLE for step 3: qemu-user-static, which is an emulator of the target
# ISA and knows nothing about how the binary was built.
#
# ⚠ WHAT IT DOES NOT MEASURE: real hardware. qemu-user emulates the ISA and
# passes syscalls to the host kernel, so it does not exercise the target's
# kernel, its page size (a real aarch64 or ppc64le host may use 64K pages),
# or its actual filesystem. See TASKS.md T-14.
#
# PINNED INPUTS
#   pacman  git tag v7.1.0 + patch-level commit
#           54d94116164b0b2202c6061c4a59c6f3e70820d8
#           (the reference PKGBUILD's pin; the tag is signed by
#            6645B0A8C7005E78DB1D7864F99FFE0FEAE999BD, Allan McRae)
#   deps    whatever 70-build-static-stack.sh installed into $PREFIX
#
# USAGE
#   ./80-build-pacman.sh [TRIPLE]      default x86_64-linux-musl
#
# EXIT CODES
#   0  built, statically linked, and ran
#   1  the measurement ran and the binary failed one of the three checks
#   2  the measurement could not run
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH= cd -- "$HERE/.." && pwd)
TRIPLE=${1:-x86_64-linux-musl}
WORK=${WORK:-/home/user/work}
ZIG=${ZIG:-$WORK/zig/zig}
JOBS=${JOBS:-$(nproc 2>/dev/null || echo 2)}
PACMAN_SRC=${PACMAN_SRC:-$ROOT/references/archlinux__pacman/tree}
PACMAN_COMMIT=54d94116164b0b2202c6061c4a59c6f3e70820d8

ARCH=${TRIPLE%%-*}
PREFIX=$WORK/out/$ARCH
BIN=$WORK/bin/$ARCH
BLD=$WORK/pacman/$ARCH
OUT="$HERE/out/80-build-pacman.$ARCH.txt"

[ -x "$ZIG" ]        || { echo "80: zig not at $ZIG" >&2; exit 2; }
[ -d "$PREFIX/lib" ] || { echo "80: no dependency prefix at $PREFIX -- run 70 first" >&2; exit 2; }
[ -x "$BIN/cc" ]     || { echo "80: no compiler wrappers at $BIN -- run 70 first" >&2; exit 2; }
[ -d "$PACMAN_SRC" ] || { echo "80: pacman source missing at $PACMAN_SRC" >&2; exit 2; }
command -v meson >/dev/null 2>&1 || { echo "80: meson not found" >&2; exit 2; }

case $ARCH in
  x86_64)      CPUFAM=x86_64;  QEMU=qemu-x86_64-static ;;
  aarch64)     CPUFAM=aarch64; QEMU=qemu-aarch64-static ;;
  riscv64)     CPUFAM=riscv64; QEMU=qemu-riscv64-static ;;
  loongarch64) CPUFAM=loongarch64; QEMU=qemu-loongarch64-static ;;
  powerpc64le) CPUFAM=ppc64;   QEMU=qemu-ppc64le-static ;;
  *) echo "80: unknown arch $ARCH" >&2; exit 2 ;;
esac

rm -rf "$BLD"; mkdir -p "$BLD" "$HERE/out" || exit 2

# ⚠ A COPY, NOT THE CORPUS. The corpus tree is shared evidence; building in
# it would leave it dirty and make every later citation ambiguous.
#
# ⭐ THE CORPUS IS PLAIN FILES, ALREADY AT THE PINNED COMMIT, and no git is
# needed to use it. Two earlier versions of this needed git and both broke:
# `git clone --shared` cannot resolve a commit outside a shallow clone's own
# refs ("reference is not a tree" for an object that is in the shared object
# store), and `git archive` needed the corpus to still carry a .git directory,
# which it does not -- a nested git directory would be committed as a gitlink
# and a fresh clone would land an empty folder.
#
# references/archlinux__pacman/PROVENANCE.md records the commit. That is the
# provenance; the tree is the evidence.
mkdir -p "$BLD"
cp -a "$PACMAN_SRC" "$BLD/src"
[ -f "$BLD/src/meson.build" ] || { echo "80: export produced no meson.build" >&2; exit 2; }

# ⛔ PATCHES. Measured, not copied: 90-bootstrap-arch.sh installed 137
# packages and then died with "error: segmentation fault" in the scriptlet
# phase, on stock v7.1.0+54d94116. The crash is in _alpm_run_chroot's forked
# child touching libcurl state the parent owns. Upstream curl issue 21466.
#
# ⚠ THE FIX IS CARRIED BY THE DOWNSTREAM FORK AND NOT BY THE CANONICAL
# PACKAGE. aur/pacman-static at 8c58e7db does not apply it; the Manjaro fork
# at aad8fa5b does. A session that took the canonical package as strictly
# newer-and-better -- which it is in every other respect -- would ship a
# pacman that segfaults at the end of every bootstrap.
#
# The reference PKGBUILD's other two patches are makepkg-only (a libdepends
# revert and a reproducible-builds change) and do not affect the pacman
# binary, so they are not applied here.
PATCHDIR=${PATCHDIR:-$ROOT/patches/pacman}
patches_applied=none
if [ "${NO_PATCHES:-0}" != 1 ] && [ -d "$PATCHDIR" ]; then
  patches_applied=
  for p in "$PATCHDIR"/*.patch; do
    [ -e "$p" ] || continue
    if patch -d "$BLD/src" -Np1 -i "$p" > "$BLD/patch.log" 2>&1; then
      patches_applied="$patches_applied $(basename "$p")"
    else
      echo "80: patch failed: $p" >&2; tail -5 "$BLD/patch.log" >&2; exit 2
    fi
  done
  [ -n "$patches_applied" ] || patches_applied=none
fi

# ⛔ -static GOES IN THE CROSS FILE, NOT THE ENVIRONMENT. meson in cross mode
# reads c_link_args from here; LDFLAGS set in the shell reaches the native
# compiler used for build-machine helpers instead, and the result links
# dynamically while every flag looks right.
cat > "$BLD/cross.ini" <<EOF
[binaries]
c          = '$BIN/cc'
cpp        = '$BIN/c++'
ar         = '$BIN/ar'
ranlib     = '$BIN/ranlib'
strip      = '$ZIG'
pkg-config = 'pkg-config'

[built-in options]
c_args     = ['-Os', '-fno-stack-protector', '-D_LARGEFILE64_SOURCE', '-I$PREFIX/include']
c_link_args = ['-static', '-L$PREFIX/lib']

[properties]
pkg_config_libdir = '$PREFIX/lib/pkgconfig'
needs_exe_wrapper = true

[host_machine]
system     = 'linux'
cpu_family = '$CPUFAM'
cpu        = '$ARCH'
endian     = 'little'
EOF

PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig";  export PKG_CONFIG_PATH
PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig"; export PKG_CONFIG_LIBDIR

rc=0
t0=$(date +%s)
# Options mirror the reference PKGBUILD's meson line, at its commit
# 8c58e7db1c52286bba77fd644ae1d77cc5db9e97.
if meson setup "$BLD/build" "$BLD/src" \
      --cross-file "$BLD/cross.ini" \
      --prefix=/usr \
      --includedir=lib/pacman/include \
      --libdir=lib/pacman/lib \
      --buildtype=plain \
      -Dbuildstatic=true \
      -Ddefault_library=static \
      -Ddoc=disabled \
      -Ddoxygen=disabled \
      -Di18n=false \
      -Dcrypto=openssl \
      -Dldconfig=/usr/bin/ldconfig \
      -Dscriptlet-shell=/usr/bin/bash \
      > "$BLD/setup.log" 2>&1; then
  setup=ok
else
  setup=FAIL; rc=1
fi

compile=skipped
if [ "$setup" = ok ]; then
  if meson compile -C "$BLD/build" -j "$JOBS" > "$BLD/compile.log" 2>&1; then
    compile=ok
  else
    compile=FAIL; rc=1
  fi
fi
t1=$(date +%s)

# ⚠ MESON PUTS TARGETS AT THE BUILD ROOT, not under src/pacman/ the way the
# source tree is laid out. The first version of this script looked in
# build/src/pacman/pacman, found nothing, skipped all three checks, and still
# printed "built, linked static, and ran" because rc had never been set. An
# absence is not a pass: a missing binary is now a failure.
PACMAN_BIN="$BLD/build/pacman"
linkage='-'; bytes='-'; run='-'; ver='-'
if [ "$compile" = ok ] && [ ! -f "$PACMAN_BIN" ]; then
  echo "80: compile reported ok but no binary at $PACMAN_BIN" >&2
  rc=1
fi
if [ -f "$PACMAN_BIN" ]; then
  if readelf -l "$PACMAN_BIN" 2>/dev/null | grep -q INTERP; then linkage=dynamic; rc=1
  else linkage=static; fi
  bytes=$(wc -c < "$PACMAN_BIN" | tr -d ' ')
  if command -v "$QEMU" >/dev/null 2>&1; then
    # ⭐ THE ONLY CHECK THAT PROVES ANYTHING. Grep the version out of the
    # banner rather than trusting the exit code: pacman prints its logo to
    # stdout and a shell that only checked $? would pass on an empty run.
    ver=$("$QEMU" "$PACMAN_BIN" --version 2>&1 | grep -oE 'Pacman v[0-9.]+ - libalpm v[0-9.]+' | head -1)
    if [ -n "$ver" ]; then run=ok; else run=FAIL; ver='(no version banner)'; rc=1; fi
  else
    run='NOT MEASURED'
  fi
fi

{
  echo "# 80-build-pacman"
  echo "date        : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host        : $(uname -srm), $(nproc 2>/dev/null) cores"
  echo "compiler    : zig $($ZIG version) cc -target $TRIPLE"
  echo "meson       : $(meson --version)"
  echo "target      : $TRIPLE"
  echo "pacman      : $PACMAN_COMMIT (v7.1.0 + patch level)"
  echo "deps prefix : $PREFIX"
  echo "patches     :$patches_applied"
  echo
  printf '%-14s %s\n' 'meson setup'   "$setup"
  printf '%-14s %s\n' 'meson compile' "$compile"
  printf '%-14s %ss\n' 'build time'   "$((t1-t0))"
  printf '%-14s %s\n' 'linkage'       "$linkage"
  printf '%-14s %s\n' 'size (bytes)'  "$bytes"
  printf '%-14s %s\n' 'emulator'      "$(command -v "$QEMU" >/dev/null 2>&1 && echo "$QEMU" || echo 'ABSENT -- run NOT MEASURED')"
  printf '%-14s %s\n' 'pacman -V'     "$run"
  echo
  printf '%-14s %s\n' 'reported'      "$ver"
  echo
  if [ "$setup" = FAIL ]; then echo "--- meson setup tail ---"; tail -25 "$BLD/setup.log"; fi
  if [ "$compile" = FAIL ]; then echo "--- meson compile tail ---"; tail -30 "$BLD/compile.log"; fi
  echo
  echo "verdict: $([ $rc -eq 0 ] && echo 'pacman built, linked static, and ran' || echo 'FAILED -- see logs in '"$BLD")"
} > "$OUT"

cat "$OUT"
exit $rc
