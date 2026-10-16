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
| DOM dump | `rustlite --dump-dom file.html` | Prints RustLite's tree in html5lib format for ad-hoc diffs |
| Screenshot | `rustlite --headless out.png` | Deterministic CPU raster; golden images from Phase 5 |
| Differential | REA + Chrome | See [rea-workflow.md](rea-workflow.md) |

## Windowed smoke test

```bash
RUST_LOG=info cargo run --release -- --exit-after-frames 120
```

Expected: `window created` and `frame times (last 120 frames)` log lines, exit 0.

## Web Platform Tests

Not run yet. A curated subset (html/syntax/parsing, css/selectors,
css/CSS2/normal-flow) will be imported once the parser exists. No WPT
compliance is claimed.

## Regenerating HTML goldens (reference-browser oracle)

```bash
python scripts/html-oracle/server.py 8765
```

Open `http://127.0.0.1:8765/` in the reference browser. The page parses every
`tests/html/*.html` with `DOMParser`, and the server writes `*.tree` plus
`oracle.txt` (user agent and document modes). Review the diff before
committing; never edit `.tree` files by hand to make a test pass. Intentional
differences go in `KNOWN_DIFFERENCES` in `golden.rs` with a reason.
