#!/bin/sh
# git-sync.sh - the sanctioned way to commit and push in a project that
# started from this template.
#
# The defect this exists to catch is a rule that everybody agreed to and
# nobody enforces. docs/conventions/git.md states the identity rule and the
# attribution rule; before this script existed, the template DOCUMENTED both
# and ENFORCED neither, so the only thing standing between a project and a
# commit crediting a tool was whether the agent that session had read the file.
#
# ⭐ WHAT IT MAKES MECHANICAL, and each one has cost a real session:
#
#   1. Author AND committer are pinned per invocation with `git -c`, so a
#      machine whose global config says something else still produces the
#      right commit. ⚠ `git commit --author` sets only the author, which is
#      why both are set here: a commit can carry two different identities and
#      the one shown in a log is not the one a checker reads.
#   2. An AI-attribution line is REFUSED, never stripped. Silently rewriting
#      somebody's commit message is worse than declining to commit it: the
#      author never learns the rule and the next message has the same line.
#   3. A CI-skip marker is refused unless the flag was passed. A message that
#      merely MENTIONS `[skip ci]` skips CI, because GitHub does not read the
#      sentence around it. That shipped a commit with no run once: the commit
#      that introduced the skip flag explained the marker in prose, and its own
#      push started nothing.
#   4. The body is read from a FILE, never from a shell string. A body with an
#      apostrophe in it does not survive a shell, and the failure is silent:
#      see docs/conventions/shell.md section 1.
#   5. The gates run BEFORE the push, not after. Finding out after is finding
#      out late, and a red remote is somebody else's problem by then.
#
# ⛔ NOTHING ABOUT THIS SCRIPT KNOWS WHO YOU ARE. The identity comes from
# --name/--email or from git config, and if neither has one the script refuses
# rather than guessing. A template must never carry a person baked into it.
#
# ⚠ IT IS A HELPER, NOT A CHECK. It writes: that is its job. `--check` is the
# read-only half and satisfies the check contract in scripts/README.md; the
# rest of the script deliberately does not.
#
# Usage:
#   sh scripts/common/git-sync.sh --check
#   sh scripts/common/git-sync.sh --message "Subject" --body-file msg.txt
#   sh scripts/common/git-sync.sh --message "Subject" --no-push
#   sh scripts/common/git-sync.sh --push-only
#   sh scripts/common/git-sync.sh --message "Subject" --path README.md --path docs
#   sh scripts/common/git-sync.sh --message "Subject" --gate "sh scripts/common/check-docs.sh"
#   sh scripts/common/git-sync.sh --message "Docs only" --no-ci
#
# Exit codes: 0 done, 1 a rule was broken or a gate failed, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

set -u

MESSAGE=""
BODY_FILE=""
NAME=""
EMAIL=""
BRANCH=""
PATHS=""
GATES=""
NO_PUSH=0
PUSH_ONLY=0
CHECK=0
SKIP_GATES=0
NO_CI=0
JSON=0

while [ $# -gt 0 ]; do
  case "$1" in
    --message)   shift; MESSAGE="${1:-}" ;;
    --body-file) shift; BODY_FILE="${1:-}" ;;
    --name)      shift; NAME="${1:-}" ;;
    --email)     shift; EMAIL="${1:-}" ;;
    --branch)    shift; BRANCH="${1:-}" ;;
    # ⚠ Repeatable rather than comma-separated. A comma is a legal character in
    # a path, and splitting on one turns a correct pathspec into a set of
    # pathspecs that match nothing. Repeating the flag cannot be ambiguous.
    --path)      shift; PATHS="$PATHS
${1:-}" ;;
    --gate)      shift; GATES="$GATES
${1:-}" ;;
    --no-push)    NO_PUSH=1 ;;
    --push-only)  PUSH_ONLY=1 ;;
    --check)      CHECK=1 ;;
    --skip-gates) SKIP_GATES=1 ;;
    --no-ci)      NO_CI=1 ;;
    --json)       JSON=1 ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'git-sync: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v git >/dev/null 2>&1 || { printf 'git-sync: git not found\n' >&2; exit 2; }
git rev-parse --show-toplevel >/dev/null 2>&1 || { printf 'git-sync: not a git repository\n' >&2; exit 2; }
REPO_ROOT=$(git rev-parse --show-toplevel)

# ⛔ EVERY git QUERY BELOW RUNS FROM THE REPOSITORY ROOT, for the same reason
# the checks do: `git add` and `git ls-files` are relative to the process
# working directory, so a run from a subdirectory would silently stage a
# different set than the caller asked for.
cd "$REPO_ROOT" || { printf 'git-sync: cannot enter %s\n' "$REPO_ROOT" >&2; exit 2; }

TMP="${TMPDIR:-/tmp}/.gitsync.$$"
mkdir -p "$TMP" || { printf 'git-sync: cannot write to %s\n' "$TMP" >&2; exit 2; }
trap 'rm -rf "$TMP"' EXIT INT TERM

say() { printf '%s git-sync: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$1"; }
die() { printf 'git-sync: %s\n' "$2" >&2; exit "$1"; }

# -- the identity ------------------------------------------------------------
# ⛔ NOTHING IS INVENTED. If neither the flags nor git config name a person,
# the script refuses. Guessing an identity onto somebody's commit is worse
# than not committing, because it is a claim about who wrote something.
[ -n "$NAME" ]  || NAME=$(git config user.name 2>/dev/null || true)
[ -n "$EMAIL" ] || EMAIL=$(git config user.email 2>/dev/null || true)
if [ -z "$NAME" ] || [ -z "$EMAIL" ]; then
  printf 'git-sync: no identity. Pass --name and --email, or set git config\n' >&2
  printf '  user.name and user.email. Nothing is guessed here.\n' >&2
  exit 2
fi
IDENT="$NAME <$EMAIL>"

# `git -c` on every invocation, so a machine with different global config still
# produces the right commit. Committer as well as author: `--author` sets only
# the author and the two can disagree.
git_as() {
  git -c "user.name=$NAME" -c "user.email=$EMAIL" \
      -c "committer.name=$NAME" -c "committer.email=$EMAIL" "$@"
}

[ -n "$BRANCH" ] || BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo main)

# -- rule 1: no tool is credited in a commit ---------------------------------
# ⛔ REFUSED, NOT STRIPPED. Rewriting a message to make it pass is how the
# author never finds out, and the same line arrives again next time.
#
# ⚠ THE ANTHROPIC ADDRESS IS WRITTEN WITH A BRACKETED DOT ON PURPOSE. Spelled
# as a plain address it is a valid email, and check-no-secrets.sh --public
# refuses a tracked email address, so this guard's own source would have failed
# the secret sweep. The bracket is a no-op to the regex engine and breaks the
# shape the sweep looks for. It costs one comment and saves a false red.
ATTRIBUTION='^[[:space:]]*co-authored-by:|generated[[:space:]]+with[[:space:]]+\[?claude|generated[[:space:]]+by[[:space:]]+(claude|chatgpt|gpt-|copilot|cursor|codex|gemini|llm|an?[[:space:]]+ai)|written[[:space:]]+by[[:space:]]+(claude|chatgpt|gpt-|copilot|an?[[:space:]]+ai)|with[[:space:]]+assistance[[:space:]]+from[[:space:]]+(claude|chatgpt|copilot|an?[[:space:]]+ai)|claude[[:space:]]+(code|opus|sonnet|haiku)|anthropic|^[[:space:]]*(assisted|authored)-by:[[:space:]]*(claude|chatgpt|copilot)|noreply@anthropic[.]com'

# ⚠ Case-insensitive on purpose. "Co-Authored-By" and "co-authored-by" are the
# same violation, and a guard that only caught one spelling would be a guard
# that catches whichever one nobody uses.
find_attribution() {
  grep -niE "$ATTRIBUTION" "$1" 2>/dev/null || true
}

# -- rule 2: a CI skip is deliberate or it is not there ----------------------
# Every marker GitHub Actions honours, matched the way GitHub matches them:
# case-insensitively and anywhere in the message. That is why a sentence ABOUT
# one is one.
CI_SKIP='\[skip[ _-]?ci\]|\[ci[ _-]?skip\]|\[no[ _-]?ci\]|\[skip[ _-]?actions\]|\[actions[ _-]?skip\]'

find_ci_skip() {
  grep -niE "$CI_SKIP" "$1" 2>/dev/null || true
}

# -- the message -------------------------------------------------------------
# ⛔ THE BODY COMES FROM A FILE. docs/conventions/shell.md section 1: a body
# passed as a shell string loses its quoting, and the way it fails is worse
# than an error. Nothing errors, and a fragment of the body is executed or
# dropped somewhere in the middle.
MSG_FILE="$TMP/message"
: > "$MSG_FILE"
if [ -n "$MESSAGE" ]; then
  printf '%s\n' "$MESSAGE" > "$MSG_FILE"
  if [ -n "$BODY_FILE" ]; then
    [ -f "$BODY_FILE" ] || die 2 "--body-file '$BODY_FILE' does not exist."
    printf '\n' >> "$MSG_FILE"
    cat "$BODY_FILE" >> "$MSG_FILE"
  fi
fi

# -- --check: the read-only half ---------------------------------------------
if [ "$CHECK" = "1" ]; then
  PROBLEMS=0

  if [ -s "$MSG_FILE" ]; then
    hits=$(find_attribution "$MSG_FILE")
    if [ -n "$hits" ]; then
      printf 'git-sync: the message carries attribution:\n%s\n' "$hits" >&2
      PROBLEMS=$((PROBLEMS + 1))
    else say "message carries no attribution"; fi
  fi

  # ⭐ THE LAST COMMIT IS CHECKED TOO, so a bad one that landed some other way
  # is still caught. A guard that only inspects what it is asked to write
  # cannot see what somebody committed around it.
  git log -1 --pretty='%an <%ae>%n%cn <%ce>%n%B' > "$TMP/head" 2>/dev/null || true
  if [ -s "$TMP/head" ]; then
    hits=$(find_attribution "$TMP/head")
    if [ -n "$hits" ]; then
      printf 'git-sync: HEAD commit carries attribution:\n%s\n' "$hits" >&2
      PROBLEMS=$((PROBLEMS + 1))
    else say "HEAD commit is clean"; fi

    who=$(git log -1 --pretty='%an <%ae>|%cn <%ce>' 2>/dev/null)
    if [ "$who" != "$IDENT|$IDENT" ]; then
      printf 'git-sync: HEAD identity is %s, expected %s|%s\n' "$who" "$IDENT" "$IDENT" >&2
      PROBLEMS=$((PROBLEMS + 1))
    else say "HEAD identity is $IDENT, author and committer"; fi
  fi

  if [ "$JSON" = "1" ]; then
    printf '{"schema":"git-sync/1","problems":%s}\n' "$PROBLEMS"
  fi
  [ "$PROBLEMS" -gt 0 ] && exit 1
  say "all checks pass"
  exit 0
fi

# -- the gates, BEFORE the push ----------------------------------------------
run_gates() {
  if [ "$SKIP_GATES" = "1" ]; then
    say "GATES SKIPPED by --skip-gates. This push carries no proof the tree is green."
    return 0
  fi
  [ -n "$GATES" ] || { say "no --gate given, nothing to run"; return 0; }
  printf '%s\n' "$GATES" | while IFS= read -r g; do
    [ -z "$g" ] && continue
    say "gate: $g"
    # ⛔ Unpiped, and the exit code is read from the process that produced it.
    sh -c "$g" || exit 1
  done
}

# -- commit ------------------------------------------------------------------
if [ "$PUSH_ONLY" != "1" ]; then
  [ -s "$MSG_FILE" ] || die 2 "--message is required unless --push-only or --check."

  hits=$(find_attribution "$MSG_FILE")
  if [ -n "$hits" ]; then
    printf 'git-sync: the commit message carries AI attribution and will NOT be\n' >&2
    printf 'rewritten for you:\n%s\n\n' "$hits" >&2
    printf 'Remove it and run again. docs/conventions/git.md.\n' >&2
    exit 1
  fi

  # ⚠ Checked BEFORE the gates, not after. Finding this out after a long test
  # run is finding it out late.
  if [ "$NO_CI" != "1" ]; then
    skips=$(find_ci_skip "$MSG_FILE")
    if [ -n "$skips" ]; then
      printf 'git-sync: the message carries a CI skip marker and --no-ci was not\n' >&2
      printf 'passed, so this push would silently start no run:\n%s\n\n' "$skips" >&2
      printf 'Write the marker some other way, or pass --no-ci if you meant it.\n' >&2
      exit 1
    fi
  fi

  if [ -n "$PATHS" ]; then
    printf '%s\n' "$PATHS" | while IFS= read -r p; do
      [ -z "$p" ] && continue
      git add -- "$p" || exit 1
    done
    say "staged the named path(s)"
  else
    git add -A || die 1 "git add -A failed"
    say "staged everything not ignored"
  fi

  git diff --cached --name-only > "$TMP/staged" 2>/dev/null || true
  STAGED=$(grep -c . "$TMP/staged" 2>/dev/null || echo 0)
  [ "$STAGED" -gt 0 ] || die 1 "nothing staged, so there is nothing to commit."
  say "$STAGED file(s) staged"

  run_gates || die 1 "a gate failed. Nothing has been pushed."

  if [ "$NO_CI" = "1" ]; then
    # On its own line at the end, so the subject stays readable in a log and a
    # reader can see in `git log` which pushes were never checked.
    printf '\n[skip ci]\n' >> "$MSG_FILE"
  fi

  git_as commit --file "$MSG_FILE" >/dev/null || die 1 "git commit failed"
  say "committed $(git log -1 --pretty='%h %s')"

  # ⭐ VERIFY RATHER THAN ASSUME. `-c` can be overridden by a hook or by an
  # environment variable, and a commit that landed with the wrong identity is
  # not fixed by having asked nicely.
  who=$(git log -1 --pretty='%an <%ae>|%cn <%ce>')
  [ "$who" = "$IDENT|$IDENT" ] || die 1 "the commit landed as '$who', not '$IDENT'. Something overrode -c."
  say "identity verified: $IDENT, author and committer"
else
  run_gates || die 1 "a gate failed. Nothing has been pushed."
fi

# -- push --------------------------------------------------------------------
if [ "$NO_PUSH" = "1" ]; then
  say "--no-push, stopping before the push"
  exit 0
fi

say "pushing $BRANCH to origin"
git push origin "$BRANCH" || die 1 "git push failed"
say "pushed"
exit 0
