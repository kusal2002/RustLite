# Roadmap

Phases follow master specification §20. A phase is "done" only when its
acceptance criteria pass; see [progress.md](progress.md) for live status.

| Phase | Scope | Acceptance | Status |
|---|---|---|---|
| 0 | Repo, tooling, docs, REA readiness | Builds, or blockers documented | **Done** (2026-10-16) |
| 1 | Window, input, resize, logging, basic drawing | Window shows a basic scene on every available platform | **Done on Windows**; Linux/macOS unverified (CI configured, not yet run) |
| 2 | HTML tokenizer, tree builder, DOM | Representative fixtures → deterministic DOM trees | **Done** (2026-10-16): 32/32 fixtures match Chromium |
| 3 | CSS parser, selectors, cascade, computed style, UA stylesheet | Fixtures → expected computed styles | Next |
| 4 | Block/inline layout, text measurement, line breaking, scrolling | Simple pages' geometry within tolerance | Planned |
| 5 | Display lists for text/images, clipping, GPU presentation | Renders a simple HTML page without another engine | Planned |
| 6 | HTTP(S), URL resolution, redirects, errors | Loads local test server + selected pages | Planned |
| 7 | Address bar, navigation, history, loading state, tabs | Navigate and recover from failures | Planned |
| 8 | Benchmarks, memory reporting, bounded caches, incremental invalidation | Measured, regression-tested | Planned (frame-time stats exist) |
| 9 | JS runtime (Boa evaluation), first DOM APIs | Simple interactive pages | Planned |
| 10 | Advanced CSS, Web APIs, isolation, security | Every claim tested + documented | Planned |

## Next milestone: Phase 3, CSS (first layer)

1. `crates/css`: CSS Syntax Level 3 tokenizer (idents, strings, numbers,
   dimensions, percentages, hashes, functions, at-keywords, comments, error
   tokens) and a stylesheet parser (qualified rules, declarations, `!important`,
   at-rule skipping with error recovery).
2. Selectors: type, universal, class, ID, descendant and child combinators,
   selector lists, specificity.
3. `crates/style`: user-agent stylesheet (display, margins, headings, lists,
   `display:none` for `head` etc.), cascade by origin/importance/specificity/order,
   inheritance, and computed values for display, width/height, margin/padding,
   border, color, background-color, font-size/family/weight, text-align, overflow.
4. Collect `<style>` elements and `style=""` attributes from the DOM.
5. Golden tests: fixtures → computed styles compared against Chromium
   `getComputedStyle` via the same oracle approach as HTML-001.
