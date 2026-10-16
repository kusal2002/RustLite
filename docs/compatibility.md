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
| Validation | ✅ build, tests, window | ✅ build + tests in CI (no window) | ✅ build + tests in CI after `ed08a83` fix (no window) |

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

### Layout (LAYOUT-001) — Tested, verified on reference browser

Evidence: [LAYOUT-001](../research/specifications/LAYOUT-001.md),
`crates/layout/tests/golden.rs` (9/9 fixtures; max 0.032px from Chromium 152).

Supported: block boxes and anonymous block boxes, `display: none`/`contents`;
CSS 2.1 width resolution with `auto` margins and percentages; `height` px/%/auto;
vertical margin collapsing (siblings, parent/child, through empty boxes,
negative margins; stopped by borders, padding and BFC roots); inline formatting
with `white-space: normal` collapsing across elements, greedy line breaking at
spaces, `<br>`, line boxes with half-leading and baseline alignment (Blink's
rounding), `text-align` left/right/center/start/end; overflow clipping when
painting; canvas background propagation from `html`/`body`; device pixel ratio.

| Area | Behavior today |
|---|---|
| Floats, `position`, flex, grid, tables | Laid out as ordinary blocks |
| `inline-block`, form controls | Treated as inline containers |
| Images | Local PNG/JPEG/GIF inside the document's directory; inline (baseline-aligned atomic) or block; `width`/`height` attributes as presentational hints; intrinsic ratio. No network/`data:`/SVG/WebP, no `object-fit`, broken images render nothing ([IMAGE-001](../research/specifications/IMAGE-001.md)) |
| List markers | Outside markers for disc/circle/square/decimal/alpha/roman; `<ol start>`, `<li value>`; no `list-style-position: inside`, `list-style-image`, `reversed`, or `::marker` styling |
| `white-space` other than `normal`, CJK/hyphen/UAX #14 break opportunities | Not supported (breaks only at spaces) |
| Bidi, vertical text, `vertical-align` | Not supported |
| Inline padding/borders/margins/backgrounds | Ignored |
| `min-*`/`max-*` sizes, `box-sizing` | Not supported (content-box) |
| Border styles | solid, dashed, dotted, double; groove/ridge/inset/outset painted solid (no 3D shading) |
| Scrollbars | None drawn; the page scrolls with wheel/keys |
| Nesting deeper than 200 block levels | Flattened into inline content (stack safety) |

### Fonts and text (FONT-001)

System fonts via fontdb on all platforms; generic families map to the first
installed of a per-platform list (e.g. serif → Times New Roman / Times /
Liberation Serif / DejaVu Serif); per-character fallback keeps combining marks
and Sinhala signs with their base; shaping with rustybuzz (kerning, ligatures);
grayscale glyph rasterization without hinting by RustLite's own on-demand
rasterizer ([ADR-0003](../research/decisions/ADR-0003-own-glyph-rasterizer.md);
text looks softer than Chromium's). No `@font-face`, no color emoji tables.

### Browser UI (SHELL-003, FIND-001)

Tabs (Ctrl/Cmd+T, Ctrl/Cmd+W, Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+PageUp/Down,
Ctrl/Cmd+1–9, middle-click to close), links in new background tabs
(Ctrl/Cmd+click, middle-click), clipboard copy/cut/paste in the address and
find fields, find in page (Ctrl/Cmd+F, Enter/F3/Ctrl+G next,
Shift+Enter/Shift+F3/Ctrl+Shift+G previous, Esc closes). Find matching is
simple per-character lowercasing (no Unicode case folding, diacritic or
whitespace normalization) and does not search across blocks, form-control
values or alt text. No text selection in page content yet.

### Networking (NET-001)

HTTP/1.1 and HTTPS (TLS 1.2/1.3 via rustls, OS trust store), gzip, redirects
(10 hops), charset detection (BOM, HTTP, `<meta>`, UTF-8, windows-1252),
`<base href>`, linked stylesheets, images over HTTP, error pages. See
[networking.md](networking.md) for limits and gaps (no cache, cookies, CORS,
HTTP/2, `data:` URLs, downloads, forms).

### CSS additions (CSS-002)

`font` shorthand (style/weight/size/line-height/family; variant and stretch
accepted and ignored; system fonts unsupported), `min-width`, `max-width`,
`min-height`, `max-height` (blocks and replaced elements), `light-dark()`
(light scheme only).

### JavaScript (SCRIPT-001)

Engine: Boa 0.22 (ECMAScript, Annex B; no `Intl`, no `Temporal`). Classic
scripts (inline and `src`) run after parsing in document order, as if
`defer`; then `DOMContentLoaded` (document) and `load` (window). Not run:
modules, `document.write`, scripts inserted by `innerHTML`.

| API | Supported |
|---|---|
| Globals | `window`, `self`, `globalThis`, `document`, `console.{log,info,warn,error,debug}`, `location.href` (read-only), `addEventListener` on window |
| `Node` | `nodeType`, `nodeName`, `textContent` (get/set), `parentNode`, `parentElement`, `firstChild`, `lastChild`, `nextSibling`, `previousSibling`, `childNodes` (array snapshot), `isConnected`, `appendChild`, `insertBefore`, `removeChild`, `contains`, `addEventListener`, `removeEventListener` |
| `Element` | `tagName`, `id`, `className`, `innerHTML` (get/set), `innerText` (= `textContent`), `children`, `firstElementChild`, `classList` (`add`, `remove`, `toggle`, `contains`, `length`), `getAttribute`, `setAttribute`, `removeAttribute`, `hasAttribute`, `remove`, `querySelector`, `querySelectorAll`, `getElementsByTagName`, `matches`, `closest`, `click()`, `onclick` |
| `Document` | `title` (get/set), `body`, `head`, `documentElement`, `getElementById`, `querySelector(All)`, `getElementsByTagName`, `createElement`, `createTextNode` |
| Events | `click` from the mouse and `click()`; bubbling to document and window (no capture phase); `preventDefault` (cancels link navigation), `stopPropagation`, `stopImmediatePropagation`, `defaultPrevented`; `on*` attributes and `return false` |

Not yet: timers (`setTimeout`), `fetch`/XHR, `style` objects (use
`setAttribute('style', …)`), `dataset`, live collections (lists are
snapshots), keyboard/input events and form controls, `Event` constructors,
images or stylesheets inserted by scripts (they are not loaded).
