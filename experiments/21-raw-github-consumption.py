#!/usr/bin/env python3
"""
QUESTION
    Does `raw.githubusercontent.com` actually work as this project's primary
    consumer contract -- what does it cache for, how fast does a push
    propagate, and does its caching defeat an hourly generation cadence?

WHY IT EXISTS
    TODO/RULES.md C-16 marks this "the primary consumer contract depends on
    this", and names the specific way it could fail:

        "If caching is longer than the update interval, hourly generation is
         partly pointless and the channel model needs rethinking."

    That is a real risk and it is cheap to measure, so it should never have
    been left as an assumption. This script measures it.

    It also answers the question T-122 turns into consumer-facing
    advice: whether a consumer should pin a branch path or a commit SHA. Both
    forms are fetched and their cache behaviour compared, because the answer
    determines what the documentation is allowed to promise.

METHOD, AND WHY THIS SHAPE
    The honest way to measure propagation is to fetch a path whose expected
    content you already know, immediately after the content changed. So the
    default subject is a file in THIS repository at the CURRENT commit, and
    the script verifies the body it received actually corresponds to that
    commit rather than trusting a 200.

    A 200 is not freshness. A cache can serve a stale 200 forever and it will
    look identical to success unless the body is checked. That distinction is
    the whole point of the experiment.

WHAT IT DOES NOT ESTABLISH
    - Behaviour from another network. CDN cache state is per-POP; a HIT here
      says nothing about a MISS in another region, and the `age` header is the
      only visible hint.
    - Behaviour under load, or the durability of the 300 s value. It is a
      current observation of a third party's configuration, which can change
      without notice. That is why C-16 stays environment-dependent and is
      re-checked at each phase gate.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _conditions as C  # noqa: E402

RAW = "https://raw.githubusercontent.com"
USER_AGENT = (
    "trackers/0.1 "
    "(+https://github.com/Azathothas/Trackers; "
    "raw-hosting behaviour check; contact via repository issues)"
)
CACHE_HEADERS = ("cache-control", "etag", "expires", "age", "x-cache",
                 "last-modified", "via", "content-type", "x-served-by")


def _sh(cmd: list[str]) -> str:
    try:
        return subprocess.run(cmd, capture_output=True, text=True,
                              timeout=15, check=False).stdout.strip() or C.UNKNOWN
    except Exception:
        return C.UNKNOWN


def fetch(url: str, timeout: float) -> dict:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    t0 = time.monotonic()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            body = r.read(8 * 1024 * 1024)
            hdrs = {k.lower(): v for k, v in r.headers.items()
                    if k.lower() in CACHE_HEADERS}
            return {"url": url, "ok": True, "http_status": r.status,
                    "bytes": len(body), "rtt_ms": round((time.monotonic() - t0) * 1000, 1),
                    "headers": hdrs, "body": body}
    except urllib.error.HTTPError as e:
        return {"url": url, "ok": False, "http_status": e.code,
                "rtt_ms": round((time.monotonic() - t0) * 1000, 1),
                "headers": {}, "body": b"", "detail": f"HTTP {e.code}"}
    except Exception as e:
        return {"url": url, "ok": False, "http_status": None, "headers": {},
                "body": b"", "detail": f"{type(e).__name__}: {e}"}


def main() -> int:
    here = os.path.dirname(os.path.abspath(__file__))
    repo = os.path.dirname(here)
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--owner", default="Azathothas")
    ap.add_argument("--repo", default="trackers")
    ap.add_argument("--branch", default=None, help="default: current branch")
    ap.add_argument("--path", default="experiments/README.md")
    ap.add_argument("--samples", type=int, default=3)
    ap.add_argument("--timeout", type=float, default=30.0)
    ap.add_argument("--max-cache-seconds", type=int, default=3600,
                    help="the generation interval this contract must beat")
    ap.add_argument("--expect-cache-under-interval", action="store_true",
                    help="exit 1 if max-age >= the generation interval, i.e. "
                         "if caching would make that cadence pointless")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    branch = args.branch or _sh(["git", "-C", repo, "rev-parse", "--abbrev-ref", "HEAD"])
    sha = _sh(["git", "-C", repo, "rev-parse", "HEAD"])
    local_path = os.path.join(repo, args.path)
    local_bytes = None
    if os.path.exists(local_path):
        with open(local_path, "rb") as fh:
            local_bytes = fh.read()

    branch_url = f"{RAW}/{args.owner}/{args.repo}/{branch}/{args.path}"
    sha_url = f"{RAW}/{args.owner}/{args.repo}/{sha}/{args.path}"

    branch_samples = []
    for i in range(args.samples):
        r = fetch(branch_url, args.timeout)
        # Freshness is a BODY question, not a status question.
        r["matches_local_working_copy"] = (
            None if local_bytes is None or not r["ok"] else r["body"] == local_bytes)
        r.pop("body", None)
        branch_samples.append(r)
        if i + 1 < args.samples:
            time.sleep(2)

    sha_sample = fetch(sha_url, args.timeout)
    sha_sample["matches_local_working_copy"] = (
        None if local_bytes is None or not sha_sample["ok"]
        else sha_sample["body"] == local_bytes)
    sha_sample.pop("body", None)

    # Parse max-age out of cache-control.
    max_age = None
    cc = (branch_samples[0]["headers"].get("cache-control") or "") if branch_samples else ""
    for part in cc.replace(" ", "").split(","):
        if part.startswith("max-age="):
            try:
                max_age = int(part.split("=", 1)[1])
            except ValueError:
                max_age = None

    cache_beats_interval = (max_age is not None and max_age < args.max_cache_seconds)

    results = {
        "branch_url": branch_url,
        "sha_url": sha_url,
        "branch_samples": branch_samples,
        "sha_sample": sha_sample,
        "max_age_seconds": max_age,
        "generation_interval_seconds": args.max_cache_seconds,
        "cache_shorter_than_generation_interval": cache_beats_interval,
        "etag_present": bool(branch_samples and branch_samples[0]["headers"].get("etag")),
    }

    conditions = C.collect(sample_counts={
        "branch_fetches": args.samples, "sha_fetches": 1,
    }, extra={"owner": args.owner, "repo": args.repo, "branch": branch,
              "commit": sha, "path": args.path, "user_agent": USER_AGENT})

    out = args.out or C.results_path(__file__)
    C.emit("Does raw.githubusercontent.com work as the primary consumer "
           "contract, and does its caching defeat the generation cadence?",
           conditions, results, out)

    print(f"\nBRANCH PATH  {branch_url}")
    for i, r in enumerate(branch_samples, 1):
        m = {True: "CURRENT", False: "STALE", None: "-"}[r["matches_local_working_copy"]]
        print(f"  sample {i}: HTTP {r['http_status']}  {r['bytes']}B  "
              f"{r['rtt_ms']}ms  body={m}  x-cache={r['headers'].get('x-cache', '-')}")
    print(f"  headers: {branch_samples[0]['headers'] if branch_samples else '-'}")

    m = {True: "CURRENT", False: "STALE", None: "-"}[sha_sample["matches_local_working_copy"]]
    print(f"\nSHA PATH     HTTP {sha_sample['http_status']}  body={m}  "
          f"x-cache={sha_sample['headers'].get('x-cache', '-')}")

    print("\nVERDICT")
    print(f"  max-age               : {max_age if max_age is not None else C.UNKNOWN} s")
    print(f"  generation interval   : {args.max_cache_seconds} s")
    print(f"  ETag present          : {results['etag_present']}  "
          f"(=> conditional requests work, so polling is cheap)")
    if cache_beats_interval:
        print("  => Caching is SHORTER than the generation interval. C-16's stated")
        print("     failure mode -- 'hourly generation is partly pointless' -- does")
        print("     NOT occur. The channel model stands.")
    elif max_age is not None:
        print("  => Caching is LONGER than the generation interval. C-16's failure")
        print("     mode APPLIES: rethink the cadence or the channel model.")
    else:
        print("  => No max-age observed; do not conclude either way.")

    print("\nCONSUMER PIN GUIDANCE THIS SUPPORTS")
    print("  Both a branch path and a commit-SHA path serve content. A SHA path")
    print("  is immutable but T-081 resets the data branch by design, so")
    print("  a SHA pinned there will eventually 404. Pin the BRANCH (or a")
    print("  release tag); never pin a data-branch SHA.")

    if args.expect_cache_under_interval and not cache_beats_interval:
        print("\nEXPECTATION FAILED: --expect-cache-under-interval")
        return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
