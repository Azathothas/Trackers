#!/usr/bin/env python3
"""Gate: the decision record's counts must agree with its rows.

RULES 7 requires that "counts must agree with rows, enforced by a checker
that runs as a gate -- not by hand". This is that checker.

The reason it exists is not tidiness. A hand-maintained count drifts the moment
somebody adds an entry and forgets the summary line, and a document whose own
arithmetic is wrong trains its readers to skim past the rest of it.

It also enforces the rule that carries actual weight: RULES 7 says
"nothing closes as 'won't fix' or 'out of scope'". A blocked entry stays open
with its blocker named. This checker fails if an entry is marked blocked
without saying what would unblock it.

Exit codes:
    0  the record is self-consistent
    1  it is not -- the mismatch is printed
    2  the record could not be read
"""

from __future__ import annotations

import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RECORD = os.path.join(REPO, "HISTORY", "decisions.md")


def main() -> int:
    if not os.path.exists(RECORD):
        print(f"decision record not found: {RECORD}", file=sys.stderr)
        return 2
    with open(RECORD, encoding="utf-8") as fh:
        text = fh.read()

    # Index rows look like: | D1 | ... | P0 | **closed** |
    rows = re.findall(r"^\|\s*(D\d+)\s*\|([^|]*)\|([^|]*)\|([^|]*)\|\s*$",
                      text, flags=re.M)
    if not rows:
        print("no index rows found; the index table shape has changed",
              file=sys.stderr)
        return 2

    ids = [r[0] for r in rows]
    statuses = [r[3].strip().lower() for r in rows]

    total = len(rows)
    closed = sum(1 for s in statuses if "closed" in s)
    openish = sum(1 for s in statuses if "open" in s)
    blocked = sum(1 for s in statuses if "blocked" in s)

    problems: list[str] = []

    # 1. ids unique and never renumbered
    if len(set(ids)) != len(ids):
        dupes = sorted({i for i in ids if ids.count(i) > 1})
        problems.append(f"duplicate decision ids: {dupes}")

    # 2. every status is one of the two allowed states
    for did, s in zip(ids, statuses):
        if "closed" not in s and "open" not in s:
            problems.append(f"{did}: status {s!r} is neither open nor closed")
        if "won't fix" in s or "wontfix" in s or "out of scope" in s:
            problems.append(
                f"{did}: RULES 7 forbids closing as won't-fix/out-of-scope")

    # 3. the declared counts must match the rows
    m = re.search(r"\*\*Counts:\*\*\s*(\d+)\s*entries?\s*, \s*(\d+)\s*closed"
                  r"\s*, \s*(\d+)\s*open\s*, \s*(\d+)\s*blocked", text)
    if not m:
        problems.append("no '**Counts:** N entries, N closed, N open, N "
                        "blocked' line found")
    else:
        declared = tuple(int(g) for g in m.groups())
        actual = (total, closed, openish, blocked)
        if declared != actual:
            problems.append(
                f"counts disagree with rows: declared entries/closed/open/"
                f"blocked = {declared}, actual = {actual}")

    # 4. every id in the index has a section, and vice versa
    sections = set(re.findall(r"^##\s*(D\d+)\b", text, flags=re.M))
    for did in ids:
        if did not in sections:
            problems.append(f"{did} is in the index but has no section")
    for did in sections - set(ids):
        problems.append(f"{did} has a section but is not in the index")

    # 5. a blocked entry must name what would unblock it
    for did, s in zip(ids, statuses):
        if "blocked" not in s:
            continue
        sec = re.search(rf"^##\s*{did}\b.*?(?=^##\s|\Z)", text,
                        flags=re.M | re.S)
        body = sec.group(0).lower() if sec else ""
        if "unblock" not in body:
            problems.append(
                f"{did} is blocked but its section never says what would "
                f"unblock it (RULES 7)")

    # 6. a closed entry owes rejected alternatives -- that is the whole point
    for did, s in zip(ids, statuses):
        if "closed" not in s:
            continue
        sec = re.search(rf"^##\s*{did}\b.*?(?=^##\s|\Z)", text,
                        flags=re.M | re.S)
        body = sec.group(0).lower() if sec else ""
        if "rejected" not in body:
            problems.append(
                f"{did} is closed without recorded rejected alternatives "
                f"(HISTORY/decisions.md, RULES 9)")

    print(f"decision record: {RECORD}")
    print(f"  entries {total}, closed {closed}, open {openish}, "
          f"blocked {blocked}")
    print(f"  ids: {', '.join(ids)}")

    if problems:
        print("\nFAIL")
        for p in problems:
            print(f"  - {p}")
        return 1
    print("\nOK  counts agree with rows; every closed entry records its "
          "rejected alternatives; every blocked entry names its unblocker.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
