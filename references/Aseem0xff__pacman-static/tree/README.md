# pacman-static — five architectures, built from source

> **This is a POC for https://github.com/pkgforge-dev/docker-archlinux**

Statically linked `pacman`, cross-compiled from upstream source for
**x86_64, aarch64, riscv64, loongarch64 and powerpc64le**, with every optional
feature linked in. No musl toolchain to build, no GCC cross-compiler, no
prebuilt binary downloaded.

Verified on 2026-08-28: all five build, link static, run under emulation, and
**bootstrap their own architecture's Arch-family distribution from the real
repositories**.

| arch | distribution | pacman | packages installed | bytes |
| --- | --- | --- | --- | --- |
| `x86_64` | Arch Linux | 7.1.0 | 137 | 14 708 504 |
| `aarch64` | Arch Linux ARM | 7.1.0 | 136 | 14 321 560 |
| `riscv64` | Arch Linux RISC-V | 7.1.0 | 135 | 18 059 496 |
| `loongarch64` | LoongArch Linux | 7.1.0 | 137 | 13 512 296 |
| `powerpc64le` | ArchPOWER | 7.1.0 | 140 | 15 419 992 |

Sizes are unstripped, with `debug_info`. Package counts come from
[`experiments/out/95-cross-arch-bootstrap.txt`](experiments/out/95-cross-arch-bootstrap.txt),
each installed by that architecture's own binary against that architecture's
own distribution.

⚠ **One known open fault:** a low-rate intermittent `SIGSEGV` *after* a
complete install — every package is there and only the exit status is wrong.
It moves between architectures across runs.
[`RESEARCH.md` §9](RESEARCH.md#9-the-crash-that-is-not-closed).

---

## Licence — take it, no attribution

`SPDX-License-Identifier: 0BSD`. **Copy, edit, ship, sell any file here — the
prose, the scripts, the patches, the fixtures — with no credit, no notice and
no permission.** Details and third-party scope in [`LICENSING.md`](LICENSING.md).

---

## Start here

| you want to… | go to |
| --- | --- |
| **build a binary** | [`examples/01-build-from-source.md`](examples/01-build-from-source.md) |
| **bootstrap a root** | [`examples/02-bootstrap-arch-rootfs.md`](examples/02-bootstrap-arch-rootfs.md) |
| **fix a system with no libc** | [`examples/03-repair-a-broken-system.md`](examples/03-repair-a-broken-system.md) |
| **build a foreign-arch root** | [`examples/04-cross-architecture-rootfs.md`](examples/04-cross-architecture-rootfs.md) |
| **avoid the traps** | [`docs/GOTCHAS.md`](docs/GOTCHAS.md) — ⛔ read **G-01** before any `chroot` work |
| **know what is in the binary** | [`docs/FEATURES.md`](docs/FEATURES.md) |
| **do the remaining work** | [`TASKS.md`](TASKS.md) |
| **check my claims** | [`RESEARCH.md`](RESEARCH.md), then run anything in `experiments/` |

---

## Minimum host requirements

**No compiler needed.** `zig cc` is the toolchain — one 55 MB download.

| tool | version | why |
| --- | --- | --- |
| `curl` | any | fetch sources |
| `git` | any | zstd tag, pacman source |
| `tar` + `xz` + `bzip2` + `gzip` | any | unpack |
| `make`, `patch` | any | build |
| `cmake` | ≥ 3.16 | xz, brotli |
| `meson` | ≥ 0.61 | pacman |
| `ninja` | any | meson backend |
| `pkg-config` or `pkgconf` | any | dependency resolution |
| `perl` | any | OpenSSL `Configure` |
| `python3` | ≥ 3.8 | meson, the page joiner |
| `binutils` | any | `readelf`, for the static-linkage assertion |

Optional but strongly recommended:

| tool | why |
| --- | --- |
| `qemu-user-static` | run and test a foreign-architecture binary |
| `gdb` | backtrace, for `92-segfault-rate.sh` |
| `node` | only to re-run the defect demo in `40-` |

```sh
# Debian / Ubuntu
apt-get install -y --no-install-recommends \
    curl git tar xz-utils bzip2 gzip make patch cmake ninja-build \
    pkg-config perl python3 python3-pip binutils qemu-user-static gdb
pip3 install meson
```

Disk: ~2 GB per architecture during the build, ~800 MB per bootstrapped root.
Time: ~5 minutes per architecture on 4 cores.

---

## Reproduce everything

```sh
git clone <this repo> && cd pacman-static
export WORK=$HOME/pacman-static-work

# toolchain: one download
mkdir -p "$WORK" && cd "$WORK"
curl -fsSL -o zig.tar.xz https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz
echo '70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00  zig.tar.xz' | sha256sum -c -
mkdir -p zig && tar xf zig.tar.xz -C zig --strip-components=1
cd -

experiments/60-fetch-sources.sh                              # ~250 MB
for a in x86_64 aarch64 riscv64 loongarch64 powerpc64le; do
    experiments/70-build-static-stack.sh "$a-linux-musl"
    experiments/80-build-pacman.sh       "$a-linux-musl"
done
experiments/85-feature-matrix.sh                             # what got linked in
experiments/95-cross-arch-bootstrap.sh                       # all five bootstrap
sudo experiments/90-bootstrap-arch.sh x86_64                 # chroot + signatures
```

Every script exits `0` on success, `1` when it ran and the thing failed, `2`
when it could not run. ⛔ `2` is never reported as a pass.

---

## What each file is

### Top level

| path | what it is |
| --- | --- |
| `README.md` | this file |
| [`RESEARCH.md`](RESEARCH.md) | the findings, the reasoning, and ⭐ **what this work got wrong about itself** |
| [`TASKS.md`](TASKS.md) | the remaining work, in dependency order, with acceptance commands |
| [`LICENSING.md`](LICENSING.md) | 0BSD, and the third-party scope notes |

### `experiments/` — every measured claim ships with the thing that measured it

| script | answers |
| --- | --- |
| `10-probe-source-hosts.sh` | which upstream source hosts answer from **your** network |
| `20-probe-arch-repos.sh` | is there a live Arch-family repository for each of the five |
| `30-reference-defects.sh` | do the reference PKGBUILDs cover the five targets — ⛔ exits **1**, the defects are real |
| `40-mine-repo-joiner-defect.sh` | does the prescribed mining script deliver the comments it claims — ⛔ exits **1** |
| `50-zig-cross-targets.sh` | can `zig cc` produce a **running** static musl binary for all five |
| `60-fetch-sources.sh` | fetch and hash the thirteen dependency sources |
| `70-build-static-stack.sh` | build the thirteen libraries for one target |
| `80-build-pacman.sh` | build pacman, assert static linkage, run it under emulation |
| `85-feature-matrix.sh` | which optional features are in each binary |
| `90-bootstrap-arch.sh` | install a working root, `chroot` it, verify signatures |
| `91-segfault-control.sh` | ⛔ **superseded** by `92-`; kept so revision 1's numbers stay traceable |
| `92-segfault-rate.sh` | crash **rate** over many trials, plus a gdb backtrace |
| `95-cross-arch-bootstrap.sh` | do the foreign-architecture binaries do real work |
| `out/` | what every run printed. **Tracked on purpose** — the evidence is the point |
| `fixtures/libc-surface.c` | the libc calls pacman actually makes (`getpwnam`, `getgrnam`, `getaddrinfo`), as a 53-line program |
| `fixtures/mussel-issue-comments-page1.json` | one captured API page, so `40-` re-runs offline |

### `patches/` — applied by the build scripts, `-p1`

| patch | why it exists |
| --- | --- |
| `brotli-1.2.0/0001-no-code-model-attribute-on-loongarch.patch` | brotli picks `__attribute__((model))` on a **compiler** test, not a target test. clang accepts the name everywhere and rejects the value on loongarch64. No-op on the other four, so it is applied unconditionally. |
| `pacman/0001-libalpm-invalidate-curl-data-in-child.patch` | libalpm's forked child touches libcurl state the parent owns. ⭐ Carried by the **Manjaro fork**, not by the canonical package. Upstream: curl issue 21466. |

### `scripts/`

| path | what it is |
| --- | --- |
| `mine-repo.sh` | vendored reference-mining script, **patched**: upstream silently writes an empty array for issue comments. See [`docs/patches/mine-repo-page-join.md`](docs/patches/mine-repo-page-join.md). Use this copy, not upstream's. |

### `docs/`

| path | what it is |
| --- | --- |
| [`FEATURES.md`](docs/FEATURES.md) | what is compiled in, what is off and why, what a static binary cannot contain |
| [`GOTCHAS.md`](docs/GOTCHAS.md) | every trap that cost time here, worst first, with a routing table |
| `patches/mine-repo-page-join.md` | the record for the one vendored change |

### `references/` — the corpus

The upstream trees at the commits every citation names, so no later session
has to re-fetch anything to check a claim. See
[`references/README.md`](references/README.md) for what each is and ⛔ what the
fetches could not get.

---

## The short version of the findings

- ⭐ **`zig cc` replaces five GCC cross toolchains.** `mussel` — the toolchain
  the task brief names — has **no `loongarch64` case at all**, and its
  toolchains are not relocatable.
- ⛔ **Do not copy the reference's architecture `case` block.** Its `riscv64`
  OpenSSL target is single-quoted, so `$CARCH` never expands. The fork that
  *declares* `riscv64` inherits it verbatim.
- ⚠ **The five distributions have five different repository layouts**, and
  `$repo/os/$arch` is correct on exactly one. ArchPOWER needs **two** repository
  sections because it splits arch-specific and `any` packages into separate
  databases.
- ⛔ **A leaked bind mount plus `rm -rf` destroyed this machine's `/dev`,
  twice**, and the resulting breakage looked like a pacman bug for hours.
  [G-01](docs/GOTCHAS.md#g-01--a-leaked-bind-mount-plus-rm--rf-destroys-your-host).

Reasoning, evidence and the corrections: [`RESEARCH.md`](RESEARCH.md).
