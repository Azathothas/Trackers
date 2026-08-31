"""Which execution profile a run is under, and what that permits.

RULES 15 states the policy: this project runs in two places with very different
budgets, and **neither may be served by weakening the other.** This module is
the mechanism -- one code path, an explicit profile, never a reduced feature set.

THE TWO PROFILES

    ci      a GitHub-hosted runner. Shared egress, datacenter address space,
            no IPv6 egress (`C-04`), provider DNS. Request noise against other
            people's servers is the binding constraint.

    local   a contributor's machine or container. Usually real IPv6, a
            residential or transit resolver, working UDP, and no reason to
            ration requests beyond politeness (RULES 4).

`ci` IS THE DEFAULT AND IS NOT AUTO-ESCAPED

A run that says nothing is `ci`, on any host, including a laptop. The expensive
mistake available here is a full-corpus sweep fired by accident; the cheap one
is a contributor running the restricted profile and wondering why. So `local`
is **opted into**, never inferred:

    TRACKERS_PROFILE=local python3 scripts/generate.py

Detection reads exactly one variable. It does not sniff `CI`, `GITHUB_ACTIONS`
or a hostname, because a profile that changes when a CI vendor renames a
variable is a profile nobody can reason about -- and because inferring `local`
from "no CI variables found" would auto-escalate the budget on any machine that
happens not to set them, which is the failure this ordering exists to prevent.

WHAT A PROFILE IS NOT

It is **not** a correctness switch. Nothing here changes what a measurement
means, what counts as `dead`, or what the pipeline outputs from the same
inputs -- determinism (RULES 3.6) is unaffected, and the offline gates pass
identically under both. A profile bounds **how much** work touches third
parties and **which optional transports** are attempted; the result of any work
that does happen is identical.

A result taken under `local` carries its profile in vantage metadata
(RULES 3.4) and is never silently merged with a `ci` result as though the two
had equal reach. Disagreement between profiles is a first-class output (T-004).
"""

from __future__ import annotations

import os
from dataclasses import dataclass

__all__ = ["Profile", "Budget", "CI", "LOCAL", "ENV_VAR", "detect", "budget_for"]

#: The one variable consulted. Named for this project so it cannot collide.
ENV_VAR = "TRACKERS_PROFILE"

CI = "ci"
LOCAL = "local"
PROFILES = (CI, LOCAL)


class Profile(str):
    """A profile name that renders as itself in a record."""

    __slots__ = ()


@dataclass(frozen=True, slots=True)
class Budget:
    """What a profile permits. Every field is read by a caller, never guessed.

    The names are deliberately about *permission*, not about *policy*: this
    says what a run may do, and the caller decides what it does do.
    """

    #: The profile these limits belong to. Goes into vantage metadata.
    profile: str

    #: Concurrent probes across all hosts. Per-host concurrency is always 1,
    #: in both profiles, and is not configurable (RULES 15.2).
    max_concurrency: int

    #: May a run probe every tracker in the corpus, or only a sample? A
    #: regression check needs enough trackers to detect a broken probe -- which
    #: is what the fake-tracker oracle is for -- not all of them.
    full_corpus_sweep: bool

    #: How many trackers a sampled run probes. `None` means the whole corpus.
    sample_size: int | None

    #: May the run attempt IPv6? The capability exists in both profiles; on a
    #: runner it is skipped for a measured reason (`C-04`), not absent.
    attempt_ipv6: bool

    #: May the run attempt transports needing a router this host may not have
    #: (i2p, yggdrasil, onion)? False does not mean `dead` -- it means
    #: `unmeasurable`, which is a statement about our vantage (RULES 3.1).
    attempt_router_networks: bool

    #: Must every upstream fetch send `If-None-Match` / `If-Modified-Since`?
    #: Mandatory in `ci`, where a 304 is the cheapest correct answer available
    #: and costs the upstream almost nothing (T-104).
    conditional_requests_required: bool

    #: Fetch each upstream at most once per run and share the snapshot. Two
    #: consumers of ngosang's list get one fetch.
    share_source_snapshots: bool

    def as_record(self) -> dict[str, object]:
        """The shape that goes into vantage metadata."""
        return {
            "profile": self.profile,
            "max_concurrency": self.max_concurrency,
            "full_corpus_sweep": self.full_corpus_sweep,
            "sample_size": self.sample_size,
            "attempt_ipv6": self.attempt_ipv6,
            "attempt_router_networks": self.attempt_router_networks,
            "conditional_requests_required": self.conditional_requests_required,
        }


#: `ci`: the tight one, and the default.
#:
#: `max_concurrency` is 8 rather than a larger number because the binding
#: constraint is other people's servers, not the runner: per-host concurrency
#: is 1 regardless, so this only bounds how many *distinct* hosts are in flight.
#: `sample_size` of 200 is a starting value and is **not** measured -- it is
#: enough trackers that a wholly broken probe cannot pass unnoticed, and it is
#: T-029's job to replace it with a number derived from the job timeout.
_CI = Budget(
    profile=CI,
    max_concurrency=8,
    full_corpus_sweep=False,
    sample_size=200,
    attempt_ipv6=False,             # C-04: measured false on both runner images
    attempt_router_networks=False,  # C-37: no i2p/yggdrasil/tor router present
    conditional_requests_required=True,
    share_source_snapshots=True,
)

#: `local`: bounded by politeness alone. Still one connection per host.
_LOCAL = Budget(
    profile=LOCAL,
    max_concurrency=16,
    full_corpus_sweep=True,
    sample_size=None,
    attempt_ipv6=True,
    attempt_router_networks=True,
    conditional_requests_required=False,
    share_source_snapshots=True,
)

_BUDGETS = {CI: _CI, LOCAL: _LOCAL}


class UnknownProfile(ValueError):
    """`TRACKERS_PROFILE` was set to something that is not a profile."""


def detect(environ: dict[str, str] | None = None) -> str:
    """The active profile.

    Reads `TRACKERS_PROFILE`; absent or empty means `ci`. An unrecognised
    value **raises** rather than falling back, because silently running the
    restrictive profile when somebody asked for `locl` is the kind of quiet
    wrongness this project exists to avoid.
    """
    env = os.environ if environ is None else environ
    raw = (env.get(ENV_VAR) or "").strip().lower()
    if not raw:
        return CI
    if raw not in PROFILES:
        raise UnknownProfile(
            f"{ENV_VAR}={raw!r} is not a profile; expected one of "
            f"{', '.join(PROFILES)}. See TODO/RULES.md section 15."
        )
    return raw


def budget_for(profile: str | None = None,
               environ: dict[str, str] | None = None) -> Budget:
    """The `Budget` for a profile, defaulting to the detected one."""
    name = detect(environ) if profile is None else profile.strip().lower()
    if name not in _BUDGETS:
        raise UnknownProfile(
            f"{name!r} is not a profile; expected one of {', '.join(PROFILES)}")
    return _BUDGETS[name]
