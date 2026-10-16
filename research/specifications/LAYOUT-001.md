# LAYOUT-001: Block and inline layout (first layer)

- **Reference browser:** Chromium 152.0.7977.130 (Claude desktop built-in browser
  pane), device pixel ratio 1
- **OS / fonts:** Windows 11 Pro 10.0.26200; Times New Roman (system copy)
- **Observation tool:** `scripts/oracle/server.py` `/layout` page: each fixture is
  rendered in an 800×600 `srcdoc` iframe and `getBoundingClientRect()` is read for
  every element. **REA was not used**: geometry needs only DOM APIs, no CDP.
- **Date:** 2026-10-16

## Scenario

9 fixtures in `tests/layout/*.html`:

- Without text (tolerance 0.01px): box model (padding, borders, fixed and
  percentage widths, `auto` margins, over-constrained margins, nesting); margin
  collapsing (siblings, parent/first-child, parent/last-child, collapsing through
  empty boxes, negative margins, borders/padding/`overflow: hidden` blocking it);
  `html`/`body` margins; `display: none` / `display: contents`.
- With text (tolerance 0.1px, require Times New Roman): line heights (`normal`,
  number, px, %), font sizes, wrapping at 300px, `text-align` center/right, an
  unbreakable long word, inline elements (`span`, `b`, `i`, mixed sizes), `<br>`
  (including an empty line), whitespace collapsing across elements, anonymous
  block boxes, and a document using only UA styles (headings, paragraph, list,
  blockquote).

## Direct observations

- First comparison: 6/9 fixtures matched. Differences:
  1. `<br>` reports a zero-width box (height = font ascent + descent) at the end of
     its line; RustLite generated no box for it.
  2. A box whose margins collapse through it is positioned as if it had a bottom
     border (CSS 2.1 §8.3.1): 40px difference.
  3. Anonymous block boxes were 6px too tall: the initial *computed* border width
     was stored as 3px instead of 0 (border-style `none`), a style-crate bug only
     anonymous boxes exposed.
- After fixing those, all 9 matched with 1px tolerance, but inline boxes sat
  exactly 0.5px lower than Chromium's. Blink floors the half-leading above the
  baseline (remainder below); RustLite now does the same.
- Final maximum deviation across all text fixtures: **0.032px** (consistent with
  Blink's 1/64px LayoutUnit rounding). Line-height `normal` for Times New Roman at
  16px is 18px in both (ascent 14 + descent 3 + line gap 1, each rounded).

## Inferences

- Matching to 0.03px implies rustybuzz advances match Chromium's HarfBuzz shaping
  for this font, and Blink rounds ascent/descent/line-gap to whole pixels.

## Unknowns / not covered

- Floats, positioning, flexbox, grid, tables (laid out as blocks), inline-block,
  replaced elements (images, form controls), list markers, `white-space` values
  other than `normal`, break opportunities other than spaces (CJK, hyphens,
  UAX #14), bidi, inline padding/borders/margins, `vertical-align`, min/max sizes.
- Fonts other than Times New Roman; macOS/Linux font metrics (not observed).
  CI showed macOS's Times New Roman has identical advances but different
  vertical metrics (16px lines instead of 18px), so text fixtures now declare
  the observed font's 16px metrics (`14 3 1`) and skip when they differ.

## Implementation status

Tested; Verified on reference browser for the fixtures listed above.
