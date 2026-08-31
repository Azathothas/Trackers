//! The shape of a torrent, independent of any torrent library.
//!
//! Scope resolution, URL composition, piece mapping, and coverage checking all
//! need the same facts: the name, whether it is multi-file, where each file
//! sits in the linear byte stream, and how the stream is cut into pieces.
//! [`Layout`] is that, and nothing else. Keeping it free of `librqbit` types
//! is what lets the addressing model be tested without a session, a network,
//! or a real `.torrent`.

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// One file within a torrent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutFile {
    /// Path components relative to the torrent root, without the torrent name.
    /// Always `/`-separated when rendered, on every platform.
    pub path: Vec<String>,
    /// Byte offset of this file within the torrent's linear byte stream.
    pub offset: u64,
    /// Length of the file in bytes.
    pub length: u64,
}

impl LayoutFile {
    /// A file from a `/`-separated path.
    pub fn new(path: &str, offset: u64, length: u64) -> Self {
        Self {
            path: path
                .split('/')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
            offset,
            length,
        }
    }

    /// The path as a single `/`-separated string.
    pub fn display_path(&self) -> String {
        self.path.join("/")
    }

    /// The final path component.
    pub fn file_name(&self) -> &str {
        self.path.last().map(String::as_str).unwrap_or_default()
    }

    /// The byte range this file occupies in the torrent's linear stream.
    pub fn range(&self) -> Range<u64> {
        self.offset..self.offset + self.length
    }
}

/// Everything about a torrent's shape that the addressing model needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    /// The torrent `name`, which is the directory name for a multi-file
    /// torrent and the file name for a single-file one.
    pub name: String,
    /// Whether the metainfo carries a `files` list.
    pub multi_file: bool,
    /// Files in torrent order.
    pub files: Vec<LayoutFile>,
    /// Length of a non-final piece.
    pub piece_length: u32,
    /// Total payload length in bytes.
    pub total_length: u64,
}

impl Layout {
    /// Build a layout, computing offsets from the file lengths in order.
    ///
    /// This is the constructor to use when you have lengths but not offsets,
    /// which is every case where the layout is being built from scratch rather
    /// than read out of a live torrent.
    pub fn from_lengths(
        name: impl Into<String>,
        multi_file: bool,
        piece_length: u32,
        files: impl IntoIterator<Item = (String, u64)>,
    ) -> Self {
        let mut offset = 0;
        let files: Vec<LayoutFile> = files
            .into_iter()
            .map(|(path, length)| {
                let file = LayoutFile::new(&path, offset, length);
                offset += length;
                file
            })
            .collect();
        Self {
            name: name.into(),
            multi_file,
            files,
            piece_length,
            total_length: offset,
        }
    }

    /// Number of pieces, including a short final one.
    pub fn piece_count(&self) -> u32 {
        if self.piece_length == 0 {
            return 0;
        }
        self.total_length.div_ceil(u64::from(self.piece_length)) as u32
    }

    /// Length of `piece`, which is shorter than `piece_length` for the last
    /// piece. `None` when the index is past the end.
    pub fn piece_size(&self, piece: u32) -> Option<u64> {
        let range = self.piece_range(piece)?;
        Some(range.end - range.start)
    }

    /// The byte range `piece` occupies, or `None` when the index is past the
    /// end.
    pub fn piece_range(&self, piece: u32) -> Option<Range<u64>> {
        if piece >= self.piece_count() {
            return None;
        }
        let start = u64::from(piece) * u64::from(self.piece_length);
        Some(start..(start + u64::from(self.piece_length)).min(self.total_length))
    }

    /// The byte range covering pieces `first..=last`, clamped to the payload.
    pub fn pieces_range(&self, first: u32, last: u32) -> Range<u64> {
        let start = u64::from(first) * u64::from(self.piece_length);
        let end = u64::from(last)
            .saturating_add(1)
            .saturating_mul(u64::from(self.piece_length))
            .min(self.total_length);
        start.min(self.total_length)..end
    }

    /// Index of the piece holding `offset`.
    pub fn piece_at(&self, offset: u64) -> Option<u32> {
        if self.piece_length == 0 || offset >= self.total_length {
            return None;
        }
        Some((offset / u64::from(self.piece_length)) as u32)
    }

    /// The whole payload as one range.
    pub fn payload(&self) -> Range<u64> {
        0..self.total_length
    }

    /// The file at `index`.
    pub fn file(&self, index: usize) -> Option<&LayoutFile> {
        self.files.get(index)
    }

    /// Index of the file holding `offset`.
    ///
    /// Zero-length files never hold a byte, so they are never returned.
    pub fn file_at(&self, offset: u64) -> Option<usize> {
        let index = self
            .files
            .partition_point(|f| f.offset + f.length <= offset);
        let file = self.files.get(index)?;
        (file.offset <= offset && file.length > 0).then_some(index)
    }

    /// Split the byte range `range` into per-file ranges, in torrent order.
    ///
    /// A range extending past the end of the payload is truncated, so the
    /// returned lengths may sum to less than the range asked for.
    pub fn split_by_file(&self, range: Range<u64>) -> Vec<FileSlice> {
        let end = range.end.min(self.total_length);
        let mut pos = range.start;
        let mut index = self.files.partition_point(|f| f.offset + f.length <= pos);
        let mut out = Vec::new();
        while pos < end {
            let Some(file) = self.files.get(index) else {
                break;
            };
            if file.length > 0 {
                let offset_in_file = pos - file.offset;
                let take = (file.length - offset_in_file).min(end - pos);
                out.push(FileSlice {
                    file: index,
                    offset: offset_in_file,
                    length: take,
                });
                pos += take;
            }
            index += 1;
        }
        out
    }

    /// Pieces a selection of files needs, as one sorted list of indices.
    ///
    /// A piece is needed when any selected file holds even one byte of it,
    /// because a piece is verified against its whole hash and a file that
    /// starts in the middle of one cannot be had without it. Indices outside
    /// the file list are ignored rather than refused: what to do about a bad
    /// index is the caller's decision and the flags that carry one already
    /// make it.
    pub fn pieces_for_selection(&self, selected: &[usize]) -> Vec<u32> {
        let mut out: Vec<u32> = selected
            .iter()
            .filter_map(|index| self.files.get(*index))
            .filter(|file| file.length > 0)
            .flat_map(|file| self.pieces_overlapping(&file.range()))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// What a selection writes into the files it did not select.
    ///
    /// A piece that straddles the boundary between a selected file and an
    /// unselected one carries bytes of both, and the piece cannot be verified
    /// without them, so those bytes are fetched and written whatever the
    /// selection said. The unselected file lands on disk holding them and
    /// nothing else, which is the surprise this reports: its length can equal
    /// its full length while almost all of it is zeroes.
    ///
    /// Only the **first and last** piece of an unselected file can be shared
    /// with another file, because every piece between them lies entirely
    /// inside it. That is `FluxDown/native/engine/src/bt_partfile.rs`'s
    /// observation in `boundary_segments`, and it is what makes this a walk of
    /// the file list rather than of the piece list. That tree keeps the bytes
    /// in a sidecar; this one reports them where they landed. See
    /// `TODO/disk-io.md`, T-184.
    ///
    /// Empty when the selection is every file, when no boundary straddles, or
    /// when the torrent has one file.
    pub fn selection_spill(&self, selected: &[usize]) -> Vec<Spill> {
        let chosen: std::collections::HashSet<usize> = selected.iter().copied().collect();
        let needed: std::collections::HashSet<u32> =
            self.pieces_for_selection(selected).into_iter().collect();

        let mut out = Vec::new();
        for (index, file) in self.files.iter().enumerate() {
            if chosen.contains(&index) || file.length == 0 {
                continue;
            }
            let pieces = self.pieces_overlapping(&file.range());
            let (first, last) = (pieces.start, pieces.end.saturating_sub(1));
            let mut candidates = vec![first];
            if last != first {
                candidates.push(last);
            }
            let mut bytes = 0u64;
            let mut written_to = 0u64;
            for piece in candidates {
                if !needed.contains(&piece) {
                    continue;
                }
                let Some(range) = self.piece_range(piece) else {
                    continue;
                };
                let start = range.start.max(file.offset);
                let end = range.end.min(file.offset + file.length);
                if start >= end {
                    continue;
                }
                bytes += end - start;
                written_to = written_to.max(end - file.offset);
            }
            if bytes > 0 {
                out.push(Spill {
                    file: index,
                    bytes,
                    written_to,
                    length: file.length,
                });
            }
        }
        out
    }

    /// Every piece index that overlaps `range`.
    pub fn pieces_overlapping(&self, range: &Range<u64>) -> Range<u32> {
        if self.piece_length == 0 || range.start >= range.end {
            return 0..0;
        }
        let first = (range.start / u64::from(self.piece_length)) as u32;
        let last = ((range.end - 1) / u64::from(self.piece_length)) as u32;
        first..last.saturating_add(1).min(self.piece_count())
    }
}

/// An unselected file a boundary piece writes into.
///
/// The three lengths are separate on purpose. `bytes` is how much of the file
/// is real payload, `written_to` is how long the file ends up on disk, and
/// `length` is how long the torrent says it is. A file where `written_to`
/// equals `length` looks complete in a directory listing and is not, which is
/// the whole reason this is reported. See `TODO/disk-io.md`, T-184.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spill {
    /// Index of the file within the torrent.
    pub file: usize,
    /// Bytes of it a boundary piece actually writes.
    pub bytes: u64,
    /// Length the file ends up with on disk: the end of the last written
    /// range. Everything before it that was not written is a hole.
    pub written_to: u64,
    /// Length the torrent says the file is.
    pub length: u64,
}

/// A contiguous byte range inside one file of a torrent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSlice {
    /// Index of the file within the torrent.
    pub file: usize,
    /// Offset of the slice within that file.
    pub offset: u64,
    /// Length of the slice in bytes.
    pub length: u64,
}

impl FileSlice {
    /// The range within the file.
    pub fn range(&self) -> Range<u64> {
        self.offset..self.offset + self.length
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture `TODO/disk-io.md` T-184 measures against: three files at
    /// the odd piece length T-177 uses, with the middle one selected.
    ///
    /// Piece 0 is inside `a.bin`, piece 1 straddles a/b, piece 2 straddles
    /// b/c, and piece 3 is inside `c.bin`. So selecting `b.bin` needs pieces 1
    /// and 2 and nothing else, and both of them reach into a file nobody asked
    /// for.
    fn straddling() -> Layout {
        Layout::from_lengths(
            "album",
            true,
            121 * 16384 + 4096,
            [
                ("a.bin".to_string(), 3_000_000),
                ("b.bin".to_string(), 1_000_000),
                ("c.bin".to_string(), 3_000_000),
            ],
        )
    }

    #[test]
    fn a_selection_needs_every_piece_its_files_touch() {
        let layout = straddling();
        assert_eq!(layout.piece_count(), 4);
        assert_eq!(layout.pieces_for_selection(&[1]), vec![1, 2]);
        assert_eq!(layout.pieces_for_selection(&[0]), vec![0, 1]);
        assert_eq!(layout.pieces_for_selection(&[2]), vec![2, 3]);
        assert_eq!(layout.pieces_for_selection(&[0, 1, 2]), vec![0, 1, 2, 3]);
    }

    #[test]
    fn an_index_past_the_file_list_selects_no_piece() {
        let layout = straddling();
        assert!(layout.pieces_for_selection(&[9]).is_empty());
        assert_eq!(layout.pieces_for_selection(&[1, 9]), vec![1, 2]);
    }

    /// The measured numbers, so the report and the disk agree.
    ///
    /// `a.bin` ends up 3,000,000 bytes on disk, its full length, holding
    /// 1,013,440 real bytes and the rest zeroes. `c.bin` ends up 1,959,680
    /// bytes, short of its 3,000,000. One looks complete and one looks
    /// truncated, and neither is what the caller asked for.
    #[test]
    fn a_selection_spills_into_the_files_around_it() {
        let layout = straddling();
        let spill = layout.selection_spill(&[1]);
        assert_eq!(
            spill,
            vec![
                Spill {
                    file: 0,
                    bytes: 1_013_440,
                    written_to: 3_000_000,
                    length: 3_000_000
                },
                Spill {
                    file: 2,
                    bytes: 1_959_680,
                    written_to: 1_959_680,
                    length: 3_000_000
                },
            ]
        );
    }

    #[test]
    fn selecting_everything_spills_nowhere() {
        let layout = straddling();
        assert!(layout.selection_spill(&[0, 1, 2]).is_empty());
    }

    /// A torrent whose boundaries land on piece edges has no spill at all,
    /// which is why this was never noticed: the fixtures that exercise
    /// `--select-file` are all aligned.
    #[test]
    fn an_aligned_torrent_spills_nowhere() {
        let layout = Layout::from_lengths(
            "aligned",
            true,
            1024,
            [
                ("a.bin".to_string(), 4096),
                ("b.bin".to_string(), 1024),
                ("c.bin".to_string(), 2048),
            ],
        );
        for selection in [vec![0], vec![1], vec![2], vec![0, 2]] {
            assert!(
                layout.selection_spill(&selection).is_empty(),
                "{selection:?} spilled on an aligned torrent"
            );
        }
    }

    /// A file wholly inside one piece that a neighbour also needs is spilled
    /// into end to end, which is the case where `first` and `last` are the
    /// same piece.
    #[test]
    fn a_file_smaller_than_a_piece_is_spilled_into_whole() {
        let layout = Layout::from_lengths(
            "tiny",
            true,
            4096,
            [
                ("a.bin".to_string(), 1000),
                ("b.bin".to_string(), 500),
                ("c.bin".to_string(), 1000),
            ],
        );
        assert_eq!(layout.piece_count(), 1);
        assert_eq!(
            layout.selection_spill(&[0]),
            vec![
                Spill {
                    file: 1,
                    bytes: 500,
                    written_to: 500,
                    length: 500
                },
                Spill {
                    file: 2,
                    bytes: 1000,
                    written_to: 1000,
                    length: 1000
                },
            ],
            "one piece covers the whole torrent, so selecting any file fetches all of it"
        );
    }

    /// A zero-length file occupies no bytes, so no piece touches it and
    /// nothing is ever written into it.
    #[test]
    fn a_zero_length_file_is_never_spilled_into() {
        let layout = Layout::from_lengths(
            "empty",
            true,
            1024,
            [
                ("a.bin".to_string(), 1500),
                ("empty.bin".to_string(), 0),
                ("c.bin".to_string(), 1500),
            ],
        );
        let spill = layout.selection_spill(&[0]);
        assert!(
            !spill.iter().any(|s| s.file == 1),
            "a zero-length file cannot hold spill: {spill:?}"
        );
    }

    fn multi() -> Layout {
        Layout::from_lengths(
            "album",
            true,
            1024,
            [
                ("disc 1/a.flac".to_string(), 1500u64),
                ("disc 1/b.flac".to_string(), 500),
                ("notes.txt".to_string(), 100),
            ],
        )
    }

    #[test]
    fn offsets_follow_from_lengths() {
        let layout = multi();
        assert_eq!(layout.file(0).unwrap().offset, 0);
        assert_eq!(layout.file(1).unwrap().offset, 1500);
        assert_eq!(layout.file(2).unwrap().offset, 2000);
        assert_eq!(layout.total_length, 2100);
    }

    #[test]
    fn paths_split_on_forward_slashes() {
        let layout = multi();
        assert_eq!(layout.file(0).unwrap().path, ["disc 1", "a.flac"]);
        assert_eq!(layout.file(0).unwrap().display_path(), "disc 1/a.flac");
        assert_eq!(layout.file(0).unwrap().file_name(), "a.flac");
    }

    #[test]
    fn the_last_piece_is_short() {
        let layout = multi();
        assert_eq!(layout.piece_count(), 3);
        assert_eq!(layout.piece_size(0), Some(1024));
        assert_eq!(layout.piece_size(1), Some(1024));
        assert_eq!(layout.piece_size(2), Some(52));
        assert_eq!(layout.piece_size(3), None);
    }

    #[test]
    fn piece_ranges_clamp_to_the_payload() {
        let layout = multi();
        assert_eq!(layout.piece_range(0), Some(0..1024));
        assert_eq!(layout.piece_range(2), Some(2048..2100));
        assert_eq!(layout.pieces_range(0, 1), 0..2048);
        assert_eq!(layout.pieces_range(0, 99), 0..2100);
    }

    #[test]
    fn offsets_map_back_to_pieces_and_files() {
        let layout = multi();
        assert_eq!(layout.piece_at(0), Some(0));
        assert_eq!(layout.piece_at(1023), Some(0));
        assert_eq!(layout.piece_at(1024), Some(1));
        assert_eq!(layout.piece_at(2100), None);
        assert_eq!(layout.file_at(0), Some(0));
        assert_eq!(layout.file_at(1499), Some(0));
        assert_eq!(layout.file_at(1500), Some(1));
        assert_eq!(layout.file_at(2100), None);
    }

    #[test]
    fn a_range_splits_across_file_boundaries() {
        let layout = multi();
        let slices = layout.split_by_file(1400..2050);
        assert_eq!(
            slices,
            vec![
                FileSlice {
                    file: 0,
                    offset: 1400,
                    length: 100
                },
                FileSlice {
                    file: 1,
                    offset: 0,
                    length: 500
                },
                FileSlice {
                    file: 2,
                    offset: 0,
                    length: 50
                },
            ]
        );
    }

    #[test]
    fn zero_length_files_are_never_asked_for_bytes() {
        let layout = Layout::from_lengths(
            "t",
            true,
            16,
            [
                ("a".to_string(), 50u64),
                ("empty".to_string(), 0),
                ("b".to_string(), 50),
            ],
        );
        assert_eq!(layout.file_at(50), Some(2));
        let slices = layout.split_by_file(40..60);
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].file, 0);
        assert_eq!(slices[1].file, 2);
    }

    #[test]
    fn overlapping_pieces_cover_the_whole_range() {
        let layout = multi();
        assert_eq!(layout.pieces_overlapping(&(0..1)), 0..1);
        assert_eq!(layout.pieces_overlapping(&(1023..1025)), 0..2);
        assert_eq!(layout.pieces_overlapping(&(0..2100)), 0..3);
        assert_eq!(layout.pieces_overlapping(&(5..5)), 0..0);
    }

    #[test]
    fn a_single_file_torrent_has_one_file_at_offset_zero() {
        let layout = Layout::from_lengths(
            "movie.mkv",
            false,
            4096,
            [("movie.mkv".to_string(), 9000u64)],
        );
        assert_eq!(layout.files.len(), 1);
        assert_eq!(layout.payload(), 0..9000);
        assert_eq!(layout.piece_count(), 3);
    }
}
