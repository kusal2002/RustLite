# RustLite architecture

RustLite is an original browser engine written in Rust. Parsing, DOM, CSS,
style, layout, paint and rendering coordination are RustLite code. No existing
browser engine (Chromium/Blink, WebKit, Gecko, Servo, CEF, system WebView) is
used to render pages. Reference browsers are used only as **oracles** for
differential testing and REA observation.

## Pipeline

```
bytes ─► html (tokenizer, tree builder) ─► dom
                                            │
css (tokenizer, parser) ─► style (cascade, computed values)
                                            │
                                  layout (box tree, geometry)
                                            │
                                  paint (display list) ─► renderer (raster) ─► platform (present)
```

Status today (Phase 3): `browser-core` owns a `Page` that is either
`about:blank` or an HTML document parsed by `rustlite-html` into a
`rustlite-dom` tree, with computed styles from `rustlite-style`. With no
layout yet, both paint as a white canvas. The desktop shell composes
browser chrome + page display list and rasterizes it on the CPU into the
window's presentation buffer. Every stage between "document" and "display list"
is still to be built (see [roadmap](roadmap.md)).

## Workspace layout (current)

| Path | Crate | Responsibility | Status |
|---|---|---|---|
| `apps/desktop` | `rustlite-desktop` (bin `rustlite`) | Window, event loop, toolbar, shortcuts, frame composition, headless mode | Implemented |
| `crates/browser-core` | `rustlite-browser-core` | Page/document lifecycle, viewport, cached display list with invalidation | Implemented (about:blank + parsed, styled HTML; no rendering of content) |
| `crates/paint` | `rustlite-paint` | Geometry (`Rect`, `Size`, `Point`), `Color`, `DisplayList` | Implemented |
| `crates/renderer` | `rustlite-renderer` | CPU rasterizer (fills, borders, clips, alpha), PNG encoder | Implemented |
| `crates/diagnostics` | `rustlite-diagnostics` | `tracing` setup, frame-time statistics | Implemented |
| `crates/dom` | `rustlite-dom` | Arena DOM, traversal, mutation, html5lib tree dump | Implemented |
| `crates/html` | `rustlite-html` | WHATWG tokenizer + tree builder | Implemented (see compatibility.md) |
| `crates/css` | `rustlite-css` | CSS tokenizer, rule/declaration parser, selectors + specificity | Implemented (first layer) |
| `crates/style` | `rustlite-style` | UA sheet, matching, cascade, computed values, serialization | Implemented (first layer) |

Planned crates are created when their first real code lands, not as empty
shells: `layout`, `fonts`, `net`, `platform`,
`js-runtime`, `web-apis`, `storage`. Their responsibilities follow the master
specification §3.

### Dependency direction

```
desktop ──► browser-core ──► html ──► dom
   │              ├──────────► style ─► css
   │              │              └────► dom
   │              └──────────► paint
   ├──► renderer ─────────────► paint
   └──► diagnostics
```

Lower layers never depend on higher ones. `paint` and `dom` have no internal dependencies;
`renderer` knows nothing about documents; `browser-core` knows nothing about
windows.

## Key decisions

- **CPU rasterization first, GPU later** — [ADR-0001](../research/decisions/ADR-0001-cpu-raster-first.md).
- **Render straight into the presentation buffer.** `Canvas` wraps the
  `softbuffer` buffer, so a frame costs no extra full-frame copy.
- **Display lists are reused across frames** (`clear()` keeps capacity) and the
  page's list is rebuilt only when invalidated (`Page::needs_paint`).
- **Event-driven redraw.** `ControlFlow::Wait`; redraw is requested only on
  resize, DPI change, hover change, or command. No busy loop.
- **No `unsafe`.** `unsafe_code = "forbid"` is set workspace-wide.
- **Browser UI ≠ page.** Toolbar state lives in the desktop crate; page state
  lives in `browser-core`.

## Dependencies

| Crate | Why | Platforms | License | Cost notes |
|---|---|---|---|---|
| `winit 0.30` | Window + input events | Win/macOS/Linux (X11+Wayland) | Apache-2.0 | Moderate; standard choice. 0.31 is still beta. |
| `softbuffer 0.4` | Present a CPU pixel buffer to a window | Win/macOS/Linux (X11+Wayland, dlopen) | MIT/Apache-2.0 | Small; no GPU stack. |
| `tracing 0.1`, `tracing-subscriber 0.3` | Structured logging | All | MIT | Small. `env-filter` + `fmt` only. |
| `thiserror 2` | Typed errors | All | MIT/Apache-2.0 | Proc-macro, compile-time only. |

Deferred until needed: `wgpu` (GPU backend), `rustybuzz`/`fontdue` (text),
`url`, `rustls`/HTTP client, `image`, `boa_engine`, `rusqlite`, `criterion`,
`proptest`. Each will be evaluated when its milestone starts.

Release binary (Windows x64, thin LTO, `panic=abort`): **1.06 MiB**.
