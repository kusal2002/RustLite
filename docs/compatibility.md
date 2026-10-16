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
  `border-{width,style,color}` and per-side longhands; `position`, `inset`
  and `top`/`right`/`bottom`/`left`, `z-index`, `float`, `clear`,
  `box-sizing`, `flex-direction`/`-wrap`/`-flow`, `justify-content`,
  `align-items`/`-self`/`-content`, `flex`/`-grow`/`-shrink`/`-basis`,
  `order`, `gap`/`row-gap`/`column-gap` (style fixture `layout-properties`).
- Values: lengths in px/em/rem/vw/vh/vmin/vmax/in/cm/mm/Q/pt/pc, percentages,
  font-size keywords, all 148 named colors, hex, `rgb()`/`rgba()`/`hsl()`/`hsla()`
  (legacy and modern syntax), `transparent`, `currentcolor`.
- Computed-value rules: blockification of the root, floats, absolutely
  positioned boxes and flex items; border width 0 for `none`/`hidden`,
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

### Layout (LAYOUT-001) — Tested, verified on reference browser

Evidence: [LAYOUT-001](../research/specifications/LAYOUT-001.md),
`crates/browser-core/tests/layout_golden.rs` (19/19 fixtures incl. `form-controls` and the LAYOUT-002 set below; within each fixture's tolerance of Chromium 152).

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
| Grid, tables, multi-column | Laid out as ordinary blocks |
| Images | Local PNG/JPEG/GIF inside the document's directory; inline (baseline-aligned atomic) or block; `width`/`height` attributes as presentational hints; intrinsic ratio. No network/`data:`/SVG/WebP, no `object-fit`, broken images render nothing ([IMAGE-001](../research/specifications/IMAGE-001.md)) |
| List markers | Outside markers for disc/circle/square/decimal/alpha/roman; `<ol start>`, `<li value>`; no `list-style-position: inside`, `list-style-image`, `reversed`, or `::marker` styling |
| `white-space` other than `normal`, CJK and other UAX #14 break opportunities | Not supported (breaks at spaces and after hyphens only) |
| Bidi, vertical text, `vertical-align` | Not supported |
| Inline padding/borders/margins/backgrounds | Ignored |
| Border styles | solid, dashed, dotted, double; groove/ridge/inset/outset painted solid (no 3D shading) |
| Scrollbars | None drawn; the page scrolls with wheel/keys |
| Nesting deeper than 200 block levels | Flattened into inline content (stack safety) |

### Advanced layout (LAYOUT-002) — Tested, verified on reference browser

Evidence: layout fixtures `positioning`, `floats`, `inline-block`,
`flexbox`, `box-sizing`, `hyphen-breaks` and `form-controls` row 6 match
Chromium 152 within 0.02 px; 36,000 extra release fuzz cases over the new
code found no failures.

Supported:
- **Positioning:** `relative` (offsets after layout, children move with
  it), `absolute` (containing block = nearest positioned ancestor's padding
  box, else the viewport; static position for `auto` insets; shrink-to-fit
  width; insets with width/height), `fixed` (viewport, does not scroll),
  `z-index` stacking for positioned boxes (negative, `auto`/0 and positive
  layers, tree order within a layer); hit testing honors scroll and fixed
  boxes.
- **Floats:** left/right floats in block and inline content, line boxes
  shortened next to floats, stacking and wrapping of floats, `clear`
  (clearance on blocks), BFC roots (`overflow`,
  floats, inline-blocks, absolutely positioned and flex boxes) contain
  their floats and are narrowed next to outside floats.
- **`inline-block`/`inline-flex`:** atomic inlines with shrink-to-fit width
  (min/max-content), baseline = last line box (or bottom margin edge when
  `overflow` is not `visible` or there are no lines).
- **Flexbox (simplified §9):** row/column and reverse directions, `wrap`/
  `wrap-reverse`, `order`, `flex-grow`/`-shrink`/`-basis` with freezing,
  `justify-content` (incl. `space-*`), `align-items`/`-self` (stretch,
  start/end, center, baseline), `align-content`, `gap`, main-axis auto
  margins, min/max sizes and the automatic minimum size on items.
- **`box-sizing: border-box`** for width/height/min/max sizes, including
  floats, absolutely positioned boxes and flex items.
- **Break opportunities after hyphens** (`-`, U+2010) between letters, as
  UAX #14 does.
- Form controls can be floated, absolutely positioned or `display: block`.

Known differences:
- `auto` margins on absolutely positioned boxes are treated as 0 (no
  centering with `inset: 0; margin: auto`).
- A float in the middle of inline content ends the current run of lines:
  it is placed below the text before it, not on the current line.
- Flex baseline alignment uses the first line's baseline and only in
  single-line rows; cross-axis `auto` margins on flex items are 0.
- Relative positioning of inline (non-atomic) elements is ignored.
- `position: sticky` behaves as `relative`; no `float: inline-start/end`,
  `z-index` on non-positioned flex items, or stacking contexts from
  `opacity`/`transform`.

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

### Web APIs (WEBAPI-001)

| API | Supported |
|---|---|
| Timers | `setTimeout`, `setInterval` (callbacks or code strings, extra arguments), `clearTimeout`, `clearInterval`; 4 ms clamp after 5 nested levels; at most 10,000 active timers |
| Frames | `requestAnimationFrame` (timestamp argument, ~60 Hz while requested), `cancelAnimationFrame`, `performance.now()` (0.1 ms resolution) |
| `element.style` | camelCase and hyphenated properties for everything the style system supports, `cssText`, `getPropertyValue`, `setProperty` (incl. `!important` and custom properties), `removeProperty`; `el.style = "…"`. Values with `;`, `{` or `}` are ignored |
| Form controls | `value`, `checked`, `type`, `name`, `focus()`, `blur()`, `document.activeElement` |
| Events | `keydown`/`keyup` (with `key`) at the focused element or body; `input`, `change`, `focus`, `blur` (not bubbling), `submit` (cancelable) |
| Resources | images and stylesheets that scripts add are fetched |

Not yet: `fetch`/XHR, `dataset`, live collections (lists are snapshots),
`Event` constructors and `dispatchEvent`, `KeyboardEvent.code`/modifier
flags, scripts inserted by scripts, composition/IME input, timers in
background tabs are not throttled.

### Forms (FORM-001) — layout verified on reference browser

Rendered as atomic inlines: text fields (`text`, `password` with bullets,
`search`, `email`, `url`, `tel`, `number` and unknown types as text, with
placeholders), `<button>` and `<input type=submit|reset|button|image>`
(text label), checkboxes and radios (13×13, accent-colored when checked);
`hidden` inputs are not rendered. Geometry matches Chromium 152 in the
`form-controls` layout fixture (13/13 fixtures pass, ≤0.02 px): text fields
are `c × size + (M − c)` wide (`c` = rounded "x" advance, `M` = rounded
font bounding box width), one line tall, and baseline-aligned on their text;
toggles align their bottom border edge. With Courier New 20px and Verdana
16px the measured Chromium widths deviate from that formula by up to 1 px.

Interaction: click focuses fields, toggles checkboxes, selects radios
(groups by `name` within a form) and presses buttons; typing, Backspace,
Delete, ←/→, Home/End edit the focused field; Tab/Shift+Tab move focus
through controls; Enter in a field or on a submit button submits the form;
Ctrl/Cmd+V pastes into a field. **Submission:** GET only, fields as
`application/x-www-form-urlencoded` (text values, checked boxes/radios,
`hidden` inputs, the submitter's name/value; disabled controls skipped).
POST forms are refused with a log message. Not yet: `<textarea>`,
`<select>`, `<label>` activation, text selection, scrolling inside fields,
validation, `:focus`/`:checked` selectors, `tabindex`.
