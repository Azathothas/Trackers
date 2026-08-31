# Example 02 — bootstrap an Arch Linux root from source, with nothing but the binary

**What this produces:** a working Arch Linux root filesystem, built from the
real repositories, on a host that is **not Arch** and has **no pacman, no
libalpm and no Arch keyring** — using only the binary from
[Example 01](01-build-from-source.md).

**Verified on:** Ubuntu 24.04.4, `x86_64`, 2026-08-28.
137 packages, 704 MB, ~30 seconds.

**Run it as a script:** [`experiments/90-bootstrap-arch.sh`](../experiments/90-bootstrap-arch.sh)
does exactly what is below and asserts each step.

---

> ## ⛔ Read this before you run anything
>
> This example creates a root you will `chroot` into. **Do not bind-mount the
> host's `/dev` into it.** If the `umount` later fails — and it does — a
> subsequent `rm -rf` on that directory deletes your **host's** device nodes.
>
> It happened twice on the machine that produced this repository, and the
> resulting breakage looked like a pacman bug for hours.
> [**G-01**](../docs/GOTCHAS.md#g-01--a-leaked-bind-mount-plus-rm--rf-destroys-your-host)
> has the full account and the repair.
>
> The recipe below uses `mknod` instead. Nothing in the root points at the
> host, so there is nothing dangerous to clean up.

---

## Step 0 — what the host needs

| | |
| --- | --- |
| the binary | `pacman` from Example 01, matching the **host's** architecture |
| root | ⚠ needed for `chroot` and `mknod`, not for the download |
| network | reaching your chosen mirror |
| CA certificates | `/etc/ssl/certs/ca-certificates.crt` must exist — curl was built with that **path** compiled in, not embedded data |

```sh
export PACMAN=$WORK/pacman/x86_64/build/pacman
export ROOT=/tmp/archroot
```

---

## Step 1 — create every directory pacman will be told about

```sh
mkdir -p "$ROOT"/var/lib/pacman \
         "$ROOT"/var/cache/pacman/pkg \
         "$ROOT"/etc/pacman.d/hooks \
         "$ROOT"/etc/pacman.d/gnupg \
         "$ROOT"/var/log \
         "$ROOT"/dev "$ROOT"/proc
```

⚠ **`--hookdir` must already exist.** pacman *creates* `--dbpath` and
`--cachedir` and *refuses* a missing `--hookdir`, with
`error: 'failed to resolve path … passed to '--hookdir''`.
[G-03](../docs/GOTCHAS.md#g-03----hookdir-must-already-exist-but---dbpath-need-not).

---

## Step 2 — write a `pacman.conf`

Keep it **outside** the root, so nothing in the transaction can rewrite it.

```sh
mkdir -p /tmp/pacboot
cat > /tmp/pacboot/pacman.conf <<'EOF'
[options]
Architecture = x86_64
SigLevel     = Never

[core]
Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch

[extra]
Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch
EOF
```

⚠ **`SigLevel = Never` for this pass only.** There is no keyring yet and no
`gpg` to build one with — see Step 5, and
[G-08](../docs/GOTCHAS.md#g-08--pacman-key-cannot-run-from-the-static-binary).
Treat everything installed in Steps 3–4 as unverified until Step 5 has run.

**For another architecture,** the `Server` line and the repository names both
change. All five are in
[G-10](../docs/GOTCHAS.md#g-10--the-five-arch-family-repositories-have-five-different-layouts);
⛔ `$repo/os/$arch` is correct on exactly one of them.

---

## Step 3 — redirect every path, then sync

⚠ **`--root` alone is not enough.** pacman keeps its database, cache, hooks,
keyring and log at compiled-in absolute paths. Miss one and it writes onto the
**host** — silent on a non-Arch host, corrupting on an Arch one.

```sh
pac() {
  "$PACMAN" \
    --config   /tmp/pacboot/pacman.conf \
    --root     "$ROOT" \
    --dbpath   "$ROOT/var/lib/pacman" \
    --cachedir "$ROOT/var/cache/pacman/pkg" \
    --hookdir  "$ROOT/etc/pacman.d/hooks" \
    --gpgdir   "$ROOT/etc/pacman.d/gnupg" \
    --logfile  "$ROOT/var/log/pacman.log" \
    "$@"
}

pac -Sy --noconfirm
```

This proves the binary reaches a real mirror over TLS and parses the
databases.

---

## Step 4 — install `base`

```sh
pac -S --noconfirm base
```

```
:: Processing package changes...
installing filesystem...
installing glibc...
…
installing base...
```

Check it:

```sh
ls -1 "$ROOT/var/lib/pacman/local" | grep -vc ALPM_DB_VERSION   # 137
du -sh "$ROOT"                                                  # 704M
```

⚠ **`base` is the right target, not a hand-picked minimal set.** Install
scriptlets run with the *target root's* shell, so a root without `bash` cannot
run them. `base` pulls it in.

---

## Step 5 — ⭐ make it trustworthy

Everything so far ran with signature checking **off**. This is the step that
turns a download into a verified system, and it runs *inside* the root,
because that is where `gpg` now exists.

```sh
# Five device nodes. NOT a bind mount of the host's /dev — see G-01.
mknod -m 666 "$ROOT/dev/null"    c 1 3
mknod -m 666 "$ROOT/dev/zero"    c 1 5
mknod -m 666 "$ROOT/dev/random"  c 1 8
mknod -m 666 "$ROOT/dev/urandom" c 1 9
mknod -m 666 "$ROOT/dev/tty"     c 5 0

mount -t proc proc "$ROOT/proc"
cp -f /etc/resolv.conf "$ROOT/etc/resolv.conf"

chroot "$ROOT" /usr/bin/bash <<'EOS'
set -e
pacman-key --init
pacman-key --populate archlinux

cat > /etc/pacman.conf <<'CONF'
[options]
Architecture = auto
SigLevel     = Required DatabaseOptional

[core]
Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch

[extra]
Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch
CONF

pacman -Sy --noconfirm
pacman -S  --noconfirm --needed --downloadonly bash    # verified download
echo "signature verification works"
EOS
```

⭐ **`--populate archlinux` is right on three of the five architectures.**
`aarch64` needs `archlinuxarm`, `powerpc64le` needs `archpower` —
[G-11](../docs/GOTCHAS.md#g-11--four-different-keyrings).

**Stronger, and not done here:** verify `archlinux-keyring` against a pinned
fingerprint on the host *before* Step 4, so pass 1 is verified too —
`TASKS.md` T-10.

---

## Step 6 — prove the root actually works

```sh
chroot "$ROOT" /usr/bin/bash -c 'echo it-runs'
chroot "$ROOT" /usr/bin/pacman --version
```

```
it-runs
 .--.                  Pacman v7.1.0 - libalpm v16.0.1
```

⭐ **This is the check that means "a working root".** It executes the new
root's own **dynamically linked** glibc binaries with the new root's own
loader. Nothing about the static builder is involved any more — including its
libc, its compiler, and its architecture assumptions.

---

## Step 7 — clean up, safely

```sh
# Unmount FIRST, and verify. `umount -l` returns success while the mount is
# still listed, so its exit code decides nothing — /proc/mounts does.
umount "$ROOT/proc" 2>/dev/null || umount -l "$ROOT/proc"

awk -v d="$ROOT" 'index($2, d) == 1 {print $2}' /proc/mounts   # must be empty

# Only now:
rm -rf "$ROOT"
```

⛔ **If that `awk` prints anything, do not run the `rm`.** G-01.

---

## Turning it into a tarball

```sh
tar --numeric-owner --xattrs --acls -C "$ROOT" -c . | zstd -19 -o archroot.tar.zst
```

`--numeric-owner` matters: the host has no Arch users, so names would not map.

---

## Known issue

`pacman -S base` occasionally ends in `error: segmentation fault` **after**
installing all 137 packages. The root is complete and usable when it happens;
only the exit status is wrong. Roughly 3 runs in 50 here.

⛔ **Check your `/dev` first.** The large cluster of these crashes seen while
writing this repository was caused by a **broken host `/dev`** — a leaked bind
mount plus an `rm -rf`. With `/dev/urandom` missing, OpenSSL and gpg fail in
ways that surface exactly like this:

```sh
head -c 4 /dev/urandom >/dev/null && echo ok
```

[G-01](../docs/GOTCHAS.md#g-01--a-leaked-bind-mount-plus-rm--rf-destroys-your-host)
has the repair. It is also why this example uses `mknod` rather than a bind
mount.

⚠ **Do not script around the rest by ignoring pacman's exit code.** Check the
installed package count instead.
[`RESEARCH.md` §9](../RESEARCH.md#9-the-crash-that-is-not-closed) has what has
and has not been ruled out.

---

## This example is executed, not just written

[`experiments/90-bootstrap-arch.sh`](../experiments/90-bootstrap-arch.sh) runs
every step above and asserts each one separately, so a step that silently
stops working fails the script rather than the reader.

```sh
sudo experiments/90-bootstrap-arch.sh x86_64
```

Its recorded output is
[`experiments/out/90-bootstrap-arch.x86_64.txt`](../experiments/out/90-bootstrap-arch.x86_64.txt).

⚠ It is **not** generated from this file — the commands are duplicated, and
nothing yet fails if the two drift apart. `TASKS.md` T-12.

---

**Next:** [Example 03](03-repair-a-broken-system.md) — the job this binary
actually exists for.
