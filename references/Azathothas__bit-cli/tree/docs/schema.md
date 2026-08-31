# The JSON contract

`bit-cli --schema-version` prints the version of everything below. This file is
what that number refers to.

Two surfaces, and they never mix. `--json` writes one document to stdout when
the run ends. `--jsonl` writes one object per line as things happen. stdout
carries data only in both, at every log level, so `bit-cli ... --json | jq`
never sees a log line.

Every document carries four fields before its own: `schema_version`,
`bit_cli_version`, `generated_at`, and `kind`. Every event carries `type`,
`seq`, and `at`.

A `bench` report is the exception, and it is the only one. It carries `kind`
and a `report_version` of its own, because `--baseline` reads a report written
by an older build and has to know which format it is holding. Its `environment`
object is not listed below either: that describes the machine a run was taken
on, and it carries fields one platform has and another does not. See
`TODO/bench.md`, T-189.

Sizes and durations are always an integer plus a rendered string, never the
string alone: `{"bytes": 1048576, "human": "1.00 MiB"}` and
`{"ms": 1500, "human": "1s"}`. Rates use the same shape as a size with
`MiB/s` in the string. Timestamps are ISO 8601 UTC with millisecond precision.

## How this file is kept true

It is generated from what the program actually writes. A test drives every
command, flattens the JSON it produced, renders this file, and fails when the
result differs from what is committed. A field added to a report therefore
fails the build until this file is regenerated:

```bash
BIT_CLI_UPDATE_SCHEMA=1 cargo test -p bit-cli --lib schema
```

A field that a given run did not produce is not listed. Optional fields are
omitted from the JSON rather than written as `null`, so a reader cannot mistake
"not applicable" for "none", and several runs of the same command are folded
together here to cover as many of them as possible.

**An event `type` can have more than one shape and a document `kind` cannot.**
`bit-cli seed --jsonl` and `bit-cli download --jsonl` both write
`type: "progress"`, and the section they share differs in fifteen of its
thirty-two rows. Six of those fifteen are `--listener-check`'s, which the run
behind that section passes and an ordinary seeder does not. Those
sections carry a third column saying which command writes each field, and name
every command above the table rather than one of them. A `kind` two commands
claimed would describe a document neither one writes, so the generator refuses
it instead. See `TODO/cli-surface.md`, T-257.

The check is containment, not equality: a row this file has and a run did not
produce passes, because these runs are timed and a failure-only field like
`sources[].error` appears only when a source fails.

**Regenerating adds and never removes.** It unions this file's rows with the
run's, and it carries across every `##` section the generator does not produce,
which is what keeps the four hand-written sections at the end of this file. A
second run in a row changes nothing.

Removing something is therefore deliberate, and it is a one-way door: a row
taken out of this file that no run produces does not come back, because there
is no automatic way to tell a stale row from a rare one. The way to check a
rare row is still real is to produce it.

## Documents

One document per run, on stdout, when `--json` is given.

### `info`

One torrent's metadata, without touching the network.

From `bit-cli info <TORRENT> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `file_count` | integer |
| `generated_at` | string |
| `http_seeds[]` | array |
| `info_hash` | string |
| `kind` | string |
| `magnet` | string |
| `multi_file` | bool |
| `name` | string |
| `name_encoding.detected` | string |
| `name_encoding.utf8_keys` | bool |
| `nodes[]` | array |
| `piece_count` | integer |
| `piece_length.bytes` | integer |
| `piece_length.human` | string |
| `private` | bool |
| `schema_version` | string |
| `source_kind` | string |
| `total.bytes` | integer |
| `total.human` | string |
| `trackers[]` | array |
| `trackers[][]` | string |
| `web_seeds[]` | string |

### `files`

The files in a torrent, with sizes, offsets, and piece ranges.

From `bit-cli files <TORRENT> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `file_count` | integer |
| `files[].first_piece` | integer |
| `files[].index` | integer |
| `files[].last_piece` | integer |
| `files[].offset` | integer |
| `files[].padding` | bool |
| `files[].path` | string |
| `files[].share` | string |
| `files[].shared[].bytes_proven.bytes` | integer |
| `files[].shared[].bytes_proven.human` | string |
| `files[].shared[].evidence` | string |
| `files[].shared[].index` | integer |
| `files[].shared[].info_hash` | string |
| `files[].shared[].path` | string |
| `files[].shared[].pieces_compared` | integer |
| `files[].shared[].proven` | bool |
| `files[].shared[].torrent` | string |
| `files[].size.bytes` | integer |
| `files[].size.human` | string |
| `generated_at` | string |
| `info_hash` | string |
| `kind` | string |
| `name` | string |
| `name_encoding.detected` | string |
| `name_encoding.utf8_keys` | bool |
| `schema_version` | string |
| `total.bytes` | integer |
| `total.human` | string |

### `tree`

The torrent's directory structure, rolled up. The nodes are a flat list in pre-order rather than a nested one, so a field sits at the same path whatever its depth. See `TODO/metainfo.md`, T-249.

From `bit-cli tree <TORRENT> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `depth_limit` | integer |
| `directory_count` | integer |
| `file_count` | integer |
| `generated_at` | string |
| `info_hash` | string |
| `kind` | string |
| `max_depth` | integer |
| `name` | string |
| `name_encoding.detected` | string |
| `name_encoding.utf8_keys` | bool |
| `nodes[].depth` | integer |
| `nodes[].directory_count` | integer |
| `nodes[].file_count` | integer |
| `nodes[].first_piece` | integer |
| `nodes[].hidden.directories` | integer |
| `nodes[].hidden.files` | integer |
| `nodes[].index` | integer |
| `nodes[].kind` | string |
| `nodes[].last_piece` | integer |
| `nodes[].name` | string |
| `nodes[].path` | string |
| `nodes[].shared_pieces` | integer |
| `nodes[].size.bytes` | integer |
| `nodes[].size.human` | string |
| `padding_count` | integer |
| `padding_total.bytes` | integer |
| `padding_total.human` | string |
| `schema_version` | string |
| `total.bytes` | integer |
| `total.human` | string |

### `magnet`

A magnet URI built from a torrent, and its parts.

From `bit-cli magnet <TORRENT> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `generated_at` | string |
| `info_hash` | string |
| `kind` | string |
| `length.bytes` | integer |
| `length.human` | string |
| `magnet` | string |
| `name` | string |
| `peers[]` | array |
| `schema_version` | string |
| `selected_files[]` | array |
| `trackers[]` | string |
| `web_seeds[]` | string |

### `verify`

What a hash check of existing data found, piece by piece.

From `bit-cli verify <TORRENT> --select-file <INDEX> --per-piece --json`.

| field | type |
| --- | --- |
| `bad_pieces[]` | array |
| `bit_cli_version` | string |
| `complete` | bool |
| `data_dir` | string |
| `files[].expected.bytes` | integer |
| `files[].expected.human` | string |
| `files[].found.bytes` | integer |
| `files[].found.human` | string |
| `files[].index` | integer |
| `files[].path` | string |
| `files[].present` | bool |
| `generated_at` | string |
| `have.bytes` | integer |
| `have.human` | string |
| `have_share` | string |
| `info_hash` | string |
| `kind` | string |
| `name` | string |
| `not_selected[]` | integer |
| `per_piece[].bytes` | integer |
| `per_piece[].not_selected` | bool |
| `per_piece[].ok` | bool |
| `per_piece[].piece` | integer |
| `piece_count` | integer |
| `pieces_bad` | integer |
| `pieces_ok` | integer |
| `schema_version` | string |
| `selected.bytes` | integer |
| `selected.human` | string |
| `total.bytes` | integer |
| `total.human` | string |

### `hash_mismatch`

The document `verify` writes instead when a piece did not check out.

From `bit-cli verify <TORRENT> --dir <DIR> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `code` | integer |
| `context.bad_pieces[]` | integer |
| `context.pieces_bad` | integer |
| `context.pieces_ok` | integer |
| `context.report.bad_pieces[]` | integer |
| `context.report.complete` | bool |
| `context.report.data_dir` | string |
| `context.report.files[].expected.bytes` | integer |
| `context.report.files[].expected.human` | string |
| `context.report.files[].found.bytes` | integer |
| `context.report.files[].found.human` | string |
| `context.report.files[].index` | integer |
| `context.report.files[].path` | string |
| `context.report.files[].present` | bool |
| `context.report.have.bytes` | integer |
| `context.report.have.human` | string |
| `context.report.have_share` | string |
| `context.report.info_hash` | string |
| `context.report.name` | string |
| `context.report.piece_count` | integer |
| `context.report.pieces_bad` | integer |
| `context.report.pieces_ok` | integer |
| `context.report.renamed[].disk_path` | string |
| `context.report.renamed[].index` | integer |
| `context.report.renamed[].reasons[]` | string |
| `context.report.renamed[].torrent_path` | string |
| `context.report.total.bytes` | integer |
| `context.report.total.human` | string |
| `generated_at` | string |
| `kind` | string |
| `message` | string |
| `schema_version` | string |

### `create`

A torrent that was just written, and what went into it.

From `bit-cli create <DIR> --output <TORRENT> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `file_count` | integer |
| `generated_at` | string |
| `info_hash` | string |
| `kind` | string |
| `magnet` | string |
| `name` | string |
| `output` | string |
| `piece_count` | integer |
| `piece_length.bytes` | integer |
| `piece_length.human` | string |
| `piece_length_reason` | string |
| `private` | bool |
| `schema_version` | string |
| `total.bytes` | integer |
| `total.human` | string |
| `written` | bool |

### `edit`

A torrent rewritten with new trackers or sources, and its info hash before and after.

From `bit-cli edit <TORRENT> --announce <URL> --force --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `changes[]` | string |
| `generated_at` | string |
| `http_seeds[]` | array |
| `info_hash_after` | string |
| `info_hash_before` | string |
| `info_hash_changed` | bool |
| `input` | string |
| `kind` | string |
| `output` | string |
| `schema_version` | string |
| `trackers[][]` | string |
| `web_seeds[]` | array |
| `written` | bool |

### `download`

A finished download: what arrived, from where, and what it cost.

From `bit-cli download <TORRENT> --web-seed <URL> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `completed` | integer |
| `disk.bytes_written.bytes` | integer |
| `disk.bytes_written.human` | string |
| `disk.write_calls` | integer |
| `disk.write_ops` | integer |
| `disk.write_time.human` | string |
| `disk.write_time.ms` | integer |
| `downloaded.bytes` | integer |
| `downloaded.human` | string |
| `elapsed_human` | string |
| `elapsed_ms` | integer |
| `failed` | integer |
| `from_peers.bytes` | integer |
| `from_peers.human` | string |
| `from_resume.bytes` | integer |
| `from_resume.human` | string |
| `from_web_seeds.bytes` | integer |
| `from_web_seeds.human` | string |
| `generated_at` | string |
| `hooks.failed` | integer |
| `hooks.ran` | integer |
| `hooks.skipped` | integer |
| `kind` | string |
| `process.cpu_ms` | integer |
| `process.cpu_system_ms` | integer |
| `process.cpu_user_ms` | integer |
| `process.open_handles` | integer |
| `process.peak_rss_bytes` | integer |
| `process.rss_bytes` | integer |
| `schema_version` | string |
| `torrents[].announced[].accepted` | integer |
| `torrents[].announced[].at_ms` | integer |
| `torrents[].announced[].event` | string |
| `torrents[].announced[].trackers` | integer |
| `torrents[].attribution.evicted` | integer |
| `torrents[].attribution.pieces_held` | integer |
| `torrents[].attribution.recorded` | integer |
| `torrents[].attribution.resolved` | integer |
| `torrents[].code` | string |
| `torrents[].downloaded.bytes` | integer |
| `torrents[].downloaded.human` | string |
| `torrents[].elapsed_human` | string |
| `torrents[].elapsed_ms` | integer |
| `torrents[].finished` | bool |
| `torrents[].from_peers.bytes` | integer |
| `torrents[].from_peers.human` | string |
| `torrents[].from_resume.bytes` | integer |
| `torrents[].from_resume.human` | string |
| `torrents[].from_web_seeds.bytes` | integer |
| `torrents[].from_web_seeds.human` | string |
| `torrents[].info_hash` | string |
| `torrents[].mean_rate.bytes` | integer |
| `torrents[].mean_rate.human` | string |
| `torrents[].mean_rate_human` | string |
| `torrents[].metalink.agreement.file_index` | integer |
| `torrents[].metalink.agreement.matched_by` | string |
| `torrents[].metalink.agreement.metalink_size` | integer |
| `torrents[].metalink.agreement.size_agrees` | bool |
| `torrents[].metalink.agreement.torrent_size` | integer |
| `torrents[].metalink.checksum.actual` | string |
| `torrents[].metalink.checksum.algorithm` | string |
| `torrents[].metalink.checksum.bytes_hashed` | integer |
| `torrents[].metalink.checksum.expected` | string |
| `torrents[].metalink.checksum.matched` | bool |
| `torrents[].metalink.checksum.path` | string |
| `torrents[].metalink.file` | string |
| `torrents[].metalink.mirrors_listed` | integer |
| `torrents[].metalink.mirrors_registered` | integer |
| `torrents[].metalink.torrent_url` | string |
| `torrents[].metalink.version` | string |
| `torrents[].name` | string |
| `torrents[].output_directory` | string |
| `torrents[].partial[].bytes.bytes` | integer |
| `torrents[].partial[].bytes.human` | string |
| `torrents[].partial[].index` | integer |
| `torrents[].partial[].length.bytes` | integer |
| `torrents[].partial[].length.human` | string |
| `torrents[].partial[].on_disk.bytes` | integer |
| `torrents[].partial[].on_disk.human` | string |
| `torrents[].partial[].path` | string |
| `torrents[].peers_seen` | integer |
| `torrents[].phase` | string |
| `torrents[].shared[].bytes_proven.bytes` | integer |
| `torrents[].shared[].bytes_proven.human` | string |
| `torrents[].shared[].from_index` | integer |
| `torrents[].shared[].from_info_hash` | string |
| `torrents[].shared[].from_path` | string |
| `torrents[].shared[].from_source` | string |
| `torrents[].shared[].index` | integer |
| `torrents[].shared[].length.bytes` | integer |
| `torrents[].shared[].length.human` | string |
| `torrents[].shared[].path` | string |
| `torrents[].shared[].pieces_compared` | integer |
| `torrents[].source` | string |
| `torrents[].sources[]` | array |
| `torrents[].sources[].blocks` | integer |
| `torrents[].sources[].connections` | integer |
| `torrents[].sources[].error` | string |
| `torrents[].sources[].http_bytes` | integer |
| `torrents[].sources[].http_requests` | integer |
| `torrents[].sources[].index` | integer |
| `torrents[].sources[].origin` | string |
| `torrents[].sources[].retries` | integer |
| `torrents[].sources[].scope` | string |
| `torrents[].sources[].served_bytes` | integer |
| `torrents[].sources[].served_human` | string |
| `torrents[].sources[].state` | string |
| `torrents[].sources[].url` | string |
| `torrents[].sources[].whole_pieces` | integer |
| `torrents[].stopped` | string |
| `torrents[].total.bytes` | integer |
| `torrents[].total.human` | string |
| `torrents[].uploaded.bytes` | integer |
| `torrents[].uploaded.human` | string |
| `torrents[].verified_files[].algorithm` | string |
| `torrents[].verified_files[].bytes` | integer |
| `torrents[].verified_files[].disk_path` | string |
| `torrents[].verified_files[].hex` | string |
| `torrents[].verified_files[].index` | integer |
| `torrents[].verified_files[].length` | integer |
| `torrents[].verified_files[].torrent_path` | string |
| `total.bytes` | integer |
| `total.human` | string |

### `download_dry_run`

What `download --dry-run` resolved: the sources, what each one is, what it would cost, and whether the network is needed. It has its own `kind` because it shares almost no fields with a real run, and a consumer selecting by `kind` would otherwise get two shapes under one name. `dry_run: true` is also on the document. See `TODO/cli-surface.md`, T-156.

From `bit-cli download <TORRENT> --web-seed <URL> --dry-run --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `directory` | string |
| `dry_run` | bool |
| `generated_at` | string |
| `kind` | string |
| `schema_version` | string |
| `torrents[].coverage` | null |
| `torrents[].coverage.complete` | bool |
| `torrents[].coverage.covered_bytes` | integer |
| `torrents[].coverage.uncovered_bytes` | integer |
| `torrents[].coverage.uncovered_pieces[]` | array |
| `torrents[].document_needs_network` | bool |
| `torrents[].info_hash` | string |
| `torrents[].kind` | string |
| `torrents[].metalink` | null |
| `torrents[].metalink.checksum.algorithm` | string |
| `torrents[].metalink.checksum.expected` | string |
| `torrents[].metalink.file` | string |
| `torrents[].metalink.mirrors_listed` | integer |
| `torrents[].metalink.mirrors_unsupported[]` | array |
| `torrents[].metalink.size` | integer |
| `torrents[].metalink.torrents[]` | string |
| `torrents[].metalink.version` | string |
| `torrents[].name` | string |
| `torrents[].needs_network` | bool |
| `torrents[].source` | string |
| `torrents[].total_bytes` | integer |
| `torrents[].trackers[]` | string |
| `torrents[].web_seeds[].mode` | string |
| `torrents[].web_seeds[].origin` | string |
| `torrents[].web_seeds[].scope` | string |
| `torrents[].web_seeds[].url` | string |

### `seed`

A finished seeding run: who connected and what they took.

From `bit-cli seed <TORRENT> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `complete` | bool |
| `data_directory` | string |
| `elapsed_human` | string |
| `elapsed_ms` | integer |
| `generated_at` | string |
| `have.bytes` | integer |
| `have.human` | string |
| `info_hash` | string |
| `kind` | string |
| `listen_addr` | string |
| `listener.consecutive_failures` | integer |
| `listener.failed` | integer |
| `listener.healthy` | bool |
| `listener.last_failure` | null |
| `listener.last_rtt_ms` | null |
| `listener.probes` | integer |
| `mean_upload_rate.bytes` | integer |
| `mean_upload_rate.human` | string |
| `mean_upload_rate_human` | string |
| `name` | string |
| `peers[]` | array |
| `peers_seen` | integer |
| `peers_served` | integer |
| `process.cpu_ms` | integer |
| `process.cpu_system_ms` | integer |
| `process.cpu_user_ms` | integer |
| `process.open_handles` | integer |
| `process.peak_rss_bytes` | integer |
| `process.rss_bytes` | integer |
| `ratio` | string |
| `schema_version` | string |
| `stopped` | string |
| `total.bytes` | integer |
| `total.human` | string |
| `trackers[]` | string |
| `uploaded.bytes` | integer |
| `uploaded.human` | string |
| `uploaded_human` | string |

### `peers`

The swarm as sampled over a window.

From `bit-cli peers <TORRENT> --peer <ADDR> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `blocked.incoming` | integer |
| `blocked.outgoing` | integer |
| `connecting` | integer |
| `dead` | integer |
| `downloaded.bytes` | integer |
| `downloaded.human` | string |
| `generated_at` | string |
| `info_hash` | string |
| `kind` | string |
| `live` | integer |
| `name` | string |
| `peers[].addr` | string |
| `peers[].choked` | integer |
| `peers[].chunks` | integer |
| `peers[].connect_ms` | integer |
| `peers[].direction` | string |
| `peers[].disconnects[].at` | string |
| `peers[].disconnects[].reason` | string |
| `peers[].downloaded_bytes` | integer |
| `peers[].encryption` | string |
| `peers[].errors` | integer |
| `peers[].mean_piece_ms` | integer |
| `peers[].state` | string |
| `peers[].unchoked` | integer |
| `peers[].uploaded_bytes` | integer |
| `peers[].verified_pieces` | integer |
| `peers[].web_seed` | bool |
| `queued` | integer |
| `sampled_human` | string |
| `sampled_ms` | integer |
| `schema_version` | string |
| `seen` | integer |

### `trackers`

What each tracker answered.

From `bit-cli trackers <TORRENT> --tracker <URL> --json`.

| field | type |
| --- | --- |
| `action` | string |
| `announced_port` | integer |
| `announces` | integer |
| `bit_cli_version` | string |
| `failed` | integer |
| `families[].announces` | integer |
| `families[].family` | string |
| `families[].peers[]` | string |
| `families[].responded` | integer |
| `generated_at` | string |
| `info_hash` | string |
| `kind` | string |
| `leechers` | integer |
| `left.bytes` | integer |
| `left.known` | bool |
| `left.reason` | string |
| `name` | string |
| `peers[]` | string |
| `responded` | integer |
| `schema_version` | string |
| `scrape_url` | string |
| `seeders` | integer |
| `tracker_count` | integer |
| `trackers[].completed` | integer |
| `trackers[].elapsed_ms` | integer |
| `trackers[].endpoint` | string |
| `trackers[].failure` | string |
| `trackers[].family` | string |
| `trackers[].http_status` | integer |
| `trackers[].interval_s` | integer |
| `trackers[].invalid_peers[]` | string |
| `trackers[].leechers` | integer |
| `trackers[].min_interval_s` | integer |
| `trackers[].ok` | bool |
| `trackers[].peers[]` | array |
| `trackers[].protocol` | string |
| `trackers[].seeders` | integer |
| `trackers[].tier` | integer |
| `trackers[].url` | string |
| `trackers[].warning` | string |
| `withdrawn` | integer |

### `webseed_list`

Every source binding resolved to the exact URLs it would request.

From `bit-cli webseed list <TORRENT> --web-seed <URL> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `cache_budget_total.bytes` | integer |
| `cache_budget_total.human` | string |
| `cache_windows` | integer |
| `complete` | bool |
| `covered.bytes` | integer |
| `covered.human` | string |
| `generated_at` | string |
| `info_hash` | string |
| `kind` | string |
| `name` | string |
| `piece_count` | integer |
| `schema_version` | string |
| `source_count` | integer |
| `sources[].cache_budget.bytes` | integer |
| `sources[].cache_budget.human` | string |
| `sources[].files[]` | integer |
| `sources[].in_scope.bytes` | integer |
| `sources[].in_scope.human` | string |
| `sources[].in_scope_share` | string |
| `sources[].index` | integer |
| `sources[].mode` | string |
| `sources[].origin` | string |
| `sources[].partial_pieces` | integer |
| `sources[].priority` | integer |
| `sources[].scope` | string |
| `sources[].style` | string |
| `sources[].url` | string |
| `sources[].urls[].file` | integer |
| `sources[].urls[].in_scope.bytes` | integer |
| `sources[].urls[].in_scope.human` | string |
| `sources[].urls[].path` | string |
| `sources[].urls[].size.bytes` | integer |
| `sources[].urls[].size.human` | string |
| `sources[].urls[].url` | string |
| `sources[].whole_pieces` | integer |
| `total.bytes` | integer |
| `total.human` | string |
| `uncovered.bytes` | integer |
| `uncovered.human` | string |
| `uncovered_pieces[]` | array |

### `webseed_test`

One request per source: status, ranges, redirects, timing, the negotiated TLS, and the response headers worth keeping. `sources[].headers` is a map whose keys are whichever of the reported set the response carried, so the rows below are the ones the sample produced rather than the whole set: `age`, `cache-control`, `cf-cache-status`, `cf-ray`, `content-encoding`, `etag`, `last-modified`, `via`, `x-amz-id-2`, `x-amz-request-id`, `x-cache` and `x-served-by`, plus anything `--web-seed-report-header` names. See `TODO/webseed.md`, T-254.

From `bit-cli webseed test <TORRENT> --web-seed <URL> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `generated_at` | string |
| `info_hash` | string |
| `kind` | string |
| `name` | string |
| `schema_version` | string |
| `source_count` | integer |
| `sources[].at` | string |
| `sources[].content_length` | integer |
| `sources[].error` | string |
| `sources[].expected_length` | integer |
| `sources[].headers.age` | string |
| `sources[].headers.cache-control` | string |
| `sources[].headers.etag` | string |
| `sources[].headers.x-cache` | string |
| `sources[].http_version` | string |
| `sources[].index` | integer |
| `sources[].length_matches` | bool |
| `sources[].method` | string |
| `sources[].mode` | string |
| `sources[].ok` | bool |
| `sources[].origin` | string |
| `sources[].range_support` | string |
| `sources[].redirects[].from` | string |
| `sources[].redirects[].status` | integer |
| `sources[].redirects[].to` | string |
| `sources[].request_url` | string |
| `sources[].resolved_url` | string |
| `sources[].scope` | string |
| `sources[].server` | string |
| `sources[].status` | integer |
| `sources[].style` | string |
| `sources[].style_decided_by` | string |
| `sources[].tls.alpn` | string |
| `sources[].tls.cipher_suite` | string |
| `sources[].tls.connect_ms` | integer |
| `sources[].tls.handshake_ms` | integer |
| `sources[].tls.server_name` | string |
| `sources[].tls.version` | string |
| `sources[].total_ms` | integer |
| `sources[].ttfb_ms` | integer |
| `sources[].url` | string |
| `unusable` | integer |
| `usable` | integer |

### `webseed_probe`

A source measured at several concurrencies.

From `bit-cli webseed probe <TORRENT> --web-seed <URL> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `concurrency_sweep[]` | integer |
| `duration_ms` | integer |
| `generated_at` | string |
| `info_hash` | string |
| `kind` | string |
| `name` | string |
| `schema_version` | string |
| `sources[].best_concurrency` | integer |
| `sources[].best_throughput` | integer |
| `sources[].best_throughput_human` | string |
| `sources[].chunk_size.bytes` | integer |
| `sources[].chunk_size.human` | string |
| `sources[].index` | integer |
| `sources[].scope` | string |
| `sources[].steps[].bytes` | integer |
| `sources[].steps[].bytes_human` | string |
| `sources[].steps[].concurrency` | integer |
| `sources[].steps[].elapsed_ms` | integer |
| `sources[].steps[].errors` | integer |
| `sources[].steps[].max_ms` | integer |
| `sources[].steps[].p50_ms` | integer |
| `sources[].steps[].p90_ms` | integer |
| `sources[].steps[].p999_ms` | integer |
| `sources[].steps[].p99_ms` | integer |
| `sources[].steps[].requests` | integer |
| `sources[].steps[].throughput` | integer |
| `sources[].steps[].throughput_human` | string |
| `sources[].steps[].ttfb_p50_ms` | integer |
| `sources[].steps[].ttfb_p99_ms` | integer |
| `sources[].url` | string |

### `webseed_fetch`

One piece pulled from one source and checked.

From `bit-cli webseed fetch <TORRENT> --piece 0 --web-seed <URL> --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `elapsed.human` | string |
| `elapsed.ms` | integer |
| `generated_at` | string |
| `kind` | string |
| `length.bytes` | integer |
| `length.human` | string |
| `offset` | integer |
| `pieces[]` | integer |
| `rate.bytes` | integer |
| `rate.human` | string |
| `requests[].at` | string |
| `requests[].bytes` | integer |
| `requests[].curl` | string |
| `requests[].range` | string |
| `requests[].status` | integer |
| `requests[].total_ms` | integer |
| `requests[].ttfb_ms` | integer |
| `requests[].url` | string |
| `schema_version` | string |
| `source_index` | integer |
| `url` | string |
| `verified` | bool |

### `config`

Configuration as resolved, with where each value came from.

From `bit-cli config show --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `files_missing[]` | string |
| `files_read[]` | array |
| `generated_at` | string |
| `kind` | string |
| `schema_version` | string |
| `settings.color.origin.kind` | string |
| `settings.color.value` | string |
| `settings.download_directory.origin.kind` | string |
| `settings.download_directory.value` | string |
| `settings.enable_dht.origin.kind` | string |
| `settings.enable_dht.value` | string |
| `settings.enable_lsd.origin.kind` | string |
| `settings.enable_lsd.value` | string |
| `settings.enable_pex.origin.kind` | string |
| `settings.enable_pex.value` | string |
| `settings.enable_web_seeds.origin.kind` | string |
| `settings.enable_web_seeds.value` | string |
| `settings.file_allocation.origin.kind` | string |
| `settings.file_allocation.value` | string |
| `settings.listen_port.origin.kind` | string |
| `settings.listen_port.value` | string |
| `settings.log_format.origin.kind` | string |
| `settings.log_format.value` | string |
| `settings.log_level.origin.kind` | string |
| `settings.log_level.value` | string |
| `settings.max_concurrent_downloads.origin.kind` | string |
| `settings.max_concurrent_downloads.value` | string |
| `settings.max_download_rate.origin.kind` | string |
| `settings.max_download_rate.value` | string |
| `settings.max_peers.origin.kind` | string |
| `settings.max_peers.value` | string |
| `settings.max_peers_total.origin.kind` | string |
| `settings.max_peers_total.value` | string |
| `settings.max_upload_rate.origin.kind` | string |
| `settings.max_upload_rate.value` | string |
| `settings.piece_selector.origin.kind` | string |
| `settings.piece_selector.value` | string |
| `settings.seed_ratio.origin.kind` | string |
| `settings.seed_ratio.value` | string |
| `settings.seed_time.origin.kind` | string |
| `settings.seed_time.value` | string |
| `settings.web_seed_chunk_size.origin.kind` | string |
| `settings.web_seed_chunk_size.value` | string |
| `settings.web_seed_concurrency.origin.kind` | string |
| `settings.web_seed_concurrency.value` | string |
| `settings.web_seed_timeout.origin.kind` | string |
| `settings.web_seed_timeout.value` | string |
| `settings.web_seed_user_agent.origin.kind` | string |
| `settings.web_seed_user_agent.value` | string |

### `version`

The build, its features, and the exit code table.

From `bit-cli version --json`.

| field | type |
| --- | --- |
| `bit_cli_version` | string |
| `composition_modes[]` | string |
| `exit_codes[].code` | integer |
| `exit_codes[].description` | string |
| `exit_codes[].kind` | string |
| `features[]` | string |
| `generated_at` | string |
| `kind` | string |
| `lints[]` | string |
| `schema_version` | string |
| `target` | string |
| `trace_subsystems[].description` | string |
| `trace_subsystems[].name` | string |
| `version` | string |

### `disk`

The report a `bench` run writes, measured here from `bench disk`. Every target writes this document with its own `kind`. `environment` describes the machine rather than the measurement and is left out: it carries fields one platform has and another does not, so a contract holding it would say which machine last regenerated this file. See `TODO/bench.md`, T-189.

From `bit-cli bench disk --json`.

| field | type |
| --- | --- |
| `concurrency_curve[].bytes.bytes` | integer |
| `concurrency_curve[].bytes.human` | string |
| `concurrency_curve[].concurrency` | integer |
| `concurrency_curve[].elapsed.human` | string |
| `concurrency_curve[].elapsed.ms` | integer |
| `concurrency_curve[].errors` | integer |
| `concurrency_curve[].latency.complete.count` | integer |
| `concurrency_curve[].latency.complete.max_ms` | integer |
| `concurrency_curve[].latency.complete.mean_ms` | integer |
| `concurrency_curve[].latency.complete.p50_ms` | integer |
| `concurrency_curve[].latency.complete.p90_ms` | integer |
| `concurrency_curve[].latency.complete.p999_ms` | integer |
| `concurrency_curve[].latency.complete.p99_ms` | integer |
| `concurrency_curve[].latency.connect.count` | integer |
| `concurrency_curve[].latency.connect.max_ms` | integer |
| `concurrency_curve[].latency.connect.mean_ms` | integer |
| `concurrency_curve[].latency.connect.p50_ms` | integer |
| `concurrency_curve[].latency.connect.p90_ms` | integer |
| `concurrency_curve[].latency.connect.p999_ms` | integer |
| `concurrency_curve[].latency.connect.p99_ms` | integer |
| `concurrency_curve[].latency.first_byte.count` | integer |
| `concurrency_curve[].latency.first_byte.max_ms` | integer |
| `concurrency_curve[].latency.first_byte.mean_ms` | integer |
| `concurrency_curve[].latency.first_byte.p50_ms` | integer |
| `concurrency_curve[].latency.first_byte.p90_ms` | integer |
| `concurrency_curve[].latency.first_byte.p999_ms` | integer |
| `concurrency_curve[].latency.first_byte.p99_ms` | integer |
| `concurrency_curve[].rate.bytes` | integer |
| `concurrency_curve[].rate.human` | string |
| `concurrency_curve[].requests` | integer |
| `disk_steps[].bytes.bytes` | integer |
| `disk_steps[].bytes.human` | string |
| `disk_steps[].concurrency_achieved` | string |
| `disk_steps[].elapsed.human` | string |
| `disk_steps[].elapsed.ms` | integer |
| `disk_steps[].files` | integer |
| `disk_steps[].flush.human` | string |
| `disk_steps[].flush.ms` | integer |
| `disk_steps[].layout` | string |
| `disk_steps[].mean_write_us` | integer |
| `disk_steps[].rate.bytes` | integer |
| `disk_steps[].rate.human` | string |
| `disk_steps[].run_length` | integer |
| `disk_steps[].threads` | integer |
| `disk_steps[].threads_detail[].blocks` | integer |
| `disk_steps[].threads_detail[].bytes.bytes` | integer |
| `disk_steps[].threads_detail[].bytes.human` | string |
| `disk_steps[].threads_detail[].index` | integer |
| `disk_steps[].threads_detail[].mean_write_us` | integer |
| `disk_steps[].threads_detail[].write_time.human` | string |
| `disk_steps[].threads_detail[].write_time.ms` | integer |
| `disk_steps[].total_write_time.human` | string |
| `disk_steps[].total_write_time.ms` | integer |
| `disk_steps[].write_calls` | integer |
| `disk_steps[].write_ops` | integer |
| `kind` | string |
| `notes[]` | string |
| `parameters.concurrency` | integer |
| `parameters.duration.human` | string |
| `parameters.duration.ms` | integer |
| `parameters.metrics_interval.human` | string |
| `parameters.metrics_interval.ms` | integer |
| `parameters.payload_size.bytes` | integer |
| `parameters.payload_size.human` | string |
| `parameters.piece_size.bytes` | integer |
| `parameters.piece_size.human` | string |
| `parameters.warmup.human` | string |
| `parameters.warmup.ms` | integer |
| `report_version` | integer |
| `series[].at.epoch_ms` | integer |
| `series[].at.iso` | string |
| `series[].bytes.bytes` | integer |
| `series[].bytes.human` | string |
| `series[].concurrency` | integer |
| `series[].costs.disk_read.human` | string |
| `series[].costs.disk_read.ms` | integer |
| `series[].costs.disk_read_bytes.bytes` | integer |
| `series[].costs.disk_read_bytes.human` | string |
| `series[].costs.disk_write.human` | string |
| `series[].costs.disk_write.ms` | integer |
| `series[].costs.disk_write_bytes.bytes` | integer |
| `series[].costs.disk_write_bytes.human` | string |
| `series[].costs.mean_service_us` | integer |
| `series[].costs.verify.human` | string |
| `series[].costs.verify.ms` | integer |
| `series[].costs.verify_bytes.bytes` | integer |
| `series[].costs.verify_bytes.human` | string |
| `series[].cumulative_bytes.bytes` | integer |
| `series[].cumulative_bytes.human` | string |
| `series[].elapsed.human` | string |
| `series[].elapsed.ms` | integer |
| `series[].errors` | integer |
| `series[].process.cpu_ms` | integer |
| `series[].process.cpu_system_ms` | integer |
| `series[].process.cpu_user_ms` | integer |
| `series[].process.open_handles` | integer |
| `series[].process.peak_rss_bytes` | integer |
| `series[].process.rss_bytes` | integer |
| `series[].rate.bytes` | integer |
| `series[].rate.human` | string |
| `series[].requests` | integer |
| `series[].warmup` | bool |
| `summary.best_concurrency` | integer |
| `summary.bytes.bytes` | integer |
| `summary.bytes.human` | string |
| `summary.disk.read_bytes.bytes` | integer |
| `summary.disk.read_bytes.human` | string |
| `summary.disk.read_ops` | integer |
| `summary.disk.read_time.human` | string |
| `summary.disk.read_time.ms` | integer |
| `summary.disk.write_bytes.bytes` | integer |
| `summary.disk.write_bytes.human` | string |
| `summary.disk.write_calls` | integer |
| `summary.disk.write_ops` | integer |
| `summary.disk.write_time.human` | string |
| `summary.disk.write_time.ms` | integer |
| `summary.duration.human` | string |
| `summary.duration.ms` | integer |
| `summary.errors.total` | integer |
| `summary.peak_rate.bytes` | integer |
| `summary.peak_rate.human` | string |
| `summary.requests` | integer |
| `summary.sustained_rate.bytes` | integer |
| `summary.sustained_rate.human` | string |
| `target.name` | string |
| `target.piece_length.bytes` | integer |
| `target.piece_length.human` | string |
| `target.source` | string |
| `target.total.bytes` | integer |
| `target.total.human` | string |

## Events

One object per line, on stdout, when `--jsonl` is given. Every event carries
`type`, `seq`, and `at` before its own fields; `seq` counts from zero within a
run and `at` is ISO 8601 UTC with millisecond precision.

### `session_start`

The session is up. Carries the listen address and what it was asked to do.

From `bit-cli download <TORRENT> --web-seed <URL> --jsonl` and `bit-cli seed <TORRENT> --jsonl`.

More than one command writes this shape and they do not carry the same
fields. The `from` column names which of them writes each one, and reads
`both` where every one of them does, so a consumer selecting by `type`
alone knows what may be absent.

| field | type | from |
| --- | --- | --- |
| `at` | string | both |
| `data_directory` | string | seed |
| `directory` | string | download |
| `listen_addr` | string | both |
| `max_concurrent_downloads` | integer | download |
| `seq` | integer | both |
| `source` | string | seed |
| `sources` | integer | download |
| `type` | string | both |

### `torrent_added`

A source resolved to a torrent and was added to the session.

From `bit-cli download <TORRENT> --web-seed <URL> --jsonl`.

| field | type |
| --- | --- |
| `at` | string |
| `info_hash` | string |
| `name` | string |
| `seq` | integer |
| `source` | string |
| `type` | string |

### `metadata_resolved`

The torrent's metadata is known: name, files, pieces.

From `bit-cli download <TORRENT> --web-seed <URL> --jsonl`.

| field | type |
| --- | --- |
| `at` | string |
| `files` | integer |
| `info_hash` | string |
| `name` | string |
| `piece_count` | integer |
| `piece_length` | integer |
| `seq` | integer |
| `total_bytes` | integer |
| `type` | string |

### `source_added`

An HTTP or `file:` source was attached, with its scope.

From `bit-cli download <TORRENT> --web-seed <URL> --jsonl`.

| field | type |
| --- | --- |
| `at` | string |
| `index` | integer |
| `origin` | string |
| `scope` | string |
| `seq` | integer |
| `type` | string |
| `url` | string |
| `whole_pieces` | integer |

### `source_failed`

A source is out for the run: it spent its error budget, or it was proved to have served bytes the session then verified as something else. `sources[].convictions` says which, and names the block.

From `bit-cli download <TORRENT> --web-seed <404 URL> --jsonl`.

| field | type |
| --- | --- |
| `at` | string |
| `blocks` | integer |
| `connections` | integer |
| `cooldowns` | integer |
| `error` | string |
| `gone_files[].file` | integer |
| `gone_files[].pieces_dropped` | integer |
| `gone_files[].reason` | string |
| `http_bytes` | integer |
| `http_requests` | integer |
| `index` | integer |
| `origin` | string |
| `pieces_dropped` | integer |
| `retries` | integer |
| `scope` | string |
| `seq` | integer |
| `served_bytes` | integer |
| `served_human` | string |
| `state` | string |
| `type` | string |
| `url` | string |
| `whole_pieces` | integer |

### `source_cooling`

A source spent its error budget and will be tried again after `--web-seed-cooldown`.

From `bit-cli download <TORRENT> --web-seed <404 URL> --web-seed-cooldown <DUR> --jsonl`.

| field | type |
| --- | --- |
| `at` | string |
| `blocks` | integer |
| `connections` | integer |
| `cooldown_remaining_ms` | integer |
| `cooldown_until` | string |
| `cooldowns` | integer |
| `error` | string |
| `http_bytes` | integer |
| `http_requests` | integer |
| `index` | integer |
| `origin` | string |
| `retries` | integer |
| `scope` | string |
| `seq` | integer |
| `served_bytes` | integer |
| `served_human` | string |
| `state` | string |
| `type` | string |
| `url` | string |
| `whole_pieces` | integer |

### `peer_redial`

`--redial-after` fired: every peer connection was dropped and the peer list dialled again.

From `bit-cli download <TORRENT> --redial-after <DUR> --jsonl`.

| field | type |
| --- | --- |
| `at` | string |
| `at_ms` | integer |
| `attempt` | integer |
| `peers_dropped` | integer |
| `seq` | integer |
| `stalled_ms` | integer |
| `type` | string |

### `metalink_resolved`

A Metalink was read and the `.torrent` it names was fetched.

From `bit-cli download <METALINK> --jsonl`.

| field | type |
| --- | --- |
| `at` | string |
| `checksums` | integer |
| `file` | string |
| `info_hash` | string |
| `mirrors` | integer |
| `seq` | integer |
| `source` | string |
| `torrent_url` | string |
| `type` | string |
| `unsupported_mirrors` | integer |
| `version` | string |

### `metalink_checked`

The payload was checked against the Metalink's own checksum. `not_checked` says why it was not, when it was not.

From `bit-cli download <METALINK> --jsonl`.

| field | type |
| --- | --- |
| `actual` | string |
| `algorithm` | string |
| `at` | string |
| `bytes_hashed` | integer |
| `expected` | string |
| `info_hash` | string |
| `matched` | bool |
| `path` | string |
| `seq` | integer |
| `type` | string |

### `piece_verified`

A piece arrived and its hash checked out.

From `bit-cli download <TORRENT> --web-seed <URL> --jsonl`.

| field | type |
| --- | --- |
| `at` | string |
| `length` | integer |
| `piece` | integer |
| `seq` | integer |
| `type` | string |

### `file_completed`

Every piece of one file is present.

From `bit-cli download <TORRENT> --web-seed <URL> --jsonl`.

| field | type |
| --- | --- |
| `at` | string |
| `file` | integer |
| `length` | integer |
| `path` | string |
| `seq` | integer |
| `type` | string |

### `progress`

A tick of the report interval: rates, peers, and what the process costs.

From `bit-cli download <TORRENT> --web-seed <URL> --jsonl` and `bit-cli seed <TORRENT> --jsonl`.

More than one command writes this shape and they do not carry the same
fields. The `from` column names which of them writes each one, and reads
`both` where every one of them does, so a consumer selecting by `type`
alone knows what may be absent.

| field | type | from |
| --- | --- | --- |
| `at` | string | both |
| `download_rate` | integer | both |
| `eta_confidence` | string | download |
| `eta_ms` | null | download |
| `from_web_seeds` | integer | download |
| `info_hash` | string | both |
| `listener.consecutive_failures` | integer | seed |
| `listener.failed` | integer | seed |
| `listener.healthy` | bool | seed |
| `listener.last_failure` | null | seed |
| `listener.last_rtt_ms` | null | seed |
| `listener.probes` | integer | seed |
| `peer_detail[]` | array | seed |
| `peers.connecting` | integer | both |
| `peers.dead` | integer | both |
| `peers.live` | integer | both |
| `peers.queued` | integer | both |
| `peers.seen` | integer | both |
| `percent` | string | download |
| `process.cpu_ms` | integer | both |
| `process.cpu_system_ms` | integer | both |
| `process.cpu_user_ms` | integer | both |
| `process.open_handles` | integer | both |
| `process.peak_rss_bytes` | integer | both |
| `process.rss_bytes` | integer | both |
| `progress_bytes` | integer | download |
| `ratio` | string | seed |
| `seq` | integer | both |
| `total_bytes` | integer | download |
| `type` | string | both |
| `upload_rate` | integer | both |
| `uploaded_bytes` | integer | seed |

### `bench_sample`

One point of a `bench` time series.

From `bit-cli bench disk --jsonl`.

| field | type |
| --- | --- |
| `at.epoch_ms` | integer |
| `at.iso` | string |
| `bytes.bytes` | integer |
| `bytes.human` | string |
| `concurrency` | integer |
| `costs.disk_read.human` | string |
| `costs.disk_read.ms` | integer |
| `costs.disk_read_bytes.bytes` | integer |
| `costs.disk_read_bytes.human` | string |
| `costs.disk_write.human` | string |
| `costs.disk_write.ms` | integer |
| `costs.disk_write_bytes.bytes` | integer |
| `costs.disk_write_bytes.human` | string |
| `costs.mean_service_us` | integer |
| `costs.verify.human` | string |
| `costs.verify.ms` | integer |
| `costs.verify_bytes.bytes` | integer |
| `costs.verify_bytes.human` | string |
| `cumulative_bytes.bytes` | integer |
| `cumulative_bytes.human` | string |
| `elapsed.human` | string |
| `elapsed.ms` | integer |
| `errors` | integer |
| `process.cpu_ms` | integer |
| `process.cpu_system_ms` | integer |
| `process.cpu_user_ms` | integer |
| `process.open_handles` | integer |
| `process.peak_rss_bytes` | integer |
| `process.rss_bytes` | integer |
| `rate.bytes` | integer |
| `rate.human` | string |
| `requests` | integer |
| `seq` | integer |
| `type` | string |
| `warmup` | bool |

### `torrent_completed`

One torrent finished, with its totals.

From `bit-cli download <TORRENT> --web-seed <URL> --jsonl`.

| field | type |
| --- | --- |
| `at` | string |
| `downloaded_bytes` | integer |
| `elapsed_ms` | integer |
| `finished` | bool |
| `from_peers` | integer |
| `from_resume` | integer |
| `from_web_seeds` | integer |
| `info_hash` | string |
| `name` | string |
| `seq` | integer |
| `stopped` | string |
| `type` | string |

### `error`

Something failed. The same shape the final error document carries.

From `bit-cli download <TORRENT> --no-continue --jsonl`.

| field | type |
| --- | --- |
| `at` | string |
| `code` | integer |
| `context.source` | string |
| `kind` | string |
| `message` | string |
| `seq` | integer |
| `type` | string |

### `session_end`

The run is over. Always last, always present, whatever happened.

From `bit-cli bench disk --jsonl`, `bit-cli download <TORRENT> --web-seed <URL> --jsonl`, `bit-cli info <MISSING> --jsonl` and `bit-cli seed <TORRENT> --jsonl`.

More than one command writes this shape and they do not carry the same
fields. The `from` column names which of them writes each one, and reads
`all` where every one of them does, so a consumer selecting by `type`
alone knows what may be absent.

| field | type | from |
| --- | --- | --- |
| `at` | string | all |
| `elapsed_human` | string | all |
| `elapsed_ms` | integer | all |
| `error` | string | info |
| `exit_code` | integer | all |
| `exit_status` | string | all |
| `ok` | bool | all |
| `seq` | integer | all |
| `type` | string | all |

## Machine output, from the README

Two rules, and neither bends.

**stdout carries data only.** JSON, NDJSON, or the requested plain values.
`bit-cli ... --json | jq` never sees a log line in the pipe.

**stderr carries logs, progress, warnings, and errors.**

```bash
bit-cli info album.torrent --json | jq -r .info_hash
```

`--jsonl` emits one event per line as things happen, each with a monotonic
`seq` and an ISO 8601 UTC millisecond timestamp. Every `--jsonl` run ends with
a `session_end` event carrying the exit code, so a consumer can tell "finished"
from "the pipe broke".

`docs/schema.md` lists every document `kind` and every event `type` with the
fields each one carries, and `bit-cli --schema-version` prints the version it
describes. That file is generated from what the program actually writes: a test
drives every command, flattens the JSON, and fails when a report carries a field
the document does not.

Nothing is TTY-gated. Terminal detection reaches exactly two decisions, colour
and progress rendering, and never decides what the program does, computes, or
reports. Anything you can read in the terminal is a field in `--json`.

## Keeping a log

```bash
bit-cli download release.torrent \
  --log-file /var/log/bit-cli.log --log-max-size 16MiB --log-max-files 5
```

The file rotates at `--log-max-size` into `.1`, `.2`, and so on.
`--log-max-files` is the count in total, the live one included, so `5` leaves
`bit-cli.log` plus four rotated. `--log-max-size 0` never rotates.

It is a second destination, not a replacement: stderr still carries the logs,
so `bit-cli ... --json | jq` behaves the same either way. Redirect stderr if
you want only the file. The log file never carries colour escapes, whatever the
terminal is.

## On Windows

PowerShell surfaces the exit code in `$LASTEXITCODE`, not `$?`.

`bit-cli` writes UTF-8 with no BOM to stdout whatever the console code page is.
Getting those bytes into a file or a parser is the caller's half, and on Windows
that is two settings rather than one:

| | |
| --- | --- |
| `[Console]::OutputEncoding` | how the host decodes what a program wrote |
| `$OutputEncoding` | how the host encodes what it sends into one |

**Neither defaults to UTF-8.** Measured on Windows 11: both hosts read at the
console code page, `IBM437` here, and Windows PowerShell 5.1 writes `us-ascii`
into a native command. A torrent whose name is `café-λ-日本.bin` comes back
with a different name, and the JSON still parses, so nothing says so.

Set both once per session, and every form below is exact:

```powershell
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
$OutputEncoding = New-Object System.Text.UTF8Encoding $false
bit-cli info album.torrent --json | ConvertFrom-Json
```

Or keep the bytes out of the pipeline altogether, which needs nothing set:

```powershell
cmd /c "bit-cli info album.torrent --json > info.json"
```

What each form does, measured against a name no code page holds:

| form | 5.1 | 7.6.5 | |
| --- | --- | --- | --- |
| `cmd /c "... > file"` | exact | exact | copies bytes, decodes nothing |
| `> file` | no | exact | 5.1 writes UTF-16LE, and `jq` reads none of it |
| `\| ConvertFrom-Json` | no | no | exact once both encodings are set |
| `\| Set-Content -Encoding utf8` | no | no | exact once both encodings are set. 5.1 adds a BOM |
| `\| Out-File -Encoding utf8NoBOM` | no such value | no | `utf8NoBOM` arrived in PowerShell 6 |

Every row of both columns comes from one command, and it takes two seconds:

```powershell
pwsh -NoProfile -File scripts/check-redirect.ps1
```

```powershell
powershell -NoProfile -File scripts/check-redirect.ps1
```

It builds its own torrent, runs all seven forms, and prints which ones give the
bytes back. It judges nothing: what it measures is a property of the host.

## Reading a download as it arrives

```bash
bit-cli download film.torrent --piece-selector sequential --web-seed-connections 1
```

Pieces arrive front to back. Measured over ten runs on a 48 piece torrent, that
is **zero out of ten runs with any piece arriving before one already reported**,
against one such piece in every run of the default. It costs nothing at one
connection.

The default is not disordered. It asks for the first piece of each file, then
the last, then the middle in ascending order, so it is almost front to back
already and its one break is the tail arriving early. That is why the flag is
worth having and why it is not the default: `sequential` removes that break,
and above one connection it costs about seven percent of the throughput,
because every connection is pointed at the same part of the file.

Above one connection the order is not exact and cannot be. A selector decides
which piece is asked for next; it cannot decide which of four transfers already
in flight finishes first. Run the measurement yourself:

```bash
pwsh scripts/check-piece-order.ps1 -Runs 10 -Connections 1,2,4
```

`in-order` is the same thing spelled the way `aria2` spells it.

[`examples/machine-output.md`](examples/machine-output.md) is the worked
version, with the event types a real run emitted.
