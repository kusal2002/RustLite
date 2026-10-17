# ADR-0002: Font stack — fontdb + rustybuzz + fontdue

- Status: Accepted; rasterization superseded by [ADR-0003](ADR-0003-own-glyph-rasterizer.md);
  shaping and font tables superseded by [ADR-0007](ADR-0007-font-stack-harfrust-skrifa.md)
- Date: 2026-10-16

## Context

Phase 4 (layout) needs to measure text, and painting needs glyph bitmaps. The
master specification requires RustLite's own line breaking, positioning and
layout, but allows mature libraries for font discovery, shaping and
rasterization. It names rustybuzz and fontdue as candidates.

## Decision

| Concern | Crate | Version | License |
|---|---|---|---|
| System font discovery and CSS-style matching | `fontdb` | 0.24 | MIT |
| Shaping (glyph selection, advances, kerning, ligatures) | `rustybuzz` | 0.20 | MIT |
| Font tables / metrics / cmap coverage | `ttf-parser` (via both above) | 0.25 | MIT OR Apache-2.0 |
| Glyph rasterization (coverage bitmaps) | `fontdue` | 0.9 | MIT OR Apache-2.0 OR Zlib |

These sit behind `rustlite-fonts`, and the renderer only sees a
`GlyphRasterizer` trait defined in `rustlite-paint`.

## Rationale

- All are pure Rust and work the same on Windows, macOS and Linux (no FreeType,
  DirectWrite or CoreText bindings), so text metrics are reproducible across
  platforms given the same font files.
- rustybuzz is a port of HarfBuzz, which Chromium also uses for shaping, so
  advances should match the reference browser closely for the same font.
- Line breaking, whitespace processing, line-box construction and baseline
  alignment stay in `rustlite-layout`, as the specification requires.

## Alternatives considered

- **cosmic-text:** does shaping *and* line layout. Rejected: it would replace
  the layout logic RustLite must implement itself.
- **swash:** shaping and rasterization in one crate. Viable; rustybuzz was
  chosen for closer HarfBuzz parity. Revisit if two font parsers become a cost.
- **Platform text APIs:** best platform fidelity, but three code paths and less
  reproducible tests.

## Consequences

- Fonts are parsed twice (ttf-parser for shaping/metrics, fontdue for
  rasterization). The fontdue parse happens lazily, only for faces that paint.
- fontdue has no hinting or subpixel AA; glyphs look softer than Chromium on
  Windows. Pixel-exact screenshot comparison with Chromium is not a goal.
- No color emoji (COLR/CBDT/sbix) yet; such glyphs rasterize from outlines if
  present, otherwise fall back.
- Text fixtures need the same fonts as the reference machine. Tests that depend
  on a specific font skip with a message when it is not installed.
