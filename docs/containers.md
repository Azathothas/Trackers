# containers.md

Measuring something this machine cannot measure, in a machine you throw away
afterwards.

⚠ **This page is not about running the gate.** The gate is Python, offline, and
runs on any host (RULES 15.5). A container here is an **instrument**, and what
it buys is a different vantage.

---

## ⭐ When this is the right answer

The `local` profile exists because a GitHub runner cannot reach things a
contributor's machine can (RULES 15.1). A container is how a contributor gets a
vantage neither of those has, and this project has five open questions that
need one:

| what is unreachable from CI | what a container gives |
| --- | --- |
| **IPv6 egress**, measured false on both runner images (`C-04`) | a host or network with real IPv6, which turns an entire class of `unmeasurable` into a measurement |
| **I2P** | an i2pd router, and a transport that speaks to it |
| **Yggdrasil** | a node on the network |
| **`wss`** | nothing container-shaped; it needs a transport in the ladder, not a machine |
| **a second vantage for correlation** | the thing [T-031](../TODO/measurement.md) is actually about: two observers disagreeing is a first-class output, not a nuisance |

⛔ **A container does not make a result true.** A measurement taken inside one
carries its vantage like every other (RULES 3.4), and the profile that produced
it is part of the record.

⚠ **A result from a container is not a `ci` result and must never be merged
with one as though it had equal reach.** Disagreement between profiles is
[T-004](../TODO/foundation.md).

---

## The engine is a variable, never a name

⛔ **Never assume a container runtime by name.** RULES 15.5: an engine is
invoked through `CONTAINER_ENGINE`, defaulting to `docker`, and `podman` must
work unchanged. A contributor on Windows with podman and no docker is a
supported case and was measured on the machine this page was written on.

```bash
"${CONTAINER_ENGINE:-docker}" run --rm alpine:3 sh -c 'echo ok'
```

---

## ⛔ Pin it, and verify the bytes before anything executes

⚠ **A tag moves.** `alpine:latest` today is not `alpine:latest` next month, and
a measurement whose environment moved is not reproducible. Pin by digest, and
record the digest in the experiment's conditions block, which is what
[`../experiments/_conditions.py`](../experiments/_conditions.py) exists for.

⛔ **Anything fetched and then executed is verified first.** A download piped
into a shell runs code nobody reviewed, from a host nobody controls, with the
caller's privileges.

---

## ⚠ Four traps, each paid for

- ⛔ **A cache keyed without the variant serves the wrong image.** Fetching
  `--platform linux/riscv64 alpine` retags the shared local `alpine:latest` to
  the riscv64 image, so the next plain run fails with an exec-format error that
  reads as an unrelated breakage. ⭐ Name the platform on every fetch.
- ⛔ **Git Bash rewrites arguments that look like POSIX paths** before the
  engine sees them, silently. Any command whose paths are destined for the
  guest carries both `MSYS_NO_PATHCONV=1` and `MSYS2_ARG_CONV_EXCL='*'`.
  [`conventions/shell.md`](conventions/shell.md) section 6.
- ⛔ **Bound anything that can wait forever.** A container that never answers
  looks exactly like one doing slow work. Every run gets a hard time limit, and
  a timeout is recorded as "it never answered", which is a different fact from
  "it refused" and belongs in a different field (RULES 1.4).
- ⚠ **`systemd-binfmt` reports success over zero registered handlers.** A green
  unit, a complete config, installed emulators, and cross-architecture
  execution that has never once worked. ⛔ A step that can only pass verifies
  its own effect.

---

## ⛔ Decommissioning is not optional

RULES 13 and [`security/remote-ops.md`](security/remote-ops.md): anything a
session created, that session removes. For a container that means the
container, its volumes, the image if it was pulled for this run alone, and any
network it created.

⚠ **Verify the teardown by counting, not by remembering.** A count that returns
to its baseline is evidence.

⚠ **Images cost more disk than they look like they do.** A machine that runs a
few of these accumulates layers nothing references, and the engine does not
reclaim them on its own.

⚠ **Leave the engine as it was found.** Do not change its configuration, its
registries or its storage driver to make one run work. A machine-wide change
made for one measurement outlives the session that needed it.
