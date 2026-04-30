#!/bin/bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

echo "=== Rust check ==="
cargo check --all-targets

echo ""
echo "=== Rust clippy ==="
cargo clippy -- -D warnings

echo ""
echo "=== Rust fmt ==="
cargo fmt --check

echo ""
echo "=== Rust unit tests ==="
cargo test --all

echo ""
echo "=== Rust integration tests (requires DATABASE_URL) ==="
if [ -n "${DATABASE_URL:-}" ]; then
  cargo test --all -- --ignored
else
  echo "  SKIPPED — DATABASE_URL not set"
fi

echo ""
echo "=== Neovim plugin tests ==="
if command -v nvim &>/dev/null; then
  chmod +x plugin/tests/run_tests.sh
  plugin/tests/run_tests.sh
else
  echo "  SKIPPED — nvim not found"
fi

echo ""
echo "=== All tests passed ==="
