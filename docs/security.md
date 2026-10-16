# Security

**RustLite loads untrusted content from the network and runs its
JavaScript, but it is a single-process prototype without sandboxing. It is
not safe for browsing untrusted sites with sensitive data on the machine.**
Parsers, decoders, layout and the JavaScript engine (Boa) all process
attacker-controlled input in the browser process, with the user's
privileges. `--disable-javascript` turns scripting off. The planned process
split and OS sandboxes are designed in
[ADR-0005](../research/decisions/ADR-0005-process-isolation.md).

## JavaScript (SCRIPT-001)

- Engine: Boa 0.22 (Rust), without `Intl`/`Temporal`
  ([ADR-0004](../research/decisions/ADR-0004-javascript-engine-boa.md)).
- Scripts see only `window`, `document`, `console`, `location.href` and a DOM
  subset. No timers, network (`fetch`/XHR), storage, cookies, clipboard,
  file or browser-UI APIs exist for pages.
- DOM wrappers cannot be forged: the node behind a wrapper is looked up by
  object identity in Rust; objects made from our prototypes are rejected.
- The page's DOM is lent to the engine only during a call; no shared
  mutable state, no `unsafe` in our bindings.
- Limits: 10,000,000 iterations per loop and call depth 400; a stopped
  script reports an error. **Gaps:** no total time limit (many short loops
  can still run long) and no heap limit; both need process isolation.
- Scripts inserted through `innerHTML` do not run; modules are not run;
  `javascript:` URLs stay blocked.
- External scripts load only over `http(s)` (or from a local document's own
  directory), are limited to 4 MiB, and are refused when served as an image,
  audio, video or CSV type.

## Network

- TLS via rustls with the operating system's trust store
  (`rustls-platform-verifier`). Certificate validation is always on; there is
  no flag to disable it, and tests never disable it. Expired, self-signed and
  wrong-host certificates are refused (verified against badssl.com, NET-001).
- Only `http:`/`https:` are fetched. Redirects are followed manually so every
  hop is scheme-checked: a server cannot redirect into `file:` or other
  schemes. At most 10 redirects.
- Timeouts (30 s total, 10 s connect), per-destination body limits (16 / 4 /
  32 MiB), a bounded loader queue (64) and a fixed worker pool (4 threads), so
  a page cannot make RustLite allocate or queue without bound.
- Stale-result protection: superseded navigations are cancelled and their late
  results ignored by id; subresources are cancelled when the document changes.
- No cookies, no credentials, no cache, no `Referer`: nothing is persisted or
  sent beyond the request line, `Host`, `Accept` and a `User-Agent`.

## Local files

- Only the user can open `file:` URLs (command line, address bar, history).
  Page links to `file:`, `javascript:`, `data:` and unknown schemes are
  blocked.
- A local document can load images and stylesheets only from its own
  directory tree (canonicalized; `..`, absolute paths and symlinks pointing
  outside are refused).
- Network documents load subresources only over `http(s)`.
- **Mixed content is blocked:** an `https:` document loads no `http:`
  subresources at all (stylesheets, images, scripts). This is stricter than
  browsers, which upgrade or allow some passive content.

## Input hardening

- `unsafe_code = "forbid"` across the workspace (dependencies such as
  rustls/ring and the image decoders do contain `unsafe`).
- HTML parsing and layout are iterative or depth-capped (block depth 200), so
  deep nesting cannot overflow the stack; parsing is linear in input size.
- Image decoding has dimension (8192 px) and allocation limits; glyph
  rasterization caps glyph size (2048 px).
- Rasterizer tolerates malformed display lists (unbalanced clips are counted).
- The error page escapes URLs and messages.

## Fuzzing

`crates/browser-core/tests/fuzz.rs` mutates the HTML fixtures (tricky tokens,
deletions, duplication, splicing, truncation) and checks that parsing yields
a consistent tree and the whole pipeline (style, layout, paint, find,
resize) never panics; it also runs random sequences of DOM binding calls.
CI runs a fixed seed (150 cases per target); 36,000 release-mode cases with
three other seeds found no failures (2026-10-16). Decoders (PNG/JPEG/GIF)
and the CSS parser alone are not yet fuzzed separately.

## Not yet done (required before RustLite is suitable for general browsing)

1. Process isolation and OS sandboxes (designed in ADR-0005, not built).
2. A total script time limit and heap limit (need isolation or an engine
   interrupt hook).
3. CORS and CSP. Same-origin rules matter once scripts can fetch or read
   cross-origin data; today scripts cannot fetch, and cross-origin images,
   stylesheets and scripts load as they do in browsers.
4. Coverage-guided fuzzing (cargo-fuzz on nightly) and decoder fuzzing.
5. Download handling and safe file saving (downloads are refused today).
6. Dependency auditing in CI (`cargo audit` / `cargo deny`).
