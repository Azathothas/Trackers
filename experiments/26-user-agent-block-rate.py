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
    deterministically and in equal groups, so that over four runs every tracker
    has seen every arm exactly once and no run sends any tracker more than one
    request. `--rotation` selects the run. The pairing is recovered across runs
    rather than inside one, which costs the design its within-run control and
    is the only version of it that is allowed to exist.

⛔ A SUBJECT THAT ANSWERS NOBODY CANNOT PREFER AN IDENTITY
    The first run took the first 200 HTTP trackers in corpus order and **174 of
    them answered nothing at all**, so four arms were compared on eight
    informative rows. `--from-sweep` draws subjects from trackers a committed
    sweep recorded `live`, which raises the power and lowers the load at the
    same time, and `--exclude` keeps a rerun from asking anybody twice inside
    RULES 4's ceiling.

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
import json
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
from trackers.politeness import (DEFAULT_INTERVAL_SECONDS,  # noqa: E402
                                 too_soon_after)
from trackers.probe import DEFAULT_USER_AGENT, ProbeConfig, probe  # noqa: E402
from trackers.state import read_state  # noqa: E402
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


def assign_arms(urls: list[str], rotation: int) -> dict[str, str]:
    """Which arm each tracker gets on this run. Deterministic and balanced.

    ⛔ **Deterministic**, so a run is reproducible and a tracker's arm is known
    before the request is made. ⛔ **Balanced**, because the arms are compared
    with each other and an arm of four against an arm of fifteen wastes the
    subjects it did have.

    ⚠ **An earlier version took the URL's hash modulo the number of arms**, and
    on the 32 live subjects of 2026-09-08 that gave 15, 7, 6 and 4 -- the sizes
    a fair coin gives when there are only 32 of them. Sorting by hash and
    slicing into equal groups keeps every property that version had, except
    that a subject leaving the set now shifts a neighbour's arm, which is the
    price of the balance and is recorded rather than hidden.

    Over `len(ARMS)` rotations every tracker sees every arm exactly once, and
    no rotation asks any tracker twice.
    """
    names = sorted(ARMS)
    ordered = sorted(urls, key=lambda u: hashlib.sha256(u.encode("utf-8")).digest())
    out: dict[str, str] = {}
    for index, url in enumerate(ordered):
        group = (index * len(names)) // max(1, len(ordered))
        out[url] = names[(group + rotation) % len(names)]
    return out


def read_live_urls(paths) -> set[str]:
    """URLs a committed sweep recorded `live`.

    ⛔ `live` only. `unknown` is the two ways of knowing nothing and neither
    makes a subject informative here: this experiment compares what trackers
    answer, and one that answers nobody answers every arm the same way.
    """
    out: set[str] = set()
    for path in paths:
        with open(path, encoding="utf-8") as handle:
            doc = json.load(handle)
        for record in doc.get("trackers", []):
            if record.get("health_state") == "live":
                out.add(str(record.get("url", "")))
    return out - {""}


def read_last_contact(paths) -> dict[str, str]:
    """When each URL was last contacted, from any record this project writes.

    ⛔ **Both shapes, and the second one became load-bearing on 2026-09-08.**
    This read only this experiment's own results, which was enough while the
    experiment was the only thing contacting trackers on demand. The health
    sweep is **scheduled** now, every three hours, so a tracker it just probed
    is one this experiment must leave alone -- and a version of this function
    that cannot read a sweep record cannot express that. Two instruments each
    obeying D7 on their own and contacting one tracker between them is still a
    breach of D7.

        {"conditions": {"utc": ...}, "results": {"rows": [...]}}   this
        {"generated_at": ..., "trackers": [...]}                   a sweep

    ⭐ **An instant, not a set, and that is T-087's rule reaching here.** The
    earlier version returned "every URL these files mention" and excluded them
    **forever**, which is stricter than D7 and wrong in the direction that
    silently shrinks the subject set: a tracker contacted last Friday can be
    asked today, and dropping it costs the comparison a subject for nothing.
    D7's question is *how long ago*, so the answer has to be a time.
    """
    out: dict[str, str] = {}
    for path in paths:
        with open(path, encoding="utf-8") as handle:
            doc = json.load(handle)
        rows = list((doc.get("results") or {}).get("rows") or [])
        at = str((doc.get("conditions") or {}).get("utc") or "")
        sweep_rows = list(doc.get("trackers") or [])
        if sweep_rows:
            rows += sweep_rows
            at = str(doc.get("generated_at") or at)
        for row in rows:
            url = str(row.get("url", ""))
            # A sweep record carries its own instant; an experiment row does
            # not, so the run's is the honest stand-in for every row in it.
            when = str(row.get("observed_at") or at)
            if url and when and when > out.get(url, ""):
                out[url] = when
    return out


def read_state_last_seen(path: str) -> dict[str, str]:
    """`last_seen` per tracker from the published history.

    ⭐ **This is why `--exclude` no longer has to name every sweep.** The
    `data` branch's `state.jsonl` already records when every tracker was last
    contacted by any sweep, so pointing at it covers the whole schedule --
    including runs whose artefacts have expired. Rotation 1 had to be given two
    sweep files by hand and would have missed a third.

    ⛔ It does **not** cover this experiment's own contacts: results here are
    not folded into the history, so `--exclude` still carries them.
    """
    histories, _ = read_state(path)
    return {url: h.last_seen for url, h in histories.items()}


def stratum_of(result: dict) -> str:
    """Which subject population a run drew from.

    ⛔ **The strata are not pooled, and refusing to pool them is the whole
    point of this function.** The pilot took the first 200 HTTP trackers in
    corpus order and **174 of them answered nobody**; every run since has drawn
    from trackers a sweep recorded `live`. Adding those together gives an arm
    whose rate is decided by how many dead trackers happened to fall in it,
    which is a measurement of the corpus and not of the identity -- and it
    would look like a bigger sample, which is exactly what makes it dangerous.

    RULES 2: a number carries its conditions or it is not a number.
    """
    selection = result.get("subject_selection") or {}
    return "live-from-sweep" if selection.get("from_sweep") else "corpus-order"


def aggregate_series(paths) -> dict:
    """Pool the rotations into the comparison the design actually specifies.

    ⭐ **The pairing lives across runs, not inside one.** RULES 4's ceiling
    permits one request per tracker per interval, so a run gives each tracker
    exactly one arm; over `len(ARMS)` rotations every tracker has seen every
    arm. That makes the series -- not any run in it -- the unit of comparison,
    and until this existed each run was being read on its own and each was
    correctly refusing to answer.

    Two views come back, and they answer different questions:

      `pooled`   answered/contacted per arm across the stratum. Simple, and
                 confounded by which trackers happened to land in which arm.
      `paired`   restricted to subjects seen under two or more arms, which is
                 the design's own control: the same tracker, different
                 identities. Discordant counts are reported and no test
                 statistic is computed -- at these sample sizes a p-value
                 would be precision on the wrong quantity.
    """
    by_stratum: dict[str, dict] = {}
    for path in paths:
        with open(path, encoding="utf-8") as handle:
            doc = json.load(handle)
        result = doc.get("results") or {}
        stratum = by_stratum.setdefault(stratum_of(result), {
            "runs": [], "arms": {name: Counter() for name in ARMS},
            "by_url": {}})
        stratum["runs"].append({
            "path": os.path.basename(path),
            "utc": (doc.get("conditions") or {}).get("utc"),
            "rotation": result.get("rotation"),
            "subjects": len(result.get("rows") or []),
        })
        for row in result.get("rows") or []:
            arm, outcome = str(row.get("arm")), str(row.get("outcome"))
            if arm not in ARMS or outcome not in OUTCOMES:
                continue
            stratum["arms"][arm][outcome] += 1
            stratum["by_url"].setdefault(str(row.get("url")), {})[arm] = outcome

    out: dict[str, dict] = {}
    for name, stratum in sorted(by_stratum.items()):
        pooled = {}
        for arm, counts in stratum["arms"].items():
            contacted = sum(v for k, v in counts.items() if k != "not_contacted")
            answered = counts["tracker_semantic"]
            pooled[arm] = {
                "answered": answered, "contacted": contacted,
                "rate": round(answered / contacted, 4) if contacted else None,
                "outcomes": {k: counts[k] for k in OUTCOMES if counts[k]},
            }
        # The paired half: the same tracker under more than one identity.
        paired_subjects = {url: arms for url, arms in stratum["by_url"].items()
                           if len(arms) > 1}
        discordant: dict[str, dict[str, int]] = {}
        for url, arms in paired_subjects.items():
            for a in sorted(arms):
                for b in sorted(arms):
                    if a >= b:
                        continue
                    key = f"{a} vs {b}"
                    cell = discordant.setdefault(
                        key, {"both_answered": 0, "neither": 0,
                              f"only_{a}": 0, f"only_{b}": 0})
                    ok_a = arms[a] == "tracker_semantic"
                    ok_b = arms[b] == "tracker_semantic"
                    if ok_a and ok_b:
                        cell["both_answered"] += 1
                    elif ok_a:
                        cell[f"only_{a}"] += 1
                    elif ok_b:
                        cell[f"only_{b}"] += 1
                    else:
                        cell["neither"] += 1
        rates = [r["rate"] for r in pooled.values() if r["rate"] is not None]
        out[name] = {
            "runs": stratum["runs"],
            "pooled": pooled,
            "spread": (round(max(rates) - min(rates), 4)
                       if len(rates) > 1 else None),
            "subjects": len(stratum["by_url"]),
            "subjects_seen_under_two_or_more_arms": len(paired_subjects),
            "discordant": discordant,
        }
    return out


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
    parser.add_argument("--from-sweep", nargs="*", default=None,
                        metavar="RESULT",
                        help="draw subjects from trackers a committed sweep "
                             "recorded `live`. ⭐ A tracker that answers "
                             "nobody cannot express a preference about our "
                             "identity, so this raises the power and lowers "
                             "the load at the same time")
    parser.add_argument("--exclude", nargs="*", default=None,
                        metavar="RESULT",
                        help="skip any URL these earlier results contacted "
                             "within D7's interval. ⛔ RULES 4's ceiling is "
                             "per tracker per its stated interval, and two "
                             "runs in one afternoon are two probes inside it. "
                             "⭐ It is an interval and not a blacklist: a "
                             "tracker contacted last week is a subject again")
    parser.add_argument("--state", default=None, metavar="STATE_JSONL",
                        help="the published history, which records when every "
                             "sweep last contacted each tracker. ⭐ Covers the "
                             "whole schedule at once, so a rotation no longer "
                             "has to be handed each sweep file by name and "
                             "cannot miss one (T-087)")
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
    parser.add_argument("--series", nargs="*", default=None, metavar="RESULT",
                        help="aggregate these committed results into the "
                             "comparison the design specifies, and contact "
                             "nobody. ⭐ The pairing lives ACROSS rotations "
                             "because RULES 4 permits one arm per tracker per "
                             "run, so the series is the unit of comparison and "
                             "no single run of it could ever have answered")
    parser.add_argument("--min-per-arm", type=int, default=20,
                        help="how many contacted subjects an arm needs before "
                             "its rate is allowed to decide anything. ⛔ Two "
                             "failures in an arm of seven move a raw rate by "
                             "0.29 and mean nothing; a threshold that fires on "
                             "that is precision on the wrong quantity")
    args = parser.parse_args()

    if args.series:
        # ⛔ Reads committed evidence and opens no socket. The control is not
        # run: there is no request to control for, and reporting a tier-0 pass
        # beside numbers nothing measured today would be a green tick over an
        # experiment that did not happen.
        series = aggregate_series(args.series)
        thin_by_stratum = {}
        for name, block in series.items():
            thin = sorted(arm for arm, row in block["pooled"].items()
                          if row["contacted"] < args.min_per_arm)
            thin_by_stratum[name] = thin
            block["underpowered_arms"] = thin
            block["supports_a_verdict"] = not thin and len(block["pooled"]) > 1
        conditions = C.collect(sample_counts={
            "strata": len(series),
            "runs": sum(len(b["runs"]) for b in series.values()),
            "subjects": sum(b["subjects"] for b in series.values()),
        })
        C.emit("Does the identity this project sends change what a tracker "
               "answers? (series aggregate, no tracker contacted)",
               conditions,
               {"series": series, "min_per_arm": args.min_per_arm,
                "difference_threshold": args.difference_threshold,
                "what_this_is_not": (
                    "Not a fresh measurement. Every row was measured by a run "
                    "committed under experiments/results/; this pools them "
                    "and contacts nobody. Strata are NOT pooled with each "
                    "other: the pilot drew subjects in corpus order and 174 "
                    "of 200 answered nobody, so adding it to the live draws "
                    "would measure the corpus rather than the identity.")},
               args.out)
        for name, block in sorted(series.items()):
            print(f"\nSTRATUM {name}  "
                  f"{len(block['runs'])} run(s), {block['subjects']} subjects")
            print(f"ARM           answered  contacted  rate")
            for arm in sorted(block["pooled"]):
                row = block["pooled"][arm]
                rate = "-" if row["rate"] is None else f"{row['rate']:.3f}"
                print(f"  {arm:12s} {row['answered']:8d}  "
                      f"{row['contacted']:9d}  {rate}")
            print(f"  spread {block['spread']}, "
                  f"{block['subjects_seen_under_two_or_more_arms']} subject(s) "
                  f"seen under two or more arms")
            for pair, cell in sorted(block["discordant"].items()):
                print(f"    paired {pair}: {cell}")
            if block["underpowered_arms"]:
                print(f"  ⚠ NO VERDICT: "
                      f"{', '.join(block['underpowered_arms'])} under "
                      f"{args.min_per_arm} contacted subjects.")
        if args.expect_arms:
            for name, block in sorted(series.items()):
                spread = block["spread"]
                if (block["supports_a_verdict"] and spread is not None
                        and spread > args.difference_threshold):
                    print(f"\nEXPECTATION FAILED: in stratum {name} the arms "
                          f"differ by {spread}, over the "
                          f"{args.difference_threshold} threshold.")
                    return C.EXIT_MEASURED_AND_FAILED
        return C.EXIT_MEASURED

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
        if args.from_sweep:
            wanted = read_live_urls(args.from_sweep)
            if not wanted:
                print("--from-sweep matched no live tracker; refusing to "
                      "probe a set chosen by nothing", file=sys.stderr)
                return C.EXIT_COULD_NOT_RUN
            subjects = [t for t in subjects if t.url in wanted]
        # ⛔ ONE RULE, TWO SOURCES OF WHEN. `politeness.too_soon_after` is
        # what the health sweep enforces (T-087); this experiment contacts the
        # same trackers and must not enforce a second, differently-wrong
        # version of the same ceiling. `--state` carries every sweep's last
        # contact, `--exclude` carries this experiment's own runs, and the
        # later of the two wins per tracker.
        contacted: dict[str, str] = {}
        if args.state:
            contacted.update(read_state_last_seen(args.state))
        if args.exclude:
            for url, when in read_last_contact(args.exclude).items():
                if when > contacted.get(url, ""):
                    contacted[url] = when
        if contacted:
            now = C.utc()
            held = [t for t in subjects
                    if too_soon_after(contacted.get(t.url), now)]
            subjects = [t for t in subjects
                        if not too_soon_after(contacted.get(t.url), now)]
            print(f"D7 holds {len(held)} of {len(held) + len(subjects)} "
                  f"subjects contacted within {DEFAULT_INTERVAL_SECONDS}s",
                  file=sys.stderr)
        subjects.sort(key=lambda t: t.url)
        if args.limit:
            subjects = subjects[:args.limit]
        if not subjects:
            # ⛔ A run that probes nothing and exits 0 is the step that did
            # nothing and reported success.
            print("no subject survived the filters; nothing was probed",
                  file=sys.stderr)
            return C.EXIT_COULD_NOT_RUN

        vantage = detect_vantage()
        resolver = Resolver()
        assignment = assign_arms([t.url for t in subjects], args.rotation)
        for tracker in subjects:
            arm = assignment[tracker.url]
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
    # ⛔ A verdict needs a sample in EVERY arm, not on average. One thin arm is
    # enough to make a spread meaningless, and the spread is what the check
    # would fire on: two failures in an arm of seven move a rate by 0.29.
    thin = sorted(name for name, row in rates.items()
                  if row["contacted"] < args.min_per_arm)
    powered = not thin and len(measured) > 1

    results = {
        "control": control,
        "rotation": args.rotation,
        "arms": rates,
        "spread": spread,
        "difference_threshold": args.difference_threshold,
        "min_per_arm": args.min_per_arm,
        "underpowered_arms": thin,
        "supports_a_verdict": powered,
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

    results["subject_selection"] = {
        "from_sweep": list(args.from_sweep or []),
        "excluded_results": list(args.exclude or []),
        "limit": args.limit,
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
    if thin:
        print(f"⚠ NO VERDICT: {', '.join(thin)} contacted fewer than "
              f"{args.min_per_arm} subjects. The rates above are reported and "
              f"they are not compared.")

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
        if powered and spread is not None and spread > args.difference_threshold:
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
