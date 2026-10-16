# Security

**RustLite is a single-process prototype and is not secure for untrusted web
content.** It currently loads no external content at all.

## Current posture

- No networking, no file loading, no script execution.
- `unsafe_code = "forbid"` across the workspace.
- Command-line sizes are bounded (≤ 8192 px per side) so a headless framebuffer
  cannot exceed ~256 MiB.
- Rasterizer bounds all clip nesting (256) and tolerates malformed display lists
  (unbalanced clips are counted and logged, not fatal).

## Required before loading untrusted content

1. TLS with certificate validation (rustls; never disabled for tests).
2. Scheme allow-list (http, https, about); no unrestricted `file:`.
3. Response size limits, timeouts, cancellation, redirect limits.
4. Decoder failure isolation for images and fonts.
5. Process model: separate renderer process per site with OS sandboxing;
   design to be recorded as an ADR before Phase 6 ships.
6. Script execution limits and no privileged host APIs exposed to pages.
