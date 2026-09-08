#!/usr/bin/env python3
"""
QUESTION
    Does the identity this project sends change what an HTTP tracker answers?
    If a self-identifying User-Agent is refused where a client-like one is
    served, every HTTP health measurement here is contaminated: a refusal
    recorded as an unreachable tracker.

WHY IT EXISTS
    T-012, and RULES 4.1 withdrew the claim it tests. The only prior evidence
    was `experiments/05`: six targets on one day, five of which answered, and
    all six newTrackon-live at capture -- the friendliest possible subjects.
    `C-64` is one intermediary refusing this project's descriptive UA with HTTP
    420 and serving `curl/8.5.0` in the same second, which is not a tracker and
    raises the prior rather than settling it.

⛔ ONE AXIS, NOT TWO, AND THAT IS A FINDING RATHER THAN A SIMPLIFICATION
    T-012's design crosses the User-Agent with the BEP 20 `peer_id` prefix,
    because `C-63` establishes that a tracker's client filtering is written
    against the prefix at least as much as against the header.

    **This project never sends a `peer_id`.** It scrapes, and BEP 48's scrape
    carries `info_hash` and nothing else; `peer_id` is an announce parameter
    and there is no announce code path here (RULES 4). So the second axis does
    not exist in the request that is actually made, and varying it would mean
    adding a parameter BEP 48 does not define -- which changes the request
    shape and confounds the very thing being measured.

    ⚠ The consequence runs the other way and is worth stating: if trackers
    filter on the prefix, this project's scrapes carry **no prefix at all**,
    and nothing short of announcing could change that.

⛔ THE PAIRED DESIGN AND THE POLITENESS CEILING ARE IN TENSION
    A paired design wants every arm against every target. RULES 4's ceiling is
    one probe per tracker per its stated interval, defaulting to three hours,
    so four arms fired at one tracker in one run is four times the load that
    ceiling permits -- an experiment that breaks the project's own rule to
    measure whether the project is polite.

    ⭐ **The resolution is rotation.** One arm per tracker per run, assigned
    deterministically from the URL and the run index, so that over four runs
    every tracker has seen every arm exactly once and no run sends any tracker
    more than one request. `--rotation` selects the run. The pairing is
    recovered across runs rather than inside one, which costs the design its
    within-run control and is the only version of it that is allowed to exist.

CONTROL
    tier 0  a tracker this process starts on loopback, which records the
            headers it received. It proves the arms reach the wire as four
            different requests -- without it, "no difference between arms"
            could mean the arms were never different.

            ⚠ Its own responder rather than `tests/fake_tracker.py`, which is
            what `experiments/05` does and for the reason `D1`'s checker
            enforces: an experiment imports the standard library and this
            project's own `src/`, and nothing else.
    tier 2  the subjects: real HTTP trackers, one request each.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run
"""

from __future__ import annotations

import argparse
import hashlib
import http.server
import os
import sys
import threading
from collections import Counter

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, "src"))
sys.path.insert(0, os.path.join(REPO, "scripts"))

import _conditions as C  # noqa: E402
from generate import load_corpus  # noqa: E402
from trackers.bep34 import Resolver  # noqa: E402
from trackers.model import Transport  # noqa: E402
from trackers.probe import DEFAULT_USER_AGENT, ProbeConfig, probe  # noqa: E402
from trackers.vantage import detect as detect_vantage  # noqa: E402

FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")

#: The four identities, and each is here for a reason rather than for symmetry.
#:
#:   absent      no header at all, which is what BEP 15 sends on every UDP
#:               probe this project makes and therefore the honest baseline.
#:   descriptive the string this project has historically sent. RULES 4.1
#:               withdrew the claim that it is correct; this measures it.
#:   client_like qBittorrent 4.3.9, which is what newTrackon sends (`C-68`) --
#:               the operator of the closest production analogue judged the
#:               descriptive route not worth the risk.
#:   minimal     a generic browser string, to separate "not a known client"
#:               from "not a client at all".
ARMS: dict[str, str | None] = {
    "absent": None,
    "descriptive": DEFAULT_USER_AGENT,
    "client_like": "qBittorrent/4.3.9",
    "minimal": "Mozilla/5.0",
}

#: What a response is filed as. ⛔ `refused_by_policy` is separated from
#: `no_answer` because they support different conclusions: one is a decision
#: about us and the other may be anything at all.
OUTCOMES = ("tracker_semantic", "refused_by_policy", "rate_limited",
            "not_a_tracker", "no_answer", "not_contacted")


def arm_for(url: str, rotation: int) -> str:
    """Which arm this tracker gets on this run.

    ⛔ **Deterministic, so a run is reproducible and a tracker's arm is known
    before the request is made.** The hash is of the URL alone; the rotation
    offsets it, so over `len(ARMS)` runs every tracker sees every arm exactly
    once and no tracker is asked twice in one run.
    """
    names = sorted(ARMS)
    digest = hashlib.sha256(url.encode("utf-8")).digest()
    return names[(digest[0] + rotation) % len(names)]


def classify_result(result) -> str:
    """File one probe result under the outcome vocabulary above."""
    failure = result.failure.value
    if result.ok:
        return "tracker_semantic"
    if failure == "blocked_by_policy":
        return "refused_by_policy"
    if failure == "rate_limited":
        return "rate_limited"
    if failure in ("not_a_tracker", "truncated_response", "protocol_error"):
        return "not_a_tracker"
    if failure in ("excluded_by_operator", "exclusion_undetermined",
                   "unsupported", "dns_failure", "dns_undetermined",
                   "resolver_divergence", "no_usable_address"):
        return "not_contacted"
    return "no_answer"


class _Recorder(http.server.BaseHTTPRequestHandler):
    """Answers as a tracker and remembers who asked."""

    seen: list[str] = []

    def do_GET(self):  # noqa: N802 - the base class names it
        type(self).seen.append(self.headers.get("User-Agent", ""))
        body = b"d5:filesd20:aaaaaaaaaaaaaaaaaaaad8:completei1e10:downloadedi0e"
        body += b"10:incompletei0eeee"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):  # keep the experiment's output readable
        return


def tier0() -> dict:
    """Do the four arms reach the wire as four different requests?

    ⛔ Without this, "the arms agree" is indistinguishable from "the arms were
    never sent differently", which is the absence-is-not-a-zero failure applied
    to an experiment's own construction.
    """
    from trackers.normalize import parse

    vantage = detect_vantage()
    if "ipv4" not in vantage.ip_families:
        return {"ok": False, "detail": "no ipv4 route from this vantage"}
    _Recorder.seen = []
    try:
        server = http.server.HTTPServer(("127.0.0.1", 0), _Recorder)
    except OSError as exc:
        return {"ok": False, "detail": f"could not bind loopback: {exc}"}
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        port = server.server_address[1]
        for _, value in sorted(ARMS.items()):
            probe(parse(f"http://127.0.0.1:{port}/announce"),
                  ProbeConfig(timeout=2.0, retries=0, user_agent=value),
                  vantage)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2.0)
    distinct = len(set(_Recorder.seen))
    return {"ok": distinct == len(ARMS), "distinct_user_agents": distinct,
            "expected": len(ARMS), "received": len(_Recorder.seen),
            "detail": f"{distinct} of {len(ARMS)} arms reached the wire distinctly"}


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", default=None)
    parser.add_argument("--fixtures", default=FIXTURES)
    parser.add_argument("--rotation", type=int, default=0,
                        help="which run of the rotation this is. Over "
                             "len(ARMS) runs every tracker sees every arm once")
    parser.add_argument("--limit", type=int, default=None,
                        help="probe at most this many HTTP trackers, in corpus "
                             "order. A pilot states its size rather than "
                             "implying a corpus")
    parser.add_argument("--timeout", type=float, default=8.0)
    parser.add_argument("--offline", action="store_true",
                        help="run the control only and contact no tracker")
    parser.add_argument("--expect-arms", action="store_true",
                        help="exit 1 if the control fails, or if the arms "
                             "differ enough that the identity is deciding what "
                             "this project measures")
    parser.add_argument("--difference-threshold", type=float, default=0.2,
                        help="how far two arms' answer rates may differ before "
                             "--expect-arms calls it material")
    args = parser.parse_args()

    control = tier0()

    rows: list[dict] = []
    by_arm: dict[str, Counter] = {name: Counter() for name in ARMS}
    if not args.offline:
        try:
            aggregate, _, _ = load_corpus(True, args.fixtures)
        except Exception as exc:  # noqa: BLE001
            print(f"could not build the corpus: {exc}", file=sys.stderr)
            return C.EXIT_COULD_NOT_RUN
        subjects = [t for t in aggregate.trackers
                    if t.transport in (Transport.HTTP, Transport.HTTPS)]
        subjects.sort(key=lambda t: t.url)
        if args.limit:
            subjects = subjects[:args.limit]

        vantage = detect_vantage()
        resolver = Resolver()
        for tracker in subjects:
            arm = arm_for(tracker.url, args.rotation)
            result = probe(tracker,
                           ProbeConfig(timeout=args.timeout, retries=0,
                                       user_agent=ARMS[arm]),
                           vantage, resolver=resolver)
            outcome = classify_result(result)
            by_arm[arm][outcome] += 1
            rows.append({"url": tracker.url, "arm": arm, "outcome": outcome,
                         "http_status": result.http_status,
                         "failure": result.failure.value,
                         "sent_user_agent": result.sent_user_agent})

    def answered(counts: Counter) -> tuple[int, int]:
        contacted = sum(v for k, v in counts.items() if k != "not_contacted")
        return counts["tracker_semantic"], contacted

    rates = {}
    for name, counts in by_arm.items():
        ok, contacted = answered(counts)
        rates[name] = {
            "answered": ok, "contacted": contacted,
            "rate": round(ok / contacted, 4) if contacted else None,
            "outcomes": {k: counts[k] for k in OUTCOMES if counts[k]},
        }

    measured = [r["rate"] for r in rates.values() if r["rate"] is not None]
    spread = round(max(measured) - min(measured), 4) if len(measured) > 1 else None

    results = {
        "control": control,
        "rotation": args.rotation,
        "arms": rates,
        "spread": spread,
        "difference_threshold": args.difference_threshold,
        "peer_id_axis": (
            "NOT MEASURED AND NOT MEASURABLE ON THIS PATH. A BEP 48 scrape "
            "carries info_hash and nothing else; peer_id is an announce "
            "parameter and this project has no announce code path (RULES 4). "
            "C-63 still stands: a tracker's filtering may key on the prefix, "
            "and this project sends none at all."),
        "rows": rows,
        "what_this_is_not": (
            "Not a paired comparison within one run. RULES 4's ceiling is one "
            "probe per tracker per interval, so each tracker sees one arm per "
            "run and the pairing is recovered across rotations."),
    }

    conditions = C.with_network_vantage(C.collect(sample_counts={
        "subjects": len(rows),
        "arms": len(ARMS),
        "rotation": args.rotation,
    }))
    C.emit("Does the identity this project sends change what a tracker answers?",
           conditions, results, args.out)

    print(f"\nCONTROL  four arms reach the wire distinctly: "
          f"{'PASS' if control['ok'] else 'FAIL'}  {control.get('detail', '')}")
    print(f"\nARM           answered  contacted  rate")
    for name in sorted(rates):
        row = rates[name]
        rate = "-" if row["rate"] is None else f"{row['rate']:.3f}"
        print(f"  {name:12s} {row['answered']:8d}  {row['contacted']:9d}  {rate}")
    print(f"\nSPREAD between arms: {spread if spread is not None else '-'}")

    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - Anything about the peer_id axis. This project sends none, and")
    print("    a scrape has no field for one (C-63 stands, unmeasured here).")
    print("  - That one run settles it. Each tracker sees ONE arm per run;")
    print("    the comparison needs every rotation and a second day.")
    print("  - That a difference is the tracker's doing rather than the path.")

    if args.expect_arms:
        if not control["ok"]:
            print("\nEXPECTATION FAILED: the control did not pass, so no arm")
            print("  row above can be quoted.")
            return C.EXIT_MEASURED_AND_FAILED
        if spread is not None and spread > args.difference_threshold:
            print(f"\nEXPECTATION FAILED: arms differ by {spread}, over the")
            print(f"  {args.difference_threshold} threshold. The identity is")
            print("  deciding what this project measures, which is T-012's")
            print("  contamination case and RULES 4.1's open question.")
            return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
