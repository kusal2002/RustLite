//! End-to-end navigation against a local HTTP server: documents, redirects,
//! subresources, stale-result protection, error pages and links.

use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustlite_browser_core::{Initiator, NavState, Page, Tab, load_id_space};
use rustlite_fonts::FontContext;
use rustlite_net::{FetchConfig, LoadResult, Loader, Url};
use rustlite_paint::Size;

fn png_4x4() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 128, 0, 255]));
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || handle(stream));
        }
    });
    format!("http://{addr}")
}

fn handle(mut stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok() && line != "\r\n" && !line.is_empty() {
        line.clear();
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_owned();
    let (status, headers, body): (&str, String, Vec<u8>) = match path.as_str() {
        "/page" => (
            "200 OK",
            "Content-Type: text/html; charset=utf-8\r\n".into(),
            b"<!DOCTYPE html><title>Page</title><link rel=stylesheet href=style.css>\
              <p id=p>styled</p><img id=img src=img.png><p><a id=link href=/other>next</a></p>"
                .to_vec(),
        ),
        "/style.css" => (
            "200 OK",
            "Content-Type: text/css\r\n".into(),
            b"p { color: rgb(1, 2, 3) }".to_vec(),
        ),
        "/img.png" => (
            "200 OK",
            "Content-Type: image/png\r\nCache-Control: public, max-age=60\r\n".into(),
            png_4x4(),
        ),
        "/app" => (
            "200 OK",
            "Content-Type: text/html\r\n".into(),
            b"<title>App</title><script src=/app.js></script>\
              <script>document.title += ' + inline';</script>\
              <script src=/missing.js></script><script src=/img.png></script>\
              <script>console.log('last')</script>"
                .to_vec(),
        ),
        "/dynamic" => (
            "200 OK",
            "Content-Type: text/html\r\n".into(),
            b"<title>Dynamic</title><body><script>setTimeout(() => {\
                const img = document.createElement('img');\
                img.id = 'late'; img.setAttribute('src', '/img.png');\
                document.body.appendChild(img);\
              }, 0);</script>"
                .to_vec(),
        ),
        "/app.js" => (
            "200 OK",
            "Content-Type: text/javascript\r\n".into(),
            b"document.title = 'External';".to_vec(),
        ),
        "/other" => (
            "200 OK",
            "Content-Type: text/html\r\n".into(),
            b"<title>Other</title>".to_vec(),
        ),
        "/redirect" => ("302 Found", "Location: /page\r\n".into(), Vec::new()),
        "/missing" => (
            "404 Not Found",
            "Content-Type: text/html\r\n".into(),
            b"<title>Missing</title>".to_vec(),
        ),
        "/text" => (
            "200 OK",
            "Content-Type: text/plain\r\n".into(),
            b"<b>not markup</b>".to_vec(),
        ),
        "/slow" => {
            std::thread::sleep(Duration::from_millis(800));
            (
                "200 OK",
                "Content-Type: text/html\r\n".into(),
                b"<title>Slow</title>".to_vec(),
            )
        }
        "/fast" => (
            "200 OK",
            "Content-Type: text/html\r\n".into(),
            b"<title>Fast</title>".to_vec(),
        ),
        _ => ("500 Error", String::new(), Vec::new()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
}

fn tab() -> (Tab, Receiver<LoadResult>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let loader = Loader::new(
        4,
        &FetchConfig::default(),
        Arc::new(move |r| {
            let _ = tx.lock().unwrap().send(r);
        }),
    );
    let page = Page::with_fonts(Size::new(800, 600), FontContext::new());
    (Tab::new(page, Some(Arc::new(loader))), rx)
}

/// Delivers results until the tab is idle with no subresources in flight
/// and `extra` more results (stale ones) have been seen, or a deadline passes.
fn pump(tab: &mut Tab, rx: &Receiver<LoadResult>, mut extra: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let idle = tab.state() == NavState::Idle && tab.pending_subresources() == 0;
        if idle && extra == 0 {
            return;
        }
        if let Ok(r) = rx.recv_timeout(Duration::from_millis(50))
            && !tab.on_load_result(r)
            && extra > 0
        {
            extra -= 1;
        }
    }
    panic!("navigation did not settle: {:?}", tab.state());
}

fn element(tab: &mut Tab, id: &str) -> rustlite_dom::NodeId {
    let (dom, _) = tab.page_mut().layout().unwrap();
    dom.descendants(dom.root())
        .find(|&n| dom.attribute(n, "id") == Some(id))
        .unwrap()
}

#[test]
fn loads_document_with_stylesheet_and_image() {
    let base = server();
    let (mut t, rx) = tab();
    t.navigate(&format!("{base}/page")).unwrap();
    assert!(matches!(t.state(), NavState::Loading { .. }));
    pump(&mut t, &rx, 0);
    assert_eq!(t.page().title().as_deref(), Some("Page"));

    let p = element(&mut t, "p");
    let (_, styles) = t.page_mut().computed_styles().unwrap();
    assert_eq!(
        rustlite_style::resolved_value(styles.get(p).unwrap(), "color").unwrap(),
        "rgb(1, 2, 3)",
        "linked stylesheet applied"
    );

    let img = element(&mut t, "img");
    let (_, tree) = t.page_mut().layout().unwrap();
    let rect = rustlite_layout::element_rects(tree)[&img];
    assert_eq!(
        (rect.width, rect.height),
        (4.0, 4.0),
        "network image decoded and sized"
    );
}

#[test]
fn redirects_commit_the_final_url() {
    let base = server();
    let (mut t, rx) = tab();
    t.navigate(&format!("{base}/redirect")).unwrap();
    pump(&mut t, &rx, 0);
    assert_eq!(t.url().unwrap().path(), "/page");
}

#[test]
fn a_superseded_slow_load_never_replaces_the_newer_page() {
    let base = server();
    let (mut t, rx) = tab();
    t.navigate(&format!("{base}/slow")).unwrap();
    t.navigate(&format!("{base}/fast")).unwrap();
    // Wait for /fast, then for the late /slow result, which must be ignored.
    pump(&mut t, &rx, 1);
    assert_eq!(t.page().title().as_deref(), Some("Fast"));
    assert_eq!(t.url().unwrap().path(), "/fast");
    assert!(
        !t.can_go_back(),
        "the cancelled navigation left no history entry"
    );
}

#[test]
fn http_errors_show_the_server_page_and_network_errors_show_ours() {
    let base = server();
    let (mut t, rx) = tab();
    t.navigate(&format!("{base}/missing")).unwrap();
    pump(&mut t, &rx, 0);
    assert_eq!(t.page().title().as_deref(), Some("Missing"));

    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    t.navigate(&format!("http://127.0.0.1:{port}/")).unwrap();
    pump(&mut t, &rx, 0);
    assert_eq!(t.page().title().as_deref(), Some("Page not available"));
}

#[test]
fn plain_text_is_shown_as_text() {
    let base = server();
    let (mut t, rx) = tab();
    t.navigate(&format!("{base}/text")).unwrap();
    pump(&mut t, &rx, 0);
    let (dom, _) = t.page_mut().layout().unwrap();
    let pre = dom.find_element(dom.root(), "pre").unwrap();
    assert_eq!(dom.text_content(pre), "<b>not markup</b>");
    assert!(dom.find_element(dom.root(), "b").is_none());
}

#[test]
fn clicking_a_link_navigates() {
    let Some(fonts) = Some(FontContext::with_system_fonts()).filter(|f| f.face_count() > 0) else {
        eprintln!("note: no system fonts; skipping link test");
        return;
    };
    let base = server();
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let loader = Loader::new(
        2,
        &FetchConfig::default(),
        Arc::new(move |r| {
            let _ = tx.lock().unwrap().send(r);
        }),
    );
    let mut t = Tab::new(
        Page::with_fonts(Size::new(800, 600), fonts),
        Some(Arc::new(loader)),
    );
    t.navigate(&format!("{base}/page")).unwrap();
    pump(&mut t, &rx, 0);

    let link = element(&mut t, "link");
    let (_, tree) = t.page_mut().layout().unwrap();
    let r = rustlite_layout::element_rects(tree)[&link];
    let url = t
        .page_mut()
        .link_at(r.x + 2.0, r.y + 2.0)
        .expect("a link under the point");
    assert_eq!(url, Url::parse(&format!("{base}/other")).unwrap());
    t.navigate_to(url, Initiator::Page).unwrap();
    pump(&mut t, &rx, 0);
    assert_eq!(t.page().title().as_deref(), Some("Other"));
    assert!(t.back());
    pump(&mut t, &rx, 0);
    assert_eq!(t.page().title().as_deref(), Some("Page"));
}

#[test]
fn tabs_sharing_a_loader_get_their_own_results() {
    let base = server();
    let (mut a, rx) = tab();
    let loader = a.loader().cloned();
    let page = Page::with_fonts(Size::new(800, 600), FontContext::new());
    let mut b = Tab::with_id_space(page, loader, 7);
    assert_eq!((a.id_space(), b.id_space()), (0, 7));
    a.navigate(&format!("{base}/fast")).unwrap();
    b.navigate(&format!("{base}/page")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline
        && (a.state() != NavState::Idle
            || b.state() != NavState::Idle
            || b.pending_subresources() > 0)
    {
        if let Ok(r) = rx.recv_timeout(Duration::from_millis(50)) {
            match load_id_space(r.id) {
                0 => a.on_load_result(r),
                7 => b.on_load_result(r),
                other => panic!("unknown id space {other}"),
            };
        }
    }
    assert_eq!(a.page().title().as_deref(), Some("Fast"));
    assert_eq!(b.page().title().as_deref(), Some("Page"));
}

#[test]
fn cacheable_subresources_are_reused_until_reload() {
    let base = server();
    let (mut t, rx) = tab();
    let loader = t.loader().cloned().unwrap();
    t.navigate(&format!("{base}/page")).unwrap();
    pump(&mut t, &rx, 0);
    let first = loader.cache_stats();
    assert_eq!(
        (first.entries, first.hits),
        (1, 0),
        "only the max-age image is stored"
    );

    t.navigate(&format!("{base}/page")).unwrap();
    pump(&mut t, &rx, 0);
    assert_eq!(loader.cache_stats().hits, 1, "image served from cache");
    let img = element(&mut t, "img");
    let (_, tree) = t.page_mut().layout().unwrap();
    assert_eq!(rustlite_layout::element_rects(tree)[&img].width, 4.0);

    assert!(t.reload());
    pump(&mut t, &rx, 0);
    assert_eq!(loader.cache_stats().hits, 1, "reload bypasses the cache");
}

#[test]
fn external_scripts_run_in_document_order() {
    let base = server();
    let (mut t, rx) = tab();
    t.navigate(&format!("{base}/app")).unwrap();
    pump(&mut t, &rx, 0);
    assert_eq!(
        t.page().title().as_deref(),
        Some("External + inline"),
        "the external script ran before the later inline one"
    );
    assert_eq!(
        t.page_mut().console(),
        ["last"],
        "a 500 script and an image served as a script are skipped"
    );
}

#[test]
fn images_inserted_by_timers_are_fetched() {
    let base = server();
    let (mut t, rx) = tab();
    t.navigate(&format!("{base}/dynamic")).unwrap();
    pump(&mut t, &rx, 0);
    let due = t.next_timer().expect("timer scheduled");
    assert!(t.run_timers(due + Duration::from_millis(1)));
    assert_eq!(t.pending_subresources(), 1, "the new image was requested");
    pump(&mut t, &rx, 0);
    let img = element(&mut t, "late");
    let (_, tree) = t.page_mut().layout().unwrap();
    assert_eq!(rustlite_layout::element_rects(tree)[&img].width, 4.0);
    // Running timers again requests nothing twice.
    assert!(!t.run_timers(due + Duration::from_secs(1)));
    assert_eq!(t.pending_subresources(), 0);
}
