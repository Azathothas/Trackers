"""The operator's refusal, for the instruments that contact trackers directly.

⛔ **Found by a door sweep on 2026-09-05, after the gate had already been
built.** `src/trackers/probe.py` consults BEP 34 before it opens a socket, and
that was treated as "the project honours exclusions". It was not: experiments
`02` and `05` reach the same action -- contacting somebody's tracker -- down a
different path, and `p0-ground-truth.yml` ran both of them **twice on the day
the gate landed**, against 11 UDP and 6 HTTP endpoints, consulting nothing.

RULES 4 is absolute and says nothing about which module does the contacting. A
control enforced on one path into an operation while a sibling reaches it
ungated is the most recurring hole there is
(`docs/conventions/forbidden-patterns.md`), and this is that hole with the
project's own headline feature on the wrong side of it.

⭐ **One rule, one enforcer.** This imports `src/trackers/bep34.py` rather than
reimplementing the lookup. A second copy of a consent check is two places for
it to be wrong, and the copy nobody reads is the one that keeps probing.

WHAT A CALLER OWES

Call `permits()` before each subject and skip the ones it refuses, recording
the refusal in the results. A skipped subject is **not** a failed measurement
and must never be counted as one: it is a subject this project is not
permitted to measure, which is a different fact and belongs in a different
field.
"""

from __future__ import annotations

import os
import sys
from urllib.parse import urlsplit

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(
    os.path.abspath(__file__))), "src"))

from trackers.bep34 import (Decision, Resolver,  # noqa: E402
                            protocol_for_transport)

__all__ = ["permits", "shared_resolver"]

_RESOLVER: Resolver | None = None


def shared_resolver() -> Resolver:
    """One resolver per process, so a host is asked about once.

    An experiment's subject list repeats hosts, and asking DNS once per subject
    would multiply this project's lookup load for nothing (RULES 15.2).
    """
    global _RESOLVER
    if _RESOLVER is None:
        _RESOLVER = Resolver()
    return _RESOLVER


def permits(url: str, *, default_port: int | None = None) -> tuple[bool, str]:
    """May this experiment contact `url`? Returns `(permitted, why)`.

    ⛔ **A lookup that does not answer returns `False`.** A DNS failure is not
    consent, and an instrument that probed on "we could not tell" would be
    doing the thing the record it failed to read exists to prevent.
    """
    parts = urlsplit(url)
    host = parts.hostname or ""
    if not host:
        return False, f"no host in {url!r}"
    scheme = (parts.scheme or "").lower()
    port = parts.port or default_port
    if port is None:
        port = 443 if scheme in ("https", "wss") else 80
    verdict = shared_resolver().consult(
        host, protocol_for_transport(scheme), port)
    return verdict.decision is Decision.ALLOW, f"{verdict.decision.value}: {verdict.detail}"
