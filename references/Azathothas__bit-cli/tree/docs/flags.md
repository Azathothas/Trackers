# Short flags

Short flags are scarce and a wrong one is worse than none, because a script
written from `aria2` muscle memory will silently do something else.

Two rules:

1. A short flag is assigned only where `aria2` already assigns that letter to
   the same concept, or where the letter is unclaimed by `aria2` and the
   meaning is obvious.
2. An `aria2` letter is never reassigned to a different concept.

`crates/bit-cli/src/cli.rs` carries a test,
`every_short_flag_is_documented_in_the_flags_table`, that reads this file's
table and fails when the two disagree, **in both directions**: a short flag with
no row, and a row for a short flag the binary no longer defines. Two more tests
sit beside it: `no_short_flag_is_defined_twice` rejects a letter used twice in
one command, and `short_flags_never_contradict_aria2` rejects an `aria2` letter
reassigned to a different concept.

The "Assigned" table below is regenerated rather than edited by hand when a flag
is added or removed:

```bash
BIT_CLI_UPDATE_FLAGS=1 cargo test -p bit-cli --lib short_flag
```

That **merges**, it does not render. Three of the five columns, `Scope`,
`aria2` and `Note`, are things the command tree does not know, so an existing
row is kept exactly as written and a new flag gets a row with those three cells
empty for a person to fill in. A row whose flag no longer exists is dropped, and
if the letter should stay reserved, move it to the section below by hand. See
`TODO/cli-surface.md`, T-118 and T-158.

`-h` is not in the command tree the test walks: `clap` creates `--help` while
building a command and the test walks an unbuilt one. It is added by hand there
so this file's row for it is checked like any other.

## The -v and -V question

`aria2` uses `-v` for `--version` and `-V` for `--check-integrity`. Most modern
tools use `-v` for verbosity.

**`bit-cli` uses `-v` for verbosity and gives `--version` no short form.**

`-V` keeps its `aria2` meaning, `--check-integrity`. Taking `-V` for
`--version` would have been the other way to resolve it, and it is the worse
one: it reassigns an `aria2` letter to a different concept, which rule 2
forbids. So `bit-cli -V` on a torrent means "check the integrity of what is on
disk", exactly as `aria2c -V` does, and `bit-cli --version` is spelled in full.

`clap` assigns `-V` to `--version` by default, so `disable_version_flag` is set
in `cli.rs` to stop it.

## Assigned

| Flag | Long form | Scope | `aria2` | Note |
| --- | --- | --- | --- | --- |
| `-c` | `--continue` | download | `-c` continue | Same concept. |
| `-d` | `--dir` | global | `-d` dir | Same concept. |
| `-h` | `--help` | global | `-h` help | Universal. |
| `-j` | `--max-concurrent-downloads` | download | `-j` max concurrent downloads | Same letter, narrower meaning: `bit-cli` has no queue across invocations, so this is parallelism inside one run. |
| `-k` | `--min-split-size` | download | `-k` min split size | Same concept, as a **floor** under `--web-seed-chunk-size` rather than a value. |
| `-l` | `--log-file` | global | `-l` log | Same concept. |
| `-O` | `--index-out` | download, verify, seed | `-O` index out | Same concept, zero-based like every other index flag here. The path is a request: it is sanitised and disambiguated like a torrent path, so it cannot escape the output directory. The three commands are one payload read three ways: `download` writes it there, `verify` reads it back from there, and `seed` serves it from there. |
| `-o` | `--out` | download | `-o` out | Same concept. |
| `-o` | `--output` | create, edit, magnet, man | unclaimed in this position | `-` means stdout, following `intermodal`. |
| `-q` | `--quiet` | global | `-q` quiet | Same concept. |
| `-s` | `--split` | download | `-s` split | Same concept, and the **same knob as `-x`** here. See below. |
| `-u` | `--max-upload-rate` | download, seed | `-u` max upload limit | Same concept. |
| `-V` | `--check-integrity` | download | `-V` check integrity | Same concept. See above. |
| `-v` | `--verbose` | global | `-v` version | **Deliberate divergence.** See above. |
| `-x` | `--max-connection-per-server` | download | `-x` max connection per server | Same letter, **per source rather than per server**. See below. |

### The three that are close and not exact

`-x`, `-s` and `-k` are the flags a script written from `aria2` muscle memory
already passes, and all three are accepted. Two of them do not mean quite what
they mean there, and `bit-cli` says so rather than hiding it, which is what
rule 2 asks for when a letter cannot be refused outright.

**`-x` caps per source, not per server.** In `aria2` it holds connections to
one server; here it holds concurrent ranged requests to one source. They differ
when two sources share a host: `-x 4` with two such sources is eight requests
to that host. `--web-seed-max-total` is the run-wide cap when that matters. A
warning says this on first use.

**`-x` and `-s` are one knob.** `aria2` splits a file into `-s` ranges and caps
per-server connections at `-x` separately. There is one setting here, so the
two are spellings of it and the **larger given wins**: `-x 4 -s 16` is sixteen
concurrent requests per source, not sixty-four. A warning says so when the two
differ.

**`-k` is a floor.** It raises `--web-seed-chunk-size` and never lowers it, so
`-k 1MiB` against the 4 MiB default changes nothing.

The number behind taking them at all is measured, not assumed: 940 MiB/s at one
concurrent request, 3.44 GiB/s at eight, and past eight throughput stops while
p99 doubles. `bench/split-20260823T182709577Z.json` is the curve.

## Reserved and not assigned

These letters mean something specific in `aria2`. `bit-cli` does not use them
yet, and when it does they keep the meaning below.

| Flag | `aria2` meaning | Status here |
| --- | --- | --- |
| `-D` | daemon | Never. Phase C, decision 7.4. |
| `-i` | input file | Reserved. `TODO/cli-surface.md` T-114. |
| `-M` | metalink file | Reserved. A Metalink is a positional source here, the way `.torrent` is: `bit-cli download release.meta4`. |
| `-m` | max tries | Reserved. |
| `-P` | parameterized uri | Reserved. |
| `-R` | remote time | Reserved. |
| `-S` | show files | Reserved. `bit-cli files` covers it as a verb. |
| `-T` | torrent file | Reserved. `bit-cli` takes a positional source instead. |
| `-t` | timeout | Reserved. `--timeout` is global and long-only for now. |
| `-U` | user agent | Reserved. |
| `-Z` | force sequential | Reserved. |

## Unclaimed by aria2

Free to use where the meaning is obvious. None is assigned today:

```
-A -B -C -E -F -G -H -I -J -K -L -N -Q -W -X -Y
-a -b -e -f -g -n -p -r -w -y -z
```
