# Changelog

## Unreleased

### Added — 2026-10-16 (Phase 0 + 1)
- Cargo workspace: `rustlite-desktop`, `rustlite-browser-core`, `rustlite-paint`,
  `rustlite-renderer`, `rustlite-diagnostics`.
- Cross-platform window (winit 0.30) presenting a CPU-rasterized frame (softbuffer 0.4).
- Toolbar scene with DPI-aware layout, hover highlight and hit testing.
- Display list (fill, border, clip) and CPU rasterizer with alpha blending and clipping.
- Headless rendering to PNG (`--headless`), dependency-free PNG encoder.
- `about:blank` page with cached, invalidation-driven display list.
- Structured logging (`RUST_LOG`), frame-time statistics.
- Shortcuts: Ctrl/Cmd+Q/W quit, F12 paint debug overlay.
- Docs, ADR-0001, check scripts, CI matrix (Windows/Linux/macOS).
