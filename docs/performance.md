# Performance

Performance targets are **hypotheses** until measured on a defined setup.

## Reference machine

| | |
|---|---|
| CPU | AMD Ryzen 5 5600G |
| GPU | AMD Radeon integrated (unused: CPU raster) |
| OS | Windows 11 Pro 10.0.26200 |
| Toolchain | Rust 1.95.0, `x86_64-pc-windows-msvc` |
| Build | `--release` (thin LTO, codegen-units=1, panic=abort) |
| Scale factor | 1.0 |

## Targets

| Target | Status |
|---|---|
| Idle app memory < 50 MB | Met: 16.8 MB working set blank; 28.8 MB with the sample page |
| Simple static page < 100 MB | Met: 29.3 MB working set with three tabs (2026-10-16) |
| 60 FPS scrolling | Met on the reference machine: scroll repaint + raster is ~1 ms regardless of page length (paint culls to the viewport) |

## Benchmark log

| Date | Scenario | Metric | Result | How measured |
|---|---|---|---|---|
| 2026-10-16 | Release binary size | bytes | 1,106,944 (1.06 MiB) | `ls -l target/release/rustlite.exe` |
| 2026-10-16 | Idle window, 1024×768, about:blank, 3 s after launch | Working set / private bytes | 16.8 MB / 5.4 MB | PowerShell `Get-Process` (`WorkingSet64`, `PrivateMemorySize64`) |
| 2026-10-16 | 120 continuous redraws, 1024×768 | frame time mean / p95 / max | 1.02 / 2.26 / 3.66 ms | `rustlite --exit-after-frames 120` log summary (compose + raster + present) |
| 2026-10-16 | Headless compose + raster, 1024×640, 10 display items | time | 0.47 ms | `rustlite --headless` log |
| 2026-10-16 | HTML parse, 4.3 MB synthetic page (20k blocks with misnested formatting, lists, tables), 540,009 nodes, 20k parse errors | parse time (3 runs) | 194.8 / 193.0 / 193.6 ms (~22 MB/s) | `rustlite --dump-dom` `parse_ms` log field, release build |
| 2026-10-16 | Same, peak memory | working set | Not measured (PowerShell 5.1 does not expose peak WS after exit; tooling TBD) | — |
| 2026-10-16 | `tests/pages/sample.html` (24 elements, Georgia + Sinhala fallback), 900×900 headless | parse / style / layout / raster | 0.16 / 0.04 / 11.8 / 129 ms (raster includes first-time glyph rasterization) | `rustlite --headless` log, release |
| 2026-10-16 | System font discovery (365 faces) | time | 32–35 ms | `system fonts loaded` log |
| 2026-10-16 | Window with sample page, 1024×768 | frame time mean / p95 / max over 60 frames | 3.47 / 2.93 / 130 ms (max = first frame: fonts + layout + glyphs) | `--exit-after-frames 60` log |
| 2026-10-16 | Window with sample page, 4 s after launch | working set / private bytes | 73.6 MB / 62.7 MB | PowerShell `Get-Process`. Up from 16.8 MB blank: font files are copied to the heap and fontdue parses whole faces (incl. the large Sinhala fallback face) |
| 2026-10-16 | Release binary size | bytes | 2,202,624 (2.1 MiB; was 1.06 MiB before fonts/layout) | file size |
| 2026-10-16 | Window with sample page after replacing fontdue (ADR-0003) | working set / private bytes | 23.9 MB / 12.1 MB (was 73.6 / 62.7 MB) | PowerShell `Get-Process`, 4 s after launch |
| 2026-10-16 | Window with extended sample page (lists, 2 images, patterned borders) | working set / private bytes | 24.4 MB / 12.2 MB | same |
| 2026-10-16 | Extended sample page, 900×1000 headless | image load / layout / raster | 1.0 / 14.1 / 66.4 ms (raster includes first-time glyphs) | `rustlite --headless` log, release |
| 2026-10-16 | Release binary size with `image` (png/jpeg/gif) | bytes | 2,661,376 (2.5 MiB) | file size |
| 2026-10-16 | Release binary size with networking (ureq, rustls/ring, platform verifier, url, encoding_rs) | bytes | 4,814,336 (4.6 MiB) | file size |
| 2026-10-16 | Window showing https://example.com/, 5 s after launch | working set / private bytes / threads | 28.0 MB / 12.0 MB / 15 | PowerShell `Get-Process` |
| 2026-10-16 | Three tabs (two with tests/pages/sample.html, one blank), find bar used | working set | 29.1 MB | PowerShell `Get-Process`; tabs share one font context |
| 2026-10-16 | Phase 8 build: sample page, then three tabs | working set / private bytes | 28.8 / 16.0 MB → 29.3 / 16.5 MB | PowerShell `Get-Process`, scripted run |
| 2026-10-16 | Release binary size (Phase 8) | bytes | 4,908,032 (4.7 MiB) | file size |
| 2026-10-16 | Release binary size with Boa (Phase 9) | bytes | 11,310,592 (10.8 MiB) | file size |
| 2026-10-16 | Release binary size with Web APIs and forms (Phase 10) | bytes | 11,507,200 (11.0 MiB) | file size |
| 2026-10-16 | `interactive.html` after typing into its form (timers and a 60-frame animation ran) | working set / private bytes | 26.6 MB / 12.0 MB | scripted window run, PowerShell `Get-Process` |
| 2026-10-16 | JavaScript realm with DOM bindings | creation time / private memory | 0.7–0.9 ms / ~0.29 MB each (50 realms: 15.3 MB vs 0.7 MB) | `realm_cost` example + PowerShell `Get-Process` |
| 2026-10-16 | `tests/pages/interactive.html` (scripts build a list, 3 listeners) | script time incl. realm / window memory after 4 clicks | 2.1 ms headless / 25.1 MB WS, 10.7 MB private | `scripts run` log; scripted mouse run |
| 2026-10-16 | Style computation, same page (300,005 elements, 160 rules incl. UA), 800×600 | style time (3 runs) | 71.4 / 88.1 / 71.7 ms; 299,987 elements served by style sharing | `rustlite --dump-style` `style_ms` log field, release build. Best case for sharing: the page repeats one structure 20,000 times |
| 2026-10-17 | Release binary size with advanced layout, renderer processes and sandbox (Phase 10) | bytes | 11,759,616 (11.2 MiB) | file size |
| 2026-10-17 | Form typing and submission with the page in a sandboxed renderer | working set, browser + renderer | 22.8 MB + 14.9 MB | scripted window run, PowerShell `Get-Process` |
| 2026-10-17 | Three tabs (`layout.html`, `sample.html`, `interactive.html`), one renderer each | processes / total working set | 4 / 64.8 MB (browser 23.2 MB; renderers 10.3, 15.2, 16.1 MB) | scripted window run, PowerShell `Get-Process`; all renderers exited with the browser |
| 2026-10-17 | Three tabs with brokered fonts and restricted-token renderers | processes / total working set | 4 / 70.8 MB (browser 28.7 MB; renderers 10.4, 15.1, 16.5 MB) | scripted window run, PowerShell `Get-Process`; the browser now holds the font data it serves |
| 2026-10-17 | Release binary size with the broker and sandbox launch | bytes | 11,795,456 (11.25 MiB) | file size |
| 2026-10-17 | `article-400` warm layout after floats/positioning/flex support | change vs. Phase 8 baseline | about +10–20% over repeated runs (one outlier run ×1.93) | `rustlite-bench` compare; static-position tracking is skipped unless a box is out of flow |

Rows above are single runs. From Phase 8 on, pipeline timings come from the
benchmark harness below (medians of repeated runs).

## Pipeline benchmarks (Phase 8)

`benchmarks/` (`rustlite-bench`, no external harness) times each stage on
deterministic workloads: `sample` (tests/pages/sample.html), `article-50`
and `article-400` (generated pages: 50/400 sections of prose with inline
markup and links, lists and box grids, a 3,000-word vocabulary with a
skewed frequency, and 100/400 class, descendant and attribute rules).
Stages: `parse` (HTML), `cascade` (stylist build), `style`, `layout-cold`
(word shaping cache emptied first), `layout` (warm), `paint`, `raster`
(1280×800). Each stage runs warm-up iterations, then N timed runs; the
median is reported.

```bash
cargo run --release -p rustlite-bench                      # full table
cargo run --release -p rustlite-bench -- --save FILE       # write a baseline
cargo run --release -p rustlite-bench -- --compare FILE    # fail on >1.5x
```

`benchmarks/baseline-windows-x64.tsv` is the reference-machine baseline.
CI only smoke-runs the harness (`--quick --iterations 3`): shared runners
are too noisy to gate on, so regressions are checked with `--compare` on
the reference machine before a phase is closed.

### Phase 8 results (reference machine, medians, ms)

| Workload | Stage | Before Phase 8 | After | Change |
|---|---|---|---|---|
| article-50 (3,476 elements) | layout (cold) | 740.1 | 19.7 | shaping faces cached; word shaping cache |
| article-50 | layout (warm relayout) | 740.1 | 9.7 | |
| article-50 | style | 12.3 | 5.1 | attribute-name rule bucket; ancestor key filter |
| article-400 (27,626 elements) | layout (cold) | 6,147 | 112 | |
| article-400 | style | 390.7 | 113 | |
| article-400 | paint | 13.1 | 0.10 | paint culls to the viewport |
| article-400 | raster | 26.1 | 0.62 | 128,441 → 364 display items |
| sample | layout (cold) | 1.89 | 0.51 | |

"Before" for layout was measured with the 20-word vocabulary the harness
first used; the cold number after the change uses the 3,000-word one, so
the comparison understates the gain. Findings and causes:

- **Shaping:** `rustybuzz::Face::from_slice` took 83 µs per call (parsing
  GSUB/GPOS) versus 3.5 µs to shape a word, and was called per word. Faces
  are now parsed once per loaded font (`self_cell`), and shaped words are
  cached (≤48-byte strings, 16,384 entries, cleared when full). Since
  ADR-0007, shaping uses `harfrust`, whose owned `Font` keeps parsed layout
  data per face without `self_cell` (~15 KB instead of ~200 KB per face).
- **Style:** attribute selectors had no bucket, so every `[attr]` rule was
  tried on every element; descendant selectors walked ancestors for every
  candidate rule. Rules are now bucketed by attribute name, and each rule
  stores hashed id/class/tag keys its ancestors must have; an element's
  ancestor keys are collected once and reject most candidates without a
  tree walk (the idea behind browsers' ancestor Bloom filters).
- **Paint/raster:** the whole document was painted and every item was
  clipped during raster. Paint now skips lines and box decorations outside
  the viewport (with slack for overhanging glyphs); blocks that clip their
  overflow are skipped entirely when off-screen.
- **Remaining hot spots:** HTML parse (~23 µs per element on article-400),
  warm layout (~0.8 µs per word: inline item collection and per-item font
  selection, which formats a cache key string), and style at 400 rules.

## Memory reporting

`Page::memory_report()` estimates heap bytes per subsystem (DOM arena and
strings, distinct computed styles, layout boxes/fragments/glyph arrays,
decoded images, display list, glyph and shaping caches). Estimates count
payloads and container slots, not allocator overhead. The desktop app logs
it when a network page finishes loading and for every tab on exit; headless
renders log it after the frame. Example (`tests/pages/sample.html`):
dom 15 KiB (95 nodes) · styles 7 KiB (31 distinct) · layout 39 KiB ·
images 3 KiB · display list 12 KiB · glyph cache 26 KiB · shape cache
50 KiB. Font files and the HTTP cache are reported separately.

## Bounded caches

| Cache | Bound | Policy |
|---|---|---|
| Glyph bitmaps (`FontContext`) | entry cap | cleared when full |
| Shaped words (`FontContext`) | 16,384 entries, strings ≤48 bytes | cleared when full |
| HTTP subresources (`Loader`) | 32 MiB of bodies; entries ≤ ¼ budget | LRU eviction; `max-age` expiry |
| Style sharing | one document | dropped with the styles |

## Invalidation

| Change | Restyle | Relayout | Repaint |
|---|---|---|---|
| Scroll | no | no | yes (viewport-culled) |
| Viewport size or device scale | only if a computed value used `vw`/`vh`/`vmin`/`vmax` | yes | yes |
| Image arrives | no | yes | yes |
| Stylesheet arrives | yes | yes | yes |
| Font context handed to another tab | no | only if the page had a different context | yes |

Not yet incremental: restyle and relayout are whole-document when they
happen (no dirty bits), and repaint rasterizes the whole window.

## Principles in effect

- Display-list and frame allocations are reused across frames.
- Page display list rebuilt only on invalidation.
- No redraw without a cause (`ControlFlow::Wait`).
- Frame stats use a fixed-size ring; no per-frame allocation.
- DOM nodes live in one arena; `Option<NodeId>` is 4 bytes.
- Parser text runs are one allocation per run, not per character.
- Scope checks skip the stack walk when no element of that name is open, and
  leaf insertion skips the cycle walk; deep nesting parses in linear time.
- Rules are indexed by the subject's ID, class, or tag, so each element tests
  only plausible rules; identical siblings share one `Rc<ComputedStyle>`.
- Glyph bitmaps are cached (bounded at 4,096 entries); font selection and
  fallback lookups are cached per context.
- Glyphs are decoded one at a time from the font file (no whole-face parsing).
- Decoded images are shared between elements that use the same file.
- Known costs to revisit: font data is copied per face in use; images decode
  on the UI thread at load; tag/attribute names are owned strings (no
  interning); named-reference lookup is a linear scan; the style-sharing key
  clones the matched-rule list; `--dump-style` output is slow for huge pages.
