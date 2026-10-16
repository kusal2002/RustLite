# ADR-0004: Boa as the JavaScript engine, bound to the DOM without `unsafe`

- Status: Accepted
- Date: 2026-10-16
- Decided with the user (major dependency): Boa chosen over QuickJS
  (rquickjs) and over deferring the engine.

## Context

Phase 9 adds scripting. Options considered:

| | Boa 0.22 | QuickJS via rquickjs | Defer |
|---|---|---|---|
| Language | Rust | C behind FFI bindings | — |
| Memory safety in-process | Rust (Boa itself contains `unsafe`, e.g. its GC) | C engine in the browser process | — |
| Speed | Interpreter, slower than QuickJS | Faster, smaller | — |
| Fits "no unsafe in our crates" | Yes (see below) | Bindings need care | — |

Without process isolation (ADR-0005), every script runs in the browser
process, so the memory-safety story of the engine matters more than its
speed. Boa keeps the whole stack in Rust.

## Decision

- `boa_engine = 0.22.0` with default features **off** and only `annex-b`
  (web-compatibility syntax). No `intl` or `temporal` (their data adds
  megabytes); `Intl` and `Temporal` are absent for pages.
- New crate `rustlite-script` owns all engine use. `browser-core` sees a
  small API: `ScriptHost::new`, `run_script`, `dispatch_event`,
  `has_listeners`, `console`.
- **Lending, not sharing, the DOM.** The page owns its `Document`. For each
  call into JavaScript the document is moved into the Boa context's host
  data (`Context::insert_data`) and moved back afterwards. Bindings never
  alias page state, so no `Rc<RefCell<…>>` and no `unsafe`.
- **Unforgeable wrappers.** Wrapper objects are plain JS objects with our
  prototypes; the node they stand for is found in a Rust table keyed by the
  object's identity (`JsObject` hashes by pointer). A script cannot point a
  wrapper at another node, and an object made with
  `Object.create(prototype)` is rejected (`TypeError: not a Node`). Native
  object data would need `unsafe impl Trace`, which `forbid(unsafe_code)`
  rules out.
- **Limits.** Boa's runtime limits: 10,000,000 iterations per loop and a
  call depth of 400. A stopped script reports an error and the realm stays
  usable.
- **Execution model.** Classic scripts run after parsing, in document order,
  as if `defer` (no `document.write`, no parser-blocking scripts); then
  `DOMContentLoaded` and `load`. Modules are not run. External scripts are
  fetched as subresources (`Destination::Script`, 4 MiB limit, refused when
  served as image/audio/video/CSV); a failed one is skipped.

## Measurements (reference machine, release)

| | Value |
|---|---|
| Release binary | 4.7 MiB → 10.8 MiB (+6.1 MiB for Boa and its dependencies) |
| Realm creation (with DOM bindings) | 0.7–0.9 ms |
| Private memory per realm | ~0.29 MB (50 realms: 15.3 MB vs 0.7 MB for none) |
| `tests/pages/interactive.html` scripts, incl. realm creation | 2.1 ms headless |
| Window with that page after clicks | 25.1 MB working set, 10.7 MB private |

Realms are created only for documents that have scripts.

## Consequences

- Binary size more than doubles; memory per scripted page grows modestly.
- Performance of script-heavy pages will lag C/JIT engines; acceptable for
  the "simple interactive pages" acceptance of Phase 9.
- No total time or heap limit: a script can still run long (many short
  loops) or allocate a lot. Mitigation needs an interrupt hook or process
  isolation (ADR-0005).
- `--disable-javascript` turns scripting off.
