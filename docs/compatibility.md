# Compatibility

## Platform matrix

| | Windows | Linux | macOS |
|---|---|---|---|
| Rust target | `x86_64-pc-windows-msvc` (verified) | `x86_64-unknown-linux-gnu` | `aarch64-apple-darwin`, `x86_64-apple-darwin` |
| Build prerequisites | MSVC Build Tools + Windows SDK | gcc/clang; X11 and/or Wayland libs at **runtime** (loaded via dlopen) | Xcode command-line tools |
| Windowing | winit (Win32) | winit (X11, Wayland) | winit (AppKit) |
| Presentation | softbuffer (GDI) | softbuffer (X11 SHM / Wayland SHM) | softbuffer (CoreGraphics) |
| GPU backend | Not used yet (planned wgpu: DX12/Vulkan) | Planned wgpu: Vulkan/GL | Planned wgpu: Metal |
| Font discovery | Not implemented | Not implemented | Not implemented |
| Certificate store | Not implemented (planned rustls + platform verifier) | Same | Same |
| Shortcut modifier | Ctrl | Ctrl | Cmd |
| Validation | ✅ build, tests, window | ❌ unverified (CI defined) | ❌ unverified (CI defined) |

## Web platform

### HTML parsing (HTML-001) — Tested, verified on reference browser

Spec: WHATWG HTML §13.2. Evidence: [HTML-001](../research/specifications/HTML-001.md),
`crates/html/tests/golden.rs` (32/32 fixtures match Chromium 152 `DOMParser`).

Supported:
- Full tokenizer state machine except the items below; newline normalization;
  named, legacy, and numeric character references (including C1 remapping
  and the attribute-value exception).
- Tree construction: all insertion modes except template modes; adoption
  agency algorithm; reconstruction of active formatting elements; Noah's Ark
  clause; foster parenting; implied end tags; form pointer; frameset.
- Document mode (no-quirks / limited-quirks / quirks) from the doctype.
- Parse errors are reported (codes follow the standard where implemented).

Not supported yet (intentional, documented deviations):

| Area | Behavior today | Impact |
|---|---|---|
| `<template>` | Ordinary element; no template contents fragment | Template children appear in the main tree |
| SVG / MathML | Parsed as ordinary HTML elements (no foreign content, no CDATA) | Inline SVG structure close, but namespaces/case-adjusted names wrong |
| Script data escape states | `<script><!-- … </script> … -->` not special-cased | Rare legacy script hiding pattern |
| Named references | Legacy set + ~120 common names (not all 2,231) | Rare names stay literal text |
| `<select>` | Classic content model; Chromium's 2025 relaxed parser not followed | Rich content inside `<select>` is dropped |
| Scripting flag | Always "disabled" (`<noscript>` content parsed) | Matches `DOMParser`; differs from navigation once JS exists |
| Encoding | Input must be decoded text; no BOM/meta sniffing | Non-UTF-8 pages need Phase 6 |
| Tree depth | No depth cap (Blink caps at 512) | Very deep documents differ from Chrome in shape |

### DOM (DOM-001) — Tested

Arena storage with 4-byte `NodeId`; document/doctype/element/text/comment
nodes; parent/child/sibling links; insert/append/detach/reparent with
hierarchy checks; text merging; attributes; mutation generation counter;
non-recursive traversal and dump. HTML namespace only. No DOM events or
mutation observers yet (Phase 9).

### CSS (CSS-001) — Tested, verified on reference browser

Evidence: [CSS-001](../research/specifications/CSS-001.md),
`crates/style/tests/golden.rs` (11/11 fixtures match Chromium 152 `getComputedStyle`).

Supported:
- Syntax: CSS Syntax 3 tokenizer (escapes, comments, numbers, dimensions, URLs,
  bad strings/URLs), qualified rules, declaration lists with `!important`,
  error recovery for invalid declarations and unterminated input.
- Sources: user-agent sheet (`crates/style/src/ua.css`, from HTML §15), `<style>`
  elements (no `media`, or `all`/`screen`), `style=""` attributes.
- Selectors: type, `*`, `#id`, `.class`, `[attr]` with `=` `~=` `|=` `^=` `$=` `*=`
  and `i`/`s` flags, combinators (descendant, `>`, `+`, `~`), `:root`
  `:first-child` `:last-child` `:only-child` `:first-of-type` `:last-of-type`
  `:only-of-type` `:empty` `:link` `:any-link`; `:visited` `:hover` `:active`
  `:focus*` parse but never match yet; pseudo-elements parse but match nothing.
  Quirks-mode case-insensitive class/ID matching.
- Cascade: origin and importance, inline styles above selectors, specificity,
  source order; inheritance; `inherit` `initial` `unset` (`revert` ≈ `unset`).
- Properties: `display`, `color`, `background-color` (and single-color
  `background`), `font-size`, `font-weight` (incl. `bolder`/`lighter`),
  `font-style`, `font-family`, `line-height`, `text-align`, `overflow(-x/-y)`,
  `width`, `height`, `margin(-*)`, `padding(-*)`, `border`, `border-*`,
  `border-{width,style,color}` and per-side longhands.
- Values: lengths in px/em/rem/vw/vh/vmin/vmax/in/cm/mm/Q/pt/pc, percentages,
  font-size keywords, all 148 named colors, hex, `rgb()`/`rgba()`/`hsl()`/`hsla()`
  (legacy and modern syntax), `transparent`, `currentcolor`.
- Computed-value rules: root blockification, border width 0 for `none`/`hidden`,
  border-width snapping, overflow `visible`/`clip` fix-up.

Not supported yet:

| Area | Behavior today |
|---|---|
| At-rules (`@media`, `@import`, `@supports`, `@font-face`, `@keyframes`, `@layer`) | Skipped entirely |
| `<link rel=stylesheet>` | Ignored (needs networking, Phase 6) |
| `:not()` `:is()` `:where()` `:has()` `:nth-*()` and other pseudo-classes | Rule dropped |
| Custom properties, `var()`, `calc()`, `ex`/`ch` units | Declaration dropped |
| `font` shorthand; `background` with images/positions | Declaration dropped |
| Monospace default size (13px), `th`/`center` `-internal-center` | Not modeled |
| Default font family | `"Times New Roman"` (`Times` on macOS; Linux/macOS not observed) |
| Positioned/floated blockification | Only the root element is blockified |

Layout, text, networking and JavaScript are not implemented yet.
