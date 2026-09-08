#!/usr/bin/env python3
"""Probe the corpus and write health records. T-024 and T-029.

⛔ **This is the one thing in the tree that opens sockets to other people's
servers on purpose**, so read what it is bounded by before running it:

  * **BEP 34 first.** `src/trackers/bep34.py` is consulted per host before any
    probe, and a denial or an undetermined lookup skips the tracker. There is
    no flag that turns that off. RULES 4.
  * **Never announces.** The probe stops at BEP 15 connect and HTTP scrape;
    there is no announce code path to reach.
  * **One connection per host at a time**, in both profiles, not configurable.
  * **A concurrency bound, a per-attempt timeout and a whole-run deadline**,
    all from `src/trackers/sweep.py`.

⚠ **`ci` is the default profile on every host, including yours** (RULES 15.1),
so an unqualified run probes a **sample** rather than the corpus. That is
deliberate: the expensive mistake available here is a full sweep fired by
accident.

```sh
TRACKERS_PROFILE=local python3 scripts/probe-corpus.py --out out/health
```

⛔ **There is no offline mode, and its absence is the honest answer.** A run
that opened no socket could still emit a record per tracker saying `unknown`,
and that file would satisfy `scripts/check-vantage-metadata.py` while nothing
had been measured -- a green tick over nothing, which is the forbidden pattern
about a step that exits 0 having done what it was not asked to do. Use
`--dry-run` to see what *would* be probed; it writes nothing.

Exit codes:
    0  probed and wrote records
    1  wrote nothing because a check failed
    2  could not run
"""

from __future__ import annotations

import argparse
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))

from generate import _NoSources, display_path, load_corpus  # noqa: E402
from trackers import __version__  # noqa: E402
from trackers.bep34 import Resolver  # noqa: E402
from trackers.profile import budget_for  # noqa: E402
from trackers.sweep import (SweepConfig, render_sweep, select,  # noqa: E402
                            sweep, udp_budget)
from trackers.vantage import detect as detect_vantage  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")
DEFAULT_OUT = os.path.join(REPO, "out", "health")


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default=DEFAULT_OUT,
                    help="directory for health.json")
    ap.add_argument("--fixtures", default=DEFAULT_FIXTURES)
    ap.add_argument("--offline-corpus", action="store_true",
                    help="build the tracker list from committed fixtures "
                         "instead of fetching sources. The PROBE still needs "
                         "a network; this only avoids re-fetching the lists.")
    ap.add_argument("--timeout", type=float, default=5.0)
    ap.add_argument("--deadline", type=float, default=None,
                    help="seconds for the whole run. Anything not reached is "
                         "recorded `unknown`, never `dead`.")
    ap.add_argument("--generated-at", default="1970-01-01T00:00:00Z",
                    help="INJECTED clock (RULES 3.6)")
    ap.add_argument("--dry-run", action="store_true",
                    help="print what would be probed and write nothing")
    ap.add_argument("--only-source", default=None, metavar="SOURCE_ID",
                    help="narrow the corpus to trackers this source "
                         "contributed, by provenance. Aims the request budget "
                         "at one question instead of spending it uniformly "
                         "(T-034). It narrows WHAT is probed and changes "
                         "nothing about HOW: BEP 34, the per-host rule, the "
                         "concurrency bound and the deadline all still apply.")
    ap.add_argument("--only-host", default=None, metavar="HOSTNAME",
                    help="narrow the corpus to one hostname. The same "
                         "narrowing as --only-source and the smallest one "
                         "available: it is how a single host's behaviour is "
                         "demonstrated without spending the request budget on "
                         "everybody else (T-037).")
    args = ap.parse_args()

    try:
        agg, _, _ = load_corpus(args.offline_corpus, args.fixtures)
    except _NoSources as exc:
        print(exc, file=sys.stderr)
        return 2

    if not agg.trackers:
        print("the corpus is empty; refusing to report on nothing",
              file=sys.stderr)
        return 2

    corpus = agg.trackers
    selection = {"mode": "whole corpus", "corpus_before_selection": len(corpus)}
    if args.only_source:
        known = sorted({s for srcs in agg.provenance.values() for s in srcs})
        if args.only_source not in known:
            # ⛔ Never silently probe nothing. A run that matched no tracker and
            # exited 0 is the forbidden pattern about a step that exits 0
            # having done nothing it was asked to do, and here it would also
            # publish an empty sweep as if it were a measured one.
            print(f"--only-source {args.only_source!r} contributed nothing to "
                  f"this corpus. Known: {', '.join(known)}", file=sys.stderr)
            return 2
        corpus = [t for t in corpus
                  if args.only_source in agg.provenance.get(t.url, ())]
        selection = {
            "mode": "one source, by provenance",
            "source": args.only_source,
            "corpus_before_selection": len(agg.trackers),
            "selected_by_provenance": len(corpus),
        }
        if not corpus:
            print(f"--only-source {args.only_source!r} matched no tracker",
                  file=sys.stderr)
            return 2

    if args.only_host:
        wanted = args.only_host.lower()
        narrowed = [t for t in corpus if t.host.lower() == wanted]
        if not narrowed:
            # Same rule as --only-source: a run that matched nothing and
            # exited 0 would publish an empty sweep as a measured one.
            print(f"--only-host {args.only_host!r} matched no tracker in this "
                  f"corpus of {len(corpus)}", file=sys.stderr)
            return 2
        selection = dict(selection, mode="one host", host=wanted,
                         selected_by_host=len(narrowed),
                         corpus_before_selection=len(agg.trackers))
        corpus = narrowed

    budget = budget_for()
    vantage = detect_vantage()
    config = SweepConfig(timeout=args.timeout, deadline_seconds=args.deadline)
    # ⛔ PREVIEW ONLY. `sweep()` selects; this script must not, or the corpus it
    # hands over IS the sample and `counts.corpus` reports the sample size as
    # the corpus. Run 33938543488 published `corpus: 200` against a corpus of
    # 1327 for exactly that reason: the sample was correct and its denominator
    # was not. One selector, one place (docs/conventions/code.md).
    chosen = select(corpus, budget)

    print(f"profile:      {budget.profile}")
    print(f"vantage:      {vantage.environment_class}, "
          f"families {list(vantage.ip_families)}")
    print(f"corpus:       {len(corpus)}"
          f"{'' if not args.only_source else f' (of {len(agg.trackers)}, '
            f'contributed by {args.only_source})'}")
    # "a sample" is only true when something was left out. Saying it over a
    # census is the same class of mislabelled denominator as `counts.corpus`
    # reporting the sample size, and it is worth the extra branch to not.
    print(f"selected:     {len(chosen)}"
          f"{'' if len(chosen) == len(corpus) else ' (a sample; RULES 15.2)'}")
    print(f"concurrency:  {budget.max_concurrency} hosts, 1 connection per host")
    print(f"udp budget:   {udp_budget(args.timeout):.2f}s worst case per tracker")
    print(f"deadline:     {args.deadline if args.deadline else 'none'}")

    if args.dry_run:
        print("\n--dry-run: nothing was probed and nothing was written.")
        return 0

    if vantage.environment_class == "authoring-sandbox-proxied":
        # C-62. A header-sensitive measurement taken through a proxy measures
        # the proxy too, and a record that says otherwise is worse than none.
        print("\nrefusing to probe from a proxied vantage (C-62): the results "
              "would measure the egress proxy as well as the tracker.",
              file=sys.stderr)
        return 2

    result = sweep(corpus, config=config, budget=budget, vantage=vantage,
                   resolver=Resolver(), observed_at=args.generated_at)

    doc = render_sweep(result, generated_at=args.generated_at,
                       vantage=vantage, budget=budget, config=config)
    doc["code_version"] = __version__
    # What this run was pointed at, so a narrowed sweep can never be read as
    # a sample of the whole corpus (RULES 3.4: the conditions travel with
    # the records).
    doc["selection"] = selection

    os.makedirs(args.out, exist_ok=True)
    path = os.path.join(args.out, "health.json")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(doc, fh, indent=2, sort_keys=True)
        fh.write("\n")

    print(f"\nprobed {result.probed}, refused or undetermined {result.refused}, "
          f"unmeasurable {result.unmeasurable}, "
          f"not reached {result.not_reached}")
    print(f"states: {result.states()}")
    print(f"wrote -> {display_path(path, REPO)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
