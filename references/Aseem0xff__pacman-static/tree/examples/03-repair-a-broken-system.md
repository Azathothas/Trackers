# Example 03 — repair a system whose libc is gone

**The job this binary actually exists for.** Arch's own package description is
*"Statically-compiled pacman (to fix or install systems without libc)"*.

**What this shows:** a root filesystem with its dynamic loader deleted —
nothing in it can execute at all, including its own `pacman` and its own
`bash` — repaired from outside by the static binary.

**Verified on:** Ubuntu 24.04.4, `x86_64`, 2026-08-28, against a root built by
[Example 02](02-bootstrap-arch-rootfs.md).

---

## Why a dynamically linked pacman cannot do this

An interrupted `pacman -Syu` across a `glibc` update, a bad `rm` in
`/usr/lib`, a failed disk — and the loader is missing or mismatched. From that
moment:

- the system's `pacman` cannot start, because it needs the loader;
- so it cannot reinstall the loader;
- and neither can anything else on the machine.

⭐ **A static binary has no such dependency.** It carries its own libc, so it
runs on a system that has none.

---

## Step 1 — break a root, exactly the way a real failure does

⚠ Do this to the **throwaway root from Example 02**, not to your machine.

```sh
export ROOT=/tmp/archroot
rm -f "$ROOT/usr/lib/ld-linux-x86-64.so.2"
```

Confirm how dead it is:

```sh
chroot "$ROOT" /usr/bin/bash   -c 'echo hi'
chroot "$ROOT" /usr/bin/pacman --version
```

```
chroot: failed to run command '/usr/bin/bash': No such file or directory
chroot: failed to run command '/usr/bin/pacman': No such file or directory
```

⚠ **"No such file or directory" is about the loader, not the binary.** Both
files are present and executable. The kernel cannot find the `PT_INTERP` they
name. This error message has sent many people looking for the wrong missing
file.

---

## Step 2 — repair it from outside

The static binary never enters the broken root to do this. It reads the root's
package database, downloads `glibc`, and unpacks it — all with its own libc.

```sh
export PACMAN=$WORK/pacman/x86_64/build/pacman

"$PACMAN" \
  --config   /tmp/pacboot/pacman.conf \
  --root     "$ROOT" \
  --dbpath   "$ROOT/var/lib/pacman" \
  --cachedir "$ROOT/var/cache/pacman/pkg" \
  --hookdir  "$ROOT/etc/pacman.d/hooks" \
  --gpgdir   "$ROOT/etc/pacman.d/gnupg" \
  --logfile  "$ROOT/var/log/pacman.log" \
  -S --noconfirm glibc
```

```
(2/4) Configuring dynamic linker run-time bindings...
(3/4) Creating iconv module configuration cache...
(4/4) Arming ConditionNeedsUpdate...
```

Exit status `0`.

⚠ **The hooks ran.** `ldconfig` executed **inside** the target root, using the
loader that had just been restored. That ordering is the whole trick: pacman
unpacks first, then runs the hooks.

---

## Step 3 — confirm the repair

```sh
chroot "$ROOT" /usr/bin/bash -c 'echo REPAIRED'
chroot "$ROOT" /usr/bin/pacman --version
```

```
REPAIRED
 .--.                  Pacman v7.1.0 - libalpm v16.0.1
```

The root's own dynamically linked pacman runs again.

---

## Doing this to a real machine

Same commands, with the broken system mounted somewhere and no `--config`
override, because the real `/etc/pacman.conf` is already there.

From a rescue USB, with the broken root at `/mnt`:

```sh
./pacman-static -r /mnt -Syu --noconfirm
```

Or reinstall just what is broken:

```sh
./pacman-static -r /mnt -S --noconfirm glibc
./pacman-static -r /mnt -S --noconfirm pacman bash coreutils
```

To find what is damaged before touching anything:

```sh
./pacman-static -r /mnt -Qkk        # verify every file of every package
```

⚠ **`-r /mnt` alone leaves `--gpgdir` and `--hookdir` at their compiled-in
paths**, which under `-r` resolve inside `/mnt` — so this case is fine. It is
the *Example 02* case, redirecting each path individually against an empty
directory, where missing one writes onto the host.
[G-03](../docs/GOTCHAS.md#g-03----hookdir-must-already-exist-but---dbpath-need-not).

### Repairing a machine of a different architecture

⭐ You do not need a working system of that architecture. Take the
`pacman-static` for the **broken machine's** architecture from
[Example 01](01-build-from-source.md), copy it onto that machine's rescue
media, and run it there. It has no dependencies to satisfy.

---

## What still has to be true

The static binary removes the libc dependency. It does not remove these:

| | |
| --- | --- |
| a **kernel** that can execute it | the binary is static, not a bootloader |
| a **working `/dev`** | `/dev/urandom` at minimum; OpenSSL and gpg need entropy. [G-01](../docs/GOTCHAS.md#g-01--a-leaked-bind-mount-plus-rm--rf-destroys-your-host) is what a broken `/dev` looks like from the outside |
| **`/etc/ssl/certs/ca-certificates.crt`** on the machine running it, for `https` | curl was built with that **path**, not embedded certificates. Use an `http` mirror if you have no CA bundle |
| a **writable target** | mount it read-write first |
| ⚠ **matching architecture** | a `x86_64` static pacman cannot repair an `aarch64` root's binaries; build the right one |
