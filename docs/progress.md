# Progress tracker

Statuses: Planned · Investigating · Specified · Implementing · Implemented ·
Tested · Verified on reference browser · Blocked · Deferred.

## Feature matrix

| ID | Feature | Status | Evidence |
|---|---|---|---|
| PLAT-001 | Desktop window (winit) | Tested (Windows) | `rustlite --exit-after-frames 120` ran, exit 0 |
| PLAT-002 | Resize + DPI-aware toolbar layout | Implemented | `toolbar::tests::layout_scales_with_dpi`; manual resize not yet scripted |
| PLAT-003 | Keyboard shortcuts (Ctrl/Cmd+Q/W quit, F12 overlay) | Tested | `commands::tests` |
| PLAT-004 | Mouse hover/click hit-testing on toolbar | Tested | `toolbar::tests::hit_testing` |
| RENDER-001 | Display list (fill, border, clip) | Tested | `rustlite-paint` unit tests |
| RENDER-002 | CPU rasterizer with alpha + clipping | Tested | `rustlite-renderer` unit tests |
| RENDER-003 | Headless frame → PNG | Tested | `tests/png_roundtrip.rs`, `apps/desktop/tests/smoke.rs` |
| RENDER-004 | Paint debug overlay | Tested | `frame::tests::debug_overlay_outlines_rects` |
| CORE-001 | Page with cached display list + invalidation | Tested | `browser-core` tests |
| DIAG-001 | Structured logging (`RUST_LOG`) | Implemented | Manual run |
| DIAG-002 | Frame-time stats (min/mean/p95/max) | Tested | `frame_stats` tests |
| HTML-001 | HTML tokenizer + tree builder | Verified on reference browser (fixture set) | 32/32 golden trees match Chromium 152 `DOMParser`; [spec](../research/specifications/HTML-001.md) |
| DOM-001 | Arena DOM | Tested | `rustlite-dom` tests (incl. 200k-deep traversal) |
| CORE-002 | Page loads parsed HTML; document title | Tested | `browser-core` tests (content not painted until Phase 3–4) |
| DIAG-003 | `--dump-dom` CLI (html5lib tree format) | Tested | `apps/desktop/tests/smoke.rs` |
| CSS-001 | CSS tokenizer, parser, selectors, cascade, computed values | Verified on reference browser (fixture set) | 11/11 style goldens match Chromium 152; [spec](../research/specifications/CSS-001.md) |
| CORE-003 | Page computes styles lazily; viewport change invalidates | Tested | `browser-core` tests |
| DIAG-004 | `--dump-style` CLI | Tested | `apps/desktop/tests/smoke.rs` |
| FONT-001 | System fonts, selection, fallback, shaping, glyph raster | Tested | `rustlite-fonts` tests ([ADR-0002](../research/decisions/ADR-0002-font-stack.md)) |
| LAYOUT-001 | Box tree, block layout, margin collapsing, inline layout, line breaking | Verified on reference browser (fixture set) | 9/9 geometry goldens match Chromium 152 (max 0.032px off); [spec](../research/specifications/LAYOUT-001.md) |
| RENDER-005 | Paint of backgrounds, borders, text; device scale; overflow clip | Tested | `layout` + `renderer` tests; headless render of `tests/pages/sample.html` |
| SHELL-001 | Open a local file, title, wheel/keyboard scrolling | Tested | `commands` tests; windowed run with sample page |
| FONT-002 | On-demand glyph rasterizer (fontdue removed) | Tested | `raster` tests; memory 73.6 → 23.9 MB ([ADR-0003](../research/decisions/ADR-0003-own-glyph-rasterizer.md)) |
| LIST-001 | `list-style-type`, outside list markers, `<ol start>`, `<li value>` | Tested | `layout` tests (ordinals, counter styles, placement); visual check |
| IMAGE-001 | Local `<img>` (PNG/JPEG/GIF), presentational hints, replaced sizing, inline/block | Verified on reference browser (fixture set) | 2/2 image fixtures match Chromium 152; [spec](../research/specifications/IMAGE-001.md) |
| RENDER-006 | Image drawing (bilinear, alpha, clip); dashed/dotted/double borders | Tested | `renderer` + `paint` tests; visual check |
| NET-001 | HTTP(S) fetch: TLS (OS trust store), manual redirects with scheme checks, limits, timeouts, cancellation, charset decoding, background loader | Tested | `net` unit + local-server integration tests; badssl.com manual checks ([spec](../research/specifications/NET-001.md)) |
| NAV-001 | Tab navigation: stale-result protection, history, error pages, URL policy, linked stylesheets and images over HTTP | Tested | `browser-core` navigation tests against a local server |
| SHELL-002 | Address bar editing, link clicks, back/forward/reload/stop, loading bar | Tested | `address_bar`, `commands`, `toolbar` tests; manual windowed use |
| SHELL-003 | Multiple tabs (tab strip, per-tab load ids on one shared loader, one shared font context), links in new tabs, clipboard in text fields | Tested | `tabstrip`, `commands`, `address_bar` tests; shared-loader routing test; scripted windowed run |
| FIND-001 | Find in page: case-insensitive matches across inline elements and soft wraps, highlights, next/previous, scroll to match | Tested | layout `find_text` test; `findbar` tests; scripted windowed run |
| PERF-001 | Benchmark harness, baseline and regression compare | Tested | `rustlite-bench` unit tests; baseline file; CI smoke run |
| PERF-002 | Shaping face cache, word shaping cache, attribute bucket, ancestor key filter, viewport-culled paint | Tested | style golden fixtures still 12/12; `ancestor_filter_keeps_every_true_match`; `paint_emits_only_what_the_viewport_can_show` |
| PERF-003 | Memory report per subsystem; bounded HTTP subresource cache; restyle on resize only with viewport units | Tested | `memory_report_covers_each_subsystem`; cache unit tests and `cacheable_subresources_are_reused_until_reload`; `resize_restyles_only_viewport_dependent_documents` |
| SCRIPT-001 | JavaScript (Boa): classic scripts in order, DOM subset, click events, `on*` handlers, limits | Tested | `rustlite-script` unit tests; page tests; `external_scripts_run_in_document_order`; scripted windowed clicks on `tests/pages/interactive.html` |
| SEC-002 | Mixed-content blocking; script MIME refusal; seeded fuzzing in CI; isolation ADR | Tested | `https_pages_do_not_load_http_subresources`; `fuzz.rs` (3 targets); ADR-0005 |
| WEBAPI-001 | Timers, animation frames, `performance.now`, event loop in the window and headless renders, `element.style`, fetching resources scripts insert | Tested | `rustlite-script` timer/style tests; `timers_change_the_page_later`; `images_inserted_by_timers_are_fetched` |
| FORM-001 | Form controls (text, password, buttons, checkbox, radio): layout, paint, focus, editing, keyboard events, GET submission | Verified on reference browser (layout) / Tested | `form-controls` layout fixture vs Chromium 152; `interaction.rs` (9 tests); scripted window typing |
| LAYOUT-002 | Positioning (relative/absolute/fixed, z-index), floats and clearance, inline-block/inline-flex, flexbox, `box-sizing`, hyphen break opportunities; style properties for all of them | Verified on reference browser (fixture set) | `positioning`, `floats`, `inline-block`, `flexbox`, `box-sizing`, `hyphen-breaks` layout fixtures and `layout-properties` style fixture match Chromium 152 (≤0.02 px); 36,000 extra release fuzz cases |
| PROC-001 | Renderer process per tab per site, framed IPC, watchdog and crash fallback, `--single-process` | Tested | `apps/desktop/tests/renderer.rs` (end-to-end page, hung renderer, garbage renderer); IPC/message decode unit tests; scripted window runs with 3 tabs |
| SANDBOX-001 | Windows sandbox: renderer created suspended with a restricted lockdown token (deny-only groups, NULL restricting SID, no privileges, low integrity) and a warm-up token it drops; job object per renderer (memory cap, no child processes, UI limits, kill on close); mitigations | Tested (Windows) | `sandboxed_renderer_cannot_touch_files_network_or_processes` (write, read, process start and TCP connect denied); window runs: every renderer logged a complete lockdown at integrity 0x1000; ADR-0006 |
| SANDBOX-002 | Linux (no_new_privs, Landlock, default-deny seccomp-bpf allowlist) and macOS (`sandbox_init`) renderer sandboxes | Tested (CI) | CI run 39049665123: the self-test (write, read, process start, TCP connect and an unlisted system call denied, the last with `EPERM`) and brokered-page tests pass on ubuntu-latest and macos-latest; every renderer logged a complete lockdown; strace shows no needed call refused; `linux::tests` check the filter program |
| BROKER-001 | Renderers open no files: font face list, font data, fallback faces and local subresources come from the browser by upcall | Tested | `local_subresources_and_fonts_come_from_the_browser` (stylesheet, script, image, Sinhala fallback; a stylesheet outside the directory is refused; same image placement as a local page); `upcalls_are_answered_only_within_policy` |
| SEC-003 | Dependency auditing (`cargo deny`: advisories, licenses, bans, sources) | Tested (CI) | `deny.toml`; `audit` CI job; first run found `rustybuzz`/`ttf-parser` unmaintained, since replaced (FONT-003); no acknowledged advisories |
| FONT-003 | Font stack on maintained crates: `harfrust` shaping, `skrifa` outlines, `read-fonts` metrics/cmap (rustybuzz, ttf-parser, self_cell removed) | Verified on reference browser (layout fixtures) / Tested | 19/19 layout fixtures still match Chromium 152; 302 workspace tests; raster parity vs the old path on 7 Windows fonts ([ADR-0007](../research/decisions/ADR-0007-font-stack-harfrust-skrifa.md)) |
| CSS-002 | `font` shorthand, `min-/max-width/height`, `light-dark()` | Verified on reference browser (fixture set) | `font-shorthand` style and `min-max-sizes` layout fixtures match Chromium 152 |
| JS-001 | JS runtime | Deferred (Phase 9) | — |

## Platform validation

| Platform | Build | Tests | Window | Notes |
|---|---|---|---|---|
| Windows 11 x64 (MSVC) | ✅ | ✅ 302/302 | ✅ | Ryzen 5 5600G, Radeon iGPU; sandboxed renderers (restricted token) |
| Linux x64 | ✅ CI | ✅ CI (run 38864693788) | Unverified | No display in CI; text layout goldens skip (no Times New Roman) |
| macOS | ✅ CI | ✅ CI (run 38864693788) | Unverified | Layout text goldens pin Times New Roman and skip if its metrics differ from the reference machine |

## Known limitations

- Local documents load images/stylesheets only from their own directory; network
  documents only over http(s). No `data:` URLs, cookies, cache or JavaScript.
- Renderer sandboxes are verified on Windows (locally) and on Linux and
  macOS (CI); see [security.md](security.md) for their gaps.
- No grid, tables or `position: sticky` (see compatibility.md).
- CSS gaps (at-rules, `:not()`, `var()`, `calc()`, external sheets) are listed in
  [compatibility.md](compatibility.md#css-css-001--tested-verified-on-reference-browser).
- HTML parser deviations are listed in [compatibility.md](compatibility.md#html-parsing-html-001--tested-verified-on-reference-browser).
- Toolbar has no text (no font stack yet); buttons only log clicks.
- No vsync/frame pacing (softbuffer on Windows presents immediately).
- Window path has no automated test that runs without a display.

## Milestone log

### 2026-10-16 — Phase 0 + Phase 1 foundation

- **What:** Cargo workspace (5 crates), desktop window with toolbar scene, CPU
  rasterizer, PNG headless capture, logging, frame stats, docs, CI, scripts.
- **Why:** Smallest end-to-end vertical slice: display list → raster → window.
- **Tests:** `cargo fmt --check` ✅ · `cargo clippy --workspace --all-targets -D warnings` ✅ ·
  `cargo test --workspace` ✅ 47 passed · `cargo build --release` ✅.
- **Measurements:** see [performance.md](performance.md).
- **Next:** Phase 2 (DOM + HTML tokenizer), see [roadmap.md](roadmap.md).

### 2026-10-16 — Phase 2: HTML parser and DOM

- **What:** `rustlite-dom` (arena DOM), `rustlite-html` (WHATWG tokenizer + tree
  builder), browser-core `Page::load_html` + `title()`, `--dump-dom` CLI,
  reference-browser oracle script, 32 golden fixtures.
- **Why:** First stage of the real pipeline; everything after (style, layout)
  consumes this DOM.
- **Fixes along the way:** cycle check in `insert_before` made O(1) for leaf
  nodes and open-element name counts added to the tree builder; both turned
  deeply nested markup from quadratic into linear time.
- **Tests:** fmt ✅ · clippy `-D warnings` ✅ · `cargo test --workspace` ✅ 84 passed
  (incl. 32/32 golden fixtures vs Chromium) · release build ✅.
- **Measurements:** 4.3 MB synthetic page → 540k nodes in ~194 ms (release), see
  [performance.md](performance.md).
- **Next:** Phase 3, CSS tokenizer/parser, selectors, cascade.

### 2026-10-16 — Phase 3: CSS and computed styles

- **What:** `rustlite-css` (CSS Syntax 3 tokenizer, rule/declaration parser,
  selector parser with specificity) and `rustlite-style` (UA stylesheet,
  indexed rule matching, cascade, inheritance, computed values, style sharing,
  resolved-value serialization). Page computes styles lazily; `--dump-style` CLI.
  Oracle generalized to `scripts/oracle/server.py` with `/html` and `/style` pages.
- **Why:** Layout (Phase 4) consumes computed styles.
- **Evidence-driven fixes:** the style oracle found three differences on the first
  run (alpha serialization, default font family, form-control UA styles); all
  fixed, now 11/11 fixtures match Chromium 152.
- **Tests:** fmt ✅ · clippy `-D warnings` ✅ · `cargo test --workspace` ✅ 137 passed
  (incl. 32 HTML + 11 style goldens) · release build ✅.
- **Measurements:** 300,005 elements styled in ~72 ms (release), see
  [performance.md](performance.md).
- **Next:** Phase 4, block and inline layout.

### 2026-10-16 — Phase 4: fonts, layout, and rendering real pages

- **What:** `rustlite-fonts` (fontdb discovery, CSS face selection, per-character
  fallback, rustybuzz shaping, fontdue glyph raster, bounded glyph cache);
  `rustlite-layout` (box tree with anonymous blocks, block layout with margin
  collapsing, inline layout with whitespace collapsing and line breaking, paint);
  text in the display list and renderer; device-pixel-ratio support; the desktop
  app opens a local HTML file, shows its title, and scrolls (wheel, arrows,
  Page Up/Down, Space, Home/End); `--dump-layout`; `/layout` oracle page.
- **CI check:** Windows and Linux green for all phases; macOS failed Phase 3 only
  on the Windows-recorded default font family, fixed in `ed08a83`.
- **Evidence-driven fixes:** the layout oracle found `<br>` boxes, collapsed-through
  positioning, a 3px initial border-width bug, and Blink's floored half-leading.
  All 9 fixtures now match Chromium; max deviation 0.032px.
- **Tests:** fmt ✅ · clippy `-D warnings` ✅ · `cargo test --workspace` ✅ 165 passed ·
  release build ✅ · windowed run with `tests/pages/sample.html` ✅.
- **Measurements:** see [performance.md](performance.md) (memory with a page loaded
  is 73.6 MB; font data handling is the main cost).
- **Next:** Phase 5 completion (images, list markers) and Phase 6 networking.

### 2026-10-16 — Phase 5: images, list markers, border styles, font memory

- **What:** an on-demand glyph rasterizer replaces fontdue (ADR-0003);
  `list-style-type` with outside markers and list ordinals; local `<img>`
  loading (PNG/JPEG/GIF, size limits, document-directory policy) with
  presentational `width`/`height` hints, replaced-element sizing, atomic
  inline and block images; bilinear image drawing; dashed, dotted and double
  borders. The layout golden harness moved to `browser-core` so fixtures run
  through the real page pipeline including image loading; the oracle serves
  image assets.
- **Evidence:** both new image fixtures matched Chromium 152 on the first run;
  all 11 layout fixtures pass.
- **Bug found visually:** double-border corners overlapped; now drawn as two
  nested rings (unit-tested).
- **Tests:** fmt ✅ · clippy `-D warnings` ✅ · `cargo test --workspace` ✅ 184 passed ·
  release build ✅.
- **Measurements:** sample page with images: 24.4 MB working set (was 73.6 MB
  with fontdue); binary 2.5 MiB. See [performance.md](performance.md).
- **Next:** Phase 6 networking.

### 2026-10-16 — Phase 6: networking and navigation

- **What:** `rustlite-net` (ureq + rustls with the OS trust store, manual
  redirects with per-hop scheme checks, size limits, timeouts, cancellation,
  encoding_rs charset detection, a 4-thread bounded loader); `Tab` in
  `browser-core` (navigation state machine with stale-result protection,
  history, error pages, `file:` only for user-initiated navigations, `<base>`,
  linked stylesheets in cascade order, images over HTTP); a real address bar,
  link clicking with a pointer cursor, back/forward/reload/stop and a loading
  bar in the desktop app; headless rendering of URLs.
- **Real sites:** example.com renders over HTTPS; expired, self-signed and
  wrong-host certificates (badssl.com) are refused. Rendering example.com
  exposed three missing CSS features (`font` shorthand, `max-width`,
  `light-dark()`), now implemented and verified with Chromium fixtures; the
  page's paragraph wraps identically to Chromium.
- **Tests:** fmt ✅ · clippy `-D warnings` ✅ · `cargo test --workspace` ✅ 226 passed
  (incl. local HTTP server tests) · release build ✅ · windowed smoke ✅.
- **Measurements:** example.com in the window: 28.0 MB working set / 12.0 MB
  private; binary 4.6 MiB (TLS and encodings). See [performance.md](performance.md).
- **Next:** Phase 7 completion (tabs, history UI) or Phase 8 (benchmarks,
  incremental invalidation); process isolation design before JavaScript.

### 2026-10-16 — Phase 7: tabs, clipboard, find in page

- **What:** a tab strip (new, close, switch, middle-click close, `+` button,
  per-tab loading indicator); tabs share one loader, with each tab's load ids
  in its own 32-bit space so results route to the right tab and closed tabs'
  loads are cancelled; the window's single font context moves to the active
  tab instead of each tab loading system fonts; Ctrl/Cmd+click and
  middle-click open links in background tabs; copy, cut and paste in the
  address bar (arboard); find in page with a floating bar, highlights, match
  count, next/previous and scrolling to the current match.
- **Layout change:** text fragments now keep their text, which find uses;
  matches span inline elements and soft line breaks but not blocks.
- **Tests:** fmt ✅ · clippy `-D warnings` ✅ · `cargo test --workspace` ✅ 239 passed
  · release build ✅ · scripted windowed run (SendKeys + screenshots): find
  highlights align with text, Enter steps to 3/4, Ctrl+T/W/Tab, Ctrl+L/C,
  Ctrl+V into a new tab, Ctrl+Q all behave.
- **Not verified:** Ctrl+click and middle-click on links were not exercised
  in the scripted run (no mouse automation); clipboard on Linux/macOS is
  unit-tested only through the editing model.
- **Measurements:** three tabs, two showing the sample page: 29.1 MB working
  set (one tab: ~24–28 MB); binary 4.6 MiB.
- **Next:** Phase 8 (benchmarks, incremental invalidation, HTTP cache) or a
  history list; process isolation design before JavaScript.

### 2026-10-16 — Phase 8: benchmarks, memory reporting, bounded caches

- **What:** `rustlite-bench` (per-stage timings on deterministic workloads,
  baseline save/compare, CI smoke run); shaping faces parsed once per font
  and a bounded word shaping cache; attribute-name rule bucket and an
  ancestor key filter in the stylist; viewport-culled paint; restyle on
  resize only when viewport units were used; `Page::memory_report()` logged
  by the app; a 32 MiB LRU HTTP cache for subresources honoring
  `Cache-Control: max-age`/`no-store`/`no-cache`/`private`, bypassed on reload.
- **Results:** article-50 layout 740 → 19.7 ms cold / 9.7 ms warm; style
  12.3 → 5.1 ms; article-400 paint + raster 39 → 0.7 ms. See
  [performance.md](performance.md#phase-8-results-reference-machine-medians-ms).
- **Bug found:** command-line relative paths (`rustlite tests/pages/sample.html`)
  were parsed as host names, so CI's headless render had shown an error page
  since Phase 6. The command line now resolves existing files to absolute
  paths (the address bar is unchanged).
- **Bug caught by a new test:** the first ancestor-filter version treated
  compounds left of a sibling combinator as ancestors (`.a + .b .c`); fixed
  before landing.
- **Tests:** fmt ✅ · clippy `-D warnings` ✅ · `cargo test --workspace` ✅ 252 passed
  · release build ✅ · benchmark baseline saved · windowed memory run.
- **Measurements:** 28.8 MB working set with the sample page, 29.3 MB with
  three tabs; binary 4.7 MiB.
- **Not done:** dirty-bit incremental restyle/relayout and dirty-rect
  repaint (whole-document work remains, but it is now 10–100× cheaper);
  HTTP revalidation and heuristic freshness.
- **Next:** Phase 9 prerequisites (process-isolation ADR, same-origin policy
  for subresources, parser fuzzing) before a JavaScript engine.

### 2026-10-16 — Phase 9: JavaScript (Boa) and first DOM APIs

- **What:** `rustlite-script` binds Boa 0.22 to a DOM subset (see
  [compatibility.md](compatibility.md#javascript-script-001)); scripts run
  after parsing in document order, then `DOMContentLoaded`/`load`; mouse
  clicks are dispatched to page listeners first and `preventDefault` stops
  link navigation; DOM changes restyle and relayout the page;
  `--disable-javascript`. Prerequisites: mixed-content blocking, script MIME
  refusal, seeded mutation fuzzing in CI, ADR-0004 (engine) and ADR-0005
  (process isolation design).
- **Design:** the page's DOM is lent to the engine per call; wrappers map to
  nodes by object identity, so no `unsafe` and no forgeable references.
- **Engine choice:** Boa, confirmed with the user before adding the
  dependency.
- **Tests:** fmt ✅ · clippy `-D warnings` ✅ · `cargo test --workspace` ✅ 270 passed
  · release build ✅ · 36,000 extra release fuzz cases, no failures ·
  windowed run with real mouse clicks: counter, task toggle, cancelled link,
  title updates.
- **Measurements:** binary 4.7 → 10.8 MiB; a realm costs ~0.29 MB private
  memory and 0.7–0.9 ms; the interactive page runs its scripts in 2.1 ms
  (headless) and the window uses 25.1 MB working set.
- **Not done / gaps:** timers, `fetch`, style objects, form controls and
  keyboard events; no total script time or heap limit; scripts run in the
  browser process (isolation designed, not built).
- **Next:** Phase 10 candidates: timers and an event loop, forms and
  keyboard input, then the process split (ADR-0005).

### 2026-10-16 — Phase 10 (part 1): Web APIs, forms and keyboard input

- **Scope chosen with the user:** Web APIs first (advanced CSS layout and
  process isolation remain for later Phase 10 parts).
- **What:** timers (`setTimeout`/`setInterval`), `requestAnimationFrame`,
  `performance.now()`; the window's event loop sleeps until the next page
  timer or frame and paces animation at ~60 Hz; headless renders let timers
  due within 500 ms of load run; `element.style`; resources inserted by
  scripts are fetched (each element requested once per document); form
  controls rendered as atomic inlines with Chromium-matched sizes and
  baselines; focus, caret, typing, Tab navigation, Ctrl/Cmd+V into fields;
  `keydown`/`keyup`/`input`/`change`/`focus`/`blur`/`submit` events; GET form
  submission; script `value`/`checked`/`focus()`/`activeElement`.
- **Verification:** a new `form-controls` layout fixture matches Chromium 152
  (all 13 layout fixtures pass, ≤0.02 px); the intrinsic-width formula was
  derived from 60 Chromium measurements over 4 fonts × 3 sizes × 5 `size`
  values. A scripted window run typed "Ada Lovelace", edited it to
  "Ada Byron", ticked a checkbox and pressed Enter; the page's submit handler
  showed "HELLO, ADA BYRON!", while its clock (`setInterval`) and
  progress-bar animation (`requestAnimationFrame` + `style.width`) ran.
  The very first such run did not register the typing; I could not
  determine why (no logs were captured for it). Three later runs with key
  logging all worked, and the same sequence is now an automated test.
- **Tests:** fmt ✅ · clippy `-D warnings` ✅ · `cargo test --workspace` ✅ 288 passed
  · release build ✅ · 18,000 extra release fuzz cases (interaction and new
  bindings included), no failures.
- **Measurements:** binary 11.0 MiB (+0.2 MiB); the demo page after typing:
  26.6 MB working set, 12.0 MB private.
- **Not done:** POST forms, `<textarea>`, `<select>`, text selection,
  `:focus`/`:checked`, `fetch`, throttling background-tab timers, a total
  script time limit.
- **Next (Phase 10 part 2 or 3):** advanced CSS layout (inline-block,
  floats, positioning, flexbox) or the process split (ADR-0005).

### 2026-10-17 — Phase 10 (parts 2 and 3): advanced CSS layout, renderer processes, Windows sandbox

- **Scope chosen with the user:** the remaining Phase 10 parts. For the
  sandbox the user chose scoped `unsafe` in one new crate using
  `windows-sys` (ADR-0006); Linux/macOS sandboxes are the next step.
- **Layout:** `position` relative/absolute/fixed with `z-index` paint
  order, floats with clearance and BFC narrowing, inline-block and
  inline-flex, min/max-content sizing and shrink-to-fit, flexbox
  (simplified §9), `box-sizing`, break opportunities after hyphens;
  block-level form controls. 21 new style longhands and 4 shorthands.
- **Isolation:** each tab's page runs in `rustlite --renderer` (one per tab
  per site), driven over framed stdin/stdout IPC with hostile-input
  decoding; a 10 s watchdog replaces hung, crashed or misbehaving renderers
  with an error page; `--single-process` for debugging.
- **Sandbox (Windows):** job object per renderer (1 GiB memory, no child
  processes, UI limits, kill on close) and renderer lockdown (low
  integrity, no privileges, no dynamic code, image-load policy).
  `unsafe` exists only in `crates/sandbox/src/windows.rs`.
- **Verification:** 19/19 layout fixtures (6 new) and 14/14 style fixtures
  match Chromium 152 (≤0.02 px); 36,000 extra release fuzz cases over the
  new layout code, no failures. The sandbox self-test confirms low
  integrity, job membership, denied file writes and process starts.
  Scripted window runs: typing and GET submission through a renderer; three
  tabs gave four processes, each renderer logged a complete lockdown at
  integrity 0x1000, and all renderers exited with the browser.
- **Tests:** fmt ✅ · clippy `-D warnings` ✅ · `cargo test --workspace` ✅ 299 passed
  · release build ✅.
- **Measurements:** binary 11.2 MiB (+0.2 MiB); three tabs 64.8 MB working
  set over 4 processes (browser 23.2 MB, renderers 10.3–16.1 MB); warm
  layout of `article-400` about 10–20% slower than the Phase 8 baseline
  (one run showed ×1.93; repeated runs ×1.05–1.47).
- **Not done:** sandboxes on Linux and macOS; AppContainer (renderers can
  still read files and open sockets on Windows); local `file:` pages read
  their subresources in the renderer; `auto` margins for absolutely
  positioned boxes; grid, tables, sticky positioning.
- **Next:** see [roadmap.md](roadmap.md).

### 2026-10-17 — Sandbox hardening: brokered files and fonts, restricted token, Unix sandboxes, dependency audit

- **Scope chosen with the user:** the security follow-ups first; Linux and
  macOS sandboxes with `libc` and our own code (asked and decided).
- **Broker:** renderers no longer read font files or local subresources.
  The browser sends its face list at start and answers upcalls for font
  data, fallback faces and local files (only inside the document's
  directory). Display lists name fonts by face-list position.
- **Windows:** renderers start suspended with a restricted lockdown token
  and a warm-up impersonation token they drop before reading input; the
  job is assigned before they run; only their stdio handles are inherited.
- **Linux/macOS:** Landlock + seccomp-bpf and a `sandbox_init` profile,
  applied by the renderer at start.
- **CI:** `cargo deny` job (advisories, licenses, bans, sources).
- **Verification:** the sandbox self-test now also requires file reads and
  a TCP connect to fail — all four probes denied on Windows 11. A page with
  a local stylesheet, script and image renders identically through the
  broker, a `../` stylesheet is refused, and a Sinhala fallback face is
  chosen by the browser. Window run: three tabs, every renderer locked
  down, all exited with the browser. Linux/macOS code compiles and passes
  clippy for those targets; CI run 38986502575 then ran the full self-test
  and broker tests on ubuntu-latest and macos-latest: all passed, every
  renderer fully locked down. The first audit run flagged `rustybuzz` and
  `ttf-parser` as unmaintained and our own crates as unlicensed; the
  former are acknowledged in `deny.toml`, the latter excluded as private.
- **Tests:** fmt ✅ · clippy `-D warnings` ✅ · `cargo test --workspace` ✅ 302 passed.
- **Measurements:** binary 11.25 MiB; three tabs 70.8 MB working set
  (browser 28.7 MB, renderers 10.4/15.1/16.5 MB). A first run showed
  96.4 MB because each font crossing the pipe was copied three times in
  the renderer; fixed before this measurement.
- **Not done:** seccomp allowlist; deny-default macOS profile; memory
  limits outside Windows; replacing `rustybuzz`/`ttf-parser` (done in the
  next entry).

### 2026-10-17 — Font stack: harfrust + skrifa replace rustybuzz + ttf-parser

- **Why:** `cargo deny` flagged `rustybuzz` (RUSTSEC-2026-0206) and
  `ttf-parser` (RUSTSEC-2026-0192) as unmaintained. Proposal, prototype
  numbers and risks in ADR-0007; the dependency change was approved by the
  user before implementation.
- **What:** shaping on `harfrust 0.14` (an owned `Font` per face replaces
  the `self_cell` shaping cache); metrics and cmap from its `read-fonts`
  model; outlines for our rasterizer from `skrifa 0.48`. `fontdb 0.24`
  stays (it no longer uses `ttf-parser`). `FontBroker::face_data` and
  `FontContext::face_data` return `Arc<[u8]>`. Fallback search skips
  symbol-encoded faces (Wingdings), as before.
- **Evidence:** a scratch prototype gave identical glyph IDs, advances and
  line metrics for Latin and Sinhala on 5 Windows fonts, shaping 5–48 %
  faster, and ~13–19 KB heap per face instead of ~205–267 KB. The
  rasterizer compared against the old ttf-parser path on 7 fonts × 3
  sizes: 70,720 of 70,734 glyph bitmaps identical; the rest differ by
  ≤5/255 except one unnamed Times New Roman glyph that skrifa places at
  its `hmtx` side bearing, as FreeType does. That case showed the bitmap
  must be sized from the drawn outline, not the `glyf` header box, which
  is now done.
- **Tests:** fmt ✅ · clippy `-D warnings` ✅ · `cargo test --workspace` ✅ 302
  passed, including 19/19 layout fixtures vs Chromium 152. Cross-target:
  `rustlite-fonts` passes clippy for x86_64 Linux and aarch64 macOS, and
  winit 0.30.13 with the new feature set compiles for Linux; the full
  workspace could not be cross-checked locally (`ring` needs a C cross
  compiler), so CI covers Linux and macOS.
- **Measurements:** release `rustlite.exe` 11,795,456 → 12,236,288 bytes
  (+0.42 MiB, +3.7 %; the prototype estimated +0.85 MiB).
- **cargo deny:** both ignores removed from `deny.toml`; not installed
  locally, so CI runs the check. The new crates have no RustSec entries and
  their licenses are all on the allow-list. `ttf-parser` also came in
  through `winit` → `sctk-adwaita` → `ab_glyph` (Wayland title-bar text);
  with the user's go-ahead winit now uses `wayland-csd-adwaita-notitle`
  instead of its default `wayland-csd-adwaita`, so Wayland compositors
  without server-side decorations (GNOME) draw the title bar without its
  text. `ttf-parser`, `owned_ttf_parser`, `ab_glyph`, `rustybuzz` and
  `self_cell` are no longer in `Cargo.lock`.

### 2026-10-17 — Linux renderer seccomp filter: denylist → allowlist

- **What:** the seccomp-bpf filter in `crates/sandbox/src/linux.rs` is now
  default-deny. Only the calls a locked-down renderer makes are allowed
  (stdio pipe I/O, memory, threads, futexes, signals to itself, clocks,
  sleep, `getrandom`); the rest return `EPERM`. Arguments are checked
  where it is cheap: `mmap`/`mprotect` without `PROT_EXEC`, `clone` only
  with glibc's thread flags (`clone3` → `ENOSYS` so glibc falls back),
  `tgkill` only to the renderer's own pid, `ioctl` only `TCGETS`, `fcntl`
  and `prctl` limited to a few commands. Each rule's BPF body ends in a
  return, so jumps stay local and the list can grow without overflowing
  the 8-bit jump offsets. The self-test gained `unlisted_syscall`
  (`getppid` must fail with `EPERM`).
- **How the list was derived:** from the renderer's code path (single
  thread after lockdown; stdio IPC; logging to stderr; fonts and files by
  upcall), then confirmed in CI with a new non-gating step,
  `scripts/renderer-syscalls.sh`, which runs the renderer tests under
  `strace -ff` and lists each renderer's calls after the filter went in.
  Run 39049665123: 6 renderers traced; after lockdown they made `read` (76),
  `write` (69), `brk` (20), `mmap` (8), `munmap` (5), `sigaltstack`,
  `exit_group`, plus the self-test's probes; the only refusals were the
  probes (`openat` ×3, `getcwd` ×2, `unlink`, `socket`, `getppid`).
- **Tests:** Windows: fmt ✅ · clippy `-D warnings` ✅ · `cargo test
  --workspace` ✅ 302 passed. `rustlite-sandbox` passes clippy `-D
  warnings` for x86_64 and aarch64 Linux locally. CI run 39049665123: all four
  jobs green (ubuntu, macos, windows, cargo deny); ubuntu ran 304 tests,
  including four `linux::tests` that run the filter through a small BPF
  interpreter (allowed, refused and argument-checked calls, foreign
  architecture, x32); all 3 `--renderer` processes logged a complete
  lockdown, none an incomplete one.
- **Not covered:** aarch64 Linux is compiled and clippy-checked but not
  run; the windowed browser on Linux has not been run under the
  allowlist (no display in CI).
