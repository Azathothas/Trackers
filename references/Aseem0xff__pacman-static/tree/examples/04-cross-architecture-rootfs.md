# Example 04 — build a `loongarch64` root on an `x86_64` laptop

**What this produces:** a complete root filesystem for an architecture your
machine cannot execute — built by that architecture's own `pacman`, running
under emulation, against that architecture's own distribution.

**Verified on:** Ubuntu 24.04.4, `x86_64`, 2026-08-28, for all five targets.

**Run it as a script:** [`experiments/95-cross-arch-bootstrap.sh`](../experiments/95-cross-arch-bootstrap.sh)

| arch | distribution | packages | size |
| --- | --- | --- | --- |
| `aarch64` | Arch Linux ARM | 136 | 790 M |
| `riscv64` | Arch Linux RISC-V | 135 | 836 M |
| `loongarch64` | LoongArch Linux | 137 | 841 M |
| `powerpc64le` | ArchPOWER | 140 | — |

⭐ **Neither reference PKGBUILD can produce a `loongarch64` or `powerpc64le`
binary at all**, so this is not a faster route to something that already
exists.

---

## What you need

- `pacman` for the **target** architecture, from [Example 01](01-build-from-source.md)
- `qemu-user-static` with that architecture's emulator

⚠ **The emulator's name is not the triple prefix.** Debian ships ppc64le's as
`qemu-ppc64le-static` while the triple says `powerpc64le`.
[G-09](../docs/GOTCHAS.md#g-09--an-emulators-name-is-not-the-triple-prefix).

```sh
export WORK=${WORK:-$HOME/pacman-static-work}
export ARCH=loongarch64
export PACMAN=$WORK/pacman/$ARCH/build/pacman
export QEMU=qemu-loongarch64-static
export ROOT=/tmp/loongroot
```

---

## Step 1 — the right `pacman.conf` for that distribution

⛔ **This is where most of the time goes if you assume the five are alike.**
They are not. `$repo/os/$arch` is correct on exactly one of them.

### loongarch64 — LoongArch Linux

```ini
[options]
Architecture = loong64
SigLevel     = Never

[core]
Server = https://mirrors.wsyu.edu.cn/loongarch/archlinux/$repo/os/loong64

[extra]
Server = https://mirrors.wsyu.edu.cn/loongarch/archlinux/$repo/os/loong64
```

⚠ The directory is **`loong64`**, not `loongarch64`, so `$arch` cannot be used
and `Architecture` must say `loong64` too.

### aarch64 — Arch Linux ARM

```ini
Architecture = aarch64
[core]
Server = http://mirror.archlinuxarm.org/$arch/$repo
```

⚠ `$arch` and `$repo` are **swapped** relative to Arch, and the mirror answered
only over plain **HTTP** here.

### riscv64 — Arch Linux RISC-V

```ini
Architecture = riscv64
[core]
Server = https://archriscv.felixc.at/repo/$repo
```

⚠ **Flat** — there is no `os/$arch` component at all.

### powerpc64le — ArchPOWER

```ini
Architecture = powerpc64le

[base]
Server = https://repo.archlinuxpower.org/$repo/powerpc64le

[base-any]
Server = https://repo.archlinuxpower.org/base/any
```

⛔ **Three separate traps in one distribution:**
1. The core-equivalent repository is named **`base`**.
2. There is **no `extra`** — `extra.db` answers 404 and the whole sync fails.
3. ⭐ It splits arch-specific and `any`-architecture packages into **two
   databases**: `base/powerpc64le/base.db` (3736 packages) and
   `base/any/base-any.db` (2200). `iana-etc` and `openssl` live only in the
   second, so with the first alone pacman says *"unable to satisfy dependency
   'iana-etc' required by filesystem"* and unwinds all the way to `base`.
   Two `Server` lines under one repo name would be **mirrors of one
   database** — that is not what is needed. They are two databases and need
   two repo sections.

---

## Step 2 — bootstrap

Identical to [Example 02](02-bootstrap-arch-rootfs.md), with the emulator in
front of the binary:

```sh
mkdir -p "$ROOT"/var/lib/pacman "$ROOT"/var/cache/pacman/pkg \
         "$ROOT"/etc/pacman.d/hooks "$ROOT"/etc/pacman.d/gnupg "$ROOT"/var/log

"$QEMU" "$PACMAN" \
  --config /tmp/loong.conf --root "$ROOT" \
  --dbpath "$ROOT/var/lib/pacman" --cachedir "$ROOT/var/cache/pacman/pkg" \
  --hookdir "$ROOT/etc/pacman.d/hooks" --gpgdir "$ROOT/etc/pacman.d/gnupg" \
  --logfile "$ROOT/var/log/pacman.log" \
  -Sy --noconfirm base
```

---

## Step 3 — check what actually landed

⭐ **Read the ELF back. Do not trust the package name.**

```sh
file "$ROOT/usr/bin/bash"
# ELF 64-bit LSB pie executable, LoongArch, version 1 (SYSV), dynamically linked

ls -1 "$ROOT/var/lib/pacman/local" | grep -vc ALPM_DB_VERSION   # 137
du -sh "$ROOT"                                                  # 841M
```

That is a LoongArch userland, assembled on an x86_64 machine, by a LoongArch
`pacman` this repository built from source.

---

## ⛔ What fails, and why it is fine

The last lines of the install read:

```
(12/12) Reloading system bus configuration...
call to execv failed (Exec format error)
error: command failed to execute correctly
```

**Post-transaction hooks fail.** A hook `exec`s a binary of the *target*
architecture inside the chroot, and the host kernel has no `binfmt_misc`
handler registered for it. The emulator only wraps the process you launched;
it does not follow an `exec` into a `chroot`.

⚠ **The packages are installed and the root is complete.** What has not run is
`ldconfig`, `locale-gen`, `update-ca-trust` and the systemd hooks.

**Two ways to finish the job:**

1. **Register binfmt on the host** — modifies host kernel state:
   ```sh
   docker run --rm --privileged multiarch/qemu-user-static --reset -p yes
   ```
2. **Copy the emulator into the root** and run the hooks by hand:
   ```sh
   cp "$(command -v $QEMU)" "$ROOT/usr/bin/"
   chroot "$ROOT" /usr/bin/$QEMU /usr/bin/ldconfig
   ```

Neither is done in `95-cross-arch-bootstrap.sh`, because both change state
outside the working directory. `TASKS.md` T-13.

---

## ⚠ What this does not prove

`qemu-user` emulates the **instruction set** and passes syscalls to the
**host** kernel. It does not exercise:

- the target's kernel, or any syscall behaviour that differs there;
- the target's **page size** — a real `aarch64` or `ppc64le` host may use 64 K
  pages, and a static binary's segment alignment is exactly where that bites;
- real hardware errata, or that distribution's actual boot path.

⛔ **Every non-`x86_64` claim in this repository is a `qemu-user` claim.**
`TASKS.md` T-14.

---

## Packing it up

```sh
tar --numeric-owner --xattrs --acls -C "$ROOT" -c . | zstd -19 -o loongarch64-root.tar.zst
```
