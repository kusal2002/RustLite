//! Subresources of local (`file:`) documents: stylesheets, scripts and
//! images next to the document.
//!
//! The page asks a [`LocalFiles`] for them by the `href`/`src` text. In the
//! browser process (and in `--single-process` mode) [`DirFiles`] reads them
//! under the directory policy of [`crate::images::resolve_local`]; a
//! renderer process forwards the request to the browser, so it never needs
//! to open files itself.

use std::path::Path;

/// What a local subresource is for (each kind has its own size limit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalKind {
    Stylesheet,
    Script,
    Image,
}

impl LocalKind {
    pub fn max_bytes(self) -> u64 {
        match self {
            LocalKind::Stylesheet => 4 * 1024 * 1024,
            LocalKind::Script => crate::scripts::MAX_SCRIPT_BYTES,
            LocalKind::Image => crate::images::MAX_IMAGE_FILE_BYTES,
        }
    }

    pub(crate) fn to_u8(self) -> u8 {
        match self {
            LocalKind::Stylesheet => 0,
            LocalKind::Script => 1,
            LocalKind::Image => 2,
        }
    }

    pub(crate) fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => LocalKind::Stylesheet,
            1 => LocalKind::Script,
            2 => LocalKind::Image,
            _ => return None,
        })
    }
}

/// Supplies a local document's subresources by reference text.
pub trait LocalFiles {
    /// The bytes of `href`, or `None` if policy refuses it or it cannot be
    /// read.
    fn read(&mut self, kind: LocalKind, href: &str) -> Option<Vec<u8>>;
}

/// Reads subresources from a document's directory (the directory policy).
#[derive(Debug, Clone, Copy)]
pub struct DirFiles<'a>(pub &'a Path);

impl LocalFiles for DirFiles<'_> {
    fn read(&mut self, kind: LocalKind, href: &str) -> Option<Vec<u8>> {
        read_in_dir(self.0, kind, href)
    }
}

/// Reads `href` from `dir` if the directory policy and the size limit for
/// `kind` allow it.
pub fn read_in_dir(dir: &Path, kind: LocalKind, href: &str) -> Option<Vec<u8>> {
    let path = match crate::images::resolve_local(dir, href) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(href, ?kind, error = ?e, "local subresource refused");
            return None;
        }
    };
    let size = std::fs::metadata(&path).ok()?.len();
    if size > kind.max_bytes() {
        tracing::warn!(href, ?kind, size, "local subresource too large");
        return None;
    }
    std::fs::read(&path).ok()
}
