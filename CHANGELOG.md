# Changelog

## Unreleased

### Added — 2026-10-16 (Phase 8)
- `rustlite-bench`: per-stage pipeline benchmarks with baseline save and
  compare; reference baseline in `benchmarks/`; CI smoke run.
- `Page::memory_report()` (DOM, styles, layout, images, display list, glyph
  and shaping caches), logged by the desktop app and headless renders.
- In-memory HTTP cache for stylesheets and images in `Loader` (32 MiB LRU,
  `Cache-Control: max-age`, never for documents), bypassed by reload.
- `FontContext` word shaping cache (bounded).

### Changed
- Shaping no longer re-parses the font for every word (rustybuzz faces are
  kept per loaded font via `self_cell`): layout ~37× faster on a 3,500-element page.
- Stylist buckets attribute selectors and rejects rules with an ancestor key
  filter: style ~2.4× faster on the same page.
- Paint emits only what the viewport can show; scroll repaint and raster
  cost no longer grow with page length.
- A resize restyles only documents that use viewport units.

### Fixed
- Relative file paths on the command line opened `https://<first segment>/…`
  (an error page) instead of the file; CI's headless frame was affected.

### Added — 2026-10-16 (Phase 7)
- Tabs: tab strip with close and new-tab buttons, keyboard switching,
  middle-click close, links in background tabs (Ctrl/Cmd+click,
  middle-click). Tabs share one loader (`Tab::with_id_space`,
  `load_id_space`) and one font context (`Page::take_fonts`/`set_fonts`);
  dropping a tab cancels its loads.
- Clipboard copy/cut/paste in the address bar (arboard).
- Find in page: `rustlite_layout::find_text`, `Page::find`, a floating find
  bar with match count, highlights, next/previous and scroll to match.
- Layout text fragments keep their text.

### Changed
- Ctrl/Cmd+W closes the current tab (the window closes with its last tab)
  instead of quitting.

### Added — 2026-10-16 (Phase 6)
- `rustlite-net`: HTTP(S) fetching (ureq + rustls, OS trust store), manual
  redirects with scheme checks, body limits, timeouts, cancellation, charset
  detection (encoding_rs), and a bounded background loader.
- `Tab` navigation in browser-core: stale-result protection, history,
  back/forward/reload/stop, error pages, `file:` only for user navigations,
  `<base href>`, linked stylesheets and images over HTTP, link hit testing.
- Desktop: editable address bar, link clicks, toolbar text and icons,
  loading bar, navigation shortcuts; headless rendering of URLs.
- CSS: `font` shorthand, `min-/max-width/height`, `light-dark()`.
- Docs: networking.md, NET-001, security.md rewritten for network content.

### Added — 2026-10-16 (Phase 5)
- On-demand glyph rasterizer in `rustlite-fonts` (fontdue removed; ADR-0003):
  sample-page memory 73.6 → 23.9 MB.
- `list-style-type` / `list-style`, UA list styles, outside list markers with
  disc/circle/square/decimal/alpha/roman counters, `<ol start>`, `<li value>`.
- Local images: PNG/JPEG/GIF decoding with limits, document-directory access
  policy, `width`/`height` presentational hints, replaced-element sizing,
  inline atomic and block images, bilinear image drawing.
- Dashed, dotted and double border painting.
- Layout goldens run through the full page pipeline (moved to
  `browser-core/tests`); two image fixtures match Chromium 152.

### Added — 2026-10-16 (Phase 4)
- `rustlite-fonts`: system font discovery, CSS-style face selection,
  per-character fallback, rustybuzz shaping, fontdue glyph rasterization with a
  bounded cache (ADR-0002).
- `rustlite-layout`: box tree with anonymous blocks, block layout with margin
  collapsing, inline layout with whitespace collapsing, line breaking, `<br>`,
  half-leading, text alignment, and painting of backgrounds, borders and text.
- Text display items and glyph blending in the renderer; device pixel ratio.
- Desktop: open a local HTML file (`rustlite FILE`), window title from
  `<title>`, wheel and keyboard scrolling; `--dump-layout`; headless rendering
  of files.
- `/layout` oracle page and 9 geometry fixtures matching Chromium 152.

### Fixed
- Initial computed `border-width` is 0 (border-style `none`), not 3px.
- macOS CI: style golden adapts the Windows-recorded default font family.

### Added — 2026-10-16 (Phase 3)
- `rustlite-css`: CSS Syntax 3 tokenizer, rule and declaration parser with
  error recovery, selector parser (type/universal/id/class/attribute,
  combinators, pseudo-classes, pseudo-elements) and specificity.
- `rustlite-style`: UA stylesheet from HTML §15, indexed rule matching, cascade
  (origin, importance, inline, specificity, order), inheritance, computed
  values for 33 longhands, style sharing, Chrome-format resolved values.
- browser-core computes styles lazily and restyles on viewport change.
- `--dump-style` CLI option.
- Style oracle (`/style` page of `scripts/oracle/server.py`, moved from
  `scripts/html-oracle/`) and 11 golden fixtures matching Chromium 152.

### Added — 2026-10-16 (Phase 2)
- `rustlite-dom`: arena DOM with 4-byte `NodeId`, safe mutation, traversal,
  html5lib-format tree dump.
- `rustlite-html`: WHATWG HTML tokenizer and tree builder (all non-template
  insertion modes, adoption agency, foster parenting, quirks mode).
- `Page::load_html` and document title in browser-core.
- `--dump-dom` CLI option.
- Reference-browser oracle (`scripts/html-oracle/server.py`) and 32 golden
  fixtures matching Chromium 152.
- `.gitattributes` enforcing LF line endings.

### Added — 2026-10-16 (Phase 0 + 1)
- Cargo workspace: `rustlite-desktop`, `rustlite-browser-core`, `rustlite-paint`,
  `rustlite-renderer`, `rustlite-diagnostics`.
- Cross-platform window (winit 0.30) presenting a CPU-rasterized frame (softbuffer 0.4).
- Toolbar scene with DPI-aware layout, hover highlight and hit testing.
- Display list (fill, border, clip) and CPU rasterizer with alpha blending and clipping.
- Headless rendering to PNG (`--headless`), dependency-free PNG encoder.
- `about:blank` page with cached, invalidation-driven display list.
- Structured logging (`RUST_LOG`), frame-time statistics.
- Shortcuts: Ctrl/Cmd+Q/W quit, F12 paint debug overlay.
- Docs, ADR-0001, check scripts, CI matrix (Windows/Linux/macOS).
