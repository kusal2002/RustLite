//! Page scripts: collecting `<script>` elements, fetching their sources, and
//! running them in document order once every source is available.
//!
//! Execution model (simplified from HTML §8.1.4): scripts run after parsing,
//! in document order, as if all were `defer` (no `document.write`, no
//! parser-blocking). Then `DOMContentLoaded` fires at the document and `load`
//! at the window. A failed external script is skipped; the rest still run.

use rustlite_dom::{Document, NodeId};
use rustlite_script::ScriptHost;

/// Largest script source accepted (local file or network body).
pub const MAX_SCRIPT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    Ready(String),
    /// External script still being fetched.
    Pending,
    Failed,
}

#[derive(Debug)]
pub(crate) struct Entry {
    pub node: NodeId,
    pub source: Source,
    pub label: String,
}

#[derive(Debug, Default)]
pub(crate) struct Scripts {
    pub entries: Vec<Entry>,
    /// Created when the first script runs (a realm costs memory).
    pub host: Option<ScriptHost>,
    pub ran: bool,
}

impl Scripts {
    /// Classic scripts of `dom` in document order. External ones are read
    /// from `base_dir` for local documents (same directory policy as
    /// images), or left `Pending` for the network.
    pub fn collect(dom: &Document, base_dir: Option<&std::path::Path>) -> Self {
        let mut entries = Vec::new();
        for n in dom.descendants(dom.root()) {
            if !dom.is_element_named(n, "script")
                || !rustlite_script::is_classic_script_type(dom.attribute(n, "type"))
            {
                continue;
            }
            let (source, label) = match dom.attribute(n, "src") {
                None => (
                    Source::Ready(dom.text_content(n)),
                    "inline script".to_owned(),
                ),
                Some(src) => {
                    let source = match base_dir {
                        Some(dir) => read_local(dir, src),
                        None => Source::Pending,
                    };
                    (source, src.trim().to_owned())
                }
            };
            entries.push(Entry {
                node: n,
                source,
                label,
            });
        }
        Self {
            entries,
            host: None,
            ran: false,
        }
    }

    pub fn ready(&self) -> bool {
        !self.ran && self.entries.iter().all(|e| e.source != Source::Pending)
    }

    /// Nodes of external scripts that still need fetching.
    pub fn pending(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.entries
            .iter()
            .filter(|e| e.source == Source::Pending)
            .map(|e| e.node)
    }

    pub fn set_source(&mut self, node: NodeId, source: Option<String>) {
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.node == node && e.source == Source::Pending)
        {
            e.source = source.map_or(Source::Failed, Source::Ready);
        }
    }
}

fn read_local(base_dir: &std::path::Path, src: &str) -> Source {
    let bytes = crate::images::resolve_local(base_dir, src)
        .ok()
        .and_then(|path| {
            let size = std::fs::metadata(&path).ok()?.len();
            (size <= MAX_SCRIPT_BYTES).then(|| std::fs::read(&path).ok())?
        });
    match bytes {
        Some(b) => Source::Ready(rustlite_net::decode_text(&b, None).text),
        None => {
            tracing::warn!(src, "script not loaded");
            Source::Failed
        }
    }
}

/// May a response with this MIME type run as a script? Browsers refuse
/// image, audio, video and CSV bodies (Fetch §"should response to request
/// be blocked due to its MIME type").
pub fn script_mime_allowed(mime: Option<&str>) -> bool {
    !mime.is_some_and(|m| {
        m.starts_with("image/")
            || m.starts_with("audio/")
            || m.starts_with("video/")
            || m == "text/csv"
    })
}
