#!/usr/bin/env python3
"""Gate: the TODO record has to agree with itself.

`TODO/RULES.md` section 7: counts are never maintained by hand. Closing one
entry moves several numbers -- the index row, the counts line, one row of the
priority table, that row's total, the overall row -- and doing that arithmetic
by hand is how a record starts lying.

What it asserts, independently of whatever wrote the files:

  * every index row names an entry that exists, and every entry has a row;
  * no id is duplicated, anywhere;
  * the status in the index matches the status in the entry;
  * the declared counts and the priority table agree with the rows;
  * every entry carries the required fields;
  * a `done` entry records its acceptance, and a `blocked` entry names what
    would unblock it (RULES 8: nothing closes as won't-fix);
  * every `TODO/*.md` link resolves.

Exit codes:
    0  the record agrees with itself
    1  it does not; every disagreement is printed
    2  the record could not be read
"""

from __future__ import annotations

import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TODO = os.path.join(REPO, "TODO")
INDEX = os.path.join(TODO, "INDEX.md")

PRIORITIES = ("P0", "P1", "P2", "P3")
STATUSES = ("open", "blocked", "done")

REQUIRED_FIELDS = ("Source:", "Category:", "Priority:", "Effort:", "Status:",
                   "Problem:", "Premise:", "Approach:", "Prove:")

# | [T-001](claims.md) | P0 | claims | open | Title |
ROW = re.compile(
    r"^\|\s*\[(T-\d+)\]\(([^)]+)\)\s*\|\s*(P[0-3])\s*\|\s*([a-z-]+)\s*\|"
    r"\s*\**([a-z]+)\**\s*\|\s*(.+?)\s*\|\s*$")

# ### T-001 Title
HEADING = re.compile(r"^###\s+(T-\d+)\s+(.*?)\s*$")


def read(path: str) -> str:
    with open(path, encoding="utf-8") as fh:
        return fh.read()


def parse_entries() -> tuple[dict, list[str]]:
    """Every entry in every TODO/*.md, keyed by id."""
    entries, problems = {}, []
    for fn in sorted(os.listdir(TODO)):
        if not fn.endswith(".md") or fn in ("INDEX.md", "PROGRESS.md", "RULES.md"):
            continue
        text = read(os.path.join(TODO, fn))
        blocks = re.split(r"^(?=###\s+T-\d+)", text, flags=re.M)
        for block in blocks:
            m = HEADING.match(block.splitlines()[0] if block.splitlines() else "")
            if not m:
                continue
            tid, title = m.group(1), m.group(2)
            if tid in entries:
                problems.append(f"{tid}: defined twice "
                                f"({entries[tid]['file']} and {fn})")
                continue
            sm = re.search(r"^Status:\s*\**([a-z]+)\**", block, flags=re.M)
            pm = re.search(r"^Priority:\s*(P[0-3])", block, flags=re.M)
            cm = re.search(r"^Category:\s*([a-z-]+)", block, flags=re.M)
            entries[tid] = {
                "file": fn, "title": title, "block": block,
                "status": sm.group(1) if sm else None,
                "priority": pm.group(1) if pm else None,
                "category": cm.group(1) if cm else None,
            }
    return entries, problems


def main() -> int:
    if not os.path.isdir(TODO) or not os.path.exists(INDEX):
        print(f"no TODO record at {TODO}", file=sys.stderr)
        return 2

    index_text = read(INDEX)
    entries, problems = parse_entries()

    rows = {}
    for line in index_text.splitlines():
        m = ROW.match(line)
        if not m:
            continue
        tid, target, prio, cat, status, title = m.groups()
        if tid in rows:
            problems.append(f"{tid}: appears twice in INDEX.md")
            continue
        rows[tid] = {"file": target, "priority": prio, "category": cat,
                     "status": status, "title": title}

    if not rows:
        print("no index rows parsed; the table shape has changed",
              file=sys.stderr)
        return 2

    # --- rows and entries correspond ----------------------------------------
    for tid, row in sorted(rows.items()):
        e = entries.get(tid)
        if e is None:
            problems.append(f"{tid}: in INDEX.md but no entry exists")
            continue
        if e["file"] != row["file"]:
            problems.append(f"{tid}: INDEX links {row['file']}, entry is in "
                            f"{e['file']}")
        if e["status"] != row["status"]:
            problems.append(f"{tid}: INDEX says {row['status']!r}, entry says "
                            f"{e['status']!r}")
        if e["priority"] != row["priority"]:
            problems.append(f"{tid}: INDEX says {row['priority']}, entry says "
                            f"{e['priority']}")
        if e["category"] != row["category"]:
            problems.append(f"{tid}: INDEX says category {row['category']!r}, "
                            f"entry says {e['category']!r}")

    for tid in sorted(set(entries) - set(rows)):
        problems.append(f"{tid}: entry exists but has no INDEX row")

    # --- every entry is well formed -----------------------------------------
    for tid, e in sorted(entries.items()):
        for field in REQUIRED_FIELDS:
            if not re.search(rf"^{re.escape(field)}", e["block"], flags=re.M):
                problems.append(f"{tid}: missing required field {field!r}")
        if e["status"] not in STATUSES:
            problems.append(f"{tid}: status {e['status']!r} is not one of "
                            f"{STATUSES}")
        if e["status"] == "done" and "**Done." not in e["block"]:
            problems.append(f"{tid}: marked done without a '**Done.'"
                            f" paragraph recording what closed it")
        if e["status"] == "blocked":
            if not re.search(r"^Blocker:", e["block"], flags=re.M):
                problems.append(f"{tid}: blocked without a 'Blocker:' field")
            if not re.search(r"^Unblocked by:", e["block"], flags=re.M):
                problems.append(f"{tid}: blocked without 'Unblocked by:' "
                                f"(RULES 8: nothing closes as won't-fix)")
        for bad in ("won't fix", "wontfix", "out of scope"):
            if bad in e["block"].lower():
                problems.append(f"{tid}: contains {bad!r}; RULES 8 forbids it")
        # An entry that cites itself as its own source says nothing. Eighteen
        # did on 2026-08-31, all produced by a substitution pass that rewrote
        # the brief's section number into the id of the entry that replaced it
        # -- so the provenance of every one of them was destroyed by the very
        # edit that was supposed to preserve it.
        source = re.search(r"^Source:\s*(.+)$", e["block"], flags=re.M)
        # `T-030` and `T-030, items 3-18` are the same defect: the id names the
        # entry, so it cannot also name where the entry came from.
        if source and re.match(rf"^{re.escape(tid)}\b", source.group(1).strip()):
            problems.append(f"{tid}: `Source:` is its own id, which records "
                            f"nothing. Name where the requirement came from.")

    # --- the definition of done agrees with the entries ---------------------
    #
    # `HISTORY/gates.md` carries a checklist whose items name entries. An item
    # left unticked while its entry is `done` -- or ticked while its entry is
    # open -- is the same defect as a count that disagrees with its rows, and
    # it is worse in one way: the checklist is what a reader consults to decide
    # whether the project is finished. Three items disagreed on 2026-08-31.
    gates_path = os.path.join(os.path.dirname(TODO), "HISTORY", "gates.md")
    try:
        with open(gates_path, encoding="utf-8") as fh:
            gates_text = fh.read()
    except OSError:
        gates_text = ""
    for line in gates_text.splitlines():
        m = re.match(r"^- \[( |x)\] (.*)$", line)
        if not m:
            continue
        ticked = m.group(1) == "x"
        named = re.findall(r"\bT-(\d{3})\b", m.group(2))
        if not named:
            continue
        # An item may cite several entries; it is satisfied only when all of
        # them are, and an item citing a done entry as its blocker is wrong.
        states = [entries[f"T-{n}"]["status"] for n in named if f"T-{n}" in entries]
        if not states:
            continue
        all_done = all(s == "done" for s in states)
        if all_done and not ticked:
            problems.append(
                f"HISTORY/gates.md: item is unticked but every entry it names "
                f"is done ({', '.join('T-' + n for n in named)}): "
                f"{m.group(2)[:70]}")
        if ticked and not all_done and "done" not in line:
            open_ids = [f"T-{n}" for n, s in zip(named, states) if s != "done"]
            problems.append(
                f"HISTORY/gates.md: item is ticked but {', '.join(open_ids)} "
                f"is not done: {m.group(2)[:70]}")

    # --- links resolve ------------------------------------------------------
    for fn in sorted(os.listdir(TODO)):
        if not fn.endswith(".md"):
            continue
        # Code spans are stripped first. Markdown does not linkify inside
        # backticks, so `[int](2.65)` in a note about PowerShell rounding is a
        # specimen and not a link to a file called 2.65. Reported as broken on
        # 2026-09-01, which was a false positive; `check-citations.py` had the
        # same defect and carries the same fix.
        body = re.sub(r"`[^`\n]*`", " ", read(os.path.join(TODO, fn)))
        for target in re.findall(r"\]\(([^)#][^)]*)\)", body):
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            resolved = os.path.normpath(os.path.join(TODO, target.split("#")[0]))
            if not os.path.exists(resolved):
                problems.append(f"TODO/{fn}: link does not resolve: {target}")

    # --- counts derived from the rows ---------------------------------------
    total = len(rows)
    by_status = {s: sum(1 for r in rows.values() if r["status"] == s)
                 for s in STATUSES}
    table = {p: {s: sum(1 for r in rows.values()
                        if r["priority"] == p and r["status"] == s)
                 for s in STATUSES} for p in PRIORITIES}

    m = re.search(r"^\*\*Counts:\*\*\s*(\d+)\s*entries\s*, \s*(\d+)\s*open"
                  r"\s*, \s*(\d+)\s*blocked\s*, \s*(\d+)\s*done\s*$",
                  index_text, flags=re.M)
    if not m:
        problems.append("no '**Counts:** N entries, N open, N blocked, "
                        "N done' line in INDEX.md")
    else:
        declared = tuple(int(g) for g in m.groups())
        actual = (total, by_status["open"], by_status["blocked"],
                  by_status["done"])
        if declared != actual:
            problems.append(f"counts line disagrees with rows: declared "
                            f"entries/open/blocked/done={declared}, "
                            f"actual={actual}")

    for p in PRIORITIES:
        pm = re.search(rf"^\|\s*{p}\s*\|\s*(\d+)\s*\|\s*(\d+)\s*\|\s*(\d+)"
                       rf"\s*\|\s*(\d+)\s*\|\s*$", index_text, flags=re.M)
        want = (table[p]["open"], table[p]["blocked"], table[p]["done"],
                sum(table[p].values()))
        if not pm:
            problems.append(f"priority table has no row for {p}")
        elif tuple(int(g) for g in pm.groups()) != want:
            problems.append(f"priority table row {p} disagrees with rows: "
                            f"declared={tuple(int(g) for g in pm.groups())}, "
                            f"actual={want}")

    am = re.search(r"^\|\s*\*\*All\*\*\s*\|\s*\*\*(\d+)\*\*\s*\|\s*\*\*(\d+)"
                   r"\*\*\s*\|\s*\*\*(\d+)\*\*\s*\|\s*\*\*(\d+)\*\*\s*\|\s*$",
                   index_text, flags=re.M)
    want_all = (by_status["open"], by_status["blocked"], by_status["done"],
                total)
    if not am:
        problems.append("priority table has no **All** row")
    elif tuple(int(g) for g in am.groups()) != want_all:
        problems.append(f"priority table **All** row disagrees: "
                        f"declared={tuple(int(g) for g in am.groups())}, "
                        f"actual={want_all}")

    # --- report -------------------------------------------------------------
    print(f"TODO record: {total} entries across "
          f"{len({e['file'] for e in entries.values()})} category files")
    print(f"  open {by_status['open']}, blocked {by_status['blocked']}, "
          f"done {by_status['done']}")
    for p in PRIORITIES:
        print(f"  {p}: open {table[p]['open']}, blocked "
              f"{table[p]['blocked']}, done {table[p]['done']}, "
              f"total {sum(table[p].values())}")

    if problems:
        print(f"\nFAIL  {len(problems)} disagreement(s):")
        for p in problems:
            print(f"  - {p}")
        return 1
    print("\nOK  index agrees with entries; counts derived from the rows "
          "match; every entry is well formed; every link resolves.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
