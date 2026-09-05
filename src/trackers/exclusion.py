"""Which upstream exclusions this project adopts, and which it declines to.

An upstream blacklist mixes two completely different kinds of statement, and
adopting or rejecting it wholesale gets one of them wrong:

  * **"The operator does not want you here."** This project MUST honour that
    (RULES 4), regardless of whether the tracker works. It is not a
    measurement and it is not ours to second-guess.

  * **"We measured this tracker and disliked what we found."** That is an
    *opinion derived from somebody else's measurement*, taken from a vantage we
    cannot inspect, by a generator that is not published (`C-22`). HISTORY/reference-sweep.md is explicit that consuming an upstream's output "inherits its
    filtering decisions -- including the domain/IP deduplication and blacklist,
    which are *opinions*, not facts."

So the reasons are classified rather than counted. Measured distribution of
`ngosang/trackerslist` `blacklist.txt` @ `1e61597`, 346 entries:

    178  registered torrents              -> OPINION   (editorial policy)
    135  duplicate of <url>               -> OPINION   (a resolved-address inference)
     11  malfunction                      -> OPINION   (measurable by us)
      7  deprecated by owner              -> HONOUR
      5  detected by antivirus software   -> SAFETY
      2  fake seeds                       -> OPINION   (and contested: see below)
      2  requested by sysadmin            -> HONOUR
      2  malfunction issue #374           -> OPINION
      1  error                            -> OPINION
      1  blocked by IDNA ban              -> OPINION
      1  redirects to <url>               -> OPINION
      1  detected as suspicious           -> SAFETY

"Fake seeds" is classified OPINION deliberately, and the evidence says the
classification is doing real work. `http://bt.okmp3.ru:2710/announce` is
blacklisted here for exactly that reason, is listed **live** by newTrackon, and
answered this project's own runner probe as a well-formed tracker
(`experiments/05`, run 33246108348). Three observers, three answers. Silently
adopting one of them would delete the disagreement, which RULES 3.4 calls
the most informative thing this dataset could publish.

The counter-example is equally instructive: newTrackon issue #353 independently
reports `torrent.tracker.durukanbal.com` for implausible peer counts, and
ngosang blacklists that same tracker as "fake seeds". Two independent observers
agreeing is a much stronger signal than one -- but it is a signal to *record and
publish*, not a reason to make an entry disappear without trace.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from enum import Enum


class ExclusionClass(str, Enum):
    #: The operator asked not to be included. Excluded, always. RULES 4.
    HONOUR = "honour"
    #: Credible harm to a consumer. Excluded.
    SAFETY = "safety"
    #: Somebody else's measurement or editorial policy. Kept, and flagged.
    OPINION = "opinion"
    #: The URL carries somebody's private-tracker credential. Excluded, and
    #: this one is **ours** rather than an upstream's (T-107, `C-70`).
    PRIVATE = "private"


# --- T-107: a private tracker's credential must not be republished ------------
#
# A private tracker authenticates by a passkey carried in the announce URL:
# `?passkey=<hex>`, or a long token as a path component beside `announce` or
# `scrape`. Upstream lists publish them because a contributor pasted their own
# URL, and a project whose premise is doing better than concatenating those
# lists must not pass them on.
#
# ⭐ **This is the one definition of that shape in the tree.**
# `scripts/check-no-secrets.py` imports it rather than carrying a second copy:
# two patterns for one rule are two places for it to be wrong, and they will be
# wrong differently (`docs/conventions/forbidden-patterns.md`).
PRIVATE_CREDENTIAL = re.compile(
    r"[?&]pass(key|_key|kee)=[A-Za-z0-9]{16,}"
    r"|/[A-Za-z0-9]{20,}/(announce|scrape)\b"
    r"|/(announce|scrape)/[A-Za-z0-9]{20,}\b",
    re.IGNORECASE)

#: What replaces the token when a refusal is written down. The audit says which
#: host was refused and why; it does not repeat the secret that got it refused.
REDACTED = "<redacted>"


def carries_private_credential(url: str) -> bool:
    """Whether this URL authenticates somebody. RULES 6: no private-tracker data."""
    return PRIVATE_CREDENTIAL.search(url) is not None


def mask_credential(url: str) -> str:
    """The URL with the credential removed, **for the audit record only**.

    ⛔ **Never published as a tracker.** T-107 forbids redact-and-republish
    because a URL with its token stripped is an endpoint that answers
    differently, and offering it as the tracker is the invented-endpoint
    mistake `C-66` already cost this project once.

    This is the other half of that rule rather than an exception to it. RULES
    3.10 says a tracker that disappears owes the consumer who noticed a reason,
    and the reason has to name *something*. Naming the raw URL would write a
    live credential into the run report -- refusing to publish it in one file
    and printing it in the next one -- so the host is named and the token is
    not.
    """
    def strip(m: re.Match[str]) -> str:
        text = m.group(0)
        if "=" in text:
            return text.split("=", 1)[0] + "=" + REDACTED
        # A path-component token, before or after `announce`/`scrape`.
        return "/".join(
            part if part.lower() in ("announce", "scrape") or not part else REDACTED
            for part in text.split("/"))
    return PRIVATE_CREDENTIAL.sub(strip, url)


#: Matched case-insensitively against the reason text, in order. First match
#: wins, so more specific patterns come first.
_PATTERNS: tuple[tuple[str, ExclusionClass], ...] = (
    (r"requested by (sysadmin|admin|owner|operator)", ExclusionClass.HONOUR),
    (r"deprecated by owner",                          ExclusionClass.HONOUR),
    (r"owner request",                                ExclusionClass.HONOUR),
    (r"opt(ed)?[- ]out",                              ExclusionClass.HONOUR),

    (r"antivirus",                                    ExclusionClass.SAFETY),
    (r"malware|malicious|phishing",                   ExclusionClass.SAFETY),
    (r"detected as suspicious",                       ExclusionClass.SAFETY),
)

#: Reasons that are explicitly measurement claims. Listed so that the OPINION
#: default is a considered position rather than a fallthrough.
_KNOWN_OPINIONS = (
    "registered torrents", "duplicate of", "malfunction", "fake seeds",
    "error", "blocked by idna ban", "redirects to",
)


@dataclass(frozen=True, slots=True)
class Exclusion:
    url: str
    reason: str
    klass: ExclusionClass
    source_id: str

    @property
    def excluded(self) -> bool:
        return self.klass in (ExclusionClass.HONOUR, ExclusionClass.SAFETY)


def classify_reason(reason: str) -> ExclusionClass:
    """Classify one upstream exclusion reason.

    Defaults to OPINION. That default is the safe direction: treating an
    operator request as an opinion would mean continuing to probe somebody who
    asked us to stop, so HONOUR is matched explicitly and generously, while
    everything unrecognised merely stays in the dataset with a flag.
    """
    text = (reason or "").strip().lower()
    for pattern, klass in _PATTERNS:
        if re.search(pattern, text):
            return klass
    return ExclusionClass.OPINION


def parse_blacklist(body: str, source_id: str) -> list[Exclusion]:
    """Parse `url # reason` lines, KEEPING the reason.

    The ordinary normalizer strips a trailing comment, because for a tracker
    list the comment is noise. Here the comment is the entire point: without
    the reason there is no way to tell an operator's request from somebody
    else's opinion, and the two have opposite consequences.
    """
    from .normalize import parse
    from .model import InvalidTracker

    out: list[Exclusion] = []
    for raw in body.splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "://" not in line:
            continue
        m = re.search(r"\s#\s*(.*)$", line)
        reason = m.group(1).strip() if m else ""
        try:
            t = parse(line)
        except InvalidTracker:
            continue
        out.append(Exclusion(url=t.url, reason=reason,
                             klass=classify_reason(reason),
                             source_id=source_id))
    return out


def summarise(exclusions: list[Exclusion]) -> dict[str, int]:
    from collections import Counter
    c = Counter(e.klass.value for e in exclusions)
    return {k: c.get(k, 0) for k in ("honour", "safety", "opinion")}
