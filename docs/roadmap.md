# Roadmap

Phases follow master specification §20. A phase is "done" only when its
acceptance criteria pass; see [progress.md](progress.md) for live status.

| Phase | Scope | Acceptance | Status |
|---|---|---|---|
| 0 | Repo, tooling, docs, REA readiness | Builds, or blockers documented | **Done** (2026-10-16) |
| 1 | Window, input, resize, logging, basic drawing | Window shows a basic scene on every available platform | **Done on Windows**; Linux/macOS unverified (CI configured, not yet run) |
| 2 | HTML tokenizer, tree builder, DOM | Representative fixtures → deterministic DOM trees | **Done** (2026-10-16): 32/32 fixtures match Chromium |
| 3 | CSS parser, selectors, cascade, computed style, UA stylesheet | Fixtures → expected computed styles | **Done** (2026-10-16): 11/11 fixtures match Chromium |
| 4 | Block/inline layout, text measurement, line breaking, scrolling | Simple pages' geometry within tolerance | **Done** (2026-10-16): 9/9 fixtures within 0.1px of Chromium |
| 5 | Display lists for text/images, clipping, GPU presentation | Renders a simple HTML page without another engine | **Partly done**: text, boxes, clipping, device scale render; images, list markers, GPU remain |
| 6 | HTTP(S), URL resolution, redirects, errors | Loads local test server + selected pages | Planned |
| 7 | Address bar, navigation, history, loading state, tabs | Navigate and recover from failures | Planned |
| 8 | Benchmarks, memory reporting, bounded caches, incremental invalidation | Measured, regression-tested | Planned (frame-time stats exist) |
| 9 | JS runtime (Boa evaluation), first DOM APIs | Simple interactive pages | Planned |
| 10 | Advanced CSS, Web APIs, isolation, security | Every claim tested + documented | Planned |

## Next milestones

**Phase 5 completion**
1. List markers (`list-style-type` disc/decimal) and `::marker` boxes.
2. Replaced elements: `<img>` with intrinsic/attribute sizes (decoding needs the
   `image` crate; local files first), inline-block atomic inlines.
3. Font memory: share memory-mapped font data instead of copying, and rasterize
   glyphs lazily per glyph (fontdue parses whole faces); target the 73.6 MB
   sample-page working set.
4. Border styles (dashed, dotted, double), inline backgrounds/borders.

**Phase 6 networking** (see master spec §12): `url` resolution, an HTTP(S)
client with rustls and certificate validation off the UI thread, redirects,
size limits, cancellation, `<link rel=stylesheet>`, and an error page.
