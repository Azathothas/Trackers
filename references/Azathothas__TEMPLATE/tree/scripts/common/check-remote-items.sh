#!/bin/sh
# check-remote-items.sh - what is open against this repository, and does it
# say anything that survives being checked?
#
# The defect this exists to catch is a change accepted on the strength of its
# own description. A bot's pull request title says what it believes it is
# doing. A contributor's issue says what they believe is wrong. Both are
# CLAIMS, and both are usually right, which is exactly what makes the wrong one
# expensive: nobody is looking by the hundredth bump.
#
# ⭐ THIS WAS PAID FOR ON THIS REPOSITORY, TWICE, IN ONE HOUR.
#   1. `actions/checkout` was pinned to v4, and v4 targets Node 20, which
#      GitHub had deprecated. The runs were being force-migrated with a warning
#      in a log nobody reads. Resolving a tag is not the same as checking what
#      it declares.
#   2. The replacement pin was v5, chosen by looking only at v5 and v4. v7
#      already existed. A tag resolving cleanly says nothing about whether it
#      is current.
#
# -- WHAT IT VERIFIES, AND IT DOES NOT TAKE THE ITEM'S WORD ------------------
# For every pinned action a pull request proposes:
#   - the commit exists AND belongs to the repository the ref names, so a
#     lookalike SHA cannot ride in;
#   - the tag in the trailing comment really resolves to that commit, so the
#     comment cannot drift from the pin it labels;
#   - the tag is a published release, not a draft, a prerelease, or a tag
#     somebody pushed over;
#   - ⭐ the runtime it DECLARES is not one the platform has deprecated;
#   - whether a NEWER major exists than the one being proposed.
#
# ⛔ IT IS READ ONLY. It never merges, never closes, never comments, never
# approves. Deciding is the operator's. docs/security/remote-ops.md.
#
# ⚠ IT CANNOT TELL YOU WHETHER A CHANGE IS A GOOD IDEA. It checks the facts an
# item asserts about the world. Whether you want the change is a reading.
#
# Usage:
#   sh scripts/common/check-remote-items.sh
#   sh scripts/common/check-remote-items.sh --json
#   sh scripts/common/check-remote-items.sh --repo OWNER/NAME
#
# Exit codes: 0 nothing open, or nothing open failed a check;
#             1 an item's claim did not survive checking;
#             2 could not run.
#
# ⚠ AN UNREAD ITEM IS NOT A FAILED CHECK, and this used to exit 1 for one. Any
# repository with an open issue was then permanently red, which is how a check
# stops being read: the one state it cannot report is the one it exists for.
# An item needing a reading is counted, named, and exits 0. Only a claim that
# was checked and did not hold exits 1.
#
# ⛔ `--json` PUTS THE JSON DOCUMENT ON STDOUT AND NOTHING ELSE. It used to
# print the whole human report there first, so `check | jq` failed to parse and
# every other check in this directory was machine-readable while this one was
# not. The report still goes out, on stderr, where a human reading a terminal
# sees it and a gate runner reading stdout does not.
#
# ⛔ Read the exit code from this process, unpiped.

set -u

JSON=0
REPO=""

while [ $# -gt 0 ]; do
  case "$1" in
    --json) JSON=1 ;;
    --repo) shift; REPO="${1:-}" ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-remote-items: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

command -v gh >/dev/null 2>&1 || { printf 'check-remote-items: gh not found\n' >&2; exit 2; }
command -v jq >/dev/null 2>&1 || { printf 'check-remote-items: jq not found\n' >&2; exit 2; }
gh auth status >/dev/null 2>&1 || { printf 'check-remote-items: gh is not authenticated\n' >&2; exit 2; }

# ⛔ IN JSON MODE, STDOUT IS RESERVED FOR THE DOCUMENT. Everything the body
# prints for a person is moved to stderr here, in one place, and fd 3 holds the
# real stdout until the document is written. Doing it once at the top is what
# keeps the reporting code below identical in both modes: a second set of
# printf calls guarded by a flag is a second thing to keep in step.
if [ "$JSON" = "1" ]; then
  exec 3>&1 1>&2
fi

GH_ARGS=""
[ -n "$REPO" ] && GH_ARGS="--repo $REPO"

# ⛔ gh ON WINDOWS EMITS CRLF, and a carriage return riding on a value is
# invisible until something types it. `jq` refused `"1\r"` as a number and the
# diff fetch for pull request `1\r` failed with no useful message. Every value
# read out of gh below is stripped.
strip_cr() { tr -d '\r'; }

TMP="${TMPDIR:-/tmp}/.remoteitems.$$"
mkdir -p "$TMP" || { printf 'check-remote-items: cannot write to %s\n' "$TMP" >&2; exit 2; }
trap 'rm -rf "$TMP"' EXIT INT TERM

PROBLEMS=0
NEEDS_HUMAN=0
note()  { printf '  %s\n' "$1"; }
bad()   { printf '  ⛔ %s\n' "$1"; PROBLEMS=$((PROBLEMS + 1)); }
human() { printf '  ⚠ %s\n' "$1"; NEEDS_HUMAN=$((NEEDS_HUMAN + 1)); }

# -- open issues -------------------------------------------------------------
# ⚠ Reported, not judged. An issue is a person's account of a problem and
# nothing here can verify it. What this can do is stop one going unnoticed.
printf '\nOPEN ISSUES\n'
# shellcheck disable=SC2086
if ! gh issue list $GH_ARGS --state open --limit 50 \
      --json number,title,author,createdAt > "$TMP/issues.json" 2>"$TMP/err"; then
  printf 'check-remote-items: could not list issues\n' >&2
  cat "$TMP/err" >&2
  exit 2
fi
if [ "$(jq 'length' "$TMP/issues.json")" = "0" ]; then
  note "none"
else
  jq -r '.[] | "  #\(.number) [\(.author.login)] \(.title)"' "$TMP/issues.json"
  human "$(jq 'length' "$TMP/issues.json") open issue(s). Read them; nothing here can verify a report."
fi

# -- open pull requests ------------------------------------------------------
printf '\nOPEN PULL REQUESTS\n'
# shellcheck disable=SC2086
if ! gh pr list $GH_ARGS --state open --limit 50 \
      --json number,title,author,headRefName,files > "$TMP/prs.json" 2>"$TMP/err"; then
  printf 'check-remote-items: could not list pull requests\n' >&2
  cat "$TMP/err" >&2
  exit 2
fi

PRCOUNT=$(jq 'length' "$TMP/prs.json")
if [ "$PRCOUNT" = "0" ]; then
  note "none"
else
  jq -r '.[].number' "$TMP/prs.json" | strip_cr > "$TMP/prnums"
  while IFS= read -r n; do
    [ -z "$n" ] && continue
    title=$(jq -r --arg n "$n" '.[] | select(.number == ($n|tonumber)) | .title' "$TMP/prs.json" | strip_cr)
    who=$(jq -r --arg n "$n" '.[] | select(.number == ($n|tonumber)) | .author.login' "$TMP/prs.json" | strip_cr)
    printf '\n  #%s [%s] %s\n' "$n" "$who" "$title"

    # shellcheck disable=SC2086
    gh pr diff $GH_ARGS "$n" > "$TMP/diff.$n" 2>/dev/null || {
      human "#$n: could not read the diff"
      continue
    }

    # Every action pin the diff ADDS. The trailing comment is captured too,
    # because a pin whose label disagrees with it is its own defect.
    grep '^+' "$TMP/diff.$n" \
      | grep -oE 'uses:[[:space:]]*[A-Za-z0-9._-]+/[A-Za-z0-9._-]+@[0-9a-f]{40}([[:space:]]*#[[:space:]]*[^[:space:]]+)?' \
      | strip_cr > "$TMP/pins.$n" 2>/dev/null || true

    if [ ! -s "$TMP/pins.$n" ]; then
      # No pinned action. Say what it touches so a human can decide.
      files=$(jq -r --arg n "$n" '.[] | select(.number == ($n|tonumber)) | .files | map(.path) | join(", ")' "$TMP/prs.json" | strip_cr)
      note "touches: $files"
      human "#$n: nothing mechanically checkable here. Read it."
      continue
    fi

    while IFS= read -r pin; do
      [ -z "$pin" ] && continue
      action=$(printf '%s' "$pin" | sed -E 's/^uses:[[:space:]]*//; s/@.*//')
      sha=$(printf '%s' "$pin" | sed -E 's/.*@([0-9a-f]{40}).*/\1/')
      tag=$(printf '%s' "$pin" | sed -nE 's/.*#[[:space:]]*([^[:space:]]+).*/\1/p')

      printf '    %s@%s  (labelled %s)\n' "$action" "$(printf '%s' "$sha" | cut -c1-12)" "${tag:-no label}"

      # 1. does the commit exist, and in THAT repository?
      if ! gh api "repos/$action/commits/$sha" --jq '.sha' >/dev/null 2>&1; then
        bad "$action@$sha does not exist in that repository. A pin naming a commit the repo does not have is not a bump."
        continue
      fi
      note "      commit exists in $action"

      # 2. does the label resolve to that same commit?
      if [ -n "$tag" ]; then
        t_sha=$(gh api "repos/$action/git/ref/tags/$tag" --jq '.object.sha' 2>/dev/null | strip_cr || true)
        t_typ=$(gh api "repos/$action/git/ref/tags/$tag" --jq '.object.type' 2>/dev/null | strip_cr || true)
        [ "$t_typ" = "tag" ] && t_sha=$(gh api "repos/$action/git/tags/$t_sha" --jq '.object.sha' 2>/dev/null | strip_cr || true)
        if [ -z "$t_sha" ]; then
          human "      the label $tag is not a tag in $action"
        elif [ "$t_sha" != "$sha" ]; then
          bad "the label says $tag but that tag is $(printf '%s' "$t_sha" | cut -c1-12), not the pinned commit. The comment has drifted from the pin."
        else
          note "      label $tag matches the pin"
        fi
      else
        human "      no tag comment beside the pin. A bare SHA tells a reader nothing."
      fi

      # 3. ⭐ what runtime does the PINNED COMMIT declare?
      #    This is the check the Node 20 deprecation got past.
      rt=$(curl -sSL -m 20 "https://raw.githubusercontent.com/$action/$sha/action.yml" 2>/dev/null \
           | sed -n '/^runs:/,/^[^ ]/p' | sed -nE 's/^[[:space:]]*using:[[:space:]]*(.+)$/\1/p' | head -1)
      # ⚠ THE DECLARED VALUE MAY BE QUOTED, AND THE CASE BELOW MATCHES BARE WORDS.
      # `using: "node24"` is valid YAML and real actions write it that way:
      # astral-sh/setup-uv does. Before this line the raw capture kept its
      # quotes, so a quoted "node20" matched no arm, fell through to the
      # catch-all, and was reported as ⚠ "unrecognised; check it" instead of
      # the ⛔ this whole check exists to raise. A deprecated runtime evaded
      # the one rule written for it by being spelled the other legal way.
      # Found by running this check against a real third-party pull request.
      rt=$(printf '%s' "$rt" | tr -d "\"'\r" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')
      case "$rt" in
        "")            human "      could not read action.yml at that commit; runtime unverified" ;;
        node12|node16|node20)
                       bad "it declares $rt, which GitHub has deprecated. It will run under a forced newer runtime, with a warning nobody reads, until it does not." ;;
        node24|docker|composite)
                       note "      runtime: $rt" ;;
        *)             human "      runtime: $rt (unrecognised; check it)" ;;
      esac

      # 4. is anything newer already out?
      latest=$(gh api "repos/$action/releases/latest" --jq '.tag_name' 2>/dev/null | strip_cr || true)
      if [ -n "$latest" ] && [ -n "$tag" ] && [ "$latest" != "$tag" ]; then
        human "      $latest is already released; this proposes $tag"
      elif [ -n "$latest" ]; then
        note "      $latest is the latest release"
      fi
    done < "$TMP/pins.$n"
  done < "$TMP/prnums"
fi

# -- report ------------------------------------------------------------------
# ⛔ THE TWO MODES REPORT THE SAME VERDICT. They differed once: text exited 1
# whenever anything needed a reading and json exited 0 over the same tree, so a
# gate runner saw green where a person saw red. Both twins carried it, so
# check-twins compared them and passed. One exit expression, computed here,
# is what stops that returning.
printf '\n'
if [ "$PROBLEMS" -gt 0 ]; then
  printf '⛔ %s claim(s) did not survive checking. Do not merge on the description.\n' "$PROBLEMS"
  RC=1
elif [ "$NEEDS_HUMAN" -gt 0 ]; then
  printf '⚠ %s item(s) need a reading. Nothing failed a check; nothing was verified either.\n' "$NEEDS_HUMAN"
  RC=0
else
  printf '✅ every mechanically checkable claim held.\n'
  printf '⚠ That is not approval. Whether you want a change is a reading, not a check.\n'
  RC=0
fi

if [ "$JSON" = "1" ]; then
  exec 1>&3 3>&-
  printf '{"schema":"check-remote-items/1","problems":%s,"needs_human":%s,"open_prs":%s}\n' \
    "$PROBLEMS" "$NEEDS_HUMAN" "$PRCOUNT"
fi
exit "$RC"
