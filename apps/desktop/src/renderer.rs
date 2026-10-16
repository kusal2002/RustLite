//! Renderer processes: how the browser starts them, and the `--renderer`
//! entry point that runs inside them (ADR-0005, ADR-0006).

use std::process::{Command, Stdio};
use std::rc::Rc;

use rustlite_browser_core::{Spawned, Spawner};
use rustlite_sandbox::{DEFAULT_RENDERER_MEMORY, RendererJob};

/// The hidden command-line switch that turns this executable into a renderer.
pub const RENDERER_SWITCH: &str = "--renderer";
/// Test-only mode: lock down, then report what the sandbox allows.
pub const SELFTEST_SWITCH: &str = "--renderer-selftest";

/// Starts renderers as `<this executable> --renderer` with piped stdin and
/// stdout (the IPC channel) and inherited stderr (logs), each in its own
/// sandbox job, assigned before the renderer receives any input.
pub fn spawner() -> Spawner {
    Rc::new(|| {
        let exe = std::env::current_exe()?;
        let child = Command::new(exe)
            .arg(RENDERER_SWITCH)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let job = RendererJob::new(DEFAULT_RENDERER_MEMORY)?;
        if let Err(e) = job.assign(&child) {
            // Never run an unconfined renderer where confinement exists.
            let mut child = child;
            let _ = child.kill();
            return Err(e);
        }
        tracing::info!(
            pid = child.id(),
            sandboxed = rustlite_sandbox::SUPPORTED,
            "renderer process started"
        );
        Ok(Spawned {
            child,
            guard: Some(Box::new(job)),
        })
    })
}

/// The renderer process main: lock down, then serve the browser on
/// stdin/stdout.
pub fn run() -> std::io::Result<()> {
    let lockdown = rustlite_sandbox::lockdown();
    tracing::info!(?lockdown, integrity = ?rustlite_sandbox::integrity_level(), "renderer locked down");
    if rustlite_sandbox::SUPPORTED && !lockdown.complete() {
        tracing::warn!("renderer sandbox incomplete");
    }
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    rustlite_browser_core::serve(&mut stdin.lock(), &mut stdout.lock())
}

/// `--renderer-selftest <dir>`: waits for a line on stdin (so the parent
/// can confine it first), locks down, then tries what a compromised
/// renderer might and prints one `key=value` line per check.
pub fn selftest(dir: &std::path::Path) {
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    let lockdown = rustlite_sandbox::lockdown();
    println!("lockdown={}", lockdown.complete());
    println!(
        "integrity={}",
        rustlite_sandbox::integrity_level().map_or("unknown".into(), |l| format!("{l:#x}"))
    );
    println!("in_job={:?}", rustlite_sandbox::in_job());
    let target = dir.join("renderer-write-probe.txt");
    let write = std::fs::write(&target, b"x");
    println!("write={}", if write.is_ok() { "allowed" } else { "denied" });
    let _ = std::fs::remove_file(&target);
    let spawn = Command::new(std::env::current_exe().unwrap_or_default())
        .arg("--help")
        .stdout(Stdio::null())
        .spawn();
    println!("spawn={}", if spawn.is_ok() { "allowed" } else { "denied" });
    let read = std::fs::read_dir(dir).is_ok();
    println!("read={}", if read { "allowed" } else { "denied" });
}
