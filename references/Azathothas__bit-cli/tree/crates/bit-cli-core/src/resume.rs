//! A resume cache: the verified bitfield of a torrent, kept between runs.
//!
//! Seeding a payload re-hashes all of it, on every invocation, and the session
//! offers no way to skip the check. Measured: **0.32 s for 512 MiB**, about
//! 1.6 GiB/s, so a 40 GiB seed spends about **25 seconds** of disk read before
//! it announces anything. `TODO/disk-io.md` T-016 has the measurement and the
//! correction to the eight minute figure that entry used to carry.
//!
//! `librqbit` has the machinery for this already, in `fastresume`: it loads a
//! bitfield, checks its length against the torrent, re-verifies a sample of
//! the pieces it claims, and throws the whole thing away if any of them fails.
//! What it did not have was a way to store one **without** turning on session
//! persistence, which writes a record of every torrent in the session and is
//! stored state that decision 7.4 puts in Phase C. The vendored tree now takes
//! a `BitVFactory` on `SessionOptions`, and this is that factory.
//!
//! **The distinction that makes this Phase B work.** A resume cache is derived
//! data: delete it and the next run recomputes it, slowly, and is otherwise
//! identical. A session store is state: delete it and the session forgets what
//! it was doing. 7.4 is about the second.
//!
//! ## Where it lives
//!
//! `<cache root>/<info hash>.bitv`, beside a `<info hash>.meta` describing the
//! payload the bitfield was taken from. The root is `--fastresume-dir` when
//! given and `<download directory>/.bit-cli-resume` otherwise: beside the data
//! it describes, so moving or deleting the payload takes the cache with it.
//!
//! ## When it is thrown away
//!
//! Three layers, cheapest first.
//!
//! 1. **The sidecar.** Every file's length and modification time, and the
//!    torrent's total length and piece count. Any disagreement and the cache
//!    is not offered. This is the layer that catches a payload edited between
//!    runs, and it costs one `stat` per file.
//! 2. **The length check**, `librqbit`'s: a bitfield of the wrong size for
//!    this torrent is refused.
//! 3. **The sample**, `librqbit`'s: at least one claimed piece per file plus a
//!    random sample of the rest are re-hashed, and one failure discards the
//!    lot and clears the cache.
//!
//! Layer 1 is ours because layers 2 and 3 are probabilistic about the middle
//! of a large file: a payload whose bytes changed without its length or
//! timestamp changing can still pass a sample. Nothing here is a substitute
//! for `--verify full` on data that matters.

use std::path::{Path, PathBuf};

use librqbit::BF;
use librqbit::api::TorrentIdOrHash;
use librqbit::bitv::{BitV, DiskBackedBitV};
use librqbit::bitv_factory::BitVFactory;
use librqbit::spawn_utils::BlockingSpawner;

/// The directory name used under the download directory when no other root is
/// given. Dotted, so it sorts out of the way and reads as machine-owned.
pub const DEFAULT_DIR_NAME: &str = ".bit-cli-resume";

/// What the payload looked like when the bitfield was taken.
///
/// Deliberately a flat text file rather than a serialized struct: it is read
/// by this program and by a person deciding whether to delete it, and one line
/// per file is both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    pub total_length: u64,
    pub pieces: u32,
    /// One `(relative path, length, modified unix millis)` per file, in the
    /// torrent's own order.
    pub files: Vec<(String, u64, i64)>,
}

impl Fingerprint {
    /// The on-disk form. One `key value` header line, then one line per file.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("bit-cli-resume 1\n");
        out.push_str(&format!("total_length {}\n", self.total_length));
        out.push_str(&format!("pieces {}\n", self.pieces));
        for (path, len, modified) in &self.files {
            // The path is last so a name containing a space cannot be
            // mistaken for another field.
            out.push_str(&format!("file {len} {modified} {path}\n"));
        }
        out
    }

    /// Parse what [`Self::render`] wrote. `None` for anything unexpected,
    /// because an unreadable fingerprint and a mismatched one mean the same
    /// thing here: do not trust the bitfield beside it.
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        if lines.next()? != "bit-cli-resume 1" {
            return None;
        }
        let total_length = lines.next()?.strip_prefix("total_length ")?.parse().ok()?;
        let pieces = lines.next()?.strip_prefix("pieces ")?.parse().ok()?;
        let mut files = Vec::new();
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let rest = line.strip_prefix("file ")?;
            let (len, rest) = rest.split_once(' ')?;
            let (modified, path) = rest.split_once(' ')?;
            files.push((path.to_owned(), len.parse().ok()?, modified.parse().ok()?));
        }
        Some(Fingerprint {
            total_length,
            pieces,
            files,
        })
    }

    /// Take a fingerprint of what is on disk now.
    ///
    /// A file that is missing or cannot be stated is recorded as length zero
    /// at time zero rather than skipped, so a payload that lost a file does
    /// not fingerprint the same as one that never had it.
    pub fn of(
        directory: &Path,
        files: &[(String, u64)],
        total_length: u64,
        pieces: u32,
    ) -> Fingerprint {
        let entries = files
            .iter()
            .map(|(relative, _)| {
                let path = directory.join(relative);
                let (len, modified) = match std::fs::metadata(&path) {
                    Ok(meta) => (meta.len(), modified_millis(&meta)),
                    Err(_) => (0, 0),
                };
                (relative.clone(), len, modified)
            })
            .collect();
        Fingerprint {
            total_length,
            pieces,
            files: entries,
        }
    }
}

fn modified_millis(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// A `BitVFactory` backed by one file per info hash in one directory.
pub struct FileResumeCache {
    root: PathBuf,
    spawner: BlockingSpawner,
    /// The fingerprint the caller expects, per info hash, set before the
    /// torrent is added. A hash with no entry is not served from the cache:
    /// this refuses to answer about a torrent nobody described.
    expected: std::sync::Mutex<std::collections::HashMap<String, Fingerprint>>,
}

impl FileResumeCache {
    pub fn new(root: PathBuf, spawner: BlockingSpawner) -> Self {
        Self {
            root,
            spawner,
            expected: Default::default(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Record what the payload for `info_hash` should look like.
    ///
    /// Called before the torrent is added, because `load` happens inside the
    /// session and has nothing else to check against.
    pub fn expect(&self, info_hash: &str, fingerprint: Fingerprint) {
        if let Ok(mut guard) = self.expected.lock() {
            guard.insert(info_hash.to_ascii_lowercase(), fingerprint);
        }
    }

    fn hash_of(id: TorrentIdOrHash) -> Option<String> {
        match id {
            TorrentIdOrHash::Hash(h) => Some(format!("{h:?}").to_ascii_lowercase()),
            // The session keys every call this cache sees by info hash. An id
            // cannot be resolved to one here, and guessing would serve the
            // wrong torrent's bitfield.
            TorrentIdOrHash::Id(_) => None,
        }
    }

    fn bitv_path(&self, hash: &str) -> PathBuf {
        self.root.join(format!("{hash}.bitv"))
    }

    fn meta_path(&self, hash: &str) -> PathBuf {
        self.root.join(format!("{hash}.meta"))
    }

    /// Whether the sidecar beside this bitfield still describes the payload.
    fn fingerprint_matches(&self, hash: &str) -> bool {
        let Ok(guard) = self.expected.lock() else {
            return false;
        };
        let Some(want) = guard.get(hash) else {
            return false;
        };
        let Ok(text) = std::fs::read_to_string(self.meta_path(hash)) else {
            return false;
        };
        Fingerprint::parse(&text).as_ref() == Some(want)
    }

    fn remove(&self, hash: &str) {
        let _ = std::fs::remove_file(self.bitv_path(hash));
        let _ = std::fs::remove_file(self.meta_path(hash));
    }
}

#[async_trait::async_trait]
impl BitVFactory for FileResumeCache {
    async fn load(&self, id: TorrentIdOrHash) -> anyhow::Result<Option<Box<dyn BitV>>> {
        let Some(hash) = Self::hash_of(id) else {
            return Ok(None);
        };
        if !self.fingerprint_matches(&hash) {
            // Stale, or about a torrent nothing described. Removed rather than
            // left: a bitfield that will never be trusted again is a file
            // nobody will ever delete.
            self.remove(&hash);
            return Ok(None);
        }
        match DiskBackedBitV::new(self.bitv_path(&hash), self.spawner.clone()).await {
            Ok(bitv) => Ok(Some(bitv.into_dyn())),
            Err(e) => {
                if let Some(io) = e.downcast_ref::<std::io::Error>()
                    && io.kind() == std::io::ErrorKind::NotFound
                {
                    return Ok(None);
                }
                Err(e)
            }
        }
    }

    async fn clear(&self, id: TorrentIdOrHash) -> anyhow::Result<()> {
        if let Some(hash) = Self::hash_of(id) {
            self.remove(&hash);
        }
        Ok(())
    }

    async fn store_initial_check(
        &self,
        id: TorrentIdOrHash,
        b: BF,
    ) -> anyhow::Result<Box<dyn BitV>> {
        let hash = Self::hash_of(id)
            .ok_or_else(|| anyhow::anyhow!("a resume cache is keyed by info hash"))?;
        std::fs::create_dir_all(&self.root)?;

        let path = self.bitv_path(&hash);
        let tmp = path.with_extension("bitv.tmp");
        std::fs::write(&tmp, b.as_raw_slice())?;
        std::fs::rename(&tmp, &path)?;

        // The sidecar is written after the bitfield and describes it. A crash
        // between the two leaves a bitfield with no sidecar, which `load`
        // refuses, which is the safe way round.
        if let Ok(guard) = self.expected.lock()
            && let Some(fingerprint) = guard.get(&hash)
        {
            let _ = std::fs::write(self.meta_path(&hash), fingerprint.render());
        }

        Ok(DiskBackedBitV::new(path, self.spawner.clone())
            .await?
            .into_dyn())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Fingerprint {
        Fingerprint {
            total_length: 1024,
            pieces: 4,
            files: vec![
                ("a.bin".into(), 512, 1_700_000_000_000),
                ("sub/b b.bin".into(), 512, 1_700_000_000_001),
            ],
        }
    }

    #[test]
    fn a_fingerprint_round_trips() {
        let f = sample();
        assert_eq!(Fingerprint::parse(&f.render()), Some(f));
    }

    /// A path with a space in it is the reason the path is the last field.
    #[test]
    fn a_path_with_a_space_survives() {
        let parsed = Fingerprint::parse(&sample().render()).unwrap();
        assert_eq!(parsed.files[1].0, "sub/b b.bin");
    }

    #[test]
    fn anything_unexpected_parses_as_nothing() {
        assert_eq!(Fingerprint::parse(""), None);
        assert_eq!(Fingerprint::parse("bit-cli-resume 2\n"), None);
        assert_eq!(
            Fingerprint::parse("bit-cli-resume 1\ntotal_length x\npieces 4\n"),
            None
        );
    }

    /// One byte more on disk is a different fingerprint, which is the whole
    /// point of the sidecar.
    #[test]
    fn a_changed_length_does_not_match() {
        let mut other = sample();
        other.files[0].1 += 1;
        assert_ne!(Fingerprint::parse(&sample().render()), Some(other));
    }

    #[test]
    fn a_changed_modification_time_does_not_match() {
        let mut other = sample();
        other.files[0].2 += 1;
        assert_ne!(Fingerprint::parse(&sample().render()), Some(other));
    }
}
