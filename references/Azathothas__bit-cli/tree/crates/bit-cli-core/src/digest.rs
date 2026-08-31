//! Hashing a file, streamed, in the three algorithms this tree can compute.
//!
//! One implementation, because there were two: [`crate::metalink`] needed a
//! streaming digest to check a Metalink's own checksum, and
//! `download --verify-on-complete` needs the same thing to report a digest per
//! file. A second copy would be a second answer to "what does this file hash
//! to", which is the one question where two answers is the whole problem. See
//! `TODO/multi-source.md`, T-136.
//!
//! **Streamed in fixed-size reads**, so peak memory does not depend on the
//! payload size. The file this hashes is the file that was just downloaded and
//! it can be an ISO.

use std::io::Read;
use std::path::Path;

use crate::error::{Error, Result, from_io};

/// Bytes read per `read` call.
///
/// 256 KiB rather than a piece length: this reads whole files rather than
/// pieces, and the number that matters is that it is a constant.
const CHUNK: usize = 256 * 1024;

/// What hashing a file produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDigest {
    /// The algorithm, in this module's normalised spelling.
    pub algorithm: String,
    /// Lowercase hex.
    pub hex: String,
    /// Bytes read. A caller comparing this against the length a torrent
    /// declares learns whether it hashed the file it meant to.
    pub bytes: u64,
}

/// Hash one file.
///
/// An unsupported algorithm is an error rather than a pass. A digest that was
/// not computed is not a digest that matched.
pub fn hash_file(path: &Path, algorithm: &str) -> Result<FileDigest> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| from_io(e, format!("cannot read {}", path.display())))?;
    let mut digest = Digest::new(algorithm)?;
    let mut buffer = vec![0u8; CHUNK];
    let mut bytes = 0u64;
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| from_io(e, format!("cannot read {}", path.display())))?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        digest.update(&buffer[..n]);
    }
    Ok(FileDigest {
        algorithm: algorithm.to_string(),
        hex: digest.finish(),
        bytes,
    })
}

/// One of the three digests, behind one interface.
///
/// An enum rather than a `Box<dyn Digest>`: `digest::DynDigest` would work and
/// pulls a trait object into a loop that runs once per 256 KiB, and there are
/// exactly three algorithms.
pub enum Digest {
    Sha256(sha2::Sha256),
    Sha1(sha1::Sha1),
    Md5(md5::Md5),
}

impl Digest {
    /// A digest for `algorithm`, or a usage error naming what was asked for.
    pub fn new(algorithm: &str) -> Result<Self> {
        use sha2::Digest as _;
        match algorithm {
            "sha256" => Ok(Self::Sha256(sha2::Sha256::new())),
            "sha1" => Ok(Self::Sha1(sha1::Sha1::new())),
            "md5" => Ok(Self::Md5(md5::Md5::new())),
            other => Err(
                Error::usage(format!("{other} is not an algorithm this can compute"))
                    .with("algorithm", other.to_string()),
            ),
        }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        use sha2::Digest as _;
        match self {
            Self::Sha256(d) => d.update(bytes),
            Self::Sha1(d) => d.update(bytes),
            Self::Md5(d) => d.update(bytes),
        }
    }

    /// Lowercase hex.
    pub fn finish(self) -> String {
        use sha2::Digest as _;
        let bytes: Vec<u8> = match self {
            Self::Sha256(d) => d.finalize().to_vec(),
            Self::Sha1(d) => d.finalize().to_vec(),
            Self::Md5(d) => d.finalize().to_vec(),
        };
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("payload.bin");
        std::fs::write(&path, bytes).expect("write the payload");
        (dir, path)
    }

    /// The published vectors for the empty input, in all three algorithms.
    /// Checked against the algorithms rather than against this code's own
    /// previous output, which would only prove it is consistent.
    #[test]
    fn the_empty_file_hashes_to_the_published_vectors() {
        let (_dir, path) = written(b"");
        for (algorithm, expected) in [
            (
                "sha256",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            ("sha1", "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
            ("md5", "d41d8cd98f00b204e9800998ecf8427e"),
        ] {
            let digest = hash_file(&path, algorithm).expect("hash the file");
            assert_eq!(digest.hex, expected, "{algorithm}");
            assert_eq!(digest.bytes, 0);
            assert_eq!(digest.algorithm, algorithm);
        }
    }

    /// `abc`, the other vector every one of the three publishes.
    #[test]
    fn abc_hashes_to_the_published_vectors() {
        let (_dir, path) = written(b"abc");
        for (algorithm, expected) in [
            (
                "sha256",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            ("sha1", "a9993e364706816aba3e25717850c26c9cd0d89d"),
            ("md5", "900150983cd24fb0d6963f7d28e17f72"),
        ] {
            let digest = hash_file(&path, algorithm).expect("hash the file");
            assert_eq!(digest.hex, expected, "{algorithm}");
            assert_eq!(digest.bytes, 3);
        }
    }

    /// Longer than one read, so the streaming loop is what is being tested
    /// rather than a single `update`.
    #[test]
    fn a_file_larger_than_one_read_hashes_the_same_as_the_bytes_do() {
        let bytes: Vec<u8> = (0..CHUNK * 3 + 17).map(|i| (i % 251) as u8).collect();
        let (_dir, path) = written(&bytes);
        let streamed = hash_file(&path, "sha256").expect("hash the file");
        assert_eq!(streamed.bytes, bytes.len() as u64);

        let mut whole = Digest::new("sha256").expect("a digest");
        whole.update(&bytes);
        assert_eq!(streamed.hex, whole.finish());
    }

    #[test]
    fn an_algorithm_this_cannot_compute_is_an_error() {
        let (_dir, path) = written(b"x");
        let error = hash_file(&path, "sha512").expect_err("sha512 is not one of the three");
        assert_eq!(error.code(), crate::ExitCode::Usage);
        assert!(error.message().contains("sha512"), "{}", error.message());
    }

    #[test]
    fn a_file_that_is_not_there_is_an_error_rather_than_an_empty_digest() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        assert!(hash_file(&dir.path().join("absent.bin"), "sha256").is_err());
    }
}
