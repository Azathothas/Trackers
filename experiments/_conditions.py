"""Shared conditions block for every experiment in this directory.

Per `docs/methodology/experiments.md` (Azathothas/TEMPLATE), every experiment
prints its conditions on the way out: host, tool versions, date, sample count.
This module exists so those conditions are collected identically everywhere and
cannot be forgotten in one script and remembered in another.

It is NOT a framework. It collects facts and formats them. Nothing here decides
whether an experiment passed.

Exit-code vocabulary used by every experiment (TEMPLATE experiments.md):
    0  the measurement ran
    1  the measurement ran and the thing being measured failed an expectation
    2  the measurement could not run
"""

from __future__ import annotations

import json
import os
import platform
import socket
import subprocess
import sys
import time
from datetime import datetime, timezone

# Exit codes, named so a caller never writes a bare integer.
EXIT_MEASURED = 0
EXIT_MEASURED_AND_FAILED = 1
EXIT_COULD_NOT_RUN = 2

UNKNOWN = "-"  # RULES 1.5: where a value is unknown, write a dash.


def _sh(cmd: list[str], timeout: int = 10) -> str:
    """Run a command, return stripped stdout, or UNKNOWN. Never raises."""
    try:
        out = subprocess.run(
            cmd, capture_output=True, text=True, timeout=timeout, check=False
        )
        return out.stdout.strip() or UNKNOWN
    except Exception:
        return UNKNOWN


def environment_class() -> str:
    """Name the class of machine this is running on.

    This is the single most important condition in this project: a measurement
    taken from a GitHub-hosted runner does not generalise to a residential
    connection, and vice versa. RULES 3.4 requires it in every health
    record, so the experiments record it too.
    """
    if os.environ.get("GITHUB_ACTIONS") == "true":
        # Distinguish hosted from self-hosted: a self-hosted runner has an
        # entirely different network position and the label must not lie.
        labels = os.environ.get("RUNNER_LABELS", "")
        if "self-hosted" in labels:
            return "github-actions-self-hosted"
        return "github-actions-hosted"
    if os.environ.get("CCR_AGENT_PROXY_ENABLED") or os.environ.get("HTTPS_PROXY"):
        return "authoring-sandbox-proxied"
    return "unclassified-host"


def git_commit(start: str | None = None) -> str:
    """The commit this instrument ran at, so a result can be traced to code."""
    here = os.path.dirname(os.path.abspath(start or __file__))
    out = _sh(["git", "-C", here, "rev-parse", "HEAD"])
    dirty = _sh(["git", "-C", here, "status", "--porcelain"])
    if out != UNKNOWN and dirty not in ("", UNKNOWN):
        return out + "-dirty"
    return out


def public_ip() -> tuple[str, str]:
    """Best-effort public IPv4 and its network operator.

    Returns (ip, org). Either may be UNKNOWN. This is best-effort by
    construction: it depends on a third party, so it is never load-bearing and
    a failure here must not fail an experiment.
    """
    ip = UNKNOWN
    for url in (
        "https://checkip.amazonaws.com",
        "https://api.ipify.org",
    ):
        got = _sh(["curl", "-sS", "--max-time", "10", url], timeout=15)
        if got != UNKNOWN and len(got) <= 45 and " " not in got:
            ip = got
            break
    org = UNKNOWN
    if ip != UNKNOWN:
        raw = _sh(["curl", "-sS", "--max-time", "10", f"https://ipinfo.io/{ip}/json"], timeout=15)
        if raw != UNKNOWN:
            try:
                org = json.loads(raw).get("org") or UNKNOWN
            except Exception:
                org = UNKNOWN
    return ip, org


def has_ipv6_stack() -> bool:
    """Whether this host can even create an AF_INET6 socket.

    Distinct from 'has IPv6 egress': a host may have the stack and no route.
    The two are separately reported because conflating them is how an
    IPv6-only tracker gets labelled dead (RULES 3.4, C-04).
    """
    try:
        s = socket.socket(socket.AF_INET6, socket.SOCK_STREAM)
        s.close()
        return True
    except OSError:
        return False


def collect(sample_counts: dict[str, int] | None = None, extra: dict | None = None) -> dict:
    """Gather the conditions block. Cheap fields only; network fields are opt-in."""
    cond = {
        "utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "environment_class": environment_class(),
        "hostname": socket.gethostname(),
        "platform": platform.platform(),
        "python": sys.version.split()[0],
        "repo_commit": git_commit(),
        "ipv6_stack_present": has_ipv6_stack(),
        "sample_counts": sample_counts or {},
    }
    # GitHub Actions self-describes; record it verbatim so a run is findable.
    if os.environ.get("GITHUB_ACTIONS") == "true":
        cond["github"] = {
            "run_id": os.environ.get("GITHUB_RUN_ID", UNKNOWN),
            "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT", UNKNOWN),
            "runner_os": os.environ.get("RUNNER_OS", UNKNOWN),
            "runner_arch": os.environ.get("RUNNER_ARCH", UNKNOWN),
            "runner_image": os.environ.get("ImageOS", UNKNOWN),
            "runner_image_version": os.environ.get("ImageVersion", UNKNOWN),
            "repository": os.environ.get("GITHUB_REPOSITORY", UNKNOWN),
            "ref": os.environ.get("GITHUB_REF", UNKNOWN),
            "sha": os.environ.get("GITHUB_SHA", UNKNOWN),
        }
    if extra:
        cond.update(extra)
    return cond


def with_network_vantage(cond: dict) -> dict:
    """Add the vantage fields that cost a network round trip."""
    ip, org = public_ip()
    cond["public_ipv4"] = ip
    cond["public_ipv4_org"] = org
    return cond


def emit(question: str, conditions: dict, results, out_path: str | None = None) -> None:
    """Print the human-readable block and, if asked, write the machine-readable one.

    TEMPLATE experiments.md: 'It does not clean up its own output. The evidence
    is the point.' So this writes and never deletes.
    """
    print("=" * 78)
    print("QUESTION: " + question)
    print("=" * 78)
    print("CONDITIONS")
    for k, v in conditions.items():
        if isinstance(v, dict):
            print(f"  {k}:")
            for k2, v2 in v.items():
                print(f"    {k2}: {v2}")
        else:
            print(f"  {k}: {v}")
    print("=" * 78)
    if out_path:
        os.makedirs(os.path.dirname(out_path), exist_ok=True)
        # An explicit newline. Without one, Python translates to the platform
        # separator on write, so the same instrument on Windows and on a runner
        # produces results that differ on every line. A committed result is
        # evidence, and evidence whose bytes depend on who ran it cannot be
        # diffed against the next run. RULES 15.5.
        with open(out_path, "w", encoding="utf-8", newline="\n") as fh:
            json.dump(
                {"question": question, "conditions": conditions, "results": results},
                fh,
                indent=2,
                sort_keys=True,
            )
            fh.write("\n")
        print(f"machine-readable result: {out_path}")


def results_path(script_file: str, tag: str | None = None) -> str:
    """A results path derived from the script's own location, never from cwd.

    TEMPLATE experiments.md: 'no dependence on the directory it runs from'.
    """
    here = os.path.dirname(os.path.abspath(script_file))
    name = os.path.basename(script_file).rsplit(".", 1)[0]
    env = environment_class()
    stamp = tag or datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return os.path.join(here, "results", f"{name}.{env}.{stamp}.json")


class Timer:
    """Wall-clock milliseconds. Monotonic, so a clock step cannot make it negative."""

    def __enter__(self):
        self.t0 = time.monotonic()
        return self

    def __exit__(self, *a):
        self.ms = (time.monotonic() - self.t0) * 1000.0
        return False
