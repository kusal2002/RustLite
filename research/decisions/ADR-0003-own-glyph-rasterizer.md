# ADR-0003: Replace fontdue with an on-demand glyph rasterizer

- Status: Accepted (supersedes the rasterization part of ADR-0002)
- Date: 2026-10-16

## Context

With fontdue, the sample page used **73.6 MB** working set (62.7 MB private)
versus 16.8 MB for a blank window. fontdue's `Font::from_bytes` parses and
stores every glyph outline of a face up front; the page touches a few Latin
faces plus a large Sinhala fallback face (Nirmala UI), so most of that memory
held outlines that were never drawn. Memory is RustLite's first priority.

## Decision

Remove fontdue. Rasterize glyphs in `rustlite-fonts` (`raster.rs`): read the
outline of the one requested glyph through `ttf-parser`'s `outline_glyph`,
flatten curves, and accumulate exact signed coverage per cell (the font-rs
accumulation technique), then convert to an 8-bit mask. Bitmaps stay in the
existing bounded cache (4,096 entries).

Persistent memory mapping of font files (`fontdb::make_shared_face_data`) was
also considered. It is `unsafe` and, after measuring, unnecessary: copying the
few faces in use costs little once whole-face parsing is gone.

## Results (same page, same machine)

| | fontdue | own rasterizer |
|---|---|---|
| Working set | 73.6 MB | 23.9 MB |
| Private bytes | 62.7 MB | 12.1 MB |
| Release binary | 2.20 MB | 2.18 MB |

Visual quality is equivalent (both unhinted grayscale coverage).

## Consequences

- One fewer dependency; one font parser (ttf-parser) instead of two.
- The rasterizer is RustLite code with unit tests (single-cell area split,
  closed shapes, a real "O" with an empty counter).
- Still no hinting, subpixel AA or color glyphs (COLR/CBDT/sbix).
