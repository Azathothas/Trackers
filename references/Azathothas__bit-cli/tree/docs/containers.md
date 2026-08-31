# Measuring in a throwaway container

Some things this repository has to measure cannot be measured on the machine
doing the measuring. A browser newer than the installed one, a
filesystem this host does not have, a libc this host does not use: each needs a
different machine, and waiting for CI to be that machine costs five minutes a
question.

A throwaway WSL2 distro is that machine, and it leaves nothing behind. Creating
one from `debian:bookworm-slim` and running a command in it is seconds; what
takes time is whatever the command does, which for installing a browser is a
few minutes.

**This page is a procedure, not a dependency.** Nothing in `scripts/` requires
a container to run, and no gate does. A check that can use one says so and
exits **2** when there is none, the same way
[`../scripts/check-browser-fingerprint.ps1`](../scripts/check-browser-fingerprint.ps1)
exits 2 when there is no browser. A machine with no container engine is not a
failing build.

## What is on this machine

| | |
| --- | --- |
| engine | podman 5.8.6, with a `podman-machine-default` WSL distro |
| distro tool | `wsl-ephemeral.ps1`, from `Azathothas/ToolKit` |
| this repository's entry point | [`../scripts/wsl-tool.ps1`](../scripts/wsl-tool.ps1) |
| networking | WSL2 in **NAT** mode, set deliberately in `.wslconfig` |

## Getting the tool: run `scripts/wsl-tool.ps1` and nothing else

```bash
pwsh -NoProfile -File scripts/wsl-tool.ps1 -Action List
```

That resolves the tool at the revision
[`../scripts/toolkit-pin.json`](../scripts/toolkit-pin.json) names, verifies the
bytes against the digest beside it, and forwards every other argument
unchanged. There is no second copy of the rules about fetching it, and a
session does not have to remember any of them.

**Pin a commit, never a branch.** `main` moves, and a moved reference runs code
nobody reviewed. The pin file carries the commit and both digests. To move it:

```bash
gh api repos/Azathothas/ToolKit/commits/main --jq .sha
```

```bash
curl -sSL "https://raw.githubusercontent.com/Azathothas/ToolKit/<SHA>/scripts/powershell-windows/wsl-ephemeral.ps1" | sha256sum
```

**Read a digest from the raw endpoint and never from a working tree.** That
repository stores `.ps1` with CRLF in a checkout and LF in the index, so a
locally computed digest disagrees with what the endpoint serves. It fails
closed, which is safe and takes an hour to work out.

**A stale copy beside the launcher wins over the pin, silently.** The launcher
resolves in three steps and the first hit wins: an explicit local path, then a
`wsl-ephemeral.ps1` **beside** the launcher, then the pinned ref. Measured here
with an older copy left in `.tmp/`: a run passing both a ref and a digest
printed `Using the copy beside this launcher`, ran that file, and verified
nothing. `wsl-tool.ps1` keeps its cache directory holding the
launcher alone and removes any sibling before running, which is why that
resolution order cannot bite here.

`List` is the first thing to run and the last. It reports every distro the tool
made, every distro it will never touch, and any rootfs tarball a cancelled run
orphaned.

## Running something in one

```bash
pwsh -NoProfile -File scripts/wsl-tool.ps1 -Action New -Image debian:bookworm-slim -Name eph-bitcli-x -Force -CommandB64 "$(base64 -w0 < script.sh)"
```

**Pass the command as `-CommandB64`.** It is not a preference. A command sent
as text is parsed by PowerShell before it reaches the distro: `$VAR` expands in
transit, a backtick opens a command substitution, and Windows PowerShell 5.1
drops a double quote out of a child process's argument list before the script
ever sees it. Base64 has no character any shell touches.

**Write the script with LF endings.** The command channel is byte exact and
will not repair anybody's payload, so a CRLF script makes `/bin/sh` read the
carriage return as part of the last word on every line. A here-string inside a
`.ps1` in this repository is CRLF, because `.gitattributes` says so, and has to
be normalised before it is encoded.

**`/bin/sh` is dash on Debian.** `/dev/tcp` is a bash builtin and is not there;
call `bash -c` when you need it. `ip` is not in `-slim` images either.

**Bound anything that can wait forever.** `-TimeoutSeconds` bounds the
questions the tool asks a distro for itself, deliberately **not** `-Command`: a
build that runs for an hour is a legitimate command. So a guest command that
can hang carries its own `timeout`. Measured here: a Chrome that could
not complete a handshake sat until the distro was killed, because nothing in
the chain bounded it.

**The exit code is the command's.** `New -Command` and `Run -Command` both
return what ran inside, where `New` used to report 0 over a failing command.
A destructive action reads the directory back afterwards and exits non-zero
when the disk is still there, so `Remove` and `Purge` can now fail where they
used to print success.

**Failure messages go to stderr and results go to stdout**, so a value can be
taken straight off a command:

```bash
pwsh -NoProfile -File scripts/wsl-tool.ps1 -Action HostAddress 2>/dev/null
```

## Reaching this host from inside one

WSL is in **NAT** mode here, so a distro does **not** reach the Windows
loopback. `localhost` inside the distro is the distro, and the failure is
silent: a fixture bound to loopback simply never receives a connection and
nothing on either side says why.

**Ask the tool. It answers without creating a distro.**

```bash
pwsh -NoProfile -File scripts/wsl-tool.ps1 -Action HostAddress
```

```
172.23.96.1
```

| the mode in `.wslconfig` | the answer |
| --- | --- |
| `mirrored` | `127.0.0.1`, and a caller's branch disappears |
| `nat`, which is the default and what this machine uses | the host's address on its WSL adapter |
| `bridged` | refused, because the distro is on the LAN and which host address it reaches is a choice rather than a lookup |

**Read it, never record it.** WSL assigns the address and it changes. The value
above is what this machine had on the day it was measured.

**There is no port forwarding and there will not be.** It was asked for and
refused: forwarding a port on Windows means `netsh interface portproxy`, which
needs an elevated session and leaves a rule behind after the tool exits. Bind
the host service to the address above instead. `0.0.0.0` is not the answer,
because it accepts the LAN as well.

[`../crates/bit-cli-core/examples/loopback-tlsprobe/`](../crates/bit-cli-core/examples/loopback-tlsprobe/)
takes `--bind` for exactly this, defaults to loopback, and refuses the
unspecified address by name.

## Getting a browser into one

Two sources, and which one is right depends on which half of a fingerprint is
being read. The versions below are what they gave when this was written; read
them again rather than trusting them.

| source | version it gave | what it is |
| --- | --- | --- |
| **Chrome for Testing** | `Stable` 152.0.7977.64, `Beta` 153.0.8010.12, and `Dev` and `Canary` beyond | Google's own per-channel index of the builds it publishes for automation, with a download URL per platform |
| `debian:bookworm-slim` plus Google's apt repository | 152.0.7977.64 | Google's branded stable package, and stable only |

```bash
curl -sS https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json
```

**Chrome for Testing is addressable by channel**, which the apt package is not:
it reaches Beta, Dev and Canary as well as Stable, so a profile can be captured
**before** a version ships rather than after.

**Chrome for Testing is unbranded, and that is a real limit rather than
cosmetic.** Measured here, its `sec-ch-ua` carries no Google Chrome
entry at all:

```
chrome-for-testing  "Not?A_Brand";v="24", "Chromium";v="152"
```

So a capture that has to read the brand list uses the apt package, and one that
has to reach a channel uses Chrome for Testing.
`scripts/check-browser-fingerprint.ps1 -Source apt|cft` is that choice.

**Download it into the distro, never onto the host.** Installing a browser on
somebody's machine is a system change nobody asked for; installing it into a
distro that is destroyed afterwards is not.

### Two things about driving a browser in a distro

**Chrome on Linux does not read `~/.pki/nssdb` for server authentication.**
Measured here: the probe's own authority was added with
`certutil -t "C,,"`, `certutil -L` listed it, and Chrome 152 still answered
`CertificateUnknown` and closed. Chrome uses its own root store there. So a
capture against a fixture's throwaway certificate passes
`--ignore-certificate-errors --test-type` **to the browser**, which changes what
the browser accepts after the handshake rather than the `ClientHello` it sends.
Nothing that ships has such a flag.

**A browser opens sockets it then abandons.** Measured on the same run: driving
Chrome 152 at the probe produced 13 connections, the **first** carrying no
HTTP/2 at all, and every one after the second carrying `pre_shared_key` because
the session resumed. A capture has to be the first connection that reached
HTTP/2: the first is a preconnect and the last is a resumption, and neither is
what a cold client sends. The probe's `--until-h2` waits on that condition.

## Decommissioning, which is not optional

Every distro is removed in the same run that made it, and the run checks rather
than assuming.

```bash
pwsh -NoProfile -File scripts/wsl-tool.ps1 -Action Remove -Name eph-bitcli-x -Force
```

```bash
pwsh -NoProfile -File scripts/wsl-tool.ps1 -Action List
```

`-Ephemeral` on `New` does both in one call and destroys the distro even when
the command fails. Use it when the distro is wanted for exactly one command;
use an explicit `Remove` when the guest's output is read afterwards, so a
failure to run and a failure to clean up stay legible as two things.

**A cancelled run leaves a distro registered and a rootfs tarball of several
hundred MiB** in `%LOCALAPPDATA%\wsl-ephemeral\`, because the cleanup is a
`finally` and a hard interrupt does not run one. Measured here,
after a run was killed mid-install: one registered distro and a 74.3 MiB
orphan, both removed by `Purge`.

```bash
pwsh -NoProfile -File scripts/wsl-tool.ps1 -Action Purge -Force
```

`List` reports each tarball with its size and the time it was written. Read the
time first: a `New` that is running right now has its tarball in the same
directory and nothing can tell the two apart.

**Never run `wsl --shutdown`.** It is machine wide, and it is the command a
person reaches for after finishing with a throwaway distro. It stops every
distro on the machine including `podman-machine-default`, which takes the
container runtime down. The tool never runs it and neither should a session.

## What the tool will not do

The removal path is constrained four ways and every destructive call goes
through all of them: a fixed `eph-` prefix, a refusal for any name without it,
a protected list that includes `podman-machine-default` and the Docker and
Rancher distros, and a directory deletion confined to one base directory. A
mistake here cannot destroy the container runtime.

`Purge` finishes both its loops before reporting, so one item it cannot remove
does not hide the state of everything after it.

## Two traps that are the kernel's rather than the tool's

**A distro's lifetime is not the kernel's lifetime.** `--terminate` and
`--unregister` restart or remove the distro userspace, and the WSL2 kernel keeps
running in the utility VM. `binfmt_misc` registrations, loaded modules and
pinned superblocks survive, so a throwaway distro used to reproduce a
kernel-level condition can read state hours old and answer confidently.

**A foreign-architecture rootfs may run rather than fail**, which is the worse
outcome. That shared kernel carries whatever `qemu-*` handlers anything on the
machine registered, and the `F` flag holds the interpreter open, so a riscv64
rootfs on this x86_64 host boots and answers `riscv64` to `uname -m`. The tool
names `--platform linux/<arch>` on every pull and create for that reason: the
local image store is keyed by tag and not by architecture, so one
`pull --platform linux/riscv64 alpine` repoints the shared `alpine:latest` and
every later unqualified pull hands back the wrong one.

## Images cost more disk than they look like they do

An 8 MiB Alpine rootfs becomes a 76 MiB VHDX and a 74 MiB Debian one becomes
172 MiB: the cost is dominated by a fixed floor rather than by a multiple of the
input. `New` measures free space before importing and refuses rather than
leaving a half-written disk and a registered distro that does not work. It asks
for 256 MiB plus twice the tarball, and a volume it cannot measure is imported
anyway with a line saying so.

```
  * space: 405 MiB needed, 326,617 MiB free
```

## Leaving the engine as it was found

A session that pulls an image or creates a volume removes it. The engine is
shared with everything else on the machine and a session's leftovers are
somebody else's disk.

```bash
pwsh -NoProfile -File scripts/wsl-tool.ps1 -Action Resources
```

That reports what this script made, what else WSL has registered and never
touches, and what the engine is holding, and it prints every cleanup command
without running one. Reclaiming somebody's disk is their decision.

```bash
podman system df
```

That is the one number to read before finishing: `RECLAIMABLE` at 100 percent
of a large `SIZE` means something stopped cleaning up after itself. A named
volume with no container attached is not necessarily garbage, which is why the
report is a count and a size rather than a recommendation.
