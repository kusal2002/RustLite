# Roadmap

Phases follow master specification §20. A phase is "done" only when its
acceptance criteria pass; see [progress.md](progress.md) for live status.

| Phase | Scope | Acceptance | Status |
|---|---|---|---|
| 0 | Repo, tooling, docs, REA readiness | Builds, or blockers documented | **Done** (2026-10-16) |
| 1 | Window, input, resize, logging, basic drawing | Window shows a basic scene on every available platform | **Done on Windows**; Linux/macOS unverified (CI configured, not yet run) |
| 2 | HTML tokenizer, tree builder, DOM | Representative fixtures → deterministic DOM trees | Next |
| 3 | CSS parser, selectors, cascade, computed style, UA stylesheet | Fixtures → expected computed styles | Planned |
| 4 | Block/inline layout, text measurement, line breaking, scrolling | Simple pages' geometry within tolerance | Planned |
| 5 | Display lists for text/images, clipping, GPU presentation | Renders a simple HTML page without another engine | Planned |
| 6 | HTTP(S), URL resolution, redirects, errors | Loads local test server + selected pages | Planned |
| 7 | Address bar, navigation, history, loading state, tabs | Navigate and recover from failures | Planned |
| 8 | Benchmarks, memory reporting, bounded caches, incremental invalidation | Measured, regression-tested | Planned (frame-time stats exist) |
| 9 | JS runtime (Boa evaluation), first DOM APIs | Simple interactive pages | Planned |
| 10 | Advanced CSS, Web APIs, isolation, security | Every claim tested + documented | Planned |

## Next milestone: Phase 2, step 1

1. Add `crates/dom`: arena storage with compact `NodeId(u32)`, document /
   element / text / comment / doctype nodes, attributes, parent/child/sibling
   links, append/insert/remove with detached-node safety, debug-tree dump.
2. Add `crates/html`: tokenizer state machine (data, tag open, tag name,
   attribute states, comment, doctype, RCDATA/RAWTEXT for `title`/`style`/`script`),
   named + numeric character references (initial subset, documented).
3. Tree builder: insertion modes `initial` → `in body` subset, implied
   `html/head/body`, void elements, `p` auto-closing, basic misnested-formatting
   recovery; documented unsupported rules.
4. Golden tests in `tests/html/` (input `.html` → expected tree `.txt`),
   comparing against Chrome via REA `inspect-web-page` for a small seed set.
5. `browser-core` gains `Document::Html(dom)` for `--file`-free built-in
   fixtures (no networking yet).
