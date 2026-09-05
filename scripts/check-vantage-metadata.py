#!/usr/bin/env python3
"""Gate: no health record may omit its vantage, and nothing unmeasurable may be dead.

This is the `Prove` clause of decision D2 (`HISTORY/decisions.md`), and it
enforces the two requirements that make a single-vantage dataset honest rather
than merely convenient:

  1. RULES 3.4 -- every health record carries vantage metadata. A latency
     or a liveness flag without the vantage it was taken from is the
     "confident wrongness" failure RULES 3 names: the consumer reads "dead" and the
     data means "dead from AS8075".

  2. RULES 3.1 requirement 1 -- a tracker on a transport or network this
     project cannot measure MUST be `unmeasurable` and MUST NOT be `dead`.
     Measured basis: runners have no IPv6 egress (C-04), and i2p/yggdrasil
     need routers this environment does not have (C-37).

IT DOES NOT PASS VACUOUSLY.

That is deliberate and it is the whole reason this file reads the way it does.
Health records are a P2 deliverable and do not exist yet. A checker that
returns 0 over an empty set would report "vantage metadata is present on every
record" while checking nothing, and that green tick is worse than no checker:
it is a false assurance that survives into the phase where it matters. So when
there is nothing to check it exits 2 -- "could not run" -- per the exit-code
vocabulary this project uses everywhere else.

Exit codes:
    0  health records exist and every one satisfies both requirements
    1  a record violates one of them
    2  there are no health records to check yet
"""

from __future__ import annotations

import glob
import json
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Where P2 will emit health records. Checked in order; first non-empty wins.
CANDIDATE_GLOBS = [
    os.path.join(REPO, "data", "**", "*.json"),
    os.path.join(REPO, "out", "**", "*.json"),
    os.path.join(REPO, "build", "**", "*.json"),
]

REQUIRED_VANTAGE_FIELDS = {
    "environment_class",   # github-actions-hosted vs anything else
    "probe_version",       # which code took the measurement
    "ip_families",         # what the probe could actually reach (C-04)
}

# Established by measurement, not by taste. See TODO/RULES.md C-04, C-37, C-36.
UNMEASURABLE_NETWORKS = {"i2p", "yggdrasil", "onion"}
UNMEASURABLE_TRANSPORTS = {"ws", "wss"}
FORBIDDEN_STATE_FOR_UNMEASURABLE = {"dead", "live", "degraded"}


def globs_for(root: str | None) -> list[str]:
    """Where to look. `root` overrides the tree's own output directories.

    A sweep writes into a scratch directory rather than the tree -- running a
    check must not dirty the working copy, which RULES 10.3 step 6 needs and
    which the offline census once broke -- so the gate points this at the
    directory the sweep just wrote.
    """
    if root is None:
        return list(CANDIDATE_GLOBS)
    return [os.path.join(root, "**", "*.json")]


def iter_records(patterns=None):
    for pattern in (CANDIDATE_GLOBS if patterns is None else patterns):
        files = sorted(glob.glob(pattern, recursive=True))
        found = False
        for path in files:
            try:
                with open(path, encoding="utf-8") as fh:
                    doc = json.load(fh)
            except Exception:
                continue
            recs = doc.get("trackers") if isinstance(doc, dict) else None
            if isinstance(recs, list):
                found = True
                for r in recs:
                    if isinstance(r, dict):
                        yield path, r
        if found:
            return


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    root = None
    if "--path" in argv:
        i = argv.index("--path")
        if i + 1 >= len(argv):
            print("--path needs a directory", file=sys.stderr)
            return 2
        root = argv[i + 1]
        if not os.path.isdir(root):
            print(f"--path {root!r} is not a directory", file=sys.stderr)
            return 2

    patterns = globs_for(root)
    problems: list[str] = []
    seen = 0

    for path, rec in iter_records(patterns):
        seen += 1
        rel = _display(path, root)
        url = rec.get("url", "<no url>")

        vantage = rec.get("vantage")
        if not isinstance(vantage, dict):
            problems.append(f"{rel}: {url}: no `vantage` object (RULES 3.4)")
        else:
            missing = REQUIRED_VANTAGE_FIELDS - set(vantage)
            if missing:
                problems.append(
                    f"{rel}: {url}: vantage missing {sorted(missing)}")

        state = str(rec.get("health_state", "")).lower()
        network = str(rec.get("network", "")).lower()
        transport = str(rec.get("transport", "")).lower()

        unmeasurable_here = (network in UNMEASURABLE_NETWORKS
                             or transport in UNMEASURABLE_TRANSPORTS
                             or rec.get("ipv6_only") is True)
        if unmeasurable_here and state in FORBIDDEN_STATE_FOR_UNMEASURABLE:
            problems.append(
                f"{rel}: {url}: transport={transport or '-'} "
                f"network={network or '-'} is not measurable from this "
                f"vantage, but health_state={state!r}. RULES 3.1 "
                f"requirement 1 requires `unmeasurable`.")

        if state and rec.get("measurement_rung") in (None, ""):
            problems.append(
                f"{rel}: {url}: health_state without measurement_rung "
                f"(the ladder in TODO/measurement.md -- a state with no rung is unfalsifiable)")

    if seen == 0:
        print("no health records found.")
        print()
        print("COULD NOT RUN (exit 2), deliberately.")
        print("  Health records are a P2 deliverable and do not exist yet.")
        print("  Returning 0 here would report 'every record carries vantage")
        print("  metadata' while checking nothing -- a green tick that means")
        print("  nothing and would survive into the phase where it matters.")
        print()
        print("  Searched:")
        for p in patterns:
            print(f"    {_display(p, root)}")
        return 2

    print(f"checked {seen} health records")
    if problems:
        print("\nFAIL")
        for p in problems:
            print(f"  - {p}")
        return 1
    print("\nOK  every record carries vantage metadata and a measurement rung; "
          "nothing unmeasurable is reported live, dead or degraded.")
    return 0


def _display(path: str, root: str | None) -> str:
    """A path to print. `relpath` raises across drives on Windows."""
    for start in ([root] if root else []) + [REPO]:
        try:
            return os.path.relpath(path, start)
        except ValueError:
            continue
    return os.path.abspath(path)


if __name__ == "__main__":
    sys.exit(main())
