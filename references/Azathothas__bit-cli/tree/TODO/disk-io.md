# Disk I/O

Thirty-seven issues in the upstream corpus touch storage. These are the ones
that change what `bit-cli` has to do.

---

### T-010 pwrite takes a read lock where it needs a write lock

Source:      https://github.com/ikatson/rqbit/issues/502 (closed, 2025-10-21)
Category:    disk-io
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `FilesystemStorage::pwrite_all` and `pwrite_all_vectored` take
             `lock_read()` on the opened file. The lock exists to keep other
             threads out while the file handle is being swapped; taking the
             read half lets two writers proceed at once.
Relevance:   **Not fixed in the pinned 9.0.0.** Verified in
             `storage/filesystem/fs.rs:69-88`: both writers still call
             `lock_read()` on non-Windows. `lock_write` exists in
             `opened_file.rs:169` and is marked `#[allow(dead_code)]`, so it is
             defined and unused. On Windows the path goes through
             `try_mark_sparse()` instead, which also returns a read guard.
Approach:    The guard protects an `Option<File>` swap, not the file contents,
             and `pwrite` at an offset is safe against itself at a different
             offset. So this may be benign in practice. Establish which it is
             before doing anything: read what the lock protects, then either
             switch to `lock_write` or record why the read half is correct.
Acceptance:  This entry states, with the line numbers, whether concurrent
             `pwrite_all` calls through one `OpenedFile` can interleave
             destructively. If they can, `bit-cli` carries a storage wrapper
             that serialises them and a test that fails without it.

**Concurrent `pwrite_all` calls cannot interleave destructively, and the read
half is the correct one.** `bit-cli` supplies its own storage and does not use
`FilesystemStorage`, so the finding is about `crate::storage::Slot`, which has
the same shape by design.

What the guard protects, in `crates/bit-cli-core/src/storage.rs`:

- `Slot::file` is a `RwLock<Option<File>>`. The `Option` is the thing under the
  lock, not the file's contents.
- The only writers of that `Option` are `Slot::close`, `Slot::take`, and the
  open in `SafeStorage::ensure_open`. All three take the write half, so a
  handle can never be swapped while a read or a write is using it.
- Every read and write is positioned. `pwrite_all` is `write_all_at` on Unix
  and a `seek_write` loop on Windows, and `pread_exact` is `read_exact_at` and
  a `seek_read` loop. None of them uses the file's cursor, so two of them at
  different offsets do not affect each other, and two at the same offset were
  already a caller bug that no lock can fix.

On Windows `seek_write` does move the file's cursor as a side effect. That
would matter to a cursor-based reader, and there is not one: nothing in this
storage calls `read`, `write`, or `seek`.

Taking the write half instead would serialise every write on a file to one at a
time, which is the opposite of what the storage is for: reads and writes are
addressed by index and offset precisely so several pieces can be in flight
against one file.

The test is `storage::tests::concurrent_positioned_writes_to_one_file_do_not_interleave`:
eight threads, sixty-four separate 64 KiB writes into one file at interleaved
offsets, then every block is checked for the byte its writer owned. It fails if
a write ever lands inside another one.

```
$ cargo test -p bit-cli-core --lib storage
test result: ok. 20 passed; 0 failed
```

### T-011 No file handle pool, so long runs exhaust descriptors

Source:      https://github.com/ikatson/rqbit/issues/520 (closed, 2026-01-17)
Category:    disk-io
Priority:    P1
Effort:      M
Status:      **done**

Problem:     `FilesystemStorage` opens one handle per file and keeps it. A
             reporter measured 5,194 open handles and 8.6 GB RSS after six and
             a half days.
Relevance:   The netdisk deployment seeds many torrents from one process. A
             torrent with 20,000 files is one torrent; ten of them is 200,000
             handles.
Approach:    `--max-open-files` parses today and does nothing. Implement it as
             an LRU over opened handles in a storage wrapper, closing the least
             recently used file when the cap is reached. Measure RSS and handle
             count before and after over a long seed.
Acceptance:  `bit-cli seed <TORRENT> --max-open-files 64` on a torrent with
             more than 64 files keeps the process below 64 payload handles for
             the whole run, measured with `Get-Process | Select-Object
             HandleCount` on Windows and `/proc/<pid>/fd` on Linux.

`--max-open-files` did not parse at all; the entry was wrong about that. It
exists now, on `download` and on `seed`, and it does something.

`SafeStorage` opens a payload file when it is first touched rather than when
the torrent is added, and `OpenSet` keeps the handles ordered so the least
recently opened closes when the cap is reached. The default is 128, chosen to
sit under the 512 stream limit a Windows CRT allows and far under a typical
Linux `RLIMIT_NOFILE` of 1024, so the default never runs a process out on its
own.

The order is by open rather than by access, deliberately. Recording an access
would mean taking the set's lock on every read and write, which costs more than
it saves: the expensive event is opening a handle, and the least recently
opened file is the one least recently needed, both for a download walking
pieces and for a seeder answering requests.

Two guards are never held at once. A slot's read guard is dropped before the
eviction runs, and the eviction takes each victim's write guard on its own, so
two threads evicting each other's file cannot deadlock.

Acceptance, `scripts/check-handles.ps1`, 300 files of 16 KiB, seeded for twelve
seconds at each cap with the process handle count sampled every 200 ms,
2026-08-20T00:54:40.625Z. Report: `bench/handles-20260820T005440625Z.json`.

```
$ pwsh -NoProfile -File scripts/check-handles.ps1

cap peak_process_handles complete
--- -------------------- --------
  8                  195     True
 64                  251     True
128                  315     True

cap 8 to 64: 56 more handles, cap grew by 56
cap 64 to 128: 64 more handles, cap grew by 64
```

The absolute count includes everything else the process holds: threads,
sockets, and libraries. That part is the same whatever the cap is, so it
cancels, and what is left is exactly one handle per payload file the cap
allows. A step of 56 in the cap moves the handle count by 56, and a step of 64
moves it by 64. Before this, 300 files meant 300 handles and the flag did not
exist.

`storage::tests::the_handle_cap_closes_the_least_recently_opened_file` asserts
the invariant directly: eight files, a cap of three, and the open count never
exceeds the cap while every file is still written correctly.
`a_reopened_file_reads_back_what_was_written_before_it_was_closed` proves a
closed file is reopened rather than lost.

### T-012 Preallocation is not implemented

Source:      https://github.com/ikatson/rqbit/issues/412 (open)
Category:    disk-io
Priority:    P2
Effort:      M
Status:      **done**

Problem:     `--file-allocation none|prealloc|sparse|falloc` parses and is
             carried through the config, but nothing acts on it. `librqbit`
             calls `set_len` and relies on the filesystem, which produces a
             sparse file on NTFS and ext4 and a fully allocated one elsewhere.
Relevance:   Rule 0.3 requires an explicit allocation strategy. On a netdisk
             the difference between sparse and preallocated is whether a
             half-finished 40 GB torrent shows as 40 GB of committed space.
Approach:    Four real behaviours: `none` writes nothing up front, `sparse`
             marks the file sparse (`FSCTL_SET_SPARSE` on Windows, the default
             on ext4), `prealloc` writes zeroes, `falloc` calls
             `posix_fallocate` on Linux and `SetFileValidData` on Windows.
             Windows `SetFileValidData` needs `SeManageVolumePrivilege`, so it
             has to degrade to `prealloc` with a warning rather than fail.
Acceptance:  For each method, `bit-cli download --file-allocation <M>
             --dry-run=false` on a 1 GiB torrent, then the on-disk size
             reported by `fsutil file layout` on Windows and `du --apparent-size`
             against `du` on Linux, both recorded here.

`bit_cli_core::alloc` implements all four, and `SafeStorage::ensure_file_length`
is where they run, because that is the first thing the session does to a file it
intends to use.

| Method | What happens |
| --- | --- |
| `none` | `set_len` and nothing else. |
| `sparse` | `FSCTL_SET_SPARSE` through `DeviceIoControl`, then `set_len`. Marking comes first because punching a hole into a file that is already long is a different operation on some filesystems. |
| `prealloc` | `set_len`, then zeroes written across the whole file in 1 MiB chunks, then `sync_all`. Without the sync the space is a page cache full of zeroes that a full disk refuses later. |
| `falloc` | `posix_fallocate` on Linux. |

`falloc` on Windows degrades to `prealloc` and says so. `SetFileValidData` is
the equivalent call and it needs `SeManageVolumePrivilege`, which an ordinary
process does not hold; it also exposes whatever was previously on those disk
blocks until they are written, which is why the privilege exists. Asking for the
privilege would be the wrong trade for a download tool, so the fallback is the
answer and the warning is how the caller finds out.

Acceptance, `scripts/check-allocation.ps1`, 512 MiB payload on NTFS,
2026-08-20T00:52:50.659Z. Report: `bench/allocation-20260820T005250659Z.json`.

The measurement that separates the methods is taken **before any payload
arrives**: the torrent is added against a source that answers nothing, so the
files are created and sized and nothing is downloaded. That is the state the
question is about, and volume free space either side of it is the number a
capacity plan is made from.

```
$ pwsh -NoProfile -File scripts/check-allocation.ps1 -PayloadSize 512MiB

method    reserved    allocated  sparse  volume gave up  payload
none      512.00 MiB  512.00 MiB  False      511.96 MiB  matches
sparse    512.00 MiB        0 B    True      114.48 MiB  matches
prealloc  512.00 MiB  512.00 MiB  False      637.39 MiB  matches
falloc    512.00 MiB  512.00 MiB  False      514.00 MiB  matches
```

Three things this says:

- `sparse` reserves nothing. A 512 MiB file costs the volume 114 MiB, and that
  114 MiB is other activity on a live machine rather than the file. Every other
  method costs the volume the whole 512 MiB.
- **`none` is not sparse on NTFS.** The Problem above assumed `set_len`
  produces a hole on NTFS the way it does on ext4. It does not: it allocates.
  So on Windows `sparse` is the only way to get a hole, and the two methods are
  genuinely different rather than two names for one behaviour.
- `falloc` degraded, and said so on stderr:

```
warning: --file-allocation falloc is not available here, so prealloc was used
instead: SetFileValidData needs SeManageVolumePrivilege, which this process
does not hold
```

All four produce a payload whose SHA-256 matches the source. An allocation
method that loses data would be worse than one that reserves nothing, so that
is checked on every method rather than assumed.

`GetCompressedFileSize` reports zero for a sparse NTFS file even when it holds
data, which is why the allocated column reads `0 B` for `sparse` and why volume
free space is the number the check asserts on. `fsutil file layout` would show
the extents directly and needs elevation, so it is not used.

The unit tests cover what can be asserted without a filesystem-specific tool:
every method sets the length, `prealloc` reads back as zeroes and replaces
existing bytes, `sparse` reserves a gibibyte in under five seconds (which it
could not do if it wrote the bytes), and `falloc` either works or names why it
fell back.

```
$ cargo test -p bit-cli-core --lib alloc
test result: ok. 8 passed; 0 failed
```

### T-013 Selecting a subset of files still creates all of them

Source:      https://github.com/ikatson/rqbit/issues/484 (open)
Category:    disk-io
Priority:    P2
Effort:      S
Status:      **done**

Problem:     Adding a torrent with `only_files` set creates every path in the
             torrent, not only the selected ones.
Relevance:   `--select-file` is how a caller pulls one ISO out of a
             twelve-image torrent. Creating the other eleven as empty files is
             surprising and, on a filesystem without sparse support, expensive.
Approach:    Confirm against the pinned 9.0.0 first. If it still creates them,
             either delete the unselected files after initialisation or supply
             a storage factory that refuses to create them.
Acceptance:  `bit-cli download <MULTI> --select-file 0 --json` finishes with
             only the selected file present under `--dir`, and the JSON lists
             the skipped paths.

Confirmed on a five-file torrent, before the fix:

```
$ bit-cli download multi.torrent --web-seed $URL --web-seed-only \
    --dir out --select-file 0 --port 0 --json
       0 multi/deep.bin
  262144 multi/file0.bin
       0 multi/file1.bin
       0 multi/file2.bin
       0 multi/file3.bin
```

After:

```
  262144 multi/file0.bin
```

and the same torrent with no selection still lands all five files, each
hashing equal to its source.

Two causes, both in `bit-cli`'s own storage rather than the session's:

- `SafeStorage::init` opened every planned path.
- The hash check reads every piece of every file to learn what is already on
  disk, and the open used for a read created the file.

The fix for the first is the same one that closes
[T-011](#t-011-no-file-handle-pool-so-long-runs-exhaust-descriptors): files open
when they are first touched. The fix for the second is `Intent`: a write creates
a file and a read does not, so a read of a file that is not there answers "not
there" rather than bringing one into existence.

Between them, no selection has to be plumbed into storage at all, which is what
makes this correct for the case a selection cannot express: a piece that spans
a selected file and an unselected one still writes into both, and both are
created because both were written.

Directories are still created up front. An empty directory a selection did not
fill is cheap and visible; an empty file pretending to be payload is not.

**Corrected 2026-08-22T01:40Z, while measuring [T-185](cli-surface.md).** The
acceptance above is true and is only half the shape. It selects index 0, so
every unselected file is **after** the selection and none of them is created.
Select index 1 of a two-file torrent whose file 0 ends exactly on a piece
boundary and file 0 lands as a zero byte file, which is what the last sentence
above says does not happen.

The rule the fix states, "a write creates a file and a read does not", is not
the thing that broke. What broke is what counts as a write: `librqbit` issues a
zero length write to the file **before** a chunk that starts on a file boundary,
and a write of no bytes is not a write. Filed with the cause and the line
numbers as [T-188](#t-188-a-chunk-starting-on-a-file-boundary-creates-the-file-before-it).
Nothing here needs the selection plumbed into storage, which is this entry's
argument and still holds.

### T-014 Adding a torrent can fail with "File exists (os error 17)"

Source:      https://github.com/ikatson/rqbit/issues/504 (open)
Category:    disk-io
Priority:    P2
Effort:      S
Status:      **done**

Problem:     Adding a torrent fails outright when the session's own cache files
             already exist.
Relevance:   `bit-cli` runs with persistence off, so its exposure is smaller,
             but the same class of failure reaches `add` through `overwrite`.
Approach:    `bit-cli` maps this to `ExitCode::Disk` in
             `engine::classify_add_error` by matching "os error 17" in the
             error chain. That is text matching and it is fragile. Replace it
             with a real classification once `librqbit` exposes a typed error,
             and meanwhile add a test that pins the string.
Acceptance:  A test adds a torrent over an existing conflicting path and
             asserts exit code 8, not exit code 1.

The classification is by type rather than by text. `storage::AlreadyExists` is
an error type carrying the path, `SafeStorage::init` returns it before anything
is written, and `engine::classify_add_error` finds it by downcasting the error
chain. So the exit code does not change when somebody rewords a message.

The text classifier is still there for what `librqbit` reports as prose with no
type to match, and the phrases it matches on are pinned by a test, which is
what keeps a reworded upstream phrase from silently changing an exit code.

```
$ cargo test -p bit-cli-core --lib engine::tests::an_existing_file_is_classified_by_type
test result: ok. 1 passed; 0 failed
```

Three tests hold it. `an_existing_file_is_classified_by_type_rather_than_by_its_wording`
asserts exit code 8 and that the message names both the path and
`--allow-overwrite`. `every_text_classification_maps_to_the_code_it_is_there_for`
pins the nine phrases. `a_type_beats_the_text_when_both_could_match` uses a path
containing the word "connect", which the text classifier would call a network
failure, and asserts the type wins.

### T-015 Hash checking can hang at 0 percent

Source:      https://github.com/ikatson/rqbit/issues/347 (open)
Category:    disk-io
Priority:    P1
Effort:      M
Status:      **done**

Problem:     Roughly one add in twenty of a torrent with existing files sticks
             at 0 percent or 100 percent "checking files" and never leaves.
             Removing and re-adding sometimes clears it.
Relevance:   `bit-cli download` and `bit-cli seed` both wait on
             `wait_until_initialized`. A hang there is a hang with no output
             and no exit.
Approach:    `--timeout` and `--stop-after` already bound the whole run, so a
             hang is survivable today, but the run reports a deadline rather
             than the real cause. Add an initialisation-specific deadline that
             names the hash check, and reproduce the hang with a torrent whose
             files are on a slow or contended volume.
Acceptance:  `bit-cli download <TORRENT> --timeout 30s` against a stuck hash
             check exits 9 with `"phase": "initializing"` in the error context.

`Engine::wait_until_initialized_within` is the initialisation-specific
deadline. It exits 9 and the context says which phase and how far the check
had got, so a caller can tell a stuck hash check from a slow one:

```json
{
  "phase": "initializing",
  "info_hash": "...",
  "waited_ms": 30000,
  "checked_bytes": 0,
  "total_bytes": 1073741824,
  "checked_percent": "0.00%",
  "state": "initializing"
}
```

`checked_percent` is what separates the two failures the entry names.
Consecutive samples that do not move are the hang; samples that move are a
volume that is slower than the deadline allows, and the fix for that is a
longer deadline rather than a bug report.

The reproduction is
`webseed_e2e::a_hash_check_that_has_not_finished_names_the_phase_it_is_in`,
which hash-checks a 64 MiB payload with a one-millisecond deadline. That is a
check that cannot finish in time rather than one that is stuck, and it exercises
the same path: no test can make the upstream hang happen on demand, and one that
waited for it would be a test that usually passes for the wrong reason.

```
$ cargo test -p bit-cli-core --test webseed_e2e a_hash_check_that_has_not_finished
test result: ok. 1 passed; 0 failed
```

### T-016 fastresume is not used when adding a torrent

Source:      https://github.com/ikatson/rqbit/issues/349 (open)
Category:    disk-io
Priority:    P2
Effort:      M
Status:      **done**, 2026-08-22T19:28Z

Problem:     A cached bitfield at `.cache/rqbit/{infohash}.bitv` is not read
             when a torrent is added, so every add re-hashes the whole payload.
Relevance:   Re-hashing a 40 GB payload to seed it costs minutes of disk read
             every invocation. For a foreground one-shot tool that is the
             difference between usable and not.
Approach:    `SessionOptions::fastresume` exists in 9.0.0 and `bit-cli` leaves
             it off, because a stored bitfield is state that outlives the
             process and decision 7.4 puts stored session state in Phase C.
             The distinction worth making: a resume cache is derived data that
             can be recomputed, not session state. Decide explicitly whether
             `--fastresume` is in scope for Phase B, and if so where the cache
             lives and how it is invalidated.
Acceptance:  Either a `--fastresume` flag with a documented cache location and
             a test that a stale cache is detected and discarded, or an entry
             in `phase-c.md` saying why not.

## The cost, measured

512 MiB payload, one file, 1 MiB pieces, release build, seeded three times:

```
$ bit-cli seed p.torrent --data . --verify <MODE> --exit-when-idle 1s --json

--verify full     6087 ms wall
--verify quick    6372 ms
--verify none     6398 ms
```

Identical within noise, because all three do the same thing. Roughly 85 MiB/s
of hashing plus process startup, so a 40 GiB payload costs about eight minutes
of disk read on every `seed` invocation. That is the number the entry was
asking for.

**Those two numbers are wrong and the correction is under "Closed 2026-08-22"
below.** Most of the six seconds was `--exit-when-idle 1s` waiting for a peer,
not hashing. The measured rate is about 1.6 GiB/s and 40 GiB is about 25
seconds. The paragraph above is kept as written because it is what the entry
claimed and the correction belongs under it rather than in place of it.

## The blocker

**`fastresume` in `librqbit` 9.0.0 does nothing without session persistence.**
`session.rs:640-680`:

```rust
match &opts.persistence {
    Some(SessionPersistenceConfig::Json { folder }) => { ... make_result!(s) }
    None => Ok((None, Arc::new(NonPersistentBitVFactory {}))),
}
```

`make_result!` is the only place `opts.fastresume` is read, and it is only
reached when `persistence` is `Some`. With `persistence: None`, which is what
decision 7.4 requires, the bitfield factory is `NonPersistentBitVFactory`
whatever `fastresume` says.

So getting a resume cache means turning on `SessionPersistenceConfig::Json`,
which writes a store of every torrent in the session. That is stored session
state, and 7.4 puts it in Phase C.

`AddTorrentOptions` in 9.0.0 also carries no way to skip the initial check:
`paused`, `only_files`, `overwrite`, `list_only`, `output_folder`,
`sub_folder`, `peer_opts`, `force_tracker_interval`, `disable_trackers`,
`ratelimits`, `initial_peers`, `peer_limit`, `preferred_id`, and the storage
factory. Nothing else. So there is no second route either.

## What would unblock it

One of three, in the order they are worth trying:

1. An upstream `SessionOptions` that takes a `BitVFactory` directly, or a
   `fastresume` that works without a persistence store. Then `bit-cli` supplies
   a factory that reads and writes one file per info hash beside the payload,
   with the file length and modification time recorded so a stale cache is
   detected and discarded, and nothing about the session is stored.
2. A `TorrentStorage` hook that lets storage answer "this piece is already
   verified". `bit-cli` already supplies its own storage, so this would need no
   session state at all. The trait has no such method in 9.0.0.
3. Candidate C, a native fetch and verification path that does not go through
   `librqbit`'s initialisation at all.

Until one of those exists this cannot be built without contradicting 7.4, so it
stays open here rather than moving to `phase-c.md`: the cache itself is derived
data that can be recomputed, which is not what 7.4 is about, and the thing
blocking it is an upstream API rather than a decision.

## Closed 2026-08-22, and it was option 1

The section above lists three things that would unblock this and puts "an
upstream `SessionOptions` that takes a `BitVFactory` directly" first. The trees
were vendored the same day, so that is what was built.

**`bit-cli seed --fastresume`**, with `--fastresume-dir` to move the cache.
`SessionOptions::bitv_factory` takes a factory, used wherever the session would
otherwise refuse to keep a bitfield, and
[`crates/bit-cli-core/src/resume.rs`](../crates/bit-cli-core/src/resume.rs) is
that factory. Nothing about session persistence is turned on and no session
state is written, so decision 7.4 is untouched.

**Where the cache lives**, which the Acceptance asks to be documented:
`<data>/.bit-cli-resume/<info hash>.bitv`, beside a `.meta` sidecar.
`--fastresume-dir` overrides the root. Beside the payload by default so moving
or deleting the data takes the cache with it.

**How a stale cache is caught**, three layers, cheapest first:

1. **The sidecar**, ours: every file's length and modification time, the total
   length and the piece count. One `stat` per file, and any disagreement means
   the cache is not offered and is deleted.
2. **The length check**, `librqbit`'s: a bitfield of the wrong size for this
   torrent is refused.
3. **The sample**, `librqbit`'s: at least one claimed piece per file plus a
   random sample of the rest are re-hashed, and one failure discards the lot.

Layers 2 and 3 already existed and are what make this safe at all. Layer 1 is
ours because the other two are probabilistic about the middle of a large file.

**Measured**, `scripts/check-fastresume.ps1`, one 512 MiB payload of 1 MiB
pieces, five runs, `bench/fastresume-20260822T192324469Z.json`:

| run | `--fastresume` | elapsed | reports complete |
| --- | --- | --- | --- |
| `cold`, empty cache | yes | 2.38 s | yes |
| `warm` | yes | **2.06 s** | yes |
| `stale`, one byte rewritten | yes | 2.38 s | **no** |
| `refresh` | yes | 2.05 s | no |
| `no_flag` | no | 2.37 s | no |

```bash
pwsh -NoProfile -File scripts/check-fastresume.ps1
```

The clock says the check was skipped and the `complete` column says the cache
was right. `stale` is the case the whole entry rests on: it refuses the cache,
hashes again, and finds the one piece that changed. A run that trusted it would
have announced a piece it does not hold, and the peer on the other end would be
what found out.

**The premise about what hashing costs was wrong, and the correction belongs
here.** "The cost, measured" above reads 6,087 ms for a 512 MiB seed and infers
"roughly 85 MiB/s of hashing" and eight minutes for 40 GiB. Most of those six
seconds was `--exit-when-idle 1s` waiting for a peer that never came. Measured
against `--announce-only`, which stops as soon as the torrent is live, the same
payload costs **0.32 s** of hashing, which is about **1.6 GiB/s**. So 40 GiB is
about **25 seconds**, not eight minutes. The flag is still worth having and its
value is a quarter of a minute per invocation rather than eight, and the entry
should not go on claiming the larger number.

**Why the timing is judged as a difference and not a ratio.** Most of each run
is a fixed two second settle, so two runs that differ only in whether they
hashed differ by exactly the hashing. A ratio over the whole run is 1.16 for a
check that was skipped entirely, which says nothing.

**Seeding only, deliberately.** `bit-cli download` does not take the flag. The
sidecar keys on modification time, which is correct for a payload nothing is
writing, and a download writes its payload continuously: every run would find
its own cache stale. Resuming a partial download needs invalidation of a
different shape and is not built. `--verify` still says what it does and still
warns for `quick` and `none`, because the session still offers no way to skip
the check other than this.

## What ships in the meantime

`seed --verify` now says what it does. All three values behaved identically and
only `none` warned, so `quick` claimed to be a quick check and was a full one.
Both `quick` and `none` warn now, naming what actually happens, and `--help`
says the same. A flag whose values are all the same is worse than no flag; a
flag that says so is not.

---

### T-017 Concurrent receive paths contend on the payload file

Source:      the [T-090](bench.md) `bench leech` measurement
Category:    disk-io
Priority:    P1
Effort:      M
Status:      **done**

Problem:     The same 1 GiB of payload writes costs 1,137 ms totalled across
             one receive path and 14,036 ms totalled across eight. That is the
             same bytes, the same file, the same block size, and twelve times
             the time. Per path it is 20% of the run at one path and 50% of
             the available path time at eight.
Relevance:   It is what caps [T-009](webseed.md), and it is the first thing to
             check for [T-030](performance.md), which is throughput collapsing
             with several torrents at once. Several torrents is several
             receive paths against several files, and this is several receive
             paths against one.
Approach:    Two candidate causes, and the measurement does not yet separate
             them:

             1. **The handle.** `SafeStorage` holds one `std::fs::File` per
                payload file and every path writes through it. On Windows that
                is a synchronous handle, and `seek_write` is `WriteFile` with
                an `OVERLAPPED` offset against it. Whether concurrent
                positioned writes to one synchronous handle serialise on the
                file object is what has to be established, and the answer
                differs between Windows and Linux.
             2. **The session.** `librqbit` takes a per-torrent write lock on
                every received chunk and runs the write under a
                `block_in_place` semaphore of eight permits. Eight paths
                against eight permits is exactly the shape of the measured
                curve.

             What separates them is a micro-benchmark that writes the same
             bytes through `SafeStorage` from N threads with no session at
             all. If the curve reproduces, it is the handle; if it does not,
             it is the session. That benchmark belongs under `bench/` and does
             not exist.
Acceptance:  The micro-benchmark is committed and run on both platforms, this
             entry records which cause it found, and the fix is measured
             against `bench leech` at one, two, four, and eight bridges.

**Neither cause. The writes serialise on the file, not on the handle, and the
session is not involved.** A third finding came out of the same measurement and
is the one that matters: the serialisation is charged per write operation
rather than per byte.

The benchmark is `bit-cli bench disk`, in `bit_cli_core::bench::disk`. It
writes a payload through the same `SafeStorage` a download writes through, from
N threads, with no session and no network. Three layouts, all writing the same
bytes in the same block size from the same threads onto the same volume:

| Layout | Files | Handles | Who writes what |
| --- | --- | --- | --- |
| `shared` | 1 | 1 | Every thread interleaves blocks into one file. This is a torrent with one payload file and several peers. |
| `handles` | 1 | N | The same file at the same offsets, but thread `i` writes through its own handle. |
| `split` | N | N | Thread `i` owns file `i` and writes it end to end. |

`shared` and `handles` differ only in how many open handles the identical
writes go through, which is what makes the pair decisive. `split` is the
control that says whether anything scales at all.

Acceptance, `scripts/check-disk-contention.ps1`, 1 GiB per step, three
iterations, medians, 2026-08-20T06:48:13.208Z. Report:
`bench/disk-contention-20260820T064813208Z.json`.

```
$ pwsh -NoProfile -File scripts/check-disk-contention.ps1

Where the limit lives, in 16KiB blocks:

threads shared     handles        split      shared x1 handles x1 split x1
      1 2.25 GiB/s 2.29 GiB/s     2.27 GiB/s      1.00       1.00     1.00
      2 1.52 GiB/s 1.55 GiB/s     3.96 GiB/s      0.68       0.68     1.75
      4 1.45 GiB/s 1.21 GiB/s     4.58 GiB/s      0.65       0.53     2.02
      8 1.31 GiB/s 1,012.63 MiB/s 2.29 GiB/s      0.58       0.43     1.01

verdict: the file, not the handle: more handles on one file reach 1x, the same
writes over separate files reach 2.021x. Writes to one file serialise whatever
handle they arrive on.
```

Three things it says.

**More writers on one file make it slower, not faster.** One thread reaches
2.25 GiB/s and eight reach 1.31 GiB/s, which is 0.58x. Adding a writer to a
file costs throughput from the second one onward.

**More handles do not help.** `handles` gives each of the eight threads its own
open handle to the same file, writing the same offsets, and it reaches 0.43x
where one handle reaches 0.58x. So the serialisation is not the synchronous
file object that candidate 1 named. It is the file. Spreading the writes over
separate files is what scales, and only that: `split` reaches 2.02x at four
threads.

**The session is not involved.** There is no session in this benchmark and the
curve reproduces, so candidate 2 is out as well.

The order the layouts run in alternates inside each iteration and flips between
iterations, because the volume's own state carries between steps. Each step
drains its writeback before the next starts, reported as `flush` and not
counted in the rate, or a step that filled the page cache would hand its cost
to whichever step ran after it.

## What it is charged for

The same script's second phase writes the same 1 GiB to one file from the same
threads, in blocks from 16 KiB up:

```
What one write costs, shared layout:

block  t1         t2         t4         t8           ops
16KiB  2.07 GiB/s 1.50 GiB/s 1.22 GiB/s 1.24 GiB/s 65536
64KiB  3.09 GiB/s 2.49 GiB/s 2.64 GiB/s 2.54 GiB/s 16384
256KiB 3.27 GiB/s 3.18 GiB/s 3.21 GiB/s 1.56 GiB/s  4096
1MiB   3.47 GiB/s 3.51 GiB/s 3.28 GiB/s 2.84 GiB/s  1024

charged: per operation: at 8 threads, 1MiB writes reach 2.296x what 16KiB
writes reach for the same bytes
```

The same bytes to the same file from the same threads, and the only thing that
changed is how many write operations they were split into. At 1 MiB the thread
count stops mattering at all: 3.47, 3.51, 3.28, 2.84 across one to eight
writers. The per-write serialisation is still there and still exact, visible in
the mean write time growing by the thread count at every block size, but with
few enough operations it costs nothing in wall time.

That is what a fix would have to do, and it is recorded as
[T-018](#t-018-the-write-path-issues-one-operation-per-16-kib-block).

## Whether it caps a download

**It does not.** The Relevance above said this was what caps
[T-009](webseed.md). The numbers say otherwise.

`bench leech` at eight bridges, re-measured in the same session,
2026-08-20T06:53:25.367Z, report `bench/leech-20260820T065325367Z.json`:

```
$ pwsh -NoProfile -File scripts/bench-leech.ps1 -PayloadSize 1GiB -Runs 3 -ConnectionSweep "1,2,4,8"

fetch, no bridge                   662.45 MiB/s   100.00%
leech, 1 connection(s)             162.51 MiB/s    24.53%
leech, 2 connection(s)             314.01 MiB/s    47.40%
leech, 4 connection(s)             340.31 MiB/s    51.37%
leech, 8 connection(s)             407.97 MiB/s    61.58%
```

The deepest bridge count moves 408 MiB/s. Storage at the same eight writers, on
one file, in the same 16 KiB blocks, moves 1.31 GiB/s. That is 3.3 times what
the download asks of it, so the write path has headroom rather than being the
wall.

What the disk does cost the download is its share of the wall clock. 1 GiB of
16 KiB writes at eight writers takes about 806 ms of wall time on its own,
against the leech run's 2,510 ms, so writes occupy at most 32% of the run and
removing them entirely could not do better than that.
[T-018](#t-018-the-write-path-issues-one-operation-per-16-kib-block) is what is
actually available there.

So the sentence in the Relevance above is wrong, and it is left standing with
this correction under it rather than edited away. What caps
[T-009](webseed.md) is the per-peer serial receive path, which is what that
entry already says.

## Not run on Linux

The acceptance asks for both platforms and this machine has neither. `wsl
--list` reports no installed distribution, and no container runtime is present:

```
$ wsl -l -v
Windows Subsystem for Linux has no installed distributions.

$ docker info
docker: command not found
```

Installing either is a system-level change that rule 0.4 puts outside what this
session may do. To unblock it, one of:

```
wsl --install -d Ubuntu
```

```
winget install -e --id Docker.DockerDesktop
```

Then the same command produces the Linux half, because `bench disk` is in the
binary rather than in a script:

```
cargo build --release --bins && ./target/release/bit-cli bench disk \
    --payload-size 1GiB --concurrency-sweep 1,2,4,8 --layout shared --format text
```

The answer is expected to differ. Linux `pwrite` on a regular file takes the
inode lock shared for a write inside the existing size on ext4 and xfs, so
`shared` should track `split` there rather than falling behind it. That is a
prediction and not a result, and it stays one in this entry until the command
above has been run.

---

### T-018 The write path issues one operation per 16 KiB block

Source:      the [T-017](#t-017-concurrent-receive-paths-contend-on-the-payload-file) measurement
Category:    disk-io
Priority:    P2
Effort:      M
Status:      **done**, 2026-08-22T11:13Z, with the last clause's fixture
             corrected below

Problem:     The session hands storage one 16 KiB block at a time and storage
             turns each into one positioned write.
             [T-017](#t-017-concurrent-receive-paths-contend-on-the-payload-file)
             measured that writes to one file serialise per operation rather
             than per byte, so 1 GiB in 16 KiB writes costs 2.30 times what the
             same gigabyte costs in 1 MiB writes at eight writers. A 1 GiB
             `bench leech` run issues 65,536 write operations where 1,024 would
             carry the same bytes.
Relevance:   Bounded, and the bound is measured rather than guessed. Writes take
             about 806 ms of the 2,510 ms an eight-bridge `bench leech` run
             takes. Coalescing to 1 MiB would take that to about 352 ms, so the
             change is worth at most 454 ms of 2,510 ms, which is 18%, and
             408 MiB/s becoming about 497 MiB/s. Worth having, and not worth
             having before the open P0 items.
Approach:    A write-combining buffer per active region in `SafeStorage`, not
             one buffer per file: with N peers the writes arrive as N
             interleaved sequential streams, because the bridge fetches a 1 MiB
             range and answers 16 KiB blocks out of it, so one buffer per file
             would thrash where N would not.

             Three correctness constraints, and none of them can be traded:

             1. A read has to see a buffered write. The session reads every
                piece back to hash it as soon as its last block lands, so a
                buffer that `pread_exact` does not consult fails the piece.
             2. Every buffer flushes before the file is closed, before the
                handle is evicted by `--max-open-files`, and at the end of the
                run.
             3. Losing a buffered block on a crash is recoverable for a
                download, which re-fetches it. It is not recoverable for
                anything that reports progress from it, so nothing reports
                progress from a byte that is still in a buffer.
Acceptance:  `bit-cli bench disk --block-size 16KiB --layout shared
             --concurrency-sweep 1,2,4,8` reaches within 10% of the same run at
             `--block-size 1MiB`, `bench leech` at eight bridges improves and
             the improvement is recorded here with both reports, and the whole
             suite passes including `scripts/interop-roundtrip.ps1`, which is
             what proves no byte moved.

**Two implementations of this exact change exist, and one of them has the
tests.** `TorrentNG/crates/rt-storage/src/elevator.rs:223`
`coalesce_ready_ops` and `:251` `can_merge` merge adjacent ready operations on
the same file into one dispatch, and its test names carry the rule that matters
here: `ready_reads_are_offset_sorted_and_coalesced_per_file` and
`writes_are_ordered_but_not_coalesced`. **Reads are offset-sorted and
coalesced; writes are ordered and not.** That is a stronger constraint than the
three above and worth understanding before overriding it: a write-combining
buffer reorders nothing, but it does defer, and constraint 1 above is exactly
the reason that tree kept writes un-coalesced.

`TorrentNG/crates/rt-storage/src/handle_cache.rs` is a path-and-access
keyed LRU of open descriptors bounded by a fraction of `RLIMIT_NOFILE`, which
is what [T-011](#t-011-no-file-handle-pool-so-long-runs-exhaust-descriptors)
built here as `--max-open-files`. Its doc names the property that makes a
shared descriptor safe and that this entry depends on: **no per-operation
`seek`, so concurrent readers and writers do not race a file cursor.**

`TorrentNG/crates/rt-storage/src/io_class.rs:7` is the piece this entry does not have and
probably wants: an `IoClass` ordering of
`Metadata < Recheck < MoveCopy < PeerWrite < PeerRead < Foreground` with
**per-class concurrency caps that differ for spinning disks and SSDs** (`:24`
`hdd_concurrency`, `:36` `ssd_concurrency`; peer reads 4 against 16, recheck 1
against 4). Its stated invariant is that peer reads must never be starved by
bulk recheck or background copy, which is precisely the failure
`bit-cli bench disk` was built to expose and has not yet been pointed at.

anacrolix [PR 1051](https://github.com/anacrolix/torrent/pull/1051) (OPEN) is
the same problem solved for the same platform: cache the writable handle so a
16 KiB chunk write stops reopening the file, and buffer piece-completion
persistence on a **1 s or 128 MiB checkpoint** rather than per piece, while
keeping `complete = false` immediate. That asymmetry, batch the optimistic
update, never batch the pessimistic one, is the safe shape for constraint 3
above.

One hazard from the same tree, because this entry adds a buffer that both a
reader and a writer consult: anacrolix
[PR 1074](https://github.com/anacrolix/torrent/pull/1074) fixed a deadlock from
a **recursive `RLock`**, because `sync::RWMutex` is not recursive and a queued
writer arriving between two read locks wedges both. That is the same class as
[T-010](#t-010-pwrite-takes-a-read-lock-where-it-needs-a-write-lock), already
closed here, and constraint 1 is a re-entrancy invitation of exactly that
shape: `pread_exact` consulting a buffer that `pwrite_all` holds a lock on.

**Re-measured on the current tree, 2026-08-22, before building anything. The
gap is still there and it is worth more than the Relevance line says.**

The `bench disk` half of the Acceptance, run as written at 512 MiB:

| block size | 1 thread | 2 | 4 | 8 |
| --- | --- | --- | --- | --- |
| `--block-size 16KiB` | 2.35 GiB/s | 1.70 | 1.78 | 1.70 |
| `--block-size 1MiB` | 3.72 GiB/s | 3.88 | 3.69 | 3.67 |

At eight writers that is **2.16 times**, against the 2.30 T-017 measured, so
the finding this entry rests on holds. The Acceptance asks for the 16 KiB run
to come within 10% of the 1 MiB one, and it is at 46% of it.

**The Relevance line's arithmetic does not work and the numbers are not
comparable.** It reads "writes take about 806 ms of the 2,510 ms an
eight-bridge `bench leech` run takes", which treats write time as a slice of
wall clock. It is not. A block is written, and at a piece boundary read back
and hashed, **inline on the receive path that got it**, so eight paths each
spend their own write time and the total exceeds the wall clock. Measured now
at eight bridges over 512 MiB, `bench/leech-20260822T090848152Z.json`:

| | |
| --- | --- |
| wall | 1,262 ms |
| path time, wall times eight bridges | 10,096 ms |
| writing the payload | **5,101 ms**, 50.52% of path time |
| piece checks, read plus hash | 1,362 ms, 13.49% |
| of which reading back | 1,105 ms, 10.94% |

5,101 ms of writing against a 1,262 ms wall is what says the two were never a
ratio. The report's own `attribution` block compares write time to path time,
which is the comparison that means something, and by it **writing is the single
largest thing a receive path does**.

**The same run carries the control that makes the case on its own.** The
`control` stage moves the same 512 MiB in the same 32,768 write operations over
**one** receive path instead of eight:

| stage | receive paths | write ops | write time |
| --- | --- | --- | --- |
| control, 1 connection | 1 | 32,768 | **468 ms** |
| leech, 8 connections | 8 | 32,768 | **5,101 ms** |
| the same URL named 8 times | 8 | 32,768 | 6,155 ms |

Same bytes, same operation count, and **10.9 times the write time** for putting
eight paths on the file. That is T-017's per-operation serialisation measured
through the download path rather than through `bench disk`, and it is the whole
argument for coalescing: the fix removes 63 of every 64 operations, and it is
operations that contend.

**What it is worth, recomputed.** Writing is 638 ms of each path's 1,262 ms.
Taking write throughput from the 16 KiB figure to the 1 MiB one is 2.16x, which
would put it at about 295 ms, saving roughly **342 ms of a 1,262 ms wall, 27%**,
and 405.71 MiB/s becoming about 555 MiB/s. That is a ceiling and not a
forecast: the fetch shares the path, so removing write time exposes whatever is
behind it. It is above the 18% the Relevance line claims, and the reason is the
control row above rather than a better disk.

**Not built.** The measurement is here so the build starts from the current
tree rather than from T-017's, and the three correctness constraints in the
Approach are unchanged. What the numbers add to the Approach is that the
`--web-seed-connections` count is the multiplier: at one receive path the write
path costs 468 ms and coalescing would be worth almost nothing, and everything
this entry is worth appears between one path and eight.

**Built 2026-08-22. The download path is 25% faster and one acceptance clause
is not met, which is why this was partial rather than done.** The clause was
met later the same day once the fixture stopped standing in its way; that is
the last section of this entry and it is what closed it.

`Coalescer` in `crates/bit-cli-core/src/storage.rs` holds up to `WRITE_RUNS`
contiguous runs of at most `WRITE_REGION`, one per **active region** rather
than one per file, exactly as the Approach asks. A write that continues a run
extends it; a write that continues nothing starts one, displacing the oldest
when there is no room; a write already the size of a region goes straight
through, because copying a megabyte to save nothing is not a saving.

The three correctness constraints, and how each is answered:

1. **A read sees a buffered write.** `pread_exact` flushes every run
   overlapping what it is about to read, **before** reading. Answered by
   flushing rather than by serving the read out of the buffer, which is the
   re-entrancy the Approach warns about: the coalescer's lock is never held
   across I/O, so `pread_exact` taking it, dropping it, and then writing
   cannot wedge against `pwrite_all` doing the same. That is the anacrolix
   PR 1074 shape avoided rather than reproduced.
2. **Everything flushes before the handle goes.** Before a handle is evicted
   by `--max-open-files`, in `flush_all`, and in a `Drop` that should never
   have anything to do, because the piece read-back has already taken it. A
   file being removed **discards** its runs instead: writing bytes into
   something on its way to being deleted is work that can only fail. And
   `TorrentStorage::take` moves the held runs with the handles they belong to,
   or the old instance's `Drop` would write them through handles it had just
   given away.
3. **Nothing reports progress from a buffered byte.** Nothing can:
   `bit-cli` keeps no resume state by decision 7.4, so a byte that never
   reached the file is simply re-fetched. `SafeStorage::buffered_bytes` is
   there for the resume cache that would need it.

**The read-back is what makes this safe rather than lucky.** The session reads
every piece back to hash it the moment its last block lands, so a run never
holds more than one piece and every held byte is on the device before the piece
it belongs to is declared complete. Correctness does not rest on anything
remembering to flush later.

**A second counter came with it, because the first one stopped meaning what it
said.** `StorageCounts::write_ops` counted writes the session asked for and
operations that reached the device, which were the same number. They are not
any more. `write_calls` is the first and `write_ops` is the second, and
`write_ops / write_calls` is the coalescing factor. The fan-out assertion in
`a_torrent_whose_pieces_straddle_every_boundary_downloads_byte_for_byte` moved
to `write_calls`, which is where the property it protects actually lives: a
block spanning a file boundary still issues one write per file, and runs are
keyed by file so two files can never be combined.

**What it bought, measured at eight bridges over 512 MiB.**
`bench/leech-20260822T090848152Z.json` before, `bench/leech-20260822T094440861Z.json`
after, the same script and parameters minutes apart:

| | before | after | |
| --- | --- | --- | --- |
| rate | 405.71 MiB/s | **508.44 MiB/s** | +25.3% |
| wall | 1,262 ms | 1,007 ms | −20.2% |
| writing the payload | 5,101 ms, 50.52% of path time | **1,806 ms, 22.42%** | −64.6% |
| write operations | 32,768 | 21,014 | −35.9% |
| control, one receive path | 468 ms over 32,768 ops | 204 ms over 10,600 ops | −56.4% |

The ceiling worked out above was about 27% and this landed at 25.3%, so
removing the write time exposed very little behind it. Writing has gone from
the largest thing a receive path does to less than a quarter of it.

**The acceptance's first clause is not met, and the fixture is why.**
`bench disk --block-size 16KiB --layout shared` reaches 670.68 MiB/s at eight
threads against 1.20 GiB/s at `--block-size 1MiB`, which is 55% and not the
90% the clause asks for. It cannot reach it: `assignment` gives the shared
layout `block % threads` as the owner, so at eight threads each thread's next
write is eight blocks past its last and **nothing is ever contiguous**. The
buffer coalesces nothing there and pays for the copy.

That is the instrument's shape and not the download's. Measured in one run of
`scripts/check-disk-contention.ps1`, so the three are comparable to each other,
`bench/disk-contention-20260822T094609951Z.json`:

| 16 KiB blocks | 1 thread | 8 threads |
| --- | --- | --- |
| `shared`, strided, through the buffer | 985.21 MiB/s | 670.68 MiB/s |
| `handles`, raw, no buffer at all | 1.07 GiB/s | 730.15 MiB/s |
| `split`, contiguous per thread, through the buffer | 1,016.66 MiB/s | **1.14 GiB/s** |

`split` is the layout whose threads write contiguous ranges, which is what a
receive path does, and it reaches **1.56 times** the raw unbuffered path at
eight threads. `shared` pays about 8% against that same raw path for a copy it
gets nothing back for. So the buffer helps exactly where the writes are
sequential and costs a little where they are not, and the download is the
former.

**What would close this.** Either the shared layout stops striding, so
`bench disk` can show what the download path does, or the clause moves to
`--layout split` and asks for the number that fixture can produce: 1.14 GiB/s
against `split` at 1 MiB, which is 1.54 GiB/s at eight threads, so 74% and
still not 90%. Neither is a change to the write path, and both are decisions
about what `bench disk` is for, which is [T-017](#t-017-concurrent-receive-paths-contend-on-the-payload-file)'s
question rather than this one's.

**Decided 2026-08-22T11:13Z, and neither of the two was the answer.** Striding
is not a property of `shared` worth removing and `split` is not the fixture to
move to. Striding is one end of a scale nothing could name, so the scale is
named: `--run-length N` is how many consecutive blocks one thread writes before
the next takes over, under `shared` and `handles` alike, and it defaults to 1.

- At 1 it is exactly what it always was, so every number recorded above and in
  [T-017](#t-017-concurrent-receive-paths-contend-on-the-payload-file) stays
  comparable and none of them had to be taken again.
- At 64 with a 16 KiB block, a thread writes one 1 MiB range per turn. That is
  a receive path: the bridge fetches a range and answers blocks out of it.
- `handles` takes the same run length, because the two are a pair and a pair
  measured at two different arrangements answers nothing.

Removing the striding outright would have thrown away the most contended
arrangement there is, which is the one
[T-017](#t-017-concurrent-receive-paths-contend-on-the-payload-file) needed,
and invalidated every table either entry rests on.

**Measured at 512 MiB, sparse, four configurations back to back,
`bench/disk-runlength-20260822T111026806.json`.** Rates first:

| 512 MiB, shared | 1 thread | 2 | 4 | 8 |
| --- | --- | --- | --- | --- |
| 16 KiB, `--run-length 1` | 927.83 MiB/s | 939.35 MiB/s | 696.85 MiB/s | 673.44 MiB/s |
| 16 KiB, `--run-length 64` | 956.40 MiB/s | **1.26 GiB/s** | **1.25 GiB/s** | 1.10 GiB/s |
| 1 MiB, `--run-length 1` | 1.25 GiB/s | 1.28 GiB/s | 1.29 GiB/s | 1.31 GiB/s |
| 16 KiB, run 64, `handles` | 1.11 GiB/s | 1.27 GiB/s | 1.03 GiB/s | 1.24 GiB/s |

As a share of the 1 MiB run, which is what the clause asks about:

| | 1 thread | 2 | 4 | 8 |
| --- | --- | --- | --- | --- |
| `--run-length 64` | 74.7% | **98.6%** | **96.7%** | 84.3% |
| `--run-length 1` | 72.5% | 71.6% | 52.8% | 50.3% |

**The clause is met at two and four threads and not at one and eight**, against
a fixture where it was 50% to 72% everywhere. That is the answer the clause was
asking for, and the fixture was what stood between it and an answer.

**The coalescing itself is exact and the operation counts say so.** Every run
asks storage for 32,768 writes. What reaches the device:

| | 1 thread | 2 | 4 | 8 |
| --- | --- | --- | --- | --- |
| `--run-length 1` | 512 | 7,152 | 32,374 | 32,532 |
| `--run-length 64` | **512** | **512** | **512** | **512** |

64 to 1 at every thread count, because a run reaches `WRITE_REGION` and flushes
on the block that fills it. The strided row is the finding that was not
predicted: the buffer belongs to the storage rather than to a thread, so two
threads writing adjacent blocks in order **do** extend one run, and at one
thread striding is a no-op so all 32,768 combine into 512. Between two threads
and four the accident stops happening. That is why the strided row is a
scheduling outcome and is measured rather than asserted anywhere.

**What the residual gap is, isolated.** At one thread the two 16 KiB runs and
the 1 MiB run all reach the device in exactly 512 operations, and they differ
only in how many calls produced them: 32,768 against 512. 956.40 MiB/s against
1.25 GiB/s for the same operations is therefore **the buffer's copy and the
call overhead, not the device**. 512 MiB is copied once on its way through.

**And the buffer does not pay for itself in this instrument at all**, which is
the finding that matters most and was invisible while the fixture strided.
`handles` writes the same contiguous runs with no buffer, 32,768 operations
rather than 512, and it is **faster** at one thread and at eight: 1.11 GiB/s
against 956.40 MiB/s, and 1.24 GiB/s against 1.10 GiB/s. The earlier table
above read the other way round only because it compared `split` through the
buffer against `handles` while `handles` was still striding, which was not the
same arrangement on both sides.

Against `bench leech`, where the same buffer is worth **+25.3%**, that is a
contradiction with an explanation: the download's write goes through
`librqbit`'s per-torrent lock and its `block_in_place` semaphore, so removing
63 of every 64 operations removes 63 of every 64 lock acquisitions and waits.
`bench disk` has neither, so it can only see the device, and at the device a
16 KiB positioned write on NTFS is already cheap enough that a memcpy is not
worth paying to avoid it. **`bench disk` measures the device; it cannot measure
what the write buffer is worth to a download.** That is
[T-017](#t-017-concurrent-receive-paths-contend-on-the-payload-file)'s question
answered, and it is filed as [T-192](#t-192-what-the-write-buffer-is-worth-depends-on-what-is-above-it).

**A counter was carrying the wrong number, and it was carrying it into the
schema.** `DiskStep::write_ops` is documented as writes that reached the device
and was the sum of the threads' own block counts, which is what they asked for.
So `bench disk` reported no coalescing at all, and `summary.disk.write_calls`
was zero in every report while `summary.disk.write_ops` held the value that
belongs under it. The step reports both now, taken from the storage counters
after the flush, and `StorageMetrics::observe_write` counts a call as well as
an operation so the `handles` layout, which has no buffer, reads as one to one
rather than as a divide by zero. Two tests asserted the old meaning and both
moved to `write_calls`, which is the same correction
[T-188](#t-188-a-chunk-starting-on-a-file-boundary-creates-the-file-before-it)
recorded for the straddling fixture. A report written before 2026-08-22 is
unaffected either way: with no buffer between them the two numbers were one
number.

Both fields went into `docs/schema.md` by the mechanism
[T-189](bench.md) built hours earlier, which failed the build naming
`disk_steps[].run_length` and `disk_steps[].write_calls` until the file was
regenerated. That is the first time it caught anything.

Acceptance, restated with what the instrument can now show, and met:

```bash
target/release/bit-cli bench disk --payload-size 512MiB --block-size 16KiB \
  --layout shared --run-length 64 --concurrency-sweep 1,2,4,8 --format json
```

`scripts/check-disk-contention.ps1` takes `-RunLength` so the three layouts can
be compared at either arrangement.

The other two clauses are met. `bench leech` at eight bridges improves and both
reports are named above, and the whole suite passes:

```powershell
pwsh -NoProfile -File scripts/gates.ps1
pwsh -NoProfile -File scripts/interop-roundtrip.ps1
```

1,113 tests, and three of three cases round trip byte for byte through
`aria2c` 1.37.0, which is what says no byte moved.

**Nine tests hold the buffer**, in `storage.rs`: a read seeing a held write, a
read elsewhere leaving it alone, 64 sequential blocks becoming one operation,
four interleaved streams not flushing each other, a ninth stream displacing the
oldest, a write over held bytes keeping the later one, a full region going
straight through, `Drop` writing what it held, and `remove_file` discarding it.

**And the storage-shaped acceptance scripts were run against it afterwards**,
because this is new code in the one path where a defect is silent and
permanent. `check-shared-files.ps1`: three torrents holding one file, one
fetch, three copies, one hash. `check-piece-order.ps1`: pass.
`check-allocation.ps1`: every method behaving as [T-012](#t-012-preallocation-is-not-implemented)
documents, once its own stale paths were fixed, which is
[T-190](#t-190-the-rule-for-where-a-payload-lands-says-one-thing-and-the-code-does-another)
and was failing before this change as well.



### T-192 What the write buffer is worth depends on what is above it

Source:      found closing [T-018](#t-018-the-write-path-issues-one-operation-per-16-kib-block), 2026-08-22
Category:    disk-io
Priority:    P2
Effort:      M
Status:      open

Problem:     The same `Coalescer` is worth **+25.3%** to `bench leech` at eight
             bridges and **less than nothing** to `bench disk` at the same
             thread count. With the arrangement controlled at
             `--run-length 64`, the buffered `shared` layout reaches
             1.10 GiB/s where the unbuffered `handles` layout reaches
             1.24 GiB/s over the same contiguous runs, and at one thread
             956.40 MiB/s against 1.11 GiB/s.
Relevance:   Two instruments disagree about a change that is already shipped in
             the write path, and only one of them is the product. The
             explanation is stated in
             [T-018](#t-018-the-write-path-issues-one-operation-per-16-kib-block)
             and is not yet measured: a download's write goes through
             `librqbit`'s per-torrent lock and its `block_in_place` semaphore,
             so combining 64 writes removes 63 lock acquisitions as well as 63
             operations, and `bench disk` holds neither lock. If that is right,
             the buffer is buying serialisation rather than device time, and
             the thing worth optimising next is the lock rather than the
             operation count. If it is wrong, then something else explains the
             25% and the buffer is costing throughput on some machine.
Approach:    The measurement that separates them is a `bench disk` step that
             takes the same locks the session takes, or a `bench leech` run
             with the buffer disabled at runtime. The second is cheaper and
             needs a way to turn the coalescer off, which nothing has: a
             `WRITE_RUNS` of zero would do it and there is no flag for it.
             Either way the answer is one number, the write time per receive
             path with and without, at one and eight bridges, and the control
             row already exists in
             [T-018](#t-018-the-write-path-issues-one-operation-per-16-kib-block)
             for the with case.
             Corpus: `TorrentNG/crates/rt-storage/src/io_class.rs:7` is the
             per-class concurrency cap this tree does not have, and it is the
             shape of a fix that works on the lock rather than on the
             operation.
Acceptance:  This entry says which of the two the 25% is, measured, and the
             `bench disk` documentation says what that instrument can and
             cannot see about the write buffer so the next reader does not
             have to rediscover that the two disagree.

### T-177 A piece that spans a file boundary has no adversarial fixture

Source:      `reference/RESEARCH.md` section D, 2026-08-21
Category:    disk-io
Priority:    P2
Effort:      S
Status:      **done**

Problem:     In a multi-file torrent a piece may straddle a file boundary: its
             first bytes belong to one file and its last to the next. Writing
             such a piece requires splitting it **at the boundary** and issuing
             one write per file. `bit-cli` has span-mapping code for this in
             `crates/bit-cli-core/src/span.rs` and `layout.rs`, and no test
             built to break it.
Relevance:   fx-torrent
             [Issue 98](https://github.com/yoep/fx-torrent/issues/98) (OPEN)
             is what happens when the split is missing: pieces are written
             **entirely into the file they start in**. The reporter's symptom
             is the memorable part: in a multi-file FLAC album, **only the
             first file is playable**, and the issue body carries a
             reproducible CC-licensed magnet. Every byte was transferred, every
             piece hashed against something, and the payload was wrong.

             That failure mode is the one `bit-cli` can least afford, for the
             same reason [T-074](windows.md) was a P1: a wrong payload that
             reports success is worse than a failure. It is also close to
             defects this project has already had.
             [T-036](performance.md) was a multi-file torrent losing its
             directory, and mkbrr
             [PR 154](https://github.com/autobrr/mkbrr/pull/154) is the
             verification-side twin: mapped files got *compacted* offsets that
             skipped missing files while verification used torrent-level
             offsets, so **every piece after a missing-file gap was reported
             bad and completion showed 0 per cent** on intact data.
             `bit-cli verify` reports per-piece results and has the same two
             coordinate systems to keep straight.
Approach:    A fixture, and the positive counterpart already exists to copy
             from. `vortex/bittorrent/src/file_store.rs` is the answer to
             fx-torrent 98 and its test names are the specification:
             `basic_multifile_alinged`, `small_multifile_misalinged`,
             `small_multifile_misalinged_files_and_subpiece`,
             `multifile_not_multiple_of_piece_size`, `multifile_misalinged_v2`,
             `multifile_misalinged_v3`, `single_file_misaligned`,
             `basic_single_file_aligned_unaligned_subpiece`. Eight cases, each
             naming one way the arithmetic goes wrong.

             Two of those eight interact with entries already open here. The
             `not_multiple_of_piece_size` case is
             [T-174](metainfo.md). And a boundary piece is what makes
             `--select-file` dishonest: `FluxDown/native/engine/src/bt_partfile.rs`
             documents the case where the bytes of an *unselected* file inside
             a boundary piece are discarded, so those pieces can never be
             verified or uploaded afterwards. `bit-cli` has `--select-file`
             and a `seed` command, so it has that problem too and nothing
             records it.
Acceptance:  A multi-file fixture whose pieces straddle every boundary, with
             at least one file shorter than a piece, passes `download`,
             `verify`, `seed` and a web seed fetch, and every file's bytes are
             asserted individually rather than by the torrent-level hash. Plus
             one assertion that a piece spanning two files issues two writes,
             not one.

**The arithmetic was already right, so this cost the one test the entry said it
would.** That is the result, and it is worth stating plainly rather than
quietly: the entry allowed for the other outcome, where a missing case was
hiding a P0, and it is not what happened. What was missing was the proof, and
the proof is now four tests over one fixture.

**The fixture is one torrent, and it serves this entry and
[T-174](metainfo.md) together**, because a piece length that is not a multiple
of 16 KiB and a set of files whose boundaries fall inside pieces are the same
fixture seen from two sides. It lives in
`crates/bit-cli-core/tests/webseed_e2e.rs`:

| | |
| --- | --- |
| piece length | 1,986,560, which is `121 * 16384 + 4096` |
| `a.bin` | 1,500,000 bytes, **shorter than one piece** |
| `b.bin` | 2,500,000 |
| `c.bin` | 900,000 |
| total | 4,900,000, so three pieces and the last one short |

Piece 0 covers 0 to 1,986,560 and the `a`/`b` boundary at 1,500,000 falls
inside it. Piece 2 is short and the `b`/`c` boundary at 4,000,000 falls inside
it. Piece 1 is entirely inside `b.bin`, which is the control case. **Every
boundary in the torrent is straddled**, and one test asserts that rather than
assuming it, so a later edit to the file lengths that accidentally aligns a
boundary fails instead of quietly weakening the fixture.

`a.bin` being shorter than a piece is the part that matters most: it means
piece 0 **cannot** be contained in the file it starts in, so a writer that
clamps a piece to its starting file has nowhere to hide.

**Four tests, each covering a different layer of the same claim.**

1. `a_piece_that_straddles_a_boundary_splits_into_one_slice_per_file` is
   `Layout::split_by_file` with no session and no server in the way. Piece 0
   yields two slices, piece 1 yields one, piece 2 yields two, and each set sums
   to the piece's own length.
2. `the_last_block_of_a_non_final_piece_is_four_kibibytes` is
   [T-174](metainfo.md), recorded there.
3. `a_block_that_straddles_a_boundary_is_fetched_as_one_request_per_file` is
   the web seed side: a 16 KiB block positioned with 8192 bytes on each side of
   the `a`/`b` boundary produces **two** `RangeRequest`s, and
   `Fetcher::read` returns the two files' bytes in order.
4. `a_torrent_whose_pieces_straddle_every_boundary_downloads_byte_for_byte` is
   the whole path: a real `librqbit` session, a real ranged HTTP mirror, and
   this fixture.

**The end-to-end assertion is per file, which is the entry's point.** fx-torrent
[issue 98](https://github.com/yoep/fx-torrent/issues/98) is a payload where
every byte transferred, every piece hashed against something, and only the
first file of a multi-file album was playable. A check that read the
torrent-level result would have passed it. So each of the three files is
compared to the bytes that were written, individually, and each file is filled
from a different seed so a byte landing in the wrong file is a mismatch rather
than a coincidence.

**The write fan-out is counted exactly, not bounded.** The first draft asserted
`write_ops >= pieces + 2`, which is five, and the real number is **303**. That
assertion was true and tested nothing, which is the failure mode
[RULES.md](RULES.md) warns about in a different form: a test that passes for a
reason other than the one it names.

The real number is arithmetic, and the test computes it rather than hard-coding
it. `bit-cli`'s storage layer is addressed by file index
(`crates/bit-cli-core/src/storage.rs`, `pwrite_all(file_id, offset, buf)`), so
it never sees a cross-file write at all: something above it splits at the
boundary. What reaches it is one write per 16 KiB block, plus one extra for
each block a file boundary falls inside.

```
piece 0: 1,986,560 -> 121 whole blocks + 4,096  = 122
piece 1: 1,986,560 ->                            122
piece 2:   926,880 ->  56 whole blocks + 9,376  =  57
                                          blocks  301
        both boundaries fall inside a block        +2
                                   write_calls    303
```

`assert_eq!(counts.write_calls, blocks + straddling_blocks)`. A payload written
without the split would report 301, and a whole piece landing in one file would
report fewer still. `write_bytes` is asserted equal to the payload length, so
nothing is written twice.

It was `write_ops` until 2026-08-22, when the two stopped being one number.
[T-018](#t-018-the-write-path-issues-one-operation-per-16-kib-block) combines a
run of sequential writes into one device operation, so `write_ops` counts
operations and `write_calls` counts what the session asked for. This fan-out is
a property of what the session asked for, so it moved to the counter that still
holds it; combining can never merge across files, because a run is keyed by
file.

**`--select-file` and a boundary piece is a real gap and it is not this entry.**
`FluxDown/native/engine/src/bt_partfile.rs` documents the case: the bytes of an
*unselected* file inside a boundary piece are discarded, so those pieces can
never be verified or uploaded afterwards. `bit-cli` has `--select-file` and a
`seed` command, so it has that problem too. This fixture selects every file, so
it does not touch it. Filed as [T-184](#t-184-a-boundary-piece-under---select-file-has-no-decided-behaviour)
rather than folded in here, because it is a decision about what `seed` may
claim after a partial download and not an arithmetic bug.

```
$ cargo test -p bit-cli-core --test webseed_e2e
test a_piece_that_straddles_a_boundary_splits_into_one_slice_per_file ... ok
test the_last_block_of_a_non_final_piece_is_four_kibibytes ... ok
test a_block_that_straddles_a_boundary_is_fetched_as_one_request_per_file ... ok
test a_torrent_whose_pieces_straddle_every_boundary_downloads_byte_for_byte ... ok
```

### T-190 The rule for where a payload lands says one thing and the code does another

Source:      found running `check-allocation.ps1` during T-018's review, 2026-08-22
Category:    disk-io
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-22T10:38Z, with the premise corrected below

Problem:     `crates/bit-cli-core/src/engine.rs` said, at :575-577 before
             2026-08-22, "A caller that
             named an output directory gets exactly that directory. Otherwise
             the session's rule applies and a multi-file torrent goes into a
             directory named after itself", and passes `subfolder: false` when
             `--dir` was given. A multi-file torrent downloaded with `--dir out`
             lands at `out/<name>/...` anyway.
Relevance:   It is not a data bug: the bytes are right and the layout is what
             every end-to-end test asserts. It is a comment that describes a
             behaviour this tool does not have, in the function that decides
             where somebody else's bytes are written, and it is the kind of
             claim `RULES.md` says costs a session every time.
Approach:    Decide which is true and make the other match. The evidence says
             the behaviour is intended: `webseed_e2e.rs` reads its results back
             from `out.path().join("album")` in three separate tests, including
             the multi-file alignment one, and all of them pass. So the comment
             is probably the wrong half. What has to be read before changing it
             is what `subfolder: false` **does** achieve, because
             `SafeStorageFactory` uses it at `storage.rs:410` to decide its own
             path plan while `AddTorrentOptions::output_folder` goes to the
             session as well, and the extra directory may be the session's
             rather than the factory's. If it is the session's, then
             `subfolder: false` prevents a **second** copy of the name rather
             than the first, and the comment should say that.
Acceptance:  The comment and the behaviour agree, and a test names the landing
             path for a multi-file torrent with `--dir` given explicitly so the
             next reader does not have to run one to find out.

**How it was found, and the script it had broken.**
`scripts/check-allocation.ps1` builds a multi-file torrent named `payload` and
looked for the result at `<outDir>/movie.bin`, which is what the comment above
describes. The file is at `<outDir>/payload/movie.bin`. `Test-Path` on a path
that is not there gives a length of zero and a hash of nothing, so the script
reported **the payload does not match the source** and **reserved 0 bytes** on
all four allocation methods, while every download was byte for byte correct.

It failed the same way on the tree before [T-018](#t-018-the-write-path-issues-one-operation-per-16-kib-block)
landed, checked in a worktree at `f46d4fd`, so it is not that change. The last
committed record, `bench/allocation-20260820T005250659Z.json`, passed, so it
broke somewhere between 2026-08-20 and now and nothing noticed: this script is
not in `gates.ps1` and nothing else runs it.

The paths are corrected and the script measures again, with every method
behaving as [T-012](#t-012-preallocation-is-not-implemented) documents:

```
method   reserved  allocated  sparse  free delta  matches source
none     32.00 MiB 32.00 MiB  False   31.94 MiB   True
sparse   32.00 MiB 0 B        True    4.00 KiB    True
prealloc 32.00 MiB 32.00 MiB  False   32.01 MiB   True
falloc   32.00 MiB 32.00 MiB  False   31.91 MiB   True
```

`sparse` reserving four kilobytes of volume for a 32 MiB file, against
`prealloc` reserving all of it, is the distinction the whole script exists to
draw, and it had been invisible.

**The premise, corrected: the comment was true, and it was about a different
flag.** The `Problem` above reads "a caller that named an output directory" as
`--dir`. It is not. `--dir` becomes the **session's** default output directory,
`Engine`'s `download_directory`, at `crates/bit-cli/src/swarm.rs:240` and
`crates/bit-cli/src/cmd/download.rs:364`, so it takes the `None` branch of the
match, `subfolder: true`, and `out/<name>/` is what that branch is for. The
comment's "output directory" is `AddOptions::output_folder`, whose own field
doc at `crates/bit-cli-core/src/engine.rs:146` already said "write here instead
of the session default", and which has exactly one caller in this tree:
`crates/bit-cli/src/cmd/seed.rs:259`, naming a payload root it has already
resolved. So no behaviour was wrong and nothing about where bytes land changed.

The `Approach` guessed half of it. `subfolder: false` does prevent a **second**
copy of the name rather than the first, which is what `seed` needs when
`--data <parent>/<name>` has already resolved to the torrent directory. The
other half is wrong: the extra directory is the **factory's**, not the
session's. `SafeStorage` joins its own `root` for every path it opens,
`crates/bit-cli-core/src/storage.rs:1055` and `:1199`, and that root is decided
at `storage.rs:410` from `subfolder_for`, `storage.rs:1337`. librqbit's session
computes an output folder of its own by the same rule at
`librqbit-9.0.0/src/session.rs:1286-1296`, but with a storage factory supplied
that value reaches only the default filesystem storage,
`librqbit-9.0.0/src/storage/filesystem/fs.rs:133`, and `Session::delete`, which
deletes through the storage trait anyway and which this tree never calls.

What was really wrong is that the sentence could be read as `--dir` by anyone
who did not already know the field, and two readers did read it that way: it is
what pointed `check-allocation.ps1` at `<outDir>/movie.bin`, and it is what
this entry was filed on. The comment now names both flags and says where a
`--dir` download lands, at `engine.rs:576-585`.

Acceptance, met: `dir_lands_a_multi_file_torrent_under_its_own_name_and_a_single_file_one_directly`
in `crates/bit-cli/src/cmd/download.rs` fetches both a multi-file and a
single-file torrent with `--dir` given explicitly and asserts the directory
each lands in, and that neither lands in the other's. The per-add override is
already pinned by `either_spelling_of_data_seeds_the_same_payload`,
`crates/bit-cli/src/cmd/seed.rs:829`.

```bash
cargo test -p bit-cli --lib dir_lands_a_multi_file
```

```
test cmd::download::tests::dir_lands_a_multi_file_torrent_under_its_own_name_and_a_single_file_one_directly ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 377 filtered out
```

### T-184 A boundary piece under --select-file has no decided behaviour

Source:      split out of [T-177](#t-177-a-piece-that-spans-a-file-boundary-has-no-adversarial-fixture) while building its fixture, 2026-08-21
Category:    disk-io
Priority:    P2
Effort:      M
Status:      **done**, with the premise corrected below

Problem:     `--select-file` downloads a subset of a multi-file torrent. A
             piece that straddles a boundary between a selected file and an
             unselected one contains bytes of both, and nothing in `bit-cli`
             says what happens to the unselected half.

             Whatever happens, the piece cannot be verified afterwards without
             it, because a piece hash covers the whole piece. So a torrent
             downloaded with `--select-file` may hold pieces it can never
             prove, and `bit-cli seed` will offer them.
Relevance:   `FluxDown/native/engine/src/bt_partfile.rs` documents the case
             directly: the bytes of an unselected file inside a boundary piece
             are discarded, so those pieces can never be verified or uploaded
             afterwards. That tree carries a partfile abstraction to hold them
             instead, which is the shape of the answer.

             `bit-cli` has both halves of the problem and neither is written
             down. [T-013](#t-013-selecting-a-subset-of-files-still-creates-all-of-them)
             is closed, so a subset selection now creates only the files it
             selected, which means the unselected half of a boundary piece has
             nowhere on disk to go at all. And `seed` exists, so those pieces
             are offered to a swarm.

             The failure is quiet in the way this project keeps finding: a
             seeder that announces a piece it cannot serve looks to a peer like
             a peer that lies, and gets dropped. It is the same family as
             [T-074](windows.md), a false hash-check pass, and
             [T-177](#t-177-a-piece-that-spans-a-file-boundary-has-no-adversarial-fixture),
             a payload that hashes and is wrong.
Approach:    Decide first, then measure. Three positions, and the first is the
             recommendation:

             1. **Announce only whole pieces the selection covers.** A boundary
                piece is not announced by `seed` and is reported as unverifiable
                by `verify`. Costs nothing on disk, is honest to the swarm, and
                matches how the web seed bridge already decides its bitfield:
                `webseed/bridge.rs` announces only pieces a source covers **in
                full**, and the reasoning is identical.
             2. Keep the unselected bytes, in the file or beside it. Correct
                and complete, and it contradicts T-013, which exists because
                creating files a caller did not ask for was a defect.
             3. Refuse a selection whose boundary pieces are not whole. Simple
                and wrong: it refuses the common case, because a boundary that
                falls on a piece edge is the exception rather than the rule.

             Position 1 needs the piece-to-selection map that
             `Layout::split_by_file` already provides, and the bitfield filter
             `seed` does not yet have.
Acceptance:  A three-file fixture whose boundaries all fall inside pieces, at
             the piece length [T-177](#t-177-a-piece-that-spans-a-file-boundary-has-no-adversarial-fixture)
             already uses, downloaded with `--select-file` naming the middle
             file only. `verify --json` names the boundary pieces as
             unverifiable rather than bad, `seed --json` does not announce
             them, and a second client fetching from that seeder never requests
             one. The distinction between "unverifiable" and "bad" is the part
             a test has to pin: they are different words for the caller and the
             same symptom on the wire.

**The premise is wrong, and the measurement is what found it. The correction
goes here rather than over the text above**, the way
[T-017](#t-017-concurrent-receive-paths-contend-on-the-payload-file) and
[T-021](peers.md) established.

This entry says a selection "may hold pieces it can never prove, and `seed`
will offer them". It can prove them, and `seed` offers exactly what it can
serve. The unselected half of a boundary piece is not discarded and does not
have nowhere to go: it is written into the file it belongs to, because a piece
is verified against its whole hash and the write path is addressed by file
offset with no notion of a selection. So the piece verifies, the session holds
it honestly, and a seeder announcing it is telling the truth.

[T-013](#t-013-selecting-a-subset-of-files-still-creates-all-of-them)'s own
closing said so in one line and this entry was written without reading it:
"a piece that spans a selected file and an unselected one still writes into
both, and both are created because both were written." One command would have
checked it. That is another entry written from a specification rather than from
the binary, which is the common cause `INDEX.md` already names for the three
whose titles are now known false.

**What the measurement found instead is worse, and nothing said it.** A fixture
of three files at the odd piece length
[T-177](#t-177-a-piece-that-spans-a-file-boundary-has-no-adversarial-fixture)
uses, `a.bin` 3,000,000, `b.bin` 1,000,000 and `c.bin` 3,000,000, has piece 0
inside `a.bin`, piece 1 straddling a/b, piece 2 straddling b/c and piece 3
inside `c.bin`. Selecting `b.bin` alone:

```
have=[false, true, true, false]   progress=3,973,120   finished=true
a.bin: 3,000,000 bytes of 3,000,000, correct=false, zero_bytes=1,990,558
b.bin: 1,000,000 bytes of 1,000,000, correct=true
c.bin: 1,959,680 bytes of 3,000,000, correct=false
```

`a.bin` lands at its **full length** holding 1,013,440 real bytes and the rest
zeroes. In a directory listing it is indistinguishable from a complete file.
`c.bin` lands short. Which of the two happens depends on where the boundary
write ended, so the same flag produces a file that looks finished and a file
that looks truncated in one run, and before this neither was mentioned
anywhere.

**Reported, because it cannot be prevented.** Not writing those bytes would
make the boundary piece unverifiable, which is what the entry wrongly believed
already happened; writing them beside the file contradicts
[T-013](#t-013-selecting-a-subset-of-files-still-creates-all-of-them), which
exists because creating files a caller did not ask for was the defect. So they
are written where they belong and named. `download --json` gains
`torrents[].partial`, one row per unselected file a boundary piece writes into:
`{index, path, bytes, on_disk, length}`. The three lengths are separate on
purpose, and `on_disk == length` is the row worth reading. The same thing goes
to stderr, before anything is fetched, so a caller about to see files it did
not ask for knows why they are there.

`Layout::selection_spill` computes it with no I/O. Only the **first and last**
piece of an unselected file can be shared with another file, because every
piece between them lies entirely inside it, so it is a walk of the file list
rather than of the piece list. That is
`FluxDown/native/engine/src/bt_partfile.rs`'s observation in
`boundary_segments`; what that tree does with the bytes, a sidecar partfile, is
position 2 above and is not taken.

**`verify` could not tell "outside the selection" from "wrong".** It has no
selection of its own, so verifying what a `--select-file` download wrote
reported every piece outside the selection as a failure and exited non-zero.
That is true of the bytes and wrong about the run: nothing ever asked to fetch
them. `verify --select-file` and `--exclude-file` list them under
`not_selected`, leave them unread rather than hashing bytes nobody asked for,
count `pieces_ok` against what was asked for, and measure `have_share` against
`selected` rather than `total`. That last part is not cosmetic: on this fixture
the selection is 2,048 bytes of a 3,700 byte torrent, so a run that got
everything it asked for would otherwise read 55.35 per cent complete.

The flags are `crate::selection`, shared with `download`, because a second copy
of an index parser is a second set of off-by-one bugs. The one difference is
the file count: `verify` has the metainfo on disk before it parses a flag and
passes it, so `--exclude-file` alone resolves to its complement here.
`download` may be handed a magnet and passes `None`, which is why the same flag
does nothing there. That is [T-185](cli-surface.md), filed while measuring
this.

**Acceptance, in its corrected form.** The entry asked for `verify` to name the
boundary pieces as unverifiable and for `seed` not to announce them. Both are
the wrong way round, and the tests assert what is true instead:

```
$ cargo test -p bit-cli --lib -- a_selection_reports a_download_with_no_selection a_selection_separates a_boundary_piece_under an_exclusion_alone a_seeder_of_a_selection
test selection::tests::an_exclusion_alone_needs_the_file_count ... ok
test selection::tests::an_exclusion_alone_is_every_other_file_when_the_count_is_known ... ok
test cmd::verify::tests::an_exclusion_alone_selects_the_complement ... ok
test cmd::download::tests::a_download_with_no_selection_reports_no_partial_files ... ok
test cmd::download::tests::a_selection_reports_the_files_its_boundary_pieces_write_into ... ok
test cmd::verify::tests::a_boundary_piece_under_a_selection_verifies ... ok
test cmd::verify::tests::a_selection_separates_pieces_nobody_asked_for_from_pieces_that_are_wrong ... ok
test cmd::seed::tests::a_seeder_of_a_selection_holds_the_boundary_pieces_and_says_so ... ok
test result: ok. 8 passed; 0 failed
```

`TorrentFixture::straddling` is the small version of the same shape, three
files at a 1024 byte piece length with both boundaries inside a piece, chosen
so the two outcomes differ: `a.bin` lands at 1500 of 1500 holding 476 real
bytes and `c.bin` at 872 of 1500. The download test asserts the report and then
asserts the disk agrees with it, because the first alone is a claim about
arithmetic. `a_boundary_piece_under_a_selection_verifies` is the corrected
premise stated as a test: pieces 1 and 2 each hold bytes of a file nobody
selected and both verify. `a_seeder_of_a_selection_holds_the_boundary_pieces_and_says_so`
is the `seed` half: 2048 bytes of 3700, `complete: false`, which is exactly the
two boundary pieces.

**One thing that test found on the way, and it is not this entry's.** `seed
--data` is the session's download directory, so a multi-file torrent's payload
is expected at `<data>/<name>/`; `verify --data` accepts either the parent or
the torrent directory and picks whichever holds the first file. Pointing `seed`
at the torrent directory reports `have: 0` and warns "this is a partial seed",
which is the wrong reason for the right observation. Filed as
[T-186](cli-surface.md).

**Position 3 stays rejected and position 1 turns out to need no code.** A
selection whose boundary pieces are not whole is the common case, so refusing it
refuses the common case. And announcing only whole pieces the selection covers,
the recommendation, is already what happens: the announced set is the hash
check's result, the boundary pieces are in it, and they belong there because
their bytes are all present. The rule the web seed bridge uses for its own
bitfield is a different rule for a different reason: a **source** scoped to part
of a payload genuinely cannot serve a piece it holds half of, because the other
half is on a mirror it does not control.


### T-188 A chunk starting on a file boundary creates the file before it

Source:      found while measuring [T-185](cli-surface.md), 2026-08-22
Category:    disk-io
Priority:    P3
Effort:      S
Status:      **done**, 2026-08-22T03:20Z

Problem:     A file the selection did not choose lands on disk as a zero byte
             file when the selection starts after it and no piece straddles the
             boundary between them. Nothing is written into it, and it is
             created anyway.
Relevance:   This is the exact state [T-013](#t-013-selecting-a-subset-of-files-still-creates-all-of-them)
             closed on, so its closing claim, "finishes with only the selected
             file present under `--dir`", is true in one direction and not the
             other. **The correction is written under T-013.**

             It is P3 rather than P2 because it costs a directory entry and no
             bytes: an empty file, not a fetch. It is worth an entry because it
             is the difference between a selection that is invisible on disk and
             one that leaves a trail, and because a caller scripting over
             `--dir` cannot tell an empty payload file from a skipped one.
Approach:    The cause is upstream and the fix is local.

             `librqbit-9.0.0/src/file_ops.rs:319-322` walks the file list to
             place a chunk and skips a file with `if absolute_offset > file_len`,
             strictly greater. A chunk that begins at exactly the first byte of
             the next file leaves `absolute_offset == file_len` for the file
             before it, so that file is **not** skipped: `remaining_len` is 0,
             `to_write` is 0, and `pwrite_all_vectored(file_idx, file_len, [])`
             is called with nothing to write.

             `SafeStorage::pwrite_all_vectored` in
             `crates/bit-cli-core/src/storage.rs:1119` takes `Intent::Write`
             before it looks at the slices, and `Intent::Write` is what creates
             a file that is not there. The empty slices are skipped inside the
             closure, after the file exists. `pwrite_all` at :1107 has the same
             shape.

             So: return `Ok` for a write of zero bytes before opening anything.
             That is correct independently of the upstream off-by-one, because
             a write of no bytes changes no file, and it keeps
             [T-013](#t-013-selecting-a-subset-of-files-still-creates-all-of-them)'s
             rule intact: a write creates a file, and this is not a write.

             Do **not** reach for the selection here. T-013's whole argument is
             that storage needs no selection plumbed into it, and that argument
             still holds: a piece that genuinely straddles into an unselected
             file writes real bytes and the file is created, which is
             [T-184](#t-184-a-boundary-piece-under---select-file-has-no-decided-behaviour).
Acceptance:  `bit-cli download <MULTI> --select-file 1` on a torrent whose file
             0 ends exactly on a piece boundary finishes with only file 1 under
             `--dir`, and the same run's `partial` array stays empty because
             nothing spilled.

**Measured, not read.** The `donor` fixture from [T-185](cli-surface.md):
`extra-a.txt` 1024 bytes at index 0 and `shared.bin` 4096 at index 1, at a 1024
byte piece length, so file 0 is exactly piece 0 and nothing straddles.

```
$ bit-cli --json download donor.torrent --dir out --web-seed-only \
    --web-seed http://127.0.0.1:57364/ --web-seed-mode prefix \
    --no-torrent-web-seed --no-tracker --no-dht --no-lsd --port 0 \
    --select-file 1 --stop-after 20s
stopped= completed downloaded= 4096
-rw-r--r--  0     extra-a.txt
-rw-r--r--  4096  shared.bin
```

`partial` is `null` on that run, which is the proof that no piece spilled: this
is not [T-184](#t-184-a-boundary-piece-under---select-file-has-no-decided-behaviour)
seen again. The reverse direction, `--exclude-file 1`, resolves to the same
selection read the other way and leaves `shared.bin` absent entirely, because
there is no file after index 1 for a boundary chunk to start on.

The hash check is not the cause. `--hash-check-only --select-file 1` against an
empty directory creates `shared.bin` at 4096 and does **not** create
`extra-a.txt`, so the file appears when the first chunk is written and not
before.

**Closed 2026-08-22T03:20Z**, and it is the two lines the approach named.
`SafeStorage::pwrite_all_vectored` and `pwrite_all` answer a write of no bytes
before they open anything. That is correct independently of the upstream
off-by-one, which is why it goes here rather than being carried until upstream
moves: a write of no bytes changes no file, so opening one for it is work with
no result and, in this storage, a side effect.

Same command, after:

```
$ bit-cli --json download donor.torrent --dir out --web-seed-only     --web-seed http://127.0.0.1:52346/ --web-seed-mode prefix     --no-torrent-web-seed --no-tracker --no-dht --no-lsd --port 0     --select-file 1 --stop-after 20s
stopped= completed downloaded= 4096 partial= None
files on disk: donor/shared.bin
```

`partial` is still `null`, which is what says this was never
[T-184](#t-184-a-boundary-piece-under---select-file-has-no-decided-behaviour):
nothing spilled, and the file was created by a write with nothing in it.

Two tests. `storage::tests::a_write_of_no_bytes_creates_no_file` is the unit,
and it checks the plain and the vectored form and then that a write with bytes
in it still creates and still lands.
`a_selection_that_starts_after_file_zero_leaves_it_off_the_disk` is the
end-to-end one and was run against the old behaviour first, where it fails with
`["donor/extra-a.txt", "donor/shared.bin"]`.

[T-013](#t-013-selecting-a-subset-of-files-still-creates-all-of-them)'s closing
claim is true again, and its correction note stays where it is: a doc that was
wrong for a session is part of the record.
