# Progress tracker

Statuses: Planned · Investigating · Specified · Implementing · Implemented ·
Tested · Verified on reference browser · Blocked · Deferred.

## Feature matrix

| ID | Feature | Status | Evidence |
|---|---|---|---|
| PLAT-001 | Desktop window (winit) | Tested (Windows) | `rustlite --exit-after-frames 120` ran, exit 0 |
| PLAT-002 | Resize + DPI-aware toolbar layout | Implemented | `toolbar::tests::layout_scales_with_dpi`; manual resize not yet scripted |
| PLAT-003 | Keyboard shortcuts (Ctrl/Cmd+Q/W quit, F12 overlay) | Tested | `commands::tests` |
| PLAT-004 | Mouse hover/click hit-testing on toolbar | Tested | `toolbar::tests::hit_testing` |
| RENDER-001 | Display list (fill, border, clip) | Tested | `rustlite-paint` unit tests |
| RENDER-002 | CPU rasterizer with alpha + clipping | Tested | `rustlite-renderer` unit tests |
| RENDER-003 | Headless frame → PNG | Tested | `tests/png_roundtrip.rs`, `apps/desktop/tests/smoke.rs` |
| RENDER-004 | Paint debug overlay | Tested | `frame::tests::debug_overlay_outlines_rects` |
| CORE-001 | Page with cached display list + invalidation | Tested | `browser-core` tests |
| DIAG-001 | Structured logging (`RUST_LOG`) | Implemented | Manual run |
| DIAG-002 | Frame-time stats (min/mean/p95/max) | Tested | `frame_stats` tests |
| HTML-001 | HTML tokenizer | Planned | — |
| DOM-001 | Arena DOM | Planned | — |
| CSS-001 | CSS parser | Planned | — |
| TEXT-001 | Text shaping/rendering | Planned | — |
| NET-001 | HTTP(S) loading | Planned | — |
| NAV-001 | Navigation state machine | Planned | — |
| JS-001 | JS runtime | Deferred (Phase 9) | — |

## Platform validation

| Platform | Build | Tests | Window | Notes |
|---|---|---|---|---|
| Windows 11 x64 (MSVC) | ✅ | ✅ 47/47 | ✅ | Ryzen 5 5600G, Radeon iGPU |
| Linux x64 | Unverified | Unverified | Unverified | CI job defined in `.github/workflows/ci.yml`, not yet run |
| macOS | Unverified | Unverified | Unverified | CI job defined, not yet run |

## Known limitations

- Only `about:blank` exists; no HTML/CSS/layout yet.
- Toolbar has no text (no font stack yet); buttons only log clicks.
- No vsync/frame pacing (softbuffer on Windows presents immediately).
- Window path has no automated test that runs without a display.
- Single process, no sandboxing — not secure for untrusted content (nothing
  untrusted is loaded yet).

## Milestone log

### 2026-10-16 — Phase 0 + Phase 1 foundation

- **What:** Cargo workspace (5 crates), desktop window with toolbar scene, CPU
  rasterizer, PNG headless capture, logging, frame stats, docs, CI, scripts.
- **Why:** Smallest end-to-end vertical slice: display list → raster → window.
- **Tests:** `cargo fmt --check` ✅ · `cargo clippy --workspace --all-targets -D warnings` ✅ ·
  `cargo test --workspace` ✅ 47 passed · `cargo build --release` ✅.
- **Measurements:** see [performance.md](performance.md).
- **Next:** Phase 2 (DOM + HTML tokenizer), see [roadmap.md](roadmap.md).
