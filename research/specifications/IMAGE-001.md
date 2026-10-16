# IMAGE-001: Image loading and replaced-element sizing

- **Reference browser:** Chromium 152.0.7977.130 (Claude desktop built-in browser
  pane), device pixel ratio 1
- **OS:** Windows 11 Pro 10.0.26200
- **Observation tool:** `scripts/oracle/server.py` `/layout` page (it now also
  serves `tests/layout/assets/`); `getBoundingClientRect()` per element.
  **REA was not used.**
- **Date:** 2026-10-16

## Scenario

Two fixtures with generated PNG assets (40×20 opaque, 10×30 opaque, 8×8
checkerboard with transparency):

- `images-block` (no text, tolerance 0.01px): `display: block` images with
  intrinsic size; `width` attribute only (ratio preserved); CSS height only;
  both attributes; `width: 25%`; `margin: auto` centering; CSS `width`
  overriding the `width` attribute; padding + border + margin.
- `images-inline` (Times New Roman metrics `14 3 1`, tolerance 0.1px): inline
  images in text lines, a tall image raising the line box, an image forcing a
  line break, adjacent images with margins and borders.

## Direct observations

- Both fixtures matched Chromium on the first run.
- Confirms: `width`/`height` attributes act as presentational hints below
  author CSS; the default sizing algorithm keeps the intrinsic ratio when one
  dimension is specified; an inline replaced element's bottom margin edge sits
  on the baseline and contributes its margin-box height to the line.

## Not covered / known gaps

- Broken images: Chromium shows an icon and/or alt text; RustLite renders
  nothing (0×0 unless sizes are given).
- `object-fit`, `object-position`, `min/max` sizes, `vertical-align`, `srcset`,
  `<picture>`, SVG and WebP/AVIF decoding, animated GIFs (first frame only).
- Network and `data:` URLs (only files inside the document's directory load).
- Image downscaling uses bilinear sampling without mipmaps (aliasing).

## Implementation status

Tested; Verified on reference browser for the fixtures listed above.
