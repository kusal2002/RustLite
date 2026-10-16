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
| CSS-002 | `font` shorthand, `min-/max-width/height`, `light-dark()` | Verified on reference browser (fixture set) | `font-shorthand` style and `min-max-sizes` layout fixtures match Chromium 152 |
| JS-001 | JS runtime | Deferred (Phase 9) | — |

## Platform validation

| Platform | Build | Tests | Window | Notes |
|---|---|---|---|---|
| Windows 11 x64 (MSVC) | ✅ | ✅ 270/270 | ✅ | Ryzen 5 5600G, Radeon iGPU |
| Linux x64 | ✅ CI | ✅ CI (run 38864693788) | Unverified | No display in CI; text layout goldens skip (no Times New Roman) |
| macOS | ✅ CI | ✅ CI (run 38864693788) | Unverified | Layout text goldens pin Times New Roman and skip if its metrics differ from the reference machine |

## Known limitations

- Local documents load images/stylesheets only from their own directory; network
  documents only over http(s). No `data:` URLs, cookies, cache or JavaScript.
- Single process, no sandbox (see [security.md](security.md)).
- Layout covers normal flow only: no floats, positioning, flex, grid, tables,
  images or list markers yet (see compatibility.md).
- CSS gaps (at-rules, `:not()`, `var()`, `calc()`, external sheets) are listed in
  [compatibility.md](compatibility.md#css-css-001--tested-verified-on-reference-browser).
- HTML parser deviations are listed in [compatibility.md](compatibility.md#html-parsing-html-001--tested-verified-on-reference-browser).
- Toolbar has no text (no font stack yet); buttons only log clicks.
- No vsync/frame pacing (softbuffer on Windows presents immediately).
- Window path has no automated test that runs without a display.
- Single process, no sandboxing — not secure for untrusted content (nothing
  untrusted is loaded yet).

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
