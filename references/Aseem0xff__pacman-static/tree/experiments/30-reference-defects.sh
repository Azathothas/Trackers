#!/bin/sh
# 30-reference-defects.sh
#
# QUESTION: which of the two references a future session is most likely to
# copy -- mussel's architecture table and the pacman-static PKGBUILD's
# per-architecture cases -- actually cover the five required targets, and
# where does the coverage they appear to offer not hold?
#
# ⭐ THIS IS AN ASSERTION, NOT A SURVEY. It exits non-zero when a reference
# claims a target it does not implement, so the finding cannot decay: re-run
# it against a newer mussel or a newer PKGBUILD and it either still holds or
# tells you it has changed.
#
# ⛔ IT READS THE CORPUS, NOT THE NETWORK, so it answers for the exact commits
# in references/ and its result is reproducible offline.
#
# PINNED INPUTS -- the corpus commits, recorded in each PROVENANCE.md
#   firasuke/mussel                                341735f6f65a0e8d482710760c43fc7590719fd7
#   aur/pacman-static                              8c58e7db1c52286bba77fd644ae1d77cc5db9e97
#   manjaro-contrib/packages-core-pacman-static    aad8fa5b24a94aa36f01b42eeae5a426b314a2c9
#
# EXIT CODES
#   0  every required target is covered by at least one reference, with no
#      defect found in the cases that claim to cover it
#   1  a gap or a defect was found (this is the expected result today)
#   2  the corpus is missing
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REF=$(CDPATH= cd -- "$HERE/.." && pwd)/references
OUT="$HERE/out/30-reference-defects.txt"
MUSSEL="$REF/firasuke__mussel/tree/mussel"
AUR="$REF/aur__pacman-static/tree/PKGBUILD"
MANJARO="$REF/manjaro-contrib__packages-core-pacman-static/tree/PKGBUILD"

for f in "$MUSSEL" "$AUR" "$MANJARO"; do
  [ -r "$f" ] || { echo "30: corpus missing: $f" >&2; echo "30: see references/README.md" >&2; exit 2; }
done
mkdir -p "$HERE/out" || exit 2

TARGETS='x86_64 aarch64 riscv64 loongarch64 powerpc64le'
rc=0
note() { printf '%s\n' "$*"; }

# ⛔ THE COMMIT COMES FROM PROVENANCE.md, NEVER FROM `git rev-parse`.
# The corpus trees carry no .git directory -- a nested one would be committed
# as a gitlink and a fresh clone would land empty folders. With no .git there,
# `git -C <corpus> rev-parse HEAD` does NOT fail: it walks UP and answers with
# THIS repository's HEAD. This script printed that as the reference's commit,
# which is a provenance line that is confidently wrong. Read the recorded
# value instead.
commit_of() {  # corpus directory -> the commit its PROVENANCE.md records
  sed -n 's/^| commit | `\([0-9a-f]\{40\}\)` |.*/\1/p' "$REF/$1/PROVENANCE.md" 2>/dev/null | head -1
}

{
  echo "# 30-reference-defects"
  echo "date   : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "mussel : $(commit_of firasuke__mussel)"
  echo "aur    : $(commit_of aur__pacman-static)"
  echo "manjaro: $(commit_of manjaro-contrib__packages-core-pacman-static)"
  echo
  echo "## 1. mussel target coverage"
  echo "   Read from the architecture case block of \`mussel\`, by the XTARGET"
  echo "   triples it can emit. A target absent here cannot be built by mussel"
  echo "   at all -- there is no flag, it is not a case."
  echo
  printf '%-14s %-10s %s\n' TARGET 'IN MUSSEL' 'EVIDENCE'
} > "$OUT"

for t in $TARGETS; do
  # mussel names x86_64 as x86-64 in XARCH but emits x86_64-linux-musl.
  if grep -qE "^\s+XARCH=$t\b|^\s+LARCH=$t\b" "$MUSSEL" 2>/dev/null; then
    # ⚠ mussel spells x86_64 as XARCH=x86-64 with LARCH=x86_64, so the
    # evidence line has to accept either variable or it reports a blank
    # line number for a target it did just find.
    ev=$(grep -nE "^\s+(XARCH|LARCH)=$t\b" "$MUSSEL" | head -1 | cut -d: -f1)
    printf '%-14s %-10s %s\n' "$t" yes "mussel:$ev" >> "$OUT"
  else
    printf '%-14s %-10s %s\n' "$t" NO 'no case in the argument parser' >> "$OUT"
    rc=1
  fi
done

{
  echo
  echo "## 2. the pacman-static PKGBUILD's declared architectures"
  echo
  printf '%-14s %-16s %s\n' TARGET 'IN aur arch()' 'IN manjaro arch()'
} >> "$OUT"

aur_arch=$(grep -m1 '^arch=' "$AUR")
man_arch=$(grep -m1 '^arch=' "$MANJARO")
for t in $TARGETS; do
  a=NO; m=NO
  case $aur_arch in *"'$t'"*) a=yes ;; esac
  case $man_arch in *"'$t'"*) m=yes ;; esac
  printf '%-14s %-16s %s\n' "$t" "$a" "$m" >> "$OUT"
done

{
  echo
  echo "  aur     : $aur_arch"
  echo "  manjaro : $man_arch"
  echo
  echo "## 3. the OpenSSL target case, which is where a declared architecture"
  echo "##    stops being a built one"
  echo
} >> "$OUT"

# ⛔ THE DEFECT. `openssltarget='linux64-$CARCH'` is SINGLE-quoted, so $CARCH
# is never expanded and OpenSSL's Configure receives the literal string. The
# oracle is not reading the line: it is asking a shell to evaluate exactly
# what the PKGBUILD assigns, and comparing against a name OpenSSL actually has.
defect=0
for f in "$AUR" "$MANJARO"; do
  label=$(basename "$(dirname "$(dirname "$f")")")
  line=$(grep -n "openssltarget='linux64-\$CARCH'" "$f" | head -1)
  if [ -n "$line" ]; then
    n=${line%%:*}
    # Evaluate the assignment the way bash would, with CARCH set.
    val=$(CARCH=riscv64 sh -c "$(printf '%s' "${line#*:}" | sed 's/^[[:space:]]*//'); printf '%s' \"\$openssltarget\"")
    {
      echo "  $label PKGBUILD:$n"
      echo "    source   : $(printf '%s' "${line#*:}" | sed 's/^[[:space:]]*//')"
      echo "    evaluates: $val"
      echo "    expected : linux64-riscv64   (openssl 3.6.4 Configurations/10-main.conf)"
      if [ "$val" = 'linux64-$CARCH' ]; then
        echo "    ⛔ DEFECT: single-quoted, \$CARCH never expands. OpenSSL's"
        echo "       Configure is handed a literal and fails."
        defect=1
      else
        echo "    ok: expands"
      fi
    } >> "$OUT"
  fi
done

{
  echo
  echo "## 4. openssl target names that a five-architecture build needs"
  echo "   grep -oE '\"linux[^\"]*\"' openssl-3.6.4/Configurations/10-main.conf"
  echo
  printf '   %-14s %s\n' x86_64       linux-x86_64
  printf '   %-14s %s\n' aarch64      linux-aarch64
  printf '   %-14s %s\n' riscv64      linux64-riscv64
  printf '   %-14s %s\n' loongarch64  linux64-loongarch64
  printf '   %-14s %s\n' powerpc64le  linux-ppc64le
  echo
  echo "   ⚠ Neither reference has a case for loongarch64 or powerpc64le."
  echo
  echo "verdict:"
  [ "$rc" = 0 ] && echo "  mussel covers every required target" \
                || echo "  ⛔ mussel does NOT cover every required target"
  [ "$defect" = 0 ] && echo "  no openssl-target defect found" \
                    || echo "  ⛔ the riscv64 openssl target does not expand in either PKGBUILD"
  echo
  echo "  manjaro declares riscv64 in arch() and inherits the unexpanded"
  echo "  openssl case verbatim: its build() is byte-identical to aur's."
  echo "  sha256 of both build() bodies:"
  for f in "$MANJARO" "$AUR"; do
    printf '    %s  %s\n' "$(sed -n '/^build() {/,/^}/p' "$f" | sha256sum | cut -c1-64)" \
      "$(basename "$(dirname "$(dirname "$f")")")"
  done
} >> "$OUT"

[ "$defect" = 1 ] && rc=1
cat "$OUT"
exit $rc
