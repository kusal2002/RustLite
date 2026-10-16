# Testing

## Run everything

```bash
./scripts/check.sh        # Linux/macOS
```

```powershell
./scripts/check.ps1       # Windows
```

Both run: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `cargo build --release`.

## Test layers

| Layer | Where | Notes |
|---|---|---|
| Unit | `#[cfg(test)]` in each crate | Geometry, raster, PNG, toolbar, commands, options, invalidation |
| Integration | `crates/*/tests/` | PNG container structure |
| App smoke | `apps/desktop/tests/smoke.rs` | Runs the real binary headless; no display required |
| Windowed smoke | manual / CI with display | `rustlite --exit-after-frames 120` must exit 0 |
| Golden HTML | `tests/html/*.html` → `*.tree` via `crates/html/tests/golden.rs` | Expected trees come from a reference browser (below) |
| Golden style | `tests/style/*.html` → `*.styles` via `crates/style/tests/golden.rs` | `getComputedStyle` from a reference browser, 800×600 |
| Style dump | `rustlite --size 800x600 --dump-style file.html` | Same format as the style goldens |
| Golden layout | `tests/layout/*.html` → `*.boxes` via `crates/browser-core/tests/layout_golden.rs` (full page pipeline, images from `tests/layout/assets/`) | `getBoundingClientRect` from a reference browser, 800×600; per-fixture tolerance; text fixtures skip when their font is missing |
| Layout dump | `rustlite --size 800x600 --dump-layout file.html` | Same format as the layout goldens |
| Visual check | `rustlite --headless out.png tests/pages/sample.html` | Renders a real page to PNG |
| DOM dump | `rustlite --dump-dom file.html` | Prints RustLite's tree in html5lib format for ad-hoc diffs |
| Screenshot | `rustlite --headless out.png` | Deterministic CPU raster; golden images from Phase 5 |
| Differential | REA + Chrome | See [rea-workflow.md](rea-workflow.md) |

## Network tests

`crates/net/tests/fetch.rs` and `crates/browser-core/tests/navigation.rs` start
a local HTTP server on `127.0.0.1` inside the test process; CI needs no
external network. Certificate validation was checked manually against
badssl.com (see NET-001); an offline TLS test with a test CA is a gap.

## Benchmarks

`cargo run --release -p rustlite-bench` prints per-stage medians;
`--compare benchmarks/baseline-windows-x64.tsv` exits non-zero when a stage
is more than 1.5× its baseline (stages under 0.5 ms are ignored as noise).
CI runs the harness only as a smoke test. See
[performance.md](performance.md#pipeline-benchmarks-phase-8).

## Windowed smoke test

```bash
RUST_LOG=info cargo run --release -- --exit-after-frames 120
```

Expected: `window created` and `frame times (last 120 frames)` log lines, exit 0.

## Web Platform Tests

Not run yet. A curated subset (html/syntax/parsing, css/selectors,
css/CSS2/normal-flow) will be imported once the parser exists. No WPT
compliance is claimed.

## Regenerating goldens (reference-browser oracle)

```bash
python scripts/oracle/server.py 8765
```

Open `http://127.0.0.1:8765/html`, `/style` and `/layout` in the
reference browser. `/layout` renders `tests/layout/*.html` in an 800×600
`srcdoc` iframe and writes `*.boxes` from `getBoundingClientRect`. Layout
fixtures declare `<!-- tolerance: px -->` and, if they contain text,
`<!-- fonts: Family; ascent descent gap -->` (rounded 16px metrics of the observed
font); they are skipped with a note when no installed font of that family has
those metrics
(Linux CI has no Times New Roman; macOS has one with different vertical
metrics). `/html` parses every `tests/html/*.html` with `DOMParser`
and writes `*.tree`; `/style` renders every `tests/style/*.html` in an
800×600 `srcdoc` iframe and writes `*.styles` from `getComputedStyle`. Each
writes `oracle.txt` (user agent, device pixel ratio, document modes).
A style fixture can begin with `<!-- compare: prop prop ... -->` to choose the
compared properties. `srcdoc` documents are never in quirks mode, so quirks
behavior needs unit tests instead. Review the diff before
committing; never edit `.tree` files by hand to make a test pass. Intentional
differences go in `KNOWN_DIFFERENCES` in `golden.rs` with a reason.
