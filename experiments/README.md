# experiments/

Every measured number this project publishes was taken by a script in this
directory. If a number appears in a document and no script here produced it,
the number is wrong until proved otherwise.

The rules are `Azathothas/TEMPLATE`'s `docs/methodology/experiments.md`, read
2026-08-29 at `6eaf4b5` and re-read 2026-08-31 at `6206166` -- **byte-identical
between the two** -- adopted here in full and restated normatively in RULES 2:

* **An experiment is a file, not a transcript.** Numbered, kept forever.
* **A number is never reused.** A replaced experiment gets the next number, so
  that a citation of `02-` keeps meaning what it meant.
* **Every input is pinned.** See `fixtures/`.
* **Conditions are printed on the way out** -- host, environment class, tool
  versions, date, sample counts, and the public address the probe went out from.
* **The exit code means something.** `0` measured, `1` measured and an
  expectation failed, `2` could not run.
* **No dependence on the working directory.** Paths resolve from the script.
* **Nothing cleans up its own output.** The evidence is the point. What that
  does and does not license is under *Which results are kept* below.
* **A negative result is a result and it gets committed.**
* **A correlation is not a cause**, and naming a culprit needs a control that
  isolates it.
* **Pick a metric that is stable under changes you do not care about.**

## The scripts

| # | question it answers | claims |
| --- | --- | --- |
| `01-host-network-baseline.py` | Which network egress does this host actually have -- which TCP ports, which UDP ports, does IPv6 leave the machine? | C-01, C-04 |
| `02-udp-bep15-connect.py` | Does a BEP 15 connect complete against known-good UDP trackers, and if not, is it the network, the trackers, or the probe? | C-01, C-03, C-30, C-31 |
| `03-inbound-connectivity.py` | Can anything on the public internet open a connection to this host? | C-02 |
| `04-dns-resolver-divergence.py` | Does this host's resolver answer tracker hostnames the way public resolvers do? | C-06 |
| `05-http-tracker-protocol.py` | Is a bencoded response the reliable discriminator between an HTTP tracker and a web server? | C-32, C-35, C-40 |
| `19-scheme-census.py` | Which URL schemes and reachability networks actually occur across every source this project might consume? | C-21, C-36, C-37, C-52 |
| `20-newtrackon-api-surface.py` | What does newTrackon's API actually serve, and is machine-readable uptime obtainable? | C-23, C-24, C-26, C-53 |
| `21-raw-github-consumption.py` | Does `raw.githubusercontent.com` behave the way the consumer contract assumes -- caching, content type, propagation? | C-16 |
| `22-actions-platform-contract.py` | Does GitHub's documentation still say the things this project's schedule and publication design assume? | C-10, C-11, C-12, C-19b, C-55 |

**The numbering is the order they were written and a number is never reused.**
It is deliberately *not* the numbering of the brief's twenty-item experiment
programme, which is tracked separately by [T-030](../TODO/measurement.md) -- that
list's items 3-18 are unrun, and conflating the two numbering schemes would
make a citation ambiguous.

## The control hierarchy, and why every script has one

RULES 2: **an absence is not a zero.** A probe that returns nothing may
mean the thing is dead, or that the probe was never able to speak in the first
place. Those are indistinguishable without a control that *does* answer.

So each script runs its subjects and its controls **through the same code
path**, and reports which tier broke:

```
tier 0   a responder this process starts, on loopback
         -> proves the probe code can build, send and read the protocol.
            If tier 0 fails, no other row in the output may be quoted.

tier 1   a third party on a non-53 UDP port that answers deterministically
         (STUN/RFC 5389, NTP/RFC 5905)
         -> separates "the network blocks this" from "the code is broken".

tier 2   the subjects: real trackers.
```

`05-http-tracker-protocol.py` adds the control that matters most for this
project's honesty: a **negative** control. It starts a plain web server that
returns HTTP 200 with HTML, and asserts the probe does **not** call it a
tracker. That is the first row of RULES 11's anti-pattern table -- *"treating HTTP 200 as
'tracker alive'"* -- wired to a non-zero exit so it cannot come back unnoticed.

## Running them

```sh
python3 experiments/01-host-network-baseline.py
python3 experiments/02-udp-bep15-connect.py
python3 experiments/05-http-tracker-protocol.py --expect-controls
```

**`19` is the only one that runs offline**, from its committed fixture cache,
which is why it is the one in the gate:

```sh
python3 experiments/19-scheme-census.py --offline --expect-known-schemes
```

The rest touch a third party, so they run deliberately and not on every push
(RULES 15.2). `20`, `21` and `22` read public documentation and APIs; `01`-`05`
need a GitHub runner to mean anything at all, because the whole question they
answer is what *that* vantage can do.

Python 3.11+, standard library only. No dependency to install, and none to rot
during the five years this project is meant to run.

The `--expect-*` flags turn an experiment into a regression check: it exits `1`
when the expectation is violated. They are deliberately **off** in the P0
workflow, because P0's job is to find out what is true rather than to assert
what was hoped.

## Where results go

`results/`, named `<script>.<environment-class>.<timestamp>.json`, committed.
The environment class is in the filename because it is the condition that
matters most here: a measurement from a GitHub-hosted runner and a measurement
from the authoring sandbox answer different questions, and a file named only by
date invites reading one as the other.

### Which results are kept, and why that is not "all of them"

"Nothing cleans up its own output" means **an experiment never deletes its own
evidence**. It does not mean the repository hoards indistinguishable re-runs,
and by 2026-08-31 it had: ten committed runs of the offline scheme census, all
reporting the same numbers, differing only in a timestamp.

The distinction that decides it is **whether the result is reproducible on
demand**:

| kind of run | keep | why |
| --- | --- | --- |
| touched a **third party or the network** (`20`, `21`, `22`) | **every one** | irreproducible. It records what somebody else's server said at one moment, and that moment does not come back |
| taken on a **GitHub runner** (`01`-`05`) | **every one** | irreproducible from here at all, and the whole measured baseline rests on them |
| **offline**, against pinned fixtures (`19 --offline`) | the **first** and the **most recent** | the script plus the committed fixture *is* the evidence; anyone can regenerate the rest in a second. The first is kept because documents cite it; the most recent because it shows the instrument still runs |

Where a run is deleted under this rule, it is deleted because it can be
regenerated by a command in this file -- never because it was inconvenient. A
result that cannot be regenerated is never deleted, no matter how many like it
already exist.

## What these experiments cannot tell you

* **That a result generalises.** One machine on one day is one machine on one
  day. Both environments used so far are datacenter address space; neither is a
  residential connection, which is where this dataset's consumers actually sit.
* **That a tracker is dead.** They establish that a tracker did not answer *us*.
  RULES 3.4 is the whole reason those are different sentences.
* **That an unreachable port is blocked by GitHub rather than by the tracker.**
  Tier 1 separates those, and only tier 1.
