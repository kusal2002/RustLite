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

CSS, layout, text, networking and JavaScript are not implemented yet.
