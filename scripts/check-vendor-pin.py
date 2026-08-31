#!/usr/bin/env python3
"""Gate: the vendored helpers still are what the pin says they are.

`scripts/vendor/toolkit/` holds four files fetched from `Azathothas/ToolKit`
at the commit `PIN.json` records. They are not this project's code and are not
edited here; `docs/methodology/vendoring.md` is the rule for when that changes.

THE DEFECT THIS EXISTS TO CATCH
    A pinned copy that quietly stops matching its pin. Three ways it happens,
    and none of them announces itself:

      - somebody edits the vendored file to fix something local, and the pin
        now describes bytes that are not on disk, so the next reconciliation
        against a newer upstream silently discards the fix;
      - somebody bumps `ref` in `PIN.json` without re-fetching, and the record
        names a commit the tree does not hold;
      - a checkout mangles a line ending. `.gitattributes` keeps `.ps1` as
        CRLF in the working tree and LF in the index, which is why the
        recorded digest is of the bytes the raw endpoint serves, and why this
        check normalises before it compares.

WHAT IT DOES NOT DO
    It never fetches. A gate that reaches the network is red whenever somebody
    else's host is, and a check that downloads the thing it is judging is not a
    check. Comparing against a newer upstream is a deliberate act, and
    `docs/methodology/template-sync.md` is the procedure for it.

Usage:
    python3 scripts/check-vendor-pin.py [--json]

Exit codes:
    0  every vendored file matches its recorded digest
    1  at least one does not
    2  the check could not run
"""

from __future__ import annotations

import hashlib
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _scope  # noqa: E402

PIN = os.path.join("scripts", "vendor", "toolkit", "PIN.json")


def digest(path: str) -> str:
    """SHA-256 of the file with CRLF normalised to LF.

    The recorded digest is of what the raw endpoint serves, which is LF. A
    working tree may hold CRLF for a `.ps1` by `.gitattributes`, and comparing
    those bytes directly would fail on every Windows checkout and pass on
    every Linux one, which is the worst available outcome for a guard.
    """
    with open(path, "rb") as fh:
        raw = fh.read()
    return hashlib.sha256(raw.replace(b"\r\n", b"\n")).hexdigest()


def main(argv: list[str]) -> int:
    json_mode = "--json" in argv
    pin_path = os.path.join(_scope.REPO, PIN)
    if not os.path.isfile(pin_path):
        raise _scope.CouldNotRun("%s does not exist" % PIN)
    try:
        with open(pin_path, encoding="utf-8") as fh:
            pin = json.load(fh)
    except (OSError, ValueError) as exc:
        raise _scope.CouldNotRun("%s is not readable JSON: %s" % (PIN, exc))

    files = pin.get("files") or {}
    if not files:
        raise _scope.CouldNotRun("%s records no files" % PIN)

    report: list[str] = []
    base = os.path.dirname(pin_path)
    for name, meta in sorted(files.items()):
        local = os.path.join(base, name)
        if not os.path.isfile(local):
            report.append(
                "%s is in the pin and not in the tree. Re-fetch it from %s at "
                "%s, or remove the row." % (name, pin["repository"], pin["ref"]))
            continue
        actual = digest(local)
        if actual != meta["sha256"]:
            report.append(
                "%s does not match the pin.\n      recorded %s\n      on disk "
                " %s\n      Either it was edited here, in which case "
                "docs/methodology/vendoring.md\n      says to record the "
                "change, or the pin was moved without re-fetching."
                % (name, meta["sha256"], actual))

    on_disk = {f for f in os.listdir(base)
               if f.endswith((".sh", ".ps1"))}
    for extra in sorted(on_disk - set(files)):
        report.append(
            "%s is in the tree and not in the pin. An unpinned vendored file "
            "is code nobody reviewed." % extra)

    return _scope.emit(
        json_mode, "check-vendor-pin/1", len(report), report,
        "vendor pin ok: %d file(s) match %s at %s"
        % (len(files), pin["repository"], pin["ref"][:12]),
        files=len(files))


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except _scope.CouldNotRun as exc:
        print("check-vendor-pin: %s" % exc, file=sys.stderr)
        sys.exit(2)
