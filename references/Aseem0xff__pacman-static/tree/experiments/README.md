# experiments

Every measured claim in [`../RESEARCH.md`](../RESEARCH.md) was taken by a
script in this directory. The scripts are the deliverable; the numbers are
what they printed on one machine on one day.

Numbered in the order they were first run. **A number is never reused.** If a
script is replaced, the old one stays and the new one takes the next number,
because a citation of `70-` in the write-up has to keep meaning what it meant.

| script | question it answers | exit today |
| --- | --- | --- |
| `10-probe-source-hosts.sh` | which upstream source hosts answer from this network | 0 |
| `20-probe-arch-repos.sh` | does a live Arch-family repository exist for each of the five architectures | 0 |
| `30-reference-defects.sh` | do the two references cover the five targets, and where does declared coverage not hold | **1 (defects present)** |
| `40-mine-repo-joiner-defect.sh` | does the prescribed mining script deliver the comments it reports fetching | **1 (defect present)** |
| `50-zig-cross-targets.sh` | can `zig cc` produce a running static musl binary for all five targets | 0 |
| `60-fetch-sources.sh` | fetch and hash the thirteen dependency sources | 0 |
| `70-build-static-stack.sh` | does pacman's whole static dependency stack build with `zig cc` | 0 |
| `80-build-pacman.sh` | does pacman itself cross-build, link static, and run | 0 |
| `85-feature-matrix.sh` | which optional features are in each architecture's binary | 0 |
| `90-bootstrap-arch.sh` | can the resulting binary install a working Arch root, `chroot` it, and verify signatures | 0 |
| `91-segfault-control.sh` | ⛔ **superseded by `92-`** — a 2×2 factor matrix over an intermittent fault, which cannot see one | 0 |
| `92-segfault-rate.sh` | how often does `pacman -S base` end in SIGSEGV, and where | 0 |
| `95-cross-arch-bootstrap.sh` | do the foreign-architecture binaries do real work against their own distributions | 0 |

`91-` is **kept and not run**. It asks a factor question about a fault that
turned out to be intermittent, so every cell passed and the table said
nothing. Deleting it would orphan the numbers in
`out/91-segfault-control.txt`, so it stays, labelled, and `92-` replaces it.
⭐ That is the rule: a superseded instrument is kept, with the reason.

`30-` and `40-` exit **1 on purpose**. They are assertions about defects that
are present today: re-run them against a newer reference and a `0` means the
defect is gone, which is the result you want and cannot get from prose.

## Exit codes

Uniform across every script here.

| code | meaning |
| --- | --- |
| 0 | the measurement ran and the thing passed |
| 1 | the measurement ran and the thing failed |
| 2 | the measurement could not run (missing tool, missing corpus, no network) |

⛔ **`2` is never reported as a pass.** Three of these scripts were corrected
during this sweep for exactly that failure — see "what this sweep got wrong"
in [`../RESEARCH.md`](../RESEARCH.md#0-what-this-sweep-got-wrong-about-itself).

## Running them

Nothing here depends on the directory it is run from; each resolves paths
from its own location. Order matters only in one place: `70-` needs `60-`,
and `80-` needs `70-`.

```sh
cd experiments

./10-probe-source-hosts.sh          # network reachability, ~30s
./20-probe-arch-repos.sh            # repository liveness, ~20s
./30-reference-defects.sh           # offline, reads references/ only
./40-mine-repo-joiner-defect.sh     # one live fetch, then offline forever

./50-zig-cross-targets.sh           # needs zig; see its header for the fetch
./60-fetch-sources.sh               # ~250 MB of tarballs
./70-build-static-stack.sh loongarch64-linux-musl
./80-build-pacman.sh    loongarch64-linux-musl
./85-feature-matrix.sh              # after building every arch you care about

sudo ./90-bootstrap-arch.sh x86_64  # ⛔ read docs/GOTCHAS.md G-01 first
./95-cross-arch-bootstrap.sh        # foreign architectures, under qemu
./92-segfault-rate.sh 20            # the open intermittent fault
```

`70-` needs `60-`; `80-` needs `70-`; `85-`, `90-`, `92-` and `95-` need `80-`.

`90-` and `92-` need **root**, because they `chroot` and `mknod`.

Environment knobs, all optional: `WORK` (default `/home/user/work`), `SRC`,
`ZIG`, `JOBS`, `ROOT`, `PACMAN`, `ARCHES`, `NO_PATCHES`.

## Output

`out/` holds what each run printed, and it is **tracked on purpose**. A
script that deletes what it measured is the same failure as a sweep that
keeps only its conclusions.

## fixtures/

| file | what it is |
| --- | --- |
| `libc-surface.c` | the libc calls pacman actually makes — `getpwnam`, `getgrnam`, `getaddrinfo` — as a 53-line program, so a toolchain can be tested for them without building pacman |
| `mussel-issue-comments-page1.json` | one captured API page, so `40-` can re-run offline and does not depend on a third party still being up |
