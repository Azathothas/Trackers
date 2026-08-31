# Example 01 — build `pacman-static` from source, for any of the five architectures

**What this produces:** a statically linked `pacman` binary for a target of
your choice, built from upstream source, on a host that does not run that
architecture and has no musl toolchain installed.

**Verified on:** Ubuntu 24.04.4, `x86_64`, 4 cores, 2026-08-28. All five
targets.

**Time:** about 5 minutes per architecture on 4 cores, plus a one-off
~250 MB source download.

---

## What the host needs

Nothing exotic, and **no cross compiler**.

| tool | why | Debian/Ubuntu | Alpine |
| --- | --- | --- | --- |
| `curl` | fetch sources | `curl` | `curl` |
| `git` | fetch zstd and pacman | `git` | `git` |
| `tar`, `xz`, `bzip2`, `gzip` | unpack | `tar xz-utils bzip2 gzip` | `tar xz bzip2` |
| `make`, `patch` | build | `make patch` | `make patch` |
| `cmake` ≥ 3.16 | xz and brotli | `cmake` | `cmake` |
| `meson` ≥ 0.61, `ninja` | pacman | `meson ninja-build` | `meson samurai` |
| `pkg-config` | dependency resolution | `pkg-config` | `pkgconf` |
| `perl` | OpenSSL's Configure | `perl` | `perl` |
| `python3` | meson | `python3` | `python3` |
| `readelf` | the static-linkage check | `binutils` | `binutils` |
| `qemu-user-static` | *optional*, to run a foreign binary | `qemu-user-static` | `qemu-<arch>` |

⚠ **No `gcc` is needed** — `zig cc` is the compiler. A host compiler is only
used if your `cmake`/`meson` probe for one; the builds here do not.

```sh
# Debian / Ubuntu
apt-get install -y --no-install-recommends \
    curl git tar xz-utils bzip2 gzip make patch cmake ninja-build \
    pkg-config perl python3 python3-pip binutils qemu-user-static
pip3 install meson
```

---

## Step 1 — get zig

One 55 MB download. This is the entire toolchain.

```sh
export WORK=${WORK:-$HOME/pacman-static-work}
mkdir -p "$WORK"
cd "$WORK"

curl -fsSL -o zig.tar.xz \
  https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz

echo '70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00  zig.tar.xz' \
  | sha256sum -c -

mkdir -p zig && tar xf zig.tar.xz -C zig --strip-components=1
"$WORK/zig/zig" version        # 0.16.0
```

⚠ If you are on a host that is not `x86_64`, take the matching tarball from
`https://ziglang.org/download/` and its published checksum.

---

## Step 2 — fetch the sources

```sh
cd /path/to/this/repo
WORK="$WORK" experiments/60-fetch-sources.sh
```

Thirteen tarballs plus a pinned `zstd` tag. It prints what it fetched, the
byte count and the sha256 of each.

⛔ **Those sha256 lines record what arrived, not upstream provenance.** For a
build you intend to trust, verify the upstream `sha512sums` and PGP
signatures — `TASKS.md` T-07.

If a host does not answer, find out which before guessing:

```sh
experiments/10-probe-source-hosts.sh
```

---

## Step 3 — build the dependency stack

Pick one target triple:

```
x86_64-linux-musl   aarch64-linux-musl   riscv64-linux-musl
loongarch64-linux-musl   powerpc64le-linux-musl
```

```sh
WORK="$WORK" experiments/70-build-static-stack.sh loongarch64-linux-musl
```

Thirteen libraries, in dependency order. Expected output — ⚠ the times are
wall clock on a shared 4-core box and drift by a few seconds a package between
runs; only the `ok` column and the leak check matter:

```
PACKAGE        STATUS   TIME
zlib           ok         1s
xz             ok        13s
bzip2          ok         0s
zstd           ok         5s
brotli         ok         7s
openssl        ok        54s
libarchive     ok        22s
nghttp2        ok        12s
curl           ok        34s
libgpg-error   ok        10s
libassuan      ok         8s
gpgme          ok        12s
libseccomp     ok         5s

cross-prefix leak check:
  none: every absolute path in this prefix points at …/out/loongarch64

verdict: whole stack built
```

⭐ **The leak check is not decoration.** It is the assertion for G-02, the
failure that produces a per-architecture prefix that looks complete and links
another architecture's objects. A non-zero exit here means stop.

---

## Step 4 — build pacman

```sh
WORK="$WORK" experiments/80-build-pacman.sh loongarch64-linux-musl
```

```
meson setup    ok
meson compile  ok
build time     5s
linkage        static
size (bytes)   13512296
emulator       qemu-loongarch64-static
pacman -V      ok

reported       Pacman v7.1.0 - libalpm v16.0.1
```

The binary is at `$WORK/pacman/<arch>/build/pacman`.

⭐ **Three separate checks, and only the third means anything:** meson
compiled it; the ELF has no `PT_INTERP`; and the binary **ran** under its
architecture's emulator and printed its version.

---

## Step 5 — check it yourself

Do not take the script's word for it.

```sh
BIN=$WORK/pacman/loongarch64/build/pacman

file "$BIN"
# ELF 64-bit LSB executable, LoongArch, version 1 (SYSV), statically linked

readelf -l "$BIN" | grep -c INTERP
# 0   ← no dynamic loader; this is what "static" means

qemu-loongarch64-static "$BIN" --version
#  .--.                  Pacman v7.1.0 - libalpm v16.0.1
```

⚠ `qemu-user` emulates the ISA and passes syscalls to the **host** kernel. It
does not exercise the target's kernel or its page size. A binary that passes
here is not yet a binary proven on real hardware — `TASKS.md` T-14.

---

## Build all five

```sh
for t in x86_64 aarch64 riscv64 loongarch64 powerpc64le; do
    experiments/70-build-static-stack.sh "$t-linux-musl" || break
    experiments/80-build-pacman.sh       "$t-linux-musl" || break
done
experiments/85-feature-matrix.sh
```

Measured sizes, unstripped with `debug_info`:

| arch | bytes |
| --- | --- |
| `x86_64` | 14 708 504 |
| `aarch64` | 14 321 560 |
| `riscv64` | 18 059 496 |
| `loongarch64` | 13 512 296 |
| `powerpc64le` | 15 419 992 |

⚠ Strip before shipping — `TASKS.md` T-16.

---

## If it fails

| symptom | cause | read |
| --- | --- | --- |
| `--with-openssl was given but OpenSSL could not be detected` | Brotli encoder, not OpenSSL | [G-04](../docs/GOTCHAS.md#g-04--curl-says-openssl-is-missing-when-brotli-is-the-problem) |
| `libzstd.a … is incompatible with <arch>` | a source tree reused across architectures | [G-02](../docs/GOTCHAS.md#g-02--reusing-one-source-tree-across-architectures) |
| `code model 'small' is not supported` | brotli on loongarch64 with clang | [G-12](../docs/GOTCHAS.md#g-12--brotli-does-not-build-for-loongarch64-with-clang) |
| `linkage dynamic` | `-static` in the environment instead of the cross file | [G-06](../docs/GOTCHAS.md#g-06---static-in-ldflags-does-nothing-under-a-meson-cross-build) |
| a source host times out | not your build | [G-13](../docs/GOTCHAS.md#g-13--two-of-the-references-source-hosts-do-not-answer) |

---

**Next:** [Example 02](02-bootstrap-arch-rootfs.md) — use this binary to build
an Arch root from nothing.
