# Roadmap

Phases follow master specification §20. A phase is "done" only when its
acceptance criteria pass; see [progress.md](progress.md) for live status.

| Phase | Scope | Acceptance | Status |
|---|---|---|---|
| 0 | Repo, tooling, docs, REA readiness | Builds, or blockers documented | **Done** (2026-10-16) |
| 1 | Window, input, resize, logging, basic drawing | Window shows a basic scene on every available platform | **Done on Windows** (window verified); Linux/macOS build and tests pass in CI (run 38864693788), windows there not verified (no display) |
| 2 | HTML tokenizer, tree builder, DOM | Representative fixtures → deterministic DOM trees | **Done** (2026-10-16): 32/32 fixtures match Chromium |
| 3 | CSS parser, selectors, cascade, computed style, UA stylesheet | Fixtures → expected computed styles | **Done** (2026-10-16): 11/11 fixtures match Chromium |
| 4 | Block/inline layout, text measurement, line breaking, scrolling | Simple pages' geometry within tolerance | **Done** (2026-10-16): 9/9 fixtures within 0.1px of Chromium |
| 5 | Display lists for text/images, clipping, GPU presentation | Renders a simple HTML page without another engine | **Done on CPU** (2026-10-16): text, boxes, images, list markers, border styles, clipping, device scale. GPU presentation deferred (ADR-0001) |
| 6 | HTTP(S), URL resolution, redirects, errors | Loads local test server + selected pages | **Done** (2026-10-16): local-server tests; example.com; bad certificates refused |
| 7 | Address bar, navigation, history, loading state, tabs | Navigate and recover from failures | **Done** (2026-10-16): address bar, links, history, loading state, error recovery, tabs, clipboard, find in page |
| 8 | Benchmarks, memory reporting, bounded caches, incremental invalidation | Measured, regression-tested | **Done** (2026-10-16): harness + baseline, memory report, bounded glyph/shape/HTTP caches, narrower invalidation; dirty-bit incremental restyle/relayout deferred |
| 9 | JS runtime (Boa evaluation), first DOM APIs | Simple interactive pages | **Done** (2026-10-16): Boa, DOM subset, click events; `tests/pages/interactive.html` works by mouse; mixed content blocked; fuzzing in CI; isolation ADR |
| 10 | Advanced CSS, Web APIs, isolation, security | Every claim tested + documented | **Done** (2026-10-17): Web APIs, forms, keyboard; positioning, floats, inline-block, flexbox (19/19 layout fixtures match Chromium); renderer processes with watchdog; Windows sandbox. Linux/macOS sandboxes pending |

## Next milestones

**Phase 7 follow-ups:** a history list / back-button menu, text selection
and copy in page content, tab drag-reordering, session restore.

**Phase 8 follow-ups:** dirty-bit incremental restyle/relayout, dirty-rect
repaint, HTTP revalidation (`ETag`/`Last-Modified`), faster HTML parsing
(~23 µs per element on the large benchmark).

**Phase 10 follow-ups:**
- ~~Brokered fonts and local files; restricted-token renderers on Windows;
  Linux (Landlock + seccomp) and macOS (`sandbox_init`) sandboxes;
  `cargo deny` in CI~~ — done 2026-10-17, verified on all three platforms.
  Next: seccomp allowlist, deny-default macOS profile, memory limits
  outside Windows; move fonts from the unmaintained `rustybuzz`/`ttf-parser`
  to `harfrust`/`skrifa`.
- Layout: grid, tables, `position: sticky`, `auto` margins on absolutely
  positioned boxes, floats placed mid-line.
- POST forms, `<textarea>`, `<select>`, `fetch`; a graceful script timeout
  instead of losing the renderer.

**Before broad use:** sandboxes verified on every platform, CORS/CSP (see
[security.md](security.md)).

Still open from earlier phases: inline backgrounds/borders, 3D border
shading, broken-image rendering, GPU backend.
