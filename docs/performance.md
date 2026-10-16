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
| Idle app memory < 50 MB | Baseline met for blank page (16.8 MB WS); revalidate once fonts/DOM land |
| Simple static page < 100 MB | Not measurable yet (no HTML) |
| 60 FPS scrolling | Not measurable yet (no scrolling) |

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
| 2026-10-16 | Style computation, same page (300,005 elements, 160 rules incl. UA), 800×600 | style time (3 runs) | 71.4 / 88.1 / 71.7 ms; 299,987 elements served by style sharing | `rustlite --dump-style` `style_ms` log field, release build. Best case for sharing: the page repeats one structure 20,000 times |

Single runs; no variance analysis yet. Criterion benchmarks are not set up yet; parse timings come from the
`parse_ms` log field. A repeatable benchmark harness is a Phase 8 item.

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
- Known costs to revisit: font data copied per face and eager fontdue parsing
  (the main memory cost now); tag/attribute names are owned strings (no
  interning); named-reference lookup is a linear scan; the style-sharing key
  clones the matched-rule list; `--dump-style` output is slow for huge pages.
