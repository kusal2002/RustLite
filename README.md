# RustLite

An original, cross-platform web browser engine written in Rust: HTML, DOM, CSS,
layout, paint and rendering are RustLite code, not a wrapped engine.

**Status:** Phase 3. It opens a window with a toolbar rendered by RustLite's
own CPU rasterizer, parses HTML into a DOM, and computes CSS styles (both
verified against Chromium on golden fixtures). It does not lay out, render,
or load web pages yet. See [docs/roadmap.md](docs/roadmap.md) and
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
```

Shortcuts: Ctrl+Q / Ctrl+W (Cmd on macOS) quit · F12 debug overlay.
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
