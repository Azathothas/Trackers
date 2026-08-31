#!/bin/sh
# 60-fetch-sources.sh
#
# QUESTION: which upstream host actually serves each of pacman-static's
# thirteen dependencies from THIS network, and what is the sha256 of what it
# served?
#
# ⚠ THIS IS NOT A CONVENIENCE WRAPPER. Two of the reference PKGBUILD's source
# hosts do not answer here at all, and mussel's own default mirror
# (ftpmirror.gnu.org) is the subject of an OPEN upstream issue about exactly
# this -- firasuke/mussel#57, where the maintainer's own answer is
# "modify the script to use mirrors that are available on your end".
# 10-probe-source-hosts.sh measures the reachability; this one does the fetch
# and records what arrived.
#
# Versions are the reference PKGBUILD's at commit
# 8c58e7db1c52286bba77fd644ae1d77cc5db9e97 (aur/pacman-static, 2026-08-27).
#
# ⛔ THE SHA256 LINES BELOW ARE OBSERVED, NOT UPSTREAM-PUBLISHED. They record
# what this host received on the date in the output. They are a change
# detector, not a provenance claim: the reference PKGBUILD carries upstream
# sha512sums and PGP signatures, and a real builder must verify THOSE.
# See TASKS.md T-07.
#
# EXIT CODES
#   0  every source present and hashed
#   1  at least one source could not be fetched
#   2  the measurement could not run
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
WORK=${WORK:-/home/user/work}
SRC=${SRC:-$WORK/src}
OUT="$HERE/out/60-fetch-sources.txt"
A=https://distfiles.alpinelinux.org/distfiles/edge

command -v curl >/dev/null 2>&1 || { echo "60: curl not found" >&2; exit 2; }
mkdir -p "$SRC" "$HERE/out" || exit 2

# name|url
URLS="
zlib|https://distfiles.alpinelinux.org/distfiles/edge/zlib-1.3.2.tar.gz
xz|https://distfiles.alpinelinux.org/distfiles/edge/xz-5.8.3.tar.gz
bzip2|https://sourceware.org/pub/bzip2/bzip2-1.0.8.tar.gz
brotli|https://distfiles.alpinelinux.org/distfiles/edge/brotli-1.2.0.tar.gz
openssl|https://www.openssl.org/source/openssl-3.6.4.tar.gz
nghttp2|https://distfiles.alpinelinux.org/distfiles/edge/nghttp2-1.70.0.tar.xz
curl|https://curl.se/download/curl-8.21.0.tar.xz
libarchive|https://www.libarchive.org/downloads/libarchive-3.8.9.tar.xz
libgpg-error|https://gnupg.org/ftp/gcrypt/libgpg-error/libgpg-error-1.61.tar.bz2
libassuan|https://gnupg.org/ftp/gcrypt/libassuan/libassuan-3.0.0.tar.bz2
gpgme|https://gnupg.org/ftp/gcrypt/gpgme/gpgme-2.1.2.tar.bz2
libseccomp|https://distfiles.alpinelinux.org/distfiles/edge/libseccomp-2.6.0.tar.gz
"
# ⚠ zstd ships its release tarball only as a GitHub release asset, which this
# network answers 403 for over https while serving the same repo over the git
# protocol. It is fetched by tag instead, and the tag is pinned.
ZSTD_TAG=v1.5.7

rc=0
{
  echo "# 60-fetch-sources"
  echo "date : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host : $(uname -srm)"
  echo "dest : $SRC"
  echo
  printf '%-14s %-8s %-10s %s\n' PACKAGE STATUS BYTES SHA256
} > "$OUT"

printf '%s\n' "$URLS" | while IFS='|' read -r name url; do
  [ -n "${name:-}" ] || continue
  f="$SRC/${url##*/}"
  st=cached
  if [ ! -s "$f" ]; then
    if curl -fsSL --retry 5 --retry-all-errors --retry-delay 3 --max-time 900 \
         -o "$f.part" "$url"; then
      mv "$f.part" "$f"; st=fetched
    else
      rm -f "$f.part"; st=FAIL
    fi
  fi
  if [ "$st" = FAIL ]; then
    printf '%-14s %-8s %-10s %s\n' "$name" FAIL - "$url" >> "$OUT"
    echo 1 > "$SRC/.60.failed"
  else
    printf '%-14s %-8s %-10s %s\n' "$name" "$st" "$(wc -c < "$f" | tr -d ' ')" \
      "$(sha256sum "$f" | cut -c1-64)" >> "$OUT"
  fi
done

if [ ! -d "$SRC/zstd-git/.git" ]; then
  if git clone --quiet --depth 1 --branch "$ZSTD_TAG" \
       https://github.com/facebook/zstd "$SRC/zstd-git" 2>/dev/null; then
    zst=cloned
  else
    zst=FAIL; echo 1 > "$SRC/.60.failed"
  fi
else
  zst=cached
fi
printf '%-14s %-8s %-10s %s\n' zstd "$zst" - \
  "git $ZSTD_TAG $(git -C "$SRC/zstd-git" rev-parse HEAD 2>/dev/null || echo -)" >> "$OUT"

[ -f "$SRC/.60.failed" ] && { rc=1; rm -f "$SRC/.60.failed"; }
{
  echo
  echo "verdict: $([ $rc -eq 0 ] && echo 'every source present' || echo 'at least one source failed')"
} >> "$OUT"
cat "$OUT"
exit $rc
