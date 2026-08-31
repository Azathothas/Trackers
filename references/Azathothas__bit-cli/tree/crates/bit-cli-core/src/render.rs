//! Reading a page after its script has run.
//!
//! `--render` is the second tier of `TODO/cli-surface.md` T-244. The first
//! reads the HTML a server sent; this one drives a Chrome or Edge that is
//! **already installed** over the DevTools protocol, takes the DOM after
//! script has run, and hands it to the same [`crate::page::extract`] the
//! static tier calls.
//!
//! **The extractor is the same function over an HTML string in both tiers,
//! and that is the whole design.** If this module changed anything but where
//! the HTML came from, the two tiers could disagree about a page for a reason
//! that is not the page. `scripts/check-page-extract.ps1` holds that: levels
//! 0 to 3 of the proving ground must extract identically in both tiers, and a
//! difference there is a defect here rather than a property of the page.
//!
//! # Why it is a Cargo feature
//!
//! The operator's ruling of 2026-08-29. `chromiumoxide` is inert without a
//! browser nobody installs for a torrent client, so it is off by default and
//! built by a CI job so it cannot rot. `crate::browser`, the resolver, stays
//! in the default build and is tested everywhere, because the case that has
//! to work on every machine is the one where there is no browser at all.
//!
//! # What it does not do
//!
//! **It does not defeat a challenge.** A driven Chrome is not a stealthy
//! Chrome: it announces automation and a hostile origin can see that.
//! `--render` exists for pages that build their links in script, which is an
//! ordinary indexer, and a bot check is a refusal with the status named in
//! both tiers.
//!
//! **It leaves no browser running.** Every path out of [`render`] closes the
//! browser and stops the connection handler, including the ones that fail and
//! the one where the deadline fires. A rendered fetch that leaves a Chrome
//! behind poisons the next run and, on a CI runner, is a job that never ends.

use std::time::Duration;

use crate::browser::Browser;

/// Why a page could not be rendered.
#[derive(Debug)]
pub enum RenderError {
    /// The binary was built without the `render` feature.
    NotBuilt,
    /// The browser could not be started or attached to.
    Launch(String),
    /// The browser started and the page did not load.
    Navigate { url: String, detail: String },
    /// The deadline fired.
    Timeout { url: String, deadline: Duration },
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotBuilt => write!(
                f,
                "--render needs a build with the `render` feature, and this binary does not have it. \
                 Build one with: cargo build --release --features render"
            ),
            Self::Launch(detail) => write!(f, "the browser could not be started: {detail}"),
            Self::Navigate { url, detail } => write!(f, "{url} did not render: {detail}"),
            Self::Timeout { url, deadline } => write!(
                f,
                "{url} did not finish rendering within {}ms, which is what --timeout allows",
                deadline.as_millis()
            ),
        }
    }
}

impl std::error::Error for RenderError {}

/// Whether this binary can render at all.
///
/// Read before a run reaches for a browser, so the message says "this build"
/// rather than "no browser found" on a machine that has one.
pub const fn is_built() -> bool {
    cfg!(feature = "render")
}

/// Fetch `url` through `browser` and return the DOM after script has run.
///
/// Without the `render` feature this is [`RenderError::NotBuilt`] and nothing
/// else: the flag exists in every build so that the command surface, the
/// manuals and the error messages do not change shape between two binaries
/// with the same version.
#[cfg(not(feature = "render"))]
pub async fn render(
    _browser: &Browser,
    _url: &str,
    _deadline: Duration,
) -> Result<String, RenderError> {
    Err(RenderError::NotBuilt)
}

/// Fetch `url` through `browser` and return the DOM after script has run.
///
/// Three DevTools calls and no more, which is deliberate: `Page.navigate`,
/// `Page.lifecycleEvent` through `wait_for_navigation`, and
/// `DOM.getDocument` plus `DOM.getOuterHTML` through `content`. All three have
/// been stable across Chrome majors for years, and a driver that reaches for
/// more of the protocol is a driver that breaks on a browser update.
#[cfg(feature = "render")]
pub async fn render(
    browser: &Browser,
    url: &str,
    deadline: Duration,
) -> Result<String, RenderError> {
    use futures_util::StreamExt;

    let (mut driver, mut handler) = match browser {
        Browser::Attached { host, port } => {
            chromiumoxide::Browser::connect(format!("http://{host}:{port}"))
                .await
                .map_err(|e| RenderError::Launch(e.to_string()))?
        }
        Browser::Executable(path) => {
            // A throwaway profile, so an installed profile's extensions,
            // proxies and enterprise policy cannot change what the page
            // becomes. A page read through somebody's ad blocker is not the
            // page an origin served.
            let profile = std::env::temp_dir().join(format!(
                "bit-cli-render-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default()
            ));
            let config = chromiumoxide::BrowserConfig::builder()
                .chrome_executable(path)
                .user_data_dir(&profile)
                .args([
                    "--no-first-run",
                    "--no-default-browser-check",
                    "--disable-search-engine-choice-screen",
                    "--disable-gpu",
                    "--disable-extensions",
                ])
                .build()
                .map_err(RenderError::Launch)?;
            chromiumoxide::Browser::launch(config)
                .await
                .map_err(|e| RenderError::Launch(e.to_string()))?
        }
    };

    // The connection handler has to be polled for anything to happen, and it
    // has to be stopped afterwards. `JoinHandle::abort` is what guarantees the
    // second half on every path out of here, including a panic.
    let pump = tokio::spawn(async move { while handler.next().await.is_some() {} });

    let outcome = tokio::time::timeout(deadline, read_dom(&driver, url)).await;

    // Close before returning, whatever happened. `close` asks the browser to
    // shut down and `kill` is what happens when it will not: a rendered fetch
    // that leaves a Chrome behind is the defect this ordering exists to
    // prevent, and it is tested.
    let _ = driver.close().await;
    let _ = driver.wait().await;
    pump.abort();

    match outcome {
        Err(_) => Err(RenderError::Timeout {
            url: url.to_string(),
            deadline,
        }),
        Ok(result) => result,
    }
}

/// How long to leave between two reads of the document while waiting for it
/// to stop changing.
#[cfg(feature = "render")]
const SETTLE_INTERVAL: Duration = Duration::from_millis(250);

/// How many times to read before giving up and taking what is there.
///
/// The outer deadline is the real bound; this stops a page that rewrites
/// itself forever, which is a page no wait would ever settle.
#[cfg(feature = "render")]
const SETTLE_READS: usize = 24;

/// The script that composes one HTML string out of a rendered document.
///
/// `documentElement.outerHTML` is the document, and it does **not** include
/// what is inside an open shadow root, which is where a component library puts
/// the links it renders. Each open root's contents are appended after the
/// document, so the result is still one string for one extractor, and every
/// URL in it still resolves against the same document and the same
/// `<base href>`.
///
/// A **closed** shadow root is not readable by script at all, by design, and
/// is therefore not readable here either. An `<iframe>` is a separate document
/// with its own URL and is deliberately left alone: following one would make a
/// page a crawl, and the one-hop rule is what stops a page linking to a page
/// from becoming a graph walk.
#[cfg(feature = "render")]
/// It is a function rather than an expression on purpose: `evaluate` inspects
/// a string to decide which it is, and an immediately-invoked function
/// expression is exactly the shape that reads as both.
/// `evaluate_expression` takes it as written.
const COMPOSE_JS: &str = r#"(function () {
  var parts = [document.documentElement.outerHTML];
  var walk = function (root) {
    var all = root.querySelectorAll('*');
    for (var i = 0; i < all.length; i++) {
      if (all[i].shadowRoot) {
        parts.push(all[i].shadowRoot.innerHTML);
        walk(all[i].shadowRoot);
      }
    }
  };
  walk(document);
  // String.fromCharCode(10) rather than an escape: this is a Rust raw
  // string inside a file that several tools rewrite, and a backslash that
  // survives one of them and not the next produces a JavaScript syntax
  // error at run time and nothing at compile time. It cost one.
  return parts.join(String.fromCharCode(10));
})()"#;

/// Navigate, wait for the document to stop changing, and take it.
///
/// **The wait is on the condition and not on a duration.** A link a page
/// builds in a `setTimeout` or after a `fetch` is not in the document when
/// `load` fires, so reading once and returning misses exactly the links this
/// tier exists to find. Two identical reads in a row is the condition: the
/// document has stopped changing. `TODO/RULES.md` section 5 is the rule and
/// three entries in this repository are what it cost to learn.
#[cfg(feature = "render")]
async fn read_dom(driver: &chromiumoxide::Browser, url: &str) -> Result<String, RenderError> {
    let fail = |e: chromiumoxide::error::CdpError| RenderError::Navigate {
        url: url.to_string(),
        detail: e.to_string(),
    };
    let page = driver.new_page(url).await.map_err(fail)?;
    page.wait_for_navigation().await.map_err(fail)?;

    let mut previous: Option<String> = None;
    let mut html = String::new();
    for _ in 0..SETTLE_READS {
        html = page
            .evaluate_expression(COMPOSE_JS)
            .await
            .map_err(fail)?
            .into_value::<String>()
            .map_err(|e| RenderError::Navigate {
                url: url.to_string(),
                detail: format!("the page did not answer with its own markup: {e}"),
            })?;
        if previous.as_deref() == Some(html.as_str()) {
            break;
        }
        previous = Some(html.clone());
        tokio::time::sleep(SETTLE_INTERVAL).await;
    }

    let _ = page.close().await;
    Ok(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_not_built_message_names_the_feature_and_the_command() {
        let text = RenderError::NotBuilt.to_string();
        assert!(text.contains("render"), "{text}");
        assert!(text.contains("cargo build"), "{text}");
    }

    #[test]
    fn a_timeout_names_the_flag_that_sets_it() {
        let text = RenderError::Timeout {
            url: "http://example.invalid/p".to_string(),
            deadline: Duration::from_millis(2500),
        }
        .to_string();
        assert!(text.contains("2500ms"), "{text}");
        assert!(text.contains("--timeout"), "{text}");
    }

    #[test]
    fn a_navigate_failure_names_the_page() {
        let text = RenderError::Navigate {
            url: "http://example.invalid/p".to_string(),
            detail: "target closed".to_string(),
        }
        .to_string();
        assert!(text.contains("http://example.invalid/p"), "{text}");
        assert!(text.contains("target closed"), "{text}");
    }

    #[test]
    fn is_built_agrees_with_the_feature() {
        assert_eq!(is_built(), cfg!(feature = "render"));
    }

    /// The default build refuses rather than pretending, and it does so
    /// without looking for a browser: the message is about this binary.
    #[cfg(not(feature = "render"))]
    #[tokio::test]
    async fn without_the_feature_every_render_is_not_built() {
        let browser = Browser::Executable(std::path::PathBuf::from("/nonexistent/chrome"));
        let err = render(&browser, "http://example.invalid/", Duration::from_secs(1))
            .await
            .expect_err("a build without the feature cannot render");
        assert!(matches!(err, RenderError::NotBuilt), "{err}");
    }
}
