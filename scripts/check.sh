#!/usr/bin/env sh
# Runs the full local check suite. Exits non-zero on the first failure.
set -eu
cd "$(dirname "$0")/.."

echo "==> fmt";    cargo fmt --all --check
echo "==> clippy"; cargo clippy --workspace --all-targets -- -D warnings
echo "==> test";   cargo test --workspace
echo "==> release build"; cargo build --release --workspace
echo "All checks passed."
