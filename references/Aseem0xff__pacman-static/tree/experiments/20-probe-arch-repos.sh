#!/bin/sh
# 20-probe-arch-repos.sh
#
# QUESTION: for each of the five required architectures, is there a live
# Arch-family package repository a static pacman could actually bootstrap
# from -- and what are its repo names, its architecture directory, and its
# keyring?
#
# ⭐ THIS IS THE QUESTION THAT DECIDES WHETHER THE GOAL IS REACHABLE. Building
# a pacman binary for loongarch64 is worth nothing if no loongarch64 Arch
# repository exists to install packages from. The five distributions are five
# separate projects with five different layouts, three different repo-name
# sets, and five different signing keyrings; `pacman-key --populate archlinux`
# is correct on exactly one of them.
#
# ⚠ THREE LAYOUT TRAPS ARE WHAT THIS SCRIPT EXISTS TO RECORD:
#   - LoongArch Linux's architecture directory is `loong64`, not
#     `loongarch64`, so the obvious $arch substitution 404s.
#   - ArchPOWER's repository is named `base`, not `core`, and its path is
#     $repo/$arch with no `os` component.
#   - Arch Linux RISC-V is flat: repo/$repo/, with no os/$arch at all.
#
# ORACLE: a repository is "live" when its database file answers 200 AND
# parses as a real pacman database (a zstd/gzip tar carrying %FILENAME%
# entries), not when its index page answers 200. A mirror serving an HTML
# error page with a 200 is the failure this distinction catches.
#
# EXIT CODES
#   0  every architecture has a live, parsing repository
#   1  at least one does not
#   2  the measurement could not run
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
OUT="$HERE/out/20-probe-arch-repos.txt"
TMP=$(mktemp -d) || exit 2
trap 'rm -rf "$TMP"' EXIT
command -v curl >/dev/null 2>&1 || { echo "20: curl not found" >&2; exit 2; }
mkdir -p "$HERE/out" || exit 2

# arch | distribution | db url | keyring package name | pacman-key --populate arg
ROWS="
x86_64|Arch Linux|https://geo.mirror.pkgbuild.com/core/os/x86_64/core.db|archlinux-keyring|archlinux
aarch64|Arch Linux ARM|http://mirror.archlinuxarm.org/aarch64/core/core.db|archlinuxarm-keyring|archlinuxarm
riscv64|Arch Linux RISC-V|https://archriscv.felixc.at/repo/core/core.db|archlinux-keyring|archlinux
loongarch64|LoongArch Linux|https://mirrors.wsyu.edu.cn/loongarch/archlinux/core/os/loong64/core.db|archlinux-keyring|archlinux
powerpc64le|ArchPOWER|https://repo.archlinuxpower.org/base/powerpc64le/base.db|archpower-keyring|archpower
"

rc=0
{
  echo "# 20-probe-arch-repos"
  echo "date : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host : $(uname -srm)"
  echo "curl : $(curl --version | head -1)"
  echo
  printf '%-12s %-19s %-5s %-9s %-7s %s\n' ARCH DISTRIBUTION HTTP BYTES PARSES PKGS
} > "$OUT"

printf '%s\n' "$ROWS" | while IFS='|' read -r arch dist url keyring populate; do
  [ -n "${arch:-}" ] || continue
  f="$TMP/$arch.db"
  code=$(curl -sSL -o "$f" -w '%{http_code}' --max-time 60 "$url" 2>/dev/null || echo 000)
  bytes=$(wc -c < "$f" 2>/dev/null | tr -d ' '); [ -n "$bytes" ] || bytes=0
  parses=no; pkgs=0
  if [ "$code" = 200 ] && [ "$bytes" -gt 0 ]; then
    # A pacman db is a compressed tar of <pkg>-<ver>/desc entries. Listing it
    # is the check; an HTML error page served with 200 fails here, which is
    # the whole reason this is not a HEAD request.
    if tar tf "$f" > "$TMP/$arch.list" 2>/dev/null; then
      pkgs=$(grep -c '/desc$' "$TMP/$arch.list" 2>/dev/null || echo 0)
      [ "$pkgs" -gt 0 ] && parses=yes
    fi
  fi
  [ "$parses" = yes ] || echo 1 >> "$TMP/failed"
  printf '%-12s %-19s %-5s %-9s %-7s %s\n' "$arch" "$dist" "$code" "$bytes" "$parses" "$pkgs" >> "$OUT"
done

[ -f "$TMP/failed" ] && rc=1

{
  echo
  echo "## repository coordinates, for pacman.conf"
  echo
  printf '%-12s %-19s %-8s %-13s %s\n' ARCH DISTRIBUTION REPOS KEYRING 'Server ='
  printf '%-12s %-19s %-8s %-13s %s\n' x86_64 'Arch Linux' 'core,extra' archlinux-keyring \
    'https://geo.mirror.pkgbuild.com/$repo/os/$arch'
  printf '%-12s %-19s %-8s %-13s %s\n' aarch64 'Arch Linux ARM' 'core,extra' archlinuxarm-keyring \
    'http://mirror.archlinuxarm.org/$arch/$repo'
  printf '%-12s %-19s %-8s %-13s %s\n' riscv64 'Arch Linux RISC-V' 'core,extra' archlinux-keyring \
    'https://archriscv.felixc.at/repo/$repo'
  printf '%-12s %-19s %-8s %-13s %s\n' loongarch64 'LoongArch Linux' 'core,extra' archlinux-keyring \
    'https://mirrors.wsyu.edu.cn/loongarch/archlinux/$repo/os/loong64'
  printf '%-12s %-19s %-8s %-13s %s\n' powerpc64le 'ArchPOWER' 'base,extra' archpower-keyring \
    'https://repo.archlinuxpower.org/$repo/powerpc64le'
  echo
  echo "⚠ \$arch does NOT substitute correctly for loongarch64 (dir is loong64)"
  echo "⚠ ArchPOWER's core-equivalent repo is named 'base'"
  echo "⚠ Arch Linux RISC-V has no os/\$arch path component"
  echo
  echo "verdict: $([ $rc -eq 0 ] && echo 'all five architectures have a live repository' \
                                 || echo 'at least one architecture has no live repository')"
} >> "$OUT"

cat "$OUT"
exit $rc
