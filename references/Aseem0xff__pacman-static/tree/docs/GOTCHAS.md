# GOTCHAS

Everything that cost time here, so it costs you none. Ordered by **how much
damage it does**, not by how likely it is.

Each entry says what you see, what is actually wrong, and the fix.

---

## Routing: which document, and when

| when you are… | read |
| --- | --- |
| about to `rm -rf` anything you have `chroot`ed into or bind-mounted | **G-01 below. Now. It ate this machine's `/dev` twice.** |
| deciding how to cross-compile | [`RESEARCH.md` §7](../RESEARCH.md#7-why-this-shape), then `TASKS.md` T-01 |
| writing the build | `TASKS.md` T-01…T-06, with G-02…G-07 open beside it |
| a build failed and the error names the wrong component | G-04 (curl blames OpenSSL), G-05 (an "incompatible" library from another arch) |
| bootstrapping a root | [`examples/02-bootstrap-arch-rootfs.md`](../examples/02-bootstrap-arch-rootfs.md), then G-01, G-08, G-09 |
| targeting a non-`x86_64` architecture | G-10 (repository layouts), G-11 (keyrings) |
| about to believe a number in this repository | [`RESEARCH.md` §0](../RESEARCH.md#0-what-this-sweep-got-wrong-about-itself) — it lists what this sweep got wrong about itself |
| deciding whether a claim was measured | [`experiments/README.md`](../experiments/README.md); every claim names its script |

---

## G-01 ⛔ A leaked bind mount plus `rm -rf` destroys your host

**Severity: this is the one that does real damage.** It happened twice on the
machine that produced this repository.

**What you see.** Nothing, at first. Later, unrelated things break: builds
fail, `gpg` hangs, and `pacman -S base` ends in `error: segmentation fault`
after installing every package successfully.

**What is actually wrong.** A bootstrap script does this:

```sh
mount --bind /dev "$ROOT/dev"     # for the chroot
...
umount "$ROOT/dev"                # ← fails with "target is busy", unchecked
```

The mount survives the script. The **next** run's `rm -rf "$ROOT"` then walks
*through* the live bind mount and deletes the **host's** device nodes.
`/dev/zero`, `/dev/urandom` and `/dev/tty` disappear; `/dev/null` is left
behind as a regular file. Everything that needs entropy — OpenSSL, gpg,
pacman — then misbehaves in ways that look like bugs in those programs.

⛔ **A whole round of hypothesis and control-matrix work in this repository
went into a "pacman segfault" that was this.** [`RESEARCH.md` §0](../RESEARCH.md#0-what-this-sweep-got-wrong-about-itself).

**The fix, in order of how much it buys:**

1. ⭐ **Do not bind-mount `/dev` at all.** Create the nodes inside the root:
   ```sh
   mknod -m 666 "$ROOT/dev/null"    c 1 3
   mknod -m 666 "$ROOT/dev/zero"    c 1 5
   mknod -m 666 "$ROOT/dev/random"  c 1 8
   mknod -m 666 "$ROOT/dev/urandom" c 1 9
   mknod -m 666 "$ROOT/dev/tty"     c 5 0
   ```
   Those five are enough for install scriptlets and for `gpg`. Nothing then
   points at the host, so the cleanup path has nothing dangerous to undo.
2. **Guard the `rm` anyway.** Refuse to delete a directory with anything
   mounted under it:
   ```sh
   awk -v d="$ROOT" 'index($2, d) == 1 {print $2}' /proc/mounts | grep -q . && exit 2
   ```
3. **Verify every `umount`.** `umount -l` returns success while the mount is
   still listed, so its exit code decides nothing. Re-read `/proc/mounts`.

Both 1 and 2 are in [`experiments/90-bootstrap-arch.sh`](../experiments/90-bootstrap-arch.sh).

**If it has already happened,** restore the nodes:

```sh
rm -f /dev/null
mknod -m 666 /dev/null c 1 3   ; mknod -m 666 /dev/zero    c 1 5
mknod -m 666 /dev/full c 1 7   ; mknod -m 666 /dev/random  c 1 8
mknod -m 666 /dev/urandom c 1 9; mknod -m 666 /dev/tty     c 5 0
mknod -m 600 /dev/console c 5 1; mknod -m 666 /dev/ptmx    c 5 2
ln -sf /proc/self/fd /dev/fd
ln -sf /proc/self/fd/0 /dev/stdin
ln -sf /proc/self/fd/1 /dev/stdout
ln -sf /proc/self/fd/2 /dev/stderr
```

Check with `head -c 4 /dev/urandom >/dev/null && echo ok`.

---

## G-02 ⛔ Reusing one source tree across architectures

**What you see.** Every architecture reports a successful build and a full set
of `.a` files. Then one link fails:

```
ld.lld: error: /…/out/x86_64/lib/libzstd.a(zstd_lazy.o)
        is incompatible with aarch64linux
```

**What is actually wrong.** The **aarch64** prefix's own pkg-config file:

```
out/aarch64/lib/pkgconfig/libcrypto.pc:2:prefix=/…/out/x86_64
```

OpenSSL bakes the prefix into `configdata.pm` and its generated `.pc` files.
A second `Configure` in a tree that still holds the first build's artifacts
does not regenerate all of them. autotools packages do the same through
`config.status`.

⚠ **Copying a source directory is not enough either.** The second attempt at
this fix copied `$SRC/<pkg>` per architecture — and `$SRC/openssl-3.6.4` had
already been configured once, so every copy inherited the poison.

**The fix.** Extract from the tarball, per architecture, every time. Pristine
by construction. `srcdir()` in
[`experiments/70-build-static-stack.sh`](../experiments/70-build-static-stack.sh).

**How to catch it.** Assert that no prefix contains a path to another
architecture:

```sh
grep -o "$WORK/out/[a-z0-9_]*" "$WORK/out/$ARCH"/lib/pkgconfig/*.pc | sort -u
# must name only $ARCH
```

`70-` fails the run on this.

---

## G-03 ⚠ `--hookdir` must already exist, but `--dbpath` need not

**What you see.**

```
error: 'failed to resolve path '…/etc/pacman.d/hooks' passed to '--hookdir':
No such file or directory
```

**What is actually wrong.** pacman **creates** `--dbpath` and `--cachedir` and
**refuses** a missing `--hookdir`. The asymmetry is undocumented.

**The fix.** Create all of them:

```sh
mkdir -p "$ROOT"/var/lib/pacman "$ROOT"/var/cache/pacman/pkg \
         "$ROOT"/etc/pacman.d/hooks "$ROOT"/etc/pacman.d/gnupg "$ROOT"/var/log
```

⚠ **And redirect all of them.** `--root` alone is not enough: pacman keeps its
database, cache, hooks, keyring and log at compiled-in absolute paths. Miss
one and it writes onto the **host** — silent on a non-Arch host, corrupting on
an Arch one.

---

## G-04 ⚠ curl says OpenSSL is missing when Brotli is the problem

**What you see.**

```
configure: error: --with-openssl was given but OpenSSL could not be detected
```

with a perfectly good `libcrypto.a` right there.

**What is actually wrong.** `config.log` has the truth:

```
ld.lld: error: undefined symbol: BrotliEncoderCreateInstance
>>> referenced by c_brotli.c
>>>               libcrypto-lib-c_brotli.o in archive …/libcrypto.a
```

OpenSSL 3.6 built `enable-brotli` puts `c_brotli.o` in `libcrypto.a`, and that
object needs the Brotli **encoder**. curl's probe links only `libbrotlidec`
and `libbrotlicommon`.

**The fix.**

```sh
LIBS=-lbrotlienc ./configure --with-openssl …
```

⭐ Cheaper than the reference's, which patches `configure.ac` and then needs
`autoreconf -if` and autotools on the build host. Upstream: curl issue 17678.

**General lesson.** ⚠ **A configure error names the option that failed, not
the library that caused it.** Read `config.log`, not the error.

---

## G-05 ⚠ Never copy the reference's architecture `case` block

Both the canonical `pacman-static` PKGBUILD and the Manjaro fork contain:

```sh
riscv64)
    openssltarget='linux64-$CARCH'     # ← single quotes
```

`$CARCH` never expands. OpenSSL's `Configure` gets a literal string and fails.
The fork **declares** `riscv64` in `arch()` and inherits this verbatim.

⭐ **Declared is not built.** Nothing in the fork's CI catches it: it has one
runner tag, `aarch64`.

Neither reference has a case for `loongarch64` or `powerpc64le` at all. The
full table is in `TASKS.md` T-04. Asserted by
[`experiments/30-reference-defects.sh`](../experiments/30-reference-defects.sh).

---

## G-06 ⚠ `-static` in `LDFLAGS` does nothing under a meson cross build

**What you see.** Every flag in your script says `-static`, and
`readelf -l` finds a `PT_INTERP`.

**What is actually wrong.** meson in cross mode takes link arguments from the
cross file. `LDFLAGS` from the environment reaches the **build machine**
compiler used for helper programs.

**The fix.**

```ini
[built-in options]
c_link_args = ['-static', '-L<prefix>/lib']
```

**And check, do not assume:**

```sh
readelf -l build/pacman | grep -c INTERP    # must be 0
```

---

## G-07 ⚠ meson puts the binary at the build root

Not at `build/src/pacman/pacman`, mirroring the source layout — at
**`build/pacman`**.

⛔ **A script that looks in the wrong place and does not treat "not found" as a
failure will report a pass for something it never measured.** That is exactly
what happened here; [`RESEARCH.md` §0](../RESEARCH.md#0-what-this-sweep-got-wrong-about-itself), claim 4.

---

## G-08 ⚠ `pacman-key` cannot run from the static binary

It is a **shell script that drives `gpg`**, and there is no `gpg` inside a
static pacman. So the first pass into an empty root cannot verify signatures —
there is nothing there to verify them with.

**The two-pass shape:** install with `SigLevel = Never`, then `chroot` in and
run `pacman-key --init && pacman-key --populate <keyring>`, then rewrite
`/etc/pacman.conf` with `SigLevel = Required DatabaseOptional` and re-sync.

⚠ **A bootstrap that stops after pass 1 has proved the download worked and
nothing about trust.** Worked example:
[`examples/02-bootstrap-arch-rootfs.md`](../examples/02-bootstrap-arch-rootfs.md).

---

## G-09 ⚠ An emulator's name is not the triple prefix

Debian ships ppc64le's user emulator as **`qemu-ppc64le-static`**; the triple
says `powerpc64le`. A script that derives the name with `cut -d- -f1` reports
"no emulator" for one that is installed — and then prints `-` in a results
column, which reads like a pass.

| triple prefix | emulator |
| --- | --- |
| `x86_64` | `qemu-x86_64-static` |
| `aarch64` | `qemu-aarch64-static` |
| `riscv64` | `qemu-riscv64-static` |
| `loongarch64` | `qemu-loongarch64-static` |
| **`powerpc64le`** | **`qemu-ppc64le-static`** |

⛔ **An absence is not a zero.** Print `NOT MEASURED`, never `-`, and never
count it as a pass.

---

## G-10 ⚠ The five Arch-family repositories have five different layouts

`$repo/os/$arch` is correct on exactly one of them.

| arch | `Server =` | trap |
| --- | --- | --- |
| `x86_64` | `https://geo.mirror.pkgbuild.com/$repo/os/$arch` | — |
| `aarch64` | `http://mirror.archlinuxarm.org/$arch/$repo` | `$arch` and `$repo` are **swapped**; **plain HTTP** |
| `riscv64` | `https://archriscv.felixc.at/repo/$repo` | **flat** — no `os/$arch` at all |
| `loongarch64` | `https://mirrors.wsyu.edu.cn/loongarch/archlinux/$repo/os/loong64` | the directory is **`loong64`**, so `$arch` cannot be used |
| `powerpc64le` | `https://repo.archlinuxpower.org/$repo/powerpc64le` **and** `…/base/any` | three traps at once — see below |

⛔ **ArchPOWER needs two repository sections, not one.** Its core-equivalent
repo is named `base`; it has **no `extra`** (so `extra.db` 404s and the whole
sync fails); and it splits arch-specific and `any`-architecture packages into
**separate databases** — `base/powerpc64le/base.db` (3736 packages) and
`base/any/base-any.db` (2200). `iana-etc` and `openssl` are only in the
second, so with the first alone pacman answers *"unable to satisfy dependency
'iana-etc' required by filesystem"* and unwinds all the way to `base`.

⚠ Two `Server` lines under one repo name are **mirrors of one database**,
which is not what is needed here:

```ini
[base]
Server = https://repo.archlinuxpower.org/$repo/powerpc64le

[base-any]
Server = https://repo.archlinuxpower.org/base/any
```

Each of the other traps 404s silently if you assume uniformity. Verified live by
[`experiments/20-probe-arch-repos.sh`](../experiments/20-probe-arch-repos.sh),
which checks the database **parses**, not merely that it answers 200 — a
mirror serving an HTML error page with a 200 fails that check.

---

## G-11 ⚠ Four different keyrings

`pacman-key --populate archlinux` is correct on three of the five.

| arch | keyring package | `--populate` argument |
| --- | --- | --- |
| `x86_64`, `riscv64`, `loongarch64` | `archlinux-keyring` | `archlinux` |
| `aarch64` | `archlinuxarm-keyring` | `archlinuxarm` |
| `powerpc64le` | `archpower-keyring` | `archpower` |

---

## G-12 ⚠ brotli does not build for loongarch64 with clang

**What you see.**

```
c/common/context.c:6:20: error: code model 'small' is not supported on this target
```

**What is actually wrong.** brotli selects `__attribute__((model))` on a
**compiler version** test, not a target test. clang answers
`__has_attribute(model)` affirmatively on every target and then rejects the
value where the target has no code models. GCC's loongarch backend accepts it,
so this is clang-only.

**The fix.** [`patches/brotli-1.2.0/0001-no-code-model-attribute-on-loongarch.patch`](../patches/brotli-1.2.0/).
It is a no-op on the other four targets, so it is applied unconditionally.

---

## G-13 ⚠ Two of the reference's source hosts do not answer

Measured from one network; re-measure on yours with
[`experiments/10-probe-source-hosts.sh`](../experiments/10-probe-source-hosts.sh).

| host | result | substitute |
| --- | --- | --- |
| `ftpmirror.gnu.org` | **502** | `sourceware.org/pub/{binutils,gcc}` |
| `libisl.sourceforge.io` | **403** | only needed with `--enable-isl` |
| `musl.libc.org` | **intermittent** | Alpine's `distfiles` mirror |
| `zlib.net` | **404** for the pinned version | Alpine's `distfiles` mirror |

⚠ **`HEAD` is not a control for `GET`.** `musl.libc.org` answers `000` to a
`HEAD` and serves the tarball on a `GET`. A `HEAD`-only probe condemns a
working mirror. This document said it was dead until the probe was fixed.

This is `firasuke/mussel` issue **57**, still open; the maintainer's own answer
is that the GNU redirector sends you to "faulty but nearby" mirrors.

---

## G-14 ⚠ mussel toolchains cannot be moved

`--prefix` and `--with-sysroot` are baked in at configure time. A cached
toolchain artefact only works if every runner unpacks it at the **identical
absolute path**.

`firasuke/mussel` issue **29**, closed with no comment and no fix in the tree.
⛔ This, not build time, is what makes mussel expensive in CI.

---

## G-15 ⚠ A `--debug` run is a different run

Twice here, adding `--debug` to reproduce a crash made it stop reproducing —
because the run also differed in another way that had not been held still.

⭐ **Change one thing.** And for an intermittent fault, a 2×2 matrix with two
runs per cell proves nothing: any cell can pass twice by luck. Measure a
**rate** over many identical trials instead —
[`experiments/92-segfault-rate.sh`](../experiments/92-segfault-rate.sh)
replaced [`91-segfault-control.sh`](../experiments/91-segfault-control.sh)
for exactly that reason, and 91- is kept in the tree, labelled, so its numbers
stay traceable.

---

## G-16 ⛔ `git -C <dir> rev-parse HEAD` answers for the wrong repository

**What you see.** A provenance line naming a commit. It is confidently wrong.

**What is actually wrong.** If `<dir>` has no `.git` of its own but sits inside
another git repository, `git -C <dir> rev-parse HEAD` **does not fail** — it
walks up and answers with the *enclosing* repository's HEAD. A `|| echo
<fallback>` guard never fires, because the command succeeded.

⛔ **This produced a citation in this repository that pointed at the wrong
tree**, and it appeared the moment the corpus trees had their nested `.git`
directories stripped — which is itself required, because a nested `.git`
commits as a gitlink and a fresh clone lands an empty folder.

**The fix.** Record the commit in a file when you capture it, and read that:

```sh
sed -n 's/^| commit | `\([0-9a-f]\{40\}\)` |.*/\1/p' "$dir/PROVENANCE.md"
```

**To ask git safely**, pin the boundary:

```sh
GIT_CEILING_DIRECTORIES="$(dirname "$dir")" git -C "$dir" rev-parse --show-toplevel
```

---

## G-17 ⛔ A vendored tree's own `.gitignore` silently drops files from your commit

**What you see.** Everything builds on your machine. A fresh clone fails:

```
ERROR: File …/build-aux/edit-script.sh.in does not exist.
```

**What is actually wrong.** `git add -A` honours a `.gitignore` at **any**
depth. Vendor a source tree and you vendor its ignore rules with it — pacman's
own `build-aux/.gitignore` excludes `edit-script.sh.in`, which is generated in
*its* build but is a **committed source file** in yours.

⚠ **A top-level negation cannot rescue it.** Deeper `.gitignore` files
override shallower ones, so `!references/**` at the root does nothing.

⛔ **The tested tree and the committed tree were different**, and only a fresh
clone could tell. Five files across two vendored trees here.

**The fix.** Delete the nested `.gitignore` files from the vendored tree and
record that you did. `git add -f` also works but relies on everybody
remembering it forever.

**The check**, worth putting in CI:

```sh
diff <(git ls-files references/ | sort) \
     <(find references \( -type f -o -type l \) | sort)
```

⚠ **Count symlinks.** `find -type f` alone undercounts and makes a clean tree
look broken.

---

## G-18 ⚠ `git clone --shared` from a shallow corpus cannot reach a commit

**What you see.**

```
fatal: reference is not a tree: 54d94116164b0b2202c6061c4a59c6f3e70820d8
```

for an object that is demonstrably in the shared object store.

**The fix.** `git archive <commit> | tar -x -C dest`. It resolves the commit in
the source repository, works against a shallow clone, and leaves the corpus
untouched.
