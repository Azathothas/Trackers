#!/bin/sh
# check-changelog.sh - does CHANGELOG.md still obey the four rules that a
# machine can hold?
#
# The defect this exists to catch is a changelog that stopped being orderable.
# docs/conventions/docs.md states four rules and says in as many words that
# each is mechanical enough to check. Nothing checked them, which is the exact
# shape this template warns about: a rule stated in a document and enforced by
# nobody is a preference, and a preference stated as a rule is what makes an
# agent stop believing the rules that matter.
#
# In the project this came from, the file drifted in all four ways at once:
# entries landing mid-file, one section ascending while the rest descended, and
# headings with no date to order by.
#
# -- WHAT IT CHECKS ----------------------------------------------------------
#   1. ⛔ NEWEST FIRST. Dates inside a section descend. This is the rule that
#      breaks most often, because appending is what an editor does by default.
#   2. ⛔ Every entry heading carries a date, ISO 8601. Several entries sharing
#      one day cannot be ordered from what was written down, so a full UTC
#      stamp is accepted and a bare date is accepted; no date is not.
#   3. ⛔ Every entry names its record. An entry with no record is a claim.
#   4. ⛔ Every entry says whether it deployed. Silence is not an answer;
#      "not deployed" is.
#
# ⛔ WHAT IT DELIBERATELY DOES NOT CHECK IS WHETHER AN ENTRY IS TRUE. That is a
# reading and it belongs to the claim audit, docs/methodology/reviews.md lens 3.
# A guard that tried to verify prose would either pass vacuously or refuse
# legitimate writing, and both are worse than an honest scope.
#
# ⚠ NO CHANGELOG IS "COULD NOT RUN", NOT "PASS". A project with no CHANGELOG.md
# has not broken these rules and has not satisfied them either, and reporting
# green over an absent file is how a check quietly stops applying. Exit 2.
#
# ⚠ THE TEMPLATE SKELETON IS EXEMPT. docs/templates/CHANGELOG.md is a file of
# placeholders by design, and a check that fails on a correct tree gets
# switched off within a week.
#
# Usage:
#   sh scripts/common/check-changelog.sh
#   sh scripts/common/check-changelog.sh --json
#   sh scripts/common/check-changelog.sh --file path/to/CHANGELOG.md
#
# Exit codes: 0 clean, 1 a rule was broken, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

set -u

JSON=0
FILE=""

while [ $# -gt 0 ]; do
  case "$1" in
    --json) JSON=1 ;;
    --file) shift; FILE="${1:-}" ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-changelog: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v git >/dev/null 2>&1 || { printf 'check-changelog: git not found\n' >&2; exit 2; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { printf 'check-changelog: not a git repository\n' >&2; exit 2; }
command -v awk >/dev/null 2>&1 || { printf 'check-changelog: awk not found\n' >&2; exit 2; }
REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT" || { printf 'check-changelog: cannot enter %s\n' "$REPO_ROOT" >&2; exit 2; }

[ -n "$FILE" ] || FILE=CHANGELOG.md
if [ ! -f "$FILE" ]; then
  printf 'check-changelog: no %s in this repository.\n' "$FILE" >&2
  printf '  That is "could not run", not "passed": a project with no changelog\n' >&2
  printf '  has neither broken these rules nor satisfied them.\n' >&2
  exit 2
fi

# ⚠ An entry heading is `### `. A section heading is `## `. The check compares
# dates WITHIN a section, because "Unreleased" sitting above "1.0.0" is correct
# and comparing across them would report that as backwards.
# ⛔ STDERR IS NOT SUPPRESSED HERE, AND THAT IS DELIBERATE. An earlier version
# of this block ended with `2>/dev/null`, and the awk program inside it had a
# syntax error: `END` cannot be OR-ed into a pattern expression. awk wrote
# nothing, the problem count parsed as zero, and the check reported a clean
# changelog it had never read. ⭐ A guard that passes vacuously is worse than
# no guard, because it also answers the question "is this checked?" with yes.
# It was caught by comparing the reported entry count against a fixture whose
# entry count was known.
awk '
  function flush(   ) {
    if (!started) return
    if (!has_record) {
      printf "  %s: the entry at line %d names no record. An entry with no record is a claim.\n", FILENAME, entry_line
      problems++
    }
    if (!has_deploy) {
      printf "  %s: the entry at line %d does not say whether it deployed. Silence is not an answer.\n", FILENAME, entry_line
      problems++
    }
    started = 0
  }
  /^## / {
    flush()
    # ⚠ prev resets per SECTION. "Unreleased" above "1.0.0" is correct, and
    # comparing dates across a section boundary would report that as backwards.
    prev = ""
    next
  }
  /^### / {
    flush()
    n_entries++
    entry_line = NR
    # Rule 2: a date. A full ISO 8601 UTC stamp and a bare date both order, and
    # both are accepted; no date is not. ⚠ The stamp form sorts correctly as a
    # string only because ISO 8601 is designed to, which is why the date format
    # is a rule rather than a preference.
    if (match($0, /[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9](T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]Z)?/)) {
      d = substr($0, RSTART, RLENGTH)
    } else {
      printf "  %s:%d no date in the heading. Nothing can order it.\n", FILENAME, NR
      problems++
      d = ""
    }
    # Rule 1: newest first, within the section.
    if (d != "" && prev != "" && d > prev) {
      printf "  %s:%d out of order: %s comes after %s. Newest first.\n", FILENAME, NR, d, prev
      problems++
    }
    if (d != "") prev = d
    has_record = 0; has_deploy = 0; started = 1
    next
  }
  started {
    low = tolower($0)
    if (low ~ /record:/) has_record = 1
    if (low ~ /deploy/)  has_deploy = 1
  }
  END { flush(); print "ENTRIES " n_entries+0; print "PROBLEMS " problems+0 }
' "$FILE" > "${TMPDIR:-/tmp}/.ccl.$$"

OUT="${TMPDIR:-/tmp}/.ccl.$$"
COUNT=$(awk '/^PROBLEMS /{print $2}' "$OUT")
ENTRIES=$(awk '/^ENTRIES /{print $2}' "$OUT")
BODY=$(grep -v '^PROBLEMS \|^ENTRIES ' "$OUT" || true)
rm -f "$OUT"
[ -n "${COUNT:-}" ] || COUNT=0
[ -n "${ENTRIES:-}" ] || ENTRIES=0

if [ "$JSON" = "1" ]; then
  printf '{"schema":"check-changelog/1","problems":%s,"entries":%s}\n' "$COUNT" "$ENTRIES"
  [ "$COUNT" -gt 0 ] && exit 1
  exit 0
fi

if [ "$COUNT" -gt 0 ]; then
  printf 'changelog check failed, %s problem(s):\n\n%s\n\n' "$COUNT" "$BODY"
  printf 'The rules are in docs/conventions/docs.md. ⛔ Fix the entry; do not\n'
  printf 'reorder the whole file in the commit that adds to it. Tidying is its\n'
  printf 'own commit, or both become unreviewable.\n'
  exit 1
fi

printf 'changelog ok: %s entries, in order, each dated with a record and a deploy line\n' "$ENTRIES"
exit 0
