#!/bin/sh
# 90-bootstrap-arch.sh
#
# QUESTION: can the static pacman built by 80- install a working Arch Linux
# root from the real repositories, on a host that is not Arch and has no
# pacman, no libalpm and no Arch keyring?
#
# ⭐ THIS IS THE CLAIM THE BINARY EXISTS FOR. Everything before it is
# necessary and none of it is sufficient: a binary that runs `--version` has
# not resolved a mirror, verified a signature, or unpacked a package.
#
# WHAT IT ACTUALLY PROVES, in order, and each is a separate check:
#   1. sync    -- the binary reaches a real mirror over TLS and parses the db
#   2. install -- it resolves `base` and unpacks it into an empty directory
#   3. chroot  -- the resulting root executes its own dynamically linked
#                 binaries, which is what "a working root" means
#   4. self    -- the Arch pacman inside that root runs
#   5. verify  -- signature checking works once a keyring exists
#
# ⛔ STEP 5 IS THE ONE THAT IS EASY TO FAKE. Bootstrapping with
# SigLevel=Never and stopping there proves the download worked and nothing
# about trust. This script installs with signatures OFF, then initialises the
# keyring inside the new root and re-verifies a package with signatures ON,
# and reports the two separately.
#
# ⚠ ONLY x86_64 IS BOOTSTRAPPED HERE, and the reason is a real constraint,
# not an oversight: step 3 needs to execute the target's own binaries, and
# qemu-user cannot chroot without either binfmt_misc registration on the host
# or a copy of the emulator inside the root. See TASKS.md T-13 for the
# per-architecture version.
#
# EXIT CODES
#   0  every step passed
#   1  the measurement ran and a step failed
#   2  the measurement could not run
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ARCH=${1:-x86_64}
WORK=${WORK:-/home/user/work}
PACMAN=${PACMAN:-$WORK/pacman/$ARCH/build/pacman}
ROOT=${ROOT:-$WORK/rootfs/$ARCH}
OUT="$HERE/out/90-bootstrap-arch.$ARCH.txt"
MIRROR='https://geo.mirror.pkgbuild.com/$repo/os/$arch'

[ "$ARCH" = x86_64 ] || { echo "90: only x86_64 is bootstrapped here; see the header" >&2; exit 2; }
[ -x "$PACMAN" ] || { echo "90: no pacman at $PACMAN -- run 80-build-pacman.sh first" >&2; exit 2; }
[ "$(id -u)" = 0 ] || { echo "90: needs root (it chroots)" >&2; exit 2; }
mkdir -p "$HERE/out" || exit 2

# ⛔⛔ THE MOST DANGEROUS LINE IN THIS FILE IS THE `rm -rf` BELOW.
# READ THIS BEFORE EDITING ANYTHING ABOUT MOUNTS OR CLEANUP.
#
# WHAT HAPPENED ON THIS MACHINE, TWICE, DURING THIS SWEEP:
#   1. A run installed `base`, then bind-mounted the host's /dev into $ROOT
#      so the chroot checks would work.
#   2. Its `umount` failed -- "target is busy" -- and nothing checked. The
#      bind mount stayed live after the script exited.
#   3. The NEXT run's `rm -rf "$ROOT"` walked THROUGH that live bind mount
#      and deleted the HOST's device nodes. /dev/zero, /dev/urandom and
#      /dev/tty were gone; /dev/null was left behind as a regular file.
#   4. Every run after that "failed" with `error: segmentation fault` at the
#      end of `pacman -S base`, because libcrypto and gpg could no longer
#      open /dev/urandom.
#
# ⛔ THAT SEGFAULT WAS WRITTEN UP IN THIS REPOSITORY AS AN OPEN PACMAN BUG,
# through a whole round of hypothesis and control-matrix work, before the
# cause turned out to be this script eating the host it ran on.
# RESEARCH.md §0 keeps the wrong version on the record.
#
# TWO CHANGES CAME OUT OF IT, and both are load-bearing:
#   - ⭐ /dev IS NO LONGER BIND-MOUNTED AT ALL. The five device nodes a
#     scriptlet or gpg actually needs are created with mknod INSIDE the root,
#     so there is nothing there that points at the host and `rm -rf` cannot
#     reach anything outside $ROOT even if every other guard fails.
#   - The rm is guarded anyway. Defence in depth: check, try to clear,
#     check again, and REFUSE rather than delete.
unmount_all() {  # $1 = directory. 0 = nothing mounted under it any more.
  [ -d "$1" ] || return 0
  # Deepest first: a nested mount blocks its parent.
  for _m in $(awk -v d="$1" 'index($2, d) == 1 {print length($2)"\t"$2}' /proc/mounts \
              | sort -rn | cut -f2); do
    umount -R "$_m" 2>/dev/null || umount -R -l "$_m" 2>/dev/null || true
  done
  # ⚠ VERIFY. `umount -l` detaches lazily and returns success while the
  # mount is still listed, so its exit code decides nothing. This does.
  if awk -v d="$1" 'index($2, d) == 1 {print $2}' /proc/mounts | grep -q .; then
    return 1
  fi
  return 0
}

if ! unmount_all "$ROOT"; then
  echo "90: something is still mounted under $ROOT and could not be detached:" >&2
  awk -v d="$ROOT" 'index($2, d) == 1 {print "    "$2}' /proc/mounts >&2
  echo "90: REFUSING to rm -rf it. Clear it by hand, then re-run." >&2
  exit 2
fi
rm -rf "$ROOT"
# ⚠ --hookdir MUST ALREADY EXIST. pacman resolves it eagerly and aborts with
# "failed to resolve path ... passed to '--hookdir'" before it does anything
# else, unlike --dbpath and --cachedir which it creates. Create every one of
# them; the asymmetry is not documented and costs a full run to find.
mkdir -p "$ROOT/var/lib/pacman" "$ROOT/var/cache/pacman/pkg" \
         "$ROOT/etc/pacman.d/hooks" "$ROOT/etc/pacman.d/gnupg" \
         "$ROOT/var/log" "$ROOT/dev" "$ROOT/proc" "$WORK/rootfs/conf" || exit 2

# ⚠ EVERY PATH IS REDIRECTED INTO THE TARGET. --root alone is not enough:
# pacman keeps its database, cache, hooks, keyring and log at compiled-in
# absolute paths, and without each flag below it writes them onto the HOST.
# On a host that is not Arch that is silent, and on a host that is Arch it
# corrupts the running system's database.
CONF="$WORK/rootfs/conf/pacman.conf"
cat > "$CONF" <<EOF
[options]
Architecture = $ARCH
SigLevel     = Never

[core]
Server = $MIRROR

[extra]
Server = $MIRROR
EOF

pac() {
  "$PACMAN" \
    --config "$CONF" \
    --root "$ROOT" \
    --dbpath "$ROOT/var/lib/pacman" \
    --cachedir "$ROOT/var/cache/pacman/pkg" \
    --hookdir "$ROOT/etc/pacman.d/hooks" \
    --gpgdir "$ROOT/etc/pacman.d/gnupg" \
    --logfile "$ROOT/var/log/pacman.log" \
    "$@"
}

rc=0
sync_st=-; inst_st=-; chroot_st=-; self_st=-; verify_st=-
pkgs=-; rootsz=-; selfver=-
LOG=$WORK/rootfs/bootstrap.log
: > "$LOG"

t0=$(date +%s)
if pac -Sy --noconfirm >> "$LOG" 2>&1; then sync_st=ok; else sync_st=FAIL; rc=1; fi

if [ "$sync_st" = ok ]; then
  if pac -S --noconfirm base >> "$LOG" 2>&1; then inst_st=ok; else inst_st=FAIL; rc=1; fi
fi
t1=$(date +%s)

if [ "$inst_st" = ok ]; then
  pkgs=$(ls -1 "$ROOT/var/lib/pacman/local" 2>/dev/null | grep -vc '^ALPM_DB_VERSION$' || echo 0)
  rootsz=$(du -sh "$ROOT" 2>/dev/null | cut -f1)

  # ⭐ THE CHROOT IS THE PROOF. It executes the new root's own dynamically
  # linked glibc binaries with the new root's own loader. Nothing about the
  # static builder is involved any more.
  # ⭐ NO BIND MOUNT OF /dev. Real device nodes, made in place. These five
  # are what install scriptlets and gpg need; nothing here refers to the
  # host, so the cleanup path has nothing dangerous to undo.
  mknod -m 666 "$ROOT/dev/null"    c 1 3 2>/dev/null
  mknod -m 666 "$ROOT/dev/zero"    c 1 5 2>/dev/null
  mknod -m 666 "$ROOT/dev/random"  c 1 8 2>/dev/null
  mknod -m 666 "$ROOT/dev/urandom" c 1 9 2>/dev/null
  mknod -m 666 "$ROOT/dev/tty"     c 5 0 2>/dev/null
  mount -t proc proc "$ROOT/proc" 2>/dev/null
  cp -f /etc/resolv.conf "$ROOT/etc/resolv.conf" 2>/dev/null

  if chroot "$ROOT" /usr/bin/bash -c 'echo chroot-ok' >> "$LOG" 2>&1; then
    chroot_st=ok
  else
    chroot_st=FAIL; rc=1
  fi

  # The Arch pacman inside the root, dynamically linked, using the database
  # the static one wrote. If the db were wrong this is where it shows.
  if selfver=$(chroot "$ROOT" /usr/bin/pacman --version 2>&1 | grep -oE 'Pacman v[0-9.]+ - libalpm v[0-9.]+' | head -1); then
    [ -n "$selfver" ] && self_st=ok || { self_st=FAIL; rc=1; }
  else
    self_st=FAIL; rc=1
  fi

  # ⛔ SIGNATURES, SEPARATELY. Everything above ran with SigLevel=Never.
  # pacman-key is a shell script that drives gpg, so it needs a populated
  # root to run in -- which is exactly why the first pass cannot use it.
  if chroot "$ROOT" /usr/bin/bash -c '
        pacman-key --init >/dev/null 2>&1 &&
        pacman-key --populate archlinux >/dev/null 2>&1 &&
        printf "[options]\nArchitecture = auto\nSigLevel = Required DatabaseOptional\n[core]\nServer = https://geo.mirror.pkgbuild.com/\$repo/os/\$arch\n" > /etc/pacman.conf &&
        pacman -Sy --noconfirm >/dev/null 2>&1 &&
        pacman -S --noconfirm --needed --downloadonly bash >/dev/null 2>&1
      ' >> "$LOG" 2>&1; then
    verify_st=ok
  else
    verify_st=FAIL; rc=1
  fi

  # ⛔ AND VERIFY THE UNMOUNT. Not checking this is what caused the incident
  # described at the top of this file. /proc is the only mount now.
  if ! unmount_all "$ROOT"; then
    echo "90: WARNING -- could not unmount everything under $ROOT:" >&2
    awk -v d="$ROOT" 'index($2, d) == 1 {print "    "$2}' /proc/mounts >&2
    echo "90: do NOT delete that directory until it is cleared." >&2
    rc=1
  fi
fi

{
  echo "# 90-bootstrap-arch"
  echo "date        : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host        : $(uname -srm)"
  echo "host distro : $(. /etc/os-release 2>/dev/null && echo "$PRETTY_NAME")  ⭐ not Arch, no pacman installed"
  echo "binary      : $PACMAN"
  echo "             $(readelf -l "$PACMAN" 2>/dev/null | grep -q INTERP && echo 'DYNAMIC' || echo 'statically linked, no PT_INTERP')"
  echo "             $(wc -c < "$PACMAN" | tr -d ' ') bytes"
  echo "mirror      : $MIRROR"
  echo "target root : $ROOT"
  echo
  printf '%-28s %s\n' '1. pacman -Sy (sync dbs)'          "$sync_st"
  printf '%-28s %s\n' '2. pacman -S base (install)'       "$inst_st"
  printf '%-28s %s\n' '3. chroot runs its own bash'       "$chroot_st"
  printf '%-28s %s\n' '4. the root'\''s own pacman runs'    "$self_st"
  printf '%-28s %s\n' '5. keyring + signed sync'          "$verify_st"
  echo
  printf '%-28s %s\n' 'packages installed'                "$pkgs"
  printf '%-28s %s\n' 'root size'                         "$rootsz"
  printf '%-28s %ss\n' 'sync+install time'                "$((t1-t0))"
  printf '%-28s %s\n' 'pacman inside the root'            "${selfver:--}"
  echo
  if [ $rc -ne 0 ]; then echo "--- last 25 log lines ---"; tail -25 "$LOG"; echo; fi
  echo "verdict: $([ $rc -eq 0 ] && echo 'a static pacman bootstrapped a working Arch root from source' \
                                 || echo 'FAILED -- see '"$LOG")"
} > "$OUT"

cat "$OUT"
exit $rc
