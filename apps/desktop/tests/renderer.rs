//! Renderer processes and their sandbox, end to end with the real binary.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

use rustlite_browser_core::{PageKey, RemotePage, Spawner};
use rustlite_paint::{DisplayItem, Size};

const EXE: &str = env!("CARGO_BIN_EXE_rustlite");

fn spawner(args: &'static [&'static str]) -> Spawner {
    rustlite_desktop::renderer::sandboxed_spawner(EXE, args)
}

fn scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn sandboxed_renderer_cannot_touch_files_network_or_processes() {
    let dir = scratch_dir("sandbox-selftest");
    std::fs::write(dir.join("visible.txt"), b"x").unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    let args = [
        OsStr::new("--renderer-selftest"),
        dir.as_os_str(),
        OsStr::new(&port),
    ];
    let mut launched = rustlite_sandbox::launch(
        std::path::Path::new(EXE),
        &args,
        rustlite_sandbox::DEFAULT_RENDERER_MEMORY,
    )
    .unwrap();
    // The renderer waits for this line before it locks down and checks.
    launched.stdin.write_all(b"go\n").unwrap();
    let lines: Vec<String> = BufReader::new(launched.stdout)
        .lines()
        .map_while(Result::ok)
        .collect();
    let get = |key: &str| {
        lines
            .iter()
            .find_map(|l| l.strip_prefix(&format!("{key}=")).map(str::to_owned))
            .unwrap_or_else(|| panic!("no {key} in {lines:?}"))
    };
    if rustlite_sandbox::SUPPORTED {
        assert_eq!(get("lockdown"), "true", "{lines:?}");
        if cfg!(windows) {
            assert_eq!(get("integrity"), "0x1000", "low integrity: {lines:?}");
        }
        assert_eq!(get("write"), "denied", "no writing files: {lines:?}");
        assert_eq!(get("read"), "denied", "no reading files: {lines:?}");
        assert_eq!(get("spawn"), "denied", "no child processes: {lines:?}");
        assert_eq!(
            get("network_connect"),
            "denied",
            "no network connections: {lines:?}"
        );
        if cfg!(target_os = "linux") {
            assert_eq!(
                get("unlisted_syscall"),
                "denied",
                "seccomp is default-deny (EPERM): {lines:?}"
            );
        }
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
fn local_subresources_and_fonts_come_from_the_browser() {
    let dir = scratch_dir("renderer-local");
    let doc = dir.join("doc");
    std::fs::create_dir_all(&doc).unwrap();
    std::fs::write(
        doc.join("style.css"),
        "#box { float: left; width: 123px; height: 7px }",
    )
    .unwrap();
    std::fs::write(doc.join("app.js"), "document.title = 'from file';").unwrap();
    let fb = rustlite_renderer::Framebuffer::new(3, 2).unwrap();
    let mut png = Vec::new();
    rustlite_renderer::encode_png(&fb, &mut png).unwrap();
    std::fs::write(doc.join("pic.png"), &png).unwrap();
    // Outside the document's directory: must stay unreadable.
    std::fs::write(dir.join("secret.css"), "#box { margin-left: 50px }").unwrap();

    let html = "<!DOCTYPE html><link rel=stylesheet href=style.css>\
         <link rel=stylesheet href=../secret.css><script src=app.js></script>\
         <body style='margin:0'><div id=box></div><img src=./pic.png>\
         <p style='font-family: Arial'>Latin and සිංහල</p>";
    let image_rect = |list: &rustlite_paint::DisplayList| {
        list.items().iter().find_map(|i| match i {
            DisplayItem::Image { rect, image } => Some((*rect, image.width, image.height)),
            _ => None,
        })
    };
    let mut local = rustlite_browser_core::Page::new(Size::new(400, 300));
    local.load_html_with_base("file:///doc/index.html", html, Some(&doc));
    let expected = image_rect(local.display_list());

    let mut page = remote(Size::new(400, 300));
    page.load_html_with_base("file:///doc/index.html", html, Some(&doc));
    assert!(!page.crashed());
    assert_eq!(page.title().as_deref(), Some("from file"));
    let list = page.display_list().clone();
    let (rect, w, h) = image_rect(&list).expect("the image was read by the browser");
    assert_eq!((w, h), (3, 2));
    // style.css applied (the image sits beside the 123px float); the sheet
    // outside the document's directory did not (no 50px margin).
    assert_eq!(rect.x, 123, "{list:?}");
    assert_eq!(
        Some((rect, w, h)),
        expected,
        "same as a single-process page"
    );
    if page.fonts().has_family("Arial") && page.fonts().has_family("Nirmala UI") {
        // Sinhala needs a fallback face, chosen by the browser.
        assert_eq!(page.find("සිංහල").len(), 1);
        let faces: std::collections::HashSet<_> = list
            .items()
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { run, .. } => Some(run.font),
                _ => None,
            })
            .collect();
        assert!(faces.len() >= 2, "primary and fallback faces: {faces:?}");
    }
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
