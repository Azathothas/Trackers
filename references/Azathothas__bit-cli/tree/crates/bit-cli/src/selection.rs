//! Turning `--select-file` and `--exclude-file` into explicit file indices.
//!
//! Shared because two commands need the same answer from the same two flags:
//! `download` decides what to fetch and `verify` decides what a piece outside
//! the selection means. A second copy of this would be a second set of
//! off-by-one bugs.
//!
//! The one thing that differs between callers is whether the file count is
//! known yet. `verify` reads a `.torrent` off the disk and knows it before it
//! parses a flag; `download` may be handed a magnet, where the file list does
//! not exist until the metadata resolves over the network. Two forms need the
//! count and nothing else does: an exclusion with no selection beside it, and
//! an open-ended range. [`needs_file_count`] is which, asked before a source is
//! added so a magnet's metadata is resolved first rather than guessed at. See
//! `TODO/cli-surface.md`, T-185.

use std::collections::{BTreeMap, HashSet};

use bit_cli_core::error::{Error, Result};

/// Resolve the two flags into the file indices to work on.
///
/// `None` means every file, which is not the same as an empty list: an empty
/// list would select nothing at all, and that is a usage error.
///
/// `file_count` is the number of files in the torrent when it is known.
pub fn resolve(
    select: &[String],
    exclude: &[String],
    file_count: Option<usize>,
) -> Result<Option<Vec<usize>>> {
    if select.is_empty() && exclude.is_empty() {
        return Ok(None);
    }
    let selected = parse(select, "select-file", file_count)?;
    let excluded: HashSet<usize> = parse(exclude, "exclude-file", file_count)?
        .into_iter()
        .collect();

    // With no selection **flag**, the selection is everything the exclusion
    // leaves. That needs the file count. A caller who cannot supply one has
    // asked for something this function cannot answer, and the answer it used
    // to give was `None`, every file, which is the exclusion doing the
    // opposite of what it says. `needs_file_count` is how a caller asks
    // whether it has to go and find the count first. See
    // `TODO/cli-surface.md`, T-185.
    //
    // Keyed on whether the flag was given rather than on what it resolved to.
    // `--select-file 9-` on a five-file torrent resolves to nothing, and that
    // is a caller asking for files that are not there, not a caller asking for
    // all of them.
    let selected = match (select.is_empty(), file_count) {
        (true, Some(count)) => (0..count).collect(),
        // Refused rather than answered with `None`, which is every file and is
        // the opposite of what the caller asked for. A caller that cannot
        // supply the count asks `needs_file_count` first and goes and finds
        // one; this is what it costs to skip that.
        (true, None) => {
            return Err(Error::usage(
                "--exclude-file with no --select-file needs the file count; name the files to keep with --select-file instead",
            ));
        }
        (false, _) => selected,
    };

    let mut out: Vec<usize> = selected
        .into_iter()
        .filter(|index| !excluded.contains(index))
        .collect();
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        return Err(Error::usage(
            "--select-file and --exclude-file together select no files at all",
        ));
    }
    Ok(Some(out))
}

/// Whether these two flags can be resolved at all without the file count.
///
/// Two forms need it and nothing else does: an exclusion with no selection
/// beside it, whose answer is every other file, and an open-ended range, whose
/// answer runs to the last one. Every other spelling resolves to the same
/// indices with the count and without it.
///
/// `download` asks this before it adds a source, because the answer decides
/// whether a magnet's metadata has to be resolved before the add rather than
/// after it. See `TODO/cli-surface.md`, T-185.
pub fn needs_file_count(select: &[String], exclude: &[String]) -> bool {
    if select.is_empty() && !exclude.is_empty() {
        return true;
    }
    terms(select).chain(terms(exclude)).any(is_open_ended)
}

/// One flag's values split the way [`parse`] splits them.
///
/// Shared so that what counts as a term cannot drift between deciding that the
/// count is needed and using it.
fn terms(values: &[String]) -> impl Iterator<Item = &str> {
    values
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|term| !term.is_empty())
}

/// Whether a term is a range with no upper bound, such as `3-`.
fn is_open_ended(term: &str) -> bool {
    matches!(term.split_once('-'), Some((_, "")))
}

/// Parse one flag's worth of indices and ranges.
fn parse(values: &[String], flag: &str, file_count: Option<usize>) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    for term in terms(values) {
        match term.split_once('-') {
            None => out.push(term.parse::<usize>().map_err(|_| index_error(flag, term))?),
            Some((start, "")) => {
                let start: usize = start.trim().parse().map_err(|_| index_error(flag, term))?;
                // An open-ended range needs an upper bound. Refuse rather
                // than guessing at one when the file count is not known.
                let Some(count) = file_count else {
                    return Err(Error::usage(format!(
                        "--{flag} `{term}`: an open-ended range needs the file count; list the indices or use a closed range"
                    )));
                };
                out.extend(start..count);
            }
            Some((start, end)) => {
                let start: usize = start.trim().parse().map_err(|_| index_error(flag, term))?;
                let end: usize = end.trim().parse().map_err(|_| index_error(flag, term))?;
                if start > end {
                    return Err(Error::usage(format!("--{flag} `{term}` runs backwards")));
                }
                out.extend(start..=end);
            }
        }
    }
    Ok(out)
}

/// Parse `-O`/`--index-out` into a file index to the path asked for.
///
/// `INDEX=PATH`, repeatable, zero-based like every other index flag here.
/// `PATH` is relative to the output directory and `/`-separated, and it is a
/// **request**: `paths::plan_with` sanitises, truncates and disambiguates it
/// exactly as it does a torrent's own path, so `-O 0=../../etc/passwd` renames
/// the file to `etc/passwd` inside the output directory rather than escaping
/// it.
///
/// `file_count` is checked when it is known. An index past the end is a usage
/// error rather than a rename that silently does nothing, because a caller who
/// mistyped an index wants to hear about it before the download, not after.
/// A magnet has no count before its metadata resolves, and `None` skips that
/// check rather than guessing. See `TODO/cli-surface.md`, T-116.
pub fn index_out(values: &[String], file_count: Option<usize>) -> Result<BTreeMap<usize, String>> {
    let mut out = BTreeMap::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let Some((index, path)) = value.split_once('=') else {
            return Err(
                Error::usage(format!("--index-out `{value}` is not INDEX=PATH"))
                    .with("value", value.to_string()),
            );
        };
        let index: usize = index.trim().parse().map_err(|_| {
            Error::usage(format!(
                "--index-out `{value}`: `{}` is not a file index",
                index.trim()
            ))
            .with("value", value.to_string())
        })?;
        let path = path.trim();
        if path.is_empty() {
            return Err(Error::usage(format!(
                "--index-out `{value}` names no path for file {index}"
            ))
            .with("value", value.to_string()));
        }
        if let Some(count) = file_count
            && index >= count
        {
            return Err(Error::usage(format!(
                "--index-out `{value}`: the torrent has {count} file(s), so there is no file {index}"
            ))
            .with("value", value.to_string())
            .with("file_count", count));
        }
        // Last wins, and it is not an error. Two `-O` for one index is a
        // caller overriding an earlier argument, which is what every other
        // repeated flag on this surface does.
        out.insert(index, path.replace('\\', "/"));
    }
    Ok(out)
}

fn index_error(flag: &str, term: &str) -> Error {
    Error::usage(format!(
        "--{flag} `{term}` is not a file index or an index range"
    ))
    .with("value", term.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bit_cli_core::ExitCode;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn no_flags_means_every_file() {
        assert_eq!(resolve(&[], &[], None).unwrap(), None);
        assert_eq!(resolve(&[], &[], Some(5)).unwrap(), None);
    }

    #[test]
    fn indices_and_ranges_both_select() {
        assert_eq!(resolve(&args(&["0"]), &[], None).unwrap(), Some(vec![0]));
        assert_eq!(
            resolve(&args(&["1-3"]), &[], None).unwrap(),
            Some(vec![1, 2, 3])
        );
        assert_eq!(
            resolve(&args(&["1-3", "7"]), &[], None).unwrap(),
            Some(vec![1, 2, 3, 7])
        );
    }

    #[test]
    fn an_exclusion_narrows_a_selection() {
        assert_eq!(
            resolve(&args(&["0-4"]), &args(&["2"]), None).unwrap(),
            Some(vec![0, 1, 3, 4])
        );
    }

    /// With the count, an exclusion on its own is the complement.
    #[test]
    fn an_exclusion_alone_is_every_other_file_when_the_count_is_known() {
        assert_eq!(
            resolve(&[], &args(&["1"]), Some(4)).unwrap(),
            Some(vec![0, 2, 3])
        );
        assert_eq!(
            resolve(&[], &args(&["0", "3"]), Some(4)).unwrap(),
            Some(vec![1, 2])
        );
    }

    /// Without it, it is refused. Answering `None` is answering "every file",
    /// which is the exclusion doing the opposite of what it says, and that is
    /// what `TODO/cli-surface.md` T-185 was.
    #[test]
    fn an_exclusion_alone_without_the_count_is_refused_rather_than_ignored() {
        let err = resolve(&[], &args(&["1"]), None).unwrap_err();
        assert_eq!(err.code(), ExitCode::Usage);
        assert!(err.message().contains("file count"), "{}", err.message());
    }

    /// Which spellings need the count, so a caller knows whether it has to go
    /// and find one before it can resolve anything.
    #[test]
    fn only_an_exclusion_alone_and_an_open_ended_range_need_the_count() {
        assert!(!needs_file_count(&[], &[]));
        assert!(!needs_file_count(&args(&["0"]), &[]));
        assert!(!needs_file_count(&args(&["1-3"]), &args(&["2"])));
        assert!(needs_file_count(&[], &args(&["1"])));
        assert!(needs_file_count(&args(&["3-"]), &[]));
        // An open-ended exclusion beside a selection needs it just as much.
        assert!(needs_file_count(&args(&["0-9"]), &args(&["3-"])));
        // Found inside a comma-separated value, not only as a whole one.
        assert!(needs_file_count(&args(&["0,4-"]), &[]));
        // Whitespace and empty terms are not terms.
        assert!(!needs_file_count(&args(&["0, 1 ,"]), &[]));
    }

    /// For selections that are otherwise well formed, `needs_file_count` is
    /// exactly the set `resolve` refuses without one. Pinned as a pair because
    /// a caller that trusts the answer and skips the count hits the refusal.
    #[test]
    fn needs_file_count_agrees_with_what_resolve_refuses() {
        let cases: [(&[&str], &[&str]); 8] = [
            (&[], &[]),
            (&["0"], &[]),
            (&["1-3"], &["2"]),
            (&["0,4"], &["4"]),
            (&[], &["1"]),
            (&["3-"], &[]),
            (&["0-9"], &["3-"]),
            (&["0,4-"], &[]),
        ];
        for (select, exclude) in cases {
            let (select, exclude) = (args(select), args(exclude));
            let resolved = resolve(&select, &exclude, None);
            assert_eq!(
                needs_file_count(&select, &exclude),
                resolved.is_err(),
                "{select:?} {exclude:?} resolved to {:?}",
                resolved.map(|r| r.is_some())
            );
        }
    }

    #[test]
    fn excluding_every_file_is_a_usage_error() {
        let err = resolve(&[], &args(&["0-3"]), Some(4)).unwrap_err();
        assert_eq!(err.code(), ExitCode::Usage);
    }

    #[test]
    fn an_open_ended_range_resolves_against_the_count() {
        assert_eq!(
            resolve(&args(&["2-"]), &[], Some(5)).unwrap(),
            Some(vec![2, 3, 4])
        );
    }

    #[test]
    fn an_open_ended_range_with_no_count_says_why_it_cannot_be_resolved() {
        let err = resolve(&args(&["2-"]), &[], None).unwrap_err();
        assert_eq!(err.code(), ExitCode::Usage);
        assert!(err.message().contains("file count"), "{}", err.message());
    }

    /// An open-ended range starting past the end selects nothing, which is a
    /// usage error rather than a silent empty download.
    /// `-O`/`--index-out`, the happy shapes. T-116.
    #[test]
    fn index_out_parses_index_equals_path() {
        let parsed = index_out(&args(&["0=renamed.bin", "2=sub/other.bin"]), Some(3)).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[&0], "renamed.bin");
        assert_eq!(parsed[&2], "sub/other.bin");

        // A backslash is a separator on the platform this is developed on, and
        // the plan splits on `/`. Normalised so `-O 0=sub\x.bin` names a path
        // rather than one component with a backslash in it.
        let parsed = index_out(&args(&[r"0=sub\x.bin"]), Some(1)).unwrap();
        assert_eq!(parsed[&0], "sub/x.bin");

        // Repeating an index overrides the earlier one, which is what every
        // other repeated flag on this surface does.
        let parsed = index_out(&args(&["0=a.bin", "0=b.bin"]), Some(1)).unwrap();
        assert_eq!(parsed[&0], "b.bin");

        // A `=` in the path is part of the path: only the first splits.
        let parsed = index_out(&args(&["0=a=b.bin"]), Some(1)).unwrap();
        assert_eq!(parsed[&0], "a=b.bin");

        assert!(index_out(&[], None).unwrap().is_empty());
    }

    /// Every way of getting it wrong is a usage error rather than a rename
    /// that silently does nothing. T-116.
    #[test]
    fn index_out_refuses_what_it_cannot_use() {
        for value in ["renamed.bin", "0", "x=renamed.bin", "0="] {
            let err = index_out(&args(&[value]), Some(3)).unwrap_err();
            assert_eq!(err.code(), ExitCode::Usage, "`{value}` should be refused");
        }
        // An index past the end, when the count is known. This is the one that
        // matters: a caller who mistyped an index wants to hear about it
        // before the download rather than after.
        let err = index_out(&args(&["7=x.bin"]), Some(3)).unwrap_err();
        assert_eq!(err.code(), ExitCode::Usage);
        assert!(err.message().contains("3 file(s)"), "{}", err.message());
        // And with no count, which is a magnet before its metadata resolves,
        // it is accepted here and checked again once the count exists.
        assert!(index_out(&args(&["7=x.bin"]), None).is_ok());
    }

    #[test]
    fn an_open_ended_range_past_the_end_selects_nothing() {
        let err = resolve(&args(&["9-"]), &[], Some(5)).unwrap_err();
        assert_eq!(err.code(), ExitCode::Usage);
    }

    #[test]
    fn selecting_nothing_at_all_is_a_usage_error_rather_than_an_empty_selection() {
        let err = resolve(&args(&["1-2"]), &args(&["1-2"]), None).unwrap_err();
        assert_eq!(err.code(), ExitCode::Usage);
    }

    #[test]
    fn a_bad_index_names_the_flag_and_the_value() {
        let err = resolve(&args(&["two"]), &[], None).unwrap_err();
        assert_eq!(err.code(), ExitCode::Usage);
        assert!(err.message().contains("select-file"), "{}", err.message());
        assert_eq!(err.context()["value"], "two");

        let err = resolve(&[], &args(&["two"]), Some(4)).unwrap_err();
        assert!(err.message().contains("exclude-file"), "{}", err.message());
    }

    #[test]
    fn a_backwards_range_is_refused() {
        let err = resolve(&args(&["5-2"]), &[], None).unwrap_err();
        assert!(err.message().contains("backwards"), "{}", err.message());
    }
}
