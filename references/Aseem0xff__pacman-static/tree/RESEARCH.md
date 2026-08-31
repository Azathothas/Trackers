# pacman-static — a static, five-architecture pacman

A sweep of the two named references plus the one they both descend from, and a
working build taken far enough to say which parts are measured and which are
still guesses.

**Question:** build `pacman`, statically linked, with as many features as can
be linked statically, for `x86_64`, `aarch64`, `riscv64`, `loongarch64` and
`ppc64le`, and prove it can bootstrap an Arch root.

**Answer:** yes, for all five, and the toolchain the brief points at is not
the way to get there.

---

## 0. What this sweep got wrong about itself

⚠ **Read this before the recommendations.** A reader who reaches §1 first has
already stopped reading.

**Nine claims were corrected. One of them was a root cause I had already
written down, one was destroying the machine, and one was found only by
cloning what had already been pushed.**

| # | what I claimed | what measurement said |
| --- | --- | --- |
| 1 | An `error: segmentation fault` ending `pacman -S base` was a pacman bug, fixed by the patch the Manjaro fork carries. | ⛔ **Wrong twice over.** The patch changed nothing. The cause was **this repository's own bootstrap script deleting the host's `/dev`** — §9. |
| 2 | `powerpc64le` has no `qemu-user` emulator here, so it cannot be run-tested. | ⛔ **Wrong.** `qemu-ppc64le-static` was installed the whole time. My script derived the emulator name from the triple prefix (`powerpc64le`); Debian names it `ppc64le`. **An absence was reported where an emulator existed.** |
| 3 | `musl.libc.org` is unreachable from this network. | ⚠ **Wrong as stated.** A `HEAD` fails and a `GET` succeeds. It is *intermittent* — a different fact with a different remedy. |
| 4 | `80-build-pacman.sh` reported *"pacman built, linked static, and ran"*. | ⛔ **It had measured none of those.** meson puts the binary at `build/pacman`; the script looked in `build/src/pacman/pacman`, found nothing, skipped all three checks, and never set a failure code. |
| 5 | Copying each source tree per architecture fixes the cross-contamination. | ⛔ **Insufficient.** `$SRC/openssl-3.6.4` had already been configured once, so every copy inherited a poisoned `configdata.pm`. Only extracting from the tarball is pristine. |
| 6 | ArchPOWER's repositories are `base` and `extra`. | ⛔ **Wrong.** It has **no `extra`**, and it splits arch-specific from `any` packages into **two databases**. `base` is unsatisfiable without both. |
| 7 | The crash is path-specific / needs sync and install in one process. | ⛔ **Both wrong.** Small samples of an intermittent fault. Each was disproved by the next control. |
| 8 | `30-reference-defects.sh` printed each reference's commit. | ⛔ **It printed *this* repository's.** Once the corpus trees lost their `.git` directories, `git -C <corpus> rev-parse HEAD` did not fail — it walked **up** and answered with the enclosing repository's HEAD. A provenance line that is confidently wrong is worse than a missing one. It now reads the commit out of `PROVENANCE.md`. |
| 9 | The repository was committed and pushed, and it reproduced. | ⛔ **A fresh clone failed at `meson setup`.** `git add -A` honours a `.gitignore` at any depth: pacman's own `build-aux/.gitignore` excluded a file its build generates but this tree needs as source. **The tested tree and the committed tree were different**, and only cloning what had been pushed could tell. [G-17](docs/GOTCHAS.md#g-17--a-vendored-trees-own-gitignore-silently-drops-files-from-your-commit). |

⭐ **Claims 2, 4 and 8 are the same defect in three costumes: a tool answering
confidently where it should have failed.** All three were mine, in instruments
written to prevent exactly that.

⚠ **Claims 8 and 9 were found by the review pass and by cloning what had been
pushed — not by any run of the work itself.** Claim 9 in particular could only
be found that way: everything passed on the machine that wrote it. ⭐ **Clone
your own output before believing it reproduces.**

⭐ **Claim 1 is the expensive one.** A whole round of hypothesis, a control
matrix, and a written-up "open pacman bug" went into a fault this repository's
own script caused. [G-01](docs/GOTCHAS.md#g-01--a-leaked-bind-mount-plus-rm--rf-destroys-your-host).

**Assume more remain.**

### What was never tested at all

- **Real hardware, on any architecture.** Every non-`x86_64` claim here is a
  `qemu-user` claim. qemu emulates the ISA and passes syscalls to the host
  kernel; it does not exercise the target's kernel or its **page size**.
- **`chroot` into a foreign-architecture root.** Post-transaction hooks fail
  under `qemu-user` with `Exec format error`; fixing it needs `binfmt_misc`
  registration, which changes host kernel state. T-13.
- **Signature verification of any source tarball.** Sources were fetched over
  TLS and sha256'd *on arrival*. That is a change detector, not provenance. T-07.
- **`i686`, `arm`, `armv7h`** — supported by the reference, untouched here.
- **Reproducibility.** Built once, never diffed against a second build.
- **The shipped shape.** Every binary measured here is unstripped with
  `debug_info`.

### Sources this sweep could not reach

Each could change a conclusion: GitHub **discussions** on both references
(GraphQL only; the credential-free route is REST); the Manjaro fork's **real
tracker**, which is on GitLab, not the GitHub mirror that was mined; the **AUR
comment thread**; and **pacman's own GitLab issues**.

---

## 1. Bottom line

**The plan the brief implies — five `mussel` toolchains — is the expensive
path, and it does not reach `loongarch64` at all.**

One `zig cc` install cross-compiles the whole thing.

| | mussel | `zig cc` |
| --- | --- | --- |
| covers all five targets | ⛔ **no** — no `loongarch64` case | ✅ yes |
| toolchain build | five GCC builds | **none** |
| download | binutils, GCC, gmp, mpc, mpfr, musl, kernel headers | one 55 MB tarball |
| relocatable | ⛔ no (issue 29, closed unanswered) | ✅ yes |
| default mirrors reachable here | ⚠ two of five failed | ✅ |

### What was built and run

- The **13-package dependency stack** — zlib, xz, bzip2, zstd, brotli,
  OpenSSL, libarchive, nghttp2, curl, libgpg-error, libassuan, gpgme,
  libseccomp — for **all five** targets. About **three and a half minutes each**
  on 4 cores.
- **pacman v7.1.0**, statically linked, no `PT_INTERP`, running under each
  architecture's emulator, reporting `libalpm v16.0.1`. Identical feature set
  on all five.
- ⭐ **A real Arch bootstrap on x86_64.** On Ubuntu 24.04 with no pacman
  installed: sync from `geo.mirror.pkgbuild.com`, install `base` — **137
  packages, 704 MB** — `chroot` in, run the root's own `bash` and its own
  `pacman`, then `pacman-key --init && --populate` and a **signature-verified**
  re-sync. All five steps pass.
- ⭐ **Real bootstraps for all five architectures**, each against its own
  distribution: Arch Linux ARM (136 packages), Arch Linux RISC-V (135),
  LoongArch Linux (137), ArchPOWER (140), Arch Linux (137). The installed
  `/usr/bin/bash` was read back with `file(1)` and is that architecture's ELF.
- **A system with its dynamic loader deleted, repaired.** `chroot` failed for
  both `bash` and `pacman`; the static binary reinstalled `glibc` from outside
  and the root came back.

### And one thing not closed

A low-rate intermittent `SIGSEGV` at the very end of `pacman -S base`, **after
the install has completed**. Roughly 3 occurrences in ~50 runs since the `/dev`
damage was repaired, never reproduced in a controlled series. §9.

### The honest trade

`zig cc` is clang 21 and LLD, not the GCC the reference uses. Nothing in the
stack needed a GCC-specific extension and OpenSSL's assembly built clean for
all five — but the reference's binary and this one are not the same artefact,
and the reference has years of production behind it.

⭐ **If you need bit-for-bit what Arch ships, use their toolchain. If you need a
binary for `loongarch64` or `ppc64le`, they do not have one and this is how you
get one.**

---

## 2. How to read this

| you have | read |
| --- | --- |
| **two minutes** | §0, then §1 |
| **ten minutes** | §0, §1, §9, then [`TASKS.md`](TASKS.md) |
| **the implementation to do** | [`TASKS.md`](TASKS.md) in order, [`docs/GOTCHAS.md`](docs/GOTCHAS.md) open beside it |
| **to actually build something** | [`examples/`](examples/) — four verified walkthroughs |
| **a reason to distrust me** | §0, then run anything in [`experiments/`](experiments/) |

---

## 3. Test environment

One machine, one day.

| | |
| --- | --- |
| host | Linux 6.18.44, `x86_64`, 4 cores, 15 GiB RAM |
| distribution | Ubuntu 24.04.4 LTS — ⭐ **not Arch: no pacman, no libalpm, no Arch keyring** |
| date | 2026-08-28 |
| compiler under test | zig 0.16.0 (clang 21.1.0), `sha256 70e49664a743…3d00` |
| meson / ninja | 1.12.0 / 1.11.1 |
| emulator | qemu-user-static 8.2.2 |
| network | egress through a filtering proxy; §8 records what it blocks |

⚠ **The host not being Arch is load-bearing.** A `pacman-static` that only
works where pacman already exists has proved nothing.

---

## 4. The reference landscape

⭐ **The reference the brief names is a fork of one it does not.**

| reference | commit | verdict |
| --- | --- | --- |
| [`aur/pacman-static`](references/aur__pacman-static) | `8c58e7db1c52286bba77fd644ae1d77cc5db9e97`, 2026-08-27 | ⭐ **adopt** — the canonical recipe |
| [`manjaro-contrib/packages-core-pacman-static`](references/manjaro-contrib__packages-core-pacman-static) | `aad8fa5b24a94aa36f01b42eeae5a426b314a2c9` | **adopt one patch**; anti-pattern exhibit otherwise |
| [`firasuke/mussel`](references/firasuke__mussel) | `341735f6f65a0e8d482710760c43fc7590719fd7`, 2026-08-27 | **refused** for this project — §7.1 |

### 4.1 The brief's recorded HEAD has moved

The brief cites the Manjaro repository at `8c7a7c2262d5d51ee4d7301d403133a9c932c2f6`,
mined 2026-08-26. It is at `aad8fa5b…` today. ⚠ **Re-mine rather than trusting a
recorded HEAD.**

The whole diff between fork and canonical is **65 lines**: the maintainer
header; `pkgrel` 15 against 16 (⭐ the canonical package is **ahead**);
`riscv64` added to `arch()`; one extra patch; and an older OpenSSL
`validpgpkeys` list.

`build()` is **byte-identical**:
`sha256 c9b3a946296d70c34ecef861d7d9e5852201b0bd4dc1f4453299b2930d909e12`.

### 4.2 ⛔ Its `riscv64` support does not hold

Both PKGBUILDs contain, at `aur:256` / `manjaro:251`:

```sh
openssltarget='linux64-$CARCH'      # single quotes
```

`$CARCH` never expands. OpenSSL's `Configure` gets a literal and fails. In the
canonical package this is harmless dead code — `riscv64` is not in its
`arch()`. In the fork it is the one architecture the fork exists to add.

⭐ **Declared is not built.** Nothing in the fork's CI would catch it:
`.gitlab-ci.yml` has one runner tag, `aarch64`.

Asserted by [`experiments/30-reference-defects.sh`](experiments/30-reference-defects.sh),
which exits 1 while it stands.

### 4.3 ⭐ But the fork carries a fix the canonical package does not

`0001-libalpm-invalidate-curl-data-in-child.patch`, by Christian Hesse,
against `lib/libalpm/util.c`:

```c
#ifdef HAVE_LIBCURL
    /* invalidate the curl data - we must not touch it in child */
    handle->curlm = NULL;
#endif
```

in the forked child of `_alpm_run_chroot`, citing curl issue 21466. It applies
cleanly to the pinned pacman commit and is **not** in `aur/pacman-static`.

⚠ **This reverses the obvious reading.** The canonical package is better
maintained in every other respect; on this one line the fork is ahead. A
session that took "canonical is strictly better" as a rule would drop it.

⛔ **It does not fix the crash in §9** — applied, built, crash unchanged. Adopt
it because it is a real fix for a real child-process bug, not for that.

### 4.4 What the canonical recipe gets right

Read at `references/aur__pacman-static/tree/PKGBUILD`, commit `8c58e7db`:

| | |
| --- | --- |
| **build order** | compression libraries **before** OpenSSL, because OpenSSL 3.6 links brotli, zlib and zstd for certificate compression. Get this wrong and OpenSSL silently loses features. |
| **`-D_LARGEFILE64_SOURCE`** | turns on musl's `func64` interface; libarchive and curl probe for the `*64` symbols and quietly lose large-file support without it |
| **`install-<specific-target>`** | never a bare `make install`; only `install-libLTLIBRARIES`, `install-pkgconfigDATA` and friends, so nothing drags in docs or `.la` files |
| **GCC 16 workaround** | `-fno-link-libatomic`, because GCC 16 added `-latomic_asneeded` and `musl-gcc`'s search path does not cover it |
| **`rm …/lib*.la`** | libtool archives poison a static link with paths that do not exist on the target |
| **`PKGEXT=.pkg.tar.xz`** | ⭐ deliberate: one use of this package is recovering a system whose libarchive cannot read zstd |
| **the pin** | signed annotated tag **plus** a patch-level commit, with `git describe` asserting descent |

⭐ **The pin verifies.** `git tag -v v7.1.0` reports a signature by
`6645B0A8C7005E78DB1D7864F99FFE0FEAE999BD` — Allan McRae, the fingerprint the
brief quotes — and `git describe --tags --abbrev=0 54d94116…` answers `v7.1.0`.

---

## 5. What the trackers said

⚠ **This section nearly did not exist.** The prescribed mining script wrote an
**empty array** for issue comments while printing `comments: ok`. §10 has the
defect; the four findings below were invisible until it was fixed.

| issue | state | what it tells you |
| --- | --- | --- |
| `mussel#29` — toolchains can't be moved | closed, ⛔ **no comment, no fix** | `--prefix` and `--with-sysroot` are baked in at configure time. A cached toolchain artefact only works if every runner unpacks it at the identical absolute path. ⛔ This, not build time, is what makes mussel expensive in CI. |
| `mussel#57` — downloads not robust | **open** | The maintainer's own answer: `ftpmirror.gnu.org` redirects to "faulty but nearby" mirrors, and users should substitute mirrors that work for them. Independently measured here as a **502** — §8. |
| `mussel#2` — "patches explicitly produce a broken libc" | closed | Filed by **Rich Felker**, musl's author, against patches that removed configure checks: *"seriously wrong results, especially for the `strtod` and `printf` families"*. They were removed; the tree now carries only two CVE patches. ⭐ Check what a toolchain generator patches into libc before trusting its output. |
| `mussel#54` — fails on Alpine x86_64 | closed | Kernel-header install failed on Alpine 3.23; fixed in `7f16e60`. ⚠ The brief asks for a build "ideally on alpine/musl"; the tracker says that combination has needed fixing recently. |

⛔ **The Manjaro GitHub mirror has zero issues and zero pull requests.** That is
not evidence nothing was reported — it is a mirror, and its own
`.gitlab-ci.yml` points at GitLab, where the real tracker is. **Not fetched.**

---

## 6. Measured results

### 6.1 The compiler — [`50-`](experiments/out/50-zig-cross-targets.txt)

```
TARGET                     BUILD    E_MACHINE  LINKAGE   BYTES    RUN
x86_64-linux-musl          ok       x86-64     static    52240    ALL_CALLS_RESOLVED
aarch64-linux-musl         ok       aarch64    static    59824    ALL_CALLS_RESOLVED
riscv64-linux-musl         ok       riscv64    static    51064    ALL_CALLS_RESOLVED
loongarch64-linux-musl     ok       loongarch  static    66816    ALL_CALLS_RESOLVED
powerpc64le-linux-musl     ok       ppc64      static    68872    ALL_CALLS_RESOLVED
```

⭐ **The fixture is not "hello world".** It calls `getpwnam`, `getgrnam` and
`getaddrinfo` — §7.2 says why those three.

⚠ **`LINKAGE` is read from the program headers, not the compiler flag.**
`-static` was requested; the absence of `PT_INTERP` proves it landed.

### 6.2 The dependency stack — [`70-`](experiments/out/)

`x86_64`, 4 cores, from
[`experiments/out/70-build-static-stack.x86_64.txt`](experiments/out/70-build-static-stack.x86_64.txt):

zlib 1 s, xz 13 s, bzip2 0 s, zstd 4 s, brotli 7 s, openssl 53 s, libarchive 29 s, nghttp2 13 s, curl 39 s, libgpg-error 11 s, libassuan 8 s, gpgme 12 s, libseccomp 6 s.

**196 s total.** Sixteen `.a` files and seventeen `.pc` files.

Totals for the other four, same host: aarch64 207 s, riscv64 209 s,
loongarch64 199 s, powerpc64le 204 s.

⚠ **These drift by a few seconds a package between runs** — they are wall
clock on a shared 4-core box, not a benchmark. Read them as "about three and
a half minutes per architecture", and re-run the script rather than quoting
this table.

All five architectures pass, including the cross-prefix leak assertion.

### 6.3 pacman, and what is in it — [`85-`](experiments/out/85-feature-matrix.txt)

```
ARCH           libarchive libcurl  gpgme  libcrypto libseccomp landlock
x86_64         3.8.9      8.21.0   2.1.2  3.6.4     2.6.0      yes
aarch64        3.8.9      8.21.0   2.1.2  3.6.4     2.6.0      yes
riscv64        3.8.9      8.21.0   2.1.2  3.6.4     2.6.0      yes
loongarch64    3.8.9      8.21.0   2.1.2  3.6.4     2.6.0      yes
powerpc64le    3.8.9      8.21.0   2.1.2  3.6.4     2.6.0      yes
```

Every one static, every one running under its emulator, every one reporting
`libalpm v16.0.1`. ⭐ **Nothing in pacman's feature set had to be dropped to
link statically.** [`docs/FEATURES.md`](docs/FEATURES.md).

### 6.4 ⭐ The x86_64 bootstrap — [`90-`](experiments/out/90-bootstrap-arch.x86_64.txt)

| step | result |
| --- | --- |
| 1. `pacman -Sy` — reach a real mirror over TLS, parse the databases | **ok** |
| 2. `pacman -S base` — resolve and unpack into an empty directory | **137 packages, 704 MB** |
| 3. `chroot` runs the new root's own `bash` | **ok** |
| 4. the root's own dynamically linked pacman runs | **`Pacman v7.1.0 - libalpm v16.0.1`** |
| 5. `pacman-key --init && --populate`, then a **signature-verified** sync | **ok** |

Step 3 is what "a working root" means: it executes the new root's own glibc
binaries with the new root's own loader. Step 5 is the one that is easy to
fake — everything before it ran with `SigLevel = Never`.

### 6.5 ⭐ All five architectures bootstrap — [`95-`](experiments/out/95-cross-arch-bootstrap.txt)

| arch | distribution | sync | install | packages | `file` on `/usr/bin/bash` |
| --- | --- | --- | --- | --- | --- |
| `aarch64` | Arch Linux ARM | ok | ok | 136, 790 M | ARM aarch64 ✅ |
| `riscv64` | Arch Linux RISC-V | ok | ok | 135, 836 M | UCB RISC-V ✅ |
| `loongarch64` | LoongArch Linux | ok | **SEGV\*** | 137, 841 M | LoongArch ✅ |
| `powerpc64le` | ArchPOWER | ok | ok | 140, 835 M | 64-bit PowerPC ✅ |
| `x86_64` | Arch Linux | ok | ok | 137, 704 M | x86-64 ✅ |

⚠ **`SEGV*` means the install completed and the process then died** — the
package count and the ELF check both pass. It is the open fault in §9, and the
instrument counts it separately rather than calling it either a pass or a
failed bootstrap. In this run it landed on `loongarch64`; in the previous run
on `x86_64` and `powerpc64le`. ⭐ **Which architecture it hits varies between
runs, which is itself evidence that it is not architecture-specific.**

⭐ **This is the evidence upgrade that matters.** Before it, every non-`x86_64`
claim rested on `pacman --version` under qemu — which proves the binary starts
and exercises none of the software. This exercises libcurl, TLS, libarchive,
the database parser and the installer, and reads the resulting ELF back.

⛔ **Post-transaction hooks fail** under `qemu-user` with `Exec format error`:
a hook `exec`s a target-architecture binary inside the chroot and the host
kernel has no `binfmt` handler. The packages are installed and the root is
complete. T-13.

⚠ **Earlier runs of this table were not clean, for two reasons that are not
the binary's.** `loongarch64`'s mirror timed out once, and `powerpc64le`
needed the corrected two-database config (§0 claim 6). Re-run it; the network
is a variable.

---

## 7. Why this shape

### 7.1 Why not mussel

Not a judgement about mussel, which does its job. Three measured facts:

1. ⛔ **No `loongarch64` case.** Not a flag — an absent branch. Cases exist at
   lines 259 (`aarch64`), 435 (`powerpc64le`), 443 (`riscv64`), 508 (`x86_64`).
2. **Adding one is not a one-liner.** mussel applies
   `patches/gcc/glaucus/0001-pure64-for-$XPURE64.patch` to rewrite GCC's
   `MULTILIB_OSDIRNAMES` from `../lib64` to `../lib`, because musl installs to
   `lib/`. Patches exist for riscv64, powerpc64, aarch64, x86-64, mips64 and
   s390x — ⚠ **none for loongarch.** T-17.
3. **Not relocatable** — `mussel#29`.

### 7.2 Why musl, not static glibc

pacman calls `getpwnam()` in **four** places, to resolve `DownloadUser`:

```
lib/libalpm/dload.c:1165    lib/libalpm/handle.c:615
lib/libalpm/sandbox.c:57    lib/libalpm/util.c:967
```

(at pacman `54d94116`.)

`getpwnam` and `getaddrinfo` are the two calls that make a *static glibc*
binary need NSS shared objects at run time — the one thing a bootstrap binary
cannot assume exist. musl resolves both inside libc with no plugin mechanism.

⭐ **That is why the reference builds against musl**, and why the fixture in
§6.1 probes exactly those calls rather than `printf`.

### 7.3 What is given up

`libseccomp` is `required: false` in pacman's meson, so a target without it
loses the download sandbox and nothing else. ⭐ Not an issue for any of the
five: libseccomp 2.6.0 carries backends for `aarch64`, `loongarch64`, `ppc64`,
`riscv64` and `x86_64` (`src/arch-*.c` at `c7c0caed`).

What a static binary cannot contain — `pacman-key`, `makepkg`, the scriptlet
shell — is in [`docs/FEATURES.md`](docs/FEATURES.md).

---

## 8. The network, and why it is in a research document

Two of the reference's own source hosts do not answer from here, and a build
plan that assumes them fails on its first run.

| host | result | who needs it |
| --- | --- | --- |
| `ftpmirror.gnu.org` | **502** | mussel's binutils, GCC, gmp, mpc, mpfr |
| `libisl.sourceforge.io` | **403** | mussel, only with `--enable-isl` |
| `musl.libc.org` | ⚠ **intermittent** | mussel |
| `zlib.net` | **404** for the pinned version | zlib |
| `sourceware.org/pub/{gcc,binutils}` | 206 | the working substitute |
| everything the recommended plan needs | 200/206 | — |

⚠ **`HEAD` is not a control for `GET`.** `musl.libc.org` answers `000` to a
`HEAD` and serves the tarball on a `GET`. This document said it was dead until
the probe was fixed (§0, claim 3).

[`experiments/10-probe-source-hosts.sh`](experiments/10-probe-source-hosts.sh)

---

## 9. The crash that is not closed

```
installing base...
error: segmentation fault
```

**After** the install completes. 137 packages are in the local database, the
root is ~700 MB, it `chroot`s, and its own pacman runs. Only the exit status is
wrong — which is still a blocker, because no caller can tell it from a real
failure.

### ⛔ What it mostly was, and it was mine

The large early cluster is **explained**. This repository's bootstrap script
bind-mounted the host's `/dev` into the target root for the `chroot` checks,
its `umount` failed with `target is busy`, nothing checked, and the **next**
run's `rm -rf "$ROOT"` walked through the live mount and deleted the host's
device nodes. `/dev/urandom` was gone; OpenSSL and gpg then failed in ways that
surfaced as this segfault.

⭐ **It happened twice**, the second time while cleaning up a directory created
*before* the fix. Full account and repair:
[G-01](docs/GOTCHAS.md#g-01--a-leaked-bind-mount-plus-rm--rf-destroys-your-host).
The script no longer bind-mounts `/dev` at all — it `mknod`s five nodes inside
the root — and it refuses to `rm -rf` a directory with anything mounted under
it.

### What remains, stated as weakly as the evidence allows

A **low-rate intermittent** crash survives the repair.

| observation | count |
| --- | --- |
| native trials, healthy `/dev` ([`92-`](experiments/out/92-segfault-rate.txt), 10 + 20) | **0 crashes / 30** |
| an 8-cell factor matrix (dev nodes × invocation count × cache location) | **0 / 8** |
| native-cold vs qemu-cold control, 5 each | **0 / 10** |
| the five-architecture run in [`95-`](experiments/out/95-cross-arch-bootstrap.txt) | **2 crashes** |
| one `90-` run with verified-healthy `/dev` | **1 crash** |

≈ **3 in 50**, and ⚠ **all of them under load** — five ~800 MB bootstraps
running back to back. Nothing isolated it further.

⭐ **It moves between architectures across runs** — `x86_64` and
`powerpc64le` in one five-architecture run, `loongarch64` in the next, none in
a third. That rules out an architecture-specific cause and is consistent with
load or timing.

### Ruled out, by measurement

| hypothesis | how it died |
| --- | --- |
| the missing `invalidate curl data in child` patch | applied, rebuilt, crash identical |
| static libcurl/OpenSSL teardown in general | `filesystem`, `iana-etc`, `-Q`, `-Si`, `-Sy` all exit 0 |
| hooks are inherently fatal | a `--debug` run executed `locale-gen`, `ldconfig`, `iconvconfig`, `update-ca-trust` and eight `systemd-hook` calls and exited 0 |
| path-specific | the same path exits 0 with `--debug`, and other paths crash |
| sync and install in one process | an 8-cell matrix in which all 8 passed |
| running under `qemu-user` | 5 native-cold and 5 qemu-cold trials, all 0 |

### Do next

1. `experiments/92-segfault-rate.sh 100`, **with concurrent load**, since load
   is the only surviving correlate. It already enables core dumps and runs
   `gdb` on the first crash.
2. Take the backtrace. ⛔ **Three guesses have now been wrong**; the next
   statement about the cause should come from a stack, not a hypothesis.
3. Check whether an **Arch-built** `pacman-static` shows it on the same host.
   If it does, this is not a `zig cc` artefact and is not this project's.

⚠ **Do not script around it by ignoring pacman's exit code.** Check the
installed package count instead.

---

## 10. The instrument that lied, and the fix

The prescribed mining script recovers paginated API responses by counting `[`
and `]` over the concatenated raw text. That counts brackets inside **string
values**. Comment bodies are markdown.

Measured against `json.load` over the same captured bytes:

| | |
| --- | --- |
| items, oracle | **100** |
| items, the script's joiner | **0** |
| `[` or `]` inside comment bodies | 38 |
| net imbalance | +2 |

and the enclosing function printed `comments: ok`.

Fixed in [`scripts/mine-repo.sh`](scripts/mine-repo.sh) — each page parsed as
its own document — plus a guard that reports a join yielding `[]` from a
non-empty page as a failure. **Measured after: comments 0 → 202.** Everything
in §5 came out of those 202.

Recorded at [`docs/patches/mine-repo-page-join.md`](docs/patches/mine-repo-page-join.md);
asserted by [`experiments/40-mine-repo-joiner-defect.sh`](experiments/40-mine-repo-joiner-defect.sh).

---

## 11. Verdicts

| reference | verdict | reason |
| --- | --- | --- |
| `aur/pacman-static` | ⭐ **adopt** | build order, `install-<target>` discipline, `-D_LARGEFILE64_SOURCE`, the signed-tag-plus-commit pin — §4.4 |
| `manjaro-contrib/…` | **adopt one patch** | `0001-libalpm-invalidate-curl-data-in-child.patch` — §4.3 |
| `manjaro-contrib/…` | **anti-pattern exhibit** | ⛔ declares `riscv64`, cannot build it, and its CI has one `aarch64` runner so nothing catches it. Kept on purpose as the clearest example of a declared architecture that is not a built one — §4.2 |
| `firasuke/mussel` | **refused** | no `loongarch64`, not relocatable, and its own tracker records both — §7.1. Revisit only if `zig cc` is ruled out. |
| `archlinux/pacman` | **confirms** | `buildstatic` is a real meson option; the four `getpwnam` sites are why musl — §7.2 |
| `seccomp/libseccomp` | **confirms** | backends exist for all five; no work needed |
| `Azathothas/TEMPLATE` `mine-repo.sh` | **adopt, patched** | §10 |

---

## 12. What to double-check

Ranked by what a wrong answer costs.

1. ⛔ **The intermittent crash (§9).** Three guesses have been wrong. Get a
   backtrace.
2. ⚠ **Every non-`x86_64` claim.** All `qemu-user`. No real hardware, no real
   page sizes, no `chroot` into a foreign root.
3. ⚠ **The five repositories.** Measured live on one day, one mirror each.
   `mirror.archlinuxarm.org` answered only over **plain HTTP**; the LoongArch
   mirror timed out once mid-sweep.
4. ⚠ **Source provenance (T-07).** No signature was verified anywhere.
5. ⚠ **`zig cc` versus GCC on the exotic three.** They build, install and
   produce correct ELF. No pacman transaction has run on real hardware.
6. ⚠ **The `-Di18n=false` choice.** Untested against the reference's default.
7. ⚠ **`mussel#2`.** Felker's finding was about patches that are gone; the two
   CVE patches now in the tree were **not** reviewed here.

⛔ **Assume more remain.**
