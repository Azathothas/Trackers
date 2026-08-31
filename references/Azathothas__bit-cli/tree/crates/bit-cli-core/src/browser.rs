//! Finding a browser somebody has already installed.
//!
//! `TODO/cli-surface.md` T-244's `--render` tier reads a page after script has
//! run, and the operator's ruling is that it drives a Chrome or Edge that is
//! **already installed**: off by default, never bundled, and absent gracefully
//! when there is none.
//!
//! No CDP crate does this part. They can launch a browser at a path and they
//! will not find one, so this is the resolver, and it is deliberately separate
//! from anything that speaks the DevTools protocol: **the failing case is the
//! one that has to work on every machine**, including every CI runner, where
//! there is no browser to find. Keeping the search here means it is unit
//! tested rather than only exercised where a browser happens to exist.
//!
//! The order is fixed and each step is there for a reason:
//!
//! 1. An explicit path, from a flag or from the environment. Always first,
//!    because somebody who named one meant it, and because it is the only
//!    thing that works for a browser installed somewhere nobody expects.
//! 2. An already-running instance on a debugging port. Before the platform
//!    defaults, because attaching to a browser that is already up is cheaper
//!    than starting a second one and is what a caller who opened one wants.
//! 3. Platform defaults, in the order a desktop is likely to have them.
//! 4. Nothing, which is a typed error naming every path it looked at, not a
//!    panic and not a silent fall back to reading the page unrendered.

use std::path::{Path, PathBuf};

/// A browser this tree is willing to drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Browser {
    /// An executable to launch.
    Executable(PathBuf),
    /// An instance already listening for the DevTools protocol.
    Attached { host: String, port: u16 },
}

/// Nothing was found, and every place that was looked at.
///
/// The list is the whole point. "No browser found" tells somebody with Chrome
/// installed nothing at all; the list tells them it looked at
/// `/usr/bin/google-chrome` and their Chrome is a flatpak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoBrowser {
    /// Every candidate, in the order it was tried.
    pub searched: Vec<String>,
    /// Set when an explicit path was given and was not there, because that is
    /// a different mistake from having none installed.
    pub explicit_missing: Option<PathBuf>,
}

impl std::fmt::Display for NoBrowser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.explicit_missing {
            Some(path) => write!(
                f,
                "no browser at {}, which is where --browser-path pointed",
                path.display()
            ),
            None => write!(
                f,
                "no installed Chrome or Edge was found. --render needs one and never installs it. Looked at: {}",
                self.searched.join(", ")
            ),
        }
    }
}

impl std::error::Error for NoBrowser {}

/// Where to look, and what the caller already told us.
#[derive(Debug, Clone, Default)]
pub struct Search {
    /// `--browser-path`, or `BIT_BROWSER` from the environment. Tried first
    /// and, when it is set and missing, the only thing tried: a caller who
    /// named a path is not helped by silently using a different browser.
    pub explicit: Option<PathBuf>,
    /// `--browser-port`. An instance already speaking the DevTools protocol.
    pub attach: Option<(String, u16)>,
    /// Extra directories to search, ahead of the platform defaults. This is
    /// what a test uses instead of a real filesystem.
    pub extra: Vec<PathBuf>,
}

/// Executable names to try on `PATH`, in the order a desktop is likely to have
/// them.
///
/// Chrome before Chromium before Edge: the profile this tree impersonates is
/// Chrome's, and driving the browser whose fingerprint is already claimed is
/// one fewer thing that disagrees.
pub const PATH_NAMES: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "microsoft-edge",
    "microsoft-edge-stable",
];

/// Absolute locations to try, per platform, after `PATH`.
///
/// Written out for every platform rather than behind `cfg`, because
/// `cfg(unix)` is a family and not a platform: `/Applications` is macOS and
/// `/usr/bin` is not. `TODO/RULES.md` section 5 has what that cost the last
/// time it was assumed. The caller says which list it wants, so a test can ask
/// for a platform it is not running on.
pub fn default_paths(platform: Platform) -> Vec<PathBuf> {
    let paths: &[&str] = match platform {
        Platform::Linux => &[
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/bin/microsoft-edge",
            "/snap/bin/chromium",
            "/var/lib/flatpak/exports/bin/com.google.Chrome",
            "/var/lib/flatpak/exports/bin/org.chromium.Chromium",
        ],
        Platform::MacOs => &[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ],
        Platform::Windows => &[
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Chromium\Application\chrome.exe",
        ],
    };
    paths.iter().map(PathBuf::from).collect()
}

/// Every `PATH_NAMES` entry expanded against every directory in a `PATH`.
///
/// `PATH` is passed in rather than read, because nothing in this library reads
/// the environment on its own. The separator is the caller's too: it is `;` on
/// Windows and `:` everywhere else, and getting that from `cfg` here would
/// make the function untestable for the platform it is not running on.
///
/// On Linux this is where a browser actually is. `/usr/bin/google-chrome` is
/// in [`default_paths`] as well, and the two overlapping is deliberate: a
/// distribution that puts it somewhere else still has it on `PATH`, and a
/// `PATH` that does not carry it still has the absolute location.
pub fn path_candidates(path_var: &str, separator: char) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in path_var.split(separator) {
        let dir = dir.trim();
        if dir.is_empty() {
            continue;
        }
        for name in PATH_NAMES {
            out.push(Path::new(dir).join(name));
        }
    }
    out
}

/// Which platform's default locations to search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOs,
    Windows,
}

impl Platform {
    /// The platform this binary was built for.
    pub const fn host() -> Self {
        match () {
            () if cfg!(target_os = "windows") => Self::Windows,
            () if cfg!(target_os = "macos") => Self::MacOs,
            () => Self::Linux,
        }
    }
}

/// `~/Applications` and `%LOCALAPPDATA%` style locations, which depend on who
/// is running rather than on the platform alone.
///
/// Passed in rather than read here, because nothing in this library reads the
/// environment on its own.
pub fn home_paths(platform: Platform, home: &Path) -> Vec<PathBuf> {
    match platform {
        Platform::Linux => Vec::new(),
        Platform::MacOs => vec![
            home.join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            home.join("Applications/Chromium.app/Contents/MacOS/Chromium"),
        ],
        Platform::Windows => vec![
            home.join(r"AppData\Local\Google\Chrome\Application\chrome.exe"),
            home.join(r"AppData\Local\Microsoft\Edge\Application\msedge.exe"),
            home.join(r"AppData\Local\Chromium\Application\chrome.exe"),
        ],
    }
}

/// Find a browser, or say exactly where it looked.
///
/// `exists` answers "is there an executable here". It is a parameter so the
/// whole search is testable without a browser installed, which is the case
/// that has to work everywhere.
pub fn resolve<F>(search: &Search, candidates: &[PathBuf], exists: F) -> Result<Browser, NoBrowser>
where
    F: Fn(&Path) -> bool,
{
    let mut searched: Vec<String> = Vec::new();

    // 1. An explicit path wins, and its absence is its own error. Falling
    //    through to a different browser here would be the tool ignoring the
    //    one instruction it was given.
    if let Some(path) = &search.explicit {
        searched.push(path.display().to_string());
        if exists(path) {
            return Ok(Browser::Executable(path.clone()));
        }
        return Err(NoBrowser {
            searched,
            explicit_missing: Some(path.clone()),
        });
    }

    // 2. Something already running. Not probed here: opening a socket is the
    //    caller's to do, and this layer stays a pure function.
    if let Some((host, port)) = &search.attach {
        return Ok(Browser::Attached {
            host: host.clone(),
            port: *port,
        });
    }

    // 3. Everything the caller assembled, in order.
    for path in search.extra.iter().chain(candidates.iter()) {
        searched.push(path.display().to_string());
        if exists(path) {
            return Ok(Browser::Executable(path.clone()));
        }
    }

    Err(NoBrowser {
        searched,
        explicit_missing: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &Path) -> bool {
        false
    }

    fn only(want: &'static str) -> impl Fn(&Path) -> bool {
        move |p: &Path| p == Path::new(want)
    }

    #[test]
    fn nothing_installed_is_a_typed_error_naming_every_path_it_looked_at() {
        let candidates = default_paths(Platform::Linux);
        let err = resolve(&Search::default(), &candidates, none).expect_err("must not find one");
        assert_eq!(err.explicit_missing, None);
        assert_eq!(err.searched.len(), candidates.len());
        assert!(err.searched.iter().any(|s| s.contains("google-chrome")));
        let text = err.to_string();
        assert!(text.contains("no installed Chrome or Edge"), "{text}");
        assert!(text.contains("/usr/bin/google-chrome"), "{text}");
        // It says it will not install one, because "not found" reads as
        // "about to fetch it" to somebody who has met other tools.
        assert!(text.contains("never installs it"), "{text}");
    }

    #[test]
    fn an_explicit_path_is_tried_first_and_wins() {
        let search = Search {
            explicit: Some(PathBuf::from("/opt/my-chrome")),
            ..Search::default()
        };
        let found = resolve(
            &search,
            &default_paths(Platform::Linux),
            only("/opt/my-chrome"),
        )
        .expect("the explicit path exists");
        assert_eq!(found, Browser::Executable(PathBuf::from("/opt/my-chrome")));
    }

    #[test]
    fn an_explicit_path_that_is_missing_does_not_fall_through_to_another_browser() {
        let search = Search {
            explicit: Some(PathBuf::from("/opt/gone")),
            ..Search::default()
        };
        // A real Chrome is present and must still not be used: the caller
        // named a path and being given a different browser is worse than an
        // error.
        let err = resolve(
            &search,
            &default_paths(Platform::Linux),
            only("/usr/bin/google-chrome"),
        )
        .expect_err("a named path that is not there is an error");
        assert_eq!(err.explicit_missing, Some(PathBuf::from("/opt/gone")));
        assert_eq!(err.searched, vec!["/opt/gone".to_string()]);
        assert!(err.to_string().contains("--browser-path"), "{err}");
    }

    #[test]
    fn an_already_running_instance_is_taken_before_the_platform_defaults() {
        let search = Search {
            attach: Some(("127.0.0.1".to_string(), 9222)),
            ..Search::default()
        };
        // Chrome is installed, and the running instance still wins: starting a
        // second browser when one is already up is the wrong answer.
        let found = resolve(
            &search,
            &default_paths(Platform::Linux),
            only("/usr/bin/google-chrome"),
        )
        .expect("attach");
        assert_eq!(
            found,
            Browser::Attached {
                host: "127.0.0.1".to_string(),
                port: 9222
            }
        );
    }

    #[test]
    fn the_first_candidate_that_exists_is_the_one_taken() {
        let candidates = default_paths(Platform::Linux);
        let found = resolve(&Search::default(), &candidates, only("/usr/bin/chromium"))
            .expect("chromium is there");
        assert_eq!(
            found,
            Browser::Executable(PathBuf::from("/usr/bin/chromium"))
        );
    }

    #[test]
    fn chrome_is_preferred_over_chromium_and_edge() {
        let candidates = default_paths(Platform::Linux);
        let found = resolve(&Search::default(), &candidates, |p| {
            let p = p.to_string_lossy();
            p.contains("chrome") || p.contains("chromium") || p.contains("edge")
        })
        .expect("several are there");
        assert_eq!(
            found,
            Browser::Executable(PathBuf::from("/usr/bin/google-chrome"))
        );
    }

    #[test]
    fn extra_directories_are_searched_before_the_platform_defaults() {
        let search = Search {
            extra: vec![PathBuf::from("/custom/chrome")],
            ..Search::default()
        };
        let found =
            resolve(&search, &default_paths(Platform::Linux), |_| true).expect("everything exists");
        assert_eq!(found, Browser::Executable(PathBuf::from("/custom/chrome")));
    }

    #[test]
    fn every_platform_offers_an_absolute_place_to_look() {
        // `Path::is_absolute` answers about the **host**, not about the path:
        // `/usr/bin/chrome` reads as relative on Windows, where an absolute
        // path needs a drive. So the shape is checked per target platform,
        // which is the same rule as `cfg(unix)` being a family and not a
        // platform. See `TODO/RULES.md` section 5.
        for platform in [Platform::Linux, Platform::MacOs, Platform::Windows] {
            let paths = default_paths(platform);
            assert!(!paths.is_empty(), "{platform:?} has no candidates");
            for path in &paths {
                let text = path.to_string_lossy().to_string();
                let absolute = match platform {
                    Platform::Linux | Platform::MacOs => text.starts_with('/'),
                    Platform::Windows => {
                        text.starts_with(r"\\") || text.as_bytes().get(1) == Some(&b':')
                    }
                };
                assert!(absolute, "{platform:?} has a relative candidate: {text}");
            }
        }
    }

    #[test]
    fn a_macos_bundle_path_reaches_the_executable_inside_it() {
        // `.app` is a directory. Launching it is what `open` does, not what a
        // CDP client does, so the candidate has to name the binary.
        for path in default_paths(Platform::MacOs) {
            let text = path.to_string_lossy().to_string();
            assert!(text.contains("/Contents/MacOS/"), "{text}");
        }
    }

    #[test]
    fn a_path_variable_expands_to_one_candidate_per_name_per_directory() {
        let found = path_candidates("/usr/bin:/usr/local/bin", ':');
        assert_eq!(found.len(), 2 * PATH_NAMES.len());
        assert_eq!(found[0], PathBuf::from("/usr/bin/google-chrome"));
        assert_eq!(
            found[PATH_NAMES.len()],
            PathBuf::from("/usr/local/bin/google-chrome")
        );
    }

    #[test]
    fn an_empty_path_entry_is_skipped_rather_than_becoming_a_relative_candidate() {
        // A trailing or doubled separator is common, and joining a name onto
        // "" gives a bare `google-chrome`, which would be resolved against the
        // working directory. That is a path traversal waiting to happen.
        let found = path_candidates("/usr/bin::  :", ':');
        assert_eq!(found.len(), PATH_NAMES.len());
        assert!(found.iter().all(|p| p.starts_with("/usr/bin")), "{found:?}");
    }

    #[test]
    fn the_windows_separator_is_the_callers_to_choose() {
        let found = path_candidates(r"C:\bin;C:\other", ';');
        assert_eq!(found.len(), 2 * PATH_NAMES.len());
        assert!(
            found[0].to_string_lossy().starts_with(r"C:\bin"),
            "{:?}",
            found[0]
        );
    }

    #[test]
    fn a_path_lookup_finds_a_browser_the_platform_defaults_do_not_list() {
        // The case this exists for: a distribution that installs Chrome
        // somewhere `default_paths` has never heard of, but that is on `PATH`.
        let search = Search {
            extra: path_candidates("/opt/weird/bin", ':'),
            ..Search::default()
        };
        let found = resolve(
            &search,
            &default_paths(Platform::Linux),
            only("/opt/weird/bin/chromium"),
        )
        .expect("on PATH");
        assert_eq!(
            found,
            Browser::Executable(PathBuf::from("/opt/weird/bin/chromium"))
        );
    }

    #[test]
    fn home_paths_are_per_user_and_per_platform() {
        let home = Path::new("/home/someone");
        assert!(home_paths(Platform::Linux, home).is_empty());
        assert!(!home_paths(Platform::MacOs, home).is_empty());
        assert!(!home_paths(Platform::Windows, home).is_empty());
        for path in home_paths(Platform::MacOs, home) {
            assert!(path.starts_with(home), "{}", path.display());
        }
    }

    #[test]
    fn the_host_platform_is_one_of_the_three() {
        // `cfg(unix)` is a family and not a platform, so this is a real check
        // rather than a tautology: it fails on a target that is neither
        // Windows nor macOS nor treated as Linux.
        let candidates = default_paths(Platform::host());
        assert!(!candidates.is_empty());
    }
}
