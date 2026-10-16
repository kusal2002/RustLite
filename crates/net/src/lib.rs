//! Networking for RustLite: HTTP(S) fetching with explicit security limits,
//! character decoding, and a bounded background loader.
//!
//! Security properties (see docs/security.md):
//! - Only `http:` and `https:` are fetched; redirects are followed manually
//!   (at most [`MAX_REDIRECTS`]) and every hop is scheme-checked, so a server
//!   cannot redirect into `file:` or other schemes.
//! - TLS uses rustls with the operating system's trust store
//!   (`rustls-platform-verifier`). Certificate validation cannot be disabled.
//! - Bodies are size-limited per destination; requests have timeouts and can
//!   be cancelled between chunks.

mod cache;
mod encoding;
mod loader;

use std::io::Read;
use std::time::Duration;

pub use cache::{CacheStats, DEFAULT_CACHE_BYTES, HttpCache, freshness};
pub use encoding::{DecodedText, decode_html, decode_text};
pub use loader::{CancelToken, LoadResult, Loader, LoaderError};
pub use url::Url;

/// Redirect hops followed before failing.
pub const MAX_REDIRECTS: u32 = 10;

/// What a response will be used for; selects the body size limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Destination {
    Document,
    Style,
    Image,
    Script,
}

impl Destination {
    pub fn max_body_bytes(self) -> u64 {
        match self {
            Destination::Document => 16 * 1024 * 1024,
            Destination::Style | Destination::Script => 4 * 1024 * 1024,
            Destination::Image => 32 * 1024 * 1024,
        }
    }

    fn accept(self) -> &'static str {
        match self {
            Destination::Document => "text/html,application/xhtml+xml;q=0.9,*/*;q=0.8",
            Destination::Style => "text/css,*/*;q=0.1",
            Destination::Image => "image/png,image/jpeg,image/gif,image/*;q=0.8,*/*;q=0.5",
            Destination::Script => "*/*",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: Url,
    pub destination: Destination,
    pub cache: CacheMode,
}

/// Whether a request may be answered from the HTTP cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheMode {
    /// Use a fresh cached response if there is one.
    #[default]
    Default,
    /// Always fetch (the user asked to reload); the result is still stored.
    Reload,
}

/// A completed HTTP response (any status; 404 pages have bodies too).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// URL after redirects.
    pub url: Url,
    pub status: u16,
    /// MIME type essence, lowercased (e.g. `text/html`).
    pub mime: Option<String>,
    /// `charset` parameter of `Content-Type`, if any.
    pub charset: Option<String>,
    pub body: Vec<u8>,
    /// URLs redirected through, in order.
    pub redirects: Vec<Url>,
    /// How long the response may be reused, from `Cache-Control`
    /// (`None`: not cacheable). See [`freshness`].
    pub cache_lifetime: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    #[error("unsupported URL scheme `{0}`")]
    UnsupportedScheme(String),
    #[error("redirected to a URL with unsupported scheme `{0}`")]
    RedirectToUnsupportedScheme(String),
    #[error("redirect without a valid Location")]
    BadRedirect,
    #[error("too many redirects (more than {MAX_REDIRECTS})")]
    TooManyRedirects,
    #[error("host not found")]
    HostNotFound,
    #[error("connection failed")]
    ConnectionFailed,
    #[error("timed out")]
    Timeout,
    #[error("certificate error: {0}")]
    Certificate(String),
    #[error("TLS error: {0}")]
    Tls(String),
    #[error("response body exceeds {0} bytes")]
    BodyTooLarge(u64),
    #[error("cancelled")]
    Cancelled,
    #[error("network error: {0}")]
    Other(String),
}

impl NetError {
    /// Short stable code for error pages and logs.
    pub fn code(&self) -> &'static str {
        match self {
            NetError::UnsupportedScheme(_) => "ERR_UNSUPPORTED_SCHEME",
            NetError::RedirectToUnsupportedScheme(_) => "ERR_UNSAFE_REDIRECT",
            NetError::BadRedirect => "ERR_INVALID_REDIRECT",
            NetError::TooManyRedirects => "ERR_TOO_MANY_REDIRECTS",
            NetError::HostNotFound => "ERR_NAME_NOT_RESOLVED",
            NetError::ConnectionFailed => "ERR_CONNECTION_FAILED",
            NetError::Timeout => "ERR_TIMED_OUT",
            NetError::Certificate(_) => "ERR_CERT_INVALID",
            NetError::Tls(_) => "ERR_SSL_PROTOCOL_ERROR",
            NetError::BodyTooLarge(_) => "ERR_RESPONSE_TOO_LARGE",
            NetError::Cancelled => "ERR_ABORTED",
            NetError::Other(_) => "ERR_FAILED",
        }
    }
}

fn map_error(e: ureq::Error) -> NetError {
    match e {
        ureq::Error::HostNotFound => NetError::HostNotFound,
        ureq::Error::ConnectionFailed => NetError::ConnectionFailed,
        ureq::Error::Timeout(_) => NetError::Timeout,
        ureq::Error::TooManyRedirects => NetError::TooManyRedirects,
        ureq::Error::BodyExceedsLimit(n) => NetError::BodyTooLarge(n),
        ureq::Error::Tls(msg) => NetError::Tls(msg.to_owned()),
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => NetError::Timeout,
        ureq::Error::Io(io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::ConnectionReset
            ) =>
        {
            NetError::ConnectionFailed
        }
        other => {
            // rustls errors are only reachable through ureq's wrapper; their
            // Display text identifies certificate problems.
            let text = other.to_string();
            if text.contains("certificate") || text.contains("Certificate") {
                NetError::Certificate(text)
            } else if text.starts_with("rustls") {
                NetError::Tls(text)
            } else {
                NetError::Other(text)
            }
        }
    }
}

/// Fetch settings.
#[derive(Debug, Clone)]
pub struct FetchConfig {
    pub timeout: Duration,
    pub connect_timeout: Duration,
    pub user_agent: String,
    /// Byte budget of the in-memory subresource cache (0 disables it).
    pub cache_bytes: usize,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            user_agent: format!(
                "RustLite/{} (+https://github.com/kusal2002/RustLite)",
                env!("CARGO_PKG_VERSION")
            ),
            cache_bytes: DEFAULT_CACHE_BYTES,
        }
    }
}

/// Performs HTTP(S) requests. Cheap to clone (shares a connection pool).
#[derive(Debug, Clone)]
pub struct Fetcher {
    agent: ureq::Agent,
}

fn is_fetchable(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// Splits `Content-Type` into (essence, charset).
pub fn parse_content_type(value: &str) -> (Option<String>, Option<String>) {
    let mut parts = value.split(';');
    let essence = parts
        .next()
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| e.contains('/'));
    let charset = parts.find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| v.trim().trim_matches('"').to_ascii_lowercase())
    });
    (essence, charset)
}

impl Fetcher {
    pub fn new(config: &FetchConfig) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            // Redirects are followed by `fetch` so each hop is policy-checked.
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(config.timeout))
            .timeout_connect(Some(config.connect_timeout))
            .user_agent(config.user_agent.as_str())
            .build()
            .new_agent();
        Self { agent }
    }

    /// Fetches `request`, following redirects, honoring `cancel`.
    pub fn fetch(&self, request: &Request, cancel: &CancelToken) -> Result<Response, NetError> {
        let mut url = request.url.clone();
        let mut redirects = Vec::new();
        loop {
            if cancel.is_cancelled() {
                return Err(NetError::Cancelled);
            }
            if !is_fetchable(&url) {
                return Err(if redirects.is_empty() {
                    NetError::UnsupportedScheme(url.scheme().to_owned())
                } else {
                    NetError::RedirectToUnsupportedScheme(url.scheme().to_owned())
                });
            }
            let mut response = self
                .agent
                .get(url.as_str())
                .header("Accept", request.destination.accept())
                .call()
                .map_err(map_error)?;
            let status = response.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or(NetError::BadRedirect)?;
                let next = url.join(location).map_err(|_| NetError::BadRedirect)?;
                if redirects.len() as u32 >= MAX_REDIRECTS {
                    return Err(NetError::TooManyRedirects);
                }
                tracing::debug!(from = %url, to = %next, status, "redirect");
                redirects.push(url);
                url = next;
                continue;
            }
            let (mime, charset) = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(parse_content_type)
                .unwrap_or((None, None));
            let cache_lifetime = freshness(
                response
                    .headers()
                    .get("cache-control")
                    .and_then(|v| v.to_str().ok()),
            );
            let limit = request.destination.max_body_bytes();
            let body = read_body(response.body_mut(), limit, cancel)?;
            return Ok(Response {
                url,
                status,
                mime,
                charset,
                body,
                redirects,
                cache_lifetime,
            });
        }
    }
}

fn read_body(body: &mut ureq::Body, limit: u64, cancel: &CancelToken) -> Result<Vec<u8>, NetError> {
    // Let ureq read one byte past the limit so an oversized body is detected
    // here rather than silently truncated.
    let mut reader = body.with_config().limit(limit + 1).reader();
    let mut out = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        if cancel.is_cancelled() {
            return Err(NetError::Cancelled);
        }
        let n = match reader.read(&mut chunk) {
            Ok(0) => return Ok(out),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(
                    match e
                        .into_inner()
                        .and_then(|i| i.downcast::<ureq::Error>().ok())
                    {
                        Some(ureq_error) => map_error(*ureq_error),
                        None => NetError::Other("body read failed".into()),
                    },
                );
            }
        };
        out.extend_from_slice(&chunk[..n]);
        if out.len() as u64 > limit {
            return Err(NetError::BodyTooLarge(limit));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_parsing() {
        assert_eq!(
            parse_content_type("Text/HTML; Charset=\"ISO-8859-1\""),
            (Some("text/html".into()), Some("iso-8859-1".into()))
        );
        assert_eq!(
            parse_content_type("image/png"),
            (Some("image/png".into()), None)
        );
        assert_eq!(parse_content_type("garbage"), (None, None));
    }

    #[test]
    fn non_http_schemes_are_refused_without_network() {
        let f = Fetcher::new(&FetchConfig::default());
        for u in [
            "file:///etc/passwd",
            "ftp://example.com/",
            "data:text/html,hi",
            "javascript:alert(1)",
        ] {
            let req = Request {
                url: Url::parse(u).unwrap(),
                destination: Destination::Document,
                cache: CacheMode::Default,
            };
            assert!(
                matches!(
                    f.fetch(&req, &CancelToken::new()),
                    Err(NetError::UnsupportedScheme(_))
                ),
                "{u}"
            );
        }
    }

    #[test]
    fn cancelled_before_start() {
        let f = Fetcher::new(&FetchConfig::default());
        let cancel = CancelToken::new();
        cancel.cancel();
        let req = Request {
            url: Url::parse("http://127.0.0.1:9/").unwrap(),
            destination: Destination::Document,
            cache: CacheMode::Default,
        };
        assert_eq!(f.fetch(&req, &cancel), Err(NetError::Cancelled));
    }
}
