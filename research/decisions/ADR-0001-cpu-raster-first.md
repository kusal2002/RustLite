# ADR-0001: CPU rasterization via softbuffer first; wgpu later

- Status: Accepted
- Date: 2026-10-16

## Context

The specification names wgpu as the initial GPU infrastructure "if it meets the
platform requirements", and asks for a CPU fallback when practical. Phase 1
needs a window that displays a basic scene on every platform, and low memory is
the top product priority.

## Decision

Start with a CPU rasterizer (`rustlite-renderer`) presenting through
`softbuffer`. Design the display list so that a wgpu backend can consume the
same `DisplayList` later.

## Rationale

- The CPU path **is** the required fallback; building it first means it never
  rots behind a GPU path.
- Deterministic pixels: headless rendering produces bit-identical output on
  every platform, which makes golden/screenshot tests reliable.
- Much smaller dependency graph, binary and idle memory than a GPU device +
  swapchain (measured idle working set: 16.8 MB, see docs/performance.md).
- Current scenes (rects, borders, clips) rasterize in ~1 ms at 1024×768.

## Consequences

- Full-window CPU raster will not scale to large scrolling pages with images;
  wgpu is planned for Phase 5 and will be gated on measured frame times.
- softbuffer does not vsync on Windows; frame pacing must be handled when
  animation/scrolling arrives.
- Revisit when: p95 frame time exceeds 8 ms on the reference machine, or
  image/text-heavy pages arrive.
