//! Renderer processes and their sandbox, end to end with the real binary.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;

use rustlite_browser_core::{PageKey, RemotePage, Spawned, Spawner};
use rustlite_paint::{DisplayItem, Size};
use rustlite_sandbox::RendererJob;

const EXE: &str = env!("CARGO_BIN_EXE_rustlite");

fn spawner(args: &'static [&'static str]) -> Spawner {
    Rc::new(move || {
        let child = Command::new(EXE)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let job = RendererJob::new(rustlite_sandbox::DEFAULT_RENDERER_MEMORY)?;
        job.assign(&child)?;
        Ok(Spawned {
            child,
            guard: Some(Box::new(job)),
        })
    })
}

fn scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn sandboxed_renderer_cannot_write_files_or_start_processes() {
    let dir = scratch_dir("sandbox-selftest");
    let mut child = Command::new(EXE)
        .arg("--renderer-selftest")
        .arg(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let job = RendererJob::new(rustlite_sandbox::DEFAULT_RENDERER_MEMORY).unwrap();
    job.assign(&child).unwrap();
    // The renderer waits for this line, so it is confined before it checks.
    child.stdin.take().unwrap().write_all(b"go\n").unwrap();
    let lines: Vec<String> = BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map_while(Result::ok)
        .collect();
    child.wait().unwrap();
    let get = |key: &str| {
        lines
            .iter()
            .find_map(|l| l.strip_prefix(&format!("{key}=")).map(str::to_owned))
            .unwrap_or_else(|| panic!("no {key} in {lines:?}"))
    };
    if rustlite_sandbox::SUPPORTED {
        assert_eq!(get("lockdown"), "true", "{lines:?}");
        assert_eq!(get("integrity"), "0x1000", "low integrity: {lines:?}");
        assert_eq!(get("in_job"), "Some(true)");
        assert_eq!(
            get("write"),
            "denied",
            "no writing to user files: {lines:?}"
        );
        assert_eq!(get("spawn"), "denied", "no child processes: {lines:?}");
        assert_eq!(
            get("read"),
            "allowed",
            "fonts and local pages stay readable"
        );
    } else {
        assert_eq!(get("lockdown"), "false");
    }
}

fn remote(viewport: Size) -> RemotePage {
    RemotePage::new(spawner(&["--renderer"]), viewport)
}

#[test]
fn a_page_in_a_renderer_process_works_like_a_local_one() {
    let mut page = remote(Size::new(400, 300));
    page.load_html(
        "https://site.test/",
        "<!DOCTYPE html><title>Remote</title><body style='margin:0'>\
         <p id=p style='font-family: Arial'>hello world</p>\
         <input id=i style='position: absolute; top: 100px; left: 0; width: 100px'>\
         <script>document.title += ' + script';\
           setTimeout(() => document.title = 'timer', 0);</script>",
    );
    assert!(page.process_id().is_some());
    assert_eq!(page.title().as_deref(), Some("Remote + script"));

    // The display list arrives with fonts mapped to this process's faces.
    let list = page.display_list().clone();
    let texts = list
        .items()
        .iter()
        .filter(|i| matches!(i, DisplayItem::Text { .. }))
        .count();
    let has_fonts = page.fonts().face_count() > 0;
    if has_fonts {
        assert!(texts > 0, "text survives the trip: {list:?}");
    }

    // Timers run in the renderer when due.
    let due = page.next_timer().expect("timer pending");
    assert!(page.run_timers(due + Duration::from_millis(1)));
    assert_eq!(page.title().as_deref(), Some("timer"));

    // Input: focus the field and type.
    let out = page.click(20.0, 110.0);
    assert!(out.consumed);
    assert!(page.focused().is_some());
    page.key_down(&PageKey::Text("ok".into()));
    let report = page.memory_report();
    assert!(report.dom_nodes > 5);
    if has_fonts {
        assert_eq!(page.find("hello").len(), 1);
    }

    // Same-site navigations keep the process; cross-site ones replace it.
    let first = page.process_id();
    page.load_html("https://site.test/2", "<p>same site</p>");
    assert_eq!(page.process_id(), first);
    page.load_html("https://other.test/", "<p>other site</p>");
    assert_ne!(page.process_id(), first);
    assert_eq!(page.spawn_count, 2);
}

#[test]
fn a_hung_renderer_is_killed_and_replaced_by_an_error_page() {
    // The self-test mode waits for a newline the protocol never sends, so
    // it never answers: a hung renderer.
    let mut page = RemotePage::new(spawner(&["--renderer-selftest", "."]), Size::new(200, 100));
    page.timeout = Duration::from_millis(500);
    page.load_html("https://hang.test/", "<p>x</p>");
    assert!(page.crashed());
    assert_eq!(page.title().as_deref(), Some("Page not available"));
    assert!(!page.display_list().is_empty(), "the error page paints");
    // The browser side keeps working; a new navigation tries a new renderer.
    page.load_html("https://hang.test/again", "<p>y</p>");
    assert_eq!(page.spawn_count, 2);
}

#[test]
fn a_renderer_sending_garbage_is_dropped() {
    // `--help` prints text (not frames) and exits.
    let mut page = RemotePage::new(spawner(&["--help"]), Size::new(200, 100));
    page.load_html("https://bad.test/", "<p>x</p>");
    assert!(page.crashed());
    assert_eq!(page.title().as_deref(), Some("Page not available"));
}
