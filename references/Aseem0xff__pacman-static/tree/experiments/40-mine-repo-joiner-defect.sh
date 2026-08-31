#!/bin/sh
# 40-mine-repo-joiner-defect.sh
#
# QUESTION: does the reference-mining script this sweep was told to use
# actually deliver the issue comments it reports as fetched?
#
# ⭐ IT DOES NOT, AND THE ANSWER MATTERS BEYOND THIS SWEEP. references.md
# names comments as the source that only it has -- "the maintainer's ruling
# is nearly always in a comment" -- and the script writes an empty array for
# them while printing "comments: ok". A sweep that trusted the output would
# conclude the trackers were silent. This one nearly did.
#
# THE DEFECT, in scripts/common/mine-repo.sh as fetched from
# Azathothas/TEMPLATE at 6eaf4b5fbe8e3207de231f86641e95179e3bc79f:
# the proxy route pages by hand, concatenates the pages into one buffer, and
# recovers the array bounds by counting `[` and `]` characters over the RAW
# TEXT. That counts brackets inside string values too. Markdown links and
# pasted logs in comment bodies are full of them, so the depth counter never
# returns to zero, nothing is pushed, and `[]` is written.
#
# ORACLE: a real JSON parser (python3's json.load) over the same bytes. The
# comparison is between two readings of ONE captured file, so it does not
# depend on the network being up or the tracker being unchanged.
#
# ⚠ IT NEEDS ONE LIVE FETCH to capture the fixture the first time, then runs
# offline against fixtures/mussel-issue-comments-page1.json forever after.
#
# EXIT CODES
#   0  the joiner and the oracle agree (the defect is gone)
#   1  they disagree (the defect is present -- expected today)
#   2  the measurement could not run
set -u

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
FIX="$HERE/fixtures/mussel-issue-comments-page1.json"
OUT="$HERE/out/40-mine-repo-joiner-defect.txt"
URL='https://api.gh.pkgforge.dev/repos/firasuke/mussel/issues/comments?per_page=100&page=1'

command -v python3 >/dev/null 2>&1 || { echo "40: python3 not found" >&2; exit 2; }
command -v node    >/dev/null 2>&1 || { echo "40: node not found (needed to run the original joiner)" >&2; exit 2; }
mkdir -p "$HERE/out" "$HERE/fixtures" || exit 2

if [ ! -s "$FIX" ]; then
  command -v curl >/dev/null 2>&1 || { echo "40: no fixture and no curl" >&2; exit 2; }
  # ⛔ THE PROXY REFUSES A BROWSER-LIKE OR EMPTY USER-AGENT WITH HTTP 420,
  # which is not a status any client special-cases. Send curl's own.
  curl -sS -A 'curl/8' --max-time 60 -o "$FIX.part" "$URL" || {
    echo "40: could not capture the fixture" >&2; rm -f "$FIX.part"; exit 2; }
  mv "$FIX.part" "$FIX"
fi

TMP=$(mktemp -d) || exit 2
trap 'rm -rf "$TMP"' EXIT

# The joiner exactly as mine-repo.sh has it. Kept verbatim on purpose: a
# paraphrase would not be evidence about the script that shipped.
node -e '
  const fs=require("fs");
  const raw=fs.readFileSync(process.argv[1],"utf8");
  const out=[]; let d=0,s=0;
  for(let i=0;i<raw.length;i++){ if(raw[i]==="["){ if(d===0)s=i; d++; }
    else if(raw[i]==="]"){ d--; if(d===0) out.push(...JSON.parse(raw.slice(s,i+1))); } }
  fs.writeFileSync(process.argv[2], JSON.stringify(out,null,1));
' "$FIX" "$TMP/joined.json" 2>"$TMP/node.err"

oracle=$(python3 -c 'import json,sys;print(len(json.load(open(sys.argv[1]))))' "$FIX" 2>/dev/null || echo ERR)
joiner=$(python3 -c 'import json,sys;print(len(json.load(open(sys.argv[1]))))' "$TMP/joined.json" 2>/dev/null || echo ERR)

stats=$(python3 - "$FIX" <<'PY'
import json,sys
d=json.load(open(sys.argv[1]))
o=sum((c.get('body') or '').count('[') for c in d)
c=sum((c.get('body') or '').count(']') for c in d)
print(f"{o+c} {o-c}")
PY
)
brackets=${stats%% *}; imbalance=${stats##* }

rc=0
[ "$oracle" = "$joiner" ] || rc=1

{
  echo "# 40-mine-repo-joiner-defect"
  echo "date     : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host     : $(uname -srm)"
  echo "node     : $(node --version)"
  echo "python3  : $(python3 --version 2>&1)"
  echo "fixture  : fixtures/$(basename "$FIX") ($(wc -c < "$FIX" | tr -d ' ') bytes)"
  echo "subject  : scripts/common/mine-repo.sh fetch_list() page joiner,"
  echo "           Azathothas/TEMPLATE @ 6eaf4b5fbe8e3207de231f86641e95179e3bc79f"
  echo
  printf '%-34s %s\n' 'items, oracle (json.load)'        "$oracle"
  printf '%-34s %s\n' 'items, mine-repo bracket scanner' "$joiner"
  printf '%-34s %s\n' '[ or ] inside comment bodies'     "$brackets"
  printf '%-34s %s\n' 'bracket imbalance in bodies'      "$imbalance"
  echo
  [ -s "$TMP/node.err" ] && { echo "joiner stderr:"; sed 's/^/  /' "$TMP/node.err"; echo; }
  if [ $rc -eq 0 ]; then
    echo "verdict: joiner and oracle agree -- the defect is not present"
  else
    echo "verdict: ⛔ the joiner drops $((oracle - joiner)) of $oracle items and the"
    echo "         calling script still prints \"comments: ok\"."
    echo
    echo "the fix, applied in this tree at scripts/mine-repo.sh:"
    echo "  keep each page as its own file and join with a real JSON parser."
    echo "  Measured after the fix, on firasuke/mussel: comments 0 -> 202."
    echo "  A guard was added too: a join that yields [] from a page that has"
    echo "  \"url\" in it is reported as a failure rather than as an empty tracker."
  fi
} > "$OUT"

cat "$OUT"
exit $rc
