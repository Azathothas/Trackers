//! A payload that is an exact multiple of the piece length.
//!
//! This is the case `librqbit` 9.0.0's `create_torrent` gets wrong: its final
//! flush tests `remaining_piece_length > 0 && length > 0`, and by then the
//! loop that closed the last complete piece has already reset
//! `remaining_piece_length` to a whole piece, so it hashes nothing and appends
//! the SHA-1 of an empty buffer. The result declares one more piece than the
//! payload has.
//!
//! `bit-cli` creates torrents with its own code, so nothing it writes has the
//! defect. The test is here rather than under a fix because the defect is
//! upstream's and this is what pins it: both creators run over the same bytes,
//! and if `librqbit` is fixed, the second half of this fails and
//! `TODO/create-seed.md` T-080 gets its answer.
//!
//! See `TODO/create-seed.md`, T-080.

use std::path::Path;

use bit_cli_core::torrent::create::{CreateOptions, InputFile, create};
use bit_cli_core::torrent::{Lint, Metainfo};

/// Ten whole pieces and not a byte more.
const PIECE_LENGTH: u32 = 32 * 1024;
const PIECES: u64 = 10;
const TOTAL: u64 = PIECE_LENGTH as u64 * PIECES;

fn payload(root: &Path) -> std::path::PathBuf {
    let file = root.join("aligned.bin");
    std::fs::write(&file, vec![0x5Au8; TOTAL as usize]).expect("write the payload");
    file
}

#[test]
fn bit_cli_writes_one_hash_per_piece_for_an_exactly_aligned_payload() {
    let temp = tempfile::tempdir().expect("temp dir");
    let file = payload(temp.path());

    let created = create(
        vec![InputFile {
            source: file.clone(),
            path: "aligned.bin".to_string(),
            length: TOTAL,
        }],
        &CreateOptions {
            name: "aligned.bin".to_string(),
            multi_file: false,
            piece_length: Some(PIECE_LENGTH),
            allowed_lints: Lint::ALL.iter().copied().collect(),
            ..Default::default()
        },
        |path: &Path| {
            std::fs::File::open(path)
                .map_err(|e| bit_cli_core::error::from_io(e, format!("open {}", path.display())))
        },
    )
    .expect("create");

    let meta = Metainfo::parse(&created.bytes).expect("the torrent it wrote parses");
    assert_eq!(meta.info().pieces.len() as u64, PIECES);
    assert_eq!(meta.layout().total_length, TOTAL);
}

/// The same payload through `librqbit`, which writes eleven hashes for ten
/// pieces. `bit-cli`'s own parser refuses the result, which is what would have
/// happened to a caller handed one of these by another tool.
#[test]
fn librqbit_writes_one_hash_too_many_and_bit_cli_refuses_it() {
    let temp = tempfile::tempdir().expect("temp dir");
    let file = payload(temp.path());

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let created = runtime
        .block_on(async {
            librqbit::create_torrent(
                &file,
                librqbit::CreateTorrentOptions {
                    name: Some("aligned.bin"),
                    piece_length: Some(PIECE_LENGTH),
                    ..Default::default()
                },
                &librqbit::spawn_utils::BlockingSpawner::new(1),
            )
            .await
        })
        .expect("librqbit create_torrent");
    let bytes = created.as_bytes().expect("serialize");

    let error = Metainfo::parse(&bytes).expect_err("a torrent with a spurious hash parses");
    let message = error.to_string();
    assert!(
        message.contains("11 pieces") && message.contains("needs 10"),
        "the refusal does not name the count: {message}"
    );
}
