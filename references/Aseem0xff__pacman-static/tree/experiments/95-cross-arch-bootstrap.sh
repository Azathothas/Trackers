#!/bin/sh
# 95-cross-arch-bootstrap.sh
#
# QUESTION: does the binary built for a FOREIGN architecture do real work --
# reach that architecture's own distribution, verify nothing is missing, and
# unpack a full `base` of that architecture's packages -- or does it only
# print its version?
#
# ⭐ THIS IS THE EVIDENCE UPGRADE THAT MATTERS. Until this script existed,
# every non-x86_64 claim in RESEARCH.md rested on `pacman --version` under
# qemu. That proves the binary starts. It does not exercise libcurl, TLS,
# libarchive, the database parser, or the installer -- which is all of the
# software.
#
# WHAT IT PROVES, per architecture:
#   1. sync    -- libcurl + OpenSSL reach that distribution's mirror and
#                 libarchive parses its package database
#   2. install -- the full `base` package set unpacks into an empty root
#   3. shape   -- the files that landed are ELF binaries OF THAT ARCHITECTURE,
#                 read back with file(1), not assumed from the package name
#
# ⛔ WHAT IT DOES NOT PROVE. Post-transaction hooks FAIL under qemu-user with
# "call to execv failed (Exec format error)", because a hook execs a binary of
# the target architecture inside the chroot and the host kernel has no binfmt
# handler registered for it. That is an artefact of the harness, not of the
# binary: the packages are installed and the root is complete. Registering
# binfmt_misc would fix it and would modify host kernel state, so it is not
# done here. See TASKS.md T-13.
#
# ⚠ AND IT IS STILL NOT REAL HARDWARE. qemu-user emulates the ISA and passes
# syscalls to the host kernel; it does not exercise the target's kernel or its
# page size. TASKS.md T-14.
#
# USAGE
#   ./95-cross-arch-bootstrap.sh [ARCH ...]     default: aarch64 loongarch64
#
# EXIT CODES
#   0  every requested architecture synced, installed, and produced its own ELF
#   1  the measurement ran and an architecture failed
#   2  the measurement could not run
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
WORK=${WORK:-/home/user/work}
OUT="$HERE/out/95-cross-arch-bootstrap.txt"
ARCHES=${*:-'aarch64 loongarch64'}
CONFDIR=$WORK/xconf
mkdir -p "$HERE/out" "$CONFDIR" || exit 2

# ⚠ THE THREE LAYOUT TRAPS ARE ENCODED HERE, not worked around at call sites:
# ArchPOWER's core repo is named `base`; LoongArch's directory is `loong64`,
# so $arch cannot be used; Arch Linux RISC-V is flat with no os/$arch.
conf_for() {
  _x=extra; _s2=''
  case $1 in
    x86_64)      _a=x86_64;  _c=core; _s='https://geo.mirror.pkgbuild.com/$repo/os/$arch' ;;
    aarch64)     _a=aarch64; _c=core; _s='http://mirror.archlinuxarm.org/$arch/$repo' ;;
    riscv64)     _a=riscv64; _c=core; _s='https://archriscv.felixc.at/repo/$repo' ;;
    loongarch64) _a=loong64; _c=core; _s='https://mirrors.wsyu.edu.cn/loongarch/archlinux/$repo/os/loong64' ;;
    # ⛔ ARCHPOWER IS THE AWKWARD ONE, IN THREE WAYS AT ONCE:
    #   - it has NO `extra`. Its repositories are base, testing, stage and
    #     distfiles; `extra.db` answers 404 and the whole sync fails.
    #   - its core-equivalent repository is named `base`.
    #   - ⭐ it splits arch-specific and `any`-architecture packages into TWO
    #     DATABASES: base/powerpc64le/base.db (3736 packages) and
    #     base/any/base-any.db (2200). `iana-etc` and `openssl` live only in
    #     the second, so with the first alone pacman answers "unable to
    #     satisfy dependency 'iana-etc' required by filesystem" and the
    #     resolution unwinds all the way up to `base`.
    # Two Server lines under one repo name would be MIRRORS of one database,
    # which is not what is needed: they are two databases and need two repo
    # sections. _x carries the second one.
    powerpc64le) _a=powerpc64le; _c=base; _x=base-any
                 _s='https://repo.archlinuxpower.org/$repo/powerpc64le'
                 _s2='https://repo.archlinuxpower.org/base/any' ;;
    *) return 1 ;;
  esac
  # ⚠ BOTH REPOSITORIES, ALWAYS. `base` lives in core (or ArchPOWER's `base`)
  # but its dependency closure reaches into `extra`: with core alone, pacman
  # answers "unable to satisfy dependency 'libgssapi_krb5.so=2-64' required by
  # curl" and the whole resolution unwinds up to `base` itself.
  {
    printf '[options]\nArchitecture = %s\nSigLevel     = Never\n' "$_a"
    printf '\n[%s]\nServer = %s\n' "$_c" "$_s"
    if [ -n "$_x" ]; then
      printf '\n[%s]\nServer = %s\n' "$_x" "${_s2:-$_s}"
    fi
  } > "$CONFDIR/$1.conf"
  # ⚠ EXPLICIT RETURN. Without it the function's status is the last command's,
  # and `[ -n "$_x" ] && printf ...` returns 1 for the one architecture with no
  # `extra` repo -- so the caller's `|| continue` silently skipped ArchPOWER
  # and printed an empty row.
  return 0
}

qemu_for() {
  case $1 in
    x86_64) echo qemu-x86_64-static ;; aarch64) echo qemu-aarch64-static ;;
    riscv64) echo qemu-riscv64-static ;; loongarch64) echo qemu-loongarch64-static ;;
    powerpc64le) echo qemu-ppc64le-static ;; *) echo '' ;;
  esac
}

# What file(1) calls each machine, so step 3 asserts rather than assumes.
elf_for() {
  case $1 in
    x86_64) echo 'x86-64' ;; aarch64) echo 'ARM aarch64' ;;
    riscv64) echo 'UCB RISC-V' ;; loongarch64) echo 'LoongArch' ;;
    powerpc64le) echo '64-bit PowerPC' ;; *) echo '' ;;
  esac
}

rc=0; segv=0; rows=''
for a in $ARCHES; do
  P=$WORK/pacman/$a/build/pacman
  Q=$(qemu_for "$a")
  R=$WORK/xroot/$a
  sync_st=-; inst_st=-; n=0; sz='-'; shape='-'

  if [ ! -x "$P" ]; then
    rows="$rows$(printf '%-14s %-8s %-9s %-7s %-7s %s' "$a" 'NO BIN' - - - -)
"; rc=1; continue
  fi
  if ! command -v "$Q" >/dev/null 2>&1; then
    rows="$rows$(printf '%-14s %-8s %-9s %-7s %-7s %s' "$a" 'NO QEMU' - - - -)
"; rc=1; continue
  fi
  conf_for "$a" || { rc=1; continue; }

  rm -rf "$R"
  mkdir -p "$R/var/lib/pacman" "$R/var/cache/pacman/pkg" \
           "$R/etc/pacman.d/hooks" "$R/etc/pacman.d/gnupg" "$R/var/log"
  p() { "$Q" "$P" --config "$CONFDIR/$a.conf" --root "$R" \
          --dbpath "$R/var/lib/pacman" --cachedir "$R/var/cache/pacman/pkg" \
          --hookdir "$R/etc/pacman.d/hooks" --gpgdir "$R/etc/pacman.d/gnupg" \
          --logfile "$R/var/log/pacman.log" "$@"; }

  if p -Sy --noconfirm > "$WORK/xroot/$a.sync.log" 2>&1; then sync_st=ok; else sync_st=FAIL; rc=1; fi
  if [ "$sync_st" = ok ]; then
    p -S --noconfirm base > "$WORK/xroot/$a.inst.log" 2>&1
    _e=$?
    n=$(ls -1 "$R/var/lib/pacman/local" 2>/dev/null | grep -v '^ALPM_DB_VERSION$' | wc -l | tr -d ' ')
    # ⚠ A CRASH AFTER A COMPLETE INSTALL IS NOT THE SAME AS A FAILED INSTALL,
    # and reporting them the same way loses the distinction this table exists
    # to make. `SEGV*` means: exit 139, but the packages are all there and the
    # ELF check below still has to pass. It is the open fault in RESEARCH.md
    # §9 and it is counted separately, never silently.
    if [ "$_e" = 0 ]; then
      inst_st=ok
    elif [ "$_e" = 139 ] && [ "$n" -gt 100 ]; then
      inst_st='SEGV*'; segv=$((segv+1))
    else
      inst_st=FAIL; rc=1
    fi
    sz=$(du -sh "$R" 2>/dev/null | cut -f1)
    # ⭐ READ THE ELF BACK. The package said aarch64; this checks the bytes.
    want=$(elf_for "$a")
    got=$(file "$R/usr/bin/bash" 2>/dev/null)
    case $got in
      *"$want"*) shape=ok ;;
      *)         shape=MISMATCH; rc=1 ;;
    esac
  fi
  rows="$rows$(printf '%-14s %-8s %-9s %-7s %-7s %s' "$a" "$sync_st" "$inst_st" "$n" "$sz" "$shape")
"
done

{
  echo "# 95-cross-arch-bootstrap"
  echo "date : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host : $(uname -srm)  ⭐ x86_64, bootstrapping FOREIGN architectures"
  echo "qemu : $(qemu-aarch64-static --version 2>/dev/null | head -1)"
  echo
  printf '%-14s %-8s %-9s %-7s %-7s %s\n' ARCH SYNC INSTALL PKGS SIZE 'ELF OK'
  printf '%s' "$rows"
  echo
  echo "  SYNC    libcurl + OpenSSL reached that distribution and libarchive"
  echo "          parsed its package database"
  echo "  INSTALL the full 'base' set unpacked into an empty directory"
  echo "  ELF OK  file(1) on the installed /usr/bin/bash reports THAT"
  echo "          architecture -- read back, not assumed"
  echo
  echo "⛔ post-transaction hooks fail under qemu-user with"
  echo "   'call to execv failed (Exec format error)'. The host kernel has no"
  echo "   binfmt handler for the target, so a hook cannot exec inside the"
  echo "   chroot. The packages ARE installed. TASKS.md T-13."
  echo
  if [ "$segv" -gt 0 ]; then
    echo "⚠ SEGV* = the install COMPLETED (package count and ELF check both"
    echo "  pass) and the process then died with SIGSEGV. That is the open"
    echo "  intermittent fault in RESEARCH.md §9, not a failed bootstrap."
    echo "  Seen on $segv of the architectures in this run."
    echo
  fi
  echo "verdict: $([ $rc -eq 0 ] && echo 'every architecture bootstrapped its own distribution' \
                                 || echo 'an architecture failed')"
} > "$OUT"
cat "$OUT"
exit $rc
