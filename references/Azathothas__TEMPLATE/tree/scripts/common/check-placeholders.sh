#!/bin/sh
# check-placeholders.sh - did a template placeholder survive into a real file?
#
# The defect this exists to catch is a document that reads as finished and is
# not. A leftover {{PLACEHOLDER}} in a router, a record or a licence is a
# sentence that looks authoritative and says nothing, and the next session acts
# on it. The failure is quiet: nothing errors, and the file is the right shape.
#
# It also catches the other half, which is easier to miss: a template GUIDANCE
# comment left in a real file. Those read as instructions and are addressed to
# whoever was filling the file in, not to whoever is reading it now.
#
# Run it at the end of a bootstrap, and as a gate afterwards.
#
# Usage:
#   sh scripts/common/check-placeholders.sh
#   sh scripts/common/check-placeholders.sh --json
#   sh scripts/common/check-placeholders.sh --path docs
#
# Exit codes: 0 clean, 1 something survived, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

set -u

JSON=0
SCOPE=""

while [ $# -gt 0 ]; do
  case "$1" in
    --json) JSON=1 ;;
    --path) shift; SCOPE="${1:-}" ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-placeholders: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v git >/dev/null 2>&1 || { printf 'check-placeholders: git not found\n' >&2; exit 2; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { printf 'check-placeholders: not a git repository\n' >&2; exit 2; }
SELF=check-placeholders
REPO_ROOT=$(git rev-parse --show-toplevel)

# ⛔ EVERY git QUERY BELOW RUNS FROM THE REPOSITORY ROOT. `git ls-files` is
# relative to the process working directory, so without this a run from a
# subdirectory silently scopes itself to that subtree and reports clean over
# everything else. The scope of a guard must not depend on who called it.
cd "$REPO_ROOT" || { printf '%s: cannot enter %s\n' "$SELF" "$REPO_ROOT" >&2; exit 2; }

# ⛔ TRACKED **PLUS UNTRACKED-BUT-NOT-IGNORED**. `git ls-files` alone cannot see
# a file that has never been staged, which is exactly when a new file is most
# likely to carry a defect and exactly what the next `git add -A` will take.
# Ignored files stay out: they are ignored on purpose.
list_files() {
  {
    git ls-files -- "$@" 2>/dev/null
    git ls-files --others --exclude-standard -- "$@" 2>/dev/null
  } | sort -u
}


# ⚠ THE TEMPLATE DIRECTORY IS EXEMPT AND MUST BE. Its whole job is to hold
# placeholders, so a check that failed on it would fail on a correct tree, and
# a check that fails on a correct tree gets switched off within a week.
# The exemption is by path rather than by content: a file with placeholders
# ANYWHERE ELSE is the defect.
# ⛔ BOTH implementations are exempt, because each one contains the patterns
# it looks for. Exempting only one is how the twins disagree, and it did: the
# sh side scanned the new ps1 twin and reported four categories the ps1 side
# did not.
#
# -- ⛔ AND THE TEMPLATE EXEMPTION IS CONDITIONAL. HERE IS WHY -------------
#
# A directory-shaped exemption inherited by a project grants itself to whatever
# lands in that directory. A project built from this template copied
# docs/templates/ across whole, with every double-brace marker unfilled, and
# this check reported the tree clean for as long as that was true, because the
# exemption came with the directory. Its own maintainer filed it as a defect.
#
# ⭐ REPRODUCED HERE ON 2026-08-30, on a fixture that is that project's tree:
# docs/templates/ kept, bootstrap/ deleted, one real README. The unconditional
# version prints "no placeholders survived in 1 files" and exits 0. This
# version reports two categories over the same tree, names the directory, and
# exits 1. Both halves agree on it.
#
# ⭐ SO THE EXEMPTION LASTS EXACTLY AS LONG AS bootstrap/ DOES. During a
# bootstrap the skeletons are being read from and must not fail; bootstrap/
# BOOTSTRAP.md step 7 deletes both in one command; and the moment the bootstrap
# is over the skeletons are scanned like any other file. A project that kept
# them fails at its first gate instead of at the moment somebody believes one.
#
# ⚠ bootstrap/BOOTSTRAP.md is the marker rather than the directory, because an
# empty bootstrap/ is not tracked by git and a stray one is not evidence of
# anything.
if [ -f "$REPO_ROOT/bootstrap/BOOTSTRAP.md" ]; then
  EXEMPT='^(docs/templates/|dotfiles/|bootstrap/|scripts/common/check-placeholders\.(sh|ps1))'
  TEMPLATES_EXEMPT=1
else
  EXEMPT='^(dotfiles/|scripts/common/check-placeholders\.(sh|ps1))'
  TEMPLATES_EXEMPT=0
fi

if [ -n "$SCOPE" ]; then
  FILES=$(list_files "$SCOPE" | grep -Ev "$EXEMPT" || true)
else
  FILES=$(list_files | grep -Ev "$EXEMPT" || true)
fi

if [ -z "$FILES" ]; then
  printf 'check-placeholders: no files in scope\n' >&2
  exit 2
fi

COUNT=0
REPORT=""

# 1. A double-brace placeholder.
# ⚠ `${{ }}` is GitHub Actions expression syntax, not a placeholder. A rule that
#    fires on it fires on every correct workflow file, and a rule that fires on
#    correct files gets switched off within a week.
# shellcheck disable=SC2016
# The single quotes are deliberate: the literal characters are wanted here, not
# an expansion of them.
# ⚠ `{{.Field}}` is a GO TEMPLATE, not a placeholder. `podman info --format
#    '{{.Host.Arch}}'` and every `docker inspect --format` string has that
#    shape, and this rule fired on one the day a script using it arrived.
#    ⭐ Narrowed rather than switched off, and narrowed on a shape that cannot
#    collide: every placeholder this template ships is a word or a sentence,
#    and every one of them begins with an UPPERCASE letter.
# ⚠ EXCLUDING ONLY `{{.` WAS TOO NARROW, and the gap is not hypothetical: it
#    fired on `podman image inspect --format '{{json .Config.Env}}'` in this
#    repository's own documentation. A Go template calls functions as well as
#    reading fields, so `{{json .X}}`, `{{range .X}}`, `{{printf ...}}`,
#    `{{if .X}}` and `{{end}}` all begin with a lowercase letter instead. The
#    exclusion is therefore "a dot or a lowercase letter", which still cannot
#    collide with a placeholder, and it covers every docker, podman, helm and
#    kubectl format string rather than only field access.
# ⚠ The cost of the wider rule: a placeholder written in lowercase would be
#    missed. None is, the convention is uppercase, and the ⭐ marker rule in
#    docs/conventions/prose.md is the same kind of explicit-list trade.
BRACE=$(printf '%s\n' "$FILES" | tr '\n' '\0' | xargs -0 grep -nI '{{' 2>/dev/null \
  | grep -v '\${{' | grep -vE '\{\{ *[a-z.]' || true)
if [ -n "$BRACE" ]; then
  COUNT=$((COUNT + 1))
  REPORT="$REPORT
== a placeholder survived ==
$BRACE"
fi

# 2. A template guidance comment. It is addressed to whoever was filling the
#    file in, and reads as an instruction to whoever opens it now.
GUIDE=$(printf '%s\n' "$FILES" | tr '\n' '\0' \
  | xargs -0 grep -nIE '<!-- *TEMPLATE|delete this comment|Fill every' 2>/dev/null || true)
if [ -n "$GUIDE" ]; then
  COUNT=$((COUNT + 1))
  REPORT="$REPORT
== a template guidance comment survived ==
$GUIDE"
fi

# 3. The obvious stand-ins. ⚠ Deliberately narrow: these are the ones that mean
#    "somebody meant to change this", not every occurrence of the word example.
#    A rule that fires on example.com in a legitimate sentence is a rule nobody
#    keeps, and example.com is the CORRECT thing to write in a public document.
STAND=$(printf '%s\n' "$FILES" | tr '\n' '\0' \
  | xargs -0 grep -nIE 'YOUR_(NAME|EMAIL|PROJECT|TOKEN)|CHANGEME|<your-|TODO: fill' 2>/dev/null || true)
if [ -n "$STAND" ]; then
  COUNT=$((COUNT + 1))
  REPORT="$REPORT
== a stand-in value survived ==
$STAND"
fi

# 4. OWNER/REPO, but only where it is configuration rather than prose.
# ⚠ It is deliberately NOT in the list above. `OWNER/REPO` is the RECOMMENDED
#    generic for a public document, and docs/public/README.md says so in as many
#    words, so a rule against it everywhere would fire on correct writing. It is
#    a defect only in a file that was meant to be filled in.
OWNERREPO=$(printf '%s\n' "$FILES" | grep -v '\.md$' | tr '\n' '\0' \
  | xargs -0 grep -nIE 'OWNER/REPO' 2>/dev/null || true)
if [ -n "$OWNERREPO" ]; then
  COUNT=$((COUNT + 1))
  REPORT="$REPORT
== OWNER/REPO survived in a configuration file ==
$OWNERREPO"
fi

if [ "$JSON" = "1" ]; then
  printf '{"schema":"check-placeholders/1","categories":%s,"files_scanned":%s}\n' \
    "$COUNT" "$(printf '%s\n' "$FILES" | wc -l | tr -d ' ')"
  [ "$COUNT" -gt 0 ] && exit 1
  exit 0
fi

if [ "$COUNT" -gt 0 ]; then
  printf '%s\n\n' "$REPORT"
  printf '⛔ %s category/categories survived into real files.\n\n' "$COUNT"
  printf 'Each one is a sentence that looks authoritative and says nothing.\n'
  printf 'Fill it in, or delete the section it is in. ⚠ Do not delete the\n'
  printf 'placeholder alone and leave the sentence around it: that produces a\n'
  printf 'claim nobody wrote.\n'
  if [ "$TEMPLATES_EXEMPT" = "0" ] && [ -d "$REPO_ROOT/docs/templates" ]; then
    printf '\n⛔ docs/templates/ IS IN SCOPE HERE, because bootstrap/ has gone.\n'
    printf 'Those are the template'"'"'s own skeletons and this project kept them.\n'
    printf 'Delete the directory: step 5 of the bootstrap is what reads from it\n'
    printf 'and nothing after step 5 has a use for it.\n'
  fi
  exit 1
fi

if [ "$TEMPLATES_EXEMPT" = "1" ]; then
  _exempt_note='docs/templates, dotfiles and bootstrap are exempt'
else
  _exempt_note='dotfiles is exempt; docs/templates is IN SCOPE because bootstrap/ has gone'
fi
printf 'no placeholders survived in %s files (%s)\n' \
  "$(printf '%s\n' "$FILES" | wc -l | tr -d ' ')" "$_exempt_note"
exit 0
