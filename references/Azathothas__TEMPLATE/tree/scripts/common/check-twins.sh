#!/bin/sh
# check-twins.sh - do the two probe implementations still answer the same way?
#
# The defect this exists to catch is DRIFT between two scripts that are supposed
# to do the same job. One gets a fix, a field, a flag; the other does not; and
# six months later one is polished and the other is a barebones copy that nobody
# noticed had fallen behind. The failure is silent, because each one works fine
# on its own host and nobody runs both.
#
# -- ⭐ EVERYTHING IN common/ HAS A TWIN, AND THIS FILE COVERS ALL OF THEM ----
#
# ⛔ A POSIX sh check cannot be assumed to run on Windows. Measured on one
# Windows 11 machine on 2026-08-25, from a native PowerShell session with Git
# Bash off PATH: `sed` was not installed at all, and `sort` resolved to
# PowerShell's own `Sort-Object` alias, which accepts `-u`, compares
# case-insensitively, and over the five values `b A a B a` returned `A b`
# where coreutils returns `A B a b`. ⚠ A missing tool fails loudly; an ALIASED
# one succeeds and answers differently.
#
# ⛔ So the rule is: wherever a twin exists, THIS CHECK covers it. Adding a
# twin without adding it here is how drift starts.
#
# ⚠ THIS HEADER USED TO SAY THE OPPOSITE, that only the probe needed a twin.
# It did not go stale: it shipped in the SAME COMMIT as the section 7 below
# that says the current rule, and it survived a later maintenance pass over
# this very file. So the file whose job is stopping two implementations
# drifting spent its whole life telling its next maintainer not to write the
# second one, with the correction two hundred lines further down. The retired
# wording is in docs/history/twins-and-scripts.md.
#
# -- WHAT DIFFERENCE IS CORRECT ----------------------------------------------
#
# ⚠ Some disagreement is honest and must not be flattened away. Each twin
# reports what ITS OWN host can reach, and on a Windows machine with msys
# installed those genuinely differ: `bash` and `tar` resolve to different
# binaries, `zsh` is on the msys PATH and not the native one, and
# PSScriptAnalyzer is a PowerShell module invisible to sh.
#
# ⭐ So this compares the SHAPE and the FACTS, not the tool-by-tool verdicts:
# the schema, the keys, the host and repo values that describe one machine.
# A machine cannot have two architectures.
#
# Usage:
#   sh scripts/common/check-twins.sh
#   sh scripts/common/check-twins.sh --json
#   sh scripts/common/check-twins.sh --verbose      also list per-tool differences
#
# Exit codes: 0 they agree, 1 they have drifted, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

set -u

JSON=0
VERBOSE=0

while [ $# -gt 0 ]; do
  case "$1" in
    --json) JSON=1 ;;
    --verbose) VERBOSE=1 ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-twins: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

CDPATH=''
export CDPATH
SCRIPT_DIR=$(cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(cd -- "$SCRIPT_DIR/../.." && pwd)

SH_PROBE="$REPO_ROOT/scripts/doctor/doctor.sh"
PS_PROBE="$REPO_ROOT/scripts/doctor/doctor.ps1"

[ -f "$SH_PROBE" ] || { printf 'check-twins: missing %s\n' "$SH_PROBE" >&2; exit 2; }
[ -f "$PS_PROBE" ] || { printf 'check-twins: missing %s\n' "$PS_PROBE" >&2; exit 2; }

# ⚠ A missing interpreter is "could not run", exit 2, not "they disagree",
# exit 1. Those are different facts and a caller has to be able to tell them
# apart: one blocks a merge and the other is a note about the runner.
PWSH=""
for c in pwsh powershell; do
  if command -v "$c" >/dev/null 2>&1; then PWSH="$c"; break; fi
done
[ -n "$PWSH" ] || { printf 'check-twins: no pwsh or powershell on PATH; cannot compare\n' >&2; exit 2; }
command -v jq >/dev/null 2>&1 || { printf 'check-twins: jq not found; cannot compare json\n' >&2; exit 2; }

TMP="${TMPDIR:-/tmp}/.checktwins.$$"
mkdir -p "$TMP" || { printf 'check-twins: cannot write to %s\n' "$TMP" >&2; exit 2; }
trap 'rm -rf "$TMP"' EXIT INT TERM

# ⚠ Run both from the repo root, so `repo.*` describes the same tree.
( cd "$REPO_ROOT" && sh "$SH_PROBE" --json > "$TMP/sh.json" 2> "$TMP/sh.err" ) || {
  printf 'check-twins: doctor.sh failed\n' >&2; cat "$TMP/sh.err" >&2; exit 2; }
( cd "$REPO_ROOT" && "$PWSH" -NoProfile -File "$PS_PROBE" -Json > "$TMP/ps.json" 2> "$TMP/ps.err" ) || {
  printf 'check-twins: doctor.ps1 failed\n' >&2; cat "$TMP/ps.err" >&2; exit 2; }

jq -e . "$TMP/sh.json" >/dev/null 2>&1 || { printf 'check-twins: doctor.sh emitted invalid json\n' >&2; exit 2; }
jq -e . "$TMP/ps.json" >/dev/null 2>&1 || { printf 'check-twins: doctor.ps1 emitted invalid json\n' >&2; exit 2; }

DRIFT=0
note() { printf '  DRIFT  %s\n' "$1"; DRIFT=$((DRIFT + 1)); }
ok()   { [ "$JSON" = "1" ] || printf '  ok     %s\n' "$1"; }

j() { jq -r "$1" "$2" 2>/dev/null; }

# --- 1. the schema string -----------------------------------------------------
s_schema=$(j '.schema' "$TMP/sh.json"); p_schema=$(j '.schema' "$TMP/ps.json")
if [ "$s_schema" = "$p_schema" ]; then ok "schema: $s_schema"
else note "schema: sh=$s_schema ps=$p_schema"; fi

# --- 2. the key sets, per section --------------------------------------------
# ⛔ A field added to one and not the other is the commonest shape of drift,
# and it is invisible to anything that only compares values.
for sec in host repo summary probe; do
  a=$(jq -r --arg s "$sec" '.[$s] | keys_unsorted | sort | join(",")' "$TMP/sh.json" 2>/dev/null)
  b=$(jq -r --arg s "$sec" '.[$s] | keys_unsorted | sort | join(",")' "$TMP/ps.json" 2>/dev/null)
  if [ "$a" = "$b" ]; then ok "$sec keys match"
  else
    note "$sec keys differ"
    printf '           sh: %s\n' "$a"
    printf '           ps: %s\n' "$b"
  fi
done

t=$(jq -r 'keys_unsorted | sort | join(",")' "$TMP/sh.json" 2>/dev/null)
u=$(jq -r 'keys_unsorted | sort | join(",")' "$TMP/ps.json" 2>/dev/null)
if [ "$t" = "$u" ]; then ok "top-level keys match"
else note "top-level keys differ: sh=[$t] ps=[$u]"; fi

# --- 3. the facts about THIS machine -----------------------------------------
# ⚠ These describe one host, so they cannot honestly differ. `flavor` is
# excluded on purpose: it reports the SHELL environment, so msys and native are
# both correct answers from the same machine.
for f in os wsl container arch distro distro_version; do
  a=$(j ".host.$f" "$TMP/sh.json"); b=$(j ".host.$f" "$TMP/ps.json")
  if [ "$a" = "$b" ]; then ok "host.$f = $a"
  else note "host.$f: sh=[$a] ps=[$b]"; fi
done

for f in is_git dirty commits remote_looks_like_template has_codegraph; do
  a=$(j ".repo.$f" "$TMP/sh.json"); b=$(j ".repo.$f" "$TMP/ps.json")
  if [ "$a" = "$b" ]; then ok "repo.$f = $a"
  else note "repo.$f: sh=[$a] ps=[$b]"; fi
done

a=$(jq -r '.repo.ecosystems | sort | join(",")' "$TMP/sh.json" 2>/dev/null)
b=$(jq -r '.repo.ecosystems | sort | join(",")' "$TMP/ps.json" 2>/dev/null)
if [ "$a" = "$b" ]; then ok "repo.ecosystems = [${a:-none}]"
else note "repo.ecosystems: sh=[$a] ps=[$b]"; fi

# --- 4. the tool object's shape ----------------------------------------------
a=$(jq -r '[.tools[0] | keys_unsorted | sort | .[]] | join(",")' "$TMP/sh.json" 2>/dev/null)
b=$(jq -r '[.tools[0] | keys_unsorted | sort | .[]] | join(",")' "$TMP/ps.json" 2>/dev/null)
if [ "$a" = "$b" ]; then ok "tool object shape: $a"
else note "tool object shape: sh=[$a] ps=[$b]"; fi

# --- 5. the probed-tool sets --------------------------------------------------
# ⚠ Not the verdicts. A tool one host can reach and the other cannot is an
# honest difference. What is NOT honest is one twin having forgotten to probe
# something the other does, so the ID SETS are compared and the known
# host-specific extras are named rather than silently tolerated.
jq -r '.tools[].id' "$TMP/sh.json" | sort > "$TMP/sh.ids"
jq -r '.tools[].id' "$TMP/ps.json" | sort > "$TMP/ps.ids"

# Documented, host-specific, and each with a reason. Anything outside this list
# is drift.
cat > "$TMP/allowed" <<'ALLOWED'
psscriptanalyzer
ALLOWED

only_ps=$(comm -13 "$TMP/sh.ids" "$TMP/ps.ids" | grep -vxF -f "$TMP/allowed" || true)
only_sh=$(comm -23 "$TMP/sh.ids" "$TMP/ps.ids" || true)

if [ -z "$only_ps" ] && [ -z "$only_sh" ]; then
  ok "both probe the same $(wc -l < "$TMP/sh.ids" | tr -d ' ') tools"
else
  [ -n "$only_sh" ] && note "probed by doctor.sh only: $(printf '%s' "$only_sh" | tr '\n' ' ')"
  [ -n "$only_ps" ] && note "probed by doctor.ps1 only: $(printf '%s' "$only_ps" | tr '\n' ' ')"
  printf '           If a difference is correct, add the id to the allowed list\n'
  printf '           in this script WITH THE REASON. An unexplained one is drift.\n'
fi

# --- 6. the CLI surface -------------------------------------------------------
# ⛔ A FLAG IN ONE TWIN AND NOT THE OTHER IS DRIFT THE JSON CANNOT SHOW.
# Everything above compares what the probes OUTPUT. Nothing above compares what
# they ACCEPT, and the two are independent: `doctor.sh --text` exited 0 while
# `doctor.ps1 -Text` exited 1 with a parameter-binding error, and every
# comparison in this file passed while that was true. The README's flag table
# listed four flags and the sh probe had five.
#
# ⚠ The names are compared, not the spellings. `--fast` and `-Fast` are the
# same flag: POSIX and PowerShell conventions differ and neither is wrong.
# Help is excluded: PowerShell supplies `-?` itself and there is nothing to
# match it against.
sh_flags=$(awk '
  /^while \[ \$# -gt 0 \]/ { inloop = 1 }
  inloop && /^done/        { exit }
  inloop && match($0, /^[[:space:]]*-[^)]*\)/) {
    s = substr($0, RSTART, RLENGTH - 1)
    gsub(/[[:space:]]/, "", s)
    n = split(s, parts, "|")
    for (i = 1; i <= n; i++) {
      f = parts[i]
      sub(/^-+/, "", f)
      if (f != "h" && f != "help") print tolower(f)
    }
  }
' "$SH_PROBE" | sort -u)

ps_flags=$(awk '
  /^param\(/ { inparam = 1; next }
  inparam && /^\)/ { exit }
  inparam && match($0, /\$[A-Za-z][A-Za-z0-9]*/) {
    f = substr($0, RSTART + 1, RLENGTH - 1)
    print tolower(f)
  }
' "$PS_PROBE" | sort -u)

if [ "$sh_flags" = "$ps_flags" ]; then
  ok "cli flags match: $(printf '%s' "$sh_flags" | tr '\n' ' ')"
else
  note "cli flags differ"
  printf '           sh: %s\n' "$(printf '%s' "$sh_flags" | tr '\n' ' ')"
  printf '           ps: %s\n' "$(printf '%s' "$ps_flags" | tr '\n' ' ')"
  printf '           A flag one twin accepts and the other rejects is drift a\n'
  printf '           schema comparison cannot see. Add it to BOTH.\n'
fi

# --- 7. every OTHER twin pair, compared on this tree --------------------------
# ⛔ THE PROBE IS NO LONGER THE ONLY TWIN, so it is no longer the only thing
# compared. Every check in common/ has a PowerShell implementation, because the
# sh ones cannot run on a Windows host that has no POSIX layer: measured on one
# Windows 11 machine, native PowerShell had no `sed` at all and `sort` resolved
# to PowerShell's own Sort-Object alias rather than the coreutils binary. ⚠ The
# second is the dangerous one: a missing tool fails loudly, an aliased one
# silently returns a different answer.
#
# ⭐ THE COMPARISON IS THE --json OUTPUT AND THE EXIT CODE, both read from the
# process that produced them. Two implementations of one rule agreeing on a
# clean tree proves very little; they are mutation-proven together, which is
# what scripts/README.md asks for.
# ⚠ IT COMPARES ANSWERS ON THIS TREE, NOT THE RULES THEMSELVES. A scope
# difference with nothing in the tree to exercise it is INVISIBLE here:
# dropping `.py` from one twin's extension list changed no number, because this
# repository has no `.py` file. Dropping `.md` was caught instantly. ⭐ So a
# scope rule is proven by adding a fixture that exercises it, not by trusting
# this comparison to notice.
printf '\n  twin pairs, same tree:\n'
compare_pair() {
  _p_name="$1"; _p_sh="$2"; _p_shargs="$3"; _p_ps="$4"; _p_psargs="$5"

  # shellcheck disable=SC2086
  # The argument strings are deliberately word-split: each is a fixed literal
  # written in the table below, never user input.
  # ⛔ RUN UNPIPED, READ THE EXIT CODE, THEN FILTER. Writing this as
  # `check | grep '^{'` and reading $? gives the GREP's status, so a check that
  # exited 1 reads as 0 and a check that exited 2 reads as 1. That is this
  # repository's oldest stated rule and it was broken here, in the file whose
  # job is comparing guards, while writing this very function.
  #
  # ⚠ ONLY THE MACHINE-READABLE LINE IS COMPARED. A pair that also printed
  # timestamped progress reported a disagreement while agreeing exactly, because
  # two runs a second apart are never byte-identical. Comparing the JSON
  # compares the ANSWER; comparing the transcript compares the clock.
  _a_raw=$( cd "$REPO_ROOT" && sh "$REPO_ROOT/scripts/common/$_p_sh" $_p_shargs 2>/dev/null ); ra=$?
  # shellcheck disable=SC2086
  _b_raw=$( cd "$REPO_ROOT" && "$PWSH" -NoProfile -File "$REPO_ROOT/scripts/common/$_p_ps" $_p_psargs 2>/dev/null ); rb=$?
  a=$(printf '%s\n' "$_a_raw" | grep '^{' || true)
  b=$(printf '%s\n' "$_b_raw" | grep '^{' || true)

  if [ "$a" = "$b" ] && [ "$ra" = "$rb" ]; then
    ok "$_p_name: both say $( [ -n "$a" ] && printf '%s' "$a" || printf 'nothing' ), exit $ra"
  else
    note "$_p_name: the twins disagree"
    printf '           sh: exit %s  %s\n' "$ra" "$a"
    printf '           ps: exit %s  %s\n' "$rb" "$b"
    printf '           ⛔ One rule, two answers. Fix BOTH; do not widen this\n'
    printf '           comparison to make the failure go away.\n'
  fi
}

compare_pair "check-docs"           check-docs.sh           "--json"          check-docs.ps1           "-Json"
compare_pair "check-placeholders"   check-placeholders.sh   "--json"          check-placeholders.ps1   "-Json"
compare_pair "check-control-bytes"  check-control-bytes.sh  "--json"          check-control-bytes.ps1  "-Json"
# ⚠ THIS PAIR IS THE ONE MOST WORTH COMPARING AND THE ONE LEAST PROVED BY THE
# COMPARISON. Both halves decode UTF-8 by hand, from opposite directions: the
# sh half walks bytes with an ordinal table and the PowerShell half walks .NET
# chars and has to rejoin a surrogate pair. Two decoders agreeing on a tree
# that contains no character outside the five is two decoders agreeing about
# nothing. ⭐ Prove this one with a planted character, in both halves, the way
# scripts/README.md says to. It was, on U+2014 and on U+1F600.
compare_pair "check-markers"        check-markers.sh        "--json"          check-markers.ps1        "-Json"
compare_pair "check-one-home"       check-one-home.sh       "--json"          check-one-home.ps1       "-Json"
compare_pair "check-changelog"      check-changelog.sh      "--json"          check-changelog.ps1      "-Json"
compare_pair "check-no-secrets"     check-no-secrets.sh     "--json"          check-no-secrets.ps1     "-Json"
compare_pair "check-no-secrets pub" check-no-secrets.sh     "--public --json" check-no-secrets.ps1     "-Public -Json"

# ⭐ mine-repo IS COMPARED THROUGH --selftest, AND THAT IS THE WHOLE POINT.
# This pair used to be excluded, on the reasoning that comparing two miners
# means fetching a live third-party repository twice on every run. That
# reasoning still holds for a FETCH and it never applied to the JOIN, which is
# the part that was wrong: the sh half joined paginated pages by counting
# bracket characters over raw text, dropped every comment body containing a
# markdown link, and printed "ok". A consumer found it, not this check.
#
# ⚠ --selftest touches no network and no credential. There was never a reason
# to leave the joiner uncompared, and the exclusion note that covered the fetch
# had been read as covering the whole script.
compare_pair "mine-repo --selftest" mine-repo.sh            "--selftest --json" mine-repo.ps1          "-SelfTest -Json"

# ⚠ THIS PAIR NEEDS THE NETWORK AND AN AUTHENTICATED gh, and both twins exit 2
# when they do not have them. Two 2s is agreement: it says the pair could not
# run, not that it passed. ⛔ Do not drop the row on a machine with no gh; a
# comparison skipped for convenience is a comparison that stops happening.
compare_pair "check-remote-items"   check-remote-items.sh   "--json"          check-remote-items.ps1   "-Json"

# --- 8. per-tool verdicts, on request ----------------------------------------
if [ "$VERBOSE" = "1" ]; then
  printf '\n  per-tool differences (informational, not drift):\n'
  jq -r '.tools[] | [.id, (.found|tostring), .version] | @tsv' "$TMP/sh.json" | sort > "$TMP/sh.t"
  jq -r '.tools[] | [.id, (.found|tostring), .version] | @tsv' "$TMP/ps.json" | sort > "$TMP/ps.t"
  join -t"$(printf '\t')" "$TMP/sh.t" "$TMP/ps.t" 2>/dev/null \
    | awk -F'\t' '$2 != $4 || $3 != $5 { printf "    %-16s sh=%s/%s  ps=%s/%s\n", $1, $2, $3, $4, $5 }'
fi

# --- report -------------------------------------------------------------------
if [ "$JSON" = "1" ]; then
  printf '{"schema":"check-twins/1","drift":%s}\n' "$DRIFT"
  [ "$DRIFT" -gt 0 ] && exit 1
  exit 0
fi

printf '\n'
if [ "$DRIFT" -gt 0 ]; then
  printf '⛔ the twins have drifted in %s place(s).\n\n' "$DRIFT"
  printf 'A field, a flag or a fact in one and not the other. Fix BOTH, or, if the\n'
  printf 'difference is genuinely host-specific, record it in this script with the\n'
  printf 'reason. ⛔ Do not widen the comparison to make a failure go away: that is\n'
  printf 'how the check stops checking.\n'
  exit 1
fi
printf '✅ every twin pair agrees on this tree.\n'
exit 0
