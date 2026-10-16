# RustLite

An original, cross-platform web browser engine written in Rust: HTML, DOM, CSS,
layout, paint and rendering are RustLite code, not a wrapped engine.

**Status:** Phase 1 (window + rendering foundation). It opens a window with a
toolbar and an `about:blank` page rendered by RustLite's own CPU rasterizer.
It does not load web pages yet. See [docs/roadmap.md](docs/roadmap.md) and
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
