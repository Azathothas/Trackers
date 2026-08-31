//! `bit-cli tree`: the shape a torrent carries, rolled up by directory.
//!
//! `bit-cli files` prints one row per file, which is the right answer for two
//! files and the wrong one for four hundred: the directory structure the
//! torrent actually carries ends up in the path column for a reader to
//! reassemble. Nothing new is measured here. It is the same
//! [`bit_cli_core::layout::Layout`] `files` reads, rendered as a tree.
//!
//! See `TODO/metainfo.md`, T-249.

use std::ops::Range;

use bit_cli_core::ExitCode;
use bit_cli_core::error::Result;
use bit_cli_core::layout::Layout;
use bit_cli_core::units::{Size, format_size};
use serde::Serialize;

use crate::cli::{Global, TreeArgs};
use crate::env::Env;
use crate::output::{Renderer, table};
use crate::source::{Kind, resolve_source};

/// What one line of the tree is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Directory,
    File,
    /// A BEP 47 padding file: real bytes in the linear stream, no content of
    /// its own. Marked rather than hidden, because its bytes are in every
    /// total above it and a reader subtracting them has to know they are
    /// there.
    Padding,
}

impl NodeKind {
    const fn is_file(self) -> bool {
        matches!(self, Self::File | Self::Padding)
    }
}

/// What a `--depth` cut off, so nothing is dropped in silence.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Hidden {
    pub files: usize,
    pub directories: usize,
}

/// One node of the tree.
///
/// The nodes are a **flat list in pre-order**, not a nested structure. A
/// nested one would put the same field at a different JSON path for every
/// depth, so `docs/schema.md` would carry as many `children[].children[]` rows
/// as the deepest fixture happened to have. Flat, `depth` says where a node
/// sits and the order says what it sits under.
#[derive(Debug, Clone, Serialize)]
pub struct Node {
    /// Distance from the root, which is zero.
    pub depth: usize,
    /// Path from the torrent root, `/`-separated, without the torrent name.
    /// Empty for the root of a multi-file torrent.
    pub path: String,
    /// The last component, which is what the tree prints.
    pub name: String,
    pub kind: NodeKind,
    /// Index of the file within the torrent, for a file and a padding file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    /// Bytes below this node, padding included.
    pub size: Size,
    /// Files below this node, padding included. One for a file.
    pub file_count: usize,
    /// Directories below this node. Zero for a file.
    pub directory_count: usize,
    /// First piece any file below this node touches.
    pub first_piece: u32,
    /// Last piece any file below this node touches.
    pub last_piece: u32,
    /// Pieces in `first_piece..=last_piece` that also hold bytes of a file
    /// **outside** this node.
    ///
    /// The span alone does not say whether a subtree can be fetched without
    /// touching the rest, which is the reason for wanting it: a piece
    /// straddling a directory boundary belongs to both sides, and a torrent
    /// may interleave two directories in its file order. Zero here is what
    /// says the span is the subtree's own.
    pub shared_pieces: u32,
    /// What `--depth` cut off below this node. Absent when nothing was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<Hidden>,
}

/// What `bit-cli tree` reports.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub info_hash: String,
    pub name: String,
    /// The whole payload, which is what `bit-cli info` reports as `total`.
    pub total: Size,
    pub file_count: usize,
    pub directory_count: usize,
    /// BEP 47 padding files, counted in `file_count` and in every size.
    pub padding_count: usize,
    pub padding_total: Size,
    /// Depth of the deepest node the torrent carries, before `--depth`.
    pub max_depth: usize,
    /// The `--depth` that was applied. Absent when the whole tree is printed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_limit: Option<usize>,
    /// How this torrent's `name` and `path` keys were turned into text, when
    /// that says something the ordinary torrent's does not. Shared with `info`
    /// and `files`. See `TODO/bep-coverage.md`, T-103.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_encoding: Option<bit_cli_core::torrent::NameEncoding>,
    pub nodes: Vec<Node>,
}

/// `n thing` or `n things`, so a count of one does not read as a template.
fn plural(count: usize, one: &str, many: &str) -> String {
    match count {
        1 => format!("1 {one}"),
        _ => format!("{count} {many}"),
    }
}

/// The four strings a tree is drawn with.
///
/// ASCII is the default and the box-drawing set is behind the same decision
/// `--color` already makes. A code point outside a console's code page has
/// cost this repository a red CI job before, and every context that is not a
/// terminal which asked for colour gets the set that cannot.
struct Glyphs {
    branch: &'static str,
    last: &'static str,
    riser: &'static str,
    blank: &'static str,
}

impl Glyphs {
    const ASCII: Self = Self {
        branch: "|-- ",
        last: "`-- ",
        riser: "|   ",
        blank: "    ",
    };

    const BOX: Self = Self {
        branch: "\u{251c}\u{2500}\u{2500} ",
        last: "\u{2514}\u{2500}\u{2500} ",
        riser: "\u{2502}   ",
        blank: "    ",
    };
}

impl Report {
    /// The text rendering.
    fn lines(&self, glyphs: &Glyphs, sizes: bool) -> Vec<String> {
        // One entry per open level: whether the ancestor at that depth still
        // has a sibling below it, which is what says to draw a vertical bar
        // rather than a gap.
        let mut risers: Vec<bool> = Vec::new();
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut shared_seen = false;

        let draw = |risers: &[bool]| {
            let mut out = String::new();
            for open in risers {
                out.push_str(match open {
                    true => glyphs.riser,
                    false => glyphs.blank,
                });
            }
            out
        };

        for (position, node) in self.nodes.iter().enumerate() {
            // Pre-order, so a later sibling is the next node at this depth
            // reached before the walk climbs above it.
            let more = self
                .nodes
                .iter()
                .skip(position + 1)
                .take_while(|other| other.depth >= node.depth)
                .any(|other| other.depth == node.depth);
            risers.truncate(node.depth.saturating_sub(1));
            let mut label = draw(&risers);
            if node.depth > 0 {
                label.push_str(match more {
                    true => glyphs.branch,
                    false => glyphs.last,
                });
                risers.push(more);
            }
            label.push_str(&node.name);
            match node.kind {
                NodeKind::Directory => label.push('/'),
                NodeKind::Padding => label.push_str(" (padding)"),
                NodeKind::File => {}
            }

            let pieces = match node.file_count {
                0 => "-".to_string(),
                _ => {
                    let mut span = format!("{}-{}", node.first_piece, node.last_piece);
                    if node.shared_pieces > 0 {
                        span.push('+');
                        shared_seen = true;
                    }
                    span
                }
            };

            let mut row = vec![label];
            if sizes {
                row.push(format_size(node.size.0));
                row.push(match node.kind.is_file() {
                    true => String::new(),
                    false => node.file_count.to_string(),
                });
            }
            row.push(pieces);
            rows.push(row);

            if let Some(hidden) = node.hidden {
                let mut label = draw(&risers);
                label.push_str(glyphs.last);
                let mut counts = Vec::new();
                if hidden.files > 0 {
                    counts.push(plural(hidden.files, "file", "files"));
                }
                if hidden.directories > 0 {
                    counts.push(plural(hidden.directories, "directory", "directories"));
                }
                label.push_str(&format!("{} not shown", counts.join(" and ")));
                let mut row = vec![label];
                if sizes {
                    row.push(String::new());
                    row.push(String::new());
                }
                row.push(String::new());
                rows.push(row);
            }
        }

        let headers: &[&str] = match sizes {
            true => &["PATH", "SIZE", "FILES", "PIECES"],
            false => &["PATH", "PIECES"],
        };
        // A rolled-up line has nothing in any column but the first, and
        // `table` pads every column but the last, so the row would end in the
        // width of two. Trimming here rather than in `table` keeps that
        // function's rule, which is that only the last column is unpadded.
        let mut out: Vec<String> = table(headers, &rows)
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .collect();

        out.push(String::new());
        out.push(format!(
            "{}, {}, {}",
            plural(self.file_count, "file", "files"),
            plural(self.directory_count, "directory", "directories"),
            format_size(self.total.0)
        ));
        if self.padding_count > 0 {
            out.push(format!(
                "{}, {}, counted in every total above",
                plural(self.padding_count, "padding file", "padding files"),
                format_size(self.padding_total.0)
            ));
        }
        if shared_seen {
            out.push(
                "a + on a piece range means the span also holds bytes of a file outside that entry"
                    .to_string(),
            );
        }
        if let Some(limit) = self.depth_limit
            && self.max_depth > limit
        {
            out.push(format!(
                "depth limited to {limit}, and the torrent goes {} deep",
                self.max_depth
            ));
        }
        if let Some(encoding) = &self.name_encoding {
            out.push(format!("names decoded as {}", encoding.describe()));
        }
        out
    }
}

/// A directory being built.
struct Dir {
    name: String,
    path: String,
    /// Children in the order the torrent lists them: a directory takes the
    /// position of the first file that put it there. So the tree is the
    /// torrent's own file order, grouped, rather than an order this command
    /// invented. Grouping is what makes it differ from `files --sort index`:
    /// a file whose directory appeared earlier prints above a file with a
    /// lower index whose directory appeared later.
    children: Vec<Child>,
}

#[derive(Clone, Copy)]
enum Child {
    Dir(usize),
    File(usize),
}

/// What rolling a subtree up produced.
#[derive(Default, Clone)]
struct Rolled {
    size: u64,
    files: usize,
    directories: usize,
    padding: usize,
    padding_bytes: u64,
    /// Whether any file below this node touches a piece at all. A directory
    /// holding only zero-length files has a size and no span.
    spans: bool,
    first_piece: u32,
    last_piece: u32,
    /// Byte ranges the subtree occupies, merged and sorted. One entry for the
    /// ordinary directory, whose files are contiguous.
    ranges: Vec<Range<u64>>,
}

impl Rolled {
    /// How many pieces the span covers.
    const fn span(&self) -> u32 {
        match self.spans {
            true => self.last_piece - self.first_piece + 1,
            false => 0,
        }
    }
}

/// Merge ranges into the fewest that cover the same bytes.
fn merge(mut ranges: Vec<Range<u64>>) -> Vec<Range<u64>> {
    ranges.retain(|range| range.start < range.end);
    ranges.sort_by_key(|range| range.start);
    let mut out: Vec<Range<u64>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match out.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => out.push(range),
        }
    }
    out
}

/// How many whole pieces lie entirely inside `ranges`.
///
/// A piece that is only partly covered holds bytes of a file outside the
/// subtree, which is the one thing the span does not say on its own.
fn pieces_wholly_inside(ranges: &[Range<u64>], layout: &Layout) -> u32 {
    let length = u64::from(layout.piece_length);
    let count = u64::from(layout.piece_count());
    if length == 0 || count == 0 {
        return 0;
    }
    let mut inside: u64 = 0;
    for range in ranges {
        let low = range.start.div_ceil(length);
        // The final piece ends at the payload's end rather than at a multiple
        // of the piece length, so a range reaching the end covers it whole.
        let high = match range.end >= layout.total_length {
            true => Some(count - 1),
            false => (range.end / length).checked_sub(1),
        };
        if let Some(high) = high
            && high >= low
        {
            inside += high - low + 1;
        }
    }
    inside.min(count) as u32
}

/// Build the arena, root first, every child at a higher index than its parent.
fn build(layout: &Layout) -> (Vec<Dir>, usize) {
    let mut arena = vec![Dir {
        name: layout.name.clone(),
        path: String::new(),
        children: Vec::new(),
    }];
    let mut deepest = 0;
    for (index, file) in layout.files.iter().enumerate() {
        let components = &file.path;
        let directories = &components[..components.len().saturating_sub(1)];
        let mut at = 0usize;
        for component in directories {
            let existing = arena[at].children.iter().find_map(|child| match child {
                Child::Dir(node) if arena[*node].name == *component => Some(*node),
                _ => None,
            });
            at = match existing {
                Some(node) => node,
                None => {
                    let path = match arena[at].path.is_empty() {
                        true => component.clone(),
                        false => format!("{}/{component}", arena[at].path),
                    };
                    arena.push(Dir {
                        name: component.clone(),
                        path,
                        children: Vec::new(),
                    });
                    let node = arena.len() - 1;
                    arena[at].children.push(Child::Dir(node));
                    node
                }
            };
        }
        arena[at].children.push(Child::File(index));
        deepest = deepest.max(components.len());
    }
    (arena, deepest)
}

/// Roll every directory up, from the leaves back to the root.
///
/// In reverse index order rather than by recursion: [`build`] pushes a child
/// after its parent, so every child has a higher index and one backward pass
/// is a post-order traversal that no torrent's path depth can overflow.
fn rollup(arena: &[Dir], layout: &Layout, padding: &[bool]) -> Vec<Rolled> {
    let mut rolled: Vec<Rolled> = vec![Rolled::default(); arena.len()];
    for node in (0..arena.len()).rev() {
        let mut out = Rolled::default();
        let mut ranges = Vec::new();
        for child in &arena[node].children {
            match child {
                Child::Dir(index) => {
                    let below = rolled[*index].clone();
                    out.size += below.size;
                    out.files += below.files;
                    out.directories += below.directories + 1;
                    out.padding += below.padding;
                    out.padding_bytes += below.padding_bytes;
                    if below.spans {
                        out.first_piece = match out.spans {
                            true => out.first_piece.min(below.first_piece),
                            false => below.first_piece,
                        };
                        out.last_piece = out.last_piece.max(below.last_piece);
                        out.spans = true;
                    }
                    ranges.extend(below.ranges);
                }
                Child::File(index) => {
                    let file = &layout.files[*index];
                    out.size += file.length;
                    out.files += 1;
                    if padding.get(*index).copied().unwrap_or(false) {
                        out.padding += 1;
                        out.padding_bytes += file.length;
                    }
                    let pieces = layout.pieces_overlapping(&file.range());
                    if pieces.start < pieces.end {
                        out.first_piece = match out.spans {
                            true => out.first_piece.min(pieces.start),
                            false => pieces.start,
                        };
                        out.last_piece = out.last_piece.max(pieces.end - 1);
                        out.spans = true;
                    }
                    ranges.push(file.range());
                }
            }
        }
        out.ranges = merge(ranges);
        rolled[node] = out;
    }
    rolled
}

/// Walk the arena in pre-order and emit one [`Node`] per line.
///
/// `root_is_file` is the single-file torrent, whose root **is** the file: it
/// has no directory, and inventing one would print a level the torrent does
/// not carry.
fn flatten(
    arena: &[Dir],
    rolled: &[Rolled],
    layout: &Layout,
    padding: &[bool],
    limit: Option<usize>,
    root_is_file: bool,
) -> Vec<Node> {
    let file_node = |index: usize, depth: usize| {
        let file = &layout.files[index];
        let pieces = layout.pieces_overlapping(&file.range());
        let spans = pieces.start < pieces.end;
        Node {
            depth,
            path: file.display_path(),
            name: file.file_name().to_string(),
            kind: match padding.get(index).copied().unwrap_or(false) {
                true => NodeKind::Padding,
                false => NodeKind::File,
            },
            index: Some(index),
            size: Size(file.length),
            file_count: 1,
            directory_count: 0,
            first_piece: match spans {
                true => pieces.start,
                false => 0,
            },
            last_piece: match spans {
                true => pieces.end - 1,
                false => 0,
            },
            shared_pieces: match spans {
                true => (pieces.end - pieces.start)
                    .saturating_sub(pieces_wholly_inside(&[file.range()], layout)),
                false => 0,
            },
            hidden: None,
        }
    };

    if root_is_file {
        return vec![file_node(0, 0)];
    }

    let mut out = Vec::new();
    let mut stack: Vec<(Child, usize)> = vec![(Child::Dir(0), 0)];
    while let Some((child, depth)) = stack.pop() {
        match child {
            Child::Dir(node) => {
                let rolled = &rolled[node];
                let cut = limit.is_some_and(|limit| depth >= limit);
                let hidden = match cut && !arena[node].children.is_empty() {
                    true => Some(Hidden {
                        files: rolled.files,
                        directories: rolled.directories,
                    }),
                    false => None,
                };
                out.push(Node {
                    depth,
                    path: arena[node].path.clone(),
                    name: arena[node].name.clone(),
                    kind: NodeKind::Directory,
                    index: None,
                    size: Size(rolled.size),
                    file_count: rolled.files,
                    directory_count: rolled.directories,
                    first_piece: rolled.first_piece,
                    last_piece: rolled.last_piece,
                    shared_pieces: rolled
                        .span()
                        .saturating_sub(pieces_wholly_inside(&rolled.ranges, layout)),
                    hidden,
                });
                if cut {
                    continue;
                }
                for child in arena[node].children.iter().rev() {
                    stack.push((*child, depth + 1));
                }
            }
            Child::File(index) => out.push(file_node(index, depth)),
        }
    }
    out
}

/// Run the command.
pub fn run(
    args: &TreeArgs,
    global: &Global,
    renderer: &mut Renderer,
    env: &mut Env,
) -> Result<ExitCode> {
    let kind = Kind::classify(&args.source.source, env)?;
    let meta = resolve_source(&kind, env, global, None, &args.swarm, &args.page)?;
    let layout = meta.layout();
    let padding: Vec<bool> = meta.info().files.iter().map(|f| f.is_padding()).collect();

    let root_is_file = !layout.multi_file && layout.files.len() == 1;
    let (arena, deepest) = build(&layout);
    let rolled = rollup(&arena, &layout, &padding);
    let nodes = flatten(&arena, &rolled, &layout, &padding, args.depth, root_is_file);

    let report = Report {
        info_hash: meta.info_hash().hex(),
        name: layout.name.clone(),
        total: Size(layout.total_length),
        file_count: rolled[0].files,
        directory_count: rolled[0].directories,
        padding_count: rolled[0].padding,
        padding_total: Size(rolled[0].padding_bytes),
        max_depth: match root_is_file {
            true => 0,
            false => deepest,
        },
        depth_limit: args.depth,
        name_encoding: crate::cmd::info::reportable_name_encoding(meta.info().name_encoding),
        nodes,
    };

    // Two conditions, and both are about the sink rather than the torrent.
    // Colour is the knob the caller already has for "this is for a person to
    // look at", and `out_is_unicode` is whether the thing at the far end can
    // carry a box-drawing character at all. A console at `IBM437` cannot, and
    // this repository has cost itself a red job over exactly that.
    let glyphs = match env.wants_color(global.color.into()) && env.out_is_unicode {
        true => &Glyphs::BOX,
        false => &Glyphs::ASCII,
    };
    let sizes = !args.no_sizes;
    renderer.emit(env, "tree", &report, || report.lines(glyphs, sizes))?;
    Ok(ExitCode::Success)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TorrentFixture, run_err, run_json, run_ok};

    #[test]
    fn a_multi_file_torrent_prints_its_directories() {
        let fixture = TorrentFixture::multi_file();
        let out = run_ok(&["tree", fixture.path_str()], fixture.dir());
        assert!(out.contains("album/"), "{out}");
        assert!(out.contains("disc 1/"), "{out}");
        assert!(out.contains("a.flac"), "{out}");
        assert!(out.contains("notes.nfo"), "{out}");
        // The path column carries the leaf, not the whole path: that is the
        // difference between this and `files`.
        assert!(!out.contains("disc 1/a.flac"), "{out}");
    }

    #[test]
    fn the_tree_is_ascii_when_nothing_asked_for_colour() {
        let fixture = TorrentFixture::deep();
        let out = run_ok(&["tree", fixture.path_str()], fixture.dir());
        assert!(out.contains("`-- "), "{out}");
        assert!(out.is_ascii(), "{out}");
    }

    /// The acceptance's last clause. A console at `IBM437` cannot render a
    /// box-drawing character, so it gets the ASCII set whatever `--color`
    /// says. See `TODO/metainfo.md`, T-249.
    #[test]
    fn a_console_that_cannot_carry_the_glyphs_gets_ascii_anyway() {
        let fixture = TorrentFixture::multi_file();
        let (mut env, captured) = Env::test(
            &["--color", "always", "tree", fixture.path_str()],
            fixture.dir(),
        );
        env.out_is_terminal = true;
        env.out_is_unicode = false;
        assert_eq!(crate::run(&mut env), ExitCode::Success);
        let out = captured.out();
        assert!(out.contains("`-- "), "{out}");
        assert!(out.is_ascii(), "{out}");
    }

    #[test]
    fn colour_switches_the_glyphs_to_box_drawing() {
        let fixture = TorrentFixture::multi_file();
        let out = run_ok(
            &["--color", "always", "tree", fixture.path_str()],
            fixture.dir(),
        );
        assert!(out.contains('\u{2514}'), "{out}");
        assert!(!out.contains("`-- "), "{out}");
    }

    /// The acceptance's first half: directory totals sum to what `info`
    /// reports.
    #[test]
    fn directory_totals_sum_to_the_whole_payload() {
        let fixture = TorrentFixture::multi_file();
        let tree = run_json(&["tree", fixture.path_str()], fixture.dir());
        let info = run_json(&["info", fixture.path_str()], fixture.dir());
        assert_eq!(tree["total"]["bytes"], info["total"]["bytes"]);
        assert_eq!(tree["file_count"], info["file_count"]);

        let nodes = tree["nodes"].as_array().expect("nodes");
        let root = &nodes[0];
        assert_eq!(root["depth"], 0);
        assert_eq!(root["kind"], "directory");
        assert_eq!(root["size"]["bytes"], info["total"]["bytes"]);

        let leaves: u64 = nodes
            .iter()
            .filter(|node| node["kind"] != "directory")
            .map(|node| node["size"]["bytes"].as_u64().expect("bytes"))
            .sum();
        assert_eq!(leaves, root["size"]["bytes"].as_u64().expect("bytes"));
    }

    #[test]
    fn a_single_file_torrent_is_one_line() {
        let fixture = TorrentFixture::single_file();
        let doc = run_json(&["tree", fixture.path_str()], fixture.dir());
        let nodes = doc["nodes"].as_array().expect("nodes");
        assert_eq!(nodes.len(), 1, "{doc}");
        assert_eq!(nodes[0]["kind"], "file");
        assert_eq!(nodes[0]["name"], "payload.bin");
        assert_eq!(nodes[0]["depth"], 0);
        assert_eq!(doc["directory_count"], 0);
        assert_eq!(doc["max_depth"], 0);
    }

    #[test]
    fn depth_rolls_the_rest_up_and_says_what_it_hid() {
        let fixture = TorrentFixture::deep();
        let doc = run_json(&["tree", "--depth", "1", fixture.path_str()], fixture.dir());
        let nodes = doc["nodes"].as_array().expect("nodes");
        assert_eq!(nodes.len(), 2, "{doc}");
        assert_eq!(nodes[1]["depth"], 1);
        assert_eq!(nodes[1]["hidden"]["files"], 1);
        assert_eq!(nodes[1]["hidden"]["directories"], 4);
        assert_eq!(doc["max_depth"], 6);
        assert_eq!(doc["depth_limit"], 1);

        let text = run_ok(&["tree", "--depth", "1", fixture.path_str()], fixture.dir());
        assert!(text.contains("not shown"), "{text}");
        assert!(text.contains("depth limited to 1"), "{text}");
    }

    #[test]
    fn no_depth_limit_leaves_the_field_out() {
        let fixture = TorrentFixture::multi_file();
        let doc = run_json(&["tree", fixture.path_str()], fixture.dir());
        assert!(doc["depth_limit"].is_null(), "{doc}");
    }

    #[test]
    fn no_sizes_drops_the_two_size_columns_and_nothing_else() {
        let fixture = TorrentFixture::multi_file();
        let out = run_ok(&["tree", "--no-sizes", fixture.path_str()], fixture.dir());
        assert!(out.starts_with("PATH"), "{out}");
        assert!(!out.contains("SIZE"), "{out}");
        assert!(out.contains("PIECES"), "{out}");
        assert!(out.contains("a.flac"), "{out}");
    }

    /// A piece that straddles a boundary belongs to both sides, so the span
    /// alone does not say a subtree can be fetched on its own. `shared_pieces`
    /// is what does. See `TODO/metainfo.md`, T-249.
    #[test]
    fn a_straddling_boundary_is_reported_as_a_shared_piece() {
        let fixture = TorrentFixture::straddling();
        let doc = run_json(&["tree", fixture.path_str()], fixture.dir());
        let nodes = doc["nodes"].as_array().expect("nodes");
        // `a.bin` is 1500 bytes at 1024 byte pieces: piece 0 is its own and
        // piece 1 it shares with `b.bin`.
        let a = nodes.iter().find(|n| n["name"] == "a.bin").expect("a.bin");
        assert_eq!(a["first_piece"], 0);
        assert_eq!(a["last_piece"], 1);
        assert_eq!(a["shared_pieces"], 1, "{a}");

        // The root holds every file, so nothing in its span is outside it.
        assert_eq!(nodes[0]["shared_pieces"], 0, "{}", nodes[0]);

        let text = run_ok(&["tree", fixture.path_str()], fixture.dir());
        assert!(text.contains("0-1+"), "{text}");
        assert!(text.contains("outside that entry"), "{text}");
    }

    #[test]
    fn a_padding_file_is_marked_rather_than_hidden() {
        let fixture = TorrentFixture::padded();
        let doc = run_json(&["tree", fixture.path_str()], fixture.dir());
        assert_eq!(doc["padding_count"], 1);
        assert_eq!(doc["padding_total"]["bytes"], 548);
        let nodes = doc["nodes"].as_array().expect("nodes");
        let pad = nodes
            .iter()
            .find(|node| node["kind"] == "padding")
            .expect("a padding node");
        assert_eq!(pad["name"], "548");
        assert_eq!(pad["size"]["bytes"], 548);

        // Its bytes are in the root total, which is what `info` reports.
        let info = run_json(&["info", fixture.path_str()], fixture.dir());
        assert_eq!(doc["total"]["bytes"], info["total"]["bytes"]);

        let text = run_ok(&["tree", fixture.path_str()], fixture.dir());
        assert!(text.contains("548 (padding)"), "{text}");
        assert!(text.contains("counted in every total above"), "{text}");
    }

    /// The whole acceptance in one run: three levels, a padding file marked,
    /// totals that sum to `info`, and ASCII.
    #[test]
    fn three_levels_a_padding_file_and_totals_that_add_up() {
        let fixture = TorrentFixture::padded();
        let doc = run_json(&["tree", fixture.path_str()], fixture.dir());
        assert_eq!(doc["max_depth"], 3);
        let nodes = doc["nodes"].as_array().expect("nodes");
        assert!(
            nodes.iter().any(|node| node["depth"] == 3),
            "no node three deep: {doc}"
        );
        let root = &nodes[0];
        assert_eq!(
            root["size"]["bytes"].as_u64().expect("bytes"),
            doc["total"]["bytes"].as_u64().expect("bytes")
        );
        let text = run_ok(&["tree", fixture.path_str()], fixture.dir());
        assert!(text.is_ascii(), "{text}");
    }

    #[test]
    fn a_torrent_that_cannot_be_read_fails_the_command() {
        let fixture = TorrentFixture::multi_file();
        let err = run_err(
            &["tree", "nope.torrent"],
            fixture.dir(),
            ExitCode::SourceResolution,
        );
        assert!(err.contains("nope.torrent"), "{err}");
    }

    /// `TODO/bep-coverage.md`, T-103, on the third command whose whole output
    /// is paths.
    #[test]
    fn a_path_that_is_not_utf8_is_printed_decoded() {
        let fixture = TorrentFixture::names_that_are_not_utf8();
        let doc = run_json(&["tree", fixture.path_str()], fixture.dir());
        assert_eq!(doc["name_encoding"]["utf8_keys"], true);
        let nodes = doc["nodes"].as_array().expect("nodes");
        assert!(nodes.iter().any(|node| node["name"] == "曲.bin"), "{doc}");
    }

    #[test]
    fn merging_ranges_joins_what_touches_and_keeps_what_does_not() {
        assert_eq!(merge(vec![0..10, 10..20]), vec![0..20]);
        assert_eq!(merge(vec![10..20, 0..10]), vec![0..20]);
        assert_eq!(merge(vec![0..10, 12..20]), vec![0..10, 12..20]);
        assert_eq!(merge(vec![0..10, 0..0, 5..7]), vec![0..10]);
    }
}
