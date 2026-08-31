#!/bin/sh
# check-markers.sh - only the five defined characters, and not too many of them.
#
# Two rules, one subject, one home. docs/conventions/prose.md is the rule and
# this is the machine behind it.
#
#   1. THE CHARACTER SET. Every tracked text file is ASCII, with the three
#      prose markers and the two status glyphs as the only exception.
#   2. THE DENSITY. A file carrying more markers than the ceiling below is
#      refused, because a page where every paragraph shouts has no markers at
#      all.
#
# -- WHY THIS OWNS THE CHARACTER RULE AND check-docs.sh NO LONGER DOES -------
#
# ⛔ THE RULE USED TO SCAN MARKDOWN ALONE, which left every .sh, .ps1, .c,
# .yml and .mjs in the tree unchecked. Measured on this repository on
# 2026-08-28, before this check existed: 2290 characters outside the five, in
# 22 files, and every one of them was in a script rather than a document. An
# adopter of this template found the same shape independently and had to clear
# 76 of them across 11 files before it could arm a check at all.
#
# ⛔ Two checks enforcing one rule is two places for it to be wrong, and they
# WOULD have been wrong differently: check-docs strips fenced blocks and code
# spans before it looks, and a whole-tree scan that did not would refuse the
# page that names the character it bans. So the rule moved here entire, the
# same way the control-byte rule moved out of check-docs into its own file,
# and for the same reason.
#
# -- THE DENSITY CEILING, AND WHERE THE NUMBER CAME FROM --------------------
#
# ⭐ prose.md has always said to use markers "sparingly enough that they are
# still visible" and nothing checked it, so an agent kept strictly to the five
# allowed characters and spammed them until the documents were unreadable.
# That is not a hypothetical: it is what this ceiling was measured against.
#
# Markers per 100 non-blank lines, measured 2026-08-28 on one Windows 11 Pro
# 26200 machine, over the tracked markdown of three trees:
#
#   pkgforge-dev/docker-bsd            38.6 overall, worst file 53.3
#   Azathothas/TEMPLATE (this tree)     9.0 overall, worst file 26.3
#   pkgforge-dev/cross-libc-dlopen      8.6 overall, worst file 21.8
#
# ⭐ The two ADOPTER trees were ranked by eye before any of this was counted,
# and the ranking came out in that order. ⚠ Only those two were ranked; this
# tree was not placed against them and its number simply falls between. One
# number reproducing a reading is the argument for having the number at all.
#
# The ceiling is 30. It passes every file in the two trees that read well and
# refuses 7 of the 12 files in the one that does not.
#
# ⚠ IT IS A CONSTANT, NOT A FLAG. A ceiling anybody can raise from a command
# line is a ceiling that gets raised instead of met. A project that genuinely
# needs a different number edits the line below, which is a change somebody
# reviews.
#
# ⚠ WHAT IT CANNOT SEE. Density is a count, and a marker used wrongly is a
# reading: a status glyph carrying a rule, or a ⛔ on a preference, passes this
# check and fails a review. prose.md says so in as many words, and the split is
# the same one already true of every other rule a linter holds.
#
# ⚠ AND WHAT check-twins CANNOT SEE ABOUT THIS PAIR. It compares the two
# halves' ANSWERS on the tree it runs against, not their rules. Measured on
# 2026-08-28 with three deliberate divergences planted in the .ps1 half:
#
#   ceiling 30 changed to 20      -> caught, exit 1
#   `md` dropped from the scope   -> caught, exit 1 (files 96 -> 38)
#   ⛔ `py` dropped from the scope -> INVISIBLE, exit 0
#
# The third is invisible because this tree holds no .py file, so the smaller
# scope produces an identical number. ⭐ Prove a scope rule with a fixture, not
# by trusting the comparison to notice. scripts/README.md records the same
# experiment against an older pair; it reproduces here unchanged.
#
# -- THE TWO EXEMPTIONS, EACH FOR A REASON THAT WOULD OTHERWISE BREAK ---------
#
# ⛔ LICENSES/*.txt IS EXEMPT. Those are canonical SPDX texts. GPL-3.0 and
# LGPL-3.0 carry typographic quotes and a copyright sign, and four of the
# twelve must never have their notice altered at all because the copyright line
# is somebody else's. LICENSES/README.md says which four and why. A check that
# asked anybody to edit these would be asking for a corruption.
#
# ⚠ A LEADING BYTE-ORDER MARK IS EXEMPT, and only a leading one. Every .ps1
# here begins with one and PowerShell on Windows wants it there. A BOM anywhere
# else in a file is a real defect and is still reported.
#
# ⭐ A SPECIMEN INSIDE A CODE SPAN OR A FENCED BLOCK IS PERMITTED, in markdown.
# Without it the rule is unwritable: a page that bans a character cannot show a
# reader which character it means, and this file's own mutation test cannot
# record the plant that proves the check fires.
#
# Usage:
#   sh scripts/common/check-markers.sh
#   sh scripts/common/check-markers.sh --json
#
# Exit codes: 0 clean, 1 a character or a density was refused, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

set -u

JSON=0
CEILING=30

while [ $# -gt 0 ]; do
  case "$1" in
    --json) JSON=1 ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-markers: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v git >/dev/null 2>&1 || { printf 'check-markers: git not found\n' >&2; exit 2; }
command -v awk >/dev/null 2>&1 || { printf 'check-markers: awk not found\n' >&2; exit 2; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { printf 'check-markers: not a git repository\n' >&2; exit 2; }

# ⛔ PINNED TO THE REPOSITORY ROOT. `git ls-files` is relative to the process
# working directory, so a guard invoked from a subdirectory silently reports on
# a smaller tree and calls it clean. check-control-bytes.sh carries the same
# note and the same fix, and it was paid for there.
REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT" || { printf 'check-markers: cannot enter %s\n' "$REPO_ROOT" >&2; exit 2; }

# Extensions asserted to be TEXT. Anything else is out of scope by
# construction, which is the same reasoning check-control-bytes.sh states: an
# allowlist of "binaries that are fine" is the kind of list that quietly
# absorbs a real finding.
TEXT_RE='\.(ts|tsx|js|mjs|cjs|jsx|json|md|sql|css|scss|html|toml|yaml|yml|sh|ps1|py|rs|go|c|h|cpp|hpp|java|rb|php|txt|cfg|ini|conf)$'

# ⛔ TRACKED PLUS UNTRACKED-BUT-NOT-IGNORED. A file that has never been staged
# is exactly when a new file is likeliest to carry the defect, and it is what
# the next `git add -A` would take.
FILES=$(
  {
    git ls-files 2>/dev/null
    git ls-files --others --exclude-standard 2>/dev/null
  } | sort -u | grep -E "$TEXT_RE" | grep -v '^LICENSES/.*\.txt$' || true
)
if [ -z "$FILES" ]; then
  printf 'check-markers: no text files in scope\n' >&2
  exit 2
fi

TMP="${TMPDIR:-/tmp}/.checkmarkers.$$"
mkdir -p "$TMP" || { printf 'check-markers: cannot write to %s\n' "$TMP" >&2; exit 2; }
trap 'rm -rf "$TMP"' EXIT INT TERM

# ⛔ THE ALLOWED SEQUENCES ARE BUILT BY printf, NOT WRITTEN AS LITERALS, and
# they are matched as BYTES. Two separate reasons, and both have bitten a check
# in this repository before:
#
#   1. A pattern written as a literal escape is not expanded by every tool.
#      POSIX grep reads `\001` inside a bracket expression as a backslash and
#      two digits, and that exact mistake once reported a control byte in all
#      48 clean files of this tree. printf makes real bytes first.
#   2. A byte class under a UTF-8 locale splits a three-byte character into
#      three fragments, so a count comes out three times too high and looks
#      like real output. LC_ALL=C below states that this pass is byte-oriented,
#      which is the half of that lesson that holds on every machine.
#
# U+26D4 stop, U+2B50 star, U+26A0 warning, U+2705 pass, U+274C fail.
M1=$(printf '\342\233\224')
M2=$(printf '\342\255\220')
M3=$(printf '\342\232\240')
M4=$(printf '\342\234\205')
M5=$(printf '\342\235\214')
BOM=$(printf '\357\273\277')

PROBLEMS=0
NFILES=0
MARKERS=0
WORST=0
WORSTF="-"
REPORT=""

report() { REPORT="$REPORT  $1
"; PROBLEMS=$((PROBLEMS + 1)); }

for f in $FILES; do
  [ -f "$f" ] || continue          # tracked but deleted; git reports that itself
  NFILES=$((NFILES + 1))

  case "$f" in
    *.md) IS_MD=1 ;;
    *)    IS_MD=0 ;;
  esac

  # One awk pass per file: strip the exempt regions, count the markers, count
  # the non-blank lines, and emit every offending byte position.
  LC_ALL=C awk -v M1="$M1" -v M2="$M2" -v M3="$M3" -v M4="$M4" -v M5="$M5" \
               -v BOM="$BOM" -v ISMD="$IS_MD" '
    BEGIN {
      for (i = 0; i < 256; i++) ORD[sprintf("%c", i)] = i
      nmark = 0; nonblank = 0; fence = 0
    }
    {
      line = $0

      # ⚠ A LEADING BOM IS EXEMPT, AND ONLY A LEADING ONE. Stripped from the
      # first line of the file and nowhere else, so a BOM that a merge left in
      # the middle of a file is still a finding.
      if (NR == 1 && index(line, BOM) == 1) line = substr(line, length(BOM) + 1)

      if (line ~ /[^ \t]/) nonblank++

      # Markers are counted BEFORE anything is stripped, because the density
      # rule is about what a reader sees on the page.
      line2 = line
      for (k = 1; k <= 5; k++) {
        m = (k == 1 ? M1 : k == 2 ? M2 : k == 3 ? M3 : k == 4 ? M4 : M5)
        while ((p = index(line2, m)) > 0) {
          nmark++
          line2 = substr(line2, 1, p - 1) substr(line2, p + length(m))
        }
      }

      # ⭐ THE SPECIMEN EXEMPTION, markdown only. A fenced block is skipped
      # entire and an inline code span is cut out, so a page can name the
      # character it bans. Outside markdown there is no exemption: a source
      # file has no reader who needs a specimen.
      if (ISMD) {
        if (line ~ /^[ \t]*```/) { fence = !fence; next }
        if (fence) next
        while (match(line2, /`[^`]*`/))
          line2 = substr(line2, 1, RSTART - 1) substr(line2, RSTART + RLENGTH)
      }

      # Whatever survives must be ASCII. Report the first offender per line;
      # a line with one wrong character usually has several of the same.
      #
      # ⭐ THE CODEPOINT IS DECODED AND REPORTED, not just the position. This
      # check took over the em-dash rule from check-docs.sh, which named that
      # one character in its message, and a message reading only "something
      # non-ASCII on line 12" would have been a step backwards. U+2014 tells a
      # reader exactly which character to search for, for every offender rather
      # than for the one that was special-cased.
      for (i = 1; i <= length(line2); i++) {
        b1 = ORD[substr(line2, i, 1)]
        if (b1 < 128) continue
        cp = 0
        if (b1 >= 240) {
          cp = (b1 % 8) * 262144 + (ORD[substr(line2, i+1, 1)] % 64) * 4096 \
             + (ORD[substr(line2, i+2, 1)] % 64) * 64 + (ORD[substr(line2, i+3, 1)] % 64)
        } else if (b1 >= 224) {
          cp = (b1 % 16) * 4096 + (ORD[substr(line2, i+1, 1)] % 64) * 64 \
             + (ORD[substr(line2, i+2, 1)] % 64)
        } else if (b1 >= 192) {
          cp = (b1 % 32) * 64 + (ORD[substr(line2, i+1, 1)] % 64)
        }
        printf "CHAR\t%d\t%04X\n", NR, cp
        break
      }
    }
    END { printf "STAT\t%d\t%d\n", nmark, nonblank }
  ' "$f" > "$TMP/out" 2>/dev/null || true

  fmark=0
  fnon=1
  while IFS="$(printf '\t')" read -r kind a b; do
    case "${kind:-}" in
      CHAR) report "$f:$a U+$b is outside the five. docs/conventions/prose.md" ;;
      STAT) fmark=${a:-0}; fnon=${b:-1} ;;
    esac
  done < "$TMP/out"

  [ "$fnon" -lt 1 ] && fnon=1
  MARKERS=$((MARKERS + fmark))

  # Density, as an integer per 100 non-blank lines. Integer arithmetic on
  # purpose: `awk` would do this in floating point and a shell cannot, and a
  # ceiling comparison does not need the fraction.
  dens=$(( fmark * 100 / fnon ))
  if [ "$dens" -gt "$WORST" ]; then WORST=$dens; WORSTF="$f"; fi
  if [ "$dens" -gt "$CEILING" ]; then
    report "$f $fmark markers in $fnon non-blank lines, ${dens} per 100. The ceiling is $CEILING. docs/conventions/prose.md"
  fi
done

if [ "$JSON" = "1" ]; then
  printf '{"schema":"check-markers/1","problems":%s,"files":%s,"markers":%s,"ceiling":%s,"worst_density":%s}\n' \
    "$PROBLEMS" "$NFILES" "$MARKERS" "$CEILING" "$WORST"
  [ "$PROBLEMS" -gt 0 ] && exit 1
  exit 0
fi

if [ "$PROBLEMS" -gt 0 ]; then
  printf 'marker check failed, %s problem(s):\n\n%s\n' "$PROBLEMS" "$REPORT"
  printf 'The five are the three prose markers and the two status glyphs.\n'
  printf 'Everything else is ASCII. docs/conventions/prose.md is the rule.\n'
  exit 1
fi

printf 'markers ok: %s files, %s markers, densest %s per 100 non-blank lines (%s), ceiling %s\n' \
  "$NFILES" "$MARKERS" "$WORST" "$WORSTF" "$CEILING"
exit 0
