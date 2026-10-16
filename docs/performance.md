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

Single runs; no variance analysis yet. Criterion benchmarks arrive with the
first parser (Phase 2) — `benchmarks/` is reserved for them.

## Principles in effect

- Display-list and frame allocations are reused across frames.
- Page display list rebuilt only on invalidation.
- No redraw without a cause (`ControlFlow::Wait`).
- Frame stats use a fixed-size ring; no per-frame allocation.
