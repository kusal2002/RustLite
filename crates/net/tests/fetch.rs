//! Integration tests against a local HTTP server (no external network).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::channel;
use std::time::Duration;

use rustlite_net::{
    CancelToken, Destination, FetchConfig, Fetcher, Loader, NetError, Request, Url, decode_html,
};

/// Starts a tiny HTTP/1.1 server; returns its base URL.
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
    // Drain headers.
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok() && line != "\r\n" && !line.is_empty() {
        line.clear();
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_owned();
    let respond = |stream: &mut TcpStream, status: &str, headers: &str, body: &[u8]| {
        let head = format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body);
    };
    match path.as_str() {
        "/ok" => respond(
            &mut stream,
            "200 OK",
            "Content-Type: text/html; charset=ISO-8859-1\r\n",
            b"<p>caf\xe9</p>",
        ),
        "/redirect" => respond(&mut stream, "302 Found", "Location: /ok\r\n", b""),
        "/relative" => respond(&mut stream, "301 Moved", "Location: ok\r\n", b""),
        "/loop" => respond(&mut stream, "302 Found", "Location: /loop\r\n", b""),
        "/to-file" => respond(
            &mut stream,
            "302 Found",
            "Location: file:///etc/passwd\r\n",
            b"",
        ),
        "/no-location" => respond(&mut stream, "302 Found", "", b""),
        "/missing" => respond(
            &mut stream,
            "404 Not Found",
            "Content-Type: text/html\r\n",
            b"<h1>nope</h1>",
        ),
        "/big" => respond(
            &mut stream,
            "200 OK",
            "Content-Type: text/css\r\n",
            &vec![b'a'; 5 * 1024 * 1024],
        ),
        "/slow" => {
            std::thread::sleep(Duration::from_secs(3));
            respond(&mut stream, "200 OK", "", b"late");
        }
        p if p.starts_with("/chain/") => {
            let n: u32 = p[7..].parse().unwrap_or(0);
            if n == 0 {
                respond(
                    &mut stream,
                    "200 OK",
                    "Content-Type: text/plain\r\n",
                    b"end",
                );
            } else {
                respond(
                    &mut stream,
                    "302 Found",
                    &format!("Location: /chain/{}\r\n", n - 1),
                    b"",
                );
            }
        }
        _ => respond(&mut stream, "500 Internal Server Error", "", b""),
    }
}

fn req(base: &str, path: &str, destination: Destination) -> Request {
    Request {
        url: Url::parse(&format!("{base}{path}")).unwrap(),
        destination,
    }
}

fn fetcher() -> Fetcher {
    Fetcher::new(&FetchConfig {
        timeout: Duration::from_millis(1500),
        ..FetchConfig::default()
    })
}

#[test]
fn fetches_and_decodes_with_http_charset() {
    let base = server();
    let r = fetcher()
        .fetch(
            &req(&base, "/ok", Destination::Document),
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.mime.as_deref(), Some("text/html"));
    assert_eq!(r.charset.as_deref(), Some("iso-8859-1"));
    assert_eq!(
        decode_html(&r.body, r.charset.as_deref()).text,
        "<p>café</p>"
    );
    assert!(r.redirects.is_empty());
}

#[test]
fn follows_redirects_including_relative_and_chains() {
    let base = server();
    let f = fetcher();
    let r = f
        .fetch(
            &req(&base, "/redirect", Destination::Document),
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(r.url.path(), "/ok");
    assert_eq!(r.redirects.len(), 1);
    let r = f
        .fetch(
            &req(&base, "/relative", Destination::Document),
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(r.url.path(), "/ok");
    let r = f
        .fetch(
            &req(&base, "/chain/10", Destination::Document),
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(r.redirects.len(), 10);
    assert_eq!(r.body, b"end");
}

#[test]
fn redirect_limits_and_scheme_policy() {
    let base = server();
    let f = fetcher();
    let c = CancelToken::new();
    assert_eq!(
        f.fetch(&req(&base, "/loop", Destination::Document), &c),
        Err(NetError::TooManyRedirects)
    );
    assert_eq!(
        f.fetch(&req(&base, "/chain/11", Destination::Document), &c),
        Err(NetError::TooManyRedirects)
    );
    assert_eq!(
        f.fetch(&req(&base, "/to-file", Destination::Document), &c),
        Err(NetError::RedirectToUnsupportedScheme("file".into()))
    );
    assert_eq!(
        f.fetch(&req(&base, "/no-location", Destination::Document), &c),
        Err(NetError::BadRedirect)
    );
}

#[test]
fn error_statuses_return_their_body() {
    let base = server();
    let r = fetcher()
        .fetch(
            &req(&base, "/missing", Destination::Document),
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(r.status, 404);
    assert_eq!(r.body, b"<h1>nope</h1>");
}

#[test]
fn body_size_limit_per_destination() {
    let base = server();
    let f = fetcher();
    let c = CancelToken::new();
    // 5 MiB exceeds the 4 MiB stylesheet limit but fits a document.
    assert!(matches!(
        f.fetch(&req(&base, "/big", Destination::Style), &c),
        Err(NetError::BodyTooLarge(_))
    ));
    assert_eq!(
        f.fetch(&req(&base, "/big", Destination::Document), &c)
            .unwrap()
            .body
            .len(),
        5 * 1024 * 1024
    );
}

#[test]
fn timeouts_and_refused_connections() {
    let base = server();
    let f = fetcher();
    let c = CancelToken::new();
    assert_eq!(
        f.fetch(&req(&base, "/slow", Destination::Document), &c),
        Err(NetError::Timeout)
    );
    // Bind and drop a listener to get a port that refuses connections.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let closed = format!("http://127.0.0.1:{port}");
    // Linux/macOS refuse immediately; Windows retries SYNs for ~2s, which can
    // exceed this test's 1.5s timeout. Either way it must be an error.
    let refused = f.fetch(&req(&closed, "/", Destination::Document), &c);
    assert!(
        matches!(refused, Err(NetError::ConnectionFailed | NetError::Timeout)),
        "{refused:?}"
    );
}

#[test]
fn https_to_a_plain_http_server_is_a_tls_error_not_a_success() {
    let base = server().replace("http://", "https://");
    let err = fetcher()
        .fetch(
            &req(&base, "/ok", Destination::Document),
            &CancelToken::new(),
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            NetError::Tls(_) | NetError::Certificate(_) | NetError::Other(_)
        ),
        "{err:?}"
    );
}

#[test]
fn loader_delivers_results_and_honors_cancellation() {
    let base = server();
    let (tx, rx) = channel();
    let tx = std::sync::Mutex::new(tx);
    let loader = Loader::new(
        1,
        &FetchConfig::default(),
        Arc::new(move |r| {
            let _ = tx.lock().unwrap().send(r);
        }),
    );
    // One worker: the slow request occupies it, so the cancelled job is
    // still queued when its token is cancelled.
    loader
        .load(
            1,
            req(&base, "/slow", Destination::Document),
            CancelToken::new(),
        )
        .unwrap();
    let cancel = CancelToken::new();
    loader
        .load(2, req(&base, "/ok", Destination::Document), cancel.clone())
        .unwrap();
    cancel.cancel();
    loader
        .load(
            3,
            req(&base, "/ok", Destination::Document),
            CancelToken::new(),
        )
        .unwrap();

    let mut results: Vec<_> = (0..3)
        .map(|_| rx.recv_timeout(Duration::from_secs(10)).unwrap())
        .collect();
    results.sort_by_key(|r| r.id);
    assert_eq!(results[0].result.as_ref().unwrap().body, b"late");
    assert_eq!(results[1].result, Err(NetError::Cancelled));
    assert_eq!(results[2].result.as_ref().unwrap().status, 200);
}
