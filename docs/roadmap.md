# Roadmap

Phases follow master specification §20. A phase is "done" only when its
acceptance criteria pass; see [progress.md](progress.md) for live status.

| Phase | Scope | Acceptance | Status |
|---|---|---|---|
| 0 | Repo, tooling, docs, REA readiness | Builds, or blockers documented | **Done** (2026-10-16) |
| 1 | Window, input, resize, logging, basic drawing | Window shows a basic scene on every available platform | **Done on Windows**; Linux/macOS unverified (CI configured, not yet run) |
| 2 | HTML tokenizer, tree builder, DOM | Representative fixtures → deterministic DOM trees | **Done** (2026-10-16): 32/32 fixtures match Chromium |
| 3 | CSS parser, selectors, cascade, computed style, UA stylesheet | Fixtures → expected computed styles | **Done** (2026-10-16): 11/11 fixtures match Chromium |
| 4 | Block/inline layout, text measurement, line breaking, scrolling | Simple pages' geometry within tolerance | Next |
| 5 | Display lists for text/images, clipping, GPU presentation | Renders a simple HTML page without another engine | Planned |
| 6 | HTTP(S), URL resolution, redirects, errors | Loads local test server + selected pages | Planned |
| 7 | Address bar, navigation, history, loading state, tabs | Navigate and recover from failures | Planned |
| 8 | Benchmarks, memory reporting, bounded caches, incremental invalidation | Measured, regression-tested | Planned (frame-time stats exist) |
| 9 | JS runtime (Boa evaluation), first DOM APIs | Simple interactive pages | Planned |
| 10 | Advanced CSS, Web APIs, isolation, security | Every claim tested + documented | Planned |

## Next milestone: Phase 4, layout (first layer)

1. `crates/layout`: box tree from DOM + computed styles (block, inline,
   anonymous block wrappers, `display: none` pruning).
2. Block formatting context: containing blocks, `width`/`height`/`auto`,
   margins/padding/borders, percentage resolution, vertical margin collapsing.
3. Inline formatting context: whitespace processing, line boxes, line breaking
   at break opportunities with a text-measurement trait.
4. `crates/fonts`: font loading and metrics for measurement (evaluate
   rustybuzz/fontdue vs. alternatives; record an ADR).
5. Paint the box tree into the display list (backgrounds, borders); text
   painting follows with glyph rasterization.
6. Golden geometry tests: `getBoundingClientRect` from Chromium via the oracle,
   compared within a documented tolerance.
