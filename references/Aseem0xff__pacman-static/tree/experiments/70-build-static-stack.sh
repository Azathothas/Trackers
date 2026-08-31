#!/bin/sh
# 70-build-static-stack.sh
#
# QUESTION: does pacman's whole static dependency stack -- the thirteen
# libraries the reference PKGBUILD links -- actually build with `zig cc` as
# the cross compiler, for an arbitrary target, on a host with no musl cross
# toolchain installed?
#
# 50-zig-cross-targets.sh established that zig cc links a static musl binary
# for all five targets. That is a 53-line fixture. This asks the question that
# decides the build plan: whether autotools, cmake and OpenSSL's Configure all
# survive the same compiler, which is where a cross-compiler substitution
# usually dies.
#
# ⭐ THE PER-PACKAGE RESULT IS THE POINT. A stack that fails at package nine
# is a far more useful answer than "it did not work", because the next session
# starts at nine.
#
# PINNED INPUTS -- versions are the reference PKGBUILD's, at its commit
#   8c58e7db1c52286bba77fd644ae1d77cc5db9e97 (aur/pacman-static, 2026-08-27)
#     zlib 1.3.2  xz 5.8.3  bzip2 1.0.8  zstd 1.5.7  brotli 1.2.0
#     openssl 3.6.4  nghttp2 1.70.0  curl 8.21.0  libarchive 3.8.9
#     libgpg-error 1.61  libassuan 3.0.0  gpgme 2.1.2  libseccomp 2.6.0
#   zig 0.16.0, sha256 70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
#
# USAGE
#   ./70-build-static-stack.sh [TRIPLE]      default x86_64-linux-musl
#
# EXIT CODES
#   0  every package built
#   1  the measurement ran and at least one package failed
#   2  the measurement could not run
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TRIPLE=${1:-x86_64-linux-musl}
WORK=${WORK:-/home/user/work}
SRC=${SRC:-$WORK/src}
ZIG=${ZIG:-$WORK/zig/zig}
JOBS=${JOBS:-$(nproc 2>/dev/null || echo 2)}

ARCH=${TRIPLE%%-*}
PATCHROOT=${PATCHROOT:-$(CDPATH= cd -- "$HERE/.." && pwd)/patches}
PREFIX=$WORK/out/$ARCH
BUILD=$WORK/bld/$ARCH
BIN=$WORK/bin/$ARCH
LOGS=$BUILD/logs
OUT="$HERE/out/70-build-static-stack.$ARCH.txt"

[ -x "$ZIG" ] || { echo "70: zig not at $ZIG (see 50-zig-cross-targets.sh)" >&2; exit 2; }
[ -d "$SRC" ] || { echo "70: sources not at $SRC (see 60-fetch-sources.sh)" >&2; exit 2; }

# ⚠ OPENSSL TARGET NAMES ARE NOT THE TRIPLE and are not uniform: three of the
# five carry a `64` in the middle and two do not. Read from openssl 3.6.4
# Configurations/10-main.conf. The reference PKGBUILD has no case for
# loongarch64 or ppc64le at all, and its riscv64 case is single-quoted
# ('linux64-$CARCH'), so $CARCH never expands -- see 30-reference-defects.sh.
case $ARCH in
  x86_64)      SSLTARGET=linux-x86_64;        SSLOPT='enable-ec_nistp_64_gcc_128' ;;
  aarch64)     SSLTARGET=linux-aarch64;       SSLOPT='no-afalgeng' ;;
  riscv64)     SSLTARGET=linux64-riscv64;     SSLOPT='' ;;
  loongarch64) SSLTARGET=linux64-loongarch64; SSLOPT='' ;;
  powerpc64le) SSLTARGET=linux-ppc64le;       SSLOPT='' ;;
  *) echo "70: no openssl target mapping for $ARCH" >&2; exit 2 ;;
esac

# cmake's own name for the processor, which is not the triple prefix either.
case $ARCH in
  x86_64)      CMAKE_PROC=x86_64 ;;
  aarch64)     CMAKE_PROC=aarch64 ;;
  riscv64)     CMAKE_PROC=riscv64 ;;
  loongarch64) CMAKE_PROC=loongarch64 ;;
  powerpc64le) CMAKE_PROC=ppc64le ;;
esac

rm -rf "$BUILD" "$BIN"
mkdir -p "$PREFIX/lib" "$PREFIX/include" "$BUILD" "$BIN" "$LOGS" "$HERE/out" || exit 2

# ---------------------------------------------------------------- wrappers --
# ⚠ REAL EXECUTABLES, NOT `CC="zig cc -target ..."`. A two-word $CC survives
# autoconf but breaks the moment a Makefile does `$(AR) rcs` or cmake writes
# the compiler into a response file. One file per tool, on PATH, is what every
# build system already expects.
cat > "$BIN/cc" <<EOF
#!/bin/sh
exec "$ZIG" cc -target $TRIPLE "\$@"
EOF
cat > "$BIN/c++" <<EOF
#!/bin/sh
exec "$ZIG" c++ -target $TRIPLE "\$@"
EOF
for t in ar ranlib; do
  cat > "$BIN/$t" <<EOF
#!/bin/sh
exec "$ZIG" $t "\$@"
EOF
done
chmod +x "$BIN"/*
for t in cc c++ ar ranlib; do ln -sf "$BIN/$t" "$BIN/$TRIPLE-$t"; done

PATH="$BIN:$PATH"; export PATH
CC="$BIN/cc";  export CC
CXX="$BIN/c++"; export CXX
AR="$BIN/ar"; export AR
RANLIB="$BIN/ranlib"; export RANLIB
# -D_LARGEFILE64_SOURCE turns on musl's func64 interface; libarchive and curl
# probe for the *64 symbols and silently lose large-file support without it.
# The reference PKGBUILD sets the same flag for the same reason.
CFLAGS="-Os -fno-stack-protector -D_LARGEFILE64_SOURCE"; export CFLAGS
CPPFLAGS="-I$PREFIX/include"; export CPPFLAGS
LDFLAGS="-L$PREFIX/lib"; export LDFLAGS
PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"; export PKG_CONFIG_PATH
PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig"; export PKG_CONFIG_LIBDIR
PKG_CONFIG_SYSROOT_DIR=""; export PKG_CONFIG_SYSROOT_DIR

cat > "$BUILD/cmake-toolchain.cmake" <<EOF
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR $CMAKE_PROC)
set(CMAKE_C_COMPILER $BIN/cc)
set(CMAKE_CXX_COMPILER $BIN/c++)
set(CMAKE_AR $BIN/ar)
set(CMAKE_RANLIB $BIN/ranlib)
set(CMAKE_FIND_ROOT_PATH $PREFIX)
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
EOF

results=""
rc=0
say() { printf '%s\n' "$*"; }

# ⛔ EVERY ARCHITECTURE GETS ITS OWN COPY OF EVERY SOURCE TREE.
#
# The first version of this script built all five targets in the shared
# $SRC/<pkg> directories. It looked fine: each run printed "whole stack built"
# and produced a full set of .a files in the right per-arch prefix. The
# aarch64 pacman link is where it surfaced --
#
#   ld.lld: error: /home/user/work/out/x86_64/lib/libzstd.a(zstd_lazy.o)
#           is incompatible with aarch64linux
#
# -- and the cause was in the aarch64 prefix's own pkg-config files:
#
#   out/aarch64/lib/pkgconfig/libcrypto.pc:2:prefix=/home/user/work/out/x86_64
#
# OpenSSL's Configure bakes the prefix into configdata.pm and the generated
# .pc files, and a second Configure in a tree that still holds the first
# build's artifacts does not regenerate all of them. autotools packages carry
# the same hazard through config.status and stale objects: aarch64's
# libcurl.pc picked up a stray -L.../x86_64/lib the same way.
#
# WARNING: THE FAILURE IS SILENT UNTIL THE FINAL LINK, and it produces a
# per-arch prefix that LOOKS complete. Anything that reuses a source tree
# across targets has to prove it cleaned it; copying is cheaper than proving.
srcdir() {  # package-directory-name -> a pristine private tree for this arch
  _d=$BUILD/src/$1
  if [ ! -d "$_d" ]; then
    mkdir -p "$BUILD/src"
    # ⛔ EXTRACT FROM THE TARBALL, NEVER COPY $SRC/<pkg>.
    # Copying was the second version of this function and it was still wrong.
    # $SRC/openssl-3.6.4 had been configured once for x86_64, so the copy
    # carried that build's configdata.pm and its generated .pc files, and
    # OpenSSL's Configure did not regenerate them. aarch64 and riscv64 both
    # ended up with `prefix=/home/user/work/out/x86_64` in their own
    # libcrypto.pc. Extracting is pristine by construction; copying is only
    # as clean as whatever last touched the directory.
    _tb=$(ls "$SRC/$1".tar.* 2>/dev/null | head -1)
    if [ -n "$_tb" ]; then
      _tmp=$BUILD/src/.x.$$
      rm -rf "$_tmp"; mkdir -p "$_tmp"
      tar xf "$_tb" -C "$_tmp" || { rm -rf "$_tmp"; return 1; }
      mv "$_tmp"/* "$_d" 2>/dev/null || { rm -rf "$_tmp"; return 1; }
      rm -rf "$_tmp"
    elif [ -d "$SRC/$1" ]; then
      # zstd has no release tarball this network can fetch, so it arrives as
      # a git checkout. Copy it, then hard-clean it back to the tag.
      cp -a "$SRC/$1" "$_d"
      if [ -d "$_d/.git" ]; then
        git -C "$_d" clean -xfdq 2>/dev/null
        git -C "$_d" checkout -- . 2>/dev/null
      fi
    else
      return 1
    fi
    # ⭐ PATCHES ARE PART OF THE SOURCE, so they are applied here, once, on a
    # tree that has never been configured. patches/<pkg>/*.patch, -p1.
    for _p in "$PATCHROOT/$1"/*.patch; do
      [ -e "$_p" ] || continue
      patch -d "$_d" -Np1 -i "$_p" >> "$LOGS/patches.log" 2>&1 || return 1
      echo "patched $1 with $(basename "$_p")" >> "$LOGS/patches.log"
    done
  fi
  printf '%s' "$_d"
}

run_pkg() {  # name  workdir
  _name=$1; _dir=$2
  _t0=$(date +%s)
  if ( cd "$_dir" && pkg_body ) > "$LOGS/$_name.log" 2>&1; then
    _st=ok
  else
    _st=FAIL; rc=1
  fi
  _t1=$(date +%s)
  results="$results$(printf '%-14s %-6s %5ss' "$_name" "$_st" "$((_t1-_t0))")
"
  say "  $_name: $_st ($((_t1-_t0))s)"
  [ "$_st" = ok ]
}

# ------------------------------------------------------------------ build ---
say "== building pacman's static dependency stack for $TRIPLE =="

# 1. zlib -- classic configure, no --host; CC from the environment is enough.
pkg_body() { ./configure --prefix="$PREFIX" --static && make -j"$JOBS" libz.a && make install; }
run_pkg zlib "$(srcdir zlib-1.3.2)"

# 2. xz/liblzma -- the 5.8.3 source archive carries CMakeLists but no
#    pre-generated configure, so cmake is the route that needs no autotools.
pkg_body() {
  rm -rf b && cmake -S . -B b -DCMAKE_TOOLCHAIN_FILE="$BUILD/cmake-toolchain.cmake" \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DBUILD_SHARED_LIBS=OFF -DXZ_NLS=OFF -DXZ_TOOL_XZ=OFF -DXZ_TOOL_XZDEC=OFF \
    -DXZ_TOOL_LZMADEC=OFF -DXZ_TOOL_LZMAINFO=OFF -DXZ_TOOL_SCRIPTS=OFF \
    -DCMAKE_INSTALL_LIBDIR=lib &&
  cmake --build b -j"$JOBS" && cmake --install b
}
run_pkg xz "$(srcdir xz-5.8.3)"

# 3. bzip2 -- hand-rolled Makefile, patched the way the reference does.
pkg_body() {
  make -j"$JOBS" libbz2.a CC="$CC" AR="$AR" RANLIB="$RANLIB" CFLAGS="$CFLAGS -D_FILE_OFFSET_BITS=64" &&
  install -Dm644 bzlib.h "$PREFIX/include/bzlib.h" &&
  install -Dm644 libbz2.a "$PREFIX/lib/libbz2.a"
}
run_pkg bzip2 "$(srcdir bzip2-1.0.8)"

# 4. zstd -- library only.
pkg_body() {
  make -C lib -j"$JOBS" libzstd.a CC="$CC" AR="$AR" RANLIB="$RANLIB" &&
  make -C lib PREFIX="$PREFIX" install-pc install-static install-includes
}
run_pkg zstd "$(srcdir zstd-git)"

# 5. brotli -- openssl 3.6 links it for certificate compression, so it has to
#    come before openssl. cmake.
pkg_body() {
  rm -rf b && cmake -S . -B b -DCMAKE_TOOLCHAIN_FILE="$BUILD/cmake-toolchain.cmake" \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
    -DBUILD_SHARED_LIBS=OFF -DCMAKE_INSTALL_LIBDIR=lib &&
  cmake --build b -j"$JOBS" && cmake --install b
}
run_pkg brotli "$(srcdir brotli-1.2.0)"

# 6. openssl
pkg_body() {
  ./Configure --prefix="$PREFIX" --openssldir=/etc/ssl --libdir=lib \
    --with-brotli-include="$PREFIX/include" --with-brotli-lib="$PREFIX/lib" \
    --with-zlib-include="$PREFIX/include" --with-zlib-lib="$PREFIX/lib" \
    --with-zstd-include="$PREFIX/include" --with-zstd-lib="$PREFIX/lib" \
    no-shared no-ssl3-method no-tests no-docs \
    enable-brotli enable-zlib enable-zstd $SSLOPT "$SSLTARGET" \
    "$CFLAGS" &&
  make -j"$JOBS" build_libs && make install_dev
}
run_pkg openssl "$(srcdir openssl-3.6.4)"

# 7. libarchive
pkg_body() {
  ./configure --host="$TRIPLE" --prefix="$PREFIX" --disable-shared --enable-static \
    --without-xml2 --without-nettle --without-expat --without-iconv \
    --disable-bsdtar --disable-bsdcat --disable-bsdcpio --disable-bsdunzip &&
  make -j"$JOBS" && make install-includeHEADERS install-libLTLIBRARIES install-pkgconfigDATA
}
run_pkg libarchive "$(srcdir libarchive-3.8.9)"

# 8. nghttp2 -- library only, so no C++ and no examples.
pkg_body() {
  ./configure --host="$TRIPLE" --prefix="$PREFIX" --disable-shared --enable-static \
    --enable-lib-only --disable-examples --disable-python-bindings &&
  make -C lib -j"$JOBS" && make -C lib install
}
run_pkg nghttp2 "$(srcdir nghttp2-1.70.0)"

# 9. curl
# ⛔ THE ONE PACKAGE THAT DOES NOT BUILD FROM ITS STOCK CONFIGURE.
# OpenSSL 3.6 built with enable-brotli puts c_brotli.o in libcrypto.a, and
# that object references the Brotli ENCODER (BrotliEncoderCreateInstance and
# friends). curl's OpenSSL probe assembles its link line from libbrotlidec
# and libbrotlicommon only, so the conftest fails to link and configure
# reports "--with-openssl was given but OpenSSL could not be detected" --
# naming the wrong library, which is why this costs an hour to diagnose.
# Upstream curl issue 17678. The reference PKGBUILD patches configure.ac to
# add a libbrotlienc probe, which then needs `autoreconf -if` and therefore
# autotools on the build host. Measured here: exporting LIBS=-lbrotlienc
# fixes the same conftest with no patch and no autoreconf.
pkg_body() {
  LIBS="-lbrotlienc"; export LIBS
  ./configure --host="$TRIPLE" --prefix="$PREFIX" --disable-shared --enable-static \
    --with-openssl --with-brotli --with-zstd --with-nghttp2 \
    --with-ca-bundle=/etc/ssl/certs/ca-certificates.crt \
    --enable-ipv6 --enable-threaded-resolver \
    --disable-dict --disable-gopher --disable-imap --disable-ldap --disable-ldaps \
    --disable-manual --disable-pop3 --disable-rtsp --disable-smb --disable-smtp \
    --disable-telnet --disable-tftp --disable-libcurl-option \
    --without-libidn2 --without-librtmp --without-libssh2 --without-libpsl \
    --without-gssapi --without-nghttp3 --without-ngtcp2 &&
  make -C lib -j"$JOBS" && make -C lib install && make -C include install &&
  make install-pkgconfigDATA
  _r=$?; unset LIBS; return $_r
}
run_pkg curl "$(srcdir curl-8.21.0)"

# 10. libgpg-error
pkg_body() {
  ./configure --host="$TRIPLE" --prefix="$PREFIX" --disable-shared --enable-static \
    --disable-doc --disable-tests &&
  make -C src -j"$JOBS" &&
  make -C src install-binSCRIPTS install-libLTLIBRARIES install-nodist_includeHEADERS install-pkgconfigDATA
}
run_pkg libgpg-error "$(srcdir libgpg-error-1.61)"

# 11. libassuan
pkg_body() {
  ./configure --host="$TRIPLE" --prefix="$PREFIX" --disable-shared --enable-static \
    --with-libgpg-error-prefix="$PREFIX" --disable-doc &&
  make -C src -j"$JOBS" &&
  make -C src install-binSCRIPTS install-libLTLIBRARIES install-nodist_includeHEADERS install-pkgconfigDATA
}
run_pkg libassuan "$(srcdir libassuan-3.0.0)"

# 12. gpgme
pkg_body() {
  ./configure --host="$TRIPLE" --prefix="$PREFIX" --disable-shared --enable-static \
    --disable-fd-passing --disable-languages --disable-gpgsm-test --disable-gpg-test \
    --with-libgpg-error-prefix="$PREFIX" --with-libassuan-prefix="$PREFIX" &&
  make -C src -j"$JOBS" &&
  make -C src install-binSCRIPTS install-libLTLIBRARIES install-nodist_includeHEADERS install-pkgconfigDATA
}
run_pkg gpgme "$(srcdir gpgme-2.1.2)"

# 13. libseccomp -- pacman treats it as optional, so a failure here costs the
#     download sandbox and nothing else.
pkg_body() {
  ./configure --host="$TRIPLE" --prefix="$PREFIX" --disable-shared --enable-static --disable-python &&
  make -j"$JOBS" && make install
}
run_pkg libseccomp "$(srcdir libseccomp-2.6.0)"

# ----------------------------------------------------------------- report ---
{
  echo "# 70-build-static-stack"
  echo "date        : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host        : $(uname -srm), $(nproc 2>/dev/null) cores"
  echo "host distro : $(. /etc/os-release 2>/dev/null && echo "$PRETTY_NAME")"
  echo "compiler    : zig $($ZIG version) cc -target $TRIPLE"
  echo "target      : $TRIPLE"
  echo "openssl tgt : $SSLTARGET $SSLOPT"
  echo "prefix      : $PREFIX"
  echo "jobs        : $JOBS"
  echo
  echo "patches applied:"
  if [ -s "$LOGS/patches.log" ]; then
    grep '^patched ' "$LOGS/patches.log" | sed 's/^/  /' || echo "  (none)"
  else
    echo "  (none)"
  fi
  echo
  printf '%-14s %-6s %6s\n' PACKAGE STATUS TIME
  printf '%s' "$results"
  echo
  echo "static libs installed:"
  ls -1 "$PREFIX/lib"/*.a 2>/dev/null | sed "s|$PREFIX/lib/|  |" || echo "  (none)"
  echo
  echo "pkg-config files:"
  ls -1 "$PREFIX/lib/pkgconfig" 2>/dev/null | sed 's|^|  |' || echo "  (none)"
  echo
  echo "cross-prefix leak check:"
  _leak=$(cat "$PREFIX/lib/pkgconfig"/*.pc 2>/dev/null \
          | grep -o "$WORK/out/[a-z0-9_]*" 2>/dev/null \
          | sort -u | grep -v "^$WORK/out/$ARCH$" || true)
  if [ -n "$_leak" ]; then
    echo "  FOREIGN PREFIX IN $ARCH pkgconfig:"; printf '    %s\n' $_leak
    rc=1
  else
    echo "  none: every absolute path in this prefix points at $PREFIX"
  fi
  echo
  echo "verdict: $([ $rc -eq 0 ] && echo 'whole stack built' || echo 'at least one package failed; see logs')"
  echo "logs   : $LOGS"
} > "$OUT"

cat "$OUT"
exit $rc
