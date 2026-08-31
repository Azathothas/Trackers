# Licensing

`SPDX-License-Identifier: 0BSD`

## Everything in this repository is yours to take

The prose, the scripts, the patches, the fixtures, the experiment output, the
`pacman.conf` snippets, the examples — **all of it** is released under the
[BSD Zero Clause License](LICENSE).

**You may copy, modify, publish, use, compile, sell, or distribute any of it,
in whole or in part, for any purpose, commercial or otherwise.**

- ⭐ **No attribution required.** No credit, no notice, no copyright header,
  no "based on" line, no link back.
- ⭐ **No permission required.** You do not need to ask, and you do not need
  to tell anyone.
- ⭐ **No copyleft.** Your derivative work can be under any licence you like,
  including a proprietary one.
- ⭐ **No conditions at all.** 0BSD is the ISC licence with the attribution
  clause deleted. There is nothing left to comply with.

Paste a script into your build system, lift a patch into your package, copy a
table into your own documentation, rewrite the whole thing and put your name
on it. All explicitly fine.

## Two scope notes

**1. This covers this repository's own content.** It says nothing about the
third-party projects surveyed under [`references/`](references), each of which
carries its own licence:

| under `references/` | upstream licence |
| --- | --- |
| `archlinux__pacman/` | GPL-2.0-or-later |
| `aur__pacman-static/` | GPL-2.0-or-later (the `PKGBUILD`'s own declaration) |
| `manjaro-contrib__packages-core-pacman-static/` | GPL-2.0-or-later |
| `firasuke__mussel/` | ISC |
| `seccomp__libseccomp/` | LGPL-2.1 |

Those trees are kept as evidence so citations can be checked. They are **not**
relicensed by this file, and redistributing them carries their terms.

The same applies to the software the experiments build: pacman, OpenSSL, curl,
libarchive, gpgme and the rest remain under their own licences. A binary you
produce with these scripts is governed by those, not by this file.

**2. Facts are not copyrightable anyway.** Version numbers, measured build
times, binary sizes, repository URLs, and the observation that a `$CARCH` in
single quotes does not expand — use them freely regardless of any licence.

## Patches in `patches/`

The patches under [`patches/`](patches) are diffs against third-party source.
The **diff text** is 0BSD like everything else here. Applying one produces a
derivative of the upstream file, which stays under that file's licence:

| patch | applies to | upstream licence |
| --- | --- | --- |
| `patches/brotli-1.2.0/` | google/brotli | MIT |
| `patches/pacman/` | pacman (patch authored by Christian Hesse) | GPL-2.0-or-later |

## No warranty

This is draft research, written in one session, and
[`RESEARCH.md` §0](RESEARCH.md#0-what-this-sweep-got-wrong-about-itself)
lists four claims it got wrong about itself and says to assume more remain. It
is provided as-is.
