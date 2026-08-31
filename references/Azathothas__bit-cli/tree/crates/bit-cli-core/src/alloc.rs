//! How space is reserved for a payload file.
//!
//! Four strategies, and they are four different behaviours rather than four
//! names for the same one. On a netdisk the difference between `sparse` and
//! `prealloc` is whether a half-finished 40 GiB torrent shows as 40 GiB of
//! committed space, which is the difference between a capacity plan that holds
//! and one that does not.
//!
//! | Strategy | What happens |
//! | --- | --- |
//! | `none` | The length is set and nothing else. What the filesystem does with the hole is its business. |
//! | `sparse` | The file is marked sparse first, so the hole is explicit rather than accidental. |
//! | `prealloc` | Zeroes are written across the whole file. Slow, and the space is certainly there. |
//! | `falloc` | The filesystem is asked to reserve the blocks without writing them. |
//!
//! `falloc` is a different call on every platform, and on one of them it is no
//! call at all. On Linux and the BSDs it is `posix_fallocate`. On the Apple
//! platforms that symbol does not exist and the interface is
//! `fcntl(F_PREALLOCATE)`, which reserves blocks without moving the end of the
//! file. On Windows the nearest equivalent, `SetFileValidData`, needs
//! `SeManageVolumePrivilege`, which an ordinary process does not hold, so it
//! degrades to `prealloc` and says so rather than failing. A benchmark that
//! silently did something other than what it was told is worse than one that
//! refuses.

use std::fs::File;
use std::io::Write;

/// How space is reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Allocation {
    /// Set the length and nothing else.
    None,
    /// Mark the file sparse, then set the length.
    #[default]
    Sparse,
    /// Write zeroes across the whole file.
    Prealloc,
    /// Ask the filesystem to reserve the blocks without writing them.
    Falloc,
}

impl Allocation {
    /// Every strategy, in the order `--help` lists them.
    pub const ALL: [Self; 4] = [Self::None, Self::Sparse, Self::Prealloc, Self::Falloc];

    /// The stable name used on the command line and in output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Sparse => "sparse",
            Self::Prealloc => "prealloc",
            Self::Falloc => "falloc",
        }
    }

    /// Parse a name, or report the ones that exist.
    pub fn parse(name: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|a| a.as_str() == name)
            .ok_or_else(|| {
                format!(
                    "`{name}` is not an allocation method (use {})",
                    Self::ALL
                        .iter()
                        .map(|a| a.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }
}

/// What reserving space actually did.
///
/// The strategy that ran is not always the one asked for, and a caller has to
/// be able to see that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub requested: Allocation,
    pub used: Allocation,
    /// Why the strategy changed, when it did.
    pub note: Option<String>,
}

impl Outcome {
    fn exact(strategy: Allocation) -> Self {
        Self {
            requested: strategy,
            used: strategy,
            note: None,
        }
    }

    fn degraded(requested: Allocation, used: Allocation, note: impl Into<String>) -> Self {
        Self {
            requested,
            used,
            note: Some(note.into()),
        }
    }

    /// Whether the strategy asked for is the one that ran.
    pub fn as_asked(&self) -> bool {
        self.requested == self.used
    }
}

/// Reserve `length` bytes for `file` under one strategy.
///
/// The length is always set, whatever the strategy, because the session needs
/// the file to be the size the torrent says before it writes into the middle
/// of it.
pub fn reserve(file: &File, length: u64, strategy: Allocation) -> std::io::Result<Outcome> {
    match strategy {
        Allocation::None => {
            file.set_len(length)?;
            Ok(Outcome::exact(strategy))
        }
        Allocation::Sparse => {
            // Marking comes before the length: a hole punched into a file that
            // is already long is a different operation on some filesystems,
            // and marking an empty file is always cheap.
            let marked = mark_sparse(file);
            file.set_len(length)?;
            match marked {
                Ok(()) => Ok(Outcome::exact(strategy)),
                Err(reason) => Ok(Outcome::degraded(strategy, Allocation::None, reason)),
            }
        }
        Allocation::Prealloc => {
            file.set_len(length)?;
            write_zeroes(file, length)?;
            Ok(Outcome::exact(strategy))
        }
        Allocation::Falloc => match fallocate(file, length) {
            Ok(()) => Ok(Outcome::exact(strategy)),
            Err(reason) => {
                file.set_len(length)?;
                write_zeroes(file, length)?;
                Ok(Outcome::degraded(strategy, Allocation::Prealloc, reason))
            }
        },
    }
}

/// The buffer zeroes are written from.
///
/// One megabyte is large enough that the syscall count is not the cost and
/// small enough that it is not worth thinking about the allocation.
const ZERO_CHUNK: usize = 1024 * 1024;

/// Write zeroes across the whole file.
fn write_zeroes(file: &File, length: u64) -> std::io::Result<()> {
    if length == 0 {
        return Ok(());
    }
    let zeroes = vec![0u8; ZERO_CHUNK.min(length as usize)];
    let mut written = 0u64;
    while written < length {
        let want = (length - written).min(zeroes.len() as u64) as usize;
        write_at(file, written, &zeroes[..want])?;
        written += want as u64;
    }
    // The bytes have to reach the filesystem for the space to be reserved.
    // Without this the whole point of `prealloc` is a page cache full of
    // zeroes that a full disk will refuse later.
    (&*file).flush()?;
    file.sync_all()
}

#[cfg(unix)]
fn write_at(file: &File, offset: u64, buf: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(buf, offset)
}

#[cfg(windows)]
fn write_at(file: &File, mut offset: u64, mut buf: &[u8]) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_write(buf, offset)? {
            0 => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "the write made no progress",
                ));
            }
            written => {
                offset += written as u64;
                buf = &buf[written..];
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn mark_sparse(file: &File) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;

    /// `FSCTL_SET_SPARSE`.
    const FSCTL_SET_SPARSE: u32 = 0x0009_00C4;

    unsafe extern "system" {
        fn DeviceIoControl(
            device: isize,
            control_code: u32,
            in_buffer: *mut u8,
            in_size: u32,
            out_buffer: *mut u8,
            out_size: u32,
            returned: *mut u32,
            overlapped: *mut u8,
        ) -> i32;
    }

    let mut returned: u32 = 0;
    // SAFETY: the handle comes from an open `File` this call does not outlive,
    // and both buffers are null with a zero size, which is what
    // `FSCTL_SET_SPARSE` takes.
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle() as isize,
            FSCTL_SET_SPARSE,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    match ok {
        0 => Err(format!(
            "the filesystem refused to mark the file sparse: {}",
            std::io::Error::last_os_error()
        )),
        _ => Ok(()),
    }
}

/// On Linux a file grown with `set_len` is already sparse on every filesystem
/// that supports holes, and there is no per-file flag to set on the ones that
/// do not.
#[cfg(unix)]
fn mark_sparse(_file: &File) -> Result<(), String> {
    Ok(())
}

/// `posix_fallocate` is not a unix interface, it is a Linux and BSD one.
///
/// The Apple platforms do not have the symbol at all, and OpenBSD does not
/// either, so a `#[cfg(unix)]` extern block declaring it compiles everywhere
/// and then fails at **link** time on those targets:
///
/// ```text
/// Undefined symbols for architecture arm64:
///   "_posix_fallocate", referenced from: ...
/// ```
///
/// That is what `Test (macos-latest)` had been failing on. See
/// `TODO/cli-surface.md`, T-145.
#[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "openbsd")))]
fn fallocate(file: &File, length: u64) -> Result<(), String> {
    use std::os::unix::io::AsRawFd;

    unsafe extern "C" {
        fn posix_fallocate(fd: i32, offset: i64, len: i64) -> i32;
    }

    if length == 0 {
        return Ok(());
    }
    // SAFETY: the descriptor comes from an open `File` this call does not
    // outlive, and the offset and length are non-negative.
    let code = unsafe { posix_fallocate(file.as_raw_fd(), 0, length as i64) };
    match code {
        0 => Ok(()),
        // `posix_fallocate` returns the error rather than setting `errno`.
        code => Err(format!(
            "posix_fallocate: {}",
            std::io::Error::from_raw_os_error(code)
        )),
    }
}

/// `F_PREALLOCATE` is the Apple equivalent, and it is a different shape.
///
/// Three differences from `posix_fallocate`, and each one is a line of code
/// here. It reserves blocks without moving the end of the file, so the length
/// is set afterwards. It measures from the current end of the file rather than
/// from an absolute offset, so what is asked for is the shortfall rather than
/// the total. And it takes a contiguous run first, which is what the
/// filesystem prefers and is allowed to refuse, so the request is repeated
/// without that constraint before it counts as a failure.
///
/// A refusal is not an error here: the caller falls back to `prealloc` and
/// reports the reason, the same as on Windows.
#[cfg(target_vendor = "apple")]
fn fallocate(file: &File, length: u64) -> Result<(), String> {
    use std::os::unix::io::AsRawFd;

    /// `fcntl(2)`, `F_PREALLOCATE`.
    const F_PREALLOCATE: i32 = 42;
    /// Allocate as one contiguous run. The filesystem may refuse.
    const F_ALLOCATECONTIG: u32 = 0x0000_0002;
    /// Allocate all of it, contiguous or not.
    const F_ALLOCATEALL: u32 = 0x0000_0004;
    /// `fst_length` is measured from the current end of the file.
    const F_PEOFPOSMODE: i32 = 3;

    /// `fstore_t` from `<sys/fcntl.h>`. `off_t` is 64 bits on every Apple
    /// target Rust supports.
    ///
    /// `offset` and `bytesalloc` are set here and read by the kernel rather
    /// than by this code. They stay because the layout is the interface.
    #[repr(C)]
    struct Fstore {
        flags: u32,
        posmode: i32,
        offset: i64,
        length: i64,
        bytesalloc: i64,
    }

    unsafe extern "C" {
        // Declared variadic because it is: on `aarch64-apple-darwin` a
        // variadic argument is passed on the stack rather than in a register,
        // so calling this through a fixed-arity declaration passes garbage.
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }

    if length == 0 {
        return Ok(());
    }
    let current = file
        .metadata()
        .map_err(|e| format!("cannot read the file length: {e}"))?
        .len();
    // `posix_fallocate` grows a file that is shorter than the region and never
    // shrinks one that is longer. Match that, so `falloc` means the same thing
    // on both.
    if current >= length {
        return Ok(());
    }
    let shortfall = (length - current) as i64;

    let mut store = Fstore {
        flags: F_ALLOCATECONTIG,
        posmode: F_PEOFPOSMODE,
        offset: 0,
        length: shortfall,
        bytesalloc: 0,
    };
    // SAFETY: the descriptor comes from an open `File` this call does not
    // outlive, and `store` is a live, correctly shaped `fstore_t`.
    let mut code = unsafe { fcntl(file.as_raw_fd(), F_PREALLOCATE, &raw mut store) };
    if code == -1 {
        store.flags = F_ALLOCATEALL;
        store.bytesalloc = 0;
        // SAFETY: as above.
        code = unsafe { fcntl(file.as_raw_fd(), F_PREALLOCATE, &raw mut store) };
    }
    if code == -1 {
        return Err(format!(
            "F_PREALLOCATE: {}",
            std::io::Error::last_os_error()
        ));
    }
    // The blocks are reserved but the file is still its old length.
    file.set_len(length)
        .map_err(|e| format!("F_PREALLOCATE reserved the space and set_len failed: {e}"))
}

/// OpenBSD has no interface that reserves blocks without writing them, so
/// `falloc` degrades to `prealloc` there the same way it does on Windows.
#[cfg(all(unix, target_os = "openbsd"))]
fn fallocate(_file: &File, _length: u64) -> Result<(), String> {
    Err("OpenBSD has no fallocate interface".to_string())
}

/// `SetFileValidData` is the Windows equivalent and needs
/// `SeManageVolumePrivilege`, which an ordinary process does not hold. It also
/// exposes whatever was previously on those disk blocks until they are
/// written, which is why the privilege exists. Rather than ask for it, this
/// reports why it cannot and the caller falls back to `prealloc`.
#[cfg(windows)]
fn fallocate(_file: &File, _length: u64) -> Result<(), String> {
    Err(
        "SetFileValidData needs SeManageVolumePrivilege, which this process does not hold"
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(name);
        (dir, path)
    }

    fn create(path: &std::path::Path) -> File {
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(path)
            .expect("create")
    }

    #[test]
    fn every_strategy_sets_the_length() {
        for strategy in Allocation::ALL {
            let (_dir, path) = temp("payload.bin");
            let file = create(&path);
            let outcome = reserve(&file, 4096, strategy)
                .unwrap_or_else(|e| panic!("{}: {e}", strategy.as_str()));
            drop(file);
            assert_eq!(
                std::fs::metadata(&path).unwrap().len(),
                4096,
                "{} did not set the length",
                strategy.as_str()
            );
            assert_eq!(outcome.requested, strategy);
        }
    }

    #[test]
    fn a_zero_length_file_is_reserved_without_writing_anything() {
        for strategy in Allocation::ALL {
            let (_dir, path) = temp("empty.bin");
            let file = create(&path);
            reserve(&file, 0, strategy).unwrap_or_else(|e| panic!("{}: {e}", strategy.as_str()));
            drop(file);
            assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        }
    }

    #[test]
    fn prealloc_writes_zeroes_that_read_back_as_zeroes() {
        let (_dir, path) = temp("zeroed.bin");
        let file = create(&path);
        let outcome = reserve(&file, 3 * 1024 * 1024 + 7, Allocation::Prealloc).unwrap();
        assert!(outcome.as_asked());
        drop(file);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 3 * 1024 * 1024 + 7);
        assert!(bytes.iter().all(|b| *b == 0));
    }

    #[test]
    fn preallocating_over_existing_data_replaces_it_with_zeroes() {
        let (_dir, path) = temp("dirty.bin");
        std::fs::write(&path, vec![0xAB; 8192]).unwrap();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        reserve(&file, 8192, Allocation::Prealloc).unwrap();
        drop(file);
        assert!(std::fs::read(&path).unwrap().iter().all(|b| *b == 0));
    }

    #[test]
    fn falloc_either_works_or_says_why_it_fell_back() {
        let (_dir, path) = temp("falloc.bin");
        let file = create(&path);
        let outcome = reserve(&file, 65536, Allocation::Falloc).unwrap();
        assert_eq!(outcome.requested, Allocation::Falloc);
        match outcome.as_asked() {
            true => assert!(outcome.note.is_none()),
            false => {
                assert_eq!(outcome.used, Allocation::Prealloc);
                let note = outcome.note.as_deref().unwrap_or_default();
                assert!(
                    !note.is_empty(),
                    "a fallback with no reason is not a report"
                );
            }
        }
        drop(file);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 65536);
    }

    #[test]
    fn sparse_reserves_the_length_without_writing_the_bytes() {
        let (_dir, path) = temp("sparse.bin");
        let file = create(&path);
        // A gibibyte. If this wrote the bytes the test would take long enough
        // to notice, which is itself the assertion.
        let began = std::time::Instant::now();
        let outcome = reserve(&file, 1024 * 1024 * 1024, Allocation::Sparse).unwrap();
        let elapsed = began.elapsed();
        drop(file);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 1024 * 1024 * 1024);
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "reserving a sparse gibibyte took {elapsed:?}, which means it wrote it"
        );
        assert_eq!(outcome.requested, Allocation::Sparse);
    }

    #[test]
    fn every_name_round_trips_and_an_unknown_one_lists_the_others() {
        for strategy in Allocation::ALL {
            assert_eq!(Allocation::parse(strategy.as_str()), Ok(strategy));
        }
        let error = Allocation::parse("magic").unwrap_err();
        for strategy in Allocation::ALL {
            assert!(error.contains(strategy.as_str()), "{error}");
        }
    }

    #[test]
    fn the_default_is_sparse() {
        assert_eq!(Allocation::default(), Allocation::Sparse);
    }
}
