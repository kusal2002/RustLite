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

Status today (Phase 4): `browser-core` owns a `Page` that is either
`about:blank` or an HTML document: parsed by `rustlite-html` into a
`rustlite-dom` tree, styled by `rustlite-style`, laid out by `rustlite-layout`
(using `rustlite-fonts` for shaping and metrics), and painted into a display
list that the CPU renderer rasterizes with glyphs from the font context.
Each stage is cached and invalidated independently (scrolling only repaints). The desktop shell composes
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
| `crates/fonts` | `rustlite-fonts` | Font discovery, selection, fallback, shaping, metrics, glyph raster | Implemented ([ADR-0002](../research/decisions/ADR-0002-font-stack.md)) |
| `crates/layout` | `rustlite-layout` | Box tree, block/inline layout, line breaking, paint, hit testing | Implemented (normal flow) |
| `benchmarks` | `rustlite-bench` | Pipeline benchmark harness (binary + library) | Implemented ([performance.md](performance.md)) |
| `crates/net` | `rustlite-net` | HTTP(S) fetch with limits, redirects, charset decoding, background loader | Implemented ([networking.md](networking.md)) |

Planned crates are created when their first real code lands, not as empty
shells: `platform`,
`js-runtime`, `web-apis`, `storage`. Their responsibilities follow the master
specification §3.

### Dependency direction

```
desktop ──► browser-core ──► net
   │              ├──────────► html ──► dom
   │              ├──────────► style ──► css, dom
   │              ├──────────► layout ─► style, fonts, paint, dom
   │              └──────────► fonts ──► paint
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
| `fontdb 0.24` | System font discovery and CSS-style matching | All (pure Rust; reads fontconfig files on Linux) | MIT | Indexes ~365 faces in ~35 ms. |
| `rustybuzz 0.20` | Shaping (HarfBuzz port) | All | MIT | Pure Rust. |
| `ttf-parser 0.25` | Font tables, metrics, cmap | All | MIT/Apache-2.0 | Shared by fontdb and rustybuzz. |
| `image 0.25` (png, jpeg, gif only) | Image decoding | All | MIT/Apache-2.0 | ~0.45 MiB of binary; limits on dimensions and allocation. |
| `ureq 3.4` (rustls, platform-verifier, gzip) | HTTP/1.1 client | All (ring needs a C compiler) | MIT/Apache-2.0 | Synchronous; run on loader threads. Brings rustls + ring + rustls-platform-verifier |
| `arboard 3.7` (no default features) | System clipboard text | All (X11 on Linux; no Wayland data-control without the feature) | MIT/Apache-2.0 | Desktop app only; opened lazily on first copy/paste |
| `self_cell 1.2` | Safe self-referential struct (font bytes + parsed rustybuzz face) | All | Apache-2.0/GPL-2.0 | No dependencies; keeps `unsafe` out of our crates. Same use as cosmic-text |
| `url 2.5` | WHATWG URL parsing and resolution | All | MIT/Apache-2.0 | |
| `encoding_rs 0.8` | WHATWG Encoding Standard decoders | All | MIT/Apache-2.0 + WHATWG BSD-3 data | Encoding tables add ~1 MiB of binary |

Deferred until needed: `wgpu` (GPU backend),
`url`, `rustls`/HTTP client, `image`, `boa_engine`, `rusqlite`, `criterion`,
`proptest`. Each will be evaluated when its milestone starts.

Release binary (Windows x64, thin LTO, `panic=abort`): **4.6 MiB** (2.5 MiB before networking).
Glyph rasterization is RustLite code ([ADR-0003](../research/decisions/ADR-0003-own-glyph-rasterizer.md));
fontdue was removed.
