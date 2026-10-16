//! Navigation: a [`Tab`] drives a [`Page`] through loads, history and errors.
//!
//! State machine:
//!
//! ```text
//!   Idle ──navigate──► Loading{id} ──result(id)──► Idle (committed or error page)
//!     ▲                    │  └──navigate──► Loading{id'}   (old id cancelled;
//!     └────────stop────────┘                                its late result is ignored)
//! ```
//!
//! Every network load carries an id. Results whose id is not the pending
//! navigation's (or a subresource of the committed document) are dropped,
//! so a slow, superseded load can never replace a newer page.
//!
//! URL policy: `http(s)` loads through the [`Loader`]; `file:` only when the
//! user initiated it (address bar, command line, history), never from page
//! links; `about:blank` is built in; anything else is refused.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use rustlite_dom::NodeId;
use rustlite_net::{
    CacheMode, CancelToken, Destination, LoadResult, Loader, NetError, Request, Url,
};

use crate::Page;

/// Who asked for a navigation; decides which URL schemes are allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Initiator {
    /// Address bar, command line, history buttons.
    User,
    /// A link in the page.
    Page,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavState {
    Idle,
    Loading { url: Url },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NavigationError {
    #[error("`{0}` is not a valid URL")]
    InvalidUrl(String),
    #[error("navigation to `{0}` was blocked")]
    Blocked(String),
}

/// Local documents larger than this are refused.
const MAX_LOCAL_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryMode {
    /// A new entry after the current one (forward entries are dropped).
    Push,
    /// Re-visit an existing entry (back, forward, reload).
    Traverse(usize),
}

#[derive(Debug)]
struct Pending {
    id: u64,
    url: Url,
    cancel: CancelToken,
    mode: HistoryMode,
}

#[derive(Debug)]
struct Subresource {
    destination: Destination,
    nodes: Vec<NodeId>,
}

/// One browsing context: a page plus navigation and history.
#[derive(Debug)]
pub struct Tab {
    page: Page,
    loader: Option<Arc<Loader>>,
    next_id: u64,
    pending: Option<Pending>,
    subresources: HashMap<u64, Subresource>,
    subresource_cancel: CancelToken,
    history: Vec<Url>,
    index: Option<usize>,
    /// The pending navigation is a reload (subresources skip the cache).
    reloading: bool,
}

/// The tab id space a load id belongs to (see [`Tab::with_id_space`]).
pub fn load_id_space(id: u64) -> u32 {
    (id >> 32) as u32
}

impl Drop for Tab {
    /// A closed tab's loads are cancelled so they stop using the shared loader.
    fn drop(&mut self) {
        self.stop();
        self.subresource_cancel.cancel();
    }
}

/// Interprets address-bar input: full URLs as typed, filesystem paths as
/// `file:` URLs, `localhost[:port]` as `http:`, and anything else as an
/// `https:` host.
pub fn parse_user_input(input: &str) -> Result<Url, NavigationError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(NavigationError::InvalidUrl(input.into()));
    }
    let path = std::path::Path::new(input);
    let looks_like_path = path.is_absolute()
        || input.starts_with("./")
        || input.starts_with(".\\")
        || (cfg!(windows)
            && input.len() > 2
            && input.as_bytes()[1] == b':'
            && input.as_bytes()[0].is_ascii_alphabetic());
    if looks_like_path {
        let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        return Url::from_file_path(&absolute)
            .map_err(|()| NavigationError::InvalidUrl(input.into()));
    }
    if let Ok(url) = Url::parse(input)
        && url.has_host_or_opaque()
    {
        return Ok(url);
    }
    let scheme = if input.starts_with("localhost") || input.starts_with("127.0.0.1") {
        "http"
    } else {
        "https"
    };
    Url::parse(&format!("{scheme}://{input}"))
        .map_err(|_| NavigationError::InvalidUrl(input.into()))
}

trait HasHostOrOpaque {
    fn has_host_or_opaque(&self) -> bool;
}

impl HasHostOrOpaque for Url {
    /// Rejects `example.com:8080`-style input that parses as scheme
    /// `example.com` with an opaque path, while accepting `about:blank`.
    fn has_host_or_opaque(&self) -> bool {
        self.has_host() || matches!(self.scheme(), "about" | "data" | "javascript" | "file")
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The HTML of RustLite's error page.
pub fn error_page_html(url: &str, code: &str, message: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><title>Page not available</title><style>\
         body {{ font-family: sans-serif; background: #f6f6f6; color: #333; margin: 0 }}\
         main {{ width: 560px; margin: 80px auto; background: white; border: 1px solid #ddd; padding: 24px 32px }}\
         h1 {{ font-size: 22px; margin: 0 0 12px }} .url {{ color: #555 }} .code {{ color: #999; font-size: 13px }}\
         </style></head><body><main><h1>This page couldn't be loaded</h1>\
         <p class=url>{}</p><p>{}</p><p class=code>{}</p></main></body></html>",
        escape(url),
        escape(message),
        escape(code)
    )
}

fn plain_text_html(text: &str) -> String {
    format!(
        "<!DOCTYPE html><pre style=\"white-space: pre-wrap\">{}</pre>",
        escape(text)
    )
}

impl Tab {
    /// A tab showing `page`. Without a loader, only `file:` and `about:`
    /// URLs can be opened.
    pub fn new(page: Page, loader: Option<Arc<Loader>>) -> Self {
        Self::with_id_space(page, loader, 0)
    }

    /// Like [`Tab::new`], but load ids carry `space` in their high 32 bits so
    /// several tabs can share one loader: route a [`LoadResult`] to the tab
    /// whose space is [`load_id_space`]`(result.id)`.
    pub fn with_id_space(page: Page, loader: Option<Arc<Loader>>, space: u32) -> Self {
        Self {
            page,
            loader,
            next_id: (u64::from(space) << 32) | 1,
            pending: None,
            subresources: HashMap::new(),
            subresource_cancel: CancelToken::new(),
            history: Vec::new(),
            index: None,
            reloading: false,
        }
    }

    /// The loader this tab fetches with (shared between tabs).
    pub fn loader(&self) -> Option<&Arc<Loader>> {
        self.loader.as_ref()
    }

    pub fn page(&self) -> &Page {
        &self.page
    }

    pub fn page_mut(&mut self) -> &mut Page {
        &mut self.page
    }

    pub fn state(&self) -> NavState {
        match &self.pending {
            Some(p) => NavState::Loading { url: p.url.clone() },
            None => NavState::Idle,
        }
    }

    /// The committed URL (`None` before the first navigation).
    pub fn url(&self) -> Option<&Url> {
        self.index.and_then(|i| self.history.get(i))
    }

    pub fn can_go_back(&self) -> bool {
        self.index.is_some_and(|i| i > 0)
    }

    pub fn can_go_forward(&self) -> bool {
        self.index.is_some_and(|i| i + 1 < self.history.len())
    }

    /// Number of subresource loads still in flight for the current document.
    pub fn pending_subresources(&self) -> usize {
        self.subresources.len()
    }

    /// The id space given to [`Tab::with_id_space`].
    pub fn id_space(&self) -> u32 {
        load_id_space(self.next_id)
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        // Stay inside this tab's space (2^32 loads per tab before wrapping).
        let seq = (id as u32).wrapping_add(1).max(1);
        self.next_id = (id & !0xFFFF_FFFF) | u64::from(seq);
        id
    }

    /// Navigates to user-typed or command-line input.
    pub fn navigate(&mut self, input: &str) -> Result<(), NavigationError> {
        let url = parse_user_input(input)?;
        self.navigate_to(url, Initiator::User)
    }

    /// Navigates to `url` on behalf of `initiator`.
    pub fn navigate_to(&mut self, url: Url, initiator: Initiator) -> Result<(), NavigationError> {
        self.start(url, HistoryMode::Push, initiator)
    }

    pub fn back(&mut self) -> bool {
        self.traverse(-1)
    }

    pub fn forward(&mut self) -> bool {
        self.traverse(1)
    }

    /// Reloads the current entry; its subresources bypass the HTTP cache.
    pub fn reload(&mut self) -> bool {
        let started = self.traverse(0);
        self.reloading = started;
        started
    }

    fn traverse(&mut self, delta: isize) -> bool {
        let Some(i) = self.index.and_then(|i| i.checked_add_signed(delta)) else {
            return false;
        };
        let Some(url) = self.history.get(i).cloned() else {
            return false;
        };
        // History entries were reached legitimately, so revisiting them is
        // user-initiated (this is what lets Back return to a file: page).
        self.start(url, HistoryMode::Traverse(i), Initiator::User)
            .is_ok()
    }

    /// Cancels the pending navigation, if any.
    pub fn stop(&mut self) {
        if let Some(p) = self.pending.take() {
            p.cancel.cancel();
        }
    }

    fn start(
        &mut self,
        url: Url,
        mode: HistoryMode,
        initiator: Initiator,
    ) -> Result<(), NavigationError> {
        self.reloading = false;
        match url.scheme() {
            "http" | "https" => {
                self.stop();
                let id = self.next_id();
                let cancel = CancelToken::new();
                let queued = match &self.loader {
                    Some(loader) => loader
                        .load(
                            id,
                            Request {
                                url: url.clone(),
                                destination: Destination::Document,
                                cache: CacheMode::Default,
                            },
                            cancel.clone(),
                        )
                        .map_err(|e| e.to_string()),
                    None => Err("networking is not available".to_owned()),
                };
                match queued {
                    Ok(()) => {
                        tracing::info!(%url, id, "navigation started");
                        self.pending = Some(Pending {
                            id,
                            url,
                            cancel,
                            mode,
                        });
                    }
                    Err(message) => self.commit_error(url, mode, "ERR_FAILED", &message),
                }
                Ok(())
            }
            "file" if initiator == Initiator::User => {
                self.stop();
                self.load_local_file(url, mode);
                Ok(())
            }
            "about" if url.as_str() == "about:blank" => {
                self.stop();
                self.page.load_html("about:blank", "");
                self.commit(url, mode, false);
                Ok(())
            }
            scheme => {
                if initiator == Initiator::Page {
                    tracing::warn!(%url, "page navigation blocked");
                    return Err(NavigationError::Blocked(url.to_string()));
                }
                self.stop();
                let message = format!("RustLite cannot open `{scheme}:` URLs.");
                self.commit_error(url, mode, "ERR_UNSUPPORTED_SCHEME", &message);
                Ok(())
            }
        }
    }

    fn load_local_file(&mut self, url: Url, mode: HistoryMode) {
        let read = url
            .to_file_path()
            .map_err(|()| "not a local path".to_owned())
            .and_then(|path| {
                let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
                if size > MAX_LOCAL_DOCUMENT_BYTES {
                    return Err(format!(
                        "file is larger than {MAX_LOCAL_DOCUMENT_BYTES} bytes"
                    ));
                }
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                Ok((path, bytes))
            });
        match read {
            Ok((path, bytes)) => {
                let text = rustlite_net::decode_html(&bytes, None).text;
                self.page
                    .load_html_with_base(url.as_str(), &text, path.parent());
                self.commit(url, mode, false);
            }
            Err(message) => self.commit_error(url, mode, "ERR_FILE_NOT_FOUND", &message),
        }
    }

    fn commit_error(&mut self, url: Url, mode: HistoryMode, code: &str, message: &str) {
        tracing::warn!(%url, code, message, "navigation failed");
        self.page
            .load_html(url.as_str(), &error_page_html(url.as_str(), code, message));
        self.commit(url, mode, false);
    }

    /// Makes the page's document current: updates history and (for network
    /// documents) starts loading subresources.
    fn commit(&mut self, url: Url, mode: HistoryMode, load_subresources: bool) {
        match mode {
            HistoryMode::Push => {
                let keep = self.index.map_or(0, |i| i + 1);
                self.history.truncate(keep);
                self.history.push(url);
                self.index = Some(self.history.len() - 1);
            }
            HistoryMode::Traverse(i) => {
                self.history[i] = url;
                self.index = Some(i);
            }
        }
        // Subresources of the previous document are no longer wanted.
        self.subresource_cancel.cancel();
        self.subresource_cancel = CancelToken::new();
        self.subresources.clear();
        if load_subresources {
            self.request_subresources();
        }
    }

    fn request_subresources(&mut self) {
        let Some(loader) = self.loader.clone() else {
            return;
        };
        let requests = self.page.subresource_requests();
        for (destination, url, nodes) in requests {
            let id = self.next_id();
            let request = Request {
                url,
                destination,
                cache: if self.reloading {
                    CacheMode::Reload
                } else {
                    CacheMode::Default
                },
            };
            match loader.load(id, request, self.subresource_cancel.clone()) {
                Ok(()) => {
                    self.subresources
                        .insert(id, Subresource { destination, nodes });
                }
                Err(e) => tracing::warn!(error = %e, "subresource not requested"),
            }
        }
    }

    /// Applies a finished load. Returns true if the page changed and should
    /// be redrawn. Results for superseded navigations or documents are
    /// ignored.
    pub fn on_load_result(&mut self, r: LoadResult) -> bool {
        if self.pending.as_ref().is_some_and(|p| p.id == r.id) {
            let pending = self.pending.take().expect("checked above");
            self.finish_navigation(pending, r.result);
            return true;
        }
        if let Some(sub) = self.subresources.remove(&r.id) {
            return self.apply_subresource(sub, r);
        }
        tracing::debug!(id = r.id, url = %r.request.url, "stale load result ignored");
        false
    }

    fn finish_navigation(
        &mut self,
        pending: Pending,
        result: Result<rustlite_net::Response, NetError>,
    ) {
        let response = match result {
            Ok(r) => r,
            Err(NetError::Cancelled) => return,
            Err(e) => {
                let message = e.to_string();
                return self.commit_error(pending.url, pending.mode, e.code(), &message);
            }
        };
        let mime = response.mime.as_deref().unwrap_or("text/html");
        let html = match mime {
            "text/html" | "application/xhtml+xml" => {
                rustlite_net::decode_html(&response.body, response.charset.as_deref()).text
            }
            m if m.starts_with("text/") || m == "application/json" => plain_text_html(
                &rustlite_net::decode_text(&response.body, response.charset.as_deref()).text,
            ),
            m if m.starts_with("image/") => format!(
                "<!DOCTYPE html><body style=\"margin: 0\"><img src=\"{}\">",
                escape(response.url.as_str())
            ),
            other => {
                let message = format!("RustLite cannot display `{other}` content.");
                return self.commit_error(
                    response.url,
                    pending.mode,
                    "ERR_UNSUPPORTED_CONTENT",
                    &message,
                );
            }
        };
        tracing::info!(url = %response.url, status = response.status, bytes = response.body.len(), "navigation committed");
        self.page.load_html(response.url.as_str(), &html);
        self.commit(response.url, pending.mode, true);
    }

    fn apply_subresource(&mut self, sub: Subresource, r: LoadResult) -> bool {
        let response = match r.result {
            Ok(resp) if (200..300).contains(&resp.status) => resp,
            Ok(resp) => {
                tracing::warn!(url = %r.request.url, status = resp.status, "subresource failed");
                return false;
            }
            Err(e) => {
                tracing::warn!(url = %r.request.url, error = %e, "subresource failed");
                return false;
            }
        };
        match sub.destination {
            Destination::Style => {
                let css =
                    rustlite_net::decode_text(&response.body, response.charset.as_deref()).text;
                for node in sub.nodes {
                    self.page.set_external_stylesheet(node, &css);
                }
                true
            }
            Destination::Image => match crate::images::decode_bytes(&response.body) {
                Ok(image) => {
                    let image = Rc::new(image);
                    for node in sub.nodes {
                        self.page.set_image(node, image.clone());
                    }
                    true
                }
                Err(e) => {
                    tracing::warn!(url = %r.request.url, error = ?e, "image not decoded");
                    false
                }
            },
            Destination::Document => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_input_parsing() {
        assert_eq!(
            parse_user_input("example.com").unwrap().as_str(),
            "https://example.com/"
        );
        assert_eq!(
            parse_user_input("example.com/a?b").unwrap().as_str(),
            "https://example.com/a?b"
        );
        assert_eq!(
            parse_user_input("http://x.test/").unwrap().as_str(),
            "http://x.test/"
        );
        assert_eq!(
            parse_user_input("localhost:8080").unwrap().as_str(),
            "http://localhost:8080/"
        );
        assert_eq!(
            parse_user_input("about:blank").unwrap().as_str(),
            "about:blank"
        );
        assert!(parse_user_input("   ").is_err());
        let file = std::env::temp_dir().join("rustlite-input-test.html");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(
            parse_user_input(file.to_str().unwrap()).unwrap().scheme(),
            "file"
        );
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn error_page_escapes_its_inputs() {
        let html = error_page_html("http://x/<script>", "ERR_X", "a & b");
        assert!(html.contains("http://x/&lt;script&gt;"));
        assert!(html.contains("a &amp; b"));
        assert!(!html.contains("<script>"));
    }

    fn tab() -> Tab {
        Tab::new(
            Page::with_fonts(
                rustlite_paint::Size::new(800, 600),
                rustlite_fonts::FontContext::new(),
            ),
            None,
        )
    }

    #[test]
    fn about_blank_and_unsupported_schemes() {
        let mut t = tab();
        t.navigate("about:blank").unwrap();
        assert_eq!(t.url().unwrap().as_str(), "about:blank");
        t.navigate("ftp://example.com/").unwrap();
        assert_eq!(t.url().unwrap().scheme(), "ftp");
        assert!(t.page().title().unwrap().contains("not available"));
        assert!(t.can_go_back());
    }

    #[test]
    fn pages_cannot_navigate_to_files_or_scripts() {
        let mut t = tab();
        for u in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,x",
        ] {
            let url = Url::parse(u).unwrap();
            assert!(
                matches!(
                    t.navigate_to(url, Initiator::Page),
                    Err(NavigationError::Blocked(_))
                ),
                "{u}"
            );
        }
        assert!(
            t.url().is_none(),
            "blocked navigations leave the tab unchanged"
        );
    }

    #[test]
    fn local_files_history_back_and_forward() {
        let dir = std::env::temp_dir().join(format!("rustlite-tab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.html");
        let b = dir.join("b.html");
        std::fs::write(&a, "<title>A</title>").unwrap();
        std::fs::write(&b, "<title>B</title>").unwrap();
        let mut t = tab();
        t.navigate(a.to_str().unwrap()).unwrap();
        t.navigate(b.to_str().unwrap()).unwrap();
        assert_eq!(t.page().title().as_deref(), Some("B"));
        assert!(t.back());
        assert_eq!(t.page().title().as_deref(), Some("A"));
        assert!(!t.back(), "no entry before A");
        assert!(t.can_go_forward());
        assert!(t.forward());
        assert_eq!(t.page().title().as_deref(), Some("B"));
        // A new navigation from A drops the forward entry.
        t.back();
        t.navigate("about:blank").unwrap();
        assert!(!t.can_go_forward());
        // Missing files show an error page.
        t.navigate(dir.join("missing.html").to_str().unwrap())
            .unwrap();
        assert!(t.page().title().unwrap().contains("not available"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn http_without_a_loader_shows_an_error_page() {
        let mut t = tab();
        t.navigate("http://example.test/").unwrap();
        assert_eq!(t.state(), NavState::Idle);
        assert!(t.page().title().unwrap().contains("not available"));
    }
}
