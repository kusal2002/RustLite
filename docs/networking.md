# Networking

`crates/net` fetches resources; `crates/browser-core` (`Tab`) decides what to
fetch and what to do with the results. The UI thread never performs network
I/O.

## Pieces

| Piece | Where | Role |
|---|---|---|
| `Fetcher` | `net/src/lib.rs` | One request: HTTP(S) via `ureq` + rustls, manual redirects, body limits, cancellation between chunks |
| `Loader` | `net/src/loader.rs` | 4 worker threads, bounded queue (64); results delivered through a callback |
| `decode_html` / `decode_text` | `net/src/encoding.rs` | BOM → HTTP charset → `<meta>` prescan → UTF-8 if valid → windows-1252 |
| `Tab` | `browser-core/src/tab.rs` | Navigation state machine, history, subresources, error pages, URL policy |
| Desktop | `apps/desktop` | Loader results become winit user events (`AppEvent::Load`) that wake the UI |

## Navigation flow

1. User input → `parse_user_input` (URLs as typed; paths → `file:`;
   `localhost`/`127.0.0.1` → `http:`; other bare hosts → `https:`).
2. `Tab::navigate_to` cancels any pending navigation, assigns a new id and
   queues the request. State becomes `Loading { url }`.
3. The result arrives with its id. If it is not the pending id, it is dropped
   (stale). Otherwise the document commits: HTML is decoded, the page is
   replaced, history is updated (push, or replace for back/forward/reload).
4. On commit, `<link rel=stylesheet>` and `<img>` URLs (resolved against the
   document URL or `<base href>`) are requested with a token tied to this
   document; committing another document cancels them. Stylesheets rebuild
   the cascade in document order; images invalidate layout.
5. Errors (DNS, refused, timeout, TLS/certificate, too large, unsupported
   content) commit RustLite's error page with a stable code (`ERR_CERT_INVALID`,
   `ERR_TOO_MANY_REDIRECTS`, ...). HTTP error statuses show the server's page.

## Limits

| Limit | Value |
|---|---|
| Redirect hops | 10 |
| Total timeout / connect timeout | 30 s / 10 s |
| Body size | document 16 MiB, stylesheet 4 MiB, image 32 MiB |
| Decoded image | 8192 × 8192 px |
| Loader queue | 64 pending requests; more are refused |
| Workers | 4 threads |

## Content types

`text/html` and `application/xhtml+xml` render as HTML; other `text/*` and JSON
render as plain text in a `<pre>`; `image/*` renders the image; anything else
shows an error page (no downloads yet).

## Cache

`Loader` keeps an in-memory cache of stylesheet and image responses, shared
by all tabs: only `200` responses with `Cache-Control: max-age` > 0 and
without `no-store`, `no-cache` or `private`; never documents; 32 MiB of
bodies (`FetchConfig::cache_bytes`) with least-recently-used eviction, and no
single entry over a quarter of the budget. Entries expire at their max-age.
A reload (`Tab::reload`) fetches subresources again and refreshes the cache.
There is no revalidation (`ETag`, `Last-Modified`), no heuristic freshness,
no `Vary` handling and nothing is written to disk.

## Not implemented yet

HTTP revalidation and disk cache, cookies, `Referer`, CORS and other origin checks for subresources,
CSP, `data:` URLs, proxies, HTTP/2 (ureq speaks HTTP/1.1), redirects for
`POST` (no forms yet), download handling, `@import`, `@charset`, and a
persistent or per-site connection pool policy.
