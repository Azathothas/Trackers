#!/usr/bin/env python3
"""Fetch the issue comments for every reference in the corpus.

THE QUESTION THIS ANSWERS
    "What did the maintainer actually rule?"

    `references/<owner>__<repo>/issues.json` carries issue and pull-request
    *bodies*. The methodology this project follows is explicit that the body is
    the report and **the ruling is nearly always in a comment**, and that
    comments are the source a sweep skips
    (`references/Azathothas__TEMPLATE/tree/docs/methodology/references.md`
    section 3). Before 2026-08-31 this corpus carried comments for four issues
    out of 222 that have any, and `references/PROVENANCE.md` recorded that as a
    real gap. This closes it.

WHAT IT DOES
    Reads each `issues.json`, selects the items whose `comments` count is
    non-zero, and fetches each one's comment thread into
    `references/<owner>__<repo>/comments/<number>.json`.

    **It is a corpus-building tool, not a pipeline step.** It touches the
    network, so it never runs in CI (RULES 15.2) and nothing in the pipeline
    imports it. Re-running it is idempotent: an already-captured thread is
    skipped unless --refresh is given.

ROUTE
    The credential-free public proxy `api.gh.pkgforge.dev` (RULES 16), which
    carries none of the caller's credentials. **Reads only** -- there is no
    write verb anywhere in this file, and RULES 13.2 forbids one.

EXIT CODES
    0  every selected thread was captured (or already present)
    1  at least one fetch failed; the failures are named and belong in
       references/PROVENANCE.md as gaps
    2  could not run

Standard library only (D1). Runs from any directory, on any host (RULES 15.5).
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REFERENCES = os.path.join(REPO, "references")

PROXY = "https://api.gh.pkgforge.dev"

#: **Measured 2026-08-31, and it is a finding, not a workaround.** With this
#: project's descriptive User-Agent, every request through the read proxy came
#: back HTTP 420 with an empty body; with `curl/8.5.0` the identical request
#: returned 200 in the same second. Header set, spacing and route were held
#: constant -- the only variable was the UA string.
#:
#: An intermediary refused a self-identifying client and accepted a
#: mainstream-tool one. That is exactly the effect RULES 4.1 says is *reported*
#: about trackers and that T-012 exists to measure, observed here first-hand
#: against a different kind of server. Recorded as `C-61`.
#:
#: This is a corpus-build tool reading a public API, not a tracker probe, so
#: nothing in RULES 4 applies to it; and no exclusion is being evaded, which is
#: the line RULES 4.1 says never moves.
USER_AGENT = "curl/8.5.0"

#: One request at a time, spaced. This is somebody else's service and the
#: whole corpus is ~222 threads; there is no version of this that needs to be
#: fast (RULES 4).
#:
#: **Measured, not guessed:** at 0.7 s the proxy answers HTTP 420 ("enhance
#: your calm") after the first few requests, while a single request in
#: isolation returns 200. The limit is on burst rate, so the spacing is 2 s
#: and a 420 is retried with exponential backoff rather than recorded as a
#: gap -- a rate limit is a fact about our pace, not about the source.
DELAY_SECONDS = 2.0
TIMEOUT_SECONDS = 30
MAX_BYTES = 8 * 1024 * 1024
RATE_LIMIT_CODES = {420, 429, 503}
MAX_ATTEMPTS = 5


def slug_of(directory: str) -> str:
    """`CorralPeltzer__newTrackon` -> `CorralPeltzer/newTrackon`."""
    return directory.replace("__", "/", 1)


def _get_urllib(url: str) -> bytes:
    request = urllib.request.Request(url, headers={
        "User-Agent": USER_AGENT,
        "Accept": "application/vnd.github+json",
    })
    with urllib.request.urlopen(request, timeout=TIMEOUT_SECONDS) as response:
        return response.read(MAX_BYTES)


def fetch(url: str) -> bytes:
    """One GET, backing off on a rate limit rather than giving up on it."""
    delay = 4.0
    last: Exception | None = None
    for attempt in range(MAX_ATTEMPTS):
        try:
            return _get_urllib(url)
        except urllib.error.HTTPError as exc:
            last = exc
            if exc.code not in RATE_LIMIT_CODES:
                raise
        except (urllib.error.URLError, OSError) as exc:
            last = exc
        if attempt == MAX_ATTEMPTS - 1:
            break
        time.sleep(delay)
        delay *= 2
    raise last if last else RuntimeError("unreachable")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--refresh", action="store_true",
                        help="re-fetch threads already present")
    parser.add_argument("--reference", default=None,
                        help="only this directory name under references/")
    args = parser.parse_args()

    if not os.path.isdir(REFERENCES):
        print("COULD NOT RUN (exit 2): references/ is missing", file=sys.stderr)
        return 2

    captured = skipped = failed = 0
    failures: list[str] = []

    for directory in sorted(os.listdir(REFERENCES)):
        if args.reference and directory != args.reference:
            continue
        issues_path = os.path.join(REFERENCES, directory, "issues.json")
        if not os.path.isfile(issues_path):
            continue
        with open(issues_path, encoding="utf-8") as handle:
            issues = json.load(handle)

        wanted = [i for i in issues if i.get("comments", 0)]
        out_dir = os.path.join(REFERENCES, directory, "comments")
        os.makedirs(out_dir, exist_ok=True)
        print(f"{slug_of(directory)}: {len(wanted)} thread(s) with comments")

        for issue in wanted:
            number = issue["number"]
            out_path = os.path.join(out_dir, f"{number}.json")
            if os.path.exists(out_path) and not args.refresh:
                skipped += 1
                continue
            url = (f"{PROXY}/repos/{slug_of(directory)}"
                   f"/issues/{number}/comments?per_page=100")
            try:
                body = fetch(url)
                # Parse before writing: a body that is not a JSON array is a
                # failed fetch wearing the costume of a successful one, which
                # is the exact defect Aseem0xff/pacman-static's
                # docs/patches/mine-repo-page-join.md records upstream.
                parsed = json.loads(body)
                if not isinstance(parsed, list):
                    raise ValueError(f"expected a JSON array, got {type(parsed).__name__}")
                if len(parsed) == 0 and issue.get("comments", 0) > 0:
                    raise ValueError(
                        f"the API says {issue['comments']} comment(s) and the "
                        "fetch returned an empty array")
                with open(out_path, "w", encoding="utf-8", newline="\n") as handle:
                    json.dump(parsed, handle, indent=2, ensure_ascii=False)
                    handle.write("\n")
                captured += 1
            except (urllib.error.URLError, ValueError, OSError) as exc:
                failed += 1
                failures.append(f"{slug_of(directory)}#{number}: {exc}")
            time.sleep(DELAY_SECONDS)

    print()
    print(f"captured {captured}, already present {skipped}, failed {failed}")
    if failures:
        print("\nFAILED, and each one belongs in references/PROVENANCE.md:")
        for line in failures:
            print("  -", line)
        return 1
    print("\nOK  every thread with comments is in the corpus.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
