"""Where a measurement was taken from, recorded so it can never be read as universal.

RULES 3.4 and decision D2 require every health record to carry the vantage it
was taken from. The reason is the project's central failure mode: a consumer
reads `dead` and the data means **`dead from AS8075 Microsoft datacenter
space, over IPv4 only, on 2026-08-29`**. Those are different statements and
only one of them is true.

`scripts/check-vantage-metadata.py` is the gate. It deliberately exits 2 rather
than 0 when no health record exists, so this module existing is what turns that
2 into a 0.

WHAT THIS MODULE REFUSES TO DO

It does not guess. Two facts here are commonly conflated and are kept apart:

    ipv6_stack_present   can this host create an AF_INET6 socket?
    ipv6_route_present   is there a route for a global IPv6 destination?

Neither is `ipv6_egress`, which is whether packets actually get there and come
back. Only a real round trip answers that, and `experiments/01` is what takes
it (`C-04`: stack present, egress **false**, on both runner images). So
`ipv6_egress` is `None` here unless a caller passes in a measured value, and
`None` renders as a dash. An unknown marked unknown costs nothing; an unknown
dressed as a measurement contaminates everything downstream.

The route check itself sends **no packets**. `connect()` on a UDP socket
performs a routing-table lookup and sets the peer address; it does not transmit.
So determining reachability this way costs nobody any traffic.
"""

from __future__ import annotations

import hashlib
import os
import socket
from dataclasses import dataclass, field
from typing import Any

from .profile import Budget, budget_for

__all__ = ["PROBE_VERSION", "Vantage", "detect", "probe_code_sha256", "UNKNOWN"]

UNKNOWN = "-"  # RULES 1.5: where a value is unknown, write a dash.

#: Bumped by hand when probe behaviour changes in a way that makes old and new
#: measurements non-comparable. It is a human statement of intent, and because a
#: human can forget to bump it, `probe_code_sha256` is recorded alongside and
#: cannot be forgotten.
PROBE_VERSION = "0.1.0"

#: Files whose contents define what a measurement means. A change to any of
#: them changes the hash, so "which code took this measurement" is answerable
#: exactly rather than approximately.
_CODE_FILES = ("bencode.py", "bep15.py", "model.py", "probe.py", "vantage.py")

#: A global address used only as a routing-table lookup key. Never contacted.
_V6_ROUTE_PROBE = "2001:4860:4860::8888"
_V4_ROUTE_PROBE = "8.8.8.8"


def probe_code_sha256() -> str:
    """sha256 over the source of the modules that define a measurement.

    Deterministic across runs on identical code, and independent of the order
    the files happen to sit on disk.
    """
    here = os.path.dirname(os.path.abspath(__file__))
    h = hashlib.sha256()
    for name in _CODE_FILES:
        path = os.path.join(here, name)
        h.update(name.encode())
        try:
            with open(path, "rb") as fh:
                h.update(fh.read())
        except OSError:
            # A missing file is itself a fact about the code that ran; record
            # it in the hash rather than pretending the file was empty.
            h.update(b"<absent>")
    return h.hexdigest()


def environment_class() -> str:
    """Name the class of machine this is. The single most important condition.

    A measurement from a GitHub-hosted runner does not generalise to a
    residential connection, and vice versa. Self-hosted is distinguished from
    hosted because a self-hosted runner has an entirely different network
    position and the label must not lie about it.
    """
    if os.environ.get("GITHUB_ACTIONS") == "true":
        if "self-hosted" in os.environ.get("RUNNER_LABELS", ""):
            return "github-actions-self-hosted"
        return "github-actions-hosted"
    if os.environ.get("CCR_AGENT_PROXY_ENABLED") or os.environ.get("HTTPS_PROXY"):
        return "authoring-sandbox-proxied"
    return "unclassified-host"


def _stack_present(family: int) -> bool:
    try:
        socket.socket(family, socket.SOCK_DGRAM).close()
        return True
    except OSError:
        return False


def _route_present(family: int, dest: str) -> bool:
    """Whether a route exists to `dest`. Sends nothing.

    A UDP `connect()` is a routing-table lookup plus a peer-address
    assignment. No datagram leaves the host, so this is free for everyone
    else and costs one syscall for us.
    """
    try:
        s = socket.socket(family, socket.SOCK_DGRAM)
    except OSError:
        return False
    try:
        s.connect((dest, 53))
        return True
    except OSError:
        return False
    finally:
        s.close()


@dataclass(frozen=True, slots=True)
class Vantage:
    """The measurement position, as a value that travels with every record."""

    environment_class: str
    probe_version: str
    probe_code_sha256: str
    #: Families for which a route exists, so the probe is willing to try them.
    #: **Not** a claim that egress works -- see the module docstring.
    ip_families: tuple[str, ...]
    ip_families_method: str
    ipv6_stack_present: bool
    ipv6_route_present: bool
    #: Only ever set from a real measured round trip (`experiments/01`).
    #: `None` renders as a dash.
    ipv6_egress: bool | None = None
    region: str = UNKNOWN
    repo_commit: str = UNKNOWN
    runner: dict[str, str] = field(default_factory=dict)
    #: The execution profile this measurement was taken under (RULES 15).
    #: A `local` result and a `ci` result do not have equal reach and must
    #: never be merged as though they did (T-004).
    execution_profile: str = "ci"

    def as_dict(self) -> dict[str, Any]:
        """The form written into a health record. Keys are stable public contract."""
        return {
            "environment_class": self.environment_class,
            "probe_version": self.probe_version,
            "probe_code_sha256": self.probe_code_sha256,
            "ip_families": list(self.ip_families),
            "ip_families_method": self.ip_families_method,
            "ipv6_stack_present": self.ipv6_stack_present,
            "ipv6_route_present": self.ipv6_route_present,
            "ipv6_egress": self.ipv6_egress if self.ipv6_egress is not None else UNKNOWN,
            "region": self.region,
            "repo_commit": self.repo_commit,
            "runner": dict(self.runner),
            "execution_profile": self.execution_profile,
        }

    @property
    def can_attempt_ipv6(self) -> bool:
        """Whether the probe should even try an IPv6 destination.

        False here is why an IPv6-only tracker is `unmeasurable` and never
        `dead`: we did not fail to reach it, we never asked.
        """
        return "ipv6" in self.ip_families


def detect(*, ipv6_egress: bool | None = None, repo_commit: str = UNKNOWN,
           budget: Budget | None = None) -> Vantage:
    """Collect the vantage. No network traffic, no third party, no clock.

    `ipv6_egress` is an input rather than something measured here, because
    measuring it honestly means sending packets to somebody else's host and
    that belongs in an experiment, not in every probe run.

    `budget` is the execution profile's permissions (RULES 15). It is an
    argument so a test can pin it; left `None` it is detected from the
    environment, and the detected default is `ci`.
    """
    budget = budget_for() if budget is None else budget
    v6_stack = _stack_present(socket.AF_INET6)
    v6_route = _route_present(socket.AF_INET6, _V6_ROUTE_PROBE) if v6_stack else False
    v4_route = _route_present(socket.AF_INET, _V4_ROUTE_PROBE)

    families: list[str] = []
    if v4_route:
        families.append("ipv4")
    # A route is necessary but not sufficient. Where egress has been MEASURED
    # false, believe the measurement over the routing table: the whole point of
    # C-04 is that a runner has an IPv6 stack and a route and still cannot get
    # a packet out. Attempting anyway would record healthy trackers as dead.
    # Three separate facts, and all three must hold before the probe will
    # send an IPv6 packet: a route exists, egress has not been MEASURED
    # false, and the profile permits it. The middle one is why C-04 exists --
    # a runner has a stack and a route and still cannot get a packet out --
    # and the last is RULES 15.4: the capability is present in both profiles
    # and skipped in `ci` for a measured reason, never absent from the code.
    if v6_route and ipv6_egress is not False and budget.attempt_ipv6:
        families.append("ipv6")

    runner: dict[str, str] = {}
    if os.environ.get("GITHUB_ACTIONS") == "true":
        for key, env in (
            ("run_id", "GITHUB_RUN_ID"),
            ("run_attempt", "GITHUB_RUN_ATTEMPT"),
            ("os", "RUNNER_OS"),
            ("arch", "RUNNER_ARCH"),
            ("image", "ImageOS"),
            ("image_version", "ImageVersion"),
        ):
            runner[key] = os.environ.get(env, UNKNOWN)

    return Vantage(
        environment_class=environment_class(),
        probe_version=PROBE_VERSION,
        probe_code_sha256=probe_code_sha256(),
        ip_families=tuple(families),
        ip_families_method=(
            "UDP connect() routing-table lookup, zero packets sent"
            + ("; ipv6 withheld by measured egress=false"
               if v6_route and ipv6_egress is False else "")
            + (f"; ipv6 withheld by the {budget.profile} profile"
               if v6_route and ipv6_egress is not False
               and not budget.attempt_ipv6 else "")
        ),
        ipv6_stack_present=v6_stack,
        ipv6_route_present=v6_route,
        ipv6_egress=ipv6_egress,
        # GitHub does not expose the runner's region. Recorded as unknown
        # rather than inferred from an IP geolocation service, which would be a
        # third party's guess presented as our measurement.
        region=UNKNOWN,
        repo_commit=repo_commit,
        runner=runner,
        execution_profile=budget.profile,
    )
