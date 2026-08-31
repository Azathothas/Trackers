//! Asking the session for pieces front to back.
//!
//! `librqbit` 9.0.0's picker is not configurable and is not rarest-first
//! either, which is worth stating because the flag that selects it said
//! otherwise. `ChunkTracker::iter_queued_pieces` walks the files in priority
//! order and, within each file, yields **the first piece, then the last, then
//! the middle in ascending order**. Nothing anywhere in it counts how many
//! peers hold a piece. Measured on a 48 piece torrent over four connections,
//! the order a download verifies pieces in is:
//!
//! ```text
//! 47, 0, 1, 3, 4, 6, 2, 5, 7, 8, 9, 10, 12, 14, 11, 13, 15, 16, ... 46
//! ```
//!
//! Last piece second, then ascending with local reordering from concurrent
//! connections finishing out of order. Near-sequential, and not sequential:
//! that `47` is a descent, and it is the first thing a consumer reading the
//! stream front to back would trip over.
//!
//! # The one lever there is
//!
//! `PieceTracker::acquire_piece` checks a `priority_pieces` iterator **before**
//! the natural order, and that iterator comes from the streaming subsystem:
//! every registered [`librqbit::FileStream`] contributes the pieces covering
//! the 32 MiB after its own read position. So a stream held at the earliest
//! piece the torrent still needs makes that piece, and the ones after it, what
//! every peer is handed first.
//!
//! [`InOrder`] is that stream and nothing else. It never reads a byte: it seeks,
//! which is what moves the window, and lets the session do the work. One stream
//! is open at a time, on whichever file holds the earliest missing piece.
//!
//! # What it costs
//!
//! One permit from `librqbit`'s blocking spawner semaphore, held for as long as
//! the stream is open. That semaphore is sized at the session's worker thread
//! count, eight by default, so this is one eighth of the concurrency available
//! to blocking storage work. `TODO/performance.md` T-032 records what that is
//! worth in throughput, measured rather than assumed.

use std::sync::Arc;

use librqbit::ManagedTorrent;
use tokio::io::AsyncSeekExt;

use crate::layout::Layout;

/// The open stream, as a trait object.
///
/// `ManagedTorrent::stream` is public and returns `librqbit::FileStream`, whose
/// module is not, so the type is reachable and unnameable at the same time and
/// cannot be a field. Only `AsyncSeek` is wanted from it anyway: this never
/// reads a byte. Boxing keeps the `Drop` that deregisters the stream, which is
/// what releases the priority and the semaphore permit.
type Priority = std::pin::Pin<Box<dyn tokio::io::AsyncSeek + Send>>;

/// Holds the session's piece priority at the front of what is missing.
pub struct InOrder {
    handle: Arc<ManagedTorrent>,
    layout: Arc<Layout>,
    /// The open stream and which file it is on. `None` before the first
    /// advance and after the torrent is complete.
    current: Option<(usize, Priority)>,
    /// The piece the window was last pointed at, so an advance that changes
    /// nothing does no work.
    at: Option<u32>,
}

impl InOrder {
    pub fn new(handle: Arc<ManagedTorrent>, layout: Arc<Layout>) -> Self {
        Self {
            handle,
            layout,
            current: None,
            at: None,
        }
    }

    /// Which piece the priority window currently starts at, for a caller that
    /// wants to report it. `None` before the first advance and once the
    /// torrent is complete.
    pub fn pointing_at(&self) -> Option<u32> {
        self.at
    }

    /// Point the window at the earliest piece `have` says is still missing.
    ///
    /// Returns that piece, or `None` when nothing is missing, in which case the
    /// stream is dropped and the priority released.
    ///
    /// Cheap to call often, and meant to be: it does nothing at all when the
    /// earliest missing piece has not moved.
    pub async fn advance(&mut self, have: &[bool]) -> anyhow::Result<Option<u32>> {
        let Some(piece) = have.iter().position(|present| !*present) else {
            self.current = None;
            self.at = None;
            return Ok(None);
        };
        let piece = piece as u32;
        if self.at == Some(piece) {
            return Ok(Some(piece));
        }

        let offset = u64::from(piece) * u64::from(self.layout.piece_length);
        let Some((index, within)) = file_at(&self.layout, offset) else {
            // Past the last byte of the last file, which means every piece
            // that holds payload is present and the rest is padding.
            self.current = None;
            self.at = None;
            return Ok(None);
        };

        // A stream is per file, so crossing a file boundary means a new one.
        // The old one is dropped first: holding two would hold two permits and
        // give the session two priority windows, and the second one would be
        // behind the first.
        if self.current.as_ref().map(|(file, _)| *file) != Some(index) {
            self.current = None;
            let stream = self.handle.clone().stream(index).await?;
            self.current = Some((index, Box::pin(stream)));
        }
        if let Some((_, stream)) = self.current.as_mut() {
            stream.seek(std::io::SeekFrom::Start(within)).await?;
        }
        // What `--trace picker` promises, for the half this repository decides.
        // The one lever there is over `librqbit`'s order is where this window
        // sits, so a record that says which piece it moved to and which it
        // came from is the whole of "why was this piece asked for next". The
        // rest of the decision is the session's, and `librqbit::picker` is the
        // other target the subsystem raises. See `TODO/cli-surface.md`, T-219.
        tracing::trace!(
            target: "bit_cli::picker",
            piece,
            was = ?self.at,
            file = index,
            within,
            "moved the priority window"
        );
        self.at = Some(piece);
        Ok(Some(piece))
    }
}

/// The file holding a torrent byte offset, and the offset within it.
///
/// Zero length files are skipped: no offset is ever inside one, and `librqbit`
/// will not stream one either. A free function rather than a method so it can
/// be tested without a session, which is the part of this module that has
/// arithmetic in it.
fn file_at(layout: &Layout, offset: u64) -> Option<(usize, u64)> {
    layout
        .files
        .iter()
        .enumerate()
        .find(|(_, file)| {
            file.length > 0 && offset >= file.offset && offset < file.offset + file.length
        })
        .map(|(index, file)| (index, offset - file.offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LayoutFile;

    fn layout(files: &[(&str, u64)], piece_length: u32) -> Layout {
        let mut offset = 0;
        let files: Vec<LayoutFile> = files
            .iter()
            .map(|(path, length)| {
                let file = LayoutFile::new(path, offset, *length);
                offset += *length;
                file
            })
            .collect();
        Layout {
            name: "t".into(),
            multi_file: files.len() > 1,
            total_length: offset,
            files,
            piece_length,
        }
    }

    /// Which file an offset lands in is what decides which stream to open, and
    /// it is the only arithmetic here that a session is not needed to check.
    #[test]
    fn an_offset_maps_to_the_file_that_holds_it() {
        let layout = layout(&[("a.bin", 100), ("b.bin", 50), ("c.bin", 1)], 16);
        assert_eq!(file_at(&layout, 0), Some((0, 0)));
        assert_eq!(file_at(&layout, 99), Some((0, 99)));
        // The first byte of the next file, not the last of this one.
        assert_eq!(file_at(&layout, 100), Some((1, 0)));
        assert_eq!(file_at(&layout, 149), Some((1, 49)));
        assert_eq!(file_at(&layout, 150), Some((2, 0)));
        // Past the end is not an error, it is nothing to point at.
        assert_eq!(file_at(&layout, 151), None);
    }

    /// A zero length file holds no byte, so nothing maps into it.
    ///
    /// It matters because a torrent may carry one and `librqbit` refuses to
    /// stream it, so pointing at one would fail the advance rather than
    /// skipping it.
    #[test]
    fn a_zero_length_file_is_never_the_answer() {
        let layout = layout(&[("empty", 0), ("a.bin", 32), ("also-empty", 0)], 16);
        assert_eq!(file_at(&layout, 0), Some((1, 0)));
        assert_eq!(file_at(&layout, 31), Some((1, 31)));
        assert_eq!(file_at(&layout, 32), None);
    }

    /// A single file torrent is the common case and has no boundaries at all.
    #[test]
    fn a_single_file_torrent_maps_every_offset_to_file_zero() {
        let layout = layout(&[("only.bin", 1 << 20)], 1 << 16);
        for offset in [0u64, 1, 65535, 65536, (1 << 20) - 1] {
            assert_eq!(file_at(&layout, offset), Some((0, offset)));
        }
        assert_eq!(file_at(&layout, 1 << 20), None);
    }
}
