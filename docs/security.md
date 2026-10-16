# Security

**RustLite loads untrusted content from the network and runs its
JavaScript. Pages run in separate renderer processes; on Windows those are
sandboxed, on Linux and macOS they are not yet. It is a prototype: do not
rely on it for browsing untrusted sites with sensitive data on the
machine.** `--disable-javascript` turns scripting off. Design:
[ADR-0005](../research/decisions/ADR-0005-process-isolation.md) (process
split) and [ADR-0006](../research/decisions/ADR-0006-sandbox-crate-scoped-unsafe.md)
(sandbox).

## Process isolation (PROC-001)

- Each tab's page (HTML/CSS parsing, style, layout, paint, JavaScript,
  image decoding) runs in a renderer process, `rustlite --renderer`, one per
  tab per site; cross-site navigation starts a new process. The browser
  process keeps the window, UI, navigation decisions, history, network
  stack, HTTP cache and clipboard.
- IPC is stdin/stdout with length-prefixed frames (≤64 MiB). The browser
  decodes every renderer message as hostile: bounds-checked lengths, finite
  floats, display-list fonts accepted only as faces the browser already
  indexed, image sizes capped at 16384 px a side.
- **Watchdog:** a renderer that does not reply within 10 s, exits or sends
  a malformed frame is killed; the tab shows a local "Page not available"
  page and the next navigation gets a new renderer. This is also the total
  time scripts can run per request (the in-process gap of Phase 9).
- `--single-process` puts pages back in the browser process (debugging
  only; no isolation).

## OS sandbox (SANDBOX-001)

Windows (verified by `apps/desktop/tests/renderer.rs`, which runs the real
binary in its sandbox):

- **Job object per renderer:** 1 GiB committed-memory cap, no child
  processes (`ActiveProcessLimit = 1`), killed when the browser exits or
  crashes, immediate exit on unhandled exceptions, no clipboard, desktop
  switching, global atoms, other processes' window handles, system
  parameters or display settings.
- **Renderer lockdown** before it reads any input: low integrity level
  (cannot write the user's files, registry or medium-integrity processes),
  all privileges disabled, no dynamic code, no DLLs from remote or
  low-integrity locations.
- The `unsafe` needed for these Win32 calls lives only in
  `crates/sandbox/src/windows.rs`, one documented call per block; all other
  crates forbid `unsafe` (ADR-0006).

**Gaps:**

- Renderers can still **read** files the user can read and could open
  **network** connections; RustLite's renderer code does neither for web
  pages, but the OS does not prevent it. Local `file:` documents read their
  own directory's subresources from the renderer. AppContainer (which
  removes both) is the next step.
- **Linux and macOS:** renderer processes, watchdog and memory isolation
  by process, but no OS sandbox yet (seccomp + namespaces, `sandbox_init`).
- No per-renderer heap limit inside Boa: the 1 GiB job cap ends the
  renderer instead (Windows only).

## JavaScript (SCRIPT-001)

- Engine: Boa 0.22 (Rust), without `Intl`/`Temporal`
  ([ADR-0004](../research/decisions/ADR-0004-javascript-engine-boa.md)).
- Scripts see only `window`, `document`, `console`, `location.href`,
  timers, animation frames, `performance.now()` and a DOM subset. No network
  (`fetch`/XHR), storage, cookies, clipboard, file or browser-UI APIs exist
  for pages.
- Timers are bounded (10,000 active per page; at most 1,000 callbacks per
  event-loop turn) and run on the UI thread between events; `performance.now()`
  is coarsened to 0.1 ms.
- Forms submit by GET only and the resulting navigation is a page-initiated
  navigation, so the usual URL policy applies (no `file:`, `javascript:` or
  `data:` targets).
- DOM wrappers cannot be forged: the node behind a wrapper is looked up by
  object identity in Rust; objects made from our prototypes are rejected.
- The page's DOM is lent to the engine only during a call; no shared
  mutable state, no `unsafe` in our bindings.
- Limits: 10,000,000 iterations per loop and call depth 400; a stopped
  script reports an error. The renderer watchdog (10 s per request) bounds
  total script time and the job object bounds memory on Windows; in
  `--single-process` mode neither applies.
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

1. OS sandboxes on Linux and macOS; AppContainer on Windows (no file
   reads or network for renderers).
2. A graceful script timeout (today a hung script costs the whole
   renderer, after 10 s) and a heap limit outside Windows.
3. CORS and CSP. Same-origin rules matter once scripts can fetch or read
   cross-origin data; today scripts cannot fetch, and cross-origin images,
   stylesheets and scripts load as they do in browsers.
4. Coverage-guided fuzzing (cargo-fuzz on nightly) and decoder fuzzing.
5. Download handling and safe file saving (downloads are refused today).
6. Dependency auditing in CI (`cargo audit` / `cargo deny`).
