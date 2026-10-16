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
| 6 | HTTP(S), URL resolution, redirects, errors | Loads local test server + selected pages | Planned |
| 7 | Address bar, navigation, history, loading state, tabs | Navigate and recover from failures | Planned |
| 8 | Benchmarks, memory reporting, bounded caches, incremental invalidation | Measured, regression-tested | Planned (frame-time stats exist) |
| 9 | JS runtime (Boa evaluation), first DOM APIs | Simple interactive pages | Planned |
| 10 | Advanced CSS, Web APIs, isolation, security | Every claim tested + documented | Planned |

## Next milestone: Phase 6, networking

1. `crates/net`: URL parsing and resolution (`url` crate), an HTTP(S) client
   with rustls and platform certificate validation, run off the UI thread;
   redirects (bounded), timeouts, response-size limits, cancellation of stale
   navigations, MIME sniffing basics and charset decoding.
2. Navigation state machine in `browser-core` (idle → loading → committed /
   failed) with stale-result protection, and an error page.
3. Subresources: `<link rel=stylesheet>` and `<img>` over HTTP(S), deduplicated
   and bounded; images move off the UI thread.
4. Desktop: address bar text input, Go/Reload/Back/Forward.
5. Tests against a local test server (no external network in CI).

Still open from earlier phases: inline backgrounds/borders, `inline-block`,
floats/positioning, 3D border shading, broken-image rendering, GPU backend.
