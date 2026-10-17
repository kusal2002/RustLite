# RustLite

An original, cross-platform web browser engine written in Rust: HTML, DOM, CSS,
layout, paint and rendering are RustLite code, not a wrapped engine.

**Status:** Phase 10 done. RustLite browses: it loads pages over
HTTP(S) with certificate validation, follows links, keeps history, has tabs
and find-in-page, runs page JavaScript (Boa engine, a DOM subset, timers,
animation frames, events), has working form fields and GET forms, and
renders with its own HTML parser, CSS engine, layout (normal flow, floats,
positioning, inline-block, flexbox), glyph rasterizer and CPU renderer
(each verified against Chromium on golden fixtures). Each tab's page runs in
a separate renderer process that can open no files, make no network
connections and start no processes (verified on Windows; the Linux and
macOS sandboxes are new and not yet verified). No grid or tables. See
[docs/security.md](docs/security.md) before browsing untrusted sites. See [docs/roadmap.md](docs/roadmap.md) and
[docs/progress.md](docs/progress.md).

## Build and run

Requires Rust 1.95 (pinned in `rust-toolchain.toml`).

```bash
cargo run --release -p rustlite-desktop
```

Options:

```
--size <WxH>              Initial window size (default 1024x768)
--headless <PNG>          Render one frame to a PNG without a window
--exit-after-frames <N>   Close after N frames (smoke test)
--debug-overlay           Outline every painted rectangle (toggle: F12)
--disable-javascript      Do not run page scripts
--single-process          Render pages in the browser process (no renderer
                          processes or sandbox; for debugging)
--dump-dom <HTML_FILE>    Print the parsed DOM of a local file (html5lib format)
--dump-style <HTML_FILE>  Print computed styles of a local file (viewport = --size)
--dump-layout <HTML_FILE> Print element boxes after layout (viewport = --size)
```

Open a page: `cargo run --release -p rustlite-desktop -- https://example.com/` or a local file:
`cargo run --release -p rustlite-desktop -- tests/pages/sample.html`. Without an argument the
address bar is focused; type a URL or path and press Enter.

Shortcuts (Cmd instead of Ctrl on macOS): Ctrl+L / Alt+D / F6 address bar ·
Alt+←/→ back/forward · F5 / Ctrl+R reload · Esc stop · Ctrl+T new tab ·
Ctrl+W close tab · Ctrl+Tab / Ctrl+Shift+Tab / Ctrl+1–9 switch tabs ·
Ctrl+F find (Enter / Shift+Enter, F3 next/previous) · Tab / Shift+Tab move
between form fields · Ctrl+C/X/V in text
fields · Ctrl+Q quit · F12 debug overlay · wheel, ↑/↓, Page Up/Down,
Space/Shift+Space, Home/End scroll. Click links to follow them;
Ctrl+click or middle-click opens them in a background tab.
Logging: `RUST_LOG=debug cargo run -p rustlite-desktop`.

## Checks

```bash
./scripts/check.sh   # or scripts/check.ps1 on Windows
```

## Docs

[Architecture](docs/architecture.md) · [Roadmap](docs/roadmap.md) ·
[Compatibility](docs/compatibility.md) · [Performance](docs/performance.md) ·
[Security](docs/security.md) · [Testing](docs/testing.md) ·
[REA workflow](docs/rea-workflow.md) · [Research](research/README.md)

## License

Not yet chosen.
