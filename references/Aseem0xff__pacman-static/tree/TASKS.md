# TASKS — building `pacman-static` for five architectures

The actionable list. Every task names **what to do**, **why it is not
obvious**, the **acceptance command**, and the **evidence** behind it.

⚠ **Read [`RESEARCH.md` §0](RESEARCH.md#0-what-this-sweep-got-wrong-about-itself)
first.** This sweep corrected four of its own claims mid-flight and one of
them reversed a conclusion. The list below inherits that error rate.

**Ordering is a dependency order, not a priority order.** T-01 through T-06
are the build; T-07 through T-12 make it trustworthy; T-13 onward make it
shippable. **T-08 is the one open blocker** and it is not last because it
matters least.

| | |
| --- | --- |
| ✅ **done and measured here** | the experiment that proves it is in `experiments/` |
| 🔧 **do this** | not done here, and the reason is stated |
| ⛔ **blocker** | the goal is not met until this is closed |

---

## Phase 1 — the build

### ✅ T-01 · Use `zig cc`, not five GCC cross toolchains

**Do:** cross-compile every target with one `zig cc` install. Do **not** build
mussel toolchains unless T-17 forces it.

**Why it is not obvious:** the task brief points at `mussel`, and mussel is a
good toolchain generator. It is also five GCC builds, and
`experiments/30-reference-defects.sh` shows **mussel has no `loongarch64`
case at all** — not a flag, not an option, an absent branch. Adding one means
carrying a GCC multilib patch (T-17). `zig cc` ships musl and LLVM for every
one of the five targets, is one 55 MB download, and needs no toolchain build
at all.

**Acceptance:**
```sh
experiments/50-zig-cross-targets.sh     # exit 0
```
Measured: all five targets build static, carry the right `e_machine`, have no
`PT_INTERP`, and **run** under `qemu-user` with `getpwnam`, `getgrnam` and
`getaddrinfo` all resolving.

**Evidence:** `experiments/out/50-zig-cross-targets.txt`

⚠ **The cost you are accepting:** the binary is built by clang/LLVM, not by
the GCC the reference uses. `RESEARCH.md` §7 lists what that changes.

---

### ✅ T-02 · Give every architecture its own copy of every source tree

**Do:** copy each unpacked source into a per-architecture build directory
before configuring. Never configure two targets in one tree.

**Why it is not obvious:** ⛔ **this failure is silent until the final link,
and it produces a per-architecture prefix that looks complete.** Building all
five in the shared source directories printed `whole stack built` for every
one and produced a full set of `.a` files in each per-arch prefix. It
surfaced only when pacman was linked for aarch64:

```
ld.lld: error: /home/user/work/out/x86_64/lib/libzstd.a(zstd_lazy.o)
        is incompatible with aarch64linux
```

The cause was in the **aarch64** prefix's own pkg-config files:

```
out/aarch64/lib/pkgconfig/libcrypto.pc:2:prefix=/home/user/work/out/x86_64
```

OpenSSL bakes the prefix into `configdata.pm` and the generated `.pc` files,
and a second `Configure` in a tree still holding the first build's artifacts
does not regenerate all of them. autotools packages carry the same hazard
through `config.status`: aarch64's `libcurl.pc` picked up a stray
`-L…/x86_64/lib` the same way.

**Acceptance:** `70-build-static-stack.sh` now asserts it — any absolute path
in a prefix's `.pc` files pointing at another architecture fails the run.
```sh
grep -o "$WORK/out/[a-z0-9_]*" "$WORK/out/$ARCH"/lib/pkgconfig/*.pc | sort -u
# must name only $ARCH
```

**Evidence:** `experiments/70-build-static-stack.sh`, the `srcdir()` comment.

---

### ✅ T-03 · curl cannot detect a Brotli-enabled static OpenSSL

**Do:** export `LIBS=-lbrotlienc` when configuring curl.

**Why it is not obvious:** curl reports the wrong library.
```
configure: error: --with-openssl was given but OpenSSL could not be detected
```
OpenSSL 3.6 built `enable-brotli` puts `c_brotli.o` in `libcrypto.a`, and that
object references the Brotli **encoder**. curl's OpenSSL probe assembles its
link line from `libbrotlidec` and `libbrotlicommon` only, so the conftest
fails on `undefined symbol: BrotliEncoderCreateInstance` — and configure
blames OpenSSL.

**The reference solves the same problem differently.** `aur/pacman-static`
carries `curl-8.19.0-brotli-static.patch`, which adds a `libbrotlienc` probe
to `configure.ac` and therefore needs `autoreconf -if` and autotools on the
build host. ⭐ Measured here: the `LIBS` export fixes the same conftest with
no patch and no autoreconf.

**Acceptance:** curl's configure summary reads
`SSL: enabled (OpenSSL)` and `brotli: enabled`.

**Evidence:** upstream curl issue 21466 is a *different* bug; the brotli one
is curl issue 17678, named in the reference PKGBUILD's own comment.

---

### ✅ T-04 · OpenSSL target names for all five architectures

**Do:** use this table. It is not derivable from the triple.

| arch | OpenSSL target | extra |
| --- | --- | --- |
| `x86_64` | `linux-x86_64` | `enable-ec_nistp_64_gcc_128` |
| `aarch64` | `linux-aarch64` | `no-afalgeng` |
| `riscv64` | `linux64-riscv64` | |
| `loongarch64` | `linux64-loongarch64` | |
| `powerpc64le` | `linux-ppc64le` | |

**Why it is not obvious:** three carry a `64` in the middle and two do not.
⛔ **Neither reference has a case for `loongarch64` or `powerpc64le`**, and
both have a `riscv64` case that cannot work — see T-05.

**Acceptance:** `experiments/30-reference-defects.sh` prints the table and
asserts the defect.

---

### ✅ T-05 · Do not copy the reference's architecture `case` block

**Do:** write your own. Treat the reference's as a source of ideas, not code.

**Why:** the `riscv64` branch in **both** PKGBUILDs is

```sh
openssltarget='linux64-$CARCH'     # single quotes
```

`$CARCH` never expands. OpenSSL's `Configure` receives the literal string and
fails. The Manjaro fork **declares** `riscv64` in `arch=()` and inherits this
verbatim: its `build()` is byte-identical to the canonical one
(`sha256 c9b3a946296d70c34ecef861d7d9e5852201b0bd4dc1f4453299b2930d909e12`
for both bodies).

⭐ **Declared is not built.** The task brief lists `riscv64` among the fork's
architectures; that is the fork's own claim, and it does not hold.

**Acceptance:**
```sh
experiments/30-reference-defects.sh    # exit 1 while the defect stands
```

---

### ✅ T-06 · Put `-static` in the meson cross file, not the environment

**Do:**
```ini
[built-in options]
c_link_args = ['-static', '-L<prefix>/lib']
```

**Why it is not obvious:** meson in cross mode reads `c_link_args` from the
cross file. `LDFLAGS` exported in the shell reaches the **build machine**
compiler used for helper programs instead. The result links dynamically while
every flag in your script looks right.

Also: pacman's meson puts its targets at the **build root** (`build/pacman`),
not under `build/src/pacman/`. An instrument that looks in the source-shaped
path finds nothing — and, if it is written carelessly, reports a pass. This
sweep did exactly that; see `RESEARCH.md` §0.

**Acceptance:**
```sh
readelf -l build/pacman | grep -c INTERP     # must be 0
```

---

## Phase 2 — make it trustworthy

### ⛔ 🔧 T-07 · Verify sources by signature, not by "it downloaded"

**Do:** carry the reference's `sha512sums` and `validpgpkeys`, verify both,
and fail closed.

**Why:** ⛔ **this sweep did not do it.** `experiments/60-fetch-sources.sh`
records the sha256 of what *arrived on this host on this day*. That is a
change detector. It is **not** provenance, and it must not be presented as
provenance.

What the reference does, and what to adopt:

| pin | how |
| --- | --- |
| pacman | signed annotated git tag `v7.1.0`, verified against two fingerprints |
| dependency tarballs | upstream `sha512sums` plus detached `.asc`/`.sig` |

⭐ **The signed tag holds.** Verified here: `git tag -v v7.1.0` reports a
signature by RSA key `6645B0A8C7005E78DB1D7864F99FFE0FEAE999BD` (Allan
McRae), which is the fingerprint the reference pins. And the patch-level
commit `54d94116164b0b2202c6061c4a59c6f3e70820d8` really is a descendant:
`git describe --tags --abbrev=0` answers `v7.1.0`.

⚠ **Write down which pin you chose and why.** A signed tag plus a pinned key
is defensible; a bare commit hash is stronger against tag movement. The
reference chose the tag *and* pins the patch-level commit, which is both.

**Acceptance:** a build that fails when a tarball's signature does not verify,
proven by feeding it a corrupted tarball.

---

### ⛔ 🔧 T-08 · An intermittent SIGSEGV at the end of `pacman -S base` — OPEN

**What happens:** on x86_64, into an empty root, from the real Arch mirror:
137 packages install, the root is 704 MB, every post-transaction hook runs —
and then

```
installing base...
error: segmentation fault
```

The root is **usable**: it `chroot`s, runs its own `bash`, and its own
dynamically linked pacman reports `Pacman v7.1.0 - libalpm v16.0.1`. Only the
exit status is wrong. ⛔ **That still blocks shipping** — no caller can tell it
from a real failure.

#### ⛔ Most of it was the harness, not pacman. Read this before spending an hour.

The large early cluster of these crashes was caused by **this repository's own
bootstrap script deleting the host's `/dev`**: it bind-mounted `/dev` into the
target root, its `umount` failed unchecked, and the next run's `rm -rf`
walked through the live mount. With `/dev/urandom` gone, OpenSSL and gpg fail
in ways that surface exactly like this.

[G-01](docs/GOTCHAS.md#g-01--a-leaked-bind-mount-plus-rm--rf-destroys-your-host)
has the account and the repair. `90-bootstrap-arch.sh` no longer bind-mounts
`/dev` and refuses to `rm -rf` a directory with anything mounted under it.

⚠ **If you see this crash, check `/dev` first**:
`head -c 4 /dev/urandom >/dev/null && echo ok`.

#### What actually remains

| observation | count |
| --- | --- |
| native trials, healthy `/dev` (`92-`, 10 + 20) | **0 / 30** |
| 8-cell factor matrix | **0 / 8** |
| native-cold vs qemu-cold, 5 each | **0 / 10** |
| the five-architecture run in `95-` | **2 crashes** |
| one `90-` run with verified-healthy `/dev` | **1 crash** |

≈ **3 in 50**, ⚠ **all three under load** — five ~800 MB bootstraps back to
back. That is the only surviving correlate and it is weak.

#### Ruled out, by measurement

| hypothesis | how it died |
| --- | --- |
| the missing `libalpm` curl-in-child patch | applied, rebuilt, crash identical |
| static libcurl/OpenSSL teardown in general | small transactions and `-Q`, `-Si`, `-Sy` all exit 0 |
| hooks are inherently fatal | a `--debug` run ran every hook and exited 0 |
| path-specific | same path exits 0 with `--debug`; other paths crash |
| sync and install in one process | 8-cell matrix, all 8 passed |
| running under `qemu-user` | 5 native-cold and 5 qemu-cold, all 0 |

#### Do next, in this order

1. **`experiments/92-segfault-rate.sh 100`, with concurrent load.** Load is the
   only correlate left. The script already enables core dumps and runs `gdb`
   on the first crash.
2. **Take the backtrace.** ⛔ Three guesses have been wrong. The next statement
   about the cause comes from a stack, not a hypothesis.
3. Rebuild with `-g -O0` if the optimised backtrace is unreadable.
4. Check whether an **Arch-built** `pacman-static` shows it on the same host.
   If it does, this is not a `zig cc` artefact and is not this project's.

⚠ **Do not script around it by ignoring pacman's exit code.** Check the
installed package count instead.

---

### ✅ T-09 · Per-architecture repository coordinates

**Do:** ship one `pacman.conf` per architecture. There is no single template.

| arch | distribution | repos | `Server =` | keyring |
| --- | --- | --- | --- | --- |
| `x86_64` | Arch Linux | `core`, `extra` | `https://geo.mirror.pkgbuild.com/$repo/os/$arch` | `archlinux-keyring` |
| `aarch64` | Arch Linux ARM | `core`, `extra` | `http://mirror.archlinuxarm.org/$arch/$repo` | `archlinuxarm-keyring` |
| `riscv64` | Arch Linux RISC-V | `core`, `extra` | `https://archriscv.felixc.at/repo/$repo` | `archlinux-keyring` |
| `loongarch64` | LoongArch Linux | `core`, `extra` | `https://mirrors.wsyu.edu.cn/loongarch/archlinux/$repo/os/loong64` | `archlinux-keyring` |
| `powerpc64le` | ArchPOWER | **`base`** + **`base-any`**, no `extra` | `https://repo.archlinuxpower.org/$repo/powerpc64le` **and** `…/base/any` | `archpower-keyring` |

⛔ **Three traps, each of which 404s silently if you assume uniformity:**
- LoongArch Linux's architecture directory is **`loong64`**, so `$arch` does
  not substitute.
- ArchPOWER's core-equivalent repository is named **`base`**, and its path has
  no `os/` component.
- Arch Linux RISC-V is **flat**: no `os/$arch` at all.
- ⭐ ArchPOWER needs **two** repository sections. It has no `extra`, and it
  splits arch-specific and `any` packages into separate databases
  (`base.db`, 3736 packages; `base-any.db`, 2200). `iana-etc` and `openssl`
  are only in the second, so `base` is unsatisfiable without it.

**Acceptance, two levels:**
```sh
experiments/20-probe-arch-repos.sh      # the repositories are live
experiments/95-cross-arch-bootstrap.sh  # each one actually installs `base`
```
Measured: all five answer 200 **and** parse as real pacman databases — 297,
311, 296, 293 and 3736 packages respectively. A mirror serving an HTML error
page with a 200 fails this check, which is why it is not a `HEAD` request.

⭐ **And all five then install `base` with that architecture's own binary** —
136, 135, 137, 140 and 137 packages, with `file(1)` on the resulting
`/usr/bin/bash` confirming each is that architecture's ELF.

---

### ✅ T-10 · The keyring chicken-and-egg (done for x86_64)

**Do:** document and test the two-pass bootstrap.

**Why:** `pacman-key` is a **shell script that drives `gpg`**. The static
binary has no `gpg` inside it and cannot make one. So the first pass into an
empty root cannot verify signatures — there is nothing to verify them with.

The sequence that works:

1. Pass 1, `SigLevel = Never`, install `base`. This is the pass that has no
   trust; treat its output as untrusted until step 3.
2. `chroot` into the new root, `pacman-key --init && pacman-key --populate <keyring>`.
3. Rewrite `/etc/pacman.conf` with `SigLevel = Required DatabaseOptional` and
   re-sync **inside** the root. From here on every package is verified.

⚠ **A bootstrap that stops at step 1 has proved the download worked and
nothing about trust.** `90-bootstrap-arch.sh` reports steps 1 and 3
separately for exactly that reason.

**Acceptance:** `experiments/90-bootstrap-arch.sh x86_64` reports step 5
separately from steps 1–4 and exits 0. Worked example:
[`examples/02-bootstrap-arch-rootfs.md`](examples/02-bootstrap-arch-rootfs.md).

**Still to do:** pre-seed the keyring from the host by verifying
`archlinux-keyring`'s own signature against a pinned fingerprint before step 1,
so pass 1 is verified too. And run the same two-pass flow on the other four
architectures, which needs T-13.

---

### ✅ T-11 · `--hookdir` must already exist

**Do:** create every redirected directory before the first invocation.

```sh
mkdir -p "$ROOT"/var/lib/pacman "$ROOT"/var/cache/pacman/pkg \
         "$ROOT"/etc/pacman.d/hooks "$ROOT"/etc/pacman.d/gnupg "$ROOT"/var/log
```

**Why it is not obvious:** pacman **creates** `--dbpath` and `--cachedir` and
**refuses** a missing `--hookdir`:
```
error: 'failed to resolve path '…/etc/pacman.d/hooks' passed to '--hookdir':
No such file or directory
```
The asymmetry is undocumented and costs a full run to find.

⚠ **And every path must be redirected.** `--root` alone is not enough: pacman
keeps its database, cache, hooks, keyring and log at compiled-in absolute
paths. Miss one and it writes onto the **host** — silent on a non-Arch host,
and corrupting on an Arch one.

---

### 🔧 T-12 · Make a test execute the examples

**Done:** four verified walkthroughs in [`examples/`](examples/), each with
its commands run on the machine that wrote them.

**Still to do:** a test that reads the commands **out of** the markdown and
executes them, so the documents cannot drift from the scripts. Today
`90-bootstrap-arch.sh` duplicates `examples/02`'s commands rather than
sourcing them, and nothing fails if the two diverge.

⚠ **A guide nothing executes rots at the first version bump.**

---

### 🔧 T-13 · `chroot` each architecture, not just x86_64

**Why not done here:** steps 3–5 of the bootstrap check `chroot` and run the
new root's own binaries. Under `qemu-user` that needs either `binfmt_misc`
registered on the host — which modifies host kernel state — or the emulator
copied inside the root and every exec routed through it.

**Do:** copy `qemu-<arch>-static` into `$ROOT/usr/bin/` and register binfmt in
the CI container, then run `90-bootstrap-arch.sh` per architecture.

**Already measured:** `95-cross-arch-bootstrap.sh` installs `base` for all
five under `qemu-user`, so resolution and unpacking are done. ⛔ **What is not
done is the `chroot`**: post-transaction hooks fail with `Exec format error`
because the host kernel has no `binfmt` handler for the target, so
`ldconfig`, `locale-gen` and `update-ca-trust` never run.

⚠ ArchPOWER needed a corrected config before `base` would resolve at all —
two repository sections, because it splits arch-specific and `any` packages
into separate databases. T-09.

---

### 🔧 T-14 · Test on real hardware, or say you did not

`qemu-user` emulates the ISA and passes syscalls to the **host** kernel. It
does not exercise the target's kernel, its **page size** (a real aarch64 or
ppc64le host may use 64 K pages, and a static binary's segment alignment is
where that bites), or its filesystem.

⛔ **Every claim in `RESEARCH.md` about a non-x86_64 architecture is a
qemu-user claim.** Say so wherever it is repeated.

---

### 🔧 T-15 · Release assets, checksummed and listed

One binary per architecture, `xz`-compressed, with a detached signature and a
checksum in the release body — the shape `build-packages.sh` in the reference
already uses.

---

### 🔧 T-16 · Reproducibility

`SOURCE_DATE_EPOCH`, `-ffile-prefix-map`, sorted archive members, and a
`--strip`ped output. Then build twice on different hosts and diff.

⚠ **Not attempted here.** The binary is 14.7 MB **unstripped, with
`debug_info`**; a shipped one should not be.

---

## Phase 4 — only if `zig cc` is rejected

### 🔧 T-17 · Add a `loongarch64` case to mussel

Mussel has cases for `aarch64` (line 259), `powerpc64le` (435), `riscv64`
(443) and `x86_64` (508), and **none** for `loongarch64`.

A new case needs `XARCH=loongarch64`, `LARCH=loongarch` (the kernel's own
directory name), `MARCH=loongarch64`, `XTARGET=loongarch64-linux-musl`, and
`XGCCARGS="--with-arch=loongarch64 --with-abi=lp64d"`.

⚠ **It also needs a `pure64` patch that does not exist yet.** Mussel applies
`patches/gcc/glaucus/0001-pure64-for-$XPURE64.patch` to rewrite GCC's
`MULTILIB_OSDIRNAMES` from `../lib64` to `../lib`, because musl installs to
`lib/`. There is a patch for riscv64, powerpc64, aarch64, x86-64, mips64 and
s390x — and none for loongarch. Write one against
`gcc/config/loongarch/t-linux`.

**Then re-run** `experiments/30-reference-defects.sh`; a `0` means the gap is
closed.

---

### 🔧 T-18 · Mussel toolchains are not relocatable

`--prefix` and `--with-sysroot` are baked in at configure time, so a toolchain
cannot be moved from where it was built. That is `firasuke/mussel` issue **29**,
closed with **no comment and no fix in the tree**.

⛔ **This is what makes mussel expensive in CI**, not the build time: a cached
toolchain artefact only works if every runner unpacks it at the identical
absolute path.

---

### 🔧 T-19 · Mussel's default mirrors

`ftpmirror.gnu.org` answered **502** from this network;
`libisl.sourceforge.io` answered **403**; `musl.libc.org` was intermittent
(one clean `GET`, one `curl` exit 35). Substitute `sourceware.org/pub/…` for
binutils and GCC.

This is `firasuke/mussel` issue **57**, still open, and the maintainer's own
answer is that the GNU redirector sends you to "faulty but nearby" mirrors and
that you should point the script at mirrors that work for you.

**Acceptance:** `experiments/10-probe-source-hosts.sh`

---

## Phase 5 — applies throughout, not at the end

⚠ These two are numbered last and belong first. Numbers are never reused, so a
task found late keeps the next free number.

### ⛔ ✅ T-20 · Never `rm -rf` a path that may have something mounted under it

**This is the most damaging trap in the whole project**, and it is in *your*
harness, not in pacman.

A bootstrap script bind-mounts the host's `/dev` into the target root for the
`chroot` checks. Its `umount` fails with `target is busy`. Nothing checks. The
**next** run's `rm -rf "$ROOT"` walks through the live mount and deletes the
**host's** device nodes.

⛔ **It happened twice here, and the resulting breakage was written up as an
open pacman bug** before the cause was found —
[`RESEARCH.md` §0](RESEARCH.md#0-what-this-sweep-got-wrong-about-itself).

**Do, in order of what it buys:**

1. ⭐ **Do not bind-mount `/dev`.** `mknod` the five nodes a scriptlet or gpg
   needs — `null`, `zero`, `random`, `urandom`, `tty` — inside the root.
   Nothing then points at the host.
2. **Guard the `rm`:**
   ```sh
   awk -v d="$ROOT" 'index($2, d) == 1 {print $2}' /proc/mounts | grep -q . && exit 2
   ```
3. **Verify every `umount` against `/proc/mounts`.** `umount -l` returns
   success while the mount is still listed, so its exit code decides nothing.

**Acceptance:** point `90-bootstrap-arch.sh` at a directory with something
mounted under it; it must exit **2** and delete nothing.

Both 1 and 2 are implemented. Full account and repair:
[G-01](docs/GOTCHAS.md#g-01--a-leaked-bind-mount-plus-rm--rf-destroys-your-host).

---

### ✅ T-21 · brotli needs a patch for loongarch64 under clang

**What you see:**
```
c/common/context.c:6:20: error: code model 'small' is not supported on this target
```

**Why:** brotli selects `__attribute__((model))` on a **compiler version**
test, not a target test. clang answers `__has_attribute(model)` affirmatively
on every target and then rejects the value where the target has no code
models. GCC's loongarch backend accepts it, so this is clang-only — and it is
the one place in the whole stack where `zig cc` needed a source change that
GCC would not have.

**The fix:**
[`patches/brotli-1.2.0/0001-no-code-model-attribute-on-loongarch.patch`](patches/brotli-1.2.0/).
Guarded by `defined(__loongarch__) && defined(__clang__)`, so it is a no-op on
the other four targets and is applied unconditionally by
`70-build-static-stack.sh`.

⚠ **Check at the next brotli release whether it still applies.** If upstream
changes the test to a target test, delete the patch.

---

## What this list does not cover

- **i686 / arm / armv7.** The reference supports them; nothing here was built
  or tested for them.
- **`nettle` instead of `openssl`.** pacman's meson offers it; not tried.
- **Landlock.** pacman 7.1 probes `linux/landlock.h` as well as seccomp.
  Untested on every architecture here.
- **`makepkg` and the other scripts.** Only the `pacman` binary was built and
  run. The reference ships `pacman-conf`, `vercmp`, `testpkg` and the shell
  scripts too.
- **i18n.** Built with `-Di18n=false` here. The reference leaves it on.
