#!/bin/sh
# check-gate.sh - run the whole local gate, in one command, and read every
# exit code from the process that produced it.
#
# The defect this exists to catch is a gate that is a LIST. Part (a) of
# docs/methodology/gate.md names several checks, and a list run by hand is run
# in the order somebody recalls it. ⛔ The session that first wrote this ran its
# gate five times and typed a different subset each time. Nothing failed; the
# gate simply was not the same gate twice.
#
# ⭐ IT DELEGATES. It holds no rules of its own and it is not a second opinion
# about anything. Every verdict here is some other script's, read unpiped.
#
# -- ⛔ A SKIPPED CHECK IS A SKIP, NEVER A PASS -----------------------------
#
# `pwsh`, `jq`, `gh` and `shellcheck` are not on every machine. A runner that
# quietly dropped one and printed green would be the row in
# docs/conventions/forbidden-patterns.md that reads *a step that exits 0 having
# done nothing it was asked to do*.
#
# So a skip is counted, named, and printed on its own line. ⚠ The exit code is
# still 0, because a machine that cannot run a check has not failed it; ⭐ pass
# --strict to make a skip a failure, which is what a CI job should do, since
# there the tools are installed on purpose and a skip means the install broke.
#
# -- ⛔ IT DOES NOT RUN ITSELF, AND THAT IS NOT THEORETICAL ------------------
#
# This runs check-twins.sh, which runs both halves of every pair. ⚠ A version
# of this idea in another repository hit an unbounded recursion with
# check-twins that left twenty stray shells holding their own files open. That
# is the reported symptom; the mechanism here is plain enough that it does not
# need re-deriving. A runner that appears in the pair list runs the comparison
# that runs the runner.
#
# So check-gate is NOT in check-twins.sh's pair list, and check-twins is
# invoked here directly rather than through anything that could re-enter.
# ⚠ The two exclusions are a shared contract: removing one reintroduces the
# hang.
#
# Usage:
#   sh scripts/common/check-gate.sh
#   sh scripts/common/check-gate.sh --fast     # skips check-twins
#   sh scripts/common/check-gate.sh --strict   # a skip is a failure
#   sh scripts/common/check-gate.sh --json
#
# Exit codes: 0 nothing failed, 1 something failed, 2 could not run.
#
# ⛔ Read the exit code from this process, unpiped.

set -u

JSON=0
FAST=0
STRICT=0

while [ $# -gt 0 ]; do
  case "$1" in
    --json)   JSON=1 ;;
    --fast)   FAST=1 ;;
    --strict) STRICT=1 ;;
    -h|--help) awk 'NR>1 { if (/^#/) { sub(/^# ?/, ""); print } else exit }' "$0"; exit 0 ;;
    *) printf 'check-gate: unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
  shift
done

# ⛔ RESOLVED FROM THIS SCRIPT'S OWN LOCATION, not from the working directory.
# A runner found by a relative path runs a different set depending on who
# called it, which is the same class of defect as a guard whose scope depends
# on the process working directory.
HERE=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)

PASS=0
FAIL=0
SKIP=0
ROWS=""

row() { ROWS="$ROWS  $1
"; }

# ⛔ THE EXIT CODE IS TAKEN FROM THE PROCESS, UNPIPED. Output goes to a file
# and $? is read on the next line. `run ... | tee` would report tee's status,
# which is 0 whatever the check did, and that is the single defect this whole
# repository is most emphatic about.
OUT="${TMPDIR:-/tmp}/.checkgate.$$"
mkdir -p "$OUT" || { printf 'check-gate: cannot write to %s\n' "$OUT" >&2; exit 2; }
trap 'rm -rf "$OUT"' EXIT INT TERM

# ⚠ NO PRESENCE TEST LIVES HERE. An earlier version tested `$1` after the
# shift, which is the interpreter rather than the script, so every row reported
# "not present" and the runner printed a green verdict having executed nothing.
# ⭐ That is the exact defect this script's header is about, produced by the
# script itself on its first run. Presence is decided by the caller, which is
# the only place that knows the path.
run() {   # name  command...
  _name="$1"; shift
  "$@" > "$OUT/log" 2>&1
  _rc=$?
  case "$_rc" in
    0) row "✅ ok    $_name"; PASS=$((PASS + 1)) ;;
    2) row "SKIP  $_name  ($(head -1 "$OUT/log" 2>/dev/null | cut -c1-60))"; SKIP=$((SKIP + 1)) ;;
    *) row "❌ FAIL  $_name  (exit $_rc)"; FAIL=$((FAIL + 1))
       [ "$JSON" = "1" ] || sed 's/^/          /' "$OUT/log" | head -12 ;;
  esac
}

have_pwsh=0
command -v pwsh >/dev/null 2>&1 && have_pwsh=1

# The sh halves. Each is the authority on its own subject.
for c in check-docs check-markers check-one-home check-placeholders \
         check-control-bytes check-changelog check-no-secrets; do
  if [ -f "$HERE/$c.sh" ]; then
    run "$c" sh "$HERE/$c.sh"
  else
    row "SKIP  $c  (not present)"; SKIP=$((SKIP + 1))
  fi
done

# ⚠ --public is a DIFFERENT question from the default run, not a stricter one.
# Emails, absolute home paths and long hex are legitimate content in a private
# project, so this row is a second call rather than a flag on the first.
[ -f "$HERE/check-no-secrets.sh" ] && run "check-no-secrets --public" sh "$HERE/check-no-secrets.sh" --public

# ⚠ NEEDS gh AND THE NETWORK, so it exits 2 on a machine without them and that
# reads as a skip rather than a pass. That is correct: nothing was verified.
[ -f "$HERE/check-remote-items.sh" ] && run "check-remote-items" sh "$HERE/check-remote-items.sh"

# ⭐ THE SLOW ONE. Measured on one Windows 11 Pro 26200 machine, 2026-08-28:
# check-twins alone is most of a full run's wall time, because it starts both
# halves of every pair. --fast drops it and nothing else.
if [ "$FAST" = "1" ]; then
  row "SKIP  check-twins  (--fast)"
  SKIP=$((SKIP + 1))
elif [ -f "$HERE/check-twins.sh" ]; then
  run "check-twins" sh "$HERE/check-twins.sh"
else
  row "SKIP  check-twins  (not present)"; SKIP=$((SKIP + 1))
fi

# ⚠ THE POWERSHELL HALVES ARE NOT RE-RUN HERE. check-twins already runs both
# halves of every pair and compares them, so running them again would double
# the slowest part of the gate to learn nothing. On a machine with no pwsh at
# all, check-twins reports that itself.
[ "$have_pwsh" = "1" ] || row "note  pwsh absent; the PowerShell halves were not exercised"

TOTAL=$((PASS + FAIL + SKIP))

# ⛔ A RUN THAT PASSED NOTHING IS NOT A GREEN RUN. Zero failures out of zero
# checks executed is the shape this script exists to refuse, and it produced
# exactly that on its own first run through a broken presence test. Nothing
# passing is a failure of the gate regardless of --strict.
if [ "$PASS" -eq 0 ]; then
  RC=1
elif [ "$STRICT" = "1" ] && [ "$SKIP" -gt 0 ]; then
  RC=1
elif [ "$FAIL" -gt 0 ]; then
  RC=1
else
  RC=0
fi

if [ "$JSON" = "1" ]; then
  printf '{"schema":"check-gate/1","total":%s,"passed":%s,"failed":%s,"skipped":%s,"strict":%s}\n' \
    "$TOTAL" "$PASS" "$FAIL" "$SKIP" "$([ "$STRICT" = "1" ] && printf true || printf false)"
  exit "$RC"
fi

printf '\n%s\n' "$ROWS"
printf '%s checks: %s passed, %s failed, %s skipped\n' "$TOTAL" "$PASS" "$FAIL" "$SKIP"

if [ "$SKIP" -gt 0 ]; then
  printf -- '⚠ A SKIP IS NOT A PASS. Those checks did not run and nothing about\n'
  printf 'their subject was verified. Pass --strict to make a skip a failure.\n'
fi
if [ "$PASS" -eq 0 ]; then
  printf -- '❌ NOTHING RAN. Zero checks passed, so this is red whatever the skips say.\n'
elif [ "$FAIL" -gt 0 ]; then
  printf -- '❌ the gate is red.\n'
else
  printf -- '✅ nothing failed.\n'
  printf -- '⚠ That is part (a) of the gate only. Driving the real thing and the\n'
  printf 'deep reviews are the other two, and each is blind to what this catches.\n'
  printf 'docs/methodology/gate.md.\n'
fi
exit "$RC"
