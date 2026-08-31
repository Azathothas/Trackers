#!/usr/bin/env python3
"""
QUESTION
    Do GitHub's own current docs still state the four platform behaviours this
    project's schedule design depends on -- and will this script FAIL when one
    of them changes?

WHY IT EXISTS
    C-10, C-11, C-12 and C-19b are documentation claims. They cannot be
    measured from a single session: "scheduled workflows are disabled after 60
    days of inactivity" would take 60 days of deliberate silence to observe,
    and "some queued jobs may be dropped" needs months of run history.

    That is exactly why they are dangerous. An unmeasurable claim tends to get
    read once, believed forever, and never re-checked -- and C-12 is the one
    whose failure mode TODO/RULES.md calls "the worst failure mode available":
    a dataset that stops updating without telling anyone.

    So this instrument does the only honest thing available: it pins the
    SENTENCES, re-fetches the page, and exits non-zero when a sentence is no
    longer there. It converts "we read the docs once" into a check the project
    keeps running. That is `references.md`'s rule -- an instrument takes an
    `--expect` argument and exits non-zero on mismatch, so research becomes a
    regression check instead of decaying.

WHAT IT DOES NOT ESTABLISH
    - That the documented behaviour is the ACTUAL behaviour. Platform docs are
      correct about intent and sometimes behind the platform (RULES 1.1).
      A passing run here means "GitHub still says this", never "GitHub does
      this". Only observation over time can say the second, and where that is
      required the register says so.
    - Anything about a private repository. The 60-day rule is stated for
      PUBLIC repositories and this project is public; a fork that is private
      has not been checked.

EXIT CODES
    0  the measurement ran and every pinned assertion still holds
    1  the measurement ran and a pinned assertion is GONE -- re-read the docs
    2  the measurement could not run (network, proxy, page moved)
"""

from __future__ import annotations

import argparse
import os
import re
import sys
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _conditions as C  # noqa: E402

USER_AGENT = (
    "trackers/0.1 "
    "(+https://github.com/Azathothas/Trackers; "
    "documentation assertion check; contact via repository issues)"
)

DOCS_URL = ("https://docs.github.com/en/actions/reference/"
            "workflows-and-actions/events-that-trigger-workflows")

# Each assertion is (claim id, a regex that must match, why it is load-bearing).
# The regexes are deliberately loose about whitespace and markup and strict
# about the words that carry the meaning.
ASSERTIONS = [
    ("C-10",
     r"shortest\s+interval\s+you\s+can\s+run\s+scheduled\s+workflows\s+is\s+once\s+every\s+5\s+minutes",
     "Sets the floor on cadence. The chosen cadence is far above it, so this "
     "is a sanity bound rather than a constraint."),
    ("C-11",
     r"schedule.{0,40}event\s+can\s+be\s+delayed\s+during\s+periods\s+of\s+high\s+loads",
     "Delayed runs are expected, not exceptional. State handling must be "
     "idempotent under late execution."),
    ("C-11b",
     r"some\s+queued\s+jobs\s+may\s+be\s+dropped",
     "Runs can be DROPPED entirely, not merely delayed. A missed cycle must "
     "never be read as 'all trackers went dead'."),
    ("C-12",
     r"scheduled\s+workflows\s+are\s+automatically\s+disabled\s+when\s+no\s+repository\s+activity\s+has\s+occurred\s+in\s+60\s+days",
     "THE LOAD-BEARING ONE. A public repository's schedule stops after 60 days "
     "of inactivity. Without a mitigation the dataset silently stops updating, "
     "which TODO/RULES.md calls the worst failure mode available."),
    ("C-19b",
     r"GITHUB_TOKEN.{0,40}(events\s+)?do\s+not\s+create\s+workflow\s+runs",
     "Commits pushed by a workflow with the default token do NOT trigger "
     "further workflows. Protects against infinite loops; breaks any design "
     "expecting a chained trigger."),
]


def fetch(url: str, timeout: float) -> tuple[str | None, dict]:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            body = r.read(16 * 1024 * 1024).decode("utf-8", "replace")
            return body, {"ok": True, "http_status": r.status, "bytes": len(body)}
    except urllib.error.HTTPError as e:
        return None, {"ok": False, "http_status": e.code, "detail": f"HTTP {e.code}"}
    except Exception as e:
        return None, {"ok": False, "http_status": None,
                      "detail": f"{type(e).__name__}: {e}"}


def normalise(html: str) -> str:
    """Strip tags and collapse whitespace so a regex sees prose, not markup."""
    t = re.sub(r"<script.*?</script>", " ", html, flags=re.S | re.I)
    t = re.sub(r"<style.*?</style>", " ", t, flags=re.S | re.I)
    t = re.sub(r"<[^>]+>", " ", t)
    t = (t.replace("&quot;", '"').replace("&#39;", "'").replace("&amp;", "&")
          .replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">"))
    return re.sub(r"\s+", " ", t)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--url", default=DOCS_URL)
    ap.add_argument("--timeout", type=float, default=45.0)
    ap.add_argument("--expect-all", action="store_true",
                    help="exit 1 if any pinned assertion is no longer present")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    body, meta = fetch(args.url, args.timeout)
    if body is None:
        # Try the mandated read-only proxy before giving up: TODO/RULES.md 2.1 says
        # skipping a reference because our own route is blocked is unacceptable.
        body, meta2 = fetch(f"https://api.rv.pkgforge.dev/{args.url}", args.timeout)
        meta = {"direct": meta, "via_proxy": meta2}
        if body is None:
            print(f"could not fetch docs: {meta}", file=sys.stderr)
            C.emit("Do GitHub's docs still state the four scheduling behaviours "
                   "this project depends on?",
                   C.collect(extra={"url": args.url}),
                   {"fetch": meta, "assertions": None},
                   args.out or C.results_path(__file__))
            return C.EXIT_COULD_NOT_RUN

    text = normalise(body)
    checked = []
    for cid, pattern, why in ASSERTIONS:
        m = re.search(pattern, text, flags=re.I)
        checked.append({
            "claim": cid,
            "present": bool(m),
            "pattern": pattern,
            "why_load_bearing": why,
            # Quote what the page actually says, so the result file carries the
            # evidence and not merely a boolean.
            "quote": (text[max(0, m.start() - 90):m.end() + 90].strip()
                      if m else C.UNKNOWN),
        })

    missing = [c["claim"] for c in checked if not c["present"]]
    results = {"fetch": meta, "url": args.url, "assertions": checked,
               "missing": missing, "all_present": not missing}

    conditions = C.collect(sample_counts={"assertions": len(ASSERTIONS),
                                          "fetches": 1},
                           extra={"url": args.url, "user_agent": USER_AGENT})
    out = args.out or C.results_path(__file__)
    C.emit("Do GitHub's docs still state the four scheduling behaviours this "
           "project depends on?", conditions, results, out)

    print("\nPINNED ASSERTIONS")
    for c in checked:
        print(f"  {c['claim']:7s} {'PRESENT' if c['present'] else 'GONE   '}")
        if c["present"]:
            print(f"          \"...{c['quote'][:150]}...\"")
        print(f"          why: {c['why_load_bearing']}")

    print("\nWHAT A PASS HERE MEANS")
    print("  GitHub still SAYS these things. It does not mean GitHub DOES")
    print("  them. Documentation is correct about intent and is sometimes")
    print("  behind the platform (TODO/RULES.md 3.1). C-11 and C-12 in particular")
    print("  need observation over weeks and months; this check cannot")
    print("  substitute for that and does not claim to.")

    if missing:
        print(f"\nEXPECTATION FAILED: assertions no longer present: {missing}")
        print("  Re-read the page. A platform behaviour this project depends on")
        print("  may have changed, and the register rows must be re-checked.")
        if args.expect_all:
            return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
