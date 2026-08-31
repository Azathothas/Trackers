//! Where a payload actually is, given what `--data` pointed at.
//!
//! Shared because two commands need the same answer from the same flag, the
//! way [`crate::selection`] is shared for the same reason. `verify` reads a
//! payload and `seed` serves one, both read the layout `download` wrote, and
//! their `--data` flags carry the same name and the same help text. A caller
//! who verified a payload one way and seeded it the other used to get two
//! answers, one of them a seeder holding nothing. See
//! `TODO/cli-surface.md`, T-186.

use std::path::{Path, PathBuf};

use bit_cli_core::layout::Layout;

/// Where the payload is, and whether it was found there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    /// The directory the torrent's own file paths hang off.
    pub path: PathBuf,
    /// Whether the torrent's first file is there.
    ///
    /// `false` is not an error and not a wrong directory either: a payload
    /// nobody has fetched yet is missing from both candidates. It is what lets
    /// a caller say "I looked here and here" rather than "this is a partial
    /// seed", which is the right observation with the wrong reason.
    pub found: bool,
    /// The candidate that was not chosen, when the layout has two.
    ///
    /// `None` for a single-file torrent, which lays its one file directly
    /// under `--data` and has no directory of its own to be pointed at.
    /// Filled whichever way the choice went, because a caller holding nothing
    /// wants to know where else it could have looked, and by then a seeder has
    /// created the tree it was searching for and the choice looks decided.
    pub other: Option<PathBuf>,
}

/// Resolve `--data` against a torrent's layout.
///
/// A multi-file torrent lays its files under a directory named after itself,
/// so a caller can point at the parent or at the torrent directory and mean
/// the same payload. Whichever holds the first file wins, and the first file
/// is enough: a torrent whose first file is under a directory has the rest
/// there too, and reading more of them would cost a `stat` per file to answer
/// a question one already answers.
///
/// Neither holding it leaves `base` as the answer, because a payload that is
/// not there is no evidence for either spelling.
pub fn resolve(base: &Path, layout: &Layout) -> Root {
    resolve_with(base, layout, &std::collections::BTreeMap::new())
}

/// The same, for a payload some of whose files were renamed on the way in.
///
/// `-O 0=renamed.bin` is the flag that moves the file this looks for, so a
/// resolver that only knows the torrent's own paths reports "not found" for
/// both candidates and falls back to `base`. On a multi-file torrent that is
/// the parent of the directory the payload is actually in, and every file is
/// then looked for one level too high. See `TODO/cli-surface.md`, T-213.
pub fn resolve_with(
    base: &Path,
    layout: &Layout,
    index_out: &std::collections::BTreeMap<usize, String>,
) -> Root {
    let base = base.to_path_buf();
    let Some(first) = layout.files.first() else {
        return Root {
            path: base,
            found: false,
            other: None,
        };
    };
    // Where file 0 is on disk: the caller's path when they gave one, and the
    // torrent's otherwise. A renamed file lands at the root of the output
    // directory, which is what the path plan does with it.
    let holds = |root: &Path| match index_out.get(&0) {
        Some(requested) => requested
            .split('/')
            .fold(root.to_path_buf(), |a, c| a.join(c))
            .exists(),
        None => first
            .path
            .iter()
            .fold(root.to_path_buf(), |a, c| a.join(c))
            .exists(),
    };

    // Only a multi-file torrent has a directory of its own. A single-file
    // torrent's `name` **is** its file name, so a nested candidate would be
    // `<base>/payload.bin/payload.bin`, which is a path nothing writes.
    if !layout.multi_file {
        return Root {
            found: holds(&base),
            path: base,
            other: None,
        };
    }
    let nested = base.join(&layout.name);
    // The parent wins a tie, which cannot happen with a payload one command
    // wrote and is decided rather than left to whichever check ran first.
    let (path, other, found) = match (holds(&base), holds(&nested)) {
        (true, _) => (base, nested, true),
        (false, true) => (nested, base, true),
        (false, false) => (base, nested, false),
    };
    Root {
        path,
        found,
        other: Some(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(multi_file: bool, files: &[(&str, u64)]) -> Layout {
        Layout::from_lengths(
            "album".to_string(),
            multi_file,
            1024,
            files.iter().map(|(p, n)| ((*p).to_string(), *n)),
        )
    }

    fn place(root: &Path, relative: &str) {
        let target = relative
            .split('/')
            .fold(root.to_path_buf(), |acc, part| acc.join(part));
        std::fs::create_dir_all(target.parent().expect("a parent")).expect("mkdir");
        std::fs::write(&target, b"x").expect("write");
    }

    /// The parent and the torrent directory both mean the same payload, which
    /// is the whole of `TODO/cli-surface.md` T-186.
    #[test]
    fn either_spelling_of_data_finds_a_multi_file_payload() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(true, &[("disc 1/a.flac", 1500), ("notes.nfo", 500)]);
        place(dir.path(), "album/disc 1/a.flac");

        let from_parent = resolve(dir.path(), &layout);
        assert_eq!(from_parent.path, dir.path().join("album"));
        assert!(from_parent.found);
        assert_eq!(from_parent.other, Some(dir.path().to_path_buf()));

        let from_torrent_dir = resolve(&dir.path().join("album"), &layout);
        assert_eq!(from_torrent_dir.path, dir.path().join("album"));
        assert!(from_torrent_dir.found);
        // The candidate not chosen is reported whichever way the choice went,
        // because a seeder holding nothing wants both names and by then it has
        // created the tree that decided the choice.
        assert_eq!(
            from_torrent_dir.other,
            Some(dir.path().join("album").join("album"))
        );
    }

    /// Nothing on disk is neither spelling, and the caller is told which two
    /// directories were looked in rather than that this is a partial seed.
    #[test]
    fn a_payload_that_is_not_there_leaves_data_as_given_and_names_the_other() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(true, &[("disc 1/a.flac", 1500), ("notes.nfo", 500)]);

        let root = resolve(dir.path(), &layout);
        assert_eq!(root.path, dir.path());
        assert!(!root.found);
        assert_eq!(root.other, Some(dir.path().join("album")));
    }

    /// A single-file torrent has no directory of its own, so there is no
    /// second candidate to offer and none is invented.
    #[test]
    fn a_single_file_torrent_has_one_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(false, &[("album", 3000)]);

        let missing = resolve(dir.path(), &layout);
        assert_eq!(missing.path, dir.path());
        assert!(!missing.found);
        assert_eq!(missing.other, None);

        place(dir.path(), "album");
        let found = resolve(dir.path(), &layout);
        assert_eq!(found.path, dir.path());
        assert!(found.found);
        assert_eq!(found.other, None);
    }

    /// A directory the caller renamed still resolves, because the parent is
    /// only tried when the files are not directly under what was given.
    #[test]
    fn a_renamed_payload_directory_still_resolves_when_it_is_named_directly() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(true, &[("disc 1/a.flac", 1500)]);
        place(dir.path(), "renamed/disc 1/a.flac");

        let root = resolve(&dir.path().join("renamed"), &layout);
        assert_eq!(root.path, dir.path().join("renamed"));
        assert!(root.found);
    }
}
