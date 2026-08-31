# FEATURES — what is in the binary, what is not, and why

Measured by [`experiments/85-feature-matrix.sh`](../experiments/85-feature-matrix.sh),
which reports **build-time** resolution and **runtime** execution separately,
because a dependency meson "found" is not yet a feature that works.

Everything below is identical across all five architectures.

---

## Enabled

| feature | library | version | what it gives you |
| --- | --- | --- | --- |
| **package format** | libarchive | 3.8.9 | reads `.pkg.tar.zst`, `.pkg.tar.xz`, `.pkg.tar.gz`, `.pkg.tar.bz2` |
| **downloads** | libcurl | 8.21.0 | `http`, `https`, `ftp`, `ftps`, `file` |
| **TLS** | OpenSSL | 3.6.4 | `libssl` + `libcrypto`, no-shared |
| **HTTP/2** | nghttp2 | 1.70.0 | multiplexed parallel downloads |
| **signatures** | GPGME | 2.1.2 | `SigLevel` verification against a keyring |
| ↳ | libassuan | 3.0.0 | GPGME's IPC transport |
| ↳ | libgpg-error | 1.61 | shared error codes |
| **package hashing** | OpenSSL `libcrypto` | 3.6.4 | MD5 and SHA-256 of package files |
| **download sandbox** | libseccomp | 2.6.0 | syscall filter for the download child |
| ↳ | Linux Landlock | header present | filesystem restriction for the same child |
| **compression** | zlib 1.3.2, xz 5.8.3, bzip2 1.0.8, zstd 1.5.7, brotli 1.2.0 | | every algorithm Arch packages and databases use |
| **large files** | musl `func64` | | via `-D_LARGEFILE64_SOURCE`; see G-02 note below |
| **IPv6** | curl | | `--enable-ipv6` |
| **threaded resolver** | curl | | `--enable-threaded-resolver` |

⭐ **Nothing in pacman's feature set had to be dropped to link statically.**
Every optional dependency pacman looks for is present.

### Curl protocols deliberately narrowed

Compiled out: `dict`, `gopher`, `imap`, `ldap`, `ldaps`, `pop3`, `rtsp`,
`smb`, `smtp`, `telnet`, `tftp`, and the `--libcurl` option.

**Why:** pacman fetches over `http`, `https` and `file`. Every other protocol
is attack surface in a binary whose whole job is to run on a broken or
untrusted system. This mirrors the reference PKGBUILD.

Also disabled: `libidn2`, `librtmp`, `libssh2`, `libpsl`, GSSAPI, `nghttp3`,
`ngtcp2`. Each is a dependency that would have to be built statically for a
capability pacman does not use. **HTTP/3 is therefore not available.**

---

## Disabled, and why

| feature | why | what it costs you | how to turn it back on |
| --- | --- | --- | --- |
| **i18n** (`-Di18n=false`) | pacman's messages are translated through `libintl`. musl provides `gettext` as a **stub**, so translations would not be loaded at run time even with the option on — and the option pulls `msgfmt` into the build. A bootstrap binary that prints English is a feature: its output is what you paste into a bug report. | pacman speaks English only | `-Di18n=true`, and provide a `libintl` for musl |
| **docs** (`-Ddoc=disabled`) | needs `asciidoc` and `a2x` on the build host, and produces man pages that a single-binary tool does not ship | no man pages in the build tree | install asciidoc, `-Ddoc=enabled` |
| **doxygen** (`-Ddoxygen=disabled`) | API documentation for libalpm; nothing consumes it here | none | `-Ddoxygen=enabled` |
| **nettle** (`-Dcrypto=openssl`) | pacman takes **one** crypto provider. OpenSSL is already linked for TLS, so nettle would be a second implementation of the same hashes. | none | `-Dcrypto=nettle`, and drop OpenSSL if you also drop TLS |
| **HTTP/3** | needs `nghttp3` and `ngtcp2` built statically; no Arch mirror requires it | HTTP/2 is the ceiling | build both, `--with-nghttp3 --with-ngtcp2` |

---

## Not in the binary, and cannot be

These are **not build options.** They are things a single static executable
cannot contain, and every one of them will surprise somebody.

| | why | what to do instead |
| --- | --- | --- |
| ⛔ **`pacman-key`** | a **shell script** that drives the `gpg` executable. A static pacman has no `gpg` inside it and cannot make one. | run it inside the target root after the first pass — G-08 and [`examples/02`](../examples/02-bootstrap-arch-rootfs.md) |
| ⛔ **`makepkg`, `repo-add`, `pacman-conf` scripts** | also shell scripts | they are separate files in the build tree; ship them alongside if you need them |
| ⚠ **install scriptlets** | run with the **target root's** shell (`/usr/bin/bash` as configured here), not one inside the binary | an empty root has no shell, so scriptlets only work once `bash` is installed — which is why `base` works and a hand-picked minimal set may not |
| ⚠ **NSS lookups beyond files** | musl resolves users and groups from `/etc/passwd` and `/etc/group` directly. No LDAP, no SSSD, no `nsswitch.conf`. | ⭐ this is the **point**, not a limitation: it is why a static musl pacman works on a system whose libc is broken |
| ⚠ **CA certificates** | curl is built with `--with-ca-bundle=/etc/ssl/certs/ca-certificates.crt`, a **path**, not embedded data | that file must exist on the machine running the binary, or pass `SSLVerify`/use a mirror over plain HTTP |

---

## Verifying all of this yourself

```sh
experiments/85-feature-matrix.sh
```

Reports, per architecture: the version meson resolved for each dependency,
whether `linux/landlock.h` was found, the linkage, the size, and the version
banner the binary prints **when executed under its own architecture's
emulator**.

Its exit code is an assertion: non-zero if the architectures disagree, if a
binary is missing, or if any binary is not statically linked.
