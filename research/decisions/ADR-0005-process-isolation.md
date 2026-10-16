# ADR-0005: Process isolation design (renderer processes per site)

- Status: Proposed (design only; not implemented in Phase 9)
- Date: 2026-10-16

## Context

Today RustLite is one process: networking, parsing, style, layout, raster,
the UI and now JavaScript share one address space and the user's
privileges. Our own crates forbid `unsafe`, but dependencies contain it
(Boa's GC, image decoders, rustls/ring, the windowing stack), and any bug
there — or a resource-exhaustion script — affects the whole browser and
everything the user can access. Browsers answer this with process
isolation plus an OS sandbox. This ADR fixes the target design so later
work (and APIs added now) do not paint us into a corner.

## Decision (target architecture)

1. **Browser process** (trusted): window, chrome UI, input routing,
   navigation decisions, history, the network stack and HTTP cache, file
   access for user-initiated `file:` navigations, clipboard.
2. **Renderer processes** (untrusted): HTML/CSS parsing, style, layout,
   paint and JavaScript for one *site* (scheme + registrable domain). One
   process per site per browsing context group; cross-site navigation swaps
   process. Renderers produce display lists (already our paint/raster
   boundary) and rasterize into shared memory or send display lists.
3. **No ambient authority in renderers.** Renderers request every network
   load and every local subresource from the browser process, which applies
   the policies that live in `browser-core` today (scheme checks, mixed
   content, document-directory rule for `file:`). Renderers never open
   files or sockets.
4. **IPC**: typed messages over OS pipes, serialized with a small explicit
   format (no general object deserialization). Every message from a
   renderer is validated as hostile input (sizes, ids, URLs).
5. **OS sandboxes**, per platform, once the split exists: Windows AppContainer
   / restricted token + job object (memory and process limits); Linux
   namespaces + seccomp-bpf; macOS App Sandbox (`sandbox_init` profile).
6. **Resource limits**: per-renderer memory cap (job object / cgroup /
   `RLIMIT_AS`) and a watchdog that kills a renderer whose script runs past
   a deadline — giving the total time limit Boa lacks in-process.

## Why not now

- The single-process code paths are still changing quickly; splitting now
  would double the cost of every feature.
- The design has prerequisites that *are* done in Phase 9: one
  choke point for subresource fetch policy (`subresource_url`), display
  lists as the paint/raster boundary, script access to the DOM only through
  `rustlite-script` with no file/network/UI APIs.

## Rules for code written before isolation

- No new API lets page content reach files, sockets, processes, the
  clipboard or browser UI directly; such features go through the browser
  side (today: `BrowserWindow`/`Tab`) as explicit requests.
- Keep renderer-side state free of OS handles so it can move processes.
- Treat everything derived from page content as untrusted in browser code
  (URLs from links, titles, sizes).

## Consequences

- Until implemented, `docs/security.md` must state plainly that pages,
  including their scripts, run in the browser process without a sandbox.
- Estimated cost: IPC layer and process management first (Windows), then
  sandboxes per platform; memory overhead of ~10–20 MB per renderer
  process is expected and must be measured against the memory targets.
