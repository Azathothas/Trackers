#!/bin/sh
# check-no-secrets.sh - does any file in this tree carry something that must
# not be published?
#
# ⚠ THE SCOPE IS TRACKED PLUS UNTRACKED-BUT-NOT-IGNORED, not tracked alone.
# `git ls-files` cannot see a file that has never been staged, which is exactly
# when a new file is most likely to carry a credential and exactly what the
# next `git add -A` would take. This header said "tracked" for longer than the
# code did.
#
# The defect this exists to catch is a credential, or a fingerprint of a private
# system, reaching a remote. Once it does, a history rewrite does not undo it:
# the value was readable, and it may be cached, mirrored or already indexed.
# Rotation is the fix; this is what stops it needing one.
#
# ⛔ IT FINDS THE SHAPES IT KNOWS, AND A GREEN RUN IS NOT A CLEARANCE.
# It cannot find a password that looks like a word, a hostname that reads as
# prose, or a page of correct-looking examples that happens to describe a real
# system. It narrows the reading. It does not replace it.
#
# Usage:
#   sh scripts/common/check-no-secrets.sh              tracked + untracked
#   sh scripts/common/check-no-secrets.sh --public     also the fingerprint rules
#   sh scripts/common/check-no-secrets.sh --json
#   sh scripts/common/check-no-secrets.sh --all-history   ⚠ slow; scans every blob
#
# --public adds the rules that only matter for a repository that is or will be
# public: emails, absolute home paths, long hex identifiers. In a private
# project those are legitimate content, which is why they are not the default.
#
# Exit codes: 0 nothing found, 1 something found, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped. Piping it into anything
# reports the pipeline's status, so a run that failed reads as green.

set -u

PUBLIC=0
JSON=0
HISTORY=0

while [ $# -gt 0 ]; do
  case "$1" in
    --public)      PUBLIC=1 ;;
    --json)        JSON=1 ;;
    --all-history) HISTORY=1 ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-no-secrets: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v git >/dev/null 2>&1 || { printf 'check-no-secrets: git not found\n' >&2; exit 2; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { printf 'check-no-secrets: not a git repository\n' >&2; exit 2; }
SELF=check-no-secrets
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
# shellcheck disable=SC2120
# It takes optional pathspecs. Every caller here wants the whole tree, so none
# passes any; the parameter exists so a scoped caller can be added without
# changing the function.
list_files() {
  {
    git ls-files -- "$@" 2>/dev/null
    git ls-files --others --exclude-standard -- "$@" 2>/dev/null
  } | sort -u
}


FOUND=0
REPORT=""

hit() {
  FOUND=$((FOUND + 1))
  REPORT="$REPORT
== $1 ==
$2"
}

# --- 1. a credential FILE is tracked -----------------------------------------
# The strongest signal there is: not a value that looks like a secret, but a
# file whose whole purpose is to hold one.
CREDS=$(list_files \
  | grep -E '(^|/)(\.env(\..+)?|\.dev\.vars(\..+)?|.*\.(pem|key|p12|pfx|keystore|jks)|id_rsa|id_ed25519|id_ecdsa|credentials\.json|service-account.*\.json)$' \
  | grep -vE '\.example$|\.sample$|\.template$' || true)
[ -n "$CREDS" ] && hit "a credential file is tracked" "$CREDS"

# --- 2. secret-shaped strings ------------------------------------------------
# Each pattern is a vendor's documented token shape. A generic "high entropy"
# rule is deliberately absent: it fires on hashes, minified code and base64
# fixtures, and a check that cries wolf is a check somebody switches off.
scan() {
  _s_name="$1"; _s_pat="$2"
  _s_out=$(list_files | tr '\n' '\0' | xargs -0 grep -nIE "$_s_pat" 2>/dev/null || true)
  [ -n "$_s_out" ] && hit "$_s_name" "$_s_out"
}

scan "a private key block"      'BEGIN (RSA |OPENSSH |EC |DSA |PGP )?PRIVATE KEY'
scan "an aws access key id"     'AKIA[0-9A-Z]{16}'
scan "a github token"           'gh[pousr]_[A-Za-z0-9]{30,}'
scan "a slack token"            'xox[abprs]-[0-9A-Za-z-]{10,}'
scan "a google api key"         'AIza[0-9A-Za-z_-]{35}'
scan "a stripe key"             'sk_(live|test)_[0-9A-Za-z]{16,}'
scan "a npm token"              'npm_[A-Za-z0-9]{36}'
scan "a bearer literal"         'Bearer [A-Za-z0-9._-]{24,}'
scan "a password in a url"      '://[A-Za-z0-9._%+-]+:[^@/[:space:]]{6,}@'

# --- 3. public-only: fingerprints of a private system ------------------------
if [ "$PUBLIC" = "1" ]; then
  scan "an email address"       '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'

  # ⚠ Narrowed, not switched off. A pinned GitHub Action is a 40-hex commit on
  # a PUBLIC repository, and pinning is the SAFE practice this template asks
  # for: a tag moves and a moved tag runs unreviewed code. A rule that fires on
  # correct hardening is a rule somebody disables, so the `uses:` form is
  # excluded by shape rather than the whole hex rule being dropped.
  #
  # ⚠ A DECLARED PIN is the second such shape: a commit and a SHA-256 written
  # into a script that fetches and verifies code before executing it, so 40 hex
  # and 64 hex, both public by construction, both the SAFE practice.
  # ⚠ THE WRAPPER THAT FIRST PRODUCED THIS SHAPE HAS LEFT THIS TREE, and the
  # exclusion stays because docs/containers.md still tells a project to write
  # one. It is an exclusion for a shape this template TEACHES, not for a file
  # it ships, which is the distinction that keeps it from being a dead
  # exemption granting itself to whatever lands there next.
  # ⛔ Excluded by NAME, narrowly. The hex has to be assigned to an identifier
  # that says it is a pin, because a credential is not assigned to something
  # called PinnedSha256. Widening this to all hex would remove the rule.
  _hex_out=$(list_files | tr '\n' '\0' \
    | xargs -0 grep -nIE '\b[0-9a-f]{24,}\b' 2>/dev/null \
    | grep -vE 'uses:[[:space:]]*[A-Za-z0-9._-]+/[A-Za-z0-9._-]+@[0-9a-f]{40}' \
    | grep -vE '[Pp]inned(Ref|Sha256|Commit|Digest)|PINNED_(REF|SHA256)' || true)
  [ -n "$_hex_out" ] && hit "a long hex identifier" "$_hex_out"
  # ⚠ Narrowed rather than switched off. `/home/linuxbrew/` and `/home/runner/`
  # are well-known generic paths, not a fingerprint of anybody's machine, and a
  # check that fires on them is one somebody disables. Whenever this produces a
  # false positive, add the generic path here; do not widen the exclusion to
  # the whole rule.
  _home_out=$(list_files | tr '\n' '\0' \
    | xargs -0 grep -nIE '([A-Za-z]:[\\/]Users[\\/]|/home/|/Users/)[A-Za-z0-9._-]+' 2>/dev/null \
    | grep -vE '/home/(linuxbrew|runner|user|vagrant|ubuntu|node)/' \
    | grep -vE '/Users/(runner|user)/' || true)
  [ -n "$_home_out" ] && hit "an absolute home path" "$_home_out"
fi

# --- 4. the whole history, on request ----------------------------------------
# ⚠ Slow. It reads every blob ever committed, which on a large repository is
# minutes rather than seconds. Worth running once before a repository is first
# published, and not on every commit.
if [ "$HISTORY" = "1" ]; then
  _h_out=$(git rev-list --objects --all 2>/dev/null \
    | git cat-file --batch-check='%(objecttype) %(objectname) %(rest)' 2>/dev/null \
    | awk '$1 == "blob" { print $2, $3 }' \
    | while read -r sha path; do
        if git cat-file blob "$sha" 2>/dev/null \
           | grep -qIE 'BEGIN (RSA |OPENSSH |EC |DSA |PGP )?PRIVATE KEY|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{30,}'; then
          printf '%s  %s\n' "$sha" "$path"
        fi
      done)
  [ -n "$_h_out" ] && hit "a secret shape in history (rotate first, then decide about the history)" "$_h_out"
fi

# --- report -------------------------------------------------------------------
if [ "$JSON" = "1" ]; then
  printf '{"schema":"check-no-secrets/1","findings":%s,"public_rules":%s,"history_scanned":%s}\n' \
    "$FOUND" \
    "$([ "$PUBLIC" = 1 ] && echo true || echo false)" \
    "$([ "$HISTORY" = 1 ] && echo true || echo false)"
  [ "$FOUND" -gt 0 ] && exit 1
  exit 0
fi

if [ "$FOUND" -gt 0 ]; then
  printf '%s\n\n' "$REPORT"
  printf '⛔ %s category/categories matched.\n\n' "$FOUND"
  printf 'If any of it is a real credential, IN THIS ORDER:\n'
  printf '  1. ROTATE IT. Now, before anything else. It is compromised from the\n'
  printf '     moment it was written, and removing the file does not change that.\n'
  printf '  2. Tell the operator. They own the account.\n'
  printf '  3. Remove it from the tree, and add the ignore rule.\n'
  printf '  4. A history rewrite is the operator%s call and the operator%s action.\n' "'s" "'s"
  printf '     It is tidying after the fix, not the fix.\n\n'
  printf 'If it is a false positive, narrow the pattern in this script rather than\n'
  printf 'switching the check off. See docs/security/secrets.md.\n'
  exit 1
fi

printf 'no secret shapes found in %s files (tracked plus untracked-not-ignored)' "$(list_files | wc -l | tr -d ' ')"
[ "$PUBLIC" = "1" ] && printf ' (public rules included)'
printf '\n'
printf '⚠ This finds the shapes it knows. It is not a clearance: read the diff.\n'
exit 0
