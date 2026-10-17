//! OS sandboxing for renderer processes (ADR-0005, ADR-0006).
//!
//! The browser starts renderers with [`launch`]; each renderer calls
//! [`lockdown`] before it reads any input. Together they confine a renderer
//! to computing on what the browser sends it: no files, no network, no new
//! processes. (Renderers get fonts and local files from the browser over
//! IPC, so they need none of these.)
//!
//! - **Windows:** the renderer is created suspended with a restricted
//!   primary token (all groups deny-only, the NULL SID as the only
//!   restricting SID, low integrity, no privileges), assigned to its own job
//!   object (memory cap, no child processes, UI restrictions, killed with
//!   the browser), and resumed under a same-access warm-up impersonation
//!   token so the loader can map DLLs. [`lockdown`] turns on the
//!   dynamic-code and image-load mitigations and drops the warm-up token
//!   (`RevertToSelf`); from then on only the restricted token applies.
//! - **Linux:** [`lockdown`] sets `no_new_privs`, applies a Landlock ruleset
//!   that handles every filesystem (and, where supported, TCP) access right
//!   with no rules — so none is granted — and installs a default-deny
//!   seccomp-bpf filter: only the system calls a renderer needs (pipe I/O,
//!   memory without `PROT_EXEC`, threads, futexes, signals to itself,
//!   clocks, randomness) are allowed, and the rest fail with `EPERM`.
//! - **macOS:** [`lockdown`] enters a `sandbox_init` profile that denies
//!   reading and writing files outside system library paths, all network
//!   access, and process creation.
//!
//! This is the only crate where `unsafe` is allowed; every block calls one
//! OS function with arguments checked as described in its SAFETY note.

use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::path::Path;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// What [`lockdown`] achieved. Each part is attempted independently and
/// reported, so a partial sandbox is visible in the logs and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lockdown {
    /// Opening files is denied (restricted token, Landlock, or the macOS
    /// profile).
    pub filesystem: bool,
    /// Creating network connections is denied.
    pub network: bool,
    /// Starting processes is denied (job limit, seccomp, or the profile).
    pub processes: bool,
    /// Privileges are gone and cannot come back (no privileges and low
    /// integrity on Windows; `no_new_privs` on Linux; sandboxed processes
    /// cannot leave the sandbox on macOS).
    pub privileges: bool,
    /// Integrity level RID on Windows (0x1000 low), read before the
    /// warm-up token is dropped.
    pub integrity: Option<u32>,
}

impl Lockdown {
    pub fn complete(&self) -> bool {
        self.filesystem && self.network && self.processes && self.privileges
    }
}

/// Default per-renderer memory limit (Windows job commit limit).
pub const DEFAULT_RENDERER_MEMORY: usize = 1024 * 1024 * 1024;

/// Is OS sandboxing implemented on this platform?
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "linux", target_os = "macos"));

/// A running renderer process. Dropping it kills the process.
#[derive(Debug)]
pub struct Process {
    #[cfg(windows)]
    inner: windows::Process,
    #[cfg(not(windows))]
    inner: std::process::Child,
}

impl Process {
    pub fn id(&self) -> u32 {
        self.inner.id()
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.inner.kill()
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.inner.kill();
        let _ = self.inner.wait();
    }
}

/// A launched renderer: its process and the pipe ends connected to its
/// stdin and stdout. Its stderr is the browser's.
pub struct Launched {
    pub process: Process,
    pub stdin: Box<dyn Write + Send>,
    pub stdout: Box<dyn Read + Send>,
}

impl std::fmt::Debug for Launched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launched")
            .field("process", &self.process)
            .finish_non_exhaustive()
    }
}

/// Starts `exe args…` as a sandboxed renderer (see the module docs) with
/// `memory_limit` bytes of memory where the platform can enforce it.
pub fn launch(exe: &Path, args: &[&OsStr], memory_limit: usize) -> io::Result<Launched> {
    #[cfg(windows)]
    {
        windows::launch(exe, args, memory_limit)
    }
    #[cfg(not(windows))]
    {
        use std::process::{Command, Stdio};
        let _ = memory_limit;
        let mut child = Command::new(exe)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("no stdout"))?;
        Ok(Launched {
            process: Process { inner: child },
            stdin: Box::new(stdin),
            stdout: Box::new(stdout),
        })
    }
}

/// Locks down the current (renderer) process. Call once, after startup and
/// before handling any untrusted input; nothing may open files afterwards.
pub fn lockdown() -> Lockdown {
    #[cfg(windows)]
    {
        windows::lockdown()
    }
    #[cfg(target_os = "linux")]
    {
        linux::lockdown()
    }
    #[cfg(target_os = "macos")]
    {
        macos::lockdown()
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Lockdown::default()
    }
}

/// Calls a system call the Linux renderer filter does not list and reports
/// whether it failed with `EPERM`, as every unlisted call should after
/// [`lockdown`]. `None` on other platforms.
pub fn unlisted_syscall_denied() -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        Some(linux::unlisted_syscall_denied())
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}
