//! Rendering results.
//!
//! Every command produces a value and hands it here. The renderer decides
//! whether that becomes a JSON document, a stream of NDJSON events, or lines a
//! person reads. Commands never write to a stream themselves, so the rule that
//! stdout carries data only holds by construction rather than by discipline.
//!
//! Every JSON document carries `schema_version`, `generated_at`, and
//! `bit_cli_version`. Errors in JSON mode are written to stdout as an object
//! *and* produce a non-zero exit code, so a caller sees both.

use std::io::Write;

use bit_cli_core::error::{Error, ErrorReport, Result};
use bit_cli_core::time::Timestamp;
use serde::Serialize;
use serde_json::Value;

use crate::cli::{Global, ProgressMode};
use crate::env::Env;

/// The version of the JSON contract.
///
/// It changes when a field is removed or its meaning changes. Adding a field
/// is not a breaking change and does not bump it.
pub const SCHEMA_VERSION: &str = "1";

/// How output is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Lines a person reads.
    Text,
    /// One JSON document.
    Json,
    /// One JSON object per line, as things happen.
    Jsonl,
}

impl Format {
    /// Whether this format is machine-readable.
    pub const fn is_machine(self) -> bool {
        matches!(self, Self::Json | Self::Jsonl)
    }
}

/// Output settings resolved from the global flags.
#[derive(Debug, Clone)]
pub struct Renderer {
    pub format: Format,
    pub quiet: bool,
    pub color: bool,
    pub progress: ProgressMode,
    /// `--stats`: render the document field by field rather than the summary.
    pub stats: bool,
    next_seq: u64,
}

impl Renderer {
    /// Build from the global flags and the environment.
    pub fn new(global: &Global, env: &Env) -> Self {
        let format = match (global.json, global.jsonl) {
            (_, true) => Format::Jsonl,
            (true, _) => Format::Json,
            _ => Format::Text,
        };
        // `--json` implies no progress bar: progress on stdout would corrupt
        // the document, and a caller asking for JSON is not watching anyway.
        let progress = match (global.progress, format) {
            (ProgressMode::Auto, Format::Text) if env.out_is_terminal => ProgressMode::Plain,
            (ProgressMode::Auto, _) => ProgressMode::None,
            (explicit, _) => explicit,
        };
        Self {
            format,
            quiet: global.quiet,
            color: env.wants_color(global.color.into()),
            progress,
            stats: global.stats,
            next_seq: 0,
        }
    }

    /// Wrap a payload in the standard document envelope.
    pub fn envelope(&self, kind: &str, payload: Value) -> Value {
        let mut doc = serde_json::Map::new();
        doc.insert(
            "schema_version".into(),
            Value::String(SCHEMA_VERSION.into()),
        );
        doc.insert(
            "bit_cli_version".into(),
            Value::String(bit_cli_core::VERSION.into()),
        );
        doc.insert("generated_at".into(), Value::String(Timestamp::now().iso()));
        doc.insert("kind".into(), Value::String(kind.into()));
        if let Value::Object(fields) = payload {
            for (key, value) in fields {
                doc.insert(key, value);
            }
        } else {
            doc.insert("data".into(), payload);
        }
        Value::Object(doc)
    }

    /// Emit a result: one JSON document, or the text rendering.
    ///
    /// `text` is only called when the format is text, so a command never pays
    /// to build a human rendering nobody will read.
    pub fn emit<T: Serialize>(
        &self,
        env: &mut Env,
        kind: &str,
        value: &T,
        text: impl FnOnce() -> Vec<String>,
    ) -> Result<()> {
        match self.format {
            Format::Text => {
                if self.quiet {
                    return Ok(());
                }
                // `--stats` renders the document rather than the command's own
                // summary. The numbers are the same numbers: the summary is a
                // selection from this, and the selection is what a caller
                // reaching for `--stats` is asking to see past.
                let lines = match self.stats {
                    true => match serde_json::to_value(value) {
                        Ok(payload) => stats_lines(&self.envelope(kind, payload)),
                        // A document that will not serialize cannot be
                        // rendered field by field, and refusing to print
                        // anything would be worse than printing the summary.
                        Err(_) => text(),
                    },
                    false => text(),
                };
                for line in lines {
                    env.say(line)
                        .map_err(|e| bit_cli_core::error::from_io(e, "cannot write to stdout"))?;
                }
                Ok(())
            }
            Format::Json | Format::Jsonl => {
                let payload = serde_json::to_value(value)
                    .map_err(|e| Error::generic(format!("cannot serialize the result: {e}")))?;
                let doc = self.envelope(kind, payload);
                let rendered = match self.format {
                    Format::Json => serde_json::to_string_pretty(&doc),
                    _ => serde_json::to_string(&doc),
                }
                .map_err(|e| Error::generic(format!("cannot render JSON: {e}")))?;
                env.say(rendered)
                    .map_err(|e| bit_cli_core::error::from_io(e, "cannot write to stdout"))
            }
        }
    }

    /// Emit one NDJSON event, with a monotonic sequence number.
    ///
    /// Events go nowhere in text mode: they are a machine surface, and the
    /// text rendering of progress is the progress display.
    pub fn event<T: Serialize>(
        &mut self,
        env: &mut Env,
        event_type: &str,
        value: &T,
    ) -> Result<()> {
        if self.format != Format::Jsonl {
            return Ok(());
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        let mut doc = serde_json::Map::new();
        doc.insert("type".into(), Value::String(event_type.into()));
        doc.insert("seq".into(), Value::Number(seq.into()));
        doc.insert("at".into(), Value::String(Timestamp::now().iso()));
        if let Ok(Value::Object(fields)) = serde_json::to_value(value) {
            for (key, value) in fields {
                doc.insert(key, value);
            }
        }
        let rendered = serde_json::to_string(&Value::Object(doc))
            .map_err(|e| Error::generic(format!("cannot render an event: {e}")))?;
        env.say(rendered)
            .map_err(|e| bit_cli_core::error::from_io(e, "cannot write to stdout"))
    }

    /// Write a warning to stderr, unless quiet.
    pub fn warn(&self, env: &mut Env, message: impl AsRef<str>) {
        if self.quiet {
            return;
        }
        let _ = env.note(format!("warning: {}", message.as_ref()));
    }

    /// Report a failure.
    ///
    /// In JSON mode the error object goes to stdout, because a caller reading
    /// stdout must be able to see what went wrong without also capturing
    /// stderr. In text mode it goes to stderr, because stdout carries data.
    /// Either way the process exits non-zero.
    pub fn fail(&self, env: &mut Env, error: &Error) {
        let report = error.report();
        if self.format.is_machine() {
            let doc = self.envelope("error", serde_json::to_value(&report).unwrap_or_default());
            let rendered = match self.format {
                Format::Json => serde_json::to_string_pretty(&doc),
                _ => serde_json::to_string(&doc),
            };
            if let Ok(text) = rendered {
                let _ = env.say(text);
            }
        }
        let _ = env.note(format!("error: {error}"));
        let _ = env.err.flush();
        let _ = env.out.flush();
        let _ = &report;
    }
}

/// A key and value for the plain text rendering.
///
/// Text output is aligned so a column of values lines up. Nothing is
/// truncated: a path that is too wide wraps in the terminal rather than losing
/// characters, because a silently shortened path is a wrong path.
pub fn field(key: &str, value: impl std::fmt::Display) -> String {
    format!("{key:<20} {value}")
}

/// Every field of a document, one per line, for `--stats`.
///
/// Paths are the ones `docs/schema.md` names, so a line here and a row there
/// are the same field: dotted for an object, `[n]` for an array element. A
/// `null` is skipped, because the document omits an optional field rather than
/// writing `null` and a reader should not have to tell "not applicable" from
/// "none". An empty array or object prints as itself, because "this run had
/// none" is an answer.
pub fn stats_lines(value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk_stats("", value, &mut out);
    out
}

fn walk_stats(prefix: &str, value: &Value, out: &mut Vec<String>) {
    let join = |key: &str| match prefix.is_empty() {
        true => key.to_string(),
        false => format!("{prefix}.{key}"),
    };
    match value {
        Value::Object(fields) if fields.is_empty() => out.push(field(prefix, "{}")),
        Value::Object(fields) => {
            for (key, child) in fields {
                walk_stats(&join(key), child, out);
            }
        }
        Value::Array(items) if items.is_empty() => out.push(field(prefix, "[]")),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk_stats(&format!("{prefix}[{index}]"), item, out);
            }
        }
        Value::Null => {}
        Value::String(text) => out.push(field(prefix, text)),
        other => out.push(field(prefix, other)),
    }
}

/// Render a table with aligned columns.
pub fn table(headers: &[&str], rows: &[Vec<String>]) -> Vec<String> {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            if index < widths.len() {
                widths[index] = widths[index].max(cell.chars().count());
            }
        }
    }
    let render = |cells: &[String]| {
        cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let width = widths.get(index).copied().unwrap_or(0);
                // The last column is never padded, so lines have no trailing
                // whitespace and diffing two runs is clean.
                match index + 1 == cells.len() {
                    true => cell.clone(),
                    false => format!("{cell:<width$}"),
                }
            })
            .collect::<Vec<_>>()
            .join("  ")
    };
    let mut out = vec![render(
        &headers.iter().map(|h| h.to_string()).collect::<Vec<_>>(),
    )];
    out.extend(rows.iter().map(|row| render(row)));
    out
}

/// The error report shape, re-exported so tests can name it.
pub type Report = ErrorReport;

#[cfg(test)]
mod tests {
    use super::*;

    /// `--stats` names every field the way `docs/schema.md` names it, so a
    /// line here and a row there are the same field.
    #[test]
    fn stats_lines_name_a_field_the_way_the_schema_does() {
        let value = serde_json::json!({
            "kind": "download",
            "total": {"bytes": 1024, "human": "1.00 KiB"},
            "torrents": [{"source": "a.torrent"}, {"source": "b.torrent"}],
        });
        let lines = stats_lines(&value);
        assert!(lines.iter().any(|l| l.starts_with("kind ")), "{lines:?}");
        assert!(
            lines.iter().any(|l| l.starts_with("total.bytes ")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.starts_with("torrents[1].source ")),
            "{lines:?}"
        );
    }

    /// A `null` is skipped and an empty collection is printed.
    ///
    /// The document omits an optional field rather than writing `null`, so a
    /// `null` that does reach here carries no information. An empty array is
    /// different: "this run had none" is an answer.
    #[test]
    fn a_null_is_skipped_and_an_empty_collection_is_not() {
        let value = serde_json::json!({
            "name": serde_json::Value::Null,
            "nodes": [],
            "context": {},
            "count": 0,
        });
        let lines = stats_lines(&value);
        assert!(!lines.iter().any(|l| l.starts_with("name ")), "{lines:?}");
        assert!(
            lines.iter().any(|l| l.trim() == "nodes                []"),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.trim() == "context              {}"),
            "{lines:?}"
        );
        assert!(lines.iter().any(|l| l.starts_with("count ")), "{lines:?}");
    }

    use crate::cli::{Cli, Command};
    use clap::Parser;

    fn global(args: &[&str]) -> Global {
        let mut full = vec!["bit-cli"];
        full.extend_from_slice(args);
        full.push("info");
        full.push("x.torrent");
        let cli = Cli::try_parse_from(full).unwrap();
        assert!(matches!(cli.command, Some(Command::Info(_))));
        cli.global
    }

    #[test]
    fn the_format_follows_the_flags() {
        let (env, _) = Env::test(&[], "/w");
        assert_eq!(Renderer::new(&global(&[]), &env).format, Format::Text);
        assert_eq!(
            Renderer::new(&global(&["--json"]), &env).format,
            Format::Json
        );
        assert_eq!(
            Renderer::new(&global(&["--jsonl"]), &env).format,
            Format::Jsonl
        );
    }

    #[test]
    fn progress_is_off_when_stdout_is_not_a_terminal() {
        let (env, _) = Env::test(&[], "/w");
        assert_eq!(
            Renderer::new(&global(&[]), &env).progress,
            ProgressMode::None
        );
    }

    #[test]
    fn progress_is_on_at_a_terminal_and_off_in_json_mode() {
        let (mut env, _) = Env::test(&[], "/w");
        env.out_is_terminal = true;
        assert_eq!(
            Renderer::new(&global(&[]), &env).progress,
            ProgressMode::Plain
        );
        assert_eq!(
            Renderer::new(&global(&["--json"]), &env).progress,
            ProgressMode::None
        );
    }

    #[test]
    fn an_explicit_progress_mode_is_always_honoured() {
        let (env, _) = Env::test(&[], "/w");
        assert_eq!(
            Renderer::new(&global(&["--progress", "plain"]), &env).progress,
            ProgressMode::Plain
        );
    }

    #[test]
    fn every_document_carries_the_envelope_fields() {
        let (env, _) = Env::test(&[], "/w");
        let renderer = Renderer::new(&global(&["--json"]), &env);
        let doc = renderer.envelope("info", serde_json::json!({ "name": "x" }));
        assert_eq!(doc["schema_version"], SCHEMA_VERSION);
        assert_eq!(doc["bit_cli_version"], bit_cli_core::VERSION);
        assert_eq!(doc["kind"], "info");
        assert_eq!(doc["name"], "x");
        assert!(doc["generated_at"].as_str().unwrap().ends_with('Z'));
    }

    #[test]
    fn a_non_object_payload_lands_under_data() {
        let (env, _) = Env::test(&[], "/w");
        let renderer = Renderer::new(&global(&["--json"]), &env);
        let doc = renderer.envelope("magnet", serde_json::json!("magnet:?xt=x"));
        assert_eq!(doc["data"], "magnet:?xt=x");
    }

    #[test]
    fn text_mode_writes_lines_and_json_mode_writes_a_document() {
        let (mut env, captured) = Env::test(&[], "/w");
        let renderer = Renderer::new(&global(&[]), &env);
        renderer
            .emit(&mut env, "info", &serde_json::json!({"a": 1}), || {
                vec!["a 1".into()]
            })
            .unwrap();
        assert_eq!(captured.out(), "a 1\n");

        let (mut env, captured) = Env::test(&[], "/w");
        let renderer = Renderer::new(&global(&["--json"]), &env);
        renderer
            .emit(&mut env, "info", &serde_json::json!({"a": 1}), || {
                vec!["never".into()]
            })
            .unwrap();
        assert_eq!(captured.json().unwrap()["a"], 1);
    }

    #[test]
    fn quiet_silences_text_but_not_json() {
        let (mut env, captured) = Env::test(&[], "/w");
        let renderer = Renderer::new(&global(&["--quiet"]), &env);
        renderer
            .emit(&mut env, "info", &serde_json::json!({}), || {
                vec!["hi".into()]
            })
            .unwrap();
        assert_eq!(captured.out(), "");

        let (mut env, captured) = Env::test(&[], "/w");
        let renderer = Renderer::new(&global(&["--quiet", "--json"]), &env);
        renderer
            .emit(&mut env, "info", &serde_json::json!({"a": 1}), Vec::new)
            .unwrap();
        assert_eq!(
            captured.json().unwrap()["a"],
            1,
            "a machine caller still gets its data"
        );
    }

    #[test]
    fn events_carry_a_monotonic_sequence_and_only_appear_in_jsonl() {
        let (mut env, captured) = Env::test(&[], "/w");
        let mut renderer = Renderer::new(&global(&["--jsonl"]), &env);
        for _ in 0..3 {
            renderer
                .event(&mut env, "progress", &serde_json::json!({"done": 1}))
                .unwrap();
        }
        let events = captured.jsonl().unwrap();
        assert_eq!(events.len(), 3);
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event["seq"], index);
            assert_eq!(event["type"], "progress");
            assert!(event["at"].as_str().unwrap().ends_with('Z'));
        }

        let (mut env, captured) = Env::test(&[], "/w");
        let mut renderer = Renderer::new(&global(&["--json"]), &env);
        renderer
            .event(&mut env, "progress", &serde_json::json!({}))
            .unwrap();
        assert_eq!(captured.out(), "", "events are a jsonl-only surface");
    }

    #[test]
    fn a_failure_in_json_mode_reaches_stdout_and_stderr() {
        let (mut env, captured) = Env::test(&[], "/w");
        let renderer = Renderer::new(&global(&["--json"]), &env);
        let error = bit_cli_core::error::Error::coverage_gap("pieces 1-2 have no source")
            .with("uncovered_pieces", vec![1, 2]);
        renderer.fail(&mut env, &error);

        let doc = captured.json().unwrap();
        assert_eq!(doc["code"], 11);
        assert_eq!(doc["kind"], "coverage_gap");
        // Machine-readable detail lives under `context`, so a caller reads a
        // field instead of parsing the message.
        assert_eq!(
            doc["context"]["uncovered_pieces"],
            serde_json::json!([1, 2])
        );
        assert_eq!(doc["schema_version"], SCHEMA_VERSION);
        assert!(captured.err().contains("pieces 1-2 have no source"));
    }

    #[test]
    fn a_failure_in_text_mode_keeps_stdout_clean() {
        let (mut env, captured) = Env::test(&[], "/w");
        let renderer = Renderer::new(&global(&[]), &env);
        renderer.fail(&mut env, &bit_cli_core::error::Error::network("no route"));
        assert_eq!(captured.out(), "", "stdout carries data only");
        assert!(captured.err().contains("error: no route"));
    }

    #[test]
    fn warnings_never_reach_stdout() {
        let (mut env, captured) = Env::test(&[], "/w");
        let renderer = Renderer::new(&global(&[]), &env);
        renderer.warn(&mut env, "a mirror is slow");
        assert_eq!(captured.out(), "");
        assert!(captured.err().contains("warning: a mirror is slow"));
    }

    #[test]
    fn tables_align_columns_without_trailing_whitespace() {
        let rows = vec![
            vec!["0".into(), "a.bin".into(), "1.00 KiB".into()],
            vec![
                "1".into(),
                "a-much-longer-name.bin".into(),
                "2.00 MiB".into(),
            ],
        ];
        let lines = table(&["INDEX", "PATH", "SIZE"], &rows);
        assert_eq!(lines.len(), 3);
        for line in &lines {
            assert_eq!(line.trim_end(), *line, "trailing whitespace in {line:?}");
        }
        // The PATH column is padded to the widest entry, so SIZE lines up.
        let size_column = lines[1].find("1.00 KiB").unwrap();
        assert_eq!(lines[2].find("2.00 MiB"), Some(size_column));
    }

    #[test]
    fn fields_are_aligned_to_a_fixed_column() {
        assert_eq!(field("name", "album"), format!("{:<20} album", "name"));
    }
}
