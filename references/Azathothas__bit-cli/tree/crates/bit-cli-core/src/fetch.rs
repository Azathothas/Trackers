//! One door for fetching a source document, and two clients behind it.
//!
//! Everything `bit-cli` reads over HTTP that is not payload bytes comes
//! through here: a `.torrent`, a Metalink, a web page that links to one. The
//! two clients differ in what they put on the wire and in nothing else, so the
//! caller picks a [`ClientProfile`] and stops thinking about it.
//!
//! - [`ClientProfile::Browser`] is the default. It presents as the Chrome this
//!   tree records in [`crate::page`]: Chrome's `ClientHello`, Chrome's HTTP/2
//!   settings, Chrome's pseudo-header order, Chrome's header set in Chrome's
//!   order. An origin that fingerprints its callers sends a different page to
//!   a client it does not recognise, and a reader parsing that page is reading
//!   a page nobody else gets.
//! - [`ClientProfile::Plain`] sends `bit-cli/<version>` and nothing else,
//!   which is what every request here sent before T-244.
//!
//! **A web seed is not a source document and does not come through here.** It
//! is a mirror the caller configured, fetching payload bytes, and it keeps
//! `bit-cli/<version>`: impersonating at a mirror somebody pointed us at buys
//! nothing and hides who is asking. `crates/bit-cli-core/src/webseed/` is that
//! path and it is deliberately separate.
//!
//! See `TODO/cli-surface.md`, T-244.

use std::time::Duration;

use crate::page::{
    BROWSER_H2_HEADER_TABLE_SIZE, BROWSER_H2_OMIT_MAX_FRAME_SIZE, BROWSER_H2_STREAM_PRIORITY,
    ClientProfile,
};

/// What to fetch, and the two bounds every fetch here carries.
#[derive(Debug, Clone)]
pub struct FetchRequest<'a> {
    pub url: &'a str,
    /// Stop reading past this many bytes. The body is read in chunks and the
    /// cap is checked as it grows, so it bounds what is held in memory rather
    /// than only what is returned.
    pub max_bytes: usize,
    /// The whole request, connect to last byte.
    pub deadline: Duration,
}

/// What came back.
#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub status: u16,
    /// Every response header, names lowercased, in the order they arrived.
    /// Values are carried because a report names a CDN cache hit by one; see
    /// `TODO/webseed.md`, T-254.
    pub headers: Vec<(String, String)>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

impl FetchResponse {
    /// The first value of a header, matched case insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Why a fetch did not produce a body.
///
/// Structural rather than a formatted string, because the caller turns each
/// one into a different exit code: a deadline that fired is not a 404 and
/// neither is a body that ran past its ceiling.
#[derive(Debug, Clone)]
pub enum FetchError {
    /// The deadline fired.
    Timeout { url: String, deadline: Duration },
    /// The server answered, and not with a success status.
    Status { url: String, status: u16 },
    /// The body ran past `max_bytes`.
    TooLarge { url: String, max_bytes: usize },
    /// The client could not be built at all. A profile that cannot be
    /// constructed is a defect here, not a network condition.
    Build(String),
    /// Anything else the transport reported.
    Network {
        url: String,
        /// What was being attempted, for the message: "cannot fetch",
        /// "cannot read the body of".
        what: String,
        detail: String,
    },
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { url, deadline } => write!(
                f,
                "{url}: no answer within {}ms, which is what --timeout allows",
                deadline.as_millis()
            ),
            Self::Status { url, status } => write!(f, "{url}: {status}"),
            Self::TooLarge { url, max_bytes } => write!(
                f,
                "{url} answered with more than {max_bytes} bytes, which is larger than any document a source can be"
            ),
            Self::Build(detail) => write!(f, "cannot build an HTTP client: {detail}"),
            Self::Network { url, what, detail } => write!(f, "{what} {url}: {detail}"),
        }
    }
}

impl std::error::Error for FetchError {}

/// One `GET`, with a ceiling and a deadline.
///
/// **There is no second request anywhere behind this trait.** No retry, no
/// backoff, no re-fetch with a different profile. A page that answers with a
/// bot check is an error carrying the status, which is the operator's ruling:
/// a challenge is a refusal, not a thing to defeat.
#[async_trait::async_trait]
pub trait Fetcher: Send + Sync {
    /// Which client this is, for a report.
    fn profile(&self) -> ClientProfile;

    async fn get(&self, request: FetchRequest<'_>) -> Result<FetchResponse, FetchError>;
}

/// Who this client says it is.
///
/// Carried as one value rather than as a loose `&str`, because the User-Agent
/// and the rest of the header set are one decision: a request sending Chrome's
/// `sec-ch-ua` beside `bit-cli/0.2.0` is more distinctive than either alone.
#[derive(Debug, Clone)]
pub struct Identity {
    pub user_agent: String,
    /// Whether the caller named the agent. A profile does not overwrite one
    /// somebody passed on purpose.
    pub user_agent_given: bool,
    pub profile: ClientProfile,
}

impl Identity {
    /// `bit-cli/<version>` and nothing else, which is what every request here
    /// sent before T-244.
    pub fn plain(user_agent: &str) -> Self {
        Self {
            user_agent: user_agent.to_string(),
            user_agent_given: true,
            profile: ClientProfile::Plain,
        }
    }
}

/// The fetcher for an identity.
///
/// One call rather than a match at every call site, so a new profile is one
/// arm here and nothing anywhere else.
pub fn fetcher_for(
    identity: &Identity,
    deadline: Duration,
) -> Result<Box<dyn Fetcher>, FetchError> {
    match identity.profile {
        ClientProfile::Plain => Ok(Box::new(PlainFetcher::new(identity, deadline)?)),
        ClientProfile::Browser => Ok(Box::new(BrowserFetcher::new(identity, deadline)?)),
    }
}

// ===== the plain client =====

/// `bit-cli/<version>` over this tree's own `reqwest`.
pub struct PlainFetcher {
    client: reqwest::Client,
}

impl PlainFetcher {
    pub fn new(identity: &Identity, deadline: Duration) -> Result<Self, FetchError> {
        let mut builder = reqwest::Client::builder()
            .timeout(deadline)
            .user_agent(&identity.user_agent);
        // The same added roots the browser profile honours, for the same
        // reason and with the same limit: added to the usual ones, never
        // instead of them. Both profiles read it so that both are readable by
        // `scripts/check-fingerprint.ps1` over a real handshake.
        for der in extra_roots()? {
            let certificate = reqwest::Certificate::from_der(&der)
                .map_err(|e| FetchError::Build(format!("{EXTRA_CA_FILE_ENV}: {e}")))?;
            builder = builder.add_root_certificate(certificate);
        }
        let client = builder
            .build()
            .map_err(|e| FetchError::Build(e.to_string()))?;
        Ok(Self { client })
    }
}

#[async_trait::async_trait]
impl Fetcher for PlainFetcher {
    fn profile(&self) -> ClientProfile {
        ClientProfile::Plain
    }

    async fn get(&self, request: FetchRequest<'_>) -> Result<FetchResponse, FetchError> {
        let mut response = self
            .client
            .get(request.url)
            .send()
            .await
            .map_err(|e| plain_error(e, request.url, "cannot fetch", request.deadline))?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            return Err(FetchError::Status {
                url: request.url.to_string(),
                status,
            });
        }
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_ascii_lowercase(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect::<Vec<_>>();
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| plain_error(e, request.url, "cannot read the body of", request.deadline))?
        {
            body.extend_from_slice(&chunk);
            if body.len() > request.max_bytes {
                return Err(FetchError::TooLarge {
                    url: request.url.to_string(),
                    max_bytes: request.max_bytes,
                });
            }
        }
        Ok(finish(status, headers, body))
    }
}

fn plain_error(err: reqwest::Error, url: &str, what: &str, deadline: Duration) -> FetchError {
    if err.is_timeout() {
        return FetchError::Timeout {
            url: url.to_string(),
            deadline,
        };
    }
    FetchError::Network {
        url: url.to_string(),
        what: what.to_string(),
        detail: err.to_string(),
    }
}

// ===== the impersonating client =====

/// A current Chrome, off the wire rather than out of a header set.
///
/// The `ClientHello`, the HTTP/2 SETTINGS and WINDOW_UPDATE, the
/// pseudo-header order and the field order all come from one fingerprint. The
/// header set is this tree's, in [`crate::page`], so a staleness check has one
/// file to rewrite rather than a vendored one.
pub struct BrowserFetcher {
    client: impit::impit::Impit<impit::cookie::Jar>,
}

/// The fingerprint the browser profile presents.
///
/// **All of it is [`crate::page`]'s now**, ciphers, groups, signature
/// algorithms, extension order, HTTP/2 settings and headers, where the TLS and
/// HTTP/2 halves used to be `impit`'s Chrome 151 database entry. `TODO/RULES.md`
/// section 6b is the ruling: the vendored database is a starting point rather
/// than an authority, and it has already been wrong here. This is one line so
/// that there is exactly one place a profile comes from.
fn browser_fingerprint() -> impit::fingerprint::BrowserFingerprint {
    crate::page::browser_fingerprint()
}

/// Extra roots the source-document fetcher will trust, as PEM, from the path
/// in `BIT_CLI_EXTRA_CA_FILE`.
///
/// **This adds a root, it never removes one.** The platform roots and the
/// bundled `webpki` roots are still there and a certificate still has to
/// verify against one of them. It exists because
/// `scripts/check-fingerprint.ps1` has to complete a real handshake against
/// `loopback-tlsprobe` to read the HTTP/2 half of this client's fingerprint,
/// and the alternative is a flag that stops verifying certificates, which is
/// not something to put in a shipping binary for a test.
///
/// It is an environment variable rather than a flag for the same reason
/// `SSL_CERT_FILE` is one: it is an operator's trust decision about the whole
/// process, not a per-run option, and a flag would put it in the help of nine
/// commands where somebody would reach for it.
pub const EXTRA_CA_FILE_ENV: &str = "BIT_CLI_EXTRA_CA_FILE";

fn extra_roots() -> Result<Vec<Vec<u8>>, FetchError> {
    let Some(path) = std::env::var_os(EXTRA_CA_FILE_ENV) else {
        return Ok(Vec::new());
    };
    let pem = std::fs::read(&path).map_err(|e| {
        FetchError::Build(format!(
            "{EXTRA_CA_FILE_ENV} names {} and it cannot be read: {e}",
            std::path::Path::new(&path).display()
        ))
    })?;
    use rustls_pki_types::pem::PemObject;
    let mut reader = std::io::BufReader::new(pem.as_slice());
    let mut roots = Vec::new();
    for item in rustls_pki_types::CertificateDer::pem_reader_iter(&mut reader) {
        let der = item.map_err(|e| {
            FetchError::Build(format!(
                "{EXTRA_CA_FILE_ENV} names {} and it is not a PEM certificate bundle: {e}",
                std::path::Path::new(&path).display()
            ))
        })?;
        roots.push(der.to_vec());
    }
    if roots.is_empty() {
        return Err(FetchError::Build(format!(
            "{EXTRA_CA_FILE_ENV} names {} and it holds no certificate",
            std::path::Path::new(&path).display()
        )));
    }
    tracing::warn!(
        path = %std::path::Path::new(&path).display(),
        count = roots.len(),
        "trusting extra certificate roots named by {EXTRA_CA_FILE_ENV}"
    );
    Ok(roots)
}

impl BrowserFetcher {
    pub fn new(identity: &Identity, deadline: Duration) -> Result<Self, FetchError> {
        let mut builder = impit::impit::Impit::<impit::cookie::Jar>::builder()
            .with_fingerprint(browser_fingerprint())
            .with_http2_header_table_size(Some(BROWSER_H2_HEADER_TABLE_SIZE))
            .with_http2_omit_max_frame_size(BROWSER_H2_OMIT_MAX_FRAME_SIZE)
            .with_http2_stream_priority(Some(impit::h2_ext::StreamPriority::new(
                BROWSER_H2_STREAM_PRIORITY.0,
                BROWSER_H2_STREAM_PRIORITY.1,
                BROWSER_H2_STREAM_PRIORITY.2,
            )))
            .with_default_timeout(deadline)
            // A source document is one hop. Following a redirect is still one
            // document, and a mirror answering 302 is the normal case, so the
            // default of ten stands.
            .with_redirect(impit::impit::RedirectBehavior::FollowRedirect(10));
        // Somebody who passed `--web-seed-user-agent` meant it, and silently
        // sending Chrome's instead would be the tool disagreeing with its own
        // flag.
        if identity.user_agent_given {
            builder = builder.with_headers(vec![(
                "user-agent".to_string(),
                identity.user_agent.clone(),
            )]);
        }
        let roots = extra_roots()?;
        if !roots.is_empty() {
            builder = builder.with_extra_roots(roots);
        }
        let client = builder
            .build()
            .map_err(|e| FetchError::Build(e.to_string()))?;
        Ok(Self { client })
    }
}

#[async_trait::async_trait]
impl Fetcher for BrowserFetcher {
    fn profile(&self) -> ClientProfile {
        ClientProfile::Browser
    }

    async fn get(&self, request: FetchRequest<'_>) -> Result<FetchResponse, FetchError> {
        let mut response = self
            .client
            .get(request.url.to_string(), None, None)
            .await
            .map_err(|e| browser_error(&e, request.url, "cannot fetch", request.deadline))?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            return Err(FetchError::Status {
                url: request.url.to_string(),
                status,
            });
        }
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_ascii_lowercase(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect::<Vec<_>>();
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| {
            browser_error(
                &impit::errors::ImpitError::from(e, None),
                request.url,
                "cannot read the body of",
                request.deadline,
            )
        })? {
            body.extend_from_slice(&chunk);
            if body.len() > request.max_bytes {
                return Err(FetchError::TooLarge {
                    url: request.url.to_string(),
                    max_bytes: request.max_bytes,
                });
            }
        }
        Ok(finish(status, headers, body))
    }
}

fn browser_error(
    err: &impit::errors::ImpitError,
    url: &str,
    what: &str,
    deadline: Duration,
) -> FetchError {
    use impit::errors::ImpitError;
    match err {
        ImpitError::TimeoutException(_)
        | ImpitError::ConnectTimeout
        | ImpitError::ReadTimeout
        | ImpitError::WriteTimeout
        | ImpitError::PoolTimeout => FetchError::Timeout {
            url: url.to_string(),
            deadline,
        },
        ImpitError::HTTPStatusError(status) => FetchError::Status {
            url: url.to_string(),
            status: *status,
        },
        other => FetchError::Network {
            url: url.to_string(),
            what: what.to_string(),
            detail: other.to_string(),
        },
    }
}

/// The one place a body and its headers become a response, so the two clients
/// cannot disagree about what `content_type` means.
fn finish(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> FetchResponse {
    let content_type = headers
        .iter()
        .find(|(name, _)| name == "content-type")
        .map(|(_, value)| value.clone());
    FetchResponse {
        status,
        headers,
        content_type,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_identity_says_plain() {
        let id = Identity::plain("bit-cli/0.2.0");
        assert_eq!(id.profile, ClientProfile::Plain);
        assert!(id.user_agent_given);
    }

    #[test]
    fn a_response_finds_a_header_case_insensitively() {
        let r = finish(
            200,
            vec![
                ("content-type".to_string(), "text/html".to_string()),
                ("x-cache".to_string(), "HIT".to_string()),
            ],
            Vec::new(),
        );
        assert_eq!(r.content_type.as_deref(), Some("text/html"));
        assert_eq!(r.header("X-Cache"), Some("HIT"));
        assert_eq!(r.header("x-missing"), None);
    }

    #[test]
    fn the_browser_fingerprint_carries_this_trees_header_list() {
        let fingerprint = browser_fingerprint();
        let names: Vec<&str> = fingerprint
            .headers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        let want: Vec<&str> = crate::page::BROWSER_HEADERS
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(names, want);
    }

    #[test]
    fn the_client_takes_the_whole_profile_from_page_and_nothing_from_the_database() {
        // What this used to assert was that our TLS half equals `impit`'s
        // `chrome_151` entry, which made the vendored database the authority.
        // TODO/RULES.md section 6b rules the other way and T-264 moved the
        // profile here, so the assertion is now that this client presents
        // exactly what `page.rs` declares. A bump edits that file and this
        // test follows it without being touched.
        let ours = browser_fingerprint();
        let declared = crate::page::browser_fingerprint();
        assert_eq!(ours.tls, declared.tls);
        assert_eq!(
            ours.http2.pseudo_header_order,
            declared.http2.pseudo_header_order
        );
        assert_eq!(ours.headers, declared.headers);
        assert_eq!(ours.version, crate::page::BROWSER_MAJOR.to_string());
    }

    #[test]
    fn every_error_says_which_url() {
        let e = FetchError::Status {
            url: "http://example.invalid/x".to_string(),
            status: 403,
        };
        assert!(e.to_string().contains("http://example.invalid/x"));
        assert!(e.to_string().contains("403"));
    }

    #[test]
    fn a_timeout_names_the_flag_that_sets_it() {
        let e = FetchError::Timeout {
            url: "http://example.invalid/x".to_string(),
            deadline: Duration::from_millis(1500),
        };
        assert!(e.to_string().contains("1500ms"));
        assert!(e.to_string().contains("--timeout"));
    }

    #[test]
    fn no_extra_roots_without_the_environment_variable() {
        // The variable is not set in a test run, and an absent one is not an
        // error: it is the shipping case.
        if std::env::var_os(EXTRA_CA_FILE_ENV).is_none() {
            assert!(extra_roots().expect("absent is fine").is_empty());
        }
    }
}
