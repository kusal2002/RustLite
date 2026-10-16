//! End-to-end smoke tests that run the real `rustlite` binary.
//!
//! The windowed path needs a display and is exercised manually or in CI with
//! `rustlite --exit-after-frames N` (see docs/testing.md); these tests cover
//! everything that can run without one.

use std::path::PathBuf;
use std::process::Command;

fn rustlite() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustlite"))
}

fn temp_png(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rustlite-smoke-{}-{name}.png", std::process::id()))
}

#[test]
fn headless_render_writes_png_of_requested_size() {
    let path = temp_png("headless");
    let status = rustlite()
        .args(["--size", "320x200", "--headless"])
        .arg(&path)
        .status()
        .expect("failed to launch rustlite");
    assert!(status.success(), "rustlite exited with {status}");

    let png = std::fs::read(&path).expect("PNG not written");
    let _ = std::fs::remove_file(&path);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&png[12..16], b"IHDR");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 320);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 200);
}

#[test]
fn help_exits_successfully() {
    let output = rustlite().arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--headless"));
}

#[test]
fn invalid_option_exits_with_usage_error() {
    let output = rustlite().arg("--nope").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown option"));
}

#[test]
fn dump_dom_prints_parsed_tree() {
    let path = std::env::temp_dir().join(format!("rustlite-smoke-{}.html", std::process::id()));
    std::fs::write(&path, "<title>T</title><p>a<b>b</p>c").unwrap();
    let output = rustlite().arg("--dump-dom").arg(&path).output().unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "| <html>\n",
            "|   <head>\n",
            "|     <title>\n",
            "|       \"T\"\n",
            "|   <body>\n",
            "|     <p>\n",
            "|       \"a\"\n",
            "|       <b>\n",
            "|         \"b\"\n",
            "|     <b>\n",
            "|       \"c\"\n",
        )
    );
}

#[test]
fn dump_dom_reports_missing_file() {
    let output = rustlite()
        .args(["--dump-dom", "definitely-missing.html"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read"));
}
