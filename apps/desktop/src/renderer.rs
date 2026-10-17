//! Renderer processes: how the browser starts them, and the `--renderer`
//! entry point that runs inside them (ADR-0005, ADR-0006).

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::rc::Rc;

use rustlite_browser_core::{RendererProcess, Spawned, Spawner};
use rustlite_sandbox::{DEFAULT_RENDERER_MEMORY, Lockdown};

/// The hidden command-line switch that turns this executable into a renderer.
pub const RENDERER_SWITCH: &str = "--renderer";
/// Test-only mode: lock down, then report what the sandbox allows.
pub const SELFTEST_SWITCH: &str = "--renderer-selftest";

struct Sandboxed(rustlite_sandbox::Process);

impl RendererProcess for Sandboxed {
    fn id(&self) -> u32 {
        self.0.id()
    }

    fn kill(&mut self) {
        // Dropping the process also kills and reaps it.
        let _ = self.0.kill();
    }
}

/// Starts `exe args…` in the OS sandbox ([`rustlite_sandbox::launch`]).
pub fn sandboxed_spawner(
    exe: impl AsRef<Path> + 'static,
    args: &'static [&'static str],
) -> Spawner {
    Rc::new(move || {
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        let launched = rustlite_sandbox::launch(exe.as_ref(), &args, DEFAULT_RENDERER_MEMORY)?;
        tracing::info!(
            pid = launched.process.id(),
            sandboxed = rustlite_sandbox::SUPPORTED,
            "renderer process started"
        );
        Ok(Spawned {
            process: Box::new(Sandboxed(launched.process)),
            stdin: launched.stdin,
            stdout: launched.stdout,
        })
    })
}

/// Starts renderers as `<this executable> --renderer`, sandboxed, with
/// stdin and stdout as the IPC channel and the browser's stderr for logs.
pub fn spawner() -> io::Result<Spawner> {
    Ok(sandboxed_spawner(
        std::env::current_exe()?,
        &[RENDERER_SWITCH],
    ))
}

/// The renderer process main: lock down, then serve the browser on
/// stdin/stdout.
pub fn run() -> io::Result<()> {
    let lockdown = rustlite_sandbox::lockdown();
    tracing::info!(?lockdown, "renderer locked down");
    if rustlite_sandbox::SUPPORTED && !lockdown.complete() {
        tracing::warn!(?lockdown, "renderer sandbox incomplete");
    }
    rustlite_browser_core::serve(io::stdin().lock(), io::stdout().lock())
}

/// Prints one self-test line; a closed stdout (the browser gave up) is
/// ignored rather than a panic.
fn say(line: std::fmt::Arguments<'_>) {
    use std::io::Write;
    let _ = writeln!(io::stdout(), "{line}");
}

fn report(lockdown: &Lockdown) {
    say(format_args!("lockdown={}", lockdown.complete()));
    say(format_args!("filesystem={}", lockdown.filesystem));
    say(format_args!("network={}", lockdown.network));
    say(format_args!("processes={}", lockdown.processes));
    say(format_args!("privileges={}", lockdown.privileges));
    say(format_args!(
        "integrity={}",
        lockdown
            .integrity
            .map_or("none".into(), |l| format!("{l:#x}"))
    ));
}

fn verdict(allowed: bool) -> &'static str {
    if allowed { "allowed" } else { "denied" }
}

/// `--renderer-selftest <dir> [port]`: waits for a line on stdin, locks
/// down, then tries what a compromised renderer might — write and read
/// files in `dir`, start a process, connect to `127.0.0.1:port` — and
/// prints one `key=value` line per check.
pub fn selftest(dir: &Path, port: Option<u16>) {
    let exe = std::env::current_exe().unwrap_or_default();
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
    let lockdown = rustlite_sandbox::lockdown();
    report(&lockdown);
    let target = dir.join("renderer-write-probe.txt");
    let write = std::fs::write(&target, b"x").is_ok();
    say(format_args!("write={}", verdict(write)));
    let _ = std::fs::remove_file(&target);
    let read = std::fs::read_dir(dir).is_ok();
    say(format_args!("read={}", verdict(read)));
    let spawn = Command::new(&exe)
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|mut c| {
            let _ = c.kill();
        })
        .is_ok();
    say(format_args!("spawn={}", verdict(spawn)));
    if let Some(port) = port {
        // Under the Windows lockdown token Winsock cannot even start, and
        // std panics on that instead of returning an error: also a denial.
        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let net =
            std::panic::catch_unwind(|| std::net::TcpStream::connect(("127.0.0.1", port)).is_ok())
                .unwrap_or(false);
        std::panic::set_hook(quiet);
        say(format_args!("network_connect={}", verdict(net)));
    }
}
