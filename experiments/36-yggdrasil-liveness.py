#!/usr/bin/env python3
"""
QUESTION
    Is the corpus's yggdrasil tracker alive, measured from a node that is
    actually on the yggdrasil network?

WHY IT EXISTS
    T-039, the half `experiments/35` does not cover. Every record this project
    has taken calls yggdrasil `unmeasurable`, which is honest about our data
    and says nothing about the tracker (RULES 3.1). `C-37` is the structural
    reason: yggdrasil needs a node on the overlay and no vantage here ran one.

⭐ THE SUBJECT IS FOUND BY RESOLVING, NOT BY READING A URL
    There is **no yggdrasil URL in the corpus.** `scripts/probe-corpus.py`
    classifies by *transport and network*, and the network is decided after
    resolution: `yggtracker.i2p.rocks` is an ordinary DNS name whose only
    address is in `0200::/7`. So this instrument resolves the corpus itself
    rather than filtering on a scheme -- the same mistake in reverse would be
    RULES 3.1's `.i2p` trap, where a hostname suffix was read as a URL scheme.

⛔ ROUTE (e) IS MEASURED CLOSED, WHICH IS WHY THE EXPENSIVE ROUTE IS JUSTIFIED
    T-031's cheapest route is the dual-stack shortcut: many "unreachable"
    hosts are only unreachable in one list. Measured 2026-09-09 against three
    public resolvers (`1.1.1.1`, `8.8.8.8`, `9.9.9.9`) as well as the host's
    own: `yggtracker.i2p.rocks` has **no A record at all** and exactly one
    AAAA, inside `0200::/7`. The parent domain `i2p.rocks` is ordinary
    clearnet IPv4, so this is not a resolver failing -- the host is
    yggdrasil-only. `--check-dual-stack` re-runs that check and it is cheap.

CONTROL
    tier 0  the node joins: an interface, an address in `0200::/7`, and a
            non-empty routing table. Reported, and a run without it exits 2
            rather than reporting a subject that was never reachable.
    tier 1  ⭐ **a PEER's own overlay address**, taken from this run's own
            log, pinged over the overlay. Reaching it proves we are routing.
            ⛔ An absence is not a zero (RULES 2): without this, "the tracker
            did not answer" and "we never joined the network" are the same
            observation. The peer is chosen from the session rather than from
            a hardcoded list of services, which would rot.
    tier 2  the subject.

⛔ WHAT A NEGATIVE RESULT HERE IS AND IS NOT
    A subject that does not answer while the control does is **one
    observation** that this vantage could not reach it. It is not `dead`:
    `MIN_SAMPLES_FOR_DEATH` is 3 and this is one, and RULES 3.1 governs the
    rest. It never announces and never scrapes: the subject has no derivable
    scrape endpoint, so this reaches `CONNECTED` at most.

EXIT CODES
    0  the measurement ran
    1  the measurement ran and an --expect assertion failed
    2  the measurement could not run
"""

from __future__ import annotations

import argparse
import glob
import ipaddress
import json
import os
import re
import socket
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, "src"))
sys.path.insert(0, os.path.join(REPO, "scripts"))

import _conditions as C  # noqa: E402
from generate import load_corpus  # noqa: E402
from trackers.bep34 import Resolver  # noqa: E402
from trackers.model import Rung  # noqa: E402

FIXTURES = os.path.join(REPO, "tests", "fixtures", "sources")

#: Yggdrasil's address space. A host resolving only into this is on the
#: overlay and nowhere else.
YGGDRASIL_NET = ipaddress.ip_network("200::/7")

#: ⛔ Pinned by digest, never by tag. `docs/containers.md`: a tag moves, and a
#: measurement whose environment moved is not reproducible. Alpine is used
#: because it packages yggdrasil; the version it packages is recorded by the
#: run itself rather than asserted here.
DEFAULT_IMAGE = "alpine:3"

#: Public peers. ⚠ These are other people's machines and this instrument
#: connects to them; they are the published public-peer list rather than
#: anything discovered. A peer that has gone away costs a connection attempt
#: and nothing else, which is why there are several.
PEERS = (
    "tls://ygg.mkg20001.io:443", "tcp://ygg.mkg20001.io:80",
    "tcp://vpn.itrus.su:7991", "tls://vpn.itrus.su:7992",
    "tcp://146.103.111.53:65535", "tcp://51.15.204.214:12345",
    "tls://51.15.204.214:54321", "tcp://s2.i2pd.xyz:39565",
)

#: The payload run inside the container. ⛔ It is a constant here rather than a
#: tracked `.sh` because RULES 15.5 forbids a gate depending on a shell script;
#: this is not a gate, and keeping it beside the driver is what stops the two
#: drifting apart.
GUEST = r"""
set -u
apk add --no-cache yggdrasil curl iproute2 >/dev/null 2>&1 || true
yggdrasil -genconf > /tmp/ygg.conf 2>/dev/null
sed -i "s|Peers: \[\]|Peers: [__PEERS__]|" /tmp/ygg.conf
yggdrasil -useconf < /tmp/ygg.conf > /tmp/ygg.log 2>&1 &
i=0
while [ $i -lt 60 ]; do
  ADDR=$(ip -6 addr show tun0 2>/dev/null | grep -oE '2[0-9a-f]{2}:[0-9a-f:]+' | head -1)
  ROUTES=$(yggdrasilctl getself 2>/dev/null | grep -oE 'Routing table size:[^0-9]*[0-9]+' | grep -oE '[0-9]+$')
  if [ -n "${ADDR:-}" ] && [ "${ROUTES:-0}" -gt 0 ]; then break; fi
  i=$((i + 1)); sleep 1
done
echo "YGG_VERSION=$(yggdrasil -version 2>&1 | head -1)"
echo "YGG_ADDRESS=${ADDR:-none}"
echo "YGG_ROUTES=${ROUTES:-0}"
echo "YGG_WAITED=$i"
PEERADDR=$(grep -oE 'Connected outbound: [0-9a-f:]+' /tmp/ygg.log | head -1 | sed 's/Connected outbound: //')
echo "CONTROL_PEER=${PEERADDR:-none}"
if [ -n "${PEERADDR:-}" ]; then
  echo "CONTROL_PING=$(ping -6 -c 3 -W 5 "$PEERADDR" 2>&1 | grep -oE '[0-9]+ packets received' | head -1)"
else
  echo "CONTROL_PING=no peer"
fi
echo "SUBJECT_PING=$(ping -6 -c 4 -W 8 __SUBJECT__ 2>&1 | grep -oE '[0-9]+ packets received' | head -1)"
for port in __PORTS__; do
  echo "SUBJECT_TCP_${port}=$(curl -s -o /dev/null -m 25 -w '%{http_code}/%{time_connect}' "http://[__SUBJECT__]:${port}/" 2>&1 || echo failed)"
done
pkill yggdrasil 2>/dev/null || true
"""


def yggdrasil_subjects(fixtures: str, resolver: Resolver,
                       records: list[str] | None = None) -> list[dict]:
    """Corpus hosts whose only addresses are yggdrasil ones.

    ⛔ Resolved, not pattern-matched. The network is a property of where a name
    points, and `yggtracker.i2p.rocks` looks like an ordinary clearnet name.

    ⭐ **But the committed sweeps are asked first, and that is RULES 15.3
    rather than an optimisation.** The sweeps already resolved and classified
    every host they probed, so the candidate list comes out of our own
    snapshot for free; resolving the whole corpus to rediscover it would be
    **965 lookups to learn what is already written down**, and the first draft
    of this function did exactly that. Only the candidates are re-resolved, to
    confirm the classification still holds today.
    """
    candidates: set[str] = set()
    for path in (records or []):
        try:
            with open(path, encoding="utf-8") as handle:
                doc = json.load(handle)
        except OSError:
            continue
        for row in doc.get("trackers") or []:
            if row.get("network") == "yggdrasil":
                candidates.add(str(row.get("url", "")))

    aggregate, _, _ = load_corpus(True, fixtures)
    trackers = sorted(aggregate.trackers, key=lambda t: t.url)
    if candidates:
        trackers = [t for t in trackers if t.url in candidates]

    out: list[dict] = []
    seen: set[str] = set()
    for tracker in trackers:
        if tracker.host in seen:
            continue
        seen.add(tracker.host)
        try:
            answer = resolver.addresses(tracker.host)
        except Exception:  # noqa: BLE001 - a name we cannot resolve is not one
            continue
        addresses = [a for family in answer.get("addresses", {}).values()
                     for a in family]
        if not addresses:
            continue
        ygg = [a for a in addresses if _in_yggdrasil(a)]
        if ygg and len(ygg) == len(addresses):
            out.append({"url": tracker.url, "host": tracker.host,
                        "address": ygg[0], "addresses": addresses,
                        "port": tracker.port,
                        "found_by": ("a committed sweep's classification"
                                     if candidates else
                                     "resolving the corpus, because no "
                                     "committed sweep named one")})
    return out


def _in_yggdrasil(address: str) -> bool:
    try:
        return ipaddress.ip_address(address) in YGGDRASIL_NET
    except ValueError:
        return False


def dual_stack_check(host: str, resolver: Resolver) -> dict:
    """T-031's route (e): does this host have a clearnet sibling after all?

    ⭐ The cheapest route, and it has to be re-run rather than remembered: a
    host that is yggdrasil-only today may publish an A record tomorrow, and
    that would retire this whole instrument for that subject.
    """
    record: dict = {"host": host}
    try:
        infos = socket.getaddrinfo(host, None, socket.AF_INET)
        record["system_ipv4"] = sorted({i[4][0] for i in infos})
    except OSError as exc:
        record["system_ipv4"] = []
        record["system_ipv4_error"] = f"{type(exc).__name__}: {exc}"
    try:
        record["public"] = resolver.addresses(host)
    except Exception as exc:  # noqa: BLE001
        record["public_error"] = f"{type(exc).__name__}: {exc}"
    v4 = list(record.get("system_ipv4") or [])
    v4 += list((record.get("public") or {}).get("addresses", {}).get("ipv4", []))
    record["has_clearnet_sibling"] = bool(v4)
    record["detail"] = (
        "a clearnet address exists, so this subject does not need the overlay"
        if v4 else
        "no A record from this host's resolver or from the public ones; the "
        "overlay is the only route")
    return record


def run_guest(engine: str, image: str, subject: str, ports: list[int],
              timeout: float) -> dict:
    """Bring a node up in a throwaway container and read what it saw."""
    script = (GUEST
              .replace("__PEERS__", ", ".join(f'\\"{p}\\"' for p in PEERS))
              .replace("__SUBJECT__", subject)
              .replace("__PORTS__", " ".join(str(p) for p in ports)))
    argv = [engine, "run", "--rm", "--platform", "linux/amd64",
            "--cap-add", "NET_ADMIN", "--device", "/dev/net/tun",
            image, "sh", "-c", script]
    env = dict(os.environ)
    # ⛔ Git Bash rewrites anything that looks like a POSIX path before the
    # engine sees it, and `/dev/net/tun` is exactly that shape.
    # docs/containers.md, and conventions/shell.md section 6.
    env["MSYS_NO_PATHCONV"] = "1"
    env["MSYS2_ARG_CONV_EXCL"] = "*"
    try:
        proc = subprocess.run(argv, capture_output=True, text=True,
                              encoding="utf-8", errors="replace",
                              timeout=timeout, env=env)
    except FileNotFoundError:
        return {"ok": False, "error": f"no container engine named {engine!r}"}
    except subprocess.TimeoutExpired:
        # ⚠ "It never answered" is a different fact from "it refused", and
        # belongs in a different field (RULES 1.4).
        return {"ok": False, "error": f"the container did not finish inside "
                                      f"{timeout}s"}
    fields: dict[str, str] = {}
    for line in (proc.stdout or "").splitlines():
        if "=" in line and line.split("=", 1)[0].isupper():
            key, _, value = line.partition("=")
            fields[key.strip()] = value.strip()
    return {"ok": proc.returncode == 0, "returncode": proc.returncode,
            "fields": fields,
            "stderr_tail": (proc.stderr or "").splitlines()[-3:]}


def packets(text: str) -> int:
    match = re.search(r"(\d+) packets received", text or "")
    return int(match.group(1)) if match else 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--engine",
                        default=os.environ.get("CONTAINER_ENGINE", "docker"),
                        help="container engine. ⛔ Never assumed by name: "
                             "RULES 15.5, and podman must work unchanged")
    parser.add_argument("--image", default=DEFAULT_IMAGE)
    parser.add_argument("--timeout", type=float, default=420.0)
    parser.add_argument("--fixtures", default=FIXTURES)
    parser.add_argument("--out", default=None)
    parser.add_argument("--records", nargs="*",
                        default=sorted(glob.glob(os.path.join(
                            REPO, "experiments", "results",
                            "health-sweep.*.json"))),
                        help="committed sweeps to take the candidate list "
                             "from. ⭐ They already classified every host they "
                             "probed, so this costs no lookup; without them "
                             "the corpus is resolved from scratch, which is "
                             "965 of them to rediscover what is written down")
    parser.add_argument("--check-dual-stack", action="store_true",
                        help="re-run T-031's route (e) and stop there. Cheap, "
                             "and it retires this instrument for any subject "
                             "that has grown a clearnet address")
    parser.add_argument("--expect-control", action="store_true",
                        help="exit 1 if the node did not join or the control "
                             "peer did not answer, because then no subject row "
                             "means anything")
    args = parser.parse_args()

    resolver = Resolver()
    subjects = yggdrasil_subjects(args.fixtures, resolver, args.records)
    if not subjects:
        print("no corpus host resolves only into 200::/7; nothing to measure. "
              "That is a result: re-run when one appears.", file=sys.stderr)
        return C.EXIT_COULD_NOT_RUN

    dual = [dual_stack_check(s["host"], resolver) for s in subjects]
    if args.check_dual_stack:
        for row in dual:
            print(f"{row['host']}: {row['detail']}")
        C.emit("Do the yggdrasil-only corpus hosts have a clearnet sibling?",
               C.collect(sample_counts={"subjects": len(subjects)}),
               {"dual_stack": dual, "subjects": subjects}, args.out)
        return C.EXIT_MEASURED

    subject = subjects[0]
    ports = sorted({subject.get("port") or 80, 80})
    guest = run_guest(args.engine, args.image, subject["address"], ports,
                      args.timeout)
    fields = guest.get("fields", {})
    joined = fields.get("YGG_ADDRESS", "none") != "none" and \
        int(fields.get("YGG_ROUTES", "0") or 0) > 0
    control_ok = packets(fields.get("CONTROL_PING", "")) > 0
    subject_packets = packets(fields.get("SUBJECT_PING", ""))
    tcp = {k: v for k, v in fields.items() if k.startswith("SUBJECT_TCP_")}
    reached = subject_packets > 0 or any(
        not v.startswith("000") and v != "failed" for v in tcp.values())

    results = {
        "controls": {
            "joined": {"ok": joined, "address": fields.get("YGG_ADDRESS"),
                       "routing_table": fields.get("YGG_ROUTES"),
                       "seconds_to_join": fields.get("YGG_WAITED"),
                       "yggdrasil": fields.get("YGG_VERSION")},
            "peer_reachable": {"ok": control_ok,
                               "peer": fields.get("CONTROL_PEER"),
                               "ping": fields.get("CONTROL_PING"),
                               "detail": ("a peer answered over the overlay, so "
                                          "a subject that does not answer is a "
                                          "fact about the subject" if control_ok
                                          else "no peer answered over the "
                                               "overlay; NOTHING below is a "
                                               "statement about any tracker")},
        },
        "dual_stack": dual,
        "subject": subject,
        "subject_ping_packets_received": subject_packets,
        "subject_tcp": tcp,
        "rung": (Rung.CONNECTED.value if reached else Rung.NONE.value),
        "verdict": ("reached" if reached else "did_not_answer"),
        "engine": args.engine,
        "image": args.image,
        "peers_offered": list(PEERS),
        "guest": {k: v for k, v in guest.items() if k != "fields"},
        "what_this_is_not": (
            "NOT `dead`. One observation from one node on one day, and "
            "MIN_SAMPLES_FOR_DEATH is 3 (RULES 3.1). It is also not a scrape: "
            "the subject has no derivable scrape endpoint, so this reaches "
            "CONNECTED at most and never announces."),
    }

    conditions = C.with_network_vantage(C.collect(sample_counts={
        "subjects": len(subjects), "ports": len(ports),
    }))
    C.emit("Is the corpus's yggdrasil tracker alive, from a node on yggdrasil?",
           conditions, results, args.out)

    print(f"\nCONTROL joined the network: {'PASS' if joined else 'FAIL'}  "
          f"address {fields.get('YGG_ADDRESS')}, routing table "
          f"{fields.get('YGG_ROUTES')}, after {fields.get('YGG_WAITED')}s")
    print(f"CONTROL peer answers over the overlay: "
          f"{'PASS' if control_ok else 'FAIL'}  {fields.get('CONTROL_PING')} "
          f"from {fields.get('CONTROL_PEER')}")
    print(f"\nSUBJECT {subject['url']}")
    print(f"  address    {subject['address']}")
    print(f"  ping       {fields.get('SUBJECT_PING')}")
    for key, value in sorted(tcp.items()):
        print(f"  {key.lower():10s} {value}")
    print(f"  verdict    {results['verdict']} (rung {results['rung']})")
    print("\nWHAT THIS DOES NOT ESTABLISH")
    print("  - That the tracker is dead. One observation, and RULES 3.1.")
    print("  - Anything about i2p, which experiments/35 measures separately.")
    print("  - That CI could do this. It needs a TUN device and NET_ADMIN.")

    if args.expect_control:
        if not joined:
            print("\nEXPECTATION FAILED: the node never joined, so the subject")
            print("  row measures our container and not the tracker.")
            return C.EXIT_MEASURED_AND_FAILED
        if not control_ok:
            print("\nEXPECTATION FAILED: no peer answered over the overlay, so")
            print("  a silent subject is indistinguishable from a silent us.")
            return C.EXIT_MEASURED_AND_FAILED
    return C.EXIT_MEASURED


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(C.EXIT_COULD_NOT_RUN)
