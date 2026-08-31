#!/usr/bin/env python3
"""Gate: does anything in this tree carry something that must not be published.

This repository is public. `docs/security/secrets.md` is the rule; this is the
mechanical half of it.

WHAT IT LOOKS AT
    Tracked plus untracked-but-not-ignored, not tracked alone. A file that has
    never been staged is exactly when a new file is likeliest to carry a
    credential, and it is what the next `git add -A` would take.

    `references/` is out of scope, and that exemption is load-bearing rather
    than convenient. The corpus is ten captured upstream repositories: it
    contains other people's committed email addresses, other people's example
    URLs with credentials in them, and other people's commit hashes, none of
    which this project may edit and all of which are already public at the
    commits `references/PROVENANCE.md` records. Including it produced 1649
    lines of somebody else's content and nothing of ours.
    `scripts/_scope.py` carries the general form of the rule.

WHAT IT CANNOT DO
    It finds the shapes it knows, and a green run is not a clearance. It
    cannot find a password that looks like a word, and it will not tell you
    that a file of correct-looking examples describes a real system. The sweep
    narrows the reading; it does not replace it.

    A generic high-entropy rule is deliberately absent. It fires on hashes,
    on base64 fixtures and on minified code, and a check that cries wolf is a
    check somebody switches off.

--public adds the rules that only matter for a repository that will be
public: email addresses, absolute home paths and long hex identifiers. This
project is public, so the local gate passes it.

Usage:
    python3 scripts/check-no-secrets.py [--public] [--json]

Exit codes:
    0  no secret shape found
    1  at least one category matched
    2  the check could not run
"""

from __future__ import annotations

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _scope  # noqa: E402

# A file whose whole purpose is to hold a credential. The strongest signal
# there is: not a value that looks like a secret, but a file that is one.
CREDENTIAL_FILE = re.compile(
    r"(^|/)(\.env(\..+)?|\.dev\.vars(\..+)?|.*\.(pem|key|p12|pfx|keystore|jks)"
    r"|id_rsa|id_ed25519|id_ecdsa|credentials\.json"
    r"|service-account.*\.json)$")
NOT_A_CREDENTIAL = re.compile(r"\.(example|sample|template)$")

# Each pattern is a vendor's documented token shape.
SHAPES = (
    ("a private key block", r"BEGIN (RSA |OPENSSH |EC |DSA |PGP )?PRIVATE KEY"),
    ("an aws access key id", r"AKIA[0-9A-Z]{16}"),
    ("a github token", r"gh[pousr]_[A-Za-z0-9]{30,}"),
    ("a slack token", r"xox[abprs]-[0-9A-Za-z-]{10,}"),
    ("a google api key", r"AIza[0-9A-Za-z_-]{35}"),
    ("a stripe key", r"sk_(live|test)_[0-9A-Za-z]{16,}"),
    ("a npm token", r"npm_[A-Za-z0-9]{36}"),
    ("a bearer literal", r"Bearer [A-Za-z0-9._-]{24,}"),
    ("a password in a url", r"://[A-Za-z0-9._%+-]+:[^@/\s]{6,}@"),
)

PUBLIC_SHAPES = (
    ("an email address", r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"),
    ("a long hex identifier", r"\b[0-9a-f]{24,}\b"),
    ("an absolute home path",
     r"([A-Za-z]:[\\/]Users[\\/]|/home/|/Users/)[A-Za-z0-9._-]+"),
)

# The one credential shape this project can actually leak, and does.
#
# A private tracker authenticates by a passkey carried in the announce URL:
# `?passkey=<hex>`, or a long token as a path component beside `announce` or
# `scrape`. Upstream lists publish them because a contributor pasted their own
# URL, and a project whose entire premise is doing better than concatenating
# those lists must not pass them on.
#
# It is a separate rule from the generic hex one because it needs a different
# verdict: this is somebody's live credential, and the tracker it belongs to
# can see every use of it.
TRACKER_CREDENTIAL = re.compile(
    r"[?&]pass(key|_key|kee)=[A-Za-z0-9]{16,}"
    r"|/[A-Za-z0-9]{20,}/(announce|scrape)\b"
    r"|/(announce|scrape)/[A-Za-z0-9]{20,}\b",
    re.IGNORECASE)

# An open defect this check MEASURES rather than fails on, per the check
# contract in `scripts/README.md`: record the count and judge it only past a
# stated ceiling.
#
# Six distinct credentials are in the corpus, on seven URLs (one tracker is
# listed both with and without its port), and `scripts/generate.py --offline`
# publishes all seven into `trackers_all.txt` (measured 2026-08-31, C-70).
# The fixtures themselves are verbatim captures of upstream files and are not
# edited: they are what makes the offline gate reproducible, and a fixture
# somebody rewrote is not a capture. The defect is that nothing between the
# fixture and the published dataset refuses them.
#
# T-107 is the entry that closes it. THIS CEILING COMES OFF WHEN IT DOES: an
# exemption nobody removes is a check that stopped checking. Until then a
# SEVENTH distinct credential fails the gate, which is the property that
# matters: the corpus is re-fetched from upstreams that keep publishing these.
TRACKER_CREDENTIAL_CEILING = 6

# Narrowings, not exemptions. Each names a shape that is SAFE PRACTICE and
# would otherwise make the rule fire on correct hardening, which is how a rule
# gets switched off. Whenever one of these produces a false positive, narrow it
# further here; do not drop the rule.
#
#  - a pinned GitHub Action is a 40-hex commit on a public repository, and
#    pinning is what this project's workflows are required to do;
#  - a declared pin is a commit and a SHA-256 written into a file whose job is
#    to record what was fetched, so 40 hex and 64 hex, both public by
#    construction. `scripts/vendor/toolkit/PIN.json` is the one here;
#  - a corpus commit in `references/PROVENANCE.md` is a public commit of a
#    public repository, recorded so a reader can re-fetch the same bytes;
#  - `/home/runner/` is the GitHub runner's own path, not anybody's machine.
#  - a git object id in a context that names it as one: this repository
#    records the commit of every captured reference, and a commit of a public
#    repository is what makes a citation re-checkable;
#  - a bare hex line inside a document is a protocol specimen, not a value. The
#    BEP 15 connect datagram is written out in full in one review.
NARROWINGS = (
    re.compile(r"uses:\s*[A-Za-z0-9._-]+/[A-Za-z0-9._-]+@[0-9a-f]{40}"),
    re.compile(r"[Pp]inned(Ref|Sha256|Commit|Digest)|PINNED_(REF|SHA256)"),
    re.compile(r'"[a-z_]*(sha256|sha|ref|commit)(_read)?"\s*:'),
    re.compile(r"\b(commit|COMMIT|sha|SHA|sha256|revision|rev-parse)\b"),
    re.compile(r"^[0-9a-f]+$", re.MULTILINE),
    re.compile(r"/home/(linuxbrew|runner|user|vagrant|ubuntu|node)/"),
    re.compile(r"/Users/(runner|user)/"),
)


# A narrowing looks at the line and at the two lines above it. The context that
# says a hash is a commit routinely sits on the line before the hash, because
# prose wraps: a path ending in COMMIT, then the hash on the next line. A
# one-line window reported four correct citations as findings.
NARROWING_LOOKBACK = 2


def narrowed(window: list[str]) -> bool:
    text = "\n".join(window)
    return any(n.search(text) for n in NARROWINGS)


def main(argv: list[str]) -> int:
    json_mode = "--json" in argv
    public = "--public" in argv

    files = _scope.repo_files(text_only=False)
    if not files:
        raise _scope.CouldNotRun("no files in scope")

    report: list[str] = []
    categories = 0

    creds = [f for f in files
             if CREDENTIAL_FILE.search(f) and not NOT_A_CREDENTIAL.search(f)]
    if creds:
        categories += 1
        report.append("== a credential file is tracked ==")
        report.extend(creds)

    text_files = [f for f in files if f.lower().endswith(_scope.TEXT_EXT)]

    # Counted as DISTINCT credentials, not as lines. The same URL appears in
    # three fixtures because two upstreams publish it and one is copied into
    # the test corpus; that is one person's passkey, not three, and a ceiling
    # that counted lines would move whenever a fixture was re-captured.
    creds_in_urls = {}
    for rel in text_files:
        for n, line in enumerate(_scope.read(rel).splitlines(), start=1):
            m = TRACKER_CREDENTIAL.search(line)
            if m:
                creds_in_urls.setdefault(m.group(0), []).append("%s:%d" % (rel, n))
    if len(creds_in_urls) > TRACKER_CREDENTIAL_CEILING:
        categories += 1
        report.append(
            "== a private-tracker credential in a URL: %d distinct, "
            "ceiling %d (T-107) ==" % (len(creds_in_urls),
                                       TRACKER_CREDENTIAL_CEILING))
        for token in sorted(creds_in_urls):
            report.append("%s  in %s" % (token, ", ".join(creds_in_urls[token])))

    shapes = list(SHAPES) + (list(PUBLIC_SHAPES) if public else [])
    for name, pattern in shapes:
        rx = re.compile(pattern)
        hits = []
        for rel in text_files:
            lines = _scope.read(rel).splitlines()
            for i, line in enumerate(lines):
                if TRACKER_CREDENTIAL.search(line):
                    continue      # already reported, and one finding one home
                window = lines[max(0, i - NARROWING_LOOKBACK):i + 1]
                if rx.search(line) and not narrowed(window):
                    hits.append("%s:%d:%s" % (rel, i + 1, line.strip()[:160]))
        if hits:
            categories += 1
            report.append("== %s ==" % name)
            report.extend(hits)

    ok = ("no secret shapes found in %d files (tracked plus "
          "untracked-not-ignored)%s\n"
          "  private-tracker credentials in URLs: %d distinct, "
          "ceiling %d (T-107)"
          % (len(files), " (public rules included)" if public else "",
             len(creds_in_urls), TRACKER_CREDENTIAL_CEILING))
    return _scope.emit(
        json_mode, "check-no-secrets/1", categories, report, ok,
        tail=("If any of it is a real credential, IN THIS ORDER:\n"
              "  1. ROTATE IT. Now, before anything else. It is compromised "
              "from the moment\n     it was written, and removing the file "
              "does not change that.\n"
              "  2. Tell the operator. They own the account.\n"
              "  3. Remove it from the tree, and add the ignore rule.\n"
              "  4. A history rewrite is the operator's call and the "
              "operator's action.\n     It is tidying after the fix, not the "
              "fix.\n\n"
              "If it is a false positive, narrow the pattern in this script "
              "rather than\nswitching the check off. docs/security/"
              "secrets.md."),
        files=len(files), public_rules=str(public).lower())


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except _scope.CouldNotRun as exc:
        print("check-no-secrets: %s" % exc, file=sys.stderr)
        sys.exit(2)
