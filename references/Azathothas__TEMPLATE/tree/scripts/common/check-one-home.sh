#!/bin/sh
# check-one-home.sh - does any sentence appear in two documents?
#
# ⛔ THE DEFECT: one fact with two homes. docs/conventions/prose.md has always
# said every fact lives in exactly one document, and nothing checked it, so it
# drifted the way an unchecked rule always drifts. The copy a reader trusts is
# then whichever they saw first, and the one that is wrong is invisible until
# somebody notices the two disagree.
#
# ⭐ WHAT IT COST, MEASURED RATHER THAN ASSERTED. A project built from this
# template accumulated 8 sentences appearing verbatim in two documents and 3
# whole sections that were near-copies of a convention, in a file that opened
# by saying it restated nothing. Its maintainer cut that file from 149 lines to
# 66. This template's own tree, checked for the first time on 2026-08-28, held
# 42 duplicated sentences of 8 words or more, including 5 in the very skeleton
# it ships for that job.
#
# -- ⚠ THE FIRST RUN OF THE INSTRUMENT REPORTED ZERO, AND WAS WRONG ----------
#
# ⛔ It reported no duplicates at any threshold, over a 60-file document set,
# and the reason was that its file collector matched NOTHING: a quoted pathspec
# reached git through a shell that treats a single quote as an ordinary
# character. Zero duplicates over zero files reads exactly like a clean tree.
#
# ⭐ That is why this file ends by refusing to report success over an empty
# scope, and why the collector below is deliberately dull. A guard that cannot
# distinguish "nothing wrong" from "nothing examined" is not a guard.
#
# -- THE EXEMPTIONS, AND WHY EACH IS NOT A LOOPHOLE --------------------------
#
# ⛔ THE ENTRY-POINT ROUTERS ARE EXEMPT FROM EACH OTHER, AND ONLY FROM EACH
# OTHER. AGENTS.md, ROUTE.md and docs/templates/AGENTS.md each state the
# absolutes in full, on purpose: a session may be handed exactly one of them
# and nothing else, and a rule it has to follow a link to read is a rule it
# will not read. ⚠ A sentence shared between a router and any OTHER file is
# still refused, so the exemption cannot be used to seed a copy into the tree.
#
# ⛔ docs/history/ IS EXEMPT ENTIRELY. A superseded page states things the live
# pages now state differently, which is the whole point of it.
# docs/methodology/history.md carries that rule.
#
# -- ⚠ WHAT IT CANNOT SEE ----------------------------------------------------
#
# It compares SENTENCES, so a fact restated in different words passes and fails
# a review instead. That is the same split every other prose rule here has: the
# linter owns the mechanical half and the reading owns the rest. ⭐ The
# mechanical half is still worth having, because verbatim duplication is what
# copy-and-paste actually produces.
#
# ⚠ Headings, table rows, fenced blocks and code spans are excluded. A shared
# command is not a shared fact, and two documents naming the same file in a
# table is a cross-reference rather than a copy.
#
# Usage:
#   sh scripts/common/check-one-home.sh
#   sh scripts/common/check-one-home.sh --json
#
# Exit codes: 0 clean, 1 a sentence has two homes, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

set -u

JSON=0
# ⚠ A CONSTANT, NOT A FLAG, for the reason check-markers.sh gives about its own
# ceiling: a threshold anybody can raise from a command line gets raised
# instead of met. Twelve words is long enough that two documents do not reach
# it by coincidence and short enough to catch a copied rule.
MINWORDS=12

while [ $# -gt 0 ]; do
  case "$1" in
    --json) JSON=1 ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-one-home: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v git >/dev/null 2>&1 || { printf 'check-one-home: git not found\n' >&2; exit 2; }
command -v awk >/dev/null 2>&1 || { printf 'check-one-home: awk not found\n' >&2; exit 2; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { printf 'check-one-home: not a git repository\n' >&2; exit 2; }
REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT" || { printf 'check-one-home: cannot enter %s\n' "$REPO_ROOT" >&2; exit 2; }

# ⛔ NO QUOTED PATHSPEC. The extension filter is applied here, by grep, rather
# than handed to git, because a quoted pathspec crossing a shell that does not
# treat a quote as a quote matches nothing and the check then passes over an
# empty set. That is exactly how the first version of this reported a clean
# tree it had never opened.
FILES=$(
  {
    git ls-files 2>/dev/null
    git ls-files --others --exclude-standard 2>/dev/null
  } | sort -u | grep '\.md$' | grep -v '^docs/history/' || true
)
if [ -z "$FILES" ]; then
  printf 'check-one-home: no markdown files in scope\n' >&2
  exit 2
fi

TMP="${TMPDIR:-/tmp}/.checkonehome.$$"
mkdir -p "$TMP" || { printf 'check-one-home: cannot write to %s\n' "$TMP" >&2; exit 2; }
trap 'rm -rf "$TMP"' EXIT INT TERM

NFILES=0
: > "$TMP/pairs"

for f in $FILES; do
  [ -f "$f" ] || continue
  NFILES=$((NFILES + 1))
  # One sentence per line, normalised, prefixed with its file.
  LC_ALL=C awk -v F="$f" -v MIN="$MINWORDS" '
    /^[ \t]*```/ { fence = !fence; next }
    fence        { next }
    /^[ \t]*\|/  { next }        # a table row is not a sentence
    /^[ \t]*#/   { next }        # nor is a heading
    {
      line = $0
      while (match(line, /`[^`]*`/))
        line = substr(line, 1, RSTART - 1) " " substr(line, RSTART + RLENGTH)
      gsub(/\[/, " ", line); gsub(/\]\([^)]*\)/, " ", line)
      buf = buf " " line
    }
    END {
      n = split(buf, part, /[.:!?]+[ \t]+/)
      for (i = 1; i <= n; i++) {
        s = tolower(part[i])
        gsub(/[^a-z0-9 ]/, " ", s)
        gsub(/  +/, " ", s)
        sub(/^ /, "", s); sub(/ $/, "", s)
        if (s == "") continue
        if (split(s, w, " ") < MIN) continue
        printf "%s\t%s\n", s, F
      }
    }
  ' "$f" >> "$TMP/pairs" 2>/dev/null || true
done

# ⚠ THE SCOPE IS ASSERTED BEFORE THE VERDICT. See the header: a run over zero
# files is not a clean run.
if [ "$NFILES" -lt 2 ]; then
  printf 'check-one-home: only %s file(s) in scope; nothing to compare\n' "$NFILES" >&2
  exit 2
fi

# Group by sentence, keep the ones seen in more than one DISTINCT file, then
# drop any whose files are all routers.
# ⚠ ONE RECORD PER LINE, files joined by a space. An earlier version joined
# them with a NEWLINE, so a single duplicate occupied three lines and the count
# below, which counts lines, reported one planted duplicate as three. The
# verdict was right and every number beside it was wrong.
sort -u "$TMP/pairs" | awk -F'\t' '
  BEGIN {
    R["AGENTS.md"] = 1; R["ROUTE.md"] = 1; R["docs/templates/AGENTS.md"] = 1
  }
  { key = $1; files[key] = files[key] " " $2; count[key]++ }
  END {
    for (k in count) {
      if (count[k] < 2) continue
      n = split(files[k], fs, " ")
      allrouters = 1
      for (i = 1; i <= n; i++) if (fs[i] != "" && !(fs[i] in R)) allrouters = 0
      if (allrouters) continue
      printf "%s\t%s\n", k, substr(files[k], 2)
    }
  }
' > "$TMP/dups" 2>/dev/null || true

# ⚠ awk, NOT `grep -c`. `grep -c` on a file with no matches prints 0 AND exits
# 1, so `grep -c . f || printf 0` printed BOTH zeros and the result was the
# two-line string "0\n0", which every later numeric test refused as "integer
# expected". The check still exited 0, so it looked like it worked.
COUNT=$(awk 'END { print NR + 0 }' "$TMP/dups" 2>/dev/null)
[ -n "$COUNT" ] || COUNT=0

if [ "$JSON" = "1" ]; then
  printf '{"schema":"check-one-home/1","problems":%s,"files":%s,"min_words":%s}\n' \
    "$COUNT" "$NFILES" "$MINWORDS"
  [ "$COUNT" -gt 0 ] && exit 1
  exit 0
fi

if [ "$COUNT" -gt 0 ]; then
  printf 'one fact, one home: %s sentence(s) appear in more than one document:\n\n' "$COUNT"
  while IFS="$(printf '\t')" read -r s rest; do
    [ -n "$s" ] || continue
    printf '  "%s"\n' "$(printf '%s' "$s" | cut -c1-88)"
    for one in $rest; do
      printf '      %s\n' "$one"
    done
    printf '\n'
  done < "$TMP/dups"
  printf 'Keep the fact in the document that owns it and make the other a pointer.\n'
  printf 'docs/conventions/prose.md, "one fact, one home".\n'
  exit 1
fi

printf 'one fact one home: %s documents, no sentence of %s+ words in two of them\n' \
  "$NFILES" "$MINWORDS"
exit 0
