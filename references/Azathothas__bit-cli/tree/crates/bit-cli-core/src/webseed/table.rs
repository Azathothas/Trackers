//! The binding table: a file that declares sources, scopes, and compositions.
//!
//! Anything expressible on the command line is expressible here, and a few
//! things only a file expresses cleanly: per-source headers, per-source rate
//! limits, and a dozen sources without a dozen repeated flags.
//!
//! TOML and JSON are both accepted with the same schema, so a generator emits
//! whichever is easier. The format is detected from the file extension, then
//! from the content, so a `.conf` holding JSON still works.
//!
//! ```toml
//! [[source]]
//! url         = "https://mirror-a.example.com/pub/"
//! scope       = "*"
//! mode        = "auto"
//! priority    = 10
//! concurrency = 8
//!
//! [[source]]
//! url   = "https://cdn.example.com/blobs/a3f1b2/payload.bin"
//! scope = "file:0"
//! mode  = "exact"
//!
//! [[source]]
//! url     = "https://partial.example.com/chunks/{piece}.bin"
//! scope   = "piece:0-2047"
//! mode    = "template"
//! headers = { X-Region = "apac" }
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Context, Error, Result, from_io};
use crate::units::{parse_rate, parse_size};
use crate::webseed::binding::{Auth, Origin, SourceLimits, SourceSpec, StatusSet, Style};
use crate::webseed::composition::Mode;
use crate::webseed::scope::Scope;

/// A parsed binding table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Table {
    /// Defaults applied to every source that does not override them.
    #[serde(default, alias = "defaults")]
    pub default: Defaults,
    /// The sources, in order. Order breaks priority ties.
    #[serde(default, alias = "sources")]
    pub source: Vec<Entry>,
}

/// Table-wide defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connections: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_errors: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_status: Option<StatusSet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fatal_status: Option<StatusSet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

/// One `[[source]]` entry.
///
/// Sizes and rates are strings so they can carry binary units, which is the
/// whole point of writing `chunk_size = "4MiB"` instead of `4194304`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Base URL, or the template when `mode` is `template`.
    pub url: String,
    /// Info hash of the one torrent this source is for. Absent means every
    /// torrent in the invocation, which is the default and what a single
    /// torrent run wants.
    ///
    /// The same file sits at a different index in two different torrents, so a
    /// run over both needs to say which one a binding means. See
    /// `TODO/multi-source.md`, T-133.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torrent: Option<String>,
    /// Scope selector. Defaults to the whole torrent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Composition mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    /// Template, when it is not `url` itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Wire style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,
    /// Bias among sources. Higher wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    /// Concurrent ranged requests against this source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<usize>,
    /// Peer connections this source is presented over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connections: Option<usize>,
    /// Bytes per ranged request, with units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_size: Option<String>,
    /// Rate cap, with units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<String>,
    /// Per-request timeout in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Connect timeout in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_timeout_ms: Option<u64>,
    /// Per-request retries before an attempt counts as an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,
    /// Errors before the source is cooled down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_errors: Option<u32>,
    /// How long a cooled-down source stays out, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_ms: Option<u64>,
    /// Statuses worth retrying that would otherwise retire the source, as
    /// codes and inclusive ranges: `[403, 429]`, `["500-599"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_status: Option<StatusSet>,
    /// Statuses that retire the source that would otherwise be retried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fatal_status: Option<StatusSet>,
    /// User-Agent for this source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// Extra request headers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// Credentials, as `basic:user:pass`, `bearer:TOKEN`, `netrc`, or `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
}

impl Table {
    /// Read a table from a file, detecting TOML or JSON.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            from_io(
                e,
                format!("cannot read the binding table {}", path.display()),
            )
        })?;
        let json_by_extension = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"));
        Self::parse(&text, json_by_extension)
            .with_context(|| format!("cannot parse the binding table {}", path.display()))
    }

    /// Parse a table from text.
    ///
    /// `prefer_json` comes from the file extension. When it is false the
    /// content still decides, so a JSON table in a `.conf` parses.
    ///
    /// Only a leading `{` is taken as a JSON signal. A leading `[` is how
    /// every TOML table starts (`[default]`, `[[source]]`), and a top-level
    /// JSON array could not be a table anyway.
    pub fn parse(text: &str, prefer_json: bool) -> Result<Self> {
        let looks_like_json = text.trim_start().starts_with('{');
        if prefer_json || looks_like_json {
            return serde_json::from_str(text)
                .map_err(|e| Error::config(format!("invalid JSON: {e}")));
        }
        toml::from_str(text).map_err(|e| Error::config(format!("invalid TOML: {e}")))
    }

    /// Turn the table into source specs.
    ///
    /// Every entry is validated here rather than at first use, so a typo in
    /// entry nine is reported before entry one issues a request.
    /// Turn every entry into a spec, keeping only those that apply to this
    /// torrent.
    ///
    /// `torrent` is the info hash of the torrent the specs are for, or `None`
    /// when it is not known yet. An entry with no `torrent` field applies to
    /// every torrent; one that names a hash applies to that torrent alone, and
    /// naming a hash at all is an error when the caller does not yet know
    /// which torrent it has. See `TODO/multi-source.md`, T-133.
    pub fn into_specs(self, origin: Origin, torrent: Option<&str>) -> Result<Vec<SourceSpec>> {
        let defaults = self.default;
        let mut specs = Vec::new();
        for (index, entry) in self.source.into_iter().enumerate() {
            if let Some(wanted) = entry.torrent.as_deref() {
                let Some(have) = torrent else {
                    return Err(Error::config(format!(
                        "source {index} in the binding table names torrent {wanted}, and this source's info hash is not known until its metadata resolves"
                    )));
                };
                if !have.eq_ignore_ascii_case(wanted) {
                    continue;
                }
            }
            specs.push(
                entry
                    .into_spec(&defaults, origin)
                    .with_context(|| format!("source {index} in the binding table is invalid"))?,
            );
        }
        Ok(specs)
    }
}

impl Entry {
    fn into_spec(self, defaults: &Defaults, origin: Origin) -> Result<SourceSpec> {
        let base = SourceLimits::default();
        let size = |value: Option<&String>, fallback: u64, what: &str| -> Result<u64> {
            match value {
                None => Ok(fallback),
                Some(text) => parse_size(text)
                    .map_err(|e| Error::config(format!("{what}: {e}")).with("value", text.clone())),
            }
        };
        let rate = |value: Option<&String>| -> Result<Option<u64>> {
            match value {
                None => Ok(None),
                Some(text) => parse_rate(text).map(Some).map_err(|e| {
                    Error::config(format!("rate_limit: {e}")).with("value", text.clone())
                }),
            }
        };

        let chunk_size = size(
            self.chunk_size.as_ref().or(defaults.chunk_size.as_ref()),
            base.chunk_size,
            "chunk_size",
        )?;
        if chunk_size == 0 {
            return Err(Error::config("chunk_size must be at least one byte"));
        }
        let rate_limit = rate(self.rate_limit.as_ref().or(defaults.rate_limit.as_ref()))?;
        let concurrency = self
            .concurrency
            .or(defaults.concurrency)
            .unwrap_or(base.concurrency);
        let connections = self
            .connections
            .or(defaults.connections)
            .unwrap_or(base.connections);
        if connections == 0 {
            return Err(Error::config("connections must be at least 1"));
        }
        if concurrency == 0 {
            return Err(Error::config("concurrency must be at least 1"));
        }

        let mode = self.mode.or(defaults.mode).unwrap_or_default();
        let scope = match self.scope.as_deref() {
            None => Scope::all(),
            Some(text) => Scope::parse(text)?,
        };

        // Entry headers win over table defaults key by key, so a table can set
        // one shared header and a single source can still override it.
        let mut headers = defaults.headers.clone();
        headers.extend(self.headers);

        let spec = SourceSpec {
            url: self.url,
            scope,
            mode,
            template: self.template,
            style: self.style.or(defaults.style).unwrap_or_default(),
            priority: self.priority.unwrap_or(0),
            headers,
            user_agent: self.user_agent.or_else(|| defaults.user_agent.clone()),
            auth: match self.auth.as_deref() {
                None => Auth::None,
                Some(spec) => Auth::parse(spec)?,
            },
            limits: SourceLimits {
                concurrency,
                connections,
                chunk_size,
                timeout_ms: self
                    .timeout_ms
                    .or(defaults.timeout_ms)
                    .unwrap_or(base.timeout_ms),
                connect_timeout_ms: self
                    .connect_timeout_ms
                    .or(defaults.connect_timeout_ms)
                    .unwrap_or(base.connect_timeout_ms),
                retries: self.retries.or(defaults.retries).unwrap_or(base.retries),
                max_errors: self
                    .max_errors
                    .or(defaults.max_errors)
                    .unwrap_or(base.max_errors),
                cooldown_ms: self
                    .cooldown_ms
                    .or(defaults.cooldown_ms)
                    .unwrap_or(base.cooldown_ms),
                rate_limit,
                retry_status: self
                    .retry_status
                    .or_else(|| defaults.retry_status.clone())
                    .unwrap_or_default(),
                fatal_status: self
                    .fatal_status
                    .or_else(|| defaults.fatal_status.clone())
                    .unwrap_or_default(),
            },
            origin,
        };
        spec.limits.check_status_policy()?;
        Ok(spec)
    }
}

/// Read a newline-separated URL list.
///
/// Blank lines and `#` comments are ignored. This is the format of
/// `--web-seed-file` and `--tracker-file`, and the body of whatever
/// `--web-seed-list-url` returns.
pub fn parse_url_list(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// Read a newline-separated tracker list, grouped into BEP 12 tiers.
///
/// A blank line separates tiers, which is the convention every tracker list on
/// the internet already uses. Comments do not break a tier.
pub fn parse_tier_list(text: &str) -> Vec<Vec<String>> {
    let mut tiers: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            // A comment-only line leaves `line` empty too, so only a truly
            // blank source line should break the tier.
            if raw.trim().is_empty() && !current.is_empty() {
                tiers.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(line.to_string());
    }
    if !current.is_empty() {
        tiers.push(current);
    }
    tiers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::MIB;

    const TOML: &str = r#"
[default]
concurrency = 6
user_agent  = "bit-cli/0.1.0"

[[source]]
url         = "https://mirror-a.example.com/pub/"
scope       = "*"
mode        = "auto"
priority    = 10
concurrency = 8

[[source]]
url   = "https://cdn.example.com/blobs/a3f1b2/payload.bin"
scope = "file:0"
mode  = "exact"

[[source]]
url        = "https://slow.example.com/iso/"
scope      = "*"
mode       = "prefix"
priority   = 1
rate_limit = "5MiB/s"
chunk_size = "1MiB"
headers    = { X-Region = "apac" }
"#;

    #[test]
    fn a_toml_table_parses_into_specs() {
        let table = Table::parse(TOML, false).unwrap();
        let specs = table.into_specs(Origin::Config, None).unwrap();
        assert_eq!(specs.len(), 3);

        assert_eq!(specs[0].url, "https://mirror-a.example.com/pub/");
        assert_eq!(specs[0].mode, Mode::Auto);
        assert_eq!(specs[0].priority, 10);
        assert_eq!(
            specs[0].limits.concurrency, 8,
            "an entry overrides the default"
        );

        assert_eq!(specs[1].mode, Mode::Exact);
        assert_eq!(specs[1].scope.text(), "file:0");
        assert_eq!(
            specs[1].limits.concurrency, 6,
            "the default applies where the entry is silent"
        );

        assert_eq!(specs[2].limits.rate_limit, Some(5 * MIB));
        assert_eq!(specs[2].limits.chunk_size, MIB);
        assert_eq!(
            specs[2].headers.get("X-Region").map(String::as_str),
            Some("apac")
        );
    }

    #[test]
    fn defaults_reach_every_entry() {
        let specs = Table::parse(TOML, false)
            .unwrap()
            .into_specs(Origin::Config, None)
            .unwrap();
        for spec in &specs {
            assert_eq!(spec.user_agent.as_deref(), Some("bit-cli/0.1.0"));
        }
    }

    #[test]
    fn json_and_toml_give_the_same_result() {
        let json = r#"{
          "default": { "concurrency": 6, "user_agent": "bit-cli/0.1.0" },
          "source": [
            { "url": "https://mirror-a.example.com/pub/", "scope": "*", "mode": "auto", "priority": 10, "concurrency": 8 },
            { "url": "https://cdn.example.com/blobs/a3f1b2/payload.bin", "scope": "file:0", "mode": "exact" },
            { "url": "https://slow.example.com/iso/", "scope": "*", "mode": "prefix", "priority": 1,
              "rate_limit": "5MiB/s", "chunk_size": "1MiB", "headers": { "X-Region": "apac" } }
          ]
        }"#;
        let from_json = Table::parse(json, true)
            .unwrap()
            .into_specs(Origin::Config, None)
            .unwrap();
        let from_toml = Table::parse(TOML, false)
            .unwrap()
            .into_specs(Origin::Config, None)
            .unwrap();
        assert_eq!(from_json, from_toml);
    }

    #[test]
    fn json_is_detected_from_the_content_when_the_extension_lies() {
        let json = r#"{ "source": [ { "url": "https://e.com/" } ] }"#;
        let table = Table::parse(json, false).unwrap();
        assert_eq!(table.source.len(), 1);
    }

    #[test]
    fn the_plural_key_names_also_work() {
        let toml = r#"
[defaults]
concurrency = 2

[[sources]]
url = "https://e.com/"
"#;
        let specs = Table::parse(toml, false)
            .unwrap()
            .into_specs(Origin::Config, None)
            .unwrap();
        assert_eq!(specs[0].limits.concurrency, 2);
    }

    #[test]
    fn an_unknown_key_is_reported_rather_than_ignored() {
        let toml = "[[source]]\nurl = \"https://e.com/\"\nconcurency = 4\n";
        let err = Table::parse(toml, false).unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::Config);
        assert!(err.message().contains("concurency"), "{}", err.message());
    }

    #[test]
    fn a_bad_size_names_the_field_and_the_value() {
        let toml = "[[source]]\nurl = \"https://e.com/\"\nchunk_size = \"4 potatoes\"\n";
        let err = Table::parse(toml, false)
            .unwrap()
            .into_specs(Origin::Config, None)
            .unwrap_err();
        assert_eq!(err.code(), crate::exit::ExitCode::Config);
        assert!(err.to_string().contains("chunk_size"), "{err}");
    }

    #[test]
    fn a_bad_scope_is_caught_when_the_table_is_read() {
        let toml = "[[source]]\nurl = \"https://e.com/\"\nscope = \"piece:9-2\"\n";
        assert!(
            Table::parse(toml, false)
                .unwrap()
                .into_specs(Origin::Config, None)
                .is_err()
        );
    }

    #[test]
    fn zero_valued_limits_are_refused() {
        for bad in ["concurrency = 0", "chunk_size = \"0\""] {
            let toml = format!("[[source]]\nurl = \"https://e.com/\"\n{bad}\n");
            assert!(
                Table::parse(&toml, false)
                    .unwrap()
                    .into_specs(Origin::Config, None)
                    .is_err(),
                "{bad} should be refused"
            );
        }
    }

    #[test]
    fn entry_headers_win_over_table_headers() {
        let toml = r#"
[default]
headers = { X-Region = "eu", X-Trace = "on" }

[[source]]
url     = "https://e.com/"
headers = { X-Region = "apac" }
"#;
        let specs = Table::parse(toml, false)
            .unwrap()
            .into_specs(Origin::Config, None)
            .unwrap();
        assert_eq!(specs[0].headers["X-Region"], "apac");
        assert_eq!(specs[0].headers["X-Trace"], "on");
    }

    #[test]
    fn an_empty_table_yields_no_sources() {
        assert!(
            Table::parse("", false)
                .unwrap()
                .into_specs(Origin::Config, None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn url_lists_drop_blanks_and_comments() {
        let text = "# mirrors\nhttps://a.example.com/\n\n  https://b.example.com/  # eu\n\n#\n";
        assert_eq!(
            parse_url_list(text),
            vec![
                "https://a.example.com/".to_string(),
                "https://b.example.com/".to_string()
            ]
        );
    }

    #[test]
    fn tracker_lists_split_into_tiers_on_blank_lines() {
        let text = "udp://a:80\nudp://b:80\n\nudp://c:80\n";
        assert_eq!(
            parse_tier_list(text),
            vec![
                vec!["udp://a:80".to_string(), "udp://b:80".to_string()],
                vec!["udp://c:80".to_string()],
            ]
        );
    }

    #[test]
    fn a_comment_line_does_not_break_a_tier() {
        let text = "udp://a:80\n# still tier one\nudp://b:80\n";
        assert_eq!(parse_tier_list(text).len(), 1);
        assert_eq!(parse_tier_list(text)[0].len(), 2);
    }

    #[test]
    fn repeated_blank_lines_do_not_make_empty_tiers() {
        assert_eq!(parse_tier_list("udp://a:80\n\n\n\nudp://b:80\n").len(), 2);
        assert!(parse_tier_list("\n\n\n").is_empty());
    }

    #[test]
    fn a_table_round_trips_through_toml() {
        let table = Table::parse(TOML, false).unwrap();
        let rendered = toml::to_string(&table).unwrap();
        let back = Table::parse(&rendered, false).unwrap();
        assert_eq!(
            back.into_specs(Origin::Config, None).unwrap(),
            table.into_specs(Origin::Config, None).unwrap()
        );
    }
}
