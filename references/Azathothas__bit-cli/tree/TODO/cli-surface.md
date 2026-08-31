# CLI surface gaps

Everything in A3 that parses today and does not yet do what `--help` says. A
flag that looks like it works and does not is worse than one that errors, so
each of these either ships or starts refusing.

This file is not in the A4 file list. It exists because these items belong to no
upstream category, and dropping them to match a list would lose them.

---

### T-110 The --jsonl event stream is incomplete

Source:      the operator's brief
Category:    cli
Priority:    P1
Effort:      M
Status:      **done**

Problem:     A3.10 documents eleven event types. `download` emits
             `session_start`, `torrent_added`, `metadata_resolved`,
             `piece_verified`, `file_completed`, `source_added`,
             `source_failed`, `progress`, `torrent_completed`, and `error`.
             `session_end` is emitted by nothing, and `seed`, `peers`, and
             `trackers` emit only `session_start` and `progress`.
Relevance:   An agent consuming NDJSON needs the stream to end with something
             that says it ended, or it cannot tell "finished" from "the pipe
             broke".
Approach:    Emit `session_end` from the one place every command returns
             through, carrying the exit code and the elapsed time, so it cannot
             be forgotten per command. Then audit each command against the
             eleven.
Acceptance:  `bit-cli <any command> --jsonl` ends with a `session_end` event
             carrying `exit_code`, and `docs/schema.md` has a worked example of
             every type.

**Done.** `session_end` is emitted from `bit_cli::run`, the one place every
command returns through, so a command added later cannot forget it. It carries
`exit_code`, `exit_status`, `ok`, `elapsed_ms`, `elapsed_human`, and `error`
when there was one.

```
$ bit-cli --jsonl info album.torrent | tail -1
{"at":"2026-08-20T15:01:59.553Z","elapsed_human":"4ms","elapsed_ms":4,
 "exit_code":0,"exit_status":"success","ok":true,"seq":0,"type":"session_end"}
```

Three tests: `every_jsonl_run_ends_with_session_end` walks every command that
runs without a network and checks the last line of each,
`a_failed_jsonl_run_ends_with_session_end_carrying_the_error` checks the
failure shape, and `session_end_does_not_appear_outside_jsonl` checks that
`--json` and text output do not gain a stray object.

The one case with no event is a flag that `clap` refuses: before the arguments
parse there is no format to emit one in, so a usage error ends the stream by
ending it. That is stated in `run`.

**It broke one reader, and that is worth knowing before adding another event.**
`scripts/interop-roundtrip.ps1` read the seeder's report as the last line of
its `--jsonl` stream, which was right until `session_end` became the last line.
Both seeding cases then failed with "bit-cli seed served no peer" while the
transfer had in fact succeeded: 490,012 bytes uploaded to `aria2/1.37.0`, in
the stream, two lines up. The script now walks backwards for the object whose
`kind` is `seed`. Anything else consuming this stream by position rather than
by `type` or `kind` has the same fault.

The audit the Approach asks for is `docs/schema.md`, built by
[T-117](#t-117---schema-version-has-no-schema-behind-it). Fourteen event types
are documented, not the eleven A3.10 lists: `source_cooling`, `peer_redial`,
and `bench_sample` were added by later entries.

### T-111 piece_verified and file_completed are derived from polling

Source:      the operator's brief
Category:    cli
Priority:    P2
Effort:      M
Status:      open

Problem:     Both events come from comparing consecutive snapshots on the
             report interval rather than from the engine pushing them. The
             counts are exact; the timestamps are only as precise as
             `--report-interval`.
Relevance:   For a caller measuring per-piece timing, an event stamped up to a
             second late is not a measurement. Rule 0.2 says an estimate has to
             say it is one.
Approach:    Either take a push notification from `librqbit` if one exists, or
             name the imprecision in the event: add `"timing": "polled"` and
             the interval, so a consumer knows what the timestamp is worth.
Acceptance:  Each `piece_verified` event says how its timestamp was obtained.

### T-112 --log-file does not write or rotate anything

Source:      the operator's brief
Category:    cli
Priority:    P1
Effort:      M
Status:      **done**

Problem:     `--log-file`, `--log-max-size`, and `--log-max-files` all parse,
             and `--log-max-size` is even validated, but no file is opened.
Relevance:   A cron job that cannot keep a log has no way to explain a failure
             after the fact.
Approach:    Append to the named file, rotate at `--log-max-size` by renaming
             to `.1`, `.2`, and so on, and keep `--log-max-files` of them.
             Rotation on Windows has to handle a reader holding the file open,
             which means retrying the rename with backoff.
Acceptance:  A run with `--log-file x.log --log-max-size 1KiB --log-max-files 3`
             produces `x.log`, `x.log.1`, `x.log.2`, and no `x.log.3`.

**Done, exactly as the acceptance states it.**

```
$ bit-cli download torrent_c.torrent --dir out --web-seed file:///.../ \
    --web-seed-only --port 0 --allow-overwrite -vvv \
    --log-file x.log --log-max-size 1KiB --log-max-files 3

-rw-r--r--  258 x.log
-rw-r--r-- 1022 x.log.1
-rw-r--r-- 1002 x.log.2
```

`crates/bit-cli/src/logging.rs` holds a `Rotating` writer behind a mutex, given
to `tracing_subscriber` as a second destination through `MakeWriterExt::and`.
Four decisions in it, each for a reason a reader should not have to guess:

- **It adds a destination rather than replacing stderr.** Ground rule 0.11 says
  stderr carries the logs, and it should hold whatever else is set. A caller
  who wants only the file redirects stderr, which is one shell operator against
  a rule that would otherwise have an exception in it.
- **`--log-max-files N` is N files in total**, the live one included, so `3`
  leaves `x.log`, `.1`, and `.2`. `1` keeps no history and starts the live file
  over rather than leaving a rotated copy the caller said it did not want.
- **The size is seeded from the file that is already there.** Appending to a
  full log rotates on the first write rather than after this process has
  written a whole file's worth of its own.
- **A rename that will not happen is skipped, not fatal.** Windows refuses to
  rename a file another process has open, and a log file is exactly the file
  someone is tailing. Five attempts with a doubling wait covers a reader
  between reads; past that the log keeps growing, which is better than losing
  a line or failing the run.

Five tests in `logging::tests`, four of them driving the writer directly
because a run producing exactly 1 KiB of log lines would be testing the log
volume rather than the rotation:
`rotation_keeps_the_live_file_and_max_files_minus_one_behind_it`,
`a_zero_max_size_never_rotates`,
`one_file_total_truncates_instead_of_keeping_a_copy`,
`an_existing_full_log_rotates_on_the_next_write`, and
`a_run_with_a_log_file_writes_to_it_and_still_writes_to_stderr`.

### T-113 Metalink is not implemented

Source:      the operator's brief, decision 7.7
Category:    cli
Priority:    P1
Effort:      L
Status:      **done**

Problem:     `source.rs` classifies `.meta4` and `.metalink` and reports that
             they need resolving. Nothing resolves them. `quick-xml` is already
             a dependency and unused.
Relevance:   Metalink is in scope because it is a torrent format: one file
             carrying a `.torrent`, a mirror list, and checksums, which is
             exactly the hybrid case this tool exists for. Everything a user
             would otherwise assemble with `--web-seed` repeated twelve times,
             a Metalink gives in one file.
Approach:    RFC 5854 for `.meta4`, plus the older `.metalink`. Parse the
             `<metaurl mediatype="torrent">` entry to find the torrent, the
             `<url>` entries to register as web seeds, `<size>`, and
             `<hash type="sha-256">`. Then the part that matters: verify the
             checksums the Metalink supplies against the piece hashes the
             torrent supplies, and report loudly if they disagree, because that
             means one of the two is wrong and the caller needs to know which.
             Out of scope: language and OS filtering, version negotiation.
Acceptance:  `bit-cli download release.meta4` resolves the torrent, registers
             every listed mirror, downloads, and verifies against the
             Metalink's own checksum. Run against a real `.meta4`.

**The parser is done and the wiring is not.** `bit_cli_core::metalink` reads
both versions in one pass over `quick-xml` events, which is the half of this
with all the format knowledge in it:

| | Metalink 4, RFC 5854, `.meta4` | Metalink 3, `.metalink` |
| --- | --- | --- |
| files | `<file>` under `<metalink>` | `<file>` under `<files>` |
| hashes | `<hash type="sha-256">` | `<hash type="sha256">` under `<verification>` |
| mirrors | `<url>` | `<url type="http">` under `<resources>` |
| torrent | `<metaurl mediatype="torrent">` | `<url type="bittorrent">` |
| preference | `priority`, **lower** first | `preference`, **higher** first |

Both come out of the parser under version 4's rule, so a caller sorting by
`priority` gets the document's intent whichever file it read, and `sha-256` and
`sha256` normalise to one spelling.

Four things it refuses or drops on purpose, each with a test:

- A `<metaurl>` that is not a torrent is dropped rather than registered as a
  mirror. It names another document, so a source pointed at it would serve XML
  as payload.
- The per-piece `<hash piece="0">` entries under `<pieces>` are not whole-file
  checksums and are not collected as if they were. Without that a version 3
  file comes out with four checksums, two of which are one piece each.
- `ftp:` mirrors are kept out of the source list and counted, because a source
  this cannot fetch from is worse than one it never had.
- A document that simply stops is refused. `check_end_names` catches a
  mismatched closing tag and not an EOF, so the parser counts depth and fails
  at zero-plus-open. A truncated mirror list that parses is the "plausible
  wrong answer" this repository keeps finding.

```
$ cargo test -p bit-cli-core --lib metalink
test result: ok. 15 passed; 0 failed
```

**The wiring is done and the five steps are closed.** `bit-cli download
release.meta4` reads the document, fetches the `.torrent` it names, registers
every mirror as a source, downloads, and checks the payload against the
document's own checksum.

```
$ pwsh scripts/check-metalink.ps1
verdict: pass          (ten cases on loopback)
$ pwsh scripts/check-metalink-real.ps1
verdict: pass          (four cases against download.documentfoundation.org)
```

Both records are committed: `bench/metalink-20260821T045751697Z.json` and
`bench/metalink-real-20260821T045805559Z.json`.

What each step turned into.

1. **Resolving the torrent.** `source::resolve_metalink` reads the document,
   takes `single_file()`, and fetches the torrent. Not `torrents[0]`:
   `torrents_by_priority()`, and each in turn until one parses, because a
   document that lists several torrents is a mirror list for the `.torrent`
   itself and its first choice can be gone. The failures are kept and reported
   as `torrent_fallbacks`, so a report says the preferred one was not the one
   used. `source::fetch_torrent` now returns the bytes as well as the parse,
   and `Engine::add_bytes` hands those exact bytes to the session.
   **Fetching the URL twice was the alternative and it is wrong**: the session
   would fetch a URL this run has already fetched, and two fetches of one URL
   can return two documents, so the report would describe one torrent while the
   session downloaded another.
2. **Registering the mirrors.** `webseed_args::collect` takes an
   `Option<&MetalinkFile>` and emits one `SourceSpec` per mirror in
   `mirrors_by_priority()` order, with `Origin::Metalink`, which already
   existed and had no producer.

   Two things the entry did not anticipate. The composition is **`exact`**, not
   BEP 19's `auto`: a Metalink `<url>` is the complete resource, never a
   directory to append a name to. And `exact` on a multi-file torrent is a
   binding error unless the scope resolves to one file, so the scope is the
   file the document was attributed to. A document that cannot be attributed to
   exactly one file of a multi-file torrent registers **nothing**, because a
   mirror serving one file's bytes into a piece range nobody has identified is
   worse than no mirror.

   `--no-torrent-web-seed` drops them, and its help now says "the torrent's or
   the metalink's". Both mean "the sources the source document declared rather
   than the ones you named", which is one idea under one flag.
3. **Verifying the checksum.** `Checksum::verify_file` streams the file in 256
   KiB reads through `sha2`, `sha1`, or `md-5`. `sha2` was a declared
   dependency of `bit-cli-core` with no user until now.

   An algorithm this cannot compute is an **error, not a pass**. The report
   carries `not_checked` with the reason, and `matched` is absent rather than
   `true`. Every guard that stops the check writes one: a download that did not
   finish, a file that could not be named on disk, an attribution that failed.
   A checksum that was not computed is not a checksum that passed.
4. **Which document is wrong.** This is the part the entry called the part that
   matters, and it turned into two checks rather than one.

   The **size** check costs nothing and runs before a byte is fetched.
   `MetalinkFile::agreement(&Layout)` attributes the entry to a file in the
   torrent and compares the two declared lengths. Lengths that differ mean the
   two documents describe different files, and the caller learns it before
   spending the bytes rather than after.

   The **digest** check runs on a payload the session has already verified
   piece by piece against the torrent's own SHA-1 hashes. That ordering is the
   whole argument: a digest that then disagrees is evidence about the Metalink,
   not about the bytes, and the warning says so in those words.
   `scripts/check-metalink.ps1` proves it rather than asserting it, by hashing
   the payload on disk against the source bytes in the mismatch case.

   Both exit **7**, `HashMismatch`, and the report keeps them apart:
   `agreement.size_agrees` and `checksum.matched`. One exit code because both
   are the same finding, that the payload does not match what the Metalink
   claims about it.
5. **A real `.meta4`.** `scripts/check-metalink-real.ps1`, and it found the one
   thing worth knowing about this format in practice.

**No MirrorBrain instance reachable in August 2026 emits `<metaurl
mediatype="torrent">`.** `download.documentfoundation.org` generates a document
per file on demand, and the one for
`LibreOffice_25.8.7_Win_x86-64_helppack_ast.msi` carries 58 real HTTPS mirrors
with dense `priority` 1 to 58 and `location` codes, three whole-file checksums,
a `<pieces>` block, and an OpenPGP `<signature>`, and **no torrent at all**.
The same is true of `download.opensuse.org` and of every LibreOffice file
checked. MirrorBrain emits a `<metaurl>` only when its operator has configured
torrents, and none of them has. So the shape a user actually meets is a
Metalink with nothing for `bit-cli download` to start from, and the message is
built for it:

```
$ bit-cli download real.meta4
the metalink lists no torrent for LibreOffice_25.8.7_Win_x86-64_helppack_ast.msi,
so there is nothing to download here. It lists 58 HTTP mirror(s); pass one with
--web-seed against a .torrent you already have.
```

`real_with_torrent` closes the loop without faking anything that could be real.
It adds one `<metaurl>` line to the document the mirror generated and changes
nothing else: the payload comes down over the public internet from the 58
mirrors the mirror chose, and the digest it is verified against is the sha-256
The Document Foundation published. Measured on the run recorded in
`bench/metalink-real-20260821T045805559Z.json`: 3,801,088 bytes served, 58
sources registered with `origin=metalink`, **1 of the 58 mirrors actually
served bytes** on that run and 3 on the run before it, and the published
sha-256 matched both times. How many mirrors take part is the swarm's
decision and not a number this controls.

Two other real-document findings, both now tests.

- **Version 4 writes its per-piece hashes as bare `<hash>` children of
  `<pieces>` with no attributes at all.** The parser's rule was version 3's,
  which marks each child `piece="N"`, and it never saw these. They were dropped
  anyway, by the guard that refuses a hash with an empty `type`, which is the
  right answer for the wrong reason: one document written with a `type` on the
  child would have put two piece hashes in `checksums` and let
  `best_checksum()` return twenty bytes of one piece. The parser now tracks the
  depth `<pieces>` opened at and ignores every `<hash>` inside it.
- The OpenPGP `<signature>` block is text under an element the parser does not
  know, and it must not become the value of the element before it.

**What is not covered**, recorded rather than deferred:

- A Metalink named by URL is still classified as `Kind::Url` and handed to the
  session as a torrent, which fails on the bencode parse. Real documents are
  served over HTTP, so this is the common way to meet one. T-154 has it.
- A multi-file Metalink is refused with the list, by `single_file()`. Several
  files is several downloads, and taking the first would report success for one
  of them.
- `--hash-check-only` returns before the checksum check, so a metalink run with
  it reports no `metalink` block at all. The block is about what was
  downloaded, and that flag downloads nothing, but the document's own claims
  could still be reported. T-155 has it.
- Language, OS, and country filtering, and `<signature>` verification. Out of
  scope by the entry.

### T-114 -i/--input-file batch input is not implemented

Source:      the operator's brief
Category:    cli
Priority:    P2
Effort:      M
Status:      open

Problem:     `aria2` takes one source per line with indented option lines
             beneath it applying to that entry only. `bit-cli` has no `-i`.
Relevance:   It is how a script drives a hundred downloads with per-entry
             options, and `-i` is one of the reserved `aria2` letters.
Approach:    Parse the `aria2` format exactly, because the point is that an
             existing input file works unchanged. An unindented line is a
             source; an indented `key=value` line sets an option for the
             preceding source only. Reject an option that is not a known flag
             rather than ignoring it.
Acceptance:  An `aria2` input file with three sources and per-entry `dir` and
             `out` options drives `bit-cli download -i` to the same result
             `aria2c -i` produces.

### T-115 Hooks do not fire for every documented trigger

Source:      the operator's brief
Category:    cli
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T08:00Z

Problem:     `--on-complete` and `--on-error` ran once for the whole `download`
             run. `--on-piece-verified` did not run at all, and neither hook
             runs from `seed`.
Relevance:   `--on-complete` firing once per run rather than once per torrent
             is wrong for a `-j 4` invocation.
Approach:    Fire per torrent, from the same place `torrent_completed` is
             emitted. `--on-piece-verified` is high frequency by construction,
             so it needs a documented cost and probably a rate limit. Arguments
             already arrive through the environment as `BIT_CLI_*` and never by
             interpolation into a shell string, which is the part that matters
             for a torrent-supplied filename.
Acceptance:  `bit-cli download a.torrent b.torrent -j 2 --on-complete <CMD>`
             runs the command twice, once per torrent, with
             `BIT_CLI_INFO_HASH` differing. `docs/` lists every variable.

**Done 2026-08-23T08:00Z**, both clauses, and the acceptance is a test rather
than a run recorded here: `on_complete_fires_once_per_torrent_with_its_own_info_hash`
downloads two torrents at `-j 2`, and the hook creates a directory named
`on-complete-<info hash>`. Two directories, two hashes. Reading what the hook
wrote rather than what the report says is the point: the report is the run's
account of itself and the directories are what actually ran.

**The old shape could not express a mixed run at all.** It picked one hook for
the whole run by `report.failed`, so a run where one torrent finished and one
did not fired `--on-error` for both or `--on-complete` for both, with the first
torrent's info hash and the run's totals, which describes neither.
`a_mixed_run_fires_on_complete_and_on_error` holds the fix.

**`--on-piece-verified` fires now, and the entry's "probably a rate limit" is
answered with a measurement rather than a flag.** One piece is one process and a
process is not free: **1,025 invocations took 47.55 seconds on this machine**,
46 ms each. That number is honest about what it measured and the doc says so:
the command was `cmd /C rem` and a hook is already run through `cmd /C`, so each
invocation started two processes, about 23 ms per `cmd`. Either way a 4 GiB
torrent at a 1 MiB piece length is 4,096 pieces. Two bounds rather than a rate
limit, because a rate limit silently loses notifications and a caller cannot
tell which:

- **Its own thread.** The watch loop hands over a map and returns. Without this
  a hook at that cost would cap the download at tens of pieces a second whatever
  the network could do.
- **A bounded queue, 1,024 deep, and what does not fit is counted.**
  `--json` carries `hooks.skipped` and a run with any warns on stderr. Nothing
  is dropped silently and nothing waits.

`docs/hooks.md` is the second clause: every variable, what it holds, what the
piece hook costs, and what an exit code does.
`every_hook_variable_is_documented` fails when a variable has no row there and
`every_variable_a_hook_sets_is_in_the_list` fails when the code and the list
disagree **in either direction**, the same pattern
[T-118](#t-118-the-short-flag-table-is-not-checked-in-ci) settled for
`docs/flags.md`.

**A defect the acceptance found in the hook runner itself, which had been there
since hooks existed.** `swarm::run_hook` built `cmd /C <command>` with
`Command::arg`. Rust quotes an argument for the C runtime's parser, and
`cmd.exe` does not use that parser: it re-reads the command line with rules of
its own. So a hook whose command contained a quoted path, a redirect or an `&&`
reached `cmd` mangled and exited with "The filename, directory name, or volume
label syntax is incorrect". The acceptance's own hook is
`mkdir "<dir>\%BIT_CLI_HOOK%-%BIT_CLI_INFO_HASH%"`, and the first run of it
fired twice, as asked, and **failed twice**. `raw_arg` is the fix, which is what
`sh -c` had always effectively done on the other platform. Nothing but a hook
with a quoted argument would have shown it.

**`seed` still runs no hooks**, which is the Problem's third clause and is
**not** done. It is not in the Acceptance and is carried as its own entry rather
than left implied: [T-214](#t-214-seed-runs-no-hooks). `bit-cli seed` has no
`--on-*` flag at all, so there is no flag that does nothing; what is missing is
the feature.

```
$ cargo test -p bit-cli --lib hooks::
test result: ok. 6 passed; 0 failed; 0 ignored; 400 filtered out

$ cargo test -p bit-cli --lib on_complete_fires
test result: ok. 1 passed; 0 failed; 0 ignored; 407 filtered out

$ cargo test -p bit-cli --lib on_piece_verified_fires
test result: ok. 1 passed; 0 failed; 0 ignored; 408 filtered out
```

`ACCEPTED_WITHOUT_A_READER` in `cli.rs` is **empty** now. It held
`on_piece_verified` and `index_out`, and both closed on 2026-08-23.

### T-116 -O/--index-out cannot rename a file

Source:      the operator's brief
Category:    cli
Priority:    P3
Effort:      S
Status:      **done** 2026-08-23T07:40Z

Problem:     `-O/--index-out INDEX=PATH` parses and does nothing.
Relevance:   It is a reserved `aria2` letter and the natural answer to a
             torrent whose paths collide on Windows, T-072.
Approach:    Needs a storage wrapper mapping a torrent file index to a
             different on-disk path, which is the same machinery T-071 needs
             for sanitisation. Build them together.
Acceptance:  `bit-cli download <TORRENT> -O 0=renamed.bin` writes the first
             file as `renamed.bin` and `--json` reports the mapping.

**Done 2026-08-23T07:40Z, and no storage wrapper was needed.** The Approach
priced this as a wrapper mapping an index to a path, built alongside T-071. It
is one argument to the function T-071 already built: `paths::plan_with` takes
the overrides and applies each one **before** anything else happens, so a
requested path is sanitised, truncated and disambiguated exactly as a torrent
path is. `plan` is `plan_with` with an empty map.

**That ordering is the whole safety argument, and it is what makes this small.**
`-O 0=../../etc/passwd` renames the file to `__/__/etc/passwd` inside the output
directory; `-O 0=CON.txt` gets `CON_.txt`; `-O 1=a.bin` against a torrent whose
file 0 is already `a.bin` gets `a-1.bin`. Not one of those decisions is new, and
`a_requested_path_cannot_escape_or_name_a_device` is the case that holds it.
Nothing about `-O` could have reached outside the output directory without
first defeating T-071, which is why it is one function rather than two.

**`Reason::Requested` is a new reason and it is first in the enum.** It is the
only one that is a request rather than a defect in the torrent, and `--json`
carries `reasons` in enum order, so a reader scanning a rename sees it before
anything that reads as a complaint. `renamed[].torrent_path` stays the path the
metainfo gives, because the mapping is only useful with both ends in it.

**An index the torrent does not have is a usage error**, checked before the
session starts wherever the count is already known. A magnet has no count until
its metadata resolves, so `-O` now joins `--exclude-file` and an open-ended
`--select-file` in `plan_selection`'s "await the count" branch: the metadata is
resolved first, which is a round trip the magnet was going to make anyway, and
the index is checked against a real file list. Without that, `-O 9=x` against a
five-file magnet would have renamed nothing and said nothing.

**Half of it would have shipped without the second command, and that half was
found by asking.** `verify` looks where the bytes went rather than where the
torrent said, which is [T-076](windows.md), and it builds that answer from
`paths::plan` — which knows nothing about `-O`. So the tree could rename a file
its own verifier then reported as missing. `verify` takes `-O` too now, and
`verify_finds_a_file_renamed_by_index_out_when_it_is_told` holds both
directions: told, `present: true` and `complete: true`; not told, `present:
false` and a `hash_mismatch` document.

`seed` is **not** covered, and this is the residual, named rather than implied:
`bit-cli seed` resolves its payload through the same plan and has no `-O`, so a
payload downloaded with `-O` cannot be seeded from the directory it landed in.
It is `crates/bit-cli/src/cmd/seed.rs:260`, where `AddOptions` is built without
`index_out`. [T-213](#t-213-seed-cannot-serve-a-payload-renamed-by-index-out)
carries it.

```
$ cargo test -p bit-cli --lib index_out
test result: ok. 4 passed; 0 failed; 0 ignored; 395 filtered out

$ cargo test -p bit-cli-core --lib paths::
test result: ok. 35 passed; 0 failed; 0 ignored; 660 filtered out
```

The acceptance itself, `index_out_writes_the_file_where_the_caller_asked`:
`--json` reports `{"index":0,"disk_path":"renamed/first.bin","reasons":["requested"]}`,
the bytes are at that path and byte-identical to the torrent's first file, and
nothing is left at the path the torrent named.

### T-117 --schema-version has no schema behind it

Source:      the operator's brief
Category:    cli
Priority:    P1
Effort:      M
Status:      **done**

Problem:     `--schema-version` prints `1`. There is no `docs/schema.md`, so
             the number refers to nothing a caller can check against.
Relevance:   A versioned contract nobody has written down is not a contract.
Approach:    Document every JSON document and every event type with a worked
             example, generated from the real types rather than written by hand
             so it cannot drift. A test that serialises one of each and checks
             the example still matches is the mechanism.
Acceptance:  `docs/schema.md` exists, covers every `kind` and every event
             `type`, and a test fails when a field is added without updating it.

**Done. Every one of the thirty-one names has a run behind it, and
`schema::NOT_YET_COVERED` is empty.**

The document is generated rather than written. `crates/bit-cli/src/schema.rs`
holds the two tables of names with their descriptions and a flattener that
turns a JSON document into `path -> type` rows, dotting nested objects and
collapsing arrays to `[]`. `crates/bit-cli/src/schema_gen.rs` is a test module
that drives every command in process against fixtures, folds what each run
wrote into a sample per name, renders the whole file, and compares.

```bash
BIT_CLI_UPDATE_SCHEMA=1 cargo test -p bit-cli --lib schema
```

is the only way the file is ever edited.

Seventeen document kinds and fourteen event types, **669 field rows and 992
lines**, up from 444 rows and 751 lines when eight names were still uncovered.
`hash_mismatch` was found while building it: `verify` writes a different `kind`
when a piece does not check out, and nothing had said so.

**The comparison is containment, not equality, and the asymmetry is the
point.** A field added to a report produces a row the committed file does not
have and fails the test. A row the committed file has that a given run did not
produce does not fail, because these runs are timed: a download that finished
before its second report tick emits no `progress`, and one that raced its own
deadline emits no `torrent_completed`. Requiring equality made the check flaky
on the first `--workspace` run, and a flaky contract check is worse than none.
Section headings are still compared exactly, because those do not depend on
timing.

Two more tests hold the ends together.
`every_produced_kind_and_event_is_documented` fails when the program writes a
`kind` the tables do not name, which is what caught `hash_mismatch`, and it
names the command that produced it, because an undocumented `kind` is usually
an error document from a run that was meant to succeed.
`coverage_of_the_documented_names_matches_what_is_recorded` compares the set of
names no run produces against `schema::NOT_YET_COVERED`, which is now empty, so
a name that stops being produced fails the build rather than quietly losing its
field table.

**The eight fixtures, and what each one needed.** None of them touches the
network.

| name | what it needed |
| --- | --- |
| `webseed_test`, `webseed_probe`, `webseed_fetch` | the `FileServer` that was already there, plus `--no-torrent-web-seed` |
| `source_failed` | a source that answers, and fails, inside the run |
| `source_cooling` | the same source with `--web-seed-retry-status 404` and a cooldown |
| `bench_sample` | a `bench disk` run long enough to tick |
| `peers` | a seeder on a thread and `--peer` pointed at it |
| `trackers` | a loopback tracker, and a second tracker that is dead |

Four of them found something.

- **`--no-torrent-web-seed`, or the generator reaches the internet.** The
  fixture torrent carries `https://mirror.example.com/pub/` in its url-list, so
  that was source zero: `webseed fetch --piece 0` fetched from it and failed,
  and `test` and `probe` waited out a connect timeout against a name no test
  should be resolving.
- **A source has to answer to fail.** Both failing runs first pointed at
  `http://127.0.0.1:9/`, which on this machine is blackholed rather than
  refused. The bridge makes a request only when the session asks it for a
  block, so the request sat in a connect that never completed: no error, no
  budget spent, no event, for the 30 seconds until the request timeout. That is
  [T-141](webseed.md), written up with its measurements. Pointed at a path the
  live server does not have, the same run fails in the first second.
- **A fatal status never cools down.** 404 is fatal by default, and a fatal
  status retires a source without spending the error budget a cooldown waits
  out, so `source_cooling` needs `--web-seed-retry-status 404` as well as
  `--web-seed-cooldown`. The two runs are otherwise identical, which is what
  makes the pair worth having: they are the two ends of the same state machine.
- **`bench_sample` needs a run longer than its own sample interval.** At
  4 MiB the disk bench finished in 5 ms and emitted no sample at all. 64 MiB at
  a 10 ms interval emits two. It is the same lesson the soak in
  [T-040](memory.md) turns on, at a different scale.

**`peers` produced nothing at all, and that was the command rather than the
fixture.** It added its torrent paused, and a paused torrent in `librqbit`
9.0.0 never gets its peer stream, so it never announced and never dialled.
Every `bit-cli peers` run ever made reported an empty swarm. That is
[T-142](peers.md), fixed and tested.

The `bench` report itself is deliberately not in these tables. It is a
versioned document of its own, with `report_version` and its own `kind`, and
under `--jsonl` it renders as NDJSON records carrying `record` rather than
`type`, so the generator sees only its events.

```
$ cargo test -p bit-cli --lib schema
test result: ok. 7 passed; 0 failed
```

`--schema-version` still prints `1` and now refers to something whole. Bumping
it is a separate decision and belongs with the first field that is removed or
changes meaning, which has not happened.

### T-118 The short-flag table is not checked in CI

Source:      the operator's brief; premise disproved 2026-08-21, see the correction below
Category:    cli
Priority:    P3
Effort:      S
Status:      **done** 2026-08-23T07:05Z

Problem:     A3.2 requires `docs/flags.md` with the full short-flag table and a
             CI check, so a new subcommand cannot quietly reuse a letter that
             `aria2` assigns to something else. Neither the file nor the check
             exists.
Relevance:   A script written from `aria2` muscle memory doing something else
             silently is the failure this prevents.
Approach:    Generate the table from the `clap` command tree, compare it to the
             reserved list in A3.2, and fail on any letter used for a different
             concept.
Acceptance:  `docs/flags.md` exists and a test regenerates it and fails on
             drift.

**"Neither the file nor the check exists" is false, and both have existed for
some time.** `docs/flags.md` is 79 lines with the table, the two rules, and the
`-v` / `-V` reasoning. Four tests read the `clap` command tree and fail on
drift, and they run in `cargo test`, which is to say in CI on all three
platforms:

| Test | Where | What fails it |
| --- | --- | --- |
| `every_short_flag_is_documented_in_the_flags_table` | `cli.rs:2927` | a short flag with no row in `docs/flags.md` |
| `no_short_flag_is_defined_twice` | `cli.rs:2705` | one letter used twice in one command |
| `short_flags_never_contradict_aria2` | `cli.rs:2741` | an `aria2` letter reassigned to a different concept |
| `short_flags_keep_their_aria2_meanings` | `cli.rs:2436` | `-V` no longer meaning `--check-integrity` |

```
$ cargo test -p bit-cli --lib short_flag
test result: ok. 4 passed; 0 failed; 0 ignored; 303 filtered out
```

The third of those is the one A3.2 actually asked for: it holds the reserved
list: `d` dir, `o` out/output, `j` max-concurrent-downloads, `u`
max-upload-rate, `q` quiet, `c` continue, `V` check-integrity, `O` index-out,
`l` log-file. It requires any flag carrying one of those letters to name the
matching id or not exist.

**One clause of the Acceptance was genuinely unmet, and it is why this stayed
open.** The Acceptance says a test "regenerates it and fails on drift". The test
*asserted* and did not regenerate: it failed with the exact row to add, which a
reader then pasted in. That is a deliberate difference and probably the better
one, see [T-158](#t-158-regenerating-the-schema-deletes-fields-the-sample-did-not-produce),
where the regenerating half of the schema check deletes rows the sample did not
produce, but the entry asked for regeneration and did not get it, so the
honest state is open with the gap narrowed to one clause. Dropped from P2 to
P3: nothing is unprotected.

`docs/flags.md` named the test as `every_short_flag_is_documented`, which is
not its name. Corrected in the same pass. A doc citing a symbol that does not
exist is the same defect class as an entry describing a state the tree is not
in, which is what this correction is.

**Done 2026-08-23T07:05Z**, and the regeneration is a **merge** rather than a
render, which is the only shape T-158 leaves available.

```bash
BIT_CLI_UPDATE_FLAGS=1 cargo test -p bit-cli --lib short_flag
```

Three of the table's five columns, `Scope`, `aria2` and `Note`, are things the
command tree cannot know: nothing in `clap` knows what `aria2` calls a letter or
why `-v` diverges. So `merge_flags_table` keeps an existing row **verbatim**,
adds a row for a flag that has none with those three cells empty for a person,
and drops a row whose flag the binary no longer defines. Rendering the table
instead would delete every hand-written cell in it, which is T-158 arriving in a
second file.

**A second direction of drift was open the whole time and nobody had noticed.**
The old test walked the flags and asked the table about each, so a row for a
flag that no longer exists passed. That is the drift `-O`/`--index-out` would
leave behind if [T-116](#t-116--o--index-out-cannot-rename-a-file) were ever
answered by removing the flag rather than implementing it. Both directions fail
now.

**`-h` is not in the tree the test walks**, which the stale-row check found the
first time it ran. `clap` creates `--help` while **building** a command, and
`Cli::command()` returns one that is not built, so `get_arguments()` does not
carry it. The table's row for `-h` had therefore never been checked in either
direction. `short_flags` adds the pair by hand, with why.

Two tests, not one. The assertion runs against the committed file and the merge
is tested on a fixture of its own, because on the committed file the merge is a
no-op by construction: the assertion fails the build whenever it would not be.
`regenerating_the_flags_table_adds_and_removes_rows_without_touching_prose`
checks that a kept row keeps every hand-written cell, that a new one arrives
empty, that a dead one goes, that "Reserved and not assigned" is untouched, and
that a second run changes nothing.

```
$ cargo test -p bit-cli --lib short_flag
test result: ok. 4 passed; 0 failed; 0 ignored; 386 filtered out

$ BIT_CLI_UPDATE_FLAGS=1 cargo test -p bit-cli --lib every_short_flag_is_documented
test result: ok. 1 passed; 0 failed; 0 ignored; 389 filtered out
$ git diff --stat docs/flags.md
(nothing)
```

### T-144 The MSRV job fails: the tree needs a newer rustc than it claims

Source:      CI run 32386960166, 2026-08-20
Category:    ci
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `ci.yml`'s `MSRV` job pins rustc 1.85.1 and runs
             `cargo check --workspace --locked --all-features`. It fails:

             ```
             serde_with@3.21.0 requires rustc 1.88
             serde_with_macros@3.21.0 requires rustc 1.88
             where <compatible-ver> is the latest version supporting rustc 1.85.1
             ```

             So the minimum supported version the repository advertises is not
             a version the repository builds on, and the job has been red
             since the dependency moved.
Relevance:   An MSRV nobody can build with is worse than none: it fails every
             push, and a red job that is always red stops being read. It also
             misleads anyone packaging this for a distribution with an older
             toolchain.
Approach:    Three ways, and the choice is the operator's rather than the
             build's. Raise the MSRV to 1.88 and say so in `Cargo.toml` and the
             README. Or pin `serde_with` back to the last release that builds
             on 1.85.1, with `cargo update serde_with@3.21.0 --precise <ver>`,
             and add a comment saying why the pin exists. Or drop the MSRV job
             and the claim with it.

             Raising it is the honest default: nothing here needs an old
             toolchain, and pinning a dependency back to keep a number is the
             tail wagging the dog.
Acceptance:  The `MSRV` job passes, and the version it pins is the version
             `Cargo.toml` and the README name.

**Raised to 1.88, which is measured rather than chosen.** 1.88 is the highest
`rust-version` in the resolved dependency graph, and the graph is what says so:

```
$ cargo metadata --format-version 1 --all-features
```

Nine packages ask for it. `serde_with`, `serde_with_macros`, and `hdrhistogram`
are direct dependencies; `time`, `time-core`, `time-macros`, `darling`,
`darling_core`, and `darling_macro` arrive underneath them. Nothing in the
graph asks for more. So 1.88 is not a round number picked to make a job pass:
it is the number the tree already needed while claiming 1.85.

Three files carried the claim and none of them checked the others, which is how
it drifted in the first place. `crates/bit-cli/tests/msrv_is_declared_once.rs`
now ties them together: it reads `rust-version` out of `Cargo.toml` and fails
if `.github/workflows/ci.yml` does not pin exactly that toolchain, or if
`README.md` does not name it, or if the version grows a patch level that
`cargo` would ignore and `dtolnay/rust-toolchain` would not.

```
$ cargo test -p bit-cli --test msrv_is_declared_once
test result: ok. 3 passed; 0 failed
```

**Raising it turned on two clippy lints and both were real.** Clippy suppresses
a lint whose fix needs an API newer than the declared `rust-version`, so the
1.85 claim had been hiding them:

- `manual_is_multiple_of` in `webseed/fetch.rs`, because `u64::is_multiple_of`
  stabilised in 1.87.
- `collapsible_if` in `source.rs`, because let-chains stabilised in 1.88.

Both are fixed rather than allowed. That is the second thing a wrong MSRV
costs: not just a red job, but lint coverage nobody knew was off.

**And the raise had a second cost that had to be paid before the job could go
green.** With `rust-version` at 1.85 the tree also compiled `core::arch`'s
`__cpuid` and `__get_cpuid_max` without an `unsafe` block, because a current
toolchain has made those safe to call. At 1.88 they are still `unsafe fn`, so
`cargo check` under the pinned toolchain failed with two `E0133`s that no
amount of local testing on a current compiler would ever show. Writing the
block and allowing `unused_unsafe` is what compiles under either, and the
allowance carries the note that says when to drop it.

That is the whole argument for having an `MSRV` job at all: the claim is only
worth making if something compiles against it.

**The run is in.** `MSRV` passed in 1m1s on CI run 32440386139, 2026-08-21,
compiling the whole workspace with `--locked --all-features` on rustc 1.88:

https://github.com/Azathothas/bit-cli/actions/runs/32440386139

`Clippy` passed in the same run, which is the other half: the two lints the
raise turned on are fixed rather than allowed.

### T-145 The macOS test job fails to link

Source:      CI run 32386960166, 2026-08-20
Category:    ci
Priority:    P2
Effort:      M
Status:      **done**

Problem:     `Test (macos-latest)` fails during linking, not compilation:

             ```
             error: linking with `cc` failed: exit status: 1
             clang: error: linker command failed with exit code 1
             ```

             It happens for every test binary, `hostile_paths` and
             `bit_cli_core` among them, on `aarch64-apple-darwin`. The linker
             line carries `aws-lc-sys`, `ring`, and `network-interface` build
             outputs, so the first thing to check is which of those three fails
             to produce a library on that target.
Relevance:   macOS is not a release target: decision 9 names
             `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, and
             `x86_64-pc-windows-msvc`. So this is not a shipped platform, and
             the job is testing something nobody gets. It matters because a red
             job trains everyone to ignore the light.
Approach:    Two honest options. Fix the link, which means finding which native
             dependency does not build for `aarch64-apple-darwin` and whether a
             feature choice avoids it, `rust-tls` against `aws-lc-rs` being the
             likeliest lever. Or take macOS out of the test matrix and say in
             the README that it is untested, which is what decision 9 already
             implies.

             Do not leave it red either way.
Acceptance:  Either `Test (macos-latest)` passes, or the matrix does not
             include it and `README.md` says which platforms are tested.

**The entry's premise is wrong and the log says so.** None of the three native
dependencies fails to build. The undefined symbol is ours:

```
Undefined symbols for architecture arm64:
  "_posix_fallocate", referenced from: ...
ld: symbol(s) not found for architecture arm64
```

`bit_cli_core::alloc::fallocate` was written under `#[cfg(unix)]` with an
`extern "C"` declaration of `posix_fallocate`. That compiles on any unix,
because an extern declaration is a promise rather than a lookup, and it links
only where the symbol exists. It does not exist on the Apple platforms, and it
does not exist on OpenBSD either. So the failure had nothing to do with
`aws-lc-sys`, `ring`, or `network-interface`: those three names are on the
linker line because everything is on the linker line. The `ld:` warnings about
`ring` objects built for a newer macOS are warnings, and they are noise here.

The lesson is the cheaper half of the entry: `cfg(unix)` is not a platform, it
is a family, and an FFI symbol needs the platform.

**Fixed by giving each platform the call it actually has.** Linux and the BSDs
keep `posix_fallocate`. The Apple platforms get `fcntl(F_PREALLOCATE)`, which
is the same idea in a different shape: it reserves blocks without moving the
end of the file, it measures from the current end rather than from an absolute
offset, and it takes a contiguous run first and may refuse, so the request is
repeated without that constraint before it counts as a failure. The length is
set afterwards, which is what makes `falloc` mean the same thing on both.
OpenBSD returns a reason and degrades to `prealloc`, exactly as Windows does.

The Apple path cannot be run on this machine, so what was checked here is that
it compiles for the real target with warnings denied:

```
$ rustup target add aarch64-apple-darwin
$ rustc --target aarch64-apple-darwin --edition 2024 --emit=metadata -D warnings <the function>
```

The behaviour is checked by CI. `alloc::tests::falloc_either_works_or_says_why_it_fell_back`
runs on `macos-latest` and asserts that the file ends up 65536 bytes long and
that the outcome is either `falloc` with no note or `prealloc` with a reason,
and `every_strategy_sets_the_length` runs `Falloc` alongside the other three.
So the macOS job stops being a job nobody reads and becomes the evidence for
this entry.

**The link was the first defect and not the only one.** With it fixed, the job
compiled, linked, ran, and failed six tests, all on the same cause and all the
same shape as the first: `sysinfo::platform` was written `#[cfg(unix)]` and
reads `/proc`. macOS has no `/proc`, so every read missed. The report it
produced on an M-series Mac:

```json
"host": {
  "cpu": {"architecture": "aarch64", "logical_cores": 3, "model": "unknown"},
  "memory_total": {"bytes": 0, "human": "0 B"},
  "os": {"name": "Linux", "version": "unknown"},
  "unavailable": ["os.version", "memory_total", "network"]
},
"process": {"cpu_ms": 0, "open_handles": 0, "peak_rss_bytes": 0, ...}
```

`os.name` says `Linux` on a Mac. The module has an `unavailable` list for
exactly this and it was populated correctly, and the field beside it was still
a lie, because the fallback was a hardcoded `"Linux"` rather than a read that
failed. A benchmark carries its environment so two numbers can be compared;
this one would have said two Macs and a Linux box were the same machine.

There is now a third implementation, from libSystem, with no new dependency:
`getrusage` for processor time and the resident high-water mark, which on
Darwin is in bytes where Linux reports the same field in kilobytes;
`proc_pidinfo` for resident size now and for the open descriptor count; and
`sysctlbyname` for the kernel name and version, the product version, the CPU
brand string, and physical memory. Link speeds are not read and say so:
`getifaddrs` plus an ioctl per interface is more than anything here compares
across machines today.

The struct layouts are transcribed from the system headers, and a
transcription that is one field out does not fail, it reads the wrong offset
and returns a plausible wrong number. `const _: () = assert!(size_of::<..>())`
on all three fails the build instead.

Checked here the same way the link fix was, since this machine is not a Mac:

```
$ rustc --target aarch64-apple-darwin --edition 2024 --emit=metadata -D warnings <the module>
```

**The run is in.** `Test (macos-latest)` passed in 2m10s on CI run
32444424026, 2026-08-21:

https://github.com/Azathothas/bit-cli/actions/runs/32444424026

Every job in that run is green, which is the first time the whole matrix has
been. Getting there took four rounds, and each one uncovered the next: the link
failure hid six `sysinfo` failures, which hid [T-152](bench.md), which hid one
last per-platform assertion. A red job does not cost one defect, it costs every
defect behind it.

The last of those is worth naming because it is not a defect.
`sysinfo::tests::the_host_names_its_cpu_os_and_memory` asserted
`host.unavailable.is_empty()`, and the Apple reader reports `network` as
unavailable on purpose: link speeds need `getifaddrs` plus an `SIOCGIFMEDIA`
ioctl per interface, which nothing measured here compares across machines yet,
and saying so beats reporting an empty list as though the machine had no
interfaces. The test now compares the set to `["network"]` on Apple and to `[]`
elsewhere, so a second field going unreadable still fails the build, and so
does this one being fixed without the expectation being updated. That gap is
[T-153](#t-153-link-speeds-are-not-read-on-macos).


### T-146 CI built a Windows binary against the dynamic C runtime

Source:      CI run 32405312793, 2026-08-20
Category:    ci
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `Build (x86_64-pc-windows-msvc)` failed its own static CRT check:

             ```
             check-static: the binary depends on the dynamic C runtime:
             VCRUNTIME140.dll, api-ms-win-crt-math-l1-1-0.dll, ...
             ```

             `.cargo/config.toml` sets `-C target-feature=+crt-static` for all
             three release targets, and it works locally. `ci.yml` sets
             `RUSTFLAGS: -D warnings` at the workflow level, and the
             `RUSTFLAGS` environment variable **replaces** the per-target
             `rustflags` from `config.toml` rather than adding to them. So
             every CI job built without `+crt-static`, and the one job that
             checks caught it.
Relevance:   A Windows binary that needs a Visual C++ redistributable fails to
             start on a clean machine with a dialog box rather than an error a
             script can read. `scripts/check-static.ps1` exists for exactly
             this and did its job.
Approach:    Repeat the flag where the variable is set. The build step now
             carries `RUSTFLAGS: -D warnings -C target-feature=+crt-static`
             with a comment saying why it cannot be inherited.
Acceptance:  `Build (x86_64-pc-windows-msvc)` passes, and the run is named
             here.

**The run is in.** `Build (x86_64-pc-windows-msvc)` passed in 8m44s on CI run
32407214253, 2026-08-20, which is the first run carrying the repeated flag:

https://github.com/Azathothas/bit-cli/actions/runs/32407214253

The job runs `scripts/check-static.ps1` against the binary it just built, so
the pass is the check rather than the absence of a failure.

**`release.yml` was never affected, which is the part worth knowing.** It sets
no `RUSTFLAGS` at all, so `.cargo/config.toml` applies there and every
published artifact has been statically linked. The defect was in the
verification path rather than in the release path, and the verification path
is where it was caught.

### T-150 Clippy pins a floating toolchain, so a Rust release can turn the tree red

Source:      CI run 32437262089, 2026-08-21
Category:    ci
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T09:45Z

Problem:     The `Clippy` job pins `toolchain: stable`, which is whatever
             Rust released most recently. Three lints fired there that do not
             fire on the toolchain in front of me:

             ```
             error: using `chunks_exact` with a constant chunk size
               --> crates/bit-cli-core/src/engine.rs:631:25
               --> crates/bit-cli-core/src/torrent/metainfo.rs:459:10
               --> crates/bit-cli-core/src/tracker.rs:655:10
             ```

             `cargo clippy --workspace --all-targets --all-features --
             -D warnings` is clean on rustc 1.97.1 here, on a cold lint of the
             same crate. So a commit that was green when it was written goes
             red six weeks later with nobody having touched it, and the person
             who finds it is whoever pushed next.
Relevance:   `-D warnings` plus a floating toolchain means the build gate moves
             on its own. This is not hypothetical: it happened in the run
             above, and the three findings were mixed in with four real
             failures, which is exactly the noise that makes a red light stop
             being read. The lints themselves were worth fixing, which is the
             argument for keeping a floating job somewhere rather than for
             having the gate float.
Approach:    Two jobs rather than one, which is the shape that keeps both
             properties. A pinned `Clippy` at a named version is the gate and
             blocks the merge. A second job on `stable`, allowed to fail,
             reports what the next toolchain will want. Bumping the pin is then
             a commit with a message, the same as the MSRV in
             [T-144](#t-144-the-msrv-job-fails-the-tree-needs-a-newer-rustc-than-it-claims).

             The same question applies to `Format`, `Test`, and `Build`, which
             all pin `stable` too. `rustfmt` output is stable across releases
             in practice and the test jobs want the newest compiler, so the
             case is weakest there and strongest for the job that runs lints
             with `-D warnings`.
Acceptance:  `ci.yml` names a version for the gating lint job, a second job
             tracks `stable` without blocking, and this entry records a run
             where the tracking job is red and the gate is green.

**The split is built. The entry stays open until the run that shows it, which
is the third clause of its own Acceptance.**

**The previous revision of this paragraph said there was nothing to demonstrate
against, and that was true of `stable` and false of the next toolchain.** One
command settled it, and it is the command the new job runs:

```
$ cargo +beta clippy --workspace --all-targets --all-features -- -D warnings
error: use of deprecated method `std::sync::atomic::Atomic::<u64>::fetch_update`:
       renamed to `try_update` for consistency
  --> crates/bit-cli-core/src/webseed/bridge.rs:450:14
```

`rustc 1.99.0-beta.1`, on a tree that is clean on 1.98.0. That is the entry's
own scenario six weeks before it becomes everybody's problem, and it is
[T-218](#t-218-the-next-stable-release-fails-the-build-on-a-method-the-bridge-calls)
now. So the demonstration is a real finding rather than a defect introduced to
prove a point, which is what waiting was for.

**The Approach undercounted the blast radius, and the measurement is what
shows it.** It says the case for pinning is "strongest for the job that runs
lints with `-D warnings`" and weakest for `Format`, `Test` and `Build`. But
`RUSTFLAGS: -D warnings` is set at the top of `ci.yml` for the **whole
workflow**, so every job that compiles is a lint gate: the error above is a
`rustc` deprecation, not a clippy lint, and `cargo test` and `cargo build` fail
on it identically. Pinning only `Clippy` would have left six jobs floating and
fixed nothing.

**What landed:**

- **`RUST_GATE`**, one named version, in `ci.yml`'s `env` beside the flag that
  makes it necessary. All seven gating jobs take it. `release.yml` takes it too
  and carries its own copy, because a workflow cannot read another one's `env`,
  and a release is the one build that has to be reproducible.
- **`clippy-next`**, `continue-on-error: true`, a matrix of `stable` and
  `beta`. `stable` is the leg this entry's Acceptance names. `beta` is the one
  that is useful, because by the time `stable` reports a lint the release has
  already happened. It runs `clippy`, which compiles every target, so it covers
  what `test` and `build` would find on the same toolchain.
- **A check, so neither property is left to a review.**
  `scripts/check-todo.ps1` fails when two workflows name different versions for
  `RUST_GATE`, and when a job installs `stable`, `beta` or `nightly` without
  carrying `continue-on-error: true`. Reintroducing a floating gate is one line
  in a diff and looks like every other job.

**The check was run against both defects it claims to catch**, which is
[T-217](../TODO/windows.md#t-217-the-text-gate-caught-one-control-byte-and-not-the-other-twenty-eight)'s
lesson. With `fmt` put back on `stable` it reports
``ci.yml:56 : job `fmt` installs `stable` ...``; with `release.yml` moved to
`1.97.1` it reports `the workflows disagree about RUST_GATE`. The tracking job,
which floats on purpose, is reported by neither.

**The run, which is the third clause.** Run **32631078557**, commit `e0718f7`,
nineteen jobs:

```
success  Clippy                        the gate, 1.98.0
success  Clippy (tracking stable)      stable is 1.98.0 today
failure  Clippy (tracking beta)        1.99.0-beta.1
success  ... every other job
```

The run's own conclusion is `success`. That is the property the split exists
for: a toolchain six weeks away found something, said so by name, and stopped
nothing. Without it the same finding would have arrived as sixteen red jobs on
whichever commit happened to be pushed the day 1.99 shipped.

### T-151 Only one of the three release targets was checked for static linking

Source:      found here, 2026-08-21, while acting on an operator request
Category:    ci
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `scripts/check-static.ps1` reads a PE import table and refuses a
             binary that needs `VCRUNTIME140.dll`. Both `ci.yml` and
             `release.yml` ran it `if: runner.os == 'Windows'`. The two musl
             targets make the same promise and nothing checked it, so
             `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` could
             have been shipping a binary that needs a loader and nobody would
             have found out until it failed to start.
Relevance:   [T-146](#t-146-ci-built-a-windows-binary-against-the-dynamic-c-runtime)
             is the proof that this is not theoretical: CI did build against
             the dynamic CRT, for weeks, and the reason it was caught at all is
             that the one target with a check had one. Two thirds of the
             release matrix had no such luck.
Approach:    One script, two formats, chosen by the file's own magic bytes
             rather than by the host, so a cross-built artifact is checked the
             same way wherever the checking happens. For ELF that is: no
             `PT_INTERP` program header and no `DT_NEEDED` entry in
             `.dynamic`. Read from the file directly rather than through `ldd`,
             which on a static binary prints "not a dynamic executable" on
             glibc and runs the binary on some other libcs, and neither is a
             thing to build a gate on.
Acceptance:  The check runs on all three targets in `ci.yml` and in
             `release.yml`, and it fails a dynamically linked ELF.

**The run is in.** CI run 32440386139, 2026-08-21, with the new flags and the
new check on all three:

| job | result |
| --- | --- |
| `Build (x86_64-unknown-linux-musl)` | pass, 4m19s |
| `Build (aarch64-unknown-linux-musl)` | pass, 3m53s |
| `Build (x86_64-pc-windows-msvc)` | pass, 8m15s |

https://github.com/Azathothas/bit-cli/actions/runs/32440386139

Each one built with `+crt-static -C prefer-dynamic=no`, the musl pair also with
`-C link-self-contained=yes -C link-arg=-Wl,--build-id=none`, and each one then
had its own binary read back. So the two musl artifacts are now known to carry
no `PT_INTERP` and no `DT_NEEDED` rather than assumed to.

**Both directions were proven before it shipped as a gate**, because a check
that cannot fail is not a check and there is no Linux on this machine to try it
against. Two synthetic ELF64 files were built, one with a `PT_INTERP` naming
`/lib/ld-musl-x86_64.so.1` and one `DT_NEEDED` entry, and one with neither:

```
$ pwsh -NoProfile -File scripts/check-static.ps1 -Path static.elf
interp:  none
needed:  0 shared object(s)
static confirmed: no PT_INTERP and no DT_NEEDED          # exit 0

$ pwsh -NoProfile -File scripts/check-static.ps1 -Path dynamic.elf
interp:  /lib/ld-musl-x86_64.so.1
needed:  1 shared object(s)
check-static: the binary is not statically linked: it names the dynamic
loader /lib/ld-musl-x86_64.so.1, it needs 1 shared object(s)   # exit 1
```

The PE path is unchanged and still passes against this machine's own release
build.

### T-153 Link speeds are not read on macOS

Source:      found here, 2026-08-21, while closing [T-145](#t-145-the-macos-test-job-fails-to-link)
Category:    ci
Priority:    P3
Effort:      M
Status:      open

Problem:     `sysinfo::platform::host` on the Apple platforms reports the OS,
             the CPU, the core count, and physical memory, and reports
             `network` as unavailable rather than reading it. Windows uses
             `GetIfTable` and Linux reads `/sys/class/net`; macOS has neither.
Relevance:   `Host::link_speed_bps` is what says whether a throughput number
             was bounded by the wire, and a report from a Mac cannot answer
             that. It is P3 rather than higher because macOS is not a release
             target under decision 9 and nothing measured so far compares
             across machines on link speed, so the field is unused where it is
             missing.
Approach:    `getifaddrs(3)` gives the interface names and the `IFF_UP` flag.
             The speed needs an `SIOCGIFMEDIA` ioctl per interface against a
             datagram socket, decoding `ifm_active` into a rate. That is real
             FFI against `if_media.h` constants that change between releases,
             which is why it is not in already: it cannot be run here and a
             wrong decode would report a plausible wrong speed rather than
             failing.
Acceptance:  `bit-cli bench webseed --format json` on `macos-latest` carries a
             `host.network` array with at least one interface, and
             `host.unavailable` is empty, which is what
             `sysinfo::tests::the_host_names_its_cpu_os_and_memory` asserts per
             platform today.

The test names the gap rather than tolerating any gap: it compares
`host.unavailable` to `["network"]` on Apple and to `[]` everywhere else, so a
second field going unreadable fails the build, and so does this one being
fixed without the expectation being updated.

### T-154 A Metalink named by URL is not recognised

Source:      `bit-cli` design, found closing [T-113](#t-113-metalink-is-not-implemented)
Category:    cli
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T07:18Z

Problem:     `Kind::classify` checked the `http://` and `https://` prefixes
             before it checked the `.meta4` and `.metalink` extensions, so
             `bit-cli download https://example.org/release.meta4` is a
             `Kind::Url`, is handed to the session as a `.torrent`, and fails
             on the bencode parse with a message about the torrent rather than
             about the metalink.
Relevance:   Every real Metalink is served over HTTP. `MirrorBrain` generates
             one on demand for any file it publishes, so a URL is the way a
             caller normally meets one, and a local `.meta4` is what you get
             after saving it by hand.
Approach:    A fourth branch: a URL whose path ends in `.meta4` or `.metalink`
             is a remote Metalink. `source::resolve_metalink` already takes a
             parsed `Metalink`, so the only new code is fetching the document
             before parsing it, which is `fetch_bytes` plus `Metalink::parse`.
             The redirect case needs a decision the local path does not have:
             a `MirrorBrain` document is generated per request and its
             `<origin dynamic="true">` names the URL it came from, so nothing
             has to be resolved relative to it, but a document with relative
             mirror URLs would.
Acceptance:  `bit-cli download <URL ending .meta4>` behaves exactly as the same
             document saved to disk does, proven by running
             `scripts/check-metalink-real.ps1` against the URL rather than the
             saved copy and getting the same report.

**Done 2026-08-23T07:18Z.** `Kind::MetalinkUrl(String)` is the fourth branch,
`source::fetch_metalink` is the only new code on the resolve path, and
`resolve_metalink` is unchanged: it takes a parsed document and does not know
where it came from, which is what the Approach predicted.

**The extension is read from the URL's path and not from the whole string**, and
that is a decision the entry did not name. `?file=r.meta4` is a query naming a
file and `#r.metalink` is a fragment, and neither says what the URL serves; a
`MirrorBrain` instance generating a document per request is exactly the place a
query string turns up. `only_the_url_path_decides_whether_it_is_a_metalink`
holds both directions, including `https://e.com/r.meta4?mirrorlist`.

**The redirect case needed no decision after all.** The Approach said one was
owed. Nothing in either path resolves a mirror URL relative to anything, so a
document fetched over HTTP is treated exactly as one read from disk: absolute
URLs are used and relative ones are refused, on both paths. A document with
relative mirror URLs would need a base, and refusing it on one path and
resolving it on the other is the divergence worth avoiding.

**`--dry-run` is the one place the two kinds differ, and it is a decision.** A
saved `.meta4` is readable with nothing running, so a dry run reports every
claim in it. A URL is not: the document itself is the thing to fetch. It is not
fetched, for the reason already written into that same function about
`--web-seed-list-url`, and `torrents[].document_needs_network` on the row is
what says the block is absent because nothing was contacted rather than because
the document was empty.

**The acceptance, run against the live mirror**, is
`bench/metalink-real-20260823T071745617Z.json`, case `real_by_url` beside
`real_as_served`. Same exit code and the same message, character for character,
from a document the instance generated per request:

```
real_as_served  exit 4  the metalink lists no torrent for LibreOffice_...msi,
                        so there is nothing to download here. It lists 58 HTTP
                        mirror(s); ...
real_by_url     exit 4  (identical)
```

**That case cannot prove the download half**, because no reachable MirrorBrain
instance emits `<metaurl mediatype="torrent">`, which is what `real_as_served`
has recorded since [T-113](#t-113-metalink-is-not-implemented). The download
half is proven on loopback: case `url_source` in `scripts/check-metalink.ps1`,
`bench/metalink-20260823T071256391Z.json`, which serves the `v4_ok` document
over HTTP and compares the resulting `metalink` block **field by field** with
the run over the saved copy. They are identical except `checksum.path`, which
must differ because each case writes into its own output directory and which is
asserted separately rather than dropped.

`a_metalink_named_by_url_downloads_the_same_as_one_on_disk` is the same
comparison in `cargo test`, so CI carries it: CI runs neither script.

```
$ cargo test -p bit-cli --lib a_metalink_named_by_url
test result: ok. 1 passed; 0 failed; 0 ignored; 394 filtered out
```

### T-155 --hash-check-only drops the metalink report

Source:      `bit-cli` design, found closing [T-113](#t-113-metalink-is-not-implemented)
Category:    cli
Priority:    P3
Effort:      S
Status:      **done** 2026-08-23T07:06Z

Problem:     `one_inner` returned early for `--hash-check-only`, before the
             block that builds `TorrentReport::metalink`. So a Metalink run
             with that flag reports nothing about the document at all: not the
             mirror count, not the torrent it resolved, not the size
             comparison, none of which needs a download.
Relevance:   `--hash-check-only` over a Metalink is a reasonable thing to ask
             for: check what is already on disk, and tell me whether the two
             documents agree about it. The size comparison in particular is
             computed before the early return and then thrown away.
Approach:    Build the report at both exits rather than at one. `check_metalink`
             already writes a `not_checked` reason for a run that did not
             finish, so the early path needs the same call and no new branch.
             The interesting case is a payload that is complete on disk: the
             hash check proves it against the torrent, so the Metalink's
             checksum could be checked there too and would be the strongest
             thing this flag could report.
Acceptance:  `bit-cli download release.meta4 --hash-check-only --json` over a
             complete payload reports the `metalink` block with
             `agreement.size_agrees` set, and either a checked digest or a
             `not_checked` reason.

**Done 2026-08-23T07:06Z, and the interesting case is the one that happened.**
The Acceptance allows either a checked digest or a `not_checked` reason, and
over a complete payload it is the digest: `matched: true`, 2,097,152 bytes
hashed, against the file at the path the report names. That is the strongest
thing this flag can report, because the hash check has already proved those
bytes against the torrent and the checksum then proves the same bytes against
the Metalink. `check_metalink` decides it from `report.finished` and needed no
branch of its own, which is what the Approach predicted.

The block that built the report was inline at `one_inner`'s normal exit. It is
`apply_metalink` now, called at both exits, so the two cannot drift apart the
way they did.

**`bench/metalink-20260823T070301761Z.json`** is the run, case
`hash_check_only`, eleventh in `scripts/check-metalink.ps1`:

```json
{"agreement":{"file_index":0,"matched_by":"only_file","metalink_size":2097152,
 "size_agrees":true,"torrent_size":2097152},
 "checksum":{"algorithm":"sha256","matched":true,"bytes_hashed":2097152},
 "mirrors_listed":1,"mirrors_registered":1,"version":"4"}
```

**The same case is in `cargo test` as well, and that is deliberate.** CI does
not run `scripts/check-metalink.ps1`, so an acceptance that lived only there
would catch a return moved back above the call only when somebody ran it by
hand. `hash_check_only_over_a_metalink_still_reports_the_document` downloads the
payload, then checks it, and asserts the block.

**It was checked against the defect rather than assumed to cover it.** With the
`apply_metalink` call removed from that exit the test fails on
`no metalink block`, and the document it prints is a `download` report with no
`metalink` key at all. A test written for a fixed defect and never run against
the defect is a test that may be asserting something else.

```
$ cargo test -p bit-cli --lib hash_check_only_over_a_metalink
test result: ok. 1 passed; 0 failed; 0 ignored; 390 filtered out
```

### T-156 A dry run writes a different shape under the same document kind

Source:      `bit-cli` design, found closing [T-113](#t-113-metalink-is-not-implemented)
Category:    cli
Priority:    P3
Effort:      S
Status:      **done** 2026-08-23T06:44Z

Problem:     `download --dry-run --json` writes `kind: "download"` and a
             document that shares almost no fields with a real run's:
             `dry_run`, `directory`, and per-torrent `kind`, `needs_network`,
             `coverage`, `trackers[]`, `web_seeds[]`, and `total_bytes`, and
             none of `stopped`, `finished`, `sources[]`, or `total`. A consumer
             selecting by `kind`, which is the documented way to select, gets
             two shapes.
Relevance:   `docs/schema.md` is generated by folding every run of a command
             into one table per `kind`. Sampling the dry run would make the
             `download` table a union of two documents with nothing saying
             which fields belong to which, so the generator does not sample it
             and the dry run's fields are undocumented.
Approach:    Give it its own kind, `download_dry_run`, and sample it. That is a
             breaking change to a document nothing is known to consume, and it
             is the shape the rest of the surface already uses: `verify` writes
             `hash_mismatch` rather than a `verify` with different fields.
             `dry_run: true` stays, because a reader who has the document in
             hand should not have to know the kind changed.
Acceptance:  `bit-cli download <SOURCE> --dry-run --json | jq -r .kind` prints
             `download_dry_run`, `DOCUMENT_KINDS` names it, and
             `docs/schema.md` carries its field table from a run the generator
             drives.

**Done, all three clauses.** `dry_run` in `cmd/download.rs` emits
`download_dry_run`, `DOCUMENT_KINDS` in `schema.rs` names it with why it is its
own kind, and `schema_gen.rs` drives two runs it folds into one table.
`dry_run: true` stays on the document, so a reader holding one does not have to
know the kind changed.

**Two runs rather than one, because neither reaches every field.** A Metalink
dry run is the only source kind that fills `torrents[].metalink`; a torrent one
is the only one that resolves a file layout, so it is the only one with
`torrents[].coverage` and a real `info_hash`.

**The order of the two is load-bearing, and the first attempt got it wrong.**
`Sample::merge` is `or_insert`, so the **first** observation of a path names its
type and later ones can only add paths. With the Metalink run first, the
committed table said `torrents[].info_hash`, `name` and `total_bytes` were
`null`, which is what a Metalink dry run leaves them as and is not what the
field is. Taking the torrent run first gives `string`, `string` and `integer`,
and the Metalink run still contributes every `metalink.*` row. This is the same
shape of defect as [T-158](#t-158-regenerating-the-schema-deletes-fields-the-sample-did-not-produce):
what the generator writes depends on what the sample happened to contain.

`a_dry_run_writes_its_own_document_kind` asserts both halves. A real run is
still `download` and carries no `dry_run` field, which is what stops the case
passing if the kind is simply renamed everywhere.

```
$ cargo test -p bit-cli --lib a_dry_run_writes_its_own
test result: ok. 1 passed; 0 failed; 0 ignored; 388 filtered out
```

**A defect in the tooling turned up on the way, and it is fixed here.**
`scripts/check-man.ps1 -Fix` generates the manuals by running
`target/release/bit-cli.exe`, and it did not build one first. A stale binary
regenerated all three files from the command surface as it was at the last
release build, wrote them, and printed "regenerated"; `git diff man/` was then
empty while `cargo test --test man_is_current` went on failing, because that
test renders from the crate being compiled. `gates.ps1` reported `man ok` and
`test FAILED` in the same run, which reads as the test being wrong. `-Fix`
builds first now, and without `-Fix` the script compares the binary's timestamp
against the newest `.rs` under `crates/` and defers to the test rather than
answering about a surface that no longer exists.

### T-158 Regenerating the schema deletes fields the sample did not produce

Source:      `docs/schema.md`, found during the doc pass of 2026-08-21
Category:    cli
Priority:    P2
Effort:      S
Status:      **done**

Problem:     `BIT_CLI_UPDATE_SCHEMA=1 cargo test -p bit-cli --lib schema`
             overwrites `docs/schema.md` with exactly what that run produced.
             A field that only appears on a path the sample did not take is
             silently deleted from the document.
Relevance:   That command is the documented way to update the schema, and it is
             in `CHANGELOG.md` and in the panic message the check itself
             prints. Following the instruction makes the document worse.
Approach:    Merge rather than replace. Read the committed file, union its rows
             with the rendered ones, and write the union sorted. A row that is
             genuinely gone then needs deleting on purpose, which is the right
             cost for removing a documented field.
Acceptance:  Regenerating twice in a row on a machine whose sample takes a
             different path both times leaves every row that either run
             produced, and `git diff docs/schema.md` is empty when nothing
             changed.

**Found by following the instruction.** Regenerating on 2026-08-21 removed one
row and added none:

```
-| `sources[].error` | string |
```

**Re-measured on 2026-08-21 in the doc pass, and it removes two rows now, not
one.** Regenerated into a scratch copy and diffed rather than committed, which
is the workaround this entry exists to remove:

```
$ cp docs/schema.md /tmp/committed.md
$ BIT_CLI_UPDATE_SCHEMA=1 cargo test -p bit-cli --lib schema
$ diff /tmp/committed.md docs/schema.md
338d337
< | `torrents[].sources[].error` | string |
535d533
< | `sources[].error` | string |
$ git checkout -- docs/schema.md
```

Both are the same field seen from the two document shapes, and both are real:
a source that errored carries an `error` string. The sample simply had no
erroring source on this machine on this run. Note what that means for the
count: the number of rows lost is a property of the run rather than of the
tree, so it grows and shrinks and "one row" was never the number. The
mechanism is the defect, not the size.

The read-only half of the check is fine and stays fine.
`schema_gen.rs:1599` `the_committed_schema_matches_what_the_program_writes`
passes, and it is deliberately a **containment** check rather than an equality
one, for the reason its own comment gives: these runs are timed, so a download
that finished before its second report tick emits no `progress`, and requiring
equality would make the contract check flaky. The regenerating branch at `:739`
is a plain `std::fs::write` of the rendered text, with none of that tolerance.
So the check is asymmetric on purpose and the regenerator is symmetric by
accident, and the fix is to give `:739` the same tolerance the assertion
already has.

That field is real. `crates/bit-cli/src/cmd/webseed.rs:285` and
`crates/bit-cli-core/src/webseed/probe.rs:457` both carry
`error: Option<String>` with `skip_serializing_if`, so it appears when a source
fails and not when every source succeeds. The generator's sample had no failing
source, so the row went. Three regenerations in a row produced the same
deletion, so it is deterministic rather than a flake.

The regeneration was **not committed**, and the committed schema is the
accurate one.

**Why the check did not catch it.** `the_committed_schema_matches_what_the
_program_writes` is a containment check on purpose: a row this run produced and
the file lacks is a failure, and a row the file has and this run did not
produce is not. That asymmetry is right, and its comment explains why: these
runs are timed, so a download that beats its own report tick emits no
`progress`. The gap is that the **writer** does not share the reader's
asymmetry. The check tolerates extra rows and the generator deletes them.

**Fixed by giving the writer the reader's tolerance, which is what the entry
already said the fix was.**

`merge_schema` in `crates/bit-cli/src/schema_gen.rs` reads the committed file,
indexes its field rows by the section they sit under, and unions them into what
this run rendered. Where both carry a path, **this run's type wins**: the
committed one is a record of an older measurement and this one is current.
Where only the committed file has a path, the row survives.

The section key is the `##` heading and the `###` heading together, not the
`###` alone. A document kind and an event type may share a name, and their
field lists are different things; keying on the inner heading alone would let
one section's rows leak into the other's. The test asserts that directly with a
row that exists only under `## Events`.

**A field that is genuinely gone now has to be deleted on purpose.** That is
the right cost for removing something from a versioned contract, and it is the
trade this entry named.

**Found again by following the instruction, on this session's own change.**
Adding `gone_files` and `pieces_dropped` to `SourceReport` for
[T-005](webseed.md) made the contract check fail, correctly, naming the four
new rows. Regenerating the way the panic message says removed two:

```
$ diff /tmp/committed.md docs/schema.md     # the old overwriting writer
535d534
< | `sources[].error` | string |
793d791
< | `cooldowns` | integer |
794a793,795
> | `gone_files[].file` | integer |
> | `gone_files[].pieces_dropped` | integer |
> | `gone_files[].reason` | string |
798a800
> | `pieces_dropped` | integer |
```

Two rows lost, and neither is the `sources[].error` pair this entry recorded
before: `cooldowns` is new to the list. That is the entry's own point about the
count made a third time. **The number of rows lost is a property of the run**,
so it was one, then two of one kind, then two of two kinds. The mechanism is
the defect.

With the merging writer, the same regeneration on the same tree:

```
$ diff /tmp/committed.md docs/schema.md     # the merging writer
794a795,797
> | `gone_files[].file` | integer |
> | `gone_files[].pieces_dropped` | integer |
> | `gone_files[].reason` | string |
798a802
> | `pieces_dropped` | integer |
```

Additions only. `sources[].error` and `cooldowns` survive.

**Two tests, and they are the acceptance in its own words.**

`regenerating_the_schema_keeps_rows_this_run_did_not_produce` is the unit case
on hand-written input: a row only the committed file has survives, a row only
this run produced is added, a path both carry takes this run's type and not
both, a row from another section does not leak in, and the merged rows stay
sorted by path so merging does not churn the diff.

`regenerating_the_schema_is_idempotent` is "regenerating twice in a row leaves
every row that either run produced, and `git diff` is empty when nothing
changed", stated as two equalities: merging a render into itself reproduces it
exactly, and merging again changes nothing.

**What was not changed.** The read-side check stays a containment check, and
its asymmetry stays deliberate: these runs are timed, so a download that
finishes before its second report tick emits no `progress` and requiring
equality would make the contract check flaky. The two halves are now tolerant
in the same direction, which is all that was ever wrong.

### T-159 Subcommand flags are filed under "Report options" in the help

Source:      `bit-cli bench <SUB> --help`, found in the doc pass of 2026-08-21
Category:    cli
Priority:    P3
Effort:      S
Status:      **done** 2026-08-23T06:52Z

Problem:     `--peers`, `--torrents`, `--dir`, and `--connect-timeout` appear
             under the heading **Report options** in `bench swarm --help`.
             None of them is a report option. `bench leech`, `bench seed`, and
             `bench disk` have the same defect, so four of the six subcommands
             mis-file their own flags.
Relevance:   The headings exist so a reader can find a flag by what it does.
             One that files `--peers` beside `--fail-under` is worse than none,
             because it is confidently wrong.
Approach:    `clap`'s `next_help_heading` applies to every argument declared
             after it, including the ones in the outer struct that follow a
             flattened inner one. `BenchShared` sets the benchmark heading and
             flattens `ReportArgs`, which sets the report heading, and the
             outer struct's own fields are declared after that flatten, so they
             inherit it. Give each subcommand struct its own
             `#[command(next_help_heading = "...")]`, or flatten the shared
             groups last.
Acceptance:  For every `bench` subcommand, the only flags under **Report
             options** are `--report`, `--format`, `--baseline`, and
             `--fail-under`. A test walks `clap`'s command tree and asserts it,
             so the next subcommand cannot reintroduce it.

Reproduce, and see it on four of six:

```bash
for s in webseed leech seed disk probe swarm; do
  echo "== $s"
  bit-cli bench $s --help | sed -n '/^Report options:/,/^[A-Za-z].*options:$/p'
done
```

`webseed` and `probe` are correct, and they are correct by accident: neither
declares a flag after its flatten.

**Done, and the entry undercounted the defect.** It named four `bench`
subcommands. The fifth place it happens is the front door: `bit-cli --help` had
**no "Arguments" section at all**, because `Cli::sources` is declared after the
`Global` flatten and a positional inherits the running heading like any other
argument. `[SOURCE]...` was documented at the bottom of "Global options", 100
lines below the usage line that names it. Nothing in the entry predicted that,
and it was found by the test rather than by reading:
`no_positional_is_pulled_into_a_help_heading` walks the whole command tree and
failed on `sources` the first time it ran.

Each of the four subcommand structs now sets its own heading, `Swarm options`,
`Leech options`, `Seed options` and `Disk options`, **and flattens the shared
groups last**. The heading alone is not enough: `next_help_heading` is applied
once at the top of `augment_args`, so a field after a flatten still inherits
whatever that flatten left behind. `bench probe` gets `Probe options` too, so
its two flags are correct by construction rather than by accident.

`help_heading = None` on the three positionals is the other half.
`#[command(next_help_heading)]` covers a struct's positionals as well, so
without it `<TARGET>` moved out of "Arguments" and rendered *after*
`--connect-timeout`, which is a worse place than the one it started in.

Three tests, and the split matters. `only_report_flags_are_filed_under_report_options`
asserts the property rather than the fix, so flattening last is not the only
shape that passes. `every_bench_subcommand_files_its_report_flags_under_report_options`
is its inverse, because a heading that files *nothing* under it would pass the
first one. `no_positional_is_pulled_into_a_help_heading` walks every command,
not just `bench`.

```
$ cargo test -p bit-cli --lib report_options
test result: ok. 2 passed; 0 failed; 0 ignored; 386 filtered out

$ cargo test -p bit-cli --lib no_positional
test result: ok. 1 passed; 0 failed; 0 ignored; 387 filtered out
```

The acceptance, run:

```
webseed  --report --format --baseline --fail-under
leech    --report --format --baseline --fail-under
seed     --report --format --baseline --fail-under
disk     --report --format --baseline --fail-under
probe    --report --format --baseline --fail-under
swarm    --report --format --baseline --fail-under
```

`man/` is unchanged by this, checked with `scripts/check-man.ps1 -Fix` and an
empty `git diff man/`. The generated manuals group flags by command rather than
by help heading, so the heading is a terminal-only surface.

### T-160 A peers test raced its own seeder

Source:      local `cargo test --workspace` and CI run 32458314378, 2026-08-21
Category:    ci
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `cmd::peers::tests::a_sampled_swarm_carries_what_came_from_each_peer`
             starts a seeder on a thread and dials it from the test thread with
             nothing in between. `free_port` binds a port to learn its number
             and drops the listener, so there is a window where the number is
             known and nothing is listening. A dial that lands in that window
             fails, the peer is marked dead with one error, and `librqbit` does
             not retry it for ten seconds, which is twice the test's own
             `--duration 5s`. Every assertion after the dial then fails.
Relevance:   [T-148](bench.md#t-148-the-peer-probe-test-asserted-an-exit-code-inside-its-own-retry-loop)
             is the precedent, and this is the same mistake in another test: a
             fixture whose readiness is assumed rather than waited for. A test
             that fails one run in twenty turns CI red on somebody else's push
             and costs more to diagnose there than here.
Approach:    Wait on the condition, not on a guessed duration.
             `test_support::wait_for_listener` dials the port until something
             accepts or ten seconds pass, and the test asserts it came up
             before it asserts anything about the swarm, so a fixture that
             never started says so instead of failing three assertions later.
Acceptance:  The test is named, the race is named, and the fix is in the test
             rather than in a retry around it, the way T-148 was fixed.

**Found twice and named once.** It failed one local `cargo test --workspace`
and was not reproduced in fourteen further runs, including six sequential and
two concurrent pairs run to provoke it, with the name lost because the command
filtering the output matched only the summary line. Then it failed
`Test (ubuntu-latest)` on CI run 32458314378, which was a **documentation-only
commit**, and the CI log carried what the local filter had thrown away:

```
---- cmd::peers::tests::a_sampled_swarm_carries_what_came_from_each_peer stdout ----
thread '...' panicked at crates/bit-cli/src/cmd/peers.rs:427:9:
assertion `left == right` failed: {... "dead":1, "live":0,
  "peers":[{"errors":1,"downloaded_bytes":0,"verified_pieces":0,"state":"dead"}]}
  left: Number(1)
 right: 0
```

`errors: 1` and `downloaded_bytes: 0` are the whole diagnosis: the peer never
connected, so nothing followed. A commit that changed only Markdown is what
proves the test and not the code.

**Fixed, and in two places.** `crates/bit-cli/src/schema_gen.rs` has the same
seeder-on-a-thread-then-dial shape and now waits too. There it is quieter and
worse: nothing asserts, so a lost race would sample a `peers` document with a
dead peer and silently write a schema missing whatever a live peer carries.
That is [T-158](#t-158-regenerating-the-schema-deletes-fields-the-sample-did-not-produce)
arriving by a second route.

Two things worth keeping from how this was found. Filter for
`^test \S+ \.\.\. FAILED` and not for the summary line, or the name is lost.
And a green run does not mean a suite has no race: this one passed twenty
consecutive local runs and sixteen CI jobs before failing on a commit that
touched no code.

**It failed again on 2026-08-21T17:00Z, differently, and the fix above was only
half of it.** CI run **32505742044** turned `Test (ubuntu-latest)` red on the
[T-172](metainfo.md) push:

```
thread '...' panicked at crates/bit-cli/src/cmd/peers.rs:448:9:
assertion `left == right` failed: {... "connecting":1, "live":0, "seen":1,
  "peers":[{"errors":0,"downloaded_bytes":0,"state":"connecting"}]}
  left: Number(0)
 right: 2000
```

Read it against the failure above: `errors: 0` and `state: connecting` where
the first one had `errors: 1` and `state: dead`. The dial was **not** lost this
time. `wait_for_listener` did its job, the peer was reached, and the handshake
was still in flight when the five second sample ended. So the first fix
addressed the race it named and left the guessed duration behind it, which is
the half [RULES.md](RULES.md) actually states: a test waits on the condition,
never on a guessed duration.

Rerunning the same job on the same commit with no change passed, which is what
separates a flake from a break and is worth doing before touching anything.

**Fixed by sampling until the bytes arrive.** `--duration` is the command's own
contract and it samples for exactly that long, so the test cannot make one
sample longer without changing what it is testing. What it can do is sample
again: the run repeats until a report shows bytes moved or twenty-five seconds
pass, and asserts on that report. On an unloaded machine the first sample
succeeds and it costs nothing.

The seeder is no longer joined. It runs for forty seconds so the retries have
something to dial, and waiting for it to time out would have made every run of
this test as long as its worst case: joining a 90 second seeder took the test
from six seconds to ninety-one, measured. The thread dies with the test binary.

**What this says about the previous fix, and about the next one.** T-160's
`Approach` line was already the right rule, written down and then applied to
only one of the two guesses in the test. A fix that quotes the rule and half
applies it reads as complete in review, which is how this cost a second red
job. Every timing assumption in a test has to be listed before one of them is
fixed, not after the next failure names it.

### T-161 A CI action still targets Node.js 20, which is deprecated

Source:      CI run 32457763652 annotations, 2026-08-21
Category:    ci
Priority:    P3
Effort:      S
Status:      **done** 2026-08-23T08:35Z

Problem:     Three jobs annotate:

             ```
             Node.js 20 is deprecated. The following actions target Node.js 20
             but are being forced to run on Node.js 24:
             ilammy/setup-nasm@v1.5.2
             ```

             The run is green. Being forced onto a runtime it was not built
             for is a warning today and a failure whenever the forcing stops.
Relevance:   Same shape as [T-150](#t-150-clippy-pins-a-floating-toolchain-so-a-rust-release-can-turn-the-tree-red):
             a gate that moves without this repository touching it. The
             difference is that this one announces itself first, so it is worth
             acting on before it announces itself as a red job.
Approach:    `ilammy/setup-nasm` is used at **four** call sites in **four**
             jobs, not the two an earlier revision of this entry named:
             `test` at `.github/workflows/ci.yml:62`, `build` at `:97`,
             `interop` at `:199`, and `determinism` at `:238`. On the matrix
             those are `Test (windows-latest)`,
             `Build (x86_64-pc-windows-msvc)`,
             `Create round trip (windows-latest)` and
             `Create determinism (windows-latest)`. Patching two of the four
             leaves the annotation on the other two and leaves the tree half
             fixed, which is the reason the count is written out here. Take a release that
             declares `node24`, or replace it: NASM is needed only by `aws-lc-
             rs`, and `choco install nasm` on the runner is one line with no
             action behind it. Every other action in the file is already on a
             current major.
Acceptance:  A CI run with no Node.js deprecation annotation, and the Windows
             jobs still green, which is what says NASM is still being found.

Recorded rather than acted on, because the run this came from is green on all
sixteen jobs and changing a build dependency of the one target that has to link
statically is not a change to make in the same push as everything else.

**Done, and it was done in the session of 2026-08-23 that closed
[T-199](#t-199-the-ci-supply-chain-was-unwatched-and-one-action-was-abandoned)
without this entry being closed with it.** The action is gone from all four call
sites and from `release.yml`'s fifth: every one of them runs
`pwsh -NoProfile -File scripts/setup-nasm.ps1`, which pins the version and
checks what it downloads. `.github/workflows/ci.yml:88` is the comment that says
why, and it names the action this entry is about, which is what made it look
present to anything searching the file for a string.

The Acceptance holds: the Windows jobs are green, which is what says NASM is
still being found, and no run since carries a Node.js deprecation annotation for
it. Confirmed against run **32628316314**, and `grep -rn "uses:" .github/`
carries eight distinct actions and `ilammy/setup-nasm` is not one of them.

**Why nothing caught it, and what does now.** Two gaps in
`scripts/check-todo.ps1`, both closed on 2026-08-23:

1. **`.github/` was not in the cited-path prefixes at all.** The regex resolved
   `crates|scripts|docs|vendor|patches|man` and nothing else, so this entry's
   four citations of `.github/workflows/ci.yml:<line>` were never checked for
   the file existing, for the line existing, or for anything else. That is now
   a sixth prefix.
2. **Nothing compared an entry's premise to the workflows.** A new check reads
   the `uses:` lines of `.github/workflows/*.yml` and fails when an **open or
   partial** entry names an `owner/name@ref` pin that no workflow carries. That
   is the one shape of "this entry describes a state the tree is not in" that
   can be decided mechanically: nothing else in this record is spelled
   `owner/name@ref`. Closed entries are exempt, because one quoting the pin it
   removed is evidence, which is the same rule the drifted-line check already
   follows for a fenced citation.

**The first draft of check 2 passed this entry**, and the reason is worth
keeping: it searched the raw text of the workflow files, and `ci.yml` carries
the comment "Ours, not ilammy/setup-nasm: that action is unmaintained". A
substring search found the very action the comment exists to say is gone. It
reads `uses:` lines only now.

```
$ pwsh -NoProfile -File scripts/check-todo.ps1
  [stale-premise] cli-surface.md:1804 : T-161 is open and names the action
  `ilammy/setup-nasm@v1.5.2`, which no workflow uses.
```

That is the output that closed this entry, produced by the check written for it.

### T-181 Four flags are accepted in silence and reach no code

Source:      the flag audit of 2026-08-21
Category:    cli
Priority:    P1
Effort:      M
Status:      **done**

Problem:     Four flags parse, are carried into a struct, and are never read
             again anywhere in the workspace:

             | Flag | Declared | Read |
             | --- | --- | --- |
             | `--no-pex` | `cli.rs:1335` | nowhere |
             | `--tracker-list-url <URL>` | `cli.rs:700` | nowhere |
             | `--max-overall-download-rate <RATE>` | `cli.rs:741` | nowhere |
             | `--max-overall-upload-rate <RATE>` | `cli.rs:745` | nowhere |

             The check is one command: every `pub` field in `cli.rs` grepped
             for outside that file. Six fields have no reader. Two of the six
             are already owned, `index_out` by
             [T-116](#t-116--o--index-out-cannot-rename-a-file) and
             `on_piece_verified` by
             [T-115](#t-115-hooks-do-not-fire-for-every-documented-trigger),
             and these four are owned by nothing.
Relevance:   This is the P1 definition in `INDEX.md` verbatim: "a documented
             capability does not work, or a flag does nothing." It is also the
             rule `cli-surface.md` opens with: a flag that looks like it works
             and does not is worse than one that errors.

             Each of the four fails a different way and none of them fails
             loudly.

             `--no-pex` is the one with a security shape. A user passing it
             believes peer exchange is off. It is not off, and their address
             keeps being gossiped to the swarm. That is a privacy expectation
             silently unmet rather than a performance knob silently ignored.

             `--tracker-list-url` promises a tracker list fetched over HTTP.
             Nothing is fetched, so the run announces to fewer trackers than
             the user asked for and finds fewer peers, which reads as a quiet
             swarm rather than as a missing feature.

             `--max-overall-download-rate` and `--max-overall-upload-rate` are
             the pair that matter most on the operator's own case. They are the
             whole-run caps, and the per-torrent ones next to them
             (`--max-download-rate`, `--max-upload-rate`) **do** work and are
             measured under [T-031](performance.md). So a user who caps a
             single torrent gets a cap and a user who caps the whole run gets
             nothing, from two flags that sit four lines apart in the same
             struct and read identically in `--help`. `performance.md` under
             T-031 already noted these two were not covered by that
             measurement, and no entry picked them up.
Approach:    Two of the four are work and two are a decision.

             **`--max-overall-*-rate`** is the one to build. `librqbit`'s
             `LimitsConfig` is per-session, and `bit-cli` runs one session per
             invocation, so a session-wide cap is where these belong and the
             per-torrent flags are the ones that need dividing. Care is needed
             on one point [T-132](multi-source.md) already records: a session
             cap applies to peers **and** to HTTP sources together, because a
             web seed reaches the session as a peer. So `--max-overall-*` and
             `--web-seed-speed-limit` interact, and the acceptance has to
             measure both together or it proves nothing.

             **`--tracker-list-url`** is a small fetch: GET the URL, one
             tracker per line, blank line separating BEP 12 tiers, which is
             the format `--tracker-file` already parses at `cli.rs:697`. The
             work is reusing that parser and bounding the fetch, because the
             URL is user-supplied and the response is untrusted. Cap the body,
             set a deadline, and refuse a non-HTTP scheme.

             **`--no-pex` cannot be built here.** `librqbit` 9.0.0 has no
             switch for it: `swarm.rs:160-161` shows `--no-dht` and `--no-lsd`
             reaching `enable_dht` and `enable_lsd`, and there is no
             `enable_pex` beside them.
             `nanotorrent/patches/0004-pex-toggle.patch` adds exactly that:
             `SessionOptions::disable_pex`, gating **both** directions, which
             is the shape of the upstream change needed and the evidence that
             it is a small one. Until it exists, the flag must either warn or
             refuse.

             **The pattern for all four in the meantime already exists in this
             tree.** `crates/bit-cli/src/cmd/seed.rs:105`: `--superseed` is
             accepted and prints a warning naming the entry that would close
             it. That is the honest behaviour for a flag that cannot yet do
             what it says, and it is why `--superseed` is not on the list
             above. Do that for all four today, and remove each warning as its
             flag starts working.
Acceptance:  Two parts, and the first is what stops this recurring.

             A test that walks the `clap` command tree and asserts every flag
             either reaches code or is on an explicit, named exception list,
             so a fifth cannot be added silently. The exception list is the
             deliverable: it is short, it is reviewed, and it makes the
             warning above mechanical rather than remembered.
             `cli.rs:2927` `every_short_flag_is_documented_in_the_flags_table`
             is the model: it already walks the tree and fails with the exact
             fix to apply.

             Then, per flag: `--max-overall-download-rate 4MiB/s` over `-j 4`
             holds the aggregate within ten per cent, measured against an
             uncapped run of the same four torrents, with both numbers here;
             `--tracker-list-url` against a loopback URL serving three
             trackers announces to all three and reports them in `--json`;
             `--no-pex` warns, naming this entry, until the upstream switch
             exists.

**All four are resolved, and building two of them found a fifth this entry's
own audit could not have caught.**

| Flag | Now | Where |
| --- | --- | --- |
| `--max-overall-download-rate` | works, session-wide | `swarm.rs` `engine_options` |
| `--max-overall-upload-rate` | works, session-wide | the same |
| `--tracker-list-url` | works, fetched over HTTP | `swarm.rs` `tracker_list` |
| `--no-pex` | warns, naming this entry | `cmd/seed.rs` |

**The rate pair was two flags aiming at one field, and the wrong one arrived.**
`librqbit` 9.0.0 has two rate limits and they are different structures:
`SessionOptions::ratelimits` caps the session and `AddTorrentOptions::ratelimits`
caps one torrent. `bit-cli` set only the session one, and it set it from
`--max-download-rate`. So the per-torrent flag capped the whole run and the
whole-run flag capped nothing. Each flag now goes to the field it names, and
`SessionSetup::torrent_rates` parses the per-torrent pair in one place so a
command cannot wire one and forget the other.

`--max-download-rate` therefore changes behaviour, and the change is the fix.
[T-031](performance.md) measured it at `-j 1`, where per-torrent and whole-run
are the same number, so that measurement stays true. This is the measurement
that tells them apart:

```
$ pwsh -NoProfile -File scripts/check-overall-rate.ps1 -Rate 4MiB/s -PayloadSize 64MiB -Torrents 4

phase       exit wall  bytes     rate
uncapped       0 0.2s  64.00 MiB 392.64 MiB/s
overall        0 15.2s 64.00 MiB 4.20 MiB/s
per_torrent    0 3.3s  64.00 MiB 19.69 MiB/s

verdict: both scopes hold
```

`--max-overall-download-rate 4MiB/s` over `-j 4` holds at **4.20 MiB/s**, 5.05%
over the cap and inside the ten per cent this entry asked for, against **392.64
MiB/s** uncapped, which is 93 times faster. The third phase is the one that
proves the two flags are two fields: `--max-download-rate 4MiB/s` over the same
four torrents reaches **19.69 MiB/s**, near the 16 MiB/s that four torrents at
4 MiB/s each should sum to, and 4.7 times what the whole-run cap allows. Before
this change phases 2 and 3 were the same run.

Evidence: `bench/overall-rate-20260821T140422453Z.json`, and
`scripts/check-overall-rate.ps1` is the script. The sources are HTTP web seeds
rather than peers, which is deliberate: a web seed reaches the session as a
peer, so the session limiter is what bounds it, and that is exactly the
interaction [T-132](multi-source.md) is about. The rate is computed from the
wall clock and the bytes the report says landed, never from the report's own
mean, so the limiter is not measured by the thing it limits.

**`--tracker-list-url` is a bounded fetch, and the bound is the point.** The
URL comes from the caller and the body comes from whoever answers it, so
`crate::source::fetch_list` refuses a scheme that is not HTTP or HTTPS, sets a
thirty second deadline over the whole exchange, and caps the body at one
mebibyte. It reads in chunks rather than calling `bytes()`, so a server
declaring a small `Content-Length` and sending more is stopped at the cap
rather than after it. A body over the cap is **refused rather than truncated**:
half a tracker list is a run announcing to a set of trackers nobody chose, and
a truncated last line is a URL that is not the URL anyone wrote.

It reads with the same parser `--tracker-file` uses, so two flags that read
identically in `--help` behave identically. That parser flattens, and a blank
line does not open a BEP 12 tier here any more than it does in a file;
announcing in tier order is [T-063](trackers.md) and is not this.

The fetcher is injected the way `webseed_args::collect` already takes one, so
the assembly is testable without a network and a command that must not reach
out passes `no_network`. `download --dry-run` is one of those: a dry run
reports without doing, which is the decision `--web-seed-list-url` already
took on that same command.

Proven end to end against three loopback trackers, in
`a_tracker_list_url_is_fetched_and_every_tracker_in_it_is_announced_to`. Three
rather than one, because the failure this guards against is a list read and
then partly dropped, and one tracker cannot tell a whole list from its first
line. Each tracker records what it was asked, so the proof is on the tracker's
side rather than in a count the run reports about itself.

**`--no-pex` cannot be built and now says so.** `librqbit` 9.0.0's
`SessionOptions` carries `dht` and `disable_local_service_discovery` and
nothing beside them for peer exchange, which `swarm.rs` shows: `--no-dht` and
`--no-lsd` reach `enable_dht` and `enable_lsd` and there is no `enable_pex` to
reach. `nanotorrent/patches/0004-pex-toggle.patch` adds exactly that,
`SessionOptions::disable_pex` gating both directions, which is the shape of the
upstream change needed and the evidence that it is a small one.

The warning names what is still happening rather than what is missing, because
this flag's failure is a privacy expectation and not a performance knob:

```
--no-pex is accepted but peer exchange stays on: librqbit 9.0.0 has no switch
for it, so your address is still gossiped to the swarm; see
TODO/cli-surface.md T-181
```

`--no-pex` is declared on `seed` and on no other command, so there is one place
to warn from and it is the one `--superseed` already warns from.

**The test that stops this recurring is
`every_flag_reaches_code_or_is_a_named_exception` in `crates/bit-cli/src/cli.rs`.**
It walks the `clap` tree, reads every `.rs` file in both crates except `cli.rs`
itself, and fails on any flag whose field name appears nowhere. Two names are
on the exception list and each carries the entry that owns it:
`index_out` ([T-116](#t-116--o--index-out-cannot-rename-a-file)) and
`on_piece_verified` ([T-115](#t-115-hooks-do-not-fire-for-every-documented-trigger)).
The list is checked in both directions, so a name that something now reads
fails as stale rather than sitting there.

It reads the tree rather than `include_str!`ing a fixed list, because a file
added later would otherwise silently stop being searched, which is the same
class of gap this test exists for. It asserts it read more than twenty files
and found more than a hundred flags, so a test that is looking at nothing fails
instead of passing.

Proven by unwiring one flag:

```
$ cargo test -p bit-cli --lib -- every_flag_reaches_code   # engine_options download_rate set to None
these flags parse and nothing outside cli.rs reads them...
  max_overall_download_rate  (bit-cli download)
test result: FAILED. 0 passed; 1 failed
```

**What the check is deliberately weak about, and why.** It cannot tell a flag
that works from one that only warns, because `--superseed` and `--no-pex` both
read their field and both do nothing but print. Warning is the honest behaviour
for a flag that cannot yet do what it says, so a test that failed on it would
push the wrong way. What it catches is the case that hid for a whole session: a
field nothing reads at all.

**And it is weak in a second way, which a fifth flag found immediately.**
`--web-seed-list-url` passes this test. Its field is read, in
`crates/bit-cli/src/webseed_args.rs`, and what it was read into was a function
that always errors, on every call site including `download`. So the flag
parsed, was read, and could only ever fail. That is
[T-183](#t-183---web-seed-list-url-is-read-only-into-a-refusal), filed and
fixed in the same session, and it is why the count in `CHANGELOG.md` is now
written as "the test is the point" rather than as a number. Two revisions of
that section have been wrong about the number in opposite directions.

```
$ cargo test --workspace
test cli::tests::every_flag_reaches_code_or_is_a_named_exception ... ok
test swarm::tests::a_tracker_list_url_contributes_every_tracker_it_names ... ok
test swarm::tests::a_tracker_list_url_composes_with_the_flags_beside_it ... ok
test swarm::tests::a_tracker_list_url_on_a_no_network_command_fails_clearly ... ok
test swarm::tests::the_overall_rate_caps_the_session_and_the_plain_one_caps_a_torrent ... ok
test swarm::tests::one_rate_scope_never_stands_in_for_the_other ... ok
test cmd::seed::tests::no_pex_warns_that_peer_exchange_stays_on ... ok
test cmd::seed::tests::a_seed_without_no_pex_says_nothing_about_peer_exchange ... ok
test cmd::download::tests::a_tracker_list_url_is_fetched_and_every_tracker_in_it_is_announced_to ... ok
```

### T-182 A macOS test asserted an invariant across two kernel subsystems

Source:      CI run 32478382564, 2026-08-21
Category:    ci
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `Test (macos-latest)` failed on a **documentation-only commit**:

             ```
             test sysinfo::tests::a_process_sample_reports_memory_cpu_and_handles ... FAILED
             thread '...' panicked at crates/bit-cli-core/src/sysinfo.rs:1144:9:
             assertion failed: sample.peak_rss_bytes >= sample.rss_bytes
             ```

             The other fifteen jobs were green, including `Test
             (windows-latest)` and `Test (ubuntu-latest)` running the same
             test.
Relevance:   A peak below the current reading is not a peak, so the assertion
             is asking for something a reader of the report would also assume.
             It failed anyway, and the reason is that on Darwin the two numbers
             do not come from the same place.
Approach:    Read where each number comes from on each platform before
             deciding whether the test or the code is wrong.
Acceptance:  The assertion holds on all three platforms for a reason rather
             than by luck, and the reason is written where the code is.

**The code was wrong, not the test, and the platform is why.**

`Process::sample()` fills `peak_rss_bytes` and `rss_bytes` from one source on
two platforms and from two sources on the third:

| Platform | `peak_rss_bytes` | `rss_bytes` | Same source? |
| --- | --- | --- | --- |
| Windows | `PROCESS_MEMORY_COUNTERS.PeakWorkingSetSize` (`sysinfo.rs:442`) | the same struct | yes |
| Linux | `VmHWM:` in `/proc/self/status` (`sysinfo.rs:663`) | `VmRSS:` in the same read | yes |
| macOS | `getrusage(RUSAGE_SELF).ru_maxrss` (`sysinfo.rs:986`) | `proc_pidinfo`'s Mach `resident_size` (`sysinfo.rs:993`) | **no** |

`ru_maxrss` is the BSD layer's high-water mark. `resident_size` is the current
Mach task footprint and counts pages the BSD accounting does not. They are two
subsystems' numbers, so on Darwin the current reading can exceed the recorded
peak, and no ordering between them is guaranteed. Windows and Linux each read
both fields from one structure, so neither can disagree with itself this way,
which is why fifteen jobs were green.

**Fixed by clamping at the source rather than by weakening the test.**
`peak_rss_bytes = peak_rss_bytes.max(rss_bytes)` in the Darwin path, applied
only when `getrusage` actually succeeded, so a failed read stays in
`unavailable` rather than being backfilled from another subsystem. The clamp is
honest: the process has just been observed at `rss_bytes`, so its peak is at
least that. Weakening the assertion to `#[cfg(not(target_os = "macos"))]` would
have made the test pass and left `bench` and `soak` reports carrying a
`peak_rss_bytes` that means one thing on two platforms and another on the
third, which is the field [T-042](memory.md) built and [T-040](memory.md)'s
slope rests on.

The assertion now also prints both numbers on failure. The original was a bare
`assert!` and the CI log carried no values, so the first question a reader asks
had to be answered by reasoning about the platform rather than by reading the
output.

**The fourth documentation-only commit to turn a job red, and the fourth time
that was the cleanest available proof the test was wrong rather than the
tree.** [T-160](#t-160-a-peers-test-raced-its-own-seeder),
[T-162](webseed.md) and this one all had nothing but Markdown in the diff.
[T-148](bench.md) is the same family found locally. The rule they keep writing
is in [RULES.md](RULES.md): a test never asserts that the machine cannot fail
some other way. This one asserted that two kernel subsystems agree.

`cfg(unix)` being a family and not a platform is the same lesson
[T-145](#t-145-the-macos-test-job-fails-to-link) cost a red job for, where
`posix_fallocate` was declared under `cfg(unix)` and does not exist on Darwin.
That one was a link error and loud. This one was an assertion that held on the
developer's machine and on two of the three runners, which is quieter and took
a push to find.

```
$ cargo test -p bit-cli-core --lib sysinfo
test sysinfo::tests::a_process_sample_reports_memory_cpu_and_handles ... ok
test result: ok. 20 passed; 0 failed
```

### T-183 --web-seed-list-url is read, only into a refusal

Source:      found while building [T-181](#t-181-four-flags-are-accepted-in-silence-and-reach-no-code), 2026-08-21
Category:    cli
Priority:    P1
Effort:      S
Status:      **done**

Problem:     `--web-seed-list-url <URL>` fetches a newline-separated list of
             web seed URLs. Its field was read, at
             `crates/bit-cli/src/webseed_args.rs`, and what it was read into
             was `webseed_args::no_network`, a function whose entire body is an
             error. **Every** call site passed it: `download`, `bench leech`,
             `bench webseed`, `webseed list`, and the dry runs. So the flag
             parsed, was read, and could only ever fail, on every command that
             accepts it, with a message telling the caller that "this command
             does not use the network" on a command whose whole job is the
             network.
Relevance:   This is the P1 definition in `INDEX.md` verbatim, and it is worse
             than the four T-181 found in one specific way: it is invisible to
             the audit that found them. That audit was "every `pub` field in
             `cli.rs` grepped for a reader outside that file", and this field
             has a reader. So does the `clap`-tree test T-181 built to stop a
             fifth appearing, which is why that test's own entry now says what
             it is weak about.

             The flag is also undocumented: it appears nowhere in `README.md`.
             That is how it stayed unnoticed, because nothing described it and
             so nothing contradicted what it did. `docs/flags.md` is not the
             gap, and saying so would mislead: that file is **short flags
             only**, by its own opening line and by the rule it exists to
             enforce, and no long flag has a row in it. The sibling
             `--web-seed-file` is undocumented in `README.md` too, so this is
             one instance of a wider gap rather than a hole around one flag.
Approach:    Give the commands that use the network a real fetcher, and leave
             the ones that must not with the refusal. The fetch itself is
             shared with `--tracker-list-url`, because the two flags read the
             same format, take the same risks, and were built in the same
             session.
Acceptance:  A loopback URL serving one mirror produces one source with
             `origin: "list_url"` in `download --json`, and that source serves
             the payload.

**Fixed with the same bounded fetcher T-181 built.**
`crate::source::fetch_list` refuses a scheme that is not HTTP or HTTPS, sets a
thirty second deadline, caps the body at one mebibyte, and reads in chunks so
the cap bounds what is held rather than only what is returned.
`crate::source::list_fetcher` binds it to the runtime the command has already
made, rather than building a second runtime inside the first.

**Which commands fetch, and which still refuse, is now a decision rather than
an accident.**

| Command | Behaviour | Why |
| --- | --- | --- |
| `download` | fetches | it is the command that downloads |
| `bench leech` | fetches | it downloads for real, and measures what it downloads |
| `download --dry-run` | refuses | a dry run reports without doing |
| `bench leech --dry-run` | refuses | the same |
| `webseed list`, `test`, `fetch` | refuses | `cmd/webseed.rs` `resolve` is documented as resolving "without touching the network", which is what makes `webseed list` safe to run against an unknown torrent |
| `bench webseed` | refuses | it measures the sources it is given, and fetching a list would change what is being measured |

The refusal's message was rewritten while it was there. It named
`--web-seed-list-url` specifically, and it now backs `--tracker-list-url` too,
so it names the URL rather than a flag.

**The lesson is about the audit, not the flag.** "A field with no reader" and
"a field whose reader cannot succeed" look identical from the outside and are
found by different methods. The first is one `grep`. The second was found by
reading the call sites of the function the field is read into, while wiring an
unrelated flag through the same file. Nothing systematic found it, and the
`clap`-tree test in [T-181](#t-181-four-flags-are-accepted-in-silence-and-reach-no-code)
does not find it either. What that test does is stop the cheap case, and the
honest thing is to say so in its own docstring, which it does.

```
$ cargo test -p bit-cli --lib -- a_web_seed_list_url_is_fetched
test cmd::download::tests::a_web_seed_list_url_is_fetched_and_its_sources_are_used ... ok
test result: ok. 1 passed; 0 failed
```

The test points `--web-seed-list-url` at a loopback file server serving a
one-line list, and asserts the torrent finishes, that its single source has
`origin: "list_url"`, and that the source served all 2000 bytes. Before the
fix the run exits on a usage error, so the assertion that fails first is the
exit code.


### T-185 --exclude-file on its own selects nothing and downloads everything

Source:      found while measuring [T-184](disk-io.md), 2026-08-21
Category:    cli
Priority:    P1
Effort:      S
Status:      **done**, 2026-08-22T01:40Z

Problem:     `--exclude-file <INDEX>` skips files. Used **without**
             `--select-file` it does nothing at all: `selection` in
             `crates/bit-cli/src/cmd/download.rs` returns `None` the moment the
             selected list is empty, `None` means every file, and the excluded
             set is dropped on the floor. The comment above that `return`
             says the exclusion "is applied once the metadata resolves", and
             nothing anywhere applies it. `options.only_files` has exactly one
             reader, the `AddOptions` built in `one_inner`, and it receives
             `None`.
Relevance:   This is the `INDEX.md` P1 definition verbatim, and it is the third
             of its family after [T-181](#t-181-four-flags-are-accepted-in-silence-and-reach-no-code)
             and [T-183](#t-183---web-seed-list-url-is-read-only-into-a-refusal).
             It is a different shape from both, which is why neither audit
             found it: the field **is** read, and the reader **can** succeed.
             What is wrong is that one branch of the function that computes the
             value discards half its input. A flag that works when paired with
             another flag and silently does nothing alone is invisible to any
             check that asks whether a field reaches code.

             The cost is not a missing file, it is a fetched one.
             `--exclude-file` is how a caller skips the 40 GiB extras track in a
             torrent it wants 200 MiB of, so the failure mode is a download
             that is two orders of magnitude larger than asked for and reports
             `completed`.
Approach:    The excluded set alone needs the file count, which is the reason it
             was left. Both halves of that are available:

             1. **A local `.torrent`, a fetched one, or a Metalink.** `run`
                already parses the metainfo into `metas` before any plan starts,
                for [T-140](multi-source.md)'s donation proof. The count is
                there, so the exclusion resolves before `add` and nothing is
                ever fetched.
             2. **A magnet.** No file list until the metadata resolves.
                `librqbit` 9.0.0 has `Api::api_torrent_action_update_only_files`
                at `src/api.rs:337`, so the selection can be narrowed after
                `wait_until_initialized` and before anything is asked for. A
                magnet has no payload to fetch before that point, so nothing is
                wasted.

             Refusing a magnet outright is the cheaper option and is worse: it
             would make the flag work for one source kind and error on another,
             which is the asymmetry `--select-file` does not have.

             While this is open, `--select-file` with an **open-ended** range,
             `--select-file 3-`, is refused for the same missing count. Both
             halves of the count problem have the same two answers, so decide
             them together.
Acceptance:  A two-file torrent served by one mirror, downloaded with
             `--exclude-file` naming one file and no `--select-file`, finishes
             with only the other file under `--dir`, and the mirror is never
             asked for the excluded file's URL.

**Measured, not read.** A `sharing_pair` donor fixture is `extra-a.txt` (1024
bytes) and `shared.bin` (4096) at a 1024 byte piece length, so every file is a
whole number of pieces and no boundary straddles: what lands on disk is exactly
what was selected, with none of [T-184](disk-io.md)'s boundary writes to
confuse it. `create` sorts by path, so index 0 is `extra-a.txt`.

```
$ bit-cli --json download donor.torrent --dir out --web-seed-only \
    --web-seed http://127.0.0.1:PORT/payload/ --web-seed-mode prefix \
    --no-torrent-web-seed --no-tracker --port 0 --exclude-file 1 --stop-after 20s
stopped=completed
files on disk: ["donor/extra-a.txt", "donor/shared.bin"]

$ ... --select-file 0 ...
stopped=completed
files on disk: ["donor/extra-a.txt"]
```

The first run excluded index 1 and downloaded it anyway. The second selected
index 0 and got exactly that, which is the control: the selection machinery
works and only the exclusion-alone path is dead. An earlier run of the first
command against a mirror missing that file failed with a 404, which is the same
finding from the other side: the run asked a mirror for a file it had been told
to skip.

**Closed 2026-08-22T01:40Z**, and both halves of the count problem were decided
together the way the approach asked.

The count is per source, not per run, so it is resolved per source. `run`
already parses the metainfo of a local `.torrent`, a fetched one and a
Metalink's into `metas` before any plan is handed out, so `plan_selection` in
`crates/bit-cli/src/cmd/download.rs` settles each plan's `FileSelection` there,
before the session starts. A usage error surfaces before anything is added
rather than per worker.

A magnet defers, and only when it has to.
`crate::selection::needs_file_count` is the one place that says which two
spellings need a count: an exclusion with no selection beside it, and an
open-ended range. A magnet with neither adds exactly as before and pays
nothing.

**The magnet answer is not the one the approach named, and the reason is worth
recording.** `Api::api_torrent_action_update_only_files` does exist and does
narrow a live torrent, but narrowing **after** the add is too late for the
thing this entry is about. `librqbit`'s initial check creates and opens every
file it was not told to skip, so a selection applied afterwards has already
created what it excludes. Measured twice, from both sides:
`--hash-check-only --select-file 1` against an empty directory creates the
selected file at its full length and no other, and [T-186](#t-186-seed---data-and-verify---data-resolve-the-payload-differently)'s
`seed` against an empty directory, which has no selection at all, creates the
whole tree. `Engine::resolve_with` reads the metadata first, with
the caller's own trackers and `--peer` addresses so it resolves against the
swarm the add is about to use, and it hands back the `.torrent` bytes it built.
The add then takes those bytes, so this is one metadata resolution and not two.
The seam upstream is `librqbit-9.0.0/src/session.rs:1298`, where `list_only`
returns after `resolve_magnet` and before any storage exists.

That resolution is bounded by `--init-timeout`, which `engine.add` never bounded
for a magnet at all. A swarm that never answers now reports the phase rather
than hanging the run.

`crate::selection::resolve` no longer answers `None` when it is asked for an
exclusion's complement without a count. `None` means every file, which is the
flag doing the opposite of what it says, and that silence is what this entry
was. It is a usage error now, so a caller that skips `needs_file_count` fails
loudly instead of quietly downloading everything.

Measured against `target/release/bit-cli`, on the fixture above rebuilt with
`bit-cli create --piece-length 1024`: `extra-a.txt` 1024 bytes at index 0,
`shared.bin` 4096 at index 1, five pieces, nothing straddling.

```
$ bit-cli --json download donor.torrent --dir out --web-seed-only \
    --web-seed http://127.0.0.1:57364/ --web-seed-mode prefix \
    --no-torrent-web-seed --no-tracker --no-dht --no-lsd --port 0 \
    --exclude-file 1 --stop-after 20s
stopped= completed
downloaded= 1024
files on disk: donor/extra-a.txt
mirror was asked for: GET /   GET /extra-a.txt
```

The mirror's own log is the half that says the exclusion was applied before the
fetch rather than after it: `GET /` is [T-004](webseed.md)'s style probe, and
`shared.bin` was never asked for.

The magnet, against a loopback seeder with `--peer` and no tracker, no DHT and
no LSD:

```
$ bit-cli --json download magnet:?xt=urn:btih:9bef473bd4483a6e51c2f5194e983712f8edfec0 \
    --dir out --peer 127.0.0.1:51899 --no-tracker --no-dht --no-lsd --port 0 \
    --exclude-file 1 --init-timeout 60s --stop-after 60s
stopped= completed
downloaded= 1024
files on disk: donor/extra-a.txt
```

And `--select-file 1-`, the open-ended range that was refused for the same
missing count, on the same magnet:

```
$ ... --select-file 1- ...
stopped= completed
downloaded= 4096
files on disk: donor/extra-a.txt (0 bytes), donor/shared.bin (4096 bytes)
```

Six tests. `an_exclusion_with_no_selection_skips_the_file_and_never_asks_for_it`
and `a_magnet_resolves_its_metadata_before_it_applies_an_exclusion` are the two
acceptances, and both were run against the old behaviour first: the magnet one
fails with `["donor/extra-a.txt", "donor/shared.bin"]`, which is the defect.
`crate::test_support::FileServer` grew a request log for the first of them,
because what a mirror was **not** asked for is the only evidence that a
selection was applied before the fetch.

**That third run found something this entry did not**: `extra-a.txt` lands as a
zero byte file when it is not selected and the selection starts after it. It is
not this entry's, and it is not new: `--select-file 1`, which needed no count
and went through unchanged code, does the same. It is filed as
[T-188](disk-io.md) with the cause, and it corrects
[T-013](disk-io.md)'s closing claim.

### T-186 seed --data and verify --data resolve the payload differently

Source:      found while building [T-184](disk-io.md)'s acceptance, 2026-08-21
Category:    cli
Priority:    P3
Effort:      S
Status:      **done**, 2026-08-22T03:00Z

Problem:     A multi-file torrent lays its files under a directory named after
             itself, so a payload can be pointed at two ways: at the parent, or
             at the torrent directory. `verify --data` accepts either and picks
             whichever holds the first file, which its `resolve_root` says in
             so many words. `seed --data` sets the session's download directory
             and only ever looks at `<data>/<name>/`, so the torrent directory
             is refused with no message that says so.
Relevance:   The two commands read the same layout, written by the same
             `download`, and their `--data` flags carry the same name and the
             same help text. A caller who verified a payload one way and seeds
             it the other gets a seeder holding nothing.

             What makes it worth a P3 rather than nothing is the message.
             Pointed at the torrent directory, `seed` reports `have: 0` and
             warns "only 0 B of 3.61 KiB is present, so this is a partial
             seed", which is the right observation with the wrong reason. A
             partial seed is legitimate and the warning is the one a partial
             seed gets, so nothing distinguishes "you have half the payload"
             from "you named the wrong directory".
Approach:    Give `seed` the resolution `verify` already has. It is one call,
             and the two commands would agree by construction rather than by
             both being right separately.

             The alternative, warning when nothing is found and a sibling
             directory would have worked, is a special case where a shared
             function is available, and it leaves the two flags meaning two
             things.

             Watch the direction: `verify` picks whichever candidate holds the
             **first file**, and a run that legitimately holds nothing at all
             has no first file to find. That is why `seed` cannot simply take
             the same function without deciding what it does when neither
             candidate exists, which today is what `--data` said.
Acceptance:  `bit-cli seed <MULTI> --data out/<name>` and
             `--data out` report the same `have` for the same payload on disk,
             and a `seed` that finds nothing where a sibling directory holds
             the payload says which directory it looked in.

**Measured before building, and the premise held exactly.** A two-file torrent,
3,000 and 1,000 bytes at a 1,024 byte piece length:

```
$ bit-cli verify album.torrent --data .tmp/t186        pieces ok 4 of 4
$ bit-cli verify album.torrent --data .tmp/t186/album  pieces ok 4 of 4

$ bit-cli seed album.torrent --data .tmp/t186          have 3.91 KiB of 3.91 KiB
$ bit-cli seed album.torrent --data .tmp/t186/album
warning: only 0 B of 3.91 KiB is present, so this is a partial seed
                                                       have 0 B of 3.91 KiB
```

**One thing the entry did not know**: the wrong spelling does not only report
nothing, it writes. `seed` hash-checks on add, which creates the tree it is
looking for, so pointing at the torrent directory left an empty `album/album/`
inside it at full length.

**Closed 2026-08-22T03:00Z.** `crate::payload::resolve` is the shared rule, in a
module of its own for the reason [`crate::selection`](#t-185---exclude-file-on-its-own-selects-nothing-and-downloads-everything)
is: two commands need the same answer from the same flag, and a second copy is a
second set of off-by-one bugs. `verify::resolve_root` is now the `--data` fallback chain and
one call to it.

`seed` takes the resolved root as `AddOptions::output_folder` rather than as the
session's download directory. That is what makes it right for a **renamed**
payload directory as well: naming the folder means the files hang directly off
it, where letting the session append the torrent's own name assumes the
directory is still called that.

```
$ bit-cli seed album.torrent --data .tmp/t186        have 3.91 KiB of 3.91 KiB
$ bit-cli seed album.torrent --data .tmp/t186/album  have 3.91 KiB of 3.91 KiB
```

and nothing is created a level deeper by either.

**The message went through two shapes and the second one is the point.** The
first said the first file was in neither candidate, which is what
`resolve` actually checks. It is true on the first run and false on every run
after it, because the run before created that file at full length with nothing
in it. Keyed on bytes instead:

```
$ bit-cli seed album.torrent --data .tmp/t186/empty
warning: only 0 B of 3.91 KiB is present, so this is a partial seed
warning: none of album is in <dir>\empty, which is where --data
         resolved to; a multi-file torrent's files also sit under
         <dir>\empty\album
```

Two warnings, and they say different things on purpose. The first is what a
partial seed gets and a partial seed is legitimate. The second only fires on
nothing at all, which is the case a partial seed's wording could not describe,
and it names both directories. A complete seed says neither, which
`a_complete_seed_says_nothing_about_where_it_looked` pins.

Seven tests. `either_spelling_of_data_seeds_the_same_payload` is the acceptance
and was run against the old behaviour first: the torrent directory reports
`have: 0` where the parent reports 2,000.
`a_seed_that_holds_nothing_names_the_directories_it_searched` runs twice over,
because a message keyed on the files existing would pass once and fail after.

### T-193 A citation written short was never checked at all

Source:      found in this session's own review 1, 2026-08-22
Category:    cli-surface
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-22T11:21Z

Problem:     `scripts/check-todo.ps1` resolved a citation written long, as
             `crates/bit-cli/src/cli.rs:2322`, and checked only that the file
             had that many lines. Most of `TODO/` does not write them long. A
             citation written as `cli.rs:2322` matched nothing in the pattern,
             so it was never resolved, never range checked, and never read.
Relevance:   `RULES.md` section 2 step 4 says the mechanical half of the two
             reviews answers "a cited path that does not resolve". For the
             common spelling it answered nothing, and the record is built on
             citations.
Approach:    Index every `.rs` under `crates/` by bare name and resolve a short
             citation through it, skipping a name two files share, because
             guessing which one was meant is worse than saying nothing. Then
             check the line rather than only the count: where the prose names a
             symbol beside the citation, and that symbol occurs **exactly
             once** in the file, the citation has to be within a few lines of
             it. Once, because a name the file uses twice cannot say which
             occurrence was meant, and a wrong complaint is worse than a
             missing one.
Acceptance:  A citation whose target has moved fails the check, named, with the
             line it moved to.

**What it found the day it was written: nine stale line numbers across seven
citations, in prose four sessions of two-deep-reviews had passed.**

The old line numbers are written without their file here, so this record does
not read as seven live citations and report itself.

| file | what it names | said | is at |
| --- | --- | --- | --- |
| `cli.rs` | `short_flags_keep_their_aria2_meanings` | 1833 | 1924 |
| `cli.rs` | `no_short_flag_is_defined_twice` | 2012 | 2103 |
| `cli.rs` | `short_flags_never_contradict_aria2` | 2048 | 2139 |
| `cli.rs` | `every_short_flag_is_documented_in_the_flags_table` | 2107 | 2332 |
| `schema_gen.rs` | `the_committed_schema_matches_what_the_program_writes` | 734 | 1068 |
| `storage.rs` | the two BEP 47 padding guards | 728 and 870 | 1048 and 1216 |
| `storage.rs` | `pwrite_all_vectored` and `pwrite_all` | 799 and 781 | 1119 and 1107 |

Three of the four `storage.rs` numbers, 728, 799 and 781, were correct at
`f46d4fd` and were moved by the write buffer [T-018](disk-io.md) landed the same
morning, checked by reading the file at that commit. 870 was already wrong
there: the guard it names was at 891. The five in `cli.rs` and `schema_gen.rs`
had been wrong for longer. A tenth, `storage.rs:402` in
[T-190](disk-io.md)'s own Approach, was made stale by this session and is
corrected there. Every one of them points at
plausible code, which is what makes them expensive: a reader following the old
line 870 of `storage.rs` lands on `let wanted = slash_path(path)` and has no
reason to doubt it.

Proved by putting two of them back and running the check:

```
[drifted-line] cli-surface.md:557 cites cli.rs:2012 for `no_short_flag_is_defined_twice`, which is at :2103
[drifted-line] cli-surface.md:1178 cites schema_gen.rs:734 for `the_committed_schema_matches_what_the_program_writes`, which is at :1068
```

Then corrected again, and the check is silent.

**What it cannot see**, so the next reader does not expect more of it than it
gives: a citation with no symbol named beside it, a symbol the file uses more
than once, a name shorter than ten characters or without an underscore, and a
citation into `reference/`, which is checked for range only as before. It
catches the drift that comes from editing this tree, which is the drift this
repository produces.

### T-196 A magnet that never resolves hangs download with no diagnostic

Source:      cost a measurement while proving [T-194](peers.md), 2026-08-22
Category:    cli-surface
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-22T18:33Z

Problem:     `bit-cli download <magnet>` bounds metadata resolution by
             `--init-timeout` only when a file selection forces it to resolve
             first. Without `--select-file` or `--exclude-file` it calls
             `engine.add` instead, which resolves with no bound at all, and
             the `wait_until_initialized_within` that would have applied
             `--init-timeout` is on the next line and never reached.
Relevance:   The comment beside the bounded branch already says why the bound
             is there: "a magnet that never resolves used to hang the run
             rather than report why". That is still true of the other branch,
             which is the one an ordinary invocation takes.
Approach:    Bound the unbounded branch by the same `--init-timeout`, and
             report the same timeout error with `phase: resolving_metadata`.
             The bounded branch already builds that error, so this is moving
             it rather than writing it.
Acceptance:  A magnet with one peer that cannot serve it exits non-zero within
             `--init-timeout` and names the phase, rather than running until
             something else kills it.

**How it was found.** A magnet download against a local seeder that could not
send its bitfield ran for **ten minutes** and was killed by the harness, not by
`bit-cli`. `--init-timeout` was not passed, but it would have made no
difference: that invocation had no file selection, so it took the branch with
no bound. The defect it was hiding was [T-194](peers.md), and the ten minutes
bought nothing: the seeder had already logged the reason in the first second.

Both halves of the inconsistency are in one function, about fifty lines apart.

**Closed 2026-08-22.** The add is wrapped in the same `--init-timeout` and
builds the same error, with `phase: resolving_metadata`, which is what the
Approach said to do.

**The per-torrent report carries the phase now, and it did not.** The error has
a context map and `TorrentReport` copied none of it, so a run that gave up
resolving a magnet and a run that gave up fetching its pieces both said
`timeout` and nothing else. `torrents[].phase` is a new optional field in
`docs/schema.md`; a run that got past initialising leaves it out.

**Measured**, `scripts/check-init-timeout.ps1`, a magnet whose one peer
completes the handshake and then says nothing, DHT, LSD and trackers off:

| case | before | after |
| --- | --- | --- |
| `selection`, which was already bounded | 4.05 s, `timeout` | 4.05 s, `timeout`, `resolving_metadata` |
| `no_selection`, an ordinary invocation | **10.04 s**, `source_resolution` | **4.04 s**, `timeout`, `resolving_metadata` |

```bash
pwsh -NoProfile -File scripts/check-init-timeout.ps1
```

`selection` is the control: it forces the branch that already had the bound, so
a failure in the other one cannot be blamed on the fixture.

**Where the ten seconds comes from, and why the fixture cannot show ten
minutes.** Before the fix the branch was not unbounded in this fixture, it was
bounded by somebody else's timeout: with one address and one peer, `librqbit`
gives up with "input address stream exhausted" once that peer's read/write
timeout expires. Three fixtures were tried and the first two are worth
recording, because each looks like it measures this and does not:

- **A closed port.** Two seconds, same exhaustion. The connection fails at once.
- **Accept and never write.** Ten seconds, same exhaustion.
- **Handshake and then silence**, with BEP 10's reserved bit set. Ten seconds,
  same exhaustion. Keep-alives on top of it moved the number by nothing.

What made the original run last ten minutes was a tracker and a DHT still
handing out addresses, so nothing ever exhausted. A fixture cannot reach that
without the network. What it does show is the thing the Acceptance asks for: a
4 second `--init-timeout` was ignored and is now the bound, and the phase is
named. `-Slack` defaults to 5 so a run that falls back to the ten second path
fails on the clock as well as on the code.

### T-197 Running upstream's tests filled the patch series with 14,964 patches

Source:      found by running the command `patches/README.md` gives, 2026-08-22
Category:    cli-surface
Priority:    P1
Effort:      S
Status:      **done**, 2026-08-22T14:20Z

Problem:     `scripts/vendor-diff.ps1` and `scripts/vendor-sync.ps1` walked a
             vendored tree with `Get-ChildItem -Recurse -Force` and treated
             every file they found as vendored source. Building that tree
             leaves `target/`, `node_modules/` and
             `crates/librqbit/webui/dist/` in it. `vendor-diff` then hashed
             7.2 GB across 9,894 files and wrote **14,964 patches**, having
             looked hung for seven and a half minutes first.
Relevance:   The command that produces those directories is the one
             `patches/README.md` step 5 tells a session to run, so this is
             reachable by following the instructions exactly. And a 14,964
             patch series is not a series: `vendor-status` would have reported
             the fork healthy while the record of what this repository changed
             was mostly somebody else's build output.
Approach:    Skip a path that a `.gitignore` **inside the vendored tree**
             ignores. That is upstream saying the file is generated, and it is
             derived rather than listed, so a new build directory needs nothing
             remembered. The qualifier matters: `vendor-sync`'s `Get-Swallowed`
             has to keep reporting a file that this repository's **own root**
             `.gitignore` would eat, which is the `.vscode/` case
             `docs/vendoring.md` describes, so filtering on "ignored" flat
             would have deleted a check while fixing a bug.
Acceptance:  `vendor-diff.ps1` writes the patches for the tree's real changes
             and nothing else, with a build directory present.

**Measured, on the tree that had one:**

| | before | after |
| --- | --- | --- |
| patches written | 14,964 | 7 |
| wall clock | 7 m 33 s | 6.1 s |

The seven are the two changes recorded in
[`patches/UPSTREAM.md`](../patches/UPSTREAM.md) and the two lockfiles that
follow the second one.

**The other half of the fix is not to make the mess.**
`patches/README.md` and `docs/vendoring.md` now give the command with
`--target-dir target/vendor-rqbit`, so cargo writes its build output outside a
tree that is supposed to hold nothing but somebody else's source. The scripts
had to be fixed anyway: a session that forgets the flag, or a `cargo build`
that generates the web UI, must not be able to poison the series.

**What this cost before it was found.** Twelve minutes of a session, and the
first sign of it was `vendor-diff.ps1` producing no output at all, which reads
as a hang rather than as work. It was found by checking what the script was
walking, not by waiting longer.

### T-198 An agent that wants a flag name greps for it

Source:      the operator, 2026-08-22, having watched it happen in that session
Category:    cli-surface
Priority:    P1
Effort:      M
Status:      **done**, 2026-08-22T16:00Z

Problem:     Nothing in this repository stated the command surface in a shape a
             program could read. A caller that needed a flag name had three
             options: grep the source, page `--help` one subcommand at a time,
             or guess. The last one costs a run that exits 2, or worse one that
             succeeds having done something else.
Relevance:   Most of the work on this repository is done by an agent, and the
             cost is paid on every session. It was paid in the session this
             entry was filed in.
Approach:    Generate the surface, commit it, and fail the build when it drifts.
             Three shapes, because the readers are different: troff for a
             terminal, Markdown for prose, and a CLIspec 0.3 document for a
             program.
Acceptance:  A flag renamed without regenerating fails `cargo test -p bit-cli`,
             naming the file and the line.

**What is in `man/`**, all generated from the clap definition, all committed:

| file | bytes | for |
| --- | --- | --- |
| `bit-cli.1` | 51,394 | a person at a terminal |
| `bit-cli.md` | 69,860 | prose, one table per command |
| `bit-cli.json` | 137,020 | a program: [CLIspec](https://github.com/rvben/clispec) 0.3 |

28 commands, 20 global options, and all 17 non-zero exit codes with a
`retryable` flag on each. [`docs/man.md`](../docs/man.md) says what each field
carries and why.

**It cannot go stale.** `cargo test -p bit-cli --test man_is_current` renders
all three from the crate being compiled and compares. That is in
`cargo test --workspace`, so it is in the gates and in CI on every platform.
`scripts/check-man.ps1 -Fix` regenerates, and `gates.ps1` runs the script as a
named `man` gate so a session is told what to run rather than reading a test
name out of a failure. The test is what binds: the script compares against
`target/release/bit-cli`, which can be older than the source in front of it.

**Two bugs it caught in its own first output**, both of the kind a reader would
have believed:

- **`--web-seed` was typed `boolean`** while carrying `value_name: URL`.
  `clap::Arg::get_num_args` is empty until the command is built, so every flag
  that takes a value was reported as one that does not. Read from the action
  now, and the command is built before it is walked.
- **`create --version` disappeared.** Filtering clap's generated `--version` by
  argument id also deleted the metainfo version flag, which takes `v1`, `v2` or
  `hybrid`. Filtered by action now.

Both are in a generated file that nothing was checking, which is the argument
for the test rather than for the generator.

**The one thing not generated** is `effects`, CLIspec's word for whether a
command is `read_only`, `idempotent` or `non_idempotent`, because nothing in a
clap definition says whether a command writes. It is a table in
`crates/bit-cli/src/cmd/spec.rs` and a subcommand missing from it fails
`every_subcommand_is_classified`, rather than shipping an empty `effects` that
a reader would take to mean "no side effects". Eleven nested subcommands were
caught by exactly that on the first run.

The Markdown is rendered from the CLIspec document rather than from clap a
second time, so those two cannot disagree about a flag.

[RULES.md](RULES.md) section 4a carries the rule this exists to serve: read
`man/bit-cli.json` before typing a flag.

### T-199 The CI supply chain was unwatched and one action was abandoned

Source:      the operator, 2026-08-22
Category:    cli-surface
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-22T16:00Z

Problem:     Nothing watched dependency or action versions, and
             `ilammy/setup-nasm@v1.5.2` had gone unmaintained: it is that
             project's newest release, it still runs on node20, and GitHub
             warns about the deprecation on every job. It was used in five
             places across two workflows.
Relevance:   A node20 action stops working when GitHub retires the runtime, and
             the first sign would be every Windows job failing at once. The
             warning had been there long enough to be pinned with a comment
             saying to revisit it.
Approach:    Replace the action with a script in this repository, and add
             `dependabot.yml` so the next one is noticed by a bot rather than
             by a person reading a warning.
Acceptance:  The script installs NASM and refuses an archive whose checksum
             does not match.

**`scripts/setup-nasm.ps1`** does what the action did, in about thirty lines,
and does one thing the action never did: it verifies the download against a
pinned SHA-256. Both halves were run rather than reasoned about:

```
$ pwsh -NoProfile -File scripts/setup-nasm.ps1 -Force
setup-nasm: sha256 ok
setup-nasm: NASM version 2.16.03 compiled on Apr 17 2024

$ pwsh -NoProfile -File scripts/setup-nasm.ps1 -Force -Sha256 0000...
setup-nasm: checksum mismatch for nasm-2.16.03-win64.zip
  expected 0000000000000000000000000000000000000000000000000000000000000000
  got      3ee4782247bcb874378d02f7eab4e294a84d3d15f3f6ee2de2f47a46aa7226e6
exit=2
```

It is a no-op when `nasm` is already on PATH, and on a runner it appends to
`GITHUB_PATH` so later steps see it. NASM is needed because `aws-lc-sys`
assembles its own primitives, and `cargo tree -i aws-lc-rs` says it arrives
under **two** parents: `rustls`, and `librqbit-sha1-wrapper`, which is the
SHA-1 backend every piece hash goes through. Dropping the TLS one would not
remove the need.

**`.github/dependabot.yml`** covers cargo and github-actions, weekly, grouped.
Grouped because a pull request per crate is sixteen CI runs a week for a
lockfile bump nobody reads, and the workflow's concurrency group cancels runs
in flight, so the noise costs real coverage. Two things are deliberately
excluded and the file says why: **`vendor/` is not watched**, because a bot
rewriting a vendored manifest without moving the recorded base is the one state
`patches/README.md` says must never happen, and `scripts/upstream-scan.ps1` is
how those trees are watched instead; and **`librqbit*` is ignored**, because
`[patch.crates-io]` means a registry bump for it cannot reach the build.

**One consequence worth knowing.** `scripts/setup-nasm.ps1` is now invoked by
the workflows, so `git-sync -NoCi` refuses to treat a commit touching it as
documentation-only. That is derived rather than listed: the script reads
`.github/workflows/` to work out which scripts CI depends on.

### T-213 seed cannot serve a payload renamed by --index-out

Source:      found closing [T-116](#t-116--o--index-out-cannot-rename-a-file)
Category:    cli
Priority:    P3
Effort:      S
Status:      **done** 2026-08-23T11:45Z

Problem:     `download -O 0=renamed.bin` writes the first file to
             `renamed.bin`, and `bit-cli seed` against that directory looks for
             it at the path the torrent names. `seed` builds its `AddOptions`
             at `crates/bit-cli/src/cmd/seed.rs:260` with no `index_out`, so
             the storage plan it hands the session is the unmodified one and
             the renamed file is missing as far as the seeder is concerned.
Relevance:   Downloading a payload and then seeding it back is the ordinary
             thing to do with one, and `-O` is the flag that quietly breaks it.
             P3 rather than higher because it needs the caller to have used
             `-O` in the first place, and because the failure is loud: the
             hash check finds the file missing and says so.
Approach:    The same one `verify` took when T-116 closed: an `-O` flag on
             `SeedArgs`, parsed with `crate::selection::index_out` against the
             file count the metainfo already gives, and passed into
             `AddOptions::index_out`, which the engine already carries. The
             work is the flag and the test, because the machinery underneath
             is what T-116 built.

             Worth deciding at the same time: whether `bit-cli files` should
             report the on-disk path a given `-O` would produce, so a caller
             can ask where a file will land before fetching it.
Acceptance:  A payload downloaded with `download -O 0=renamed.bin` is served by
             `seed <TORRENT> --data <DIR> -O 0=renamed.bin` with the hash check
             finding every piece, and without `-O` the same command reports the
             file missing. Both in one test, because the second is what makes
             the first mean anything.

**Done, and the Approach's "the work is the flag and the test" was wrong by one
function.** The flag went on, the plan carried the override, the report's
`renamed` array said `disc 1/a.flac -> renamed.bin`, and the seeder held
**zero of 2,000 bytes**. Every piece of this torrent touches the first file, so
nothing verified.

**`payload::resolve` is what the flag broke.** `--data` may name the parent of
a multi-file payload or the torrent's own directory, and the way those are told
apart is by looking for **the torrent's first file** under each. `-O 0=...` is
the flag that moves exactly that file, so neither candidate held it, the
resolver fell back to the base, and every file was then looked for one
directory too high. The fix is `resolve_with`, which looks for file 0 where the
caller said it would be.

**`verify` had the same defect and its own test passed anyway**, which is the
part worth keeping. [T-116](#t-116--o--index-out-cannot-rename-a-file)'s test
points `--data` straight at the torrent directory, and from there the fallback
lands on the right answer by accident. Pointing at the parent, which is the
other spelling that flag pair is documented to accept, found nothing. Both
commands take `resolve_with` now and the test carries the second spelling.

**What the second half of the acceptance measures.** Without `-O`, the same
command against the same directory reports `complete: false` and fewer than
2,000 bytes held. That is what says the first half is about the flag rather
than about the fixture.

**The deferred question is still deferred.** The Approach asks whether
`bit-cli files` should report the on-disk path a given `-O` would produce.
Nothing here needed it and no measurement asks for it, so it is not built. It
would be a new entry rather than a clause of this one.

```
$ cargo test -p bit-cli --lib index_out
test result: ok. 6 passed; 0 failed; 0 ignored; 418 filtered out
```

### T-214 seed runs no hooks

Source:      the Problem's third clause in
             [T-115](#t-115-hooks-do-not-fire-for-every-documented-trigger),
             which its Acceptance did not cover
Category:    cli
Priority:    P3
Effort:      S
Status:      **done** 2026-08-23T12:10Z

Problem:     `bit-cli seed` has no `--on-*` flag at all. `--on-complete`,
             `--on-error` and `--on-piece-verified` are on `download` only, so
             a long-lived seeder can tell an external system nothing about what
             it is doing. This is a missing feature rather than a flag that
             does nothing: there is no flag to be inert.
Relevance:   A seeder is the shape that runs for days, which is the shape most
             likely to want a hook. P3 because `--jsonl` already carries every
             event a hook would fire on, so nothing is unreachable, only
             inconvenient for a caller that wants a command rather than a
             stream reader.
Approach:    `crates/bit-cli/src/hooks.rs` is the machinery and it is not
             `download`-specific: `finished_vars` takes a struct of facts
             rather than a `TorrentReport`, and `PieceHook` takes a command.
             What a seeding run means by each trigger is the part to decide
             first, and it is not the same as a download's:

             - **`--on-complete`** has no obvious moment. A seeder does not
                complete. The candidates are "the hash check passed and it is
                now serving", which is the useful one, and "the run ended",
                which is what `--on-error`'s absence would mean.
             - **`--on-error`** is the run failing to start or dying, which is
                well defined.
             - **`--on-piece-verified`** happens once during the hash check on
                add and never again, so it would fire in a burst at the start
                and then be silent. `--on-peer-connected` is what a seeder
                would actually want, and it is a new trigger rather than a
                port of an existing one.

             Decide those before writing any of it, and add whatever variables
             a seeding run needs to `hooks::VARIABLES`, which is the one list
             both `docs/hooks.md` and the tests are held to.
Acceptance:  `bit-cli seed <TORRENT> --data <DIR> --on-complete <CMD>` runs the
             command once, when the payload has been checked and the listener
             is up, with `BIT_CLI_INFO_HASH` set. `docs/hooks.md` says which
             trigger means what on `seed`, and
             `every_hook_variable_is_documented` still passes.

**Done, and the Problem's own sentence is the thing that was wrong.** It says
"This is a missing feature rather than a flag that does nothing: there is no
flag to be inert." There were three, on four commands.

`SeedArgs` flattens `LimitArgs`, and all three `--on-*` flags lived in
`LimitArgs`. Five commands flatten it and one honoured them, so
`bit-cli seed --on-complete notify` parsed, was documented in all three
manuals, and ran nothing. So did `peers`, `bench leech` and `bench seed`. By
this repository's own priority scale that is P1, "a flag does nothing", and it
is the fourth instance after [T-181](#t-181-four-flags-are-read-and-never-acted-on),
[T-183](#t-183---seed-ratio-and---seed-time-are-read-and-never-used) and
[T-185](#t-185---exclude-file-on-its-own-selects-nothing-and-downloads-everything).

**One command surface change, one behaviour change.** The three flags are a
`HookArgs` struct now, flattened by `download` and `seed`. `peers`,
`bench leech` and `bench seed` refuse them with exit 2, which is a caller
learning at once rather than waiting for a notification that was never coming.
`--on-piece-verified` is `download`'s alone.

**The decisions the Approach asked for, all four:**

- **`--on-complete` fires once, before the serve loop**, when the payload has
  passed its hash check and the listener is up. A seeder has no completion, so
  the useful moment is the one where it starts being useful.
- **`BIT_CLI_FINISHED` there is about the payload, not the run.** A partial
  seed is legitimate, so it still fires `--on-complete`, with
  `BIT_CLI_FINISHED=false`. That is why `hook_vars` takes the hook name:
  `finished_vars` chose it from `finished`, which is the same question on a
  download and a different one here.
- **`--on-error` is the run failing to start or dying**, fired on the way out
  with the error in `BIT_CLI_ERROR` and whatever identifies the torrent, which
  for a magnet that never resolved is an info hash and no name.
- **`--on-piece-verified` is not ported.** Every piece is verified once, during
  the hash check on add, so it would fire in a burst at startup and then be
  silent for days. **No entry is filed for a per-peer hook**: the Approach names
  it as the thing a seeder would actually want, it is a new trigger rather than
  a port, and nothing has asked for one. `--jsonl` already carries the event.
- **Nothing fires on `--announce-only`**, which never serves.

**No new variables**, so `hooks::VARIABLES` is unchanged and both tests that
hold it still pass. `BIT_CLI_STOPPED` carries one value a download never sets,
`serving`.

```
$ cargo test -p bit-cli --lib on_complete_fires_once_when
test result: ok. 1 passed; 0 failed; 0 ignored; 425 filtered out

$ cargo test -p bit-cli --lib a_failing_hook_does_not
test result: ok. 1 passed; 0 failed; 0 ignored; 425 filtered out
```

### T-218 The next stable release fails the build on a method the bridge calls

Source:      measured on `beta` while closing [T-150](#t-150-clippy-pins-a-floating-toolchain-so-a-rust-release-can-turn-the-tree-red), 2026-08-23
Category:    ci
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T11:25Z

Problem:     `rustc 1.99.0-beta.1` deprecates
             `std::sync::atomic::Atomic::<u64>::fetch_update`, renamed to
             `try_update`. `request_settled`, in
             `crates/bit-cli-core/src/webseed/bridge.rs`, calls it. The line
             number is in the compiler's own output below rather than here,
             because the fix moves it and this is a record of where it was:

             ```
             error: use of deprecated method
                 `std::sync::atomic::Atomic::<u64>::fetch_update`:
                 renamed to `try_update` for consistency
               --> crates/bit-cli-core/src/webseed/bridge.rs:450:14
                 = note: `-D deprecated` implied by `-D warnings`
             ```
Relevance:   `-D warnings` is set for the whole CI workflow, so this is an
             error rather than a warning, and it is not confined to the lint
             job: `cargo test` and `cargo build` fail on it identically. On the
             day 1.99 becomes stable, every job that was still tracking
             `stable` would go red at once, on a commit nobody touched.

             It is the exact shape T-150 was filed for, arriving six weeks
             early because that entry's tracking job is now there to see it.
             T-150 recorded that the split could not be demonstrated because
             nothing was red on the next toolchain; this is what was red.
Approach:    Not `try_update`, and that is the whole decision. It does not
             exist on the pinned 1.98.0 and it does not exist on the MSRV,
             1.88, which [RULES.md](RULES.md) section 6 says is measured rather
             than chosen. Taking the new name would raise the MSRV by eleven
             releases to silence a lint.

             The call is a saturating decrement of a counter and nothing else:

             ```rust
             self.in_flight
                 .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                     Some(n.saturating_sub(1))
                 })
                 .ok();
             ```

             `fetch_update` is a compare-exchange loop with a closure. Writing
             the loop is four more lines, is what the method compiles to, and
             is not deprecated on any release this repository supports. An
             `#[allow(deprecated)]` is the other candidate and is worse: it
             silences the tracking job for that call site permanently, so the
             next rename in the same file arrives unannounced.
Acceptance:  `cargo +beta clippy --workspace --all-targets --all-features --
             -D warnings` is clean, `cargo check` on 1.88 still passes, and a
             test holds the saturating behaviour the closure carried, which
             nothing does today.

**Done, and the title undercounts it: there were two, and the first run of the
acceptance is what found the second.**

**The Acceptance command was wrong, and being wrong is what found it.**
`cargo clippy -- -D warnings` passes the flag to the crates being linted and
not to path dependencies. CI sets `RUSTFLAGS: -D warnings`, which reaches
everything, and that is the difference between a warning nobody sees and a
failed job. Run the way CI runs it, `beta` had a **second** finding:

```
error: use of deprecated constant `std::f64::INFINITY`:
       replaced by the `INFINITY` associated constant on `f64`
  --> vendor/librqbit-utp/src/congestion/cubic.rs:56:28
```

`cubic.rs` opened with `use std::{f64, ...}`, importing the **module**, so
`f64::INFINITY` resolved to the legacy module constant rather than to the
associated constant on the primitive. Dropping the import is the whole fix and
the expression is untouched. That is
[`patches/UPSTREAM.md`](../patches/UPSTREAM.md)'s nineteenth section.

**The bridge's own fix is the loop rather than the new name.** `try_update`
does not exist on the pinned 1.98.0 or on the MSRV, and
[RULES.md](RULES.md) section 6 says the MSRV is measured rather than chosen, so
taking the rename would mean raising it eleven releases to silence a lint.
`saturating_decrement` in `webseed/bridge.rs` is the compare-exchange loop
`fetch_update` compiles to.

**`#[allow(deprecated)]` was the alternative and is worse.** It silences the
call site for every future rename in the same file, which is precisely the
early warning [T-150](#t-150-clippy-pins-a-floating-toolchain-so-a-rust-release-can-turn-the-tree-red)
was built to get.

**The saturation had no test, and that is what the closure was for.** Every
settle is paired with a receive, so a plain `fetch_sub` is correct in every path
that exists today and wraps to `u64::MAX` the first time one is not. The number
is reported as `in_flight`, so the failure would be a figure a reader believes.
`a_settled_request_never_takes_the_counter_below_zero` holds it now.

**Both toolchains, the way CI runs them.**

```
$ RUSTFLAGS="-D warnings" cargo +beta clippy --workspace --all-targets --all-features
    Finished `dev` profile

$ RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets --all-features
    Finished `dev` profile
```

`rustc 1.99.0-beta.1` and the pinned `1.98.0`. The vendored trees still pass
their own tests, 149 in `rqbit` and 76 in `librqbit-utp`.

### T-219 Ten of the eleven trace subsystems raise a target nothing writes to

Source:      measured while closing [T-094](bench.md), 2026-08-23
Category:    cli
Priority:    P1
Effort:      M
Status:      **done** 2026-08-23T14:10Z

Problem:     `--trace <SUBSYSTEM>` builds one `tracing` directive per name,
             `bit_cli::<subsystem>=trace`, and `logging.rs`'s `SUBSYSTEMS`
             documents eleven of them: `peer`, `handshake`, `tracker`, `dht`,
             `http`, `piece`, `picker`, `disk`, `ratelimit`, `retry`, `config`.

             **Only `http` matches anything.** It is the one target any code in
             this repository names, at
             `crates/bit-cli-core/src/webseed/fetch.rs:1370`. Every other
             directive names a module path that does not exist: a record
             emitted from `cmd/peers.rs` has the target `bit_cli::cmd::peers`,
             which `bit_cli::peer` does not match, and there is no
             `bit_cli::disk`, `bit_cli::piece` or `bit_cli::config` module at
             all.
Relevance:   Eleven names, ten of which do nothing, and all eleven documented
             in `--help`, in all three manuals, and with a sentence each saying
             what they carry. `disk` promises "reads, writes, flushes, and
             allocation, with offsets and sizes". A caller debugging a stalled
             write turns it on, gets nothing, and concludes there were no
             writes.

             It is the same shape as
             [T-181](#t-181-four-flags-are-read-and-never-acted-on),
             [T-183](#t-183---seed-ratio-and---seed-time-are-read-and-never-used),
             [T-185](#t-185---exclude-file-on-its-own-selects-nothing-and-downloads-everything)
             and [T-214](#t-214-seed-runs-no-hooks), and it is the largest of
             them: ten flags' worth of documented capability.
Approach:    The measurement first, because it decides the size. One run of
             `download --web-seed-only` over a 1 GiB payload, tracing all ten:

             ```
             --trace peer,handshake,tracker,dht,piece,picker,disk,ratelimit,retry,config
                 0 lines of stderr
             --trace http
                 257 lines
             ```

             Then, per subsystem, either emit on that target or stop
             documenting it. Emitting is `tracing::trace!(target:
             "bit_cli::<name>", ...)` at the places that already know the
             facts, which is where the work is: `piece` and `picker` are
             decided in the vendored session rather than here, and a target
             this repository controls has to be named from code it owns, so
             some of them need a seam in `vendor/` and are their own work.

             **`ratelimit` and `retry` are the cheap two**: both are decided in
             `bit-cli-core` and neither needs a seam. `disk` is next: every
             write already goes through `SafeStorage::write_through`.
Acceptance:  A test drives one command per documented subsystem with that
             subsystem traced and asserts at least one record on that target,
             and any subsystem that cannot be made to emit is removed from
             `SUBSYSTEMS`, from the help and from the manuals in the same
             change. The list a caller reads matches the list that works.

**Done.** All eleven emit, and the acceptance is
`crates/bit-cli/tests/trace_subsystems.rs`: fifteen cases, one per subsystem
plus four that hold the edges. It drives the **binary** rather than `run`,
because the subscriber is process-global and `logging::install` is
best-effort by design, so an in-process assertion would be reading whichever
test won the race to install one.

**The measurement inverted the fix, and it is the reason this took a day rather
than a week.** The entry says ten subsystems raise a target nothing writes to,
which is true, and it reads as ten subsystems' worth of instrumentation to
write. One `-vvv` run says otherwise: **10,986 records over nineteen targets**,
and nine of the ten subsystems already had their facts on a target `--trace`
did not name.

```
librqbit::peer_connection              4108
librqbit::torrent_state::live          2154
librqbit::file_ops                     2114
librqbit::chunk_tracker                2048
librqbit_dht::dht                       221
bit_cli::http                            32
librqbit_tracker_comms::tracker_comms     1
```

So the fix is two halves rather than one. `SUBSYSTEMS` is a struct now, and a
name carries **the targets it raises** rather than one derived from its
spelling: `bit_cli::<name>` where this repository's own code writes, plus the
vendored target that carries the same fact. `filter_directive` emits one
directive per target and dedupes on the target, so two names sharing one raise
it once.

**Ten subsystems were given somewhere to write and thirteen vendored trace
calls retargeted**, `http` being the one that already had a target of its own.
`disk` in `SafeStorage`'s read, write, flush and allocate, which is where the
offsets and sizes the description promises already are. `ratelimit` in
`RateLimiter::take`, on every take rather than only the ones that wait, because
"the limiter let this through" is half the answer. `retry` in both ladders and
in `SourceStats::record_error`, which is where the budget is spent. `config` in
`Resolved::trace`, once per run and immediately after the subscriber is
installed, which is the one subsystem whose records cannot be written where the
fact is decided: the configuration decides the log level, so it is resolved
while there is still nothing to write to. `tracker` in `announce_on` and
`scrape`, request and response. `peer`,
`handshake` and `piece` in the web seed bridge, which is a real peer as far as
the session is concerned. `picker` in `InOrder::advance`. `dht` in `Engine`,
once per session, because it is the one fact the vendored crate cannot carry:
with the DHT off it writes nothing, and no records and the flag does nothing
look the same from outside.

**The vendored half is a patch and it is not cosmetic.** A `tracing` target
defaults to the module path and the modules do not divide the way the
subsystems do: `peer_connection` holds the handshake and every wire message,
and `torrent_state::live` holds the picker, the piece lifecycle and peer
management. Raising the module would have made `--trace handshake` print 266
records where 2 were asked for, on the 2 MiB fixture below. Thirteen calls
take an explicit target instead, under "handshake, piece and picker tracing have no
target of their own" in
[`patches/UPSTREAM.md`](../patches/UPSTREAM.md). Upstream's own tests were run
and are 149 passing, unchanged.

**The acceptance run.** `scripts/check-trace.ps1` takes the numbers again and
fails when a subsystem writes on none of the targets it raises. One 2 MiB
fixture, one run per name:

```
peer        399 records   bit_cli::peer=133     librqbit::peer_connection=266
handshake     5           bit_cli::handshake=3  librqbit::handshake=2
tracker       2           bit_cli::tracker=2
dht           1           bit_cli::dht=1
http          1           bit_cli::http=1
piece       281           bit_cli::piece=128    librqbit::piece=153
picker       10           bit_cli::picker=1     librqbit::picker=9
disk         82           bit_cli::disk=82
ratelimit     1           bit_cli::ratelimit=1
retry         6           bit_cli::retry=6
config        3           bit_cli::config=3
```

And the headline, on the same fixture: the ten names the entry was filed about
now write **743** lines of stderr where they wrote **0**, and an untraced run
still writes none, which is the other half of the promise.

```bash
pwsh -NoProfile -File scripts/check-trace.ps1 -Json bench/trace.json
```

The evidence is `bench/trace-subsystems-20260823T140418847Z.json`.

**Run against the defect.** With `bit_cli::disk` renamed to `bit_cli::disk_gone`
at all four storage call sites, the `disk` case fails with the message the
caller would have needed: `--trace disk raises ["bit_cli::disk"] and nothing
wrote to any of them. Targets seen on this run: {"bit_cli::disk_gone"}`. That
is exactly the state the whole surface was in.

**One target is asserted by measurement rather than by the test**, and the test
says so: `librqbit_dht`. Every trace in the vendored DHT crate is on a query, a
response or a routing table change, and all three need the public DHT, so a
test that asserted one would be asserting that a CI runner can reach the
internet. The `-vvv` run above is the evidence: 221 records.

[`docs/trace.md`](../docs/trace.md) is what a caller reads: what each name
shows, a command that puts it in the path so silence can be told apart from a
defect, the targets each raises, and the three things adding one needs.

**What it cost to find, and what it did not.** Nothing here needed a corpus and
nothing needed a design. It needed one command: a run with `-vvv` and
`--log-format json`, counting the `target` field. That is the same rule
[T-184](disk-io.md) and [T-172](metainfo.md) wrote down.


### T-220 The record gate reported on a tree the same run then rewrote

Source:      CI run 32637486414, `Record`, 2026-08-23
Category:    ci
Priority:    P2
Effort:      S
Status:      **done** 2026-08-23T12:05Z

Problem:     `gates.ps1` ran the `record` gate **before** `man` and `fmt`, and
             both of those rewrite files under `-Fix`. One of the things
             `check-todo.ps1` checks is that a `TODO/` citation names the line
             its symbol is actually on, so a formatting pass that adds lines to
             a cited file invalidates a check that has already reported `ok`.
Relevance:   Measured rather than supposed: `pwsh -File scripts/gates.ps1 -Fix`
             printed `record ok` and `all gates pass`, the push that followed
             went red on `Record`, and the reason was a citation into
             `schema_gen.rs` that `cargo fmt --all` had moved by ten lines
             **in the same run that had just approved it**.

             That is the same shape as the `check-man.ps1 -Fix` defect the
             session of 2026-08-23 found earlier: a gate that reports on a tree
             the run then changes reads as the gate being wrong, and the next
             person debugs the check rather than the file.
Approach:    Order it. The `record` block moves below `man` and `fmt`, so
             everything that rewrites has rewritten before anything checks a
             line number. Nothing else in the file depends on the order: each
             gate reports independently and the summary is printed at the end.
Acceptance:  `gates.ps1 -Fix` on a tree where formatting moves a cited line
             fails rather than passing, and the ordering is written where the
             gate is.

**Done.** The gate is at `scripts/gates.ps1:269` now, after both. The comment
above it says why, because the ordering looks arbitrary and is not.

**Proved by the case that produced it.** With the record gate first, the run
that closed [T-191](bench.md) printed `record ok` and CI then failed on
`cli-surface.md:1550 cites schema_gen.rs:1286 ... which is at :1296`. With the
gate moved, the same tree fails locally, on the same line, before a commit
exists.

### T-222 A config file reaches `config show` and nothing else

Source:      measured while closing T-219, 2026-08-23
Category:    cli
Priority:    P1
Effort:      M
Status:      **done** 2026-08-23T15:20Z

Problem:     `--config <PATH>` and `--no-config` are **global** flags, accepted
             on every command. They have two readers in the whole workspace,
             `crates/bit-cli/src/cmd/config.rs:75` and `:101`, and both are
             inside `resolve`, whose only caller is `bit-cli config show` at
             `crates/bit-cli/src/cmd/config.rs:146`. The three line numbers are
             where they are **after** this entry closed and the file grew; the
             readers themselves are `global.no_config` and `global.config`.

             So `bit-cli.toml`, the user config file, `--config`, and every
             `BIT_CLI_*` variable change what `bit-cli config show` prints and
             change nothing about what any other command does.
Relevance:   `README.md` documents the whole precedence chain as the tool's
             configuration, six layers, highest wins. `docs/trace.md` now
             documents `--trace config` as the resolution of every
             configuration value and its origin, which is honest only because
             the resolution happens in one command.

             It is the fifth entry of the flag-does-nothing shape, after
             [T-181](#t-181-four-flags-are-accepted-in-silence-and-reach-no-code),
             T-183, T-185 and T-219, and it is the one with the largest
             documented surface behind it: **22** settings in
             `config::SETTINGS`, each with a default and a description, none of
             which any run reads.

             Neither the audit that found T-181's four flags nor the `clap`
             tree test would see it, and this is a sixth distinct shape: the
             field has a reader, the reader succeeds, and the value it produces
             is read by `config show` alone, out of sixteen top-level
             commands.
Approach:    Measured before anything is designed, and the measurement is in
             the record below rather than in a paragraph.

             The work is to call `cmd::config::resolve` once in `run`, before
             `dispatch`, and to have the flag defaults come from it. That is a
             decision per setting rather than one change: `download_directory`
             is `--dir`'s default, `max_peers` is `--max-peers`'s, and a flag
             that was given on the command line has to keep winning, which is
             what `Origin::rank` already encodes and `clap` does not know
             about. `clap`'s `ArgMatches::value_source` says whether a value
             came from the command line or from a default, which is the seam.

             Two smaller things fall out and should not be separated from it.
             `--config` naming a file that does not exist is an error on
             `config show` and accepted in silence on every other command, so
             the same flag with the same value has two behaviours. And
             `--no-config` on any other command is a no-op, so a caller
             protecting a CI run from a stray `bit-cli.toml` is protected by
             the defect rather than by the flag.
Acceptance:  A test per layer, driving a command that is not `config show`: a
             `bit-cli.toml` naming a `download_directory` puts the payload
             there, a `BIT_CLI_*` variable does the same, `--config` beats
             both, a command-line flag beats all three, and `--no-config`
             turns the files off. Plus: `--config` naming a missing file is the
             same exit code on every command.

**Measured, and it is where the entry came from.** A `bit-cli.toml` naming a
`download_directory` and `max_peers = 7`, in the working directory:

```
bit-cli config show      download_directory = <configured>   (project_config)
                         max_peers          = 7              (project_config)
                         files_read         = .../work/bit-cli.toml

bit-cli download ...     exit 0, completed
                         payload in the configured directory? False
                         payload in the working directory?    True
```

And the same flag, two behaviours:

```
bit-cli --config <missing> info      exit 0
bit-cli --config <missing> download  exit 0
bit-cli --config <missing> config show
    error: cannot read ...: The system cannot find the file specified.
    exit 8
```

**Done.** Every layer reaches every command. A setting is the **default** of
the flag it names, so a flag on the command line still wins and nothing in this
repository decides precedence.

**The Approach above named the wrong seam, and the work found a better one.**
It proposed reading `clap`'s `ArgMatches::value_source` to tell a value that
came from the command line from one that came from a default, then overwriting
the field. That works, and it needs a branch per setting spread over `Global`,
`LimitArgs`, `WebSeedArgs` and five command structs, with nothing checking that
a new flag reached it.

Setting `Arg::default_value` instead moves the whole question back into `clap`,
which already knows a supplied value beats a default. So the resolution becomes
a list of `(long flag, value)`, the command tree is walked once with
`mut_args` and `mut_subcommand`, and the tree is parsed a second time.
Precedence is not implemented anywhere: it falls out. The mapping table is then
the only thing anybody has to keep true, and two tests keep it true.

`crates/bit-cli/src/config_defaults.rs` is the module and it carries the
argument in full.

**The second parse is skipped when nothing configured anything**, which is
every run with no config file and no `BIT_CLI_*` variable. A value whose origin
is a **flag** is never handed back as a default, because it is already on the
command line.

**Three things fell out and none could be separated from it.**

**`--config` on a missing file now fails the same way everywhere.** It was
exit 8 on `config show` and exit 0, in silence, on the other fifteen commands:
the same flag with the same value, two behaviours. The resolution happens in
`run` now, so there is one.

**`user_config_path` takes the environment instead of reading the process.**
This is the one that would have been a defect rather than a limitation.
Configuration decides what a run does now, so a test that resolved it against
the real process environment would read whatever config file the machine
happens to have, and pass or fail on that. `Env` already carries the variables
and `Env::test` carries none, so a test sees no user config unless it puts one
there.

**A `BIT_CLI_*` variable this program sets itself is no longer refused as a
typo, and this was found by running rather than by reading.** `apply_env`
refuses an unknown `BIT_CLI_*` name, which is right, and it used to run on one
command. Making it run on every command made **every run under `cargo test`
fail**:

```
create failed: `BIT_CLI_TARGET` is not a setting; run `bit-cli config show` for the list
```

`BIT_CLI_TARGET` is set by this repository's own build script and is in the
environment of anything `cargo` runs. The larger case is the one the test
suite did not reach: the **twenty** variables a hook receives, which
`hooks::VARIABLES` lists, are set by `bit-cli` itself, so a hook whose command
is `bit-cli` would have had the child refuse its parent's variables. `resolve`
passes a reserved list now, and it is derived from `hooks::VARIABLES` rather
than written twice.

**The acceptance, and every case drives a command that is not `config show`.**
`download_directory` is the setting under test in most of them because its
effect is a file on the disk rather than a number in a report:

| case | what it asserts |
| --- | --- |
| `a_project_config_decides_where_a_download_lands` | the payload is in the configured directory and not in the working one |
| `an_environment_variable_decides_it_too` | `BIT_CLI_DOWNLOAD_DIRECTORY` does the same |
| `an_explicit_config_beats_the_project_one_in_a_run` | `--config` wins, and the project file's directory is empty |
| `a_flag_beats_every_layer_in_a_run` | `--dir` beats a file and a variable together |
| `no_config_turns_the_files_off_for_a_run` | the payload lands in the working directory |
| `a_missing_explicit_config_fails_the_same_way_on_every_command` | exit 8 on `config show`, `version` and `info` |
| `a_variable_this_program_sets_itself_does_not_fail_a_run` | `BIT_CLI_HOOK`, `BIT_CLI_TARGET` and `BIT_CLI_INFO_HASH` pass, and a real typo still fails |

```bash
cargo test -p bit-cli --lib cmd::config
```

```bash
cargo test -p bit-cli --lib config_defaults
```

**What a configured boolean cannot do, and it is written down rather than
hidden.** Three settings are `enable_*` and the flags are `--no-*`, so the
value is inverted on the way in. `enable_dht = false` makes `--no-dht` default
to true and there is no `--dht` to turn it back on for one run. `--no-config`
is the escape hatch. Adding three negations is a bigger change to the surface
than this entry is, and nothing has asked for one.

**What is not covered.** A configured value that `clap` cannot parse is
reported against the flag rather than against the file it came from: the
message names `--max-peers` and not `bit-cli.toml`. `--trace config` shows the
origin of every value, which is the answer, but the error itself does not carry
it.

### T-226 `download --out` is parsed and never read

Source:      measured while opening T-103, 2026-08-23
Category:    cli
Priority:    P1
Effort:      S
Status:      **done** 2026-08-23T17:40Z

Problem:     `bit-cli download -o/--out <PATH>` is declared at
             `crates/bit-cli/src/cli.rs:934` as
             `SelectionArgs::out: Option<PathBuf>` and **nothing in the
             workspace reads it**. A run passes it and the payload lands where
             it would have landed anyway: under the download directory, in a
             directory named after the torrent.
Relevance:   It is the sixth entry of the flag-does-nothing shape, after
             [T-181](#t-181-four-flags-are-accepted-in-silence-and-reach-no-code),
             T-183, T-185, [T-219](#t-219-ten-of-the-eleven-trace-subsystems-raise-a-target-nothing-writes-to)
             and [T-222](#t-222-a-config-file-reaches-config-show-and-nothing-else),
             and it is the plainest of the six: the field has no reader at
             all, where T-222's had one and T-219's names raised a target.

             `man/bit-cli.json` documents it as "Write the payload here
             instead of using the torrent's name", so a caller reading the
             manual and passing `-o` gets a successful run that wrote
             somewhere else. There is no warning and the exit code is 0.
Approach:    The machinery is already there and `seed` already uses it.
             `AddOptions::output_folder` at
             `crates/bit-cli-core/src/engine.rs:179` is the per-add override,
             and `add_inner` at `:727` turns `Some(folder)` into exactly that
             directory with `subfolder: false`, which is what stops the
             torrent's name being appended. So:

             - **multi-file**: `output_folder = PATH`, and every file lands
               under `PATH` rather than under `<dir>/<name>`.
             - **single-file**: the payload is one file, so `PATH` names the
               file. That is `output_folder = PATH.parent()` plus the
               `index_out` override for index 0 set to `PATH.file_name()`,
               both of which `AddOptions` already carries.

             Two things have to be decided rather than assumed, and neither is
             covered by the existing flags:

             - **`--out` with more than one source.** `SelectionArgs` applies
               to every source in the run, so two torrents would be told to
               write to the same path. Refuse it as a usage error before the
               session starts, the way `plan_selection` refuses an index past
               the end at `crates/bit-cli/src/cmd/download.rs:2665`.
             - **`--out` beside `--dir`.** `--dir` is the run's output
               directory and `--out` is one payload's destination. Resolving
               `--out` against `--dir` keeps both meaningful and makes
               neither silently inert.
Acceptance:  `download --out` writes the payload at the named path for a
             single-file torrent and for a multi-file one, `--out` with two
             sources is a usage error before anything is added, and the
             report's `output_directory` says where it actually went.

**Measured, 2026-08-23, before anything was written.** Two runs against a
loopback file server, `--web-seed-only`, from a release build of `d3bc6a5`.

| torrent | flag | `output_directory` in the report | where the payload landed |
| --- | --- | --- | --- |
| single-file `single.bin` | `--out .tmp/t103/renamed.bin` | the working directory | `single.bin` in the working directory |
| multi-file `plain/inner.bin` | `--out .tmp/t103/o1` | the working directory | `plain/inner.bin` in the working directory |
| single-file `single.bin` | `--dir .tmp/t103/o2` | `.tmp/t103/o2` | `.tmp/t103/o2/single.bin` |

`--dir` is in the third row to show the difference is `--out`'s and not the
harness's.

**Run against the defect, which is what proves there is no reader rather than
that one grep missed it.** The field was renamed to `out_probe_unread`, with
`long = "out"` kept so the surface did not move, and
`cargo check --workspace --all-targets` finished clean. A field nothing names
cannot be being read.

```bash
cargo check --workspace --all-targets
```

**Done 2026-08-23T17:40Z, and the Approach held.** The machinery was already
there and `seed` was already using it, which is what made this effort S: the
change is where `--out` is resolved and what it is turned into, not new
plumbing.

**Multi-file**: `AddOptions::output_folder = PATH`. `add_inner` passes
`subfolder: false` for a set `output_folder`, which is exactly what stops the
torrent's own name being appended, so the files land directly under `PATH`.

**Single-file**: the payload is one file, so `PATH` names it.
`output_folder = PATH.parent()` and `index_out[0] = PATH.file_name()`. The
leaf goes through the `-O` machinery rather than around it, so it is
sanitised, truncated and disambiguated exactly as a torrent path is.

**A magnet resolves its metadata first**, on the branch `-O` already uses.
`--out` needs a different fact than the count, whether the torrent is
single-file, and it is the same round trip, so `plan_selection` returns
`AwaitingCount` for `--out` too and `run_one` reads
`resolved.layout.multi_file`. `Plan` carries `multi_file: Option<bool>` for
every other source kind, and it is not derived from the file count because a
multi-file torrent holding one file is still multi-file, which is
[T-036](performance.md).

### Three things the entry did not name, and two were found by running

**`--out` beside `--dir` was resolved the wrong way round, and the first
version escaped the output directory.** The entry proposed resolving `--out`
against `--dir`, which is right, and the first attempt wrote
`directory.join(env.resolve(path))`. `env.resolve` makes a relative path
absolute against the working directory, and joining an absolute path onto
another returns the absolute one, so `--dir out --out album` wrote to
`<cwd>/album` and `--dir out --out ../../x` wrote **two levels above the
repository**, which a run confirmed by leaving a file there. Relative paths
join onto `--dir`; an absolute `--out` is honoured, which is what `-o` means
everywhere else and what `--dir` is already allowed to do.

**The report named the run's directory rather than the torrent's.**
`output_directory` was `options.directory` at both sites, which stops being
this torrent's the moment `--out` is given, so the Acceptance's last clause
failed on the first run that otherwise worked. `finish` takes the payload
directory now and lost the `&Options` it read one field of.

**A `..` survived into that report**, because `std::fs::canonicalize` needs
every component to exist and returns an extended-length prefix on Windows that
no caller wants to read. `normalise` resolves `.` and `..` lexically instead.

### Measured, and run against the defect

Three runs against `loopback-fileserver` with `--web-seed-only`, and the
fourth is the refusal:

| torrent | flag | where the payload landed | `output_directory` |
| --- | --- | --- | --- |
| single-file | `--out .tmp/t226/renamed.bin` | `.tmp/t226/renamed.bin` | `.tmp\t226` |
| multi-file | `--out .tmp/t226/mydir` | `.tmp/t226/mydir/inner.bin` | `.tmp\t226\mydir` |
| multi-file | `--dir .tmp/t226b --out under` | `.tmp/t226b/under/inner.bin` | `.tmp\t226b\under` |
| two sources | `--out .tmp/t226/x` | nothing, exit 2 | |

Four acceptance cases in `crates/bit-cli/src/cmd/download.rs`. Each asserts
the bytes at the named path **and** that nothing landed under the torrent's
own name, because a test that only checks the first passes on a run that
ignored the flag. With the application disabled, three of the four fail and
the usage-error case still passes, which is right: it is a different code
path.

```bash
cargo test -p bit-cli --lib out_writes_a_multi_file out_names_the_file a_relative_out_resolves out_with_more_than_one
```

### The operator ruled on 2026-08-24: `--out` may leave the output directory

The question this entry raised was whether `--out ../../x` beside `--dir out`
should be allowed to write above the working directory. It is allowed, and it
stays allowed.

The argument is the one the question already carried. `--out` is the caller's
own path, typed on their own command line, and `--dir` is allowed anywhere
already. The neighbour it reads inconsistently against is `-O`/`--index-out`,
which is sanitised, and the difference is that `-O`'s path is a file **inside**
the output directory while `--out` names the destination itself.

`out_may_leave_the_output_directory_because_it_is_the_callers_path` pins it:
`--dir <tmp>/base --out ../beside` lands the payload at `<tmp>/beside`, asserts
nothing landed under `base`, and asserts the report's `output_directory` names
the resolved path rather than one carrying a `..`. Tightening this later is now
a decision somebody makes against a passing test.

```bash
cargo test -p bit-cli --lib out_may_leave_the_output_directory
```

### T-228 Two gate runs at once fail on a locked file rather than on being two

Source:      hit while closing T-041, 2026-08-23
Category:    ci
Priority:    P3
Effort:      S
Status:      **done** 2026-08-24

Problem:     `scripts/gates.ps1:330` tees `cargo test` into
             `$env:TEMP\bit-cli-gates-tests.txt`, one fixed path for every run
             on the machine. A second `gates.ps1` started while the first is
             still in its `test` gate dies on it:

             ```
             out-file: The process cannot access the file
             'C:\Users\...\Temp\bit-cli-gates-tests.txt' because it is being
             used by another process.
             ```
Relevance:   It is [T-225](create-seed.md)'s shape in a different script: the
             run failed for a reason nobody would guess from what it printed.
             Nothing in that message says "another gates run is going", so the
             next session debugs `Out-File`, and the fixed path is not
             something a reader of the failure can see.

             It cost this session about two minutes, which is why it is P3
             rather than higher. What makes it worth recording is that a
             session **does** start a second run: this one had one in the
             background and ran another in the foreground, which is the
             ordinary way an agent works, and the collision is silent until
             the `test` gate.
Approach:    Put the process id in the name, which is one line, and delete the
             file at the end of the run rather than leaving it. That is enough
             on its own. A lock file that reported "another gates run is in the
             `test` gate, started at <instant>" would be better and is more
             than a P3 is worth: with per-run paths the two runs simply both
             work, which is the outcome anybody wanted.

             The same fixed-path question applies to any other
             `$env:TEMP\bit-cli-*` in `scripts/`; check them in the same pass
             rather than fixing one.
Acceptance:  Two `gates.ps1` runs started within a second of each other both
             reach a verdict, and neither leaves a file in `$env:TEMP` behind.

#### Done 2026-08-24

**Three fixed paths, and the third was not in the entry.** `$PID` goes in the
name of `gates.ps1`'s clippy log and test log, and of `git-sync.ps1`'s test log
at `scripts/git-sync.ps1:392`, which has the same defect for the same reason
and which two pushes at once would hit. Every other `GetTempPath` in `scripts/`
was checked in the same pass and none needed changing: `check-man.ps1` uses
`Get-Random`, `vendor-sync.ps1` a GUID, `git-sync.ps1`'s other two already use
`$PID`, and `setup-nasm.ps1`'s is a download cache that is meant to be shared.

`$PID` rather than a random suffix, so a log left behind by a run that was
killed can still be tied to the process that wrote it.

**A passing run deletes both logs and a failing one keeps them**, because the
detail line points a reader at them by path and a message naming a file that is
gone is worse than no message.

**Measured, and the acceptance is the positive case**: two `-Fast` runs started
within a second of each other, `1298 tests` each, both `all gates pass`, and
zero `bit-cli-gates-*` files left in `$env:TEMP` afterwards.

**Run against the defect**, with the fixed names restored in a copy under
`scripts/` so `$PSScriptRoot` still resolves:

```
A: The process cannot access the file
   'C:\Users\...\Temp\bit-cli-gates-clippy.txt' because it is
   being used by another process.
A exit=1
B: all gates pass: 1298 tests, 89.8s
B exit=0
```

That is the entry's own symptom, one run dying on a locked file it does not
name a reason for, and the other finishing normally beside it. The first
attempt at this reproduction put the modified copy in `$env:TEMP` and every
gate failed, because `gates.ps1` derives the repository from `$PSScriptRoot`
and was therefore running `cargo` outside the tree. That result was discarded
rather than written down.

```bash
pwsh -NoProfile -File scripts/gates.ps1 -Fast
```

### T-230 A run's output reached the remote because nothing said what belongs here

Source:      the operator, 2026-08-24, on finding `under/inner.bin` on the
             remote
Category:    ci
Priority:    P1
Effort:      S
Status:      **done** 2026-08-24

Problem:     `under/inner.bin`, 1,000 bytes of `0x41`, was tracked on `main`
             from commit `2d369db` and pushed. Nothing in this repository
             wanted it and no session mentioned it.

             It came from [T-226](#t-226-download---out-is-parsed-and-never-read)'s
             own acceptance table, third row,
             `--dir .tmp/t226b --out under`. That row exists to demonstrate
             the resolution T-226 fixed: `directory.join(env.resolve(path))`
             made a relative `--out` absolute against the working directory
             first, so joining returned the absolute path and the payload
             landed at `<repo>/under/inner.bin` rather than under `.tmp/`. The
             fix is in the same commit that carries the file, which is why
             nothing looked wrong afterwards.

             **The defect that wrote it is not the reason it reached the
             remote.** Three things had to be true at once and all three were
             general:

             - `scripts/git-sync.ps1` stages with `git add -A`, which is
               right: a session that stages by hand forgets the record file.
             - `.gitignore` covered `*.iso`, `*.img`, `*.qcow2` and `*.part`
               and not `*.bin`, so nothing held it back.
             - **Nothing anywhere compared the result against what this
               repository is supposed to contain.** `gates.ps1`'s `text` gate
               reads six extensions and cannot see a seventh. `check-todo.ps1`
               reads the record. No check had an opinion about a new top level
               directory full of payload.
Relevance:   Any run that writes into the working tree gets the same ride, and
             a session runs dozens. The `--out` escape is now allowed on the
             operator's ruling, which makes it likelier rather than less.
Approach:    Say what belongs here, mechanically, and check it where a commit
             is made.
Acceptance:  The path is gone from the history on the remote, a payload of
             that shape cannot be staged, and a check refuses a tracked path
             this repository does not account for. Run against the defect on
             both of its rules.

#### Done 2026-08-24, and the check found a second defect on the day it was written

**The history was rewritten.** Eight commits carried the blob, `2d369db`
through `bd99f35`. `git filter-branch --index-filter` over `2d369db~1..HEAD`
removed the path from each, author and committer identity and dates
unchanged, 198 commits before and 198 after, and the only difference between
the old tip and the new one is that one file:

```bash
git diff --stat backup/pre-under-removal-20260824 main
```

The rewritten branch was force pushed with `--force-with-lease` pinned to the
old tip. `git-sync.ps1` was not used and could not be: it commits and pushes
work, and this is neither. Every push after it is `git-sync`'s again.
`gh api repos/Azathothas/bit-cli/contents/under` answers 404.

**`scripts/check-tree.ps1` is the check**, and it has two rules because either
one alone would have stopped this file:

| rule | what it says | what it catches |
| --- | --- | --- |
| `top-level` | the first path component is one of a fixed set | `under/`, a new top level directory |
| `kind` | outside `vendor/`, the name is a known extension or a known exact name | `inner.bin`, wherever it lands |

Both lists are measured rather than imagined: they are what `git ls-files`
holds today. `vendor/` is exempt from the second rule and only the second,
because upstream's fixtures legitimately include `.bin`, `.torrent`, `.png`
and `.svg`, and a reconciliation that had to declare each one here is a
reconciliation nobody runs.

**It reads the index rather than the working tree**, which is what lets one
script answer two questions. In `gates.ps1` and in CI the index is HEAD, so it
answers "what is in this tree". In `git-sync.ps1` it runs after `git add -A`
and before the commit, when the index is exactly what the commit will contain,
so it answers "what is about to go in". There is no second mode to keep
honest.

Three places, because the file got in through the gap between them:

- `gates.ps1`, as the `tree` gate, beside `record`.
- `.github/workflows/ci.yml`, as the `Tree` job. This is the copy
  `git-sync.ps1 -SkipGates` cannot skip.
- `git-sync.ps1`, after staging. Not behind `-SkipGates`: that switch is for a
  documentation push on a tree known green, which is likelier to carry a stray
  payload than one that ran the gates. It resets the index and commits nothing.

`.gitignore` gained `*.bin` and `*.torrent`, with `!/vendor/**/*.bin` and
`!/vendor/**/*.torrent` beside them so a reconciliation is not silently
swallowed, which `scripts/vendor-sync.ps1` refuses to finish over anyway.

**Run against the defect, on both rules.** `under/inner.bin` recreated and
force-added is refused by `top-level`; `crates/payload.dat` is refused by
`kind`; and with `.gitignore` in place `git add -A` does not stage the payload
at all, so the guard is reached only when somebody insists.

```bash
pwsh -NoProfile -File scripts/check-tree.ps1
```

**And it failed the first time it was run, on a file nobody was looking for.**
`bench/soak-20260821T012428252Z.csv`, committed evidence, ended in 176 NUL
bytes. That is [T-231](memory.md), and it is the more interesting of the two.

### T-244 A web page is not a source, and nothing extracts a link from one

Source:      the operator's brief of 2026-08-24, measured the same day
Category:    cli
Priority:    P2
Effort:      L
Status:      **done**, 2026-08-29: both tiers ship, the client's JA4 is
             Chrome's own, and staleness is detected and recommended against
             with proof

Problem:     `source.rs:93` maps an `http://` or `https://` string to
             `Kind::Url`, documented at `source.rs:47` as "an HTTP(S) URL
             pointing at a `.torrent`". A page that *links* to a torrent is
             classified the same way, fetched, and handed to the bencode
             parser, which fails on the first byte of the markup.

             There is no HTML parser in the tree and no link extraction
             anywhere.
Relevance:   A URL naming a page is how a person meets a torrent almost every
             time. Naming the `.torrent` itself is the exception, and it is the
             only case that works.

             It is also the input a script cannot pre-resolve: an indexer that
             builds its links in script has no stable `.torrent` URL to write
             into a config file.
Approach:    Ruled on by the operator on 2026-08-24: **static extraction, with
             a browser opt-in.**

             Static first, and not naively. The page is fetched with a header
             set and a TLS fingerprint that match a current browser, because an
             origin that fingerprints the client sends a different page to a
             client it does not recognise, and a scraper reading that page is
             reading a page nobody else gets.

             **`wreq` is the crate, Apache-2.0, and its two predecessors are
             traps.** Read from crates.io on 2026-08-24: `wreq` has 36
             published versions, 30 of them live, 1,814,353 downloads, a newest
             stable of 0.16.0 and a newest published version of `6.0.0-rc.31`.
             `rquest` has 152 versions and **every one of them is yanked**;
             `reqwest-impersonate` has 62 and the same. Either name still comes
             up first in a search, and neither is installable.

             **The cost is the part to weigh, and it is not the crate count.**
             `wreq` 0.16.0 has 21 required direct dependencies and 59 in total,
             and two of them are `btls` and `btls-sys`: it brings BoringSSL.
             This tree already carries `rustls`. Two TLS stacks in one binary is a
             larger change than any crate count says, and the alternative,
             ordering a `rustls` client hello by hand, does not reach the
             HTTP/2 settings fingerprint that a modern origin also reads.

             Then extraction: every `href` ending `.torrent`, every `magnet:`
             URI, and the anchor text beside each so a chooser has something to
             show. Several matches is the normal case, so a page that yields
             more than one is reported and refused rather than guessed at,
             unless a selector says which.

             `--render` is the opt-in second half: drive a Chrome or Edge that
             is **already installed** over the DevTools protocol, take the DOM
             after script has run, and extract from that. Off by default, never
             bundled, and absent gracefully when no browser is found.

             A CAPTCHA or a bot check is a refusal with the status named, not a
             thing to defeat.
Acceptance:  A page served by `loopback-fileserver` carrying one `.torrent`
             link and one magnet link resolves under `bit-cli info`, and a page
             carrying two of each exits non-zero naming both. The header set
             and the TLS fingerprint are asserted against a recorded capture
             rather than eyeballed.
Notes:       [T-245](#t-245-four-commands-refuse-the-url-download-accepts) was
             the prerequisite, and it closed on 2026-08-24: a plain `.torrent`
             URL resolves under `info` now, so a page is the remaining step
             rather than the second of two.

Correction:  **Two citations in the Problem were eight lines stale and are
             fixed above, and then moved again inside this session.**
             `source.rs:68` and `source.rs:32` became `:86` and `:40` on
             2026-08-25 when T-241 added to that file's module header, and
             `:93` and `:47` on 2026-08-29 when this entry's own work added
             seven more lines to it.

             `check-todo.ps1` catches neither, and the reason is worth
             recording: it checks a short-form citation's line only when the
             prose names a symbol occurring **exactly once** in the file, and
             `Kind::Url` occurs four times in `source.rs`. **Five more
             citations into the same file were stale by 14 to 29 lines** and
             none of them was this session's doing; every one was read and
             corrected against the line it names, which is what review 1 is
             for and is the only thing that catches this class.

             **And the Problem's sentence was imprecise in a way that mattered
             to the design.** Line 86 does not map every `http(s)` string to
             `Kind::Url`: it reads the extension off the URL's **path** and
             returns `Kind::MetalinkUrl` for a metalink, `Kind::Url` otherwise.
             A page arrives as `Kind::Url` because it is not a metalink, not
             because nothing looks. That branch is what page detection extends.

Correction:  **An off-host link is a match, and the first design said it was a
             decoy.** Restricting matches to the document's own host was
             written into the proving ground's L3 level as a decoy and is
             wrong. Measured 2026-08-29 by `scripts/check-page-fetch.ps1`:
             `kali.org` serves its download page from `www.kali.org` and every
             one of the **113** torrent links on it sits on
             `cdimage.kali.org`. A same-host rule returns nothing there. The
             host is reported per link instead, so a caller can see it.

Correction:  **An unquoted `href` is not exotic and the first count missed
             113 links because of it.** `check-page-fetch.ps1`'s first run
             counted torrent links with a quoted-only pattern and reported
             **0** for `kali.org`. Kali serves minified HTML and writes every
             link as `href=https://...iso.torrent>torrent`. All three HTML5
             attribute framings are read now, and the same page reports 113.

Measured:    **How much impersonation the static half needs, over a named
             set.** The premise the whole Approach rests on had never been
             tested, and `RESEARCH.md` review 5.5 put it first: fetch the pages
             `bit-cli` has to read with a plain client and count the failures.

             `scripts/check-page-fetch.ps1` is that measurement, committed as
             `bench/page-fetch-20260829.json`. Fifteen pages in three groups,
             all named in the script: the three mirrors RULES.md section 5
             permits, nine distribution download pages, and three public
             indexes that publish a page with no account. One `GET` per page,
             `robots.txt` fetched and honoured per host, `bit-cli/0.2.0` as the
             User-Agent, everything under `.tmp/`.

             | verdict | pages |
             | --- | --- |
             | served | **15** |
             | bot check | 0 |
             | refused | 0 |
             | error | 0 |

             **That number does not retire the impersonating tier and the
             operator ruled that it must not.** Fifteen friendly distribution
             pages are not the population `bit-cli` meets, the measurement is
             one client from one address on one day, and two of the fifteen sit
             behind Cloudflare already. The ruling of 2026-08-29 is to build
             the browser-shaped fetch rather than carry it as a contingency.

Measured:    **What this client actually looks like on the wire, and it looks
             like nothing else.** Captured 2026-08-29 with the oracle this
             session put in the tree, `bit-cli` against
             `loopback-tlsprobe --raw`:

             ```
             t13i1010h1_61a7ad8aa9b6_3fcd1a44f3e3
             ```

             Read it: TLS 1.3, no SNI because the probe is an IP literal, **10
             ciphers, 10 extensions, and ALPN offering `http/1.1`**. No GREASE,
             no ECH, no ALPS, no certificate compression, no post-quantum key
             share. A current Chrome offers 15 ciphers, 15 or 16 extensions and
             `h2`. The `a` segment alone separates the two before any hash is
             compared.

Measured:    **Whether `impit` can enter this tree at all**, which the survey
             could not know because it never read this tree. Five questions,
             four answered here and one left to CI, all on 2026-08-29,
             x86_64 Windows, with the commands recorded.

             | question | answer |
             | --- | --- |
             | MSRV 1.88 (`cargo +1.88 check --locked`) | **passes**, 289 packages |
             | `x86_64-pc-windows-msvc` with `+crt-static` | **builds**, 8.59 MiB |
             | `scripts/check-static.ps1` on that binary | **passes**, no VCRUNTIME or UCRT import |
             | apify's `rustls` fork at the workspace root | **the whole workspace checks**, vendored `librqbit` included, on one `rustls 0.23.43` |
             | `x86_64-` and `aarch64-unknown-linux-musl` | **not measured here**, no musl cross toolchain on this machine; CI's `Build` matrix is the instrument |

             The MSRV answer is the one that mattered most, because a bump
             would have been a decision above a `TODO/` item. There is none:
             the graph resolves and checks on 1.88.

             **`impit`'s own fingerprint reproduces the survey's claim, on a
             platform the survey never tested.** Captured the same way:

             ```
             t13i1515h2_8daaf6152771_806a8c22fdea
             ```

             `8daaf6152771` is the published Chrome JA4 cipher hash, and the
             extension list carries ALPS `0x44cd`, ECH `0xfe0d`, certificate
             compression `0x001b` and the ML-DSA signature algorithms
             `0x0904/5/6`.

             **The survey's `h2` defect reproduces here too, twice.** `cargo`
             prints, in the probe and again with the patch at this tree's own
             workspace root:

             ```
             warning: patch `h2 v0.4.7 (https://github.com/apify/h2?rev=7f393a72...)` was not used in the crate graph
             ```

             apify's fork is `0.4.7`, `reqwest 0.13` resolves `0.4.19`, and a
             `[patch]` that cannot satisfy the requirement is declined with a
             **warning rather than an error**. Stock `h2` then encodes the
             HEADERS frame with its own fixed pseudo-header order.

Measured:    **How far the fingerprint moves without vendoring anything, and
             exactly where it stops.** The header set is half of what an origin
             reads and it costs nothing; the `ClientHello` is the other half
             and it is `rustls`'s to decide.

             `--page-client browser`, the default, sends a current Chrome's
             header set. `reqwest` gained `http2` so ALPN offers `h2`, and the
             four decompressions so the `Accept-Encoding` that implies is one
             this client can decode. Captured either side of that change:

             | | JA4 |
             | --- | --- |
             | before | `t13i1010h1_61a7ad8aa9b6_3fcd1a44f3e3` |
             | after | `t13i1010h2_61a7ad8aa9b6_3fcd1a44f3e3` |
             | Chrome, for comparison | `t13i1515h2_8daaf6152771_806a8c22fdea` |

             **One character moved and that is the honest summary.** The `a`
             segment's ALPN marker is `h2` now rather than `h1`. The cipher and
             extension hashes did not move and will not: ten ciphers and ten
             extensions is what `rustls` offers, against Chrome's fifteen and
             fifteen, and no header set changes a `ClientHello`.

             **The header set is Chrome's and the header order is not, and
             `reqwest` cannot express one.** Measured off the wire, twice, with
             the same result both times:

             ```
             accept, sec-ch-ua, sec-ch-ua-mobile, sec-ch-ua-platform,
             upgrade-insecure-requests, sec-fetch-site, sec-fetch-mode,
             sec-fetch-user, sec-fetch-dest, accept-language, priority,
             user-agent, accept-encoding
             ```

             Chrome sends `sec-ch-ua` first, `user-agent` fifth and
             `accept-encoding` eleventh. Two causes, both outside this tree:
             `http::HeaderMap`'s iteration is not insertion order, so
             `default_headers` cannot carry a sequence; and `reqwest` appends
             `user-agent` and `accept-encoding` itself, after everything else.
             It is **stable** across runs, which is what makes a golden worth
             keeping, and it is wrong, which is another measured argument for
             the vendored tier rather than a reason to stop.

             **The Akamai HTTP/2 fingerprint of this client is not captured and
             the reason is a good one.** It exists only after a handshake
             completes and ALPN picks `h2`, and the probe's certificate is self
             signed with no CA behind it, so `bit-cli` refuses it. Reaching it
             would need a flag that stops verifying certificates, and that is
             not a flag to add to a shipping binary for a test. `--plain` on
             the probe reads the header order off cleartext HTTP/1.1 instead,
             which needs no handshake and no exception.

Measured:    **What `--render` costs, and why the flag is not here yet.** The
             resolver ships and the driver does not, because a flag that does
             nothing does not ship.

             `crates/bit-cli-core/src/browser.rs` is the part no CDP crate
             solves: finding a browser somebody already installed. Explicit
             path first, then an already-running instance, then platform
             defaults for Linux, macOS and Windows, then a typed `NoBrowser`
             naming **every path it looked at**. `exists` is a parameter rather
             than a filesystem call, so the whole search is unit tested with no
             browser present, which is the case that has to work on every CI
             runner. **Fifteen** tests, and the two that matter most are the
             ones where nothing is found and where a named path is not there.

             The driver is a dependency decision rather than work, and the
             numbers say why. Each crate given its own probe, resolved and
             checked on toolchain 1.88 on 2026-08-29:

             | crate | packages added | MSRV 1.88 | note |
             | --- | --- | --- | --- |
             | `chromiumoxide` 0.9.1 | **136** | ok | brings `reqwest` **0.13**, a second major beside this tree's 0.12 |
             | `headless_chrome` 1.0 | **143** | ok | blocking API |

             `RESEARCH.md` section 10 recommends `chromiumoxide` and the
             licence is fine, MIT OR Apache-2.0. What it did not weigh is that
             136 packages land in **every** build, including the majority that
             never pass `--render`, and that one of them is a second `reqwest`
             major. Its default features are worse still: they pull
             `chromiumoxide_fetcher`, which exists to **download a browser**,
             and the operator's ruling is "never bundled". Measured: with
             default features on, the graph is 211 packages and does not even
             compile without one of its `zip` features.

             So `--render` wants the same shape the impersonating tier does, a
             Cargo feature rather than an unconditional dependency, and that is
             one decision covering both.

Blocked:     **Three things adoption needs that are decisions rather than
             work**, all measured on 2026-08-29 and none of them fatal.

             - **`deny.toml` forbids a git source.** `unknown-git = "deny"` and
               `allow-registry` names only crates.io, so the five apify git
               dependencies fail the `deny` gate as the file stands. Vendoring
               them under `vendor/`, which is what RULES.md section 6a requires
               instead of a fork, removes the git sources and the question with
               them.
             - **`--cfg reqwest_unstable` has to be tree wide**, because
               `impit` declares no `[features]` and reqwest's `http3` is
               therefore unconditional. `.cargo/config.toml` sets **per target**
               rustflags, and a `[build]` entry does not merge with a
               `[target.*]` one, so the flag has to be added to all three
               target blocks **and** to the three `RUSTFLAGS` blocks in
               `.github/workflows/ci.yml`. That is exactly the shape of
               [T-146](#t-146-ci-built-a-windows-binary-against-the-dynamic-c-runtime).
             - **Two `reqwest` majors end up in the graph**, 0.12.28 and
               0.13.4. `deny.toml` has `multiple-versions = "warn"`, so this
               warns rather than fails, and it is a real size cost.

             **The fork the survey recommends is forbidden here and the
             sanctioned route is better.** `RESEARCH.md` section 9 says to fork
             `apify/impit` and `apify/h2` into the operator's org and open a
             pull request with the `h2` fix. RULES.md section 6a forbids every
             part of that: `Azathothas/bit-cli` is the only repository an agent
             may write to. The route this repository already uses for exactly
             this problem is `vendor/` with a derived series under `patches/`
             and a record in `patches/UPSTREAM.md`, whose `Upstream:` field is
             where "a future apify release could retire this" is written down.

Closed:      **Partial. The static tier ships and is the half that could not be
             blocked.**

             `crates/bit-cli-core/src/page.rs` is the extractor: one function
             over an HTML string, so the rendered tier can change where the
             HTML came from and nothing else. It is a forward-only tag scanner
             rather than a tree builder, because "which hrefs are on this page"
             never needs to know what nests inside what, and a scanner has no
             recovery rules to get wrong on the markup real indexers serve.

             **The parser choice is measured, not asserted.** Four candidates,
             each given its own crate and resolved and checked on toolchain
             1.88 on 2026-08-29:

             | parser | packages added to the graph | MSRV 1.88 |
             | --- | --- | --- |
             | none, a hand-written scanner | **0** | ok |
             | `tl` | **1** | ok |
             | `lol_html` | **44** | ok |
             | `scraper` | **57** | ok |

             `lol_html` is what `RESEARCH.md` section 12 recommends, on the
             grounds that `impit` already carries it. That argument only holds
             once `impit` is in the graph, and it is a **rewriter**: pulling
             anchor text out of it needs stateful handlers, where a scanner
             reads the text between two tags directly. The scanner is 33 unit
             tests and no new dependency, and every level of the proving ground
             passes on it.

             `source.rs` decides page from torrent by **attempt and fall back**:
             the body is parsed as bencode first, and only when that fails is
             it asked whether markup arrived. Deciding from `Content-Type`
             first would get a mirror serving a real `.torrent` as `text/html`
             wrong. A metainfo is a bencoded dictionary and begins `d`, so
             nothing that parses as one is ever taken for a page. One hop and
             never two: the torrent a page names is fetched with the plain
             parser, so a page linking to a page is an error rather than a
             crawl. That adds a fifth `source_kind`, `page`, for
             [T-250](#t-250-nothing-reports-how-an-input-was-resolved) to
             report.

             `--page-select TEXT` is the selector the Approach asks for, under
             a "Resolving a web page" heading flattened into the same nine
             commands `SwarmSourceArgs` is, matched case insensitively as a
             substring against both the resolved URL and the anchor text. A
             page is still refused when it leaves more than one, because a
             selector that matches two is not a selection.

             **The proving ground is new and is the thing that makes the tier
             falsifiable.** `scripts/make-page-fixture.ps1` emits six levels
             and four acceptance cases, each with the correct extraction beside
             it as JSON, carrying **two** lists: what the static tier must find
             and what a browser must find.

             `scripts/check-page-extract.ps1` serves them through
             `loopback-fileserver` and compares. Run 2026-08-29:

             ```
             ok   L0-flat        level 0  4 link(s), in order, with their anchor text
             ok   L1-structure   level 1  5 link(s), in order, with their anchor text
             ok   L2-addressing  level 2  9 link(s), in order, with their anchor text
             ok   L3-decoys      level 3  2 link(s), in order, with their anchor text
             ok   L4-script      level 4  resolved the one link, info hash 0150ba15e8305dce993d0d76b7c567862a4c89bd
             ok   L5-hostile     level 5  refused, and named --render
             ok   one-torrent    level 0  resolved the one link, info hash 0150ba15e8305dce993d0d76b7c567862a4c89bd
             ok   one-magnet     level 0  the single magnet reached the swarm resolver
             ok   one-of-each    level 0  2 link(s), in order, with their anchor text
             ok   two-of-each    level 0  4 link(s), in order, with their anchor text

             check-page-extract: 10 case(s), 10 passed, 0 failed
               links only a rendered tier reaches:
                 L4-script      level 4  static 1  rendered 7  (+6)
                 L5-hostile     level 5  static 0  rendered 2  (+2)
             ```

             **That last block is the argument for `--render` existing**, and
             it is a number rather than an opinion: eight links on two pages
             that no static tier can reach, and zero difference between the
             tiers on levels 0 to 3, where a difference would be an extractor
             defect.

             **The first half of the Acceptance holds, run:**

             ```
             $ bit-cli info http://127.0.0.1:8099/one-torrent.html
             name                 payload.bin
             info hash            528e8fdd3dd50f4fc5a4c3363303406a7076f3b7
             size                 4.00 KiB
             $ echo $LASTEXITCODE
             0
             ```

             ```
             $ bit-cli info http://127.0.0.1:8099/two-of-each.html
             error: http://127.0.0.1:8099/two-of-each.html is a web page with 4 torrent links, and nothing says which one to take. Name one of them directly, or narrow it with --page-select:
               http://127.0.0.1:8099/files/first.torrent  (Example 24.04 Desktop)
               http://127.0.0.1:8099/files/second.torrent  (Example 24.04 Server)
               magnet:?xt=urn:btih:0102030405060708090a0b0c0d0e0f1011121314&dn=Short+One  (Example 24.04 Desktop magnet)
               magnet:?xt=urn:btih:9e20e33071fae16fc950cd95e5fc6ec0059d9a63&dn=Example+Payload+24.04&...  (Example 24.04 Server magnet)
             $ echo $LASTEXITCODE
             4
             ```

             **The Acceptance's first sentence reads two ways and only one of
             them agrees with the Approach.** "A page carrying one `.torrent`
             link and one magnet link resolves" cannot mean one page carrying
             both, because the Approach rules that a page yielding more than
             one is refused. It is read as two pages, one link each, and both
             are cases: `one-torrent` and `one-magnet`. The page carrying one
             of each is a third case, `one-of-each`, and it resolves only with
             `--page-select`.

             **The second half of the Acceptance holds, run.** The header set
             and the TLS fingerprint are asserted against a recorded capture
             rather than eyeballed:

             ```
             $ pwsh -NoProfile -File scripts/check-fingerprint.ps1
             ok   browser  matches the golden
                    JA4     t13i1010h2_61a7ad8aa9b6_3fcd1a44f3e3
                    headers accept, sec-ch-ua, sec-ch-ua-mobile, sec-ch-ua-platform, upgrade-insecure-requests, sec-fetch-site, sec-fetch-mode, sec-fetch-user, sec-fetch-dest, accept-language, priority, user-agent, accept-encoding
             ok   plain    matches the golden
                    JA4     t13i1010h2_61a7ad8aa9b6_3fcd1a44f3e3
                    headers accept, user-agent, accept-encoding

             check-fingerprint: 2 profile(s), 0 failed
             ```

             The goldens are `fingerprints/bit-cli-browser.json` and
             `fingerprints/bit-cli-plain.json`, and `fingerprints/` is a new
             top level directory rather than a `bench/` file, because
             `bench/*.json` is gitignored and force-added one run at a time
             while a golden a check reads every run has to be tracked normally.
             `-Update` is the only thing that rewrites one.

             **JA4 is asserted and JA3 is not.** JA4 sorts before hashing and
             survives a client that shuffles its extensions; JA3 preserves wire
             order and flakes. JA3 is recorded in the golden for a reader and
             never compared.

             Both checks are a CI job, `Page extraction and fingerprint`, so a
             `rustls` or `reqwest` upgrade that moves the `ClientHello` or the
             header order fails a run rather than going unnoticed. That takes
             CI from twenty-two jobs to twenty-three.

             **The oracle behind it.**
             `crates/bit-cli-core/examples/loopback-tlsprobe/` is a `loopback-*`
             fixture like the other three, with `test = true` so its
             `ClientHello` parser, its HPACK decoder and its golden-manifest
             reader are in `cargo test --workspace`. It reports JA3, JA4,
             JA4_r, the Akamai HTTP/2 fingerprint and the HPACK-decoded header
             order, takes `--expect-ja4`, `--expect-akamai` and
             `--expect-file`, and writes a golden with `--write-golden`.
             `rcgen` arrives with it as a **dev** dependency only, which is
             what makes the throwaway certificate possible;
             `about.toml` sets `ignore-dev-dependencies`, so it reaches no
             released binary and no notice file.

Ruled:       **Three decisions, 2026-08-29, after the measurements above and
             recorded here so this entry stands on its own.**

             1. **The impersonating client ships in the default build, on every
                artifact.** Not a Cargo feature and not off by default. The
                operator's reason: almost everything that fetches a remote URL
                passes through this path, so a release binary that does not
                impersonate is one the work never reached. The three costs in
                the Blocked block are accepted rather than avoided.
             2. **`--render` is a Cargo feature, off by default**, built by a
                CI job so it cannot rot. 136 packages in every artifact for a
                flag that is inert without an external browser is the wrong
                trade. The browser resolver stays in the default build.
             3. **The publishing foundation is schema and staleness detection
                only.** Versioned JSON with a stable schema, and the job that
                detects drift and **emits the replacement values as proof**.
                Uploading it to a release is
                [T-260](#t-260-a-release-publishes-binaries-and-nothing-a-program-can-consume),
                filed for it; [T-261](trackers.md) is the tracker list that
                is the second consumer that format has to fit.

             **The staleness half is a first class requirement, not a
             nicety.** The operator's words: this must not break in the future
             if pages become stricter or browsers change fingerprints, and the
             tools must detect that **in time and recommend fixes with proof
             and new values**. A check that only says "your fingerprint
             changed" is half a tool.

Correction:  **The `--render` driver costs 15 packages in this tree, not 136,
             and the ruling that made it a feature was given the larger
             number.** The 136 was measured in a standalone probe with an
             empty graph. Measured here, against the graph this tree already
             has:

             ```bash
             cargo tree -e normal --prefix none -p bit-cli | sort -u | wc -l
             cargo tree -e normal --prefix none -p bit-cli --features render | sort -u | wc -l
             ```

             327 against 342. `chromiumoxide`'s dependencies are almost all
             already here: `reqwest 0.13`, `tokio`, `serde`, `futures` and
             `tracing` arrive with `librqbit` and with the impersonating
             client. `default-features = false` is what keeps
             `chromiumoxide_fetcher`, which exists to **download** a browser,
             out of it.

             **Ruling 2 was not reopened and `--render` is a feature.** The
             ruling is the operator's and this is the number it was given
             wrong, recorded so it can be revisited rather than rediscovered.

Correction:  **Three of the five apify forks the survey named are not
             vendored, and each one is a measurement rather than a
             preference.** The Blocked block above says five upstreams. It is
             five, and two of them are different ones.

             **`apify/h2` is not vendored and `hyperium/h2` at `v0.4.19` is.**
             The fork is `0.4.7` against the `0.4.19` every requirement in
             this graph asks for, which is the defect this entry already
             recorded; what it did not follow through was that vendoring the
             fork and bumping its version would leave `vendor/upstream.json`
             recording a base that does not describe the tree.
             `patches/README.md` says that makes the next merge wrong in a way
             nothing detects. So the tree is upstream `0.4.19` and the
             pseudo-header ordering is our own patch, credited to apify's
             change in `patches/UPSTREAM.md`.

             **`apify/tower-http` is not vendored.** Its whole change is two
             commented-out lines that stop `Content-Encoding` and
             `Content-Length` being removed after a response is decompressed,
             so every response would claim an encoding it no longer has and a
             length it never had. `impit` compiles against the published
             `tower-http` unchanged.

             **`apify/hyper-util` is not vendored, and `hyperium/hyper-util`
             is.** apify's change adds a status code to a proxy tunnel error,
             which needs one downcast in `impit/src/errors.rs` and which
             nothing here reads: `bit-cli` configures no proxy on this path.
             Removing the downcast is twelve lines. What **is** vendored is
             upstream at a commit past 0.1.20, unpatched, for one method
             upstream took after that release, `http2_header_table_size`.

             **A fifth tree arrived that the entry did not name:
             `seanmonstar/reqwest`, the 0.13 line.** It is the reason the
             others work at all, and the diagnosis is below.

Correction:  **`--cfg reqwest_unstable` is not needed anywhere, and the
             Blocked block's second cost is not paid.** It was going to be
             paid for `impit`'s `reqwest` feature list, which carries `http3`.
             The feature is removed from the vendored tree instead. The
             reasoning is the entry's own: adding the flag means all three
             `.cargo/config.toml` target blocks **and** the three `RUSTFLAGS`
             blocks in `ci.yml`, where a workflow's `RUSTFLAGS` replaces the
             config rather than adding to it, which is exactly
             [T-146](#t-146-ci-built-a-windows-binary-against-the-dynamic-c-runtime).
             Carrying that forever to ship an HTTP/3 path nothing in this tree
             can read a fingerprint from is the wrong trade. `patches/UPSTREAM.md`
             has the section.

Correction:  **The Blocked block's third cost was already paid before this
             work started.** It says two `reqwest` majors end up in the graph.
             They were already there: `librqbit` depends on `reqwest 0.13` for
             its tracker and UPnP clients and this repository's own crates ask
             for 0.12.

             ```bash
             cargo tree -i reqwest@0.13.4 --workspace
             ```

Correction:  **`http::HeaderMap` was not why the header order was wrong, and
             the entry recorded the wrong cause.** The previous session
             measured Chrome's header set arriving in the wrong sequence and
             named two causes: that `HeaderMap`'s iteration is not insertion
             order, and that `reqwest` appends `user-agent` and
             `accept-encoding` itself.

             Only the second is load bearing. Measured off the wire twice, on
             a thirteen header set: with `user-agent` and `accept-encoding`
             **in** the map at Chrome's positions, the order on the wire is
             Chrome's exactly, and `HeaderMap` iterated in insertion order both
             times. `http`'s own documentation calls that order arbitrary, so
             it is a coincidence to rely on rather than a contract, and the
             golden in CI is what catches it moving.

             That measurement retired a patch this session had already
             written. An `h2::ext::HeaderOrder` request extension, threaded the
             same way `PseudoOrder` is, produced a **byte-identical** capture.
             `TODO/RULES.md` section 5 says a flag that does not move a number
             does not ship, so it was removed rather than kept as insurance.

Correction:  **The last `Left:` item proposed matching the anchor text, and
             the page it was written about has none.** `linuxtracker.org`
             publishes every torrent as
             `<a href="index.php?page=downloadcheck&id=<hex>"><img
             alt="Download Torrent"></a>`: no extension, and **no anchor text
             at all**. The label is on the image. Fetched once, robots
             honoured, by `scripts/check-page-fetch.ps1`.

Measured:    **What the client puts on the wire now, against a real Chrome
             151 on the same machine on the same day.** Captured with
             `loopback-tlsprobe`, three ways: `--raw` for the `ClientHello`,
             `--plain` for the cleartext header order, and a completed
             handshake for the HTTP/2 half.

             | | before | after | Chrome 151 |
             | --- | --- | --- | --- |
             | JA4 | `t13i1010h2_61a7ad8aa9b6_3fcd1a44f3e3` | `t13i1515h2_8daaf6152771_806a8c22fdea` | `t13i1515h2_8daaf6152771_806a8c22fdea` |
             | Akamai | not reachable | `1:65536;2:0;4:6291456;6:262144\\|15663105\\|0\\|m,a,s,p` | `1:65536;2:0;4:6291456;6:262144\\|15663105\\|1:1:0:255\\|m,a,s,p` |
             | header order | `accept` first, `user-agent` twelfth | Chrome's | Chrome's |

             **The JA4 reached Chrome's exactly**, cipher hash and extension
             hash both, which is what the entry asked for and did not assume.
             Fifteen ciphers and fifteen extensions where there were ten and
             ten, carrying ALPS `0x44cd`, ECH `0xfe0d`, certificate
             compression `0x001b`, the SCT extension `0x0012` and the ML-DSA
             signature algorithms `0x0904/5/6`.

             **The Akamai fingerprint differs in one field of four**, the
             PRIORITY one, and that is [T-262](#t-262-the-http-2-fingerprint-matches-a-real-chrome-in-three-fields-of-four).

             The header order took four steps to reach and each was measured:
             the pseudo-header order needed `h2` patched, reaching `h2` needed
             `reqwest` patched, `SETTINGS_MAX_FRAME_SIZE` needed leaving out,
             and `SETTINGS_HEADER_TABLE_SIZE` needed a `hyper-util` method
             that is not in a release. `patches/UPSTREAM.md` has the table.

Measured:    **The same profile, verified against a second Chrome on a second
             platform.** The `Staleness` workflow ran on 2026-08-29, run
             **33251738663**, on `ubuntu-latest`:

             ```
             browser  /usr/bin/google-chrome
             version  Google Chrome 151.0.7922.173
               JA4     browser t13i1515h2_8daaf6152771_806a8c22fdea
               akamai  browser 1:65536;2:0;4:6291456;6:262144|15663105|1:1:0:255|m,a,s,p
             ```

             Linux, a different patch release, and the **same JA4 and the same
             Akamai fingerprint** as the Windows Chrome 151 this profile was
             checked against and as `bit-cli` itself. A fingerprint that held
             on one machine could have been a property of that machine; two
             platforms and two patch releases say it is a property of the
             browser.

             It also answers a question that was open: CI cannot supply a
             newer capture, because the runner image carries the same major
             this machine does.

Measured:    **A difference JA4 cannot see, found by the tool added to look
             for it.** `JA4_ro` keeps the wire order where JA4 and JA4_r sort,
             and the two clients diverge there:

             ```
             chrome   ciphers 4a4a,1301,1302,1303,c02b,c02f,c02c,c030,cca9,cca8,c013,c014,009c,009d,002f,0035
             bit-cli  ciphers 0a0a,1301,1302,1303,c02b,c02f,c02c,c030,cca9,cca8,c013,c014,009c,009d,002f,0035
             chrome   exts    6a6a,0005,0033,000a,44cd,0023,002d,ff01,001b,000d,000b,0017,fe0d,002b,0012,0010,4a4a
             bit-cli  exts    ff01,000b,44cd,0017,0023,000d,0005,000a,0010,0012,0033,002d,002b,001b,fe0d
             ```

             **The cipher list is Chrome's, value for value and in Chrome's
             order**, GREASE included and only the GREASE value itself
             differing, which is what a browser varies per connection. The
             extension list is Chrome's **set** in a fixed order, with no
             GREASE at either end. That is
             [T-263](#t-263-the-extension-list-is-chromes-set-in-a-fixed-order-and-chrome-shuffles-it),
             and `tlsfp.rs` asserts the absence so the entry closes when it
             starts failing.

Measured:    **What it costs, on the artifacts.** Measured either side, on
             `x86_64-pc-windows-msvc`, with a build of `HEAD` before the
             change for the binary size.

             | | before | after |
             | --- | --- | --- |
             | packages in the graph | 420 | 446 |
             | binary | 20.13 MiB | 21.38 MiB |
             | vendored source | 3.4 MB, 390 files | 9.1 MB, 824 files |
             | `--cfg reqwest_unstable` | n/a | **not needed** |

             `scripts/check-static.ps1` passes on the Windows binary and CI's
             `Build` matrix passed on both musl targets, which is the answer
             to the one question this entry left to CI.

Measured:    **Two extraction gaps closed, against the pages the measurement
             already fetched.** `scripts/check-page-fetch.ps1 -Extract` runs
             the shipping extractor over each saved body through
             `loopback-fileserver`, so no second request reaches anybody, and
             records what each rule found. Committed as
             `bench/page-extract-20260829.json`.

             | page | links | by extension | by label |
             | --- | --- | --- | --- |
             | `kali` | 113 | 113 | 0 |
             | `linuxtracker` | **75** | 0 | **75** |
             | `webtorrent-free` | 10 | 10 | 0 |
             | `ubuntu-alt` | 8 | 8 | 0 |
             | `debian-cdimage` | 3 | 3 | 0 |
             | the other ten | 0 or 1 | unchanged | 0 |

             linuxtracker went from **0** to **75**, and nothing else moved.
             One of the 75 is a false positive, that page's own template link
             with its id unset and a second parameter that is not, and it is
             written down in a test rather than argued away.

Closed:      **Done, 2026-08-29.** Every item of the `Left:` list is closed or
             carried as a filed entry with a measured reason.

             1. **The impersonating fetch tier ships in the default build.**
                `crates/bit-cli-core/src/fetch.rs` is one `Fetcher` trait over
                one `GET` with a ceiling and a deadline, and two clients behind
                it. `bit_cli_core::fetch::Identity` chooses. Five upstreams are
                vendored, the `h2` ordering is a request extension rather than
                `IMPIT_H2_PSEUDOHEADERS_ORDER`, and the corrections above say
                which trees and why.

                **A web seed is unaffected**, and so is a tracker announce, a
                peer handshake and a list fetched by `--tracker-list-url` or
                `--web-seed-list-url`: the list fetcher goes through the
                **plain** client whatever `--page-client` says, and there is a
                test.

                `--page-client` did **not** gain a third value. The only
                candidate was the old behaviour, Chrome's header set over
                this tree's own `ClientHello`, and that is not a client
                anybody is: it would be a third fingerprint belonging to
                nothing, which is the opposite of what this entry is for.
             2. **The `ClientHello`.** Measured above. It is Chrome's.
             3. **Chrome's header order.** Measured above. It is Chrome's, and
                the correction says what actually caused the old one.
             4. **`--render` ships**, behind a Cargo feature, off by default,
                built and run by a CI job named `The rendered tier`. The flag
                exists in **every** build and a binary without the feature
                refuses it by name: a manual, an error message and a command
                surface that change shape between two binaries of the same
                version is worse than a flag that says why it cannot run.

                The driver navigates, waits for the document to stop changing
                rather than for a guessed duration, and composes one HTML
                string out of the document and any open shadow roots. That
                string goes to the same `page::extract` the static tier calls.
                **No browser is left running on any path out**, including the
                deadline, and `check-page-extract.ps1` counts browser
                processes either side of the rendered tier to prove it.
             5. **`<link href>` is read**, and `type="application/x-bittorrent"`
                with it, which is what a `<link rel="alternate">` actually
                carries.
             6. **An indexer whose links do not end `.torrent` is read**, by
                the label rule, and the correction above says why matching the
                anchor text alone would not have worked.

             **The decision on item 6, and the two options it refused.**
             Following a candidate to read its `Content-Type` was refused: it
             turns one page into one request per link, and the one-hop rule is
             what stops a page becoming a crawl. A per-host rule was refused as
             a maintenance burden with no end. What ships uses only what the
             page already says about itself, costs no request, and was
             measured on fifteen real pages before it was written down.

             **The proving ground has L6 and L7**, and both tiers run over all
             thirteen cases:

             ```
             check-page-extract: 26 case(s) over static and rendered, 26 passed, 0 failed
               links only a rendered tier reaches:
                 L4-script      level 4  static 1  rendered 7  (+6)
                 L5-hostile     level 5  static 0  rendered 2  (+2)
                 L6-hidden      level 6  static 3  rendered 4  (+1)
                 L7-unfriendly  level 7  static 0  rendered 2  (+2)
               no browser was left running: 15 before, 15 after
             ```

             L6 carries the shape linuxtracker publishes and the three shapes a
             looser rule would wrongly take. L7 carries links assembled from
             split strings, one built two frames later, one behind a click and
             one behind a scroll, and a **challenge** page with a real-looking
             `cf-browser-verification` form and a meta refresh. The challenge
             case's expected answer is a refusal in both tiers, and the form is
             there so that a change which starts posting one fails.

             **Staleness is detectable and it recommends with proof**, which
             was the operator's first-class requirement.
             `scripts/check-browser-version.ps1` asks Google, Mozilla and
             Microsoft what stable is, with every fetch trapped on its own, and
             prints the replacement `BROWSER_MAJOR`, `BROWSER_USER_AGENT` and
             `sec-ch-ua` when the profile is behind. It says **Chrome 152**
             today against a profile claiming 151.
             `scripts/check-browser-fingerprint.ps1` drives the browser this
             machine has and prints the replacement `BROWSER_HEADERS` in the
             shape `page.rs` wants. Both write versioned JSON with a `schema`
             field, per ruling 3, and both run on a schedule in
             `.github/workflows/staleness.yml` rather than on a push: a browser
             shipping is not a defect in a commit.

             With no browser the fingerprint check exits **2** naming every
             path it looked at, which is the case that has to work on every
             runner.

             **The `BROWSER_MAJOR` was not bumped**, and that is a
             decision rather than an oversight. The TLS half of the profile is
             `impit`'s fingerprint database, whose newest Chrome is 151.
             Claiming 153 in the User-Agent over 151's `ClientHello` is a
             mismatch an origin can see, and it is a worse lie than being
             consistently one browser. The check reports the drift and the next
             session acts on it with a database that has moved.
Left:        **Nothing.** Two residuals are filed with their own entries and
             their own acceptance:
             [T-262](#t-262-the-http-2-fingerprint-matches-a-real-chrome-in-three-fields-of-four),
             the PRIORITY field of the Akamai fingerprint, and
             [T-263](#t-263-the-extension-list-is-chromes-set-in-a-fixed-order-and-chrome-shuffles-it),
             GREASE and the extension order. Both are P3, both are measured,
             and both are invisible to the fingerprint that is actually
             published and compared.

### T-245 Four commands refuse the URL download accepts

Source:      measured 2026-08-24 while checking the operator's brief
Category:    cli
Priority:    P1
Effort:      M
Status:      **done**, 2026-08-24

Problem:     `info`, `files`, `magnet` and `verify` all document their
             positional as "A .torrent path, an HTTP(S) URL, a magnet URI, an
             info hash, a metalink, or `-` for stdin". All four refuse the URL:

             ```
             $ bit-cli info http://127.0.0.1:56954/one.torrent
             error: http://127.0.0.1:56954/one.torrent has to be fetched before it can be read
             $ echo $LASTEXITCODE
             4
             ```

             `bit-cli download` fetches the same URL and completes.

             The refusal is `source.rs:262`, in `load_local`, which is what
             every command that does not start an engine calls.
Relevance:   Rule 0.10. The help text of four commands names an input those
             commands cannot take, and the error does not say "this command
             cannot" - it says the URL "has to be fetched", which reads as a
             missing step rather than a missing capability.

             It is the same shape as [T-181](#t-181-four-flags-are-accepted-in-silence-and-reach-no-code),
             and it is the one that blocks the most: every idea in the
             operator's brief of 2026-08-24 that treats an input as an abstract
             object needs a URL to resolve outside `download` first.
Approach:    `load_local` is the wrong name for what four commands need. Split
             it: a `resolve` that may fetch, used by every command, and the
             existing local-only path kept for a caller that must not touch the
             network.

             A fetch here is one `GET` of a small document, not a swarm lookup,
             so it does not carry `download`'s cost and does not need its
             flags. A magnet and a bare info hash stay refused under `info`,
             because those genuinely need the swarm, and that refusal is
             already correctly worded.

             Honour `--timeout` and refuse a body larger than a ceiling, so a
             URL that serves a gigabyte does not become a gigabyte in memory.
Acceptance:  `bit-cli info`, `files`, `magnet` and `verify` each resolve a
             `.torrent` served by `loopback-fileserver` and report what the
             local file reports, field for field under `--json`. A URL that
             serves markup still fails, with a message naming what arrived.

Correction:  **The title undercounts it. Nine commands refuse the URL, not
             four**, and a tenth refuses it with a different message that is
             also wrong. Measured by running every command against one URL
             before anything was changed, 2026-08-24:

             | command | before |
             | --- | --- |
             | `info`, `files`, `magnet`, `verify` | exit 4, "has to be fetched before it can be read" |
             | `webseed list`, `test`, `probe`, `fetch` | exit 4, the same message |
             | `bench webseed` | exit 4, the same message |
             | `trackers` | exit 4, "an info hash is needed to announce, and this source does not carry one" |
             | `download`, `seed`, `peers`, `bench leech` | works |

             `trackers` is the interesting one: the URL **does** carry an info
             hash, once fetched. It reaches `load_local` only for
             `Kind::Stdin`, at `crates/bit-cli/src/cmd/trackers.rs:96`, and its
             own classifier decides before that. It is left as it was and is
             carried by [T-251](../TODO/trackers.md), which is the entry that
             owns what a tracker command knows about its source.

             **And a metalink is the same defect in the same help string.**
             Every one of those nine offers "a metalink" in its `SOURCE` text
             and every one refused both metalink shapes. Fixed with the URL,
             because it is one code path and one sentence of help.

Closed:      `crates/bit-cli/src/source.rs` gained `resolve`, which fetches,
             and `resolve_blocking`, which is what a synchronous command calls.
             `load_local` stays as the local-only path and keeps the magnet and
             info hash refusal, because those need the swarm rather than one
             `GET`. Nine commands call `resolve_source` now, which reads
             `--timeout` and `--web-seed-user-agent` for them.

             Three bounds, all of them measured:

             - The deadline is `--timeout` when set and 30s otherwise. Against
               a `--stall-after 64` file server, `--timeout 2s` gave up at
               2,081ms and `--timeout 5s` at 5,090ms.
             - A fetch that runs out of time exits **9** and names the deadline
               in milliseconds. It exited 5 saying "error decoding response
               body" until that was fixed, which is `reqwest` describing the
               transport rather than the flag the caller set.
             - A `.torrent` body is capped at 16 MiB and a metalink at 1 MiB,
               counted as the bytes arrive. `fetch_metalink` read the whole
               body and measured it afterwards, so its 1 MiB cap bounded what
               was returned rather than what was held.

             The acceptance was run as a test rather than by hand:
             `read_only_commands_resolve_a_torrent_over_http_and_report_what_the_file_reports`
             compares all four commands' `--json` against the same torrent read
             off disk. Every field matches but `generated_at`, which is two
             runs, and `source_kind`, which differs because the source was a
             URL.

Prove:       ```
             cargo test -p bit-cli --lib source::
             ```

             Twenty-four tests, seven of them new: the four-command
             acceptance, the page that fails naming its content type, the body
             with no content type that fails without inventing one, the
             deadline that exits 9, the runtime guard, the local source that
             resolves from inside a runtime anyway, and the magnet that is
             refused without a fetch being attempted.

### T-246 Three inputs report a file error and two of them name the wrong cause

Source:      measured 2026-08-24 while checking the operator's brief
Category:    cli
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-24

Problem:     Three inputs produce a file error and two of them name the wrong
             cause.

             A directory:

             ```
             $ bit-cli info .tmp/ideas/payload
             error: cannot read C:\...\payload: Access is denied. (os error 5)
             $ echo $LASTEXITCODE
             4
             ```

             The `--json` form carries `"io_kind": "PermissionDenied"`. Nothing
             is denied. Reading a directory as a file is `ERROR_ACCESS_DENIED`
             on Windows and `EISDIR` on Unix, so the same input produces two
             different wrong explanations depending on the platform.

             A subcommand that does not exist:

             ```
             $ bit-cli tree one.torrent
             error: cannot read C:\...\tree: The system cannot find the file specified. (os error 2)
             ```

             The root command takes positional sources, so `tree` is read as a
             source named `tree`. A typo becomes a missing file.

             And a scheme nothing here speaks:

             ```
             $ bit-cli info ftp://host/x.torrent
             error: cannot read C:\...\ftp://host/x.torrent: The filename,
             directory name, or volume label syntax is incorrect. (os error 123)
             ```

             `classify` tests for `http://` and `https://` and falls through
             to "treat it as a path" for everything else, so a URL of any other
             scheme is a relative filename. `source.rs:93` is the test and
             `source.rs:116` is the fall-through.
Relevance:   All three are the first thing a new caller does. `bit-cli info <dir>`
             is what somebody types when they mean `create`, and a wrong
             subcommand is what somebody types when they are guessing at the
             surface. No error says what to do, and two of the three say
             something untrue.

             There is a fourth fact that ties them together and is worth
             stating once: **no input to a `SOURCE` argument produces a usage
             error.** Every one of these exits 4, because the classifier's last
             rule is "treat it as a path". Exit 2 is reachable from a flag and
             not from a source.
Approach:    Test for a directory before the read and say so, naming `create`
             as the command that takes one. Map `EISDIR` and
             `ERROR_ACCESS_DENIED` on a path that is a directory to the same
             message on both platforms.

             For the subcommand: a bare positional that matches no known
             subcommand, is not a path that exists, and is not any other
             recognised shape, is a usage error naming the nearest subcommand
             by edit distance. `clap` has the mechanism; the positional is what
             takes precedence over it.
Acceptance:  `bit-cli info <directory>` exits 2 naming `create`, on Windows and
             on Linux, with the same message. `bit-cli tre one.torrent` exits 2
             suggesting `tree`, and `bit-cli ./tre` still reports a missing
             file. `bit-cli info ftp://host/x` names the scheme and the three
             that are supported.

Closed:      All three exit 2 now and each says what to do. Run against the
             release binary:

             ```
             $ bit-cli info ideas/payload
             error: C:\...\ideas/payload is a directory, not a .torrent. `bit-cli create` is the command that takes a directory
             $ echo $LASTEXITCODE
             2
             $ bit-cli tre album.torrent
             error: `tre` is not a command, and there is no file of that name. Did you mean `bit-cli tree`?
             2
             $ bit-cli ./tre
             error: cannot read C:\...\./tre: The system cannot find the file specified. (os error 2)
             4
             $ bit-cli info ftp://host/x.torrent
             error: `ftp:` is not a scheme this reads. A source is an http:// or https:// URL, a magnet: URI, a .torrent or metalink path, a bare info hash, or `-` for stdin
             2
             ```

             The `--json` form of the first carries
             `"context": {"source_kind": "directory"}` and no `io_kind` at
             all, where it used to carry `"io_kind": "PermissionDenied"`.

             **The message is this tree's rather than the operating system's,
             which is what makes it the same on Windows and on Linux.**
             `source::read_torrent_file` at
             `crates/bit-cli/src/source.rs:218` tests `path.is_dir()` before
             the read, so neither `ERROR_ACCESS_DENIED` nor `EISDIR` is ever
             reached and there is no per-platform text to keep in step. **Nine**
             call sites read a caller-supplied `.torrent` path and all nine go
             through it: `source.rs:242` inside `load_local`,
             `download.rs:545` and `:2909`, `seed.rs:162`, `trackers.rs:95`,
             `edit.rs:68`, and `bench.rs:107`, `:1115` and `:1362`.

             `Kind::classify` gained `foreign_scheme`
             (`crates/bit-cli/src/source.rs:199`). Two things it deliberately
             does not do. A scheme of one character is not a scheme, because
             `C://Users/me/x.torrent` is a path and a drive letter; and only
             the `://` form is tested, so `urn:btih:<hash>` is still read as
             what it is rather than refused as a scheme called `urn`.

             The subcommand half is `mistyped_subcommand`
             (`crates/bit-cli/src/lib.rs:201`), on the one dispatch arm where
             a bare positional is a source. **Four conditions, and three of
             them exist to keep a real file out of the branch**: the word
             carries no `/`, `\`, `.` or `:`; nothing of that name is on
             disk; it classifies as a plain path rather than a URL, a magnet,
             an info hash or `-`; and a subcommand is within **one** edit of
             it. So `./tre` and `tre.torrent` are paths, a torrent actually
             named `tre` is downloaded, and `quuxly` is a missing file rather
             than a guess. The names come from `Cli::command()` rather than a
             list, so a subcommand added later is suggestible with nothing for
             anybody to remember.

             **The entry's own example went stale between filing and
             closing.** It used `bit-cli tree one.torrent` as the subcommand
             that does not exist, and [T-249](metainfo.md) built `tree` earlier
             in the same session. That is why the acceptance reads `tre`, and
             why `tree` is the name the suggester now offers for it.

             **The fourth fact in Relevance is no longer true, which was the
             point of writing it down.** "No input to a `SOURCE` argument
             produces a usage error" held when the entry was filed. Three
             inputs produce one now, and the exit code is the difference
             between "this source could not be resolved", which a retry might
             fix, and "this is not a source", which no retry fixes.
             `docs/exit-codes.md` already calls 2 an argument error and 4 a
             resolution failure, so nothing in the contract moved.

             Ten tests. `a_directory_as_a_source_exits_two_from_the_command_line`
             drives `info`, `files`, `tree`, `verify` and `magnet`, because one
             command proving it says nothing about the other four.
             `docs/exit-codes.md` gained the three shapes under "What exits 2".

### T-247 A dry run over a URL prints zero for a count it never took

Source:      measured 2026-08-24
Category:    cli
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-24

Problem:     `download --dry-run` over a URL prints:

             ```
             source               http://127.0.0.1:56954/one.torrent
             web seeds            0
             trackers             0
             ```

             The torrent has one tracker. Nothing was fetched, because a dry
             run does not fetch, so nothing was counted. The `--json` form of
             the same run is correct: `"name": null`, `"info_hash": null`,
             `"total_bytes": null`, `"needs_network": true`.
Relevance:   The two renderings of one document disagree, and the one a person
             reads is the wrong one. A caller checking a torrent's trackers
             before committing to a download reads `trackers 0` and concludes
             it has none.

             It is [T-156](#t-156-a-dry-run-writes-a-different-shape-under-the-same-document-kind)'s
             neighbourhood and the opposite failure: that one was two shapes
             under one name, this is one shape rendered two ways that do not
             agree.
Approach:    `cmd/download.rs:3018` already has the pattern three lines above
             the defect. `name` is emitted through `if let Some(name)`, so it
             is absent when unknown; `web seeds` and `trackers` take
             `as_array().map_or(0, Vec::len)` on a field that is an empty array
             precisely because nothing looked.

             Emit the counts only when the source was actually read, and print
             one line saying the document was not fetched when it was not.
Acceptance:  `download --dry-run <URL>` prints no count it did not take and
             says why, and `download --dry-run <LOCAL>` still prints
             `trackers 1` for a torrent with one tracker. The `--json` shape
             does not change.

Closed:      The text form says what it did not do, and counts only what it
             took. Against a plain HTTP server on loopback holding the same
             torrent that is also on disk, so the two runs differ in the source
             form and in nothing else:

             ```
             $ bit-cli download --dry-run http://127.0.0.1:8099/tracked.torrent
             source               http://127.0.0.1:8099/tracked.torrent
             not fetched          a dry run does not fetch the torrent, so its own web seeds and trackers are not counted
             web seeds            0 so far
             trackers             0 so far

             $ bit-cli download --dry-run .tmp/treedemo/tracked.torrent
             source               .tmp/treedemo/tracked.torrent
             name                 album
             web seeds            1
             trackers             1
             ```

             That torrent carries one tracker and one web seed, so the second
             run is the acceptance's other half: a count that was taken is
             printed with no qualifier on it.

             **`0 so far` rather than nothing, because zero is not always what
             a dry run over a URL knows.** A `--web-seed` or a `--tracker` on
             the command line is a real source that a real count can be taken
             of, and so is a Metalink's mirror list, which is read without the
             network. The same run with one of each prints `1 so far` for both.
             Suppressing the line entirely would have lost that, and printing
             it bare would have said the torrent has one.

             The condition is `torrent["name"].as_str()`, which is present
             exactly when the metainfo was read: `dry_run` builds a `Metainfo`
             for `Kind::File` and for nothing else, and `name`, `info_hash` and
             `total_bytes` are all `None` together. `cmd/download.rs:3027`.

             **The `--json` shape is untouched**, which the acceptance asks and
             `the_dry_run_json_still_carries_the_nulls_that_said_so` holds. The
             document already said the torrent had not been read, through those
             three nulls and `needs_network`. It is the rendering a person
             reads that was the wrong one, which is the inverse of
             [T-156](#t-156-a-dry-run-writes-a-different-shape-under-the-same-document-kind):
             that was two shapes under one name, this was one shape rendered
             two ways that did not agree.

             Three tests. [`docs/examples/inputs.md`](../docs/examples/inputs.md)
             carried a paragraph telling a reader to read the JSON instead when
             the source is a URL; it carries the output above instead.

### T-250 Nothing reports how an input was resolved

Source:      the operator's brief of 2026-08-24
Category:    cli
Priority:    P2
Effort:      M
Status:      open

Problem:     A resolution can go URL to redirect to page to `.torrent` link to
             metadata, or magnet to swarm to metadata, or metalink to torrent
             URL to metadata. What a caller sees is the result or an error.
             Nothing prints the path taken.

             The eleven `--trace` subsystems in `logging.rs:233` cover the
             wire, the disk and the config. None covers resolution.
Relevance:   Every step this repository is adding to resolution makes the chain
             longer and the failure less legible.
             [T-244](#t-244-a-web-page-is-not-a-source-and-nothing-extracts-a-link-from-one)
             adds a page hop, [T-245](#t-245-four-commands-refuse-the-url-download-accepts)
             adds a fetch to four more commands, and Metalink already adds two.
             A chain nobody can print is a chain nobody can debug.
Approach:    Ruled on by the operator on 2026-08-24: **an `--explain` flag,
             available on every command.**

             `--explain` prints the chain the real run took and then does the
             work, so it cannot describe a path other than the one taken. That
             is the property that decided it over a standalone subcommand,
             which would be a second entry point into resolution and free to
             drift from the first.

             One line per hop: what was tried, what answered, what it was
             classified as, and how long it took. Redirects are hops. A
             `Content-Type` that disagreed with the extension is a hop worth
             printing, because it is where a wrong classification shows.

             Under `--json` it is an array on the document rather than a second
             document, so a caller keeps one parse.

             `--dry-run` is the neighbour and stays what it is: resolve,
             validate, report, write nothing. `--explain` says how it resolved;
             `--dry-run` says what would happen next. They compose.
Acceptance:  `bit-cli info --explain <URL>` prints every hop of a two-redirect
             chain served by `loopback-fileserver`, in order, with a timing
             each, and the same run under `--json` carries the same hops in an
             array. A resolution with one hop prints one hop rather than a
             heading with nothing under it.

### T-252 The run's numbers exist in JSON and cannot be asked for as text

Source:      the operator's brief of 2026-08-24, measured the same day
Category:    cli
Priority:    P3
Effort:      S
Status:      **done**, 2026-08-24

Problem:     A finished `download` already carries `elapsed_ms`, `downloaded`,
             `uploaded`, `from_peers`, `from_web_seeds`, `from_resume`,
             `mean_rate`, `process.cpu_ms`, `process.cpu_user_ms`,
             `process.cpu_system_ms`, `process.rss_bytes`,
             `process.peak_rss_bytes`, `process.open_handles`, and per source
             `http_requests`, `http_bytes`, `blocks`, `whole_pieces`,
             `connections` and `retries`.

             The text rendering shows some of it and reduces the process half
             to one line:

             ```
             cost                 peak RSS 17.73 MiB, CPU 46ms, 204 handles
             ```

             There is no flag that asks for more, and `--verbose` does not
             change what the report prints.

             Two numbers are absent from both renderings: bytes written to
             disk, and time spent in disk writes. `--trace disk` carries the
             events and nothing totals them.
Relevance:   Small, and the reason it is worth an entry is that the numbers are
             already there. Somebody reading a terminal has to pipe through a
             JSON parser to see a figure the run computed and then discarded.
Approach:    `--stats` on any command that produces a report: render every
             field the document already carries, grouped, in the same text
             style as the rest. It is a rendering flag, so it changes no
             behaviour and adds no measurement.

             The disk totals are the one part that is a measurement rather than
             a rendering. Total them where `--trace disk` already emits, and
             add the two fields to the document, which is what makes them
             renderable at all.
Acceptance:  `bit-cli download <TORRENT> --stats` prints every field of the
             `download` document that has a value, and the same run under
             `--json --stats` is byte-identical to the same run without
             `--stats`. The two disk fields appear in `docs/schema.md` because
             a run produced them.

Closed:      `--stats` is **global and implemented in one place**, rather than
             a flag per command. `Renderer::emit` at
             `crates/bit-cli/src/output.rs:110` is where every document becomes
             text, so putting it there means every report that exists and every
             report added later carries it, and each command's own summary
             stays the default. `stats_lines` at `:237` is the rendering.

             ```
             $ bit-cli download album.torrent --dir out --web-seed-only --stats
             completed            1
             disk.bytes_written.bytes 444700
             disk.bytes_written.human 434.28 KiB
             disk.write_calls     32
             disk.write_ops       20
             disk.write_time.ms   0
             downloaded.bytes     444700
             elapsed_ms           3655
             from_peers.bytes     0
             from_web_seeds.bytes 444700
             process.cpu_ms       77
             process.open_handles 245
             process.peak_rss_bytes 30240768
             ```

             Paths are the ones `docs/schema.md` names, so a line here and a
             row there are the same field. A `null` is skipped, because the
             document omits an optional field rather than writing `null` and a
             reader should not have to tell "not applicable" from "none"; an
             empty array prints as `[]`, because "this run had none" is an
             answer.

             **The disk half turned out to be plumbing rather than
             measurement.** The entry called it the one part that is a
             measurement. It is not: `crate::storage::StorageMetrics` has
             counted `write_bytes` and `write_nanos` on every run since T-018,
             at two clock reads per write, and `Engine::storage_counts` already
             exposed them for `bench leech`. Nothing needed measuring. What was
             missing was a field on the `download` document, read before the
             engine is dropped because nothing else can reach the counters
             afterwards.

             Four fields rather than two, and the two extra ones cost nothing:
             `write_ops` and `write_calls` were beside the other two, and
             `write_ops` over `write_calls` is the coalescing factor
             [T-018](../TODO/disk-io.md) exists to move. Six rows in
             `docs/schema.md`, from a run.

             **The ratio moves from run to run**, which is worth saying because
             a number in a doc reads as a property. The same command twice
             reported 20 writes for 32 calls and then 17 for 32: what can be
             combined depends on the order blocks arrive in.

             Three tests. `stats_prints_every_field_and_leaves_the_json_alone`
             is the acceptance and it holds both halves: every scalar the
             `--json` document carries is a line, and the two documents are
             equal once the timestamp is removed. Byte-identical is not
             assertable across two runs, because `generated_at` is when the run
             happened; equal in every other field is the same claim.

             `docs/examples/machine-output.md` and `docs/disk.md` carry it, and
             both output blocks are from runs.

### T-253 The schema sample takes one path, so thirteen real fields went undocumented

Source:      measured 2026-08-24 while writing `docs/examples/s3-webseed.md`
Category:    cli
Priority:    P2
Effort:      S
Status:      **partial**

Problem:     `docs/schema.md` is generated from what a real run produced, and
             the runs that generate it take one path through the code. A field
             that only appears on another path has never once been produced, so
             it has never been written down.

             Two paths, thirteen fields.

             **The sample serves plain HTTP.** `loopback-fileserver` has no TLS
             and issues no redirect, so `webseed_test` documented neither.
             Against a real S3 endpoint the same command returns nine more
             fields: `sources[].server`, `sources[].resolved_url`, six under
             `sources[].tls`, and `sources[].redirects[]` with `from`, `status`
             and `to`.

             **The sample downloads a torrent whose paths are all writable.** So
             `context.report.renamed[]` has never appeared, though
             `docs/disk.md` documents it and three commands emit it:
             `cmd/download.rs:72`, `cmd/seed.rs:65` and `cmd/verify.rs:86`, all
             carrying `bit_cli_core::paths::Rename` from `paths.rs:127`.
Relevance:   `docs/schema.md` is what a caller writes a parser against. A field
             missing from it reads as a field that does not exist.

             [T-158](#t-158-regenerating-the-schema-deletes-fields-the-sample-did-not-produce)
             fixed the generator so a field the sample misses is no longer
             deleted. It did not add the fields the sample has never once
             produced, and this is that half.
Approach:    The thirteen rows are added, from output a real run produced, so
             the document is true now. The union generator keeps them.

             What is left is the mechanism, which is why this is partial rather
             than done. Two fixtures, both small:

             - `loopback-fileserver` with a self-signed certificate and a
               redirect route. What is being recorded is which fields exist,
               not which cipher a real CDN picks, so a self-signed certificate
               is enough.
             A third thing is smaller and belongs here: `webseed_test`'s own
             one-line description in `crates/bit-cli/src/schema.rs` says
             "status, ranges, redirects, and timing", and the document now also
             carries the TLS report and the server. It is incomplete rather
             than wrong, and `docs/schema.md` is generated from that string, so
             it moves when the fixture does.

             - one torrent in the sample set whose paths need sanitising. The
               three used to produce the rows here were `../../pwned.txt`,
               `CON.txt` and a path with a tab in it, which between them raise
               `escape`, `trailing-dot-or-space`, `reserved-name` and
               `illegal-character`.
Acceptance:  `BIT_CLI_UPDATE_SCHEMA=1 cargo test -p bit-cli --lib schema` on a
             machine with no network produces every row this entry added,
             rather than inheriting it from the committed file. Deleting the
             thirteen rows and regenerating puts all thirteen back.

**Done on 2026-08-30: the redirect fixture, and three of the thirteen rows now
come from a run.** `FileServer::start_redirecting(root, hops)` answers `302`
with a `Location` one `via/` segment longer until the chain is walked, then
serves the resource that was asked for. It counts hops in the path rather than
in server state, so it is stateless and two clients cannot interfere.

The schema sample drives `webseed test` through **two** hops rather than one,
because a chain and a single redirect are different shapes and only the chain
proves the array is an array.

**Proved the way the acceptance asks**, rather than by reading the file: the
three `sources[].redirects[]` rows were deleted from `docs/schema.md` and
`BIT_CLI_UPDATE_SCHEMA=1 cargo test -p bit-cli --lib schema` put all three
back, and the regenerated file is byte-identical to what was there before.

**Still partial, and two pieces are left**, both named in Approach above and
neither started:

- **TLS on `FileServer`**, for `sources[].tls`'s six fields plus
  `sources[].server` and `sources[].resolved_url`. A self-signed certificate is
  enough, and there are three worked examples of `rcgen` in this tree now, in
  `crates/bit-cli-core/examples/loopback-tlsprobe/main.rs`. The client half
  already exists: `BIT_CLI_EXTRA_CA_FILE` adds one root.
- **A sample torrent whose paths need sanitising**, for
  `context.report.renamed[]`.

**Done in the session that filed it:** the thirteen rows, each from output that
was produced rather than read off a struct. `cargo test -p bit-cli --lib schema`
passes with them, because the committed check is containment: it asserts that
everything the program writes is documented, not that everything documented was
written on this run.

```
| `context.report.renamed[].disk_path` | string |
| `context.report.renamed[].index` | integer |
| `context.report.renamed[].reasons[]` | string |
| `context.report.renamed[].torrent_path` | string |
| `sources[].redirects[].from` | string |
| `sources[].redirects[].status` | integer |
| `sources[].redirects[].to` | string |
| `sources[].resolved_url` | string |
| `sources[].server` | string |
| `sources[].tls.alpn` | string |
| `sources[].tls.cipher_suite` | string |
| `sources[].tls.connect_ms` | integer |
| `sources[].tls.handshake_ms` | integer |
| `sources[].tls.server_name` | string |
| `sources[].tls.version` | string |
```

That is fifteen rows for thirteen fields: `redirects[]` and `tls` are objects
whose members each get a row, which is how every other nested field in the
document is written.

### T-255 Regenerating the schema deletes four hand-written sections and nothing fails

Source:      measured 2026-08-24 while adding the `tree` document to
             `docs/schema.md`, T-249
Category:    cli
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-24

Problem:     `docs/schema.md` is generated, and the generator renders the
             generated half only. The file ends with four sections nothing
             produces, written by hand: "Machine output, from the README",
             "Keeping a log", "On Windows", and "Reading a download as it
             arrives". That last one carries the only committed measurement of
             what seven PowerShell redirection forms do to non-ASCII output,
             which is `scripts/check-redirect.ps1`'s whole subject.

             ```bash
             BIT_CLI_UPDATE_SCHEMA=1 cargo test -p bit-cli --lib schema
             ```

             That deleted **130 lines**, all four sections, on 2026-08-24. The
             diff read `40 insertions(+), 130 deletions(-)` and the insertions
             were the new document kind.

             **Both gates then passed on the truncated file**, measured by
             stripping the tail again and running each one unpiped:

             | check | on the truncated file |
             | --- | --- |
             | `cargo test -p bit-cli --lib schema` | exit 0, 11 passed |
             | `scripts/check-docs.ps1` | exit 0, "everything resolves" |

             `schema_gen.rs:1334`
             `the_committed_schema_matches_what_the_program_writes` is a
             containment check over **fields**, so prose is invisible to it.
             `check-docs.ps1` resolves links and would have caught an
             unreferenced page, and `docs/examples/machine-output.md` is
             linked twice from `README.md:106` and `:183` as well as from the
             deleted paragraph, so it stayed reachable.
Relevance:   It is a silent deletion of the only copy of a measurement, on the
             one command a session is told to run whenever it adds a field. It
             happened this session and was caught by reading a diff, which is
             the check RULES.md section 2 step 4 calls review 1 and which no
             gate performs.

             [T-158](#t-158-regenerating-the-schema-deletes-fields-the-sample-did-not-produce)
             is the same shape one level down: regenerating used to delete
             **fields** the sample had not produced, and the fix was to make
             the generator union rather than replace. This is that fix not
             reaching the prose, and the note under "How this file is kept
             true" says regenerating is lossy without saying what it costs.
Approach:    Union rather than replace, the same answer T-158 took. The
             generated content ends at the last event section, so everything
             after it in the committed file is carried across verbatim.

             `schema::render` at `crates/bit-cli/src/schema.rs:273` builds the
             whole document, and the writer is `schema_gen.rs:1316`. The
             cheapest correct version reads the committed file first, finds the
             first heading that is not one the generator emits, and appends
             from there.

             A test is the other half and it is what makes this stick: write a
             marker section into a copy of the file, regenerate, and assert the
             marker survived.
Acceptance:  `BIT_CLI_UPDATE_SCHEMA=1 cargo test -p bit-cli --lib schema` on a
             tree whose `docs/schema.md` carries the four sections leaves all
             four in place, `git diff --stat` reports no deletion, and a test
             fails when the carry-across is removed.

Closed:      `carry_across` at `crates/bit-cli/src/schema_gen.rs:1401`, called
             at the end of `merge_schema`. It appends every `##` section of the
             committed file whose heading line the render does not emit, in
             committed order, after the generated content.

             ```bash
             BIT_CLI_UPDATE_SCHEMA=1 cargo test -p bit-cli --lib schema
             ```

             Run twice in a row, `git diff --stat docs/schema.md` prints
             **nothing** both times. Before this it printed
             `40 insertions(+), 130 deletions(-)` on the run that added the
             `tree` document.

             **Matching by heading line rather than by position** is what makes
             it idempotent and what stops a generated section being duplicated:
             a second run finds the hand-written sections at the end, does not
             recognise them either, and puts them back in the same place. A
             fence is tracked while splitting, because a `##` inside a
             `powershell` block is not a heading and a split that thought it
             was would carry half an example across on its own.

             Three tests, at `:1344`, `:1389` and `:1406`. The last one is over
             the committed file rather than a fixture: it renders, merges, and
             asserts all four real headings survive.

             **T-158's fixture had to change, and the reason is worth
             recording.** Its `rendered` string carried `## Documents` and not
             `## Events`, where `render` always emits both. Once regeneration
             started carrying across what the generator does not produce, that
             fixture's `## Events` was read as hand-written and its deliberately
             stray row came with it. The fixture now looks like a real render
             and the test's subject, which is row leakage between sections, is
             untouched.

             **`docs/schema.md`'s own note was describing a writer that stopped
             existing at T-158**, and this session's first reading of it is
             what sent this entry looking. It said "regenerating is lossy" and
             told a reader to put back any row it removed. Regeneration has
             unioned rows since T-158 and removes none.

             What is true is narrower and was measured rather than reasoned
             about: a row **taken out by hand** that no run produces does not
             come back. Removing `sources[].tls.cipher_suite` and regenerating
             left it removed. So the note now says regenerating adds and never
             removes, and that deleting is a one-way door. It lives in the
             `HEADER` constant at `crates/bit-cli/src/schema.rs:359`, not in
             the Markdown, because the Markdown is generated: editing the file
             directly is undone by the next regeneration, which is how the
             first attempt at this correction was lost.

### T-257 Two documents answer to type progress, and the guard against that only covers documents

Source:      found reading a soak's `--jsonl` output, 2026-08-24
Category:    cli
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-25

Problem:     `bit-cli seed --jsonl` and `bit-cli download --jsonl` both emit
             `"type": "progress"`, and the two documents differ in nine of
             their seventeen fields. Measured on 2026-08-24, one torrent, both
             commands at `--report-interval 1s`:

             | | fields |
             | --- | --- |
             | both | `at`, `download_rate`, `info_hash`, `peers`, `process`, `seq`, `type`, `upload_rate` |
             | `seed` only | `peer_detail`, `ratio`, `uploaded_bytes` |
             | `download` only | `eta_confidence`, `eta_ms`, `from_web_seeds`, `percent`, `progress_bytes`, `total_bytes` |

             `docs/schema.md`'s `progress` section is the **union** of the two
             and credits it to one command: it says
             "From `bit-cli download <TORRENT> --web-seed <URL> --jsonl`" and
             lists `peer_detail[]` and six `listener.*` rows, none of which
             that command has ever emitted.
Premise:     Measured, not read. Both commands were run against the same
             torrent and the key sets compared. The schema section was then
             read against both.
Relevance:   [RULES.md](RULES.md) section 5 says anything consuming `--jsonl`
             selects by `type`, never by position. For `progress` the `type`
             does not decide which document is in hand, so a consumer that
             reads `percent` off a `progress` event gets it from `download`
             and nothing at all from `seed`.

             It is the same defect [T-191](bench.md) closed for `kind`, one
             layer down, and T-191's own Relevance predicted it in the
             abstract: "the file claims a document that exists nowhere.
             Nothing would fail."
Approach:    **The guard is the cheap half and it is already written for the
             other case.** `fold_document` at
             `crates/bit-cli/src/schema_gen.rs:71` panics when two commands
             claim one `kind`, naming both. `observe_events` at
             `crates/bit-cli/src/schema_gen.rs:122` keys by `type` and merges
             whatever arrives, with no such check. Giving events the same
             guard is a few lines and it fails on `progress` immediately,
             which is the point: the guard is what makes the second half
             unavoidable rather than optional.

             **The second half is a decision and it is not this entry's to
             take alone.** Three ways out, and the recommendation is the
             third:

             1. Rename one. `seed_progress` and `download_progress` are honest
                and break every consumer selecting `progress` today.
             2. Union them for real: emit every field from both commands, with
                the ones that do not apply as null. Honest to the schema, and
                it makes a seeder carry six fields that mean nothing to it.
             3. **Keep one `type` and record that it has two shapes**, which is
                what the data actually is: a progress tick from a run that is
                downloading and one from a run that is not. `docs/schema.md`
                grows a per-command column or a second section, and the
                generator stops crediting a union to one command.

             Three is recommended because the wire format is what consumers
             already read, and because T-191 took the same fork the same way:
             it left `kind` alone and made the collision impossible to reach
             by accident instead. Breaking the format is what `schema_version`
             is for and this is not worth one.
Prove:       ```
             cargo test -p bit-cli --lib schema
             ```

             A test that two commands cannot claim one event `type`, in the
             shape of `two_commands_cannot_claim_one_document_kind`, and
             `docs/schema.md`'s `progress` section naming what each command
             emits rather than a union credited to one of them.

#### Closed, 2026-08-25, on the operator's ruling, and it is the third option

**The ruling accepted the recommendation**: one `type`, two shapes recorded.
`type` is what a consumer selects on, breaking it is what `schema_version` is
for, and [T-191](bench.md) took the identical fork the same way for `kind`.
Nothing about the wire format changed and `schema_version` did not move.

**`docs/schema.md`'s `progress` section says who writes what.** Every command
that produces a shape is named above the table, and a third column names which
of them writes each field, reading `both` or `all` where every one of them
does:

| field | type | from |
| --- | --- | --- |
| `at` | string | both |
| `peer_detail[]` | array | seed |
| `percent` | string | download |
| `listener.probes` | integer | seed |

The six `listener.*` rows and `peer_detail[]` this entry named are attributed
to `seed` now. They were under a line reading "From
`bit-cli download <TORRENT> --web-seed <URL> --jsonl`", which has never emitted
one of them.

**The Problem's "nine of seventeen" is not the number in the committed
section, and both are right.** That measurement is two plain runs at
`--report-interval 1s`. The generator's `seed` run passes `--listener-check 5s`,
which is what puts the six `listener.*` rows in the contract at all, so the
section reads **fifteen of thirty-two**. Nine of the fifteen are there whatever
flags are passed. The code comments quote the section's figure, because that is
the one a reader sees beside the table.

**The Approach's guard is not what shipped, and that is the ruling's doing.**
It said "giving events the same guard is a few lines and it fails on `progress`
immediately". Under option 3 a shared `type` is legal, so a panic would refuse
the thing the ruling permits. What replaced it removes the failure mode rather
than detecting it: `Sample` keys its commands and records, per field, which of
them wrote it, so a section for a shape two commands produce **cannot** be
rendered as a union credited to one. `fold_document`'s panic for a document
`kind` is unchanged, because merging two documents under one name still
describes a document that exists nowhere.

**Two more shapes were being unioned and this entry named neither.**

- **`session_start`**, from `download` and `seed`, differing in five of nine
  fields: `directory`, `max_concurrent_downloads` and `sources` are the
  downloader's and `data_directory` and `source` are the seeder's.
- **`session_end`**, from **four** commands, where `error` is written by
  `bit-cli info` alone.

`progress` was the one a reader would have tripped on and it was not the only
one.

Acceptance, run:

```bash
cargo test -p bit-cli --lib schema
```

18 passed. Three tests are this entry's:
`a_shape_two_commands_write_names_which_one_writes_each_field` and
`a_shape_one_command_writes_keeps_the_two_column_table` in `schema.rs`, and
`two_commands_sharing_one_event_type_are_attributed_rather_than_merged` in
`schema_gen.rs`. `two_commands_cannot_claim_one_document_kind` is unchanged and
still panics, which is the pair the two halves make.

**The Prove section above asked for a test "that two commands cannot claim one
event `type`", and that test would now be wrong.** It was written before the
ruling and describes option 1. The test that shipped asserts the opposite and
is what option 3 needs: that a `type` two commands claim is recorded as two
shapes rather than merged into one.

### T-258 A seeder re-sends every peer it has ever seen, every report interval

Source:      the operator's six hour soak of 2026-08-24, read while it ran
Category:    cli
Priority:    P2
Effort:      S
Status:      **done**, 2026-08-25

Problem:     `peer_detail` in a `seed --jsonl` progress event carries every
             peer row the engine holds, and the engine holds rows for peers
             that disconnected hours ago. The whole array goes out again on
             every tick.

             Measured against the operator's run, at `.tmp/soak/seed.out`, 30
             second interval, 5.2 hours in:

             | | |
             | --- | --- |
             | records | 620 |
             | stdout | 139,253,847 bytes |
             | 10th record | 10,564 bytes |
             | last record | 270,050 bytes |
             | rows in the last record | 871, every one `state: "not needed"` and `direction: "incoming"` |
             | peers seen | 2,008 |

             The per-record size grows with the number of peers ever seen, so
             the total grows with the square of the run length. 139 MB of
             stdout for a 16 MiB payload.
Premise:     Measured. The numbers above are from the operator's own run while
             it was in flight, counted by reading the file rather than by
             sampling it.

             **This is not [T-040](memory.md) coming back.** That entry
             bounded the rows in memory at 1,024 per torrent and the bound
             holds: 871 rows is under it, and `rss_bytes` is flat across the
             same window. What is unbounded is not the table, it is how many
             times the table is written out.
Relevance:   A consumer of `--jsonl` gets a 270 KB object every 30 seconds, of
             which a handful of fields changed. Serialising it is real work on
             the reporting path and real bytes on a pipe that may be a
             terminal, a file or another process.

             `scripts/soak.ps1` is the caller that shows the size, because it
             redirects the stream to a file and keeps it for the length of the
             run. Nothing in that run failed, which is why this is filed
             rather than fixed in flight.

             There is no flag for it. `man/bit-cli.json` has
             `--report-interval` on `seed`, which changes how often the whole
             array goes out and not what is in it.
Approach:    The seam is one line: `crates/bit-cli/src/cmd/seed.rs:502`,
             `"peer_detail": peers`, where `peers` is `view.rows` from
             `swarm::without_probe_rows`. `download`'s progress event does not
             carry the array at all, which is [T-257](cli-surface.md).

             Two things to decide, and the second is the entry:

             **What a progress tick owes.** A row for a peer that is gone is
             history, and history belongs in the final document rather than in
             every tick. The cheapest honest version is that a tick carries the
             peers that are currently connected, and the run's last document
             carries all of them, which is where a caller counting who ever
             connected already looks.

             **Whether that is a break.** It is a narrowing of an existing
             field, so a consumer reading `peer_detail` off a tick to count
             total peers would start getting a smaller number. `peers.seen` is
             in the same event and is the count that field never was.

             A flag is the wrong answer here and is worth saying so: a knob
             with no caller is what
             [`docs/task-authoring.md`](../docs/task-authoring.md) section 3
             calls building machinery nothing asked for. If the tick carries
             the wrong rows, the fix is the rows.
Prove:       ```
             pwsh -NoProfile -File scripts/soak.ps1 -Minutes 20 -Leechers 4
             ```

             Two runs of the same length, one before and one after, with the
             seeder's stdout byte count and its last record's size beside each
             other. **See the correction below for what has to hold**: the
             assertion is not that the record stops growing, which it already
             does after two hours, but that the size it settles at is a small
             fraction of today's 270 KB. The run's own numbers must not change,
             which is what the existing soak ceilings already assert.

#### Correction, same session: it plateaus, and that is worse rather than better

The Problem above says the per-record size grows with the number of peers ever
seen, "so the total grows with the square of the run length". **The first half
is true only until the row bound engages and the second half is false.** The
numbers in the first table were also taken at two instants and presented as
one, which is how the error survived being written down.

Re-measured against the same run in one pass, every sixtieth record:

| seq | at | bytes | rows | peers seen |
| --- | --- | --- | --- | --- |
| 1 | 16:46:08 | 504 | 0 | 0 |
| 60 | 17:15:38 | 74,378 | 235 | 235 |
| 120 | 17:45:39 | 145,406 | 460 | 460 |
| 180 | 18:15:39 | 215,809 | 681 | 681 |
| 240 | 18:45:40 | 283,435 | 894 | 898 |
| 300 | 19:15:40 | 281,554 | 896 | 1,146 |
| 420 | 20:15:41 | 275,696 | 883 | 1,620 |
| 540 | 21:15:42 | 270,411 | 869 | 2,102 |
| 660 | 22:15:44 | 271,546 | 873 | 2,591 |

The record grows with `peers.seen` for the first two hours because every peer
seen is still a row. At about 894 rows `rows` stops following `seen`, and the
record size stops with it. From `t+2h` to `t+5.5h` `peers.seen` nearly triples
and the record moves by under five percent.

**894 is not [T-040](memory.md)'s ceiling and the difference matters.** That
bound is 1,024 rows per torrent and the table never reaches it: what holds the
count in a band from 869 to 896 is the reclaim T-040 added, keeping up with
the arrival rate. So the plateau is a rate balance rather than a cap, and a
run with a faster leech rate would sit higher, up to 1,024 and no further.

**So the honest statement is a constant, not a curve.** Every tick after the
bound carries about 270 KB, for as long as the process runs: 32 MB an hour at
a 30 second interval, indefinitely, for a 16 MiB payload. The run's whole
output was 151,679,859 bytes over 666 records at
2026-08-24T22:18:14Z, 5.5 hours in.

That is the finding this entry keeps. It is not a leak and it is not
accelerating; it is a fixed and fairly large per-tick cost, paid to re-send
rows for peers that disconnected hours ago. The Approach below is unchanged by
the correction, because what it proposes is that a tick carries the peers
currently connected, and at the last sample that is **zero** of the 873 rows
sent.

The `Prove` section's assertion has to change with it, and the change makes it
easier rather than harder to check: not "the per-record size stops growing
with peers seen", which is already true after two hours, but that **the record
size after the bound engages is a small fraction of what it is today**.

#### Closed, 2026-08-25, on the operator's ruling: a tick carries what is connected

**The ruling accepted the recommendation.** A `seed --jsonl` progress tick
carries the peers the session holds and the final document goes on carrying
every peer it ever held, which is where a caller counting who connected already
looks.

**What "holds" means is not a new idea, it is the event's own counts.**
`swarm::currently_held` drops the two terminal states, `dead` and `not needed`,
and keeps `live`, `connecting` and `queued`. Those three are exactly the
buckets a session reports a number for, so the length of a tick's
`peer_detail` is now `peers.live + peers.connecting + peers.queued` from the
same event. It was a length nothing in the event described.

**Two soaks, twenty minutes each, four leechers, one binary either side of the
one line.** `crates/bit-cli/src/cmd/seed.rs:506` is the line.

```bash
pwsh -NoProfile -File scripts/soak.ps1 -Minutes 20 -Leechers 4
```

`bench/soak-20260825T021602651Z.json` is the before run and
`bench/soak-20260825T023859926Z.json` the after; the stdout figures are the
seeder's own `seed.out`, which `-Keep` leaves behind and the reports do not
carry.

| | before | after |
| --- | --- | --- |
| seeder stdout | **1,046,872 bytes** | **16,993 bytes** |
| progress records | 41 | 41 |
| last record | 50,649 bytes | 410 bytes |
| rows in the last record | 160 | 0 |
| peers seen by then | 160 | 160 |

**1.6 percent of the stdout and 0.8 percent of the last
record.** That is the assertion the correction above asked for: not that the
record stops growing, which it already did after two hours, but that what it
settles at is a small fraction of what it was.

**The run's own numbers do not change**, which is what the soak's ceilings
already assert and what makes this a reporting change rather than a behaviour
one. 160 leech cycles completed before and 156 after, none failed either way,
and both runs saw the same 160 peers.

**The `peers` count in the final document is untouched.** `build` at
`crates/bit-cli/src/cmd/seed.rs:545` takes the whole `view.rows`, so
`peers_seen` and `peers_served` are computed over every peer as before, and so
is the `peers` array a `--json` run prints at the end.

**This is a narrowing and it is a break for one kind of consumer**, which the
entry said and the ruling accepted: a script reading `peer_detail` off a tick
to count total peers gets a smaller number. `peers.seen` is in the same event
and is the count that field never was. `docs/examples/machine-output.md` says
so under "What a tick carries, and what only the final document carries".

**A snapshot of a fast workload is often empty, and that is the honest
answer rather than a defect.** At the last tick of the after run that carried
no rows, `peers.seen` read **160** and `peer_detail` was empty, because
a leech cycle here connects, transfers 16 MiB and leaves inside one 30 second
interval. A cumulative array would have carried 160 rows at that
instant and none of them a peer the seeder was talking to. What a caller
wanting finer grain has is
`--report-interval`, and what a caller wanting the total has is `peers.seen` in
the same event and the `peers` array in the final document.

### T-259 The schema's prose is generated and nothing compares it to what is committed

Source:      found while closing [T-257](#t-257-two-documents-answer-to-type-progress-and-the-guard-against-that-only-covers-documents),
             2026-08-25
Category:    cli
Priority:    P3
Effort:      S
Status:      done, 2026-08-30. Everything that is not a field row is compared
             for equality now, and an edit to `HEADER` that is not regenerated
             turns the tree red.

Problem:     `docs/schema.md` is generated, and the test that keeps it true
             compares **field rows only**. `the_committed_schema_matches_what_the_program_writes`
             at `crates/bit-cli/src/schema_gen.rs:1599` filters both sides to
             lines starting with `` | ` `` before comparing, so every other
             line of the file is outside the check: the header, the "How this
             file is kept true" section, and the one-line description under
             each `###` heading.

             An edit to `HEADER` in `crates/bit-cli/src/schema.rs:431` that is
             not followed by a regeneration therefore never reaches the reader,
             and nothing fails. This session made that edit and found out by
             reading the file afterwards.
Premise:     Measured. Three sentences were added to `HEADER`, `cargo test -p
             bit-cli --lib schema` passed with 18 tests, and `docs/schema.md`
             did not carry them until `BIT_CLI_UPDATE_SCHEMA=1` was run.
Relevance:   It is the same shape as [T-255](#t-255-regenerating-the-schema-deletes-four-hand-written-sections-and-nothing-fails)
             one direction over: that entry was about regeneration **deleting**
             prose nobody generated, and this is about regeneration being the
             only way generated prose arrives, with nothing saying so.

             The cost is bounded and real: the file's own prose is what tells a
             consumer that a `progress` event has two shapes and which command
             writes which field. A stale copy of that describes a contract the
             program does not have, which is what the row check exists to stop.

             `scripts/check-docs.ps1` cannot catch it either. It resolves
             links, flags and output fields; it has no idea the file is
             generated.
Approach:    The row filter is there for a reason and it stays: the check is
             deliberately **containment** over rows, because these runs are
             timed and a download that beats its own report tick emits no
             `progress`. See the comment at
             `crates/bit-cli/src/schema_gen.rs:1630`.

             What can be equality is everything that is **not** a row.
             `HEADER` is a constant, the per-name descriptions come from
             `DOCUMENT_KINDS` and `EVENT_TYPES` at
             `crates/bit-cli/src/schema.rs:33`, and none of them depends on
             what a run happened to produce. So: compare the non-row lines for
             equality and keep the row lines as containment, in the same test.

             The hand-written sections `carry_across` preserves have to stay
             exempt, or the new half fails on the four sections at the end of
             the file that the generator does not produce. `carry_across` at
             `crates/bit-cli/src/schema_gen.rs:1401` already knows which `##`
             headings those are, and the test can ask it the same question.
             It does: that logic is `hand_written_sections` at
             `crates/bit-cli/src/schema_gen.rs:1357` now and both call it.
Prove:       ```
             cargo test -p bit-cli --lib schema
             ```

             A test in the shape of the existing one that fails when a line of
             `HEADER` differs from the committed file and passes when they
             agree, with the hand-written tail exempt. Editing `HEADER` and not
             regenerating must turn the tree red.
Closed:      **2026-08-30**, in
             `the_committed_schema_matches_what_the_program_writes`, which now
             does two comparisons rather than one. Rows stay containment, for
             the reason the comment beside them already gave. Every other line
             is compared for **equality**, after dropping the hand-written
             sections and the table separators.

             **Equality is safe because the prose is not timing dependent, and
             that was checked rather than assumed.** `schema::render` emits a
             `###` section for every name in `DOCUMENT_KINDS` and
             `EVENT_TYPES` whether a sample turned up or not, writing "not
             covered by the generator yet" when one did not. So the section
             list and the descriptions are a function of the constants alone,
             and only the rows depend on what a run produced. The old heading
             check was containment for a hazard that does not exist.

             **`carry_across`'s section logic became
             `hand_written_sections`**, called by the writer and by the check,
             so what is preserved and what is exempt are the same answer to the
             same question rather than two copies that disagree the first time
             a heading changes.

             **The proof is the defect, run.** With `A SENTINEL LINE` inserted
             into `schema::HEADER` and no regeneration, the test fails and
             names the line on both sides:

             ```
             generator: "...prints the version of everything below. A SENTINEL LINE. This file is"
             committed: "...prints the version of everything below. This file is"
             ```

             Eighteen schema tests passed over that same edit before this
             change, which is the entry's premise reproduced one more time.
             Reverted, and the eighteen pass again.

### T-260 A release publishes binaries and nothing a program can consume

Source:      the operator's ruling of 2026-08-29, while ruling on T-244
Category:    ci
Priority:    P2
Effort:      M
Status:      open

Problem:     `.github/workflows/release.yml` builds three targets, attests
             them, and creates the release with
             `gh release create --notes-file CHANGELOG.md`. Three things follow
             from that and all three are wrong for a reader and for a program.

             **The release body is the whole changelog.** `CHANGELOG.md` grows
             with every version, so the notes on `v0.3.0` will carry `v0.2.0`
             and everything before it. Nothing selects the section for the tag
             being released.

             **The checksums are per artifact and there is no manifest.** The
             build job writes `bit-cli-<target>.tar.xz.b3sum` beside each
             archive, which is right, and there is no single file listing all
             of them, so verifying a download means fetching a second asset per
             artifact and no `b3sum -c` over one file is possible. The release
             body names no checksum at all.

             **Nothing but a binary is published.** `fingerprints/` carries
             what this client puts on the wire, as versioned JSON with a
             `schema` field, and it exists only in the source tree. A program
             that wants it has to clone.
Relevance:   The operator's stated direction is that this repository publishes
             data as a service and not only a binary: the fingerprint and
             cipher profiles now, and the merged, liveness-checked,
             latency-ranked tracker list that a later entry adds. Every one of
             those is a file some other tool wants by URL, and a GitHub release
             asset is the cheapest durable URL there is.

             It is also the half of [T-244](#t-244-a-web-page-is-not-a-source-and-nothing-extracts-a-link-from-one)
             that makes its staleness detection worth anything to anybody
             outside this tree. A drift report that only this repository can
             read is a drift report for one consumer.
Approach:    Three changes to `release.yml` and one to `scripts/release.ps1`,
             none of them coupled.

             **The release body is generated.** Take the section for the tag
             out of `CHANGELOG.md`, append a checksum table, and pass that as
             `--notes-file` instead of the whole file. `scripts/release.ps1`
             already writes the section and knows where its heading is, so the
             extraction belongs beside it and gets a `-Section <version>` mode
             rather than a second parser in YAML.

             **One manifest per release.** `B3SUMS` over every published asset,
             built in the `publish` job after the artifacts are downloaded and
             before the release is created, and published as an asset itself so
             `b3sum -c B3SUMS` works against a directory of downloads. The
             per-artifact `.b3sum` files stay: they are what somebody
             downloading one archive wants.

             **The data files are assets.** Everything under `fingerprints/`,
             and whatever the tracker entry adds, uploaded with a stable name
             so a consumer can fetch
             `.../releases/latest/download/fingerprints.json` and never parse
             HTML to find it. One file rather than the directory, assembled at
             publish time, carrying the same `schema` field each source file
             does so a consumer can branch on it.

             **A published schema is a contract.** Say so in
             `docs/schema.md` or a page it links, and give the assembled
             document a `schema_version` that moves when a field changes
             meaning. `bit-cli` already treats its own JSON that way and this
             is the same promise to a different reader.
Acceptance:  A manual `workflow_dispatch` run builds and uploads and creates no
             release, which is the behaviour today and must not change. A run
             on a `v*` tag produces a release whose body is that version's
             changelog section and nothing else, plus a checksum table; a
             `B3SUMS` asset that `b3sum -c` accepts against the other assets;
             and a `fingerprints.json` asset that parses and carries a
             `schema_version`.

             Proved on a real tag in this repository, with the release
             deleted afterwards if it was a rehearsal.
Notes:       `gh release create --verify-tag` and the attestation step are
             unchanged and neither is in scope. Decision 7.5 stands: no
             crates.io credential and no token beyond the built-in
             `GITHUB_TOKEN` appears anywhere in this repository.

             The tracker list this entry publishes does not exist yet.
             [T-261](trackers.md) is where it is specified, and this entry
             does not wait for it: `fingerprints/` alone justifies the
             publishing path and the tracker file drops into it later.

### T-262 The HTTP/2 fingerprint matches a real Chrome in three fields of four

Source:      measured 2026-08-29 by `scripts/check-browser-fingerprint.ps1`,
             while closing T-244
Category:    cli
Priority:    P3
Effort:      S
Status:      done, 2026-08-30. The Akamai fingerprint is a real Chrome's in all
             four fields now, read off the wire and not derived.

Problem:     An Akamai HTTP/2 fingerprint is four fields:
             `SETTINGS|WINDOW_UPDATE|PRIORITY|PSEUDO_HEADER_ORDER`. Captured
             off the wire against `loopback-tlsprobe`, `bit-cli` and a real
             Chrome 151 on the same machine agree on three of them and differ
             on the third:

             ```
             chrome   1:65536;2:0;4:6291456;6:262144|15663105|1:1:0:255|m,a,s,p
             bit-cli  1:65536;2:0;4:6291456;6:262144|15663105|0|m,a,s,p
             ```

             Chrome opens stream 1 with priority information: exclusive, no
             dependency, weight 255. `h2` writes none, and `bit-cli`'s
             fingerprint carries `0` where a browser carries `1:1:0:255`. It is
             the one field of the four that a client comparing the two can
             still tell apart.
Relevance:   [T-244](#t-244-a-web-page-is-not-a-source-and-nothing-extracts-a-link-from-one)
             ships a client whose reason for existing is that an origin
             fingerprinting its callers cannot tell it from a browser. Three
             fields of four is most of the way and it is not all of it.

             It is P3 rather than P2 because the fields that carry the most
             signal are the ones that already match. Priority was deprecated
             in RFC 9113 section 5.3.1, which says a sender SHOULD NOT send
             the PRIORITY frame at all, so a server reading it is reading
             something the current specification tells clients not to emit.
Approach:    The mechanism is the one T-244 already built and proved.
             `vendor/h2/src/ext.rs` carries `PseudoOrder` as a request
             extension; `vendor/h2/src/proto/streams/streams.rs:276` lifts it
             out before the extension map is cleared and hands it to
             `Peer::convert_send_message`. A `StreamPriority` extension takes
             the same three steps.

             What is **not** already there is the encoding.
             `vendor/h2/src/frame/headers.rs:301`, `Headers::encode`, writes
             the frame head and the header block and never writes a priority
             payload: `stream_dep` is parsed on receive and dropped on send.
             Emitting it means setting the PRIORITY flag in the head, writing
             the five byte dependency block ahead of the HPACK block, and
             getting the length right in both the head and any CONTINUATION
             that follows.

             That is a change to a protocol library's frame writer, which is
             the part of `h2` with the least margin for being wrong, and it is
             why this is filed rather than done in the session that found it.

             The alternative shape, a standalone PRIORITY frame on the
             connection before the HEADERS, is what Chrome actually sends and
             is a different seam: it is a connection-level write rather than
             part of converting one request.
Acceptance:  `pwsh scripts/check-browser-fingerprint.ps1 -Strict` exits 0 on a
             machine with Chrome installed, where it exits 1 today naming
             `akamai`. The `known` row for `akamai` in that script is deleted
             in the same change, which is the other half of the rule about a
             check that measures an open defect.

             `cargo test --manifest-path vendor/h2/Cargo.toml --workspace
             --target-dir target/vendor-h2` still passes: the frame writer is
             what this touches and h2's own tests are what cover it.
Closed:      **2026-08-30, and the encoder needed nothing new.**
             `EncodingHeaderBlock::encode` already takes a closure that runs
             after the head and before the header block, which is how
             `PushPromise` writes its promised stream id, and the payload
             length is measured after it runs. So the five bytes are counted in
             the frame length and in any CONTINUATION split without either
             being computed by hand, which is the part the entry expected to be
             delicate.

             What was added: `h2::ext::StreamPriority`, lifted off the request
             extensions in `streams.rs` beside `PseudoOrder`;
             `StreamDependency::encode`, which is the half `load` never had;
             `Headers::set_stream_priority`, which sets the payload **and** the
             flag in one call because a head with the flag and no block is a
             frame a peer cannot parse; and
             `ImpitBuilder::with_http2_stream_priority`.

             **The value lives in `crates/bit-cli-core/src/page.rs`**,
             `BROWSER_H2_STREAM_PRIORITY`, not in the vendored fingerprint
             database. Putting it on `Http2Fingerprint` would have edited
             twenty-seven profile literals that do not use it, so it is a
             builder option beside the two HTTP/2 settings that are there for
             the same reason. [RULES.md](RULES.md) section 6b.

             Measured off the wire rather than asserted:

             | | PRIORITY field |
             | --- | --- |
             | before | `0` |
             | after | `1:1:0:255` |
             | a real Chrome 151, and a real Chrome 152 | `1:1:0:255` |

             The golden moved in that one field and nothing else, which is what
             says the change is the change:
             `fingerprints/bit-cli-browser.json`.

```bash
pwsh -NoProfile -File scripts/check-fingerprint.ps1
```

             `h2`'s own suite holds the frame writer: 437 passed, 1 ignored,
             and the one failure is the upstream flake `PROGRESS.md` already
             names, which passes on its own.

```bash
cargo test --manifest-path vendor/h2/Cargo.toml --workspace --target-dir target/vendor-h2
```

             Two new tests in `vendor/h2/src/frame/headers.rs` assert the wire
             bytes directly, `80 00 00 00 ff` after a nine byte head with the
             PRIORITY flag set, and that a frame given no priority carries
             neither the flag nor the block.
Notes:       The check recorded this and did not judge it while it was open,
             following `scripts/check-close-wait.ps1`'s pattern. **The
             exemption came off with the entry**, which is the other half of
             that rule: `$known` in
             `scripts/check-browser-fingerprint.ps1` is empty now and a row
             added back has to name the entry that owns it.

             **`StreamPriority::parse` was written and then removed.** It took
             the Akamai third field's `<exclusive>:<dependency>:<weight>` shape
             and nothing called it, because the value comes from a typed
             constant rather than from text. An untested public parser in a
             protocol library is machinery with no caller.

### T-263 The extension list is Chrome's set in a fixed order, and Chrome shuffles it

Source:      measured 2026-08-29 with `loopback-tlsprobe --raw --hello-out`,
             while closing T-244
Category:    cli
Priority:    P3
Effort:      M
Status:      done, 2026-08-30. GREASE at both ends at a codepoint chosen per
             connection, and the order permuted per connection.

Problem:     `bit-cli`'s browser profile and a real Chrome 151 on the same
             machine produce the **same JA4**,
             `t13i1515h2_8daaf6152771_806a8c22fdea`. They do not put the same
             bytes on the wire, and JA4 cannot see the difference because it
             sorts both lists and strips GREASE before hashing.

             The cipher list is Chrome's exactly, in Chrome's order, GREASE
             included. The extension list is not:

             ```
             chrome   6a6a,0005,0033,000a,44cd,0023,002d,ff01,001b,000d,000b,0017,fe0d,002b,0012,0010,4a4a
             bit-cli  ff01,000b,44cd,0017,0023,000d,0005,000a,0010,0012,0033,002d,002b,001b,fe0d
             ```

             Two differences, and both are visible to anything that reads the
             raw hello rather than a hash:

             - **Chrome sends GREASE at both ends of its extension list** and
               `bit-cli` sends none. Chrome sends it in the cipher list too,
               and there `bit-cli` does.
             - **Chrome's order is shuffled per connection** and `bit-cli`'s
               is fixed. Chrome has shuffled its extensions since 110; the
               shuffle is why JA4 sorts at all.

             **Reproduced on Chrome 152 on 2026-08-30**, from a raw hello
             captured in a container by
             [T-264](#t-264-the-browser-profile-can-only-be-refreshed-on-a-machine-that-runs-that-browser),
             so this is two versions and two platforms rather than one
             reading. The wire order, with the GREASE values it chose that
             connection:

             ```
             3a3a GREASE  0023  ff01  002d  000b  000a  12e0  0017  0010
             ca34  fe0d  0033  001b  000d  0012  44cd  002b  0005  5a5a GREASE
             ```

             Two things that reading settles which the 151 one left open.
             **The GREASE values differ at the two ends**, `0x3a3a` and
             `0x5a5a`, so this is two extensions with two chosen codepoints
             rather than one repeated. And **`alpn` is inside the shuffle**, at
             position 9, so it is not one of the extensions Chrome pins.
Relevance:   [T-244](#t-244-a-web-page-is-not-a-source-and-nothing-extracts-a-link-from-one)
             ships a client whose reason for existing is that an origin
             fingerprinting its callers cannot tell it from a browser. An
             origin comparing JA4 cannot. One comparing the raw hello can, and
             a client whose extension order never changes is more
             distinguishable than one that shuffles, not less: the fixed
             sequence is itself the signal.

             It is P3 because the fingerprint that is actually published,
             compared and sold as a service is JA4, and that one matches
             exactly. This is the layer below it.
Approach:    **The list itself is `crates/bit-cli-core/src/page.rs`'s now**,
             `BROWSER_EXTENSION_ORDER`, moved there by
             [T-264](#t-264-the-browser-profile-can-only-be-refreshed-on-a-machine-that-runs-that-browser).
             What still lives in apify's `rustls` fork is the encoder that
             turns it into a `ClientHello`,
             `vendor/rustls/rustls/src/crypto/emulation/mod.rs` and
             `vendor/rustls/rustls/src/msgs/handshake.rs`. Two changes, and the
             second is the harder one.

             **The shuffle is half built already and was not noticed.**
             `ClientExtensions::order_insensitive_extensions_in_random_order`
             sorts by a hash of a per-connection `order_seed`, and
             `used_extensions_in_encoding_order` emits those first and the
             `contiguous_extensions` list after them. Today the fingerprint
             names **every** extension in `extension_order`, so the random set
             is empty and nothing shuffles. Naming fewer of them is most of
             this entry.

             **The GREASE half needs a new shape rather than a new value.**
             `ClientExtensions` has one typed field per extension type and one
             GREASE slot, `reserved_grease: Option<()>` at the fixed codepoint
             `0xbaba`, set from `TlsExtensionsConfig::grease`. Chrome sends
             **two**, at two different chosen codepoints, one first and one
             last, which that shape cannot express. So this needs either two
             fields carrying a codepoint each or a general unknown-extension
             list, and it is why the entry is `M`.

             **GREASE at both ends** is a value from the sixteen the
             specification reserves, chosen per connection, added first and
             last. RFC 8701 is the specification and it exists precisely so a
             server tolerates them.

             **A shuffled order** means the fingerprint carries a list and the
             handshake permutes it per connection, keeping the two extensions
             the specification pins: `pre_shared_key` must be last, and this
             tree does not send one.

             `scripts/check-fingerprint.ps1` asserts JA4 and JA4_r, both of
             which sort, so neither moves when this lands. Nothing in the
             goldens has to change, which is the argument for doing it: the
             assertion that would catch a mistake is already there and already
             insensitive to the fix.
Acceptance:  `loopback-tlsprobe --raw` reports a `JA4_ro` whose extension
             segment differs between two consecutive captures of `bit-cli`,
             where it is identical today, and whose first and last extension
             are GREASE.

             `pwsh scripts/check-fingerprint.ps1` still passes with the
             goldens unchanged, which is what says the sorted forms did not
             move.

             The `!browser.extensions.iter().any(is_grease)` assertion in
             `crates/bit-cli-core/examples/loopback-tlsprobe/tlsfp.rs` is
             inverted rather than deleted, the way
             `scripts/check-listener.ps1`'s cases were when
             [T-020](peers.md) closed.
Closed:      **2026-08-30, and the shuffle half was already built.**
             `ClientExtensions::order_insensitive_extensions_in_random_order`
             sorts by a hash of a per-connection `order_seed` and
             `used_extensions_in_encoding_order` emits those before the
             `contiguous_extensions` list. Naming **every** extension in
             `extension_order` left the random set empty, so nothing moved. The
             fix is that `BROWSER_EXTENSION_ORDER` in
             `crates/bit-cli-core/src/page.rs` is now **empty**: nothing is
             pinned, and the handshake permutes the list. No rustls code was
             needed for that half at all.

             **The GREASE half needed a new shape**, as the entry predicted.
             `ClientExtensions` has one typed field per extension type and one
             GREASE slot at the fixed `0xbaba`, and a browser sends two at two
             codepoints it picks per connection. So `vendor/rustls` gained
             `ReservedGreaseFirst` and `ReservedGreaseLast`, whose enum
             codepoints are placeholders, and a `grease_codepoints` field
             carrying the pair actually written; `encode` writes those two
             directly, because `encode_one` takes the codepoint from the type.
             The last one is placed **before** any PSK offer, because RFC 8446
             requires `pre_shared_key` last.

             **The bodies were measured rather than chosen.** From a real
             Chrome's hello: the first GREASE extension has an empty body and
             the last has a single zero byte. That is why they are two fields
             rather than one repeated. The codepoints come from the provider's
             secure random, mapped onto the sixteen RFC 8701 reserves, and are
             drawn **distinct**, because the same value at both ends is a
             constant a server can key on.

             Two consecutive captures of the same binary, off the wire:

             | | first | last | order |
             | --- | --- | --- | --- |
             | capture 1 | `0x6a6a` | `0x7a7a` | one permutation |
             | capture 2 | `0x7a7a` | `0x0a0a` | a different one |
             | before | none | none | fixed, every time |

Acceptance
run:         **The goldens did not move**, which is what the entry predicted and
             is the argument for having done it: JA4 and JA4_r both sort and
             strip GREASE, so the assertion that would catch a mistake was
             already there and already insensitive to the fix.
             `check-fingerprint.ps1` passes with
             `t13i1515h2_8daaf6152771_806a8c22fdea` unchanged.

```bash
pwsh -NoProfile -File scripts/check-fingerprint.ps1
```

             `crates/bit-cli-core/examples/loopback-tlsprobe/tlsfp.rs` carries a
             fresh recorded hello and the
             `!browser.extensions.iter().any(is_grease)` assertion is
             **inverted rather than deleted**, the way
             `scripts/check-listener.ps1`'s cases were when
             [T-020](peers.md) closed. It now asserts that the first and last
             extensions are both GREASE, that they differ, and that there are
             exactly two.
Correction:  **The first version of this shipped a defect and CI caught it, at
             run 33289807801.** `The fingerprint against its golden` failed
             with an empty Akamai fingerprint on `ubuntu-latest` where every
             local run had passed.

             The cause is the read side rather than the write side.
             `ReservedGreaseFirst` and `ReservedGreaseLast` were given real
             GREASE codepoints as placeholders, `0x0a0a` and `0x1a1a`, so they
             are also what a **received** hello's GREASE extension decodes
             into. Their bodies were typed `Option<()>`, which reads an empty
             body and nothing else. RFC 8701 lets a client put any body in a
             GREASE extension, and this client puts one zero byte in the one
             at the back. When the per-connection draw put that extension on
             `0x0a0a`, `0x1a1a` or the pre-existing `ReservedGrease` at
             `0xbaba`, the server rejected the hello:
             `TrailingData("Empty")`. Three values in sixteen, so about one
             handshake in five, which is why every local run passed and CI did
             not.

             **The next CI run passed over the same defect**, at 33290401084,
             which is what says it was a rate rather than a break. A check that
             makes one handshake fails about one time in five, and a session
             that read a single green run would have concluded the first
             failure was noise.

             All three fields carry `Option<Payload>` now, so any body reads.
             **The defect was also in the pre-existing `0xbaba` field**, which
             this entry did not add: a rustls server built from this fork
             rejected a real browser's hello whenever its GREASE landed there.

             Measured with `bit-cli` driven at the probe with the CA trusted,
             counting connections that reached HTTP/2. **The middle row is the
             state after two of the three fields were fixed**, which is why its
             rate is a sixteenth rather than three, and it is recorded rather
             than dropped because it is what pointed at `0xbaba`:

             | state | broken codepoints | handshakes | reached HTTP/2 | failed |
             | --- | --- | --- | --- | --- |
             | as shipped | 3 of 16 | one CI run | — | that run |
             | two fields fixed | 1 of 16 | 29 | 27 | 2 |
             | all three fixed | 0 | 64 | 64 | **0** |

             The as-shipped rate was not measured over many handshakes, because
             it was found by a CI failure rather than by a sweep. `3/16` is
             arithmetic from the three codepoints, and `2/29` against a
             predicted `1/16` is the measurement that confirms the model.

             The regression test is
             `a_grease_extension_with_a_body_reads_at_any_reserved_codepoint`
             in `vendor/rustls/rustls/src/client/hs.rs`, which reads a
             one-byte-bodied GREASE extension at each of the sixteen reserved
             values. It cannot be run here, because rustls's own suite needs a
             `test-ca/` tree this repository does not vendor; the wire
             measurement above is what holds it.

             **The check that missed it is repaired too, and that is the part
             worth keeping.** `scripts/check-fingerprint.ps1` made **one**
             handshake, so it sampled one draw of sixteen and a
             three-in-sixteen defect reached it four times in five. It makes
             eight now, `-Handshakes` sets it, and every one of them has to
             reach HTTP/2. Three consecutive runs pass.

             **A second finding came out of doing that**, and it inverted the
             assertion that was written first. Requiring every capture to be
             identical fails, and correctly: over eleven captures of one
             binary, **eight carried `session_ticket` and three carried
             `pre_shared_key`**, because the connection resumed, and the two
             produce different JA4s. That is the client telling the truth and
             it is what a real Chrome does. So the cold capture is the one
             compared, which is the first, and the captures after it are read
             only for whether they reached HTTP/2 at all.
Notes:       `JA4_ro` is what made this visible and it was added in the same
             session, for exactly this: JA4 and JA4_r sort, so they say two
             clients are the same when their wire order says otherwise. It is
             recorded and never asserted, for the same reason JA3 is not.

             **One difference from a real Chrome is left and it is not this
             entry's.** The vendored rustls forces `encrypted_client_hello`
             second to last, and the Chrome 152 capture has it at position 11,
             in the middle of the shuffle. That is a placement rule in
             `used_extensions_in_encoding_order` rather than anything the
             profile chooses, and it is invisible to JA4 for the same reason
             the order was. It is recorded here rather than filed, because it
             is one extension's position and the entry that would own it is a
             capture of a browser this repository can now take on demand.

### T-264 The browser profile can only be refreshed on a machine that runs that browser

Source:      the operator's ruling of 2026-08-29, after T-244 closed a major
             behind stable
Category:    cli
Priority:    P2
Effort:      M
Status:      partial, 2026-08-30. Three of the four pieces are done and the
             fourth, the bump, is blocked on two TLS extensions this stack
             cannot emit. The blocker is named under "What the capture found"
             and it is measured rather than predicted.

Problem:     `crates/bit-cli-core/src/page.rs` pins the profile to Chrome 151
             and the TLS half of it comes from
             `vendor/impit/impit/src/fingerprint/database/chrome.rs`, whose
             newest entry is also 151. Chrome stable is **152**.
             `scripts/check-browser-version.ps1` reports the gap and refuses to
             recommend past the database, which is correct and is as far as it
             can go: it cannot produce a `ClientHello` from a version number.

             **Refreshing the profile needs a machine running the browser**,
             and there is no such machine. This host has Chrome 151.0.7922.76.
             The `ubuntu-latest` runner has 151.0.7922.173, measured in run
             33251738663, so CI cannot supply it either.
Relevance:   The operator's ruling, and it settles two things.

             **Upstream is not the authority; the measurement is.** `impit`'s
             database has already been found wrong here: it carries no
             `SETTINGS_HEADER_TABLE_SIZE` at all, its
             `initial_connection_window_size` is the window rather than the
             increment, and its Akamai fingerprint was claimed by the survey to
             be profile-invariant when its own entries disagree with each
             other. `scripts/upstream-scan.ps1` says when a release appears;
             what says whether it is right is a capture.

             **A derivable value is still not an acceptable value.** Chrome
             computes its `sec-ch-ua` brand list from the major version by an
             algorithm in Chromium's own source, so it is derivable in
             principle. It is refused on principle anyway: `bit-cli` cannot
             vendor Chromium, would be that port's only consumer, and a
             reimplementation that drifts is a profile that is wrong in a way
             nothing here would notice. Everything the profile claims is
             measured off a browser or it does not ship.
Approach:    **A browser installed in a throwaway container, driven at
             `loopback-tlsprobe`, and destroyed.** Nothing is installed on the
             host, and the container is a second measurement beside CI rather
             than a replacement for it.

             Four pieces, and three of the four are already measured on this
             machine.

             1. **Where the browser comes from, and Google publishes it
                itself.** Three sources were compared rather than one
                assumed, on 2026-08-29:

                | source | what it gives | notes |
                | --- | --- | --- |
                | **Chrome for Testing** | an exact build per channel, with a download URL and a machine-readable index | Google's own, versioned, made for automation. `Stable` 152.0.7977.64, `Beta` 153.0.8010.12, `Dev` and `Canary` beyond. **This is the one to use.** |
                | `debian:bookworm-slim` + Google's apt | 152.0.7977.64 | current stable, and proves the container path works |
                | a third-party image, `selenium/standalone-chrome` or `mcr.microsoft.com/playwright` | a Chrome somebody else chose | reputable, but the version is theirs and lags |

                **Chrome for Testing answers the question the entry was
                filed on**: it is Google publishing Chrome, per channel,
                addressable by version, so a capture is not limited to
                whatever a distribution happens to package. It also reaches
                **Beta, Dev and Canary**, which is how the profile gets ahead
                of a release rather than behind it: capture Beta now and the
                bump is ready the day it ships.

                The container is still where it runs. Downloading a browser
                onto the host is a system change nobody asked for; downloading
                it into a distro that is destroyed afterwards is not.
                [`docs/containers.md`](../docs/containers.md) is the
                procedure.

             2. **A reachable probe, and this is the piece that needs code.**
                WSL is in NAT mode on this host, so a distro cannot reach the
                Windows loopback and `loopback-tlsprobe` binds `127.0.0.1`
                only. **Measured**: the distro reaches the host at the WSL
                adapter address, `172.23.96.1`, and a listener bound there
                accepts the connection.

                So the probe takes `--bind <ADDR>`, defaulting to `127.0.0.1`
                and changed by nothing but this. The capture binds it to the
                WSL adapter address, which is a Hyper-V internal network and is
                not reachable from the LAN. `0.0.0.0` is not the answer and
                should not be offered.

                **`--bind` is worth having whatever the container tooling
                does**, because it is this repository's own fixture and a
                fixture that can only be reached from the machine it runs on
                cannot be reached by anything else either. What the tooling
                could remove is the **address lookup**, and that ask is
                written down under Notes.

             3. **The capture.** `scripts/check-browser-fingerprint.ps1`
                already drives a browser at the probe and already prints the
                replacement values. What it needs is a `-Container` switch that
                puts the browser in a distro rather than on the host, and a
                `--path`-equivalent inside it.

             4. **The bump, and the operator ruled on where it lands.**
                **The whole profile moves into `crates/bit-cli-core/src/page.rs`**:
                the cipher list, the key exchange groups, the signature
                algorithms, the extension list and its order, the HTTP/2
                settings and the headers. `page.rs` then constructs `impit`'s
                `BrowserFingerprint` from its own values, where today it takes
                `chrome_151::fingerprint()` and overwrites the header half.

                That is the change with the most leverage in this entry and it
                is why the entry is `M` rather than `S`. After it, a bump edits
                **one file** this repository owns, a staleness recommendation
                names that file and nothing else, and `vendor/impit` carries no
                data this repository authored. It follows directly from
                [RULES.md](RULES.md) section 6b: a starting point does not get
                to be the home of the answer.

                The mapping is onto `impit`'s enums, `CipherSuite`,
                `KeyExchangeGroup`, `SignatureAlgorithm` and `ExtensionType`,
                which stay theirs. A value those enums cannot express is a
                finding to record rather than a value to drop silently.

                **Stable is what the profile claims.** A capture can reach
                Beta, Dev and Canary; shipping one of those is the same failure
                as shipping a version nobody runs yet. Beta is captured and
                written down beside it so the next bump is ready the day it
                ships.

                `scripts/check-fingerprint.ps1 -Update` then rewrites the
                goldens and `check-browser-fingerprint.ps1 -Strict` verifies
                against the browser again.
Acceptance:  `pwsh scripts/check-browser-fingerprint.ps1 -Container` on a
             machine with podman and WSL2 reports a browser newer than
             `BROWSER_MAJOR`, prints the replacement, and leaves **no**
             registered distro and no orphaned rootfs tarball behind:
             `wsl-ephemeral.ps1 -Action List` says `(none)` afterwards.

             With no container engine it exits **2** and says which piece is
             missing, the same way the host path exits 2 with no browser. A
             machine with neither is not a failing build.

             After the bump, `pwsh scripts/check-browser-version.ps1` reports
             the profile at the same major as Stable, and
             `pwsh scripts/check-browser-fingerprint.ps1 -Strict` passes
             against a browser of that version.

             **`vendor/impit` carries no fingerprint this repository authored**,
             which `pwsh scripts/vendor-diff.ps1 -Check` proves by the series
             for `impit` not gaining a patch to
             `impit/src/fingerprint/database/chrome.rs`.
What the
capture
found:       **Done on 2026-08-30**, in the order the work order gave.

             1. **`--bind` on `loopback-tlsprobe`**, defaulting to `127.0.0.1`.
                The leaf certificate carries whatever it names, so a client
                that trusts `--ca-out` still verifies the name it dialled, and
                the announced URL carries the bound host rather than a literal.
                A hostname and the unspecified address are both refused by
                name: the certificate needs a literal, and a fixture that
                terminates TLS and records header values does not belong on
                every interface. Measured reaching `172.23.96.1:59629` from
                the host and from a distro.
             2. **`scripts/wsl-tool.ps1` and `scripts/toolkit-pin.json`.** The
                tooling is pinned to a commit with both digests recorded, and
                one script resolves it, verifies it and forwards everything
                else. Written because the launcher's resolution order made the
                pin silently ineffective; see Notes.
             3. **`scripts/check-browser-fingerprint.ps1 -Container`**, which
                installs a browser in a throwaway `debian:bookworm-slim`
                distro, drives it at the probe on the address
                `wsl-tool.ps1 -Action HostAddress` prints, and removes the
                distro in the same run, reading the state back rather than
                trusting it. `-Source cft` takes a Chrome for Testing build of
                `-Channel Stable|Beta|Dev|Canary`; `-Source apt` takes Google's
                branded stable package. Evidence:
                `bench/browser-fingerprint-cft-152.json`.
             4. **The whole profile is in `page.rs`.** Ciphers, groups,
                signature algorithms, extension order, ALPN, the HTTP/2
                settings, the pseudo-header order and the headers, with
                `page::browser_fingerprint()` constructing `impit`'s type from
                them. `vendor/impit`'s database is no longer read by anything
                that ships. **The move is behaviour neutral and that is
                measured**, not assumed: `scripts/check-fingerprint.ps1` passes
                with the goldens untouched, same JA4, same header order.

             **The bump is what is blocked, and the container is what proved
             it.** Chrome for Testing Stable 152.0.7977.64, captured in a
             distro and read off the wire:

             | | 151, this host | 152, in the container |
             | --- | --- | --- |
             | JA4 | `t13i1515h2_8daaf6152771_806a8c22fdea` | `t13i1517h2_8daaf6152771_4980c97edce0` |
             | Akamai | `...\|15663105\|1:1:0:255\|m,a,s,p` | **the same** |
             | header order | `accept-language` twelfth | `accept-language` **fourth** |

             **Two extensions are new in 152 and neither can be emitted here.**
             From the raw hello, `bench/browser-fingerprint-cft-152.json`'s
             `recommend.hello`, decoded in wire order:

             ```
             3a3a GREASE  0023  ff01  002d  000b  000a  12e0  0017  0010
             ca34  fe0d  0033  001b  000d  0012  44cd  002b  0005  5a5a GREASE
             ```

             `impit`'s `ExtensionType` names neither, and `vendor/rustls`'s
             `ClientExtensions` has a typed field per extension type and so has
             nowhere to put either. A profile claiming 152 without them is a
             `ClientHello` that exists nowhere, which [RULES.md](RULES.md)
             section 6b says is a stronger tell than being one version behind.
             **So the profile stays at 151 deliberately**, and that is the
             ruling applied rather than a deferral.

             **The two bodies were read as well as the codepoints, and they are
             not the same problem.**

             | codepoint | length | body |
             | --- | --- | --- |
             | `0x12e0` | 2 | `0000` |
             | `0xca34` | 206 | a length-prefixed list of 24 identifiers |

             `0x12e0` is two zero bytes and is reproducible by anything that
             can write an extension. `0xca34` is the trust anchors draft, and
             its body is **a snapshot of the browser's own root store**:
             twenty-four identifiers, each a length-prefixed value, in an order
             that is the browser's. That is not a constant to copy. It changes
             when Chrome's root store changes, which is on its own schedule,
             and a client that carries one build's list is advertising exactly
             which build it copied. This repository has no root store to
             enumerate and cannot generate the list honestly.

             **So the blocker is one extension rather than two**, and it is a
             harder one than a missing enum variant: emitting `0xca34` needs a
             decision about what a client with no root store of its own should
             put in it, and the honest answers are to omit it, which is what
             happens today, or to carry a captured list and accept that it
             ages. That is a question for the operator rather than a defect,
             and it is raised in `PROGRESS.md`.

             **The header half needs a second capture whatever happens to the
             TLS half.** Chrome for Testing is unbranded: its `sec-ch-ua` is
             `"Not?A_Brand";v="24", "Chromium";v="152"` with no Google Chrome
             entry at all, and a Linux container reports
             `sec-ch-ua-platform: "Linux"` and a Linux User-Agent. `-Source
             apt` is what reaches a branded build and it reaches stable only.
             The one header change that is platform independent and already
             measured is the **order**: 152 moves `accept-language` from
             twelfth to fourth.

             What is left, in order: add `0x12e0` with its two zero bytes to
             `impit`'s `ExtensionType` and to `vendor/rustls`'s extension
             encoder, for which
             [T-263](#t-263-the-extension-list-is-chromes-set-in-a-fixed-order-and-chrome-shuffles-it)'s
             two GREASE slots are the worked example; get a ruling on `0xca34`;
             capture a branded 152 with `-Source apt`; then bump.

Notes:       **The distro is removed in the same run that made it**, with
             `-Ephemeral` or an explicit `Remove`, and the acceptance checks
             that rather than trusting it. A session that leaves one behind has
             left a VHDX of a few hundred MiB on somebody's disk. Measured
             when a killed run left one: `Purge` removed a registered distro
             and a 74.3 MiB orphaned tarball.

             **Both asks made of `wsl-ephemeral.ps1` were answered**, and the
             tool is what changed rather than this entry.

             - **`-Action HostAddress` exists.** It prints the address a distro
               reaches this host at for the current networking mode, on stdout
               alone, without creating a distro. Measured here as
               `172.23.96.1`, agreeing with what `/proc/net/route` said inside
               a real distro. Every caller's little-endian hex decoding goes.
             - **`-PortForward` was asked for and refused**, with the
               documentation the ask offered as its alternative. Forwarding a
               port on Windows needs an elevated session and leaves a rule on
               the machine after the tool exits. `HostAddress` plus `--bind` is
               the answer and it is what this entry uses.

             **The pin was silently ineffective and that is why
             `wsl-tool.ps1` exists.** The launcher resolves a local path, then
             a `wsl-ephemeral.ps1` **beside** it, then the pinned ref, first hit
             winning. With the previous session's copy still in `.tmp/`, a run
             passing both `-LauncherRef` and `-LauncherSha256` printed
             `Using the copy beside this launcher`, ran the stale file and
             verified nothing; the stale copy had no `HostAddress`, so it
             surfaced as a `ValidateSet` error about an argument that does
             exist. `wsl-tool.ps1` keeps its cache holding the launcher alone
             and removes any sibling first.

             **A browser opens sockets it abandons, so `--once` is the wrong
             stop condition.** Driving Chrome 152 at the probe produced 13
             connections: the first carried no HTTP/2 at all, and every one
             after the second carried `pre_shared_key` because the session
             resumed. `--until-h2` stops at the first connection that reached
             HTTP/2, and the script picks that capture rather than the first or
             the last line.

             **Chrome on Linux does not read `~/.pki/nssdb`.** Adding the
             probe's authority with `certutil -t "C,,"` and seeing `certutil
             -L` list it still produced `CertificateUnknown`, because Chrome
             uses its own root store there. The container capture therefore
             passes `--ignore-certificate-errors --test-type` **to the
             browser**, which is the argument `browser-capture` already made
             and which nothing that ships carries.

             [T-262](#t-262-the-http-2-fingerprint-matches-a-real-chrome-in-three-fields-of-four)
             and [T-263](#t-263-the-extension-list-is-chromes-set-in-a-fixed-order-and-chrome-shuffles-it)
             are the two differences a capture of Chrome 151 already found.
             Both are edits to the vendored `rustls` and `h2` rather than to
             the fingerprint database, and neither is blocked by this entry.
             **The 152 capture reproduces both on a second version**: the
             Akamai PRIORITY field is `1:1:0:255` there too, and the raw hello
             carries GREASE at both ends, `0x3a3a` first and `0x5a5a` last,
             over a shuffled order.
