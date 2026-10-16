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
| Golden parser | `tests/html/`, `tests/css/` | From Phase 2 |
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
