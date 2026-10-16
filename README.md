# RustLite

An original, cross-platform web browser engine written in Rust: HTML, DOM, CSS,
layout, paint and rendering are RustLite code, not a wrapped engine.

**Status:** Phase 5. RustLite opens and renders local HTML files with its own
HTML parser, CSS engine, block/inline layout, glyph rasterizer and CPU
renderer (each verified against Chromium on golden fixtures), including local
images and list markers. Normal flow only; no floats, flex/grid, networking or
JavaScript yet. See [docs/roadmap.md](docs/roadmap.md) and
[docs/progress.md](docs/progress.md).

## Build and run

Requires Rust 1.95 (pinned in `rust-toolchain.toml`).

```bash
cargo run --release
```

Options:

```
--size <WxH>              Initial window size (default 1024x768)
--headless <PNG>          Render one frame to a PNG without a window
--exit-after-frames <N>   Close after N frames (smoke test)
--debug-overlay           Outline every painted rectangle (toggle: F12)
--dump-dom <HTML_FILE>    Print the parsed DOM of a local file (html5lib format)
--dump-style <HTML_FILE>  Print computed styles of a local file (viewport = --size)
--dump-layout <HTML_FILE> Print element boxes after layout (viewport = --size)
```

Open a page: `cargo run --release -- tests/pages/sample.html`

Shortcuts: Ctrl+Q / Ctrl+W (Cmd on macOS) quit · F12 debug overlay ·
wheel, ↑/↓, Page Up/Down, Space/Shift+Space, Home/End scroll.
Logging: `RUST_LOG=debug cargo run`.

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
