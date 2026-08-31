# Windows

Thirty-eight issues touch Windows specifically. Rule 0.3 lists the traps; this
file tracks which are handled and which are not.

---

### T-070 A downloaded executable cannot be run until the process exits

Source:      https://github.com/ikatson/rqbit/issues/369 (open)
Category:    windows
Priority:    P1
Effort:      M
Status:      **done**

Problem:     A `.exe` inside a completed torrent cannot be launched while the
             session still holds a handle to it, downloading, paused, or
             finished. Windows will not let a file be executed or renamed while
             another process holds it open without sharing.
Relevance:   Rule 0.3 calls this out by name. A one-shot tool that exits after
             the download hides it, but `bit-cli seed` holds handles for the
             whole run by design.
Approach:    Two halves. The finalize half: close every payload handle before
             `download` reports completion, and retry with backoff if a close
             races. The seed half: open with `FILE_SHARE_READ | FILE_SHARE_DELETE`
             so a reader is not locked out. The second needs a storage wrapper,
             because `librqbit` opens the files.
Acceptance:  `bit-cli download <TORRENT WITH EXE>` followed immediately by
             running the executable succeeds, and the same during a concurrent
             `bit-cli seed` of the same payload.

## What it actually was

Reproduced first, on a torrent holding a real 64 KiB executable and 256 KiB of
padding, served over loopback:

```
Start-Process -FilePath out\payload\tool.exe
  This command cannot be run due to the error: The process cannot access the
  file because it is being used by another process.
```

while `bit-cli seed` was serving it. `Copy-Item`, `Rename-Item`, and
`Remove-Item` on the same file all succeeded at the same moment, which is the
clue: the share mode was not the problem. Rust's `File` already opens with
`FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`, so the approach above
was aimed at something that was already true.

The problem was the **access**, not the sharing. Loading an image asks for read
access with a share mode that does not include write, and that conflicts with
an existing handle that was granted write access. A seeder held one on every
file.

## The fix

Two changes, both in `bit_cli_core::storage`:

- A read opens for reading only. `Intent::Read` opens without `.write(true)`
  and without `.create(true)`; `Intent::Write` opens for both and upgrades a
  read-only handle in place, dropping the old one first so the two never
  coexist. A seeder only ever reads, so it never upgrades.
- `ensure_file_length` does nothing when the file is already the length asked
  for. Without this the first thing a seed did was open every file for writing
  to set the size it already had, which put the writable handle back.

Together, a complete seed touches no payload file for writing at all.

## Acceptance

```
$ bit-cli download p.torrent --web-seed $URL --web-seed-only --dir out --port 0
download exit 0
exec right after download: 0

$ bit-cli seed p.torrent --data out --port 0 --stop-after 25s     # in background
seeder running: True
EXEC OK while seeding, exit 0: C:\Windows\System32\where.exe

$ bit-cli seed p.torrent --data out --port 0 --stop-after 4s --json
seed complete True, have 320.00 KiB of 320.00 KiB

$ bit-cli verify p.torrent --data out --json
verify exit 0, complete True
```

Both halves of the acceptance pass, the payload still hashes equal to the
source, and the seed still serves the whole of it.

`storage::tests::a_read_opens_for_reading_only_and_a_write_upgrades` pins the
invariant without needing Windows: a read leaves `is_writable()` false, a write
makes it true, and the upgrade replaces the handle rather than adding one.

### T-071 Reserved device names in torrent paths are not sanitised

Source:      rule 0.3
Category:    windows
Priority:    P0
Effort:      M
Status:      **done**

Problem:     A torrent can contain `CON`, `PRN`, `AUX`, `NUL`, `COM1` to
             `COM9`, `LPT1` to `LPT9`, names ending in a dot or a space, and
             the characters `< > : " | ? *`. None can exist on NTFS. Nothing in
             `bit-cli` sanitised them.

             Found while fixing it, and worse than the item as written: a path
             component of `C:` leaves the output directory entirely.
             `Path::new("D:/out").join("C:")` is `C:`, not `D:/out/C:`, so two
             characters relocate the whole download to whatever the current
             directory of drive C happens to be. `librqbit`'s own validation
             rejects `..` and rejects `/` or `\` inside a component, and `C:`
             contains none of those, so it passed. A UNC component and a
             leading separator are caught by that validation; the drive prefix
             was not.
Relevance:   A torrent is untrusted input. This is a correctness bug and a
             security one: a path that escapes or collides is a file written
             somewhere the caller did not expect.
Approach:    `bit_cli_core::paths::plan` turns a torrent's file list into
             on-disk paths, with no I/O and no platform branch, and reports
             every change with the reason.
             `bit_cli_core::storage::SafeStorageFactory` is the session's
             storage now, so the plan is what opens the files. Sanitising runs
             on every platform, not only Windows: a payload downloaded on Linux
             and copied to a Windows machine is normal, and a layout that works
             on only one of them breaks later somewhere else.

             `bit-cli create` already refuses to build such a torrent, through
             the `windows-path` and `case-collision` lints. This is the reading
             side.
Acceptance:  A fixture torrent containing `CON.txt`, `a<b.bin`, and `x .` is
             downloaded on Windows, every file lands, and `--json` carries a
             `renamed` array mapping each torrent path to its on-disk path.

Evidence:    Run at 2026-08-19T19:26Z on Microsoft Windows 10.0.26200.

    cargo test --workspace

             `cmd::download::tests::a_hostile_torrent_reports_every_renamed_path_in_json`
             drives the whole binary in process, with no terminal, over a
             torrent carrying `C:/pwned.txt`, `CON.txt`, `a<b.bin`, `x .`,
             `README`, and `readme`. It asserts the `--json` report:

    "renamed": [
      {"index":0,"torrent_path":"C:/pwned.txt","disk_path":"C_/pwned.txt","reasons":["escape","illegal-character"]},
      {"index":1,"torrent_path":"CON.txt","disk_path":"CON_.txt","reasons":["reserved-name"]},
      {"index":2,"torrent_path":"a<b.bin","disk_path":"a_b.bin","reasons":["illegal-character"]},
      {"index":3,"torrent_path":"x .","disk_path":"x","reasons":["trailing-dot-or-space"]},
      {"index":5,"torrent_path":"readme","disk_path":"readme-1","reasons":["case-collision"]}
    ]

             and then that all six files exist on disk. `README` is absent from
             the list because it kept its name, which is the property a caller
             tests for: an ordinary torrent reports no `renamed` key at all,
             asserted by `an_ordinary_torrent_reports_no_renames`.

             `crates/bit-cli-core/tests/hostile_paths.rs` runs eight cases
             through a real session and asserts what landed on disk, including
             that the naive join still escapes on this platform, so the fixture
             cannot go stale without the test saying so. 27 unit tests in
             `paths.rs` cover the rules themselves, including that no two
             planned paths collide under case folding and that every planned
             path is relative with only normal components.

             The fixtures are built in code rather than committed as
             `.torrent` files because a repository cannot contain a directory
             called `C:` or a file called `CON.txt` on Windows, which is the
             point.

Remaining:   `--json` reports the mapping for `download`. `seed` and `verify`
             read through the same storage and so land on the same paths, but
             their reports do not carry `renamed` yet. Tracked by T-076.

### T-072 Case-colliding paths silently overwrite

Source:      rule 0.3
Category:    windows
Priority:    P0
Effort:      S
Status:      **done**

Problem:     NTFS is case insensitive by default. A multi-file torrent
             containing both `README` and `readme` writes one file twice, and
             the second write wins.
Relevance:   Silent data loss on a legal torrent. On Linux the same torrent is
             fine, so it only shows up in production.
Approach:    Detected from the file list before any byte is written, which
             needs no I/O: case-fold every planned path and look for duplicates.

             The acceptance below asked for a refusal with
             `--allow-overwrite` as the escape hatch. That was implemented as a
             rename instead, which is strictly better and needs no flag: both
             files land, neither is lost, and the mapping is reported. A
             refusal would make a legal torrent undownloadable on Windows and
             downloadable on Linux, which is the platform split the rest of
             this file exists to remove. Nothing is silent either way, which
             was the actual requirement.

             The first file to claim a name keeps it, and later ones take a
             `-1`, `-2` suffix on the stem so the extension and the directory
             survive: `disc 1/track.flac` becomes `disc 1/track-1.flac`. Since
             a torrent's file order is fixed by its info hash, the result is
             deterministic and a resumed download finds the same files.
Acceptance:  `bit-cli download <COLLIDING TORRENT>` reports both paths in
             `--json` and both files land under distinct names.

Evidence:    Same run as T-071.
             `crates/bit-cli-core/tests/hostile_paths.rs::case_colliding_paths_both_land_and_neither_is_lost`
             adds a torrent carrying `README`, `readme`, and `ReadMe`, and
             asserts the planned paths are `README`, `readme-1`, `ReadMe-2` and
             that three distinct files exist on disk. Without the plan that is
             one file written three times on NTFS and APFS, and the first two
             payloads are gone.

             `paths::tests::no_two_planned_paths_collide_under_case_folding`
             asserts the property over a mixed set including names that Windows
             would strip into each other (`x`, `x .`, `x  `), which collide
             there and nowhere else.

### T-073 Long paths are not tested

Source:      rule 0.3
Category:    windows
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `git config core.longpaths true` is set, but nothing in `bit-cli`
             tests a payload path past 260 characters, and nothing uses the
             `\?\` prefix.
Relevance:   Torrent payloads routinely exceed 260 characters once the download
             directory is prepended. Without the prefix the write fails with a
             confusing error.
Approach:    Normalise the download directory to an extended-length path on
             Windows before handing it to the session, and add a fixture with a
             deep path.
Acceptance:  A torrent whose deepest path plus the download directory exceeds
             300 characters downloads and verifies on Windows.

**Done, and the fix the approach proposes turned out not to be needed.**
`TorrentFixture::deep` is a torrent whose one file sits four directories deep,
sixty characters each, and
`a_path_past_the_classic_windows_limit_lands_and_verifies` downloads it from a
loopback server into a temporary directory, asserts the resolved path is over
300 characters, reads the payload back from exactly the path that was planned,
and then runs `verify` over the result.

**Nothing here adds a `\\?\` prefix, because Rust's standard library already
does.** `std::sys::path::windows::maybe_verbatim` converts an absolute path
past the legacy limit into its verbatim form before it reaches the Win32 call,
so every `File::open`, `create_dir_all`, and `metadata` in the storage path
gets the long form without asking. `bit-cli` supplies its own storage
(`bit_cli_core::storage`) and its own reader for `verify`, and both are built
on `std::fs`, so the whole payload path is covered by that one property.

Two things it depends on, both of which hold and are worth writing down
because a change to either would break this quietly:

- **The download directory is absolute.** `swarm::download_directory` resolves
  `--dir` against the working directory, so a relative one is absolute before
  it reaches storage. `maybe_verbatim` only converts absolute paths: a relative
  path has no length limit it can fix.
- **No component is over 255 bytes.** That is a filesystem limit rather than a
  path limit, and `paths::plan` already truncates a component past it and
  reports the rename. The fixture stays under it on purpose: this entry is
  about the total, and the per-component case is
  [T-071](#t-071-reserved-device-names-in-torrent-paths-are-not-sanitised)'s.

The same thing from the command line, on a payload written by hand at a
308 character path:

```
$ bit-cli create .tmp/deep/deep --name deep --piece-length 1KiB \
    --no-creation-date --output .tmp/deep/deep.torrent --force --json
"info_hash": "6de2f4843ffb3edc91054ca792885e2b6e0d2ed5"

$ bit-cli verify .tmp/deep/deep.torrent --dir .tmp/deep --json
"complete": true
```

The test asserts `renamed` is absent, which is the part that says the path was
written rather than shortened to fit.

### T-074 A false hash-check pass on empty files

Source:      https://github.com/ikatson/rqbit/issues/625 (closed, 2026-08-15)
Category:    windows
Priority:    P1
Effort:      S
Status:      done

Problem:     `FilesystemStorage::pread_exact` ignored the byte count returned
             by `seek_read`, so a read that returned nothing was treated as a
             read of zeroes and the hash check passed over a missing file.
Relevance:   A false verification pass is the worst class of bug in this tool.
Approach:    Verify against the pinned version rather than trusting the closed
             label.
Acceptance:  **Fixed in the pinned 9.0.0.** Verified at
             `storage/filesystem/opened_file.rs:63-74`: the Windows
             `pread_exact` now loops over `seek_read` and returns
             `ErrorKind::UnexpectedEof` when a read returns zero. Checked
             2026-08-19, and re-checked 2026-08-21 against the same registry
             copy.

**Re-checked on 2026-08-21 against the corpus, and this entry was right.**
`nanotorrent/patches/0010-windows-pread-pwrite-exact.patch` is 112 lines with
tests against **librqbit 8.1.1**, and its `PATCHES.md` states the consequence
in full: `pread_exact` called `File::seek_read` once and **discarded the byte
count**, so at or past EOF `Ok(0)` was reported as success with the caller's
buffer untouched. `FileOps::initial_check` therefore never saw a file as
missing or empty and hash-checked every piece: a fresh 6.5 GiB torrent spent
about eleven seconds SHA-1'ing files holding nothing, and any short read
hashed, streamed **or served to a peer** whatever stale bytes were in the
buffer. The same patch fixes `pwrite_all`, which re-wrote the whole buffer at
the same offset on every pass while subtracting the written count from
`remaining`, duplicating data and able to underflow.

`reference/RESEARCH.md` suggests this defect is consistent with T-074 and with
[T-015](disk-io.md), and asks which librqbit `bit-cli` pins. The answer:
**9.0.0, and both halves are already fixed there.** `pread_exact` at
`opened_file.rs:63-74` loops, advances the offset, and maps `Ok(0)` to
`UnexpectedEof`. `pwrite_all` at `:87-101` loops, advances both `buf` and
`offset`, and subtracts only what was actually written. So the 8.1.1 defect is
not present in the tree, T-074 stays **done** on the evidence it always had,
and [T-015](disk-io.md) had a different cause, recorded under its own entry.
A corpus citation is evidence of what somebody else found in the version they
read; it is not evidence about this tree, and this is the worked example.

One difference survives, and it is [T-178](#t-178-librqbits-windows-pwrite_all-can-spin-forever-on-a-zero-byte-write)
below.

### T-075 PowerShell redirection encoding is not documented

Source:      rule 0.3
Category:    windows
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T10:05Z

Problem:     On Windows PowerShell 5.1, `>` and `Out-File` write UTF-16LE,
             which breaks piping JSON into `jq`. `bit-cli` writes UTF-8 without
             a BOM to stdout regardless of the console code page, but the
             redirection trap is the caller's and needs documenting.
Relevance:   A caller who redirects `--json` to a file and then cannot parse it
             will blame the tool.
Approach:    Document the working invocations in the README: pipe directly to
             `ConvertFrom-Json`, and use `-Encoding utf8NoBOM` when redirecting
             on PowerShell 7.
Acceptance:  The README carries both forms and both have been run.

**Done, and running the two forms is what disproved the Approach.** The README
carried both of them before this entry was opened. Nobody had run either, and
the acceptance is the half that says why that matters: **one of the two does
not exist on the host it is for, and the other silently corrupts the data.**

`scripts/check-redirect.ps1` is the acceptance. It builds a torrent named
`café-λ-日本.bin`, four characters no single code page holds, runs seven forms
against it, and compares what comes back with the bytes `bit-cli` wrote. It
judges nothing and is not in the gates: what it measures is a property of the
host.

| form | 5.1 | 7.6.5 |
| --- | --- | --- |
| `cmd /c "... > file"` | exact | exact |
| `> file` | UTF-16LE, `jq` reads none of it | exact |
| `| ConvertFrom-Json` | wrong name, parses fine | wrong name, parses fine |
| `| Set-Content -Encoding utf8` | wrong name | wrong name |
| `| Out-File -Encoding utf8NoBOM` | no such value | wrong name |

**The Problem named one setting and there are two.** `[Console]::OutputEncoding`
decides how the host decodes what a program wrote, and `$OutputEncoding` decides
how it encodes what it sends into one. Neither defaults to UTF-8: measured here,
both hosts read at the console code page, `IBM437`, and 5.1 writes `us-ascii`
into a native command. That is why `| ConvertFrom-Json`, which this entry
recommended as the safe form, returns a name that is not the name.

**And it parses.** Every byte of `IBM437` maps to some character, so the
corruption produces valid JSON with a valid string in it. There is no error, no
replacement character in the structure, and no exit code. A caller checking that
`ConvertFrom-Json` succeeded has checked nothing about the bytes.

**`utf8NoBOM` is not a value 5.1 has.** It arrived in PowerShell 6. The host
answers with a parameter validation error listing the eight it does have, so
that form fails loudly rather than quietly, which makes it the better of the
two failures.

**What the README says now**: set both encodings once per session, or keep the
bytes out of the pipeline with `cmd`'s redirection, which copies and decodes
nothing. Both are measured on both hosts, in the same table, with the command
that reproduces it.

```
$ pwsh -NoProfile -File scripts/check-redirect.ps1
host 7.6.5 Core, console reads ibm437, writes utf-8 into a program
> file                                       yes
cmd /c "... > file"                          yes
| ConvertFrom-Json                           NO     it parsed, and the name is not the name
| ConvertFrom-Json, encodings set            yes

$ powershell -NoProfile -File scripts/check-redirect.ps1
host 5.1.26100.9168 Desktop, console reads IBM437, writes us-ascii into a program
> file                                       NO     jq: parse error: Invalid numeric literal at line 1, column 3
cmd /c "... > file"                          yes
| ConvertFrom-Json                           NO     it parsed, and the name is not the name
| ConvertFrom-Json, encodings set            yes
```

Both runs are committed: `bench/redirect-pwsh7.json` and
`bench/redirect-ps51.json`.

**A second redirection trap, and it has moved.** Every `check-*.ps1` in this
repository runs `bit-cli` through `Start-Process` with redirect files rather
than calling it directly, and the reason on record is that under
`$ErrorActionPreference = 'Stop'` a native command writing to stderr is a
terminating error in `pwsh` 7. **That is no longer true on this machine, and
the change is upstream rather than local.** Measured on 2026-08-21:

```powershell
pwsh -NoProfile -Command "$ErrorActionPreference='Stop'; try { & pwsh -NoProfile -Command \"[Console]::Error.WriteLine('x'); exit 0\" 2>&1 | Out-Null; \"survived, LASTEXITCODE=$LASTEXITCODE\" } catch { \"TERMINATED\" }"
```

```
PSVersion: 7.6.5
PSNativeCommandUseErrorActionPreference: False
survived, LASTEXITCODE=0
```

`$PSNativeCommandUseErrorActionPreference` is the switch, it defaults to
`False` from PowerShell 7.4, and it was `True` in the 7.2 and 7.3 range where
the rule was written. So the behaviour depends on the host's `pwsh` version,
which is exactly the reason to keep the `Start-Process` pattern: a script that
works here and terminates on a 7.3 runner is worse than one that does neither.
The pattern stays. What changes is the reason given for it: it is not that
stderr always terminates, it is that whether it terminates is not this
repository's to decide.

### T-076 seed and verify do not report renamed paths

Source:      found here, 2026-08-19, while closing T-071
Category:    windows
Priority:    P2
Effort:      S
Status:      **done**

Problem:     `bit-cli download --json` carries a `renamed` array naming every
             file whose on-disk path is not the path in the torrent. `seed` and
             `verify` go through the same storage and so read and write the
             same paths, but neither report carries the mapping.
Relevance:   A caller seeding a payload whose paths were rewritten cannot tell
             from the report which file on disk is which file in the torrent.
             The data is correct; the reporting is incomplete, which is a
             headless parity gap under rule 0.11.
Approach:    `Engine::path_plan` already returns it. `SeedReport` and the
             verify report each need a `renamed` field populated the same way
             `download.rs::renames` does, plus a line in the text rendering.
Acceptance:  `bit-cli seed --json` and `bit-cli verify --json` over the hostile
             fixture both carry a `renamed` array equal to the one
             `bit-cli download --json` reports for the same torrent.

Both carry it now, and both render it in text. `seed` reads it from
`Engine::path_plan`, the same source `download` uses.

**`verify` was worse than the entry said.** It did not go through the same
storage at all: `PayloadReader::path_of` joined the torrent's own path
components onto the data directory, so on a hostile torrent it read paths that
do not exist and reported every file missing. On Windows a `C:` component would
have sent it looking outside the data directory entirely, the same way the
download path could before [T-071](#t-071-reserved-device-names-in-torrent-paths-are-not-sanitised)
was fixed. It runs the plan now and reads where the bytes actually went.

Acceptance:

```
$ cargo test -p bit-cli --lib renamed
$ cargo test -p bit-cli --lib verify_reads_the_planned_paths_and_reports_the_mapping
$ cargo test -p bit-cli --lib a_seed_of_a_hostile_torrent_reports_every_renamed_path
```

All three assert the same five pairs against the hostile fixture:

```
C:/pwned.txt  ->  C_/pwned.txt     escape
CON.txt       ->  CON_.txt         reserved-name
a<b.bin       ->  a_b.bin          illegal-character
x .           ->  x                trailing-dot-or-space
readme        ->  readme-1         case-collision
```

and the ordinary torrent carries no `renamed` key at all, so a caller can test
for its absence rather than comparing every path.

### T-147 The rename reason differed by host, so two tests only passed on Windows

Source:      CI run 32407214253, `Test (ubuntu-latest)`, 2026-08-20
Category:    windows
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `cmd::download::tests::a_hostile_torrent_reports_every_renamed_path_in_json`
             and `cmd::verify::tests::verify_reads_the_planned_paths_and_reports_the_mapping`
             fail on `ubuntu-latest` and pass on `windows-latest`:

             ```
             assertion `left == right` failed
               left: String("illegal-character")
              right: "escape"
             ```

             `paths::is_escape` decided whether a component would leave the
             output directory by running it through `std::path::Path`, which
             reads its input the way the **host** platform does.
             `Path::new("C:")` is a drive prefix on Windows and an ordinary
             file name on Linux. So the hostile fixture's `C:/pwned.txt`
             landed at `C_/pwned.txt` on both, for `escape` on one and only
             `illegal-character` on the other.
Relevance:   The module says in its own first paragraph that the plan is
             deterministic on every platform, and the disk paths were. The
             report was not, and `reasons` is in `--json` for a caller to
             branch on: a script that treats `escape` as the case worth
             refusing sees nothing to refuse on Linux. It also made the two
             tests platform-specific without saying so, which is what kept
             `Test (ubuntu-latest)` red.
Approach:    Write the three escaping shapes out rather than asking
             `std::path`: `..`, an ASCII letter followed by a colon, and a
             leading backslash. That is what `Path::components` was being
             asked for, minus the host's opinion.
Acceptance:  `paths::tests::the_escaping_shapes_are_the_same_on_every_host`
             passes, and both named tests pass on `ubuntu-latest` and on
             `windows-latest` in the same run.

The new test names nine shapes that escape and five that do not, so the
boundary is written down rather than inferred: `a:b:c` escapes because `a:` is
a drive designator whatever follows it, and `1:x` and `::x` do not because
neither starts with a letter. Every one of those matches what the Windows
parser gave before, so for them this is the same behaviour made portable.

**One answer did change, and it changed to the right one.** A component with a
backslash inside it, `foo\bar`, was an escape on Windows before, because
`Path::components` splits it into two and the old rule read anything but a
single `Normal` as an escape. It is not an escape: joined onto the output
directory it lands inside it. What is wrong with it is the separator, which is
`illegal-character`, and that is now the only reason reported. `librqbit`'s own
validation rejects a separator inside a component before this is reached, so
the case is belt and braces either way, but the reason it reports is now the
true one.

The two `bit-cli` tests were not touched. They asserted the right answer; the
planner gave the wrong one on one platform.

---

### T-178 librqbit's Windows pwrite_all can spin forever on a zero-byte write

Source:      `reference/RESEARCH.md` section D, checked against the pinned crate 2026-08-21
Category:    windows
Priority:    P3
Effort:      S
Status:      **done** 2026-08-23T09:35Z

Problem:     `librqbit` 9.0.0's Windows `pwrite_all`, at
             `storage/filesystem/opened_file.rs:87-101` in the registry copy,
             is:

             ```rust
             let mut remaining = buf.len();
             while remaining > 0 {
                 let written = self.seek_write(&buf[..remaining], offset)?;
                 remaining -= written;
                 offset += written as u64;
                 buf = &buf[written..];
             }
             ```

             The loop is correct for a short write, which is the defect
             [T-074](#t-074-a-false-hash-check-pass-on-empty-files) records as
             fixed. It has no guard for `written == 0`. `WriteFile` returning
             success with zero bytes written is rare and is not impossible: a
             full volume, a disconnected network share, or a filter driver can
             all produce it. When it happens `remaining` never decreases and
             the loop **never terminates**, on the thread that owns that write.
Relevance:   P3 because the trigger is rare, and worth an entry because of what
             it looks like when it fires. A hung write thread on a seeding box
             is not a crash and not an error: the process stays up, keeps
             answering, and quietly stops making progress on one file. That is
             the same signature as [T-037](performance.md), a run that stalls
             for minutes, and the same signature as
             [T-020](peers.md)'s listener that accepts and never answers. Two
             of this project's three hardest bugs so far have had exactly this
             shape, and the lesson each of them wrote down is that an outage no
             health check sees costs more to find than it costs to prevent.

             `bit-cli` is Windows-first, and this is on the payload write path
             for every download on the platform it targets.
Approach:    `nanotorrent/patches/0010-windows-pread-pwrite-exact.patch` maps
             `Ok(0)` to `std::io::ErrorKind::WriteZero`, which is the standard
             library's own convention for exactly this case and is what
             `write_all` does. Its three tests are the shape to copy:
             `pread_exact_fails_on_empty_file`,
             `pread_exact_fails_past_eof_and_leaves_no_stale_bytes`, and
             `pwrite_all_advances_the_offset`.

             This is a one-line upstream change and `bit-cli` cannot make it,
             so the question is what to do here. Two options, and the second is
             the one to take.

             Reporting it upstream is the clean fix and is out of `bit-cli`'s
             hands. Meanwhile `bit-cli` already owns a storage wrapper, the
             one [T-010](disk-io.md), [T-011](disk-io.md) and
             [T-013](disk-io.md) were closed with, which opens a payload file
             on first touch and holds the descriptor pool. A write that has
             made no progress for a bounded time is detectable there, and
             `bit-cli` already has the vocabulary for it: exit code 16, a
             resource ceiling the caller set, which `--max-handles` uses.
Blocker:     Not blocked, but not worth building alone. It shares its whole
             mechanism with [T-018](disk-io.md), which is about the write path
             issuing one operation per 16 KiB block, and anything that batches
             or coalesces writes there is the natural place to put a
             no-progress guard. **That place now exists**: T-018 built
             `Coalescer` on 2026-08-22 and every write to the device goes
             through `SafeStorage::write_through`, which is one function and
             is where the guard belongs.
Acceptance:  A test double whose write returns `Ok(0)` causes the run to fail
             with a named error inside a bounded time rather than hanging, and
             the error says which file and offset. Windows only, so the test is
             `#[cfg(windows)]` and the guard is in `bit-cli`'s wrapper rather
             than conditional on a librqbit version.

**Done, and two of the premises above are wrong.** Both were measured before a
line was written, which is what [RULES.md](RULES.md) section 5 asks for when an
entry describes what the code already does, and between them they invert what
the work was.

**"This is a one-line upstream change and `bit-cli` cannot make it" is stale.**
It was true when the entry was written on 2026-08-21 and stopped being true on
2026-08-22, when `librqbit` was vendored. The loop is
`vendor/rqbit/crates/librqbit/src/storage/filesystem/opened_file.rs:94`, in this
repository, and it now carries the guard. That is
[`patches/UPSTREAM.md`](../patches/UPSTREAM.md)'s eighteenth section and patch
`patches/rqbit/0014-crates-librqbit-src-storage-filesystem-opened_file.rs.patch`.

**"This is on the payload write path for every download" is false**, and it is
the more useful correction. There is exactly one `add_torrent` call in the
workspace, `crates/bit-cli-core/src/engine.rs:760`, and it installs
`SafeStorageFactory` on every add, so every payload byte goes through
`crates/bit-cli-core/src/storage.rs` and none through the vendored file. The
copy that does run **already refused a zero-length write**, with the same
`WriteZero` the entry's Approach proposes, since commit `3203d4c` on
2026-08-20: the day before this entry was filed, from work that had nothing to
do with it. An entry written from a crate the tool does not call described a
defect the tool did not have.

**What was actually missing was the half the Acceptance names.** The error said
`the write made no progress` and nothing else, so a caller learned which file
from the wrapper around it and never learned where in the file. It now reads
`cannot write to <path>: the write made no progress at offset 65536, with 8
bytes left`, and the offset is the part that says whether the write was at the
start of a piece or in the middle of one.

**And there was no test, because a real file cannot be asked to return
`Ok(0)`.** The loop takes the write as an argument now,
`write_all_positioned` beside `read_exact_positioned` in
`crates/bit-cli-core/src/storage.rs:1565`, so a double can. Five tests:
the write that makes no progress, the read that returns nothing, a short write
and a short read each continuing from the offset they reached, and the one that
drives `SafeStorage::with` so the message a caller sees carries the path and
the offset together.

**The bound is a call count rather than a clock.** The guard's whole purpose is
that the loop cannot ask again, so `calls == 1` is the condition, and
[RULES.md](RULES.md) section 5's rule about a test that waits on a guessed
duration does not get a chance to bite.

**The test was run against the defect, and this is what that costs.** With the
`0` arm replaced by `continue`, the test does not fail: it hangs, and was still
running when it was killed at 90 seconds. That is the signature the Relevance
above describes, a thread that stops making progress without failing, and it is
why the assertion is on the call count instead of on the error alone.

```
$ cargo test -p bit-cli-core --lib storage::
test result: ok. 37 passed; 0 failed; 0 ignored; 668 filtered out
```

### T-216 A seeder test waited longer for a listener than the run was allowed to live

Source:      CI run 32627489685, `Test (ubuntu-latest)`, 2026-08-23
Category:    ci
Priority:    P1
Effort:      S
Status:      **done** 2026-08-23T08:32Z

Problem:     `a_peer_that_leaves_is_reported_with_a_reason_and_a_time` runs
             `bit-cli seed --stop-after 15s` and, on a thread beside it, waits
             up to **20 seconds** for the listener before handshaking. The two
             numbers have to be ordered and were not: a slow start takes the
             listener away before the peer reaches it, and the peer returns
             `false`.
Relevance:   It turned `Test (ubuntu-latest)` red on a commit that changed one
             assertion in a `bench webseed` test and nothing else. And the
             failure said `the peer never completed a handshake, so nothing
             disconnected`, which is true of a port that was never bound, a
             connect that was refused and a read that was cut short, so a
             reader of the red job could not tell which.
Approach:    Order the two, and name the failure. The peer's patience is a
             named constant well inside the run's deadline, and the thread
             returns a `Result<(), String>` so the panic says which step failed
             and with what.
Acceptance:  The peer cannot outlast the run, and a failure names the step.

**Done.** `PEER_PATIENCE` is 10 seconds and `--stop-after` is 20, so the run
outlives the peer by a factor of two whatever the runner is doing. The thread's
three failure paths each carry their own message: no listener within the
patience, a connect that was refused with the OS error, and a read cut short.

**What is not fixed, and it is the residual.** `free_port` binds a port to learn
its number and drops the listener, so there is a window where the number is
known and nothing is listening, and another test can take it. That is
[T-160](cli-surface.md)'s own finding and this test rests on the same mechanism
at `crates/bit-cli/src/cmd/seed.rs:1152`. Ordering the deadlines removes the
failure mode that was observed; a port taken in that window would still fail,
and would now say `no listener on port N within 10s: the seeder never bound it`,
which is the sentence that would identify it.

```
$ cargo test -p bit-cli --lib a_peer_that_leaves_is_reported
test result: ok. 1 passed; 0 failed; 0 ignored; 410 filtered out
```

### T-217 The text gate caught one control byte and not the other twenty-eight

Source:      found while writing T-161's check, 2026-08-23
Category:    ci
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T08:39Z

Problem:     `gates.ps1`'s `text` gate searched tracked text files for a NUL
             and nothing else. A `0x08` backspace reached
             `scripts/check-todo.ps1` the same way `TODO/trackers.md`'s NUL
             did, from a Python `\b` escape interpreted on its way to the file,
             and the gate said `text ok` on the run that carried it.
Relevance:   The backspace landed **inside a regular expression**,
             `'^###\s+(T-\d+)\b'`, where it became a byte the pattern requires
             and nothing has. The check written that afternoon then matched no
             entry at all and passed every file, silently. A control byte is
             invisible in every editor, so nothing about reading the code says
             which of the two it is.
Approach:    Every C0 byte rather than one of them. Tab, newline and carriage
             return are text; nothing else below 32 is ever typed on purpose
             into a source file.
Acceptance:  The gate names the file, the offset and the byte, and the tree is
             clean of all of them.

**Done, and widening it found three more the old gate had passed.**

| File | Byte | What it was |
| --- | --- | --- |
| `scripts/check-todo.ps1` | `0x08` | the one that started this, in a regex |
| `scripts/gates.ps1` | `0x08` | in the comment written to explain the first |
| `TODO/windows.md` | `0x08` | `foo\bar` in prose, the backslash interpreted |
| `crates/bit-cli-core/src/mse/handshake.rs` | `0x13` | twice: the BitTorrent handshake's length byte, written as itself in `b"..."` rather than as `\x13` |

The last is the same defect [RULES.md](RULES.md) section 5 already records for
`TOLERATED_TRAILING` in `torrent/bencode.rs`, in a file written after that rule
was. Two of the four were written by the session that widened the gate, which is
the argument for the gate: the rule was known, written down, and broken twice in
one afternoon anyway.

The message names the offset and the byte now, where it used to say "NUL byte"
whatever it found.

### T-221 A seeder fixture treated a bound port as a session ready to answer

Source:      CI run 32637997195, `Test (ubuntu-latest)`, 2026-08-23
Category:    ci
Priority:    P1
Effort:      S
Status:      **done** 2026-08-23T12:20Z

Problem:     `a_peer_that_leaves_is_reported_with_a_reason_and_a_time` waits for
             the seeder's listener, then connects **once** and reads the
             handshake back. A bound port is not a session ready to answer for
             that info hash: the seeder binds before the torrent is live, so an
             early connect is accepted and dropped, and `read_exact` sees the
             close as `failed to fill whole buffer`.
Relevance:   It turned `Test (ubuntu-latest)` red on a push that moved a gate
             in `scripts/gates.ps1` and changed no source at all. That is the
             sixth test of this kind, after [T-148](bench.md),
             [T-160](cli-surface.md), [T-162](webseed.md),
             [T-215](webseed.md) and [T-216](#t-216-a-seeder-test-waited-longer-for-a-listener-than-the-run-was-allowed-to-live),
             and it is the same rule every time:
             [RULES.md](RULES.md) section 5, a test waits on the condition and
             never asserts the machine cannot fail some other way.

             **[T-216](#t-216-a-seeder-test-waited-longer-for-a-listener-than-the-run-was-allowed-to-live)
             is the near miss worth reading.** It fixed this test three hours
             earlier, ordered the two deadlines, and gave the peer thread three
             named failure paths. The failure it did not have is the fourth:
             the connect and the handshake both succeed and the seeder hangs up
             anyway, because the listener is up and the session is not.
Approach:    Retry the whole attempt, inside the same patience, and return on
             the first handshake that completes. The condition the test is
             about is a completed handshake, not an accepted connection, and
             waiting on the wrong one is what made a bound port look like
             success.
Acceptance:  The peer retries until the deadline it already had, the failure
             names the last attempt, and one attempt succeeding is what ends
             the wait.

**Done.** The connect, the send and the read back are one closure now, called
until it succeeds or the patience runs out, with a 100 ms gap. The message on
failure carries the last attempt's reason, so the four cases stay
distinguishable: no listener, a refused connect, a cut-short read, and none of
them completing in time.

**What is not changed, and why.** The seeder still binds before it is ready to
answer, and this entry does not make it stop. That is what a listening socket
means in the accept-then-check design [T-020](peers.md) closed on, and a fixture
that assumed otherwise is the thing that was wrong.

```
$ cargo test -p bit-cli --lib a_peer_that_leaves
test result: ok. 1 passed; 0 failed; 0 ignored; 427 filtered out
```
