#!/bin/sh
# 10-probe-source-hosts.sh
#
# QUESTION: which of the source hosts the two references depend on actually
# answer from this network, and which silently do not?
#
# ⚠ WHY IT MATTERS ENOUGH TO BE A SCRIPT. A build plan that assumes
# ftp.gnu.org is a build plan that fails on the first run in an environment
# that cannot reach it, after the operator has already paid for the setup.
# mussel's own tracker carries this as OPEN issue 57, with the maintainer's
# answer being that ftpmirror.gnu.org redirects to "faulty but nearby"
# mirrors and that users should substitute reachable mirrors themselves.
#
# ⛔ A NON-200 HERE IS A PROPERTY OF THIS NETWORK, NOT OF THE HOST. Every row
# says which check it ran. Re-run it on the machine you intend to build on;
# the point is that the answer differs per network and must be measured
# rather than assumed.
#
# ⚠ HEAD IS NOT A CONTROL FOR GET. musl.libc.org answers 000 to a HEAD and
# serves the tarball on a GET from this same host, so a HEAD-only probe
# reports a working mirror as dead. Every row below is a ranged GET.
#
# EXIT CODES
#   0  every host required by the recommended plan answered
#   1  at least one required host did not
#   2  the measurement could not run
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
OUT="$HERE/out/10-probe-source-hosts.txt"
command -v curl >/dev/null 2>&1 || { echo "10: curl not found" >&2; exit 2; }
mkdir -p "$HERE/out" || exit 2

# role | what needs it | url
ROWS="
required|zlib, xz, brotli, nghttp2, libseccomp (mirror)|https://distfiles.alpinelinux.org/distfiles/edge/zlib-1.3.2.tar.gz
required|openssl|https://www.openssl.org/source/openssl-3.6.4.tar.gz
required|curl|https://curl.se/download/curl-8.21.0.tar.xz
required|libarchive|https://www.libarchive.org/downloads/libarchive-3.8.9.tar.xz
required|libgpg-error, libassuan, gpgme|https://gnupg.org/ftp/gcrypt/gpgme/gpgme-2.1.2.tar.bz2
required|bzip2|https://sourceware.org/pub/bzip2/bzip2-1.0.8.tar.gz
required|zig toolchain|https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz
required|pacman source (git)|https://gitlab.archlinux.org/pacman/pacman.git/info/refs?service=git-upload-pack
required|the reference PKGBUILD (git)|https://aur.archlinux.org/pacman-static.git/info/refs?service=git-upload-pack
mussel|binutils, gcc, gmp, mpc, mpfr via GNU redirector|https://ftpmirror.gnu.org/binutils/binutils-2.46.1.tar.xz
mussel|binutils (direct)|https://sourceware.org/pub/binutils/releases/binutils-2.46.1.tar.xz
mussel|gcc (direct)|https://sourceware.org/pub/gcc/releases/gcc-16.1.0/gcc-16.1.0.tar.xz
mussel|musl|https://musl.libc.org/releases/musl-1.2.6.tar.gz
mussel|linux headers|https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.19.14.tar.xz
mussel|isl (only with --enable-isl)|https://libisl.sourceforge.io/isl-0.27.tar.xz
pkgbuild|zstd, brotli, xz, libseccomp release assets|https://github.com/facebook/zstd/releases/download/v1.5.7/zstd-1.5.7.tar.zst
pkgbuild|zlib upstream|https://zlib.net/zlib-1.3.1.tar.gz
"

rc=0
{
  echo "# 10-probe-source-hosts"
  echo "date  : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host  : $(uname -srm)"
  echo "curl  : $(curl --version | head -1 | cut -d' ' -f1-2)"
  echo "check : ranged GET of the first 1024 bytes, 30s timeout, follows redirects"
  echo
  printf '%-9s %-5s %-7s %-5s %s\n' ROLE HTTP BYTES EXIT URL
} > "$OUT"

printf '%s\n' "$ROWS" | while IFS='|' read -r role what url; do
  [ -n "${role:-}" ] || continue
  # ⚠ RANGED GET, NOT HEAD. See the header: a host that refuses HEAD and
  # serves GET is a working mirror that a HEAD probe would condemn.
  #
  # ⛔ ONE CURL CALL, AND NO `|| echo`. The first version of this script ran
  # curl twice and appended a fallback code on failure, so when curl exited
  # non-zero AFTER writing its own code the two concatenated: musl.libc.org
  # was reported as `000000` while its bytes column showed a completed 1024
  # byte transfer. A probe whose failure path corrupts its own output is
  # worse than no probe. Read both fields from one invocation and keep
  # curl's exit code as a separate column.
  read -r code bytes cexit <<EOF
$(curl -sSL -r 0-1023 -o /dev/null -w '%{http_code} %{size_download}' --max-time 30 "$url" 2>/dev/null; printf ' %s' "$?")
EOF
  [ -n "${code:-}" ] || { code=000; bytes=0; cexit=-; }
  printf '%-9s %-5s %-7s %-5s %s\n' "$role" "$code" "$bytes" "$cexit" "$url" >> "$OUT"
  case $code in 200|206) ;; *) [ "$role" = required ] && echo 1 >> "$OUT.fail" ;; esac
done

[ -f "$OUT.fail" ] && { rc=1; rm -f "$OUT.fail"; }
{
  echo
  echo "  role 'required' = needed by the plan in TASKS.md"
  echo "  role 'mussel'   = needed only if you build GCC cross toolchains"
  echo "  role 'pkgbuild' = the reference PKGBUILD's own source URLs"
  echo "  EXIT            = curl's exit code; 0 with a 2xx is a clean fetch,"
  echo "                    a non-zero EXIT beside a 2xx means the transfer"
  echo "                    was cut short (expected on a ranged GET some"
  echo "                    servers answer without honouring the range)"
  echo
  echo "verdict: $([ $rc -eq 0 ] && echo 'every required host answered' || echo 'a required host did not answer')"
} >> "$OUT"
cat "$OUT"
exit $rc
