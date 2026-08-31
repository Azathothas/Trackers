#!/bin/sh
# check-control-bytes.sh - is there a literal control byte in any text file?
#
# The defect this exists to catch is a file that is invisible to review. A
# literal control byte makes a file unreadable to BOTH review tools at once:
#
#   - `grep` calls it binary and SKIPS it, saying so in a line nobody reads,
#     and "what else calls this?" is how most real holes get found;
#   - `git diff` prints "Binary files differ", so a code review of the file
#     shows NO DIFF AT ALL. `git diff --text` renders it fine, which is the
#     proof that only reviewability was ever at stake.
#
# ⭐ The runtime value is identical either way. Write the escape, not the byte:
# `\0` is the same character and stays reviewable. Because correctness never
# depends on it, this survives for a long time unnoticed.
#
# ⚠ IT IS NOT A RULE PEOPLE CAN REMEMBER. In the project this came from, the
# rule was stated in a shared source file and four source files broke it anyway,
# and the post-mortem writing up the lesson reintroduced the byte TWICE while
# writing about it. A rule that a careful person breaks while documenting it is
# a rule that needs a check.
#
# -- THE THREE BLIND SPOTS THIS SCOPE WAS PAID FOR ---------------------------
#
# 1. ⛔ TRACKED ALONE IS NOT ENOUGH. `git ls-files` cannot see a file that has
#    never been staged, which is exactly when a new file is most likely to
#    acquire a stray byte. A brand-new test file was written with a literal NUL
#    where a trailing space belonged; grep called it binary, an assertion went
#    green for the wrong reason, and the guard reported clean because the file
#    was not tracked yet.
#
# 2. ⛔ `git ls-files` IS RELATIVE TO THE PROCESS WORKING DIRECTORY, so this
#    guard's scope used to depend on who called it. Run from the repository
#    root it saw 1071 files; run from one package directory, which is where a
#    per-package test script invoked it, it saw 391 and nothing else. Whole
#    trees were outside the scope of the one invocation that ran on every gate,
#    and a literal NUL rode through two handoffs that each reported green.
#    ⭐ A guard cannot prove itself. It is pinned to the repository root here.
#
# 3. ⚠ BINARIES ARE OUT OF SCOPE BY CONSTRUCTION, not by an allowlist. The
#    extension list below says what IS text. An "allowlist of binaries that are
#    fine" is the kind of list that quietly absorbs a real finding.
#
# ⚠ MARKDOWN IS IN SCOPE HERE AND NOWHERE ELSE. check-docs.sh used to carry a
# markdown-only copy of this rule. Two checks enforcing one rule is two places
# for it to be wrong, so the rule lives here and check-docs.sh points at it.
#
# Usage:
#   sh scripts/common/check-control-bytes.sh
#   sh scripts/common/check-control-bytes.sh --json
#
# Exit codes: 0 clean, 1 a byte was found, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

set -u

JSON=0

while [ $# -gt 0 ]; do
  case "$1" in
    --json) JSON=1 ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-control-bytes: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v git >/dev/null 2>&1 || { printf 'check-control-bytes: git not found\n' >&2; exit 2; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { printf 'check-control-bytes: not a git repository\n' >&2; exit 2; }
command -v awk >/dev/null 2>&1 || { printf 'check-control-bytes: awk not found\n' >&2; exit 2; }
REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT" || { printf 'check-control-bytes: cannot enter %s\n' "$REPO_ROOT" >&2; exit 2; }

# Extensions asserted to be TEXT. Anything else is out of scope by construction.
TEXT_RE='\.(ts|tsx|js|mjs|cjs|jsx|json|md|sql|css|scss|html|toml|yaml|yml|sh|ps1|py|rs|go|c|h|cpp|hpp|java|rb|php|txt|cfg|ini|conf|env\.example)$'

FILES=$(
  {
    git ls-files 2>/dev/null
    git ls-files --others --exclude-standard 2>/dev/null
  } | sort -u | grep -E "$TEXT_RE" || true
)
if [ -z "$FILES" ]; then
  printf 'check-control-bytes: no text files in scope\n' >&2
  exit 2
fi

# ⛔ THE PATTERN IS BUILT BY printf, NOT WRITTEN AS A LITERAL. POSIX grep does
# NOT expand `\001` inside a bracket expression: it reads the backslash and the
# digits as ordinary characters, so the class would match a backslash, a digit
# and most of the alphabet. This exact mistake reported a control byte in every
# one of forty-eight clean files in this repository, and the real count was
# zero. printf turns the escapes into real bytes first.
#
# C0 controls except the three that are legitimately in text: tab, newline and
# carriage return. NUL cannot live in a shell variable, so it is found
# separately below.
CTRL_CLASS=$(printf '[\001-\010\013\014\016-\037]')

COUNT=0
NFILES=0
REPORT=""

for f in $FILES; do
  [ -f "$f" ] || continue          # tracked but deleted; git reports that itself
  NFILES=$((NFILES + 1))

  hit=$(LC_ALL=C grep -n "$CTRL_CLASS" "$f" 2>/dev/null | head -3 || true)
  if [ -n "$hit" ]; then
    COUNT=$((COUNT + 1))
    line=$(printf '%s' "$hit" | head -1 | cut -d: -f1)
    REPORT="$REPORT  $f:$line a C0 control byte
"
    continue
  fi

  # ⚠ NUL IS A SEPARATE TEST. It cannot be put in the class above, and it is
  # the single commonest offender: it is what somebody reaches for as a
  # composite-key separator.
  n_all=$(wc -c < "$f" 2>/dev/null || echo 0)
  n_strip=$(LC_ALL=C tr -d '\000' < "$f" 2>/dev/null | wc -c || echo 0)
  if [ "$n_all" != "$n_strip" ]; then
    COUNT=$((COUNT + 1))
    REPORT="$REPORT  $f a NUL byte
"
  fi
done

if [ "$JSON" = "1" ]; then
  printf '{"schema":"check-control-bytes/1","problems":%s,"files":%s}\n' "$COUNT" "$NFILES"
  [ "$COUNT" -gt 0 ] && exit 1
  exit 0
fi

if [ "$COUNT" -gt 0 ]; then
  printf 'literal control bytes in %s file(s):\n\n%s\n' "$COUNT" "$REPORT"
  printf 'Write the ESCAPE, not the byte. The escape is the same character at\n'
  printf 'runtime, and the byte is what makes the file invisible to grep and\n'
  printf 'unreviewable in git diff. docs/conventions/shell.md section 6.\n'
  exit 1
fi

printf 'no literal control bytes in %s text files (tracked plus untracked-not-ignored)\n' "$NFILES"
exit 0
