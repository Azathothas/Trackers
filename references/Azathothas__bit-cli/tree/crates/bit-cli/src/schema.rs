//! The JSON contract, described from what the program actually writes.
//!
//! `--schema-version` prints a number. A number that refers to nothing a caller
//! can check against is not a contract, so `docs/schema.md` lists every
//! document `kind` and every event `type` with the fields each one carries.
//!
//! The document is **generated**, not written. `schema::render` takes the JSON
//! a real run produced and flattens it into a field table, and a test drives
//! every command, renders the whole file, and fails when it differs from what
//! is committed. So a field added to a report changes the generated text and
//! the test says so, rather than the documentation quietly going stale.
//!
//! Regenerate with:
//!
//! ```text
//! BIT_CLI_UPDATE_SCHEMA=1 cargo test -p bit-cli --lib schema
//! ```
//!
//! See `TODO/cli-surface.md`, T-117 and T-110.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// Where the generated document lives, relative to the repository root.
pub const SCHEMA_PATH: &str = "docs/schema.md";

/// Every document `kind` `bit-cli` emits under `--json`, with what it is.
///
/// The order here is the order in the document. It is grouped by what a reader
/// is doing rather than alphabetically, because the alphabet puts `config`
/// before `download`.
pub const DOCUMENT_KINDS: &[(&str, &str)] = &[
    (
        "info",
        "One torrent's metadata, without touching the network.",
    ),
    (
        "files",
        "The files in a torrent, with sizes, offsets, and piece ranges.",
    ),
    (
        "tree",
        "The torrent's directory structure, rolled up. The nodes are a flat list in pre-order rather than a nested one, so a field sits at the same path whatever its depth. See `TODO/metainfo.md`, T-249.",
    ),
    (
        "magnet",
        "A magnet URI built from a torrent, and its parts.",
    ),
    (
        "verify",
        "What a hash check of existing data found, piece by piece.",
    ),
    (
        "hash_mismatch",
        "The document `verify` writes instead when a piece did not check out.",
    ),
    (
        "create",
        "A torrent that was just written, and what went into it.",
    ),
    (
        "edit",
        "A torrent rewritten with new trackers or sources, and its info hash before and after.",
    ),
    (
        "download",
        "A finished download: what arrived, from where, and what it cost.",
    ),
    (
        "download_dry_run",
        "What `download --dry-run` resolved: the sources, what each one is, what it would cost, and whether the network is needed. It has its own `kind` because it shares almost no fields with a real run, and a consumer selecting by `kind` would otherwise get two shapes under one name. `dry_run: true` is also on the document. See `TODO/cli-surface.md`, T-156.",
    ),
    (
        "seed",
        "A finished seeding run: who connected and what they took.",
    ),
    ("peers", "The swarm as sampled over a window."),
    ("trackers", "What each tracker answered."),
    (
        "webseed_list",
        "Every source binding resolved to the exact URLs it would request.",
    ),
    (
        "webseed_test",
        "One request per source: status, ranges, redirects, timing, the negotiated TLS, and the response headers worth keeping. `sources[].headers` is a map whose keys are whichever of the reported set the response carried, so the rows below are the ones the sample produced rather than the whole set: `age`, `cache-control`, `cf-cache-status`, `cf-ray`, `content-encoding`, `etag`, `last-modified`, `via`, `x-amz-id-2`, `x-amz-request-id`, `x-cache` and `x-served-by`, plus anything `--web-seed-report-header` names. See `TODO/webseed.md`, T-254.",
    ),
    (
        "webseed_probe",
        "A source measured at several concurrencies.",
    ),
    (
        "webseed_fetch",
        "One piece pulled from one source and checked.",
    ),
    (
        "config",
        "Configuration as resolved, with where each value came from.",
    ),
    (
        "version",
        "The build, its features, and the exit code table.",
    ),
    (
        "disk",
        "The report a `bench` run writes, measured here from `bench disk`. Every target writes this document with its own `kind`. `environment` describes the machine rather than the measurement and is left out: it carries fields one platform has and another does not, so a contract holding it would say which machine last regenerated this file. See `TODO/bench.md`, T-189.",
    ),
];

/// Every event `type` `bit-cli` emits under `--jsonl`, with what it means.
///
/// Ordered by when a run emits them, because that is how a reader consuming
/// the stream meets them.
pub const EVENT_TYPES: &[(&str, &str)] = &[
    (
        "session_start",
        "The session is up. Carries the listen address and what it was asked to do.",
    ),
    (
        "torrent_added",
        "A source resolved to a torrent and was added to the session.",
    ),
    (
        "metadata_resolved",
        "The torrent's metadata is known: name, files, pieces.",
    ),
    (
        "source_added",
        "An HTTP or `file:` source was attached, with its scope.",
    ),
    (
        "source_failed",
        "A source is out for the run: it spent its error budget, or it was proved to have served bytes the session then verified as something else. `sources[].convictions` says which, and names the block.",
    ),
    (
        "source_cooling",
        "A source spent its error budget and will be tried again after `--web-seed-cooldown`.",
    ),
    (
        "peer_redial",
        "`--redial-after` fired: every peer connection was dropped and the peer list dialled again.",
    ),
    (
        "metalink_resolved",
        "A Metalink was read and the `.torrent` it names was fetched.",
    ),
    (
        "metalink_checked",
        "The payload was checked against the Metalink's own checksum. `not_checked` says why it was not, when it was not.",
    ),
    (
        "piece_verified",
        "A piece arrived and its hash checked out.",
    ),
    ("file_completed", "Every piece of one file is present."),
    (
        "progress",
        "A tick of the report interval: rates, peers, and what the process costs.",
    ),
    ("bench_sample", "One point of a `bench` time series."),
    (
        "torrent_completed",
        "One torrent finished, with its totals.",
    ),
    (
        "error",
        "Something failed. The same shape the final error document carries.",
    ),
    (
        "session_end",
        "The run is over. Always last, always present, whatever happened.",
    ),
];

/// Names the generator does not yet drive a run for.
///
/// Empty: every document kind and every event type has a run behind it. The
/// constant stays because the coverage test compares against it, so a name
/// that stops being produced fails the build here rather than quietly losing
/// its field table. See `TODO/cli-surface.md`, T-117.
pub const NOT_YET_COVERED: &[&str] = &[];

/// The JSON type of a value, as the document names it.
fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(number) => match number.is_f64() {
            true => "float",
            false => "integer",
        },
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Flatten a document into `path -> type`, one row per leaf.
///
/// Nested objects become dotted paths and arrays become `[]`, so
/// `sources[].scope` is one row however many sources a run had. An empty array
/// contributes its own row and nothing under it, because a run that produced
/// none cannot say what one holds.
pub fn fields(value: &Value) -> BTreeMap<String, &'static str> {
    let mut out = BTreeMap::new();
    walk("", value, &mut out);
    out
}

fn walk(prefix: &str, value: &Value, out: &mut BTreeMap<String, &'static str>) {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                let path = match prefix.is_empty() {
                    true => key.clone(),
                    false => format!("{prefix}.{key}"),
                };
                match child {
                    Value::Object(_) | Value::Array(_) => walk(&path, child, out),
                    _ => {
                        out.insert(path, type_of(child));
                    }
                }
            }
        }
        Value::Array(items) => {
            let path = format!("{prefix}[]");
            if items.is_empty() {
                out.insert(path, "array");
                return;
            }
            for item in items {
                match item {
                    Value::Object(_) | Value::Array(_) => walk(&path, item, out),
                    _ => {
                        out.insert(path.clone(), type_of(item));
                    }
                }
            }
        }
        _ => {
            out.insert(prefix.to_string(), type_of(value));
        }
    }
}

/// One field of a documented shape: its type, and who writes it.
#[derive(Clone, Debug)]
pub struct Field {
    /// The JSON type this run measured.
    pub kind: &'static str,
    /// Which of [`Sample::commands`] emit it, by the short name the `from`
    /// column carries.
    pub producers: BTreeSet<String>,
}

/// One documented shape: what produced it and what it carried.
pub struct Sample {
    /// The `kind` or event `type`.
    pub name: String,
    /// Every command that produced it, from the short name a `from` cell
    /// carries to the full label a reader can run.
    ///
    /// **More than one is normal for an event and impossible for a document.**
    /// `bit-cli seed --jsonl` and `bit-cli download --jsonl` both write
    /// `type: "progress"`, and the section below them differs in fifteen of
    /// its thirty-two rows; a
    /// document's `kind` is guarded against exactly that in
    /// `schema_gen::fold_document`. See `TODO/cli-surface.md`, T-257.
    pub commands: BTreeMap<String, String>,
    /// Every field, flattened.
    pub fields: BTreeMap<String, Field>,
}

impl Sample {
    /// The first observation of a shape.
    pub fn new(
        name: &str,
        producer: &str,
        command: &str,
        fields: BTreeMap<String, &'static str>,
    ) -> Self {
        let mut sample = Sample {
            name: name.to_string(),
            commands: BTreeMap::new(),
            fields: BTreeMap::new(),
        };
        sample.merge(producer, command, fields);
        sample
    }

    /// Fold another observation of the same shape in.
    ///
    /// One run rarely exercises every optional field: a download with no
    /// renamed paths omits `renamed`, and one with no sources omits
    /// `sources[]`. Several runs of the same command union together into the
    /// shape a reader should expect.
    ///
    /// **The union is attributed rather than anonymous**, which is the whole
    /// of T-257. Merging two commands' fields under one name and printing one
    /// command above the table describes a document neither one writes: that
    /// is what `docs/schema.md` said about `progress` for six field rows no
    /// `download` run has ever emitted. Every field remembers which producers
    /// wrote it, so a section for a shape two commands share says so.
    pub fn merge(&mut self, producer: &str, command: &str, other: BTreeMap<String, &'static str>) {
        self.commands
            .entry(producer.to_string())
            .or_insert_with(|| command.to_string());
        for (path, kind) in other {
            self.fields
                .entry(path)
                .and_modify(|field| {
                    field.producers.insert(producer.to_string());
                })
                .or_insert_with(|| Field {
                    kind,
                    producers: BTreeSet::from([producer.to_string()]),
                });
        }
    }

    /// The label a reader should copy, which is the first command seen.
    pub fn command(&self) -> &str {
        self.commands
            .values()
            .next()
            .map(String::as_str)
            .unwrap_or("")
    }
}

/// Render the whole document.
pub fn render(documents: &[Sample], events: &[Sample]) -> String {
    let mut out = String::new();
    out.push_str(HEADER);

    out.push_str("\n## Documents\n\nOne document per run, on stdout, when `--json` is given.\n");
    for (kind, description) in DOCUMENT_KINDS {
        out.push_str(&format!("\n### `{kind}`\n\n{description}\n"));
        match documents.iter().find(|sample| sample.name == *kind) {
            Some(sample) => out.push_str(&section(sample)),
            None => out.push_str(&format!(
                "\nNot covered by the generator yet, so its fields are not listed here. See\n`{SCHEMA_PATH}`'s note above.\n"
            )),
        }
    }

    out.push_str(
        "\n## Events\n\nOne object per line, on stdout, when `--jsonl` is given. Every event carries\n`type`, `seq`, and `at` before its own fields; `seq` counts from zero within a\nrun and `at` is ISO 8601 UTC with millisecond precision.\n",
    );
    for (event, description) in EVENT_TYPES {
        out.push_str(&format!("\n### `{event}`\n\n{description}\n"));
        match events.iter().find(|sample| sample.name == *event) {
            Some(sample) => out.push_str(&section(sample)),
            None => out.push_str(
                "\nNot produced by any run the generator drives, so its fields are not listed\nhere.\n",
            ),
        }
    }
    out
}

fn section(sample: &Sample) -> String {
    let labels: Vec<&str> = sample.commands.values().map(String::as_str).collect();
    if labels.len() < 2 {
        let mut out = format!(
            "\nFrom `{}`.\n\n| field | type |\n| --- | --- |\n",
            sample.command()
        );
        for (path, field) in &sample.fields {
            out.push_str(&format!("| `{path}` | {} |\n", field.kind));
        }
        return out;
    }

    // Two shapes under one name, which is what the data is: a progress tick
    // from a run that is downloading and one from a run that is not. The
    // alternative was renaming the event, which breaks every consumer
    // selecting `progress` today, and `schema_version` is what a break is for.
    // See `TODO/cli-surface.md`, T-257.
    let mut out = format!("\nFrom {}.\n", joined(&labels));
    let every = match labels.len() {
        2 => "both",
        _ => "all",
    };
    out.push_str(&format!(
        "\nMore than one command writes this shape and they do not carry the same\nfields. The `from` column names which of them writes each one, and reads\n`{every}` where every one of them does, so a consumer selecting by `type`\nalone knows what may be absent.\n",
    ));
    out.push_str("\n| field | type | from |\n| --- | --- | --- |\n");
    for (path, field) in &sample.fields {
        out.push_str(&format!(
            "| `{path}` | {} | {} |\n",
            field.kind,
            from_cell(field, labels.len())
        ));
    }
    out
}

/// What a `from` cell says: the commands that write the field, or one word
/// when every command in the section does.
///
/// A field every producer writes is the common case, and spelling all of them
/// out on every row makes the exceptions harder to find rather than easier.
/// `complete` is written by four commands here and they differ in one field.
fn from_cell(field: &Field, producers: usize) -> String {
    if field.producers.len() == producers {
        return match producers {
            2 => "both".to_string(),
            _ => "all".to_string(),
        };
    }
    field
        .producers
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

/// `` `a` ``, `` `a` and `b` ``, `` `a`, `b` and `c` ``.
fn joined(labels: &[&str]) -> String {
    let quoted: Vec<String> = labels.iter().map(|label| format!("`{label}`")).collect();
    match quoted.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

const HEADER: &str = r##"# The JSON contract

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
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattening_names_a_nested_field_by_its_path() {
        let value = serde_json::json!({
            "kind": "info",
            "total": { "bytes": 1024, "human": "1.00 KiB" },
            "trackers": ["udp://a", "udp://b"],
            "files": [{ "index": 0, "path": "a.bin" }],
            "nodes": [],
            "private": false,
        });
        let fields = fields(&value);
        assert_eq!(fields["kind"], "string");
        assert_eq!(fields["total.bytes"], "integer");
        assert_eq!(fields["total.human"], "string");
        assert_eq!(fields["trackers[]"], "string");
        assert_eq!(fields["files[].index"], "integer");
        assert_eq!(fields["files[].path"], "string");
        assert_eq!(
            fields["nodes[]"], "array",
            "an empty array says nothing more"
        );
        assert_eq!(fields["private"], "bool");
    }

    /// Two runs of one command rarely carry the same optional fields, so the
    /// union is what a reader should expect.
    #[test]
    fn merging_two_observations_keeps_every_field_either_one_had() {
        let mut sample = Sample::new(
            "download",
            "bit-cli download",
            "bit-cli download <TORRENT> --json",
            fields(&serde_json::json!({ "a": 1, "b": "x" })),
        );
        sample.merge(
            "bit-cli download",
            "bit-cli download <TORRENT> --json",
            fields(&serde_json::json!({ "b": "y", "c": true })),
        );
        assert_eq!(sample.fields.len(), 3);
        assert_eq!(sample.fields["a"].kind, "integer");
        assert_eq!(sample.fields["c"].kind, "bool");
        assert_eq!(sample.commands.len(), 1, "one command, one label");
    }

    /// A shape two commands write is rendered as two shapes, not as a union
    /// credited to whichever one ran first.
    ///
    /// This is T-257. `bit-cli seed --jsonl` and `bit-cli download --jsonl`
    /// both write `type: "progress"` and differ in fifteen of the section's
    /// thirty-two rows,
    /// and `docs/schema.md` listed the union under one `From` line, including
    /// six `listener.*` rows and `peer_detail[]` that no `download` run has
    /// ever emitted. The event keeps one `type`, because that is what
    /// consumers select on and breaking it is what `schema_version` is for.
    /// What changes is that the section says who writes what.
    #[test]
    fn a_shape_two_commands_write_names_which_one_writes_each_field() {
        let mut sample = Sample::new(
            "progress",
            "download",
            "bit-cli download <TORRENT> --jsonl",
            fields(&serde_json::json!({ "at": "t", "percent": "1.0" })),
        );
        sample.merge(
            "seed",
            "bit-cli seed <TORRENT> --jsonl",
            fields(&serde_json::json!({ "at": "t", "ratio": "1.000" })),
        );

        assert_eq!(sample.commands.len(), 2);
        assert_eq!(sample.fields["at"].producers.len(), 2, "both write it");
        assert!(sample.fields["percent"].producers.contains("download"));
        assert!(!sample.fields["percent"].producers.contains("seed"));

        let rendered = section(&sample);
        assert!(
            rendered.contains("| field | type | from |"),
            "a shared shape renders the from column: {rendered}"
        );
        assert!(
            rendered.contains("| `percent` | string | download |"),
            "and attributes a field only one of them writes: {rendered}"
        );
        assert!(
            rendered.contains("| `at` | string | both |"),
            "and says so in one word when every command writes it: {rendered}"
        );
        assert!(
            rendered.contains(
                "From `bit-cli download <TORRENT> --jsonl` and `bit-cli seed <TORRENT> --jsonl`."
            ),
            "naming both commands rather than one: {rendered}"
        );
    }

    /// One command still renders the two column table it always did.
    #[test]
    fn a_shape_one_command_writes_keeps_the_two_column_table() {
        let sample = Sample::new(
            "info",
            "info",
            "bit-cli info <TORRENT> --json",
            fields(&serde_json::json!({ "kind": "info" })),
        );
        let rendered = section(&sample);
        assert!(rendered.contains("| field | type |"), "{rendered}");
        assert!(!rendered.contains("| from |"), "{rendered}");
        assert!(
            rendered.contains("From `bit-cli info <TORRENT> --json`."),
            "{rendered}"
        );
    }

    /// Every name in the two tables is unique, so a section cannot be written
    /// twice and a reader cannot be given two answers.
    #[test]
    fn every_documented_name_appears_once() {
        for table in [DOCUMENT_KINDS, EVENT_TYPES] {
            let mut seen = std::collections::BTreeSet::new();
            for (name, description) in table {
                assert!(seen.insert(*name), "{name} is listed twice");
                assert!(!description.is_empty(), "{name} has no description");
            }
        }
    }
}
