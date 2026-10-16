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
| NET-001 | HTTP(S) loading | Planned | — |
| NAV-001 | Navigation state machine | Planned | — |
| JS-001 | JS runtime | Deferred (Phase 9) | — |

## Platform validation

| Platform | Build | Tests | Window | Notes |
|---|---|---|---|---|
| Windows 11 x64 (MSVC) | ✅ | ✅ 165/165 | ✅ | Ryzen 5 5600G, Radeon iGPU |
| Linux x64 | ✅ CI | ✅ CI | Unverified | CI (ubuntu-latest) green for Phases 1–3; no display in CI |
| macOS | ✅ CI | Pending re-run | Unverified | Style golden fixed in `ed08a83` (passed in run 38863957147); layout text goldens failed there because macOS's Times New Roman has different vertical metrics, now skipped by a metrics check |

## Known limitations

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
