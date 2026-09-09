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
import datetime
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))

import _scope  # noqa: E402 - reconfigures stdout on import
from generate import _NoSources, display_path, load_corpus  # noqa: E402
from trackers import __version__  # noqa: E402
from trackers.bep34 import Resolver  # noqa: E402
from trackers.politeness import DEFAULT_INTERVAL_SECONDS  # noqa: E402
from trackers.profile import budget_for  # noqa: E402
from trackers.state import CorruptState, read_state  # noqa: E402
from trackers.sweep import (SweepConfig, plan, render_sweep,  # noqa: E402
                            slices_for, sweep, udp_budget)
from trackers.vantage import detect as detect_vantage  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def rotation_for(generated_at: str) -> int:
    """Which slice an injected instant selects.

    One step per D7 interval since the epoch, so a run three hours after
    another takes the next slice and a re-run of the same instant repeats
    exactly.

    ⛔ **A timestamp this cannot read raises**, and an earlier version returned
    0. The adversarial pass of 2026-09-08 read that consequence out loud: a typo
    in the workflow's clock would pin **every scheduled run to slice 0**, so the
    same 190 trackers would be probed eight times a day forever and the other
    1137 never -- silently, and indistinguishably from working. A run that
    cannot tell when it is has not been told, and the caller exits 2.
    """
    try:
        text = generated_at.strip()
        if text.endswith("Z"):
            text = text[:-1] + "+00:00"
        moment = datetime.datetime.fromisoformat(text)
    except ValueError as exc:
        raise ValueError(
            f"--generated-at {generated_at!r} is not an ISO 8601 instant, so "
            f"this run cannot tell which slice of the corpus it should probe"
        ) from exc
    if moment.tzinfo is None:
        moment = moment.replace(tzinfo=datetime.timezone.utc)
    epoch = int(moment.timestamp())
    return epoch // DEFAULT_INTERVAL_SECONDS


DEFAULT_FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")
DEFAULT_OUT = os.path.join(REPO, "out", "health")


def main() -> int:
    _scope.printable_stdout()
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
    ap.add_argument("--rotation", type=int, default=None,
                    help="which slice of the corpus to probe. Derived from "
                         "--generated-at when absent, so consecutive scheduled "
                         "runs walk the corpus instead of re-probing one slice")
    ap.add_argument("--state", default=None, metavar="STATE_JSONL",
                    help="the recorded history, which is what D7's ceiling is "
                         "enforced from (T-087). ⛔ Without it the ceiling "
                         "rests on the rotation's arithmetic alone, and that "
                         "arithmetic is a wall-clock bucket: two runs inside "
                         "one bucket take the same slice, which contacted 192 "
                         "trackers twice in 98 minutes on 2026-09-08. A run "
                         "given no history says so in its output rather than "
                         "implying a ceiling it did not apply.")
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
    # ⛔ Which slice of the corpus this run takes, derived from the INJECTED
    # clock so two runs three hours apart walk to the next slice and a re-run
    # of the same instant repeats exactly (RULES 3.6). Without it a scheduled
    # sweep re-probes one slice forever and every other tracker stays at one
    # observation, which is one short of anything `MIN_SAMPLES_FOR_DEATH` can
    # ever say (T-084).
    try:
        rotation = args.rotation if args.rotation is not None else rotation_for(
            args.generated_at)
    except ValueError as exc:
        print(exc, file=sys.stderr)
        return 2
    # ⛔ D7's ceiling, read from what was recorded rather than assumed from the
    # cadence (T-087). A missing file is not an error -- a first run has no
    # history and refusing to probe because a path is absent is a sweep that
    # stops measuring the day a path changes -- but it is never silent, because
    # "nobody was held" and "nobody was checked" are the same number.
    last_seen: dict[str, str] | None = None
    ceiling_note = ("⛔ no history supplied: D7 rests on the rotation alone, "
                    "which is what let two runs contact one slice twice")
    if args.state:
        if os.path.exists(args.state):
            try:
                histories, quarantined = read_state(args.state)
            except CorruptState as exc:
                # RULES 3.9: never recover by discarding. A history this run
                # cannot read is one it cannot be polite against, and probing
                # anyway would spend the ceiling it just lost the record of.
                print(f"--state {args.state}: {exc}", file=sys.stderr)
                return 2
            last_seen = {url: h.last_seen for url, h in histories.items()}
            ceiling_note = (f"{len(last_seen)} trackers with a recorded last "
                            f"contact"
                            + (f", {len(quarantined)} lines quarantined"
                               if quarantined else ""))
        else:
            # Stated, not assumed. A path that does not exist is how a first
            # run looks and also how a typo looks, and only the reader can
            # tell them apart.
            ceiling_note = (f"⚠ {display_path(args.state, REPO)} does not "
                            f"exist; treating this as a first run")

    config = SweepConfig(timeout=args.timeout, deadline_seconds=args.deadline)
    # ⛔ PREVIEW ONLY. `sweep()` selects; this script must not, or the corpus it
    # hands over IS the sample and `counts.corpus` reports the sample size as
    # the corpus. Run 33938543488 published `corpus: 200` against a corpus of
    # 1327 for exactly that reason: the sample was correct and its denominator
    # was not. One selector, one place (docs/conventions/code.md).
    #
    # ⚠ `plan` is what `sweep()` calls too, with these same inputs, and it is
    # idempotent in its own output -- so the slice previewed here is the slice
    # probed below. A preview of a different set is not a preview, which is the
    # defect run 34252497106 exposed when the dry run defaulted its clock.
    chosen_plan = plan(corpus, budget, rotation, last_seen=last_seen,
                       now=args.generated_at)
    chosen = list(chosen_plan.trackers)

    print(f"profile:      {budget.profile}")
    slices = slices_for(len(corpus), budget.sample_size or len(corpus))
    print(f"rotation:     slice {chosen_plan.rotation % slices} of {slices} "
          f"(rotation {chosen_plan.rotation})"
          + (f"  ⭐ advanced from {rotation}: slice "
             f"{rotation % slices} was entirely inside D7's interval"
             if chosen_plan.advanced else ""))
    print(f"ceiling:      {ceiling_note}")
    if chosen_plan.held_by_ceiling:
        print(f"held back:    {len(chosen_plan.held_by_ceiling)} contacted "
              f"within {DEFAULT_INTERVAL_SECONDS}s (D7); they are not probed "
              f"and no record is written for them")
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

    if chosen_plan.exhausted:
        # ⭐ The correct outcome, and it must not read as the broken one. Every
        # slice held means the whole corpus was contacted inside D7's interval,
        # so there is nobody this run is permitted to ask. Exit 0 having probed
        # nothing is normally the forbidden pattern; here the run did exactly
        # what it was asked to do, and the alternative is breaching the ceiling.
        print(f"\nevery slice is inside D7's {DEFAULT_INTERVAL_SECONDS}s "
              f"interval: {len(chosen_plan.held_by_ceiling)} trackers were "
              f"contacted too recently, so this run contacts nobody.")
        return 0

    result = sweep(corpus, config=config, budget=budget, vantage=vantage,
                   resolver=Resolver(), observed_at=args.generated_at,
                   rotation=chosen_plan.rotation, last_seen=last_seen)

    doc = render_sweep(result, generated_at=args.generated_at,
                       vantage=vantage, budget=budget, config=config)
    doc["code_version"] = __version__
    # What this run was pointed at, so a narrowed sweep can never be read as
    # a sample of the whole corpus (RULES 3.4: the conditions travel with
    # the records).
    # ⛔ The rotation ACTUALLY taken, never the one asked for. `plan` advances
    # past a slice D7 holds entirely, and a record naming the requested one
    # would say this run probed a set it did not probe. `ceiling` above carries
    # both numbers so the advance is auditable rather than invisible.
    selection["rotation"] = chosen_plan.rotation
    selection["slice"] = chosen_plan.rotation % slices
    selection["slices"] = slices
    selection["requested_rotation"] = chosen_plan.requested_rotation
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
