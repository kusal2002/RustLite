//! OS sandboxing for renderer processes (ADR-0005, ADR-0006).
//!
//! On Windows:
//! - The browser puts every renderer in a [`RendererJob`]: a job object
//!   that caps each renderer's committed memory, allows no child
//!   processes, blocks clipboard/desktop/global-atom/system-setting access,
//!   kills renderers when the browser exits (or crashes), and turns
//!   unhandled exceptions into immediate exits.
//! - Each renderer calls [`lockdown`] before reading anything from the
//!   browser: it lowers its own token to low integrity (it can no longer
//!   write to the user's files, registry or medium-integrity processes),
//!   disables all privileges, and turns on the dynamic-code and image-load
//!   mitigations (no executable memory, no DLLs from remote or
//!   low-integrity locations).
//!
//! Elsewhere these are no-ops that report "not sandboxed" (Linux seccomp and
//! namespaces and the macOS sandbox are follow-ups).
//!
//! This is the only crate where `unsafe` is allowed; every block calls one
//! Win32 function with arguments checked as described in its SAFETY note.

use std::io;
use std::process::Child;

#[cfg(windows)]
mod windows;

/// What [`lockdown`] achieved (each step is attempted independently).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lockdown {
    pub low_integrity: bool,
    pub privileges_removed: bool,
    pub dynamic_code_blocked: bool,
    pub image_load_restricted: bool,
}

impl Lockdown {
    pub fn complete(&self) -> bool {
        self.low_integrity
            && self.privileges_removed
            && self.dynamic_code_blocked
            && self.image_load_restricted
    }
}

/// Default per-renderer commit limit.
pub const DEFAULT_RENDERER_MEMORY: usize = 1024 * 1024 * 1024;

/// The job object one renderer runs in (see the module docs). Dropping it
/// kills the renderer, so keep it as long as the renderer should live.
#[derive(Debug)]
pub struct RendererJob {
    #[cfg(windows)]
    inner: windows::Job,
}

impl RendererJob {
    /// Creates the job with a per-process memory limit of `memory_limit`
    /// bytes. On platforms without job objects this succeeds and does nothing.
    pub fn new(memory_limit: usize) -> io::Result<Self> {
        Ok(Self {
            #[cfg(windows)]
            inner: windows::Job::new(memory_limit)?,
        })
    }

    /// Puts a freshly spawned renderer in the job (before it receives input).
    pub fn assign(&self, child: &Child) -> io::Result<()> {
        #[cfg(windows)]
        {
            self.inner.assign(child)
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            Ok(())
        }
    }
}

/// Locks down the current (renderer) process. Call before handling any
/// untrusted input.
pub fn lockdown() -> Lockdown {
    #[cfg(windows)]
    {
        windows::lockdown()
    }
    #[cfg(not(windows))]
    {
        Lockdown::default()
    }
}

/// The current process's integrity level RID (0x1000 low, 0x2000 medium,
/// 0x3000 high), if the platform has one.
pub fn integrity_level() -> Option<u32> {
    #[cfg(windows)]
    {
        windows::integrity_level()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Whether the current process runs inside a job object (`None` if unknown).
pub fn in_job() -> Option<bool> {
    #[cfg(windows)]
    {
        windows::in_job()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Is OS sandboxing implemented on this platform?
pub const SUPPORTED: bool = cfg!(windows);
