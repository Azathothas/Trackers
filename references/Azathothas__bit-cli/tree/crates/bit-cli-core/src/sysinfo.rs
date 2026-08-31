//! What machine this ran on, and what this process cost.
//!
//! A benchmark without its environment recorded is not a result: two
//! throughput numbers taken on different CPUs, or before and after a kernel
//! update, are not comparable, and nothing in the number says so. So every
//! `bench` report carries a [`Host`] describing the machine and a [`Process`]
//! describing what the run cost in memory, CPU, and handles.
//!
//! Both are read through the platform's own interfaces rather than a crate:
//! `GetProcessMemoryInfo`, `GetProcessTimes`, `GetProcessHandleCount`,
//! `GlobalMemoryStatusEx`, `RtlGetVersion`, and `GetIfTable` on Windows;
//! `/proc/self/status`, `/proc/self/stat`, `/proc/self/fd`, `/proc/meminfo`,
//! `/proc/sys/kernel`, `/etc/os-release`, and `/sys/class/net` on Linux;
//! `getrusage`, `proc_pidinfo`, and `sysctlbyname` on the Apple platforms.
//!
//! Three platforms and three implementations, because `cfg(unix)` is a family
//! rather than a platform. Reading `/proc` under `cfg(unix)` compiles on macOS
//! and finds nothing there, and what came out was not an absent number but a
//! wrong one: a Mac reported as a Linux box with no memory. See
//! `TODO/cli-surface.md`, T-145.
//!
//! Nothing here fails a run. A field that cannot be read is `None`, with the
//! reason recorded in `unavailable`, because a missing NIC speed is not a
//! reason to lose a measurement.

use serde::{Deserialize, Serialize};

use crate::units::{Size, format_size};

/// What this process has cost so far.
///
/// Sampled at the end of a run, and again at intervals when a time series
/// wants it. Every field is cumulative for the life of the process, so a
/// second sample is never smaller than the first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Process {
    /// The high-water mark of resident memory, in bytes. `PeakWorkingSetSize`
    /// on Windows, `VmHWM` on Linux.
    pub peak_rss_bytes: u64,
    /// Resident memory right now, in bytes.
    pub rss_bytes: u64,
    /// Total processor time used, user plus system, in milliseconds. This is
    /// summed across every thread, so it exceeds wall time on a busy run.
    pub cpu_ms: u64,
    /// User-mode processor time, in milliseconds.
    pub cpu_user_ms: u64,
    /// Kernel-mode processor time, in milliseconds.
    pub cpu_system_ms: u64,
    /// Open handles on Windows, open file descriptors on Linux.
    pub open_handles: u64,
    /// Fields that could not be read, and why. Empty on a healthy sample.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unavailable: Vec<String>,
}

impl Process {
    /// Sample this process now.
    pub fn sample() -> Self {
        platform::process()
    }

    /// The human rendering, one line.
    pub fn summary(&self) -> String {
        format!(
            "peak RSS {}, CPU {}ms, {} handles",
            format_size(self.peak_rss_bytes),
            self.cpu_ms,
            self.open_handles
        )
    }

    /// The larger of two samples, field by field.
    ///
    /// A time series samples repeatedly and the report wants the worst of
    /// them. Taking the maximum per field rather than the last sample means a
    /// spike halfway through a run is not lost when memory is released before
    /// the end.
    pub fn max(&self, other: &Self) -> Self {
        let mut unavailable = self.unavailable.clone();
        for reason in &other.unavailable {
            if !unavailable.contains(reason) {
                unavailable.push(reason.clone());
            }
        }
        Self {
            peak_rss_bytes: self.peak_rss_bytes.max(other.peak_rss_bytes),
            rss_bytes: self.rss_bytes.max(other.rss_bytes),
            cpu_ms: self.cpu_ms.max(other.cpu_ms),
            cpu_user_ms: self.cpu_user_ms.max(other.cpu_user_ms),
            cpu_system_ms: self.cpu_system_ms.max(other.cpu_system_ms),
            open_handles: self.open_handles.max(other.open_handles),
            unavailable,
        }
    }

    /// This sample less an earlier one, for the cost of one window rather than
    /// of the whole process. Peaks and handle counts are levels rather than
    /// counters, so they are carried across unchanged.
    pub fn since(&self, earlier: &Self) -> Self {
        Self {
            peak_rss_bytes: self.peak_rss_bytes,
            rss_bytes: self.rss_bytes,
            cpu_ms: self.cpu_ms.saturating_sub(earlier.cpu_ms),
            cpu_user_ms: self.cpu_user_ms.saturating_sub(earlier.cpu_user_ms),
            cpu_system_ms: self.cpu_system_ms.saturating_sub(earlier.cpu_system_ms),
            open_handles: self.open_handles,
            unavailable: self.unavailable.clone(),
        }
    }
}

/// The operating system this ran on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Os {
    /// `Windows` or `Linux`.
    pub name: String,
    /// The version as the kernel reports it: `10.0.26200` on Windows,
    /// `6.8.0-45-generic` on Linux.
    pub version: String,
    /// The distribution name where there is one, from `/etc/os-release`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distribution: Option<String>,
}

/// The processor this ran on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cpu {
    /// The brand string, for example `AMD Ryzen 9 5950X 16-Core Processor`.
    pub model: String,
    /// The architecture this binary was compiled for.
    pub architecture: String,
    /// Logical processors visible to this process.
    pub logical_cores: usize,
}

/// One network interface and the speed it negotiated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nic {
    /// The interface name: `eth0`, or the adapter description on Windows.
    pub name: String,
    /// Negotiated link speed in bits per second, when the driver reports one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_speed_bps: Option<u64>,
    /// The same speed for a person to read, for example `1.00 Gbit/s`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_speed_human: Option<String>,
    /// Whether the interface is up and carrying traffic.
    pub up: bool,
}

/// The machine a measurement was taken on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    pub os: Os,
    pub cpu: Cpu,
    /// Total physical memory.
    pub memory_total: Size,
    /// Every interface that is up and reports a link speed, fastest first.
    ///
    /// Loopback is left out: it is not a link and its speed is meaningless. So
    /// is anything whose driver does not report a speed, because an interface
    /// with no number attached tells a reader nothing they can compare.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub network: Vec<Nic>,
    /// Fields that could not be read, and why.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unavailable: Vec<String>,
}

impl Host {
    /// Describe this machine.
    pub fn capture() -> Self {
        platform::host()
    }

    /// Whether two hosts are similar enough that comparing measurements taken
    /// on them means anything.
    ///
    /// CPU model, core count, and OS name have to agree. Kernel patch level
    /// and memory do not: an OS update is worth comparing across, and a
    /// different machine is not.
    pub fn comparable_to(&self, other: &Self) -> bool {
        self.cpu.model == other.cpu.model
            && self.cpu.logical_cores == other.cpu.logical_cores
            && self.os.name == other.os.name
    }

    /// What differs between two hosts, for the message that refuses a
    /// comparison.
    pub fn differences(&self, other: &Self) -> Vec<String> {
        let mut out = Vec::new();
        if self.cpu.model != other.cpu.model {
            out.push(format!(
                "CPU model {} then {}",
                other.cpu.model, self.cpu.model
            ));
        }
        if self.cpu.logical_cores != other.cpu.logical_cores {
            out.push(format!(
                "logical cores {} then {}",
                other.cpu.logical_cores, self.cpu.logical_cores
            ));
        }
        if self.os.name != other.os.name {
            out.push(format!("OS {} then {}", other.os.name, self.os.name));
        }
        out
    }

    /// The fastest link speed reported by any interface that is up.
    pub fn link_speed_bps(&self) -> Option<u64> {
        self.network.iter().filter_map(|n| n.link_speed_bps).max()
    }
}

/// Format a link speed in bits per second the way network hardware is sold,
/// in decimal multiples, because a `1 Gbit/s` NIC is 1,000,000,000 bit/s and
/// calling it `953.67 Mibit/s` helps nobody.
pub fn format_link_speed(bps: u64) -> String {
    const K: u64 = 1_000;
    const M: u64 = 1_000_000;
    const G: u64 = 1_000_000_000;
    match bps {
        b if b >= G => format!("{:.2} Gbit/s", b as f64 / G as f64),
        b if b >= M => format!("{:.2} Mbit/s", b as f64 / M as f64),
        b if b >= K => format!("{:.2} Kbit/s", b as f64 / K as f64),
        b => format!("{b} bit/s"),
    }
}

/// Logical processors visible to this process, never zero.
fn logical_cores() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// The processor brand string from `CPUID` leaves `0x80000002` through
/// `0x80000004`.
///
/// This is the same string every other tool prints, it needs no filesystem and
/// no registry, and it works the same on both platforms. Only x86 has it; the
/// other architectures fall through to the platform reader.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn cpu_brand() -> Option<String> {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::{__cpuid, __get_cpuid_max};
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::{__cpuid, __get_cpuid_max};

    // The extended leaves are only read after the maximum leaf says they
    // exist. A processor that does not carry a brand string falls through to
    // the platform reader.
    //
    // The `unsafe` block is required at the declared MSRV and redundant on a
    // current toolchain: `__cpuid` and `__get_cpuid_max` were `unsafe fn` when
    // 1.88 shipped and are safe now, and `-D warnings` would fail on the
    // `unused_unsafe` the newer compiler reports. Writing both and allowing
    // the lint is what compiles under either, which the `MSRV` job is there to
    // catch. Drop the block and the allowance together when the MSRV passes
    // the release that made them safe.
    #[allow(unused_unsafe)]
    // SAFETY: `CPUID` needs no preconditions beyond running on x86, which the
    // `cfg` above guarantees, and the extended leaves are read only after the
    // maximum-leaf query says they exist.
    let bytes = unsafe {
        if __get_cpuid_max(0x8000_0000).0 < 0x8000_0004 {
            return None;
        }
        let mut bytes = Vec::with_capacity(48);
        for leaf in 0x8000_0002u32..=0x8000_0004 {
            let result = __cpuid(leaf);
            for word in [result.eax, result.ebx, result.ecx, result.edx] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        bytes
    };
    let text = String::from_utf8_lossy(&bytes)
        .trim_end_matches('\0')
        .trim()
        .to_string();
    match text.is_empty() {
        true => None,
        false => Some(text),
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn cpu_brand() -> Option<String> {
    None
}

#[cfg(windows)]
mod platform {
    use super::{Cpu, Host, Nic, Os, Process, cpu_brand, format_link_speed, logical_cores};
    use crate::units::Size;

    type Dword = u32;
    type Handle = isize;
    type Bool32 = i32;

    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCounters {
        cb: Dword,
        page_fault_count: Dword,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct FileTime {
        low: Dword,
        high: Dword,
    }

    impl FileTime {
        /// `FILETIME` counts 100 nanosecond intervals.
        fn millis(self) -> u64 {
            ((u64::from(self.high) << 32) | u64::from(self.low)) / 10_000
        }
    }

    #[repr(C)]
    struct MemoryStatusEx {
        length: Dword,
        memory_load: Dword,
        total_physical: u64,
        available_physical: u64,
        total_page_file: u64,
        available_page_file: u64,
        total_virtual: u64,
        available_virtual: u64,
        available_extended_virtual: u64,
    }

    #[repr(C)]
    struct OsVersionInfoW {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform_id: u32,
        csd_version: [u16; 128],
    }

    /// `MIB_IFROW` from `iphlpapi.h`.
    ///
    /// Every member is a `DWORD` or a fixed byte array, so the C layout is
    /// unambiguous and `repr(C)` reproduces it exactly. The newer
    /// `MIB_IF_ROW2` carries a 64 bit link speed but also enums and a bitfield
    /// whose padding is not worth guessing at, and a `DWORD` of bits per
    /// second describes every link short of 4.29 Gbit/s.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MibIfRow {
        name: [u16; 256],
        index: Dword,
        kind: Dword,
        mtu: Dword,
        speed: Dword,
        phys_addr_len: Dword,
        phys_addr: [u8; 8],
        admin_status: Dword,
        oper_status: Dword,
        last_change: Dword,
        in_octets: Dword,
        in_ucast_pkts: Dword,
        in_nucast_pkts: Dword,
        in_discards: Dword,
        in_errors: Dword,
        in_unknown_protos: Dword,
        out_octets: Dword,
        out_ucast_pkts: Dword,
        out_nucast_pkts: Dword,
        out_discards: Dword,
        out_errors: Dword,
        out_qlen: Dword,
        descr_len: Dword,
        descr: [u8; 256],
    }

    /// `IF_TYPE_SOFTWARE_LOOPBACK`.
    const IF_TYPE_LOOPBACK: Dword = 24;
    /// `MIB_IF_OPER_STATUS_OPERATIONAL`.
    const IF_OPER_STATUS_OPERATIONAL: Dword = 5;
    const ERROR_INSUFFICIENT_BUFFER: Dword = 122;
    const NO_ERROR: Dword = 0;

    unsafe extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn K32GetProcessMemoryInfo(
            process: Handle,
            counters: *mut ProcessMemoryCounters,
            cb: Dword,
        ) -> Bool32;
        fn GetProcessTimes(
            process: Handle,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> Bool32;
        fn GetProcessHandleCount(process: Handle, count: *mut Dword) -> Bool32;
        fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> Bool32;
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
    }

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn GetIfTable(table: *mut u8, size: *mut u32, order: Bool32) -> Dword;
    }

    pub(super) fn process() -> Process {
        let mut out = Process::default();
        // SAFETY: `GetCurrentProcess` returns a pseudo-handle that is always
        // valid for the calling process and needs no close. Each call below
        // writes into a stack value of exactly the size it is told, and the
        // value is only read when the call reports success.
        unsafe {
            let handle = GetCurrentProcess();

            let mut counters = ProcessMemoryCounters {
                cb: size_of::<ProcessMemoryCounters>() as Dword,
                ..Default::default()
            };
            match K32GetProcessMemoryInfo(handle, &mut counters, counters.cb) {
                0 => out.unavailable.push("peak_rss_bytes".into()),
                _ => {
                    out.peak_rss_bytes = counters.peak_working_set_size as u64;
                    out.rss_bytes = counters.working_set_size as u64;
                }
            }

            let (mut creation, mut exit, mut kernel, mut user) = Default::default();
            match GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) {
                0 => out.unavailable.push("cpu_ms".into()),
                _ => {
                    out.cpu_user_ms = user.millis();
                    out.cpu_system_ms = kernel.millis();
                    out.cpu_ms = out.cpu_user_ms + out.cpu_system_ms;
                }
            }

            let mut handles: Dword = 0;
            match GetProcessHandleCount(handle, &mut handles) {
                0 => out.unavailable.push("open_handles".into()),
                _ => out.open_handles = u64::from(handles),
            }
        }
        out
    }

    pub(super) fn host() -> Host {
        let mut unavailable = Vec::new();
        Host {
            os: os(&mut unavailable),
            cpu: Cpu {
                model: cpu_brand()
                    .or_else(|| std::env::var("PROCESSOR_IDENTIFIER").ok())
                    .unwrap_or_else(|| "unknown".into()),
                architecture: std::env::consts::ARCH.to_string(),
                logical_cores: logical_cores(),
            },
            memory_total: Size(memory_total(&mut unavailable)),
            network: interfaces(&mut unavailable),
            unavailable,
        }
    }

    fn os(unavailable: &mut Vec<String>) -> Os {
        let mut info = OsVersionInfoW {
            size: size_of::<OsVersionInfoW>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform_id: 0,
            csd_version: [0; 128],
        };
        // SAFETY: `RtlGetVersion` fills a caller-owned structure whose size it
        // is told through the first field. It is the documented way to read
        // the real version; `GetVersionEx` reports 6.2 unless the binary
        // carries a compatibility manifest.
        let status = unsafe { RtlGetVersion(&mut info) };
        if status != 0 {
            unavailable.push("os.version".into());
            return Os {
                name: "Windows".into(),
                version: "unknown".into(),
                distribution: None,
            };
        }
        Os {
            name: "Windows".into(),
            version: format!("{}.{}.{}", info.major, info.minor, info.build),
            distribution: None,
        }
    }

    fn memory_total(unavailable: &mut Vec<String>) -> u64 {
        let mut status = MemoryStatusEx {
            length: size_of::<MemoryStatusEx>() as Dword,
            memory_load: 0,
            total_physical: 0,
            available_physical: 0,
            total_page_file: 0,
            available_page_file: 0,
            total_virtual: 0,
            available_virtual: 0,
            available_extended_virtual: 0,
        };
        // SAFETY: the structure is stack-owned and its `length` field is set
        // to its own size, which is how the call knows what it may write.
        match unsafe { GlobalMemoryStatusEx(&mut status) } {
            0 => {
                unavailable.push("memory_total".into());
                0
            }
            _ => status.total_physical,
        }
    }

    fn interfaces(unavailable: &mut Vec<String>) -> Vec<Nic> {
        let mut size: u32 = 0;
        // SAFETY: a null table pointer with a zero size is the documented way
        // to ask `GetIfTable` how large a buffer it needs; it writes only
        // through `size` in that case.
        let probe = unsafe { GetIfTable(std::ptr::null_mut(), &mut size, 0) };
        if probe != ERROR_INSUFFICIENT_BUFFER || size == 0 {
            unavailable.push("network".into());
            return Vec::new();
        }
        // `MIB_IFTABLE` is a `DWORD` count followed by the rows. The buffer is
        // over-aligned to the row alignment so the rows can be read directly
        // out of it.
        let words = (size as usize).div_ceil(size_of::<u32>()) + 1;
        let mut buffer = vec![0u32; words];
        let bytes = buffer.as_mut_ptr().cast::<u8>();
        // SAFETY: the buffer is at least `size` bytes and aligned for `u32`,
        // which is the alignment `MIB_IFTABLE` needs.
        let status = unsafe { GetIfTable(bytes, &mut size, 1) };
        if status != NO_ERROR {
            unavailable.push("network".into());
            return Vec::new();
        }
        let count = buffer[0] as usize;
        let mut out = Vec::new();
        for index in 0..count {
            // SAFETY: `GetIfTable` reported success, so the buffer holds
            // `count` rows laid out after the leading count. The offset is
            // computed from the same C layout the structure declares, and the
            // row is copied out before anything reads it.
            let row: MibIfRow = unsafe {
                let base = bytes.add(row_offset(index));
                std::ptr::read_unaligned(base.cast::<MibIfRow>())
            };
            if row.kind == IF_TYPE_LOOPBACK || row.oper_status != IF_OPER_STATUS_OPERATIONAL {
                continue;
            }
            // A `dwSpeed` of `0xFFFFFFFF` is the saturation value of the
            // field, not a 4.29 Gbit/s link. Every NDIS filter layer and
            // virtual adapter on a Windows box reports it, and repeating it as
            // a speed would put a made-up number in a benchmark report.
            let speed = match row.speed {
                0 | Dword::MAX => None,
                speed => Some(u64::from(speed)),
            };
            let Some(speed) = speed else { continue };
            let mac =
                row.phys_addr[..(row.phys_addr_len as usize).min(row.phys_addr.len())].to_vec();
            out.push((
                mac,
                Nic {
                    name: describe(&row),
                    link_speed_bps: Some(speed),
                    link_speed_human: Some(format_link_speed(speed)),
                    up: true,
                },
            ));
        }

        // One row per link. `GetIfTable` returns every NDIS binding, so a
        // single ethernet port comes back once as itself and again for each
        // filter driver layered over it, all with the same physical address
        // and the same speed. Two interfaces with one MAC are one link seen
        // twice, and a report that lists it five times is harder to read for
        // no gain. The name kept is the shortest, because a filter layer is
        // named for its parent with a suffix appended.
        //
        // A Hyper-V external switch also shares its MAC with the physical port
        // it binds, so those two collapse into one row as well. Both describe
        // the same wire at the same speed, which is what the field is for.
        out.sort_by(|(a_mac, a), (b_mac, b)| {
            a_mac
                .cmp(b_mac)
                .then_with(|| a.name.len().cmp(&b.name.len()))
                .then_with(|| a.name.cmp(&b.name))
        });
        out.dedup_by(|(a_mac, _), (b_mac, _)| !a_mac.is_empty() && a_mac == b_mac);

        let mut out: Vec<Nic> = out.into_iter().map(|(_, nic)| nic).collect();
        out.sort_by_key(|nic| std::cmp::Reverse(nic.link_speed_bps));
        out
    }

    /// Byte offset of row `index` inside a `MIB_IFTABLE` buffer.
    ///
    /// The table is `DWORD dwNumEntries` followed by the rows, and the rows
    /// are aligned to the structure's own alignment rather than packed after
    /// the count.
    const fn row_offset(index: usize) -> usize {
        let header = align_of::<MibIfRow>().next_multiple_of(size_of::<u32>());
        header + index * size_of::<MibIfRow>()
    }

    /// The adapter description, which is what a person recognises. It is a
    /// non-Unicode string of `descr_len` bytes.
    fn describe(row: &MibIfRow) -> String {
        let len = (row.descr_len as usize).min(row.descr.len());
        let text = String::from_utf8_lossy(&row.descr[..len])
            .trim_end_matches('\0')
            .trim()
            .to_string();
        if !text.is_empty() {
            return text;
        }
        let name: Vec<u16> = row.name.iter().copied().take_while(|c| *c != 0).collect();
        match String::from_utf16_lossy(&name).trim().to_string() {
            empty if empty.is_empty() => format!("interface {}", row.index),
            named => named,
        }
    }
}

/// `/proc` is a Linux interface, not a unix one.
///
/// This module was written `#[cfg(unix)]`, so on macOS every read here missed
/// and the report said the host was Linux with an unknown kernel, no memory,
/// and a process using no CPU. Wrong numbers are worse than absent ones: the
/// `unavailable` list existed and was populated, and the `os.name` beside it
/// still said `Linux`. See `TODO/cli-surface.md`, T-145.
#[cfg(all(unix, not(target_vendor = "apple")))]
mod platform {
    use super::{Cpu, Host, Nic, Os, Process, cpu_brand, format_link_speed, logical_cores};
    use crate::units::Size;

    pub(super) fn process() -> Process {
        let mut out = Process::default();
        match std::fs::read_to_string("/proc/self/status") {
            Ok(status) => {
                out.peak_rss_bytes = kb_field(&status, "VmHWM:").unwrap_or(0) * 1024;
                out.rss_bytes = kb_field(&status, "VmRSS:").unwrap_or(0) * 1024;
                if out.peak_rss_bytes == 0 {
                    out.unavailable.push("peak_rss_bytes".into());
                }
            }
            Err(_) => out.unavailable.push("peak_rss_bytes".into()),
        }

        match cpu_times() {
            Some((user_ms, system_ms)) => {
                out.cpu_user_ms = user_ms;
                out.cpu_system_ms = system_ms;
                out.cpu_ms = user_ms + system_ms;
            }
            None => out.unavailable.push("cpu_ms".into()),
        }

        match std::fs::read_dir("/proc/self/fd") {
            // The directory handle opened to read the count is itself one of
            // the entries, so it is taken back out.
            Ok(entries) => out.open_handles = (entries.count() as u64).saturating_sub(1),
            Err(_) => out.unavailable.push("open_handles".into()),
        }
        out
    }

    /// User and system time for the whole process, in milliseconds.
    ///
    /// `/proc/self/stat` reports per-thread fields in `utime` and `stime` and
    /// the children's in `cutime` and `cstime`; the whole-process figures are
    /// what a benchmark wants, so this reads the process-wide file rather than
    /// a thread's.
    fn cpu_times() -> Option<(u64, u64)> {
        let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
        // `comm` is parenthesised and may itself contain spaces, so the fields
        // are counted from after the closing parenthesis rather than from the
        // start of the line.
        let rest = stat.rsplit_once(')')?.1;
        let fields: Vec<&str> = rest.split_whitespace().collect();
        // After `comm` the fields are state, ppid, pgrp, session, tty_nr,
        // tpgid, flags, minflt, cminflt, majflt, cmajflt, utime, stime.
        let utime: u64 = fields.get(11)?.parse().ok()?;
        let stime: u64 = fields.get(12)?.parse().ok()?;
        let ticks = clock_ticks();
        Some((utime * 1000 / ticks, stime * 1000 / ticks))
    }

    /// Clock ticks per second, which is what `/proc/self/stat` counts in.
    ///
    /// `sysconf(_SC_CLK_TCK)` is the authority. It has been 100 on every Linux
    /// this runs on since the kernel started reporting `USER_HZ` independently
    /// of the timer frequency, and that is the fallback when the call is not
    /// available.
    fn clock_ticks() -> u64 {
        unsafe extern "C" {
            fn sysconf(name: i32) -> i64;
        }
        /// `_SC_CLK_TCK`.
        const SC_CLK_TCK: i32 = 2;
        // SAFETY: `sysconf` reads a static system property and touches no
        // caller memory.
        match unsafe { sysconf(SC_CLK_TCK) } {
            ticks if ticks > 0 => ticks as u64,
            _ => 100,
        }
    }

    /// Read a `Key: 1234 kB` line out of `/proc/self/status`.
    fn kb_field(status: &str, key: &str) -> Option<u64> {
        status
            .lines()
            .find_map(|line| line.strip_prefix(key))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    }

    pub(super) fn host() -> Host {
        let mut unavailable = Vec::new();
        Host {
            os: os(&mut unavailable),
            cpu: Cpu {
                model: cpu_brand()
                    .or_else(cpuinfo_model)
                    .unwrap_or_else(|| "unknown".into()),
                architecture: std::env::consts::ARCH.to_string(),
                logical_cores: logical_cores(),
            },
            memory_total: Size(memory_total(&mut unavailable)),
            network: interfaces(&mut unavailable),
            unavailable,
        }
    }

    fn os(unavailable: &mut Vec<String>) -> Os {
        let version = read_trimmed("/proc/sys/kernel/osrelease").unwrap_or_else(|| {
            unavailable.push("os.version".into());
            "unknown".into()
        });
        Os {
            name: read_trimmed("/proc/sys/kernel/ostype").unwrap_or_else(|| "Linux".into()),
            version,
            distribution: distribution(),
        }
    }

    /// `PRETTY_NAME` from `/etc/os-release`, which is the distribution's own
    /// name for itself.
    fn distribution() -> Option<String> {
        let text = std::fs::read_to_string("/etc/os-release").ok()?;
        let value = text
            .lines()
            .find_map(|line| line.strip_prefix("PRETTY_NAME="))?;
        Some(value.trim_matches('"').to_string())
    }

    fn cpuinfo_model() -> Option<String> {
        let text = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        for key in ["model name", "Model", "Hardware", "cpu model"] {
            if let Some(value) = text
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.trim() == key)
                .map(|(_, value)| value.trim().to_string())
                && !value.is_empty()
            {
                return Some(value);
            }
        }
        None
    }

    fn memory_total(unavailable: &mut Vec<String>) -> u64 {
        let Ok(text) = std::fs::read_to_string("/proc/meminfo") else {
            unavailable.push("memory_total".into());
            return 0;
        };
        match kb_field(&text, "MemTotal:") {
            Some(kb) => kb * 1024,
            None => {
                unavailable.push("memory_total".into());
                0
            }
        }
    }

    /// Every interface that is up, from `/sys/class/net`.
    ///
    /// `speed` is in megabits per second and is absent or `-1` for anything
    /// without a physical link, which includes every virtual and tunnel
    /// device. That is reported as no speed rather than as zero.
    fn interfaces(unavailable: &mut Vec<String>) -> Vec<Nic> {
        let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
            unavailable.push("network".into());
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "lo" {
                continue;
            }
            let path = entry.path();
            if read_trimmed(path.join("operstate")).as_deref() != Some("up") {
                continue;
            }
            let mbit = read_trimmed(path.join("speed"))
                .and_then(|text| text.parse::<i64>().ok())
                .filter(|mbit| *mbit > 0)
                .map(|mbit| mbit as u64 * 1_000_000);
            // An interface with no reported speed is left out, the same as on
            // Windows: a row with no number attached tells a reader nothing
            // they can compare.
            let Some(speed) = mbit else { continue };
            out.push(Nic {
                name,
                link_speed_bps: Some(speed),
                link_speed_human: Some(format_link_speed(speed)),
                up: true,
            });
        }
        out.sort_by_key(|nic| std::cmp::Reverse(nic.link_speed_bps));
        out
    }

    fn read_trimmed(path: impl AsRef<std::path::Path>) -> Option<String> {
        std::fs::read_to_string(path)
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    }
}

/// The Apple platforms, which have neither `/proc` nor the Windows API.
///
/// Everything here comes from libSystem, so there is no new dependency and no
/// extra link flag: `getrusage(2)` for processor time and the resident
/// high-water mark, `proc_pidinfo(3)` for resident size now and the open
/// descriptor count, and `sysctlbyname(3)` for the machine.
///
/// Written because `Test (macos-latest)` was reporting a Mac as a Linux box
/// with no memory and a process that had used no CPU. See
/// `TODO/cli-surface.md`, T-145.
#[cfg(target_vendor = "apple")]
mod platform {
    use super::{Cpu, Host, Os, Process, cpu_brand, logical_cores};
    use crate::units::Size;

    /// `RUSAGE_SELF`: this process, summed over its threads.
    const RUSAGE_SELF: i32 = 0;
    /// `PROC_PIDLISTFDS`: the open descriptor table.
    const PROC_PIDLISTFDS: i32 = 1;
    /// `PROC_PIDTASKINFO`: one `proc_taskinfo` for the whole task.
    const PROC_PIDTASKINFO: i32 = 4;
    /// `sizeof(struct proc_fdinfo)`, which is two 32 bit fields.
    const PROC_FDINFO_SIZE: i32 = 8;

    /// `struct timeval`. `tv_sec` is a 64 bit `time_t` and `tv_usec` a 32 bit
    /// `suseconds_t`, so the struct is padded out to 16 bytes.
    #[repr(C)]
    #[derive(Default)]
    struct Timeval {
        sec: i64,
        usec: i32,
        _pad: i32,
    }

    impl Timeval {
        const fn millis(&self) -> u64 {
            (self.sec as u64) * 1000 + (self.usec as u64) / 1000
        }
    }

    /// `struct rusage` from `<sys/resource.h>`. Only the first three fields
    /// are read; the rest are here so the struct is the size the kernel writes.
    #[repr(C)]
    #[derive(Default)]
    struct Rusage {
        utime: Timeval,
        stime: Timeval,
        /// The resident high-water mark. **In bytes on Darwin**, where Linux
        /// reports the same field in kilobytes.
        maxrss: i64,
        ixrss: i64,
        idrss: i64,
        isrss: i64,
        minflt: i64,
        majflt: i64,
        nswap: i64,
        inblock: i64,
        oublock: i64,
        msgsnd: i64,
        msgrcv: i64,
        nsignals: i64,
        nvcsw: i64,
        nivcsw: i64,
    }

    /// `struct proc_taskinfo` from `<sys/proc_info.h>`, 96 bytes.
    #[repr(C)]
    #[derive(Default)]
    struct ProcTaskInfo {
        virtual_size: u64,
        resident_size: u64,
        total_user: u64,
        total_system: u64,
        threads_user: u64,
        threads_system: u64,
        policy: i32,
        faults: i32,
        pageins: i32,
        cow_faults: i32,
        messages_sent: i32,
        messages_received: i32,
        syscalls_mach: i32,
        syscalls_unix: i32,
        csw: i32,
        threadnum: i32,
        numrunning: i32,
        priority: i32,
    }

    // The layouts above are transcribed from the system headers, and a
    // transcription that is one field out does not fail: it reads the wrong
    // offset and reports a plausible wrong number. These fail the build
    // instead. 16 is `timeval` padded to its 8 byte alignment, 144 is two of
    // those plus fourteen longs, and 96 is six 64 bit fields plus twelve 32
    // bit ones.
    const _: () = assert!(size_of::<Timeval>() == 16);
    const _: () = assert!(size_of::<Rusage>() == 144);
    const _: () = assert!(size_of::<ProcTaskInfo>() == 96);

    unsafe extern "C" {
        fn getrusage(who: i32, usage: *mut Rusage) -> i32;
        fn getpid() -> i32;
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut core::ffi::c_void,
            buffersize: i32,
        ) -> i32;
        fn sysctlbyname(
            name: *const core::ffi::c_char,
            oldp: *mut core::ffi::c_void,
            oldlenp: *mut usize,
            newp: *const core::ffi::c_void,
            newlen: usize,
        ) -> i32;
    }

    pub(super) fn process() -> Process {
        let mut out = Process::default();

        let mut usage = Rusage::default();
        // SAFETY: `usage` is a live, correctly shaped `struct rusage` and the
        // call writes nothing else.
        if unsafe { getrusage(RUSAGE_SELF, &raw mut usage) } == 0 {
            out.cpu_user_ms = usage.utime.millis();
            out.cpu_system_ms = usage.stime.millis();
            out.cpu_ms = out.cpu_user_ms + out.cpu_system_ms;
            out.peak_rss_bytes = usage.maxrss.max(0) as u64;
        }
        if out.peak_rss_bytes == 0 {
            out.unavailable.push("peak_rss_bytes".into());
        }

        match task_info() {
            Some(info) => out.rss_bytes = info.resident_size,
            None => out.unavailable.push("rss_bytes".into()),
        }

        // The two numbers come from two kernel subsystems that do not share an
        // accounting basis. `ru_maxrss` is the BSD layer's high-water mark;
        // `resident_size` is the current Mach task footprint, and it counts
        // pages the BSD accounting does not, so on Darwin the current reading
        // can exceed the recorded peak. Windows takes both from one
        // `PROCESS_MEMORY_COUNTERS` and Linux both from one
        // `/proc/self/status`, so neither can disagree with itself this way.
        //
        // A peak below a reading taken at the same instant is not a peak. The
        // clamp is what makes `peak_rss_bytes` mean the same thing on all
        // three platforms, which is what every report that carries it assumes.
        // Only when the peak was read at all: a failed `getrusage` stays
        // unavailable rather than being backfilled from another source.
        if !out.unavailable.iter().any(|name| name == "peak_rss_bytes") {
            out.peak_rss_bytes = out.peak_rss_bytes.max(out.rss_bytes);
        }

        match open_descriptors() {
            Some(count) => out.open_handles = count,
            None => out.unavailable.push("open_handles".into()),
        }
        out
    }

    fn task_info() -> Option<ProcTaskInfo> {
        let mut info = ProcTaskInfo::default();
        let size = size_of::<ProcTaskInfo>() as i32;
        // SAFETY: the buffer is a live `proc_taskinfo` and its true size is
        // passed, so the kernel cannot write past it.
        let written =
            unsafe { proc_pidinfo(getpid(), PROC_PIDTASKINFO, 0, (&raw mut info).cast(), size) };
        (written == size).then_some(info)
    }

    /// How many descriptors this process holds.
    ///
    /// `proc_pidinfo` with a null buffer answers with the number of bytes the
    /// table would take, which is the documented way to ask for the size. That
    /// is one `proc_fdinfo` per descriptor, so the count is the quotient.
    fn open_descriptors() -> Option<u64> {
        // SAFETY: a null buffer with a zero size asks for the length and
        // writes nothing.
        let bytes = unsafe { proc_pidinfo(getpid(), PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
        (bytes > 0).then(|| (bytes / PROC_FDINFO_SIZE) as u64)
    }

    /// A `sysctl` value that is a NUL-terminated string.
    fn sysctl_string(name: &str) -> Option<String> {
        let key = std::ffi::CString::new(name).ok()?;
        let mut len: usize = 0;
        // SAFETY: a null value pointer asks for the length and writes only
        // through `len`.
        if unsafe {
            sysctlbyname(
                key.as_ptr(),
                std::ptr::null_mut(),
                &raw mut len,
                std::ptr::null(),
                0,
            )
        } != 0
            || len == 0
        {
            return None;
        }
        let mut buffer = vec![0u8; len];
        // SAFETY: `buffer` holds exactly the `len` bytes the call above asked
        // for, and `len` is passed back unchanged.
        if unsafe {
            sysctlbyname(
                key.as_ptr(),
                buffer.as_mut_ptr().cast(),
                &raw mut len,
                std::ptr::null(),
                0,
            )
        } != 0
        {
            return None;
        }
        let text = String::from_utf8_lossy(&buffer)
            .trim_end_matches('\0')
            .trim()
            .to_string();
        (!text.is_empty()).then_some(text)
    }

    /// A `sysctl` value that is a 64 bit integer.
    fn sysctl_u64(name: &str) -> Option<u64> {
        let key = std::ffi::CString::new(name).ok()?;
        let mut value: u64 = 0;
        let mut len = size_of::<u64>();
        // SAFETY: the buffer is a live `u64` and its true size is passed.
        let ok = unsafe {
            sysctlbyname(
                key.as_ptr(),
                (&raw mut value).cast(),
                &raw mut len,
                std::ptr::null(),
                0,
            )
        };
        (ok == 0 && len == size_of::<u64>()).then_some(value)
    }

    pub(super) fn host() -> Host {
        let mut unavailable = Vec::new();
        let version = sysctl_string("kern.osrelease").unwrap_or_else(|| {
            unavailable.push("os.version".into());
            "unknown".into()
        });
        let memory_total = sysctl_u64("hw.memsize").unwrap_or_else(|| {
            unavailable.push("memory_total".into());
            0
        });
        // Link speeds would come from `getifaddrs` plus an `SIOCGIFMEDIA`
        // ioctl per interface, and nothing measured here compares them across
        // machines yet. Saying it is not read beats reporting an empty list as
        // though the machine had no interfaces.
        unavailable.push("network".into());

        Host {
            os: Os {
                name: sysctl_string("kern.ostype").unwrap_or_else(|| "Darwin".into()),
                version,
                // `kern.osproductversion` is the number people know the system
                // by, `26.1` rather than the Darwin kernel's own `25.x`.
                distribution: sysctl_string("kern.osproductversion")
                    .map(|product| format!("macOS {product}")),
            },
            cpu: Cpu {
                // `cpu_brand` reads CPUID and answers on Intel Macs. Apple
                // silicon has no CPUID, and `machdep.cpu.brand_string` is
                // where the name lives on both.
                model: cpu_brand()
                    .or_else(|| sysctl_string("machdep.cpu.brand_string"))
                    .unwrap_or_else(|| "unknown".into()),
                architecture: std::env::consts::ARCH.to_string(),
                logical_cores: logical_cores(),
            },
            memory_total: Size(memory_total),
            network: Vec::new(),
            unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_sample_reports_memory_cpu_and_handles() {
        let sample = Process::sample();
        assert!(
            sample.unavailable.is_empty(),
            "could not read {:?}",
            sample.unavailable
        );
        assert!(
            sample.peak_rss_bytes > 1024 * 1024,
            "peak RSS of {} B is not a running process",
            sample.peak_rss_bytes
        );
        // Holds on all three platforms, and on Darwin only because
        // `process()` clamps it: `ru_maxrss` and the Mach `resident_size` are
        // two subsystems' numbers and the current reading can exceed the
        // recorded peak. This assertion failed on `macos-latest` in CI run
        // 32478382564 before the clamp existed. See TODO/cli-surface.md T-182.
        assert!(
            sample.peak_rss_bytes >= sample.rss_bytes,
            "peak RSS {} B is below the current {} B",
            sample.peak_rss_bytes,
            sample.rss_bytes
        );
        assert!(
            sample.open_handles > 0,
            "a process with no open handles has no stdout"
        );
        assert_eq!(sample.cpu_ms, sample.cpu_user_ms + sample.cpu_system_ms);
    }

    #[test]
    fn cpu_time_only_goes_up() {
        let first = Process::sample();
        // Enough arithmetic to move a 10 ms clock on any machine this runs on.
        let mut sink = 0u64;
        for value in 0..12_000_000u64 {
            sink = sink.wrapping_add(value ^ sink.rotate_left(7));
        }
        assert_ne!(sink, u64::MAX, "the compiler kept the loop");
        let second = Process::sample();
        assert!(
            second.cpu_ms >= first.cpu_ms,
            "cpu time went backwards: {} then {}",
            first.cpu_ms,
            second.cpu_ms
        );
    }

    #[test]
    fn the_maximum_of_two_samples_keeps_the_larger_of_each_field() {
        let low = Process {
            peak_rss_bytes: 10,
            rss_bytes: 5,
            cpu_ms: 100,
            cpu_user_ms: 60,
            cpu_system_ms: 40,
            open_handles: 20,
            unavailable: vec!["a".into()],
        };
        let high = Process {
            peak_rss_bytes: 30,
            rss_bytes: 2,
            cpu_ms: 90,
            cpu_user_ms: 50,
            cpu_system_ms: 40,
            open_handles: 25,
            unavailable: vec!["b".into()],
        };
        let merged = low.max(&high);
        assert_eq!(merged.peak_rss_bytes, 30);
        assert_eq!(merged.rss_bytes, 5);
        assert_eq!(merged.cpu_ms, 100);
        assert_eq!(merged.open_handles, 25);
        assert_eq!(merged.unavailable, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn a_delta_subtracts_counters_and_carries_levels() {
        let first = Process {
            peak_rss_bytes: 10,
            rss_bytes: 8,
            cpu_ms: 100,
            cpu_user_ms: 60,
            cpu_system_ms: 40,
            open_handles: 20,
            unavailable: Vec::new(),
        };
        let second = Process {
            peak_rss_bytes: 40,
            rss_bytes: 30,
            cpu_ms: 250,
            cpu_user_ms: 150,
            cpu_system_ms: 100,
            open_handles: 22,
            unavailable: Vec::new(),
        };
        let delta = second.since(&first);
        assert_eq!(delta.cpu_ms, 150);
        assert_eq!(delta.cpu_user_ms, 90);
        assert_eq!(delta.cpu_system_ms, 60);
        assert_eq!(delta.peak_rss_bytes, 40, "a peak is a level, not a counter");
        assert_eq!(delta.open_handles, 22);
    }

    #[test]
    fn a_delta_never_goes_negative() {
        let later = Process {
            cpu_ms: 10,
            ..Default::default()
        };
        let earlier = Process {
            cpu_ms: 100,
            ..Default::default()
        };
        assert_eq!(later.since(&earlier).cpu_ms, 0);
    }

    #[test]
    fn the_host_names_its_cpu_os_and_memory() {
        let host = Host::capture();
        // `unavailable` is the mechanism for a field this platform cannot
        // read, so the assertion is about **which** fields are on it, not that
        // it is empty. On the Apple platforms link speeds need `getifaddrs`
        // plus an `SIOCGIFMEDIA` ioctl per interface, which nothing measured
        // here compares across machines yet, and saying so beats reporting an
        // empty list as though the machine had no interfaces. Equality rather
        // than containment, so a second field going unreadable still fails.
        // See `TODO/cli-surface.md`, T-153.
        let expected: &[&str] = match cfg!(target_vendor = "apple") {
            true => &["network"],
            false => &[],
        };
        assert_eq!(
            host.unavailable, expected,
            "the set of fields this platform cannot read changed"
        );
        assert_ne!(host.cpu.model, "unknown", "no CPU model");
        assert!(host.cpu.logical_cores >= 1);
        assert_eq!(host.cpu.architecture, std::env::consts::ARCH);
        assert!(
            host.memory_total.0 > 256 * 1024 * 1024,
            "total memory of {} is not a real machine",
            host.memory_total.0
        );
        assert!(!host.os.name.is_empty());
        assert_ne!(host.os.version, "unknown");
        // The version is dotted numbers on Windows and a kernel release on
        // Linux. Both carry at least one dot.
        assert!(host.os.version.contains('.'), "{}", host.os.version);
    }

    #[test]
    fn capturing_the_host_twice_gives_the_same_answer() {
        assert_eq!(Host::capture(), Host::capture());
    }

    #[test]
    fn the_host_survives_a_json_round_trip() {
        let host = Host::capture();
        let json = serde_json::to_string(&host).unwrap();
        assert_eq!(serde_json::from_str::<Host>(&json).unwrap(), host);
    }

    #[test]
    fn a_host_is_comparable_to_itself_and_not_to_another_cpu() {
        let host = Host::capture();
        assert!(host.comparable_to(&host));
        assert!(host.differences(&host).is_empty());

        let mut other = host.clone();
        other.cpu.model = "Some Other Processor".into();
        assert!(!host.comparable_to(&other));
        assert_eq!(host.differences(&other).len(), 1);

        // Memory and kernel patch level do not decide comparability: an OS
        // update is worth measuring across.
        let mut updated = host.clone();
        updated.os.version = "0.0.0".into();
        updated.memory_total = Size(host.memory_total.0 + 1);
        assert!(host.comparable_to(&updated));
    }

    #[test]
    fn link_speeds_are_decimal_because_that_is_how_links_are_sold() {
        assert_eq!(format_link_speed(1_000_000_000), "1.00 Gbit/s");
        assert_eq!(format_link_speed(2_500_000_000), "2.50 Gbit/s");
        assert_eq!(format_link_speed(100_000_000), "100.00 Mbit/s");
        assert_eq!(format_link_speed(56_000), "56.00 Kbit/s");
        assert_eq!(format_link_speed(300), "300 bit/s");
    }

    #[test]
    fn every_reported_interface_is_up_carries_a_speed_and_is_not_loopback() {
        for nic in Host::capture().network {
            assert!(nic.up, "{} is reported but not up", nic.name);
            assert!(!nic.name.is_empty());
            assert_ne!(nic.name, "lo");
            let bps = nic
                .link_speed_bps
                .unwrap_or_else(|| panic!("{} is listed with no link speed", nic.name));
            assert!(bps > 0, "{} reports a zero link speed", nic.name);
            assert_ne!(
                bps,
                u64::from(u32::MAX),
                "{} reports the saturation value of a DWORD, not a link speed",
                nic.name
            );
            assert_eq!(nic.link_speed_human, Some(format_link_speed(bps)));
        }
    }

    #[test]
    fn the_interfaces_are_ordered_fastest_first() {
        let speeds: Vec<Option<u64>> = Host::capture()
            .network
            .iter()
            .map(|nic| nic.link_speed_bps)
            .collect();
        let mut sorted = speeds.clone();
        sorted.sort_by_key(|speed| std::cmp::Reverse(*speed));
        assert_eq!(speeds, sorted);
    }

    #[test]
    fn a_link_is_reported_once_rather_than_once_per_driver_layer() {
        let names: Vec<String> = Host::capture()
            .network
            .into_iter()
            .map(|nic| nic.name)
            .collect();
        let unique: std::collections::BTreeSet<&String> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "a duplicate row in {names:?}");
        // A filter driver's description is its parent's with a suffix. If a
        // row's name starts with another row's name, the two are the same
        // link seen through two layers and one of them should have gone.
        for name in &names {
            for other in &names {
                assert!(
                    std::ptr::eq(name, other) || !name.starts_with(other.as_str()),
                    "{name:?} is a driver layer over {other:?}"
                );
            }
        }
    }

    #[test]
    fn the_fastest_link_is_the_one_reported_as_the_ceiling() {
        let host = Host::capture();
        assert_eq!(
            host.link_speed_bps(),
            host.network.iter().filter_map(|n| n.link_speed_bps).max()
        );
    }
}
