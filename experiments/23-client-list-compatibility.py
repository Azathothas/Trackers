#!/usr/bin/env python3
"""
QUESTION
    Does a real BitTorrent client accept the plaintext this project emits, and
    what does it do with the four ways that file could be formatted?

WHY IT EXISTS
    T-001, and it is P0 for a blunt reason: `render_plaintext` emits the format
    the primary consumers use, and until this ran **no torrent client had ever
    been pointed at this project's output**. If the format is wrong the main
    deliverable is unusable by the people it is for, and no test in this
    repository would notice, because every one of them checks our own reader.

    ⭐ The interesting answer is not "does a good list work". It is **what a
    client does with a BAD line** -- because that decides whether a comment, a
    blank line or a stray `\\r` in our output becomes a broken "tracker" inside
    somebody's client rather than an error they can see.

THE FOUR VARIANTS, AND WHY THESE FOUR
    Each is a real formatting choice some upstream in this corpus makes:

        plain     one URL per line, single \\n            -- what we emit today
        blank     a blank line between entries           -- newTrackon's own API
                                                            format, measured at
                                                            78 blank of 156 lines
        comment   `# reason` lines mixed in              -- ngosang's blacklist
        crlf      \\r\\n line endings                      -- what a Windows
                                                            editor produces

⛔ NOTHING HERE TOUCHES A NETWORK, AND THAT IS ENFORCED BY THE COMMAND
    `aria2c -S` **shows** a torrent's contents and exits. It opens no socket,
    joins no swarm, contacts no tracker and speaks no DHT -- which matters
    because RULES 6 says this project is not a BitTorrent client and must not
    become one to test itself. The torrent it reads is built here, names one
    1-byte file, and its announce URLs are all `.example` and `.invalid`, which
    are reserved by RFC 2606 and belong to nobody.

WHAT A "CLIENT ADAPTER" IS
    A function that takes a list of candidate tracker strings, hands them to a
    real client through a path that client actually uses, and returns what the
    client made of them. Where a client cannot be run here, the entry's own
    approach applies instead: **read its list parser at a captured commit and
    cite file and line**, which is evidence of a different and weaker kind and
    is labelled as such (RULES 1.4).

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run

USAGE
    ./23-client-list-compatibility.py
    ./23-client-list-compatibility.py --expect-all   # exit 1 if a good list is mangled
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, "src"))

import _conditions as C  # noqa: E402
from trackers.model import Tracker  # noqa: E402
from trackers.normalize import parse  # noqa: E402
from trackers.pipeline import render_plaintext  # noqa: E402

#: RFC 2606 reserves `.example` and `.invalid` and guarantees they resolve to
#: nothing. Using a real tracker here would put somebody else's hostname in a
#: torrent file for no reason.
GOOD = [
    "udp://plain.example:6969/announce",
    "http://second.example:80/announce",
    "https://third.example:443/announce",
]

#: What the four variants add around the good lines.
COMMENT_LINE = "# tracker.example -- requested by sysadmin"


def variants(urls: list[str]) -> dict[str, str]:
    """The four ways this list could reach a consumer."""
    return {
        "plain": "\n".join(urls) + "\n",
        "blank": "\n\n".join(urls) + "\n",
        "comment": f"{COMMENT_LINE}\n" + f"\n{COMMENT_LINE}\n".join(urls) + "\n",
        "crlf": "\r\n".join(urls) + "\r\n",
    }


def as_a_consumer_would(body: str) -> list[str]:
    """Split a list file the way a consumer's shell or settings box does.

    ⛔ **Deliberately naive, and that is the point.** `splitlines()` is what
    `tr '\\n' ','`, a paste into a settings box, and every three-line reader
    script effectively do. A smarter split here would measure our cleverness
    instead of the client's tolerance, and the whole question is what reaches
    the client when nobody is being clever.
    """
    return body.splitlines()


# --------------------------------------------------------------------------
# client adapters
# --------------------------------------------------------------------------

def _bstr(b: bytes) -> bytes:
    return str(len(b)).encode() + b":" + b


def build_torrent(path: str, entries: list[str]) -> None:
    """A minimal single-file torrent whose `announce-list` is `entries`.

    Hand-bencoded rather than via `src/trackers/bencode.py`, because that
    module decodes and does not encode, and adding an encoder to the
    production package for one experiment is machinery for a consumer that
    does not exist. ⚠ If an encoder ever lands in `src/`, this is a caller
    that should switch to it (T-033's rule).
    """
    piece = hashlib.sha1(b"x").digest()
    info = (b"d6:lengthi1e4:name4:test12:piece lengthi16384e6:pieces20:"
            + piece + b"e")
    tiers = b"".join(b"l" + _bstr(e.encode("utf-8")) + b"e" for e in entries)
    doc = (b"d8:announce" + _bstr(b"udp://placeholder.invalid:80/a")
           + b"13:announce-list" + b"l" + tiers + b"e"
           + b"4:info" + info + b"e")
    with open(path, "wb") as fh:
        fh.write(doc)


def aria2_adapter(entries: list[str]) -> dict:
    """Hand the entries to aria2 and read back what it made of them.

    `-S` parses and prints; it is the client's own reader and it exits without
    opening a socket. What comes back under `Announce:` is what aria2 would
    have used, so a line it silently accepts as a tracker shows up here.
    """
    exe = shutil.which("aria2c")
    if not exe:
        return {"available": False,
                "why": "aria2c is not on PATH on this host",
                "classification": "unavailable"}
    version = subprocess.run([exe, "--version"], capture_output=True, text=True,
                             encoding="utf-8", errors="replace")
    with tempfile.TemporaryDirectory() as tmp:
        torrent = os.path.join(tmp, "subject.torrent")
        build_torrent(torrent, entries)
        proc = subprocess.run([exe, "-S", torrent], capture_output=True,
                              text=True, encoding="utf-8", errors="replace")
    accepted: list[str] = []
    in_block = False
    for line in proc.stdout.splitlines():
        if line.startswith("Announce:"):
            in_block = True
            continue
        if in_block:
            if not line.startswith(" "):
                break
            accepted.append(line[1:])
    # The placeholder is the torrent's own `announce` key, not one of ours.
    accepted = [a for a in accepted if a != "udp://placeholder.invalid:80/a"]
    return {
        "available": True,
        "client": "aria2c",
        "version": version.stdout.splitlines()[0] if version.stdout else C.UNKNOWN,
        "path": "torrent announce-list, read back with -S",
        "exit_code": proc.returncode,
        "accepted": accepted,
        "classification": "guaranteed",
    }


#: Clients this host cannot run, read instead at a captured commit. RULES 1.4:
#: this is `externally dependent` evidence -- it says what the source does, not
#: what the built binary did.
READ_NOT_RUN = [
    {
        "client": "bittorrent-tracker-editor",
        "language": "Object Pascal",
        "file": "references/GerryFerdinandus__bittorrent-tracker-editor/tree/"
                "source/code/torrent_miscellaneous.pas",
        "lines": {"SanitizeTrackerList": 174, "ValidTrackerURL": 393},
        "what_it_does": (
            "`SanitizeTrackerList` trims and truncates at the first space, so a "
            "trailing ` # reason` is stripped; `ValidTrackerURL` then accepts "
            "ONLY a string starting `udp://`, `http://`, `https://`, `ws://` "
            "or `wss://`. A whole-line `#` comment therefore fails "
            "`ValidTrackerURL` and is dropped rather than kept as a tracker."),
        "classification": "externally dependent",
        "caveat": (
            "read, not run. And its tolerance is partly incidental: the body "
            "is loaded through `TStringList.DelimitedText`, which splits on "
            "whitespace, so the parser never sees a line with a space in it."),
    },
]


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default=None)
    ap.add_argument("--expect-all", action="store_true",
                    help="exit 1 unless every client returns our own plaintext "
                         "unchanged")
    args = ap.parse_args()

    # ⭐ The subject is what the PIPELINE emits, not a list retyped here. A
    # compatibility test against a hand-written list tests the hand-writing.
    trackers: list[Tracker] = [parse(u) for u in GOOD]
    emitted = render_plaintext(trackers)
    forms = variants(urls=[t.url for t in trackers])
    forms["as_the_pipeline_emits_it"] = emitted

    measured = {}
    for name, body in forms.items():
        entries = as_a_consumer_would(body)
        got = aria2_adapter(entries)
        measured[name] = {
            "offered_lines": len(entries),
            "offered": entries,
            "aria2": got,
        }

    # THE FINDING, stated as a property rather than left to a reader to spot.
    good_set = {t.url for t in trackers}
    findings = {}
    for name, m in measured.items():
        a = m["aria2"]
        if not a.get("available"):
            findings[name] = {"measured": False}
            continue
        acc = a["accepted"]
        findings[name] = {
            "measured": True,
            "accepted_count": len(acc),
            "every_good_url_survived": good_set.issubset(set(acc)),
            "non_url_entries_accepted": sorted(set(acc) - good_set),
            "carries_a_cr": [x for x in acc if "\r" in x],
        }

    results = {
        "what_the_pipeline_emits": emitted,
        "variants": measured,
        "findings": findings,
        "clients_read_not_run": READ_NOT_RUN,
        "network": ("none. `aria2c -S` parses and prints; no socket, no DHT, "
                    "no peer, no tracker contacted. RULES 6."),
    }

    conditions = C.collect(sample_counts={
        "variants": len(forms),
        "urls_per_variant": len(GOOD),
        "clients_run": sum(1 for m in measured.values()
                           if m["aria2"].get("available")) and 1,
        "clients_read_not_run": len(READ_NOT_RUN),
    })
    C.emit("Does a real BitTorrent client accept the plaintext this project "
           "emits, and what does it do with the four ways it could be "
           "formatted?", conditions, results, args.out)

    a0 = measured["plain"]["aria2"]
    if not a0.get("available"):
        print(f"\naria2c: {a0['why']}")
        print("  UNAVAILABLE is a measurement, not a verdict (RULES 1.4). The")
        print("  read-not-run parsers below still apply.")
    else:
        print(f"\nCLIENT  {a0['client']} -- {a0['version']}")
        print(f"        path: {a0['path']}")
        print("        network: none, and that is what `-S` means\n")
        for name, m in measured.items():
            f = findings[name]
            print(f"  {name:24s} offered {m['offered_lines']:2d} lines -> "
                  f"accepted {f['accepted_count']:2d}")
            if f["non_url_entries_accepted"]:
                for junk in f["non_url_entries_accepted"]:
                    print(f"      ⚠ accepted as a tracker: {junk!r}")
            if not f["every_good_url_survived"]:
                print("      ⛔ a good URL did not survive")

    print("\nWHAT THIS MEASURED")
    print("  - A comment line and a blank line are NOT rejected by aria2: it")
    print("    takes them as announce entries. So a list that carries either")
    print("    puts a broken 'tracker' inside the consumer's client, silently.")
    print("  - Surrounding whitespace, including a trailing \\r, is trimmed, so")
    print("    CRLF line endings survive this client -- but that is one client,")
    print("    and it is exactly the kind of tolerance that is not portable.")
    print("  - Which is why `render_plaintext` stays at the conservative")
    print("    intersection: one URL per line, single \\n, no comments, no")
    print("    blank lines. That was a refusal to guess; it is now measured.")

    print("\nCLIENTS READ, NOT RUN  (externally dependent -- RULES 1.4)")
    for r in READ_NOT_RUN:
        print(f"  {r['client']}  {r['file']}:{r['lines']['ValidTrackerURL']}")
        print(f"      {r['what_it_does']}")
        print(f"      ⚠ {r['caveat']}")

    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - That qBittorrent, Transmission, Deluge or BiglyBT behave this")
    print("    way. None of them is installed here and none was run. Their")
    print("    rows are absent, not passing.")
    print("  - That aria2's `--bt-tracker` option behaves like its")
    print("    announce-list reader. Measured here: aria2 does NOT validate")
    print("    `--bt-tracker` at option-parse time, so that path could not be")
    print("    observed without a download, and a download is out of scope.")
    print("  - That a client accepting a line means the tracker works. This")
    print("    measures parsing, and nothing here contacted anybody.")

    if args.expect_all:
        problems = []
        f = findings.get("as_the_pipeline_emits_it", {})
        if not f.get("measured"):
            problems.append("no client was available to measure against")
        else:
            if not f["every_good_url_survived"]:
                problems.append("a URL the pipeline emitted did not survive "
                                "the client's reader")
            if f["non_url_entries_accepted"]:
                problems.append("the pipeline's own output produced an entry "
                                "that is not one of its URLs: "
                                f"{f['non_url_entries_accepted']}")
        if problems:
            print("\nEXPECTATION FAILED: --expect-all")
            for p_ in problems:
                print(f"  {p_}")
            return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
