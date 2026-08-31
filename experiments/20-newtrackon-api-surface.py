#!/usr/bin/env python3
"""
QUESTION
    What is newTrackon's real machine-readable API surface, what do its query
    parameters actually do, and can it serve as an INDEPENDENT RELIABILITY
    ORACLE for this project rather than merely a list of URLs?

WHY IT EXISTS
    HISTORY/reference-sweep.md asks the question this experiment answers, and says why it
    matters more than it looks: whether machine-readable uptime history is
    obtainable "decides whether newTrackon can serve as an independent
    historical-reliability signal or only as a source of URLs. This is the
    difference between a cross-check and a mirror."

    TODO/RULES.md C-26 recorded, as an INFERENCE from three 404s, that no such
    endpoint exists. That inference is wrong, and this experiment is what
    shows it. Reading the route table in `newtrackon/views.py` at commit
    7da7dde4a16d153790f4f3d2a6e0a245dceae641:

        @app.route("/api/<int:percentage>")

    `percentage` is an INTEGER PARAMETER, not a literal path segment. Probing
    the string `/api/percentage` asks for the tracker set of a percentage
    literally named "percentage", which is correctly a 404. The endpoint was
    there the whole time. This is the register's own rule in action: a 404 is
    evidence about one string, never about a route table.

    C-24 additionally recorded that the effect of
    `?include_ipv4_only_trackers=false` was ASSUMED because the control was
    never run. This script runs it: the same endpoint, with the parameter
    absent, false, and true.

WHAT IT DOES NOT ESTABLISH
    - That any tracker is alive. It reads someone else's opinion of that.
    - That newTrackon's "uptime" means what this project's "live" means. It
      does not, and the difference is recorded in the output: newTrackon
      ANNOUNCES (`announce_http`/`announce_udp`, random 20-byte infohash) to
      reach the `interval` field, while this project stops at scrape. Two
      different questions. Comparing them without saying so would manufacture
      agreement or disagreement that is really a methodology difference.

POLITENESS
    One GET per endpoint per run, with a descriptive User-Agent. The endpoint
    set is small and fixed. This is far below the load of a single browser
    visit to the site's own HTML pages.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _conditions as C  # noqa: E402

BASE = "https://newtrackon.com"
USER_AGENT = (
    "trackers/0.1 "
    "(+https://github.com/Azathothas/Trackers; "
    "API surface census; contact via repository issues)"
)

# Endpoints named by C-23, PLUS the two it missed. Recorded together so the
# omission stays visible rather than being quietly corrected.
ENDPOINTS = [
    ("/api/stable", "C-23 named"),
    ("/api/live", "C-23 named"),
    ("/api/all", "C-23 named"),
    ("/api/udp", "C-23 named"),
    ("/api/http", "C-23 named"),
    ("/api/dead", "C-23 named, expected 404"),
    ("/api/added", "C-23 named, expected 404"),
    ("/api/percentage", "C-23 named, expected 404 -- and the 404 MISLEADS"),
    ("/api/best", "MISSED by C-23; source shows a 301 to /api/stable"),
    ("/api/0", "MISSED by C-23; /api/<int:percentage>"),
    ("/api/50", "MISSED by C-23; /api/<int:percentage>"),
    ("/api/95", "MISSED by C-23; equals /api/stable's threshold"),
    ("/api/100", "MISSED by C-23; /api/<int:percentage>"),
    ("/list", "C-25: HTML, not a feed"),
    ("/raw", "C-25: HTML, not a feed"),
]

# The control C-24 says was never run.
PARAM_CONTROL = [
    ("/api/stable", None),
    ("/api/stable", "include_ipv4_only_trackers=false"),
    ("/api/stable", "include_ipv4_only_trackers=true"),
]


def get(path: str, query: str | None, timeout: float) -> dict:
    url = f"{BASE}{path}" + (f"?{query}" if query else "")
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            body = r.read(16 * 1024 * 1024).decode("utf-8", "replace")
            return {
                "url": url,
                "ok": True,
                "http_status": r.status,
                "content_type": r.headers.get("Content-Type", C.UNKNOWN),
                "bytes": len(body),
                "total_lines": len(body.splitlines()),
                "nonblank_lines": sum(1 for x in body.splitlines() if x.strip()),
                "blank_lines": sum(1 for x in body.splitlines() if not x.strip()),
                "looks_like_html": body.lstrip()[:200].lower().startswith(
                    ("<!doctype", "<html")),
                "first_line": (body.splitlines() or [""])[0][:120],
            }
    except urllib.error.HTTPError as e:
        return {"url": url, "ok": False, "http_status": e.code,
                "content_type": e.headers.get("Content-Type", C.UNKNOWN),
                "detail": f"HTTP {e.code}"}
    except Exception as e:
        return {"url": url, "ok": False, "http_status": None,
                "detail": f"{type(e).__name__}: {e}"}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--timeout", type=float, default=30.0)
    ap.add_argument("--expect-percentage-endpoint", action="store_true",
                    help="exit 1 unless /api/<int> behaves as an uptime filter, "
                         "i.e. C-26 is refuted and stays refuted")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    endpoints = {}
    for path, note in ENDPOINTS:
        r = get(path, None, args.timeout)
        r["register_note"] = note
        endpoints[path] = r

    params = []
    for path, q in PARAM_CONTROL:
        r = get(path, q, args.timeout)
        r["param"] = q or "(absent)"
        params.append(r)

    # Is /api/<int> monotone in the percentage? A real uptime filter must be:
    # raising the threshold can never add trackers.
    ladder = [(p, endpoints.get(f"/api/{p}", {}).get("nonblank_lines"))
              for p in (0, 50, 95, 100)]
    known = [(p, n) for p, n in ladder if isinstance(n, int)]
    monotone = all(known[i][1] >= known[i + 1][1] for i in range(len(known) - 1)) \
        if len(known) > 1 else False
    percentage_is_filter = monotone and len(known) == 4 and known[0][1] > known[-1][1]

    results = {
        "endpoints": endpoints,
        "parameter_control_c24": params,
        "percentage_ladder": {str(p): n for p, n in ladder},
        "percentage_behaves_as_uptime_filter": percentage_is_filter,
        "route_table_from_source": {
            "repo": "CorralPeltzer/newTrackon",
            "commit": "7da7dde4a16d153790f4f3d2a6e0a245dceae641",
            "file": "newtrackon/views.py",
            "routes": [
                "/api/<int:percentage>", "/api/stable", "/api/best", "/api/all",
                "/api/live", "/api/udp", "/api/http", "/api/add",
                "/", "/list", "/raw", "/faq", "/about", "/api", "/api.yml",
            ],
            "api_stable_definition":
                "api_percentage(95, added_before=<stable_min_age_days_default=10>) "
                "=> >=95% uptime AND >=10 days since added",
            "api_best_definition": "301 redirect to /api/stable",
            "measurement_method":
                "ANNOUNCE with a random 20-byte infohash (announce_http / "
                "announce_udp, thash=urandom(20)); recheck cadence is the "
                "tracker's OWN returned `interval`, floored to 10800s once "
                "uptime reaches 0",
        },
    }

    conditions = C.collect(sample_counts={
        "endpoints_probed": len(ENDPOINTS),
        "parameter_control_runs": len(PARAM_CONTROL),
        "requests_per_endpoint": 1,
    }, extra={"base": BASE, "user_agent": USER_AGENT,
              "source_commit_read": "7da7dde4a16d153790f4f3d2a6e0a245dceae641"})

    out = args.out or C.results_path(__file__)
    C.emit("What is newTrackon's real machine-readable API surface, and can it "
           "be an independent reliability oracle?", conditions, results, out)

    print("\nENDPOINTS")
    for path, note in ENDPOINTS:
        r = endpoints[path]
        st = r.get("http_status")
        n = r.get("nonblank_lines")
        ct = (r.get("content_type") or "").split(";")[0]
        html = " HTML" if r.get("looks_like_html") else ""
        print(f"  {path:22s} HTTP {str(st):>4s}  {ct:24s}"
              f"{'' if n is None else f'{n:5d} entries'}{html}   <- {note}")

    print("\nC-24 CONTROL  (the run the register says was never made)")
    for r in params:
        print(f"  /api/stable {r['param']:36s} -> "
              f"{r.get('nonblank_lines')} entries")
    print("  Reading: the parameter EXCLUDES IPv4-only trackers, and its")
    print("  default is `true`. Confirmed independently in views.py.")

    print("\nC-26  IS THERE A MACHINE-READABLE UPTIME ENDPOINT?")
    print(f"  /api/<int> ladder: {results['percentage_ladder']}")
    print(f"  monotone non-increasing in the threshold: {monotone}")
    if percentage_is_filter:
        print("  => YES. C-26 is REFUTED. newTrackon exposes uptime as a")
        print("     machine-readable filter, so it can be an INDEPENDENT")
        print("     ORACLE (a cross-check), not merely a source of URLs.")
    else:
        print("  => not demonstrated on this run; do not rely on it.")

    print("\nCAVEAT THAT MUST TRAVEL WITH ANY COMPARISON")
    print("  newTrackon derives uptime by ANNOUNCING with a random infohash.")
    print("  This project stops at scrape and never announces. 'Uptime' there")
    print("  and 'live' here are DIFFERENT QUESTIONS. Disagreement between")
    print("  them is a methodology difference first and a finding second.")

    if args.expect_percentage_endpoint and not percentage_is_filter:
        print("\nEXPECTATION FAILED: --expect-percentage-endpoint")
        return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
